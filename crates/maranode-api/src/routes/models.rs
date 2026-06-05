//! /v1/models routes: list, get one, delete

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
    Json, Router,
};
use serde::Serialize;

use maranode_common::events::AuditEvent;
use maranode_common::models::{ModelId, ModelType};

use crate::error::{ApiError, ApiResult};
use crate::openai::{ModelListResponse, ModelObject};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/models/details", get(list_models_details))
        .route("/v1/models/:model_id", delete(remove_model))
}

async fn list_models(State(state): State<AppState>) -> ApiResult<Json<ModelListResponse>> {
    let manifests = state
        .store
        .list()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data = manifests
        .into_iter()
        .filter(|m| m.model_type == ModelType::Llm)
        .map(|m| ModelObject {
            id: m.model_id.to_string(),
            object: "model",
            created: m.imported_at.timestamp(),
            owned_by: "maranode".into(),
        })
        .collect();

    Ok(Json(ModelListResponse {
        object: "list",
        data,
    }))
}

#[derive(Debug, Serialize)]
struct ModelDetail {
    id: String,
    name: String,
    tag: String,
    model_type: String,
    size_bytes: u64,
    size_human: String,
    sha256: String,
    quantization: Option<String>,
    imported_at: String,
}

async fn list_models_details(State(state): State<AppState>) -> ApiResult<Json<Vec<ModelDetail>>> {
    let manifests = state
        .store
        .list()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let details = manifests
        .into_iter()
        .map(|m| ModelDetail {
            id: m.model_id.to_string(),
            name: m.model_id.name.clone(),
            tag: m.model_id.tag.clone(),
            model_type: match m.model_type {
                ModelType::Llm => "llm".into(),
                ModelType::Embedding => "embedding".into(),
            },
            size_bytes: m.size_bytes,
            size_human: human_size(m.size_bytes),
            sha256: m.sha256.clone(),
            quantization: m.quantization.clone(),
            imported_at: m.imported_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(details))
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let rt = state.rt();
    let Some(admin_key) = &rt.admin_key else {
        return Ok(());
    };
    if admin_key.is_empty() {
        return Ok(());
    }
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !maranode_common::secure::ct_eq_str(provided, admin_key) {
        return Err(ApiError::forbidden("admin key required"));
    }
    Ok(())
}

async fn remove_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let model_id = ModelId::parse(&raw_id).ok_or_else(|| {
        ApiError::bad_request(format!("invalid model id '{}': expected name:tag", raw_id))
    })?;

    let removed = state
        .store
        .remove(&model_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !removed {
        return Err(ApiError::not_found(format!("model '{}' not found", raw_id)));
    }

    let _ = state
        .audit
        .append("api", AuditEvent::ModelRemoved { model: model_id })
        .await;

    Ok(Json(serde_json::json!({ "deleted": true, "id": raw_id })))
}

fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}
