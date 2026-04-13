use anyhow::Result;
use log::warn;
use std::path::Path;

use phantasm_core::{MinimalOrchestrator, Orchestrator};

pub fn run(input: &Path, passphrase: &str, output: &Path) -> Result<()> {
    eprintln!("WARNING: passphrase on command line is insecure, use stdin in production");
    warn!("Passphrase provided on command line — insecure. Use stdin or env var in production.");

    let orchestrator = MinimalOrchestrator;
    let payload = orchestrator.extract(input, passphrase)?;

    std::fs::write(output, &payload)?;

    println!(
        "Extracted {} bytes from {} → {}",
        payload.len(),
        input.display(),
        output.display()
    );

    Ok(())
}
