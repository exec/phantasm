use anyhow::Result;
use log::warn;
use std::path::Path;

use phantasm_core::{ContentAdaptiveOrchestrator, Orchestrator};
use phantasm_cost::Uniform;

pub fn run(input: &Path, passphrase: &str, output: &Path) -> Result<()> {
    eprintln!("WARNING: passphrase on command line is insecure, use stdin in production");
    warn!("Passphrase provided on command line — insecure. Use stdin or env var in production.");

    // Extraction reads the embedded payload directly from the stego JPEG — STC
    // decoding does not consult the cost function, so any distortion is fine here.
    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
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
