//! load and write the audit HMAC key file

use std::path::Path;

use anyhow::{Context, Result};
use rand::RngCore;

const KEY_LEN: usize = 32;

/// load audit key from file. returns error if the file is missing.
///
/// use this for verify and export (not `load_or_generate`). if we generate a new key
/// here, `verify_log` can succeed with the wrong key and give a false forensic result.
pub fn load(path: &Path) -> Result<Vec<u8>> {
    if !path.exists() {
        anyhow::bail!(
            "audit key not found at {}: refusing to verify/export without it",
            path.display()
        );
    }
    check_key_permissions(path)?;
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading audit key from {}", path.display()))?;
    if bytes.len() != KEY_LEN {
        anyhow::bail!(
            "audit key at {} has unexpected length {} (expected {})",
            path.display(),
            bytes.len(),
            KEY_LEN
        );
    }
    Ok(bytes)
}

pub fn load_or_generate(path: &Path) -> Result<Vec<u8>> {
    if path.exists() {
        load(path)
    } else {
        let mut key = vec![0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);
        write_key(path, &key)?;
        Ok(key)
    }
}

#[cfg(unix)]
fn write_key(path: &Path, key: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(key)
        })
        .with_context(|| format!("writing audit key to {}", path.display()))
}

#[cfg(not(unix))]
fn write_key(path: &Path, key: &[u8]) -> Result<()> {
    std::fs::write(path, key).with_context(|| format!("writing audit key to {}", path.display()))
}

/// fail if audit.key is readable by group or other users.
/// if key is readable by others, attacker can fake log lines and break HMAC chain check.
#[cfg(unix)]
fn check_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta =
        std::fs::metadata(path).with_context(|| format!("stat audit key {}", path.display()))?;
    let mode = meta.permissions().mode();
    if mode & 0o077 != 0 {
        anyhow::bail!(
            "audit key at {} is accessible by group/other (mode {:o}); \
             tighten to 0600 (chmod 600 {})",
            path.display(),
            mode & 0o7777,
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
