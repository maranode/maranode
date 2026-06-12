use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

use maranode_common::hold::{HoldRecord, PlacementPayload, ReleasePayload};

pub fn holds_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("legal-holds")
}

pub fn hold_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("hold.key") // private key — compliance officer controls this
}

pub fn hold_pubkey_path(data_dir: &Path) -> PathBuf {
    data_dir.join("hold.pub") // public key — stored server-side for verification
}

// generate a new hold Ed25519 keypair. returns (privkey_hex, pubkey_b64)
pub fn generate_hold_key(data_dir: &Path) -> Result<(String, String)> {
    use rand_core::OsRng;
    let key = SigningKey::generate(&mut OsRng);
    let pubkey_b64 = base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes());
    let privkey_hex = hex::encode(key.to_bytes());

    // store pubkey server-side
    std::fs::write(hold_pubkey_path(data_dir), &pubkey_b64)?;

    // store privkey locally too (compliance officer should remove this and keep it offline)
    std::fs::write(hold_key_path(data_dir), &privkey_hex)?;

    Ok((privkey_hex, pubkey_b64))
}

pub fn load_hold_pubkey(data_dir: &Path) -> Result<VerifyingKey> {
    let b64 = std::fs::read_to_string(hold_pubkey_path(data_dir))
        .context("hold public key not found — run `maranode hold generate-key` first")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .context("invalid base64 in hold.pub")?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("hold pubkey must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).context("invalid Ed25519 verifying key")
}

pub fn place_hold(
    data_dir: &Path,
    hold_id: &str,
    placed_by: &str,
    reason: &str,
    seq_from: u64,
    seq_to: u64,
    expires_at: Option<chrono::DateTime<Utc>>,
    signing_key_hex: Option<&str>,
    tpm_sealed: bool,
) -> Result<HoldRecord> {
    let pubkey = load_hold_pubkey(data_dir)?;
    let pubkey_b64 = base64::engine::general_purpose::STANDARD.encode(pubkey.to_bytes());
    let placed_at = Utc::now();

    let payload = PlacementPayload {
        hold_id,
        placed_by,
        reason,
        seq_from,
        seq_to,
        placed_at: &placed_at,
        hold_key_pubkey: &pubkey_b64,
    };
    let payload_json = serde_json::to_vec(&payload)?;

    // sign placement with the hold signing key
    let signing_key = resolve_signing_key(data_dir, signing_key_hex)?;
    let sig = signing_key.sign(&payload_json);
    let placement_sig = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());

    let record = HoldRecord {
        id: hold_id.to_string(),
        placed_at,
        placed_by: placed_by.to_string(),
        reason: reason.to_string(),
        seq_from,
        seq_to,
        expires_at,
        hold_key_pubkey: pubkey_b64,
        placement_sig,
        released_at: None,
        released_by: None,
        release_sig: None,
        tpm_sealed,
    };

    save_hold(data_dir, &record)?;
    Ok(record)
}

pub fn release_hold(
    data_dir: &Path,
    hold_id: &str,
    released_by: &str,
    release_sig_b64: &str,
) -> Result<HoldRecord> {
    let mut record = load_hold(data_dir, hold_id)?;

    if !record.is_active() {
        bail!("hold '{}' is not active", hold_id);
    }

    // verify release signature was produced by the hold private key
    let pubkey = load_hold_pubkey(data_dir)?;
    let released_at = Utc::now();

    let payload = ReleasePayload {
        hold_id,
        released_by,
        released_at: &released_at,
    };
    let payload_json = serde_json::to_vec(&payload)?;

    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(release_sig_b64)
        .context("invalid base64 release signature")?;
    let sig_arr: [u8; 64] = sig_bytes.try_into()
        .map_err(|_| anyhow::anyhow!("release signature must be 64 bytes"))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);

    pubkey.verify(&payload_json, &sig)
        .context("release signature verification failed — wrong hold key?")?;

    record.released_at = Some(released_at);
    record.released_by = Some(released_by.to_string());
    record.release_sig = Some(release_sig_b64.to_string());

    save_hold(data_dir, &record)?;
    Ok(record)
}

pub fn is_seq_held(data_dir: &Path, seq: u64) -> bool {
    let Ok(holds) = list_holds(data_dir) else { return false };
    holds.iter().any(|h| h.covers_seq(seq))
}

pub fn list_holds(data_dir: &Path) -> Result<Vec<HoldRecord>> {
    let dir = holds_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            let bytes = std::fs::read(entry.path())?;
            if let Ok(r) = serde_json::from_slice::<HoldRecord>(&bytes) {
                records.push(r);
            }
        }
    }
    records.sort_by_key(|r| r.placed_at);
    Ok(records)
}

fn load_hold(data_dir: &Path, hold_id: &str) -> Result<HoldRecord> {
    let path = holds_dir(data_dir).join(format!("{hold_id}.json"));
    let bytes = std::fs::read(&path)
        .with_context(|| format!("hold '{hold_id}' not found"))?;
    serde_json::from_slice(&bytes).context("parse hold record")
}

fn save_hold(data_dir: &Path, record: &HoldRecord) -> Result<()> {
    let dir = holds_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", record.id));
    std::fs::write(path, serde_json::to_vec_pretty(record)?)?;
    Ok(())
}

fn resolve_signing_key(data_dir: &Path, hex_override: Option<&str>) -> Result<SigningKey> {
    let hex = if let Some(h) = hex_override {
        h.to_string()
    } else {
        std::fs::read_to_string(hold_key_path(data_dir))
            .context("hold private key not found — pass --key-hex or ensure hold.key exists")?
    };
    let bytes = hex::decode(hex.trim()).context("invalid hex in hold key")?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("hold signing key must be 32 bytes"))?;
    Ok(SigningKey::from_bytes(&arr))
}

// sign a release payload for use offline. compliance officer calls this with their key.
pub fn sign_release_payload(
    hold_id: &str,
    released_by: &str,
    released_at: chrono::DateTime<Utc>,
    privkey_hex: &str,
) -> Result<String> {
    let bytes = hex::decode(privkey_hex.trim()).context("invalid hex")?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("key must be 32 bytes"))?;
    let key = SigningKey::from_bytes(&arr);

    let payload = ReleasePayload {
        hold_id,
        released_by,
        released_at: &released_at,
    };
    let payload_json = serde_json::to_vec(&payload)?;
    let sig = key.sign(&payload_json);
    Ok(base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()))
}

// check if a retention deletion is blocked by a hold
pub fn guard_retention_delete(data_dir: &Path, seq_from: u64, seq_to: u64) -> Result<()> {
    let holds = list_holds(data_dir)?;
    for hold in &holds {
        if !hold.is_active() { continue; }
        // check if the requested range overlaps with any hold
        if seq_from <= hold.seq_to && seq_to >= hold.seq_from {
            bail!(
                "cannot delete audit entries {seq_from}-{seq_to}: \
                 legal hold '{}' covers seq {}-{} (reason: {})",
                hold.id, hold.seq_from, hold.seq_to, hold.reason
            );
        }
    }
    Ok(())
}
