use std::path::PathBuf;

use assert_cmd::Command;
use image::{ImageFormat, RgbImage};
use phantasm_bench::eval_corpus::{
    run_density_sweep, run_eval_corpus, EvalCorpusArgs, PayloadSource,
};
use tempfile::TempDir;

// Generate a synthetic JPEG cover image in a tempdir
fn write_jpeg(dir: &std::path::Path, name: &str, w: u32, h: u32, seed: u8) -> PathBuf {
    let n = (w as usize) * (h as usize) * 3;
    let pixels: Vec<u8> = (0..n)
        .map(|i| {
            let v = (i as u64)
                .wrapping_mul(6364136223846793005)
                .wrapping_add(seed as u64);
            (v >> 33) as u8
        })
        .collect();
    let img = RgbImage::from_raw(w, h, pixels).unwrap();
    let path = dir.join(name);
    img.save_with_format(&path, ImageFormat::Jpeg).unwrap();
    path
}

fn write_payload(dir: &std::path::Path, size: usize) -> PathBuf {
    let path = dir.join("payload.bin");
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, data).unwrap();
    path
}

// ── Test 1: Smoke test with 2-image fixture ───────────────────────────────────

#[test]
fn test_smoke_two_images() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    write_jpeg(&corpus_dir, "img1.jpg", 256, 256, 1);
    write_jpeg(&corpus_dir, "img2.jpg", 256, 256, 2);

    let payload_path = write_payload(tmp.path(), 128);
    let output_path = tmp.path().join("results.json");

    let args = EvalCorpusArgs {
        corpus: corpus_dir.clone(),
        payload_source: PayloadSource::File(payload_path),
        cost_functions: vec!["uniform".to_string(), "uerd".to_string()],
        passphrase_prefix: "test-v1".to_string(),
        limit: None,
        output: output_path.clone(),
        markdown: None,
        threads: 1,
    };

    let result = run_eval_corpus(&args).expect("eval_corpus should succeed");

    assert!(result.per_cost_function.contains_key("uniform"));
    assert!(result.per_cost_function.contains_key("uerd"));
    assert_eq!(result.corpus_image_count, 2);
    assert_eq!(result.images_processed, 2);
    assert_eq!(result.images.len(), 2);
    assert!(!result.paired_comparison.is_empty());
    assert!(result.paired_comparison.contains_key("ssim"));

    assert!(output_path.exists());
    let json_str = std::fs::read_to_string(&output_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(json["cost_functions"].is_array());
    assert_eq!(json["cost_functions"].as_array().unwrap().len(), 2);
}

// ── Test 2: Skipping on payload-too-large ────────────────────────────────────

#[test]
fn test_skip_payload_too_large() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    write_jpeg(&corpus_dir, "tiny.jpg", 64, 64, 42);

    let payload_path = write_payload(tmp.path(), 10_240);
    let output_path = tmp.path().join("results.json");

    let args = EvalCorpusArgs {
        corpus: corpus_dir,
        payload_source: PayloadSource::File(payload_path),
        cost_functions: vec!["uniform".to_string()],
        passphrase_prefix: "test-v1".to_string(),
        limit: None,
        output: output_path,
        markdown: None,
        threads: 1,
    };

    let result =
        run_eval_corpus(&args).expect("run_eval_corpus must not crash on oversized payload");

    let uniform_stats = result.per_cost_function.get("uniform").unwrap();
    assert_eq!(uniform_stats.skipped_count, 1, "expected 1 skipped image");
    assert_eq!(uniform_stats.count, 0, "expected 0 successfully processed");
    assert!(!uniform_stats.skipped_reasons.is_empty());
}

// ── Test 3: Paired delta sanity ───────────────────────────────────────────────

#[test]
fn test_paired_delta_sanity() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    write_jpeg(&corpus_dir, "a.jpg", 256, 256, 3);
    write_jpeg(&corpus_dir, "b.jpg", 256, 256, 4);

    let payload_path = write_payload(tmp.path(), 64);
    let output_path = tmp.path().join("results.json");

    let args = EvalCorpusArgs {
        corpus: corpus_dir,
        payload_source: PayloadSource::File(payload_path),
        cost_functions: vec!["uniform".to_string(), "uerd".to_string()],
        passphrase_prefix: "test-paired".to_string(),
        limit: None,
        output: output_path,
        markdown: None,
        threads: 1,
    };

    let result = run_eval_corpus(&args).unwrap();
    let ssim_cmp = result.paired_comparison.get("ssim").unwrap();

    assert!(
        ssim_cmp.win_rate_uerd >= 0.0 && ssim_cmp.win_rate_uerd <= 1.0,
        "win_rate_uerd out of range: {}",
        ssim_cmp.win_rate_uerd
    );
    let n_uniform = result.per_cost_function["uniform"].count;
    let n_uerd = result.per_cost_function["uerd"].count;
    let n_paired = n_uniform.min(n_uerd);
    assert!(ssim_cmp.images_where_uerd_better <= n_paired);
}

// ── Test 4: CLI smoke ─────────────────────────────────────────────────────────

#[test]
fn test_cli_eval_corpus_smoke() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    write_jpeg(&corpus_dir, "c1.jpg", 256, 256, 10);
    write_jpeg(&corpus_dir, "c2.jpg", 256, 256, 11);

    let payload_path = write_payload(tmp.path(), 128);
    let output_path = tmp.path().join("out.json");

    let output = Command::cargo_bin("phantasm-bench")
        .unwrap()
        .args([
            "eval-corpus",
            "--corpus",
            corpus_dir.to_str().unwrap(),
            "--payload",
            payload_path.to_str().unwrap(),
            "--cost-functions",
            "uniform",
            "--limit",
            "2",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "phantasm-bench eval-corpus failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(output_path.exists(), "JSON output file not created");
    let json_str = std::fs::read_to_string(&output_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    let cfs = json["cost_functions"].as_array().unwrap();
    assert_eq!(cfs.len(), 1);
    assert_eq!(cfs[0].as_str().unwrap(), "uniform");
}

// ── Test 5: capacity-fraction flag parses and dispatches ─────────────────────

#[test]
fn test_capacity_fraction_payload_size() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    // 256x256 images give roughly 256*256/64 * 63 / 8 ≈ 8064 bytes raw capacity
    write_jpeg(&corpus_dir, "cf1.jpg", 256, 256, 20);
    write_jpeg(&corpus_dir, "cf2.jpg", 256, 256, 21);

    let output_path = tmp.path().join("results.json");

    let args = EvalCorpusArgs {
        corpus: corpus_dir.clone(),
        payload_source: PayloadSource::Fraction(0.1),
        cost_functions: vec!["uniform".to_string()],
        passphrase_prefix: "test-frac".to_string(),
        limit: None,
        output: output_path.clone(),
        markdown: None,
        threads: 1,
    };

    let result = run_eval_corpus(&args).expect("capacity-fraction mode should succeed");

    // Verify at least one image was processed
    let uniform_stats = result.per_cost_function.get("uniform").unwrap();
    assert!(
        uniform_stats.count > 0,
        "expected at least one image processed"
    );

    // The reported payload_bytes should be around 10% of raw capacity
    // For 256x256: blocks=16x16=256, ac_positions=256*63=16128, /8=2016 bytes, 10%=~201
    // We just verify it's in a sane range (> 16 minimum, < full capacity)
    assert!(
        result.payload_bytes >= 16,
        "payload_bytes {} should be >= 16",
        result.payload_bytes
    );
    assert!(
        result.payload_bytes < 3000,
        "payload_bytes {} seems too large for 10% of 256x256",
        result.payload_bytes
    );
}

// ── Test 6: sweep mode produces nested runs array ─────────────────────────────

#[test]
fn test_sweep_mode_produces_runs() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();

    write_jpeg(&corpus_dir, "s1.jpg", 256, 256, 30);
    write_jpeg(&corpus_dir, "s2.jpg", 256, 256, 31);

    let output_path = tmp.path().join("sweep.json");

    let args = EvalCorpusArgs {
        corpus: corpus_dir.clone(),
        payload_source: PayloadSource::FractionSweep(vec![0.05, 0.1]),
        cost_functions: vec!["uniform".to_string(), "uerd".to_string()],
        passphrase_prefix: "test-sweep".to_string(),
        limit: None,
        output: output_path.clone(),
        markdown: None,
        threads: 1,
    };

    let sweep = run_density_sweep(&args).expect("sweep mode should succeed");

    assert_eq!(sweep.runs.len(), 2, "expected 2 runs for 2 fractions");
    assert_eq!(sweep.fractions.len(), 2);
    assert_eq!(sweep.mode, "capacity-fraction-sweep");

    // Verify JSON output
    assert!(output_path.exists());
    let json_str = std::fs::read_to_string(&output_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json["runs"].as_array().unwrap().len(), 2);
}

// ── Test 7: mutual exclusivity enforced via CLI ───────────────────────────────

#[test]
fn test_mutual_exclusivity_cli() {
    let tmp = TempDir::new().unwrap();
    let corpus_dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus_dir).unwrap();
    write_jpeg(&corpus_dir, "x.jpg", 64, 64, 5);

    let payload_path = write_payload(tmp.path(), 32);
    let output_path = tmp.path().join("out.json");

    // --payload and --capacity-fraction together should fail
    let output = Command::cargo_bin("phantasm-bench")
        .unwrap()
        .args([
            "eval-corpus",
            "--corpus",
            corpus_dir.to_str().unwrap(),
            "--payload",
            payload_path.to_str().unwrap(),
            "--capacity-fraction",
            "0.1",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure when --payload and --capacity-fraction are both supplied"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mutually exclusive"),
        "expected 'mutually exclusive' in stderr, got: {stderr}"
    );
}
