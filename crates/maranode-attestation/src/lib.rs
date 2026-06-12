pub mod binary;
pub mod pcr_policy;
pub mod perf;
pub mod report;
pub mod rotation;
pub mod seal;
pub mod tee;
pub mod tpm;

pub use pcr_policy::PcrPolicy;
pub use perf::{TeePerf, measure as measure_tee_perf};
pub use report::{AttestationReport, AuditLogMeasurement};
pub use rotation::{RecoveryBundle, RotationRecord, export_recovery_bundle, import_recovery_bundle, rotate_in_place, read_rotation_log};
pub use seal::{SealBackend, SealMeta, is_sealed, is_tpm2_tools_available, seal, seal_status, unseal};
pub use tee::{TeeReport, TeeType, detect_tee, get_report as get_tee_report};

pub fn tee_keygen() -> String {
    use aes_gcm::aead::rand_core::RngCore;
    let mut bytes = [0u8; 32];
    aes_gcm::aead::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub use tpm::TpmResult;
