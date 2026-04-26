//! Measure raw coefficient-parity-flip rate through recompression, per cover.
//! No STC, no embed — just read cover coefficients, recompress, compare LSBs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

use phantasm_image::jpeg as pjpeg;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long, default_value = "40")]
    limit: usize,
    #[arg(long, default_value = "85")]
    recompress_qf: u8,
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

    let mut per_image: Vec<(String, usize, usize, f64)> = Vec::new();

    for cover in &images {
        let image_key = cover
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();

        // Read cover coefficients.
        let jpeg = pjpeg::read(cover)?;
        let y = &jpeg.components[0];
        let bh = y.blocks_high;
        let bw = y.blocks_wide;

        // Recompress.
        let tmp = tempfile::Builder::new().suffix(".jpg").tempfile()?;
        let reenc = tmp.path().to_path_buf();
        recompress_twitter(cover, &reenc, args.recompress_qf)?;

        let jpeg2 = pjpeg::read(&reenc)?;
        let y2 = &jpeg2.components[0];
        if y2.blocks_high != bh || y2.blocks_wide != bw {
            eprintln!("{}: dim mismatch after reenc", image_key);
            continue;
        }

        // Count parity flips in AC coefficients.
        let total_positions = bh * bw * 63;
        let mut flips = 0usize;
        for br in 0..bh {
            for bc in 0..bw {
                for dp in 1..64 {
                    let a = y.get(br, bc, dp);
                    let b = y2.get(br, bc, dp);
                    if (a & 1) != (b & 1) {
                        flips += 1;
                    }
                }
            }
        }

        let rate = flips as f64 / total_positions as f64;
        per_image.push((image_key.clone(), total_positions, flips, rate));
        println!(
            "{:<12} total_pos={} flips={} rate={:.4}",
            image_key, total_positions, flips, rate
        );
    }

    if !per_image.is_empty() {
        let n = per_image.len();
        let mut rates: Vec<f64> = per_image.iter().map(|s| s.3).collect();
        rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean: f64 = rates.iter().sum::<f64>() / n as f64;
        let median = rates[n / 2];
        let p90 = rates[((n as f64 * 0.9) as usize).min(n - 1)];
        let max = *rates.last().unwrap();

        println!();
        println!("SUMMARY (raw cover→recompress parity flip, no embed)");
        println!("  N={}", n);
        println!(
            "  mean={:.4} median={:.4} p90={:.4} max={:.4}",
            mean, median, p90, max
        );
    }

    Ok(())
}
