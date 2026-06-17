//! `confy-tui` — the ratatui terminal UI and CLI for confy. Consumes the headless
//! [`confy_core`] crate. The `model` re-export below lets the UI modules keep
//! their `crate::model::…` paths against the core crate (see `PORTING.md`).

pub use confy_core::model;

pub mod cli;
pub mod tui;

use anyhow::Context;
use model::any_doc::AnyDocument;
use model::document::DocFormat;
use std::path::Path;

/// Host-side file load — the filesystem boundary. The core never reads files: this
/// reads the bytes, parses via the headless [`AnyDocument::from_str_as`], applies
/// the path-derived display label, and enables JSONC comments for a `.jsonc`
/// extension (the content-based enable already happens in the core parser).
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
    // A `.jsonc` extension enables authored comments even on a file with none yet.
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonc"))
    {
        doc.enable_comments();
    }
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
    fn load_document_enables_comments_for_jsonc_extension() {
        // A `.jsonc` file with no authored comments yet still gets comment support.
        let f = write_temp(".jsonc", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(doc.supports_comments());
    }

    #[test]
    fn load_document_pure_json_stays_comment_free() {
        let f = write_temp(".json", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(!doc.supports_comments());
    }

    #[test]
    fn load_document_propagates_read_error() {
        let missing = Path::new("/nonexistent/confy/does-not-exist.toml");
        assert!(load_document(missing, DocFormat::Toml).is_err());
    }
}
