//! ModelStore: SQLite for manifests and blob files stored by hash

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use maranode_common::models::{ModelFormat, ModelId, ModelManifest, ModelType};

use crate::blob;
use crate::db::ManifestDb;

#[derive(Clone)]
pub struct ModelStore {
    inner: Arc<RwLock<StoreInner>>,
}

struct StoreInner {
    db: Mutex<ManifestDb>,
    data_dir: PathBuf,
}

impl ModelStore {
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("models.db");
        let db = ManifestDb::open(&db_path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(StoreInner {
                db: Mutex::new(db),
                data_dir: data_dir.to_path_buf(),
            })),
        })
    }

    pub async fn import_from_file(
        &self,
        path: &Path,
        model_id: ModelId,
        quantization: Option<String>,
        model_type: ModelType,
    ) -> Result<ModelManifest> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("opening model file {}", path.display()))?;

        let inner = self.inner.write().await;
        let (sha256, size_bytes) = blob::import_blob(&inner.data_dir, file)?;

        let blob_path = blob::blob_absolute_path(&inner.data_dir, &sha256)?
            .to_string_lossy()
            .into_owned();

        let manifest = ModelManifest {
            id: Uuid::new_v4(),
            model_id: model_id.clone(),
            sha256: sha256.clone(),
            size_bytes,
            format: ModelFormat::Gguf,
            quantization,
            imported_at: Utc::now(),
            blob_path,
            model_type,
        };

        inner
            .db
            .lock()
            .unwrap()
            .insert(&manifest)
            .context("inserting manifest")?;

        info!(
            "Imported model {} ({} bytes, sha256:{})",
            model_id,
            size_bytes,
            &sha256[..12]
        );
        Ok(manifest)
    }

    pub async fn get(&self, model_id: &ModelId) -> Result<Option<ModelManifest>> {
        self.inner.read().await.db.lock().unwrap().get(model_id)
    }

    pub async fn blob_path_verified(&self, model_id: &ModelId) -> Result<PathBuf> {
        let inner = self.inner.read().await;
        let manifest = inner
            .db
            .lock()
            .unwrap()
            .get(model_id)?
            .ok_or_else(|| anyhow::anyhow!("model not found: {}", model_id))?;

        blob::verify_blob(&inner.data_dir, &manifest.sha256).context("blob integrity check")?;

        blob::blob_absolute_path(&inner.data_dir, &manifest.sha256)
    }

    /// path to model blob without reading file again (for trusted local lookups)
    /// path comes only from the SHA-256 digest, not from the `blob_path` column in the database
    /// if someone changes the database, they cannot redirect loads to a file outside the blobs folder
    pub async fn blob_path(&self, model_id: &ModelId) -> Result<PathBuf> {
        let inner = self.inner.read().await;
        let manifest = inner
            .db
            .lock()
            .unwrap()
            .get(model_id)?
            .ok_or_else(|| anyhow::anyhow!("model not found: {}", model_id))?;
        let path = blob::blob_absolute_path(&inner.data_dir, &manifest.sha256)?;
        if !path.exists() {
            anyhow::bail!("blob missing on disk for {}: {}", model_id, path.display());
        }
        Ok(path)
    }

    pub async fn list(&self) -> Result<Vec<ModelManifest>> {
        self.inner.read().await.db.lock().unwrap().list()
    }

    pub async fn remove(&self, model_id: &ModelId) -> Result<bool> {
        let inner = self.inner.write().await;
        let db = inner.db.lock().unwrap();
        let manifest = db.get(model_id)?;
        let removed = db.delete(model_id)?;

        if removed {
            if let Some(m) = manifest {
                let others: Vec<_> = db
                    .list()?
                    .into_iter()
                    .filter(|x| x.sha256 == m.sha256)
                    .collect();
                if others.is_empty() {
                    match blob::blob_absolute_path(&inner.data_dir, &m.sha256) {
                        Ok(path) => {
                            if let Err(e) = std::fs::remove_file(&path) {
                                warn!("could not remove blob {}: {}", path.display(), e);
                            }
                        }
                        Err(e) => warn!("skipping blob removal, invalid digest: {}", e),
                    }
                }
            }
            info!("Removed model {}", model_id);
        }

        Ok(removed)
    }

    /// download GGUF from URL, compute SHA-256 while downloading, then register in the store
    /// progress callback receives (downloaded_bytes, total_bytes_if_known)
    pub async fn pull_from_url(
        &self,
        url: &str,
        model_id: ModelId,
        quantization: Option<String>,
        model_type: ModelType,
        on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
    ) -> Result<ModelManifest> {
        use futures_util::StreamExt;
        use sha2::{Digest, Sha256};

        let inner = self.inner.write().await;
        let tmp = inner
            .data_dir
            .join("blobs")
            .join(format!("{}.pull.part", Uuid::new_v4()));
        std::fs::create_dir_all(tmp.parent().unwrap())?;

        let client = reqwest::Client::builder()
            .user_agent("maranode/0.1 (+https://maranode.com)")
            .build()?;

        let resp = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("download failed for {url}"))?;

        let total = resp.content_length();
        let mut stream = resp.bytes_stream();
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;

        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("reading chunk")?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total);
        }
        file.flush().await?;
        drop(file);

        let sha256 = hex::encode(hasher.finalize());

        // rename temp file into blob directory (atomic on same filesystem)
        let blob_path = blob::blob_absolute_path(&inner.data_dir, &sha256)?;
        if let Some(parent) = blob_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // if blob with same hash already exists, delete temp file only
        if blob_path.exists() {
            std::fs::remove_file(&tmp)?;
        } else {
            std::fs::rename(&tmp, &blob_path)
                .with_context(|| format!("finalising blob at {}", blob_path.display()))?;
        }

        let size_bytes = std::fs::metadata(&blob_path)?.len();

        let manifest = ModelManifest {
            id: Uuid::new_v4(),
            model_id: model_id.clone(),
            sha256: sha256.clone(),
            size_bytes,
            format: ModelFormat::Gguf,
            quantization,
            imported_at: Utc::now(),
            blob_path: blob_path.to_string_lossy().into_owned(),
            model_type,
        };

        inner
            .db
            .lock()
            .unwrap()
            .insert(&manifest)
            .context("inserting manifest")?;

        info!(
            "Pulled model {} ({} bytes, sha256:{})",
            model_id,
            size_bytes,
            &sha256[..12]
        );
        Ok(manifest)
    }
}
