use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn run(cover_dir: &Path, stego_dir: &Path, output: &Option<PathBuf>) -> Result<()> {
    println!("Phantasm benchmarking lives in the separate phantasm-bench binary.");
    println!();
    println!("Run one of:");
    println!("  cargo run --release -p phantasm-bench -- eval-corpus --help");
    println!("  cargo run --release -p phantasm-bench -- analyze-stealth --help");
    println!("  cargo run --release -p phantasm-bench -- research-curve --help");
    println!("  cargo run --release -p phantasm-bench -- compare --help");
    println!();
    println!("The --cover-dir, --stego-dir, and --output flags you supplied (if any)");
    println!("are not forwarded automatically — pass them to the subcommand above.");

    println!();
    println!("You supplied:");
    println!("  --cover-dir {}", cover_dir.display());
    println!("  --stego-dir {}", stego_dir.display());
    if let Some(out) = output {
        println!("  --output    {}", out.display());
    }

    Ok(())
}
