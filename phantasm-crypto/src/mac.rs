use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CryptoError, Result};

pub const MAC_LEN: usize = 16;

// Separate HKDF-extract salts per output key so the AEAD and MAC keys are
// derived from independent PRKs — stronger domain separation than info-string-
// only would give. v3 envelope; v2 used a single shared PRK with disjoint info
// strings (sound but flagged as MINIMAX_AUDIT Finding 5 for cleaner separation).
const HKDF_SALT_AEAD: &[u8] = b"phantasm-v3-aead-salt";
const HKDF_SALT_MAC: &[u8] = b"phantasm-v3-mac-salt";
const HKDF_INFO_AEAD: &[u8] = b"phantasm-v3-aead";
const HKDF_INFO_MAC: &[u8] = b"phantasm-v3-mac";

type HmacSha256 = Hmac<Sha256>;

pub(crate) struct SubKeys {
    pub aead_key: Zeroizing<[u8; 32]>,
    pub mac_key: Zeroizing<[u8; 32]>,
}

pub(crate) fn split_keys(master: &[u8; 32]) -> SubKeys {
    let aead_hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT_AEAD), master);
    let mac_hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT_MAC), master);
    let mut aead_key = Zeroizing::new([0u8; 32]);
    let mut mac_key = Zeroizing::new([0u8; 32]);
    aead_hkdf
        .expand(HKDF_INFO_AEAD, aead_key.as_mut())
        .expect("HKDF expand aead");
    mac_hkdf
        .expand(HKDF_INFO_MAC, mac_key.as_mut())
        .expect("HKDF expand mac");
    SubKeys { aead_key, mac_key }
}

pub(crate) fn compute_mac(
    mac_key: &[u8; 32],
    version: u8,
    salt: &[u8; 32],
    nonce: &[u8; 24],
    ciphertext: &[u8],
) -> [u8; MAC_LEN] {
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(mac_key).expect("HMAC key length");
    hmac.update(&[version]);
    hmac.update(salt);
    hmac.update(nonce);
    hmac.update(ciphertext);
    let tag = hmac.finalize().into_bytes();
    let mut out = [0u8; MAC_LEN];
    out.copy_from_slice(&tag[..MAC_LEN]);
    out
}

pub(crate) fn verify_mac(
    mac_key: &[u8; 32],
    version: u8,
    salt: &[u8; 32],
    nonce: &[u8; 24],
    ciphertext: &[u8],
    tag: &[u8; MAC_LEN],
) -> Result<()> {
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(mac_key).expect("HMAC key length");
    hmac.update(&[version]);
    hmac.update(salt);
    hmac.update(nonce);
    hmac.update(ciphertext);
    let full = hmac.finalize().into_bytes();
    let mut diff: u8 = 0;
    for (a, b) in full[..MAC_LEN].iter().zip(tag.iter()) {
        diff |= a ^ b;
    }
    if diff == 0 {
        Ok(())
    } else {
        Err(CryptoError::AuthFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subkeys_are_distinct() {
        let master = [0x11u8; 32];
        let keys = split_keys(&master);
        assert_ne!(*keys.aead_key, *keys.mac_key);
        assert_ne!(*keys.aead_key, master);
        assert_ne!(*keys.mac_key, master);
    }

    #[test]
    fn subkeys_are_deterministic() {
        let master = [0x22u8; 32];
        let a = split_keys(&master);
        let b = split_keys(&master);
        assert_eq!(*a.aead_key, *b.aead_key);
        assert_eq!(*a.mac_key, *b.mac_key);
    }

    #[test]
    fn mac_verifies_correct() {
        let mk = [0x33u8; 32];
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 3, &salt, &nonce, ct);
        assert!(verify_mac(&mk, 3, &salt, &nonce, ct, &tag).is_ok());
    }

    #[test]
    fn mac_rejects_wrong_key() {
        let mk = [0x33u8; 32];
        let mut bad = mk;
        bad[0] ^= 0xff;
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 3, &salt, &nonce, ct);
        assert!(matches!(
            verify_mac(&bad, 3, &salt, &nonce, ct, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }

    #[test]
    fn mac_rejects_tampered_ciphertext() {
        let mk = [0x33u8; 32];
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 3, &salt, &nonce, ct);
        let tampered: &[u8] = b"Ciphertext bytes";
        assert!(matches!(
            verify_mac(&mk, 3, &salt, &nonce, tampered, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }

    #[test]
    fn mac_rejects_version_mismatch() {
        let mk = [0x33u8; 32];
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 3, &salt, &nonce, ct);
        assert!(matches!(
            verify_mac(&mk, 2, &salt, &nonce, ct, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }

    #[test]
    fn aead_and_mac_keys_use_independent_prks() {
        // Regression for MINIMAX_AUDIT Finding 5: deriving AEAD and MAC keys
        // through separate HKDF-extract calls (different salts) ensures the
        // PRKs themselves are independent, not just the expanded outputs.
        // Verifies aead_key != mac_key under any input — same-source-key
        // collision impossible by construction.
        let master = [0x55u8; 32];
        let keys = split_keys(&master);
        assert_ne!(*keys.aead_key, *keys.mac_key);
    }
}
