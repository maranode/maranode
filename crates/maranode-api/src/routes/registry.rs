use axum::{
    extract::{Path, State},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use maranode_common::approval::ApprovalToken;
use maranode_common::events::AuditEvent;

use crate::changemgmt;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/registry/ui", get(approval_ui))
        .route("/v1/registry/submit", post(submit))
        .route("/v1/registry/pending", get(list_pending))
        .route("/v1/registry/tokens", get(list_tokens))
        .route("/v1/registry/approve/:sha256", post(approve))
        .route("/v1/registry/revoke/:sha256", post(revoke))
        .route("/v1/registry/hooks/test", post(hooks_test))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubmissionRecord {
    pub submission_id: String,
    pub model_id: String,
    pub model_sha256: String,
    pub submitted_by: String,
    pub submitted_at: chrono::DateTime<Utc>,
    pub note: Option<String>,
    pub status: SubmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cm_ticket_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cm_system: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SubmissionStatus {
    Pending,
    Approved,
    Revoked,
}

fn pending_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("registry").join("pending")
}

fn tokens_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("approval-tokens")
}

fn submission_path(data_dir: &std::path::Path, sha256: &str) -> PathBuf {
    pending_dir(data_dir).join(format!("{sha256}.json"))
}

fn load_submission(data_dir: &std::path::Path, sha256: &str) -> anyhow::Result<SubmissionRecord> {
    let path = submission_path(data_dir, sha256);
    let bytes = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_submission(data_dir: &std::path::Path, rec: &SubmissionRecord) -> anyhow::Result<()> {
    let dir = pending_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", rec.model_sha256));
    std::fs::write(path, serde_json::to_vec_pretty(rec)?)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SubmitReq {
    model_id: String,
    model_sha256: String,
    submitted_by: String,
    note: Option<String>,
}

async fn submit(
    State(state): State<AppState>,
    Json(req): Json<SubmitReq>,
) -> ApiResult<Json<SubmissionRecord>> {
    let rec = SubmissionRecord {
        submission_id: Uuid::new_v4().to_string(),
        model_id: req.model_id,
        model_sha256: req.model_sha256,
        submitted_by: req.submitted_by,
        submitted_at: Utc::now(),
        note: req.note,
        status: SubmissionStatus::Pending,
        cm_ticket_id: None,
        cm_system: None,
    };

    save_submission(&state.data_dir, &rec)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if state.change_mgmt.is_configured() {
        let cfg = state.change_mgmt.clone();
        let model_id = rec.model_id.clone();
        let sha256 = rec.model_sha256.clone();
        let note = rec.note.clone();
        let by = rec.submitted_by.clone();
        let data_dir = state.data_dir.clone();
        tokio::spawn(async move {
            if let Some((ticket_id, system)) = changemgmt::open_ticket(&cfg, &model_id, &sha256, note.as_deref(), &by).await {
                tracing::info!("CM ticket opened: {ticket_id} ({system})");
                // update submission with ticket info
                if let Ok(mut r) = load_submission(&data_dir, &sha256) {
                    r.cm_ticket_id = Some(ticket_id);
                    r.cm_system = Some(system);
                    let _ = save_submission(&data_dir, &r);
                }
            }
        });
    }

    Ok(Json(rec))
}

async fn list_pending(State(state): State<AppState>) -> ApiResult<Json<Vec<SubmissionRecord>>> {
    let dir = pending_dir(&state.data_dir);
    if !dir.exists() {
        return Ok(Json(vec![]));
    }

    let mut records = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| ApiError::internal(e.to_string()))? {
        let entry = entry.map_err(|e| ApiError::internal(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(rec) = serde_json::from_slice::<SubmissionRecord>(&bytes) {
                records.push(rec);
            }
        }
    }
    records.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
    Ok(Json(records))
}

async fn list_tokens(State(state): State<AppState>) -> ApiResult<Json<Vec<ApprovalToken>>> {
    let dir = tokens_dir(&state.data_dir);
    if !dir.exists() {
        return Ok(Json(vec![]));
    }

    let mut tokens = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| ApiError::internal(e.to_string()))? {
        let entry = entry.map_err(|e| ApiError::internal(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mrn-token") {
            continue;
        }
        if let Ok(t) = ApprovalToken::load(&path) {
            tokens.push(t);
        }
    }
    tokens.sort_by(|a, b| b.approved_at.cmp(&a.approved_at));
    Ok(Json(tokens))
}

#[derive(Debug, Deserialize)]
struct ApproveReq {
    approved_by: String,
    note: Option<String>,
    expires_in_days: Option<i64>,
}

async fn approve(
    State(state): State<AppState>,
    Path(sha256): Path<String>,
    Json(req): Json<ApproveReq>,
) -> ApiResult<Json<ApprovalToken>> {
    let mut rec = load_submission(&state.data_dir, &sha256)
        .map_err(|_| ApiError::not_found(format!("no submission for sha256 {}", &sha256[..12.min(sha256.len())])))?;

    if rec.status == SubmissionStatus::Revoked {
        return Err(ApiError::bad_request("submission already revoked"));
    }

    let key = ApprovalToken::load_or_create_signing_key(&state.data_dir)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let expires_at = req.expires_in_days.map(|d| Utc::now() + chrono::Duration::days(d));

    let token = ApprovalToken {
        token_id: Uuid::new_v4().to_string(),
        model_id: rec.model_id.clone(),
        model_sha256: sha256.clone(),
        approved_by: req.approved_by.clone(),
        approved_at: Utc::now(),
        expires_at,
        note: req.note.or_else(|| rec.note.clone()),
        signer_pubkey: String::new(),
        signature: String::new(),
    }
    .sign(&key)
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let token_path = ApprovalToken::token_path(&tokens_dir(&state.data_dir), &sha256);
    token.save(&token_path).map_err(|e| ApiError::internal(e.to_string()))?;

    rec.status = SubmissionStatus::Approved;
    save_submission(&state.data_dir, &rec).map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = state.audit.append("registry", AuditEvent::ModelApprovalGranted {
        model_id: rec.model_id.clone(),
        model_sha256: sha256.clone(),
        approved_by: req.approved_by.clone(),
        token_id: token.token_id.clone(),
        signer_pubkey: token.signer_pubkey.clone(),
    }).await;

    if state.change_mgmt.is_configured() {
        if let (Some(ticket_id), Some(system)) = (rec.cm_ticket_id.clone(), rec.cm_system.clone()) {
            let cfg = state.change_mgmt.clone();
            let resolution = format!("Approved by {} — token issued (id: {})", req.approved_by, token.token_id);
            tokio::spawn(async move {
                changemgmt::close_ticket(&cfg, &ticket_id, &system, &resolution).await;
            });
        }
    }

    Ok(Json(token))
}

#[derive(Debug, Deserialize)]
struct RevokeReq {
    revoked_by: String,
}

async fn revoke(
    State(state): State<AppState>,
    Path(sha256): Path<String>,
    Json(req): Json<RevokeReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut rec = load_submission(&state.data_dir, &sha256)
        .map_err(|_| ApiError::not_found(format!("no submission for sha256 {}", &sha256[..12.min(sha256.len())])))?;

    let token_path = ApprovalToken::token_path(&tokens_dir(&state.data_dir), &sha256);
    let token_id = if token_path.exists() {
        let t = ApprovalToken::load(&token_path).ok();
        let id = t.as_ref().map(|t| t.token_id.clone()).unwrap_or_default();
        std::fs::remove_file(&token_path).ok();
        id
    } else {
        String::new()
    };

    rec.status = SubmissionStatus::Revoked;
    save_submission(&state.data_dir, &rec).map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = state.audit.append("registry", AuditEvent::ModelApprovalRevoked {
        model_id: rec.model_id.clone(),
        model_sha256: sha256.clone(),
        revoked_by: req.revoked_by,
        token_id,
    }).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn hooks_test(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let results = changemgmt::test_connectivity(&state.change_mgmt).await;
    Ok(Json(serde_json::to_value(results).unwrap_or_default()))
}

async fn approval_ui(State(state): State<AppState>) -> Html<String> {
    let dir = pending_dir(&state.data_dir);
    let mut rows = String::new();

    if dir.exists() {
        let mut records: Vec<SubmissionRecord> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(bytes) = std::fs::read(entry.path()) {
                        if let Ok(rec) = serde_json::from_slice::<SubmissionRecord>(&bytes) {
                            records.push(rec);
                        }
                    }
                }
            }
        }
        records.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));

        for rec in &records {
            let status_class = match rec.status {
                SubmissionStatus::Pending  => "status-pending",
                SubmissionStatus::Approved => "status-approved",
                SubmissionStatus::Revoked  => "status-revoked",
            };
            let sha_short = &rec.model_sha256[..12.min(rec.model_sha256.len())];
            let note = rec.note.as_deref().unwrap_or("—");
            let actions = if rec.status == SubmissionStatus::Pending {
                format!(
                    r#"<button onclick="doApprove('{sha}')">Approve</button>
                       <button onclick="doRevoke('{sha}')">Revoke</button>"#,
                    sha = rec.model_sha256
                )
            } else {
                String::new()
            };
            rows.push_str(&format!(
                r#"<tr>
                  <td>{model_id}</td>
                  <td title="{sha256}">{sha_short}…</td>
                  <td>{submitted_by}</td>
                  <td>{submitted_at}</td>
                  <td>{note}</td>
                  <td><span class="{status_class}">{status:?}</span></td>
                  <td>{actions}</td>
                </tr>"#,
                model_id = rec.model_id,
                sha256 = rec.model_sha256,
                submitted_by = rec.submitted_by,
                submitted_at = rec.submitted_at.format("%Y-%m-%d %H:%M UTC"),
                status = rec.status,
            ));
        }
    }

    if rows.is_empty() {
        rows = "<tr><td colspan=\"7\" style=\"text-align:center;color:#888\">no submissions</td></tr>".into();
    }

    Html(format!(r#"<!doctype html><html><head><meta charset="utf-8">
<title>Maranode — Model Approval Registry</title>
<style>
  body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #f8f8f8; }}
  h1 {{ font-size: 1.4rem; margin-bottom: 1rem; }}
  table {{ border-collapse: collapse; width: 100%; background: #fff; border-radius: 6px; overflow: hidden; box-shadow: 0 1px 4px #0001; }}
  th, td {{ padding: .6rem 1rem; text-align: left; border-bottom: 1px solid #eee; font-size: .9rem; }}
  th {{ background: #f0f0f0; font-weight: 600; }}
  .status-pending  {{ color: #b45309; font-weight: 600; }}
  .status-approved {{ color: #15803d; font-weight: 600; }}
  .status-revoked  {{ color: #dc2626; font-weight: 600; }}
  button {{ padding: .3rem .8rem; border: none; border-radius: 4px; cursor: pointer; margin-right: .3rem; }}
  button:first-child {{ background: #15803d; color: #fff; }}
  button:last-child  {{ background: #dc2626; color: #fff; }}
  #msg {{ margin-top: 1rem; font-size: .9rem; color: #555; }}
</style>
</head><body>
<h1>Model Approval Registry</h1>
<table>
  <thead><tr>
    <th>Model</th><th>SHA-256</th><th>Submitted by</th>
    <th>Date</th><th>Note</th><th>Status</th><th>Actions</th>
  </tr></thead>
  <tbody>{rows}</tbody>
</table>
<div id="msg"></div>
<script>
async function doApprove(sha) {{
  const by = prompt("Approved by (name/email):");
  if (!by) return;
  const r = await fetch(`/v1/registry/approve/${{sha}}`, {{
    method: "POST", headers: {{"Content-Type":"application/json"}},
    body: JSON.stringify({{ approved_by: by }})
  }});
  document.getElementById("msg").textContent = r.ok ? "Approved." : await r.text();
  if (r.ok) setTimeout(() => location.reload(), 800);
}}
async function doRevoke(sha) {{
  const by = prompt("Revoked by (name/email):");
  if (!by) return;
  const r = await fetch(`/v1/registry/revoke/${{sha}}`, {{
    method: "POST", headers: {{"Content-Type":"application/json"}},
    body: JSON.stringify({{ revoked_by: by }})
  }});
  document.getElementById("msg").textContent = r.ok ? "Revoked." : await r.text();
  if (r.ok) setTimeout(() => location.reload(), 800);
}}
</script>
</body></html>"#))
}
