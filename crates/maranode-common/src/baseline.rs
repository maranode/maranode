use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestVector {
    pub prompt: String,
    pub temperature: f32,
    pub seed: u64,
    pub max_tokens: u32,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub model_sha256: String,
    pub model_id: String,
    pub created_at: DateTime<Utc>,
    pub vectors: Vec<TestVector>,
    pub max_mismatches: usize,
    pub signer_pubkey: String,
    pub signature: String,
}

#[derive(Serialize)]
struct SignPayload<'a> {
    schema_version: u32,
    model_sha256: &'a str,
    model_id: &'a str,
    created_at: &'a DateTime<Utc>,
    vectors: &'a [TestVector],
    max_mismatches: usize,
    signer_pubkey: &'a str,
}

impl Baseline {
    pub fn sign(
        mut self,
        key: &ed25519_dalek::SigningKey,
    ) -> anyhow::Result<Self> {
        use base64::Engine;
        use ed25519_dalek::Signer;

        let pubkey_bytes = key.verifying_key().to_bytes();
        self.signer_pubkey = base64::engine::general_purpose::STANDARD.encode(pubkey_bytes);

        let payload = SignPayload {
            schema_version: self.schema_version,
            model_sha256: &self.model_sha256,
            model_id: &self.model_id,
            created_at: &self.created_at,
            vectors: &self.vectors,
            max_mismatches: self.max_mismatches,
            signer_pubkey: &self.signer_pubkey,
        };
        let msg = serde_json::to_vec(&payload)?;
        let sig = key.sign(&msg).to_bytes();
        self.signature = base64::engine::general_purpose::STANDARD.encode(sig);
        Ok(self)
    }

    pub fn verify(&self) -> anyhow::Result<()> {
        use base64::Engine;
        use ed25519_dalek::{Signature, VerifyingKey, Verifier};

        let pubkey_bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
            .decode(&self.signer_pubkey)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("baseline pubkey wrong length"))?;
        let vk = VerifyingKey::from_bytes(&pubkey_bytes)?;

        let sig_bytes: [u8; 64] = base64::engine::general_purpose::STANDARD
            .decode(&self.signature)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("baseline signature wrong length"))?;
        let sig = Signature::from_bytes(&sig_bytes);

        let payload = SignPayload {
            schema_version: self.schema_version,
            model_sha256: &self.model_sha256,
            model_id: &self.model_id,
            created_at: &self.created_at,
            vectors: &self.vectors,
            max_mismatches: self.max_mismatches,
            signer_pubkey: &self.signer_pubkey,
        };
        let msg = serde_json::to_vec(&payload)?;
        vk.verify(&msg, &sig).map_err(|e| anyhow::anyhow!("baseline signature invalid: {e}"))
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let data = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn signing_key_path(data_dir: &std::path::Path) -> std::path::PathBuf {
        data_dir.join("baseline_signing.key")
    }

    pub fn verifying_key_path(data_dir: &std::path::Path) -> std::path::PathBuf {
        data_dir.join("baseline_signing.pub")
    }

    pub fn load_or_create_signing_key(
        data_dir: &std::path::Path,
    ) -> anyhow::Result<ed25519_dalek::SigningKey> {
        use rand::rngs::OsRng;
        let path = Self::signing_key_path(data_dir);
        if path.exists() {
            let raw = std::fs::read(&path)?;
            let bytes: [u8; 32] = raw
                .try_into()
                .map_err(|_| anyhow::anyhow!("baseline signing key wrong length"))?;
            Ok(ed25519_dalek::SigningKey::from_bytes(&bytes))
        } else {
            let key = ed25519_dalek::SigningKey::generate(&mut OsRng);
            {
                use std::io::Write;
                let mut opts = std::fs::OpenOptions::new();
                opts.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    opts.mode(0o600);
                }
                let mut f = opts.open(&path)?;
                f.write_all(&key.to_bytes())?;
                f.sync_all()?;
            }
            let pub_path = Self::verifying_key_path(data_dir);
            std::fs::write(&pub_path, key.verifying_key().to_bytes())?;
            Ok(key)
        }
    }
}

pub fn output_sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(text.as_bytes());
    hex::encode(hash)
}
