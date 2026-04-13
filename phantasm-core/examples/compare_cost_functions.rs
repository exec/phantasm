//! Throwaway research harness: embed the same payload with Uniform and UERD
//! cost functions, report embed success and output paths so we can run
//! phantasm-bench analyze-stealth against both.

use phantasm_core::{
    channel::ChannelProfile,
    plan::{EmbedPlan, HashSensitivity},
    stealth::StealthTier,
    ContentAdaptiveOrchestrator, MinimalOrchestrator, Orchestrator,
};
use phantasm_cost::{Uerd, Uniform};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: compare_cost_functions <cover.jpg> <payload> <passphrase>");
        std::process::exit(1);
    }
    let cover = PathBuf::from(&args[1]);
    let payload_path = PathBuf::from(&args[2]);
    let passphrase = &args[3];

    let payload = std::fs::read(&payload_path)?;
    println!("cover:      {}", cover.display());
    println!(
        "payload:    {} ({} bytes)",
        payload_path.display(),
        payload.len()
    );
    println!();

    let plan = EmbedPlan {
        channel: ChannelProfile::builtin("lossless").unwrap(),
        stealth_tier: StealthTier::High,
        capacity_bits: 0,
        payload_bits: 0,
        ecc_bits: 0,
        estimated_detection_error: 0.5,
        hash_constrained_positions: 0,
        hash_sensitivity: HashSensitivity::Robust,
    };

    let uniform_out = PathBuf::from("stego_uniform.jpg");
    let uerd_out = PathBuf::from("stego_uerd.jpg");

    // MinimalOrchestrator ≡ ContentAdaptiveOrchestrator(Uniform) at the pipeline level,
    // but we exercise both explicitly so the research output is unambiguous.
    let minimal = MinimalOrchestrator;
    let uniform_explicit = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
    let content_adaptive_uerd = ContentAdaptiveOrchestrator::new(Box::new(Uerd));

    println!("=== MinimalOrchestrator (uniform costs, original code path) ===");
    let r = minimal.embed(&cover, &payload, passphrase, &plan, &uniform_out)?;
    println!(
        "  bytes_embedded = {}, capacity_used = {:.2}%",
        r.bytes_embedded,
        r.capacity_used_ratio * 100.0
    );
    println!("  output: {}", uniform_out.display());
    println!();

    println!("=== ContentAdaptiveOrchestrator<Uniform> (sanity check) ===");
    let sanity_out = PathBuf::from("stego_uniform_ca.jpg");
    let r = uniform_explicit.embed(&cover, &payload, passphrase, &plan, &sanity_out)?;
    println!(
        "  bytes_embedded = {}, capacity_used = {:.2}%",
        r.bytes_embedded,
        r.capacity_used_ratio * 100.0
    );
    println!("  output: {}", sanity_out.display());
    println!();

    println!("=== ContentAdaptiveOrchestrator<UERD> ===");
    let r = content_adaptive_uerd.embed(&cover, &payload, passphrase, &plan, &uerd_out)?;
    println!(
        "  bytes_embedded = {}, capacity_used = {:.2}%",
        r.bytes_embedded,
        r.capacity_used_ratio * 100.0
    );
    println!("  output: {}", uerd_out.display());
    println!();

    // Verify roundtrip on all three
    println!("=== Roundtrip verification ===");
    let cases: [(&str, &PathBuf, &dyn Orchestrator); 3] = [
        ("uniform (MinimalOrchestrator)", &uniform_out, &minimal),
        ("uniform (ContentAdaptive)", &sanity_out, &uniform_explicit),
        ("uerd", &uerd_out, &content_adaptive_uerd),
    ];
    for (name, path, orch) in cases {
        let recovered = orch.extract(path, passphrase)?;
        let ok = recovered == payload;
        println!(
            "  {}: {} ({} bytes)",
            name,
            if ok { "identical" } else { "MISMATCH" },
            recovered.len()
        );
    }

    Ok(())
}
