//! admin HTTP routes for config reload

use std::sync::Arc;

use maranode_api::AppState;
use maranode_common::secure::ct_eq_str;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};

use crate::reload::{ReloadResponse, ReloadServices};

type ApiResult<T> = Result<T, ApiError>;

/// axum error type. same pattern as maranode-api
mod error {
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};

    pub struct ApiError {
        status: StatusCode,
        message: String,
    }

    impl ApiError {
        pub fn forbidden(msg: impl Into<String>) -> Self {
            Self {
                status: StatusCode::FORBIDDEN,
                message: msg.into(),
            }
        }
        pub fn internal(msg: impl Into<String>) -> Self {
            Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: msg.into(),
            }
        }
    }

    impl IntoResponse for ApiError {
        fn into_response(self) -> Response {
            (self.status, self.message).into_response()
        }
    }
}

use error::ApiError;

pub fn router(services: Arc<ReloadServices>) -> Router {
    Router::new()
        .route("/v1/admin/config/reload", post(reload_config))
        .with_state(services)
}

async fn reload_config(
    State(services): State<Arc<ReloadServices>>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, Json<ReloadResponse>)> {
    require_admin(&services.state, &headers)?;

    let result = services
        .reload()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok((StatusCode::OK, Json(result)))
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let rt = state.rt();
    let Some(admin_key) = &rt.admin_key else {
        return Err(ApiError::forbidden(
            "admin key required: set auth.admin_key before using admin endpoints",
        ));
    };
    if admin_key.is_empty() {
        return Err(ApiError::forbidden("admin key required"));
    }

    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()))
        .unwrap_or("");

    if !ct_eq_str(provided, admin_key) {
        return Err(ApiError::forbidden("admin key required"));
    }
    Ok(())
}
