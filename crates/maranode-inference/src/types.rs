//! request and response types for inference (internal format, not OpenAI API)

use std::path::PathBuf;

use maranode_common::models::{ChatMessage, InferenceDevice, ModelId};
use serde::{Deserialize, Serialize};

/// one inference request sent to the engine
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub request_id: String,
    pub model: ModelId,
    pub model_path: PathBuf,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub stop_sequences: Vec<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub request_id: String,
    pub model: ModelId,
    pub content: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub duration_ms: u64,
    pub device: InferenceDevice,
    pub finish_reason: FinishReason,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub text: String,
    pub is_last: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    Error,
}
