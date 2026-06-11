//! AES-256-GCM helpers for at-rest encryption of chunk text.
//!
//! Encrypted values are stored as hex strings with the prefix "enc:" so old
//! unencrypted rows stay readable without a key.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use aes_gcm::aead::rand_core::RngCore;
use anyhow::{Context, Result};

const PREFIX: &str = "enc:";

pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
    let mut blob = nonce_bytes.to_vec();
    blob.extend(ciphertext);
    Ok(format!("{}{}", PREFIX, hex::encode(blob)))
}

pub fn decrypt(key: &[u8; 32], stored: &str) -> Result<String> {
    let hex_part = stored
        .strip_prefix(PREFIX)
        .context("value does not have enc: prefix")?;
    let blob = hex::decode(hex_part).context("decoding ciphertext hex")?;
    if blob.len() < 12 {
        anyhow::bail!("ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("aes-gcm decrypt failed (wrong key or corrupted data)"))?;
    String::from_utf8(plain).context("decrypted bytes are not utf-8")
}

/// encrypt if a key is present, else return value unchanged.
pub fn maybe_encrypt(key: Option<&[u8; 32]>, value: &str) -> Result<String> {
    match key {
        Some(k) => encrypt(k, value),
        None => Ok(value.to_string()),
    }
}

/// decrypt if the value has the enc: prefix, else return unchanged.
pub fn maybe_decrypt(key: Option<&[u8; 32]>, value: &str) -> Result<String> {
    if value.starts_with(PREFIX) {
        let k = key.ok_or_else(|| {
            anyhow::anyhow!("value is encrypted but no DEK is available")
        })?;
        decrypt(k, value)
    } else {
        Ok(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [42u8; 32]
    }

    #[test]
    fn round_trip() {
        let k = key();
        let plain = "hello encrypted world";
        let enc = encrypt(&k, plain).unwrap();
        assert!(enc.starts_with("enc:"), "must have prefix");
        let dec = decrypt(&k, &enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn maybe_passthrough_when_no_key() {
        let plain = "plaintext";
        assert_eq!(maybe_encrypt(None, plain).unwrap(), plain);
        assert_eq!(maybe_decrypt(None, plain).unwrap(), plain);
    }

    #[test]
    fn maybe_decrypt_encrypted_with_key() {
        let k = key();
        let enc = maybe_encrypt(Some(&k), "test").unwrap();
        let dec = maybe_decrypt(Some(&k), &enc).unwrap();
        assert_eq!(dec, "test");
    }
}
