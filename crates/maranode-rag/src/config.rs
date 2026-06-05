//! configuration values for RAG

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RagConfig {
    pub enabled: bool,
    pub embedding_model: String,
    pub default_collection: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub top_k: usize,
    pub min_score: f32,
    pub max_context_chars: usize,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            embedding_model: "nomic-embed-text:latest".into(),
            default_collection: "default".into(),
            chunk_size: 1200,
            chunk_overlap: 200,
            top_k: 3,
            min_score: 0.25,
            max_context_chars: 4000,
        }
    }
}

impl RagConfig {
    pub fn disabled() -> Self {
        Self::default()
    }
}
