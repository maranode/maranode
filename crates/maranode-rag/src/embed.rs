//! trait for text embedding backends

use anyhow::Result;

#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    fn model_label(&self) -> String;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut out = self.embed(&[text.to_string()]).await?;
        out.pop()
            .ok_or_else(|| anyhow::anyhow!("embedder returned no vector"))
    }
}
