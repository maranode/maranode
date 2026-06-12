use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeeType {
    IntelTdx,
    AmdSevSnp,
    None,
}

impl std::fmt::Display for TeeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeeType::IntelTdx => write!(f, "intel_tdx"),
            TeeType::AmdSevSnp => write!(f, "amd_sev_snp"),
            TeeType::None => write!(f, "none"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeeReport {
    pub tee_type: TeeType,
    pub report_data: String, // hex-encoded raw report or synthetic hash
    pub report_hash: String, // sha256 of report_data
    pub measurement: String, // hex MRTD (TDX) or measurement (SEV-SNP)
    pub is_synthetic: bool,  // true when running outside a real TEE
}

pub fn detect_tee() -> TeeType {
    // Intel TDX: check for /dev/tdx-guest or /dev/tdx_guest
    if std::path::Path::new("/dev/tdx-guest").exists()
        || std::path::Path::new("/dev/tdx_guest").exists()
    {
        return TeeType::IntelTdx;
    }
    // AMD SEV-SNP: check /dev/sev-guest
    if std::path::Path::new("/dev/sev-guest").exists() {
        return TeeType::AmdSevSnp;
    }
    TeeType::None
}

pub fn get_report(nonce: &[u8]) -> TeeReport {
    let tee_type = detect_tee();

    match tee_type {
        TeeType::IntelTdx => get_tdx_report(nonce),
        TeeType::AmdSevSnp => get_snp_report(nonce),
        TeeType::None => synthetic_report(nonce),
    }
}

fn get_tdx_report(nonce: &[u8]) -> TeeReport {
    use sha2::{Digest, Sha256};
    let result = try_tdx_ioctl(nonce);
    match result {
        Some(raw) => {
            let hash = hex::encode(Sha256::digest(&raw));
            let measurement = hex::encode(&raw[..48.min(raw.len())]);
            TeeReport {
                tee_type: TeeType::IntelTdx,
                report_data: hex::encode(&raw),
                report_hash: hash,
                measurement,
                is_synthetic: false,
            }
        }
        None => {
            let mut s = synthetic_report(nonce);
            s.tee_type = TeeType::IntelTdx;
            s
        }
    }
}

fn get_snp_report(nonce: &[u8]) -> TeeReport {
    use sha2::{Digest, Sha256};
    let result = try_snp_ioctl(nonce);
    match result {
        Some(raw) => {
            let hash = hex::encode(Sha256::digest(&raw));
            let measurement = hex::encode(&raw[..48.min(raw.len())]);
            TeeReport {
                tee_type: TeeType::AmdSevSnp,
                report_data: hex::encode(&raw),
                report_hash: hash,
                measurement,
                is_synthetic: false,
            }
        }
        None => {
            let mut s = synthetic_report(nonce);
            s.tee_type = TeeType::AmdSevSnp;
            s
        }
    }
}

fn synthetic_report(nonce: &[u8]) -> TeeReport {
    use sha2::{Digest, Sha256};
    // in development/non-TEE environments, produce a deterministic synthetic report
    // using a hash of the binary + nonce so it's reproducible
    let binary_hash = crate::binary::measure_self()
        .map(|m| m.sha256)
        .unwrap_or_else(|_| "unknown".into());
    let combined = format!("{binary_hash}:{}", hex::encode(nonce));
    let report_data = hex::encode(Sha256::digest(combined.as_bytes()));
    let report_hash = hex::encode(Sha256::digest(report_data.as_bytes()));
    TeeReport {
        tee_type: TeeType::None,
        report_data: report_data.clone(),
        report_hash,
        measurement: report_data,
        is_synthetic: true,
    }
}

fn try_tdx_ioctl(_nonce: &[u8]) -> Option<Vec<u8>> {
    // real implementation would use ioctl(TDX_CMD_GET_REPORT0) on /dev/tdx-guest
    // kernel driver support needed; return None here so we fall through to synthetic
    None
}

fn try_snp_ioctl(_nonce: &[u8]) -> Option<Vec<u8>> {
    // real implementation would use ioctl(SNP_GET_REPORT) on /dev/sev-guest
    None
}
