//! 8×8 DCT-II and inverse, matching libjpeg's convention.
//! Scale: (1/4) * C(u) * C(v) where C(0) = 1/√2, C(k≠0) = 1.

use std::f64::consts::PI;

fn c(k: usize) -> f64 {
    if k == 0 {
        1.0 / 2.0_f64.sqrt()
    } else {
        1.0
    }
}

pub fn dct2d_8x8(block: &[f64; 64]) -> [f64; 64] {
    let mut out = [0.0f64; 64];
    for v in 0..8usize {
        for u in 0..8usize {
            let mut sum = 0.0f64;
            for y in 0..8usize {
                for x in 0..8usize {
                    sum += block[y * 8 + x]
                        * ((2.0 * x as f64 + 1.0) * u as f64 * PI / 16.0).cos()
                        * ((2.0 * y as f64 + 1.0) * v as f64 * PI / 16.0).cos();
                }
            }
            out[v * 8 + u] = 0.25 * c(u) * c(v) * sum;
        }
    }
    out
}

pub fn idct2d_8x8(block: &[f64; 64]) -> [f64; 64] {
    let mut out = [0.0f64; 64];
    for y in 0..8usize {
        for x in 0..8usize {
            let mut sum = 0.0f64;
            for v in 0..8usize {
                for u in 0..8usize {
                    sum += c(u)
                        * c(v)
                        * block[v * 8 + u]
                        * ((2.0 * x as f64 + 1.0) * u as f64 * PI / 16.0).cos()
                        * ((2.0 * y as f64 + 1.0) * v as f64 * PI / 16.0).cos();
                }
            }
            out[y * 8 + x] = 0.25 * sum;
        }
    }
    out
}

pub fn quantize(block: &[f64; 64], quant_table: &[u16; 64]) -> [i16; 64] {
    let mut out = [0i16; 64];
    for i in 0..64 {
        let q = quant_table[i] as f64;
        out[i] = (block[i] / q).round() as i16;
    }
    out
}

pub fn dequantize(block: &[i16; 64], quant_table: &[u16; 64]) -> [f64; 64] {
    let mut out = [0.0f64; 64];
    for i in 0..64 {
        out[i] = block[i] as f64 * quant_table[i] as f64;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dct_identity() {
        // Use a deterministic pseudo-random block
        let block: [f64; 64] = std::array::from_fn(|i| (i as f64 * 17.3 + 42.7).sin() * 100.0);
        let dct = dct2d_8x8(&block);
        let recovered = idct2d_8x8(&dct);
        for (a, b) in block.iter().zip(recovered.iter()) {
            assert!(
                (a - b).abs() < 1e-10,
                "identity failed: input={a} recovered={b} diff={}",
                (a - b).abs()
            );
        }
    }

    #[test]
    fn dct_all_128_dc() {
        let block = [128.0f64; 64];
        let dct = dct2d_8x8(&block);
        // DC coefficient (index 0) should be 1024
        assert!(
            (dct[0] - 1024.0).abs() < 1e-9,
            "DC expected 1024, got {}",
            dct[0]
        );
        // All AC coefficients ≈ 0
        for (i, &v) in dct.iter().enumerate().skip(1) {
            assert!(v.abs() < 1e-9, "AC[{i}] expected ~0, got {v}");
        }
    }

    #[test]
    fn quantize_dequantize() {
        let block = [128.0f64; 64];
        let quant: [u16; 64] = [16u16; 64];
        let q = quantize(&block, &quant);
        let dq = dequantize(&q, &quant);
        assert_eq!(dq[0], 128.0);
    }
}
