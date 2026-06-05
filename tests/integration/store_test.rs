//! tests for model store import, list, verify, remove etc

use std::io::Write;
use std::path::Path;

use maranode_common::models::{ModelId, ModelType};
use maranode_store::ModelStore;
use tempfile::TempDir;

fn write_fake_gguf(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"GGUF fake model data for testing purposes 1234567890")
        .unwrap();
    path
}

#[tokio::test]
async fn import_and_list() {
    let data_dir = TempDir::new().unwrap();
    let model_dir = TempDir::new().unwrap();
    let model_path = write_fake_gguf(model_dir.path(), "model.gguf");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id = ModelId::new("test-model", "v1");

    let manifest = store
        .import_from_file(&model_path, id.clone(), None, ModelType::Llm)
        .await
        .unwrap();
    assert_eq!(manifest.model_id, id);
    assert_eq!(manifest.sha256.len(), 64, "sha256 should be 64 hex chars");

    let models = store.list().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model_id, id);
}

#[tokio::test]
async fn deduplication_same_content() {
    let data_dir = TempDir::new().unwrap();
    let model_dir = TempDir::new().unwrap();
    let model_path = write_fake_gguf(model_dir.path(), "model.gguf");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id1 = ModelId::new("model-a", "v1");
    let id2 = ModelId::new("model-b", "v1");

    let m1 = store
        .import_from_file(&model_path, id1, None, ModelType::Llm)
        .await
        .unwrap();
    let m2 = store
        .import_from_file(&model_path, id2, None, ModelType::Llm)
        .await
        .unwrap();

    assert_eq!(m1.sha256, m2.sha256, "same content must have same sha256");

    let blobs: Vec<_> = std::fs::read_dir(data_dir.path().join("blobs"))
        .unwrap()
        .collect();
    assert_eq!(blobs.len(), 1, "deduplicated content must produce one blob");
}

#[tokio::test]
async fn remove_deletes_entry() {
    let data_dir = TempDir::new().unwrap();
    let model_dir = TempDir::new().unwrap();
    let model_path = write_fake_gguf(model_dir.path(), "model.gguf");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id = ModelId::new("remove-me", "v1");

    store
        .import_from_file(&model_path, id.clone(), None, ModelType::Llm)
        .await
        .unwrap();
    assert_eq!(store.list().await.unwrap().len(), 1);

    let removed = store.remove(&id).await.unwrap();
    assert!(removed);
    assert_eq!(store.list().await.unwrap().len(), 0);
}

#[tokio::test]
async fn remove_shared_blob_only_when_last_reference() {
    let data_dir = TempDir::new().unwrap();
    let model_dir = TempDir::new().unwrap();
    let model_path = write_fake_gguf(model_dir.path(), "model.gguf");

    let store = ModelStore::open(data_dir.path()).unwrap();
    let id1 = ModelId::new("model-a", "v1");
    let id2 = ModelId::new("model-b", "v1");

    let m1 = store
        .import_from_file(&model_path, id1.clone(), None, ModelType::Llm)
        .await
        .unwrap();
    store
        .import_from_file(&model_path, id2.clone(), None, ModelType::Llm)
        .await
        .unwrap();

    store.remove(&id1).await.unwrap();
    let blob_path = std::path::PathBuf::from(&m1.blob_path);
    assert!(
        blob_path.exists(),
        "blob must survive while id2 still references it"
    );

    store.remove(&id2).await.unwrap();
    assert!(
        !blob_path.exists(),
        "blob must be deleted when last reference is removed"
    );
}

#[tokio::test]
async fn get_nonexistent_returns_none() {
    let data_dir = TempDir::new().unwrap();
    let store = ModelStore::open(data_dir.path()).unwrap();
    let result = store.get(&ModelId::new("ghost", "v1")).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn size_bytes_is_accurate() {
    let data_dir = TempDir::new().unwrap();
    let model_dir = TempDir::new().unwrap();

    let content = b"exact size content";
    let path = model_dir.path().join("exact.gguf");
    std::fs::write(&path, content).unwrap();

    let store = ModelStore::open(data_dir.path()).unwrap();
    let manifest = store
        .import_from_file(&path, ModelId::new("size-test", "v1"), None, ModelType::Llm)
        .await
        .unwrap();

    assert_eq!(manifest.size_bytes, content.len() as u64);
}
