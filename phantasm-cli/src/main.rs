use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

mod commands;
mod logger;

use commands::{analyze, bench, channels, embed, extract};

#[derive(Parser)]
#[command(name = "phantasm")]
#[command(version = "0.1.0")]
#[command(about = "Phantasm — compression-resilient image steganography")]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(short, long)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Hide a message or file in a cover image
    Embed {
        /// Path to cover image
        #[arg(short, long)]
        input: PathBuf,

        /// Path to plaintext payload file (mutually exclusive with --layer)
        #[arg(short, long)]
        payload: Option<PathBuf>,

        /// Passphrase for encryption (WARNING: insecure on command line)
        #[arg(long)]
        passphrase: Option<String>,

        /// Path to stego output
        #[arg(short, long)]
        output: PathBuf,

        /// Channel profile
        #[arg(long, default_value = "lossless")]
        channel: ChannelChoice,

        /// Stealth tier
        #[arg(long, default_value = "high")]
        stealth: StealthChoice,

        /// Content-adaptive distortion function used to compute per-coefficient
        /// embedding costs. Choices: `uniform`, `uerd`, `j-uniward`. `uerd`
        /// (default) is content-adaptive and substantially harder to detect
        /// than `uniform` on the Fridrich RS attack; `j-uniward` is the
        /// wavelet-domain distortion function from Holub & Fridrich (2014).
        #[arg(long, default_value = "uerd")]
        cost_function: CostFunctionChoice,

        /// Channel stabilization profile. `none` (default) preserves pre-v0.1.0-alpha
        /// behavior. `twitter` enables MINICER+ROAST stabilization at a ~10-20%
        /// capacity cost but produces stego that survives Twitter re-encoding.
        /// Extract must be invoked with the same `--channel` value.
        #[arg(long, default_value = "none")]
        channel_adapter: ChannelAdapterChoice,

        /// Perceptual-hash guard. `none` (default) preserves pre-v0.1.0-alpha
        /// behavior. `phash` or `dhash` constrain the STC encoder away from
        /// coefficients whose modification would flip the selected perceptual-hash
        /// bits, preserving the cover's hash. Extract must be invoked with the
        /// same `--hash-guard` value.
        #[arg(long, default_value = "none")]
        hash_guard: HashGuardChoice,

        /// Multi-layer payload (passphrase:path) — PLAN Phase 4, not yet
        /// implemented. Hidden from `--help` in v0.1.0; still parses for
        /// forward-compat with existing scripts.
        #[arg(long, hide = true)]
        layer: Option<Vec<String>>,
    },

    /// Recover a hidden payload from a stego image
    Extract {
        /// Path to stego image
        #[arg(short, long)]
        input: PathBuf,

        /// Passphrase for decryption (WARNING: insecure on command line)
        #[arg(long)]
        passphrase: String,

        /// Path to write recovered payload
        #[arg(short, long)]
        output: PathBuf,

        /// Channel stabilization profile used at embed time. Must match the
        /// `--channel` value passed to `phantasm embed`. Currently accepted for
        /// forward-compatibility; v0.1 extract derives positions geometrically
        /// and does not consult this flag.
        #[arg(long, default_value = "none")]
        channel_adapter: ChannelAdapterChoice,

        /// Perceptual-hash guard used at embed time. Must match the
        /// `--hash-guard` value passed to `phantasm embed`. Currently accepted
        /// for forward-compatibility; v0.1 extract derives positions
        /// geometrically and does not consult this flag.
        #[arg(long, default_value = "none")]
        hash_guard: HashGuardChoice,
    },

    /// Report image capacity and characteristics
    Analyze {
        /// Image path
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Output as JSON instead of table
        #[arg(long, hide = true)]
        json: bool,
    },

    /// List available channel profiles
    Channels {
        /// Output as JSON instead of table
        #[arg(long)]
        json: bool,
    },

    /// Run steganalysis self-test (requires phantasm-bench crate)
    Bench {
        /// Directory of cover images
        #[arg(long)]
        cover_dir: PathBuf,

        /// Directory for stego output
        #[arg(long)]
        stego_dir: PathBuf,

        /// Output results to file
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ChannelChoice {
    Lossless,
    Facebook,
    Twitter,
    Instagram,
    #[value(name = "whatsapp-photo")]
    WhatsAppPhoto,
    #[value(name = "whatsapp-doc")]
    WhatsAppDoc,
    Signal,
    #[value(name = "generic-75")]
    Generic75,
}

impl std::fmt::Display for ChannelChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lossless => write!(f, "lossless"),
            Self::Facebook => write!(f, "facebook"),
            Self::Twitter => write!(f, "twitter"),
            Self::Instagram => write!(f, "instagram"),
            Self::WhatsAppPhoto => write!(f, "whatsapp-photo"),
            Self::WhatsAppDoc => write!(f, "whatsapp-doc"),
            Self::Signal => write!(f, "signal"),
            Self::Generic75 => write!(f, "generic-75"),
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CostFunctionChoice {
    Uniform,
    Uerd,
    #[value(name = "j-uniward")]
    Juniward,
}

impl std::fmt::Display for CostFunctionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uniform => write!(f, "uniform"),
            Self::Uerd => write!(f, "uerd"),
            Self::Juniward => write!(f, "j-uniward"),
        }
    }
}

/// Channel stabilization choice. Separate from the legacy `--channel` flag
/// (which selects a [`ChannelProfile`] descriptor) because the two serve
/// different purposes and must not collide with the existing flag's default.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ChannelAdapterChoice {
    None,
    Twitter,
}

impl std::fmt::Display for ChannelAdapterChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Twitter => write!(f, "twitter"),
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum HashGuardChoice {
    None,
    Phash,
    Dhash,
}

impl std::fmt::Display for HashGuardChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Phash => write!(f, "phash"),
            Self::Dhash => write!(f, "dhash"),
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StealthChoice {
    Max,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for StealthChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Max => write!(f, "max"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logger
    logger::init(cli.verbose, cli.quiet)?;

    // Print banner unless quiet
    if !cli.quiet {
        eprintln!("phantasm 0.1.0 — not for production use");
    }

    // Dispatch to subcommand
    match &cli.command {
        Commands::Embed {
            input,
            payload,
            passphrase,
            output,
            channel,
            stealth,
            cost_function,
            channel_adapter,
            hash_guard,
            layer,
        } => embed::run(embed::EmbedArgs {
            input,
            payload,
            passphrase,
            output,
            channel: *channel,
            stealth: *stealth,
            cost_function: *cost_function,
            channel_adapter: *channel_adapter,
            hash_guard: *hash_guard,
            layer,
        })?,

        Commands::Extract {
            input,
            passphrase,
            output,
            channel_adapter,
            hash_guard,
        } => extract::run(input, passphrase, output, *channel_adapter, *hash_guard)?,

        Commands::Analyze { path, json } => analyze::run(path, *json)?,

        Commands::Channels { json } => channels::run(*json)?,

        Commands::Bench {
            cover_dir,
            stego_dir,
            output,
        } => bench::run(cover_dir, stego_dir, output)?,
    }

    Ok(())
}

pub fn print_progress_stub(label: &str, _steps: usize) {
    println!("[STUB] {}", label);
}
