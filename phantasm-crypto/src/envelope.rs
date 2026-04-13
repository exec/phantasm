use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::{
    aead::{decrypt, encrypt},
    kdf::{derive_key, KdfParams},
    metadata::PayloadMetadata,
    padding::{pad, unpad},
    CryptoError, Result,
};

pub struct Envelope {
    pub salt: [u8; 32],
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

pub fn seal(
    passphrase: &str,
    metadata: PayloadMetadata,
    payload: &[u8],
    params: &KdfParams,
) -> Result<Envelope> {
    let mut salt = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = Zeroizing::new(derive_key(passphrase, &salt, params));

    let meta_bytes = metadata.to_bytes();
    let mut combined = Vec::with_capacity(meta_bytes.len() + payload.len());
    combined.extend_from_slice(&meta_bytes);
    combined.extend_from_slice(payload);

    let padded = pad(&combined)?;

    let ciphertext = encrypt(&key, &nonce, &padded, b"");

    Ok(Envelope {
        salt,
        nonce,
        ciphertext,
    })
}

pub fn open(
    passphrase: &str,
    envelope: &Envelope,
    params: &KdfParams,
) -> Result<(PayloadMetadata, Vec<u8>)> {
    let key = Zeroizing::new(derive_key(passphrase, &envelope.salt, params));

    let padded = decrypt(&key, &envelope.nonce, &envelope.ciphertext, b"")
        .map_err(|_| CryptoError::AuthFailed)?;

    let combined = unpad(&padded).map_err(|_| CryptoError::AuthFailed)?;

    let (metadata, consumed) =
        PayloadMetadata::from_bytes(&combined).map_err(|_| CryptoError::AuthFailed)?;

    let payload_bytes = combined[consumed..].to_vec();

    // Trim to declared payload_len
    let declared_len = metadata.payload_len as usize;
    if declared_len > payload_bytes.len() {
        return Err(CryptoError::AuthFailed);
    }
    let payload = payload_bytes[..declared_len].to_vec();

    Ok((metadata, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::ContentType;
    use rand::RngCore;

    fn make_params() -> KdfParams {
        // Use minimal params for test speed
        KdfParams {
            memory_kib: 8,
            iterations: 1,
            parallelism: 1,
            output_len: 32,
        }
    }

    fn make_metadata(payload_len: u64) -> PayloadMetadata {
        PayloadMetadata {
            filename: Some("test.bin".into()),
            payload_len,
            content_type: ContentType::Raw,
            version: 1,
        }
    }

    #[test]
    fn envelope_roundtrip() {
        let mut payload = vec![0u8; 1024];
        OsRng.fill_bytes(&mut payload);

        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();
        let passphrase = "correct horse battery staple";

        let envelope = seal(passphrase, metadata.clone(), &payload, &params).unwrap();
        let (recovered_meta, recovered_payload) = open(passphrase, &envelope, &params).unwrap();

        assert_eq!(recovered_meta, metadata);
        assert_eq!(recovered_payload, payload);
    }

    #[test]
    fn wrong_passphrase_rejected() {
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let envelope = seal("alpha", metadata, payload, &params).unwrap();
        let result = open("beta", &envelope, &params);
        assert!(matches!(result, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("passphrase", metadata, payload, &params).unwrap();
        envelope.ciphertext[0] ^= 0xff;
        let result = open("passphrase", &envelope, &params);
        assert!(matches!(result, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn tampered_salt_rejected() {
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("passphrase", metadata, payload, &params).unwrap();
        envelope.salt[0] ^= 0xff;
        let result = open("passphrase", &envelope, &params);
        assert!(matches!(result, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn tampered_nonce_rejected() {
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("passphrase", metadata, payload, &params).unwrap();
        envelope.nonce[0] ^= 0xff;
        let result = open("passphrase", &envelope, &params);
        assert!(matches!(result, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn padding_stripping_after_open() {
        // Verify that opened payload has exact pre-pad length regardless of block used
        for payload_len in &[10usize, 100, 500, 5000] {
            let payload = vec![0xdeu8; *payload_len];
            let metadata = make_metadata(*payload_len as u64);
            let params = make_params();

            let envelope = seal("pw", metadata, &payload, &params).unwrap();
            let (_, recovered) = open("pw", &envelope, &params).unwrap();
            assert_eq!(
                recovered.len(),
                *payload_len,
                "length mismatch for {payload_len}"
            );
            assert_eq!(recovered, payload);
        }
    }
}
