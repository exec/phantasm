use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use image::ImageFormat;
use indicatif::{ProgressBar, ProgressStyle};

use phantasm_bench::eval_corpus::{
    run_density_sweep, run_eval_corpus, EvalCorpusArgs, PayloadSource,
};
use phantasm_bench::metrics::{
    dhash_hamming, file_size_delta, mse, phash_hamming, psnr, ssim_grayscale,
};
use phantasm_bench::report::{BenchSummary, PairResult};
use phantasm_bench::stealth;
use phantasm_bench::steganalyzer::{NullDetector, Steganalyzer};

#[derive(Parser)]
#[command(name = "phantasm-bench", about = "Phantasm benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compare {
        cover_dir: PathBuf,
        stego_dir: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        markdown: Option<PathBuf>,
    },
    AnalyzeStealth {
        stego: PathBuf,
        #[arg(long)]
        cover: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "0.05")]
        threshold: f64,
    },
    EvalCorpus {
        #[arg(long)]
        corpus: PathBuf,
        /// Fixed payload file (mutually exclusive with --capacity-fraction and --capacity-fractions)
        #[arg(long)]
        payload: Option<PathBuf>,
        /// Single capacity fraction 0.0..1.0 (mutually exclusive with --payload and --capacity-fractions)
        #[arg(long)]
        capacity_fraction: Option<f64>,
        /// Comma-separated capacity fractions for sweep mode (mutually exclusive with --payload and --capacity-fraction)
        #[arg(long, value_delimiter = ',')]
        capacity_fractions: Option<Vec<f64>>,
        #[arg(long, default_value = "uniform,uerd", value_delimiter = ',')]
        cost_functions: Vec<String>,
        #[arg(long, default_value = "phantasm-corpus-eval-v1")]
        passphrase_prefix: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value = "corpus-eval-results.json")]
        output: PathBuf,
        #[arg(long)]
        markdown: Option<PathBuf>,
        #[arg(long, default_value = "1")]
        threads: usize,
    },
}

fn collect_images(dir: &Path) -> Result<HashMap<String, PathBuf>> {
    let mut map = HashMap::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if matches!(
                    ext_lower.as_str(),
                    "jpg" | "jpeg" | "png" | "bmp" | "tiff" | "webp"
                ) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        map.insert(name.to_string(), path);
                    }
                }
            }
        }
    }
    Ok(map)
}

fn compute_pair(
    cover_path: &Path,
    stego_path: &Path,
    detectors: &[Box<dyn Steganalyzer>],
) -> Result<PairResult> {
    let cover_img = image::open(cover_path)?;
    let stego_img = image::open(stego_path)?;

    // Normalize dimensions for pixel-level metrics: resize stego to cover dims if needed
    let (w, h) = (cover_img.width(), cover_img.height());
    let stego_img = if stego_img.width() != w || stego_img.height() != h {
        stego_img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
    } else {
        stego_img
    };

    let cover_rgb = cover_img.to_rgb8();
    let stego_rgb = stego_img.to_rgb8();

    let cover_bytes = cover_rgb.as_raw();
    let stego_bytes = stego_rgb.as_raw();

    let mse_val = mse(cover_bytes, stego_bytes);
    let psnr_val = psnr(cover_bytes, stego_bytes);

    let cover_gray = cover_img.to_luma8();
    let stego_gray = stego_img.to_luma8();
    let ssim_val = ssim_grayscale(cover_gray.as_raw(), stego_gray.as_raw(), w, h);

    let phash_val = phash_hamming(cover_path, stego_path)?;
    let dhash_val = dhash_hamming(cover_path, stego_path)?;
    let size_delta = file_size_delta(cover_path, stego_path)?;

    let mut steganalyzer_scores = Vec::new();
    for det in detectors {
        let score = det.detect(stego_path)?;
        steganalyzer_scores.push((det.name().to_string(), score));
    }

    Ok(PairResult {
        cover: cover_path.to_path_buf(),
        stego: stego_path.to_path_buf(),
        mse: mse_val,
        psnr_db: psnr_val,
        ssim: ssim_val,
        phash_hamming: phash_val,
        dhash_hamming: dhash_val,
        file_size_delta: size_delta,
        steganalyzer_scores,
        embed_ms: None,
        extract_ms: None,
        roundtrip_ok: None,
    })
}

fn run_compare(
    cover_dir: &Path,
    stego_dir: &Path,
    output: Option<&Path>,
    markdown: Option<&Path>,
) -> Result<()> {
    let cover_images = collect_images(cover_dir)?;
    let stego_images = collect_images(stego_dir)?;

    let mut matched: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    for (name, cover_path) in &cover_images {
        if let Some(stego_path) = stego_images.get(name) {
            matched.push((name.clone(), cover_path.clone(), stego_path.clone()));
        } else {
            eprintln!("WARNING: no stego match for cover file '{name}'");
        }
    }
    for name in stego_images.keys() {
        if !cover_images.contains_key(name) {
            eprintln!("WARNING: no cover match for stego file '{name}'");
        }
    }

    matched.sort_by(|a, b| a.0.cmp(&b.0));

    let detectors: Vec<Box<dyn Steganalyzer>> = vec![Box::new(NullDetector)];

    let pb = ProgressBar::new(matched.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap(),
    );

    let mut results = Vec::new();
    for (name, cover_path, stego_path) in &matched {
        pb.set_message(name.clone());
        match compute_pair(cover_path, stego_path, &detectors) {
            Ok(pair) => results.push(pair),
            Err(e) => eprintln!("WARNING: failed to process pair '{name}': {e}"),
        }
        pb.inc(1);
    }
    pb.finish_with_message("done");

    let summary = BenchSummary::from_pairs(results);
    let json = summary.to_json()?;

    if let Some(out_path) = output {
        std::fs::write(out_path, &json)?;
    } else {
        println!("{json}");
    }

    if let Some(md_path) = markdown {
        std::fs::write(md_path, summary.to_markdown())?;
    }

    Ok(())
}

fn run_analyze_stealth(
    stego: &Path,
    cover: Option<&Path>,
    json_only: bool,
    threshold: f64,
) -> Result<()> {
    let report = stealth::analyze_stealth(stego, cover, threshold)
        .with_context(|| format!("analyzing {}", stego.display()))?;
    let json_str = serde_json::to_string_pretty(&report)?;
    if json_only {
        println!("{json_str}");
        return Ok(());
    }
    println!("{json_str}");
    println!();
    println!("┌─────────────────────────────────────────────────────┐");
    println!("│  Stealth Analysis Summary                           │");
    println!("├──────────────────────────────────┬──────────────────┤");
    println!(
        "│ File                             │ {:<16} │",
        report.file.split('/').next_back().unwrap_or(&report.file)
    );
    println!(
        "│ Dimensions                       │ {}×{}{}│",
        report.dimensions[0],
        report.dimensions[1],
        " ".repeat(16usize.saturating_sub(
            format!("{}×{}", report.dimensions[0], report.dimensions[1]).len() + 1
        ))
    );
    println!(
        "│ Quality estimate                 │ {:<16} │",
        report
            .quality_estimate
            .map(|q| q.to_string())
            .unwrap_or_else(|| "?".to_string())
    );
    println!("├──────────────────────────────────┼──────────────────┤");
    println!(
        "│ RS attack rate                   │ {:<16.4} │",
        report.rs_attack.estimated_rate_y
    );
    println!(
        "│ RS verdict                       │ {:<16} │",
        report.rs_attack.verdict
    );
    println!(
        "│ SPA attack rate                  │ {:<16.4} │",
        report.spa_attack.estimated_rate_y
    );
    println!(
        "│ SPA verdict                      │ {:<16} │",
        report.spa_attack.verdict
    );
    println!("├──────────────────────────────────┼──────────────────┤");
    println!(
        "│ Chi-sq p-value                   │ {:<16.4} │",
        report.y_component.chi_square.p_value
    );
    println!(
        "│ ±1 transition ratio              │ {:<16.4} │",
        report.y_component.pm1_transition_ratio
    );
    println!(
        "│ LSB entropy (bits)               │ {:<16.4} │",
        report.y_component.lsb_entropy_bits
    );
    println!(
        "│ Histogram TV                     │ {:<16.3} │",
        report.y_component.histogram_tv
    );
    println!(
        "│ Non-zero AC count                │ {:<16} │",
        report.y_component.nonzero_ac_count
    );
    if let Some(delta) = report.y_component.nonzero_ac_delta {
        println!("│ Non-zero AC delta                │ {:<+16} │", delta);
    }
    println!("├──────────────────────────────────┼──────────────────┤");
    println!(
        "│ Overall verdict                  │ {:<16} │",
        report.overall_verdict
    );
    println!("└──────────────────────────────────┴──────────────────┘");
    if !report.verdict_flags.is_empty() {
        println!("\nFlags fired:");
        for f in &report.verdict_flags {
            println!("  - {f}");
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compare {
            cover_dir,
            stego_dir,
            output,
            markdown,
        } => run_compare(
            &cover_dir,
            &stego_dir,
            output.as_deref(),
            markdown.as_deref(),
        )?,
        Commands::AnalyzeStealth {
            stego,
            cover,
            json,
            threshold,
        } => {
            run_analyze_stealth(&stego, cover.as_deref(), json, threshold)?;
        }
        Commands::EvalCorpus {
            corpus,
            payload,
            capacity_fraction,
            capacity_fractions,
            cost_functions,
            passphrase_prefix,
            limit,
            output,
            markdown,
            threads,
        } => {
            // Enforce mutual exclusivity
            let flag_count = payload.is_some() as u8
                + capacity_fraction.is_some() as u8
                + capacity_fractions.is_some() as u8;
            if flag_count > 1 {
                anyhow::bail!(
                    "--payload, --capacity-fraction, and --capacity-fractions are mutually exclusive"
                );
            }
            if flag_count == 0 {
                anyhow::bail!(
                    "one of --payload, --capacity-fraction, or --capacity-fractions is required"
                );
            }

            let payload_source = if let Some(path) = payload {
                PayloadSource::File(path)
            } else if let Some(frac) = capacity_fraction {
                PayloadSource::Fraction(frac)
            } else {
                PayloadSource::FractionSweep(capacity_fractions.unwrap())
            };

            let is_sweep = matches!(&payload_source, PayloadSource::FractionSweep(_));

            let args = EvalCorpusArgs {
                corpus,
                payload_source,
                cost_functions,
                passphrase_prefix,
                limit,
                output,
                markdown,
                threads,
            };

            if is_sweep {
                run_density_sweep(&args)?;
            } else {
                run_eval_corpus(&args)?;
            }
        }
    }
    Ok(())
}

// ── Helper: save image buffer to a temp file (used in tests) ────────────────

#[allow(dead_code)]
pub fn save_rgb_to_temp(pixels: &[u8], width: u32, height: u32, fmt: ImageFormat) -> PathBuf {
    use image::RgbImage;
    let dir = std::env::temp_dir();
    let ext = match fmt {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Png => "png",
        _ => "img",
    };
    let path = dir.join(format!(
        "bench_test_{}.{ext}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let img = RgbImage::from_raw(width, height, pixels.to_vec()).unwrap();
    img.save_with_format(&path, fmt).unwrap();
    path
}
