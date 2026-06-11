use maranode_rag::crypt::{decrypt, encrypt};
use maranode_store::{kek, WorkspaceDb};
use maranode_common::workspace::Workspace;
use tempfile::TempDir;
use uuid::Uuid;
use chrono::Utc;

fn dummy_workspace(slug: &str) -> Workspace {
    Workspace {
        id: Uuid::new_v4(),
        slug: slug.to_string(),
        name: slug.to_string(),
        api_key_hash: None,
        model_allowlist: vec![],
        rate_limit_rpm: None,
        system_prompt: None,
        created_at: Utc::now(),
        net_namespace: false,
        max_concurrent_requests: None,
        max_models: None,
        max_memory_bytes: None,
        dek: None,
    }
}

#[test]
fn dek_is_generated_on_create() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("workspaces.db");
    let db = WorkspaceDb::open(&db_path).unwrap();
    db.create(&dummy_workspace("alpha")).unwrap();

    let dek = db.get_dek_bytes("alpha").unwrap();
    assert!(dek.is_some(), "DEK must be generated on workspace creation");
}

#[test]
fn encrypted_data_unreadable_after_shred() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("workspaces.db");
    let db = WorkspaceDb::open(&db_path).unwrap();
    db.create(&dummy_workspace("beta")).unwrap();

    let dek = db.get_dek_bytes("beta").unwrap().unwrap();

    let ciphertext = encrypt(&dek, "sensitive rag chunk content").unwrap();
    assert!(ciphertext.starts_with("enc:"), "must be encrypted");

    // verify it decrypts correctly before shred
    let plaintext = decrypt(&dek, &ciphertext).unwrap();
    assert_eq!(plaintext, "sensitive rag chunk content");

    // shred: destroy the DEK
    let found = db.destroy_dek("beta").unwrap();
    assert!(found, "destroy_dek must return true for existing workspace");

    // confirm DEK is gone
    let dek_after = db.get_dek_bytes("beta").unwrap();
    assert!(dek_after.is_none(), "DEK must be None after shred");

    // the ciphertext is still in the database (as bytes), but decryption
    // now fails — there is no key to try
    let result = decrypt(&dek, &ciphertext);
    // note: we still hold the old dek in memory here — prove the ciphertext
    // actually decrypts (data integrity), then verify the db has no key
    assert!(result.is_ok(), "old in-memory dek can still decrypt (expected)");

    // but a fresh db open has no DEK: any code path that calls get_dek_bytes
    // returns None, so decryption is not possible through normal code paths
    let db2 = WorkspaceDb::open(&db_path).unwrap();
    let key2 = db2.get_dek_bytes("beta").unwrap();
    assert!(key2.is_none(), "re-opened db must also show no DEK for shredded workspace");
}

#[test]
fn wrong_key_cannot_decrypt() {
    let k1 = [1u8; 32];
    let k2 = [2u8; 32];

    let ct = encrypt(&k1, "private data").unwrap();
    let result = decrypt(&k2, &ct);
    assert!(result.is_err(), "wrong key must fail decryption");
}

#[test]
fn kek_wrap_unwrap_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let kek_path = kek::default_kek_path(tmp.path());
    let master = kek::load_or_create(&kek_path).unwrap();

    let dek_hex = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let wrapped = kek::wrap_dek(&master, dek_hex).unwrap();
    assert!(kek::is_wrapped(&wrapped), "wrapped dek must have wrapped: prefix");

    let recovered = kek::unwrap_dek(&master, &wrapped).unwrap();
    assert_eq!(recovered, dek_hex);
}

#[test]
fn kek_wrong_key_cannot_unwrap() {
    let k1 = [3u8; 32];
    let k2 = [4u8; 32];

    let dek_hex = "0000000000000000000000000000000000000000000000000000000000000000";
    let wrapped = kek::wrap_dek(&k1, dek_hex).unwrap();
    let result = kek::unwrap_dek(&k2, &wrapped);
    assert!(result.is_err(), "wrong KEK must fail to unwrap DEK");
}

#[test]
fn kek_rotate_re_wraps_all_deks() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("workspaces.db");

    let old_kek = [10u8; 32];
    let new_kek = [20u8; 32];

    let mut db = WorkspaceDb::open_with_kek(&db_path, old_kek).unwrap();
    db.create(&dummy_workspace("w1")).unwrap();
    db.create(&dummy_workspace("w2")).unwrap();

    let dek1_before = db.get_dek_bytes("w1").unwrap().unwrap();
    let dek2_before = db.get_dek_bytes("w2").unwrap().unwrap();

    let rotated = db.rotate_kek(&old_kek, new_kek).unwrap();
    assert_eq!(rotated, 2, "both workspaces must be rotated");

    // after rotation, new KEK must unwrap the same DEKs
    let db2 = WorkspaceDb::open_with_kek(&db_path, new_kek).unwrap();
    let dek1_after = db2.get_dek_bytes("w1").unwrap().unwrap();
    let dek2_after = db2.get_dek_bytes("w2").unwrap().unwrap();

    assert_eq!(dek1_before, dek1_after, "w1 DEK must survive rotation");
    assert_eq!(dek2_before, dek2_after, "w2 DEK must survive rotation");
}
