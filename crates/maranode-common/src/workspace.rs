use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub api_key_hash: Option<String>,
    pub model_allowlist: Vec<String>,
    pub rate_limit_rpm: Option<u32>,
    pub system_prompt: Option<String>,
    pub created_at: DateTime<Utc>,
    /// true if Linux network namespace maranode-ws-{slug} exists for this workspace
    pub net_namespace: bool,
    /// max inference requests running at same time for this workspace
    pub max_concurrent_requests: Option<u32>,
    /// max different models loaded at same time for this workspace
    pub max_models: Option<u32>,
    /// max total loaded model size in bytes for this workspace
    pub max_memory_bytes: Option<u64>,
    /// hex-encoded 32-byte data-encryption key for this workspace.
    /// None means the DEK was destroyed (workspace shredded) or not yet generated.
    pub dek: Option<String>,
}

impl Workspace {
    pub fn allows_model(&self, name_tag: &str) -> bool {
        self.model_allowlist.is_empty() || self.model_allowlist.iter().any(|m| m == name_tag)
    }
}
