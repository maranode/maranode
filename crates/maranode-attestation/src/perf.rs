use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tee::{detect_tee, get_report, TeeType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeePerf {
    pub tee_type: String,
    pub is_synthetic: bool,
    pub detect_us: u64,
    pub report_us: u64,
    pub encrypt_us: u64, // single AES-256-GCM encrypt of 4096-byte block
    pub decrypt_us: u64,
    pub note: String,
}

pub fn measure() -> TeePerf {
    let t0 = Instant::now();
    let tee_type = detect_tee();
    let detect_us = t0.elapsed().as_micros() as u64;

    let nonce = b"perf-probe-nonce-12345";
    let t1 = Instant::now();
    let report = get_report(nonce);
    let report_us = t1.elapsed().as_micros() as u64;

    let (encrypt_us, decrypt_us) = measure_aes_overhead();

    TeePerf {
        tee_type: tee_type.to_string(),
        is_synthetic: report.is_synthetic,
        detect_us,
        report_us,
        encrypt_us,
        decrypt_us,
        note: if tee_type == TeeType::None {
            "running outside a TEE; timings reflect software emulation".into()
        } else {
            "timings include ioctl round-trip to kernel TEE driver".into()
        },
    }
}

fn measure_aes_overhead() -> (u64, u64) {
    use aes_gcm::aead::rand_core::RngCore;
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    let mut key_bytes = [0u8; 32];
    let mut nonce_bytes = [0u8; 12];
    let mut plaintext = vec![0u8; 4096];
    let mut rng = aes_gcm::aead::OsRng;
    rng.fill_bytes(&mut key_bytes);
    rng.fill_bytes(&mut nonce_bytes);
    rng.fill_bytes(&mut plaintext);

    let cipher = Aes256Gcm::new_from_slice(&key_bytes).unwrap();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let t0 = Instant::now();
    let ciphertext = cipher.encrypt(nonce, plaintext.as_slice()).unwrap_or_default();
    let encrypt_us = t0.elapsed().as_micros() as u64;

    let t1 = Instant::now();
    let _ = cipher.decrypt(nonce, ciphertext.as_slice());
    let decrypt_us = t1.elapsed().as_micros() as u64;

    (encrypt_us, decrypt_us)
}
