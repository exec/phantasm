use anyhow::Result;
use std::path::Path;

use phantasm_core::{ChannelAdapter, ContentAdaptiveOrchestrator, Orchestrator, TwitterProfile};
use phantasm_cost::Uniform;

use crate::commands::passphrase::PassphraseSource;
use crate::{ChannelAdapterChoice, HashGuardChoice};

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

    if input
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        anyhow::bail!(
            "PNG covers are not supported in phantasm v1 (JPEG only). \
             Stegos produced by older PNG-mode versions of phantasm are not \
             extractable with this build."
        );
    }

    let passphrase_str = passphrase.resolve()?;

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
    let payload = orchestrator.extract(input, &passphrase_str)?;

    std::fs::write(output, &payload)?;

    println!(
        "Extracted {} bytes from {} → {}",
        payload.len(),
        input.display(),
        output.display()
    );

    Ok(())
}
