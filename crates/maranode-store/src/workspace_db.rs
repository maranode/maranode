use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use rand::RngCore;
use rusqlite::{params, Connection};
use uuid::Uuid;

use maranode_common::workspace::Workspace;

pub struct WorkspaceDb {
    conn: Connection,
}

impl WorkspaceDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening workspace db at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("setting pragmas")?;
        conn.execute_batch(include_str!("sql/migrate_workspaces.sql"))
            .context("running workspace migration")?;
        let _ = conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN net_namespace INTEGER NOT NULL DEFAULT 0;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN max_concurrent_requests INTEGER;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN max_models INTEGER;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN max_memory_bytes INTEGER;",
        );
        let _ = conn.execute_batch(include_str!("sql/migrate_add_dek.sql"));
        Ok(Self { conn })
    }

    pub fn list(&self) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, api_key_hash, model_allowlist, rate_limit_rpm, system_prompt,
                    created_at, net_namespace, max_concurrent_requests, max_models, max_memory_bytes,
                    dek
             FROM workspaces ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<u32>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<u32>>(9)?,
                row.get::<_, Option<u32>>(10)?,
                row.get::<_, Option<u64>>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, slug, name, api_key_hash, allowlist_str, rate_limit_rpm, system_prompt,
                 created_at, net_ns, max_concurrent_requests, max_models, max_memory_bytes, dek) = row?;
            out.push(Workspace {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                slug,
                name,
                api_key_hash,
                model_allowlist: parse_allowlist(&allowlist_str),
                rate_limit_rpm,
                system_prompt,
                created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                net_namespace: net_ns != 0,
                max_concurrent_requests,
                max_models,
                max_memory_bytes,
                dek,
            });
        }
        Ok(out)
    }

    pub fn get_by_slug(&self, slug: &str) -> Result<Option<Workspace>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, api_key_hash, model_allowlist, rate_limit_rpm, system_prompt,
                    created_at, net_namespace, max_concurrent_requests, max_models, max_memory_bytes,
                    dek
             FROM workspaces WHERE slug = ?1",
        )?;
        let mut rows = stmt.query(params![slug])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let allowlist_str: String = row.get(4)?;
            let created_at: String = row.get(7)?;
            Ok(Some(Workspace {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                slug: row.get(1)?,
                name: row.get(2)?,
                api_key_hash: row.get(3)?,
                model_allowlist: parse_allowlist(&allowlist_str),
                rate_limit_rpm: row.get(5)?,
                system_prompt: row.get(6)?,
                created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                net_namespace: row.get::<_, i64>(8).unwrap_or(0) != 0,
                max_concurrent_requests: row.get(9)?,
                max_models: row.get(10)?,
                max_memory_bytes: row.get(11)?,
                dek: row.get(12)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn create(&self, ws: &Workspace) -> Result<()> {
        let dek = ws.dek.clone().unwrap_or_else(generate_dek);
        self.conn.execute(
            "INSERT INTO workspaces
                (id, slug, name, api_key_hash, model_allowlist, rate_limit_rpm, system_prompt,
                 created_at, net_namespace, max_concurrent_requests, max_models, max_memory_bytes,
                 dek)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                ws.id.to_string(),
                ws.slug,
                ws.name,
                ws.api_key_hash,
                ws.model_allowlist.join(","),
                ws.rate_limit_rpm,
                ws.system_prompt,
                ws.created_at.to_rfc3339(),
                ws.net_namespace as i64,
                ws.max_concurrent_requests,
                ws.max_models,
                ws.max_memory_bytes,
                dek,
            ],
        ).context("inserting workspace")?;
        Ok(())
    }

    pub fn update(&self, ws: &Workspace) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE workspaces SET name=?1, api_key_hash=?2, model_allowlist=?3,
             rate_limit_rpm=?4, system_prompt=?5, net_namespace=?6,
             max_concurrent_requests=?7, max_models=?8, max_memory_bytes=?9
             WHERE slug=?10",
            params![
                ws.name,
                ws.api_key_hash,
                ws.model_allowlist.join(","),
                ws.rate_limit_rpm,
                ws.system_prompt,
                ws.net_namespace as i64,
                ws.max_concurrent_requests,
                ws.max_models,
                ws.max_memory_bytes,
                ws.slug,
            ],
        ).context("updating workspace")?;
        Ok(n > 0)
    }

    pub fn delete(&self, slug: &str) -> Result<bool> {
        if slug == "default" {
            anyhow::bail!("cannot delete the default workspace");
        }
        let n = self
            .conn
            .execute("DELETE FROM workspaces WHERE slug = ?1", params![slug])
            .context("deleting workspace")?;
        Ok(n > 0)
    }

    /// returns the raw 32-byte DEK for the workspace, or None if it was destroyed.
    pub fn get_dek_bytes(&self, slug: &str) -> Result<Option<[u8; 32]>> {
        let hex: Option<String> = self.conn
            .query_row(
                "SELECT dek FROM workspaces WHERE slug = ?1",
                params![slug],
                |row| row.get(0),
            )
            .optional()
            .context("reading dek")?
            .flatten();

        match hex {
            None => Ok(None),
            Some(h) => {
                let bytes = hex::decode(&h).context("decoding dek hex")?;
                let arr: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("dek must be exactly 32 bytes"))?;
                Ok(Some(arr))
            }
        }
    }

    /// set the DEK column to NULL, making all encrypted data permanently unreadable.
    /// this is the crypto-shredding step; it cannot be undone.
    pub fn destroy_dek(&self, slug: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE workspaces SET dek = NULL WHERE slug = ?1",
            params![slug],
        ).context("destroying dek")?;
        Ok(n > 0)
    }
}

/// generate a fresh random 32-byte DEK and return it hex-encoded.
pub fn generate_dek() -> String {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    hex::encode(key)
}

fn parse_allowlist(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}
