//! check that the audit log HMAC chain is valid

use std::path::Path;

use anyhow::Result;

use maranode_common::events::AuditEntry;

use crate::chain;

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

/// verify HMAC chain for every entry in the log file
pub fn verify_log(log_path: &Path, key: &[u8]) -> Result<VerifyResult> {
    if !log_path.exists() {
        return Ok(VerifyResult {
            entries_checked: 0,
            ok: true,
            first_violation: None,
        });
    }

    let content = std::fs::read_to_string(log_path)?;
    let mut expected_seq = 1u64;
    let mut prev_hmac = chain::GENESIS_HMAC.to_string();
    let mut entries_checked = 0u64;

    for (line_no, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let entry: AuditEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                return Ok(VerifyResult {
                    entries_checked,
                    ok: false,
                    first_violation: Some(ViolationDetail {
                        seq: expected_seq,
                        detail: format!("JSON parse error at line {}: {}", line_no + 1, e),
                    }),
                });
            }
        };

        // sequence number must increase by one each line
        if entry.seq != expected_seq {
            return Ok(VerifyResult {
                entries_checked,
                ok: false,
                first_violation: Some(ViolationDetail {
                    seq: entry.seq,
                    detail: format!("sequence gap: expected {}, got {}", expected_seq, entry.seq),
                }),
            });
        }

        // prev_hmac must equal hmac from previous entry
        if entry.prev_hmac != prev_hmac {
            return Ok(VerifyResult {
                entries_checked,
                ok: false,
                first_violation: Some(ViolationDetail {
                    seq: entry.seq,
                    detail: format!(
                        "prev_hmac mismatch: expected {}, got {}",
                        prev_hmac, entry.prev_hmac
                    ),
                }),
            });
        }

        // build JSON body again and check HMAC matches
        let body = serde_json::json!({
            "ts":        entry.ts,
            "seq":       entry.seq,
            "actor":     entry.actor,
            "prev_hmac": entry.prev_hmac,
            "event":     serde_json::to_value(&entry.event)?,
        });
        let body_str = serde_json::to_string(&body)?;
        if !chain::verify(key, body_str.as_bytes(), &entry.hmac) {
            return Ok(VerifyResult {
                entries_checked,
                ok: false,
                first_violation: Some(ViolationDetail {
                    seq: entry.seq,
                    detail: "HMAC mismatch: entry has been tampered with".into(),
                }),
            });
        }

        prev_hmac = entry.hmac.clone();
        expected_seq += 1;
        entries_checked += 1;
    }

    Ok(VerifyResult {
        entries_checked,
        ok: true,
        first_violation: None,
    })
}
