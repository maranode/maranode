//! ingest local documents, compute embeddings, and search by similarity

pub mod crypt;
pub mod chunk;
pub mod config;
pub mod embed;
pub mod engine;
pub mod extract;
pub mod math;
pub mod store;

pub use config::RagConfig;
pub use embed::Embedder;
pub use engine::{IngestStats, RagEngine, RetrievedChunk, SummarizeFn};
pub use extract::{DocumentContent, DocumentMeta, Page};
pub use store::{sha256_hex, CollectionInfo, DocumentInfo, VectorStore};
