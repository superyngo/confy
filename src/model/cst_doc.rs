//! `CstDocument` — the lossless-CST backend (migration in progress; see
//! `docs/superpowers/plans/2026-06-08-cst-backend-migration.md`).
//!
//! The source of truth is a [`taplo`] rowan syntax tree. Unlike `toml_edit`, every
//! byte — including `# comment`, whitespace and newlines — is a real token with a
//! position, so a standalone comment is a *first-class, independently-positioned
//! node* (not decor glued to the following item) and `serialize()` is a lossless
//! token concatenation.
//!
//! Phase 1 (this file) implements `load`/`serialize` with a byte-identical
//! round-trip guarantee. `project` (Phase 2) and `apply` (Phase 3) are stubs until
//! ported; `CstDocument` is **not** wired into the TUI until it reaches parity with
//! `TomlDocument` (Phase 5).

use std::path::{Path, PathBuf};

use anyhow::Context;
use taplo::syntax::SyntaxNode;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::node::{Format, NodeKind, NodeTree, Seg};

pub struct CstDocument {
    /// The rowan syntax tree root — the single source of truth.
    pub(crate) syntax: SyntaxNode,
    /// Read in Phase 5 (TUI wiring) as the save target.
    #[allow(dead_code)]
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for CstDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let parse = taplo::parser::parse(&original);
        if let Some(err) = parse.errors.first() {
            anyhow::bail!("parsing {} as TOML: {}", path.display(), err);
        }
        let syntax = parse.into_syntax();
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(CstDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::cst_project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }

    fn serialize_fragment(&self, path: &[crate::model::node::Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::cst_edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[crate::model::node::Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::cst_edit::serialize_fragment_relative(&self.syntax, path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        // Mutate a copy and commit only on success (free atomic rollback). The edit
        // works on a `clone_for_update` (mutable) tree; normalize the result back to
        // an immutable tree (re-parse is byte-identical) so the next `apply` can
        // `clone_for_update` again.
        let new = crate::model::cst_edit::apply(&self.syntax, m)?;
        self.syntax = taplo::parser::parse(&new.to_string()).into_syntax();
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Toml
    }
    fn comment_prefix(&self) -> &'static str {
        "#"
    }
    fn supports_comments(&self) -> bool {
        true
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        use crate::model::document::KindTarget as KT;
        use crate::model::node::ScalarType as ST;
        let tree = self.project();
        let Some(node) = tree.node_at(path) else {
            return Vec::new();
        };
        // A scalar switches between notations of its own type; the current
        // notation is excluded. Bools/datetimes (and inf/nan floats) have one
        // notation — no options, reported below.
        let options: Vec<(String, KT)> = match &node.kind {
            NodeKind::Scalar(st) => match st {
                ST::String => [
                    (Format::BasicString, "basic string  \"…\"", KT::StringBasic),
                    (Format::Literal, "literal string  '…'", KT::StringLiteral),
                    (
                        Format::MultilineBasic,
                        "multiline string  \"\"\"…\"\"\"",
                        KT::StringMultiline,
                    ),
                    (
                        Format::MultilineLiteral,
                        "multiline literal  '''…'''",
                        KT::StringMultilineLiteral,
                    ),
                ]
                .iter()
                .filter(|(f, ..)| *f != node.format)
                .map(|(_, l, t)| ((*l).into(), *t))
                .collect(),
                ST::Integer => [
                    (Format::Decimal, "decimal", KT::IntDecimal),
                    (Format::Hex, "hex  0x…", KT::IntHex),
                    (Format::Octal, "octal  0o…", KT::IntOctal),
                    (Format::Binary, "binary  0b…", KT::IntBinary),
                ]
                .iter()
                .filter(|(f, ..)| *f != node.format)
                .map(|(_, l, t)| ((*l).into(), *t))
                .collect(),
                ST::Float if node.format == Format::Plain => {
                    // Exponent notation is told from the value text — `Format`
                    // has no variant for it.
                    let is_exp = node
                        .value
                        .as_deref()
                        .is_some_and(|v| v.contains(['e', 'E']));
                    if is_exp {
                        vec![("plain float  1.5".into(), KT::FloatPlain)]
                    } else {
                        vec![("exponent float  1e5".into(), KT::FloatExponent)]
                    }
                }
                _ => Vec::new(),
            },
            NodeKind::Array => {
                let mut opts: Vec<(String, KT)> = if node.value.is_some() {
                    vec![("multiline array  [A/M]".into(), KT::ArrayMultiline)]
                } else {
                    vec![("inline array  [A/I]".into(), KT::ArrayInline)]
                };
                // All-inline-table elements: the array can become an `[[…]]` group.
                let elems: Vec<_> = node
                    .children
                    .iter()
                    .filter(|c| !matches!(c.kind, NodeKind::Comment(_)))
                    .collect();
                if !elems.is_empty()
                    && elems
                        .iter()
                        .all(|c| matches!(c.kind, NodeKind::InlineTable))
                {
                    opts.push(("array of tables  [A/T]".into(), KT::ArrayOfTables));
                }
                opts
            }
            NodeKind::ArrayOfTables => vec![
                ("inline array     [A/I]".into(), KT::ArrayInline),
                ("multiline array  [A/M]".into(), KT::ArrayMultiline),
            ],
            NodeKind::InlineTable => vec![
                ("dotted table  [T/D]".into(), KT::TableDotted),
                ("table scope   [T/S]".into(), KT::TableScope),
            ],
            NodeKind::Table if node.format == Format::Dotted => vec![
                ("inline table  [T/I]".into(), KT::TableInline),
                ("table scope   [T/S]".into(), KT::TableScope),
            ],
            NodeKind::Table if matches!(path.last(), Some(Seg::Key(_))) => vec![
                ("dotted table  [T/D]".into(), KT::TableDotted),
                ("inline table  [T/I]".into(), KT::TableInline),
            ],
            _ => Vec::new(),
        };
        options
    }

    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        // An array element has no key; taplo can't parse a bare top-level value,
        // so it is wrapped under a synthetic key the element `Replace` ignores.
        let k = key.unwrap_or("__elem__");
        format!("{k} = {value}\n")
    }

    fn value_kind(&self, value: &str) -> Result<NodeKind, String> {
        let parse = taplo::parser::parse(&format!("__k__ = {value}\n"));
        if let Some(err) = parse.errors.first() {
            return Err(err.to_string());
        }
        crate::model::cst_project::project(&parse.into_syntax(), "")
            .root
            .children
            .into_iter()
            .next()
            .map(|n| n.kind)
            .ok_or_else(|| "fragment has no value".into())
    }

    fn split_value_comment(&self, buffer: &str) -> (String, Option<String>) {
        split_value_comment(buffer)
    }
}

/// Split `value  # comment` via taplo's lexer (a `#` inside a string is not a
/// comment). Returns `(value, Option<comment-with-#>)`; on parse failure or no
/// comment, `(buffer, None)`.
pub(crate) fn split_value_comment(buffer: &str) -> (String, Option<String>) {
    let parse = taplo::parser::parse(&format!("__k__ = {buffer}\n"));
    if !parse.errors.is_empty() {
        return (buffer.to_string(), None);
    }
    match crate::model::cst_project::project(&parse.into_syntax(), "")
        .root
        .children
        .into_iter()
        .next()
    {
        Some(n) => (
            n.value.unwrap_or_else(|| buffer.to_string()),
            n.trailing_comment,
        ),
        None => (buffer.to_string(), None),
    }
}

impl CstDocument {
    /// Write the current document to its source path.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    /// Reset the dirty baseline so `is_dirty()` returns false.
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    /// Re-parse the document from a serialized snapshot string (undo/redo restore).
    /// Propagates a parse error rather than silently no-op'ing.
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let parse = taplo::parser::parse(s);
        if let Some(e) = parse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        self.syntax = parse.into_syntax();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cst_from_str(s: &str) -> CstDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        CstDocument::load(f.path()).unwrap()
    }

    /// The Phase 1 contract: load → serialize is byte-identical for every fixture
    /// and the project's own `test.toml`. This is the go/no-go gate for the backend.
    #[test]
    fn roundtrip_is_byte_identical() {
        // `test.toml` now lives under tests/fixtures/ and is picked up by the scan.
        let mut files: Vec<PathBuf> = Vec::new();
        let fx = Path::new("tests/fixtures");
        if fx.is_dir() {
            for e in std::fs::read_dir(fx).unwrap() {
                let p = e.unwrap().path();
                if p.extension().map(|x| x == "toml").unwrap_or(false) {
                    files.push(p);
                }
            }
        }
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap();
            let doc = CstDocument::load(f).unwrap();
            assert_eq!(
                doc.serialize(),
                text,
                "round-trip not byte-identical for {f:?}"
            );
        }
    }

    #[test]
    fn scalar_fragment_uses_toml_assignment() {
        let doc = cst_from_str("a = 1\n");
        assert_eq!(doc.scalar_fragment(Some("tags"), "\"x\""), "tags = \"x\"\n");
        // An element wraps under the synthetic key the element Replace ignores.
        assert_eq!(doc.scalar_fragment(None, "42"), "__elem__ = 42\n");
    }

    #[test]
    fn split_value_comment_splits_toml() {
        let doc = cst_from_str("a = 1\n");
        assert_eq!(doc.split_value_comment("8080"), ("8080".into(), None));
        assert_eq!(
            doc.split_value_comment("8080  # http"),
            ("8080".into(), Some("# http".into()))
        );
        // a `#` inside a string is not the comment
        assert_eq!(
            doc.split_value_comment("\"a # b\""),
            ("\"a # b\"".into(), None)
        );
    }

    #[test]
    fn value_kind_classifies_toml_values() {
        use crate::model::node::ScalarType;
        let doc = cst_from_str("a = 1\n");
        assert_eq!(
            doc.value_kind("\"hi\"").unwrap(),
            NodeKind::Scalar(ScalarType::String)
        );
        assert_eq!(
            doc.value_kind("42").unwrap(),
            NodeKind::Scalar(ScalarType::Integer)
        );
        assert_eq!(doc.value_kind("[1, 2]").unwrap(), NodeKind::Array);
        assert!(doc.value_kind("= bad").is_err());
    }

    #[test]
    fn load_rejects_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"this is = = not toml").unwrap();
        assert!(CstDocument::load(f.path()).is_err());
    }

    #[test]
    fn toml_format_facets() {
        let doc = cst_from_str("a = 1\n");
        assert_eq!(doc.format(), DocFormat::Toml);
        assert_eq!(doc.comment_prefix(), "#");
        assert!(doc.supports_comments());
    }

    #[test]
    fn serialize_roundtrips_a_small_doc() {
        let src = "# top\nbasic = \"x\"\n\n# sec\n[srv]\nport = 8080\n";
        let doc = cst_from_str(src);
        assert_eq!(doc.serialize(), src);
    }
}
