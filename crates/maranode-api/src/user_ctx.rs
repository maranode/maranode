use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use maranode_common::user::{Role, User};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub struct UserCtx(pub User);

pub struct AdminCtx(pub User);

#[derive(Debug)]
pub enum AuthError {
    MissingCredentials,
    InvalidToken,
    InactiveAccount,
    Forbidden,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AuthError::MissingCredentials => (StatusCode::UNAUTHORIZED, "authentication required"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid or expired token"),
            AuthError::InactiveAccount => (StatusCode::FORBIDDEN, "account is disabled"),
            AuthError::Forbidden => (StatusCode::FORBIDDEN, "insufficient permissions"),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

fn extract_bearer(parts: &Parts) -> Option<String> {
    bearer_from_headers(&parts.headers)
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// allow host admin key or logged-in session when role passes perm
/// if admin key is not set, destructive endpoints stay open (for dev)
pub async fn authorize_privileged(
    headers: &HeaderMap,
    state: &AppState,
    perm: impl FnOnce(Role) -> bool,
) -> ApiResult<()> {
    use maranode_common::secure::ct_eq_str;

    let runtime = state.rt();
    let token = bearer_from_headers(headers);

    if let Some(ak) = &runtime.admin_key {
        if !ak.is_empty() {
            if token.as_deref().is_some_and(|t| ct_eq_str(t, ak)) {
                return Ok(());
            }
        }
    }

    if let Some(ref token) = token {
        let db = state.user_db.lock().await;
        if let Ok(Some(user)) = db.resolve_session(token) {
            if user.active && perm(user.role) {
                return Ok(());
            }
        }
    }

    if runtime.admin_key.as_ref().is_none_or(|k| k.is_empty()) {
        return Ok(());
    }

    Err(ApiError::forbidden("admin privileges required"))
}

#[async_trait]
impl FromRequestParts<AppState> for UserCtx {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts).ok_or(AuthError::MissingCredentials)?;

        // admin key works like super-user token, we build fake admin user
        // compare in constant time so key cannot leak from timing side channel
        let admin_key = state.rt().admin_key;
        if let Some(ak) = &admin_key {
            if !ak.is_empty() && maranode_common::secure::ct_eq_str(&token, ak) {
                return Ok(UserCtx(synthetic_admin()));
            }
        }

        let db = state.user_db.lock().await;
        let user = db
            .resolve_session(&token)
            .map_err(|_| AuthError::InvalidToken)?
            .ok_or(AuthError::InvalidToken)?;

        if !user.active {
            return Err(AuthError::InactiveAccount);
        }

        Ok(UserCtx(user))
    }
}

#[async_trait]
impl FromRequestParts<AppState> for AdminCtx {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let UserCtx(user) = UserCtx::from_request_parts(parts, state).await?;
        if !user.role.can_manage_users() {
            return Err(AuthError::Forbidden);
        }
        Ok(AdminCtx(user))
    }
}

pub fn synthetic_admin() -> User {
    use maranode_common::user::AuthProvider;
    use chrono::Utc;
    use uuid::Uuid;
    User {
        id: Uuid::nil(),
        username: "admin".into(),
        email: None,
        password_hash: None,
        role: Role::Admin,
        provider: AuthProvider::Local,
        provider_sub: None,
        active: true,
        created_at: Utc::now(),
        last_login: None,
    }
}
