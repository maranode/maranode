//! end-to-end API tests. need built `maranoded` binary.
//! ```bash
//! cargo build --bin maranoded
//! cargo test --test api_e2e -- --ignored
//! ```

use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::time::sleep;

const DATA_DIR_BASE: &str = "/tmp/maranode-e2e";

static NEXT_PORT: AtomicU16 = AtomicU16::new(11435);

fn alloc_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

async fn spawn_daemon_on(port: u16) -> tokio::process::Child {
    let data_dir = format!("{}-{}", DATA_DIR_BASE, port);
    let _ = std::fs::remove_dir_all(&data_dir);

    let child = tokio::process::Command::new(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("maranoded"),
    )
    .args([
        "--no-isolation",
        "--bind",
        &format!("127.0.0.1:{}", port),
        "--data-dir",
        &data_dir,
        "--log-level",
        "warn",
    ])
    .kill_on_drop(true)
    .spawn()
    .expect("failed to spawn maranoded: run `cargo build --bin maranoded` first");

    sleep(Duration::from_millis(300)).await;
    child
}

fn base(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_health_ok() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/health", base(port)))
        .await
        .expect("GET /health failed");

    assert!(resp.status().is_success());
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["ok"], true);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_models_empty_on_fresh_start() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/models", base(port)))
        .await
        .expect("GET /v1/models failed");

    assert!(resp.status().is_success());
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["object"], "list");
    assert!(json["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_chat_unknown_model_returns_404() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base(port)))
        .json(&serde_json::json!({
            "model":    "nonexistent:3b",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "max_tokens": 16,
        }))
        .send()
        .await
        .expect("POST /v1/chat/completions failed");

    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_stats_shape() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/stats", base(port)))
        .await
        .expect("GET /stats failed");

    assert!(resp.status().is_success());
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["uptime_secs"].is_number());
    assert!(json["requests"].is_number());
    assert!(json["errors"].is_number());
    assert!(json["queue_depth"].is_number());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_workspace_lifecycle() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();
    let b = base(port);

    // GET list workspaces: expect empty
    let resp = c.get(format!("{}/v1/workspaces", b)).send().await.unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["workspaces"].as_array().unwrap().is_empty());

    // POST create workspace
    let resp = c
        .post(format!("{}/v1/workspaces", b))
        .json(&serde_json::json!({
            "slug": "acme",
            "name": "Acme Corp",
            "no_key": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["workspace"]["slug"], "acme");
    assert_eq!(body["workspace"]["name"], "Acme Corp");
    assert_eq!(body["workspace"]["has_key"], false);

    // GET workspace by slug
    let resp = c
        .get(format!("{}/v1/workspaces/acme", b))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["slug"], "acme");

    // PUT update workspace name
    let resp = c
        .put(format!("{}/v1/workspaces/acme", b))
        .json(&serde_json::json!({ "name": "Acme Corp v2" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["workspace"]["name"], "Acme Corp v2");

    // GET list: expect one workspace
    let resp = c.get(format!("{}/v1/workspaces", b)).send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["workspaces"].as_array().unwrap().len(), 1);

    // DELETE workspace
    let resp = c
        .delete(format!("{}/v1/workspaces/acme", b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // GET workspace after delete: expect 404
    let resp = c
        .get(format!("{}/v1/workspaces/acme", b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_workspace_bad_slug_rejected() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/v1/workspaces", base(port)))
        .json(&serde_json::json!({
            "slug": "bad slug!",
            "name": "Bad",
            "no_key": true,
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_workspace_key_auto_generated() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/v1/workspaces", base(port)))
        .json(&serde_json::json!({ "slug": "keyed", "name": "Keyed" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["workspace"]["has_key"], true);
    assert!(body["api_key"].is_string());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_workspace_key_rotate() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();
    let b = base(port);

    c.post(format!("{}/v1/workspaces", b))
        .json(&serde_json::json!({ "slug": "rotateme", "name": "Rotate" }))
        .send()
        .await
        .unwrap();

    let resp = c
        .put(format!("{}/v1/workspaces/rotateme", b))
        .json(&serde_json::json!({ "rotate_key": true }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["api_key"].is_string(),
        "rotate_key should return a new api_key"
    );
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_user_lifecycle() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();
    let b = base(port);

    // POST create user (no API key on daemon, so no auth required)
    let resp = c
        .post(format!("{}/v1/users", b))
        .json(&serde_json::json!({
            "username": "alice",
            "password": "hunter2",
            "email":    "alice@example.com",
            "role":     "admin",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created: serde_json::Value = resp.json().await.unwrap();
    let user_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["username"], "alice");

    // POST login
    let resp = c
        .post(format!("{}/v1/auth/login", b))
        .json(&serde_json::json!({ "username": "alice", "password": "hunter2" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let token = login_body["token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());

    // GET /auth/me with bearer token
    let resp = c
        .get(format!("{}/v1/auth/me", b))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let me: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(me["username"], "alice");

    // PUT update user email
    let resp = c
        .put(format!("{}/v1/users/{}", b, user_id))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "email": "alice2@example.com" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let updated: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(updated["email"], "alice2@example.com");

    // GET user by id
    let resp = c
        .get(format!("{}/v1/users/{}", b, user_id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // PUT change password
    let resp = c
        .put(format!("{}/v1/users/{}/password", b, user_id))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "password": "new_pass_123" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // POST login with new password
    let resp = c
        .post(format!("{}/v1/auth/login", b))
        .json(&serde_json::json!({ "username": "alice", "password": "new_pass_123" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let new_token = resp.json::<serde_json::Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();

    // POST logout
    let resp = c
        .post(format!("{}/v1/auth/logout", b))
        .bearer_auth(&new_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // DELETE user
    let resp = c
        .delete(format!("{}/v1/users/{}", b, user_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // GET user after delete: expect 404
    let resp = c
        .get(format!("{}/v1/users/{}", b, user_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_login_wrong_password_is_401() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();
    let b = base(port);

    c.post(format!("{}/v1/users", b))
        .json(&serde_json::json!({ "username": "bob", "password": "correct" }))
        .send()
        .await
        .unwrap();

    let resp = c
        .post(format!("{}/v1/auth/login", b))
        .json(&serde_json::json!({ "username": "bob", "password": "wrong" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_me_without_token_is_401() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/auth/me", base(port)))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_list_users_empty_on_fresh_start() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/users", base(port)))
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["users"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_audit_entries_returns_array() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/audit/entries", base(port)))
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.is_array());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_audit_prune_returns_count() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/v1/audit/prune", base(port)))
        .json(&serde_json::json!({ "retain_days": 365 }))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["pruned"].is_number());
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_audit_export_gdpr_is_csv() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/audit/export?format=gdpr", base(port)))
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/csv"));
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_audit_export_unknown_format_is_400() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/audit/export?format=nope", base(port)))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "requires compiled maranoded binary: run with --ignored"]
async fn e2e_audit_bundle_is_zip() {
    let port = alloc_port();
    let _d = spawn_daemon_on(port).await;

    let resp = reqwest::get(format!("{}/v1/audit/bundle", base(port)))
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("application/zip"));
}
