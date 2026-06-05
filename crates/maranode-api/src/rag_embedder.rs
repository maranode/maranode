//! connects local inference engine to rag embedder trait

use std::sync::Arc;

use maranode_common::models::ModelId;
use maranode_inference::InferenceEngine;
use maranode_rag::Embedder;
use maranode_store::ModelStore;
use anyhow::Result;

pub struct EngineEmbedder {
    engine: Arc<dyn InferenceEngine>,
    store: ModelStore,
    model: ModelId,
}

impl EngineEmbedder {
    pub fn new(engine: Arc<dyn InferenceEngine>, store: ModelStore, model: ModelId) -> Self {
        Self {
            engine,
            store,
            model,
        }
    }
}

#[async_trait::async_trait]
impl Embedder for EngineEmbedder {
    fn model_label(&self) -> String {
        self.model.to_string()
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let path = self
            .store
            .blob_path_verified(&self.model)
            .await
            .map_err(|e| anyhow::anyhow!("embedding model '{}' not available: {e}", self.model))?;
        self.engine.embed(&path, texts).await
    }
}
