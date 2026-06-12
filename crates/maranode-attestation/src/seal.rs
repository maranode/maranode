// TPM2 seal/unseal for sensitive key material.
//
// Two backends are supported:
//   1. tpm2-tools (subprocess) — real TPM, production path. requires tpm2_createprimary,
//      tpm2_create, tpm2_load, tpm2_unseal to be installed and /dev/tpmrm0 available.
//   2. software — AES-256-GCM with PBKDF2-derived key, for dev/non-TPM environments.
//      the software backend does NOT bind to PCR values.
//
// Sealed blobs are stored at:
//   <data_dir>/tpm/<purpose>/sealed.pub   — public area (tpm2-tools) or header
//   <data_dir>/tpm/<purpose>/sealed.priv  — private area (tpm2-tools) or ciphertext
//   <data_dir>/tpm/<purpose>/backend      — "tpm2" or "software"
//
// DoS protection: failed unseal attempts are counted and rate-limited.
// After UNSEAL_FAIL_LOCKOUT consecutive failures, a 60-second cooldown is enforced.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    aead::rand_core::RngCore,
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const UNSEAL_FAIL_LOCKOUT: u32 = 5;
const LOCKOUT_SECS: u64 = 60;
const SOFTWARE_SALT_ITERATIONS: u32 = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealMeta {
    pub backend: SealBackend,
    pub purpose: String,
    pub sealed_at: String,
    pub pcr_list: Option<String>, // PCR list string, tpm2-tools backend only
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SealBackend {
    Tpm2,
    Software,
}

/// seal `data` using the best available backend.
/// `pcr_policy_str` is the value for --pcr-list in tpm2-tools format (e.g. "sha256:0,7").
/// if `pcr_policy_str` is None or TPM unavailable, falls back to software seal.
/// `passphrase` is used for the software backend only.
pub fn seal(
    data: &[u8],
    purpose: &str,
    data_dir: &Path,
    pcr_policy_str: Option<&str>,
    software_passphrase: &str,
) -> Result<SealMeta> {
    let blob_dir = blob_dir(data_dir, purpose);
    std::fs::create_dir_all(&blob_dir)?;

    if pcr_policy_str.is_some() && is_tpm2_tools_available() {
        match tpm2_seal(data, purpose, &blob_dir, pcr_policy_str.unwrap()) {
            Ok(meta) => return Ok(meta),
            Err(e) => {
                tracing::warn!("TPM2 seal failed, falling back to software seal: {e}");
            }
        }
    }

    software_seal(data, purpose, &blob_dir, software_passphrase)
}

/// unseal `purpose` key from the stored blob.
/// `software_passphrase` is used when backend is software.
/// returns the plaintext key bytes.
pub fn unseal(
    purpose: &str,
    data_dir: &Path,
    software_passphrase: &str,
) -> Result<Vec<u8>> {
    let blob_dir = blob_dir(data_dir, purpose);

    check_dos_limit(&blob_dir)?;

    let meta = load_meta(&blob_dir)?;
    let result = match meta.backend {
        SealBackend::Tpm2 => tpm2_unseal(purpose, &blob_dir, meta.pcr_list.as_deref()),
        SealBackend::Software => software_unseal(&blob_dir, software_passphrase),
    };

    match &result {
        Ok(_) => reset_fail_counter(&blob_dir),
        Err(_) => increment_fail_counter(&blob_dir),
    }

    result
}

pub fn seal_status(purpose: &str, data_dir: &Path) -> Option<SealMeta> {
    let blob_dir = blob_dir(data_dir, purpose);
    load_meta(&blob_dir).ok()
}

pub fn is_sealed(purpose: &str, data_dir: &Path) -> bool {
    let blob_dir = blob_dir(data_dir, purpose);
    blob_dir.join("meta.json").exists()
}

pub fn is_tpm2_tools_available() -> bool {
    std::process::Command::new("tpm2_createprimary")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn blob_dir(data_dir: &Path, purpose: &str) -> PathBuf {
    data_dir.join("tpm").join(purpose)
}

fn load_meta(blob_dir: &Path) -> Result<SealMeta> {
    let bytes = std::fs::read(blob_dir.join("meta.json"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_meta(blob_dir: &Path, meta: &SealMeta) -> Result<()> {
    std::fs::write(
        blob_dir.join("meta.json"),
        serde_json::to_vec_pretty(meta)?,
    )?;
    Ok(())
}

fn tpm2_seal(data: &[u8], purpose: &str, blob_dir: &Path, pcr_list: &str) -> Result<Vec<u8>> {
    use std::process::Command;

    let tmp = tempfile::Builder::new().prefix("mrn-tpm-").tempdir()?;
    let data_file = tmp.path().join("data.bin");
    let primary_ctx = tmp.path().join("primary.ctx");
    let policy_file = tmp.path().join("pcr.pol");
    let pub_file = blob_dir.join("sealed.pub");
    let priv_file = blob_dir.join("sealed.priv");

    std::fs::write(&data_file, data)?;

    // create primary key in owner hierarchy
    let out = Command::new("tpm2_createprimary")
        .args(["-C", "o", "-c"])
        .arg(&primary_ctx)
        .output()
        .context("tpm2_createprimary")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_createprimary failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // create PCR policy
    let out = Command::new("tpm2_createpolicy")
        .args(["--policy-pcr", "-l"])
        .arg(pcr_list)
        .args(["-L"])
        .arg(&policy_file)
        .output()
        .context("tpm2_createpolicy")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_createpolicy failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // seal data to the policy
    let out = Command::new("tpm2_create")
        .args(["-C"])
        .arg(&primary_ctx)
        .args(["-L"])
        .arg(&policy_file)
        .args(["-i"])
        .arg(&data_file)
        .args(["-u"])
        .arg(&pub_file)
        .args(["-r"])
        .arg(&priv_file)
        .output()
        .context("tpm2_create")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_create (seal) failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let sealed_at = chrono::Utc::now().to_rfc3339();
    let meta = SealMeta {
        backend: SealBackend::Tpm2,
        purpose: purpose.to_string(),
        sealed_at,
        pcr_list: Some(pcr_list.to_string()),
    };
    save_meta(blob_dir, &meta)?;

    Ok(meta.sealed_at.into_bytes()) // return meta bytes as confirmation
}

fn tpm2_unseal(purpose: &str, blob_dir: &Path, pcr_list: Option<&str>) -> Result<Vec<u8>> {
    use std::process::Command;

    let tmp = tempfile::Builder::new().prefix("mrn-tpm-").tempdir()?;
    let primary_ctx = tmp.path().join("primary.ctx");
    let sealed_ctx = tmp.path().join("sealed.ctx");
    let pub_file = blob_dir.join("sealed.pub");
    let priv_file = blob_dir.join("sealed.priv");

    // create primary key (same derivation, deterministic in owner hierarchy)
    let out = Command::new("tpm2_createprimary")
        .args(["-C", "o", "-c"])
        .arg(&primary_ctx)
        .output()
        .context("tpm2_createprimary")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_createprimary failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // load sealed object
    let out = Command::new("tpm2_load")
        .args(["-C"])
        .arg(&primary_ctx)
        .args(["-u"])
        .arg(&pub_file)
        .args(["-r"])
        .arg(&priv_file)
        .args(["-c"])
        .arg(&sealed_ctx)
        .output()
        .context("tpm2_load")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_load failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // build unseal command with optional PCR policy
    let mut cmd = Command::new("tpm2_unseal");
    cmd.args(["-c"]).arg(&sealed_ctx);
    if let Some(pcrs) = pcr_list {
        cmd.args(["-p", &format!("pcr:{}", pcrs)]);
    }
    let out = cmd.output().context("tpm2_unseal")?;
    if !out.status.success() {
        anyhow::bail!(
            "tpm2_unseal failed (PCR mismatch or locked out): {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let _ = purpose; // used for logging at the call site
    Ok(out.stdout)
}

fn software_seal(data: &[u8], purpose: &str, blob_dir: &Path, passphrase: &str) -> Result<SealMeta> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    let key = pbkdf2_key(passphrase, &salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("software seal encrypt: {e}"))?;

    // header: 4 bytes magic + 16 bytes salt + 12 bytes nonce + remaining is ciphertext
    let mut blob = Vec::with_capacity(4 + 16 + 12 + ct.len());
    blob.extend_from_slice(b"MRN1");
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(nonce_bytes.as_ref());
    blob.extend_from_slice(&ct);

    std::fs::write(blob_dir.join("sealed.bin"), &blob)?;

    let sealed_at = chrono::Utc::now().to_rfc3339();
    let meta = SealMeta {
        backend: SealBackend::Software,
        purpose: purpose.to_string(),
        sealed_at,
        pcr_list: None,
    };
    save_meta(blob_dir, &meta)?;

    Ok(meta)
}

fn software_unseal(blob_dir: &Path, passphrase: &str) -> Result<Vec<u8>> {
    let blob = std::fs::read(blob_dir.join("sealed.bin"))?;

    if blob.len() < 4 + 16 + 12 {
        anyhow::bail!("sealed.bin too short");
    }
    if &blob[..4] != b"MRN1" {
        anyhow::bail!("sealed.bin has wrong magic bytes");
    }

    let salt = &blob[4..20];
    let nonce_bytes = &blob[20..32];
    let ct = &blob[32..];

    let key = pbkdf2_key(passphrase, salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| anyhow::anyhow!("software unseal failed — wrong passphrase or corrupted blob"))
}

fn pbkdf2_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    // manual PBKDF2-HMAC-SHA256 (avoid importing pbkdf2 crate — use sha2 + hmac already present)
    pbkdf2_hmac_sha256(passphrase.as_bytes(), salt, SOFTWARE_SALT_ITERATIONS, &mut key);
    key
}

// minimal PBKDF2-HMAC-SHA256 implementation (F(P, S, c, i) per RFC 2898)
pub(crate) fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8; 32]) {
    use sha2::Sha256;
    use hmac::{Hmac, Mac};

    type HmacSha256 = Hmac<Sha256>;

    let mut u = [0u8; 32];

    // U1 = PRF(Password, Salt || INT(i))
    let mut mac = HmacSha256::new_from_slice(password).expect("HMAC key invalid");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes()); // block index 1
    let u1 = mac.finalize().into_bytes();
    u.copy_from_slice(&u1);
    out.copy_from_slice(&u);

    for _ in 1..rounds {
        let mut mac = HmacSha256::new_from_slice(password).expect("HMAC key invalid");
        mac.update(&u);
        let un = mac.finalize().into_bytes();
        u.copy_from_slice(&un);
        for (a, b) in out.iter_mut().zip(u.iter()) {
            *a ^= b;
        }
    }
}

// DoS protection — track failed unseal attempts in a small file.

fn fail_counter_path(blob_dir: &Path) -> PathBuf {
    blob_dir.join(".fail_counter")
}

#[derive(Serialize, Deserialize, Default)]
struct FailCounter {
    count: u32,
    last_fail_ts: u64,
}

fn read_fail_counter(blob_dir: &Path) -> FailCounter {
    let path = fail_counter_path(blob_dir);
    std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_fail_counter(blob_dir: &Path, fc: &FailCounter) {
    let path = fail_counter_path(blob_dir);
    if let Ok(bytes) = serde_json::to_vec(fc) {
        let _ = std::fs::write(path, bytes);
    }
}

fn check_dos_limit(blob_dir: &Path) -> Result<()> {
    let fc = read_fail_counter(blob_dir);
    if fc.count >= UNSEAL_FAIL_LOCKOUT {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(fc.last_fail_ts) < LOCKOUT_SECS {
            anyhow::bail!(
                "too many failed unseal attempts — locked out for {} seconds",
                LOCKOUT_SECS.saturating_sub(now.saturating_sub(fc.last_fail_ts))
            );
        }
    }
    Ok(())
}

fn increment_fail_counter(blob_dir: &Path) {
    let mut fc = read_fail_counter(blob_dir);
    fc.count = fc.count.saturating_add(1);
    fc.last_fail_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    write_fail_counter(blob_dir, &fc);
}

fn reset_fail_counter(blob_dir: &Path) {
    write_fail_counter(blob_dir, &FailCounter::default());
}
