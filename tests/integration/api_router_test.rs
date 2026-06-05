//! tests for HTTP router and authentication

use std::sync::Arc;

use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use std::collections::HashMap;

use maranode_api::state::Stats;
use maranode_api::{build_router, AppState, IdentityConfig, RagIngestPolicy};
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::AuditLog;
use maranode_inference::{engine::InferenceEngine, stub::StubEngine};
use maranode_store::{ModelStore, UserDb, WorkspaceDb};
use tokio::sync::Mutex;

async fn test_app() -> axum::Router {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.into_path();

    let store = ModelStore::open(&dir).unwrap();
    let audit = AuditLog::open(&default_log_path(&dir), &default_key_path(&dir)).unwrap();
    let engine: Arc<dyn InferenceEngine> = Arc::new(StubEngine);
    let workspace_db = WorkspaceDb::open(&dir.join("workspaces.db")).unwrap();
    let user_db = UserDb::open(&dir.join("users.db")).unwrap();

    let state = AppState {
        store,
        audit,
        engine,
        version: "0.1.0-test".into(),
        air_gap: false,
        data_dir: dir.clone(),
        rag: None,
        admin_key: None,
        rag_ingest_policy: RagIngestPolicy::Anyone,
        rag_ingest_allowlist: vec![],
        system_prompt: None,
        stats: Stats::new(),
        workspace_db: Arc::new(Mutex::new(workspace_db)),
        workspace_audits: Arc::new(Mutex::new(HashMap::new())),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        user_db: Arc::new(Mutex::new(user_db)),
        identity: Arc::new(IdentityConfig::default()),
    };

    build_router(state)
}

async fn body_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

#[tokio::test]
async fn health_returns_ok() {
    let app = test_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["ok"], true);
    assert_eq!(json["version"], "0.1.0-test");
}

#[tokio::test]
async fn models_list_empty_initially() {
    let app = test_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["object"], "list");
    assert_eq!(json["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn chat_unknown_model_returns_404() {
    let app = test_app().await;

    let body = serde_json::to_vec(&json!({
        "model": "nonexistent:7b",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.5,
        "max_tokens": 10
    }))
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/chat/completions")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chat_invalid_model_id_format_returns_400() {
    let app = test_app().await;

    let body = serde_json::to_vec(&json!({
        "model": "no-colon-here",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.5,
        "max_tokens": 10
    }))
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/chat/completions")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn chat_malformed_json_returns_error() {
    let app = test_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/chat/completions")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(b"{ not valid json !!!".as_ref()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.status().is_client_error(),
        "malformed JSON must produce a 4xx response, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn chat_missing_content_type_returns_error() {
    let app = test_app().await;

    let body = serde_json::to_vec(&json!({
        "model": "model:tag",
        "messages": [{"role": "user", "content": "hi"}],
    }))
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/chat/completions")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        !resp.status().is_success(),
        "missing Content-Type must not produce 2xx, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = test_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/this/does/not/exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_method_not_allowed_on_post() {
    let app = test_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
