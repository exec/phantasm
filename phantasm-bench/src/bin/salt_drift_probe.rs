//! Diagnostic: measure salt drift through the Twitter-surrogate recompression.
//!
//! For each cover: embed adapter-on, write stego, recompress at QF=85,
//! compare image_salt(cover) to image_salt(recompressed stego).
//! Reports per-cover whether the salt is stable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use phantasm_core::pipeline::diagnostics::embed_capture_pre_stc;
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

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut images = walk_jpegs(&args.corpus)?;
    images.truncate(args.limit);

    let mut rng = StdRng::seed_from_u64(0xB3E_2024);
    let mut payload = vec![0u8; args.payload_size];
    rng.fill_bytes(&mut payload);

    let juniward = Juniward;

    let mut stable = 0usize;
    let mut drifted = 0usize;
    let mut drifted_images: Vec<String> = Vec::new();

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

        // Use internal helper via reading both JPEGs and calling image_salt
        // indirectly by comparing hash_guard pHash output.
        // image_salt is pub(crate) — re-derive by duplicating the key step.
        // Simpler: compare STC-decode locations. The salt IS the locations
        // key, and permute_positions uses that. Easiest: compare the first
        // 32 bytes of derive_locations_key output across the two JPEGs.
        use phantasm_core::pipeline::diagnostics::salt_of_jpeg;
        let jpeg_cover = pjpeg::read(cover)?;
        let jpeg_stego = pjpeg::read(&stego_path)?;
        let jpeg_reenc = pjpeg::read(&reenc_path)?;
        let salt_cover = salt_of_jpeg(&jpeg_cover);
        let salt_stego = salt_of_jpeg(&jpeg_stego);
        let salt_reenc = salt_of_jpeg(&jpeg_reenc);

        let cover_eq_stego = salt_cover == salt_stego;
        let cover_eq_reenc = salt_cover == salt_reenc;
        let stego_eq_reenc = salt_stego == salt_reenc;

        let stable_end_to_end = salt_cover == salt_reenc;
        if stable_end_to_end {
            stable += 1;
        } else {
            drifted += 1;
            drifted_images.push(image_key.clone());
        }

        println!(
            "{:<12} cover==stego:{} cover==reenc:{} stego==reenc:{} stable:{}",
            image_key, cover_eq_stego, cover_eq_reenc, stego_eq_reenc, stable_end_to_end
        );
    }

    println!();
    println!(
        "SUMMARY stable={}/{} ({:.1}%) drifted={}",
        stable,
        images.len(),
        (stable as f64 / images.len() as f64) * 100.0,
        drifted
    );
    println!("drifted images: {:?}", drifted_images);

    Ok(())
}
