use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use statrs::distribution::{ChiSquared, ContinuousCDF};

use phantasm_image::jpeg::JpegComponent;

// ── RS Attack ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsResult {
    pub estimated_rate_y: f64,
    pub r_m: usize,
    pub s_m: usize,
    pub r_neg_m: usize,
    pub s_neg_m: usize,
    pub verdict: String,
}

fn flip_f1(x: u8) -> u8 {
    x ^ 1
}

fn flip_f_neg1(x: u8) -> u8 {
    flip_f1(x.wrapping_add(1)).wrapping_sub(1)
}

fn discrimination(group: &[u8]) -> i32 {
    let mut f = 0i32;
    for i in 0..group.len() - 1 {
        f += (group[i] as i32 - group[i + 1] as i32).abs();
    }
    f
}

struct RsCounts {
    r_m: usize,
    s_m: usize,
    r_neg_m: usize,
    s_neg_m: usize,
}

fn rs_counts_for_pixels(pixels: &[u8]) -> RsCounts {
    let n = 4usize;
    let num_groups = pixels.len() / n;
    let mut r_m = 0usize;
    let mut s_m = 0usize;
    let mut r_neg_m = 0usize;
    let mut s_neg_m = 0usize;

    for g in 0..num_groups {
        let group: [u8; 4] = [
            pixels[g * n],
            pixels[g * n + 1],
            pixels[g * n + 2],
            pixels[g * n + 3],
        ];
        let f_orig = discrimination(&group);

        let flipped_pos: [u8; 4] = [
            flip_f1(group[0]),
            flip_f1(group[1]),
            flip_f1(group[2]),
            flip_f1(group[3]),
        ];
        let f_pos = discrimination(&flipped_pos);

        let flipped_neg: [u8; 4] = [
            flip_f_neg1(group[0]),
            flip_f_neg1(group[1]),
            flip_f_neg1(group[2]),
            flip_f_neg1(group[3]),
        ];
        let f_neg = discrimination(&flipped_neg);

        if f_pos > f_orig {
            r_m += 1;
        } else if f_pos < f_orig {
            s_m += 1;
        }

        if f_neg > f_orig {
            r_neg_m += 1;
        } else if f_neg < f_orig {
            s_neg_m += 1;
        }
    }

    RsCounts {
        r_m,
        s_m,
        r_neg_m,
        s_neg_m,
    }
}

fn rs_solve(d0: f64, d1: f64, d0p: f64, d1p: f64) -> f64 {
    let a = 2.0 * (d0 + d1);
    let b = d1p - d1 - d0p - 3.0 * d0;
    let c = d0 - d0p;

    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return 0.0;
        }
        let z = -c / b;
        if (z - 0.5).abs() < 1e-12 {
            return 0.0;
        }
        return (z / (z - 0.5)).abs();
    }

    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return 0.0;
    }
    let sqrt_disc = disc.sqrt();
    let z1 = (-b + sqrt_disc) / (2.0 * a);
    let z2 = (-b - sqrt_disc) / (2.0 * a);

    let rate1 = if (z1 - 0.5).abs() < 1e-12 {
        f64::INFINITY
    } else {
        z1 / (z1 - 0.5)
    };
    let rate2 = if (z2 - 0.5).abs() < 1e-12 {
        f64::INFINITY
    } else {
        z2 / (z2 - 0.5)
    };

    let abs1 = rate1.abs();
    let abs2 = rate2.abs();
    if abs1 <= abs2 {
        rate1.abs()
    } else {
        rate2.abs()
    }
}

pub fn rs_attack(pixels: &[u8], threshold: f64) -> RsResult {
    let counts = rs_counts_for_pixels(pixels);
    let n = (pixels.len() / 4) as f64;

    let d0 = counts.r_m as f64 / n - counts.s_m as f64 / n;
    let d1 = counts.r_neg_m as f64 / n - counts.s_neg_m as f64 / n;

    // Simulate p=0.5 by flipping all LSBs
    let flipped: Vec<u8> = pixels.iter().map(|&x| x ^ 1).collect();
    let counts_p = rs_counts_for_pixels(&flipped);

    let d0p = counts_p.r_m as f64 / n - counts_p.s_m as f64 / n;
    let d1p = counts_p.r_neg_m as f64 / n - counts_p.s_neg_m as f64 / n;

    let rate = rs_solve(d0, d1, d0p, d1p);
    let verdict = if rate > threshold {
        "detected".to_string()
    } else {
        "clean".to_string()
    };

    RsResult {
        estimated_rate_y: rate,
        r_m: counts.r_m,
        s_m: counts.s_m,
        r_neg_m: counts.r_neg_m,
        s_neg_m: counts.s_neg_m,
        verdict,
    }
}

// ── Fridrich RS Attack (Aletheia-faithful, per-channel RGB) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FridrichRsCounts {
    pub r_m_red: usize,
    pub s_m_red: usize,
    pub r_neg_m_red: usize,
    pub s_neg_m_red: usize,
    pub r_m_green: usize,
    pub s_m_green: usize,
    pub r_neg_m_green: usize,
    pub s_neg_m_green: usize,
    pub r_m_blue: usize,
    pub s_m_blue: usize,
    pub r_neg_m_blue: usize,
    pub s_neg_m_blue: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FridrichRsResult {
    pub estimated_rate_r: f64,
    pub estimated_rate_g: f64,
    pub estimated_rate_b: f64,
    pub max_rate: f64,
    pub detected_channels: Vec<String>,
    pub raw_counts: FridrichRsCounts,
    pub verdict: String,
}

/// 2D smoothness: sum of abs differences between horizontally and vertically adjacent pixels.
/// Input is a 3×3 patch stored row-major as signed integers.
fn smoothness_2d(patch: &[i32; 9]) -> i32 {
    let mut s = 0i32;
    for row in 0..3usize {
        for col in 0..2usize {
            s += (patch[row * 3 + col] - patch[row * 3 + col + 1]).abs();
        }
    }
    for col in 0..3usize {
        for row in 0..2usize {
            s += (patch[row * 3 + col] - patch[(row + 1) * 3 + col]).abs();
        }
    }
    s
}

/// Aletheia flip for positive mask `[[0,0,0],[0,1,0],[0,0,0]]`:
///   cmask = -mask then cmask[mask>0]=0 → cmask = 0 everywhere
///   flip = group ^ abs_mask → only center XOR 1 (F1: toggle LSB)
fn apply_flip_pos(patch: &[i32; 9]) -> [i32; 9] {
    let mut out = *patch;
    out[4] = patch[4] ^ 1;
    out
}

/// Aletheia flip for negative mask `[[0,0,0],[0,-1,0],[0,0,0]]`:
///   cmask = [[0,0,0],[0,1,0],[0,0,0]] (no mask>0 entries to zero out)
///   flip = (group + cmask) ^ abs_mask - cmask
///   Center: (x + 1) ^ 1 - 1  =  F_{-1} shift-flip
fn apply_flip_neg(patch: &[i32; 9]) -> [i32; 9] {
    let mut out = *patch;
    let x = patch[4];
    out[4] = ((x + 1) ^ 1) - 1;
    out
}

/// Count Regular, Singular, and Unusable groups for the given flip function over
/// all 3×3 overlapping patches of `channel` (shape h×w, row-major).
/// Returns (R, S) normalized by total groups N, as (R/N - S/N).
fn rs_difference(channel: &[i32], w: usize, h: usize, positive_mask: bool) -> (f64, f64, f64) {
    let mut r = 0usize;
    let mut s = 0usize;
    let mut total = 0usize;

    for row in 0..(h.saturating_sub(2)) {
        for col in 0..(w.saturating_sub(2)) {
            let mut patch = [0i32; 9];
            for pr in 0..3 {
                for pc in 0..3 {
                    patch[pr * 3 + pc] = channel[(row + pr) * w + (col + pc)];
                }
            }
            let sm_orig = smoothness_2d(&patch);
            let flipped = if positive_mask {
                apply_flip_pos(&patch)
            } else {
                apply_flip_neg(&patch)
            };
            let sm_flip = smoothness_2d(&flipped);
            match sm_flip.cmp(&sm_orig) {
                std::cmp::Ordering::Greater => r += 1,
                std::cmp::Ordering::Less => s += 1,
                std::cmp::Ordering::Equal => {}
            }
            total += 1;
        }
    }

    let n = total as f64;
    let r_norm = r as f64 / n;
    let s_norm = s as f64 / n;
    (r_norm, s_norm, r_norm - s_norm)
}

/// Run Aletheia-faithful RS analysis on a single channel (h×w, row-major, i32).
/// Returns the estimated embedding rate p, and raw R/S counts (usize numerators).
fn rs_channel(channel: &[i32], w: usize, h: usize) -> (f64, usize, usize, usize, usize) {
    // Matches Aletheia naming exactly:
    //   d0   = difference(I,    +mask)
    //   d1   = difference(I^1,  +mask)
    //   n_d0 = difference(I,    -mask)
    //   n_d1 = difference(I^1,  -mask)
    let (r_m_n, s_m_n, d0) = rs_difference(channel, w, h, true);
    let (r_neg_n, s_neg_n, n_d0) = rs_difference(channel, w, h, false);

    let flipped: Vec<i32> = channel.iter().map(|&x| x ^ 1).collect();
    let (_, _, d1) = rs_difference(&flipped, w, h, true);
    let (_, _, n_d1) = rs_difference(&flipped, w, h, false);

    // Quadratic: solve 2*(d1+d0)*z^2 + (n_d0 - n_d1 - d1 - 3*d0)*z + (d0 - n_d0) = 0
    let a = 2.0 * (d1 + d0);
    let b = n_d0 - n_d1 - d1 - 3.0 * d0;
    let c = d0 - n_d0;

    let z = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            0.0
        } else {
            -c / b
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            0.0
        } else {
            let sq = disc.sqrt();
            let z0 = (-b + sq) / (2.0 * a);
            let z1 = (-b - sq) / (2.0 * a);
            if z0.abs() < z1.abs() {
                z0
            } else {
                z1
            }
        }
    };

    let rate = if (z - 0.5).abs() < 1e-12 {
        0.0
    } else {
        z / (z - 0.5)
    };

    let total = (w.saturating_sub(2)) * (h.saturating_sub(2));
    let r_m = (r_m_n * total as f64).round() as usize;
    let s_m = (s_m_n * total as f64).round() as usize;
    let r_neg_m = (r_neg_n * total as f64).round() as usize;
    let s_neg_m = (s_neg_n * total as f64).round() as usize;

    (rate, r_m, s_m, r_neg_m, s_neg_m)
}

#[derive(Debug)]
pub enum StealthError {
    Io(anyhow::Error),
}

impl std::fmt::Display for StealthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StealthError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StealthError {}

/// Aletheia-faithful Fridrich 2001 RS attack on all three RGB channels.
pub fn fridrich_rs_attack(jpeg_path: &Path) -> Result<FridrichRsResult, StealthError> {
    let img = image::open(jpeg_path).map_err(|e| StealthError::Io(anyhow::anyhow!("{e}")))?;
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let (w, h) = (w as usize, h as usize);
    let pixels = rgb.as_raw();

    let mut r_chan = vec![0i32; w * h];
    let mut g_chan = vec![0i32; w * h];
    let mut b_chan = vec![0i32; w * h];
    for i in 0..w * h {
        r_chan[i] = pixels[i * 3] as i32;
        g_chan[i] = pixels[i * 3 + 1] as i32;
        b_chan[i] = pixels[i * 3 + 2] as i32;
    }

    let (rate_r, r_m_r, s_m_r, r_neg_r, s_neg_r) = rs_channel(&r_chan, w, h);
    let (rate_g, r_m_g, s_m_g, r_neg_g, s_neg_g) = rs_channel(&g_chan, w, h);
    let (rate_b, r_m_b, s_m_b, r_neg_b, s_neg_b) = rs_channel(&b_chan, w, h);

    let max_rate = rate_r.abs().max(rate_g.abs()).max(rate_b.abs());
    let threshold = 0.05;
    let mut detected_channels = Vec::new();
    if rate_r.abs() > threshold {
        detected_channels.push("R".to_string());
    }
    if rate_g.abs() > threshold {
        detected_channels.push("G".to_string());
    }
    if rate_b.abs() > threshold {
        detected_channels.push("B".to_string());
    }

    let verdict = if max_rate > threshold {
        "detected".to_string()
    } else {
        "clean".to_string()
    };

    Ok(FridrichRsResult {
        estimated_rate_r: rate_r,
        estimated_rate_g: rate_g,
        estimated_rate_b: rate_b,
        max_rate,
        detected_channels,
        raw_counts: FridrichRsCounts {
            r_m_red: r_m_r,
            s_m_red: s_m_r,
            r_neg_m_red: r_neg_r,
            s_neg_m_red: s_neg_r,
            r_m_green: r_m_g,
            s_m_green: s_m_g,
            r_neg_m_green: r_neg_g,
            s_neg_m_green: s_neg_g,
            r_m_blue: r_m_b,
            s_m_blue: s_m_b,
            r_neg_m_blue: r_neg_b,
            s_neg_m_blue: s_neg_b,
        },
        verdict,
    })
}

// ── SPA Attack ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaResult {
    pub estimated_rate_y: f64,
    pub verdict: String,
}

pub fn spa_attack(pixels: &[u8], threshold: f64) -> SpaResult {
    let n = pixels.len() / 2;
    let mut x = 0usize;
    let mut y = 0usize;
    let mut z = 0usize;
    let mut w = 0usize;

    for i in 0..n {
        let u = pixels[2 * i];
        let v = pixels[2 * i + 1];
        if u == v {
            x += 1;
        } else {
            let u_is_even = u.is_multiple_of(2);
            let expected_y = if u_is_even {
                u.saturating_add(1)
            } else {
                u.saturating_sub(1)
            };
            let expected_z = if u_is_even {
                u.saturating_sub(1)
            } else {
                u.saturating_add(1)
            };
            if v == expected_y {
                y += 1;
            } else if v == expected_z {
                z += 1;
            } else {
                w += 1;
            }
        }
    }

    let x = x as f64;
    let y = y as f64;
    let z = z as f64;
    let w = w as f64;

    // 0.5*(W+Z)*p^2 - (2*X+W+Z)*p + 2*(Y-X) = 0
    let a = 0.5 * (w + z);
    let b = -(2.0 * x + w + z);
    let c = 2.0 * (y - x);

    let rate = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            0.0
        } else {
            (-c / b).clamp(0.0, 1.0)
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            0.0
        } else {
            let sqrt_disc = disc.sqrt();
            let r1 = (-b + sqrt_disc) / (2.0 * a);
            let r2 = (-b - sqrt_disc) / (2.0 * a);
            let r1_valid = (0.0..=1.0).contains(&r1);
            let r2_valid = (0.0..=1.0).contains(&r2);
            if r1_valid && r2_valid {
                if r1 < r2 {
                    r1
                } else {
                    r2
                }
            } else if r1_valid {
                r1
            } else if r2_valid {
                r2
            } else {
                0.0
            }
        }
    };

    let verdict = if rate > threshold {
        "detected".to_string()
    } else {
        "clean".to_string()
    };
    SpaResult {
        estimated_rate_y: rate,
        verdict,
    }
}

// ── Chi-Square on DCT Histogram ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChiSquareResult {
    pub p_value: f64,
    pub statistic: f64,
    pub df: usize,
}

pub fn chi_square_dct(comp: &JpegComponent) -> ChiSquareResult {
    let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    for (idx, &c) in comp.coefficients.iter().enumerate() {
        // Skip DC coefficients (position 0 in each 64-coeff block)
        if idx % 64 == 0 {
            continue;
        }
        if c != 0 {
            *counts.entry(c.abs() as i32).or_insert(0) += 1;
        }
    }

    let mut chi2 = 0.0f64;
    let mut df = 0usize;

    // Pair (2k, 2k+1) for k >= 1
    for k in 1..128i32 {
        let a = *counts.get(&(2 * k)).unwrap_or(&0) as f64;
        let b = *counts.get(&(2 * k + 1)).unwrap_or(&0) as f64;
        if a + b < 4.0 {
            continue;
        }
        let expected = (a + b) / 2.0;
        chi2 += (a - expected).powi(2) / expected + (b - expected).powi(2) / expected;
        df += 1;
    }

    if df == 0 {
        return ChiSquareResult {
            p_value: 1.0,
            statistic: 0.0,
            df: 0,
        };
    }

    let chi_dist = ChiSquared::new(df as f64).unwrap();
    let p_value = 1.0 - chi_dist.cdf(chi2);

    ChiSquareResult {
        p_value,
        statistic: chi2,
        df,
    }
}

// ── ±1 Transition Ratio ──────────────────────────────────────────────────────

pub fn pm1_transition_ratio(comp: &JpegComponent) -> f64 {
    let bw = comp.blocks_wide;
    let bh = comp.blocks_high;
    let mut pm1_count = 0u64;
    let mut total = 0u64;

    for br in 0..bh {
        for bc in 0..bw {
            let base = (br * bw + bc) * 64;
            let block = &comp.coefficients[base..base + 64];
            // 8x8 block: horizontal pairs (within rows)
            for row in 0..8usize {
                for col in 0..7usize {
                    let a = block[row * 8 + col].abs() as i32;
                    let b = block[row * 8 + col + 1].abs() as i32;
                    if (a - b).abs() == 1 {
                        pm1_count += 1;
                    }
                    total += 1;
                }
            }
            // Vertical pairs (within columns)
            for row in 0..7usize {
                for col in 0..8usize {
                    let a = block[row * 8 + col].abs() as i32;
                    let b = block[(row + 1) * 8 + col].abs() as i32;
                    if (a - b).abs() == 1 {
                        pm1_count += 1;
                    }
                    total += 1;
                }
            }
        }
    }

    if total == 0 {
        0.0
    } else {
        pm1_count as f64 / total as f64
    }
}

// ── Non-Zero AC Count ─────────────────────────────────────────────────────────

pub fn nonzero_ac_count(comp: &JpegComponent) -> usize {
    comp.coefficients
        .iter()
        .enumerate()
        .filter(|&(idx, &v)| idx % 64 != 0 && v != 0)
        .count()
}

// ── LSB Entropy ──────────────────────────────────────────────────────────────

pub fn lsb_entropy(comp: &JpegComponent) -> f64 {
    let mut count1 = 0u64;
    let mut total = 0u64;

    for (idx, &c) in comp.coefficients.iter().enumerate() {
        if idx % 64 == 0 {
            continue;
        }
        if c != 0 {
            if (c.unsigned_abs() as u64) & 1 == 1 {
                count1 += 1;
            }
            total += 1;
        }
    }

    if total == 0 {
        return 0.0;
    }
    let p1 = count1 as f64 / total as f64;
    let p0 = 1.0 - p1;

    let h0 = if p0 > 0.0 { -p0 * p0.log2() } else { 0.0 };
    let h1 = if p1 > 0.0 { -p1 * p1.log2() } else { 0.0 };
    h0 + h1
}

// ── Histogram Total Variation ─────────────────────────────────────────────────

pub fn histogram_tv(comp: &JpegComponent) -> f64 {
    const CAP: usize = 50;
    let mut hist = [0u64; CAP + 1];

    for (idx, &c) in comp.coefficients.iter().enumerate() {
        if idx % 64 == 0 {
            continue;
        }
        if c != 0 {
            let abs = c.unsigned_abs() as usize;
            if abs <= CAP {
                hist[abs] += 1;
            }
        }
    }

    let log_hist: Vec<f64> = hist.iter().map(|&v| (v as f64 + 1.0).ln()).collect();
    let mut tv = 0.0f64;
    for i in 1..=CAP {
        tv += (log_hist[i] - log_hist[i - 1]).abs();
    }
    tv
}

// ── Full stealth analysis ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrmLiteStats {
    pub stego_l2_norm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YComponentStats {
    pub ac_coefficient_count: usize,
    pub nonzero_ac_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonzero_ac_delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonzero_ac_delta_pct: Option<f64>,
    pub lsb_entropy_bits: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsb_entropy_delta: Option<f64>,
    pub pm1_transition_ratio: f64,
    pub histogram_tv: f64,
    pub chi_square: ChiSquareResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthReport {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    pub format: String,
    pub dimensions: [u32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_estimate: Option<u8>,
    pub y_component: YComponentStats,
    pub rs_attack: RsResult,
    pub fridrich_rs: FridrichRsResult,
    pub spa_attack: SpaResult,
    pub srm_lite: SrmLiteStats,
    pub verdict_flags: Vec<String>,
    pub overall_verdict: String,
}

pub fn analyze_stealth(
    stego_path: &Path,
    cover_path: Option<&Path>,
    threshold: f64,
) -> Result<StealthReport> {
    let stego_jpeg = phantasm_image::jpeg::read(stego_path)
        .with_context(|| format!("reading stego JPEG: {}", stego_path.display()))?;

    let stego_img = image::open(stego_path)
        .with_context(|| format!("opening stego image: {}", stego_path.display()))?;

    let stego_y_plane = extract_y_plane(&stego_img);

    let cover_jpeg_opt = if let Some(cp) = cover_path {
        Some(
            phantasm_image::jpeg::read(cp)
                .with_context(|| format!("reading cover JPEG: {}", cp.display()))?,
        )
    } else {
        None
    };

    let y_comp = stego_jpeg.components.first().context("no Y component")?;

    let ac_total = y_comp
        .coefficients
        .len()
        .saturating_sub(y_comp.blocks_wide * y_comp.blocks_high);
    let nonzero = nonzero_ac_count(y_comp);

    let (nonzero_delta, nonzero_delta_pct, lsb_entropy_delta) = if let Some(ref cj) = cover_jpeg_opt
    {
        let cover_y = cj.components.first().context("no cover Y component")?;
        let cover_nz = nonzero_ac_count(cover_y);
        let delta = nonzero as i64 - cover_nz as i64;
        let pct = if cover_nz > 0 {
            delta as f64 / cover_nz as f64 * 100.0
        } else {
            0.0
        };
        let cover_ent = lsb_entropy(cover_y);
        let stego_ent = lsb_entropy(y_comp);
        (Some(delta), Some(pct), Some(stego_ent - cover_ent))
    } else {
        (None, None, None)
    };

    let chi_sq = chi_square_dct(y_comp);
    let pm1 = pm1_transition_ratio(y_comp);
    let ent = lsb_entropy(y_comp);
    let htv = histogram_tv(y_comp);
    let rs = rs_attack(&stego_y_plane, threshold);
    let spa = spa_attack(&stego_y_plane, threshold);
    let fridrich_rs =
        fridrich_rs_attack(stego_path).map_err(|e| anyhow::anyhow!("fridrich_rs_attack: {e}"))?;

    let mut flags: Vec<String> = Vec::new();
    if rs.estimated_rate_y > threshold {
        flags.push(format!(
            "rs-attack p={:.3} > {threshold:.2}",
            rs.estimated_rate_y
        ));
    }
    if fridrich_rs.max_rate > threshold {
        flags.push(format!(
            "fridrich-rs max_rate={:.3} > {threshold:.2}",
            fridrich_rs.max_rate
        ));
    }
    if pm1 > 0.12 {
        flags.push(format!("pm1-transition {pm1:.3} > 0.12"));
    }
    if let Some(delta) = lsb_entropy_delta {
        if delta.abs() > 0.05 {
            let sign = if delta < 0.0 { "-" } else { "+" };
            flags.push(format!(
                "lsb-entropy anomaly ({sign}{:.0}%)",
                delta.abs() * 100.0
            ));
        }
    } else {
        // single-file mode: flag if entropy differs much from typical 0.88
        let deviation = (ent - 0.88).abs();
        if deviation > 0.05 {
            flags.push(format!(
                "lsb-entropy {ent:.4} (deviation from typical 0.88)"
            ));
        }
    }
    if chi_sq.p_value < 0.01 {
        flags.push(format!("chi-square p={:.4} < 0.01", chi_sq.p_value));
    }
    if htv > 22.0 {
        flags.push(format!("histogram-tv {htv:.3} > 22.0"));
    }

    // SRM-lite
    let stego_srm = SrmLiteFeatures::compute(stego_path)?;
    let stego_l2_norm = stego_srm
        .values
        .iter()
        .map(|v| v.powi(2))
        .sum::<f64>()
        .sqrt();

    let srm_lite = if let Some(cp) = cover_path {
        let cover_srm = SrmLiteFeatures::compute(cp)?;
        let dist = cover_srm.l2_distance(&stego_srm);
        // Threshold 0.020: empirically, clean vs. clean JPEG noise is <0.005;
        // uniform LSB-flip embedding at full capacity exceeds 0.020 comfortably.
        let srm_verdict = if dist > 0.020 {
            flags.push(format!("srm-lite l2-distance {dist:.4} > 0.020"));
            "detected".to_string()
        } else {
            "clean".to_string()
        };
        SrmLiteStats {
            stego_l2_norm,
            l2_distance: Some(dist),
            verdict: Some(srm_verdict),
        }
    } else {
        SrmLiteStats {
            stego_l2_norm,
            l2_distance: None,
            verdict: None,
        }
    };

    let overall = if flags.is_empty() {
        "clean".to_string()
    } else {
        "detected".to_string()
    };

    Ok(StealthReport {
        file: stego_path.to_string_lossy().to_string(),
        cover: cover_path.map(|p| p.to_string_lossy().to_string()),
        format: "JPEG".to_string(),
        dimensions: [stego_jpeg.width, stego_jpeg.height],
        quality_estimate: stego_jpeg.quality_estimate,
        y_component: YComponentStats {
            ac_coefficient_count: ac_total,
            nonzero_ac_count: nonzero,
            nonzero_ac_delta: nonzero_delta,
            nonzero_ac_delta_pct: nonzero_delta_pct,
            lsb_entropy_bits: ent,
            lsb_entropy_delta,
            pm1_transition_ratio: pm1,
            histogram_tv: htv,
            chi_square: chi_sq,
        },
        rs_attack: rs,
        fridrich_rs,
        spa_attack: spa,
        srm_lite,
        verdict_flags: flags,
        overall_verdict: overall,
    })
}

// ── SRM-lite ─────────────────────────────────────────────────────────────────

/// 196-feature SRM-lite feature vector for a single image.
#[derive(Debug, Clone)]
pub struct SrmLiteFeatures {
    pub values: [f64; 196],
}

impl SrmLiteFeatures {
    /// Compute the SRM-lite feature vector from a JPEG path. Decodes to grayscale internally.
    pub fn compute(jpeg_path: &Path) -> Result<Self, anyhow::Error> {
        let img = image::open(jpeg_path)
            .with_context(|| format!("opening image for SRM-lite: {}", jpeg_path.display()))?;
        let gray = img.to_luma8();
        let (w, h) = gray.dimensions();
        Ok(Self::from_gray_pixels(
            gray.as_raw(),
            w as usize,
            h as usize,
        ))
    }

    /// Compute from a raw grayscale pixel slice (row-major, width × height).
    pub fn from_gray_pixels(pixels: &[u8], w: usize, h: usize) -> Self {
        let residuals = [
            compute_r1(pixels, w, h),
            compute_r2(pixels, w, h),
            compute_r3(pixels, w, h),
            compute_r4(pixels, w, h),
        ];

        let mut values = [0.0f64; 196];
        for (ri, res) in residuals.iter().enumerate() {
            let cooc = cooc_horizontal(res, w, h);
            let total: u64 = cooc.iter().sum();
            let base = ri * 49;
            if total > 0 {
                let total_f = total as f64;
                for (i, &c) in cooc.iter().enumerate() {
                    values[base + i] = c as f64 / total_f;
                }
            }
        }

        SrmLiteFeatures { values }
    }

    /// L2 (Euclidean) distance between two feature vectors.
    pub fn l2_distance(&self, other: &SrmLiteFeatures) -> f64 {
        self.values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}

const T: i32 = 3;

fn truncate(v: i32) -> i32 {
    v.clamp(-T, T)
}

fn compute_r1(pixels: &[u8], w: usize, h: usize) -> Vec<Option<i32>> {
    let mut res = vec![None; w * h];
    for i in 0..h {
        for j in 0..w.saturating_sub(1) {
            let v = pixels[i * w + j + 1] as i32 - pixels[i * w + j] as i32;
            res[i * w + j] = Some(truncate(v));
        }
    }
    res
}

fn compute_r2(pixels: &[u8], w: usize, h: usize) -> Vec<Option<i32>> {
    let mut res = vec![None; w * h];
    for i in 0..h.saturating_sub(1) {
        for j in 0..w {
            let v = pixels[(i + 1) * w + j] as i32 - pixels[i * w + j] as i32;
            res[i * w + j] = Some(truncate(v));
        }
    }
    res
}

fn compute_r3(pixels: &[u8], w: usize, h: usize) -> Vec<Option<i32>> {
    let mut res = vec![None; w * h];
    if h < 2 || w < 2 {
        return res;
    }
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let v = pixels[(i - 1) * w + (j - 1)] as i32 - 2 * pixels[i * w + j] as i32
                + pixels[(i + 1) * w + (j + 1)] as i32;
            res[i * w + j] = Some(truncate(v));
        }
    }
    res
}

fn compute_r4(pixels: &[u8], w: usize, h: usize) -> Vec<Option<i32>> {
    let mut res = vec![None; w * h];
    if h < 2 || w < 2 {
        return res;
    }
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let p = |row: usize, col: usize| pixels[row * w + col] as i32;
            let v = -p(i - 1, j - 1) + 2 * p(i - 1, j) - p(i - 1, j + 1) + 2 * p(i, j - 1)
                - 4 * p(i, j)
                + 2 * p(i, j + 1)
                - p(i + 1, j - 1)
                + 2 * p(i + 1, j)
                - p(i + 1, j + 1);
            res[i * w + j] = Some(truncate(v));
        }
    }
    res
}

// 7x7 co-occurrence matrix (flattened as 49 entries) for horizontally adjacent pairs.
fn cooc_horizontal(res: &[Option<i32>], w: usize, h: usize) -> [u64; 49] {
    let mut cooc = [0u64; 49];
    let size = 2 * T + 1; // 7
    for i in 0..h {
        for j in 0..w.saturating_sub(1) {
            if let (Some(a), Some(b)) = (res[i * w + j], res[i * w + j + 1]) {
                let ai = (a + T) as usize;
                let bi = (b + T) as usize;
                cooc[ai * size as usize + bi] += 1;
            }
        }
    }
    cooc
}

fn extract_y_plane(img: &image::DynamicImage) -> Vec<u8> {
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let pixels = rgb.as_raw();
    let mut y_plane = Vec::with_capacity((w * h) as usize);
    for i in 0..(w * h) as usize {
        let r = pixels[i * 3] as f64;
        let g = pixels[i * 3 + 1] as f64;
        let b = pixels[i * 3 + 2] as f64;
        let y = (0.299 * r + 0.587 * g + 0.114 * b).round() as u8;
        y_plane.push(y);
    }
    y_plane
}
