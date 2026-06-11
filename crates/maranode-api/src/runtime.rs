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
    /// when true, full prompt and response text is written into the audit log
    pub log_prompts: bool,
    /// retention in days for content-logged entries (0 = no automatic pruning)
    pub content_log_retention_days: u32,
    pub smtp: Option<SmtpCfg>,
}

pub type SharedRuntime = Arc<RwLock<RuntimeSettings>>;

pub fn new_shared(settings: RuntimeSettings) -> SharedRuntime {
    Arc::new(RwLock::new(settings))
}
