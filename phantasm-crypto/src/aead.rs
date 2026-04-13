use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

use crate::{CryptoError, Result};

pub fn encrypt(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let xnonce = XNonce::from_slice(nonce);
    cipher
        .encrypt(
            xnonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption failed")
}

pub fn decrypt(key: &[u8; 32], nonce: &[u8; 24], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let xnonce = XNonce::from_slice(nonce);
    cipher
        .decrypt(
            xnonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8439 §2.8.2 uses 12-byte nonces (IETF ChaCha20-Poly1305), not XChaCha20.
    /// We verify our encrypt/decrypt path is internally consistent and authenticate correctly.
    /// A known XChaCha20-Poly1305 vector from libsodium test suite is used instead.
    #[test]
    fn xchacha20_poly1305_roundtrip_and_auth() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 24];
        let plaintext = b"Hello, XChaCha20-Poly1305!";
        let aad = b"additional data";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad);
        assert_ne!(&ciphertext[..plaintext.len()], plaintext);

        let recovered = decrypt(&key, &nonce, &ciphertext, aad).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn wrong_key_rejected() {
        let key = [0x42u8; 32];
        let mut bad_key = key;
        bad_key[0] ^= 0xff;
        let nonce = [0x24u8; 24];
        let ct = encrypt(&key, &nonce, b"secret", b"");
        assert!(decrypt(&bad_key, &nonce, &ct, b"").is_err());
    }

    #[test]
    fn wrong_aad_rejected() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 24];
        let ct = encrypt(&key, &nonce, b"secret", b"aad");
        assert!(decrypt(&key, &nonce, &ct, b"wrong").is_err());
    }

    /// Verify with a known XChaCha20-Poly1305 test vector.
    /// Vector from https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03 §A.3.1
    #[test]
    fn xchacha20_poly1305_known_vector() {
        // key: 80 81 82 83 ... 9f (32 bytes)
        let key: [u8; 32] = [
            0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d,
            0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b,
            0x9c, 0x9d, 0x9e, 0x9f,
        ];
        // nonce: 40 41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 50 51 52 53 54 55 56 57 (24 bytes)
        let nonce: [u8; 24] = [
            0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d,
            0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57,
        ];
        // plaintext: "Ladies and Gentlemen of the class of '99..."
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        // aad: 50 51 52 53 c0 c1 c2 c3 c4 c5 c6 c7 (12 bytes)
        let aad: &[u8] = &[
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];

        let ct = encrypt(&key, &nonce, plaintext, aad);

        // Expected ciphertext from draft-irtf-cfrg-xchacha §A.3.1
        let expected_ct: &[u8] = &[
            0xbd, 0x6d, 0x17, 0x9d, 0x3e, 0x83, 0xd4, 0x3b, 0x95, 0x76, 0x57, 0x94, 0x93, 0xc0,
            0xe9, 0x39, 0x57, 0x2a, 0x17, 0x00, 0x25, 0x2b, 0xfa, 0xcc, 0xbe, 0xd2, 0x90, 0x2c,
            0x21, 0x39, 0x6c, 0xbb, 0x73, 0x1c, 0x7f, 0x1b, 0x0b, 0x4a, 0xa6, 0x44, 0x0b, 0xf3,
            0xa8, 0x2f, 0x4e, 0xda, 0x7e, 0x39, 0xae, 0x64, 0xc6, 0x70, 0x8c, 0x54, 0xc2, 0x16,
            0xcb, 0x96, 0xb7, 0x2e, 0x12, 0x13, 0xb4, 0x52, 0x2f, 0x8c, 0x9b, 0xa4, 0x0d, 0xb5,
            0xd9, 0x45, 0xb1, 0x1b, 0x69, 0xb9, 0x82, 0xc1, 0xbb, 0x9e, 0x3f, 0x3f, 0xac, 0x2b,
            0xc3, 0x69, 0x48, 0x8f, 0x76, 0xb2, 0x38, 0x35, 0x65, 0xd3, 0xff, 0xf9, 0x21, 0xf9,
            0x66, 0x4c, 0x97, 0x63, 0x7d, 0xa9, 0x76, 0x88, 0x12, 0xf6, 0x15, 0xc6, 0x8b, 0x13,
            0xb5, 0x2e, // tag (16 bytes)
            0xc0, 0x87, 0x59, 0x24, 0xc1, 0xc7, 0x98, 0x79, 0x47, 0xde, 0xaf, 0xd8, 0x78, 0x0a,
            0xcf, 0x49,
        ];

        assert_eq!(ct, expected_ct);

        // Also verify decrypt works
        let recovered = decrypt(&key, &nonce, &ct, aad).unwrap();
        assert_eq!(recovered, plaintext);
    }
}
