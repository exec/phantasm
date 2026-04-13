use anyhow::Result;
use log::LevelFilter;

pub fn init(verbose: u8, quiet: bool) -> Result<()> {
    let level = if quiet {
        LevelFilter::Off
    } else {
        match verbose {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            _ => LevelFilter::Trace,
        }
    };

    env_logger::Builder::from_default_env()
        .filter_level(level)
        .format_timestamp(None)
        .try_init()?;

    Ok(())
}
