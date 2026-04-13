use anyhow::Result;
use std::path::Path;

use phantasm_core::{CoverFormat, MinimalOrchestrator, Orchestrator};

pub fn run(path: &Path, json: bool) -> Result<()> {
    if json {
        println!("[STUB] JSON output not yet implemented");
        return Ok(());
    }

    let orchestrator = MinimalOrchestrator;
    let analysis = orchestrator.analyze(path)?;

    println!("phantasm analyze");
    println!("  path:            {}", path.display());
    println!();

    let fmt_str = match &analysis.format {
        CoverFormat::Jpeg { quality } => format!("JPEG (QF={})", quality),
        CoverFormat::Png => "PNG".to_string(),
        CoverFormat::Other(s) => s.clone(),
    };
    println!("Format:        {}", fmt_str);
    println!(
        "Dimensions:    {}x{}",
        analysis.dimensions.0, analysis.dimensions.1
    );
    println!("Capacity:");
    for (tier, bytes) in &analysis.tier_capacities {
        println!("  Stealth {:?}:   {} bytes", tier, bytes);
    }
    println!("Hash sensitivity:  {:?}", analysis.hash_sensitivity);
    println!("Channel robustness:");
    for compat in &analysis.channel_compatibility {
        let mark = if compat.compatible { "ok" } else { "x" };
        let note = compat.note.as_deref().unwrap_or("");
        if note.is_empty() {
            println!("  {}: {}", compat.channel, mark);
        } else {
            println!("  {}: {} ({})", compat.channel, mark, note);
        }
    }

    Ok(())
}
