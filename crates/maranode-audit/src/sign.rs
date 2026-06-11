use std::path::{Path, PathBuf};

use anyhow::Result;
use ed25519_dalek::{SigningKey, Signer};
use rand::rngs::OsRng;

pub fn signing_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("bundle_signing.key")
}

pub fn verifying_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("bundle_signing.pub")
}

pub fn load_or_create(data_dir: &Path) -> Result<SigningKey> {
    let path = signing_key_path(data_dir);
    if path.exists() {
        let raw = std::fs::read(&path)?;
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| anyhow::anyhow!("bundle signing key has wrong length"))?;
        Ok(SigningKey::from_bytes(&bytes))
    } else {
        let key = SigningKey::generate(&mut OsRng);
        let bytes = key.to_bytes();

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
            f.write_all(&bytes)?;
            f.sync_all()?;
        }

        let pub_path = verifying_key_path(data_dir);
        std::fs::write(&pub_path, key.verifying_key().to_bytes())?;

        Ok(key)
    }
}

pub fn sign(key: &SigningKey, message: &[u8]) -> [u8; 64] {
    key.sign(message).to_bytes()
}
