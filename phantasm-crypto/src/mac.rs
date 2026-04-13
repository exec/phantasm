use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CryptoError, Result};

pub const MAC_LEN: usize = 16;

const HKDF_INFO_AEAD: &[u8] = b"phantasm-v2-aead";
const HKDF_INFO_MAC: &[u8] = b"phantasm-v2-mac";

type HmacSha256 = Hmac<Sha256>;

pub(crate) struct SubKeys {
    pub aead_key: Zeroizing<[u8; 32]>,
    pub mac_key: Zeroizing<[u8; 32]>,
}

pub(crate) fn split_keys(master: &[u8; 32]) -> SubKeys {
    let hk = Hkdf::<Sha256>::new(None, master);
    let mut aead_key = Zeroizing::new([0u8; 32]);
    let mut mac_key = Zeroizing::new([0u8; 32]);
    hk.expand(HKDF_INFO_AEAD, aead_key.as_mut())
        .expect("HKDF expand aead");
    hk.expand(HKDF_INFO_MAC, mac_key.as_mut())
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
        let tag = compute_mac(&mk, 2, &salt, &nonce, ct);
        assert!(verify_mac(&mk, 2, &salt, &nonce, ct, &tag).is_ok());
    }

    #[test]
    fn mac_rejects_wrong_key() {
        let mk = [0x33u8; 32];
        let mut bad = mk;
        bad[0] ^= 0xff;
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 2, &salt, &nonce, ct);
        assert!(matches!(
            verify_mac(&bad, 2, &salt, &nonce, ct, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }

    #[test]
    fn mac_rejects_tampered_ciphertext() {
        let mk = [0x33u8; 32];
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 2, &salt, &nonce, ct);
        let tampered: &[u8] = b"Ciphertext bytes";
        assert!(matches!(
            verify_mac(&mk, 2, &salt, &nonce, tampered, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }

    #[test]
    fn mac_rejects_version_mismatch() {
        let mk = [0x33u8; 32];
        let salt = [0x44u8; 32];
        let nonce = [0x55u8; 24];
        let ct = b"ciphertext bytes";
        let tag = compute_mac(&mk, 2, &salt, &nonce, ct);
        assert!(matches!(
            verify_mac(&mk, 3, &salt, &nonce, ct, &tag),
            Err(CryptoError::AuthFailed)
        ));
    }
}
