use base64::Engine;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Result};

pub struct TeeEncryptKey(Aes256Gcm);

impl TeeEncryptKey {
    pub fn from_hex(hex_key: &str) -> Result<Self> {
        let bytes = hex::decode(hex_key)?;
        if bytes.len() != 32 {
            bail!("tee encrypt key must be 32 bytes");
        }
        let cipher = Aes256Gcm::new_from_slice(&bytes)
            .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
        Ok(Self(cipher))
    }

    pub fn generate_key() -> String {
        let mut bytes = [0u8; 32];
        aes_gcm::aead::OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    // returns nonce(12) + ciphertext
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        aes_gcm::aead::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .0
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            bail!("ciphertext too short");
        }
        let (nonce_bytes, ct) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.0
            .decrypt(nonce, ct)
            .map_err(|e| anyhow::anyhow!("decrypt: {e}"))
    }

    pub fn encrypt_str(&self, s: &str) -> Result<String> {
        let raw = self.encrypt(s.as_bytes())?;
        Ok(base64::engine::general_purpose::STANDARD.encode(raw))
    }

    pub fn decrypt_str(&self, b64: &str) -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD.decode(b64)?;
        let pt = self.decrypt(&raw)?;
        Ok(String::from_utf8(pt)?)
    }
}
