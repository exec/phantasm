//! `research-curve` subcommand: security-capacity curves via the
//! `phantasm_core::research_raw` embedding path.
//!
//! For each (image, cost_fn, bit_count) combination we use
//! `research_raw_embed` to drive STC at an exact target message-bit count
//! (no envelope padding), write the resulting stego JPEG to disk, and run
//! [`crate::stealth::analyze_stealth`] on it. Results are aggregated by
//! (cost_fn, bit_count) and emitted as JSON + Markdown.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use phantasm_core::research_raw::research_raw_embed;
use phantasm_core::CoreError;
use phantasm_image::jpeg as jpeg_io;

use crate::eval_corpus::{cost_fn_from_name, walk_jpeg_files};
use crate::stealth::analyze_stealth;

// ── CLI args ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResearchCurveArgs {
    pub corpus: PathBuf,
    pub cost_functions: Vec<String>,
    pub bit_counts: Vec<usize>,
    pub seed_prefix: String,
    pub limit: Option<usize>,
    pub threshold: f64,
    pub threads: usize,
    pub output: PathBuf,
    pub output_md: Option<PathBuf>,
}

// ── Per-image record ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerImageCurvePoint {
    pub image: String,
    pub cost_function: String,
    pub bit_count: usize,
    pub achievable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fridrich_rs_max_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srm_lite_l2_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stc_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifications: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Aggregate ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveAggregate {
    pub cost_function: String,
    pub bit_count: usize,
    pub achievable_count: usize,
    pub total_count: usize,
    pub fridrich_rs_mean: f64,
    pub fridrich_rs_median: f64,
    pub fridrich_rs_p10: f64,
    pub fridrich_rs_p90: f64,
    pub srm_lite_l2_mean: f64,
    pub srm_lite_l2_median: f64,
    pub detected_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchCurveResult {
    pub generated_at: String,
    pub corpus: String,
    pub seed_prefix: String,
    pub cost_functions: Vec<String>,
    pub bit_counts: Vec<usize>,
    pub images_total: usize,
    pub aggregates: Vec<CurveAggregate>,
    pub points: Vec<PerImageCurvePoint>,
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn run_research_curve(args: &ResearchCurveArgs) -> Result<ResearchCurveResult> {
    let mut images = walk_jpeg_files(&args.corpus)
        .with_context(|| format!("walking corpus {}", args.corpus.display()))?;
    if let Some(limit) = args.limit {
        images.truncate(limit);
    }
    let images_total = images.len();
    if images_total == 0 {
        anyhow::bail!("no .jpg/.jpeg files found in {}", args.corpus.display());
    }

    if args.threads > 0 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build_global();
    }

    let total_iters = images_total * args.cost_functions.len() * args.bit_counts.len();
    let pb = ProgressBar::new(total_iters as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap(),
    );

    let collected: Mutex<Vec<PerImageCurvePoint>> = Mutex::new(Vec::with_capacity(total_iters));

    images.par_iter().for_each(|image_path| {
        let image_label = image_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from)
            .unwrap_or_else(|| image_path.display().to_string());

        let cover = match jpeg_io::read(image_path) {
            Ok(c) => c,
            Err(e) => {
                let err = format!("read jpeg: {e}");
                let mut sink = collected.lock().unwrap();
                for cf in &args.cost_functions {
                    for &bc in &args.bit_counts {
                        sink.push(PerImageCurvePoint {
                            image: image_label.clone(),
                            cost_function: cf.clone(),
                            bit_count: bc,
                            achievable: false,
                            fridrich_rs_max_rate: None,
                            srm_lite_l2_distance: None,
                            detected: None,
                            stc_rate: None,
                            modifications: None,
                            error: Some(err.clone()),
                        });
                        pb.inc(1);
                    }
                }
                return;
            }
        };

        let image_hash = sha256_short(image_path);

        for cf_name in &args.cost_functions {
            let cost_fn = cost_fn_from_name(cf_name);
            for &bc in &args.bit_counts {
                let seed = derive_seed(&args.seed_prefix, &image_hash, bc);
                let point = embed_and_analyze(
                    &image_label,
                    image_path,
                    &cover,
                    cf_name,
                    cost_fn.as_ref(),
                    bc,
                    seed,
                    args.threshold,
                );
                collected.lock().unwrap().push(point);
                pb.inc(1);
            }
        }
    });
    pb.finish_with_message("done");

    let mut points = collected.into_inner().unwrap();
    points.sort_by(|a, b| {
        a.cost_function
            .cmp(&b.cost_function)
            .then_with(|| a.bit_count.cmp(&b.bit_count))
            .then_with(|| a.image.cmp(&b.image))
    });

    let aggregates = aggregate(&points, &args.cost_functions, &args.bit_counts);

    let result = ResearchCurveResult {
        generated_at: chrono_now_iso(),
        corpus: args.corpus.display().to_string(),
        seed_prefix: args.seed_prefix.clone(),
        cost_functions: args.cost_functions.clone(),
        bit_counts: args.bit_counts.clone(),
        images_total,
        aggregates,
        points,
    };

    let json = serde_json::to_string_pretty(&result)?;
    std::fs::write(&args.output, json)
        .with_context(|| format!("writing JSON to {}", args.output.display()))?;

    if let Some(md_path) = &args.output_md {
        std::fs::write(md_path, render_markdown(&result))
            .with_context(|| format!("writing markdown to {}", md_path.display()))?;
    }

    Ok(result)
}

// ── Per-(image, cost_fn, bit_count) work unit ────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn embed_and_analyze(
    image_label: &str,
    cover_path: &Path,
    cover: &phantasm_image::jpeg::JpegCoefficients,
    cost_fn_name: &str,
    cost_fn: &dyn phantasm_cost::DistortionFunction,
    bit_count: usize,
    seed: u64,
    threshold: f64,
) -> PerImageCurvePoint {
    let res = match research_raw_embed(cover, cost_fn, bit_count, seed) {
        Ok(r) => r,
        Err(CoreError::PayloadTooLarge { .. }) => {
            return PerImageCurvePoint {
                image: image_label.to_string(),
                cost_function: cost_fn_name.to_string(),
                bit_count,
                achievable: false,
                fridrich_rs_max_rate: None,
                srm_lite_l2_distance: None,
                detected: None,
                stc_rate: None,
                modifications: None,
                error: None,
            };
        }
        Err(e) => {
            return PerImageCurvePoint {
                image: image_label.to_string(),
                cost_function: cost_fn_name.to_string(),
                bit_count,
                achievable: false,
                fridrich_rs_max_rate: None,
                srm_lite_l2_distance: None,
                detected: None,
                stc_rate: None,
                modifications: None,
                error: Some(format!("embed: {e}")),
            };
        }
    };

    let tmp = match tempfile::Builder::new().suffix(".jpg").tempfile() {
        Ok(t) => t,
        Err(e) => {
            return PerImageCurvePoint {
                image: image_label.to_string(),
                cost_function: cost_fn_name.to_string(),
                bit_count,
                achievable: false,
                fridrich_rs_max_rate: None,
                srm_lite_l2_distance: None,
                detected: None,
                stc_rate: Some(res.stc_rate),
                modifications: Some(res.modifications),
                error: Some(format!("tempfile: {e}")),
            };
        }
    };
    let stego_path = tmp.path().to_path_buf();

    if let Err(e) = jpeg_io::write_with_source(&res.stego, cover_path, &stego_path) {
        return PerImageCurvePoint {
            image: image_label.to_string(),
            cost_function: cost_fn_name.to_string(),
            bit_count,
            achievable: false,
            fridrich_rs_max_rate: None,
            srm_lite_l2_distance: None,
            detected: None,
            stc_rate: Some(res.stc_rate),
            modifications: Some(res.modifications),
            error: Some(format!("write jpeg: {e}")),
        };
    }

    let report = match analyze_stealth(&stego_path, Some(cover_path), threshold) {
        Ok(r) => r,
        Err(e) => {
            return PerImageCurvePoint {
                image: image_label.to_string(),
                cost_function: cost_fn_name.to_string(),
                bit_count,
                achievable: false,
                fridrich_rs_max_rate: None,
                srm_lite_l2_distance: None,
                detected: None,
                stc_rate: Some(res.stc_rate),
                modifications: Some(res.modifications),
                error: Some(format!("analyze_stealth: {e}")),
            };
        }
    };

    let fridrich_rs_max_rate = report.fridrich_rs.max_rate;
    let srm_lite_l2_distance = report.srm_lite.l2_distance;
    let detected = fridrich_rs_max_rate > threshold;

    PerImageCurvePoint {
        image: image_label.to_string(),
        cost_function: cost_fn_name.to_string(),
        bit_count,
        achievable: true,
        fridrich_rs_max_rate: Some(fridrich_rs_max_rate),
        srm_lite_l2_distance,
        detected: Some(detected),
        stc_rate: Some(res.stc_rate),
        modifications: Some(res.modifications),
        error: None,
    }
}

// ── Aggregation ──────────────────────────────────────────────────────────────

fn aggregate(
    points: &[PerImageCurvePoint],
    cost_functions: &[String],
    bit_counts: &[usize],
) -> Vec<CurveAggregate> {
    let mut out = Vec::new();
    for cf in cost_functions {
        for &bc in bit_counts {
            let group: Vec<&PerImageCurvePoint> = points
                .iter()
                .filter(|p| p.cost_function == *cf && p.bit_count == bc)
                .collect();
            let total_count = group.len();
            let achievable: Vec<&PerImageCurvePoint> =
                group.iter().filter(|p| p.achievable).copied().collect();
            let achievable_count = achievable.len();

            let frid: Vec<f64> = achievable
                .iter()
                .filter_map(|p| p.fridrich_rs_max_rate)
                .collect();
            let srm: Vec<f64> = achievable
                .iter()
                .filter_map(|p| p.srm_lite_l2_distance)
                .collect();
            let detected_count = achievable
                .iter()
                .filter(|p| p.detected.unwrap_or(false))
                .count();

            out.push(CurveAggregate {
                cost_function: cf.clone(),
                bit_count: bc,
                achievable_count,
                total_count,
                fridrich_rs_mean: mean(&frid),
                fridrich_rs_median: percentile(&frid, 50.0),
                fridrich_rs_p10: percentile(&frid, 10.0),
                fridrich_rs_p90: percentile(&frid, 90.0),
                srm_lite_l2_mean: mean(&srm),
                srm_lite_l2_median: percentile(&srm, 50.0),
                detected_fraction: if achievable_count == 0 {
                    0.0
                } else {
                    detected_count as f64 / achievable_count as f64
                },
            });
        }
    }
    out
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn percentile(xs: &[f64], pct: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut sorted = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (pct / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── Markdown rendering ───────────────────────────────────────────────────────

fn render_markdown(r: &ResearchCurveResult) -> String {
    let mut s = String::new();
    s.push_str("# phantasm research-curve\n\n");
    s.push_str(&format!("- Generated: {}\n", r.generated_at));
    s.push_str(&format!("- Corpus: `{}`\n", r.corpus));
    s.push_str(&format!("- Images: {}\n", r.images_total));
    s.push_str(&format!("- Seed prefix: `{}`\n", r.seed_prefix));
    s.push_str(&format!(
        "- Cost functions: {}\n",
        r.cost_functions.join(", ")
    ));
    s.push_str(&format!(
        "- Bit counts: {}\n\n",
        r.bit_counts
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    s.push_str("## Security-capacity curve\n\n");
    s.push_str("| cost | bits | n/total | det.frac | RS mean | RS p50 | RS p10 | RS p90 | SRM mean | SRM p50 |\n");
    s.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    let mut rows: Vec<&CurveAggregate> = r.aggregates.iter().collect();
    rows.sort_by(|a, b| {
        a.cost_function
            .cmp(&b.cost_function)
            .then_with(|| a.bit_count.cmp(&b.bit_count))
    });
    for a in rows {
        s.push_str(&format!(
            "| {} | {} | {}/{} | {:.3} | {:.4} | {:.4} | {:.4} | {:.4} | {:.3} | {:.3} |\n",
            a.cost_function,
            a.bit_count,
            a.achievable_count,
            a.total_count,
            a.detected_fraction,
            a.fridrich_rs_mean,
            a.fridrich_rs_median,
            a.fridrich_rs_p10,
            a.fridrich_rs_p90,
            a.srm_lite_l2_mean,
            a.srm_lite_l2_median,
        ));
    }
    s.push('\n');
    s
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn sha256_short(path: &Path) -> String {
    let mut h = Sha256::new();
    h.update(path.to_string_lossy().as_bytes());
    let digest = h.finalize();
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn derive_seed(prefix: &str, image_hash: &str, bit_count: usize) -> u64 {
    let mut h = Sha256::new();
    h.update(prefix.as_bytes());
    h.update(b"-");
    h.update(image_hash.as_bytes());
    h.update(b"-");
    h.update(bit_count.to_le_bytes());
    let digest = h.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(out)
}

fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::TempDir;

    fn write_test_jpeg(path: &Path, w: u32, h: u32, seed: u8) {
        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(w, h);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let r = ((x * 255 / w) as u8).wrapping_add(seed);
            let g = ((y * 255 / h) as u8).wrapping_add(seed.wrapping_mul(3));
            let b = ((x + y) as u8).wrapping_mul(5).wrapping_add(seed);
            *pixel = Rgb([r, g, b]);
        }
        img.save(path).expect("write jpeg");
    }

    #[test]
    fn smoke_curve_runs_end_to_end() {
        let tmp = TempDir::new().unwrap();
        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        for i in 0..3u8 {
            write_test_jpeg(&corpus.join(format!("img{i}.jpg")), 256, 256, i + 1);
        }

        let out_json = tmp.path().join("curve.json");
        let out_md = tmp.path().join("curve.md");

        let args = ResearchCurveArgs {
            corpus,
            cost_functions: vec!["uniform".into(), "uerd".into()],
            bit_counts: vec![100, 1000],
            seed_prefix: "phantasm-curve-test-v1".into(),
            limit: None,
            threshold: 0.05,
            threads: 1,
            output: out_json.clone(),
            output_md: Some(out_md.clone()),
        };

        let result = run_research_curve(&args).expect("run curve");
        assert_eq!(result.images_total, 3);
        assert_eq!(result.aggregates.len(), 4); // 2 cost fns x 2 bit counts
        assert_eq!(result.points.len(), 3 * 2 * 2);

        // JSON file is well-formed
        let raw = std::fs::read_to_string(&out_json).unwrap();
        let _parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse json");

        // Markdown table renders
        let md = std::fs::read_to_string(&out_md).unwrap();
        assert!(md.contains("Security-capacity curve"));
        assert!(md.contains("uniform"));
        assert!(md.contains("uerd"));
        assert!(md.contains("| 100 |"));
        assert!(md.contains("| 1000 |"));
    }

    #[test]
    fn seed_is_deterministic() {
        let s1 = derive_seed("prefix", "abcd1234", 100);
        let s2 = derive_seed("prefix", "abcd1234", 100);
        let s3 = derive_seed("prefix", "abcd1234", 200);
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
