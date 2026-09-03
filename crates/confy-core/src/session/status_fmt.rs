//! Kind/type/format label formatting + small scalar-repr/string utilities
//! shared across `session.rs` and its split-out siblings — split out of
//! `session.rs` (Task 15, 2026-08-11 audit remediation).

use crate::model::document::DocFormat;
use crate::model::node::{Format, KeySign, NodeKind, ScalarType};

pub fn node_type_label_str(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Root => "",
        NodeKind::Table => "table",
        NodeKind::ArrayOfTables => "array-of-tables",
        NodeKind::Array => "array",
        NodeKind::InlineTable => "inline",
        NodeKind::Scalar(ScalarType::String) => "string",
        NodeKind::Scalar(ScalarType::Integer) => "integer",
        NodeKind::Scalar(ScalarType::Float) => "float",
        NodeKind::Scalar(ScalarType::Bool) => "bool",
        NodeKind::Scalar(ScalarType::Null) => "null",
        NodeKind::Scalar(ScalarType::OffsetDatetime) => "offsetdatetime",
        NodeKind::Scalar(ScalarType::LocalDatetime) => "localdatetime",
        NodeKind::Scalar(ScalarType::LocalDate) => "localdate",
        NodeKind::Scalar(ScalarType::LocalTime) => "localtime",
        NodeKind::Comment(_) => "comment",
    }
}

/// Kind-badge label + its notation-suffix note, e.g. `("{}", "scope")`,
/// `("int", "dec")`. A container's label is an **outline glyph** (`{}` for
/// every table/map notation, `[]` for every array/sequence notation) while
/// scalars keep a friendly short word (`str`, `int`, `date`); the note
/// carries the notation that the glyph deliberately doesn't spell out and is
/// empty when it would just repeat the label. Computed once here so every
/// host renders the identical badge without re-deriving type/format
/// heuristics client-side (previously duplicated across
/// `web/kind-labels.ts`'s `kindLabelParts`/`notationGlyph`, `panel.ts`,
/// `render.ts`, `touch/render.ts`).
///
/// `doc` only picks the wording of an inline container's note: YAML calls
/// `{x: 1}` a **flow** collection, TOML/JSON call it **inline**.
pub fn badge_label_note(
    kind: &NodeKind,
    format: Format,
    is_branch: bool,
    scalar_type: Option<ScalarType>,
    doc: DocFormat,
) -> (&'static str, &'static str) {
    let label = match node_type_label_str(kind) {
        // Every table/map notation shares one glyph, every array/sequence
        // notation the other; the distinguishing notation is the note's job.
        // `"inline"` is `NodeKind::InlineTable`, i.e. a *table* — reading it
        // as a notation word used to collapse the note away (a TOML inline
        // table and a YAML flow map both badged a bare `inline`).
        "table" | "inline" => "{}",
        "array" | "array-of-tables" => "[]",
        "string" => "str",
        "integer" => "int",
        "float" => "float",
        "bool" => "bool",
        "null" => "null",
        "offsetdatetime" | "localdatetime" | "localdate" => "date",
        "localtime" => "time",
        other => other, // "" (root), "comment" pass through unchanged
    };
    let container_note = |fmt: Format| -> &'static str {
        match fmt {
            Format::Scope => "scope",
            Format::Dotted => "dotted",
            // YAML's own term for `{x: 1}` / `[1, 2]` is a *flow* collection;
            // TOML/JSON call the same shape inline.
            Format::Inline if doc == DocFormat::Yaml => "flow",
            Format::Inline => "inline",
            Format::Multiline => "multi",
            Format::Block => "block",
            _ => "",
        }
    };
    let notation_short = |fmt: Format| -> &'static str {
        match fmt {
            Format::BasicString => "\"…\"",
            Format::Decimal => "dec",
            Format::Literal => "'…'",
            Format::MultilineBasic => "\"\"\"",
            Format::MultilineLiteral => "'''",
            Format::Multiline => "\"\"\"",
            Format::Hex => "0x",
            Format::Octal => "0o",
            Format::Binary => "0b",
            Format::Exponent => "1e",
            Format::SingleQuoted => "'…'",
            Format::DoubleQuoted => "\"…\"",
            Format::LiteralBlock => "|",
            Format::Folded => ">",
            Format::Inf => "inf",
            Format::Nan => "nan",
            _ => "",
        }
    };
    let note = if is_branch {
        // An array-of-tables has no `Format` of its own, so the note is the
        // one thing distinguishing `[[a]]` from a plain array under the
        // shared `[]` glyph.
        if matches!(kind, NodeKind::ArrayOfTables) {
            "AoT"
        } else {
            container_note(format)
        }
    } else {
        let s = notation_short(format);
        if !s.is_empty() {
            s
        } else if scalar_type == Some(ScalarType::Float) && format == Format::Plain {
            "dec"
        } else {
            ""
        }
    };
    (label, note)
}

/// The full type label for a node kind (matches node_type_label in app.rs).
pub fn node_type_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Scalar(st) => format!("{st:?}").to_lowercase(),
        other => node_type_label_str(other).to_string(),
    }
}

/// Label for a node's key sign (`bare`/`quoted`/`dotted`/`none`).
pub fn key_sign_label(sign: KeySign) -> &'static str {
    match sign {
        KeySign::Bare => "bare",
        KeySign::Quoted => "quoted",
        KeySign::Dotted => "dotted",
        KeySign::None => "none",
    }
}

pub(crate) fn branch_type_format(kind: &NodeKind) -> (&'static str, &'static str) {
    match kind {
        NodeKind::Root => ("root", "-"),
        NodeKind::Table => ("table", "table"),
        NodeKind::InlineTable => ("table", "inline"),
        NodeKind::Array => ("array", "array"),
        NodeKind::ArrayOfTables => ("array", "array-of-tables"),
        NodeKind::Scalar(_) | NodeKind::Comment(_) => ("unknown", "-"),
    }
}

pub fn format_label(fmt: Format) -> Option<&'static str> {
    match fmt {
        Format::Literal => Some("literal"),
        Format::MultilineBasic => Some("multiline-basic"),
        Format::MultilineLiteral => Some("multiline-literal"),
        Format::Hex => Some("hex"),
        Format::Octal => Some("octal"),
        Format::Binary => Some("binary"),
        Format::Inline => Some("inline"),
        Format::Dotted => Some("dotted"),
        Format::Scope => Some("scope"),
        Format::Multiline => Some("multiline"),
        Format::SingleQuoted => Some("single-quoted"),
        Format::DoubleQuoted => Some("double-quoted"),
        Format::LiteralBlock => Some("literal-block"),
        Format::Folded => Some("folded"),
        Format::Block => Some("block"),
        Format::Inf => Some("inf"),
        Format::Nan => Some("nan"),
        Format::Exponent => Some("exponent"),
        Format::BasicString => Some("basic-string"),
        Format::Decimal => Some("decimal"),
        Format::Plain => None,
    }
}

/// Default file extension for a convert target format.
pub(crate) fn default_ext(fmt: DocFormat) -> &'static str {
    match fmt {
        DocFormat::Toml => "toml",
        DocFormat::Json => "json",
        DocFormat::Yaml => "yaml",
    }
}

pub(crate) fn char_byte_idx(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

pub(crate) fn clamp_scroll(scroll: usize, cursor: usize, len: usize, width: usize) -> usize {
    let w = width.max(1);
    let cur = cursor.min(len);
    let mut s = scroll;
    if cur < s {
        s = cur;
    } else if cur >= s + w {
        s = cur + 1 - w;
    }
    s.min((len + 1).saturating_sub(w))
}

pub(crate) fn unique_key(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|k| k == base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}_{n}");
        if !existing.iter().any(|k| k == &cand) {
            return cand;
        }
        n += 1;
    }
}

pub(crate) fn project_first_label(fragment: &str) -> Option<String> {
    let parse = taplo::parser::parse(fragment);
    if !parse.errors.is_empty() {
        return None;
    }
    crate::model::cst_project::project(&parse.into_syntax(), "")
        .root
        .children
        .first()
        .map(|n| node_type_label(&n.kind))
}

/// A schema enum/const JSON value's text repr for `ConfigDocument::scalar_fragment`.
/// `format!("{:?}", s)` (Rust's Debug for `&str`) produces a `"…"`
/// backslash-escaped double-quoted form that is simultaneously valid TOML
/// basic-string, JSON string, and YAML double-quoted syntax — one repr
/// serves all three backends. `Json::Null` has no TOML representation (spec
/// §2: TOML never produces `Value::Null`) — filtered out for a TOML
/// document so the enum picker never offers an unwritable option.
pub(crate) fn scalar_repr_for(
    v: &serde_json::Value,
    format: crate::model::document::DocFormat,
) -> Option<String> {
    use crate::model::document::DocFormat;
    match v {
        serde_json::Value::String(s) => Some(format!("{s:?}")),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null if format == DocFormat::Toml => None,
        serde_json::Value::Null => Some("null".to_string()),
        _ => None, // arrays/objects are not valid scalar enum options
    }
}
