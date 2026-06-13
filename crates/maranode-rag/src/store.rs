use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use sha2::{Digest, Sha256};

use crate::crypt::{maybe_decrypt, maybe_encrypt};
use crate::math::{blob_to_vec, dot, vec_to_blob};

pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[derive(Debug, Clone)]
pub struct CollectionInfo {
    pub name: String,
    pub embedding_model: String,
    pub dim: usize,
    pub documents: usize,
    pub chunks: usize,
}

#[derive(Debug, Clone)]
pub struct DocumentInfo {
    pub id: String,
    pub collection: String,
    pub source: String,
    pub sha256: String,
    pub chunks: usize,
    pub ingested_at: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub page_count: u32,
    pub summary: Option<String>,
}

pub(crate) struct ScoredChunk {
    pub chunk_id: String,
    pub doc_id: String,
    pub doc_sha256: String,
    pub content_hash: String,
    pub source: String,
    pub ordinal: usize,
    pub text: String,
    pub score: f32,
    pub page_number: u32,
    pub section: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
}

pub struct ChunkRow {
    pub text: String,
    pub embedding: Vec<f32>,
    pub page_number: u32,
    pub section: Option<String>,
    /// SHA-256 of the chunk text (computed at ingest; checked on tamper-detect)
    pub content_hash: String,
}

#[derive(Clone)]
pub struct VectorStore {
    conn: Arc<Mutex<Connection>>,
    dek: Option<[u8; 32]>,
}

impl VectorStore {
    /// lock SQLite connection. If mutex was poisoned by panic, take inner value anyway
    /// rusqlite Connection is still valid after panic (open transactions roll back on drop)
    /// So RAG can continue after one failed request instead of panic on every next call
    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("rag.db");
        let conn = Connection::open(&path)
            .with_context(|| format!("opening rag.db at {}", path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            dek: None,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn with_dek(mut self, dek: [u8; 32]) -> Self {
        self.dek = Some(dek);
        self
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            dek: None,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.lock_conn();
        conn.execute_batch(include_str!("sql/migrate.sql"))
            .context("running RAG schema migration")?;
        // Add new columns when database was created before this schema version
        for sql in [
            "ALTER TABLE rag_documents ADD COLUMN title TEXT",
            "ALTER TABLE rag_documents ADD COLUMN author TEXT",
            "ALTER TABLE rag_documents ADD COLUMN page_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE rag_documents ADD COLUMN summary TEXT",
            "ALTER TABLE rag_chunks ADD COLUMN page_number INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE rag_chunks ADD COLUMN section TEXT",
            "ALTER TABLE rag_chunks ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
        ] {
            let _ = conn.execute_batch(sql); // ignore errors (column already exists)
        }
        Ok(())
    }

    pub fn ensure_collection(&self, name: &str, embedding_model: &str, dim: usize) -> Result<()> {
        let conn = self.lock_conn();
        let existing: Option<(String, i64)> = conn
            .query_row(
                "SELECT embedding_model, dim FROM rag_collections WHERE name = ?1",
                params![name],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .ok();

        match existing {
            Some((model, existing_dim)) => {
                if model != embedding_model {
                    anyhow::bail!(
                        "collection '{name}' was created with embedding model '{model}', \
                         cannot mix with '{embedding_model}'"
                    );
                }
                if existing_dim as usize != dim {
                    anyhow::bail!("collection '{name}' has dimension {existing_dim}, got {dim}");
                }
                Ok(())
            }
            None => {
                conn.execute(
                    "INSERT INTO rag_collections (name, embedding_model, dim, created_at) VALUES (?1,?2,?3,?4)",
                    params![name, embedding_model, dim as i64, Utc::now().to_rfc3339()],
                )?;
                Ok(())
            }
        }
    }

    pub fn insert_document(
        &self,
        collection: &str,
        source: &str,
        sha256: &str,
        chunks: &[ChunkRow],
        title: Option<&str>,
        author: Option<&str>,
        page_count: u32,
    ) -> Result<String> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        let doc_id = Uuid::new_v4().to_string();

        tx.execute(
            "INSERT INTO rag_documents (id, collection, source, sha256, ingested_at, title, author, page_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![doc_id, collection, source, sha256, Utc::now().to_rfc3339(), title, author, page_count as i64],
        )?;

        for (ordinal, chunk) in chunks.iter().enumerate() {
            let encrypted_text = maybe_encrypt(self.dek.as_ref(), &chunk.text)?;
            tx.execute(
                "INSERT INTO rag_chunks (id, document_id, collection, ordinal, text, embedding, page_number, section, content_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    Uuid::new_v4().to_string(),
                    doc_id,
                    collection,
                    ordinal as i64,
                    encrypted_text,
                    vec_to_blob(&chunk.embedding),
                    chunk.page_number as i64,
                    chunk.section,
                    chunk.content_hash,
                ],
            )?;
        }

        tx.commit()?;
        Ok(doc_id)
    }

    /// re-hash the stored chunk text and compare to the stored content_hash.
    /// returns (chunk_id, stored_hash, computed_hash, matches)
    pub fn verify_chunk_hash(&self, chunk_id: &str) -> Result<(String, String, bool)> {
        let conn = self.lock_conn();
        let dek_ref = self.dek.as_ref();
        let (raw_text, stored_hash): (String, String) = conn.query_row(
            "SELECT text, content_hash FROM rag_chunks WHERE id = ?1",
            params![chunk_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let text = maybe_decrypt(dek_ref, &raw_text).unwrap_or(raw_text);
        let computed = sha256_hex(text.as_bytes());
        let matches = computed == stored_hash;
        Ok((stored_hash, computed, matches))
    }

    pub fn set_summary(&self, document_id: &str, summary: &str) -> Result<()> {
        let stored = maybe_encrypt(self.dek.as_ref(), summary)?;
        let conn = self.lock_conn();
        conn.execute(
            "UPDATE rag_documents SET summary = ?1 WHERE id = ?2",
            params![stored, document_id],
        )?;
        Ok(())
    }

    pub(crate) fn search(
        &self,
        collection: &str,
        query: &[f32],
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<ScoredChunk>> {
        let conn = self.lock_conn();
        let dek_ref = self.dek.as_ref();
        let mut stmt = conn.prepare(
            "SELECT c.id, d.id, d.sha256, c.content_hash, d.source, c.ordinal, c.text,
                    c.embedding, c.page_number, c.section, d.title, d.author
             FROM rag_chunks c
             JOIN rag_documents d ON d.id = c.document_id
             WHERE c.collection = ?1",
        )?;

        let rows = stmt.query_map(params![collection], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        })?;

        let mut scored: Vec<ScoredChunk> = Vec::new();
        for row in rows {
            let (chunk_id, doc_id, doc_sha256, content_hash, source, ordinal, raw_text,
                 blob, page_number, section, title, author) = row?;
            let text = maybe_decrypt(dek_ref, &raw_text)
                .unwrap_or(raw_text);
            let emb = blob_to_vec(&blob)?;
            if emb.len() != query.len() {
                continue;
            }
            let score = dot(query, &emb);
            if score >= min_score {
                scored.push(ScoredChunk {
                    chunk_id,
                    doc_id,
                    doc_sha256,
                    content_hash,
                    source,
                    ordinal: ordinal as usize,
                    text,
                    score,
                    page_number: page_number as u32,
                    section,
                    title,
                    author,
                });
            }
        }

        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        scored.truncate(top_k);
        Ok(scored)
    }

    pub fn list_collections(&self) -> Result<Vec<CollectionInfo>> {
        let conn = self.lock_conn();
        let mut stmt =
            conn.prepare("SELECT name, embedding_model, dim FROM rag_collections ORDER BY name")?;
        let base = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut out = Vec::with_capacity(base.len());
        for (name, embedding_model, dim) in base {
            let documents: i64 = conn.query_row(
                "SELECT COUNT(*) FROM rag_documents WHERE collection = ?1",
                params![name],
                |r| r.get(0),
            )?;
            let chunks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM rag_chunks WHERE collection = ?1",
                params![name],
                |r| r.get(0),
            )?;
            out.push(CollectionInfo {
                name,
                embedding_model,
                dim: dim as usize,
                documents: documents as usize,
                chunks: chunks as usize,
            });
        }
        Ok(out)
    }

    pub fn list_documents(&self, collection: &str) -> Result<Vec<DocumentInfo>> {
        let dek_ref = self.dek.as_ref();
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.collection, d.source, d.sha256, d.ingested_at,
                    COUNT(c.id) as chunk_count, d.title, d.author, d.page_count, d.summary
             FROM rag_documents d
             LEFT JOIN rag_chunks c ON c.document_id = d.id
             WHERE d.collection = ?1
             GROUP BY d.id
             ORDER BY d.ingested_at DESC",
        )?;
        let rows = stmt.query_map(params![collection], |row| {
            Ok(DocumentInfo {
                id: row.get(0)?,
                collection: row.get(1)?,
                source: row.get(2)?,
                sha256: row.get(3)?,
                ingested_at: row.get(4)?,
                chunks: row.get::<_, i64>(5)? as usize,
                title: row.get(6)?,
                author: row.get(7)?,
                page_count: row.get::<_, i64>(8)? as u32,
                summary: row.get::<_, Option<String>>(9)?.map(|s| {
                    maybe_decrypt(dek_ref, &s).unwrap_or(s)
                }),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_document(&self, id: &str) -> Result<Option<DocumentInfo>> {
        let dek_ref = self.dek.as_ref();
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.collection, d.source, d.sha256, d.ingested_at,
                    COUNT(c.id) as chunk_count, d.title, d.author, d.page_count, d.summary
             FROM rag_documents d
             LEFT JOIN rag_chunks c ON c.document_id = d.id
             WHERE d.id = ?1
             GROUP BY d.id",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(DocumentInfo {
                id: row.get(0)?,
                collection: row.get(1)?,
                source: row.get(2)?,
                sha256: row.get(3)?,
                ingested_at: row.get(4)?,
                chunks: row.get::<_, i64>(5)? as usize,
                title: row.get(6)?,
                author: row.get(7)?,
                page_count: row.get::<_, i64>(8)? as u32,
                summary: row.get::<_, Option<String>>(9)?.map(|s| {
                    maybe_decrypt(dek_ref, &s).unwrap_or(s)
                }),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_document_text(&self, document_id: &str) -> Result<Option<String>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT text FROM rag_chunks WHERE document_id = ?1 ORDER BY ordinal ASC",
        )?;
        let raw_rows: Vec<String> = stmt
            .query_map(params![document_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if raw_rows.is_empty() {
            return Ok(None);
        }
        let rows = raw_rows
            .iter()
            .map(|t| maybe_decrypt(self.dek.as_ref(), t))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(rows.join("\n\n")))
    }

    pub fn delete_document(&self, document_id: &str) -> Result<bool> {
        let conn = self.lock_conn();
        let n = conn.execute(
            "DELETE FROM rag_documents WHERE id = ?1",
            params![document_id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_collection(&self, name: &str) -> Result<bool> {
        let conn = self.lock_conn();
        let n = conn.execute("DELETE FROM rag_collections WHERE name = ?1", params![name])?;
        Ok(n > 0)
    }

    pub fn chunk_count(&self, collection: &str) -> Result<usize> {
        let conn = self.lock_conn();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rag_chunks WHERE collection = ?1",
            params![collection],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::normalize;

    fn norm(v: Vec<f32>) -> Vec<f32> {
        let mut v = v;
        normalize(&mut v);
        v
    }

    fn chunk(text: &str, emb: Vec<f32>) -> ChunkRow {
        ChunkRow {
            text: text.into(),
            embedding: norm(emb),
            page_number: 1,
            section: None,
        }
    }

    #[test]
    fn ensure_collection_rejects_model_mismatch() {
        let s = VectorStore::open_in_memory().unwrap();
        s.ensure_collection("c", "model-a", 3).unwrap();
        assert!(s.ensure_collection("c", "model-b", 3).is_err());
        s.ensure_collection("c", "model-a", 3).unwrap();
    }

    #[test]
    fn search_ranks_by_similarity() {
        let s = VectorStore::open_in_memory().unwrap();
        s.ensure_collection("c", "m", 3).unwrap();
        s.insert_document(
            "c",
            "doc.txt",
            "sha",
            &[
                chunk("east", vec![1.0, 0.0, 0.0]),
                chunk("north", vec![0.0, 1.0, 0.0]),
            ],
            None,
            None,
            1,
        )
        .unwrap();

        let q = norm(vec![0.9, 0.1, 0.0]);
        let hits = s.search("c", &q, 2, 0.0).unwrap();
        assert_eq!(hits[0].text, "east");
    }

    #[test]
    fn delete_collection_cascades() {
        let s = VectorStore::open_in_memory().unwrap();
        s.ensure_collection("c", "m", 2).unwrap();
        s.insert_document(
            "c",
            "d",
            "sha",
            &[chunk("t", vec![1.0, 0.0])],
            None,
            None,
            1,
        )
        .unwrap();
        assert_eq!(s.chunk_count("c").unwrap(), 1);
        assert!(s.delete_collection("c").unwrap());
        assert_eq!(s.chunk_count("c").unwrap(), 0);
    }
}
