//! main handle for the audit log: [`AuditLog`]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::Mutex;

use maranode_common::events::{AuditEntry, AuditEvent};

use crate::chain;
use crate::rotate::{self, RotateConfig};

#[derive(Clone)]
pub struct AuditLog {
    inner: Arc<Mutex<LogInner>>,
}

struct LogInner {
    file: std::fs::File,
    seq: u64,
    last_hmac: String,
    key: Vec<u8>,
    log_path: PathBuf,
    rotate: RotateConfig,
}

const ROTATE_OFF: RotateConfig = RotateConfig {
    max_bytes: 0,
    max_age_days: 0,
};

impl AuditLog {
    /// open with a pre-loaded key (for TPM-sealed key workflows)
    pub fn open_with_key(log_path: &Path, key: Vec<u8>) -> Result<Self> {
        let (seq, last_hmac) = Self::scan_tail(log_path, &key)?;

        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(log_path)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(LogInner {
                file,
                seq,
                last_hmac,
                key,
                log_path: log_path.to_path_buf(),
                rotate: ROTATE_OFF,
            })),
        })
    }

    pub fn open(log_path: &Path, key_path: &Path) -> Result<Self> {
        let key = crate::key::load_or_generate(key_path)?;

        // count existing lines to know the next sequence number
        let (seq, last_hmac) = Self::scan_tail(log_path, &key)?;

        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        // file mode 0600: only owner can read/write (threat model 6.2). Prompts may be
        // stored in full content mode, and chain fields are sensitive.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(log_path)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(LogInner {
                file,
                seq,
                last_hmac,
                key,
                log_path: log_path.to_path_buf(),
                rotate: ROTATE_OFF,
            })),
        })
    }

    /// set the size/age triggers used for automatic rotation. size rotation runs inline on
    /// append; age rotation is left to a periodic [`AuditLog::maybe_rotate`] call.
    pub async fn set_rotation(&self, cfg: RotateConfig) {
        self.inner.lock().await.rotate = cfg;
    }

    pub async fn append(&self, actor: impl Into<String>, event: AuditEvent) -> Result<()> {
        let actor = actor.into();
        let mut inner = self.inner.lock().await;

        let seq = inner.seq + 1;
        let prev_hmac = inner.last_hmac.clone();
        let ts = Utc::now();

        let body = serde_json::json!({
            "ts":        ts,
            "seq":       seq,
            "actor":     actor,
            "prev_hmac": prev_hmac,
            "event":     serde_json::to_value(&event)?,
        });
        let body_str = serde_json::to_string(&body)?;
        let hmac = chain::compute(&inner.key, body_str.as_bytes());

        let entry = AuditEntry {
            ts,
            seq,
            actor,
            event,
            prev_hmac,
            hmac: hmac.clone(),
        };

        let line = serde_json::to_string(&entry)? + "\n";
        inner.file.write_all(line.as_bytes())?;
        inner.file.sync_all()?;

        inner.seq = seq;
        inner.last_hmac = hmac;

        if inner.rotate.max_bytes > 0 {
            let path = inner.log_path.clone();
            let size_only = RotateConfig {
                max_bytes: inner.rotate.max_bytes,
                max_age_days: 0,
            };
            let _ = Self::rotate_locked(&mut inner, &path, &size_only);
        }

        Ok(())
    }

    pub async fn seq(&self) -> u64 {
        self.inner.lock().await.seq
    }

    /// rotate the active log into a sealed segment when a size or age trigger fires.
    /// the running chain is untouched: seq and last_hmac stay, so the next appended entry
    /// links to the rotated segment's last hmac. returns the new segment when one was written.
    pub async fn maybe_rotate(
        &self,
        log_path: &Path,
        cfg: &RotateConfig,
    ) -> Result<Option<rotate::Segment>> {
        if !cfg.enabled() {
            return Ok(None);
        }
        let mut inner = self.inner.lock().await;
        Self::rotate_locked(&mut inner, log_path, cfg)
    }

    pub async fn enforce_segment_retention(&self, dir: &Path, retain_days: u32) -> Result<u64> {
        let _guard = self.inner.lock().await;
        rotate::enforce_segment_retention(dir, retain_days)
    }

    fn rotate_locked(
        inner: &mut LogInner,
        log_path: &Path,
        cfg: &RotateConfig,
    ) -> Result<Option<rotate::Segment>> {
        if !cfg.enabled() {
            return Ok(None);
        }

        let len = inner.file.metadata().map(|m| m.len()).unwrap_or(0);
        if len == 0 {
            return Ok(None);
        }

        let mut hit = cfg.max_bytes > 0 && len >= cfg.max_bytes;
        if !hit && cfg.max_age_days == 0 {
            return Ok(None);
        }

        let content = std::fs::read_to_string(log_path)?;
        if !hit && cfg.max_age_days > 0 {
            if let Some(ts) = rotate::oldest_ts(&content) {
                let cutoff = Utc::now() - chrono::Duration::days(cfg.max_age_days as i64);
                if ts < cutoff {
                    hit = true;
                }
            }
        }
        if !hit {
            return Ok(None);
        }

        let sum = match rotate::summarize(&content) {
            Some(s) => s,
            None => return Ok(None),
        };

        let dir = log_path.parent().unwrap_or_else(|| Path::new("."));
        inner.file.sync_all()?;
        let seg = rotate::seal_segment(dir, content.as_bytes(), &sum)?;

        let mut topts = std::fs::OpenOptions::new();
        topts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            topts.mode(0o600);
        }
        topts.open(log_path)?;

        let mut aopts = std::fs::OpenOptions::new();
        aopts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            aopts.mode(0o600);
        }
        inner.file = aopts.open(log_path)?;

        if let Some(parent) = log_path.parent() {
            if let Ok(d) = std::fs::File::open(parent) {
                let _ = d.sync_all();
            }
        }

        Ok(Some(seg))
    }

    pub fn read_recent(log_path: &Path, limit: usize) -> Result<Vec<AuditEntry>> {
        if !log_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(log_path)?;
        let mut entries: Vec<AuditEntry> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
        Ok(entries)
    }

    fn scan_tail(log_path: &Path, _key: &[u8]) -> Result<(u64, String)> {
        let mut seq = 0u64;
        let mut last_hmac = chain::GENESIS_HMAC.to_string();

        if log_path.exists() {
            let content = std::fs::read_to_string(log_path)?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<AuditEntry>(line) {
                    seq = entry.seq;
                    last_hmac = entry.hmac.clone();
                }
            }
        }

        if seq == 0 {
            let dir = log_path.parent().unwrap_or_else(|| Path::new("."));
            if let Some((rseq, rhmac)) = rotate::recover_tail(dir) {
                return Ok((rseq, rhmac));
            }
        }

        Ok((seq, last_hmac))
    }
}

pub fn default_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("audit.jsonl")
}

pub fn default_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("audit.key")
}
