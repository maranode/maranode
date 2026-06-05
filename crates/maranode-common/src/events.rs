//! Payload types for audit log events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{InferenceDevice, ModelId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AuditEvent {
    DaemonStart {
        version: String,
        air_gap: bool,
    },
    DaemonStop {
        reason: String,
    },
    IsolationApplied {
        mode: IsolationMode,
    },
    ModelImported {
        model: ModelId,
        sha256: String,
        size_bytes: u64,
        source: ImportSource,
    },

    ModelRemoved {
        model: ModelId,
    },

    InferenceStart {
        request_id: String,
        model: ModelId,
        prompt_sha256: String,
        device: InferenceDevice,
    },

    InferenceComplete {
        request_id: String,
        model: ModelId,
        tokens_in: u32,
        tokens_out: u32,
        duration_ms: u64,
        device: InferenceDevice,
    },

    InferenceFailed {
        request_id: String,
        model: ModelId,
        reason: String,
    },

    RagDocumentIngested {
        collection: String,
        source: String,
        chunks: usize,
    },

    RagRetrieval {
        collection: String,
        query_sha256: String,
        hits: usize,
    },

    ConfigReloaded {
        path: String,
    },
    AuditVerified {
        entries: u64,
        ok: bool,
    },
    BinaryAttested {
        /// SHA-256 hash of running executable.
        binary_sha256: String,
        /// File path of running executable.
        binary_path: String,
        /// True if TPM was found and PCR values were read.
        tpm_available: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IsolationMode {
    AirGap,
    Whitelist,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Remote { url: String },
    Local { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: DateTime<Utc>,
    pub seq: u64,
    pub actor: String,
    #[serde(flatten)]
    pub event: AuditEvent,
    pub prev_hmac: String,
    pub hmac: String,
}
