//! check that the audit log HMAC chain is valid, across rotated segments and the active file.

use std::path::Path;

use anyhow::Result;

use maranode_common::events::AuditEntry;

use crate::chain;
use crate::rotate;

#[derive(Debug)]
pub struct VerifyResult {
    pub entries_checked: u64,
    pub ok: bool,
    pub first_violation: Option<ViolationDetail>,
}

#[derive(Debug)]
pub struct ViolationDetail {
    pub seq: u64,
    pub detail: String,
}

struct Cursor {
    expected_seq: u64,
    prev_hmac: String,
    checked: u64,
}

fn walk(content: &str, key: &[u8], cur: &mut Cursor) -> Result<Option<ViolationDetail>> {
    for (line_no, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let entry: AuditEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                return Ok(Some(ViolationDetail {
                    seq: cur.expected_seq,
                    detail: format!("JSON parse error at line {}: {}", line_no + 1, e),
                }));
            }
        };

        if entry.seq != cur.expected_seq {
            return Ok(Some(ViolationDetail {
                seq: entry.seq,
                detail: format!("sequence gap: expected {}, got {}", cur.expected_seq, entry.seq),
            }));
        }

        if entry.prev_hmac != cur.prev_hmac {
            return Ok(Some(ViolationDetail {
                seq: entry.seq,
                detail: format!(
                    "prev_hmac mismatch: expected {}, got {}",
                    cur.prev_hmac, entry.prev_hmac
                ),
            }));
        }

        let body = serde_json::json!({
            "ts":        entry.ts,
            "seq":       entry.seq,
            "actor":     entry.actor,
            "prev_hmac": entry.prev_hmac,
            "event":     serde_json::to_value(&entry.event)?,
        });
        let body_str = serde_json::to_string(&body)?;
        if !chain::verify(key, body_str.as_bytes(), &entry.hmac) {
            return Ok(Some(ViolationDetail {
                seq: entry.seq,
                detail: "HMAC mismatch: entry has been tampered with".into(),
            }));
        }

        cur.prev_hmac = entry.hmac.clone();
        cur.expected_seq = entry.seq + 1;
        cur.checked += 1;
    }
    Ok(None)
}

fn anchor(content: &str) -> Cursor {
    let first = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| serde_json::from_str::<AuditEntry>(l).ok());

    match first {
        Some(e) if e.seq > 1 => Cursor {
            expected_seq: e.seq,
            prev_hmac: e.prev_hmac,
            checked: 0,
        },
        _ => Cursor {
            expected_seq: 1,
            prev_hmac: chain::GENESIS_HMAC.to_string(),
            checked: 0,
        },
    }
}

/// verify the HMAC chain of a single log file in isolation.
pub fn verify_log(log_path: &Path, key: &[u8]) -> Result<VerifyResult> {
    if !log_path.exists() {
        return Ok(VerifyResult {
            entries_checked: 0,
            ok: true,
            first_violation: None,
        });
    }

    let content = std::fs::read_to_string(log_path)?;
    let mut cur = anchor(&content);
    let violation = walk(&content, key, &mut cur)?;

    Ok(VerifyResult {
        entries_checked: cur.checked,
        ok: violation.is_none(),
        first_violation: violation,
    })
}

pub fn verify_all(audit_dir: &Path, key: &[u8], active_log: &Path) -> Result<VerifyResult> {
    let mut manifest = rotate::load_manifest(audit_dir)?;
    manifest.segments.sort_by_key(|s| s.seq_start);

    if manifest.segments.is_empty() {
        return verify_log(active_log, key);
    }

    let first = &manifest.segments[0];
    let mut cur = Cursor {
        expected_seq: first.seq_start,
        prev_hmac: if first.seq_start == 1 {
            chain::GENESIS_HMAC.to_string()
        } else {
            first.first_prev_hmac.clone()
        },
        checked: 0,
    };

    for seg in &manifest.segments {
        let content = rotate::read_segment(audit_dir, seg)?;
        if let Some(v) = walk(&content, key, &mut cur)? {
            return Ok(VerifyResult {
                entries_checked: cur.checked,
                ok: false,
                first_violation: Some(v),
            });
        }
        if cur.prev_hmac != seg.last_hmac {
            return Ok(VerifyResult {
                entries_checked: cur.checked,
                ok: false,
                first_violation: Some(ViolationDetail {
                    seq: seg.seq_end,
                    detail: format!("segment {} last_hmac does not match its content", seg.file),
                }),
            });
        }
    }

    if active_log.exists() {
        let content = std::fs::read_to_string(active_log)?;
        if let Some(v) = walk(&content, key, &mut cur)? {
            return Ok(VerifyResult {
                entries_checked: cur.checked,
                ok: false,
                first_violation: Some(v),
            });
        }
    }

    Ok(VerifyResult {
        entries_checked: cur.checked,
        ok: true,
        first_violation: None,
    })
}
