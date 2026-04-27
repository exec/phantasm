//! Research-only "raw" embedding path.
//!
//! **BENCHMARKING ONLY — NOT FOR PRODUCTION USE.**
//!
//! This module exposes a variant of the embedding pipeline that bypasses the
//! crypto envelope, ECC, and fixed-tier padding. It takes an exact target
//! message-bit count, generates a deterministic pseudo-random payload from a
//! caller-supplied `u64` seed, and drives STC directly over the selected
//! coefficient cost map.
//!
//! It exists for a single purpose: measuring the security–capacity curve of a
//! distortion function (Uniform, UERD, J-UNIWARD, …) across a sweep of payload
//! sizes. The naive pipeline pads every payload to one of a handful of tier
//! sizes, so all points on a density sweep collapse to the same STC rate and
//! the curve is constant. `research_raw_embed` avoids that by letting the
//! caller pick the exact number of message bits.
//!
//! **Security caveats** — this path produces stego files with:
//!
//! - No authenticity (no HMAC / AEAD) — an attacker who knows the seed can
//!   trivially forge or mutate the embedded bits.
//! - No confidentiality (random bits are not "plaintext" but there is no
//!   encryption of caller data either; the caller doesn't supply data).
//! - No error correction — a single bitflip in the wrong position corrupts
//!   the recovered message.
//!
//! These properties are fine for measuring detectability on a research corpus
//! under an adversary that does not know the seed; they are not fine for any
//! real-world steganographic channel. This module is therefore deliberately
//! marked `#[doc(hidden)]` and is not wired into the `phantasm` CLI.

#![doc(hidden)]

use phantasm_cost::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;
use phantasm_stc::{StcConfig, StcDecoder, StcEncoder};

use crate::error::CoreError;

/// Result of a research-raw embedding operation.
pub struct ResearchRawResult {
    /// The stego coefficient block (deep-cloned from the cover).
    pub stego: JpegCoefficients,
    /// The exact random message bits that were embedded (length
    /// = `target_message_bits`). `research_raw_extract` with the same seed
    /// and target bit count should return a bitstring equal to this.
    pub message_bits: Vec<bool>,
    /// The STC rate actually used for this embed, expressed as
    /// `target_message_bits / stc_cover_bits`, where `stc_cover_bits` is the
    /// (possibly truncated) number of cover coefficients fed to the STC
    /// encoder. Between 0 and 1.
    pub stc_rate: f64,
    /// Number of coefficients that were flipped relative to the cover.
    pub modifications: usize,
}

/// Embed `target_message_bits` random bits (deterministically seeded from
/// `seed`) into `cover` using `cost_fn` for the per-coefficient cost map.
///
/// **Research only; not for production use.** See the module-level docs.
///
/// The deterministic PRNG is SplitMix64; callers sweeping a parameter space
/// should pick distinct seeds per (image, density) point or accept that
/// replication is exact.
///
/// # Errors
///
/// - Returns [`CoreError::InvalidData`] if the cover has no usable
///   coefficients or `target_message_bits == 0`.
/// - Returns [`CoreError::PayloadTooLarge`] if `target_message_bits` exceeds
///   the number of usable coefficients in the cost map (STC cannot achieve a
///   rate above 1).
/// - Propagates [`CoreError::Stc`] if the STC encoder fails internally
///   (e.g. infeasible wet-paper constraints).
pub fn research_raw_embed(
    cover: &JpegCoefficients,
    cost_fn: &dyn DistortionFunction,
    target_message_bits: usize,
    seed: u64,
) -> Result<ResearchRawResult, CoreError> {
    let costs = cost_fn.compute(cover, 0);
    let (stc_cover_bits, indices) = stc_layout(&costs, target_message_bits)?;

    let cover_bits: Vec<u8> = indices
        .iter()
        .map(|&idx| {
            let (br, bc, dp) = costs.positions[idx];
            (cover.components[0].get(br, bc, dp) & 1) as u8
        })
        .collect();

    let stc_costs: Vec<f64> = indices
        .iter()
        .map(|&idx| {
            let (br, bc, dp) = costs.positions[idx];
            let v = cover.components[0].get(br, bc, dp);
            if v == i16::MAX || v == i16::MIN {
                return f64::INFINITY;
            }
            costs.costs_plus[idx].min(costs.costs_minus[idx])
        })
        .collect();

    let message_bits = generate_message_bits(seed, target_message_bits);
    let message_u8: Vec<u8> = message_bits.iter().map(|&b| b as u8).collect();

    let stc = StcEncoder::new(StcConfig {
        constraint_height: 7,
    });
    let stego_bits = stc.embed(&cover_bits, &stc_costs, &message_u8)?;

    let mut stego = clone_coefficients(cover);
    let mut modifications = 0usize;
    for (i, &idx) in indices.iter().enumerate() {
        let (br, bc, dp) = costs.positions[idx];
        let old = stego.components[0].get(br, bc, dp);
        let new_lsb = stego_bits[i];
        if (old & 1) as u8 != new_lsb {
            stego.components[0].set(br, bc, dp, old ^ 1);
            modifications += 1;
        }
    }

    let stc_rate = target_message_bits as f64 / stc_cover_bits as f64;

    Ok(ResearchRawResult {
        stego,
        message_bits,
        stc_rate,
        modifications,
    })
}

/// Recover the random message bits from a `research_raw_embed` stego image.
///
/// The caller must pass the same `cost_fn`, `target_message_bits`, and `seed`
/// used at embed time — the cost function determines the positional ordering,
/// and the seed is not actually consumed here but is accepted for symmetry so
/// callers have one obvious round-trip signature.
///
/// **Research only; not for production use.**
pub fn research_raw_extract(
    stego: &JpegCoefficients,
    cost_fn: &dyn DistortionFunction,
    target_message_bits: usize,
    _seed: u64,
) -> Result<Vec<bool>, CoreError> {
    let costs = cost_fn.compute(stego, 0);
    let (_stc_cover_bits, indices) = stc_layout(&costs, target_message_bits)?;

    let stego_bits: Vec<u8> = indices
        .iter()
        .map(|&idx| {
            let (br, bc, dp) = costs.positions[idx];
            (stego.components[0].get(br, bc, dp) & 1) as u8
        })
        .collect();

    let decoder = StcDecoder::new(StcConfig {
        constraint_height: 7,
    });
    let bits = decoder.extract(&stego_bits, target_message_bits);
    Ok(bits.into_iter().map(|b| b != 0).collect())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Compute the STC layout for a given target bit count and cost map.
///
/// STC requires `n` (cover bits) to be a positive multiple of `m` (message
/// bits). We set `w = floor(capacity / m)` and truncate the cover to
/// `n = w * m`. `w` must be at least 1, so `m <= capacity`. The selected
/// indices are the first `n` entries of the cost map's `positions`, keeping
/// them deterministic for a fixed cover + cost function.
fn stc_layout(
    costs: &CostMap,
    target_message_bits: usize,
) -> Result<(usize, Vec<usize>), CoreError> {
    if costs.positions.is_empty() {
        return Err(CoreError::InvalidData(
            "research_raw: cost map is empty".to_string(),
        ));
    }
    if target_message_bits == 0 {
        return Err(CoreError::InvalidData(
            "research_raw: target_message_bits must be > 0".to_string(),
        ));
    }
    let capacity = costs.positions.len();
    if target_message_bits > capacity {
        return Err(CoreError::PayloadTooLarge {
            size: target_message_bits,
            capacity,
        });
    }
    let w = capacity / target_message_bits;
    let n = w * target_message_bits;
    let indices: Vec<usize> = (0..n).collect();
    Ok((n, indices))
}

/// Deterministically generate `n` bits from `seed` using SplitMix64.
fn generate_message_bits(seed: u64, n: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(n);
    let mut state = seed;
    let mut buffered: u64 = 0;
    let mut buffered_count: u32 = 0;
    while bits.len() < n {
        if buffered_count == 0 {
            state = splitmix64(state);
            buffered = state;
            buffered_count = 64;
        }
        bits.push((buffered & 1) == 1);
        buffered >>= 1;
        buffered_count -= 1;
    }
    bits
}

#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn clone_coefficients(src: &JpegCoefficients) -> JpegCoefficients {
    use phantasm_image::jpeg::{JpegComponent, JpegMarker};
    JpegCoefficients {
        components: src
            .components
            .iter()
            .map(|c| JpegComponent {
                id: c.id,
                blocks_wide: c.blocks_wide,
                blocks_high: c.blocks_high,
                coefficients: c.coefficients.clone(),
                quant_table: c.quant_table,
                h_samp_factor: c.h_samp_factor,
                v_samp_factor: c.v_samp_factor,
            })
            .collect(),
        width: src.width,
        height: src.height,
        quality_estimate: src.quality_estimate,
        markers: src
            .markers
            .iter()
            .map(|m| JpegMarker {
                marker: m.marker,
                data: m.data.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use phantasm_cost::{Juniward, Uniform};
    use phantasm_image::jpeg;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn make_test_jpeg(path: &PathBuf, width: u32, height: u32) {
        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let r = ((x * 255 / width) as u8).wrapping_add(y as u8);
            let g = ((y * 255 / height) as u8).wrapping_add(x as u8);
            let b = ((x + y) as u8).wrapping_mul(3);
            *pixel = Rgb([r, g, b]);
        }
        img.save(path).expect("failed to write test JPEG");
    }

    fn load_test_cover(dims: (u32, u32)) -> JpegCoefficients {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("cover.jpg");
        make_test_jpeg(&p, dims.0, dims.1);
        jpeg::read(&p).expect("read jpeg")
    }

    #[test]
    fn round_trip_uniform_100_bits() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Uniform;
        let seed = 0xC0FFEE_u64;
        let res = research_raw_embed(&cover, &cost_fn, 100, seed).expect("embed");
        assert_eq!(res.message_bits.len(), 100);
        let recovered = research_raw_extract(&res.stego, &cost_fn, 100, seed).expect("extract");
        assert_eq!(recovered, res.message_bits);
    }

    #[test]
    fn round_trip_uniform_1000_bits() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Uniform;
        let seed = 42;
        let res = research_raw_embed(&cover, &cost_fn, 1000, seed).expect("embed");
        assert_eq!(res.message_bits.len(), 1000);
        let recovered = research_raw_extract(&res.stego, &cost_fn, 1000, seed).expect("extract");
        assert_eq!(recovered, res.message_bits);
    }

    #[test]
    fn round_trip_uniform_10000_bits() {
        let cover = load_test_cover((256, 256));
        let cost_fn = Uniform;
        let seed = 1;
        let res = research_raw_embed(&cover, &cost_fn, 10_000, seed).expect("embed");
        assert_eq!(res.message_bits.len(), 10_000);
        let recovered = research_raw_extract(&res.stego, &cost_fn, 10_000, seed).expect("extract");
        assert_eq!(recovered, res.message_bits);
    }

    #[test]
    fn round_trip_juniward_1000_bits() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Juniward;
        let seed = 7;
        let res = research_raw_embed(&cover, &cost_fn, 1000, seed).expect("embed");
        assert_eq!(res.message_bits.len(), 1000);
        let recovered = research_raw_extract(&res.stego, &cost_fn, 1000, seed).expect("extract");
        assert_eq!(recovered, res.message_bits);
    }

    #[test]
    fn unachievable_rate_errors() {
        let cover = load_test_cover((64, 64));
        let cost_fn = Uniform;
        // 64x64 JPEG has 8x8 blocks = 64 blocks, each 63 non-DC = 4032 positions.
        // Pick something well above capacity.
        let res = research_raw_embed(&cover, &cost_fn, 1_000_000, 0);
        assert!(matches!(res, Err(CoreError::PayloadTooLarge { .. })));
    }

    #[test]
    fn zero_target_bits_errors() {
        let cover = load_test_cover((64, 64));
        let cost_fn = Uniform;
        let res = research_raw_embed(&cover, &cost_fn, 0, 0);
        assert!(matches!(res, Err(CoreError::InvalidData(_))));
    }

    #[test]
    fn deterministic_same_seed() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Juniward;
        let a = research_raw_embed(&cover, &cost_fn, 500, 99).expect("embed a");
        let b = research_raw_embed(&cover, &cost_fn, 500, 99).expect("embed b");
        assert_eq!(a.message_bits, b.message_bits);
        assert_eq!(a.modifications, b.modifications);
        assert_eq!(
            a.stego.components[0].coefficients,
            b.stego.components[0].coefficients
        );
    }

    #[test]
    fn different_seeds_differ() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Uniform;
        let a = research_raw_embed(&cover, &cost_fn, 500, 1).expect("a");
        let b = research_raw_embed(&cover, &cost_fn, 500, 2).expect("b");
        assert_ne!(a.message_bits, b.message_bits);
    }

    #[test]
    fn stc_rate_reported() {
        let cover = load_test_cover((128, 128));
        let cost_fn = Uniform;
        let res = research_raw_embed(&cover, &cost_fn, 1000, 0).expect("embed");
        assert!(res.stc_rate > 0.0 && res.stc_rate <= 1.0);
    }
}
