use anyhow::Result;
use std::path::Path;

use phantasm_core::pipeline_spatial;
use phantasm_core::{ContentAdaptiveOrchestrator, Orchestrator};
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
    _channel_adapter: ChannelAdapterChoice,
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
        // Extraction reads the embedded payload directly from the stego JPEG —
        // STC decoding does not consult the cost function, so any distortion
        // is fine here. The --channel-adapter and --hash-guard flags are
        // accepted for forward-compatibility with a future envelope format
        // that auto-detects them, but v0.1 extract derives positions
        // geometrically from the stego JPEG and does not need the values at
        // runtime.
        let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
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
