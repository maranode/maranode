//! dummy inference backend for tests

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::warn;

use maranode_common::models::InferenceDevice;

use crate::engine::{async_trait, InferenceEngine};
use crate::types::{FinishReason, InferenceRequest, InferenceResponse, Token};

pub struct StubEngine;

#[async_trait]
impl InferenceEngine for StubEngine {
    async fn generate(&self, req: InferenceRequest) -> Result<InferenceResponse> {
        warn!("StubEngine: returning canned response (llama.cpp FFI not yet integrated)");
        let start = Instant::now();
        let content = format!(
            "[STUB] Maranode inference engine is not yet connected to llama.cpp. \
             Your request for model `{}` was received. \
             Replace StubEngine with LlamaCppEngine to get real inference.",
            req.model
        );
        Ok(InferenceResponse {
            request_id: req.request_id,
            model: req.model,
            content,
            tokens_in: 10,
            tokens_out: 42,
            duration_ms: start.elapsed().as_millis() as u64,
            device: InferenceDevice::Cpu,
            finish_reason: FinishReason::Stop,
        })
    }

    async fn generate_stream(&self, req: InferenceRequest, tx: mpsc::Sender<Result<Token>>) {
        let words = vec![
            "[STUB]",
            "llama.cpp",
            "FFI",
            "not",
            "yet",
            "wired.",
            "Model:",
            &req.model.name,
        ];
        for (i, word) in words.iter().enumerate() {
            let is_last = i == words.len() - 1;
            let token = Token {
                text: format!("{} ", word),
                is_last,
            };
            if tx.send(Ok(token)).await.is_err() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    async fn embed(&self, _model_path: &Path, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        warn!("StubEngine: returning deterministic fake embeddings");
        const DIM: usize = 64;
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; DIM];
                for (i, b) in t.bytes().enumerate() {
                    v[i % DIM] += (b as f32) / 255.0;
                }
                v[0] += 1.0;
                v
            })
            .collect())
    }

    fn device(&self) -> InferenceDevice {
        InferenceDevice::Cpu
    }

    async fn load_model(&self, model_id: &str, _path: &Path) -> Result<()> {
        warn!("StubEngine: ignoring model load for {}", model_id);
        Ok(())
    }

    async fn unload_model(&self, model_id: &str) -> Result<()> {
        warn!("StubEngine: ignoring model unload for {}", model_id);
        Ok(())
    }
}
