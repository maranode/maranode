//! shared error type for all crates

use thiserror::Error;

pub type Result<T, E = MaranodeError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum MaranodeError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Model checksum mismatch: expected {expected}, got {actual}")]
    ModelChecksumMismatch { expected: String, actual: String },

    #[error("Inference failed: {0}")]
    InferenceFailed(String),

    #[error("Audit log integrity violation at sequence {seq}: {detail}")]
    AuditIntegrityViolation { seq: u64, detail: String },

    #[error("Audit log write failed: {0}")]
    AuditWriteFailed(String),

    #[error("Network isolation enforcement failed: {0}")]
    IsolationFailed(String),

    #[error("Air-gap mode is active: outbound network access is not allowed")]
    AirGapViolation,

    #[error("Model store error: {0}")]
    StoreError(String),

    #[error("Blob not found: {0}")]
    BlobNotFound(String),

    #[error("Request validation error: {0}")]
    ValidationError(String),

    #[error("Unsupported model: {0}")]
    UnsupportedModel(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
