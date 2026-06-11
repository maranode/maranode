//! Master key (KEK) management — wraps workspace DEKs so they can be
//! rotated or TPM-sealed later without touching the encrypted data.

use std::path::Path;

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    aead::rand_core::RngCore,
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};

const PREFIX: &str = "wrapped:";

/// load the master key from file, or generate and persist a new one.
pub fn load_or_create(path: &Path) -> Result<[u8; 32]> {
    if path.exists() {
        let hex = std::fs::read_to_string(path)
            .with_context(|| format!("reading master key at {}", path.display()))?;
        let bytes = hex::decode(hex.trim())
            .context("decoding master key hex")?;
        bytes.try_into().map_err(|_| anyhow::anyhow!("master key must be 32 bytes"))
    } else {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let hex = hex::encode(key);
        write_key_file(path, &hex)?;
        Ok(key)
    }
}

/// key path relative to data_dir
pub fn default_kek_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("master.key")
}

/// wrap a plaintext DEK hex string under the KEK. result has "wrapped:" prefix.
pub fn wrap_dek(kek: &[u8; 32], dek_hex: &str) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(kek));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, dek_hex.as_bytes())
        .map_err(|e| anyhow::anyhow!("kek wrap: {e}"))?;
    let mut blob = nonce_bytes.to_vec();
    blob.extend(ct);
    Ok(format!("{}{}", PREFIX, hex::encode(blob)))
}

/// unwrap a wrapped DEK back to its plaintext hex.
pub fn unwrap_dek(kek: &[u8; 32], wrapped: &str) -> Result<String> {
    let hex_part = wrapped
        .strip_prefix(PREFIX)
        .context("value does not have wrapped: prefix")?;
    let blob = hex::decode(hex_part).context("decoding wrapped dek hex")?;
    if blob.len() < 12 {
        anyhow::bail!("wrapped dek too short");
    }
    let (nonce_bytes, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(kek));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ct)
        .map_err(|_| anyhow::anyhow!("kek unwrap failed — wrong key or corrupted data"))?;
    String::from_utf8(plain).context("unwrapped dek is not utf-8")
}

/// true if the stored value looks like a wrapped DEK (as opposed to legacy plaintext)
pub fn is_wrapped(value: &str) -> bool {
    value.starts_with(PREFIX)
}

/// rotate: re-wrap every workspace's stored DEK from old_kek to new_kek.
/// returns the number of DEKs rotated.
pub fn rotate_all(
    conn: &rusqlite::Connection,
    old_kek: &[u8; 32],
    new_kek: &[u8; 32],
) -> Result<usize> {
    let slugs: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT slug, dek FROM workspaces WHERE dek IS NOT NULL")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let mut rotated = 0;
    for (slug, stored) in &slugs {
        let dek_hex = if is_wrapped(stored) {
            unwrap_dek(old_kek, stored)?
        } else {
            stored.clone()
        };
        let new_wrapped = wrap_dek(new_kek, &dek_hex)?;
        conn.execute(
            "UPDATE workspaces SET dek = ?1 WHERE slug = ?2",
            rusqlite::params![new_wrapped, slug],
        )?;
        rotated += 1;
    }
    Ok(rotated)
}

#[cfg(unix)]
fn write_key_file(path: &Path, hex: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating master key at {}", path.display()))?;
    use std::io::Write;
    f.write_all(hex.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_key_file(path: &Path, hex: &str) -> Result<()> {
    std::fs::write(path, hex)
        .with_context(|| format!("creating master key at {}", path.display()))?;
    Ok(())
}
