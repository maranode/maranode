//! [`InferenceEngine`] trait and logic to pick CPU/GPU/NPU device

use anyhow::Result;
use tokio::sync::mpsc;

use maranode_common::models::InferenceDevice;

use crate::types::{InferenceRequest, InferenceResponse, Token};

/// backend that runs model inference
#[async_trait::async_trait]
pub trait InferenceEngine: Send + Sync {
    async fn generate(&self, req: InferenceRequest) -> Result<InferenceResponse>;

    async fn generate_stream(&self, req: InferenceRequest, tx: mpsc::Sender<Result<Token>>);

    async fn embed(
        &self,
        _model_path: &std::path::Path,
        _texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        anyhow::bail!("this inference engine does not support embeddings")
    }

    fn device(&self) -> InferenceDevice;

    async fn load_model(&self, model_id: &str, path: &std::path::Path) -> Result<()>;

    async fn unload_model(&self, model_id: &str) -> Result<()>;

    fn queue_depth(&self) -> usize {
        0
    }

    fn max_queue_depth(&self) -> usize {
        0
    }
}

pub use async_trait::async_trait;
