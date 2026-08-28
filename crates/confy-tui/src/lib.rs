//! `confy-tui` — the ratatui terminal UI and CLI for confy. Consumes the headless
//! [`confy_core`] crate. The `model` re-export below lets the UI modules keep
//! their `crate::model::…` paths against the core crate (see `PORTING.md`).

pub use confy_core::model;

pub mod cli;
pub mod config;
pub mod tui;

use anyhow::Context;
use model::any_doc::AnyDocument;
use model::document::DocFormat;
use std::path::Path;

/// Host-side file load — the filesystem boundary. The core never reads files: this
/// reads the bytes, parses via the headless [`AnyDocument::from_str_as`], and
/// applies the path-derived display label. Comments are unconditionally legal
/// in every `.json`/`.jsonc` document; no extension-driven setup is needed.
pub fn load_document(path: &Path, format: DocFormat) -> anyhow::Result<AnyDocument> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc = AnyDocument::from_str_as(&text, format)
        .with_context(|| format!("parsing {}", path.display()))?;
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    doc.set_filename(filename);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::document::ConfigDocument;
    use std::io::Write;

    fn write_temp(suffix: &str, body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn load_document_sets_filename_label_from_path() {
        let f = write_temp(".toml", "a = 1\n");
        let doc = load_document(f.path(), DocFormat::Toml).unwrap();
        let root_label = doc.project().root.key;
        let expected = f.path().file_name().unwrap().to_string_lossy();
        assert_eq!(root_label, expected);
    }

    #[test]
    fn load_document_jsonc_extension_still_loads_correctly() {
        // Extension no longer drives any comment-related fact; a `.jsonc` file
        // with no authored comments yet loads exactly like `.json` would.
        let f = write_temp(".jsonc", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn load_document_json_with_existing_comment_sets_had_comments_at_open() {
        let f = write_temp(".json", "// hi\n{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(doc.had_comments_at_open());
    }

    #[test]
    fn load_document_pure_json_has_no_comments_at_open() {
        let f = write_temp(".json", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn load_document_propagates_read_error() {
        let missing = Path::new("/nonexistent/confy/does-not-exist.toml");
        assert!(load_document(missing, DocFormat::Toml).is_err());
    }
}
