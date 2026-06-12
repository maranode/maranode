use axum::{
    extract::{Path, State},
    routing::{delete, get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use maranode_common::classification::{ClassificationPolicy, CollectionPolicy, DataLabel, WorkspacePolicy};
use maranode_common::events::AuditEvent;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/classification/policy", get(get_policy))
        .route("/v1/classification/collections/:name", put(set_collection).delete(remove_collection))
        .route("/v1/classification/workspaces/:slug", put(set_workspace))
}

async fn get_policy(State(state): State<AppState>) -> ApiResult<Json<ClassificationPolicy>> {
    let policy = state.classification.read().await.clone();
    Ok(Json(policy))
}

#[derive(Debug, Deserialize)]
struct SetCollectionReq {
    label: DataLabel,
    block_on_violation: Option<bool>,
    assigned_by: Option<String>,
}

async fn set_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SetCollectionReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let block = req.block_on_violation.unwrap_or(true);
    {
        let mut policy = state.classification.write().await;
        policy.collections.insert(name.clone(), CollectionPolicy {
            label: req.label,
            block_on_violation: block,
        });
        policy.save(&state.data_dir).map_err(|e| ApiError::internal(e.to_string()))?;
    }

    let by = req.assigned_by.unwrap_or_else(|| "api".into());
    let _ = state.audit.append("classification", AuditEvent::DataLabelAssigned {
        collection: name.clone(),
        label: req.label.to_string(),
        assigned_by: by,
    }).await;

    Ok(Json(serde_json::json!({ "collection": name, "label": req.label, "block_on_violation": block })))
}

async fn remove_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let removed = {
        let mut policy = state.classification.write().await;
        let r = policy.collections.remove(&name).is_some();
        if r {
            policy.save(&state.data_dir).map_err(|e| ApiError::internal(e.to_string()))?;
        }
        r
    };
    Ok(Json(serde_json::json!({ "removed": removed })))
}

#[derive(Debug, Deserialize)]
struct SetWorkspaceReq {
    max_clearance: DataLabel,
}

async fn set_workspace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(req): Json<SetWorkspaceReq>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let mut policy = state.classification.write().await;
        policy.workspaces.insert(slug.clone(), WorkspacePolicy { max_clearance: req.max_clearance });
        policy.save(&state.data_dir).map_err(|e| ApiError::internal(e.to_string()))?;
    }
    Ok(Json(serde_json::json!({ "workspace": slug, "max_clearance": req.max_clearance })))
}
