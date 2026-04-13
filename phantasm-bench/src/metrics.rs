use std::path::Path;

use image::imageops::FilterType;
use image::GrayImage;

use crate::error::BenchError;

pub fn mse(cover: &[u8], stego: &[u8]) -> f64 {
    assert_eq!(cover.len(), stego.len(), "buffer length mismatch");
    if cover.is_empty() {
        return 0.0;
    }
    let sum: f64 = cover
        .iter()
        .zip(stego.iter())
        .map(|(&a, &b)| {
            let d = a as f64 - b as f64;
            d * d
        })
        .sum();
    sum / cover.len() as f64
}

pub fn psnr(cover: &[u8], stego: &[u8]) -> f64 {
    let m = mse(cover, stego);
    if m == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0_f64 * 255.0 / m).log10()
}

// ── SSIM ────────────────────────────────────────────────────────────────────

const SSIM_K1: f64 = 0.01;
const SSIM_K2: f64 = 0.03;
const SSIM_L: f64 = 255.0;
const SSIM_WIN: usize = 11;
const SSIM_SIGMA: f64 = 1.5;

fn gaussian_kernel_11() -> [f64; SSIM_WIN * SSIM_WIN] {
    let mut kernel = [0.0f64; SSIM_WIN * SSIM_WIN];
    let half = (SSIM_WIN / 2) as isize;
    let mut sum = 0.0;
    for y in 0..SSIM_WIN {
        for x in 0..SSIM_WIN {
            let dy = y as isize - half;
            let dx = x as isize - half;
            let v = (-(dx * dx + dy * dy) as f64 / (2.0 * SSIM_SIGMA * SSIM_SIGMA)).exp();
            kernel[y * SSIM_WIN + x] = v;
            sum += v;
        }
    }
    for v in kernel.iter_mut() {
        *v /= sum;
    }
    kernel
}

pub fn ssim_grayscale(cover: &[u8], stego: &[u8], width: u32, height: u32) -> f64 {
    assert_eq!(cover.len(), stego.len());
    let w = width as usize;
    let h = height as usize;
    let kernel = gaussian_kernel_11();
    let half = SSIM_WIN / 2;

    let c1 = (SSIM_K1 * SSIM_L).powi(2);
    let c2 = (SSIM_K2 * SSIM_L).powi(2);

    let mut ssim_sum = 0.0f64;
    let mut count = 0usize;

    for cy in half..(h - half) {
        for cx in half..(w - half) {
            let mut mu_x = 0.0f64;
            let mut mu_y = 0.0f64;
            for ky in 0..SSIM_WIN {
                for kx in 0..SSIM_WIN {
                    let py = cy + ky - half;
                    let px = cx + kx - half;
                    let w_val = kernel[ky * SSIM_WIN + kx];
                    mu_x += w_val * cover[py * w + px] as f64;
                    mu_y += w_val * stego[py * w + px] as f64;
                }
            }
            let mut sigma_x = 0.0f64;
            let mut sigma_y = 0.0f64;
            let mut sigma_xy = 0.0f64;
            for ky in 0..SSIM_WIN {
                for kx in 0..SSIM_WIN {
                    let py = cy + ky - half;
                    let px = cx + kx - half;
                    let w_val = kernel[ky * SSIM_WIN + kx];
                    let vx = cover[py * w + px] as f64 - mu_x;
                    let vy = stego[py * w + px] as f64 - mu_y;
                    sigma_x += w_val * vx * vx;
                    sigma_y += w_val * vy * vy;
                    sigma_xy += w_val * vx * vy;
                }
            }
            let num = (2.0 * mu_x * mu_y + c1) * (2.0 * sigma_xy + c2);
            let den = (mu_x * mu_x + mu_y * mu_y + c1) * (sigma_x + sigma_y + c2);
            ssim_sum += num / den;
            count += 1;
        }
    }

    if count == 0 {
        return 1.0;
    }
    ssim_sum / count as f64
}

// ── DCT helpers ──────────────────────────────────────────────────────────────

fn dct2d(block: &mut [[f64; 32]; 32]) {
    // Row-wise DCT-II
    for row in block.iter_mut() {
        dct1d(row);
    }
    // Column-wise DCT-II
    #[allow(clippy::needless_range_loop)]
    for col in 0..32 {
        let mut tmp = [0.0f64; 32];
        for row in 0..32 {
            tmp[row] = block[row][col];
        }
        dct1d(&mut tmp);
        for row in 0..32 {
            block[row][col] = tmp[row];
        }
    }
}

fn dct1d(x: &mut [f64; 32]) {
    const N: usize = 32;
    let mut out = [0.0f64; N];
    #[allow(clippy::needless_range_loop)]
    for k in 0..N {
        let mut s = 0.0f64;
        for (i, &xi) in x.iter().enumerate() {
            s += xi * (std::f64::consts::PI * k as f64 * (2 * i + 1) as f64 / (2 * N) as f64).cos();
        }
        let scale = if k == 0 {
            (1.0 / N as f64).sqrt()
        } else {
            (2.0 / N as f64).sqrt()
        };
        out[k] = s * scale;
    }
    x.copy_from_slice(&out);
}

fn load_gray_resized(path: &Path, w: u32, h: u32) -> Result<GrayImage, BenchError> {
    let img = image::open(path)?;
    let resized = img.resize_exact(w, h, FilterType::Lanczos3);
    Ok(resized.to_luma8())
}

pub fn phash_hamming(cover_path: &Path, stego_path: &Path) -> Result<u32, BenchError> {
    let a = compute_phash(cover_path)?;
    let b = compute_phash(stego_path)?;
    Ok((a ^ b).count_ones())
}

fn compute_phash(path: &Path) -> Result<u64, BenchError> {
    let gray = load_gray_resized(path, 32, 32)?;
    let pixels = gray.as_raw();

    let mut block = [[0.0f64; 32]; 32];
    for (i, &p) in pixels.iter().enumerate() {
        block[i / 32][i % 32] = p as f64;
    }
    dct2d(&mut block);

    // Flatten top-left 8×8 (64 coefficients)
    let mut vals = [0.0f64; 64];
    for r in 0..8usize {
        for c in 0..8usize {
            vals[r * 8 + c] = block[r][c];
        }
    }

    // Median of the 63 AC values (skip DC at index 0)
    let mut ac: Vec<f64> = vals[1..].to_vec();
    ac.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if ac.len().is_multiple_of(2) {
        (ac[ac.len() / 2 - 1] + ac[ac.len() / 2]) / 2.0
    } else {
        ac[ac.len() / 2]
    };

    let mut hash = 0u64;
    for (i, &v) in vals.iter().enumerate() {
        if v > median {
            hash |= 1u64 << i;
        }
    }
    Ok(hash)
}

pub fn dhash_hamming(cover_path: &Path, stego_path: &Path) -> Result<u32, BenchError> {
    let a = compute_dhash(cover_path)?;
    let b = compute_dhash(stego_path)?;
    Ok((a ^ b).count_ones())
}

fn compute_dhash(path: &Path) -> Result<u64, BenchError> {
    // 9×8 grayscale → compare adjacent pixels horizontally → 64 bits
    let gray = load_gray_resized(path, 9, 8)?;
    let pixels = gray.as_raw();

    let mut hash = 0u64;
    let mut bit = 0usize;
    for row in 0..8usize {
        for col in 0..8usize {
            let left = pixels[row * 9 + col] as i32;
            let right = pixels[row * 9 + col + 1] as i32;
            if left > right {
                hash |= 1u64 << bit;
            }
            bit += 1;
        }
    }
    Ok(hash)
}

pub fn file_size_delta(cover_path: &Path, stego_path: &Path) -> Result<i64, BenchError> {
    let cover_size = std::fs::metadata(cover_path)?.len() as i64;
    let stego_size = std::fs::metadata(stego_path)?.len() as i64;
    Ok(stego_size - cover_size)
}
