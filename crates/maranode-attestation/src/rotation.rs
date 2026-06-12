use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::rand_core::RngCore;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::seal::{self, pbkdf2_hmac_sha256, SealBackend, SealMeta};

const BUNDLE_MAGIC: &[u8; 4] = b"MRNB";
const BUNDLE_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryBundle {
    pub version: u8,
    pub created_at: chrono::DateTime<Utc>,
    pub purposes: Vec<RecoveryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryEntry {
    pub purpose: String,
    pub meta: SealMeta,
    pub key_bytes_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RotationRecord {
    pub purpose: String,
    pub rotated_at: chrono::DateTime<Utc>,
    pub reason: String,
    pub old_backend: SealBackend,
    pub new_backend: SealBackend,
    pub old_pcr_list: Option<String>,
    pub new_pcr_list: Option<String>,
}

pub fn export_recovery_bundle(
    purposes: &[&str],
    data_dir: &Path,
    passphrase: &str,
) -> Result<Vec<u8>> {
    let mut entries = Vec::new();

    for purpose in purposes {
        if !seal::is_sealed(purpose, data_dir) {
            bail!("purpose '{}' is not currently sealed", purpose);
        }

        let meta = seal::seal_status(purpose, data_dir)
            .ok_or_else(|| anyhow::anyhow!("cannot read meta for '{}'", purpose))?;

        let key_bytes = seal::unseal(purpose, data_dir, passphrase)
            .with_context(|| format!("unseal '{}' for recovery export", purpose))?;

        entries.push(RecoveryEntry {
            purpose: purpose.to_string(),
            meta,
            key_bytes_hex: hex::encode(&key_bytes),
        });
    }

    let bundle = RecoveryBundle {
        version: BUNDLE_VERSION,
        created_at: Utc::now(),
        purposes: entries,
    };

    let json = serde_json::to_vec(&bundle)?;
    encrypt_bundle(&json, passphrase)
}

pub fn import_recovery_bundle(
    bundle_bytes: &[u8],
    data_dir: &Path,
    passphrase: &str,
    new_pcr_list: Option<&str>,
) -> Result<Vec<RotationRecord>> {
    let json = decrypt_bundle(bundle_bytes, passphrase)?;
    let bundle: RecoveryBundle = serde_json::from_slice(&json)
        .context("parse recovery bundle")?;

    if bundle.version != BUNDLE_VERSION {
        bail!("unsupported bundle version {}", bundle.version);
    }

    let mut records = Vec::new();

    for entry in bundle.purposes {
        let key_bytes = hex::decode(&entry.key_bytes_hex)
            .context("decode key hex from bundle")?;

        let old_backend = entry.meta.backend.clone();
        let old_pcr_list = entry.meta.pcr_list.clone();

        let new_meta = seal::seal(
            &key_bytes,
            &entry.purpose,
            data_dir,
            new_pcr_list,
            passphrase,
        )
        .with_context(|| format!("re-seal '{}'", entry.purpose))?;

        records.push(RotationRecord {
            purpose: entry.purpose,
            rotated_at: Utc::now(),
            reason: "recovery import".to_string(),
            old_backend,
            new_backend: new_meta.backend,
            old_pcr_list,
            new_pcr_list: new_meta.pcr_list,
        });
    }

    Ok(records)
}

pub fn rotate_in_place(
    purpose: &str,
    data_dir: &Path,
    old_passphrase: &str,
    new_pcr_list: Option<&str>,
    new_passphrase: &str,
    reason: &str,
) -> Result<RotationRecord> {
    let old_meta = seal::seal_status(purpose, data_dir)
        .ok_or_else(|| anyhow::anyhow!("'{}' not sealed", purpose))?;

    let key_bytes = seal::unseal(purpose, data_dir, old_passphrase)
        .context("unseal before rotation")?;

    let new_meta = seal::seal(&key_bytes, purpose, data_dir, new_pcr_list, new_passphrase)
        .context("re-seal after rotation")?;

    let record = RotationRecord {
        purpose: purpose.to_string(),
        rotated_at: Utc::now(),
        reason: reason.to_string(),
        old_backend: old_meta.backend,
        new_backend: new_meta.backend.clone(),
        old_pcr_list: old_meta.pcr_list,
        new_pcr_list: new_meta.pcr_list,
    };

    append_rotation_log(data_dir, &record)?;

    Ok(record)
}

pub fn rotation_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("tpm").join("rotation-log.jsonl")
}

fn append_rotation_log(data_dir: &Path, record: &RotationRecord) -> Result<()> {
    use std::io::Write;

    let log_path = rotation_log_path(data_dir);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let line = serde_json::to_string(record)? + "\n";
    f.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_rotation_log(data_dir: &Path) -> Result<Vec<RotationRecord>> {
    let path = rotation_log_path(data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let records = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    Ok(records)
}

fn encrypt_bundle(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let mut dk = [0u8; 32];
    pbkdf2_hmac_sha256(passphrase.as_bytes(), &salt, 200_000, &mut dk);
    let key = Key::<Aes256Gcm>::from_slice(&dk);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("bundle encrypt: {e}"))?;

    let mut out = Vec::with_capacity(4 + 1 + 16 + 12 + ciphertext.len());
    out.extend_from_slice(BUNDLE_MAGIC);
    out.push(BUNDLE_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_bundle(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    if data.len() < 4 + 1 + 16 + 12 {
        bail!("bundle too short");
    }
    if &data[..4] != BUNDLE_MAGIC {
        bail!("not a Maranode recovery bundle");
    }
    let version = data[4];
    if version != BUNDLE_VERSION {
        bail!("unsupported bundle version {}", version);
    }

    let salt = &data[5..21];
    let nonce_bytes = &data[21..33];
    let ciphertext = &data[33..];

    let mut dk = [0u8; 32];
    pbkdf2_hmac_sha256(passphrase.as_bytes(), salt, 200_000, &mut dk);
    let key = Key::<Aes256Gcm>::from_slice(&dk);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("bundle decrypt failed — wrong passphrase?"))?;

    Ok(plaintext)
}

