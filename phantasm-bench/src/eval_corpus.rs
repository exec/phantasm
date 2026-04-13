use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use phantasm_core::channel::ChannelProfile;
use phantasm_core::content_adaptive::ContentAdaptiveOrchestrator;
use phantasm_core::orchestrator::Orchestrator;
use phantasm_core::plan::{EmbedPlan, HashSensitivity};
use phantasm_core::stealth::StealthTier;
use phantasm_cost::uerd::Uerd;
use phantasm_cost::{DistortionFunction, Uniform};
use phantasm_image::jpeg::read as jpeg_read;

use crate::metrics::{dhash_hamming, file_size_delta, mse, phash_hamming, psnr, ssim_grayscale};
use crate::stealth::analyze_stealth;

// ── CLI args ─────────────────────────────────────────────────────────────────

/// Payload source: fixed file or capacity-fraction.
#[derive(Debug, Clone)]
pub enum PayloadSource {
    File(PathBuf),
    Fraction(f64),
    FractionSweep(Vec<f64>),
}

#[derive(Debug, Clone)]
pub struct EvalCorpusArgs {
    pub corpus: PathBuf,
    pub payload_source: PayloadSource,
    pub cost_functions: Vec<String>,
    pub passphrase_prefix: String,
    pub limit: Option<usize>,
    pub output: PathBuf,
    pub markdown: Option<PathBuf>,
    pub threads: usize,
}

impl Default for EvalCorpusArgs {
    fn default() -> Self {
        Self {
            corpus: PathBuf::from("."),
            payload_source: PayloadSource::File(PathBuf::new()),
            cost_functions: vec!["uniform".into(), "uerd".into()],
            passphrase_prefix: "phantasm-corpus-eval-v1".into(),
            limit: None,
            output: PathBuf::from("corpus-eval-results.json"),
            markdown: None,
            threads: 1,
        }
    }
}

// ── Per-image metrics record ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerImageMetrics {
    pub mse: f64,
    pub psnr_db: f64,
    pub ssim: f64,
    pub phash_hamming: u32,
    pub dhash_hamming: u32,
    pub file_size_delta: i64,
    pub rs_rate_y: f64,
    pub spa_rate_y: f64,
    pub pm1_transition_ratio: f64,
    pub lsb_entropy: f64,
    pub histogram_tv: f64,
    pub nonzero_ac_delta: Option<i64>,
    pub overall_verdict_detected: bool,
    pub embed_ms: f64,
    pub capacity_used_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerImageEntry {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniform: Option<PerImageMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uerd: Option<PerImageMetrics>,
}

// ── Aggregate statistics ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistStats {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub p10: f64,
    pub p25: f64,
    pub p75: f64,
    pub p90: f64,
    pub stddev: f64,
    pub min: f64,
    pub max: f64,
}

fn compute_dist_stats(values: &[f64]) -> DistStats {
    let count = values.len();
    if count == 0 {
        return DistStats {
            count: 0,
            mean: 0.0,
            median: 0.0,
            p10: 0.0,
            p25: 0.0,
            p75: 0.0,
            p90: 0.0,
            stddev: 0.0,
            min: 0.0,
            max: 0.0,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mean = sorted.iter().sum::<f64>() / count as f64;
    let variance = sorted.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
    let stddev = variance.sqrt();

    let pct = |p: f64| -> f64 {
        let idx = (p / 100.0 * (count - 1) as f64).round() as usize;
        sorted[idx.min(count - 1)]
    };

    DistStats {
        count,
        mean,
        median: pct(50.0),
        p10: pct(10.0),
        p25: pct(25.0),
        p75: pct(75.0),
        p90: pct(90.0),
        stddev,
        min: sorted[0],
        max: sorted[count - 1],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostFunctionStats {
    pub count: usize,
    pub skipped_count: usize,
    pub skipped_reasons: HashMap<String, usize>,
    pub metrics: HashMap<String, DistStats>,
    pub overall_verdict_detected_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedMetricComparison {
    pub mean_paired_delta: f64,
    pub median_paired_delta: f64,
    pub p10_paired_delta: f64,
    pub p90_paired_delta: f64,
    pub images_where_uerd_better: usize,
    pub win_rate_uerd: f64,
}

// ── JSON output schema ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCorpusResult {
    pub generated_at: String,
    pub corpus: String,
    pub corpus_image_count: usize,
    pub images_processed: usize,
    pub payload_path: String,
    pub payload_bytes: usize,
    pub cost_functions: Vec<String>,
    pub per_cost_function: HashMap<String, CostFunctionStats>,
    pub paired_comparison: HashMap<String, PairedMetricComparison>,
    pub images: Vec<PerImageEntry>,
}

// ── Sweep output schema ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepRun {
    pub fraction: f64,
    pub images_processed: usize,
    pub per_cost_function: HashMap<String, CostFunctionStats>,
    pub paired_comparison: HashMap<String, PairedMetricComparison>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepMetricsByDensity {
    pub ssim_uerd_win_rate: Vec<f64>,
    pub ssim_mean_paired_delta: Vec<f64>,
    pub ssim_median_paired_delta: Vec<f64>,
    pub mse_mean_paired_delta: Vec<f64>,
    pub mse_median_paired_delta: Vec<f64>,
    pub file_size_delta_mean_uerd: Vec<f64>,
    pub file_size_delta_median_uerd: Vec<f64>,
    pub file_size_delta_paired_median: Vec<f64>,
    pub detection_rate_uniform: Vec<f64>,
    pub detection_rate_uerd: Vec<f64>,
    pub pm1_transition_delta_mean: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepSummary {
    pub metric_by_density: SweepMetricsByDensity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DensitySweepResult {
    pub generated_at: String,
    pub corpus: String,
    pub mode: String,
    pub fractions: Vec<f64>,
    pub runs: Vec<SweepRun>,
    pub sweep_summary: SweepSummary,
}

// ── Timestamp helper (no chrono dep) ─────────────────────────────────────────

fn epoch_to_parts(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, hour, min, sec)
}

fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, sec) = epoch_to_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
}

// ── Image discovery ───────────────────────────────────────────────────────────

fn walk_jpeg_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path().to_path_buf();
        if p.is_file() {
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                if matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg") {
                    paths.push(p);
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn deterministic_passphrase(prefix: &str, image_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(image_path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    let hex_prefix: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex_prefix}")
}

fn make_embed_plan() -> EmbedPlan {
    EmbedPlan {
        channel: ChannelProfile::builtin("lossless").expect("lossless channel always exists"),
        stealth_tier: StealthTier::Max,
        capacity_bits: 0,
        payload_bits: 0,
        ecc_bits: 0,
        estimated_detection_error: 0.0,
        hash_constrained_positions: 0,
        hash_sensitivity: HashSensitivity::Robust,
    }
}

fn cost_fn_from_name(name: &str) -> Box<dyn DistortionFunction> {
    match name {
        "uerd" => Box::new(Uerd),
        _ => Box::new(Uniform),
    }
}

// ── Cover capacity computation ────────────────────────────────────────────────

/// Returns the maximum practical payload in bytes that can actually be embedded
/// into this cover image by the current naive MinimalOrchestrator /
/// ContentAdaptiveOrchestrator pipeline, accounting for:
///
/// 1. STC rate 1/4 (orchestrator uses `inverse_rate = 4`), so the usable STC
///    message is `ac_positions / 4` bits = `ac_positions / 32` bytes.
/// 2. Envelope padding to fixed power-of-two block sizes
///    `{256, 1024, 4096, 16384, 65536, 262144}`, so the effective payload
///    is the largest block ≤ stc-byte-capacity minus crypto overhead.
/// 3. Fixed crypto overhead: salt 32 + nonce 24 + AEAD tag 16 + metadata ~20
///    + 4-byte length framing ≈ 100 bytes.
///
/// This is the value that a `--capacity-fraction F` parameter should be applied
/// against so that `F=0.8` embeds ~80% of what's actually achievable rather
/// than overshooting into `PayloadTooLarge` territory.
pub fn compute_cover_capacity_bytes(path: &Path) -> Result<usize> {
    let jpeg = jpeg_read(path).with_context(|| format!("reading JPEG {:?}", path))?;
    if jpeg.components.is_empty() {
        return Ok(0);
    }
    let y = &jpeg.components[0];
    let ac_positions = y.blocks_wide * y.blocks_high * 63;
    // STC bit budget and byte budget (orchestrator uses inverse_rate=4).
    let stc_byte_capacity = ac_positions / 32;
    // Envelope padding tiers (must match phantasm_crypto::padding).
    const BLOCK_TIERS: &[usize] = &[256, 1024, 4096, 16384, 65536, 262144];
    const ENVELOPE_OVERHEAD: usize = 100;
    let max_fit_block = BLOCK_TIERS
        .iter()
        .rev()
        .copied()
        .find(|&b| b <= stc_byte_capacity)
        .unwrap_or(0);
    Ok(max_fit_block.saturating_sub(ENVELOPE_OVERHEAD))
}

// ── Per-image embedding + analysis ───────────────────────────────────────────

fn process_one_image(
    cover_path: &Path,
    payload: &[u8],
    passphrase: &str,
    cost_name: &str,
    plan: &EmbedPlan,
) -> Result<PerImageMetrics, String> {
    let tmp = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .map_err(|e| format!("tempfile: {e}"))?;
    let stego_path = tmp.path().to_path_buf();

    let orchestrator = ContentAdaptiveOrchestrator::new(cost_fn_from_name(cost_name));

    let t0 = Instant::now();
    let embed_result = orchestrator
        .embed(cover_path, payload, passphrase, plan, &stego_path)
        .map_err(|e| e.to_string())?;
    let embed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let cover_img = image::open(cover_path).map_err(|e| e.to_string())?;
    let stego_img = image::open(&stego_path).map_err(|e| e.to_string())?;

    let (w, h) = (cover_img.width(), cover_img.height());
    let stego_img = if stego_img.width() != w || stego_img.height() != h {
        stego_img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
    } else {
        stego_img
    };

    let cover_rgb = cover_img.to_rgb8();
    let stego_rgb = stego_img.to_rgb8();
    let cover_gray = cover_img.to_luma8();
    let stego_gray = stego_img.to_luma8();

    let mse_val = mse(cover_rgb.as_raw(), stego_rgb.as_raw());
    let psnr_val = psnr(cover_rgb.as_raw(), stego_rgb.as_raw());
    let ssim_val = ssim_grayscale(cover_gray.as_raw(), stego_gray.as_raw(), w, h);
    let phash_val = phash_hamming(cover_path, &stego_path).map_err(|e| e.to_string())?;
    let dhash_val = dhash_hamming(cover_path, &stego_path).map_err(|e| e.to_string())?;
    let size_delta = file_size_delta(cover_path, &stego_path).map_err(|e| e.to_string())?;

    let stealth =
        analyze_stealth(&stego_path, Some(cover_path), 0.05).map_err(|e| e.to_string())?;

    let detected = stealth.overall_verdict == "detected";

    Ok(PerImageMetrics {
        mse: mse_val,
        psnr_db: psnr_val,
        ssim: ssim_val,
        phash_hamming: phash_val,
        dhash_hamming: dhash_val,
        file_size_delta: size_delta,
        rs_rate_y: stealth.rs_attack.estimated_rate_y,
        spa_rate_y: stealth.spa_attack.estimated_rate_y,
        pm1_transition_ratio: stealth.y_component.pm1_transition_ratio,
        lsb_entropy: stealth.y_component.lsb_entropy_bits,
        histogram_tv: stealth.y_component.histogram_tv,
        nonzero_ac_delta: stealth.y_component.nonzero_ac_delta,
        overall_verdict_detected: detected,
        embed_ms,
        capacity_used_ratio: embed_result.capacity_used_ratio,
    })
}

// ── Aggregation helpers ───────────────────────────────────────────────────────

fn aggregate_metrics(results: &[PerImageMetrics]) -> HashMap<String, DistStats> {
    let mut map = HashMap::new();

    macro_rules! agg {
        ($name:expr, $field:expr) => {
            let vals: Vec<f64> = $field;
            map.insert($name.to_string(), compute_dist_stats(&vals));
        };
    }

    agg!("mse", results.iter().map(|r| r.mse).collect());
    agg!("psnr_db", results.iter().map(|r| r.psnr_db).collect());
    agg!("ssim", results.iter().map(|r| r.ssim).collect());
    agg!(
        "phash_hamming",
        results.iter().map(|r| r.phash_hamming as f64).collect()
    );
    agg!(
        "dhash_hamming",
        results.iter().map(|r| r.dhash_hamming as f64).collect()
    );
    agg!(
        "file_size_delta",
        results.iter().map(|r| r.file_size_delta as f64).collect()
    );
    agg!("rs_rate_y", results.iter().map(|r| r.rs_rate_y).collect());
    agg!("spa_rate_y", results.iter().map(|r| r.spa_rate_y).collect());
    agg!(
        "pm1_transition_ratio",
        results.iter().map(|r| r.pm1_transition_ratio).collect()
    );
    agg!(
        "lsb_entropy",
        results.iter().map(|r| r.lsb_entropy).collect()
    );
    agg!(
        "histogram_tv",
        results.iter().map(|r| r.histogram_tv).collect()
    );
    agg!(
        "nonzero_ac_delta",
        results
            .iter()
            .filter_map(|r| r.nonzero_ac_delta.map(|v| v as f64))
            .collect()
    );
    agg!("embed_ms", results.iter().map(|r| r.embed_ms).collect());
    agg!(
        "capacity_used_ratio",
        results.iter().map(|r| r.capacity_used_ratio).collect()
    );

    map
}

fn uerd_wins(metric: &str, uniform_val: f64, uerd_val: f64) -> bool {
    let lower_better = matches!(
        metric,
        "mse"
            | "phash_hamming"
            | "dhash_hamming"
            | "file_size_delta"
            | "rs_rate_y"
            | "spa_rate_y"
            | "pm1_transition_ratio"
            | "histogram_tv"
            | "embed_ms"
    );
    if metric == "nonzero_ac_delta" {
        return uerd_val.abs() < uniform_val.abs();
    }
    if lower_better {
        uerd_val < uniform_val
    } else {
        uerd_val > uniform_val
    }
}

fn compute_paired_comparison(
    uniform_results: &HashMap<String, PerImageMetrics>,
    uerd_results: &HashMap<String, PerImageMetrics>,
) -> HashMap<String, PairedMetricComparison> {
    let mut comparisons: HashMap<String, Vec<(f64, f64)>> = HashMap::new();

    for (path, u_m) in uniform_results {
        if let Some(e_m) = uerd_results.get(path) {
            macro_rules! pair {
                ($name:expr, $u:expr, $e:expr) => {
                    comparisons
                        .entry($name.to_string())
                        .or_default()
                        .push(($u, $e));
                };
            }
            pair!("mse", u_m.mse, e_m.mse);
            pair!("psnr_db", u_m.psnr_db, e_m.psnr_db);
            pair!("ssim", u_m.ssim, e_m.ssim);
            pair!(
                "phash_hamming",
                u_m.phash_hamming as f64,
                e_m.phash_hamming as f64
            );
            pair!(
                "dhash_hamming",
                u_m.dhash_hamming as f64,
                e_m.dhash_hamming as f64
            );
            pair!(
                "file_size_delta",
                u_m.file_size_delta as f64,
                e_m.file_size_delta as f64
            );
            pair!("rs_rate_y", u_m.rs_rate_y, e_m.rs_rate_y);
            pair!("spa_rate_y", u_m.spa_rate_y, e_m.spa_rate_y);
            pair!(
                "pm1_transition_ratio",
                u_m.pm1_transition_ratio,
                e_m.pm1_transition_ratio
            );
            pair!("lsb_entropy", u_m.lsb_entropy, e_m.lsb_entropy);
            pair!("histogram_tv", u_m.histogram_tv, e_m.histogram_tv);
            pair!("embed_ms", u_m.embed_ms, e_m.embed_ms);
            pair!(
                "capacity_used_ratio",
                u_m.capacity_used_ratio,
                e_m.capacity_used_ratio
            );
            if let (Some(uv), Some(ev)) = (u_m.nonzero_ac_delta, e_m.nonzero_ac_delta) {
                pair!("nonzero_ac_delta", uv as f64, ev as f64);
            }
        }
    }

    let mut result = HashMap::new();
    for (metric, pairs) in &comparisons {
        let n = pairs.len();
        if n == 0 {
            continue;
        }
        let deltas: Vec<f64> = pairs.iter().map(|(u, e)| e - u).collect();
        let mut sorted_deltas = deltas.clone();
        sorted_deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mean_delta = deltas.iter().sum::<f64>() / n as f64;
        let median_delta = sorted_deltas[sorted_deltas.len() / 2];
        let p10 = sorted_deltas[(sorted_deltas.len() as f64 * 0.1) as usize];
        let p90_idx = ((sorted_deltas.len() as f64 * 0.9) as usize).min(sorted_deltas.len() - 1);
        let p90 = sorted_deltas[p90_idx];
        let wins = pairs
            .iter()
            .filter(|(u, e)| uerd_wins(metric, *u, *e))
            .count();

        result.insert(
            metric.clone(),
            PairedMetricComparison {
                mean_paired_delta: mean_delta,
                median_paired_delta: median_delta,
                p10_paired_delta: p10,
                p90_paired_delta: p90,
                images_where_uerd_better: wins,
                win_rate_uerd: wins as f64 / n as f64,
            },
        );
    }
    result
}

// ── Markdown report (fixed payload) ──────────────────────────────────────────

fn build_markdown(result: &EvalCorpusResult) -> String {
    let mut md = String::new();
    md.push_str("# Phantasm Corpus Eval Report\n\n");
    md.push_str(&format!(
        "**Corpus:** {} ({} images)  \n",
        result.corpus, result.corpus_image_count
    ));
    md.push_str(&format!(
        "**Payload:** {} ({} bytes)  \n",
        result.payload_path, result.payload_bytes
    ));
    md.push_str(&format!("**Generated:** {}  \n\n", result.generated_at));

    let cf_names = &result.cost_functions;

    let metrics = [
        "ssim",
        "mse",
        "psnr_db",
        "phash_hamming",
        "dhash_hamming",
        "file_size_delta",
        "rs_rate_y",
        "spa_rate_y",
        "pm1_transition_ratio",
        "lsb_entropy",
        "histogram_tv",
        "nonzero_ac_delta",
        "embed_ms",
        "capacity_used_ratio",
    ];

    md.push_str("## Per-Metric Comparison\n\n");
    md.push_str("| Metric |");
    for cf in cf_names {
        md.push_str(&format!(" {cf} mean | {cf} median |"));
    }
    if cf_names.len() == 2
        && cf_names.contains(&"uniform".to_string())
        && cf_names.contains(&"uerd".to_string())
    {
        md.push_str(" UERD win rate |");
    }
    md.push('\n');

    md.push_str("|--------|");
    for _ in cf_names {
        md.push_str("----------|-----------|");
    }
    if cf_names.len() == 2
        && cf_names.contains(&"uniform".to_string())
        && cf_names.contains(&"uerd".to_string())
    {
        md.push_str("--------------|");
    }
    md.push('\n');

    for m in &metrics {
        md.push_str(&format!("| {m} |"));
        for cf in cf_names {
            if let Some(cf_stats) = result.per_cost_function.get(cf) {
                if let Some(ds) = cf_stats.metrics.get(*m) {
                    md.push_str(&format!(" {:.4} | {:.4} |", ds.mean, ds.median));
                } else {
                    md.push_str(" — | — |");
                }
            } else {
                md.push_str(" — | — |");
            }
        }
        if cf_names.len() == 2
            && cf_names.contains(&"uniform".to_string())
            && cf_names.contains(&"uerd".to_string())
        {
            if let Some(pc) = result.paired_comparison.get(*m) {
                md.push_str(&format!(" {:.1}% |", pc.win_rate_uerd * 100.0));
            } else {
                md.push_str(" — |");
            }
        }
        md.push('\n');
    }

    md.push_str("\n## Summary\n\n");
    if let Some(pc) = result.paired_comparison.get("ssim") {
        md.push_str(&format!(
            "- SSIM: UERD wins in **{:.1}%** of images (median delta: {:+.4})\n",
            pc.win_rate_uerd * 100.0,
            pc.median_paired_delta
        ));
    }
    if let Some(pc) = result.paired_comparison.get("mse") {
        md.push_str(&format!(
            "- MSE: UERD wins in **{:.1}%** of images (median delta: {:+.4})\n",
            pc.win_rate_uerd * 100.0,
            pc.median_paired_delta
        ));
    }
    if let Some(pc) = result.paired_comparison.get("file_size_delta") {
        md.push_str(&format!(
            "- File size delta: UERD wins in **{:.1}%** of images (median delta: {:+.0} bytes)\n",
            pc.win_rate_uerd * 100.0,
            pc.median_paired_delta
        ));
    }
    if let (Some(u), Some(e)) = (
        result.per_cost_function.get("uniform"),
        result.per_cost_function.get("uerd"),
    ) {
        md.push_str(&format!(
            "- Detection rate: uniform={:.1}%, uerd={:.1}%\n",
            u.overall_verdict_detected_fraction * 100.0,
            e.overall_verdict_detected_fraction * 100.0
        ));
    }

    md
}

// ── Markdown sweep report ─────────────────────────────────────────────────────

/// Returns true if all cost functions in `run` processed zero images
/// (i.e. every image in the corpus hit PayloadTooLarge or a similar error
/// at this density).
fn run_is_empty(run: &SweepRun) -> bool {
    run.per_cost_function.values().all(|s| s.count == 0)
}

fn build_sweep_markdown(sweep: &DensitySweepResult, corpus_image_count: usize) -> String {
    let mut md = String::new();
    md.push_str("# Phantasm Density-Sweep Report\n\n");
    md.push_str(&format!(
        "**Corpus:** {} ({} images)  \n",
        sweep.corpus, corpus_image_count
    ));
    let frac_strs: Vec<String> = sweep
        .fractions
        .iter()
        .map(|f| format!("{:.2}", f))
        .collect();
    md.push_str(&format!("**Densities:** [{}]  \n\n", frac_strs.join(", ")));

    let sm = &sweep.sweep_summary.metric_by_density;

    // SSIM table
    md.push_str("## SSIM paired delta (UERD - Uniform) across densities\n\n");
    md.push_str("| Density | SSIM win rate | Mean Δ | Median Δ | p10 Δ | p90 Δ |\n");
    md.push_str("|---------|---------------|--------|----------|-------|-------|\n");
    for (i, run) in sweep.runs.iter().enumerate() {
        let frac_pct = run.fraction * 100.0;
        if run_is_empty(run) {
            md.push_str(&format!(
                "| {frac_pct:.0}% | — (insufficient data: all images skipped) | — | — | — | — |\n"
            ));
            continue;
        }
        let win_rate = sm.ssim_uerd_win_rate.get(i).copied().unwrap_or(0.0);
        let mean_d = sm.ssim_mean_paired_delta.get(i).copied().unwrap_or(0.0);
        let med_d = sm.ssim_median_paired_delta.get(i).copied().unwrap_or(0.0);
        let p10_d = run
            .paired_comparison
            .get("ssim")
            .map(|p| p.p10_paired_delta)
            .unwrap_or(0.0);
        let p90_d = run
            .paired_comparison
            .get("ssim")
            .map(|p| p.p90_paired_delta)
            .unwrap_or(0.0);
        md.push_str(&format!(
            "| {frac_pct:.0}% | {:.1}% | {:+.4} | {:+.4} | {:+.4} | {:+.4} |\n",
            win_rate * 100.0,
            mean_d,
            med_d,
            p10_d,
            p90_d,
        ));
    }

    // File inflation table
    md.push_str("\n## File inflation (bytes)\n\n");
    md.push_str("| Density | Uniform mean | UERD mean | Paired median Δ |\n");
    md.push_str("|---------|--------------|-----------|------------------|\n");
    for (i, run) in sweep.runs.iter().enumerate() {
        let frac_pct = run.fraction * 100.0;
        if run_is_empty(run) {
            md.push_str(&format!("| {frac_pct:.0}% | — | — | — |\n"));
            continue;
        }
        let uni_mean = run
            .per_cost_function
            .get("uniform")
            .and_then(|s| s.metrics.get("file_size_delta"))
            .map(|d| d.mean)
            .unwrap_or(0.0);
        let uerd_mean = sm.file_size_delta_mean_uerd.get(i).copied().unwrap_or(0.0);
        let paired_med = sm
            .file_size_delta_paired_median
            .get(i)
            .copied()
            .unwrap_or(0.0);
        md.push_str(&format!(
            "| {frac_pct:.0}% | {uni_mean:.0} | {uerd_mean:.0} | {paired_med:+.0} |\n"
        ));
    }

    // Detection rate table
    md.push_str("\n## Classical detection rate\n\n");
    md.push_str("| Density | Uniform overall_verdict=detected | UERD overall_verdict=detected |\n");
    md.push_str("|---------|----------------------------------|-------------------------------|\n");
    for (i, run) in sweep.runs.iter().enumerate() {
        let frac_pct = run.fraction * 100.0;
        if run_is_empty(run) {
            md.push_str(&format!("| {frac_pct:.0}% | — | — |\n"));
            continue;
        }
        let uni_det = sm.detection_rate_uniform.get(i).copied().unwrap_or(0.0);
        let uerd_det = sm.detection_rate_uerd.get(i).copied().unwrap_or(0.0);
        md.push_str(&format!(
            "| {frac_pct:.0}% | {:.1}% | {:.1}% |\n",
            uni_det * 100.0,
            uerd_det * 100.0,
        ));
    }

    // Takeaway narrative
    md.push_str("\n## Takeaway\n\n");
    // Only consider densities with real data (non-empty runs)
    let valid_indices: Vec<usize> = sweep
        .runs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| if run_is_empty(r) { None } else { Some(i) })
        .collect();
    if valid_indices.is_empty() {
        md.push_str("All density fractions produced zero successful embeds. Every image hit PayloadTooLarge at every density; the `--capacity-fractions` are too aggressive for this corpus + orchestrator combination, OR there is a bug in the capacity computation. Check `compute_cover_capacity_bytes` logic and re-run with smaller fractions.\n");
        return md;
    }
    // Find where UERD advantage is largest on SSIM, among valid densities only
    let max_ssim_win_idx = valid_indices
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let av = sm.ssim_uerd_win_rate.get(a).copied().unwrap_or(0.0);
            let bv = sm.ssim_uerd_win_rate.get(b).copied().unwrap_or(0.0);
            av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let max_frac_pct = sweep
        .fractions
        .get(max_ssim_win_idx)
        .copied()
        .unwrap_or(0.0)
        * 100.0;

    // Check if UERD detection drops below 100% at any VALID density (skip zero-data rows)
    let uerd_evasion = valid_indices
        .iter()
        .copied()
        .find_map(|i| {
            sm.detection_rate_uerd.get(i).and_then(|&r| {
                if r < 1.0 {
                    Some((i, r))
                } else {
                    None
                }
            })
        });

    let mut takeaway = format!(
        "UERD's SSIM advantage is largest at {:.0}% payload density (win rate {:.1}%). ",
        max_frac_pct,
        sm.ssim_uerd_win_rate
            .get(max_ssim_win_idx)
            .copied()
            .unwrap_or(0.0)
            * 100.0,
    );

    // Check whether advantage shrinks across valid densities
    let first_valid = *valid_indices.first().unwrap();
    let last_valid = *valid_indices.last().unwrap();
    let first_rate = sm.ssim_uerd_win_rate.get(first_valid).copied().unwrap_or(0.0);
    let last_rate = sm.ssim_uerd_win_rate.get(last_valid).copied().unwrap_or(0.0);
    if last_rate < first_rate {
        let last_frac_pct = sweep.fractions.get(last_valid).copied().unwrap_or(0.0) * 100.0;
        takeaway.push_str(&format!(
            "At the highest *valid* density ({last_frac_pct:.0}%) UERD's win rate drops to {:.1}%, suggesting advantage shrinks near capacity saturation. ",
            last_rate * 100.0,
        ));
    }

    if let Some((idx, rate)) = uerd_evasion {
        let frac_pct = sweep.fractions.get(idx).copied().unwrap_or(0.0) * 100.0;
        takeaway.push_str(&format!(
            "**HEADLINE: UERD evades classical detection at {:.0}% density (detection rate {:.1}%)** — this is the key security result, showing steganographic undetectability is achievable at practical payload sizes.",
            frac_pct,
            rate * 100.0,
        ));
    } else {
        takeaway.push_str(
            "Classical RS/SPA detection fires on both methods across all valid densities tested; UERD's advantage is quantitative (lower distortion) rather than a full evasion of statistical detection at these densities.",
        );
    }

    // If any densities were skipped, document that separately so the reader knows
    // what they're not seeing.
    let skipped: Vec<f64> = sweep
        .runs
        .iter()
        .filter_map(|r| if run_is_empty(r) { Some(r.fraction) } else { None })
        .collect();
    if !skipped.is_empty() {
        let skipped_pct: Vec<String> = skipped.iter().map(|f| format!("{:.0}%", f * 100.0)).collect();
        takeaway.push_str(&format!(
            "\n\n**Skipped densities:** {} — every image at these fractions hit `PayloadTooLarge`. These rows are omitted from the analysis above.",
            skipped_pct.join(", "),
        ));
    }

    md.push_str(&takeaway);
    md.push('\n');

    md
}

// ── Core eval loop (single density) ──────────────────────────────────────────

struct SingleRunResult {
    images_processed: usize,
    per_cost_function: HashMap<String, CostFunctionStats>,
    paired_comparison: HashMap<String, PairedMetricComparison>,
    image_entries: Vec<PerImageEntry>,
    payload_bytes: usize,
}

fn run_single_density(
    all_images: &[PathBuf],
    corpus: &Path,
    payload_source: &PayloadSource,
    cost_functions: &[String],
    passphrase_prefix: &str,
) -> Result<SingleRunResult> {
    let images_to_process = all_images.len();
    let plan = make_embed_plan();

    let mut cf_success: HashMap<String, HashMap<String, PerImageMetrics>> = HashMap::new();
    let mut cf_skipped: HashMap<String, usize> = HashMap::new();
    let mut cf_skip_reasons: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for cf in cost_functions {
        cf_success.insert(cf.clone(), HashMap::new());
        cf_skipped.insert(cf.clone(), 0);
        cf_skip_reasons.insert(cf.clone(), HashMap::new());
    }

    let mut image_entries: Vec<PerImageEntry> = all_images
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(corpus)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string();
            PerImageEntry {
                path: rel,
                uniform: None,
                uerd: None,
            }
        })
        .collect();

    // Track aggregate payload size for reporting
    let mut total_payload_bytes = 0usize;
    let mut payload_count = 0usize;

    for (idx, cover_path) in all_images.iter().enumerate() {
        let passphrase = deterministic_passphrase(passphrase_prefix, cover_path);

        let payload = match payload_source {
            PayloadSource::File(path) => {
                std::fs::read(path).with_context(|| format!("reading payload {:?}", path))?
            }
            PayloadSource::Fraction(frac) => {
                match compute_cover_capacity_bytes(cover_path) {
                    Ok(cap) => {
                        let target = ((cap as f64 * frac).floor() as usize).max(16);
                        let mut buf = vec![0u8; target];
                        OsRng.fill_bytes(&mut buf);
                        buf
                    }
                    Err(_) => {
                        // Skip all cost functions for this image
                        for cf_name in cost_functions {
                            *cf_skipped.get_mut(cf_name).unwrap() += 1;
                            *cf_skip_reasons
                                .get_mut(cf_name)
                                .unwrap()
                                .entry("capacity_read_error".to_string())
                                .or_insert(0) += 1;
                        }
                        continue;
                    }
                }
            }
            PayloadSource::FractionSweep(_) => {
                anyhow::bail!(
                    "FractionSweep should be handled by run_sweep, not run_single_density"
                )
            }
        };

        total_payload_bytes += payload.len();
        payload_count += 1;

        for cf_name in cost_functions {
            match process_one_image(cover_path, &payload, &passphrase, cf_name, &plan) {
                Ok(metrics) => {
                    let path_key = cover_path.to_string_lossy().to_string();
                    match cf_name.as_str() {
                        "uniform" => image_entries[idx].uniform = Some(metrics.clone()),
                        "uerd" => image_entries[idx].uerd = Some(metrics.clone()),
                        _ => {}
                    }
                    cf_success
                        .get_mut(cf_name)
                        .unwrap()
                        .insert(path_key, metrics);
                }
                Err(e) => {
                    *cf_skipped.get_mut(cf_name).unwrap() += 1;
                    let reasons = cf_skip_reasons.get_mut(cf_name).unwrap();
                    let key = if e.contains("payload too large") || e.contains("PayloadTooLarge") {
                        "PayloadTooLarge".to_string()
                    } else {
                        let shortened: String = e.chars().take(80).collect();
                        shortened
                    };
                    *reasons.entry(key).or_insert(0) += 1;
                }
            }
        }
    }

    let mut per_cost_function: HashMap<String, CostFunctionStats> = HashMap::new();
    for cf_name in cost_functions {
        let success_map = cf_success.get(cf_name).unwrap();
        let results: Vec<PerImageMetrics> = success_map.values().cloned().collect();
        let count = results.len();
        let detected_count = results
            .iter()
            .filter(|r| r.overall_verdict_detected)
            .count();
        let detected_fraction = if count > 0 {
            detected_count as f64 / count as f64
        } else {
            0.0
        };
        let metrics = aggregate_metrics(&results);

        per_cost_function.insert(
            cf_name.clone(),
            CostFunctionStats {
                count,
                skipped_count: *cf_skipped.get(cf_name).unwrap(),
                skipped_reasons: cf_skip_reasons.get(cf_name).unwrap().clone(),
                metrics,
                overall_verdict_detected_fraction: detected_fraction,
            },
        );
    }

    let paired_comparison = if cost_functions.contains(&"uniform".to_string())
        && cost_functions.contains(&"uerd".to_string())
    {
        let uniform_map = cf_success.get("uniform").unwrap();
        let uerd_map = cf_success.get("uerd").unwrap();
        compute_paired_comparison(uniform_map, uerd_map)
    } else {
        HashMap::new()
    };

    let avg_payload = if payload_count > 0 {
        total_payload_bytes / payload_count
    } else {
        0
    };

    Ok(SingleRunResult {
        images_processed: images_to_process,
        per_cost_function,
        paired_comparison,
        image_entries,
        payload_bytes: avg_payload,
    })
}

// ── Main entry points ─────────────────────────────────────────────────────────

pub fn run_eval_corpus(args: &EvalCorpusArgs) -> Result<EvalCorpusResult> {
    // Validate: FractionSweep goes to sweep path
    if matches!(&args.payload_source, PayloadSource::FractionSweep(_)) {
        anyhow::bail!("Use run_density_sweep for FractionSweep mode");
    }

    let mut all_images = walk_jpeg_files(&args.corpus)
        .with_context(|| format!("walking corpus {:?}", args.corpus))?;
    let corpus_image_count = all_images.len();

    if let Some(limit) = args.limit {
        all_images.truncate(limit);
    }

    // For file-based payload, read it once to get the size for reporting
    let (payload_path_str, payload_bytes_for_report) = match &args.payload_source {
        PayloadSource::File(p) => {
            let data = std::fs::read(p).with_context(|| format!("reading payload {:?}", p))?;
            (p.to_string_lossy().to_string(), data.len())
        }
        PayloadSource::Fraction(f) => (format!("<capacity-fraction:{f:.3}>"), 0),
        PayloadSource::FractionSweep(_) => unreachable!(),
    };

    let run = run_single_density(
        &all_images,
        &args.corpus,
        &args.payload_source,
        &args.cost_functions,
        &args.passphrase_prefix,
    )?;

    let payload_bytes = if payload_bytes_for_report > 0 {
        payload_bytes_for_report
    } else {
        run.payload_bytes
    };

    let result = EvalCorpusResult {
        generated_at: now_iso8601(),
        corpus: args.corpus.to_string_lossy().to_string(),
        corpus_image_count,
        images_processed: run.images_processed,
        payload_path: payload_path_str,
        payload_bytes,
        cost_functions: args.cost_functions.clone(),
        per_cost_function: run.per_cost_function,
        paired_comparison: run.paired_comparison,
        images: run.image_entries,
    };

    let json = serde_json::to_string_pretty(&result)?;
    std::fs::write(&args.output, &json)
        .with_context(|| format!("writing output {:?}", args.output))?;

    if let Some(md_path) = &args.markdown {
        let md = build_markdown(&result);
        std::fs::write(md_path, md).with_context(|| format!("writing markdown {:?}", md_path))?;
    }

    Ok(result)
}

pub fn run_density_sweep(args: &EvalCorpusArgs) -> Result<DensitySweepResult> {
    let fractions = match &args.payload_source {
        PayloadSource::FractionSweep(fs) => fs.clone(),
        _ => anyhow::bail!("run_density_sweep requires FractionSweep payload source"),
    };

    let mut all_images = walk_jpeg_files(&args.corpus)
        .with_context(|| format!("walking corpus {:?}", args.corpus))?;
    let corpus_image_count = all_images.len();

    if let Some(limit) = args.limit {
        all_images.truncate(limit);
    }

    let corpus_name = args
        .corpus
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| args.corpus.to_string_lossy().to_string());

    let mut runs = Vec::new();

    for &frac in &fractions {
        eprintln!("[density-sweep] fraction={frac:.2} ({:.0}%)", frac * 100.0);
        let single_source = PayloadSource::Fraction(frac);
        let run = run_single_density(
            &all_images,
            &args.corpus,
            &single_source,
            &args.cost_functions,
            &args.passphrase_prefix,
        )?;

        runs.push(SweepRun {
            fraction: frac,
            images_processed: run.images_processed,
            per_cost_function: run.per_cost_function,
            paired_comparison: run.paired_comparison,
        });
    }

    // Build sweep summary
    let mut ssim_uerd_win_rate = Vec::new();
    let mut ssim_mean_paired_delta = Vec::new();
    let mut ssim_median_paired_delta = Vec::new();
    let mut mse_mean_paired_delta = Vec::new();
    let mut mse_median_paired_delta = Vec::new();
    let mut file_size_delta_mean_uerd = Vec::new();
    let mut file_size_delta_median_uerd = Vec::new();
    let mut file_size_delta_paired_median = Vec::new();
    let mut detection_rate_uniform = Vec::new();
    let mut detection_rate_uerd = Vec::new();
    let mut pm1_transition_delta_mean = Vec::new();

    for run in &runs {
        if let Some(pc) = run.paired_comparison.get("ssim") {
            ssim_uerd_win_rate.push(pc.win_rate_uerd);
            ssim_mean_paired_delta.push(pc.mean_paired_delta);
            ssim_median_paired_delta.push(pc.median_paired_delta);
        } else {
            ssim_uerd_win_rate.push(0.0);
            ssim_mean_paired_delta.push(0.0);
            ssim_median_paired_delta.push(0.0);
        }

        if let Some(pc) = run.paired_comparison.get("mse") {
            mse_mean_paired_delta.push(pc.mean_paired_delta);
            mse_median_paired_delta.push(pc.median_paired_delta);
        } else {
            mse_mean_paired_delta.push(0.0);
            mse_median_paired_delta.push(0.0);
        }

        let uerd_fsd_mean = run
            .per_cost_function
            .get("uerd")
            .and_then(|s| s.metrics.get("file_size_delta"))
            .map(|d| d.mean)
            .unwrap_or(0.0);
        let uerd_fsd_median = run
            .per_cost_function
            .get("uerd")
            .and_then(|s| s.metrics.get("file_size_delta"))
            .map(|d| d.median)
            .unwrap_or(0.0);
        file_size_delta_mean_uerd.push(uerd_fsd_mean);
        file_size_delta_median_uerd.push(uerd_fsd_median);

        let fsd_paired_med = run
            .paired_comparison
            .get("file_size_delta")
            .map(|p| p.median_paired_delta)
            .unwrap_or(0.0);
        file_size_delta_paired_median.push(fsd_paired_med);

        let uni_det = run
            .per_cost_function
            .get("uniform")
            .map(|s| s.overall_verdict_detected_fraction)
            .unwrap_or(0.0);
        let uerd_det = run
            .per_cost_function
            .get("uerd")
            .map(|s| s.overall_verdict_detected_fraction)
            .unwrap_or(0.0);
        detection_rate_uniform.push(uni_det);
        detection_rate_uerd.push(uerd_det);

        let pm1_mean = run
            .paired_comparison
            .get("pm1_transition_ratio")
            .map(|p| p.mean_paired_delta)
            .unwrap_or(0.0);
        pm1_transition_delta_mean.push(pm1_mean);
    }

    let sweep_summary = SweepSummary {
        metric_by_density: SweepMetricsByDensity {
            ssim_uerd_win_rate,
            ssim_mean_paired_delta,
            ssim_median_paired_delta,
            mse_mean_paired_delta,
            mse_median_paired_delta,
            file_size_delta_mean_uerd,
            file_size_delta_median_uerd,
            file_size_delta_paired_median,
            detection_rate_uniform,
            detection_rate_uerd,
            pm1_transition_delta_mean,
        },
    };

    let sweep = DensitySweepResult {
        generated_at: now_iso8601(),
        corpus: corpus_name,
        mode: "capacity-fraction-sweep".to_string(),
        fractions: fractions.clone(),
        runs,
        sweep_summary,
    };

    let json = serde_json::to_string_pretty(&sweep)?;
    std::fs::write(&args.output, &json)
        .with_context(|| format!("writing output {:?}", args.output))?;

    if let Some(md_path) = &args.markdown {
        let md = build_sweep_markdown(&sweep, corpus_image_count);
        std::fs::write(md_path, md).with_context(|| format!("writing markdown {:?}", md_path))?;
    }

    Ok(sweep)
}
