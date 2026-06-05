use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::binary::{BinaryMeasurement, measure_path, measure_self};
use crate::tpm::{read_pcrs, TpmResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    /// version number of this report JSON schema
    pub version: u8,
    pub generated_at: String,
    pub binary: BinaryMeasurement,
    pub tpm: TpmResult,
    pub audit_log: Option<AuditLogMeasurement>,
    /// SHA-256 hash of report JSON without this field; can be signed externally
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogMeasurement {
    pub path: String,
    pub sha256: String,
    pub entries: u64,
    pub chain_ok: bool,
    pub chain_detail: Option<String>,
}

impl AttestationReport {
    /// build full attestation report for the running process.
    /// pass audit_log_path and audit_key_path to include audit section; pass None to skip it.
    pub fn generate(
        audit_log_path: Option<&Path>,
        audit_key_path: Option<&Path>,
    ) -> Result<Self> {
        let binary = measure_self()?;
        let tpm = read_pcrs();
        let audit_log = audit_log_path
            .map(|lp| measure_audit_log(lp, audit_key_path))
            .transpose()?;

        let mut report = Self {
            version: 1,
            generated_at: Utc::now().to_rfc3339(),
            binary,
            tpm,
            audit_log,
            report_sha256: None,
        };

        report.report_sha256 = Some(report.self_hash()?);
        Ok(report)
    }

    fn self_hash(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        // json without report_sha256 field, then hash that bytes
        let without_hash = serde_json::json!({
            "version": self.version,
            "generated_at": self.generated_at,
            "binary": self.binary,
            "tpm": self.tpm,
            "audit_log": self.audit_log,
        });
        let bytes = serde_json::to_vec(&without_hash)?;
        Ok(hex::encode(Sha256::digest(&bytes)))
    }
}

fn measure_audit_log(log_path: &Path, key_path: Option<&Path>) -> Result<AuditLogMeasurement> {
    if !log_path.exists() {
        return Ok(AuditLogMeasurement {
            path: log_path.display().to_string(),
            sha256: String::new(),
            entries: 0,
            chain_ok: true,
            chain_detail: Some("log not yet created".into()),
        });
    }

    let m = measure_path(log_path)?;

    let entries = maranode_audit::AuditLog::read_recent(log_path, usize::MAX)
        .map(|v| v.len() as u64)
        .unwrap_or(0);

    let (chain_ok, chain_detail) = if let Some(kp) = key_path {
        match maranode_audit::key::load(kp) {
            Ok(key) => match maranode_audit::verify::verify_log(log_path, &key) {
                Ok(r) => {
                    let detail = r.first_violation.as_ref().map(|v| v.detail.clone());
                    (r.ok, detail)
                }
                Err(e) => (false, Some(e.to_string())),
            },
            Err(e) => (false, Some(format!("key load failed: {}", e))),
        }
    } else {
        (true, Some("chain verification skipped: no key path provided".into()))
    };

    Ok(AuditLogMeasurement {
        path: m.path,
        sha256: m.sha256,
        entries,
        chain_ok,
        chain_detail,
    })
}
