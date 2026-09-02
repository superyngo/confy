use crate::model::any_doc::detect_format;
use crate::model::document::DocFormat;
use anyhow::Result;
use clap::{Parser, Subcommand};
use confy_core::session::{tr, tr_args, Lang};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "confy",
    version,
    about = "TUI editor for structured config files"
)]
#[command(
    args_conflicts_with_subcommands = true,
    subcommand_precedence_over_arg = true
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the config file to edit (default action)
    file: Option<PathBuf>,
    /// Override format detection (toml, json, jsonc, yaml)
    #[arg(long)]
    format: Option<String>,
    /// UI language for this session (en, zh-TW). Overrides the saved config
    /// file but does not write it. Falls back to the config file, then `en`.
    #[arg(long, global = true)]
    lang: Option<String>,
}

/// Resolve the active UI language: `--lang` > config file `lang` > default.
/// An unrecognized `--lang` value warns and falls through to the config file
/// (never panics).
fn resolve_lang(cli_lang: Option<&str>) -> confy_core::session::Lang {
    resolve_lang_with(cli_lang, &crate::config::load_config())
}

/// Pure precedence logic, split out from `resolve_lang` so tests can supply a
/// `Config` directly instead of touching the real `~/.config` file.
fn resolve_lang_with(
    cli_lang: Option<&str>,
    cfg: &crate::config::Config,
) -> confy_core::session::Lang {
    use confy_core::session::Lang;
    use std::str::FromStr;
    if let Some(s) = cli_lang {
        match Lang::from_str(s) {
            Ok(l) => return l,
            Err(()) => eprintln!("warning: unknown --lang '{s}', ignoring"),
        }
    }
    if let Some(s) = cfg.lang.as_deref() {
        if let Ok(l) = Lang::from_str(s) {
            return l;
        }
    }
    Lang::default()
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
fn resolve_format(override_str: Option<&str>, path: &Path, lang: Lang) -> Result<DocFormat> {
    match override_str {
        Some("toml") => Ok(DocFormat::Toml),
        Some("json") | Some("jsonc") => Ok(DocFormat::Json),
        Some("yaml") | Some("yml") => Ok(DocFormat::Yaml),
        Some(other) => anyhow::bail!("{}", tr_args(lang, "cli.format.unknown", &[other])),
        None => detect_format(path).ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                tr_args(
                    lang,
                    "cli.format.unrecognized",
                    &[&path.display().to_string()]
                )
            )
        }),
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
fn create_missing_file(file: &Path, fmt: DocFormat, lang: Lang) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "{}",
            tr_args(
                lang,
                "cli.create.non-interactive",
                &[&file.display().to_string()]
            )
        );
    }
    eprint!(
        "{}",
        tr_args(
            lang,
            "cli.create.confirm",
            &[&file.display().to_string(), fmt.name()]
        )
    );
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        anyhow::bail!("{}", tr(lang, "cli.create.cancelled"));
    }
    std::fs::write(file, seed_for(fmt)).map_err(|e| {
        anyhow::anyhow!(
            "{}",
            tr_args(
                lang,
                "cli.create.write-failed",
                &[&file.display().to_string(), &e.to_string()]
            )
        )
    })?;
    Ok(())
}

/// True for a bare `http(s)://` URL passed as `<file>`.
fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Best-effort filename suggestion from a URL's last non-empty path segment
/// (query string and fragment stripped). Falls back to `"config"` when the
/// URL has no path segment — `resolve_format` then rejects it exactly like
/// any other extensionless path.
fn derive_filename_from_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    // Strip `scheme://host` first, so a bare `https://host` or `https://host/`
    // (no path segment at all) doesn't fall back to the hostname.
    let after_host = without_query.split("://").nth(1).unwrap_or(without_query);
    let path = after_host
        .find('/')
        .map(|i| &after_host[i + 1..])
        .unwrap_or("");
    path.rsplit('/')
        .find(|seg| !seg.is_empty())
        .unwrap_or("config")
        .to_string()
}

/// `confy <url>` where `<url>` is an http(s) URL: prompt on the terminal for a
/// local save path (suggesting a name derived from the URL; accepting the
/// blank default keeps the suggestion), then fetch and write the URL's
/// content there so the normal load path can open it exactly like any
/// pre-existing file. A non-interactive stdin aborts without any network
/// call, matching `create_missing_file`'s non-TTY guard.
fn open_url(url: &str, fmt_override: Option<&str>, lang: Lang) -> Result<(PathBuf, DocFormat)> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("{}", tr(lang, "cli.url.non-interactive"));
    }
    let suggested = derive_filename_from_url(url);
    eprint!(
        "{}",
        tr_args(lang, "cli.url.save-prompt", &[url, &suggested])
    );
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let typed = answer.trim();
    let path = PathBuf::from(if typed.is_empty() {
        suggested.as_str()
    } else {
        typed
    });
    let fmt = resolve_format(fmt_override, &path, lang)?;
    let body = ureq::get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("{url}: {e}"))?
        .into_string()
        .map_err(|e| anyhow::anyhow!("{url}: {e}"))?;
    std::fs::write(&path, &body).map_err(|e| {
        anyhow::anyhow!(
            "{}",
            tr_args(
                lang,
                "cli.convert.write-failed",
                &[&path.display().to_string(), &e.to_string()]
            )
        )
    })?;
    Ok((path, fmt))
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
        }) => {
            let lang = resolve_lang(args.lang.as_deref());
            run_convert(&input, &output, from.as_deref(), to.as_deref(), yes, lang)
        }
        None => {
            let lang = resolve_lang(args.lang.as_deref());
            let file = args
                .file
                .ok_or_else(|| anyhow::anyhow!("{}", tr(lang, "cli.no-file")))?;
            if let Some(s) = file.to_str() {
                if is_url(s) {
                    let (path, fmt) = open_url(s, args.format.as_deref(), lang)?;
                    return crate::tui::run(&path, fmt, lang);
                }
            }
            let fmt = resolve_format(args.format.as_deref(), &file, lang)?;
            if !file.exists() {
                create_missing_file(&file, fmt, lang)?;
            }
            crate::tui::run(&file, fmt, lang)
        }
    }
}

fn run_convert(
    input: &Path,
    output: &Path,
    from: Option<&str>,
    to: Option<&str>,
    yes: bool,
    lang: Lang,
) -> Result<()> {
    let from_fmt = resolve_format(from, input, lang)?;
    let to_fmt = resolve_format(to, output, lang)?;

    let crate::LoadedDocument { doc, bom } =
        crate::load_document(input, from_fmt).map_err(|e| {
            anyhow::anyhow!(
                "{}",
                tr_args(
                    lang,
                    "cli.convert.load-failed",
                    &[&input.display().to_string(), &e.to_string()]
                )
            )
        })?;

    let result = match crate::model::convert::convert(&doc, to_fmt) {
        Ok(r) => r,
        Err(abort) => {
            // Conversion aborted: nothing is written.
            anyhow::bail!(
                "{}",
                tr_args(lang, "cli.convert.aborted", &[&abort.to_string()])
            );
        }
    };

    if !result.warnings.is_empty() {
        eprintln!(
            "{}",
            tr_args(
                lang,
                "cli.convert.warnings-header",
                &[from_fmt.name(), to_fmt.name()]
            )
        );
        for w in &result.warnings {
            eprintln!("  • {w}");
        }
        if !yes {
            if std::io::stdin().is_terminal() {
                eprint!(
                    "{}",
                    tr_args(
                        lang,
                        "cli.convert.proceed",
                        &[&output.display().to_string()]
                    )
                );
                std::io::stderr().flush().ok();
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !matches!(answer.trim(), "y" | "Y" | "yes") {
                    anyhow::bail!("{}", tr(lang, "cli.convert.cancelled"));
                }
            } else {
                anyhow::bail!("{}", tr(lang, "cli.convert.refuse-non-interactive"));
            }
        }
    }

    crate::write_document(output, &result.text, bom).map_err(|e| {
        anyhow::anyhow!(
            "{}",
            tr_args(
                lang,
                "cli.convert.write-failed",
                &[&output.display().to_string(), &e.to_string()]
            )
        )
    })?;
    eprintln!(
        "{}",
        tr_args(lang, "cli.convert.wrote", &[&output.display().to_string()])
    );
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
    fn lang_precedence_cli_flag_wins_over_config() {
        use crate::config::Config;
        use confy_core::session::Lang;
        let cfg = Config {
            lang: Some("zh-TW".into()),
        };
        assert_eq!(super::resolve_lang_with(Some("en"), &cfg), Lang::En);
    }

    #[test]
    fn lang_precedence_config_wins_over_default() {
        use crate::config::Config;
        use confy_core::session::Lang;
        let cfg = Config {
            lang: Some("zh-TW".into()),
        };
        assert_eq!(super::resolve_lang_with(None, &cfg), Lang::ZhTw);
    }

    #[test]
    fn lang_precedence_falls_back_to_default_en() {
        use crate::config::Config;
        use confy_core::session::Lang;
        assert_eq!(super::resolve_lang_with(None, &Config::default()), Lang::En);
    }

    #[test]
    fn lang_precedence_invalid_cli_flag_falls_through_to_config() {
        use crate::config::Config;
        use confy_core::session::Lang;
        let cfg = Config {
            lang: Some("zh-TW".into()),
        };
        assert_eq!(
            super::resolve_lang_with(Some("not-a-lang"), &cfg),
            Lang::ZhTw
        );
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

    #[test]
    fn is_url_accepts_http_and_https_only() {
        assert!(super::is_url("https://example.com/a.toml"));
        assert!(super::is_url("http://example.com/a.toml"));
        assert!(!super::is_url("a.toml"));
        assert!(!super::is_url("/abs/path/a.toml"));
        assert!(!super::is_url("ftp://example.com/a.toml"));
    }

    #[test]
    fn derive_filename_from_url_uses_last_path_segment() {
        assert_eq!(
            super::derive_filename_from_url("https://example.com/dir/a.toml"),
            "a.toml"
        );
        assert_eq!(
            super::derive_filename_from_url("https://example.com/a.json?raw=1"),
            "a.json"
        );
        assert_eq!(
            super::derive_filename_from_url("https://example.com/a.yaml#frag"),
            "a.yaml"
        );
        assert_eq!(
            super::derive_filename_from_url("https://example.com/dir/"),
            "dir"
        );
        assert_eq!(
            super::derive_filename_from_url("https://example.com/"),
            "config"
        );
    }

    #[test]
    fn cli_catalog_keys_exist_in_en_and_zh_tw() {
        use confy_core::session::{tr, tr_args, Lang};
        let keys = [
            "cli.convert.warnings-header",
            "cli.convert.proceed",
            "cli.convert.cancelled",
            "cli.convert.refuse-non-interactive",
            "cli.convert.write-failed",
            "cli.convert.wrote",
            "cli.convert.load-failed",
            "cli.convert.aborted",
            "cli.create.non-interactive",
            "cli.create.confirm",
            "cli.create.cancelled",
            "cli.create.write-failed",
            "cli.url.non-interactive",
            "cli.url.save-prompt",
            "cli.no-file",
            "cli.format.unknown",
            "cli.format.unrecognized",
        ];

        for k in keys {
            let en = tr(Lang::En, k);
            let zh = tr(Lang::ZhTw, k);
            assert_ne!(en, k, "key '{k}' missing from en catalog");
            assert_ne!(zh, k, "key '{k}' missing from zh-TW catalog");
            assert_ne!(en, zh, "key '{k}' should have a distinct zh-TW translation");
        }

        // Check sample formatting with arguments
        assert_eq!(
            tr_args(Lang::ZhTw, "cli.convert.wrote", &["out.json"]),
            "已寫入 out.json"
        );
    }

    #[test]
    fn resolve_format_unknown_override_returns_translated_error() {
        use confy_core::session::Lang;
        let err = super::resolve_format(Some("foo"), std::path::Path::new("a.toml"), Lang::ZhTw)
            .unwrap_err();
        assert_eq!(err.to_string(), "未知的格式：foo");
        let err_en = super::resolve_format(Some("foo"), std::path::Path::new("a.toml"), Lang::En)
            .unwrap_err();
        assert_eq!(err_en.to_string(), "unknown format: foo");
    }

    #[test]
    fn resolve_format_unrecognized_ext_returns_translated_error() {
        use confy_core::session::Lang;
        let err = super::resolve_format(None, std::path::Path::new("a.unknown_ext"), Lang::ZhTw)
            .unwrap_err();
        assert_eq!(err.to_string(), "無法識別的設定檔格式：a.unknown_ext");
        let err_en = super::resolve_format(None, std::path::Path::new("a.unknown_ext"), Lang::En)
            .unwrap_err();
        assert_eq!(
            err_en.to_string(),
            "unrecognized config format: a.unknown_ext"
        );
    }
}
