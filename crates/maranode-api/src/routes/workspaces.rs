use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use maranode_common::user::Permission;
use maranode_common::workspace::Workspace;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::user_ctx::authorize_permission;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/v1/workspaces/:slug",
            get(get_workspace)
                .put(update_workspace)
                .delete(del_workspace),
        )
}

#[derive(Serialize)]
struct WorkspaceInfo {
    id: String,
    slug: String,
    name: String,
    has_key: bool,
    model_allowlist: Vec<String>,
    rate_limit_rpm: Option<u32>,
    system_prompt: Option<String>,
    has_system_prompt: bool,
    created_at: String,
    net_namespace: bool,
    ns_active: bool,
    max_concurrent_requests: Option<u32>,
    max_models: Option<u32>,
    max_memory_bytes: Option<u64>,
}

impl From<&Workspace> for WorkspaceInfo {
    fn from(w: &Workspace) -> Self {
        let ns_active = w.net_namespace && maranode_isolation::netns::exists(&w.slug);
        Self {
            id: w.id.to_string(),
            slug: w.slug.clone(),
            name: w.name.clone(),
            has_key: w.api_key_hash.is_some(),
            model_allowlist: w.model_allowlist.clone(),
            rate_limit_rpm: w.rate_limit_rpm,
            system_prompt: w.system_prompt.clone(),
            has_system_prompt: w.system_prompt.is_some(),
            created_at: w.created_at.to_rfc3339(),
            net_namespace: w.net_namespace,
            ns_active,
            max_concurrent_requests: w.max_concurrent_requests,
            max_models: w.max_models,
            max_memory_bytes: w.max_memory_bytes,
        }
    }
}

async fn list_workspaces(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    authorize_permission(&headers, &state, Permission::WorkspaceManage).await?;
    let workspaces = state
        .workspace_db
        .lock()
        .await
        .list()
        .map_err(ApiError::from)?;
    let info: Vec<WorkspaceInfo> = workspaces.iter().map(WorkspaceInfo::from).collect();
    Ok(Json(serde_json::json!({ "workspaces": info })))
}

async fn get_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    authorize_permission(&headers, &state, Permission::WorkspaceManage).await?;
    let ws = state
        .workspace_db
        .lock()
        .await
        .get_by_slug(&slug)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found(format!("workspace '{}' not found", slug)))?;
    Ok(Json(WorkspaceInfo::from(&ws)))
}

#[derive(Deserialize)]
struct CreateWorkspaceReq {
    slug: String,
    name: String,
    api_key: Option<String>,
    #[serde(default)]
    no_key: bool,
    model_allowlist: Option<Vec<String>>,
    rate_limit_rpm: Option<u32>,
    system_prompt: Option<String>,
    #[serde(default)]
    net_namespace: bool,
    max_concurrent_requests: Option<u32>,
    max_models: Option<u32>,
    max_memory_bytes: Option<u64>,
}

#[derive(Serialize)]
struct CreateWorkspaceResp {
    workspace: WorkspaceInfo,
    api_key: Option<String>,
}

async fn create_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateWorkspaceReq>,
) -> ApiResult<impl IntoResponse> {
    authorize_permission(&headers, &state, Permission::WorkspaceManage).await?;

    let slug = req.slug.trim().to_lowercase();
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::bad_request(
            "slug must be alphanumeric with - or _",
        ));
    }

    let (api_key, api_key_hash) = if req.no_key {
        (None, None)
    } else if let Some(k) = req.api_key {
        let hash = format!("{:x}", Sha256::digest(k.as_bytes()));
        (None, Some(hash))
    } else {
        let raw = Uuid::new_v4().to_string().replace('-', "");
        let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
        (Some(raw), Some(hash))
    };

    if req.net_namespace {
        #[cfg(target_os = "linux")]
        maranode_isolation::netns::create(&slug).map_err(|e| {
            ApiError::internal(format!("failed to create network namespace: {}", e))
        })?;

        #[cfg(not(target_os = "linux"))]
        return Err(ApiError::bad_request(
            "net_namespace requires Linux",
        ));
    }

    let ws = Workspace {
        id: Uuid::new_v4(),
        slug: slug.clone(),
        name: req.name,
        api_key_hash,
        model_allowlist: req.model_allowlist.unwrap_or_default(),
        rate_limit_rpm: req.rate_limit_rpm,
        system_prompt: req.system_prompt,
        created_at: Utc::now(),
        net_namespace: req.net_namespace,
        max_concurrent_requests: req.max_concurrent_requests,
        max_models: req.max_models,
        max_memory_bytes: req.max_memory_bytes,
        dek: None,
    };

    state
        .workspace_db
        .lock()
        .await
        .create(&ws)
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateWorkspaceResp {
            api_key,
            workspace: WorkspaceInfo::from(&ws),
        }),
    ))
}

#[derive(Deserialize)]
struct UpdateWorkspaceReq {
    name: Option<String>,
    api_key: Option<String>,
    clear_key: Option<bool>,
    rotate_key: Option<bool>,
    model_allowlist: Option<Vec<String>>,
    rate_limit_rpm: Option<u32>,
    clear_rate_limit: Option<bool>,
    system_prompt: Option<String>,
    clear_system_prompt: Option<bool>,
    max_concurrent_requests: Option<u32>,
    clear_max_concurrent_requests: Option<bool>,
    max_models: Option<u32>,
    clear_max_models: Option<bool>,
    max_memory_bytes: Option<u64>,
    clear_max_memory_bytes: Option<bool>,
}

#[derive(Serialize)]
struct UpdateWorkspaceResp {
    workspace: WorkspaceInfo,
    api_key: Option<String>,
}

async fn update_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(req): Json<UpdateWorkspaceReq>,
) -> ApiResult<impl IntoResponse> {
    authorize_permission(&headers, &state, Permission::WorkspaceManage).await?;

    let db = state.workspace_db.lock().await;
    let mut ws = db
        .get_by_slug(&slug)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found(format!("workspace '{}' not found", slug)))?;

    if let Some(n) = req.name {
        ws.name = n;
    }

    let mut new_api_key: Option<String> = None;
    if req.clear_key == Some(true) {
        ws.api_key_hash = None;
    } else if req.rotate_key == Some(true) {
        let raw = Uuid::new_v4().to_string().replace('-', "");
        ws.api_key_hash = Some(format!("{:x}", Sha256::digest(raw.as_bytes())));
        new_api_key = Some(raw);
    } else if let Some(k) = req.api_key {
        ws.api_key_hash = Some(format!("{:x}", Sha256::digest(k.as_bytes())));
    }

    if let Some(al) = req.model_allowlist {
        ws.model_allowlist = al;
    }
    if req.clear_rate_limit == Some(true) {
        ws.rate_limit_rpm = None;
    } else if let Some(r) = req.rate_limit_rpm {
        ws.rate_limit_rpm = Some(r);
    }
    if req.clear_system_prompt == Some(true) {
        ws.system_prompt = None;
    } else if let Some(sp) = req.system_prompt {
        ws.system_prompt = Some(sp);
    }
    if req.clear_max_concurrent_requests == Some(true) {
        ws.max_concurrent_requests = None;
    } else if let Some(v) = req.max_concurrent_requests {
        ws.max_concurrent_requests = Some(v);
    }
    if req.clear_max_models == Some(true) {
        ws.max_models = None;
    } else if let Some(v) = req.max_models {
        ws.max_models = Some(v);
    }
    if req.clear_max_memory_bytes == Some(true) {
        ws.max_memory_bytes = None;
    } else if let Some(v) = req.max_memory_bytes {
        ws.max_memory_bytes = Some(v);
    }

    db.update(&ws).map_err(ApiError::from)?;
    Ok(Json(UpdateWorkspaceResp {
        workspace: WorkspaceInfo::from(&ws),
        api_key: new_api_key,
    }))
}

async fn del_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    authorize_permission(&headers, &state, Permission::WorkspaceManage).await?;

    let ws = state
        .workspace_db
        .lock()
        .await
        .get_by_slug(&slug)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found(format!("workspace '{}' not found", slug)))?;

    if ws.net_namespace {
        #[cfg(target_os = "linux")]
        if maranode_isolation::netns::exists(&slug) {
            if let Err(e) = maranode_isolation::netns::delete(&slug) {
                tracing::warn!("failed to delete network namespace for '{}': {}", slug, e);
            }
        }
    }

    let deleted = state
        .workspace_db
        .lock()
        .await
        .delete(&slug)
        .map_err(ApiError::from)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!(
            "workspace '{}' not found",
            slug
        )))
    }
}
