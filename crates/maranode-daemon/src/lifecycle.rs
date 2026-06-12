//! track which models are loaded in the inference engine.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::info;

use maranode_audit::AuditLog;
use maranode_common::approval::ApprovalToken;
use maranode_common::events::AuditEvent;
use maranode_common::models::ModelId;
use maranode_inference::InferenceEngine;
use maranode_store::ModelStore;

use crate::baseline_check::BaselineChecker;

pub struct LifecycleManager {
    engine: Arc<dyn InferenceEngine>,
    store: ModelStore,
    data_dir: std::path::PathBuf,
    loaded: Arc<RwLock<HashMap<String, ModelId>>>,
    baseline: Option<Arc<BaselineChecker>>,
    audit: Option<AuditLog>,
    require_approval_token: bool,
    tokens_dir: Option<std::path::PathBuf>,
}

impl LifecycleManager {
    pub fn new(engine: Arc<dyn InferenceEngine>, store: ModelStore, data_dir: std::path::PathBuf) -> Self {
        Self {
            engine,
            store,
            data_dir,
            loaded: Arc::new(RwLock::new(HashMap::new())),
            baseline: None,
            audit: None,
            require_approval_token: false,
            tokens_dir: None,
        }
    }

    pub fn with_baseline(mut self, checker: BaselineChecker, audit: AuditLog) -> Self {
        self.baseline = Some(Arc::new(checker));
        self.audit = Some(audit);
        self
    }

    pub fn with_registry(mut self, require: bool, tokens_dir: Option<std::path::PathBuf>) -> Self {
        self.require_approval_token = require;
        self.tokens_dir = tokens_dir;
        self
    }

    /// load model if it is not loaded yet
    pub async fn ensure_loaded(&self, model_id: &ModelId) -> Result<()> {
        let key = model_id.to_string();
        if self.loaded.read().await.contains_key(&key) {
            return Ok(());
        }

        let path = self.store.blob_path_verified(model_id).await?;

        if self.require_approval_token {
            if let Ok(Some(manifest)) = self.store.get(model_id).await {
                let tokens_dir = self.tokens_dir.clone().unwrap_or_else(|| {
                    self.data_dir.join("approval-tokens")
                });
                let token_path = ApprovalToken::token_path(&tokens_dir, &manifest.sha256);
                let block_reason = if !token_path.exists() {
                    Some(format!("no approval token for sha256 {}", &manifest.sha256[..12]))
                } else {
                    match ApprovalToken::load(&token_path).and_then(|t| t.verify()) {
                        Ok(_) => None,
                        Err(e) => Some(format!("approval token invalid: {e}")),
                    }
                };
                if let Some(reason) = block_reason {
                    if let Some(audit) = &self.audit {
                        let _ = audit.append("lifecycle", AuditEvent::ModelLoadBlocked {
                            model_id: model_id.to_string(),
                            model_sha256: manifest.sha256.clone(),
                            reason: reason.clone(),
                        }).await;
                    }
                    anyhow::bail!("model '{}' load blocked: {reason}", model_id);
                }
            }
        }

        self.engine.load_model(&key, &path).await?;

        if let (Some(checker), Some(audit)) = (&self.baseline, &self.audit) {
            if let Ok(Some(manifest)) = self.store.get(model_id).await {
                checker
                    .check(model_id, &manifest.sha256, &path, &self.engine, audit)
                    .await?;
            }
        }

        self.loaded
            .write()
            .await
            .insert(key.clone(), model_id.clone());
        info!("loaded model {}", key);
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
