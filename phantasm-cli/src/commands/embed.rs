use anyhow::Result;
use log::warn;
use std::path::{Path, PathBuf};

use phantasm_core::pipeline_spatial;
use phantasm_core::{
    ChannelAdapter, ChannelProfile, ContentAdaptiveOrchestrator, EmbedPlan, HashSensitivity,
    HashType, Orchestrator, SpatialCost, StealthTier, TwitterProfile,
};
use phantasm_cost::{
    DistortionFunction, Juniward, Noisy, PassphraseSubset, Sidecar, Uerd, Uniform,
    MAX_NOISE_AMPLITUDE, MIN_KEEP_FRACTION,
};

/// Detect PNG input by file extension. MVP — doesn't inspect magic bytes.
/// A mis-extensioned cover (`.jpg` containing PNG data) will route through
/// the JPEG path and fail at libjpeg; a mis-extensioned PNG (`.png`
/// containing JPEG data) will route through the spatial path and fail at
/// `image::open`. Both fail modes are clear, if not friendly.
fn is_png_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|e| e.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

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
    pub cost_noise: f64,
    pub cost_subset: f64,
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
        cost_noise,
        cost_subset,
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

    // PNG auto-dispatch: route to the spatial pipeline. This is a structural
    // branch — PNG covers don't have DCT coefficients, so the JPEG-specific
    // flags (channel adapter, hash guard, cost-noise, cost-subset) don't
    // apply. For the MVP we support only `--cost-function uniform` and a new
    // `s-uniward` selection path (chosen by default if a PNG is passed with
    // the JPEG-default `uerd`). Any other selection errors out cleanly.
    if !has_layers && is_png_path(input) {
        let payload_path = payload.as_ref().unwrap();
        let passphrase_owned = passphrase.resolve()?;
        let passphrase_str = passphrase_owned.as_str();
        let payload_bytes = std::fs::read(payload_path)?;

        if !matches!(channel_adapter, ChannelAdapterChoice::None) {
            anyhow::bail!("--channel-adapter is not supported for PNG covers in v0.2 (JPEG only)");
        }
        if !matches!(hash_guard, HashGuardChoice::None) {
            anyhow::bail!("--hash-guard is not supported for PNG covers in v0.2 (JPEG only)");
        }

        let spatial_cost = match cost_function {
            // The JPEG default `uerd` is meaningless on pixels; fall back to
            // S-UNIWARD (the spatial-domain academic baseline) silently.
            CostFunctionChoice::Uerd | CostFunctionChoice::Juniward => SpatialCost::SUniward,
            CostFunctionChoice::Uniform => SpatialCost::Uniform,
            CostFunctionChoice::FromSidecar => {
                anyhow::bail!("--cost-function from-sidecar is not supported for PNG covers")
            }
        };

        let result = pipeline_spatial::embed_png(
            input,
            &payload_bytes,
            passphrase_str,
            spatial_cost,
            output,
        )?;

        println!(
            "Embedded {} bytes into {} (cost_function={}, cover=png)",
            result.bytes_embedded,
            output.display(),
            match spatial_cost {
                SpatialCost::Uniform => "uniform",
                SpatialCost::SUniward => "s-uniward",
            },
        );
        println!("Capacity used: {:.1}%", result.capacity_used_ratio * 100.0);
        return Ok(());
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

    // --cost-noise validation and clamping
    if cost_noise < 0.0 {
        anyhow::bail!("--cost-noise must be >= 0.0 (got {})", cost_noise);
    }
    if !cost_noise.is_finite() {
        anyhow::bail!("--cost-noise must be finite (got {})", cost_noise);
    }
    let cost_noise_clamped = cost_noise.min(MAX_NOISE_AMPLITUDE);
    if cost_noise > MAX_NOISE_AMPLITUDE {
        eprintln!(
            "WARNING: --cost-noise {} exceeds recommended max {}; clamping. \
             High noise can break the underlying cost function's natural \
             distribution and degrade per-stego stealth.",
            cost_noise, MAX_NOISE_AMPLITUDE
        );
        warn!(
            "cost_noise {} clamped to {}",
            cost_noise, MAX_NOISE_AMPLITUDE
        );
    }
    if cost_noise_clamped > 1.0 {
        eprintln!(
            "NOTE: --cost-noise {:.2} is on the high side. The recommended \
             sweet-spot range is 0.25-1.0 — values above 1.0 may start to \
             trade per-stego stealth for distribution fragmentation.",
            cost_noise_clamped
        );
    }
    if matches!(cost_function, CostFunctionChoice::FromSidecar) && cost_noise_clamped > 0.0 {
        anyhow::bail!(
            "--cost-noise is incompatible with --cost-function from-sidecar (the sidecar \
             already contains the final cost map; bake the noise into the sidecar instead)"
        );
    }
    if matches!(cost_function, CostFunctionChoice::Uniform) && cost_noise_clamped > 0.0 {
        eprintln!(
            "NOTE: --cost-noise on top of --cost-function uniform produces effectively \
             random costs. This is unlikely to defend against a CNN attacker (it just \
             routes modifications uniformly with noise). Use uerd or j-uniward as the \
             base cost function for the intended fragmentation effect."
        );
    }

    // --cost-subset validation
    if !cost_subset.is_finite() || !(0.0..=1.0).contains(&cost_subset) {
        anyhow::bail!("--cost-subset must be in [0.0, 1.0] (got {})", cost_subset);
    }
    if cost_subset > 0.0 && cost_subset < MIN_KEEP_FRACTION {
        eprintln!(
            "WARNING: --cost-subset {:.3} is below the recommended minimum {:.2}. \
             STC will likely run out of usable capacity for typical payloads.",
            cost_subset, MIN_KEEP_FRACTION
        );
    }
    if matches!(cost_function, CostFunctionChoice::FromSidecar) && cost_subset < 1.0 {
        anyhow::bail!("--cost-subset is incompatible with --cost-function from-sidecar");
    }

    let pp_for_wrappers = passphrase_str.to_string();
    // Compose: subset wrapper outermost (marks wet positions), noise wrapper
    // inside (perturbs the surviving costs). Order matters slightly because
    // noise applies to ALL positions in the inner cost map; with subset on top,
    // the wet positions stay wet (∞) regardless of noise.
    fn build_distortion<D: DistortionFunction + 'static>(
        base: D,
        noise_amp: f64,
        keep_frac: f64,
        passphrase: &str,
    ) -> Box<dyn DistortionFunction> {
        if noise_amp > 0.0 && keep_frac < 1.0 {
            let noisy = Noisy::from_passphrase(base, noise_amp, passphrase);
            Box::new(PassphraseSubset::from_passphrase(
                noisy, keep_frac, passphrase,
            ))
        } else if noise_amp > 0.0 {
            Box::new(Noisy::from_passphrase(base, noise_amp, passphrase))
        } else if keep_frac < 1.0 {
            Box::new(PassphraseSubset::from_passphrase(
                base, keep_frac, passphrase,
            ))
        } else {
            Box::new(base)
        }
    }

    let distortion: Box<dyn DistortionFunction> = match cost_function {
        CostFunctionChoice::Uniform => {
            build_distortion(Uniform, cost_noise_clamped, cost_subset, &pp_for_wrappers)
        }
        CostFunctionChoice::Uerd => {
            build_distortion(Uerd, cost_noise_clamped, cost_subset, &pp_for_wrappers)
        }
        CostFunctionChoice::Juniward => {
            build_distortion(Juniward, cost_noise_clamped, cost_subset, &pp_for_wrappers)
        }
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
