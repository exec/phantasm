use anyhow::Result;
use log::warn;
use std::path::{Path, PathBuf};

use phantasm_core::{
    ChannelProfile, ContentAdaptiveOrchestrator, EmbedPlan, HashSensitivity, Orchestrator,
    StealthTier,
};
use phantasm_cost::{DistortionFunction, Uerd, Uniform};

use crate::{ChannelChoice, CostFunctionChoice, StealthChoice};

pub struct EmbedArgs<'a> {
    pub input: &'a Path,
    pub payload: &'a Option<PathBuf>,
    pub passphrase: &'a Option<String>,
    pub output: &'a Path,
    pub channel: ChannelChoice,
    pub stealth: StealthChoice,
    pub cost_function: CostFunctionChoice,
    pub layer: &'a Option<Vec<String>>,
}

pub fn run(args: EmbedArgs<'_>) -> Result<()> {
    let EmbedArgs {
        input,
        payload,
        passphrase,
        output,
        channel,
        stealth,
        cost_function,
        layer,
    } = args;
    let has_payload = payload.is_some();
    let has_passphrase = passphrase.is_some();
    let has_layers = layer.as_ref().is_some_and(|l| !l.is_empty());

    if !has_layers && (!has_payload || !has_passphrase) {
        anyhow::bail!(
            "Either --payload and --passphrase must be provided, or --layer(s) must be specified"
        );
    }

    if has_payload && has_layers {
        anyhow::bail!("--payload is mutually exclusive with --layer");
    }

    if has_passphrase && has_layers {
        anyhow::bail!("--passphrase is mutually exclusive with --layer");
    }

    if has_passphrase {
        eprintln!("WARNING: passphrase on command line is insecure, use stdin in production");
        warn!(
            "Passphrase provided on command line — insecure. Use stdin or env var in production."
        );
    }

    // Multi-layer: not yet implemented — stub output for backwards compatibility
    if has_layers {
        let layers = layer.as_ref().unwrap();
        let mut parsed = Vec::new();
        for layer_spec in layers {
            let parts: Vec<&str> = layer_spec.splitn(2, ':').collect();
            if parts.len() != 2 {
                anyhow::bail!(
                    "Layer format must be 'passphrase:path', got '{}'",
                    layer_spec
                );
            }
            parsed.push((parts[0].to_string(), parts[1].to_string()));
        }
        println!("[STUB] phantasm embed (multi-layer — not yet implemented)");
        for (pass, path) in &parsed {
            println!("  layer: {}:{}", pass, path);
        }
        return Ok(());
    }

    let payload_path = payload.as_ref().unwrap();
    let passphrase_str = passphrase.as_ref().unwrap();
    let payload_bytes = std::fs::read(payload_path)?;

    let channel_name = channel.to_string();
    let channel_profile = ChannelProfile::builtin(&channel_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown channel: {}", channel_name))?;

    let stealth_tier = match stealth {
        StealthChoice::Max => StealthTier::Max,
        StealthChoice::High => StealthTier::High,
        StealthChoice::Medium => StealthTier::Medium,
        StealthChoice::Low => StealthTier::Low,
    };

    let plan = EmbedPlan {
        channel: channel_profile,
        stealth_tier,
        capacity_bits: 0,
        payload_bits: payload_bytes.len() * 8,
        ecc_bits: 0,
        estimated_detection_error: 0.5,
        hash_constrained_positions: 0,
        hash_sensitivity: HashSensitivity::Robust,
    };

    let distortion: Box<dyn DistortionFunction> = match cost_function {
        CostFunctionChoice::Uniform => Box::new(Uniform),
        CostFunctionChoice::Uerd => Box::new(Uerd),
    };
    let orchestrator = ContentAdaptiveOrchestrator::new(distortion);
    let result = orchestrator.embed(input, &payload_bytes, passphrase_str, &plan, output)?;

    println!(
        "Embedded {} bytes into {} (cost_function={})",
        result.bytes_embedded,
        output.display(),
        cost_function
    );
    println!("Capacity used: {:.1}%", result.capacity_used_ratio * 100.0);

    Ok(())
}
