use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use maranode_common::events::AuditEvent;
use maranode_common::incident::{IncidentPhase, IncidentState};

use crate::error::ApiError;
use crate::incident::{
    generate_break_glass_cred, load_break_glass_creds, new_incident_handle, notify_webhooks,
    persist_incident, save_break_glass_cred, transition_phase, verify_and_consume_break_glass,
};
use crate::forensic::take_snapshot;
use crate::state::AppState;
use crate::user_ctx::UserCtx;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/incident/declare", post(declare))
        .route("/v1/incident/investigate", post(investigate))
        .route("/v1/incident/resolve", post(resolve))
        .route("/v1/incident/status", axum::routing::get(status))
        .route("/v1/incident/snapshot", post(snapshot))
        .route("/v1/incident/break-glass/generate", post(bg_generate))
        .route("/v1/incident/break-glass/use", post(bg_use))
}

#[derive(Deserialize)]
struct DeclareRequest {
    reason: String,
    #[serde(default)]
    webhook_urls: Vec<String>,
}

#[derive(Serialize)]
struct DeclareResponse {
    incident_id: String,
    phase: String,
    audit_frozen: bool,
}

async fn declare(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<DeclareRequest>,
) -> Result<Json<DeclareResponse>, ApiError> {
    let mut guard = state.incident.lock().await;
    if let Some(inc) = guard.as_ref() {
        if inc.phase != IncidentPhase::Resolved {
            return Err(ApiError::bad_request("an incident is already active"));
        }
    }

    // terminate active sessions by setting a quarantine flag — inference
    // handler checks audit_frozen and incident state before proceeding
    state.audit_frozen.store(true, Ordering::SeqCst);

    let id = new_incident_id();
    let webhooks = req.webhook_urls.clone();
    let incident = IncidentState::new(id.clone(), user.username().to_string(), req.reason, webhooks);

    persist_incident(&incident, &state.data_dir).await.ok();
    notify_webhooks(&incident).await;

    let sessions_terminated = {
        let usage = state.workspace_usage.lock().await;
        usage.values().map(|u| u.concurrent).sum::<u32>()
    };

    state.audit.append(
        user.username(),
        AuditEvent::IncidentDeclared {
            incident_id: id.clone(),
            declared_by: user.username().to_string(),
            reason: incident.reason.clone(),
            sessions_terminated,
        },
    ).await.ok();

    state.audit.append(
        user.username(),
        AuditEvent::AuditFrozen {
            incident_id: id.clone(),
            frozen_by: user.username().to_string(),
        },
    ).await.ok();

    let resp = DeclareResponse {
        incident_id: id,
        phase: incident.phase.to_string(),
        audit_frozen: incident.audit_frozen,
    };
    *guard = Some(incident);

    Ok(Json(resp))
}

#[derive(Deserialize)]
struct PhaseRequest {
    note: Option<String>,
}

#[derive(Serialize)]
struct PhaseResponse {
    incident_id: String,
    phase: String,
}

async fn investigate(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<PhaseRequest>,
) -> Result<Json<PhaseResponse>, ApiError> {
    let mut guard = state.incident.lock().await;
    let incident = guard.as_mut().ok_or_else(|| ApiError::not_found("no active incident"))?;

    if incident.phase != IncidentPhase::Declared {
        return Err(ApiError::bad_request("incident must be in 'declared' phase"));
    }

    let id = incident.id.clone();
    let old_phase = incident.phase.to_string();
    transition_phase(incident, IncidentPhase::Investigating, user.username(), req.note.clone(), &state.data_dir).await
        .map_err(ApiError::internal)?;

    state.audit.append(
        user.username(),
        AuditEvent::IncidentPhaseChanged {
            incident_id: id.clone(),
            old_phase,
            new_phase: "investigating".to_string(),
            changed_by: user.username().to_string(),
            note: req.note,
        },
    ).await.ok();

    Ok(Json(PhaseResponse { incident_id: id, phase: "investigating".to_string() }))
}

#[derive(Deserialize)]
struct ResolveRequest {
    summary: String,
}

async fn resolve(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<PhaseResponse>, ApiError> {
    let mut guard = state.incident.lock().await;
    let incident = guard.as_mut().ok_or_else(|| ApiError::not_found("no active incident"))?;

    let id = incident.id.clone();
    let old_phase = incident.phase.to_string();
    transition_phase(incident, IncidentPhase::Resolved, user.username(), Some(req.summary.clone()), &state.data_dir).await
        .map_err(ApiError::internal)?;

    state.audit_frozen.store(false, Ordering::SeqCst);

    state.audit.append(
        user.username(),
        AuditEvent::IncidentPhaseChanged {
            incident_id: id.clone(),
            old_phase,
            new_phase: "resolved".to_string(),
            changed_by: user.username().to_string(),
            note: Some(req.summary.clone()),
        },
    ).await.ok();

    state.audit.append(
        user.username(),
        AuditEvent::AuditUnfrozen {
            incident_id: id.clone(),
            unfrozen_by: user.username().to_string(),
        },
    ).await.ok();

    state.audit.append(
        user.username(),
        AuditEvent::IncidentResolved {
            incident_id: id.clone(),
            resolved_by: user.username().to_string(),
            summary: req.summary,
        },
    ).await.ok();

    notify_webhooks(guard.as_ref().unwrap()).await;

    Ok(Json(PhaseResponse { incident_id: id, phase: "resolved".to_string() }))
}

async fn status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let guard = state.incident.lock().await;
    let frozen = state.audit_frozen.load(Ordering::Relaxed);
    match guard.as_ref() {
        None => Ok(Json(serde_json::json!({ "active": false, "audit_frozen": frozen }))),
        Some(inc) => Ok(Json(serde_json::json!({
            "active": inc.phase != IncidentPhase::Resolved,
            "incident_id": inc.id,
            "phase": inc.phase.to_string(),
            "declared_at": inc.declared_at,
            "declared_by": inc.declared_by,
            "reason": inc.reason,
            "audit_frozen": frozen,
            "phase_log": inc.phase_log,
        }))),
    }
}

async fn snapshot(
    State(state): State<AppState>,
    user: UserCtx,
) -> Result<Json<serde_json::Value>, ApiError> {
    let incident_id = {
        let guard = state.incident.lock().await;
        guard.as_ref().map(|i| i.id.clone()).unwrap_or_else(|| "manual".to_string())
    };

    let (path, sha256) = take_snapshot(&state, &incident_id).await
        .map_err(ApiError::internal)?;

    state.audit.append(
        user.username(),
        AuditEvent::ForensicSnapshot {
            incident_id,
            snapshot_path: path.clone(),
            snapshot_sha256: sha256.clone(),
        },
    ).await.ok();

    Ok(Json(serde_json::json!({
        "snapshot_path": path,
        "sha256": sha256,
    })))
}

#[derive(Deserialize)]
struct BgGenerateRequest {
    purpose: String,
}

#[derive(Serialize)]
struct BgGenerateResponse {
    cred_id: String,
    token: String, // shown ONCE — caller must store it
    purpose: String,
}

async fn bg_generate(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<BgGenerateRequest>,
) -> Result<Json<BgGenerateResponse>, ApiError> {
    let (token, cred) = generate_break_glass_cred(&req.purpose);
    let id = cred.id.clone();
    let purpose = cred.purpose.clone();

    save_break_glass_cred(&state.data_dir, &cred)
        .map_err(ApiError::internal)?;

    tracing::warn!(actor = user.username(), cred_id = %id, purpose = %purpose, "break-glass credential generated");

    Ok(Json(BgGenerateResponse { cred_id: id, token, purpose }))
}

#[derive(Deserialize)]
struct BgUseRequest {
    token: String,
}

async fn bg_use(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<BgUseRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cred = verify_and_consume_break_glass(&state.data_dir, &req.token, user.username())
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;

    state.audit.append(
        user.username(),
        AuditEvent::BreakGlassUsed {
            cred_id: cred.id.clone(),
            used_by: user.username().to_string(),
            purpose: cred.purpose.clone(),
        },
    ).await.ok();

    tracing::warn!(actor = user.username(), cred_id = %cred.id, purpose = %cred.purpose, "break-glass credential used");

    Ok(Json(serde_json::json!({
        "cred_id": cred.id,
        "purpose": cred.purpose,
        "used_at": cred.used_at,
    })))
}

fn new_incident_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("INC-{ts:x}")
}
