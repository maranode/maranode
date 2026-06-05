use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};

use maranode_common::workspace::Workspace;

use crate::state::AppState;

/// workspace from X-Maranode-Workspace header and bearer token
#[derive(Clone)]
pub struct WorkspaceCtx(pub Workspace);

impl WorkspaceCtx {
    pub fn workspace(&self) -> &Workspace {
        &self.0
    }
}

pub struct WorkspaceAuthError(String);

impl IntoResponse for WorkspaceAuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": { "message": self.0, "type": "auth_error", "code": 401 }
            })),
        )
            .into_response()
    }
}

#[async_trait]
impl FromRequestParts<AppState> for WorkspaceCtx {
    type Rejection = WorkspaceAuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let slug = parts
            .headers
            .get("x-maranode-workspace")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default");

        let bearer = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");

        let admin_key = state.rt().admin_key;
        if let Some(admin_key) = &admin_key {
            if !admin_key.is_empty() && maranode_common::secure::ct_eq_str(bearer, admin_key) {
                let db = state.workspace_db.lock().await;
                let ws = db
                    .get_by_slug(slug)
                    .map_err(|_| WorkspaceAuthError("workspace lookup failed".into()))?
                    .ok_or_else(|| WorkspaceAuthError(format!("workspace '{}' not found", slug)))?;
                return Ok(WorkspaceCtx(ws));
            }
        }

        let db = state.workspace_db.lock().await;
        let ws = db
            .get_by_slug(slug)
            .map_err(|_| WorkspaceAuthError("workspace lookup failed".into()))?
            .ok_or_else(|| WorkspaceAuthError(format!("workspace '{}' not found", slug)))?;

        if let Some(expected_hash) = &ws.api_key_hash {
            let provided_hash = format!("{:x}", Sha256::digest(bearer.as_bytes()));
            if !maranode_common::secure::ct_eq_str(&provided_hash, expected_hash) {
                return Err(WorkspaceAuthError("invalid workspace key".into()));
            }
        }

        Ok(WorkspaceCtx(ws))
    }
}
