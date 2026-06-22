//! compute and verify HMAC values for the audit chain

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn compute(key: &[u8], data: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

pub fn verify(key: &[u8], data: &[u8], expected_hex: &str) -> bool {
    maranode_common::secure::ct_eq_str(&compute(key, data), expected_hex)
}

pub const GENESIS_HMAC: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_is_deterministic_hex() {
        let a = compute(b"key", b"data");
        let b = compute(b"key", b"data");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_accepts_only_the_matching_mac() {
        let mac = compute(b"secret", b"payload");
        assert!(verify(b"secret", b"payload", &mac));
        assert!(!verify(b"secret", b"payload", GENESIS_HMAC));
        assert!(!verify(b"wrong-key", b"payload", &mac));
        assert!(!verify(b"secret", b"tampered", &mac));
    }

    #[test]
    fn different_keys_produce_different_macs() {
        assert_ne!(compute(b"k1", b"d"), compute(b"k2", b"d"));
    }
}
