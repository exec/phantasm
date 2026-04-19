use anyhow::Result;
use std::path::Path;

use phantasm_core::pipeline_spatial;
use phantasm_core::{ChannelAdapter, ContentAdaptiveOrchestrator, Orchestrator, TwitterProfile};
use phantasm_cost::Uniform;

use crate::commands::passphrase::PassphraseSource;
use crate::{ChannelAdapterChoice, HashGuardChoice};

fn is_png_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|e| e.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

pub fn run(
    input: &Path,
    passphrase: PassphraseSource,
    output: &Path,
    channel_adapter: ChannelAdapterChoice,
    _hash_guard: HashGuardChoice,
) -> Result<()> {
    if passphrase.is_empty() {
        anyhow::bail!("one of --passphrase, --passphrase-env, or --passphrase-fd must be provided");
    }

    // Direct --passphrase leaks via `ps`; warn on stderr only (ephemeral, does
    // not persist to log files — deliberately NOT via `log::warn!`, QWEN
    // finding 1).
    if passphrase.direct.is_some() {
        eprintln!(
            "WARNING: passphrase on command line is insecure, use --passphrase-env or \
             --passphrase-fd in production"
        );
    }

    let passphrase_str = passphrase.resolve()?;

    let payload = if is_png_path(input) {
        pipeline_spatial::extract_png(input, &passphrase_str)?
    } else {
        // STC decoding is passphrase-keyed and does not consult the cost
        // function, so any distortion works for the extract side. The
        // `--channel-adapter` flag selects the ECC framing route (lossy-path
        // stegos wrap the envelope in Reed-Solomon before STC) and must match
        // the embed side; `--hash-guard` is a pure embed-time switch that
        // leaves no trace in the stego.
        let mut orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
        match channel_adapter {
            ChannelAdapterChoice::None => {}
            ChannelAdapterChoice::Twitter => {
                let adapter: Box<dyn ChannelAdapter> = Box::new(TwitterProfile::default());
                orchestrator = orchestrator.with_channel_adapter(adapter);
            }
        }
        orchestrator.extract(input, &passphrase_str)?
    };

    std::fs::write(output, &payload)?;

    println!(
        "Extracted {} bytes from {} → {}",
        payload.len(),
        input.display(),
        output.display()
    );

    Ok(())
}
