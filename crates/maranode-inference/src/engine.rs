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

    /// string identifying the llama.cpp build baked in at compile time.
    /// format: "llama-cpp-2@<version>[+deterministic]"
    fn kernel_build_id(&self) -> String {
        kernel_build_id()
    }
}

/// compile-time kernel identity — same value for every engine instance
pub fn kernel_build_id() -> String {
    let version = env!("LLAMA_CPP_2_VERSION");
    #[cfg(deterministic_kernels)]
    return format!("llama-cpp-2@{version}+deterministic");
    #[cfg(not(deterministic_kernels))]
    format!("llama-cpp-2@{version}")
}

pub use async_trait::async_trait;
