use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

use maranode_api::changemgmt::ChangeManagementConfig;
use maranode_api::dlp::DlpConfig;
use maranode_api::runtime::{new_shared, RuntimeSettings};
use maranode_api::state::{IdentityConfig, RagIngestPolicy, Stats};
use maranode_api::{build_router, AppState};
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::AuditLog;
use maranode_common::classification::ClassificationPolicy;
use maranode_common::events::{AuditEntry, AuditEvent, ProbeResult};
use maranode_inference::{engine::InferenceEngine, stub::StubEngine};
use maranode_store::{ModelStore, UserDb, WorkspaceDb};

fn make_runtime(air_gap: bool) -> maranode_api::runtime::SharedRuntime {
    new_shared(RuntimeSettings {
        admin_key: None,
        rag_ingest_policy: RagIngestPolicy::Anyone,
        rag_ingest_allowlist: vec![],
        system_prompt: None,
        identity: IdentityConfig::default(),
        air_gap,
        log_prompts: false,
        content_log_retention_days: 0,
        smtp: None,
    })
}

async fn make_app(air_gap: bool, isolation_ok: bool) -> axum::Router {
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
        version: "0.0.0-test".into(),
        data_dir: dir,
        rag: None,
        runtime: make_runtime(air_gap),
        stats: Stats::new(),
        workspace_db: Arc::new(Mutex::new(workspace_db)),
        workspace_audits: Arc::new(Mutex::new(HashMap::new())),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        workspace_usage: Arc::new(Mutex::new(HashMap::new())),
        user_db: Arc::new(Mutex::new(user_db)),
        oidc_pending: maranode_api::state::new_oidc_pending(),
        auth_ip_limiter: Arc::new(Mutex::new(HashMap::new())),
        isolation_ok: Arc::new(AtomicBool::new(isolation_ok)),
        change_mgmt: Arc::new(ChangeManagementConfig::default()),
        classification: Arc::new(tokio::sync::RwLock::new(ClassificationPolicy::default())),
        dlp: Arc::new(DlpConfig::default()),
    };

    build_router(state)
}

async fn body_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

#[test]
fn probe_result_round_trips() {
    let r = ProbeResult {
        host: "1.1.1.1".into(),
        port: 53,
        reachable: true,
    };
    let s = serde_json::to_string(&r).unwrap();
    let back: ProbeResult = serde_json::from_str(&s).unwrap();
    assert_eq!(back.host, "1.1.1.1");
    assert_eq!(back.port, 53);
    assert!(back.reachable);
}

#[test]
fn isolation_probe_event_round_trips() {
    let event = AuditEvent::IsolationProbe {
        isolated: false,
        probe_results: vec![
            ProbeResult { host: "8.8.8.8".into(), port: 53, reachable: true },
            ProbeResult { host: "1.1.1.1".into(), port: 53, reachable: false },
        ],
        iptables_hash: "abc123".into(),
    };

    let s = serde_json::to_string(&event).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(v["event"], "isolation_probe");
    assert_eq!(v["isolated"], false);
    assert_eq!(v["probe_results"][0]["reachable"], true);
    assert_eq!(v["iptables_hash"], "abc123");
}

#[test]
fn isolation_ok_atomic_starts_true_by_default() {
    let flag = Arc::new(AtomicBool::new(true));
    assert!(flag.load(Ordering::Relaxed));
    flag.store(false, Ordering::Relaxed);
    assert!(!flag.load(Ordering::Relaxed));
}

#[test]
fn probe_entry_deserializes_from_jsonl() {
    let line = r#"{"ts":"2025-01-01T00:00:00Z","seq":42,"actor":"probe","event":"isolation_probe","isolated":false,"probe_results":[{"host":"1.1.1.1","port":53,"reachable":true}],"iptables_hash":"","prev_hmac":"","hmac":"aabbcc"}"#;
    let entry: AuditEntry = serde_json::from_str(line).unwrap();
    assert_eq!(entry.seq, 42);
    if let AuditEvent::IsolationProbe { isolated, probe_results, .. } = &entry.event {
        assert!(!isolated);
        assert_eq!(probe_results[0].host, "1.1.1.1");
        assert!(probe_results[0].reachable);
    } else {
        panic!("wrong event variant");
    }
}

#[tokio::test]
async fn chat_allowed_when_air_gap_disabled() {
    let app = make_app(false, false).await;

    let body = serde_json::to_vec(&json!({
        "model": "some:model",
        "messages": [{"role": "user", "content": "hello"}]
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

    // not blocked by isolation — may still fail for other reasons (no model), but not 503
    assert_ne!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn chat_refused_when_isolation_drift_detected() {
    let app = make_app(true, false).await;

    let body = serde_json::to_vec(&json!({
        "model": "some:model",
        "messages": [{"role": "user", "content": "hello"}]
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

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let json = body_json(resp.into_body()).await;
    let msg = json["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("isolation") || msg.contains("air-gap"),
        "error message should mention isolation: {msg}"
    );
}

#[tokio::test]
async fn chat_allowed_when_air_gap_active_and_isolated() {
    let app = make_app(true, true).await;

    let body = serde_json::to_vec(&json!({
        "model": "some:model",
        "messages": [{"role": "user", "content": "hello"}]
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

    assert_ne!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
