//! tests for audit log HMAC chain and security rules

use std::path::Path;

use maranode_audit::{
    key::load_or_generate,
    log::{default_key_path, default_log_path},
    verify::verify_log,
    AuditLog,
};
use maranode_common::events::AuditEvent;
use tempfile::TempDir;

async fn open_log(dir: &Path) -> AuditLog {
    AuditLog::open(&default_log_path(dir), &default_key_path(dir)).unwrap()
}

fn load_key(dir: &Path) -> Vec<u8> {
    load_or_generate(&default_key_path(dir)).unwrap()
}

fn daemon_start(v: &str) -> AuditEvent {
    AuditEvent::DaemonStart {
        version: v.into(),
        air_gap: true,
    }
}

fn daemon_stop() -> AuditEvent {
    AuditEvent::DaemonStop {
        reason: "test".into(),
    }
}

#[tokio::test]
async fn chain_intact_after_writes() {
    let tmp = TempDir::new().unwrap();
    let log = open_log(tmp.path()).await;

    for i in 0..10u32 {
        log.append("test", daemon_start(&format!("0.1.{}", i)))
            .await
            .unwrap();
    }

    let key = load_key(tmp.path());
    let result = verify_log(&default_log_path(tmp.path()), &key).unwrap();
    assert!(
        result.ok,
        "intact chain should verify: {:?}",
        result.first_violation
    );
    assert_eq!(result.entries_checked, 10);
}

#[tokio::test]
async fn tampered_content_detected() {
    let tmp = TempDir::new().unwrap();
    let log = open_log(tmp.path()).await;

    log.append("test", daemon_start("0.1.0")).await.unwrap();
    log.append("test", daemon_stop()).await.unwrap();

    let log_path = default_log_path(tmp.path());
    let original = std::fs::read_to_string(&log_path).unwrap();
    let tampered = original.replacen("daemon_start", "daemon_XXXXX", 1);
    std::fs::write(&log_path, tampered).unwrap();

    let key = load_key(tmp.path());
    let result = verify_log(&log_path, &key).unwrap();
    assert!(!result.ok, "tampered entry must fail verification");
    assert!(result.first_violation.is_some());
}

#[tokio::test]
async fn deleted_entry_detected() {
    let tmp = TempDir::new().unwrap();
    let log = open_log(tmp.path()).await;

    for i in 0..5 {
        log.append(
            "test",
            AuditEvent::DaemonStop {
                reason: format!("r{}", i),
            },
        )
        .await
        .unwrap();
    }

    let log_path = default_log_path(tmp.path());
    let content = std::fs::read_to_string(&log_path).unwrap();
    let new_content: String = content
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&log_path, new_content).unwrap();

    let key = load_key(tmp.path());
    let result = verify_log(&log_path, &key).unwrap();
    assert!(
        !result.ok || result.entries_checked < 5,
        "deleted entry must break verification"
    );
}

#[tokio::test]
async fn sequence_increments_monotonically() {
    let tmp = TempDir::new().unwrap();
    let log = open_log(tmp.path()).await;

    assert_eq!(log.seq().await, 0, "fresh log starts at seq 0");

    log.append("test", daemon_start("0.1.0")).await.unwrap();
    assert_eq!(log.seq().await, 1);

    log.append("test", daemon_start("0.1.1")).await.unwrap();
    assert_eq!(log.seq().await, 2);
}

#[tokio::test]
async fn log_resumes_correctly_after_reopen() {
    let tmp = TempDir::new().unwrap();

    {
        let log = open_log(tmp.path()).await;
        for i in 0..3 {
            log.append("test", daemon_start(&format!("0.1.{}", i)))
                .await
                .unwrap();
        }
    }

    {
        let log = open_log(tmp.path()).await;
        assert_eq!(log.seq().await, 3, "resumed seq should be 3");
        log.append("test", daemon_stop()).await.unwrap();
        log.append("test", daemon_stop()).await.unwrap();
    }

    let key = load_key(tmp.path());
    let result = verify_log(&default_log_path(tmp.path()), &key).unwrap();
    assert!(
        result.ok,
        "chain across reopen must verify: {:?}",
        result.first_violation
    );
    assert_eq!(result.entries_checked, 5);
}

#[tokio::test]
async fn wrong_key_fails_verification() {
    let tmp = TempDir::new().unwrap();
    let log = open_log(tmp.path()).await;

    log.append("test", daemon_start("0.1.0")).await.unwrap();
    log.append("test", daemon_stop()).await.unwrap();

    let wrong_key = vec![0xAB_u8; 32];
    let result = verify_log(&default_log_path(tmp.path()), &wrong_key).unwrap();
    assert!(!result.ok, "wrong key must fail HMAC verification");
}
