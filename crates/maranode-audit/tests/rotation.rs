use std::path::Path;

use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::rotate::{self, Manifest, RotateConfig};
use maranode_audit::{verify, AuditLog};
use maranode_common::events::AuditEvent;

fn ev(n: u64) -> AuditEvent {
    AuditEvent::DaemonStop {
        reason: format!("stop-{n}"),
    }
}

async fn append_range(log: &AuditLog, from: u64, to: u64) {
    for i in from..to {
        log.append("tester", ev(i)).await.unwrap();
    }
}

fn write_manifest(dir: &Path, m: &Manifest) {
    std::fs::write(
        rotate::manifest_path(dir),
        serde_json::to_vec_pretty(m).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn rotation_seals_segment_and_chain_stays_valid() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 50).await;

    let cfg = RotateConfig {
        max_bytes: 1,
        max_age_days: 0,
    };
    let seg = log.maybe_rotate(&log_path, &cfg).await.unwrap().unwrap();
    assert_eq!(seg.seq_start, 1);
    assert_eq!(seg.seq_end, 50);
    assert_eq!(seg.entries, 50);

    let active = std::fs::read_to_string(&log_path).unwrap();
    assert!(active.trim().is_empty(), "active log should be empty after rotation");

    append_range(&log, 50, 80).await;

    let key = maranode_audit::key::load(&key_path).unwrap();
    let res = verify::verify_all(dir.path(), &key, &log_path).unwrap();
    assert!(res.ok, "verify_all failed: {:?}", res.first_violation);
    assert_eq!(res.entries_checked, 80);

    assert_eq!(rotate::load_manifest(dir.path()).unwrap().segments.len(), 1);
}

#[tokio::test]
async fn chain_survives_restart_after_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 20).await;
    let cfg = RotateConfig {
        max_bytes: 1,
        max_age_days: 0,
    };
    let seg = log.maybe_rotate(&log_path, &cfg).await.unwrap().unwrap();
    drop(log);

    let log2 = AuditLog::open(&log_path, &key_path).unwrap();
    assert_eq!(log2.seq().await, seg.seq_end);
    log2.append("tester", ev(99)).await.unwrap();
    assert_eq!(log2.seq().await, seg.seq_end + 1);

    let key = maranode_audit::key::load(&key_path).unwrap();
    let res = verify::verify_all(dir.path(), &key, &log_path).unwrap();
    assert!(res.ok, "{:?}", res.first_violation);
    assert_eq!(res.entries_checked, 21);
}

#[tokio::test]
async fn old_segments_are_pruned() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 10).await;
    let cfg = RotateConfig {
        max_bytes: 1,
        max_age_days: 0,
    };
    let seg = log.maybe_rotate(&log_path, &cfg).await.unwrap().unwrap();

    let mut m = rotate::load_manifest(dir.path()).unwrap();
    m.segments[0].ts_last = chrono::Utc::now() - chrono::Duration::days(400);
    write_manifest(dir.path(), &m);

    let seg_file = rotate::segment_dir(dir.path()).join(&seg.file);
    assert!(seg_file.exists());

    let removed = rotate::enforce_segment_retention(dir.path(), 90).unwrap();
    assert_eq!(removed, 1);
    assert!(!seg_file.exists());
    assert!(rotate::load_manifest(dir.path()).unwrap().segments.is_empty());
}

#[tokio::test]
async fn tampering_active_file_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 10).await;
    let cfg = RotateConfig {
        max_bytes: 1,
        max_age_days: 0,
    };
    log.maybe_rotate(&log_path, &cfg).await.unwrap().unwrap();
    append_range(&log, 10, 15).await;
    drop(log);

    let content = std::fs::read_to_string(&log_path)
        .unwrap()
        .replacen("stop-10", "stop-XX", 1);
    std::fs::write(&log_path, content).unwrap();

    let key = maranode_audit::key::load(&key_path).unwrap();
    let res = verify::verify_all(dir.path(), &key, &log_path).unwrap();
    assert!(!res.ok, "tampered active file should fail verification");
}

#[tokio::test]
async fn tampering_sealed_segment_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 10).await;
    let cfg = RotateConfig {
        max_bytes: 1,
        max_age_days: 0,
    };
    let seg = log.maybe_rotate(&log_path, &cfg).await.unwrap().unwrap();
    drop(log);

    let seg_file = rotate::segment_dir(dir.path()).join(&seg.file);
    let mut bytes = std::fs::read(&seg_file).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&seg_file, bytes).unwrap();

    let key = maranode_audit::key::load(&key_path).unwrap();
    assert!(verify::verify_all(dir.path(), &key, &log_path).is_err());
}

#[tokio::test]
async fn plain_log_without_rotation_still_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = default_log_path(dir.path());
    let key_path = default_key_path(dir.path());

    let log = AuditLog::open(&log_path, &key_path).unwrap();
    append_range(&log, 0, 5).await;

    let key = maranode_audit::key::load(&key_path).unwrap();
    let res = verify::verify_log(&log_path, &key).unwrap();
    assert!(res.ok);
    assert_eq!(res.entries_checked, 5);
}
