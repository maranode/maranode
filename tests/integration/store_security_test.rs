//! tests that model blobs are stored by hash and cannot be redirected

use std::io::Write;
use std::path::Path;

use maranode_common::models::{ModelId, ModelType};
use maranode_store::ModelStore;
use tempfile::TempDir;

fn write_file(dir: &Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content).unwrap();
    path
}

#[tokio::test]
async fn blob_integrity_passes_for_unmodified_blob() {
    let data_dir = TempDir::new().unwrap();
    let src_dir = TempDir::new().unwrap();
    let path = write_file(src_dir.path(), "model.gguf", b"original content");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id = ModelId::new("integrity-ok", "v1");
    store
        .import_from_file(&path, id.clone(), None, ModelType::Llm)
        .await
        .unwrap();

    let blob = store.blob_path_verified(&id).await;
    assert!(blob.is_ok(), "unmodified blob must pass integrity check");
}

#[tokio::test]
async fn blob_integrity_fails_if_blob_modified() {
    let data_dir = TempDir::new().unwrap();
    let src_dir = TempDir::new().unwrap();
    let path = write_file(src_dir.path(), "model.gguf", b"original content");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id = ModelId::new("integrity-fail", "v1");
    let manifest = store
        .import_from_file(&path, id.clone(), None, ModelType::Llm)
        .await
        .unwrap();

    std::fs::write(&manifest.blob_path, b"CORRUPTED CONTENT").unwrap();

    let result = store.blob_path_verified(&id).await;
    assert!(
        result.is_err(),
        "modified blob must fail SHA-256 integrity check"
    );
    let err = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err.contains("integ")
            || err.contains("sha")
            || err.contains("mismatch")
            || err.contains("blob"),
        "error message should mention integrity: {}",
        err
    );
}

#[tokio::test]
async fn different_content_different_sha256() {
    let data_dir = TempDir::new().unwrap();
    let src_dir = TempDir::new().unwrap();

    let path_a = write_file(src_dir.path(), "a.gguf", b"content of model A");
    let path_b = write_file(src_dir.path(), "b.gguf", b"content of model B");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let m_a = store
        .import_from_file(&path_a, ModelId::new("a", "v1"), None, ModelType::Llm)
        .await
        .unwrap();
    let m_b = store
        .import_from_file(&path_b, ModelId::new("b", "v1"), None, ModelType::Llm)
        .await
        .unwrap();

    assert_ne!(
        m_a.sha256, m_b.sha256,
        "different content must produce different SHA-256"
    );

    let blobs: Vec<_> = std::fs::read_dir(data_dir.path().join("blobs"))
        .unwrap()
        .collect();
    assert_eq!(blobs.len(), 2);
}

#[tokio::test]
async fn duplicate_model_id_rejected() {
    let data_dir = TempDir::new().unwrap();
    let src_dir = TempDir::new().unwrap();

    let path_a = write_file(src_dir.path(), "a.gguf", b"model A content");
    let path_b = write_file(src_dir.path(), "b.gguf", b"model B different content");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id = ModelId::new("same-name", "v1");

    store
        .import_from_file(&path_a, id.clone(), None, ModelType::Llm)
        .await
        .unwrap();

    let result = store
        .import_from_file(&path_b, id.clone(), None, ModelType::Llm)
        .await;
    assert!(
        result.is_err(),
        "re-importing a different file under the same model ID must be rejected"
    );
}

#[tokio::test]
async fn blob_path_stays_inside_blobs_dir() {
    let data_dir = TempDir::new().unwrap();
    let src_dir = TempDir::new().unwrap();
    let path = write_file(src_dir.path(), "model.gguf", b"safe path test");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let manifest = store
        .import_from_file(
            &path,
            ModelId::new("path-check", "v1"),
            None,
            ModelType::Llm,
        )
        .await
        .unwrap();

    let blob_path = std::path::Path::new(&manifest.blob_path);
    let blobs_dir = data_dir.path().join("blobs");

    assert!(
        blob_path.starts_with(&blobs_dir),
        "blob path {:?} must be inside blobs dir {:?}",
        blob_path,
        blobs_dir
    );

    let file_name = blob_path.file_name().unwrap().to_string_lossy();
    assert!(
        !file_name.contains(".."),
        "blob file name must not contain '..'"
    );
    assert!(
        !file_name.contains('/'),
        "blob file name must not contain '/'"
    );
}
