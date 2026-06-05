//! audit api routes: list entries, export csv, bundle zip, prune retention

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};

use maranode_audit::bundle::create_bundle;
use maranode_audit::export::{export_gdpr, export_hipaa, export_iso27001, export_soc2, ExportFilter};
use maranode_audit::log::{default_key_path, default_log_path, AuditLog};
use maranode_audit::retention::prune_log;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/audit/entries", get(list_entries))
        .route("/v1/audit/export", get(export_entries))
        .route("/v1/audit/bundle", get(download_bundle))
        .route("/v1/audit/bundle/:workspace", get(download_workspace_bundle))
        .route("/v1/audit/prune", post(do_prune))
}

fn require_admin_hdr(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let key = state
        .rt()
        .admin_key
        .as_deref()
        .map(str::to_string)
        .unwrap_or_default();
    let key = key.as_str();
    if key.is_empty() {
        return Ok(());
    }
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !maranode_common::secure::ct_eq_str(provided, key) {
        return Err(ApiError::forbidden("admin key required"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    /// max entries to return in list
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

#[derive(Debug, Serialize)]
struct AuditEntryView {
    seq: u64,
    ts: String,
    actor: String,
    event: serde_json::Value,
}

async fn list_entries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuditEntryView>>> {
    require_admin_hdr(&state, &headers)?;
    let limit = q.limit.min(500);
    let log_path = default_log_path(&state.data_dir);

    let entries =
        AuditLog::read_recent(&log_path, limit).map_err(|e| ApiError::internal(e.to_string()))?;

    let views = entries
        .into_iter()
        .map(|e| AuditEntryView {
            seq: e.seq,
            ts: e.ts.to_rfc3339(),
            actor: e.actor,
            event: serde_json::to_value(&e.event).unwrap_or(serde_json::Value::Null),
        })
        .collect();

    Ok(Json(views))
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    /// export format: gdpr, hipaa, soc2, or iso27001
    format: String,
    workspace: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn export_entries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    require_admin_hdr(&state, &headers)?;

    let (log_path, filter) = resolve_log_and_filter(&state, q.workspace.as_deref(), &q)?;
    let ws_label = q.workspace.as_deref().unwrap_or("global");

    let (csv, filename) = match q.format.as_str() {
        "gdpr"     => (export_gdpr(&log_path, &filter),     format!("audit_{ws_label}_gdpr.csv")),
        "hipaa"    => (export_hipaa(&log_path, &filter),    format!("audit_{ws_label}_hipaa.csv")),
        "soc2"     => (export_soc2(&log_path, &filter),     format!("audit_{ws_label}_soc2.csv")),
        "iso27001" => (export_iso27001(&log_path, &filter), format!("audit_{ws_label}_iso27001.csv")),
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown format '{}': expected: gdpr, hipaa, soc2, iso27001",
                other
            )))
        }
    };

    let csv = csv.map_err(|e| ApiError::internal(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        csv,
    )
        .into_response())
}

fn resolve_log_and_filter(
    state: &AppState,
    workspace: Option<&str>,
    q: &ExportQuery,
) -> ApiResult<(std::path::PathBuf, ExportFilter)> {
    let from = q
        .from
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    let to = q
        .to
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    if let Some(slug) = workspace {
        let ws_log_path = default_log_path(
            &state.data_dir.join("workspaces").join(slug),
        );
        if !ws_log_path.exists() {
            return Err(ApiError::not_found(format!(
                "no audit log found for workspace '{}': has it received any requests?",
                slug
            )));
        }
        Ok((ws_log_path, ExportFilter { workspace: None, from, to }))
    } else {
        Ok((default_log_path(&state.data_dir), ExportFilter { workspace: None, from, to }))
    }
}

async fn download_bundle(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    require_admin_hdr(&state, &headers)?;
    build_bundle_response(&state, None).await
}

async fn download_workspace_bundle(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> ApiResult<Response> {
    require_admin_hdr(&state, &headers)?;
    build_bundle_response(&state, Some(&slug)).await
}

async fn build_bundle_response(state: &AppState, workspace: Option<&str>) -> ApiResult<Response> {
    let (log_path, key_path, tmp_name, filename) = if let Some(slug) = workspace {
        let ws_dir = state.data_dir.join("workspaces").join(slug);
        let log = default_log_path(&ws_dir);
        if !log.exists() {
            return Err(ApiError::not_found(format!(
                "no audit log found for workspace '{}': has it received any requests?",
                slug
            )));
        }
        let key = default_key_path(&ws_dir);
        let tmp = state.data_dir.join(format!("audit_bundle_{}_tmp.zip", slug));
        let fname = format!("audit_bundle_{}.zip", slug);
        (log, key, tmp, fname)
    } else {
        let log = default_log_path(&state.data_dir);
        let key = default_key_path(&state.data_dir);
        let tmp = state.data_dir.join("audit_bundle_tmp.zip");
        (log, key, tmp, "audit_bundle.zip".to_string())
    };

    create_bundle(&log_path, &key_path, &tmp_name, workspace)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let bytes = std::fs::read(&tmp_name).map_err(|e| ApiError::internal(e.to_string()))?;
    let _ = std::fs::remove_file(&tmp_name);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct PruneReq {
    retain_days: u32,
}

#[derive(Debug, Serialize)]
struct PruneResp {
    pruned: u64,
}

async fn do_prune(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PruneReq>,
) -> ApiResult<Json<PruneResp>> {
    require_admin_hdr(&state, &headers)?;

    let log_path = default_log_path(&state.data_dir);
    let pruned =
        prune_log(&log_path, req.retain_days).map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(PruneResp { pruned }))
}
