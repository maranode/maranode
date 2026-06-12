//! CI reproducibility test: same deterministic request must produce same output hash.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use std::sync::atomic::AtomicBool;

use maranode_api::changemgmt::ChangeManagementConfig;
use maranode_api::dlp::DlpConfig;
use maranode_api::runtime::{new_shared, RuntimeSettings};
use maranode_api::state::{new_oidc_pending, Stats};
use maranode_api::{build_router, AppState, IdentityConfig, RagIngestPolicy};
use maranode_common::classification::ClassificationPolicy;
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::AuditLog;
use maranode_inference::{engine::InferenceEngine, stub::StubEngine};
use maranode_store::{ModelStore, UserDb, WorkspaceDb};
use tokio::sync::Mutex;

async fn test_app(tmp_path: &std::path::Path) -> axum::Router {
    let store = ModelStore::open(tmp_path).unwrap();
    let audit =
        AuditLog::open(&default_log_path(tmp_path), &default_key_path(tmp_path)).unwrap();
    let engine: Arc<dyn InferenceEngine> = Arc::new(StubEngine);
    let workspace_db = WorkspaceDb::open(&tmp_path.join("workspaces.db")).unwrap();
    let user_db = UserDb::open(&tmp_path.join("users.db")).unwrap();

    let runtime = new_shared(RuntimeSettings {
        admin_key: None,
        rag_ingest_policy: RagIngestPolicy::Anyone,
        rag_ingest_allowlist: vec![],
        system_prompt: None,
        identity: IdentityConfig::default(),
        air_gap: false,
        log_prompts: false,
        content_log_retention_days: 0,
        smtp: None,
    });

    let state = AppState {
        store,
        audit,
        engine,
        version: "test".into(),
        data_dir: tmp_path.to_path_buf(),
        rag: None,
        runtime,
        stats: Stats::new(),
        workspace_db: Arc::new(Mutex::new(workspace_db)),
        workspace_audits: Arc::new(Mutex::new(HashMap::new())),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        workspace_usage: Arc::new(Mutex::new(HashMap::new())),
        user_db: Arc::new(Mutex::new(user_db)),
        oidc_pending: new_oidc_pending(),
        auth_ip_limiter: Arc::new(Mutex::new(HashMap::new())),
        isolation_ok: Arc::new(AtomicBool::new(true)),
        change_mgmt: Arc::new(ChangeManagementConfig::default()),
        classification: Arc::new(tokio::sync::RwLock::new(ClassificationPolicy::default())),
        dlp: Arc::new(DlpConfig::default()),
        incident: maranode_api::incident::new_incident_handle(),
        audit_frozen: Arc::new(AtomicBool::new(false)),
    };

    build_router(state)
}

async fn chat_json(app: axum::Router, body: Value) -> Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

#[tokio::test]
async fn deterministic_runs_produce_same_output_hash() {
    let tmp = TempDir::new().unwrap();

    let body = json!({
        "model": "llama3.2:3b",
        "messages": [{"role": "user", "content": "What is two plus two?"}],
        "deterministic": true,
        "with_receipt": true,
    });

    let r1 = chat_json(test_app(tmp.path()).await, body.clone()).await;
    let r2 = chat_json(test_app(tmp.path()).await, body.clone()).await;

    let h1 = r1["receipt"]["output_sha256"].as_str().expect("run 1 missing receipt");
    let h2 = r2["receipt"]["output_sha256"].as_str().expect("run 2 missing receipt");

    assert_eq!(h1, h2, "deterministic runs must produce identical output_sha256");
}

#[tokio::test]
async fn deterministic_receipt_has_correct_params() {
    let tmp = TempDir::new().unwrap();

    let body = json!({
        "model": "llama3.2:3b",
        "messages": [{"role": "user", "content": "ping"}],
        "deterministic": true,
        "with_receipt": true,
    });

    let resp = chat_json(test_app(tmp.path()).await, body).await;
    let dp = &resp["receipt"]["decode_params"];

    let temperature = dp["temperature"].as_f64().unwrap_or(99.0);
    assert!(
        temperature < 1e-5,
        "deterministic run must have temperature ~0, got {temperature}"
    );
    assert_eq!(
        dp["deterministic"].as_bool(),
        Some(true),
        "decode_params.deterministic must be true"
    );
    assert_eq!(
        dp["top_k"].as_u64(),
        Some(1),
        "decode_params.top_k must be 1 in deterministic mode"
    );
    assert_eq!(
        dp["seed"].as_u64(),
        Some(0),
        "decode_params.seed must be 0 in deterministic mode"
    );
}

#[tokio::test]
async fn non_deterministic_receipt_is_not_flagged() {
    let tmp = TempDir::new().unwrap();

    let body = json!({
        "model": "llama3.2:3b",
        "messages": [{"role": "user", "content": "ping"}],
        "with_receipt": true,
    });

    let resp = chat_json(test_app(tmp.path()).await, body).await;
    let dp = &resp["receipt"]["decode_params"];

    assert_eq!(
        dp["deterministic"].as_bool(),
        Some(false),
        "normal request must have deterministic=false in receipt"
    );
    assert!(
        dp["top_k"].is_null(),
        "normal request must not set top_k in receipt"
    );
}

#[tokio::test]
async fn deterministic_env_fingerprint_present() {
    let tmp = TempDir::new().unwrap();

    let body = json!({
        "model": "llama3.2:3b",
        "messages": [{"role": "user", "content": "hello"}],
        "deterministic": true,
        "with_receipt": true,
    });

    let resp = chat_json(test_app(tmp.path()).await, body).await;
    let env = &resp["receipt"]["env"];

    assert!(
        !env["kernel_build_id"].as_str().unwrap_or("").is_empty(),
        "env.kernel_build_id must be set"
    );
    assert!(
        env["thread_count"].as_u64().unwrap_or(0) >= 1,
        "env.thread_count must be at least 1"
    );
    assert!(
        !env["device_class"].as_str().unwrap_or("").is_empty(),
        "env.device_class must be set"
    );
}
