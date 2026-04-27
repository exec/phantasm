//! J-UNIWARD: Universal Wavelet Relative Distortion for JPEG.
//!
//! Holub, Fridrich, Denemark. "Universal Distortion Function for Steganography
//! in an Arbitrary Domain." EURASIP Journal on Information Security, 2014.
//!
//! Cost formula (per DCT coefficient at block `(br, bc)`, intra-block
//! position `(i, j)`):
//!
//! ```text
//!                  3    W-1 H-1  | ξ^(k)_{ij}(br, bc, u, v) |
//!   ρ_{ij}(br,bc) = Σ    Σ   Σ  ──────────────────────────────
//!                 k=1   u=0 v=0     σ + | R^(k)(u, v) |
//! ```
//!
//! where `R^(k)` is the k-th wavelet sub-band residual of the DECODED spatial
//! image (k ∈ {LH, HL, HH}, 1-level Daubechies-8 decomposition) and
//! `ξ^(k)_{ij}(br, bc, ·)` is the impulse response of R^(k) to a unit
//! perturbation of the JPEG coefficient at `(br, bc, i, j)`. σ is a small
//! constant (the paper uses 2⁻⁶ = 1/64) to avoid division by zero.
//!
//! # Implementation strategy
//!
//! The per-coefficient impulse response `ξ^(k)_{ij}` decomposes into a fixed
//! spatially-shift-invariant kernel times the block-origin offset:
//!
//! - Let `Φ_{ij}` be the 8×8 DCT basis for mode `(i, j)` scaled by the
//!   dequantization factor `q_{ij}` (a +1 change in the stored coefficient
//!   produces the pixel image `q_{ij}·Φ_{ij}` inside block `(br, bc)` and
//!   zero elsewhere).
//! - Convolving that 8×8 image with the 16-tap 2D Daubechies-8 high-pass
//!   filter `F_k` (15-tap support → 16×16 filter) gives a 23×23 kernel
//!   `K^(k)_{ij}` (8 + 16 − 1 = 23).
//! - `ξ^(k)_{ij}(br, bc, u, v) = K^(k)_{ij}(u − 8br + pad, v − 8bc + pad)`
//!   up to a sub-band-independent constant offset induced by the wavelet
//!   filter's group delay.
//!
//! So the cost is a sum over k of the dot product between the fixed 23×23
//! kernel `|K^(k)_{ij}|` and the 23×23 window of `W^(k) = 1 / (σ + |R^(k)|)`
//! centered on the block origin. We precompute all 3·64 kernels once per call
//! (they depend on the quant table so they must be rebuilt per-image — but
//! the quant table is constant across blocks, so it's a one-time cost).
//!
//! The wavelet residuals `R^(k)` are computed as 2D convolutions of the
//! dequantized + inverse-DCT'd spatial Y channel against `F_k`.
//!
//! # Cost
//!
//! Per-image work dominated by 3 × (H × W × 256) for the residual
//! convolutions and 3 × 64 × 529 × num_blocks for the block-wise inner
//! products. For a 512×512 image: ≈2.0 GFLOP — slow but tractable for
//! research use. No optimizations beyond cache-friendly iteration order.

// This module is numeric kernel code with many small fixed-shape 2D loops
// where indexing is clearer than iterator chains. Accept the usual lints.
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::excessive_precision)]

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;

/// Division-stabilization constant from Holub & Fridrich 2014 §4.2.
const SIGMA: f64 = 1.0 / 64.0;

/// 8-tap Daubechies-8 (db8 = Daubechies length-16) orthonormal scaling filter.
///
/// Values from Daubechies, "Ten Lectures on Wavelets" (1992), Table 6.2,
/// cross-checked against PyWavelets `pywt.Wavelet('db8').dec_lo`. The
/// decomposition high-pass filter is derived as the quadrature mirror:
/// `h_hi[k] = (-1)^k · h_lo[L-1-k]`.
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

/// 2D separable outer product `outer[y][x] = a[y] * b[x]` (16×16 filter).
fn outer_product(a: &[f64; 16], b: &[f64; 16]) -> [[f64; 16]; 16] {
    let mut out = [[0.0f64; 16]; 16];
    for y in 0..16 {
        for x in 0..16 {
            out[y][x] = a[y] * b[x];
        }
    }
    out
}

/// 2D convolution of an image with a 16×16 kernel, `'same'` boundary via
/// symmetric reflection. Output has the same dimensions as input.
fn conv2_same_16(image: &[f64], width: usize, height: usize, kernel: &[[f64; 16]; 16]) -> Vec<f64> {
    let mut out = vec![0.0f64; width * height];
    // Kernel is 16×16. Center offset: 8 (so taps span [-8, +7]).
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

/// 8×8 inverse DCT used by JPEG (IDCT-II) from dequantized coefficient block.
/// Input: 64 dequantized coefficients in JPEG natural order (row-major,
/// NOT zigzag). Output: 64 pixel values in [−128, 127] range (without the
/// final +128 level shift; callers apply it).
fn idct_8x8(block: &[f64; 64]) -> [f64; 64] {
    // Precomputed basis constants.
    let mut out = [0.0f64; 64];
    let c0 = 1.0f64 / (2.0f64).sqrt();
    let factor = 0.25f64;
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0f64;
            for v in 0..8 {
                for u in 0..8 {
                    let cu = if u == 0 { c0 } else { 1.0 };
                    let cv = if v == 0 { c0 } else { 1.0 };
                    let coeff = block[v * 8 + u];
                    let cos_x = ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos();
                    let cos_y = ((2 * y + 1) as f64 * v as f64 * std::f64::consts::PI / 16.0).cos();
                    sum += cu * cv * coeff * cos_x * cos_y;
                }
            }
            out[y * 8 + x] = factor * sum;
        }
    }
    out
}

/// Forward 8×8 DCT basis evaluated at spatial position `(x, y)` for mode
/// `(u, v)`: returns `φ_{u,v}(x, y)`. This is the IDCT of a unit impulse at
/// mode `(u, v)` — same cosine kernel as [`idct_8x8`] but evaluated directly
/// rather than summed.
fn dct_basis_pixel(u: usize, v: usize, x: usize, y: usize) -> f64 {
    let c0 = 1.0f64 / (2.0f64).sqrt();
    let cu = if u == 0 { c0 } else { 1.0 };
    let cv = if v == 0 { c0 } else { 1.0 };
    let cos_x = ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos();
    let cos_y = ((2 * y + 1) as f64 * v as f64 * std::f64::consts::PI / 16.0).cos();
    0.25 * cu * cv * cos_x * cos_y
}

/// JPEG zigzag order: zigzag[k] = natural-order index of the k-th zigzag slot.
#[rustfmt::skip]
const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Decode the stored component into a spatial-domain float image by
/// dequantizing and applying IDCT to every 8×8 block. Output is
/// `component.blocks_wide * 8` wide × `component.blocks_high * 8` tall
/// float pixels centered around 0 (i.e. the natural IDCT output, no +128
/// level shift — the wavelet filters are high-pass so the DC offset is
/// irrelevant anyway).
fn decode_spatial_y(
    coefficients: &[i16],
    quant_table: &[u16; 64],
    blocks_wide: usize,
    blocks_high: usize,
) -> (Vec<f64>, usize, usize) {
    let w = blocks_wide * 8;
    let h = blocks_high * 8;
    let mut out = vec![0.0f64; w * h];
    for br in 0..blocks_high {
        for bc in 0..blocks_wide {
            let base = (br * blocks_wide + bc) * 64;
            let mut deq = [0.0f64; 64];
            // Convert zigzag → natural-order while dequantizing.
            for zz in 0..64 {
                let nat = ZIGZAG[zz];
                deq[nat] = coefficients[base + zz] as f64 * quant_table[zz] as f64;
            }
            let spatial = idct_8x8(&deq);
            for y in 0..8 {
                for x in 0..8 {
                    out[(br * 8 + y) * w + (bc * 8 + x)] = spatial[y * 8 + x];
                }
            }
        }
    }
    (out, w, h)
}

/// Build the 23×23 impulse-response kernel `K^(k)_{ij}` for sub-band `k` and
/// DCT mode `(i, j)` (row i, col j in the 8×8 block).
///
/// `filter_2d` is the 16×16 Daubechies-8 2D filter for sub-band `k`.
/// `q_ij` is the quantization factor at mode `(i, j)` (dequant multiplier).
///
/// Method: start with a 23×23 canvas, place the 8×8 pixel image
/// `q_ij · Φ_{ij}(x, y)` at offset (8, 8) (centered), then convolve in-place
/// with the 16×16 filter. The 'full' convolution size is 8 + 16 − 1 = 23.
fn make_change_kernel(
    filter_2d: &[[f64; 16]; 16],
    i: usize,
    j: usize,
    q_ij: f64,
) -> [[f64; 23]; 23] {
    // Build the 8×8 pixel image of the dequantized unit change.
    let mut phi = [[0.0f64; 8]; 8];
    for y in 0..8 {
        for x in 0..8 {
            phi[y][x] = q_ij * dct_basis_pixel(j, i, x, y);
        }
    }

    // 'full' convolution of 8×8 with 16×16 → 23×23.
    let mut out = [[0.0f64; 23]; 23];
    for oy in 0..23 {
        for ox in 0..23 {
            let mut acc = 0.0f64;
            for ky in 0..16 {
                let py = oy as i32 - ky as i32;
                if py < 0 || py >= 8 {
                    continue;
                }
                for kx in 0..16 {
                    let px = ox as i32 - kx as i32;
                    if px < 0 || px >= 8 {
                        continue;
                    }
                    acc += filter_2d[ky][kx] * phi[py as usize][px as usize];
                }
            }
            out[oy][ox] = acc;
        }
    }
    out
}

pub struct Juniward;

impl Juniward {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Juniward {
    fn default() -> Self {
        Self::new()
    }
}

impl DistortionFunction for Juniward {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let component = &jpeg.components[component_idx];
        let blocks_wide = component.blocks_wide;
        let blocks_high = component.blocks_high;

        // 1. Decode to spatial Y (float, no level shift).
        let (spatial, w, h) = decode_spatial_y(
            &component.coefficients,
            &component.quant_table,
            blocks_wide,
            blocks_high,
        );

        // 2. Build 2D Daubechies-8 filter banks (LH, HL, HH).
        let lo = DB8_LO;
        let hi = db8_hi();
        let f_lh = outer_product(&lo, &hi); // rows low-pass, cols high-pass
        let f_hl = outer_product(&hi, &lo); // rows high-pass, cols low-pass
        let f_hh = outer_product(&hi, &hi);
        let filters = [f_lh, f_hl, f_hh];

        // 3. Compute residual maps R_k and reciprocals W_k = 1 / (σ + |R_k|).
        let mut weight_maps: Vec<Vec<f64>> = Vec::with_capacity(3);
        for fk in &filters {
            let r_k = conv2_same_16(&spatial, w, h, fk);
            let w_k: Vec<f64> = r_k.iter().map(|r| 1.0 / (SIGMA + r.abs())).collect();
            weight_maps.push(w_k);
        }

        // 4. Dequantize + un-zigzag the quant table once. Natural-order quant.
        let mut q_nat = [0u16; 64];
        for zz in 0..64 {
            q_nat[ZIGZAG[zz]] = component.quant_table[zz];
        }

        // 5. Precompute 3 × 64 change kernels (|K^(k)_{ij}|, 23×23) per mode
        //    in natural order. Cost only depends on |K|, so we absolute-value
        //    up front.
        let mut abs_kernels: Vec<[[f64; 23]; 23]> = Vec::with_capacity(3 * 64);
        for fk in &filters {
            for i in 0..8 {
                for j in 0..8 {
                    let q_ij = q_nat[i * 8 + j] as f64;
                    let k_ij = make_change_kernel(fk, i, j, q_ij);
                    let mut abs_k = [[0.0f64; 23]; 23];
                    for y in 0..23 {
                        for x in 0..23 {
                            abs_k[y][x] = k_ij[y][x].abs();
                        }
                    }
                    abs_kernels.push(abs_k);
                }
            }
        }

        // 6. Per block, per mode, sum over k of <|K^(k)_{ij}|, W^(k) window>.
        //    Window is 23×23 centered on the block origin (top-left pixel of
        //    the block). Convolution 'full' output starts 8 pixels before the
        //    block origin (since 16-tap filter has group delay 8, and the
        //    8×8 input is placed at (8,8) in the 23×23 canvas).
        //
        //    Window reads from W^(k) at rows [block_y0 - 15 .. block_y0 + 8]
        //    where block_y0 = 8*br. Pixels out of bounds are dropped (zero
        //    contribution) — this is a minor approximation at the image
        //    edges but avoids boundary-reflection asymmetry biasing edge
        //    blocks.

        let num_blocks = blocks_wide * blocks_high;
        let capacity = num_blocks * 63; // skip DC
        let mut positions = Vec::with_capacity(capacity);
        let mut costs_plus = Vec::with_capacity(capacity);
        let mut costs_minus = Vec::with_capacity(capacity);

        for br in 0..blocks_high {
            for bc in 0..blocks_wide {
                let block_idx = br * blocks_wide + bc;
                let base_zz = block_idx * 64;
                let y0 = br * 8;
                let x0 = bc * 8;

                // Emit costs in zigzag order to match the CostMap convention.
                for zz in 1..64usize {
                    let nat = ZIGZAG[zz];
                    let i = nat / 8;
                    let j = nat % 8;
                    let nat_idx = i * 8 + j;
                    let mut rho = 0.0f64;
                    for k in 0..3 {
                        let kernel = &abs_kernels[k * 64 + nat_idx];
                        let w_k = &weight_maps[k];
                        let mut acc = 0.0f64;
                        for ky in 0..23 {
                            let py = y0 as i32 + ky as i32 - 15;
                            if py < 0 || py >= h as i32 {
                                continue;
                            }
                            let row = py as usize * w;
                            for kx in 0..23 {
                                let px = x0 as i32 + kx as i32 - 15;
                                if px < 0 || px >= w as i32 {
                                    continue;
                                }
                                acc += kernel[ky][kx] * w_k[row + px as usize];
                            }
                        }
                        rho += acc;
                    }

                    let coeff = component.coefficients[base_zz + zz];
                    let cp = if coeff == i16::MAX {
                        f64::INFINITY
                    } else {
                        rho
                    };
                    let cm = if coeff == i16::MIN {
                        f64::INFINITY
                    } else {
                        rho
                    };

                    positions.push((br, bc, zz));
                    costs_plus.push(cp);
                    costs_minus.push(cm);
                }
            }
        }

        CostMap {
            costs_plus,
            costs_minus,
            positions,
        }
    }

    fn name(&self) -> &str {
        "j-uniward"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phantasm_image::jpeg::{JpegCoefficients, JpegComponent};

    fn make_jpeg_from_blocks(block_coeffs: &[Vec<i16>], blocks_wide: usize) -> JpegCoefficients {
        let num_blocks = block_coeffs.len();
        assert_eq!(num_blocks % blocks_wide, 0);
        let blocks_high = num_blocks / blocks_wide;

        // Standard luminance QF=75 table (zigzag order), from libjpeg.
        #[rustfmt::skip]
        let quant_table: [u16; 64] = [
             8,  6,  5,  8, 12, 20, 26, 31,
             6,  6,  7, 10, 13, 29, 30, 28,
             7,  7,  8, 12, 20, 29, 35, 28,
             7,  9, 11, 15, 26, 44, 40, 31,
             9, 11, 19, 28, 34, 55, 52, 39,
            12, 18, 28, 32, 41, 52, 57, 46,
            25, 32, 39, 44, 52, 61, 60, 51,
            36, 46, 48, 49, 56, 50, 52, 50,
        ];

        let mut coefficients = vec![0i16; num_blocks * 64];
        for (bi, block) in block_coeffs.iter().enumerate() {
            coefficients[bi * 64..(bi + 1) * 64].copy_from_slice(block);
        }

        JpegCoefficients {
            components: vec![JpegComponent {
                id: 1,
                blocks_wide,
                blocks_high,
                coefficients,
                quant_table,
                h_samp_factor: 1,
                v_samp_factor: 1,
            }],
            width: (blocks_wide * 8) as u32,
            height: (blocks_high * 8) as u32,
            quality_estimate: Some(75),
            markers: vec![],
        }
    }

    /// Solid-gray cover: DC=1000 in every block, all AC=0. The decoded spatial
    /// image is flat, wavelet residuals are (near) zero, and weights W_k are
    /// uniformly 1/σ across the interior. Interior costs should be nearly
    /// identical across blocks (edge blocks still feel the reflection
    /// boundary, so we require an 8×8-block image and sample deep-interior
    /// blocks only — the 16-tap filter has a half-width of 8 so deep-interior
    /// = blocks whose 23×23 window is fully inside the image).
    #[test]
    fn uniform_gray_costs_are_near_uniform() {
        let mut blocks = Vec::new();
        for _ in 0..64 {
            let mut b = vec![0i16; 64];
            b[0] = 1000;
            blocks.push(b);
        }
        let jpeg = make_jpeg_from_blocks(&blocks, 8); // 8×8 blocks = 64×64 px
        let juniward = Juniward::new();
        let cost_map = juniward.compute(&jpeg, 0);

        // Deep-interior blocks: whose 23×23 window (15 pixels before to 8
        // pixels after block origin) fits fully inside the 64×64 image.
        // That requires br, bc in 2..6 (block origin y0 = 8*br, need
        // y0 - 15 ≥ 0 → br ≥ 2, and y0 + 8 < 64 → br ≤ 6).
        let interior: Vec<f64> = cost_map
            .positions
            .iter()
            .zip(cost_map.costs_plus.iter())
            .filter(|((br, bc, dp), _)| *dp == 1 && (2..6).contains(br) && (2..6).contains(bc))
            .map(|(_, c)| *c)
            .collect();
        assert!(interior.len() >= 9, "need deep-interior blocks");
        let min = interior.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = interior.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max / min < 1.01,
            "deep-interior costs should be near-uniform on flat image, got min={min} max={max}"
        );
    }

    /// DC coefficients are never emitted in the cost map.
    #[test]
    fn dc_coefficients_excluded() {
        let mut blocks = Vec::new();
        for _ in 0..4 {
            let mut b = vec![0i16; 64];
            b[0] = 100;
            blocks.push(b);
        }
        let jpeg = make_jpeg_from_blocks(&blocks, 2);
        let juniward = Juniward::new();
        let cost_map = juniward.compute(&jpeg, 0);
        for &(_, _, dp) in &cost_map.positions {
            assert_ne!(dp, 0);
        }
        assert_eq!(cost_map.len(), 4 * 63);
    }

    /// Determinism: two consecutive `compute` calls yield bit-identical maps.
    #[test]
    fn determinism() {
        let mut blocks = Vec::new();
        for bi in 0..4 {
            let mut b = vec![0i16; 64];
            b[0] = 100 + bi as i16;
            b[1] = 10;
            b[8] = -5;
            blocks.push(b);
        }
        let jpeg = make_jpeg_from_blocks(&blocks, 2);
        let juniward = Juniward::new();
        let a = juniward.compute(&jpeg, 0);
        let b = juniward.compute(&jpeg, 0);
        assert_eq!(a.costs_plus, b.costs_plus);
        assert_eq!(a.costs_minus, b.costs_minus);
        assert_eq!(a.positions, b.positions);
    }

    #[test]
    fn name_is_juniward() {
        assert_eq!(Juniward::new().name(), "j-uniward");
    }

    /// Content-adaptivity check against a real JPEG: a smooth gradient half
    /// and a high-texture half. Mean cost in the textured region should be
    /// much lower than in the smooth region.
    #[test]
    fn textured_region_has_lower_costs_than_smooth() {
        use image::{ImageBuffer, Rgb};
        use std::path::PathBuf;

        let tmp_dir = tempfile::tempdir().expect("tempdir");
        let jpeg_path: PathBuf = tmp_dir.path().join("test.jpg");

        // 128×64 image: left half smooth gradient, right half noisy.
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(128, 64, |x, y| {
            if x < 64 {
                // smooth: gray ramp
                let v = 40 + (x + y) as u8;
                Rgb([v, v, v])
            } else {
                // textured: high-freq pattern
                let v = (((x * 17 + y * 31) ^ (x * y)) & 0xFF) as u8;
                Rgb([v, v, v])
            }
        });
        img.save(&jpeg_path).expect("save JPEG");

        let jpeg = phantasm_image::jpeg::read(&jpeg_path).expect("read JPEG");
        let juniward = Juniward::new();
        let cost_map = juniward.compute(&jpeg, 0);

        // Bucket costs by block column: left half (bc < 4) = smooth,
        // right half (bc >= 4) = textured. 128/8 = 16 blocks wide.
        let mut smooth_sum = 0.0f64;
        let mut smooth_n = 0usize;
        let mut text_sum = 0.0f64;
        let mut text_n = 0usize;
        for (i, &(_br, bc, _dp)) in cost_map.positions.iter().enumerate() {
            let cost = cost_map.costs_plus[i];
            if !cost.is_finite() {
                continue;
            }
            if bc < 4 {
                smooth_sum += cost;
                smooth_n += 1;
            } else if bc >= 12 {
                text_sum += cost;
                text_n += 1;
            }
        }
        let smooth_mean = smooth_sum / smooth_n as f64;
        let text_mean = text_sum / text_n as f64;
        assert!(
            smooth_mean > text_mean * 10.0,
            "smooth mean ({smooth_mean}) should be > 10× textured mean ({text_mean})"
        );
    }

    #[test]
    fn real_fixture_smoke_test() {
        use image::{ImageBuffer, Rgb};
        use std::path::PathBuf;

        let tmp_dir = tempfile::tempdir().expect("tempdir");
        let jpeg_path: PathBuf = tmp_dir.path().join("test.jpg");

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(64, 64, |x, y| {
            let r = ((x * 2 + y) % 256) as u8;
            let g = ((x + y * 3) % 256) as u8;
            let b = ((x * y + 13) % 256) as u8;
            Rgb([r, g, b])
        });
        img.save(&jpeg_path).expect("save JPEG");

        let jpeg = phantasm_image::jpeg::read(&jpeg_path).expect("read JPEG");
        let juniward = Juniward::new();
        let cost_map = juniward.compute(&jpeg, 0);

        assert!(!cost_map.is_empty());
        let finite_positive = cost_map
            .costs_plus
            .iter()
            .filter(|&&c| c.is_finite() && c > 0.0)
            .count();
        assert!(finite_positive as f64 / cost_map.len() as f64 > 0.9);
    }

    #[test]
    fn db8_filter_is_orthonormal() {
        let lo = DB8_LO;
        let hi = db8_hi();
        // Σ lo² == 1
        let norm_lo: f64 = lo.iter().map(|v| v * v).sum();
        assert!((norm_lo - 1.0).abs() < 1e-6, "‖lo‖² = {norm_lo}");
        let norm_hi: f64 = hi.iter().map(|v| v * v).sum();
        assert!((norm_hi - 1.0).abs() < 1e-6, "‖hi‖² = {norm_hi}");
        // Σ lo = √2 (low-pass DC gain)
        let sum_lo: f64 = lo.iter().sum();
        assert!(
            (sum_lo - 2.0f64.sqrt()).abs() < 1e-6,
            "Σ lo = {sum_lo}, expected √2"
        );
        // Σ hi = 0 (high-pass has no DC)
        let sum_hi: f64 = hi.iter().sum();
        assert!(sum_hi.abs() < 1e-6, "Σ hi = {sum_hi}");
    }
}
