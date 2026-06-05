//! append-only JSONL audit log. each line is linked with HMAC. key file is audit.key in the data directory.

pub mod bundle;
pub mod chain;
pub mod export;
pub mod key;
pub mod log;
pub mod retention;
pub mod verify;

pub use log::AuditLog;
pub use verify::VerifyResult;
