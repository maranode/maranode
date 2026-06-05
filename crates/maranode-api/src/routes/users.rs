use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use maranode_common::user::{AuthProvider, Role, User};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::user_ctx::{AdminCtx, UserCtx};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/me", get(me))
        .route("/v1/users", get(list_users).post(create_user))
        .route(
            "/v1/users/:id",
            get(get_user).put(update_user).delete(del_user),
        )
        .route("/v1/users/:id/password", put(set_password))
        .route("/v1/sessions", get(list_sessions).delete(revoke_other_sessions))
        .route("/v1/sessions/:token_prefix", delete(revoke_session))
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResp {
    token: String,
    user: UserView,
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> ApiResult<Json<LoginResp>> {
    let db = state.user_db.lock().await;

    let user = db
        .get_by_username(&req.username)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::unauthorized("invalid username or password"))?;

    if !user.active {
        return Err(ApiError::forbidden("account is disabled"));
    }

    if user.provider != AuthProvider::Local {
        return Err(ApiError::bad_request(format!(
            "this account uses {} login: use the SSO flow",
            user.provider.as_str()
        )));
    }

    let hash = user
        .password_hash
        .as_deref()
        .ok_or_else(|| ApiError::internal("no password set for this account"))?;

    let ok = maranode_store::UserDb::verify_password(&req.password, hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !ok {
        return Err(ApiError::unauthorized("invalid username or password"));
    }

    let ttl = state.rt().identity.session_hours;
    let token = db
        .create_session(user.id, ttl)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(LoginResp {
        token,
        user: UserView::from(&user),
    }))
}

async fn logout(
    State(state): State<AppState>,
    UserCtx(_user): UserCtx,
    headers: axum::http::HeaderMap,
) -> ApiResult<StatusCode> {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        let _ = state.user_db.lock().await.delete_session(token);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn me(UserCtx(user): UserCtx) -> ApiResult<Json<UserView>> {
    Ok(Json(UserView::from(&user)))
}

#[derive(Serialize)]
struct UserView {
    id: String,
    username: String,
    email: Option<String>,
    role: String,
    provider: String,
    active: bool,
    created_at: String,
    last_login: Option<String>,
}

impl From<&User> for UserView {
    fn from(u: &User) -> Self {
        Self {
            id: u.id.to_string(),
            username: u.username.clone(),
            email: u.email.clone(),
            role: u.role.to_string(),
            provider: u.provider.as_str().to_string(),
            active: u.active,
            created_at: u.created_at.to_rfc3339(),
            last_login: u.last_login.map(|t| t.to_rfc3339()),
        }
    }
}

async fn list_users(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
) -> ApiResult<Json<serde_json::Value>> {
    let users = state
        .user_db
        .lock()
        .await
        .list()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let views: Vec<UserView> = users.iter().map(UserView::from).collect();
    Ok(Json(serde_json::json!({ "users": views })))
}

async fn get_user(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<UserView>> {
    let user = state
        .user_db
        .lock()
        .await
        .get_by_id(id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("user not found"))?;
    Ok(Json(UserView::from(&user)))
}

#[derive(Deserialize)]
struct CreateUserReq {
    username: String,
    password: Option<String>,
    email: Option<String>,
    #[serde(default = "default_viewer")]
    role: String,
}

fn default_viewer() -> String {
    "viewer".into()
}

async fn create_user(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
    Json(req): Json<CreateUserReq>,
) -> ApiResult<impl IntoResponse> {
    let role: Role = req
        .role
        .parse()
        .map_err(|e: String| ApiError::bad_request(e))?;

    let password_hash = if let Some(pw) = &req.password {
        Some(
            maranode_store::UserDb::hash_password(pw)
                .map_err(|e| ApiError::internal(e.to_string()))?,
        )
    } else {
        None
    };

    let user = User {
        id: Uuid::new_v4(),
        username: req.username.trim().to_lowercase(),
        email: req.email,
        password_hash,
        role,
        provider: AuthProvider::Local,
        provider_sub: None,
        active: true,
        created_at: Utc::now(),
        last_login: None,
    };

    state
        .user_db
        .lock()
        .await
        .create(&user)
        .map_err(|e| ApiError::conflict(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(UserView::from(&user))))
}

#[derive(Deserialize)]
struct UpdateUserReq {
    email: Option<String>,
    role: Option<String>,
    active: Option<bool>,
}

async fn update_user(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserReq>,
) -> ApiResult<Json<UserView>> {
    let db = state.user_db.lock().await;
    let mut user = db
        .get_by_id(id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("user not found"))?;

    if let Some(e) = req.email {
        user.email = Some(e);
    }
    if let Some(a) = req.active {
        user.active = a;
    }
    if let Some(r) = req.role {
        user.role = r.parse().map_err(|e: String| ApiError::bad_request(e))?;
    }

    db.update(&user)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(UserView::from(&user)))
}

async fn del_user(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let deleted = state
        .user_db
        .lock()
        .await
        .delete(id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("user not found"))
    }
}

#[derive(Deserialize)]
struct SetPasswordReq {
    password: String,
}

async fn set_password(
    State(state): State<AppState>,
    AdminCtx(_): AdminCtx,
    Path(id): Path<Uuid>,
    Json(req): Json<SetPasswordReq>,
) -> ApiResult<StatusCode> {
    let db = state.user_db.lock().await;
    let mut user = db
        .get_by_id(id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("user not found"))?;

    user.password_hash = Some(
        maranode_store::UserDb::hash_password(&req.password)
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );
    db.update(&user)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// sessions endpoints

#[derive(Serialize)]
struct SessionView {
    token_prefix: String,
    created_at: String,
    expires_at: String,
    is_current: bool,
    username: Option<String>,
}

fn current_token(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string()
}

async fn list_sessions(
    State(state): State<AppState>,
    UserCtx(user): UserCtx,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let tok = current_token(&headers);
    let current_prefix: String = tok.chars().take(8).collect();

    let db = state.user_db.lock().await;
    let sessions = if user.role.can_manage_users() {
        db.list_all_sessions()
    } else {
        db.list_sessions_for_user(user.id)
    }
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let views: Vec<SessionView> = sessions
        .into_iter()
        .map(|s| SessionView {
            is_current: s.token_prefix == current_prefix,
            token_prefix: s.token_prefix,
            created_at: s.created_at.to_rfc3339(),
            expires_at: s.expires_at.to_rfc3339(),
            username: s.username,
        })
        .collect();

    Ok(Json(serde_json::json!({ "sessions": views })))
}

async fn revoke_session(
    State(state): State<AppState>,
    UserCtx(user): UserCtx,
    Path(token_prefix): Path<String>,
) -> ApiResult<StatusCode> {
    let db = state.user_db.lock().await;

    let sessions = if user.role.can_manage_users() {
        db.list_all_sessions()
    } else {
        db.list_sessions_for_user(user.id)
    }
    .map_err(|e| ApiError::internal(e.to_string()))?;

    // find session row that matches prefix from path
    let matched = sessions
        .into_iter()
        .find(|s| s.token_prefix == token_prefix)
        .ok_or_else(|| ApiError::not_found("session not found"))?;

    // delete needs user_id and prefix, not full token in memory
    drop(db);
    let db = state.user_db.lock().await;

    // sql: delete where token like prefix% and user_id = matched.user_id
    // safe: prefix is 8 lowercase hex only, no sql wildcard chars
    db.delete_session_by_prefix(&matched.user_id, &token_prefix)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_other_sessions(
    State(state): State<AppState>,
    UserCtx(user): UserCtx,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let tok = current_token(&headers);
    let n = state
        .user_db
        .lock()
        .await
        .delete_sessions_for_user_except(user.id, &tok)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "revoked": n })))
}
