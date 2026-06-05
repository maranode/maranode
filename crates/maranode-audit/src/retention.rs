use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::Utc;

use maranode_common::events::AuditEntry;

/// delete entries older than retain_days days. HMAC chain is broken from the first row that remains.
pub fn prune_log(log_path: &Path, retain_days: u32) -> Result<u64> {
    if retain_days == 0 {
        anyhow::bail!("retain_days must be >= 1; refusing to prune the entire audit log");
    }
    if !log_path.exists() {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::days(retain_days as i64);
    let content = std::fs::read_to_string(log_path)?;

    let mut kept: Vec<&str> = Vec::new();
    let mut pruned: u64 = 0;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Result<AuditEntry, _> = serde_json::from_str(line);
        match entry {
            Ok(e) if e.ts < cutoff => {
                pruned += 1;
            }
            _ => kept.push(line),
        }
    }

    if pruned > 0 {
        let tmp = log_path.with_extension("jsonl.tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            for line in &kept {
                f.write_all(line.as_bytes())?;
                f.write_all(b"\n")?;
            }
            f.sync_all()?;
        }
        std::fs::rename(&tmp, log_path)?;
        if let Some(parent) = log_path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }

    Ok(pruned)
}
