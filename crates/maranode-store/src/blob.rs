//! store GGUF files on disk by SHA-256 hash (same hash = same file path)

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use maranode_common::secure::is_sha256_hex;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn blobs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("blobs")
}

pub fn blob_path_checked(data_dir: &Path, sha256: &str) -> Result<PathBuf> {
    if !is_sha256_hex(sha256) {
        anyhow::bail!(
            "invalid blob digest '{}' (expected 64-char lowercase hex)",
            sha256
        );
    }
    Ok(blobs_dir(data_dir).join(format!("sha256-{}", sha256)))
}

pub fn import_blob<R: Read>(data_dir: &Path, mut reader: R) -> Result<(String, u64)> {
    std::fs::create_dir_all(blobs_dir(data_dir))?;

    let tmp_path = blobs_dir(data_dir).join(format!(".import-{}.tmp", Uuid::new_v4()));
    let _tmp_guard = TmpGuard(tmp_path.clone());
    let mut file = std::fs::File::create(&tmp_path).context("creating temporary blob file")?;

    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 64 * 1024]; // Read input in 64 KiB blocks

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        total += n as u64;
    }

    file.sync_all()?;
    drop(file);

    let sha256 = hex::encode(hasher.finalize());
    let dest = blob_path_checked(data_dir, &sha256)?;

    if dest.exists() {
        std::fs::remove_file(&tmp_path)?;
    } else {
        std::fs::rename(&tmp_path, &dest).context("atomically placing blob")?;
    }

    Ok((sha256, total))
}

struct TmpGuard(PathBuf);

impl Drop for TmpGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

pub fn verify_blob(data_dir: &Path, expected_sha256: &str) -> Result<()> {
    let path = blob_path_checked(data_dir, expected_sha256)?;
    let mut file =
        std::fs::File::open(&path).with_context(|| format!("opening blob {}", expected_sha256))?;

    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let actual = hex::encode(hasher.finalize());

    if actual != expected_sha256 {
        anyhow::bail!(
            "blob integrity check failed: expected {}, got {}",
            expected_sha256,
            actual
        );
    }
    Ok(())
}

/// full path to blob file. Checks that sha256 is valid 64-char hex.
pub fn blob_absolute_path(data_dir: &Path, sha256: &str) -> Result<PathBuf> {
    blob_path_checked(data_dir, sha256)
}
