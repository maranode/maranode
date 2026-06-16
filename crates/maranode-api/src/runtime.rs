//! daemon settings hot reload (reload without restart)

use std::sync::{Arc, RwLock};

use crate::state::IdentityConfig;
use crate::state::RagIngestPolicy;

#[derive(Debug, Clone)]
pub struct SmtpCfg {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub starttls: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    pub admin_key: Option<String>,
    pub rag_ingest_policy: RagIngestPolicy,
    pub rag_ingest_allowlist: Vec<String>,
    pub system_prompt: Option<String>,
    pub identity: IdentityConfig,
    pub air_gap: bool,
    pub log_prompts: bool,
    pub content_log_retention_days: u32,
    pub audit_max_mb: u64,
    pub audit_max_age_days: u32,
    pub metrics_enabled: bool,
    pub metrics_require_auth: bool,
    pub smtp: Option<SmtpCfg>,
    /// hex-encoded 32-byte AES-256-GCM key; when set, prompts and responses are encrypted
    /// at the API layer (TEE deployments). None = plaintext (default).
    pub tee_encrypt_key: Option<String>,
}

pub type SharedRuntime = Arc<RwLock<RuntimeSettings>>;

pub fn new_shared(settings: RuntimeSettings) -> SharedRuntime {
    Arc::new(RwLock::new(settings))
}
