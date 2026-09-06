//! ULID-style identifiers (Crockford base32, 26 chars) derived from UUIDv7.
//!
//! A full ULID carries a 48-bit millisecond timestamp plus 80 bits of
//! randomness. We reuse `uuid` v7 bytes (same layout) and re-encode the
//! 128 bits into the canonical 26-character Crockford alphabet so exchange
//! coordinates are lexicographically sortable by creation time.

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Generates a new 26-character ULID string.
pub fn new_ulid() -> String {
    let bytes = uuid::Uuid::now_v7().into_bytes();
    // 128 bits → 26 chars × 5 bits = 130 bits; the two most significant
    // bits are zero, which is exactly how a canonical ULID is padded.
    let mut acc = u128::from_be_bytes(bytes);
    let mut out = [b'0'; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(acc & 0x1f) as usize];
        acc >>= 5;
    }
    String::from_utf8(out.to_vec()).unwrap_or_else(|_| "0".repeat(26))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_is_26_crockford_chars() {
        let id = new_ulid();
        assert_eq!(id.len(), 26);
        assert!(id.chars().all(|c| CROCKFORD.contains(&(c as u8))));
    }

    #[test]
    fn ulids_are_unique_within_a_burst() {
        let a = new_ulid();
        let b = new_ulid();
        assert_ne!(a, b);
    }
}
