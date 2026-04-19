//! Spatial-domain embed/extract pipeline for PNG covers.
//!
//! Mirrors [`crate::pipeline`] but operates on an 8-bit grayscale pixel buffer
//! instead of JPEG DCT coefficients. STC is still the underlying coder — it
//! just runs over pixel LSBs rather than coefficient LSBs.
//!
//! # MVP limitations
//!
//! - **Grayscale only.** RGB PNG covers are flattened to luma on read
//!   (`phantasm_image::png::read_png_pixels`). The embedded stego is written
//!   as an 8-bit grayscale PNG. Color preservation is a v0.3 follow-up.
//! - **Passphrase-only salt.** The locations-permutation salt is derived from
//!   the passphrase plus the cover dimensions, NOT from a perceptual hash of
//!   the pixels. A PNG stego round-tripped through a lossy channel would not
//!   recover. This is acceptable for lossless channels; a pHash-stable
//!   spatial salt is a v0.3 follow-up (see `hash_guard` for the JPEG side).
//! - **Only `Uniform` and `SUniward` costs are supported.** Other cost
//!   functions are DCT-domain and cannot run on pixels.

use std::path::Path;

use log::info;
use sha2::{Digest, Sha256};

use phantasm_cost::{CostMap, SUniward};
use phantasm_crypto::{
    derive_locations_key, open, seal, ContentType, CryptoError, KdfParams, PayloadMetadata,
};
use phantasm_image::png::{self, PngPixels};
use phantasm_stc::{StcConfig, StcDecoder, StcEncoder};

use crate::error::CoreError;
use crate::orchestrator::EmbedResult;
use crate::pipeline::{
    bits_to_bytes_lsb, bytes_to_bits_lsb, bytes_to_envelope, envelope_to_bytes, frame_bytes,
    pad_bits_random, permute_positions, unframe_bytes,
};

/// MVP cost-function selector for PNG covers.
#[derive(Debug, Clone, Copy)]
pub enum SpatialCost {
    /// Uniform cost 1.0 at every pixel. Content-non-adaptive; a sanity fallback.
    Uniform,
    /// S-UNIWARD (Holub-Fridrich 2014) spatial cost.
    SUniward,
}

/// MVP passphrase-only salt. The JPEG pipeline uses a pHash-stable salt to
/// survive lossy channels; the spatial MVP has no comparable perceptual hash
/// wired up, so we key solely off the passphrase + cover dimensions. A stego
/// round-tripped through a lossless channel will extract; a lossy channel
/// will not. This is documented in the module doc and is a v0.3 follow-up.
pub(crate) fn spatial_salt(pixels: &PngPixels) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"phantasm-png-spatial-v1");
    hasher.update(pixels.width.to_le_bytes());
    hasher.update(pixels.height.to_le_bytes());
    hasher.finalize().to_vec()
}

fn build_uniform_costs(pixels: &PngPixels) -> CostMap {
    let w = pixels.width as usize;
    let h = pixels.height as usize;
    let n = w * h;
    let mut positions = Vec::with_capacity(n);
    let mut costs_plus = Vec::with_capacity(n);
    let mut costs_minus = Vec::with_capacity(n);
    for r in 0..h {
        for c in 0..w {
            let p = pixels.pixels[r * w + c];
            positions.push((r, c, 0));
            costs_plus.push(if p == 255 { f64::INFINITY } else { 1.0 });
            costs_minus.push(if p == 0 { f64::INFINITY } else { 1.0 });
        }
    }
    CostMap {
        costs_plus,
        costs_minus,
        positions,
    }
}

pub fn embed_png(
    cover_path: &Path,
    payload: &[u8],
    passphrase: &str,
    cost: SpatialCost,
    output_path: &Path,
) -> Result<EmbedResult, CoreError> {
    let mut pixels = png::read_png_pixels(cover_path)?;

    let costs: CostMap = match cost {
        SpatialCost::Uniform => build_uniform_costs(&pixels),
        SpatialCost::SUniward => SUniward::new().compute_pixels(&pixels),
    };

    info!(
        "spatial pipeline: cost_fn={} positions={} dims={}x{}",
        match cost {
            SpatialCost::Uniform => "uniform",
            SpatialCost::SUniward => "s-uniward",
        },
        costs.len(),
        pixels.width,
        pixels.height,
    );

    let salt = spatial_salt(&pixels);

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

    let mut positions: Vec<(usize, usize, usize)> = costs.positions.clone();
    permute_positions(&mut positions, &locations_key);

    let rate_denom = 4usize;
    let stc_message_len = positions.len() / rate_denom;
    let trimmed_count = stc_message_len * rate_denom;
    positions.truncate(trimmed_count);

    if payload_bits.len() > stc_message_len {
        return Err(CoreError::PayloadTooLarge {
            size: payload_bits.len(),
            capacity: stc_message_len,
        });
    }

    let capacity_used_ratio = payload_bits.len() as f64 / stc_message_len as f64;
    pad_bits_random(&mut payload_bits, stc_message_len);

    // Cost lookup needs a `position -> cost` map. The cost map's positions
    // vector is in the canonical row-major order used by `build_uniform_costs`
    // / `SUniward::compute_pixels` (both emit `(r, c, 0)` row-major), so we
    // can compute the original index directly without a hashmap.
    let w = pixels.width as usize;
    let position_to_cost = |r: usize, c: usize| -> f64 {
        let idx = r * w + c;
        // Binary STC uses min(+, −) as the per-bit cost; saturated pixels are
        // already marked INFINITY on the overflow side.
        let cp = costs.costs_plus[idx];
        let cm = costs.costs_minus[idx];
        cp.min(cm)
    };

    let cover_bits: Vec<u8> = positions
        .iter()
        .map(|&(r, c, _)| pixels.pixels[r * w + c] & 1)
        .collect();

    let stc_costs: Vec<f64> = positions
        .iter()
        .map(|&(r, c, _)| position_to_cost(r, c))
        .collect();

    let stc = StcEncoder::new(StcConfig {
        constraint_height: 7,
    });
    let stego_bits = stc.embed(&cover_bits, &stc_costs, &payload_bits)?;

    for (i, &(r, c, _)) in positions.iter().enumerate() {
        let idx = r * w + c;
        let old = pixels.pixels[idx];
        let new_lsb = stego_bits[i];
        if (old & 1) != new_lsb {
            // Flip LSB, clamping via wrapping XOR with 1 is safe (two's complement).
            pixels.pixels[idx] = old ^ 1;
        }
    }

    png::write_png_pixels(output_path, &pixels)?;

    Ok(EmbedResult {
        bytes_embedded: payload.len(),
        capacity_used_ratio,
        estimated_detection_error: 0.5,
    })
}

pub fn extract_png(stego_path: &Path, passphrase: &str) -> Result<Vec<u8>, CoreError> {
    let pixels = png::read_png_pixels(stego_path)?;
    let salt = spatial_salt(&pixels);
    let kdf = KdfParams::default();
    let locations_key = derive_locations_key(passphrase, &salt, &kdf);

    let w = pixels.width as usize;
    let h = pixels.height as usize;
    let mut positions: Vec<(usize, usize, usize)> = Vec::with_capacity(w * h);
    for r in 0..h {
        for c in 0..w {
            positions.push((r, c, 0));
        }
    }
    permute_positions(&mut positions, &locations_key);

    let rate_denom = 4usize;
    let stc_message_len = positions.len() / rate_denom;
    let trimmed_count = stc_message_len * rate_denom;
    positions.truncate(trimmed_count);

    let stego_bits: Vec<u8> = positions
        .iter()
        .map(|&(r, c, _)| pixels.pixels[r * w + c] & 1)
        .collect();

    let decoder = StcDecoder::new(StcConfig {
        constraint_height: 7,
    });
    let message_bits = decoder.extract(&stego_bits, stc_message_len);

    let framed_bytes = bits_to_bytes_lsb(&message_bits);
    let envelope_bytes =
        unframe_bytes(&framed_bytes).map_err(|_| CoreError::Crypto(CryptoError::AuthFailed))?;
    let envelope = bytes_to_envelope(&envelope_bytes).map_err(|e| match e {
        CoreError::Crypto(CryptoError::UnsupportedVersion(_)) => e,
        _ => CoreError::Crypto(CryptoError::AuthFailed),
    })?;
    let (_metadata, payload) = open(passphrase, &envelope, &kdf)?;
    Ok(payload)
}
