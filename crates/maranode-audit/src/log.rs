//! main handle for the audit log: [`AuditLog`]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::Mutex;

use maranode_common::events::{AuditEntry, AuditEvent};

use crate::chain;

#[derive(Clone)]
pub struct AuditLog {
    inner: Arc<Mutex<LogInner>>,
}

struct LogInner {
    file: std::fs::File,
    seq: u64,
    last_hmac: String,
    key: Vec<u8>,
}

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
            })),
        })
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

        Ok(())
    }

    pub async fn seq(&self) -> u64 {
        self.inner.lock().await.seq
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
        if !log_path.exists() {
            return Ok((0, chain::GENESIS_HMAC.to_string()));
        }

        let content = std::fs::read_to_string(log_path)?;
        let mut seq = 0u64;
        let mut last_hmac = chain::GENESIS_HMAC.to_string();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<AuditEntry>(line) {
                seq = entry.seq;
                last_hmac = entry.hmac.clone();
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
