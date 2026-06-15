//! `YamlDocument` — the lossless YAML-subset backend (mirrors `json/doc.rs`).

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::node::{NodeKind, NodeTree, Seg};
use crate::model::yaml::syntax::SyntaxNode;

pub struct YamlDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for YamlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let green = crate::model::yaml::parse::parse(&original)
            .map_err(|e| anyhow::anyhow!("parsing {} as YAML: {}", path.display(), e))?;
        let syntax = SyntaxNode::new_root(green);
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(YamlDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::yaml::project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }

    fn serialize_fragment(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::yaml::edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // YAML has no dotted scope tables; relative == absolute fragment.
        self.serialize_fragment(path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        // NOTE: edit::apply is a passthrough stub until Tasks 5–6 — mutations
        // are currently no-ops (the re-parse round-trip below is a no-op too).
        let new = crate::model::yaml::edit::apply(&self.syntax, m)?;
        let text = new.to_string();
        let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Yaml
    }

    fn comment_prefix(&self) -> &'static str {
        "#"
    }

    fn supports_comments(&self) -> bool {
        true
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        kind_options(&self.project(), path)
    }

    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        match key {
            Some(k) => format!("{k}: {value}\n"),
            None => format!("- {value}\n"),
        }
    }

    fn value_kind(&self, value: &str) -> Result<NodeKind, String> {
        // Project the value as the sole member of a mapping and read its kind.
        let green = crate::model::yaml::parse::parse(&format!("__k__: {value}\n"))?;
        crate::model::yaml::project::project(&SyntaxNode::new_root(green), "")
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

    fn replace_preserves_trailing_comment(&self) -> bool {
        // YAML `Replace` swaps the whole `key: value` entry, dropping the comment.
        false
    }

    fn to_value(
        &self,
    ) -> Result<(crate::model::value::Value, Vec<String>), crate::model::document::ConvertAbort>
    {
        crate::model::convert::tree_to_value(&self.project(), DocFormat::Yaml)
    }
}

/// Split `value  # comment` via the YAML lexer (a `#` inside a quoted string is
/// not a comment). Returns `(value, Option<comment-with-#>)`; on a parse failure
/// or no comment, `(buffer, None)`.
pub(crate) fn split_value_comment(buffer: &str) -> (String, Option<String>) {
    let Ok(green) = crate::model::yaml::parse::parse(&format!("__k__: {buffer}\n")) else {
        return (buffer.to_string(), None);
    };
    match crate::model::yaml::project::project(&SyntaxNode::new_root(green), "")
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

impl YamlDocument {
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::yaml::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }
}

/// Per-node convertible-kind list (current notation excluded).
pub(crate) fn kind_options(tree: &NodeTree, path: &[Seg]) -> Vec<(String, KindTarget)> {
    use crate::model::node::{Format, NodeKind, ScalarType};
    let Some(node) = tree.node_at(path) else {
        return Vec::new();
    };
    if node.read_only {
        return Vec::new();
    }
    // A member sitting inside an inline flow collection can't take a block layout
    // (block expansion, literal/folded scalars) without breaking the one line.
    let in_flow = inside_inline_collection(tree, path);
    match &node.kind {
        NodeKind::Table | NodeKind::InlineTable | NodeKind::Array => {
            if node.format == Format::Inline {
                if in_flow {
                    Vec::new() // can't expand an inline member to block
                } else {
                    vec![("block  [_/B]".into(), KindTarget::Block)]
                }
            } else {
                vec![("flow  [_/F]".into(), KindTarget::Flow)]
            }
        }
        NodeKind::Scalar(ScalarType::String) => {
            let all = [
                (Format::Plain, "plain", KindTarget::StringPlain),
                (Format::SingleQuoted, "single", KindTarget::StringSingle),
                (Format::DoubleQuoted, "double", KindTarget::StringDouble),
                (
                    Format::LiteralBlock,
                    "literal |",
                    KindTarget::StringLiteralBlock,
                ),
                (Format::Folded, "folded >", KindTarget::StringFolded),
            ];
            all.iter()
                .filter(|(f, ..)| *f != node.format)
                // Block scalars are multi-line: not available inside a flow line.
                .filter(|(f, ..)| !(in_flow && matches!(f, Format::LiteralBlock | Format::Folded)))
                .map(|(_, l, t)| (l.to_string(), *t))
                .collect()
        }
        NodeKind::Scalar(ScalarType::Integer) => {
            let all = [
                (Format::Decimal, "dec", KindTarget::IntDecimal),
                (Format::Hex, "hex 0x", KindTarget::IntHex),
                (Format::Octal, "oct 0o", KindTarget::IntOctal),
            ];
            all.iter()
                .filter(|(f, ..)| *f != node.format)
                .map(|(_, l, t)| (l.to_string(), *t))
                .collect()
        }
        NodeKind::Scalar(ScalarType::Float) => {
            if node.format == Format::Exponent {
                vec![("plain float".into(), KindTarget::FloatPlain)]
            } else if node.format == Format::Plain {
                vec![("exponent float".into(), KindTarget::FloatExponent)]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// `true` if any strict ancestor of `path` is an inline flow collection.
fn inside_inline_collection(tree: &NodeTree, path: &[Seg]) -> bool {
    use crate::model::node::{Format, NodeKind};
    (1..path.len()).any(|len| {
        tree.node_at(&path[..len]).is_some_and(|n| {
            matches!(n.kind, NodeKind::InlineTable)
                || (matches!(n.kind, NodeKind::Array) && n.format == Format::Inline)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};
    use std::io::Write;

    /// Create a temp file with the given extension, write `s`, load it.
    fn yaml_from_str(ext: &str, s: &str) -> YamlDocument {
        let mut f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        YamlDocument::load(f.path()).unwrap()
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "a: 1\nb: two\n";
        let doc = yaml_from_str(".yaml", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Yaml);
        assert_eq!(doc.comment_prefix(), "#");
        assert!(doc.supports_comments());
    }

    #[test]
    fn load_rejects_multi_doc() {
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        f.write_all(b"---\na: 1\n---\nb: 2\n").unwrap();
        assert!(YamlDocument::load(f.path()).is_err());
    }

    #[test]
    fn kind_options_per_node() {
        use crate::model::document::KindTarget as KT;
        let src = "blk:\n  x: 1\nflow: {a: 1}\nq: 'hi'\nh: 0xff\ne: 1.5e3\n";
        let doc = yaml_from_str(".yaml", src);
        let opts =
            |p: &[Seg]| -> Vec<KT> { doc.kind_options(p).into_iter().map(|(_, t)| t).collect() };
        // block mapping -> flow
        assert_eq!(opts(&[Seg::Key("blk".into())]), vec![KT::Flow]);
        // flow mapping -> block
        assert_eq!(opts(&[Seg::Key("flow".into())]), vec![KT::Block]);
        // single-quoted string -> plain/double/literal/folded (single excluded)
        assert_eq!(
            opts(&[Seg::Key("q".into())]),
            vec![
                KT::StringPlain,
                KT::StringDouble,
                KT::StringLiteralBlock,
                KT::StringFolded
            ]
        );
        // hex int -> dec/oct (hex excluded)
        assert_eq!(
            opts(&[Seg::Key("h".into())]),
            vec![KT::IntDecimal, KT::IntOctal]
        );
        // exponent float -> plain
        assert_eq!(opts(&[Seg::Key("e".into())]), vec![KT::FloatPlain]);
    }

    #[test]
    fn kind_options_inside_flow_hides_block_forms() {
        use crate::model::document::KindTarget as KT;
        let src = "pt: {s: hi, n: {x: 1}}\n";
        let doc = yaml_from_str(".yaml", src);
        let opts =
            |p: &[Seg]| -> Vec<KT> { doc.kind_options(p).into_iter().map(|(_, t)| t).collect() };
        // A plain string member of a flow map: single/double only (plain is the
        // current form; the literal/folded block scalars are dropped inside flow).
        assert_eq!(
            opts(&[Seg::Key("pt".into()), Seg::Key("s".into())]),
            vec![KT::StringSingle, KT::StringDouble]
        );
        // A nested flow map member can't expand to block while inline.
        assert!(opts(&[Seg::Key("pt".into()), Seg::Key("n".into())]).is_empty());
    }

    #[test]
    fn split_value_comment_splits_yaml() {
        let doc = yaml_from_str(".yaml", "a: 1\n");
        assert_eq!(doc.split_value_comment("x"), ("x".into(), None));
        assert_eq!(
            doc.split_value_comment("x  # bind"),
            ("x".into(), Some("# bind".into()))
        );
        // a `#` inside a quoted string is not the comment
        assert_eq!(
            doc.split_value_comment("\"a # b\""),
            ("\"a # b\"".into(), None)
        );
    }

    #[test]
    fn scalar_fragment_uses_yaml_forms() {
        let doc = yaml_from_str(".yaml", "a: 1\n");
        assert_eq!(doc.scalar_fragment(Some("k"), "v"), "k: v\n");
        assert_eq!(doc.scalar_fragment(None, "v"), "- v\n");
    }
}
