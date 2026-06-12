//! Payload types for audit log events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{InferenceDevice, ModelId};
use crate::receipt::InferenceReceipt;

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
        /// full prompt content. only present when log_prompts is enabled
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },

    InferenceComplete {
        request_id: String,
        model: ModelId,
        tokens_in: u32,
        tokens_out: u32,
        duration_ms: u64,
        device: InferenceDevice,
        /// full response content. only present when log_prompts is enabled
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<String>,
    },

    InferenceFailed {
        request_id: String,
        model: ModelId,
        reason: String,
    },

    InferenceReceipt {
        receipt: InferenceReceipt,
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

    IsolationProbe {
        /// true if all egress probes were blocked (isolation intact)
        isolated: bool,
        /// list of hosts that were probed and whether they were reachable
        probe_results: Vec<ProbeResult>,
        /// iptables-save snapshot hash (empty string if not available)
        iptables_hash: String,
    },

    WorkspaceShredded {
        slug: String,
        actor: String,
        statement: String,
    },

    ConfigReloaded {
        path: String,
    },
    AuditVerified {
        entries: u64,
        ok: bool,
    },
    BinaryAttested {
        binary_sha256: String,
        binary_path: String,
        tpm_available: bool,
    },

    ModelBaselineChecked {
        model_id: String,
        model_sha256: String,
        vectors_run: usize,
        vectors_passed: usize,
        vectors_failed: usize,
        baseline_signer: String,
    },

    ModelDriftDetected {
        model_id: String,
        model_sha256: String,
        vectors_failed: usize,
        action_taken: String,
    },

    ModelApprovalGranted {
        model_id: String,
        model_sha256: String,
        approved_by: String,
        token_id: String,
        signer_pubkey: String,
    },

    ModelApprovalRevoked {
        model_id: String,
        model_sha256: String,
        revoked_by: String,
        token_id: String,
    },

    ModelLoadBlocked {
        model_id: String,
        model_sha256: String,
        reason: String,
    },

    DataClassificationViolation {
        workspace: String,
        collection: String,
        required_label: String,
        workspace_clearance: String,
        blocked: bool,
    },

    DataLabelAssigned {
        collection: String,
        label: String,
        assigned_by: String,
    },

    DlpSyncCompleted {
        provider: String,
        labels_imported: usize,
    },

    TpmKeySealed {
        purpose: String,
        backend: String, // "tpm2" or "software"
        pcr_list: Option<String>,
    },

    TpmUnsealFailed {
        purpose: String,
        reason: String,
        fail_count: u32,
    },

    TpmKeyRotated {
        purpose: String,
        reason: String,
        new_backend: String,
        new_pcr_list: Option<String>,
    },

    IncidentDeclared {
        incident_id: String,
        declared_by: String,
        reason: String,
        sessions_terminated: u32,
    },

    AuditFrozen {
        incident_id: String,
        frozen_by: String,
    },

    AuditUnfrozen {
        incident_id: String,
        unfrozen_by: String,
    },

    ForensicSnapshot {
        incident_id: String,
        snapshot_path: String,
        snapshot_sha256: String,
    },

    BreakGlassUsed {
        cred_id: String,
        used_by: String,
        purpose: String,
    },

    IncidentPhaseChanged {
        incident_id: String,
        old_phase: String,
        new_phase: String,
        changed_by: String,
        note: Option<String>,
    },

    IncidentResolved {
        incident_id: String,
        resolved_by: String,
        summary: String,
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
pub struct ProbeResult {
    pub host: String,
    pub port: u16,
    /// true means the connection succeeded — isolation is BROKEN for this host
    pub reachable: bool,
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
