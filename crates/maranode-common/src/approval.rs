use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalToken {
    pub token_id: String,
    pub model_id: String,
    pub model_sha256: String,
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub signer_pubkey: String,
    pub signature: String,
}

#[derive(Serialize)]
struct SignPayload<'a> {
    token_id: &'a str,
    model_id: &'a str,
    model_sha256: &'a str,
    approved_by: &'a str,
    approved_at: &'a DateTime<Utc>,
    expires_at: Option<&'a DateTime<Utc>>,
    note: Option<&'a str>,
    signer_pubkey: &'a str,
}

impl ApprovalToken {
    pub fn sign(mut self, key: &ed25519_dalek::SigningKey) -> anyhow::Result<Self> {
        use base64::Engine;
        use ed25519_dalek::Signer;

        let pubkey_bytes = key.verifying_key().to_bytes();
        self.signer_pubkey = base64::engine::general_purpose::STANDARD.encode(pubkey_bytes);

        let payload = SignPayload {
            token_id: &self.token_id,
            model_id: &self.model_id,
            model_sha256: &self.model_sha256,
            approved_by: &self.approved_by,
            approved_at: &self.approved_at,
            expires_at: self.expires_at.as_ref(),
            note: self.note.as_deref(),
            signer_pubkey: &self.signer_pubkey,
        };
        let msg = serde_json::to_vec(&payload)?;
        let sig = key.sign(&msg).to_bytes();
        self.signature = base64::engine::general_purpose::STANDARD.encode(sig);
        Ok(self)
    }

    pub fn verify(&self) -> anyhow::Result<()> {
        use base64::Engine;
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let pubkey_bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
            .decode(&self.signer_pubkey)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("approval token pubkey wrong length"))?;
        let vk = VerifyingKey::from_bytes(&pubkey_bytes)?;

        let sig_bytes: [u8; 64] = base64::engine::general_purpose::STANDARD
            .decode(&self.signature)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("approval token signature wrong length"))?;
        let sig = Signature::from_bytes(&sig_bytes);

        let payload = SignPayload {
            token_id: &self.token_id,
            model_id: &self.model_id,
            model_sha256: &self.model_sha256,
            approved_by: &self.approved_by,
            approved_at: &self.approved_at,
            expires_at: self.expires_at.as_ref(),
            note: self.note.as_deref(),
            signer_pubkey: &self.signer_pubkey,
        };
        let msg = serde_json::to_vec(&payload)?;
        vk.verify(&msg, &sig)?;

        if let Some(exp) = self.expires_at {
            if Utc::now() > exp {
                anyhow::bail!("approval token expired at {exp}");
            }
        }

        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn token_path(tokens_dir: &Path, model_sha256: &str) -> PathBuf {
        tokens_dir.join(format!("{model_sha256}.mrn-token"))
    }

    pub fn signing_key_path(data_dir: &Path) -> PathBuf {
        data_dir.join("approval_signing.key")
    }

    pub fn verifying_key_path(data_dir: &Path) -> PathBuf {
        data_dir.join("approval_signing.pub")
    }

    pub fn load_or_create_signing_key(data_dir: &Path) -> anyhow::Result<ed25519_dalek::SigningKey> {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let key_path = Self::signing_key_path(data_dir);
        if key_path.exists() {
            let bytes = std::fs::read(&key_path)?;
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("approval signing key wrong length"))?;
            return Ok(SigningKey::from_bytes(&arr));
        }

        let key = SigningKey::generate(&mut OsRng);
        if let Some(p) = key_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&key_path, key.to_bytes())?;
        std::fs::write(Self::verifying_key_path(data_dir), key.verifying_key().to_bytes())?;
        Ok(key)
    }
}
