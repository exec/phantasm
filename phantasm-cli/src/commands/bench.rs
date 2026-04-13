use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn run(cover_dir: &Path, stego_dir: &Path, output: &Option<PathBuf>) -> Result<()> {
    println!("[STUB] phantasm bench");
    println!("  cover_dir: {}", cover_dir.display());
    println!("  stego_dir: {}", stego_dir.display());
    if let Some(out) = output {
        println!("  output:    {}", out.display());
    }
    println!();
    println!("[STUB] Benchmarking requires the phantasm-bench crate (not yet implemented)");

    Ok(())
}
