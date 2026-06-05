//! daemon settings hot reload (reload without restart)

use std::sync::{Arc, RwLock};

use crate::state::IdentityConfig;
use crate::state::RagIngestPolicy;

#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    pub admin_key: Option<String>,
    pub rag_ingest_policy: RagIngestPolicy,
    pub rag_ingest_allowlist: Vec<String>,
    pub system_prompt: Option<String>,
    pub identity: IdentityConfig,
    pub air_gap: bool,
}

pub type SharedRuntime = Arc<RwLock<RuntimeSettings>>;

pub fn new_shared(settings: RuntimeSettings) -> SharedRuntime {
    Arc::new(RwLock::new(settings))
}
