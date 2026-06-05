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
