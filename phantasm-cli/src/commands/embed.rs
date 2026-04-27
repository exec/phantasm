use anyhow::Result;
use std::path::{Path, PathBuf};

use phantasm_core::{
    ChannelAdapter, ChannelProfile, ContentAdaptiveOrchestrator, EmbedPlan, HashSensitivity,
    HashType, Orchestrator, StealthTier, TwitterProfile,
};
use phantasm_cost::{DistortionFunction, Juniward, Sidecar, Uniform};

use crate::commands::passphrase::PassphraseSource;
use crate::{
    ChannelAdapterChoice, ChannelChoice, CostFunctionChoice, HashGuardChoice, StealthChoice,
};

pub struct EmbedArgs<'a> {
    pub input: &'a Path,
    pub payload: &'a Option<PathBuf>,
    pub passphrase: PassphraseSource,
    pub output: &'a Path,
    pub channel: ChannelChoice,
    pub stealth: StealthChoice,
    pub cost_function: CostFunctionChoice,
    pub cost_sidecar: Option<&'a Path>,
    pub channel_adapter: ChannelAdapterChoice,
    pub hash_guard: HashGuardChoice,
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
        cost_sidecar,
        channel_adapter,
        hash_guard,
        layer,
    } = args;
    let has_payload = payload.is_some();
    let has_passphrase = !passphrase.is_empty();
    let has_layers = layer.as_ref().is_some_and(|l| !l.is_empty());

    if !has_layers && (!has_payload || !has_passphrase) {
        anyhow::bail!(
            "Either --payload and one of --passphrase/--passphrase-env/--passphrase-fd must be \
             provided, or --layer(s) must be specified"
        );
    }

    if has_payload && has_layers {
        anyhow::bail!("--payload is mutually exclusive with --layer");
    }

    if has_passphrase && has_layers {
        anyhow::bail!("--passphrase is mutually exclusive with --layer");
    }

    // Direct --passphrase leaks via `ps`; warn on stderr (ephemeral, does not
    // persist to log files — deliberately NOT via `log::warn!`, QWEN finding 1).
    if passphrase.direct.is_some() {
        eprintln!(
            "WARNING: passphrase on command line is insecure, use --passphrase-env or \
             --passphrase-fd in production"
        );
    }

    // PNG support was removed in v1 — phantasm v1 is JPEG-only.
    if input
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        anyhow::bail!(
            "PNG covers are not supported in phantasm v1 (JPEG only). \
             Convert your cover to JPEG and try again."
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
    let passphrase_owned = passphrase.resolve()?;
    let passphrase_str = passphrase_owned.as_str();
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

    if matches!(cost_function, CostFunctionChoice::FromSidecar) && cost_sidecar.is_none() {
        anyhow::bail!("--cost-function from-sidecar requires --cost-sidecar");
    }
    if !matches!(cost_function, CostFunctionChoice::FromSidecar) && cost_sidecar.is_some() {
        anyhow::bail!("--cost-sidecar only valid with --cost-function from-sidecar");
    }

    let _ = passphrase_str;

    let distortion: Box<dyn DistortionFunction> = match cost_function {
        CostFunctionChoice::Juniward => Box::new(Juniward),
        CostFunctionChoice::Uniform => Box::new(Uniform),
        CostFunctionChoice::FromSidecar => {
            let path = cost_sidecar.expect("validated above");
            Box::new(Sidecar::new(path.to_path_buf()))
        }
    };
    let mut orchestrator = ContentAdaptiveOrchestrator::new(distortion);

    // Hash guard must be applied BEFORE channel stabilization so the guarded
    // pHash/dHash matches the ORIGINAL cover — the user's stealth intent is
    // invisibility against a database keyed on the original image. (The
    // orchestrator enforces this ordering internally.)
    match hash_guard {
        HashGuardChoice::None => {}
        HashGuardChoice::Phash => {
            orchestrator = orchestrator.with_hash_guard(HashType::PHash);
        }
        HashGuardChoice::Dhash => {
            orchestrator = orchestrator.with_hash_guard(HashType::DHash);
        }
    }

    match channel_adapter {
        ChannelAdapterChoice::None => {}
        ChannelAdapterChoice::Twitter => {
            let adapter: Box<dyn ChannelAdapter> = Box::new(TwitterProfile::default());
            orchestrator = orchestrator.with_channel_adapter(adapter);
        }
    }

    let result = orchestrator.embed(input, &payload_bytes, passphrase_str, &plan, output)?;

    println!(
        "Embedded {} bytes into {} (cost_function={}, channel_adapter={}, hash_guard={})",
        result.bytes_embedded,
        output.display(),
        cost_function,
        channel_adapter,
        hash_guard,
    );
    println!("Capacity used: {:.1}%", result.capacity_used_ratio * 100.0);

    Ok(())
}
