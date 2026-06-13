use base64::Engine;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use maranode_common::events::AuditEvent;

use crate::error::ApiError;
use crate::hold_recovery::export_hold_backup;
use crate::legal_hold::{
    generate_hold_key, guard_retention_delete, list_holds, place_hold, release_hold,
    sign_release_payload,
};
use crate::state::AppState;
use crate::user_ctx::UserCtx;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/legal-hold/generate-key", post(generate_key))
        .route("/v1/legal-hold/place", post(place))
        .route("/v1/legal-hold/release/:id", post(release))
        .route("/v1/legal-hold/sign-release", post(sign_release))
        .route("/v1/legal-hold/list", get(list))
}

#[derive(Deserialize)]
struct GenerateKeyRequest {
    #[serde(default)]
    tpm_seal: bool,
    #[serde(default)]
    tpm_passphrase: Option<String>,
    #[serde(default)]
    org_name: Option<String>,
}

#[derive(Serialize)]
struct GenerateKeyResponse {
    pubkey_b64: String,
    // private key returned ONCE — caller must store offline
    privkey_hex: String,
    recovery_card_path: Option<String>,
    tpm_sealed: bool,
}

async fn generate_key(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<GenerateKeyRequest>,
) -> Result<Json<GenerateKeyResponse>, ApiError> {
    let (privkey_hex, pubkey_b64) = generate_hold_key(&state.data_dir)?;

    let mut tpm_sealed = false;
    if req.tpm_seal {
        let pass = req.tpm_passphrase.as_deref().unwrap_or("");
        let key_bytes = hex::decode(&privkey_hex)?;
        maranode_attestation::seal(&key_bytes, "hold-key", &state.data_dir, None, pass)?;
        tpm_sealed = true;
    }

    let recovery_card_path = {
        let org = req.org_name.as_deref().unwrap_or("your organization");
        export_hold_backup(&state.data_dir, &privkey_hex, &pubkey_b64).ok();
        Some(
            state.data_dir.join("legal-holds").join("HOLD-KEY-RECOVERY.txt")
                .to_string_lossy()
                .to_string(),
        )
    };

    state.audit.append(
        user.username(),
        AuditEvent::LegalHoldKeyGenerated {
            pubkey_hex: hex::encode(
                base64::engine::general_purpose::STANDARD
                    .decode(&pubkey_b64)
                    .unwrap_or_default(),
            ),
            generated_by: user.username().to_string(),
            tpm_sealed,
        },
    ).await.ok();

    tracing::warn!(
        actor = user.username(),
        "Legal hold key generated — private key returned ONCE; store offline"
    );

    Ok(Json(GenerateKeyResponse {
        pubkey_b64,
        privkey_hex,
        recovery_card_path,
        tpm_sealed,
    }))
}

#[derive(Deserialize)]
struct PlaceRequest {
    reason: String,
    seq_from: u64,
    seq_to: u64,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    key_hex: Option<String>,
    #[serde(default)]
    tpm_seal: bool,
    #[serde(default)]
    tpm_passphrase: Option<String>,
}

async fn place(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<PlaceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.seq_to < req.seq_from {
        return Err(ApiError::bad_request("seq_to must be >= seq_from"));
    }

    let hold_id = new_hold_id();
    let mut key_hex = req.key_hex.clone();

    if req.tpm_seal && key_hex.is_none() {
        let pass = req.tpm_passphrase.as_deref().unwrap_or("");
        let bytes = maranode_attestation::unseal("hold-key", &state.data_dir, pass)?;
        key_hex = Some(hex::encode(&bytes));
    }

    let record = place_hold(
        &state.data_dir,
        &hold_id,
        user.username(),
        &req.reason,
        req.seq_from,
        req.seq_to,
        req.expires_at,
        key_hex.as_deref(),
        req.tpm_seal,
    )?;

    state.audit.append(
        user.username(),
        AuditEvent::LegalHoldPlaced {
            hold_id: record.id.clone(),
            placed_by: user.username().to_string(),
            seq_from: record.seq_from,
            seq_to: record.seq_to,
            reason: record.reason.clone(),
            tpm_sealed: record.tpm_sealed,
        },
    ).await.ok();

    Ok(Json(serde_json::json!({
        "hold_id": record.id,
        "seq_from": record.seq_from,
        "seq_to": record.seq_to,
        "placed_at": record.placed_at,
        "tpm_sealed": record.tpm_sealed,
    })))
}

#[derive(Deserialize)]
struct ReleaseRequest {
    release_sig_b64: String,
}

async fn release(
    State(state): State<AppState>,
    user: UserCtx,
    Path(hold_id): Path<String>,
    Json(req): Json<ReleaseRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let record = release_hold(
        &state.data_dir,
        &hold_id,
        user.username(),
        &req.release_sig_b64,
    ).map_err(|e| ApiError::bad_request(&e.to_string()))?;

    state.audit.append(
        user.username(),
        AuditEvent::LegalHoldReleased {
            hold_id: record.id.clone(),
            released_by: user.username().to_string(),
        },
    ).await.ok();

    Ok(Json(serde_json::json!({
        "hold_id": record.id,
        "released_at": record.released_at,
        "released_by": record.released_by,
    })))
}

#[derive(Deserialize)]
struct SignReleaseRequest {
    hold_id: String,
    released_by: String,
    #[serde(default)]
    released_at: Option<DateTime<Utc>>,
    privkey_hex: String,
}

async fn sign_release(
    Json(req): Json<SignReleaseRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let at = req.released_at.unwrap_or_else(Utc::now);
    let sig = sign_release_payload(&req.hold_id, &req.released_by, at, &req.privkey_hex)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    Ok(Json(serde_json::json!({
        "hold_id": req.hold_id,
        "released_by": req.released_by,
        "released_at": at,
        "release_sig_b64": sig,
    })))
}

async fn list(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let holds = list_holds(&state.data_dir)?;
    let active: Vec<_> = holds.iter().filter(|h| h.is_active()).collect();
    Ok(Json(serde_json::json!({
        "total": holds.len(),
        "active": active.len(),
        "holds": holds,
    })))
}

fn new_hold_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("HOLD-{ts:x}")
}
