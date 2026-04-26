//! Diagnostic: measure the post-STC envelope-byte-error rate on a single cover.
//!
//! Embeds a fixed-size random payload, writes the stego JPEG, recompresses
//! through the Twitter surrogate (image crate QF=85), then STC-decodes on the
//! recompressed stego and compares the post-STC byte stream to the pre-STC
//! byte stream captured at embed time. Reports the byte-error rate that the
//! RS layer has to correct.
//!
//! Usage:
//!   cargo run --release --bin post-stc-probe -- \
//!       --corpus research-corpus-500/qf85/720 \
//!       --limit 20 --payload-size 1000

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use phantasm_core::pipeline::diagnostics::{embed_capture_pre_stc, extract_raw_stc};
use phantasm_core::{ChannelAdapter, TwitterProfile};
use phantasm_cost::{DistortionFunction, Juniward};
use phantasm_image::jpeg as pjpeg;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long, default_value = "20")]
    limit: usize,
    #[arg(long, default_value = "1000")]
    payload_size: usize,
    #[arg(long, default_value = "85")]
    recompress_qf: u8,
    /// If set, adapter=none (no Twitter stabilization, no RS).
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

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut images = walk_jpegs(&args.corpus)?;
    images.truncate(args.limit);

    // Random payload seeded for reproducibility.
    let mut rng = StdRng::seed_from_u64(0xB3E_2024);
    let mut payload = vec![0u8; args.payload_size];
    rng.fill_bytes(&mut payload);

    let juniward = Juniward;
    let mut per_image_stats: Vec<(String, usize, usize, usize, f64, usize, usize)> = Vec::new();
    // cols: name, pre_stc_len, post_stc_len, byte_diff, byte_err_rate, first_bad_idx, raw_stc_bit_diffs

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

        let diag = embed_capture_pre_stc(
            cover,
            &payload,
            &passphrase,
            &costs,
            &stego_path,
            adapter_opt.as_deref(),
        );
        let diag = match diag {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[{}] embed fail: {}", image_key, e);
                continue;
            }
        };

        let tmp_re = tempfile::Builder::new().suffix(".jpg").tempfile()?;
        let reenc_path = tmp_re.path().to_path_buf();
        if let Err(e) = recompress_twitter(&stego_path, &reenc_path, args.recompress_qf) {
            eprintln!("[{}] recompress fail: {}", image_key, e);
            continue;
        }

        let raw = match extract_raw_stc(&reenc_path, &passphrase) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{}] raw stc extract fail: {}", image_key, e);
                continue;
            }
        };

        // Compute byte-error rate: post-STC bytes vs pre-STC bytes (both
        // "framed" form since raw_stc_bytes contains the length prefix too).
        // pre is `diag.framed_pre_stc`.
        let pre = &diag.framed_pre_stc;
        let post = &raw.raw_stc_bytes;

        let compare_len = pre.len().min(post.len());
        let mut byte_diff = 0usize;
        let mut first_bad_idx = usize::MAX;
        for i in 0..compare_len {
            if pre[i] != post[i] {
                byte_diff += 1;
                if first_bad_idx == usize::MAX {
                    first_bad_idx = i;
                }
            }
        }

        // Count bit diffs too.
        let mut bit_diff = 0usize;
        for i in 0..compare_len {
            bit_diff += (pre[i] ^ post[i]).count_ones() as usize;
        }

        let byte_err_rate = if compare_len > 0 {
            byte_diff as f64 / compare_len as f64
        } else {
            0.0
        };

        per_image_stats.push((
            image_key.clone(),
            pre.len(),
            post.len(),
            byte_diff,
            byte_err_rate,
            if first_bad_idx == usize::MAX {
                pre.len()
            } else {
                first_bad_idx
            },
            bit_diff,
        ));

        let prefix_ok = pre.len() >= 4
            && post.len() >= 4
            && pre[0] == post[0]
            && pre[1] == post[1]
            && pre[2] == post[2]
            && pre[3] == post[3];

        println!(
            "{:<12} pre={:5}B post={:5}B byte_diff={:4} ({:.4}) first_bad={:5} bits_diff={} prefix_ok={} stc_framed_len={}",
            image_key,
            pre.len(),
            post.len(),
            byte_diff,
            byte_err_rate,
            if first_bad_idx == usize::MAX {
                pre.len()
            } else {
                first_bad_idx
            },
            bit_diff,
            prefix_ok,
            raw.framed_len,
        );
    }

    // Summary.
    if !per_image_stats.is_empty() {
        let n = per_image_stats.len();
        let mean_rate: f64 = per_image_stats.iter().map(|s| s.4).sum::<f64>() / n as f64;
        let mut rates: Vec<f64> = per_image_stats.iter().map(|s| s.4).collect();
        rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = rates[n / 2];
        let p90 = rates[((n as f64 * 0.9) as usize).min(n - 1)];
        let max = *rates.last().unwrap();
        let max_byte_diffs = per_image_stats.iter().map(|s| s.3).max().unwrap();

        // Distribution of byte-errors per block (where block = data_shards × shard_size).
        let pre_stc_len = per_image_stats[0].1;
        println!();
        println!("SUMMARY");
        println!("  images: {}", n);
        println!("  pre-stc bytes (typical): {}", pre_stc_len);
        println!(
            "  byte-error rate: mean={:.4} median={:.4} p90={:.4} max={:.4}",
            mean_rate, median, p90, max
        );
        println!("  max byte-diff: {}", max_byte_diffs);
    }

    Ok(())
}
