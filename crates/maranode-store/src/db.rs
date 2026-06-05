use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use maranode_common::models::{ModelFormat, ModelId, ModelManifest, ModelType};
use chrono::Utc;
use uuid::Uuid;

pub struct ManifestDb {
    conn: Connection,
}

impl ManifestDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening models.db at {}", path.display()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("sql/migrate.sql"))
            .context("running schema migration")?;
        // ignore error if column already exists on older database
        let _ = self
            .conn
            .execute_batch(include_str!("sql/migrate_add_model_type.sql"));
        Ok(())
    }

    pub fn insert(&self, m: &ModelManifest) -> Result<()> {
        self.conn.execute(
            "INSERT INTO models
             (id, name, tag, sha256, size_bytes, format, quantization, blob_path, imported_at, model_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                m.id.to_string(),
                m.model_id.name,
                m.model_id.tag,
                m.sha256,
                m.size_bytes as i64,
                "gguf",
                m.quantization,
                m.blob_path,
                m.imported_at.to_rfc3339(),
                match m.model_type {
                    ModelType::Llm       => "llm",
                    ModelType::Embedding => "embedding",
                },
            ],
        )
        .context("inserting model manifest")?;
        Ok(())
    }

    pub fn get(&self, model_id: &ModelId) -> Result<Option<ModelManifest>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, tag, sha256, size_bytes, quantization, blob_path, imported_at, model_type
             FROM models WHERE name = ?1 AND tag = ?2",
        )?;
        let mut rows = stmt.query(params![model_id.name, model_id.tag])?;

        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let imported_str: String = row.get(7)?;
            let model_type_str: String = row.get::<_, Option<String>>(8)?.unwrap_or_default();
            Ok(Some(ModelManifest {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                model_id: ModelId::new(row.get::<_, String>(1)?, row.get::<_, String>(2)?),
                sha256: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                format: ModelFormat::Gguf,
                quantization: row.get(5)?,
                blob_path: row.get(6)?,
                imported_at: imported_str.parse().unwrap_or_else(|_| Utc::now()),
                model_type: if model_type_str == "embedding" {
                    ModelType::Embedding
                } else {
                    ModelType::Llm
                },
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list(&self) -> Result<Vec<ModelManifest>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, tag, sha256, size_bytes, quantization, blob_path, imported_at, model_type
             FROM models ORDER BY imported_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;

        let mut manifests = Vec::new();
        for row in rows {
            let (
                id,
                name,
                tag,
                sha256,
                size_bytes,
                quantization,
                blob_path,
                imported_at,
                model_type_str,
            ) = row?;
            let model_type = if model_type_str.as_deref() == Some("embedding") {
                ModelType::Embedding
            } else {
                ModelType::Llm
            };
            manifests.push(ModelManifest {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                model_id: ModelId::new(name, tag),
                sha256,
                size_bytes: size_bytes as u64,
                format: ModelFormat::Gguf,
                quantization,
                blob_path,
                imported_at: imported_at.parse().unwrap_or_else(|_| Utc::now()),
                model_type,
            });
        }
        Ok(manifests)
    }

    pub fn delete(&self, model_id: &ModelId) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM models WHERE name = ?1 AND tag = ?2",
            params![model_id.name, model_id.tag],
        )?;
        Ok(n > 0)
    }
}
