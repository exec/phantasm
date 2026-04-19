//! S-UNIWARD: spatial-domain Universal Wavelet Relative Distortion.
//!
//! Holub, Fridrich, Denemark. "Universal Distortion Function for Steganography
//! in an Arbitrary Domain." EURASIP Journal on Information Security, 2014.
//!
//! Per-pixel cost formula for a ±1 change at pixel `(r, c)`:
//!
//! ```text
//!                 3     | F_k(u - r, v - c) |
//!   ρ(r, c) = Σ   Σ    ───────────────────────
//!                k=1  u,v  σ + | R^(k)(u, v) |
//! ```
//!
//! where `F_k` is the 2D Daubechies-8 sub-band filter (k ∈ {LH, HL, HH}) and
//! `R^(k) = F_k ⊛ cover` is the wavelet residual of the cover image. σ is the
//! Holub-Fridrich stabilization constant 1/64.
//!
//! # Why this is simpler than J-UNIWARD
//!
//! In the spatial domain, the impulse response of `R^(k)` with respect to a
//! unit change at pixel `(r, c)` IS the wavelet filter `F_k` itself (shifted
//! to be centered at `(r, c)`). A single-pixel delta convolved with `F_k`
//! reproduces `F_k`. So the per-pixel inner product reduces to convolving the
//! weight map `W^(k) = 1 / (σ + |R^(k)|)` with `|F_k|` and reading off the
//! result at pixel `(r, c)`:
//!
//! ```text
//!   ρ = Σ_k (|F_k| ⊛ W^(k))
//! ```
//!
//! This is three 2D convolutions for the residuals, three for the costs — no
//! per-pixel impulse-response construction.

#![allow(clippy::needless_range_loop)]
#![allow(clippy::excessive_precision)]

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;
use phantasm_image::png::PngPixels;

/// Division-stabilization constant from Holub & Fridrich 2014 §4.2.
const SIGMA: f64 = 1.0 / 64.0;

/// 8-tap Daubechies-8 (db8 = Daubechies length-16) orthonormal scaling filter.
/// Same constants as [`crate::juniward::DB8_LO`]; duplicated here to keep the
/// two modules independent (juniward's copy is module-private).
#[rustfmt::skip]
const DB8_LO: [f64; 16] = [
    -0.000_117_476_784_002_281_5,
     0.000_675_449_405_998_557_1,
    -0.000_391_740_373_376_551_9,
    -0.004_870_352_993_451_940,
     0.008_746_094_047_015_655,
     0.013_981_027_917_015_516,
    -0.044_088_253_931_064_36,
    -0.017_369_301_002_022_108,
     0.128_747_426_620_186_0,
     0.000_472_484_573_997_209_4,
    -0.284_015_542_962_428_1,
    -0.015_829_105_256_023_893,
     0.585_354_683_654_216_9,
     0.675_630_736_298_035_8,
     0.312_871_590_914_317_13,
     0.054_415_842_243_081_4,
];

fn db8_hi() -> [f64; 16] {
    let mut hi = [0.0f64; 16];
    for k in 0..16 {
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        hi[k] = sign * DB8_LO[15 - k];
    }
    hi
}

fn outer_product(a: &[f64; 16], b: &[f64; 16]) -> [[f64; 16]; 16] {
    let mut out = [[0.0f64; 16]; 16];
    for y in 0..16 {
        for x in 0..16 {
            out[y][x] = a[y] * b[x];
        }
    }
    out
}

/// Symmetric-reflection boundary handler. Returns a valid index in `[0, len)`.
fn reflect(i: i32, len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut ii = i;
    while ii < 0 || ii >= len {
        if ii < 0 {
            ii = -ii - 1;
        } else if ii >= len {
            ii = 2 * len - ii - 1;
        }
    }
    ii
}

/// 2D convolution with a 16×16 kernel, `'same'` output via symmetric reflection.
fn conv2_same_16(image: &[f64], width: usize, height: usize, kernel: &[[f64; 16]; 16]) -> Vec<f64> {
    let mut out = vec![0.0f64; width * height];
    let r = 8i32;
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.0f64;
            for ky in 0..16 {
                let sy = y as i32 + ky as i32 - r;
                let yy = reflect(sy, height as i32) as usize;
                for kx in 0..16 {
                    let sx = x as i32 + kx as i32 - r;
                    let xx = reflect(sx, width as i32) as usize;
                    acc += kernel[ky][kx] * image[yy * width + xx];
                }
            }
            out[y * width + x] = acc;
        }
    }
    out
}

/// S-UNIWARD distortion function over spatial-domain pixels.
///
/// Produces per-pixel `±1` modification costs; `costs_plus[i] == costs_minus[i]`
/// because a one-LSB flip at pixel `i` changes the pixel by ±1 and the cost is
/// symmetric (`|F_k|` is sign-invariant). Saturated pixels (0 or 255) are
/// marked as wet on the direction that would overflow.
///
/// The positions returned by [`compute_pixels`] are `(row, col, 0)` triples —
/// the third slot is a dummy because `CostMap::positions` expects
/// `(block_row, block_col, dct_pos)`. The spatial pipeline treats each pixel
/// as a single coefficient; the tuple is just an index carrier.
pub struct SUniward;

impl SUniward {
    pub const fn new() -> Self {
        Self
    }

    /// Compute the per-pixel S-UNIWARD cost map for an 8-bit grayscale image.
    pub fn compute_pixels(&self, pixels: &PngPixels) -> CostMap {
        let w = pixels.width as usize;
        let h = pixels.height as usize;
        let n = w * h;

        // Convert u8 pixels to f64 once.
        let image: Vec<f64> = pixels.pixels.iter().map(|&p| p as f64).collect();

        // Build 2D DB8 sub-band filters (LH, HL, HH).
        let lo = DB8_LO;
        let hi = db8_hi();
        let f_lh = outer_product(&lo, &hi);
        let f_hl = outer_product(&hi, &lo);
        let f_hh = outer_product(&hi, &hi);
        let filters = [f_lh, f_hl, f_hh];

        // Weight maps W_k = 1 / (σ + |R_k|).
        let mut weight_maps: Vec<Vec<f64>> = Vec::with_capacity(3);
        for fk in &filters {
            let r_k = conv2_same_16(&image, w, h, fk);
            let w_k: Vec<f64> = r_k.iter().map(|r| 1.0 / (SIGMA + r.abs())).collect();
            weight_maps.push(w_k);
        }

        // Absolute-value filters (cost uses |F_k|, not F_k).
        let abs_filters: Vec<[[f64; 16]; 16]> = filters
            .iter()
            .map(|f| {
                let mut out = [[0.0f64; 16]; 16];
                for y in 0..16 {
                    for x in 0..16 {
                        out[y][x] = f[y][x].abs();
                    }
                }
                out
            })
            .collect();

        // ρ(r, c) = Σ_k (|F_k| ⊛ W_k)(r, c)
        let mut rho = vec![0.0f64; n];
        for k in 0..3 {
            let contrib = conv2_same_16(&weight_maps[k], w, h, &abs_filters[k]);
            for i in 0..n {
                rho[i] += contrib[i];
            }
        }

        // Emit cost map: one position per pixel, row-major.
        let mut positions = Vec::with_capacity(n);
        let mut costs_plus = Vec::with_capacity(n);
        let mut costs_minus = Vec::with_capacity(n);
        for r in 0..h {
            for c in 0..w {
                let idx = r * w + c;
                let p = pixels.pixels[idx];
                let cost = rho[idx];
                let cp = if p == 255 { f64::INFINITY } else { cost };
                let cm = if p == 0 { f64::INFINITY } else { cost };
                positions.push((r, c, 0));
                costs_plus.push(cp);
                costs_minus.push(cm);
            }
        }

        CostMap {
            costs_plus,
            costs_minus,
            positions,
        }
    }

    pub fn name(&self) -> &str {
        "s-uniward"
    }
}

impl Default for SUniward {
    fn default() -> Self {
        Self::new()
    }
}

/// S-UNIWARD does not apply to JPEG DCT coefficients — J-UNIWARD is the
/// DCT-domain variant. This impl exists only so `SUniward` satisfies the same
/// `DistortionFunction` trait shape as the JPEG costs; calling `compute` on a
/// JPEG panics to make the misuse obvious. The spatial pipeline uses
/// [`SUniward::compute_pixels`] directly.
impl DistortionFunction for SUniward {
    fn compute(&self, _jpeg: &JpegCoefficients, _component_idx: usize) -> CostMap {
        panic!("s-uniward is a spatial-domain cost function; use SUniward::compute_pixels for PNG covers, or phantasm_cost::Juniward for JPEG.");
    }

    fn name(&self) -> &str {
        "s-uniward"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_gray(w: u32, h: u32, level: u8) -> PngPixels {
        PngPixels {
            width: w,
            height: h,
            pixels: vec![level; (w * h) as usize],
        }
    }

    #[test]
    fn name_is_s_uniward() {
        assert_eq!(SUniward::new().name(), "s-uniward");
    }

    #[test]
    fn cost_map_size_matches_pixel_count() {
        let img = flat_gray(16, 16, 128);
        let costs = SUniward::new().compute_pixels(&img);
        assert_eq!(costs.len(), 16 * 16);
        assert_eq!(costs.positions.len(), costs.costs_plus.len());
        assert_eq!(costs.positions.len(), costs.costs_minus.len());
    }

    #[test]
    fn costs_are_finite_for_mid_tone_pixels() {
        let img = flat_gray(32, 32, 128);
        let costs = SUniward::new().compute_pixels(&img);
        let finite_plus = costs.costs_plus.iter().filter(|c| c.is_finite()).count();
        let finite_minus = costs.costs_minus.iter().filter(|c| c.is_finite()).count();
        // All mid-tone pixels must have finite costs in both directions.
        assert_eq!(finite_plus, costs.len());
        assert_eq!(finite_minus, costs.len());
    }

    #[test]
    fn saturated_pixels_are_marked_wet_on_overflow_side() {
        let mut img = flat_gray(16, 16, 128);
        img.pixels[0] = 0;
        img.pixels[1] = 255;
        let costs = SUniward::new().compute_pixels(&img);
        // Pixel at 0: cannot go −1 (wet), can go +1 (finite).
        assert!(costs.costs_minus[0].is_infinite());
        assert!(costs.costs_plus[0].is_finite());
        // Pixel at 255: cannot go +1 (wet), can go −1.
        assert!(costs.costs_plus[1].is_infinite());
        assert!(costs.costs_minus[1].is_finite());
    }

    #[test]
    fn textured_region_has_lower_costs_than_smooth_region() {
        // Left half smooth, right half noisy; costs in the textured columns
        // should be substantially lower than in the smooth columns.
        let w = 64u32;
        let h = 32u32;
        let mut pixels = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if x < w / 2 {
                    pixels[i] = 80 + ((x + y) as u8 / 4);
                } else {
                    pixels[i] =
                        (((x.wrapping_mul(17) ^ y.wrapping_mul(31)) ^ (x * y)) & 0xFF) as u8;
                }
            }
        }
        let img = PngPixels {
            width: w,
            height: h,
            pixels,
        };
        let costs = SUniward::new().compute_pixels(&img);

        let mut smooth_sum = 0.0f64;
        let mut smooth_n = 0usize;
        let mut text_sum = 0.0f64;
        let mut text_n = 0usize;
        for (i, &(_r, c, _)) in costs.positions.iter().enumerate() {
            let cost = costs.costs_plus[i];
            if !cost.is_finite() {
                continue;
            }
            if c < 16 {
                smooth_sum += cost;
                smooth_n += 1;
            } else if c >= 48 {
                text_sum += cost;
                text_n += 1;
            }
        }
        let smooth_mean = smooth_sum / smooth_n as f64;
        let text_mean = text_sum / text_n as f64;
        assert!(
            smooth_mean > text_mean * 5.0,
            "smooth mean ({smooth_mean}) should be > 5× textured mean ({text_mean})"
        );
    }

    #[test]
    fn determinism() {
        let img = flat_gray(16, 16, 96);
        let a = SUniward::new().compute_pixels(&img);
        let b = SUniward::new().compute_pixels(&img);
        assert_eq!(a.costs_plus, b.costs_plus);
        assert_eq!(a.costs_minus, b.costs_minus);
        assert_eq!(a.positions, b.positions);
    }
}
