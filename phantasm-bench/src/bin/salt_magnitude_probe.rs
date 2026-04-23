//! Diagnostic: measure the per-coefficient drift magnitude in the pHash 8x8
//! block between the cover and the recompressed stego, then simulate step-X
//! salt stability for a range of step values.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use phantasm_core::pipeline::diagnostics::{embed_capture_pre_stc, phash_block_of_jpeg};
use phantasm_core::{ChannelAdapter, TwitterProfile};
use phantasm_cost::{DistortionFunction, Juniward};
use phantasm_image::jpeg as pjpeg;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long, default_value = "40")]
    limit: usize,
    #[arg(long, default_value = "1000")]
    payload_size: usize,
    #[arg(long, default_value = "85")]
    recompress_qf: u8,
    #[arg(long, default_value_t = false)]
    no_adapter: bool,
}

fn deterministic_passphrase(prefix: &str, image_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(image_path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    let hex_prefix: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex_prefix}")
}

fn recompress_twitter(input: &Path, output: &Path, qf: u8) -> Result<()> {
    let img = image::open(input)
        .with_context(|| format!("decoding {}", input.display()))?
        .to_rgb8();
    let mut out = std::fs::File::create(output)?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, qf);
    img.write_with_encoder(encoder)?;
    Ok(())
}

fn walk_jpegs(corpus: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(corpus).sort_by_file_name() {
        let e = entry?;
        if e.file_type().is_file() {
            let p = e.path().to_path_buf();
            if let Some(ext) = p.extension() {
                let e = ext.to_string_lossy().to_ascii_lowercase();
                if e == "jpg" || e == "jpeg" {
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

fn quantize(v: f64, step: f64) -> i32 {
    (v / step).round() as i32
}

fn salt_stable(cover_block: &[f64], reenc_block: &[f64], step: f64) -> bool {
    cover_block
        .iter()
        .zip(reenc_block.iter())
        .all(|(a, b)| quantize(*a, step) == quantize(*b, step))
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut images = walk_jpegs(&args.corpus)?;
    images.truncate(args.limit);

    let mut rng = StdRng::seed_from_u64(0xB3E_2024);
    let mut payload = vec![0u8; args.payload_size];
    rng.fill_bytes(&mut payload);

    let juniward = Juniward;
    let steps: Vec<f64> = vec![8.0, 16.0, 32.0, 64.0, 128.0, 256.0];
    let mut stable_counts = vec![0usize; steps.len()];
    let mut max_abs_drift = 0.0_f64;
    let mut drifts_sum = 0.0_f64;
    let mut drifts_n = 0usize;

    for cover in &images {
        let image_key = cover
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        let passphrase = deterministic_passphrase("phantasm-ber-sweep-v1", cover);

        let jpeg = pjpeg::read(cover)?;
        let costs = juniward.compute(&jpeg, 0);

        let tmp_stego = tempfile::Builder::new().suffix(".jpg").tempfile()?;
        let stego_path = tmp_stego.path().to_path_buf();
        let adapter_opt: Option<Box<dyn ChannelAdapter>> = if args.no_adapter {
            None
        } else {
            Some(Box::new(TwitterProfile::default()))
        };
        if embed_capture_pre_stc(
            cover,
            &payload,
            &passphrase,
            &costs,
            &stego_path,
            adapter_opt.as_deref(),
        )
        .is_err()
        {
            continue;
        }

        let tmp_re = tempfile::Builder::new().suffix(".jpg").tempfile()?;
        let reenc_path = tmp_re.path().to_path_buf();
        recompress_twitter(&stego_path, &reenc_path, args.recompress_qf)?;

        let cov_jpeg = pjpeg::read(cover)?;
        let reenc_jpeg = pjpeg::read(&reenc_path)?;

        let cov_block = phash_block_of_jpeg(&cov_jpeg);
        let reenc_block = phash_block_of_jpeg(&reenc_jpeg);

        // Compute max and mean |drift|.
        let mut max_d = 0.0_f64;
        let mut sum_d = 0.0_f64;
        for i in 0..64 {
            let d = (cov_block[i] - reenc_block[i]).abs();
            if d > max_d {
                max_d = d;
            }
            sum_d += d;
        }
        let mean_d = sum_d / 64.0;

        if max_d > max_abs_drift {
            max_abs_drift = max_d;
        }
        drifts_sum += mean_d;
        drifts_n += 1;

        // Check stability at each step.
        let mut stability_bits = String::new();
        for (i, step) in steps.iter().enumerate() {
            if salt_stable(&cov_block, &reenc_block, *step) {
                stable_counts[i] += 1;
                stability_bits.push_str(&format!("step{}:Y ", *step as u32));
            } else {
                stability_bits.push_str(&format!("step{}:N ", *step as u32));
            }
        }
        println!(
            "{:<12} max_drift={:.2} mean_drift={:.2}  {}",
            image_key, max_d, mean_d, stability_bits
        );
    }

    let n = images.len();
    println!();
    println!("SUMMARY across {} covers (adapter={})", n, if args.no_adapter { "off" } else { "twitter" });
    println!("  max abs drift (over all covers/coeffs): {:.2}", max_abs_drift);
    println!("  mean abs drift (avg over covers): {:.2}", drifts_sum / drifts_n as f64);
    for (i, step) in steps.iter().enumerate() {
        println!(
            "  step={:>4}: stable {}/{} ({:.1}%)",
            *step as u32,
            stable_counts[i],
            n,
            stable_counts[i] as f64 / n as f64 * 100.0
        );
    }

    Ok(())
}
