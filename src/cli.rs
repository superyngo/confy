use anyhow::{bail, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "confy", about = "TUI editor for structured config files")]
struct Args {
    /// Path to the config file to edit
    file: PathBuf,
    /// Override format detection (only `toml` supported in MVP)
    #[arg(long)]
    format: Option<String>,
}

pub enum Format {
    Toml,
}

pub fn detect_format(path: &Path) -> Result<Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Ok(Format::Toml),
        other => bail!(
            "format not yet supported: {:?} (MVP supports .toml only)",
            other
        ),
    }
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    let fmt = match args.format.as_deref() {
        Some("toml") => Format::Toml,
        Some(other) => anyhow::bail!("format not yet supported: {other}"),
        None => detect_format(&args.file)?,
    };
    let Format::Toml = fmt;
    crate::tui::run(&args.file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_toml() {
        assert!(detect_format(std::path::Path::new("a.yaml")).is_err());
        assert!(detect_format(std::path::Path::new("a.toml")).is_ok());
    }
}
