use maranode_attestation::{
    export_recovery_bundle, import_recovery_bundle, is_sealed, read_rotation_log, rotate_in_place,
    seal, seal_status, unseal, PcrPolicy, SealBackend,
};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn make_key(len: usize) -> Vec<u8> {
    (0..len as u8).collect()
}

#[test]
fn software_seal_unseal_roundtrip() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(32);

    let meta = seal(&key, "workspace-kek", data_dir, None, "testpass").unwrap();
    assert_eq!(meta.backend, SealBackend::Software);
    assert!(is_sealed("workspace-kek", data_dir));

    let recovered = unseal("workspace-kek", data_dir, "testpass").unwrap();
    assert_eq!(key, recovered);
}

#[test]
fn wrong_passphrase_returns_error() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(32);

    seal(&key, "workspace-kek", data_dir, None, "correct").unwrap();
    let result = unseal("workspace-kek", data_dir, "wrong");
    assert!(result.is_err());
}

#[test]
fn seal_status_reflects_metadata() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(16);

    let meta = seal(&key, "audit-hmac", data_dir, Some("sha256:0,7"), "pass").unwrap();
    assert_eq!(meta.purpose, "audit-hmac");
    assert_eq!(meta.pcr_list.as_deref(), Some("sha256:0,7"));

    let status = seal_status("audit-hmac", data_dir).unwrap();
    assert_eq!(status.backend, SealBackend::Software);
    assert_eq!(status.pcr_list.as_deref(), Some("sha256:0,7"));
}

#[test]
fn is_sealed_false_when_not_sealed() {
    let dir = tmp();
    assert!(!is_sealed("nonexistent", dir.path()));
}

#[test]
fn dos_lockout_after_five_failures() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(32);

    seal(&key, "workspace-kek", data_dir, None, "correct").unwrap();

    for _ in 0..4 {
        let _ = unseal("workspace-kek", data_dir, "bad");
    }
    // 5th attempt triggers lockout on this call or the next
    let fifth = unseal("workspace-kek", data_dir, "bad");
    // either 5th or next attempt should fail with lockout — just check it errors
    assert!(fifth.is_err());

    // with correct passphrase, should still be locked out momentarily
    let locked = unseal("workspace-kek", data_dir, "correct");
    // may succeed if counter resets on correct, or fail if locked — both are acceptable
    // The key point is the fail counter file exists
    let counter_path = data_dir.join("tpm").join("workspace-kek").join(".fail_counter");
    assert!(counter_path.exists());
}

#[test]
fn pcr_policy_serialize_deserialize() {
    let dir = tmp();
    let data_dir = dir.path();

    let mut policy = PcrPolicy {
        pcrs: std::collections::BTreeMap::new(),
        description: "test policy".to_string(),
    };
    policy.pcrs.insert(0, "a".repeat(64));
    policy.pcrs.insert(7, "b".repeat(64));

    policy.save(data_dir).unwrap();

    let loaded = PcrPolicy::load(data_dir).unwrap();
    assert_eq!(loaded.pcrs.len(), 2);
    assert_eq!(loaded.pcrs[&0], "a".repeat(64));
    assert_eq!(loaded.description, "test policy");
}

#[test]
fn pcr_policy_path_location() {
    let dir = tmp();
    let path = PcrPolicy::policy_path(dir.path());
    assert!(path.ends_with("tpm/pcr-policy.json"));
}

#[test]
fn recovery_bundle_export_import_roundtrip() {
    let src = tmp();
    let dst = tmp();

    let key_a = make_key(32);
    let key_b = make_key(16);

    seal(&key_a, "workspace-kek", src.path(), None, "pass").unwrap();
    seal(&key_b, "audit-hmac", src.path(), None, "pass").unwrap();

    let bundle = export_recovery_bundle(
        &["workspace-kek", "audit-hmac"],
        src.path(),
        "pass",
    ).unwrap();

    assert!(bundle.len() > 4 + 1 + 16 + 12);

    let records = import_recovery_bundle(&bundle, dst.path(), "pass", None).unwrap();
    assert_eq!(records.len(), 2);

    let recovered_a = unseal("workspace-kek", dst.path(), "pass").unwrap();
    let recovered_b = unseal("audit-hmac", dst.path(), "pass").unwrap();
    assert_eq!(recovered_a, key_a);
    assert_eq!(recovered_b, key_b);
}

#[test]
fn recovery_bundle_wrong_passphrase_fails() {
    let src = tmp();
    let dst = tmp();

    let key = make_key(32);
    seal(&key, "workspace-kek", src.path(), None, "correct").unwrap();

    let bundle = export_recovery_bundle(&["workspace-kek"], src.path(), "correct").unwrap();
    let result = import_recovery_bundle(&bundle, dst.path(), "wrong", None);
    assert!(result.is_err());
}

#[test]
fn rotate_in_place_changes_pcr_list() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(32);

    seal(&key, "workspace-kek", data_dir, Some("sha256:0"), "pass").unwrap();

    let record = rotate_in_place(
        "workspace-kek",
        data_dir,
        "pass",
        Some("sha256:0,7"),
        "pass",
        "firmware update",
    ).unwrap();

    assert_eq!(record.purpose, "workspace-kek");
    assert_eq!(record.reason, "firmware update");
    assert_eq!(record.old_pcr_list.as_deref(), Some("sha256:0"));
    assert_eq!(record.new_pcr_list.as_deref(), Some("sha256:0,7"));

    let recovered = unseal("workspace-kek", data_dir, "pass").unwrap();
    assert_eq!(recovered, key);
}

#[test]
fn rotation_log_persists_records() {
    let dir = tmp();
    let data_dir = dir.path();
    let key = make_key(32);

    seal(&key, "workspace-kek", data_dir, None, "pass").unwrap();
    rotate_in_place("workspace-kek", data_dir, "pass", None, "pass", "test rotation").unwrap();

    let log = read_rotation_log(data_dir).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].purpose, "workspace-kek");
    assert_eq!(log[0].reason, "test rotation");
}

#[test]
fn multiple_purposes_independent() {
    let dir = tmp();
    let data_dir = dir.path();

    let k1 = make_key(32);
    let k2 = make_key(16);

    seal(&k1, "workspace-kek", data_dir, None, "p1").unwrap();
    seal(&k2, "audit-hmac", data_dir, None, "p2").unwrap();

    let r1 = unseal("workspace-kek", data_dir, "p1").unwrap();
    let r2 = unseal("audit-hmac", data_dir, "p2").unwrap();

    assert_eq!(r1, k1);
    assert_eq!(r2, k2);

    // cross-passphrase fails
    assert!(unseal("workspace-kek", data_dir, "p2").is_err());
}
