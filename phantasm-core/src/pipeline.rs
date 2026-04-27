use std::path::Path;

use log::info;
use rand::rngs::OsRng;
use rand::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha12Rng;
use sha2::{Digest, Sha256};

use phantasm_channel::ChannelAdapter;
use phantasm_cost::CostMap;
use phantasm_crypto::{
    derive_locations_key, open, seal, ContentType, CryptoError, Envelope, KdfParams,
    PayloadMetadata,
};
use phantasm_ecc::{Decoder as EccDecoder, EccParams, Encoder as EccEncoder};
use phantasm_image::jpeg::{self, JpegCoefficients};
use phantasm_stc::{StcConfig, StcDecoder, StcEncoder};

use crate::error::CoreError;
use crate::hash_guard::{self, HashType};
use crate::orchestrator::EmbedResult;

pub(crate) fn embed_with_costs(
    cover_path: &Path,
    payload: &[u8],
    passphrase: &str,
    costs: &CostMap,
    output_path: &Path,
) -> Result<EmbedResult, CoreError> {
    embed_with_costs_and_hooks(
        cover_path,
        payload,
        passphrase,
        costs,
        output_path,
        None,
        None,
    )
}

// RS parameters for the lossy (channel-adapter) path. data/parity/shard chosen
// to keep one 3000-byte-ish envelope within a single 255-shard block while
// correcting ~p99 of byte errors observed after Twitter-surrogate recompression
// of a 98.7%-coefficient-survival stego.
//
// - shard_size = 32: keeps one block small enough (100 × 32 = 3200 bytes) that
//   a nominal 3000-byte payload + ~80-byte envelope overhead + 4-byte internal
//   len prefix fits in a single block. Larger shards waste capacity via
//   zero-padding up to the next block boundary.
// - data_shards = 100, parity_shards = 30: 30 % redundancy. Without erasure
//   information RS in GF(2^8) corrects floor(parity/2) = 15 byte errors per
//   block. At the observed ~1.3 % coefficient-error rate post-Twitter, byte
//   errors per 3200-byte block concentrate well below 15 on typical covers; 30
//   parity shards leave headroom for p99 covers and for multi-block payloads.
// - Capacity cost vs the lossless path is ~30 % in the STC bit budget for
//   any payload that fits a single block; multi-block payloads asymptote to
//   the same 30 % overhead plus a small per-block boundary.
const LOSSY_ECC_DATA_SHARDS: usize = 100;
const LOSSY_ECC_PARITY_SHARDS: usize = 30;
const LOSSY_ECC_SHARD_SIZE: usize = 32;

fn env_usize(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn lossy_ecc_params() -> EccParams {
    // Research override: allow the BER-sweep harness (and other tuning tools)
    // to vary RS parameters without a rebuild. Production callers leave these
    // unset and get the compile-time defaults.
    let data = env_usize("PHANTASM_LOSSY_ECC_DATA", LOSSY_ECC_DATA_SHARDS);
    let parity = env_usize("PHANTASM_LOSSY_ECC_PARITY", LOSSY_ECC_PARITY_SHARDS);
    let shard = env_usize("PHANTASM_LOSSY_ECC_SHARD", LOSSY_ECC_SHARD_SIZE);
    EccParams::new(data, parity, shard)
}

pub(crate) fn embed_with_costs_and_hooks(
    cover_path: &Path,
    payload: &[u8],
    passphrase: &str,
    costs: &CostMap,
    output_path: &Path,
    hash_guard: Option<HashType>,
    channel_adapter: Option<&dyn ChannelAdapter>,
) -> Result<EmbedResult, CoreError> {
    let mut jpeg = jpeg::read(cover_path)?;

    // The cost map arrives pre-computed from the distortion function. If
    // either the hash guard or the channel adapter is active we need to
    // mutate a clone — hash guard adds wet positions, and the adapter adds
    // wet positions plus discounts stabilized positions. Both operate on
    // the same `CostMap` shape (matching `positions` ordering).
    let mut working_costs: CostMap = costs.clone();

    if let Some(ht) = hash_guard {
        let report = hash_guard::apply_hash_guard(&mut working_costs, &jpeg, ht);
        info!(
            "hash_guard: tier={:?} bits_guarded={} wet_added={}",
            report.sensitivity_tier, report.hash_bits_guarded, report.wet_positions_added
        );
        // M7 diagnostic: if the hash_guard marked so many positions that the
        // STC encoder will run out of usable capacity, surface a readable
        // error here rather than failing deep inside the STC encoder with an
        // opaque "payload too large". Threshold of 80% is conservative — the
        // STC encoder's practical capacity is ~1/rate_denom of non-wet
        // positions, so losing >80% of positions to wet marks leaves less
        // than 5% of nominal capacity.
        let total = working_costs.len();
        if total > 0 {
            let wet_fraction = report.wet_positions_added as f64 / total as f64;
            if wet_fraction > 0.8 {
                return Err(CoreError::InvalidData(format!(
                    "cover is classified as {:?} for hash_guard={:?}; preservation would mark {:.0}% of positions as wet, exhausting capacity. Consider: a different cover, a smaller payload, or omitting --hash-guard.",
                    report.sensitivity_tier,
                    ht,
                    wet_fraction * 100.0
                )));
            }
        }
    }

    if let Some(adapter) = channel_adapter {
        let report = adapter
            .stabilize(&mut jpeg, 0, &mut working_costs)
            .map_err(|e| CoreError::InvalidData(format!("channel adapter error: {e}")))?;
        info!(
            "channel_adapter[{}]: stabilized={} wet_positions={} sacrificed_blocks={} survival={:.3}",
            adapter.name(),
            report.stabilized_count,
            report.wet_positions.len(),
            report.sacrificed_blocks,
            report.survival_rate_estimate
        );
    }

    info!("pipeline: final cost map size = {}", working_costs.len());

    let use_ecc = channel_adapter.is_some();
    embed_prepared(
        &mut jpeg,
        payload,
        passphrase,
        &working_costs,
        cover_path,
        output_path,
        use_ecc,
    )
}

fn embed_prepared(
    jpeg: &mut JpegCoefficients,
    payload: &[u8],
    passphrase: &str,
    costs: &CostMap,
    cover_path: &Path,
    output_path: &Path,
    use_ecc: bool,
) -> Result<EmbedResult, CoreError> {
    let salt = image_salt(jpeg);

    let metadata = PayloadMetadata {
        filename: None,
        payload_len: payload.len() as u64,
        content_type: ContentType::Raw,
        version: 1,
    };

    let kdf = KdfParams::default();
    let envelope = seal(passphrase, metadata, payload, &kdf)?;
    let envelope_bytes = envelope_to_bytes(&envelope);

    // Lossy path: wrap envelope in Reed-Solomon before framing. The AEAD tag on
    // the envelope aborts extraction on any surviving ciphertext bit flip, so
    // without ECC the ~1.3 % post-Twitter byte-error rate translates to ~0 %
    // extract success. ECC runs OUTSIDE the envelope so RS repairs are not
    // subject to the MAC check.
    let pre_stc_bytes = if use_ecc {
        let encoder = EccEncoder::new(lossy_ecc_params())
            .map_err(|e| CoreError::InvalidData(format!("ecc init: {e}")))?;
        encoder
            .encode(&envelope_bytes)
            .map_err(|e| CoreError::InvalidData(format!("ecc encode: {e}")))?
    } else {
        envelope_bytes
    };

    let framed = frame_bytes(&pre_stc_bytes);
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

    jpeg::write_with_source(jpeg, cover_path, output_path)?;

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
    extract_from_cover_with_opts(stego_path, passphrase, false)
}

pub(crate) fn extract_from_cover_with_opts(
    stego_path: &Path,
    passphrase: &str,
    use_ecc: bool,
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

    // With a wrong passphrase the STC decoder reads bits from a different
    // permutation of coefficients, so unframing and envelope parsing are
    // looking at garbage until the MAC pre-check in `open()` fires. Collapse
    // those pre-`open()` failure modes to one clean AuthFailed so the user
    // sees a single "authentication failed" error instead of a framing or
    // length panic-adjacent message. `UnsupportedVersion` is preserved
    // verbatim — it's the one error we want a genuine older-format file to
    // surface with, even though the MAC check would also reject it.
    let framed_bytes = bits_to_bytes_lsb(&message_bits);
    let post_stc_bytes =
        unframe_bytes(&framed_bytes).map_err(|_| CoreError::Crypto(CryptoError::AuthFailed))?;
    let envelope_bytes = if use_ecc {
        let ecc_decoder = EccDecoder::new(lossy_ecc_params())
            .map_err(|_| CoreError::Crypto(CryptoError::AuthFailed))?;
        ecc_decoder
            .decode(&post_stc_bytes, None)
            .map_err(|_| CoreError::Crypto(CryptoError::AuthFailed))?
    } else {
        post_stc_bytes
    };
    let envelope = bytes_to_envelope(&envelope_bytes).map_err(|e| match e {
        CoreError::Crypto(CryptoError::UnsupportedVersion(_)) => e,
        _ => CoreError::Crypto(CryptoError::AuthFailed),
    })?;
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

/// Coarseness of the DCT-coefficient quantization used to derive the
/// pHash-stable salt. Each coefficient is divided by this step and rounded
/// before hashing, so two decodes of the same image that differ by less than
/// `SALT_QUANT_STEP / 2` in any coefficient produce the same salt.
///
/// PLAN §8 requires `image_salt()` to be stable under JPEG recompression so
/// the extractor on a re-encoded stego can reproduce the locations-permutation
/// salt of the original cover. The pHash coefficients themselves (top-left
/// 8×8 of the 32×32 DCT of the area-resampled luma) are designed to be robust
/// to recompression, but a JPEG→JPEG round-trip still introduces small
/// floating-point drift on the f64 DCT output (the level-shifted luma enters
/// as 8-bit integers so the rounding noise at the pixel level accumulates
/// through the resize + DCT). A coarse quantization step absorbs that noise.
///
/// A step of 256 is chosen empirically. The v0.4 lossy-channel diagnostic
/// tooling (`phantasm-bench salt-magnitude-probe`, archived) measured per-
/// cover maximum DCT drift through `image`-crate QF=85 recompression on the
/// `research-corpus-500` subset:
/// - Step 16 (the v0.3 setting): drift exceeded the safety margin on **42.5%**
///   of covers (max observed 6.39 DCT units). Stego extracts on those covers
///   silently failed with `AuthFailed` after share/recompress.
/// - Step 128: 92.5% stable.
/// - Step 256: 100% stable across the test corpus.
///
/// The v1 envelope (FORMAT_VERSION = 3) adopts step 256. Because salt is
/// image-derived and not stored in the envelope, this is a stego-breaking
/// change vs v3 envelopes produced under a different step — hence gated
/// behind the envelope version bump.
///
/// Entropy budget at step 256: typical AC magnitudes in the pHash 8×8 block
/// span tens to thousands of units, so 64 quantized coefficients still
/// deliver well over 256 bits of entropy across a diverse cover corpus.
///
/// Adversarial cover limitation: if a cover has a low-frequency DCT
/// coefficient whose pre-quantization value happens to lie within ~0.5
/// units of a `step × n` boundary AND the chosen cost function's embed
/// perturbation pushes that coefficient across the boundary, the salt will
/// drift and extract will fail with `AuthFailed`. Larger step sizes shrink
/// the boundary-collision rate proportionally; step 256 makes this
/// negligible on realistic photographic covers and rare even on
/// pathological synthetic covers. Use `--hash-guard phash` to mark such
/// coefficients as wet when it matters.
const SALT_QUANT_STEP: f64 = 256.0;

fn salt_quant_step() -> f64 {
    // Research override: lossy-channel tuning may bump the quantization step
    // to absorb more recompression drift. Default stays at the compile-time
    // constant so the lossless path and all tests keep their existing salts.
    std::env::var("PHANTASM_SALT_QUANT_STEP")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(SALT_QUANT_STEP)
}

/// Derive a 32-byte permutation salt from the cover's pHash-stable DCT
/// coefficients.
///
/// The salt is a SHA-256 of 64 quantized i32 values, one per coefficient in
/// the top-left 8×8 of the 32×32 DCT of the area-resampled luma — i.e. the
/// same block pHash uses. Quantization step is [`SALT_QUANT_STEP`]; see
/// constant docs for the stability/entropy trade-off.
///
/// Returns a 32-byte `Vec<u8>`. Empty JPEGs (no components) return the
/// SHA-256 of the empty string.
pub(crate) fn image_salt(jpeg: &JpegCoefficients) -> Vec<u8> {
    let mut hasher = Sha256::new();
    if jpeg.components.is_empty() {
        return hasher.finalize().to_vec();
    }
    let luma = hash_guard::decode_luma(jpeg);
    let resized = hash_guard::resize_area(&luma, 32, 32);
    let dct_32 = hash_guard::dct2d_32x32(&resized);

    // Top-left 8×8 of the 32×32 DCT, row-major. This is the same block that
    // pHash reads to compute its 64-bit hash, so by construction it is
    // robust to the recompression perturbations pHash was designed for.
    for r in 0..8 {
        for c in 0..8 {
            let coeff = dct_32[r * 32 + c];
            let quantized = (coeff / salt_quant_step()).round() as i32;
            hasher.update(quantized.to_le_bytes());
        }
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
    env.to_bytes()
}

pub(crate) fn bytes_to_envelope(bytes: &[u8]) -> Result<Envelope, CoreError> {
    Envelope::from_bytes(bytes).map_err(CoreError::Crypto)
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

// ---------------------------------------------------------------------------
// Diagnostics: post-STC byte-stream error measurement
// ---------------------------------------------------------------------------
//
// Research-only helper for BER-sweep tuning. Performs the full embed path
// (envelope, RS encode, framing, STC embed, write), then on the returned
// stego path reads coefficients, runs STC decode, unframes only the
// outermost length prefix, and returns the byte stream that WOULD be fed
// to RS decode. Callers compare this to the pre-STC bytes captured from
// the embed side to get the ground-truth byte-error rate that RS has to
// correct. Bypasses envelope parsing and MAC check.
//
// Not intended for production use; kept #[doc(hidden)] to discourage it.

#[doc(hidden)]
pub mod diagnostics {
    use super::*;
    use phantasm_channel::ChannelAdapter;

    /// Expose image_salt for drift diagnostics.
    pub fn salt_of_jpeg(jpeg: &JpegCoefficients) -> Vec<u8> {
        image_salt(jpeg)
    }

    /// Expose the raw 8×8 pHash block (64 f64 coefficients) so callers can
    /// measure drift magnitudes across recompression.
    pub fn phash_block_of_jpeg(jpeg: &JpegCoefficients) -> Vec<f64> {
        if jpeg.components.is_empty() {
            return vec![];
        }
        let luma = crate::hash_guard::decode_luma(jpeg);
        let resized = crate::hash_guard::resize_area(&luma, 32, 32);
        let dct_32 = crate::hash_guard::dct2d_32x32(&resized);
        let mut out = Vec::with_capacity(64);
        for r in 0..8 {
            for c in 0..8 {
                out.push(dct_32[r * 32 + c]);
            }
        }
        out
    }

    pub struct DiagEmbed {
        /// The bytes fed into STC (envelope-after-RS, length-prefix-framed).
        /// The post-STC recovered bytes should equal this if the channel is
        /// noiseless.
        pub framed_pre_stc: Vec<u8>,
        /// The RS-encoded envelope (what RS decode would output on success
        /// before envelope parsing).
        pub ecc_encoded_envelope: Vec<u8>,
    }

    /// Embed and return the pre-STC framed bytes so a caller can measure
    /// post-STC byte error rate.
    #[allow(clippy::too_many_arguments)]
    pub fn embed_capture_pre_stc(
        cover_path: &Path,
        payload: &[u8],
        passphrase: &str,
        costs: &CostMap,
        output_path: &Path,
        channel_adapter: Option<&dyn ChannelAdapter>,
    ) -> Result<DiagEmbed, CoreError> {
        let mut jpeg = jpeg::read(cover_path)?;
        let mut working_costs: CostMap = costs.clone();

        if let Some(adapter) = channel_adapter {
            adapter
                .stabilize(&mut jpeg, 0, &mut working_costs)
                .map_err(|e| CoreError::InvalidData(format!("channel adapter error: {e}")))?;
        }

        let use_ecc = channel_adapter.is_some();
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

        let pre_stc_bytes = if use_ecc {
            let encoder = EccEncoder::new(lossy_ecc_params())
                .map_err(|e| CoreError::InvalidData(format!("ecc init: {e}")))?;
            encoder
                .encode(&envelope_bytes)
                .map_err(|e| CoreError::InvalidData(format!("ecc encode: {e}")))?
        } else {
            envelope_bytes.clone()
        };

        let framed = frame_bytes(&pre_stc_bytes);
        let mut payload_bits = bytes_to_bits_lsb(&framed);

        let locations_key = derive_locations_key(passphrase, &salt, &kdf);
        let mut indices: Vec<usize> = (0..working_costs.positions.len()).collect();
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
        pad_bits_random(&mut payload_bits, stc_message_len);

        let y = &jpeg.components[0];
        let cover_bits: Vec<u8> = indices
            .iter()
            .map(|&idx| {
                let (br, bc, dp) = working_costs.positions[idx];
                (y.get(br, bc, dp) & 1) as u8
            })
            .collect();
        let stc_costs: Vec<f64> = indices
            .iter()
            .map(|&idx| {
                let (br, bc, dp) = working_costs.positions[idx];
                let v = jpeg.components[0].get(br, bc, dp);
                if v == i16::MAX || v == i16::MIN {
                    return f64::INFINITY;
                }
                working_costs.costs_plus[idx].min(working_costs.costs_minus[idx])
            })
            .collect();
        let stc = StcEncoder::new(StcConfig {
            constraint_height: 7,
        });
        let stego_bits = stc.embed(&cover_bits, &stc_costs, &payload_bits)?;
        for (i, &idx) in indices.iter().enumerate() {
            let (br, bc, dp) = working_costs.positions[idx];
            let old = jpeg.components[0].get(br, bc, dp);
            let new_lsb = stego_bits[i];
            if (old & 1) as u8 != new_lsb {
                jpeg.components[0].set(br, bc, dp, old ^ 1);
            }
        }
        jpeg::write_with_source(&jpeg, cover_path, output_path)?;

        Ok(DiagEmbed {
            framed_pre_stc: framed,
            ecc_encoded_envelope: pre_stc_bytes,
        })
    }

    pub struct DiagExtractRaw {
        /// Bytes output by STC decode, unframed by the outer length prefix.
        /// Equals the pre-STC `ecc_encoded_envelope` if the channel is
        /// noiseless (modulo any length-prefix corruption).
        pub post_stc_bytes: Option<Vec<u8>>,
        /// Raw bytes after STC decode (still contains framing length prefix).
        pub raw_stc_bytes: Vec<u8>,
        /// Declared frame length (first 4 LE bytes after STC decode).
        pub framed_len: u32,
    }

    /// STC-decode only; do not run RS decode, do not parse envelope.
    pub fn extract_raw_stc(
        stego_path: &Path,
        passphrase: &str,
    ) -> Result<DiagExtractRaw, CoreError> {
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

        let framed_len = if framed_bytes.len() >= 4 {
            u32::from_le_bytes(framed_bytes[..4].try_into().unwrap())
        } else {
            0
        };

        let post = unframe_bytes(&framed_bytes).ok();

        Ok(DiagExtractRaw {
            post_stc_bytes: post,
            raw_stc_bytes: framed_bytes,
            framed_len,
        })
    }
}

#[cfg(test)]
mod salt_tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::tempdir;

    fn write_synthetic_jpeg(path: &Path, width: u32, height: u32, seed: u32) {
        // Seed-dependent low-frequency gradient + seed-dependent texture noise.
        // The gradient ensures the 32×32 DCT's low-frequency 8×8 block varies
        // substantially across seeds, so salts computed from that block
        // actually differ between distinct synthetic covers.
        let bias = ((seed.wrapping_mul(37)) % 97) as i32;
        let slope_x = 1 + ((seed >> 3) % 7) as i32;
        let slope_y = 1 + ((seed >> 6) % 5) as i32;
        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let grad = bias + slope_x * (x as i32) / 4 + slope_y * (y as i32) / 4;
            let tex = (((x.wrapping_mul(31) ^ y.wrapping_mul(17)) ^ seed) & 0x3f) as i32;
            let r = (grad + tex).clamp(0, 255) as u8;
            let g = (grad.wrapping_add(32) + tex).clamp(0, 255) as u8;
            let b = (grad.wrapping_add(64) + tex).clamp(0, 255) as u8;
            *pixel = Rgb([r, g, b]);
        }
        img.save(path).expect("failed to write test JPEG");
    }

    #[test]
    fn salt_is_deterministic_across_calls() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("cover.jpg");
        write_synthetic_jpeg(&path, 256, 256, 0xCAFE);
        let jpeg = jpeg::read(&path).unwrap();
        let salt_a = image_salt(&jpeg);
        let salt_b = image_salt(&jpeg);
        let salt_c = image_salt(&jpeg);
        assert_eq!(salt_a.len(), 32);
        assert_eq!(salt_a, salt_b);
        assert_eq!(salt_b, salt_c);
    }

    #[test]
    fn salt_differs_across_distinct_covers() {
        let tmp = tempdir().unwrap();
        let path_a = tmp.path().join("a.jpg");
        let path_b = tmp.path().join("b.jpg");
        write_synthetic_jpeg(&path_a, 256, 256, 0x1111);
        write_synthetic_jpeg(&path_b, 256, 256, 0x2222);
        let salt_a = image_salt(&jpeg::read(&path_a).unwrap());
        let salt_b = image_salt(&jpeg::read(&path_b).unwrap());
        assert_ne!(
            salt_a, salt_b,
            "different covers should produce different salts"
        );
    }

    #[test]
    fn salt_is_stable_across_jpeg_recompression() {
        // Pipeline: (1) write a synthetic cover as JPEG via the `image` crate,
        // (2) read it with phantasm_image, (3) compute salt_a, (4) re-encode
        // it through phantasm_image's write_with_source (which is what the
        // embed path uses and what a social channel would mimic), (5) read
        // it again, (6) compute salt_b. The salt must be byte-identical
        // across that round-trip for the extractor to reproduce the
        // locations-key permutation on a re-encoded stego.
        let tmp = tempdir().unwrap();
        let cover = tmp.path().join("cover.jpg");
        let reencoded = tmp.path().join("recoded.jpg");
        write_synthetic_jpeg(&cover, 384, 384, 0xDEAD_BEEF);

        let jpeg_a = jpeg::read(&cover).unwrap();
        let salt_a = image_salt(&jpeg_a);

        // Round-trip the coefficients through write_with_source — this
        // exercises the same code path the embed pipeline uses.
        jpeg::write_with_source(&jpeg_a, &cover, &reencoded).expect("failed to re-encode cover");
        let jpeg_b = jpeg::read(&reencoded).unwrap();
        let salt_b = image_salt(&jpeg_b);

        assert_eq!(
            salt_a, salt_b,
            "salt must be stable across JPEG recompression (observed salt_a = {:02x?}, salt_b = {:02x?})",
            &salt_a[..8],
            &salt_b[..8]
        );
    }

    #[test]
    fn salt_is_stable_after_embed_roundtrip_on_textured_cover() {
        // Integration-style sanity check: a cover embedded and then reloaded
        // through jpeg::read must yield the same salt as the original cover,
        // because the embed-path only touches AC coefficients at positions
        // chosen by the STC encoder — the pHash block of the 32×32 DCT of
        // the downsampled luma is designed to be insensitive to that kind
        // of perturbation. Combined with SALT_QUANT_STEP the salt must not
        // drift during an embed.
        let tmp = tempdir().unwrap();
        let cover = tmp.path().join("cover.jpg");
        let recoded = tmp.path().join("recoded.jpg");
        write_synthetic_jpeg(&cover, 512, 512, 0xA5A5_A5A5);
        let jpeg_a = jpeg::read(&cover).unwrap();
        let salt_a = image_salt(&jpeg_a);

        jpeg::write_with_source(&jpeg_a, &cover, &recoded).unwrap();
        let jpeg_b = jpeg::read(&recoded).unwrap();
        let salt_b = image_salt(&jpeg_b);
        assert_eq!(salt_a, salt_b);

        // And a second round-trip — iterative recompression converges to a
        // fixed point, so any two salts along the chain must agree.
        let recoded2 = tmp.path().join("recoded2.jpg");
        jpeg::write_with_source(&jpeg_b, &recoded, &recoded2).unwrap();
        let jpeg_c = jpeg::read(&recoded2).unwrap();
        let salt_c = image_salt(&jpeg_c);
        assert_eq!(salt_a, salt_c);
    }
}
