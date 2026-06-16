//! rotation of the active audit log into sealed, compressed segments.
//! the HMAC chain keeps running across a rotation: a rotated segment records
//! its last hmac so the next active log continues from it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use maranode_common::events::AuditEntry;

const SEGMENT_DIR: &str = "audit-rotated";
const MANIFEST: &str = "segments.json";
const INNER_NAME: &str = "audit.jsonl";

#[derive(Debug, Clone, Copy)]
pub struct RotateConfig {
    pub max_bytes: u64,
    pub max_age_days: u32,
}

impl RotateConfig {
    pub fn enabled(&self) -> bool {
        self.max_bytes > 0 || self.max_age_days > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub file: String,
    pub seq_start: u64,
    pub seq_end: u64,
    pub entries: u64,
    pub first_prev_hmac: String,
    pub last_hmac: String,
    pub ts_first: DateTime<Utc>,
    pub ts_last: DateTime<Utc>,
    pub sha256: String,
    pub rotated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub version: u32,
    pub segments: Vec<Segment>,
}

pub struct Summary {
    pub seq_start: u64,
    pub seq_end: u64,
    pub entries: u64,
    pub first_prev_hmac: String,
    pub last_hmac: String,
    pub ts_first: DateTime<Utc>,
    pub ts_last: DateTime<Utc>,
}

pub fn segment_dir(audit_dir: &Path) -> PathBuf {
    audit_dir.join(SEGMENT_DIR)
}

pub fn manifest_path(audit_dir: &Path) -> PathBuf {
    segment_dir(audit_dir).join(MANIFEST)
}

pub fn load_manifest(audit_dir: &Path) -> Result<Manifest> {
    let p = manifest_path(audit_dir);
    if !p.exists() {
        return Ok(Manifest { version: 1, segments: Vec::new() });
    }
    let raw = std::fs::read_to_string(&p)?;
    let m: Manifest = serde_json::from_str(&raw).context("parse segment manifest")?;
    Ok(m)
}

fn save_manifest(audit_dir: &Path, m: &Manifest) -> Result<()> {
    let dir = segment_dir(audit_dir);
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("segments.json.tmp");
    let body = serde_json::to_vec_pretty(m)?;
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, manifest_path(audit_dir))?;
    Ok(())
}

pub fn summarize(content: &str) -> Option<Summary> {
    let mut seq_start: Option<u64> = None;
    let mut first_prev_hmac = String::new();
    let mut ts_first: Option<DateTime<Utc>> = None;
    let mut seq_end = 0u64;
    let mut last_hmac = String::new();
    let mut ts_last: Option<DateTime<Utc>> = None;
    let mut entries = 0u64;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<AuditEntry>(line) {
            if seq_start.is_none() {
                seq_start = Some(e.seq);
                first_prev_hmac = e.prev_hmac;
                ts_first = Some(e.ts);
            }
            seq_end = e.seq;
            last_hmac = e.hmac;
            ts_last = Some(e.ts);
            entries += 1;
        }
    }

    Some(Summary {
        seq_start: seq_start?,
        seq_end,
        entries,
        first_prev_hmac,
        last_hmac,
        ts_first: ts_first?,
        ts_last: ts_last?,
    })
}

pub fn oldest_ts(content: &str) -> Option<DateTime<Utc>> {
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<AuditEntry>(line) {
            return Some(e.ts);
        }
    }
    None
}

pub fn seal_segment(audit_dir: &Path, log_bytes: &[u8], sum: &Summary) -> Result<Segment> {
    let dir = segment_dir(audit_dir);
    std::fs::create_dir_all(&dir)?;

    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let file = format!("audit-{}-{}-{}.jsonl.zip", sum.seq_start, sum.seq_end, stamp);
    let out = dir.join(&file);

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(INNER_NAME, opts)?;
    zip.write_all(log_bytes)?;
    let bytes = zip.finish()?.into_inner();

    let sha256 = format!("{:x}", Sha256::digest(&bytes));

    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&out)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }

    let seg = Segment {
        file,
        seq_start: sum.seq_start,
        seq_end: sum.seq_end,
        entries: sum.entries,
        first_prev_hmac: sum.first_prev_hmac.clone(),
        last_hmac: sum.last_hmac.clone(),
        ts_first: sum.ts_first,
        ts_last: sum.ts_last,
        sha256,
        rotated_at: Utc::now(),
    };

    let mut m = load_manifest(audit_dir)?;
    if m.version == 0 {
        m.version = 1;
    }
    m.segments.push(seg.clone());
    save_manifest(audit_dir, &m)?;

    Ok(seg)
}

pub fn recover_tail(audit_dir: &Path) -> Option<(u64, String)> {
    let m = load_manifest(audit_dir).ok()?;
    let last = m.segments.iter().max_by_key(|s| s.seq_end)?;
    Some((last.seq_end, last.last_hmac.clone()))
}

pub fn read_segment(audit_dir: &Path, seg: &Segment) -> Result<String> {
    let path = segment_dir(audit_dir).join(&seg.file);
    let bytes = std::fs::read(&path).with_context(|| format!("read segment {}", seg.file))?;

    let got = format!("{:x}", Sha256::digest(&bytes));
    if !maranode_common::secure::ct_eq_str(&got, &seg.sha256) {
        anyhow::bail!("segment {} sha-256 does not match the manifest", seg.file);
    }

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut inner = zip.by_name(INNER_NAME)?;
    let mut out = String::new();
    inner.read_to_string(&mut out)?;
    Ok(out)
}

pub fn enforce_segment_retention(audit_dir: &Path, retain_days: u32) -> Result<u64> {
    if retain_days == 0 {
        return Ok(0);
    }
    let mut m = load_manifest(audit_dir)?;
    if m.segments.is_empty() {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::days(retain_days as i64);
    let mut removed = 0u64;
    let mut keep: Vec<Segment> = Vec::with_capacity(m.segments.len());

    for seg in m.segments.drain(..) {
        if seg.ts_last < cutoff {
            let p = segment_dir(audit_dir).join(&seg.file);
            let _ = std::fs::remove_file(&p);
            removed += 1;
        } else {
            keep.push(seg);
        }
    }

    if removed > 0 {
        m.segments = keep;
        save_manifest(audit_dir, &m)?;
    }
    Ok(removed)
}
