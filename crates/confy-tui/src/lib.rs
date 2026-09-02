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

/// UTF-8 byte-order mark. Windows tools (Notepad, PowerShell `>`) prepend it
/// to JSON/TOML/YAML routinely; none of the parsers accept it as content.
const UTF8_BOM: &str = "\u{feff}";

/// A document read from disk plus the byte-level facts the host must restore
/// on save that the core deliberately doesn't model.
pub struct LoadedDocument {
    pub doc: AnyDocument,
    /// The file started with a UTF-8 BOM; `write_document` re-emits it.
    pub bom: bool,
}

/// Host-side file load — the filesystem boundary. The core never reads files: this
/// reads the bytes, strips a leading BOM (remembered in `LoadedDocument::bom`),
/// parses via the headless [`AnyDocument::from_str_as`], and applies the
/// path-derived display label. Comments are unconditionally legal in every
/// `.json`/`.jsonc` document; no extension-driven setup is needed.
pub fn load_document(path: &Path, format: DocFormat) -> anyhow::Result<LoadedDocument> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (text, bom) = match raw.strip_prefix(UTF8_BOM) {
        Some(rest) => (rest, true),
        None => (raw.as_str(), false),
    };
    let mut doc = AnyDocument::from_str_as(text, format)
        .with_context(|| format!("parsing {}", path.display()))?;
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    doc.set_filename(filename);
    Ok(LoadedDocument { doc, bom })
}

/// Host-side file write — the other half of the filesystem boundary. Writes
/// `text` (re-prefixed with the BOM when `bom`) to a sibling temp file and
/// renames it over `path`, so a crash or kill mid-write can never leave the
/// user's config truncated. On Unix the destination's permission bits are
/// carried over to the temp file before the rename.
pub fn write_document(path: &Path, text: &str, bom: bool) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut tmp = tempfile::Builder::new()
        .prefix(".confy-")
        .suffix(".tmp")
        .tempfile_in(dir.unwrap_or_else(|| Path::new(".")))?;
    if bom {
        tmp.write_all(UTF8_BOM.as_bytes())?;
    }
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(tmp.path(), meta.permissions());
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
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
        let doc = load_document(f.path(), DocFormat::Toml).unwrap().doc;
        let root_label = doc.project().root.key;
        let expected = f.path().file_name().unwrap().to_string_lossy();
        assert_eq!(root_label, expected);
    }

    #[test]
    fn load_document_jsonc_extension_still_loads_correctly() {
        // Extension no longer drives any comment-related fact; a `.jsonc` file
        // with no authored comments yet loads exactly like `.json` would.
        let f = write_temp(".jsonc", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap().doc;
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn load_document_json_with_existing_comment_sets_had_comments_at_open() {
        let f = write_temp(".json", "// hi\n{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap().doc;
        assert!(doc.had_comments_at_open());
    }

    #[test]
    fn load_document_pure_json_has_no_comments_at_open() {
        let f = write_temp(".json", "{}\n");
        let doc = load_document(f.path(), DocFormat::Json).unwrap().doc;
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn load_document_propagates_read_error() {
        let missing = Path::new("/nonexistent/confy/does-not-exist.toml");
        assert!(load_document(missing, DocFormat::Toml).is_err());
    }

    #[test]
    fn load_document_strips_bom_and_write_document_restores_it() {
        let f = write_temp(".json", "\u{feff}{\"a\": 1}\n");
        let loaded = load_document(f.path(), DocFormat::Json).unwrap();
        assert!(loaded.bom);
        assert_eq!(loaded.doc.serialize(), "{\"a\": 1}\n");
        write_document(f.path(), &loaded.doc.serialize(), loaded.bom).unwrap();
        assert_eq!(
            std::fs::read(f.path()).unwrap(),
            "\u{feff}{\"a\": 1}\n".as_bytes()
        );
        // And a BOM-less file stays BOM-less.
        let g = write_temp(".toml", "a = 1\n");
        let loaded = load_document(g.path(), DocFormat::Toml).unwrap();
        assert!(!loaded.bom);
        write_document(g.path(), "a = 2\n", loaded.bom).unwrap();
        assert_eq!(std::fs::read_to_string(g.path()).unwrap(), "a = 2\n");
    }

    #[test]
    fn write_document_replaces_atomically_and_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "a = 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        write_document(&path, "a = 2\n", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "destination mode must survive the rename");
        }
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("c.toml")]);
    }
}
