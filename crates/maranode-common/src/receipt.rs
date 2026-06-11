use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const RECEIPT_VERSION: u32 = 1;

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
