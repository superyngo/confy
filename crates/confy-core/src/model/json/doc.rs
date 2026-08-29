//! `JsonDocument` — the lossless JSON/JSONC backend (mirrors `cst_doc.rs`).

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::{NodeKind, NodeTree, Seg};

pub struct JsonDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) original: String,
    /// True while `syntax` differs from `original`. Recomputed at each of the
    /// four points that can change the text (`from_str`, `mark_saved`, `apply`,
    /// `replace_from_str`), each of which already has the new serialization in
    /// hand — so `is_dirty` is O(1) rather than re-serializing the whole
    /// document per call. See `cst_doc.rs` for the full rationale.
    pub(crate) dirty: bool,
    /// Display label for the projection root (host sets it from the source path).
    pub(crate) filename: String,
    /// True iff the file already contained a `//` or `/* */` comment when it
    /// was loaded (content-derived at `from_str`, never written after
    /// construction — comments are always legal to author, so there is
    /// nothing left to "enable").
    pub(crate) had_comments_at_open: bool,
}

impl ConfigDocument for JsonDocument {
    fn project(&self) -> NodeTree {
        crate::model::json::project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn serialize_fragment(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::json::edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // JSON has no dotted scope tables, so relative == absolute fragment.
        self.serialize_fragment(path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        // `edit::apply` returns the already-normalized immutable tree and its
        // serialization, both produced by the single serialize + re-parse it needs
        // for its duplicate-key check — so there is nothing left to recompute here.
        let (syntax, text) = crate::model::json::edit::apply(&self.syntax, m)?;
        self.dirty = text != self.original;
        self.syntax = syntax;
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Json
    }

    fn comment_prefix(&self) -> &'static str {
        "//"
    }

    fn had_comments_at_open(&self) -> bool {
        self.had_comments_at_open
    }

    fn fragment_trailing_comment(&self, path: &[Seg], fragment: &str) -> Option<String> {
        match crate::model::json::edit::resolve(&self.syntax, path) {
            Some(crate::model::json::project::Target::Member(_)) => {
                crate::model::json::edit::fragment_member_trailing_comment(fragment)
            }
            Some(crate::model::json::project::Target::Element(_)) => {
                crate::model::json::edit::fragment_element_trailing_comment(fragment)
            }
            _ => None,
        }
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        kind_options(&self.project(), path)
    }

    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        match key {
            // Keyed member; the array-element `Replace` takes a bare value.
            Some(k) => format!("\"{k}\": {value}\n"),
            None => format!("{value}\n"),
        }
    }

    fn value_kind(&self, value: &str) -> Result<NodeKind, String> {
        // Project the value as the sole member of an object and read its kind.
        let green = crate::model::json::parse::parse(&format!("{{\"__k__\": {value}}}"))?;
        crate::model::json::project::project(&SyntaxNode::new_root(green), "")
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

    fn array_member_keys_addressable(&self) -> bool {
        // A scalar reached via `Key` under an array index (e.g. `arr[0].a`, a member
        // of a block-formatted object element) is looked up by identity through the
        // recursively-built `JsonIndex` (`json/project.rs::walk`), so `replace`/`rename`
        // already splice that member precisely — unlike the array *element* itself
        // (`array_elements_addressable`), which still needs external-edit wrapping.
        true
    }

    fn to_value(
        &self,
    ) -> Result<(crate::model::value::Value, Vec<String>), crate::model::document::ConvertAbort>
    {
        crate::model::convert::tree_to_value(&self.project(), DocFormat::Json)
    }
}

/// Split `value  // comment` via the JSON lexer (a `//` inside a quoted string is
/// not a comment). The value sits on its own line so the comment ends before the
/// closing brace. Returns `(value, Option<comment-with-//>)`; on parse failure or
/// no comment, `(buffer, None)`.
pub(crate) fn split_value_comment(buffer: &str) -> (String, Option<String>) {
    let Ok(green) = crate::model::json::parse::parse(&format!("{{\"__k__\": {buffer}\n}}")) else {
        return (buffer.to_string(), None);
    };
    match crate::model::json::project::project(&SyntaxNode::new_root(green), "")
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

impl JsonDocument {
    /// Parse a document from in-memory text (no file system). `had_comments_at_open`
    /// is derived from content only (a `//` or `/* */` present at load).
    /// The projection root label (`filename`) starts empty; the host sets it via
    /// [`set_filename`](Self::set_filename).
    #[allow(clippy::should_implement_trait)] // named per PORTING.md; see cst_doc.rs
    pub fn from_str(text: &str) -> anyhow::Result<Self> {
        let green = crate::model::json::parse::parse(text)
            .map_err(|e| anyhow::anyhow!("parsing JSON: {e}"))?;
        // Derived from the token stream, not raw text, so a `//` inside a string
        // value does not count as a comment.
        let had_comments_at_open = crate::model::json::parse::lex(text).iter().any(|(k, _)| {
            matches!(
                k,
                crate::model::json::syntax::SyntaxKind::LINE_COMMENT
                    | crate::model::json::syntax::SyntaxKind::BLOCK_COMMENT
            )
        });
        Ok(JsonDocument {
            syntax: SyntaxNode::new_root(green),
            original: text.to_string(),
            dirty: false,
            filename: String::new(),
            had_comments_at_open,
        })
    }

    /// Set the projection root's display label (host derives it from the source path).
    pub fn set_filename(&mut self, name: String) {
        self.filename = name;
    }

    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
        self.dirty = false;
    }

    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::json::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        self.dirty = s != self.original;
        Ok(())
    }
}

/// Per-node convertible-kind list (current notation excluded).
pub(crate) fn kind_options(tree: &NodeTree, path: &[Seg]) -> Vec<(String, KindTarget)> {
    use crate::model::node::{Format, NodeKind, ScalarType};
    let Some(node) = tree.node_at(path) else {
        return Vec::new();
    };
    match &node.kind {
        NodeKind::Table => {
            if node.format == Format::Multiline {
                vec![("inline object  [T/I]".into(), KindTarget::TableInline)]
            } else {
                vec![("multiline object  [T/M]".into(), KindTarget::TableMultiline)]
            }
        }
        NodeKind::Array => {
            if node.format == Format::Multiline {
                vec![("inline array  [A/I]".into(), KindTarget::ArrayInline)]
            } else {
                vec![("multiline array  [A/M]".into(), KindTarget::ArrayMultiline)]
            }
        }
        NodeKind::Scalar(ScalarType::Float) => {
            if node.format == Format::Exponent {
                vec![("plain float  1.5".into(), KindTarget::FloatPlain)]
            } else {
                vec![("exponent float  1e5".into(), KindTarget::FloatExponent)]
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};

    /// Parse `s`. The `ext` parameter is now unused (extension no longer
    /// drives any comment-related fact) but kept so call sites read the same;
    /// tests exercising the old `.jsonc`-extension trigger are deleted below
    /// since that trigger no longer exists.
    fn json_from_str(_ext: &str, s: &str) -> JsonDocument {
        JsonDocument::from_str(s).unwrap()
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "{\n  \"a\": 1\n}\n";
        let doc = json_from_str(".json", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.comment_prefix(), "//");
    }

    #[test]
    fn pure_json_has_no_comments_at_open() {
        let doc = json_from_str(".json", "{}\n");
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn existing_comment_sets_had_comments_at_open() {
        let doc = json_from_str(".json", "// hi\n{}\n");
        assert!(doc.had_comments_at_open());
    }

    #[test]
    fn slashes_inside_string_do_not_set_had_comments_at_open() {
        let doc = json_from_str(".json", "{\n  \"url\": \"https://a.com\"\n}\n");
        assert!(!doc.had_comments_at_open());
        let doc = json_from_str(".json", "{\n  \"glob\": \"/* not a comment */\"\n}\n");
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn scalar_fragment_uses_json_member_and_bare_element() {
        let doc = json_from_str(".json", "{}\n");
        assert_eq!(
            doc.scalar_fragment(Some("tags"), "\"x\""),
            "\"tags\": \"x\"\n"
        );
        assert_eq!(doc.scalar_fragment(None, "42"), "42\n");
    }

    #[test]
    fn json_facets_default_no_scope_table_element_not_addressable() {
        use crate::model::node::NodeKind;
        let doc = json_from_str(".json", "{}\n");
        // No scope table / AoT — an empty object is `{}`, an array `[]`.
        assert_eq!(
            doc.empty_container_fragment(&NodeKind::Table, Some("cfg")),
            "\"cfg\": {}\n"
        );
        assert_eq!(
            doc.empty_container_fragment(&NodeKind::Array, Some("xs")),
            "\"xs\": []\n"
        );
        assert!(!doc.array_elements_addressable());
        assert!(doc.array_member_keys_addressable());
        assert!(!doc.rename_can_change_type());
    }

    #[test]
    fn split_value_comment_splits_json() {
        let doc = json_from_str(".jsonc", "{}\n");
        assert_eq!(doc.split_value_comment("8080"), ("8080".into(), None));
        assert_eq!(
            doc.split_value_comment("8080  // http"),
            ("8080".into(), Some("// http".into()))
        );
        // a `//` inside a string is not the comment
        assert_eq!(
            doc.split_value_comment("\"a // b\""),
            ("\"a // b\"".into(), None)
        );
    }

    #[test]
    fn value_kind_classifies_json_values() {
        use crate::model::node::{NodeKind, ScalarType};
        let doc = json_from_str(".json", "{}\n");
        assert_eq!(
            doc.value_kind("\"hi\"").unwrap(),
            NodeKind::Scalar(ScalarType::String)
        );
        assert_eq!(
            doc.value_kind("42").unwrap(),
            NodeKind::Scalar(ScalarType::Integer)
        );
        assert_eq!(
            doc.value_kind("true").unwrap(),
            NodeKind::Scalar(ScalarType::Bool)
        );
        assert_eq!(
            doc.value_kind("null").unwrap(),
            NodeKind::Scalar(ScalarType::Null)
        );
        assert_eq!(doc.value_kind("[1, 2]").unwrap(), NodeKind::Array);
        assert_eq!(doc.value_kind("{\"a\": 1}").unwrap(), NodeKind::Table);
        // A bare TOML-style value is not legal JSON → Err keeps the editor open.
        assert!(doc.value_kind("oops").is_err());
    }

    #[test]
    fn from_str_rejects_invalid() {
        assert!(JsonDocument::from_str("{ \"a\": }").is_err());
    }

    #[test]
    fn kind_options_per_node() {
        use crate::model::document::KindTarget as KT;
        let doc = json_from_str(".json", "{\n  \"o\": {\n    \"a\": 1\n  },\n  \"arr\": [1],\n  \"f\": 1.5,\n  \"s\": \"x\",\n  \"i\": 7,\n  \"b\": true,\n  \"n\": null\n}\n");
        let opts =
            |p: &[Seg]| -> Vec<KT> { doc.kind_options(p).into_iter().map(|(_, t)| t).collect() };
        // multiline object -> can go inline
        assert_eq!(opts(&[Seg::Key("o".into())]), vec![KT::TableInline]);
        // inline array -> can go multiline
        assert_eq!(opts(&[Seg::Key("arr".into())]), vec![KT::ArrayMultiline]);
        // plain float -> exponent
        assert_eq!(opts(&[Seg::Key("f".into())]), vec![KT::FloatExponent]);
        // string/int/bool/null -> no options
        assert!(opts(&[Seg::Key("s".into())]).is_empty());
        assert!(opts(&[Seg::Key("i".into())]).is_empty());
        assert!(opts(&[Seg::Key("b".into())]).is_empty());
        assert!(opts(&[Seg::Key("n".into())]).is_empty());
    }

    #[test]
    fn kind_options_exponent_float_offers_plain() {
        use crate::model::document::KindTarget as KT;
        let doc = json_from_str(".json", "{ \"f\": 1e3 }\n");
        let opts: Vec<KT> = doc
            .kind_options(&[Seg::Key("f".into())])
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert_eq!(opts, vec![KT::FloatPlain]);
    }
}
