//! Host-owned config file (Phase 2 of the i18n plan). Persists a small set of
//! TUI preferences — currently just `lang` — across runs. `confy-core` stays
//! filesystem-free; all I/O here lives in `confy-tui`, same as
//! `lib.rs::load_document`.
//!
//! Path convention: `$XDG_CONFIG_HOME/confy/config.toml`, else
//! `~/.config/confy/config.toml` on macOS/Linux (a deliberate terminal-tool
//! convention — NOT `~/Library/Application Support`), or
//! `dirs::config_dir()/confy/config.toml` (`%APPDATA%`) on Windows.
//!
//! A missing or unparsable file always falls back to defaults; it never
//! errors. Writing is best-effort — the caller decides how to surface a
//! failure (never a crash/panic).

use std::io;
use std::path::{Path, PathBuf};

/// The full set of persisted preferences. Room to grow — only `lang` exists
/// today.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Config {
    /// Raw language code (`"en"` / `"zh-TW"`), unvalidated — the caller
    /// parses it via `Lang::from_str` and falls back on failure.
    pub lang: Option<String>,
}

/// The resolved config file path for the current platform/environment.
pub fn config_path() -> PathBuf {
    let dir = if cfg!(windows) {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    dir.join("confy").join("config.toml")
}

/// Load preferences from the standard config path. Never errors: a missing
/// or unparsable file yields `Config::default()`.
pub fn load_config() -> Config {
    load_config_from(&config_path())
}

/// Best-effort write of `cfg` to the standard config path, creating parent
/// directories as needed.
pub fn save_config(cfg: &Config) -> io::Result<()> {
    save_config_to(&config_path(), cfg)
}

fn load_config_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text),
        Err(_) => Config::default(),
    }
}

fn save_config_to(path: &Path, cfg: &Config) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, render_config(cfg))
}

/// Trivial line-oriented `key = "value"` parser — this file only ever holds a
/// couple of scalar keys, so a real TOML crate is overkill. Unknown keys and
/// malformed lines are silently ignored (never an error).
fn parse_config(text: &str) -> Config {
    let mut cfg = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        if key == "lang" && !val.is_empty() {
            cfg.lang = Some(val.to_string());
        }
    }
    cfg
}

fn render_config(cfg: &Config) -> String {
    let mut out = String::new();
    if let Some(lang) = &cfg.lang {
        out.push_str(&format!("lang = \"{lang}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist").join("config.toml");
        let cfg = load_config_from(&path);
        assert_eq!(cfg, Config::default());
        assert!(cfg.lang.is_none());
    }

    #[test]
    fn unparsable_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "!!! not toml at all ###\n").unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn save_then_load_roundtrips_lang() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("confy").join("config.toml");
        let cfg = Config {
            lang: Some("zh-TW".into()),
        };
        save_config_to(&path, &cfg).unwrap();
        let loaded = load_config_from(&path);
        assert_eq!(loaded.lang.as_deref(), Some("zh-TW"));
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("a")
            .join("b")
            .join("confy")
            .join("config.toml");
        let cfg = Config {
            lang: Some("en".into()),
        };
        save_config_to(&path, &cfg).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn parse_ignores_unknown_keys_and_comments() {
        let cfg = parse_config("# a comment\nfoo = \"bar\"\nlang = \"en\"\n");
        assert_eq!(cfg.lang.as_deref(), Some("en"));
    }
}
