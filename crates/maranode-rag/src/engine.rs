//! RAG engine: ingest documents, search chunks, summarize, add context to prompts

use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::chunk::chunk_document;
use crate::config::RagConfig;
use crate::embed::Embedder;
use crate::extract::{extract, DocumentContent};
use crate::math::normalize;
use crate::store::{ChunkRow, CollectionInfo, DocumentInfo, VectorStore};

#[derive(Debug, Clone)]
pub struct IngestStats {
    pub document_id: String,
    pub collection: String,
    pub chunks: usize,
    pub pages: u32,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub source: String,
    pub ordinal: usize,
    pub text: String,
    pub score: f32,
    pub page_number: u32,
    pub section: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone)]
pub struct RagEngine {
    store: VectorStore,
    embedder: Arc<dyn Embedder>,
    config: Arc<RwLock<RagConfig>>,
}

impl RagEngine {
    pub fn open(data_dir: &Path, embedder: Arc<dyn Embedder>, config: RagConfig) -> Result<Self> {
        Ok(Self {
            store: VectorStore::open(data_dir)?,
            embedder,
            config: Arc::new(RwLock::new(config)),
        })
    }

    pub fn open_with_dek(
        data_dir: &Path,
        embedder: Arc<dyn Embedder>,
        config: RagConfig,
        dek: [u8; 32],
    ) -> Result<Self> {
        Ok(Self {
            store: VectorStore::open(data_dir)?.with_dek(dek),
            embedder,
            config: Arc::new(RwLock::new(config)),
        })
    }

    pub fn in_memory(embedder: Arc<dyn Embedder>, config: RagConfig) -> Result<Self> {
        Ok(Self {
            store: VectorStore::open_in_memory()?,
            embedder,
            config: Arc::new(RwLock::new(config)),
        })
    }

    pub fn config(&self) -> RagConfig {
        self.config
            .read()
            .expect("rag config lock poisoned")
            .clone()
    }

    pub fn default_collection(&self) -> String {
        self.config
            .read()
            .expect("rag config lock poisoned")
            .default_collection
            .clone()
    }

    /// Copy RAG settings from config that can change at runtime
    pub fn apply_runtime_config(&self, cfg: &RagConfig) {
        let mut c = self.config.write().expect("rag config lock poisoned");
        c.default_collection = cfg.default_collection.clone();
        c.chunk_size = cfg.chunk_size;
        c.chunk_overlap = cfg.chunk_overlap;
        c.top_k = cfg.top_k;
        c.min_score = cfg.min_score;
        c.max_context_chars = cfg.max_context_chars;
    }

    pub async fn ingest_bytes(
        &self,
        collection: &str,
        source: &str,
        bytes: &[u8],
        filename: &str,
        summarizer: Option<&dyn SummarizeFn>,
    ) -> Result<IngestStats> {
        let doc = extract(bytes, filename)?;
        self.ingest_document(collection, source, doc, summarizer)
            .await
    }

    pub async fn ingest(&self, collection: &str, source: &str, text: &str) -> Result<IngestStats> {
        let doc = DocumentContent::from_plain(text.to_string());
        self.ingest_document(collection, source, doc, None).await
    }

    async fn ingest_document(
        &self,
        collection: &str,
        source: &str,
        doc: DocumentContent,
        summarizer: Option<&dyn SummarizeFn>,
    ) -> Result<IngestStats> {
        let (chunk_size, chunk_overlap) = {
            let cfg = self.config.read().expect("rag config lock poisoned");
            (cfg.chunk_size, cfg.chunk_overlap)
        };
        let rich_chunks = chunk_document(&doc, chunk_size, chunk_overlap);
        if rich_chunks.is_empty() {
            anyhow::bail!("document '{source}' produced no text to index");
        }

        let texts: Vec<String> = rich_chunks.iter().map(|c| c.text.clone()).collect();
        let mut embeddings = self.embedder.embed(&texts).await?;
        if embeddings.len() != rich_chunks.len() {
            anyhow::bail!(
                "embedder returned {} vectors for {} chunks",
                embeddings.len(),
                rich_chunks.len()
            );
        }
        let dim = embeddings.first().map(|v| v.len()).unwrap_or(0);
        if dim == 0 {
            anyhow::bail!("embedder returned empty vectors");
        }

        for v in &mut embeddings {
            normalize(v);
        }

        self.store
            .ensure_collection(collection, &self.embedder.model_label(), dim)?;

        let mut sha = Sha256::new();
        sha.update(doc.full_text.as_bytes());
        let sha256 = hex::encode(sha.finalize());

        let chunk_rows: Vec<ChunkRow> = rich_chunks
            .into_iter()
            .zip(embeddings.into_iter())
            .map(|(rc, emb)| ChunkRow {
                text: rc.text,
                embedding: emb,
                page_number: rc.page_number,
                section: rc.section,
            })
            .collect();

        let n = chunk_rows.len();
        let document_id = self.store.insert_document(
            collection,
            source,
            &sha256,
            &chunk_rows,
            doc.meta.title.as_deref(),
            doc.meta.author.as_deref(),
            doc.meta.page_count,
        )?;

        let summary = if let Some(summarize) = summarizer {
            let preview: String = doc.full_text.chars().take(8000).collect();
            match summarize.summarize(&preview).await {
                Ok(s) => {
                    let _ = self.store.set_summary(&document_id, &s);
                    Some(s)
                }
                Err(e) => {
                    debug!("RAG summarization failed for '{source}': {}", e);
                    None
                }
            }
        } else {
            None
        };

        info!(
            "RAG ingested '{source}' into '{collection}' ({n} chunks, {} page(s))",
            doc.meta.page_count
        );

        Ok(IngestStats {
            document_id,
            collection: collection.to_string(),
            chunks: n,
            pages: doc.meta.page_count,
            summary,
        })
    }

    pub async fn retrieve(
        &self,
        collection: &str,
        query: &str,
        top_k: Option<usize>,
    ) -> Result<Vec<RetrievedChunk>> {
        let (top_k, min_score) = {
            let cfg = self.config.read().expect("rag config lock poisoned");
            (top_k.unwrap_or(cfg.top_k).max(1), cfg.min_score)
        };
        let mut q = self.embedder.embed_one(query).await?;
        normalize(&mut q);
        let hits = self.store.search(collection, &q, top_k, min_score)?;
        debug!("RAG retrieved {} chunks from '{collection}'", hits.len());
        Ok(hits.into_iter().map(scored_to_retrieved).collect())
    }

    pub async fn retrieve_all_collections(
        &self,
        query: &str,
        top_k: Option<usize>,
    ) -> Result<Vec<RetrievedChunk>> {
        let (top_k, min_score) = {
            let cfg = self.config.read().expect("rag config lock poisoned");
            (top_k.unwrap_or(cfg.top_k).max(1), cfg.min_score)
        };
        let mut q = self.embedder.embed_one(query).await?;
        normalize(&mut q);

        let collections = self.store.list_collections()?;
        if collections.is_empty() {
            return Ok(vec![]);
        }

        let mut all: Vec<RetrievedChunk> = Vec::new();
        for col in &collections {
            match self.store.search(&col.name, &q, top_k, min_score) {
                Ok(hits) => all.extend(hits.into_iter().map(scored_to_retrieved)),
                Err(e) => debug!("RAG: skipping collection '{}': {}", col.name, e),
            }
        }

        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all.truncate(top_k);
        debug!(
            "RAG retrieved {} chunks across {} collections",
            all.len(),
            collections.len()
        );
        Ok(all)
    }

    pub fn list_collections(&self) -> Result<Vec<CollectionInfo>> {
        self.store.list_collections()
    }
    pub fn delete_collection(&self, name: &str) -> Result<bool> {
        self.store.delete_collection(name)
    }
    pub fn list_documents(&self, collection: &str) -> Result<Vec<DocumentInfo>> {
        self.store.list_documents(collection)
    }
    pub fn get_document(&self, id: &str) -> Result<Option<DocumentInfo>> {
        self.store.get_document(id)
    }
    pub fn get_document_text(&self, id: &str) -> Result<Option<String>> {
        self.store.get_document_text(id)
    }
    pub fn set_summary(&self, document_id: &str, summary: &str) -> Result<()> {
        self.store.set_summary(document_id, summary)
    }
    pub fn delete_document(&self, document_id: &str) -> Result<bool> {
        self.store.delete_document(document_id)
    }

    pub fn build_context_prompt(&self, chunks: &[RetrievedChunk]) -> Option<String> {
        if chunks.is_empty() {
            return None;
        }

        let max_context_chars = self
            .config
            .read()
            .expect("rag config lock poisoned")
            .max_context_chars;

        let mut context = String::new();
        let mut used = 0usize;
        for (i, c) in chunks.iter().enumerate() {
            let loc = match (&c.section, c.page_number) {
                (Some(sec), p) if p > 0 => format!("{}, p.{}", sec, p),
                (None, p) if p > 0 => format!("p.{}", p),
                (Some(sec), _) => sec.clone(),
                _ => String::new(),
            };
            let display_name = c.title.as_deref()
                .or_else(|| source_display_name(&c.source))
                .unwrap_or(&c.source);
            let cite = if loc.is_empty() {
                format!("[{}] \"{}\"", i + 1, display_name)
            } else {
                format!("[{}] \"{}\", {}", i + 1, display_name, loc)
            };
            let block = format!("{}\n{}\n\n", cite, c.text);
            if used + block.len() > max_context_chars && !context.is_empty() {
                break;
            }
            used += block.len();
            context.push_str(&block);
        }

        Some(format!(
            "The following CONTEXT comes from the user's documents. \
             Prefer information from the CONTEXT when it is relevant to the question. \
             When you use information from the CONTEXT, cite the source number, e.g. [1] or [1, p.5]. \
             You may also draw on your general knowledge to give a complete and helpful answer — \
             just make clear which parts come from the documents and which from general knowledge. \
             If the CONTEXT contains nothing relevant to the question, answer normally without citing.\n\n\
             CONTEXT:\n{}",
            context.trim_end()
        ))
    }
}

fn scored_to_retrieved(h: crate::store::ScoredChunk) -> RetrievedChunk {
    RetrievedChunk {
        source: h.source,
        ordinal: h.ordinal,
        text: h.text,
        score: h.score,
        page_number: h.page_number,
        section: h.section,
        title: h.title,
        author: h.author,
    }
}

/// Get display name from file path or URL string.
/// Returns filename without extension, or None if we cannot parse it.
fn source_display_name(source: &str) -> Option<&str> {
    // Treat source as filesystem path
    let path = std::path::Path::new(source);
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if !stem.is_empty() && stem != source {
            return Some(stem);
        }
    }
    None
}

#[async_trait::async_trait]
pub trait SummarizeFn: Send + Sync {
    async fn summarize(&self, text: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEmbedder;
    const VOCAB: &[&str] = &["patient", "blood", "pressure", "contract", "liability"];

    #[async_trait::async_trait]
    impl Embedder for FakeEmbedder {
        fn model_label(&self) -> String {
            "fake".into()
        }
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let lower = t.to_lowercase();
                    let mut v: Vec<f32> = VOCAB
                        .iter()
                        .map(|w| lower.matches(w).count() as f32)
                        .collect();
                    v.push(1.0);
                    v
                })
                .collect())
        }
    }

    fn engine() -> RagEngine {
        let cfg = RagConfig {
            enabled: true,
            chunk_size: 200,
            chunk_overlap: 20,
            ..Default::default()
        };
        RagEngine::in_memory(Arc::new(FakeEmbedder), cfg).unwrap()
    }

    #[tokio::test]
    async fn ingest_then_retrieve() {
        let e = engine();
        e.ingest(
            "default",
            "report.txt",
            "The patient blood pressure was high. The contract liability clause is broad.",
        )
        .await
        .unwrap();
        let hits = e
            .retrieve("default", "what was the patient blood pressure", Some(3))
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].text.to_lowercase().contains("patient"));
    }

    #[tokio::test]
    async fn citation_includes_page() {
        let e = engine();
        e.ingest("default", "report.txt", "The blood pressure was 120/80.")
            .await
            .unwrap();
        let hits = e
            .retrieve("default", "blood pressure", Some(1))
            .await
            .unwrap();
        let prompt = e.build_context_prompt(&hits).unwrap();
        assert!(prompt.contains("p.1") || prompt.contains("report.txt"));
    }
}
