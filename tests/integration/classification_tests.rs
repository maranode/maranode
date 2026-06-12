use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

use maranode_api::dlp::DlpConfig;
use maranode_api::changemgmt::ChangeManagementConfig;
use maranode_api::runtime::{new_shared, RuntimeSettings};
use maranode_api::state::{IdentityConfig, RagIngestPolicy, Stats};
use maranode_api::{build_router, AppState};
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::AuditLog;
use maranode_common::classification::{ClassificationPolicy, CollectionPolicy, DataLabel, WorkspacePolicy};
use maranode_inference::{engine::InferenceEngine, stub::StubEngine};
use maranode_store::{ModelStore, UserDb, WorkspaceDb};

fn default_runtime() -> maranode_api::runtime::SharedRuntime {
    new_shared(RuntimeSettings {
        admin_key: Some("test-admin-key".into()),
        rag_ingest_policy: RagIngestPolicy::Anyone,
        rag_ingest_allowlist: vec![],
        system_prompt: None,
        identity: IdentityConfig::default(),
        air_gap: false,
        log_prompts: false,
        content_log_retention_days: 0,
        smtp: None,
        tee_encrypt_key: None,
    })
}

async fn make_app_with_policy(policy: ClassificationPolicy) -> (axum::Router, std::path::PathBuf) {
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
        data_dir: dir.clone(),
        rag: None,
        runtime: default_runtime(),
        stats: Stats::new(),
        workspace_db: Arc::new(Mutex::new(workspace_db)),
        workspace_audits: Arc::new(Mutex::new(HashMap::new())),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        workspace_usage: Arc::new(Mutex::new(HashMap::new())),
        user_db: Arc::new(Mutex::new(user_db)),
        oidc_pending: maranode_api::state::new_oidc_pending(),
        auth_ip_limiter: Arc::new(Mutex::new(HashMap::new())),
        isolation_ok: Arc::new(AtomicBool::new(true)),
        change_mgmt: Arc::new(ChangeManagementConfig::default()),
        classification: Arc::new(tokio::sync::RwLock::new(policy)),
        dlp: Arc::new(DlpConfig::default()),
        incident: maranode_api::incident::new_incident_handle(),
        audit_frozen: Arc::new(AtomicBool::new(false)),
    };

    (build_router(state), dir)
}

async fn body_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

// policy GET returns the full classification policy JSON
#[tokio::test]
async fn get_policy_returns_json() {
    let mut policy = ClassificationPolicy::default();
    policy.collections.insert(
        "patient-records".into(),
        CollectionPolicy { label: DataLabel::Phi, block_on_violation: true },
    );
    policy.workspaces.insert(
        "research".into(),
        WorkspacePolicy { max_clearance: DataLabel::Phi },
    );

    let (app, _dir) = make_app_with_policy(policy).await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/classification/policy")
                .header("x-admin-key", "test-admin-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert!(body["collections"]["patient-records"]["label"].as_str().is_some());
}

// assign a label to a collection then verify the policy reflects it
#[tokio::test]
async fn put_collection_label_updates_policy() {
    let (app, _dir) = make_app_with_policy(ClassificationPolicy::default()).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/classification/collections/hr-data")
                .header("content-type", "application/json")
                .header("x-admin-key", "test-admin-key")
                .body(Body::from(serde_json::to_vec(&json!({ "label": "PII", "block_on_violation": true })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let resp2 = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/classification/policy")
                .header("x-admin-key", "test-admin-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp2.into_body()).await;
    assert_eq!(body["collections"]["hr-data"]["label"].as_str(), Some("PII"));
}

// workspace with PUBLIC clearance should be blocked from PHI collection
#[tokio::test]
async fn phi_collection_blocks_restricted_clearance_workspace() {
    let mut policy = ClassificationPolicy::default();
    policy.collections.insert(
        "phi-data".into(),
        CollectionPolicy { label: DataLabel::Phi, block_on_violation: true },
    );
    // workspace clearance defaults to PUBLIC when not set

    let (app, _dir) = make_app_with_policy(policy).await;

    // check_access: workspace=low-clearance, collection=phi-data -> should see violation
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("phi-data".into(), CollectionPolicy { label: DataLabel::Phi, block_on_violation: true });
            m
        },
        workspaces: HashMap::new(),
    };

    let violation = pol.check_access("low-clearance-ws", "phi-data");
    assert!(violation.is_some());
    let v = violation.unwrap();
    assert!(v.block);
    assert_eq!(v.required_label, DataLabel::Phi);
    assert_eq!(v.workspace_clearance, DataLabel::Public);
}

// workspace with PHI clearance can access PHI collection without violation
#[tokio::test]
async fn phi_clearance_workspace_passes_phi_collection() {
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("phi-data".into(), CollectionPolicy { label: DataLabel::Phi, block_on_violation: true });
            m
        },
        workspaces: {
            let mut m = HashMap::new();
            m.insert("clinical-ws".into(), WorkspacePolicy { max_clearance: DataLabel::Phi });
            m
        },
    };

    let violation = pol.check_access("clinical-ws", "phi-data");
    assert!(violation.is_none());
}

// collections with lower labels than workspace clearance never produce violations
#[tokio::test]
async fn lower_label_collection_never_violates() {
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("docs".into(), CollectionPolicy { label: DataLabel::Public, block_on_violation: true });
            m.insert("internal".into(), CollectionPolicy { label: DataLabel::Restricted, block_on_violation: true });
            m
        },
        workspaces: {
            let mut m = HashMap::new();
            m.insert("general-ws".into(), WorkspacePolicy { max_clearance: DataLabel::Confidential });
            m
        },
    };

    let violations = pol.check_all_collections("general-ws");
    assert!(violations.is_empty());
}

// only collections above clearance are returned by check_all_collections
#[tokio::test]
async fn check_all_collections_returns_only_violations() {
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("public-docs".into(), CollectionPolicy { label: DataLabel::Public, block_on_violation: false });
            m.insert("pii-data".into(), CollectionPolicy { label: DataLabel::Pii, block_on_violation: true });
            m.insert("phi-data".into(), CollectionPolicy { label: DataLabel::Phi, block_on_violation: true });
            m
        },
        workspaces: {
            let mut m = HashMap::new();
            m.insert("restricted-ws".into(), WorkspacePolicy { max_clearance: DataLabel::Restricted });
            m
        },
    };

    let violations = pol.check_all_collections("restricted-ws");
    assert_eq!(violations.len(), 2);
    let names: Vec<_> = violations.iter().map(|v| v.collection.as_str()).collect();
    assert!(names.contains(&"pii-data"));
    assert!(names.contains(&"phi-data"));
}

// workspace with no entry in policy defaults to PUBLIC clearance
#[tokio::test]
async fn unknown_workspace_defaults_to_public_clearance() {
    let pol = ClassificationPolicy::default();
    let clearance = pol.workspace_clearance("some-unknown-workspace");
    assert_eq!(clearance, DataLabel::Public);
}

// warn-only violations (block=false) do not block but still show up
#[tokio::test]
async fn warn_only_violation_visible_but_not_blocking() {
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("confidential-docs".into(), CollectionPolicy {
                label: DataLabel::Confidential,
                block_on_violation: false,
            });
            m
        },
        workspaces: HashMap::new(),
    };

    let violation = pol.check_access("public-ws", "confidential-docs");
    assert!(violation.is_some());
    let v = violation.unwrap();
    assert!(!v.block); // warn only
    assert_eq!(v.required_label, DataLabel::Confidential);
}

// DataLabel ordering: PHI > PII > CONFIDENTIAL > RESTRICTED > PUBLIC
#[test]
fn data_label_ordering_correct() {
    assert!(DataLabel::Phi > DataLabel::Pii);
    assert!(DataLabel::Pii > DataLabel::Confidential);
    assert!(DataLabel::Confidential > DataLabel::Restricted);
    assert!(DataLabel::Restricted > DataLabel::Public);
}

// set_collection_label inserts and overwrites correctly
#[test]
fn set_collection_label_inserts_and_overwrites() {
    let mut pol = ClassificationPolicy::default();
    pol.set_collection_label("records", DataLabel::Pii, true);
    assert_eq!(pol.collections["records"].label, DataLabel::Pii);
    assert!(pol.collections["records"].block_on_violation);

    pol.set_collection_label("records", DataLabel::Restricted, false);
    assert_eq!(pol.collections["records"].label, DataLabel::Restricted);
    assert!(!pol.collections["records"].block_on_violation);
}

// cross-workspace isolation: workspace A's clearance does not bleed into workspace B
#[test]
fn workspace_clearance_isolated_across_workspaces() {
    let pol = ClassificationPolicy {
        collections: {
            let mut m = HashMap::new();
            m.insert("clinical".into(), CollectionPolicy { label: DataLabel::Phi, block_on_violation: true });
            m
        },
        workspaces: {
            let mut m = HashMap::new();
            m.insert("ws-phi".into(), WorkspacePolicy { max_clearance: DataLabel::Phi });
            // ws-public intentionally absent, should default to Public
            m
        },
    };

    // PHI workspace can access clinical
    assert!(pol.check_access("ws-phi", "clinical").is_none());
    // public workspace cannot
    let v = pol.check_access("ws-public", "clinical");
    assert!(v.is_some());
    assert!(v.unwrap().block);
}
