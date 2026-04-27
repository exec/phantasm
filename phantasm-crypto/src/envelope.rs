use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::{
    aead::{decrypt, encrypt},
    kdf::{derive_key, KdfParams},
    mac::{compute_mac, split_keys, verify_mac, MAC_LEN},
    metadata::PayloadMetadata,
    padding::{pad, unpad},
    CryptoError, Result,
};

// Envelope format version.
// - v1: original (pre-alpha)
// - v2: added the permutation MAC (alpha polish burst)
// - v3: SALT_QUANT_STEP bumped from 16 to 256 (fixes 42.5% pHash-block drift
//   across `image`-crate QF=85 recompression — see CHANGELOG v1.0.0); HKDF
//   key separation strengthened to use independent extract calls per output
//   key instead of disjoint info strings on a shared PRK (MINIMAX_AUDIT
//   Finding 5). v1/v2 envelopes are NOT readable by v3 code.
pub const FORMAT_VERSION: u8 = 3;

const SERIALIZED_PREFIX_LEN: usize = 1 + 32 + 24 + MAC_LEN; // version + salt + nonce + mac

pub struct Envelope {
    pub version: u8,
    pub salt: [u8; 32],
    pub nonce: [u8; 24],
    pub mac: [u8; MAC_LEN],
    pub ciphertext: Vec<u8>,
}

impl Envelope {
    // Canonical on-wire byte layout:
    //   [version: u8][salt: 32][nonce: 24][mac: 16][ciphertext: ..]
    // The MAC covers version || salt || nonce || ciphertext and is checked
    // before any length parsing on extract, so a wrong passphrase returns
    // AuthFailed cleanly instead of exploding on a garbage length field.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SERIALIZED_PREFIX_LEN + self.ciphertext.len());
        out.push(self.version);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.mac);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < SERIALIZED_PREFIX_LEN {
            return Err(CryptoError::InvalidData(format!(
                "envelope too short: {} bytes (need at least {})",
                data.len(),
                SERIALIZED_PREFIX_LEN
            )));
        }
        let version = data[0];
        if version != FORMAT_VERSION {
            return Err(CryptoError::UnsupportedVersion(version));
        }
        let mut salt = [0u8; 32];
        salt.copy_from_slice(&data[1..33]);
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&data[33..57]);
        let mut mac = [0u8; MAC_LEN];
        mac.copy_from_slice(&data[57..57 + MAC_LEN]);
        let ciphertext = data[SERIALIZED_PREFIX_LEN..].to_vec();
        Ok(Self {
            version,
            salt,
            nonce,
            mac,
            ciphertext,
        })
    }
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

    let master = Zeroizing::new(derive_key(passphrase, &salt, params));
    let subkeys = split_keys(&master);

    let meta_bytes = metadata.to_bytes();
    let mut combined = Vec::with_capacity(meta_bytes.len() + payload.len());
    combined.extend_from_slice(&meta_bytes);
    combined.extend_from_slice(payload);

    let padded = pad(&combined)?;

    let ciphertext = encrypt(&subkeys.aead_key, &nonce, &padded, b"");
    let mac = compute_mac(&subkeys.mac_key, FORMAT_VERSION, &salt, &nonce, &ciphertext);

    Ok(Envelope {
        version: FORMAT_VERSION,
        salt,
        nonce,
        mac,
        ciphertext,
    })
}

pub fn open(
    passphrase: &str,
    envelope: &Envelope,
    params: &KdfParams,
) -> Result<(PayloadMetadata, Vec<u8>)> {
    if envelope.version != FORMAT_VERSION {
        return Err(CryptoError::UnsupportedVersion(envelope.version));
    }

    let master = Zeroizing::new(derive_key(passphrase, &envelope.salt, params));
    let subkeys = split_keys(&master);

    // Fast-fail wrong-passphrase pre-check. Must run before any length or
    // metadata parsing so a bad key produces AuthFailed instead of a framing
    // error from garbage bytes.
    verify_mac(
        &subkeys.mac_key,
        envelope.version,
        &envelope.salt,
        &envelope.nonce,
        &envelope.ciphertext,
        &envelope.mac,
    )?;

    let padded = decrypt(
        &subkeys.aead_key,
        &envelope.nonce,
        &envelope.ciphertext,
        b"",
    )
    .map_err(|_| CryptoError::AuthFailed)?;

    let combined = unpad(&padded).map_err(|_| CryptoError::AuthFailed)?;

    let (metadata, consumed) =
        PayloadMetadata::from_bytes(&combined).map_err(|_| CryptoError::AuthFailed)?;

    let payload_bytes = &combined[consumed..];

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
        assert_eq!(envelope.version, FORMAT_VERSION);

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
        assert!(
            matches!(result, Err(CryptoError::AuthFailed)),
            "expected AuthFailed, got {:?}",
            result.err()
        );
    }

    #[test]
    fn wrong_passphrase_tampered_ciphertext_still_authfailed() {
        let payload = b"some bytes here";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("alpha", metadata, payload, &params).unwrap();
        envelope.ciphertext[0] ^= 0xaa;

        let result = open("beta", &envelope, &params);
        assert!(matches!(result, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn tampered_mac_rejected() {
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("passphrase", metadata, payload, &params).unwrap();
        envelope.mac[0] ^= 0xff;
        let result = open("passphrase", &envelope, &params);
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
    fn tampered_ciphertext_post_mac_caught_by_aead() {
        // If the MAC is (somehow) satisfied over a tampered ciphertext, the
        // AEAD layer must still catch the integrity violation. We simulate
        // this by flipping a ciphertext byte then recomputing the MAC using
        // the same derivation the legitimate sender would — this proves the
        // AEAD backstop is still load-bearing.
        let payload = b"secret message";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let mut envelope = seal("passphrase", metadata, payload, &params).unwrap();
        envelope.ciphertext[3] ^= 0x55;

        let master = Zeroizing::new(derive_key("passphrase", &envelope.salt, &params));
        let subkeys = split_keys(&master);
        envelope.mac = compute_mac(
            &subkeys.mac_key,
            envelope.version,
            &envelope.salt,
            &envelope.nonce,
            &envelope.ciphertext,
        );

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
    fn serialization_roundtrip() {
        let payload = b"serialize me";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let envelope = seal("passphrase", metadata.clone(), payload, &params).unwrap();
        let bytes = envelope.to_bytes();
        assert_eq!(bytes[0], FORMAT_VERSION);

        let parsed = Envelope::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, envelope.version);
        assert_eq!(parsed.salt, envelope.salt);
        assert_eq!(parsed.nonce, envelope.nonce);
        assert_eq!(parsed.mac, envelope.mac);
        assert_eq!(parsed.ciphertext, envelope.ciphertext);

        let (recovered_meta, recovered_payload) = open("passphrase", &parsed, &params).unwrap();
        assert_eq!(recovered_meta, metadata);
        assert_eq!(recovered_payload, payload);
    }

    #[test]
    fn from_bytes_rejects_wrong_version() {
        let payload = b"v1 file";
        let metadata = make_metadata(payload.len() as u64);
        let params = make_params();

        let envelope = seal("passphrase", metadata, payload, &params).unwrap();
        let mut bytes = envelope.to_bytes();
        bytes[0] = 1; // pretend this is a v1 envelope
        let result = Envelope::from_bytes(&bytes);
        assert!(matches!(result, Err(CryptoError::UnsupportedVersion(1))));
    }

    #[test]
    fn from_bytes_rejects_truncated() {
        let short = vec![0u8; SERIALIZED_PREFIX_LEN - 1];
        let result = Envelope::from_bytes(&short);
        assert!(matches!(result, Err(CryptoError::InvalidData(_))));
    }

    #[test]
    fn padding_stripping_after_open() {
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
