use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::model::any_doc::detect_format;
use crate::model::document::DocFormat;

#[derive(Parser)]
#[command(
    name = "confy",
    version,
    about = "TUI editor for structured config files"
)]
#[command(args_conflicts_with_subcommands = true)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the config file to edit (default action)
    file: Option<PathBuf>,
    /// Override format detection (toml, json, jsonc, yaml)
    #[arg(long)]
    format: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a config file to another format (writes a new file; the source is
    /// never modified). Formats default from the file extensions.
    Convert {
        /// Source file to read
        input: PathBuf,
        /// Destination file to write
        output: PathBuf,
        /// Override the source format (toml, json, jsonc, yaml)
        #[arg(long)]
        from: Option<String>,
        /// Override the target format (toml, json, jsonc, yaml)
        #[arg(long)]
        to: Option<String>,
        /// Proceed without the interactive confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

/// Resolve a `--format`/`--from`/`--to` override string, falling back to the
/// file extension.
fn resolve_format(override_str: Option<&str>, path: &Path) -> Result<DocFormat> {
    match override_str {
        Some("toml") => Ok(DocFormat::Toml),
        Some("json") | Some("jsonc") => Ok(DocFormat::Json),
        Some("yaml") | Some("yml") => Ok(DocFormat::Yaml),
        Some(other) => anyhow::bail!("unknown format: {other}"),
        None => detect_format(path)
            .ok_or_else(|| anyhow::anyhow!("unrecognized config format: {}", path.display())),
    }
}

/// A minimal valid empty document for `format` — the seed written when the user
/// asks to create a not-yet-existing file. TOML/YAML accept an empty document;
/// JSON needs an empty object.
fn seed_for(format: DocFormat) -> &'static str {
    match format {
        DocFormat::Toml => "",
        DocFormat::Json => "{}\n",
        DocFormat::Yaml => "",
    }
}

/// `confy <file>` where `<file>` doesn't exist yet: confirm on the terminal, then
/// create it with a minimal valid seed for the extension-derived format so the
/// normal load path can open it. Declining (or a non-interactive stdin) aborts
/// without touching the filesystem.
fn create_missing_file(file: &Path, fmt: DocFormat) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "{} does not exist (run in a terminal to create it, or create it first)",
            file.display()
        );
    }
    eprint!(
        "{} does not exist. Create it as {}? [y/N] ",
        file.display(),
        fmt.name()
    );
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        anyhow::bail!("cancelled (no file created)");
    }
    std::fs::write(file, seed_for(fmt))
        .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", file.display()))?;
    Ok(())
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Some(Command::Convert {
            input,
            output,
            from,
            to,
            yes,
        }) => run_convert(&input, &output, from.as_deref(), to.as_deref(), yes),
        None => {
            let file = args.file.ok_or_else(|| {
                anyhow::anyhow!("no file given (try `confy <file>` or `confy convert <in> <out>`)")
            })?;
            let fmt = resolve_format(args.format.as_deref(), &file)?;
            if !file.exists() {
                create_missing_file(&file, fmt)?;
            }
            crate::tui::run(&file, fmt)
        }
    }
}

fn run_convert(
    input: &Path,
    output: &Path,
    from: Option<&str>,
    to: Option<&str>,
    yes: bool,
) -> Result<()> {
    let from_fmt = resolve_format(from, input)?;
    let to_fmt = resolve_format(to, output)?;

    let doc = crate::load_document(input, from_fmt)
        .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", input.display()))?;

    let result = match crate::model::convert::convert(&doc, to_fmt) {
        Ok(r) => r,
        Err(abort) => {
            // Conversion aborted: nothing is written.
            anyhow::bail!("conversion aborted: {}", abort);
        }
    };

    if !result.warnings.is_empty() {
        eprintln!(
            "Converting {} → {} normalizes the following (lossy):",
            from_fmt.name(),
            to_fmt.name()
        );
        for w in &result.warnings {
            eprintln!("  • {w}");
        }
        if !yes {
            if std::io::stdin().is_terminal() {
                eprint!("Proceed and write {}? [y/N] ", output.display());
                std::io::stderr().flush().ok();
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !matches!(answer.trim(), "y" | "Y" | "yes") {
                    anyhow::bail!("cancelled (no file written)");
                }
            } else {
                anyhow::bail!("refusing to write a lossy conversion without --yes");
            }
        }
    }

    std::fs::write(output, &result.text)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", output.display()))?;
    eprintln!("wrote {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::model::any_doc::detect_format;
    use crate::model::document::DocFormat;

    #[test]
    fn seed_for_each_format_round_trips() {
        use crate::model::any_doc::AnyDocument;
        use crate::model::document::ConfigDocument;
        for fmt in [DocFormat::Toml, DocFormat::Json, DocFormat::Yaml] {
            let seed = super::seed_for(fmt);
            let doc = AnyDocument::from_str_as(seed, fmt)
                .unwrap_or_else(|e| panic!("{fmt:?} seed must parse: {e}"));
            // The seed is an empty document: it has no keyed children.
            assert!(
                doc.project()
                    .root
                    .children
                    .iter()
                    .all(|c| matches!(c.kind, crate::model::node::NodeKind::Comment(_))),
                "{fmt:?} seed should be empty"
            );
        }
    }

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
