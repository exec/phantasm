use std::path::PathBuf;

use image::RgbImage;
use phantasm_bench::metrics::{dhash_hamming, mse, phash_hamming, psnr, ssim_grayscale};
use phantasm_bench::report::{BenchSummary, PairResult};
use phantasm_bench::steganalyzer::{NullDetector, Steganalyzer};
use tempfile::TempDir;

// ── Fixture helpers ──────────────────────────────────────────────────────────

fn save_rgb(pixels: &[u8], width: u32, height: u32, path: &std::path::Path) {
    let img = RgbImage::from_raw(width, height, pixels.to_vec()).unwrap();
    img.save(path).unwrap();
}

fn solid_rgb(val: u8, w: u32, h: u32) -> Vec<u8> {
    vec![val; (w * h * 3) as usize]
}

fn gradient_rgb(w: u32, h: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y) % 256) as u8;
            buf.push(v);
            buf.push(v.wrapping_add(50));
            buf.push(v.wrapping_add(100));
        }
    }
    buf
}

// ── MSE tests ────────────────────────────────────────────────────────────────

#[test]
fn test_mse_identical() {
    let buf = solid_rgb(128, 8, 8);
    assert_eq!(mse(&buf, &buf), 0.0);
}

#[test]
fn test_mse_known() {
    let a = vec![0u8; 4];
    let b = vec![1u8; 4];
    assert!((mse(&a, &b) - 1.0).abs() < 1e-10);
}

// ── PSNR tests ───────────────────────────────────────────────────────────────

#[test]
fn test_psnr_identical() {
    let buf = solid_rgb(64, 8, 8);
    assert_eq!(psnr(&buf, &buf), f64::INFINITY);
}

#[test]
fn test_psnr_known() {
    // mse=1 → psnr = 10*log10(255^2 / 1) = 48.13 dB
    let a = vec![0u8; 4];
    let b = vec![1u8; 4];
    let val = psnr(&a, &b);
    assert!((val - 48.1308).abs() < 0.01, "psnr={val}");
}

// ── SSIM tests ───────────────────────────────────────────────────────────────

fn gradient_gray(w: u32, h: u32) -> Vec<u8> {
    (0..(w * h))
        .map(|i| ((i * 37 + i / w * 11) % 256) as u8)
        .collect()
}

#[test]
fn test_ssim_identical() {
    let buf = gradient_gray(64, 64);
    let val = ssim_grayscale(&buf, &buf, 64, 64);
    assert!((val - 1.0).abs() < 1e-6, "ssim identical={val}");
}

#[test]
fn test_ssim_symmetry() {
    let a = gradient_gray(64, 64);
    let mut b = a.clone();
    // Shift by 5 pixels (wrap)
    b.rotate_right(5);
    let ab = ssim_grayscale(&a, &b, 64, 64);
    let ba = ssim_grayscale(&b, &a, 64, 64);
    assert!((ab - ba).abs() < 1e-10, "ssim not symmetric: {ab} vs {ba}");
}

// ── pHash tests ──────────────────────────────────────────────────────────────

#[test]
fn test_phash_identical() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("img.png");
    let pixels = gradient_rgb(64, 64);
    save_rgb(&pixels, 64, 64, &path);
    let dist = phash_hamming(&path, &path).unwrap();
    assert_eq!(dist, 0);
}

#[test]
fn test_phash_slight_perturbation() {
    let tmp = TempDir::new().unwrap();
    let path_a = tmp.path().join("a.png");
    let path_b = tmp.path().join("b.png");
    let mut pixels = gradient_rgb(512, 512);
    save_rgb(&pixels, 512, 512, &path_a);
    // Flip one pixel in the middle
    pixels[512 * 256 * 3] ^= 1;
    save_rgb(&pixels, 512, 512, &path_b);
    let dist = phash_hamming(&path_a, &path_b).unwrap();
    // pHash is robust to single-pixel changes → hamming distance 0
    assert_eq!(
        dist, 0,
        "expected pHash to be robust to single-pixel change, got {dist}"
    );
}

// ── dHash tests ──────────────────────────────────────────────────────────────

#[test]
fn test_dhash_identical() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("img.png");
    let pixels = gradient_rgb(64, 64);
    save_rgb(&pixels, 64, 64, &path);
    let dist = dhash_hamming(&path, &path).unwrap();
    assert_eq!(dist, 0);
}

// ── NullDetector ─────────────────────────────────────────────────────────────

#[test]
fn test_null_detector() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("img.png");
    let pixels = gradient_rgb(32, 32);
    save_rgb(&pixels, 32, 32, &path);
    let det = NullDetector;
    let score = det.detect(&path).unwrap();
    assert!((score - 0.5).abs() < 1e-10);
}

// ── BenchSummary tests ───────────────────────────────────────────────────────

fn make_pair(mse: f64, psnr_db: f64, ssim: f64) -> PairResult {
    PairResult {
        cover: PathBuf::from("cover.png"),
        stego: PathBuf::from("stego.png"),
        mse,
        psnr_db,
        ssim,
        phash_hamming: 0,
        dhash_hamming: 0,
        file_size_delta: 0,
        steganalyzer_scores: vec![],
        embed_ms: None,
        extract_ms: None,
        roundtrip_ok: None,
    }
}

#[test]
fn test_bench_summary_aggregation() {
    let pairs = vec![
        make_pair(1.0, 40.0, 0.9),
        make_pair(2.0, 30.0, 0.8),
        make_pair(3.0, 20.0, 0.7),
    ];
    let summary = BenchSummary::from_pairs(pairs);
    assert!((summary.mean_mse - 2.0).abs() < 1e-10);
    assert!((summary.mean_psnr_db - 30.0).abs() < 1e-10);
    assert!((summary.mean_ssim - 0.8).abs() < 1e-10);
    assert_eq!(summary.pair_count, 3);
}

#[test]
fn test_bench_summary_json_roundtrip() {
    let pairs = vec![make_pair(1.5, 45.0, 0.95)];
    let summary = BenchSummary::from_pairs(pairs);
    let json = summary.to_json().unwrap();
    let parsed: BenchSummary = serde_json::from_str(&json).unwrap();
    assert!((parsed.mean_mse - 1.5).abs() < 1e-10);
    assert!((parsed.mean_ssim - 0.95).abs() < 1e-10);
}
