//! track which models are loaded in the inference engine.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::info;

use maranode_common::models::ModelId;
use maranode_inference::InferenceEngine;
use maranode_store::ModelStore;

pub struct LifecycleManager {
    engine: Arc<dyn InferenceEngine>,
    store: ModelStore,
    loaded: Arc<RwLock<HashMap<String, ModelId>>>,
}

impl LifecycleManager {
    pub fn new(engine: Arc<dyn InferenceEngine>, store: ModelStore) -> Self {
        Self {
            engine,
            store,
            loaded: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// load model if it is not loaded yet
    pub async fn ensure_loaded(&self, model_id: &ModelId) -> Result<()> {
        let key = model_id.to_string();
        if self.loaded.read().await.contains_key(&key) {
            return Ok(());
        }

        let path = self.store.blob_path_verified(model_id).await?;
        self.engine.load_model(&key, &path).await?;
        self.loaded
            .write()
            .await
            .insert(key.clone(), model_id.clone());
        info!("Loaded model {}", key);
        Ok(())
    }

    /// unload model from inference engine
    pub async fn unload(&self, model_id: &ModelId) -> Result<()> {
        let key = model_id.to_string();
        self.engine.unload_model(&key).await?;
        self.loaded.write().await.remove(&key);
        info!("Unloaded model {}", key);
        Ok(())
    }

    /// return list of loaded model names
    pub async fn loaded_models(&self) -> Vec<String> {
        self.loaded.read().await.keys().cloned().collect()
    }
}
