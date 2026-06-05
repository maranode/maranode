//! default model specs offered on first run

use maranode_common::models::{ModelId, ModelType};

#[derive(Debug, Clone)]
pub struct DefaultModelSpec {
    pub model_id: ModelId,
    pub model_type: ModelType,
    pub hf_repo: &'static str,
    pub hf_filename: &'static str,
    pub local_filename: &'static str,
    pub quant: &'static str,
    pub size_hint: &'static str,
}

pub fn default_llm() -> DefaultModelSpec {
    DefaultModelSpec {
        model_id: ModelId::new("qwen2.5", "7b"),
        model_type: ModelType::Llm,
        hf_repo: "bartowski/Qwen2.5-7B-Instruct-GGUF",
        hf_filename: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        local_filename: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        quant: "Q4_K_M",
        size_hint: "~4.4 GB",
    }
}

pub fn default_embedding() -> DefaultModelSpec {
    DefaultModelSpec {
        model_id: ModelId::new("bge-m3", "latest"),
        model_type: ModelType::Embedding,
        hf_repo: "gpustack/bge-m3-GGUF",
        hf_filename: "bge-m3-Q4_K_M.gguf",
        local_filename: "bge-m3-Q4_K_M.gguf",
        quant: "Q4_K_M",
        size_hint: "~420 MB",
    }
}

impl DefaultModelSpec {
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.hf_repo, self.hf_filename
        )
    }
}
