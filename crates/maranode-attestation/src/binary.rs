use std::path::PathBuf;

use anyhow::Result;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BinaryMeasurement {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

pub fn measure_self() -> Result<BinaryMeasurement> {
    let path = std::env::current_exe()?;
    measure_path(&path)
}

pub fn measure_path(path: &std::path::Path) -> Result<BinaryMeasurement> {
    let bytes = std::fs::read(path)?;
    let hash = hex::encode(Sha256::digest(&bytes));
    Ok(BinaryMeasurement {
        path: path.display().to_string(),
        sha256: hash,
        size_bytes: bytes.len() as u64,
    })
}

/// compute SHA-256 for extra file paths (config, keys, etc.)
pub fn measure_paths(paths: &[PathBuf]) -> Vec<(String, String)> {
    paths
        .iter()
        .filter_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            let hash = hex::encode(Sha256::digest(&bytes));
            Some((p.display().to_string(), hash))
        })
        .collect()
}
