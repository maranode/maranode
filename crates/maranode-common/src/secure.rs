//! constant-time compare for secrets. avoids timing leaks

use subtle::ConstantTimeEq;

/// compare two byte slices in constant time
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// compare two secret strings. for API keys, tokens, HMAC hex
pub fn ct_eq_str(a: &str, b: &str) -> bool {
    ct_eq(a.as_bytes(), b.as_bytes())
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_equality() {
        assert!(ct_eq_str("hunter2", "hunter2"));
        assert!(!ct_eq_str("hunter2", "hunter3"));
        assert!(!ct_eq_str("short", "longer-value"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn sha256_hex_validation() {
        let good = "a".repeat(64);
        assert!(is_sha256_hex(&good));
        assert!(!is_sha256_hex(&"a".repeat(63)));
        assert!(!is_sha256_hex(&"A".repeat(64))); // uppercase letters are rejected
        assert!(!is_sha256_hex("../../etc/passwd"));
        assert!(!is_sha256_hex(&format!("{}g", "a".repeat(63))));
    }
}
