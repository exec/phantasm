use std::path::Path;

use rand::rngs::OsRng;
use rand::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha12Rng;
use sha2::{Digest, Sha256};

use phantasm_cost::CostMap;
use phantasm_crypto::{
    derive_locations_key, open, seal, ContentType, Envelope, KdfParams, PayloadMetadata,
};
use phantasm_image::jpeg::{self, JpegCoefficients};
use phantasm_stc::{StcConfig, StcDecoder, StcEncoder};

use crate::error::CoreError;
use crate::orchestrator::EmbedResult;

pub(crate) fn embed_with_costs(
    cover_path: &Path,
    payload: &[u8],
    passphrase: &str,
    costs: &CostMap,
    output_path: &Path,
) -> Result<EmbedResult, CoreError> {
    let mut jpeg = jpeg::read(cover_path)?;

    let salt = image_salt(&jpeg);

    let metadata = PayloadMetadata {
        filename: None,
        payload_len: payload.len() as u64,
        content_type: ContentType::Raw,
        version: 1,
    };

    let kdf = KdfParams::default();
    let envelope = seal(passphrase, metadata, payload, &kdf)?;
    let envelope_bytes = envelope_to_bytes(&envelope);

    let framed = frame_bytes(&envelope_bytes);
    let mut payload_bits = bytes_to_bits_lsb(&framed);

    let locations_key = derive_locations_key(passphrase, &salt, &kdf);

    // Build permuted index over cost map positions.
    let mut indices: Vec<usize> = (0..costs.positions.len()).collect();
    permute_indices(&mut indices, &locations_key);

    let rate_denom = 4usize;
    let stc_message_len = indices.len() / rate_denom;
    let trimmed_count = stc_message_len * rate_denom;
    indices.truncate(trimmed_count);

    if payload_bits.len() > stc_message_len {
        return Err(CoreError::PayloadTooLarge {
            size: payload_bits.len(),
            capacity: stc_message_len,
        });
    }

    let capacity_used_ratio = payload_bits.len() as f64 / stc_message_len as f64;
    pad_bits_random(&mut payload_bits, stc_message_len);

    // Y component is index 0.
    let y = &jpeg.components[0];
    let cover_bits: Vec<u8> = indices
        .iter()
        .map(|&idx| {
            let (br, bc, dp) = costs.positions[idx];
            (y.get(br, bc, dp) & 1) as u8
        })
        .collect();

    // For binary STC use min(costs_plus, costs_minus); also mark saturated coefficients wet.
    let stc_costs: Vec<f64> = indices
        .iter()
        .map(|&idx| {
            let (br, bc, dp) = costs.positions[idx];
            let v = jpeg.components[0].get(br, bc, dp);
            if v == i16::MAX || v == i16::MIN {
                return f64::INFINITY;
            }
            costs.costs_plus[idx].min(costs.costs_minus[idx])
        })
        .collect();

    let stc = StcEncoder::new(StcConfig {
        constraint_height: 7,
    });
    let stego_bits = stc.embed(&cover_bits, &stc_costs, &payload_bits)?;

    for (i, &idx) in indices.iter().enumerate() {
        let (br, bc, dp) = costs.positions[idx];
        let old = jpeg.components[0].get(br, bc, dp);
        let new_lsb = stego_bits[i];
        if (old & 1) as u8 != new_lsb {
            jpeg.components[0].set(br, bc, dp, old ^ 1);
        }
    }

    jpeg::write_with_source(&jpeg, cover_path, output_path)?;

    Ok(EmbedResult {
        bytes_embedded: payload.len(),
        capacity_used_ratio,
        estimated_detection_error: 0.5,
    })
}

pub(crate) fn extract_from_cover(
    stego_path: &Path,
    passphrase: &str,
) -> Result<Vec<u8>, CoreError> {
    let jpeg = jpeg::read(stego_path)?;
    let salt = image_salt(&jpeg);

    let kdf = KdfParams::default();
    let locations_key = derive_locations_key(passphrase, &salt, &kdf);

    let mut positions = usable_positions(&jpeg);
    permute_positions(&mut positions, &locations_key);

    let rate_denom = 4usize;
    let stc_message_len = positions.len() / rate_denom;
    let trimmed_count = stc_message_len * rate_denom;
    positions.truncate(trimmed_count);

    let y = &jpeg.components[0];
    let stego_bits: Vec<u8> = positions
        .iter()
        .map(|&(br, bc, dp)| (y.get(br, bc, dp) & 1) as u8)
        .collect();

    let decoder = StcDecoder::new(StcConfig {
        constraint_height: 7,
    });
    let message_bits = decoder.extract(&stego_bits, stc_message_len);

    let framed_bytes = bits_to_bytes_lsb(&message_bits);
    let envelope_bytes = unframe_bytes(&framed_bytes)?;

    let envelope = bytes_to_envelope(&envelope_bytes)?;
    let (_metadata, payload) = open(passphrase, &envelope, &kdf)?;

    Ok(payload)
}

// ---------------------------------------------------------------------------
// Shared internal helpers
// ---------------------------------------------------------------------------

pub(crate) fn usable_positions(jpeg: &JpegCoefficients) -> Vec<(usize, usize, usize)> {
    if jpeg.components.is_empty() {
        return vec![];
    }
    let y = &jpeg.components[0];
    let mut positions = Vec::with_capacity(y.blocks_wide * y.blocks_high * 63);
    for br in 0..y.blocks_high {
        for bc in 0..y.blocks_wide {
            for dp in 1..64 {
                positions.push((br, bc, dp));
            }
        }
    }
    positions
}

pub(crate) fn image_salt(jpeg: &JpegCoefficients) -> Vec<u8> {
    let mut hasher = Sha256::new();
    if jpeg.components.is_empty() {
        return hasher.finalize().to_vec();
    }
    let y = &jpeg.components[0];
    let num_blocks = y.blocks_wide * y.blocks_high;
    let sample_blocks = num_blocks.min(64);
    for block_idx in 0..sample_blocks {
        let br = block_idx / y.blocks_wide;
        let bc = block_idx % y.blocks_wide;
        let dc = y.get(br, bc, 0);
        hasher.update(dc.to_le_bytes());
    }
    hasher.finalize().to_vec()
}

fn permute_indices(indices: &mut [usize], key: &[u8; 32]) {
    let mut rng = ChaCha12Rng::from_seed(*key);
    let n = indices.len();
    for i in (1..n).rev() {
        let j = (rng.next_u64() as usize) % (i + 1);
        indices.swap(i, j);
    }
}

pub(crate) fn permute_positions(positions: &mut [(usize, usize, usize)], key: &[u8; 32]) {
    let mut rng = ChaCha12Rng::from_seed(*key);
    let n = positions.len();
    for i in (1..n).rev() {
        let j = (rng.next_u64() as usize) % (i + 1);
        positions.swap(i, j);
    }
}

pub(crate) fn bytes_to_bits_lsb(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for i in 0..8 {
            bits.push((b >> i) & 1);
        }
    }
    bits
}

pub(crate) fn bits_to_bytes_lsb(bits: &[u8]) -> Vec<u8> {
    let num_bytes = bits.len() / 8;
    let mut bytes = vec![0u8; num_bytes];
    for (i, &bit) in bits.iter().enumerate().take(num_bytes * 8) {
        bytes[i / 8] |= bit << (i % 8);
    }
    bytes
}

pub(crate) fn envelope_to_bytes(env: &Envelope) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + 24 + env.ciphertext.len());
    out.extend_from_slice(&env.salt);
    out.extend_from_slice(&env.nonce);
    out.extend_from_slice(&env.ciphertext);
    out
}

pub(crate) fn bytes_to_envelope(bytes: &[u8]) -> Result<Envelope, CoreError> {
    if bytes.len() < 56 {
        return Err(CoreError::InvalidData(format!(
            "envelope too short: {} bytes",
            bytes.len()
        )));
    }
    let salt: [u8; 32] = bytes[..32].try_into().unwrap();
    let nonce: [u8; 24] = bytes[32..56].try_into().unwrap();
    let ciphertext = bytes[56..].to_vec();
    Ok(Envelope {
        salt,
        nonce,
        ciphertext,
    })
}

pub(crate) fn frame_bytes(data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub(crate) fn unframe_bytes(data: &[u8]) -> Result<Vec<u8>, CoreError> {
    if data.len() < 4 {
        return Err(CoreError::InvalidData(format!(
            "framed data too short: {} bytes",
            data.len()
        )));
    }
    let len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    if 4 + len > data.len() {
        return Err(CoreError::InvalidData(format!(
            "declared length {} exceeds available {} bytes",
            len,
            data.len() - 4
        )));
    }
    Ok(data[4..4 + len].to_vec())
}

pub(crate) fn pad_bits_random(bits: &mut Vec<u8>, target_len: usize) {
    if bits.len() >= target_len {
        return;
    }
    let needed_bits = target_len - bits.len();
    let needed_bytes = needed_bits.div_ceil(8);
    let mut buf = vec![0u8; needed_bytes];
    OsRng.fill_bytes(&mut buf);
    for b in buf {
        for i in 0..8 {
            if bits.len() >= target_len {
                break;
            }
            bits.push((b >> i) & 1);
        }
    }
}
