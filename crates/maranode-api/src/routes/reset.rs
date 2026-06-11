use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::runtime::SmtpCfg;
use crate::state::{check_auth_ip_rate, client_ip, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/password-reset/request", post(request_reset))
        .route("/v1/auth/password-reset/confirm", post(confirm_reset))
}

#[derive(Deserialize)]
struct ResetRequest {
    email: String,
}

#[derive(Deserialize)]
struct ResetConfirm {
    token: String,
    password: String,
}

async fn request_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResetRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let ip = client_ip(&headers);
    check_auth_ip_rate(&state.auth_ip_limiter, &ip, 10).await?;

    let rt = state.rt();
    let smtp = rt
        .smtp
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("email delivery is not configured"))?;

    let db = state.user_db.lock().await;
    let user = db
        .get_by_email(&body.email)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some(u) = user {
        if u.active {
            let token = db
                .create_reset_token(u.id)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            drop(db);
            if let Err(e) = send_reset_email(smtp, &body.email, &token).await {
                tracing::warn!("failed to send reset email to {}: {}", body.email, e);
            }
        }
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn confirm_reset(
    State(state): State<AppState>,
    Json(body): Json<ResetConfirm>,
) -> ApiResult<Json<serde_json::Value>> {
    if body.password.len() < 8 {
        return Err(ApiError::bad_request("password must be at least 8 characters"));
    }

    let db = state.user_db.lock().await;
    let user_id = db
        .consume_reset_token(&body.token)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::unauthorized("invalid or expired reset token"))?;

    let mut user = db
        .get_by_id(user_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("user not found"))?;

    user.password_hash = Some(
        maranode_store::UserDb::hash_password(&body.password)
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );
    db.update(&user)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn send_reset_email(smtp: &SmtpCfg, to: &str, token: &str) -> Result<(), String> {
    use lettre::{
        message::header::ContentType,
        transport::smtp::{
            authentication::Credentials,
            client::{Tls, TlsParameters},
        },
        AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    };

    let body = format!(
        "A password reset was requested for your account.\n\n\
         Use the following token to set a new password:\n\n  {}\n\n\
         This token expires in 30 minutes. If you did not request this, ignore this email.",
        token
    );

    let email = Message::builder()
        .from(smtp.from.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .to(to.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .subject("Password reset")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| e.to_string())?;

    let tls_params = TlsParameters::new(smtp.host.clone()).map_err(|e| e.to_string())?;
    let tls = if smtp.starttls {
        Tls::Required(tls_params)
    } else {
        Tls::None
    };

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
        .port(smtp.port)
        .tls(tls);

    if let (Some(user), Some(pass)) = (smtp.username.as_deref(), smtp.password.as_deref()) {
        builder = builder.credentials(Credentials::new(user.to_string(), pass.to_string()));
    }

    let transport = builder.build();
    transport.send(email).await.map_err(|e| e.to_string())?;
    Ok(())
}
