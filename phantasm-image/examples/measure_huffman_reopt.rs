//! Measure the effect of Huffman re-optimization on stego file-size inflation.
//!
//! Strategy:
//!   1. For each sample, read coefficients.
//!   2. Flip ~2% of non-zero AC coefficients (a proxy for UERD-style embedding).
//!   3. Write with `optimize_coding = FALSE` → `stego_off.jpg`.
//!   4. Write with `optimize_coding = TRUE`  → `stego_on.jpg`.
//!   5. Report the mean (stego − cover) delta for both configurations.
//!
//! NOTE: mozjpeg's default profile (JCP_MAX_COMPRESSION) enables trellis
//! quantisation, which internally forces optimal entropy coding regardless
//! of the `optimize_coding` flag. Both configurations therefore produce
//! identical output sizes — Huffman tables are always rebuilt from the
//! post-embedding coefficient histogram. The `optimize_coding = TRUE` we
//! set in production is documentation of intent + a forward-safety net.
//!
//! Usage:
//!     cargo run -p phantasm-image --example measure_huffman_reopt -- \
//!         research-corpus/qf85/1024/0001.jpg ...

use phantasm_image::jpeg;
use std::path::Path;
use tempfile::NamedTempFile;

fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

fn flip_some(jc: &mut jpeg::JpegCoefficients, seed: u64, target_fraction: f64) -> usize {
    let mut state = seed;
    let mut flips = 0usize;
    for comp in jc.components.iter_mut() {
        for idx in 0..comp.coefficients.len() {
            if idx % 64 == 0 {
                continue;
            }
            let v = comp.coefficients[idx];
            if v == 0 {
                continue;
            }
            let r = (next(&mut state) as f64) / (u64::MAX as f64);
            if r < target_fraction {
                comp.coefficients[idx] = if v > 0 { v - 1 } else { v + 1 };
                flips += 1;
            }
        }
    }
    flips
}

struct Row {
    cover: u64,
    stego_off: u64,
    stego_on: u64,
    flips: usize,
}

fn sample(path: &Path) -> Result<Row, Box<dyn std::error::Error>> {
    let cover = std::fs::metadata(path)?.len();

    let mut jc_off = jpeg::read(path)?;
    let flips_off = flip_some(&mut jc_off, 0xdead_beef_cafe_babe, 0.02);
    let tmp_off = NamedTempFile::with_suffix(".jpg")?;
    jpeg::write_with_source_opts(&jc_off, path, tmp_off.path(), false)?;
    let stego_off = std::fs::metadata(tmp_off.path())?.len();

    let mut jc_on = jpeg::read(path)?;
    let flips_on = flip_some(&mut jc_on, 0xdead_beef_cafe_babe, 0.02);
    let tmp_on = NamedTempFile::with_suffix(".jpg")?;
    jpeg::write_with_source_opts(&jc_on, path, tmp_on.path(), true)?;
    let stego_on = std::fs::metadata(tmp_on.path())?.len();

    // Sanity: extraction round-trips cleanly on both.
    for (label, p, expected) in [
        ("off", tmp_off.path(), &jc_off),
        ("on", tmp_on.path(), &jc_on),
    ] {
        let readback = jpeg::read(p)?;
        for (ci, (m, r)) in expected
            .components
            .iter()
            .zip(readback.components.iter())
            .enumerate()
        {
            if m.coefficients != r.coefficients {
                return Err(format!("readback mismatch (reopt={label}) in component {ci}").into());
            }
        }
    }

    debug_assert_eq!(flips_off, flips_on);

    Ok(Row {
        cover,
        stego_off,
        stego_on,
        flips: flips_on,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: measure_huffman_reopt <cover1.jpg> [cover2.jpg ...]");
        std::process::exit(1);
    }

    println!(
        "{:<36} {:>10} {:>12} {:>12} {:>12} {:>12} {:>8}",
        "sample", "cover", "reopt-OFF", "Δ-OFF", "reopt-ON", "Δ-ON", "flips"
    );
    println!("{}", "-".repeat(108));

    let mut sum_cover = 0i64;
    let mut sum_off = 0i64;
    let mut sum_on = 0i64;
    let mut n = 0;

    for arg in &args {
        let p = Path::new(arg);
        match sample(p) {
            Ok(row) => {
                let d_off = row.stego_off as i64 - row.cover as i64;
                let d_on = row.stego_on as i64 - row.cover as i64;
                println!(
                    "{:<36} {:>10} {:>12} {:>+12} {:>12} {:>+12} {:>8}",
                    p.file_name().and_then(|s| s.to_str()).unwrap_or(arg),
                    row.cover,
                    row.stego_off,
                    d_off,
                    row.stego_on,
                    d_on,
                    row.flips
                );
                sum_cover += row.cover as i64;
                sum_off += row.stego_off as i64;
                sum_on += row.stego_on as i64;
                n += 1;
            }
            Err(e) => eprintln!("  {}: error {}", arg, e),
        }
    }

    if n > 0 {
        let mean_cover = sum_cover / n;
        let mean_off = sum_off / n;
        let mean_on = sum_on / n;
        let d_off = mean_off - mean_cover;
        let d_on = mean_on - mean_cover;
        println!("{}", "-".repeat(108));
        println!(
            "{:<36} {:>10} {:>12} {:>+12} {:>12} {:>+12}",
            "MEAN", mean_cover, mean_off, d_off, mean_on, d_on
        );
        println!();
        println!("mean delta, reopt OFF: {:+} B  (stego vs cover)", d_off);
        println!("mean delta, reopt ON : {:+} B  (stego vs cover)", d_on);
        println!("reduction from enabling reopt: {:+} B", d_on - d_off);
    }

    Ok(())
}
