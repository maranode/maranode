//! core domain models used by all crates

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// model ID in form name:tag
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId {
    pub name: String,
    pub tag: String,
}

impl ModelId {
    pub fn new(name: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tag: tag.into(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (name, tag) = s.split_once(':')?;
        Some(Self::new(name, tag))
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.tag)
    }
}

/// stored metadata for one model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub id: Uuid,
    pub model_id: ModelId,
    pub sha256: String,
    pub size_bytes: u64,
    pub format: ModelFormat,
    pub quantization: Option<String>,
    pub imported_at: DateTime<Utc>,
    pub blob_path: String,
    #[serde(default)]
    pub model_type: ModelType,
    #[serde(default)]
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelFormat {
    Gguf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    #[default]
    Llm,
    Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceDevice {
    Cpu,
    Gpu,
    Metal,
    /// Intel NPU through OpenVINO
    Npu,
    /// AMD Ryzen AI NPU through XDNA driver
    #[serde(rename = "ryzenai")]
    RyzenAi,
}

impl InferenceDevice {
    pub fn is_accelerated(self) -> bool {
        !matches!(self, InferenceDevice::Cpu)
    }
}

impl std::fmt::Display for InferenceDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferenceDevice::Cpu => write!(f, "cpu"),
            InferenceDevice::Gpu => write!(f, "gpu"),
            InferenceDevice::Metal => write!(f, "metal"),
            InferenceDevice::Npu => write!(f, "npu"),
            InferenceDevice::RyzenAi => write!(f, "ryzenai"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceParams {
    pub model: ModelId,
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub stream: bool,
    pub stop: Option<Vec<String>>,
}

fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> u32 {
    2048
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_parse_and_display() {
        let m = ModelId::parse("llama3.2:3b").unwrap();
        assert_eq!(m.name, "llama3.2");
        assert_eq!(m.tag, "3b");
        assert_eq!(m.to_string(), "llama3.2:3b");
        // only the first colon splits; the remainder stays in the tag
        let m2 = ModelId::parse("host:5000:tag").unwrap();
        assert_eq!(m2.name, "host");
        assert_eq!(m2.tag, "5000:tag");
        assert!(ModelId::parse("no-colon").is_none());
    }

    #[test]
    fn device_acceleration_and_display() {
        assert!(!InferenceDevice::Cpu.is_accelerated());
        assert!(InferenceDevice::Gpu.is_accelerated());
        assert!(InferenceDevice::RyzenAi.is_accelerated());
        assert_eq!(InferenceDevice::Metal.to_string(), "metal");
        assert_eq!(InferenceDevice::RyzenAi.to_string(), "ryzenai");
    }

    #[test]
    fn serde_renames_and_defaults() {
        assert_eq!(
            serde_json::to_string(&InferenceDevice::RyzenAi).unwrap(),
            "\"ryzenai\""
        );
        assert_eq!(serde_json::to_string(&ModelType::Llm).unwrap(), "\"llm\"");
        assert_eq!(ModelType::default(), ModelType::Llm);

        let d: InferenceDevice = serde_json::from_str("\"npu\"").unwrap();
        assert_eq!(d, InferenceDevice::Npu);
        let r: ChatRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(r, ChatRole::Assistant);
    }
}
