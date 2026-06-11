use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const RECEIPT_VERSION: u32 = 1;

/// one RAG chunk that was retrieved and used to ground the answer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub chunk_id: String,
    pub doc_id: String,
    pub source: String,
    pub doc_sha256: String,
    pub chunk_hash: String,
    pub score: f32,
}

/// environment snapshot at inference time, so the receipt can be reproduced.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvFingerprint {
    /// llama.cpp build id, e.g. "llama-cpp-2@0.1.146+deterministic"
    #[serde(default)]
    pub kernel_build_id: String,
    /// cpu threads available to the inference process
    #[serde(default)]
    pub thread_count: u32,
    /// device class: cpu, gpu, metal, npu, ryzenai
    #[serde(default)]
    pub device_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodeParams {
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    pub deterministic: bool,
}

/// versioned proof receipt for one inference call.
/// the signature covers all fields except `signature` itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceReceipt {
    pub version: u32,
    pub receipt_id: Uuid,
    pub request_id: String,

    pub timestamp: DateTime<Utc>,

    pub model_id: String,
    pub model_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_quant: Option<String>,

    pub input_sha256: String,
    pub output_sha256: String,

    pub decode_params: DecodeParams,

    pub tokens_in: u32,
    pub tokens_out: u32,

    pub signing_key_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tpm_pcr: Option<String>,

    /// environment snapshot: kernel build, thread count, device class
    #[serde(default)]
    pub env: EnvFingerprint,

    /// RAG source chunks used to ground this answer; empty if no RAG was used
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRef>,

    /// true if at least one RAG source was retrieved above min_score threshold
    #[serde(default)]
    pub grounded: bool,

    /// hex-encoded ed25519 signature over the canonical bytes (see `canonical_bytes`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl InferenceReceipt {
    /// bytes that are signed: stable JSON with `signature` field removed.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut copy = self.clone();
        copy.signature = None;
        serde_json::to_vec(&copy).expect("receipt serialization should never fail")
    }

    /// compute sha256 of a sequence of chat messages for `input_sha256`.
    pub fn hash_messages(messages: &[impl Serialize]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for m in messages {
            let b = serde_json::to_vec(m).unwrap_or_default();
            h.update(&b);
        }
        format!("{:x}", h.finalize())
    }

    /// compute sha256 of an output string for `output_sha256`.
    pub fn hash_output(text: &str) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(text.as_bytes()))
    }
}
