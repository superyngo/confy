use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use crate::model::any_doc::detect_format;
use crate::model::document::DocFormat;

#[derive(Parser)]
#[command(name = "confy", about = "TUI editor for structured config files")]
struct Args {
    /// Path to the config file to edit
    file: PathBuf,
    /// Override format detection (toml, json, jsonc, yaml)
    #[arg(long)]
    format: Option<String>,
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    let fmt = match args.format.as_deref() {
        Some("toml") => DocFormat::Toml,
        Some("json") | Some("jsonc") => DocFormat::Json,
        Some("yaml") | Some("yml") => DocFormat::Yaml,
        Some(other) => anyhow::bail!("unknown format: {other}"),
        None => detect_format(&args.file).ok_or_else(|| {
            anyhow::anyhow!("unrecognized config format: {}", args.file.display())
        })?,
    };
    crate::tui::run(&args.file, fmt)
}

#[cfg(test)]
mod tests {
    use crate::model::any_doc::detect_format;
    use crate::model::document::DocFormat;

    #[test]
    fn detects_known_formats() {
        let p = |s: &str| detect_format(std::path::Path::new(s));
        assert_eq!(p("a.toml"), Some(DocFormat::Toml));
        assert_eq!(p("a.json"), Some(DocFormat::Json));
        assert_eq!(p("a.jsonc"), Some(DocFormat::Json));
        assert_eq!(p("a.yaml"), Some(DocFormat::Yaml));
        assert_eq!(p("a.yml"), Some(DocFormat::Yaml));
        assert_eq!(p("a.ini"), None);
    }
}
