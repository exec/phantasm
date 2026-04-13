use anyhow::Result;
use std::path::Path;

use phantasm_core::hash_guard::{apply_hash_guard, classify_sensitivity};
use phantasm_core::{ContentAdaptiveOrchestrator, CoverFormat, HashType, Orchestrator};
use phantasm_cost::{DistortionFunction, Uniform};
use phantasm_image::jpeg;

pub fn run(path: &Path, json: bool) -> Result<()> {
    if json {
        println!("[STUB] JSON output not yet implemented");
        return Ok(());
    }

    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
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

    // Sensitivity tier + hash-guard capacity-reduction estimate. Loads the
    // raw JPEG a second time (cheap) so we can feed it to hash_guard without
    // threading a JpegCoefficients handle through CoverAnalysis.
    if matches!(analysis.format, CoverFormat::Jpeg { .. }) {
        let jpeg = jpeg::read(path)?;
        let tier = classify_sensitivity(&jpeg);
        println!("Sensitivity tier: {:?}", tier);

        let mut probe_costs = Uniform.compute(&jpeg, 0);
        let total = probe_costs.positions.len();
        let report = apply_hash_guard(&mut probe_costs, &jpeg, HashType::PHash);
        let pct = if total == 0 {
            0.0
        } else {
            100.0 * report.wet_positions_added as f64 / total as f64
        };
        println!(
            "Hash-guard (pHash) wet positions: {} / {} ({:.2}% of capacity)",
            report.wet_positions_added, total, pct
        );
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
