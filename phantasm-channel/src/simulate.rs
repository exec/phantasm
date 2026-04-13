//! Per-block re-encode simulator used by MINICER.
//!
//! Given a single 8×8 block of zigzag-indexed source coefficients plus
//! the *source* and *target* quant tables (both zigzag), simulate what
//! that block looks like after the channel decodes the JPEG to spatial
//! pixels and re-encodes at the target QF.
//!
//! Steps:
//!   1. Dequantize using the source quant table.
//!   2. Un-zigzag to natural order.
//!   3. Inverse DCT to spatial samples.
//!   4. Level-shift (+128) and clip to `[0, 255]` (the channel saturates
//!      to 8-bit before re-encoding).
//!   5. Level-shift back (−128).
//!   6. Forward DCT.
//!   7. Quantize using the target quant table (round-half-away-from-zero,
//!      matching libjpeg).
//!   8. Re-zigzag.
//!
//! This is a *single-block* approximation: a real JPEG re-encode operates
//! on the whole image, and an 8×8 block boundary effect from neighbouring
//! blocks could shift the IDCT output. For the MINICER use case the
//! single-block approximation is the right tradeoff: it is dramatically
//! faster (we re-simulate a block dozens of times per coefficient) and
//! the inter-block contribution to AC coefficient stability is small in
//! practice.

use crate::zigzag::{natural_to_zigzag, zigzag_to_natural};
use phantasm_image::dct::{dct2d_8x8, idct2d_8x8};

/// Standard JPEG luminance quantization table at QF=50 (zigzag order).
#[rustfmt::skip]
pub(crate) const STD_LUMA_Q50_ZIGZAG: [u16; 64] = [
    16, 11, 12, 14, 12, 10, 16, 14,
    13, 14, 18, 17, 16, 19, 24, 40,
    26, 24, 22, 22, 24, 49, 35, 37,
    29, 40, 58, 51, 61, 60, 57, 51,
    56, 55, 64, 72, 92, 78, 64, 68,
    87, 69, 55, 56, 80, 109, 81, 87,
    95, 98, 103, 104, 103, 62, 77, 113,
    121, 112, 100, 120, 92, 101, 103, 99,
];

/// Standard JPEG chroma quantization table at QF=50 (zigzag order).
#[rustfmt::skip]
pub(crate) const STD_CHROMA_Q50_ZIGZAG: [u16; 64] = [
    17, 18, 18, 24, 21, 24, 47, 26,
    26, 47, 99, 66, 56, 66, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// Build a libjpeg-style quantization table at the requested quality factor.
///
/// Implements the standard libjpeg scaling: scale = (qf < 50) ? 5000/qf
/// : 200 - 2*qf. Scaled entries are `(base * scale + 50) / 100`, clamped
/// to `[1, 255]` (baseline) — though we permit up to 32767 because some
/// channels use 12-bit JPEG. Returns a zigzag-ordered table.
pub fn build_quant_table(qf: u8, chroma: bool) -> [u16; 64] {
    let qf = qf.clamp(1, 100);
    let scale: i32 = if qf < 50 {
        5000 / qf as i32
    } else {
        200 - 2 * qf as i32
    };
    let base = if chroma {
        &STD_CHROMA_Q50_ZIGZAG
    } else {
        &STD_LUMA_Q50_ZIGZAG
    };
    let mut out = [0u16; 64];
    for i in 0..64 {
        let v = (base[i] as i32 * scale + 50) / 100;
        out[i] = v.clamp(1, 255) as u16;
    }
    out
}

/// Simulate one re-encode pass for an 8×8 block.
///
/// `block_zz_src` and `quant_src` are zigzag-indexed; same for `quant_tgt`.
/// Returns the post-re-encode coefficients in zigzag order.
///
/// This is the inner loop of MINICER, called many times per coefficient
/// during stabilization. Keep it fast and allocation-free.
pub fn reencode_block(
    block_zz_src: &[i16; 64],
    quant_src: &[u16; 64],
    quant_tgt: &[u16; 64],
) -> [i16; 64] {
    // 1+2: dequantize + un-zigzag in one pass.
    let mut deq_zz = [0.0f64; 64];
    for i in 0..64 {
        deq_zz[i] = block_zz_src[i] as f64 * quant_src[i] as f64;
    }
    let deq_nat = zigzag_to_natural(&deq_zz);

    // 3: IDCT.
    let mut spatial = idct2d_8x8(&deq_nat);

    // 4+5: level-shift +128, clamp, level-shift −128.
    for v in spatial.iter_mut() {
        let s = *v + 128.0;
        *v = s.clamp(0.0, 255.0) - 128.0;
    }

    // 6: forward DCT.
    let dct_nat = dct2d_8x8(&spatial);

    // 7+8: quantize + re-zigzag in one pass.
    let dct_zz = natural_to_zigzag(&dct_nat);
    let mut out = [0i16; 64];
    for i in 0..64 {
        let q = quant_tgt[i] as f64;
        // round-half-away-from-zero to match libjpeg's quantizer.
        let v = dct_zz[i] / q;
        let rounded = if v >= 0.0 {
            (v + 0.5).floor()
        } else {
            (v - 0.5).ceil()
        };
        out[i] = rounded.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_quant_table_qf_50_matches_baseline() {
        let q = build_quant_table(50, false);
        assert_eq!(q, STD_LUMA_Q50_ZIGZAG);
    }

    #[test]
    fn build_quant_table_higher_qf_smaller_values() {
        let q50 = build_quant_table(50, false);
        let q90 = build_quant_table(90, false);
        for i in 0..64 {
            assert!(
                q90[i] <= q50[i],
                "QF=90 entry {i} ({}) should be ≤ QF=50 entry ({})",
                q90[i],
                q50[i]
            );
        }
    }

    #[test]
    fn reencode_block_dc_only_round_trips() {
        // A flat block (DC only) should survive any reasonable re-encode
        // because the IDCT produces a flat field that compresses losslessly.
        let mut block = [0i16; 64];
        block[0] = 64; // DC value
        let q_src = build_quant_table(75, false);
        let q_tgt = build_quant_table(85, false);
        let reenc = reencode_block(&block, &q_src, &q_tgt);
        // DC * q_src / q_tgt rounded should be the new DC.
        let expected_dc = ((64.0 * q_src[0] as f64) / q_tgt[0] as f64).round() as i16;
        assert_eq!(reenc[0], expected_dc, "DC should survive");
        for (i, &v) in reenc.iter().enumerate().skip(1) {
            assert_eq!(v, 0, "AC[{i}] should still be zero");
        }
    }

    #[test]
    fn reencode_block_perturbation_changes_output() {
        let mut block = [0i16; 64];
        block[0] = 64;
        block[5] = 3;
        let q = build_quant_table(80, false);
        let a = reencode_block(&block, &q, &q);
        block[5] = 7;
        let b = reencode_block(&block, &q, &q);
        assert_ne!(a, b, "perturbing AC[5] should change re-encode output");
    }
}
