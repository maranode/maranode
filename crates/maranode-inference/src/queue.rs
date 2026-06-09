use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{mpsc, Semaphore};

use maranode_common::models::InferenceDevice;

use crate::engine::{async_trait, InferenceEngine};
use crate::types::{InferenceRequest, InferenceResponse, Token};

pub struct InferenceQueue {
    inner: Arc<dyn InferenceEngine>,
    semaphore: Arc<Semaphore>,
    max_parallel: usize,
    waiting: AtomicUsize,
    max_waiting: AtomicUsize,
}

impl InferenceQueue {
    pub fn new(inner: Arc<dyn InferenceEngine>, max_waiting: usize, max_parallel: usize) -> Arc<Self> {
        let parallel = max_parallel.max(1);
        Arc::new(Self {
            inner,
            semaphore: Arc::new(Semaphore::new(parallel)),
            max_parallel: parallel,
            waiting: AtomicUsize::new(0),
            max_waiting: AtomicUsize::new(max_waiting),
        })
    }

    pub fn waiting(&self) -> usize {
        self.waiting.load(Relaxed)
    }

    pub fn max_waiting(&self) -> usize {
        self.max_waiting.load(Relaxed)
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    /// change max waiting requests at runtime. Value 0 means no limit.
    pub fn set_max_waiting(&self, max_waiting: usize) {
        self.max_waiting.store(max_waiting, Relaxed);
    }

    fn check_capacity(&self) -> Result<()> {
        let max = self.max_waiting.load(Relaxed);
        if max != 0 {
            let waiting = self.waiting.load(Relaxed);
            if waiting >= max {
                anyhow::bail!(
                    "server busy: {} requests already in flight (max {}). try again shortly.",
                    waiting,
                    max,
                );
            }
        }
        Ok(())
    }
}

struct WaitGuard<'a>(&'a AtomicUsize);

impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[async_trait]
impl InferenceEngine for InferenceQueue {
    async fn generate(&self, req: InferenceRequest) -> Result<InferenceResponse> {
        self.check_capacity()?;
        self.waiting.fetch_add(1, Ordering::Relaxed);
        let _wait = WaitGuard(&self.waiting);
        let _permit = self.semaphore.acquire().await.expect("semaphore closed");
        self.inner.generate(req).await
    }

    async fn generate_stream(&self, req: InferenceRequest, tx: mpsc::Sender<Result<Token>>) {
        if let Err(e) = self.check_capacity() {
            let _ = tx.send(Err(e)).await;
            return;
        }
        self.waiting.fetch_add(1, Ordering::Relaxed);
        let _wait = WaitGuard(&self.waiting);
        let _permit = match self.semaphore.acquire().await {
            Ok(p) => p,
            Err(_) => {
                let _ = tx.send(Err(anyhow::anyhow!("inference queue closed"))).await;
                return;
            }
        };
        self.inner.generate_stream(req, tx).await;
    }

    async fn embed(&self, model_path: &std::path::Path, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed(model_path, texts).await
    }

    fn device(&self) -> InferenceDevice {
        self.inner.device()
    }

    async fn load_model(&self, model_id: &str, path: &std::path::Path) -> Result<()> {
        self.inner.load_model(model_id, path).await
    }

    async fn unload_model(&self, model_id: &str) -> Result<()> {
        self.inner.unload_model(model_id).await
    }

    fn queue_depth(&self) -> usize {
        self.waiting()
    }

    fn max_queue_depth(&self) -> usize {
        self.max_waiting()
    }
}
