//! Document-level cross-format conversion (spec §Phase 4).
//!
//! Pipeline: `source backend --to_value()--> Value (+warnings) --render(target)-->
//! String --(reparse check)--> result`. The source document is never modified.
//!
//! [`tree_to_value`] is the single generic walk shared by every backend's
//! `to_value`: it maps the projected [`NodeTree`] to the format-neutral
//! [`Value`] tree (containers by `NodeKind`, comments to `Item::Comment`,
//! trailing comments to `Item::Node.trailing`), gathers **normalization
//! warnings** from each node's writing style, and **aborts** on a YAML opaque
//! node. Scalar text is decoded per source format by the `decode_*` helpers.
//!
//! The renderers emit each target's **default style only** (the documented lossy
//! contract): TOML scope tables + bare keys, JSON 2-space multiline (`//`
//! comments only when present ⇒ JSONC), YAML block + plain scalars.

use crate::model::any_doc::AnyDocument;
use crate::model::document::{ConfigDocument, ConvertAbort, DocFormat};
use crate::model::node::{Format, Node, NodeKind, NodeTree, ScalarType, Seg};
use crate::model::value::{Item, Value};

/// The result of a successful conversion: the rendered output text plus the
/// up-front list of (deduplicated) lossy-normalization warnings.
#[derive(Debug)]
pub struct ConvertResult {
    pub text: String,
    pub warnings: Vec<String>,
}

/// Convert `doc` to `target`, applying the spec's loss & legality policy.
/// `Err(ConvertAbort)` (no output) when the source holds a construct the target
/// cannot represent (`null` → TOML, YAML opaque nodes) or the rendered output
/// fails to re-parse. The source document is untouched.
pub fn convert(doc: &AnyDocument, target: DocFormat) -> Result<ConvertResult, ConvertAbort> {
    let (value, mut warnings) = doc.to_value()?;
    analyze(&value, target, &mut warnings)?;

    let text = match target {
        DocFormat::Toml => render_toml(&value)?,
        DocFormat::Json => render_json(&value),
        DocFormat::Yaml => render_yaml(&value),
    };

    reparse_check(&text, target)?;

    warnings.sort();
    warnings.dedup();
    Ok(ConvertResult { text, warnings })
}

// ── source → Value (generic walk) ──────────────────────────────────────────────

/// Lower a projected [`NodeTree`] to the neutral [`Value`] tree, decoding scalars
/// as `src` and gathering normalization warnings. Shared by every backend's
/// `to_value`.
pub fn tree_to_value(
    tree: &NodeTree,
    src: DocFormat,
) -> Result<(Value, Vec<String>), ConvertAbort> {
    let mut warnings = Vec::new();
    let value = root_to_value(&tree.root, src, &mut warnings)?;
    warnings.sort();
    warnings.dedup();
    Ok((value, warnings))
}

fn is_comment(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Comment(_))
}

/// The Root projects as a Map (keyed children), a Seq (keyless value children),
/// or — for a bare-scalar document — that scalar directly.
fn root_to_value(
    root: &Node,
    src: DocFormat,
    warnings: &mut Vec<String>,
) -> Result<Value, ConvertAbort> {
    let non_comment: Vec<&Node> = root.children.iter().filter(|c| !is_comment(c)).collect();

    // Bare-scalar document (JSON/YAML): one *keyless* scalar leaf, no comments.
    if root.children.len() == 1
        && non_comment.len() == 1
        && matches!(non_comment[0].kind, NodeKind::Scalar(_))
        && !matches!(non_comment[0].path.last(), Some(Seg::Key(_)))
    {
        return node_value(non_comment[0], src, warnings);
    }

    let keyed = non_comment
        .iter()
        .any(|c| matches!(c.path.last(), Some(Seg::Key(_))));
    let items = items_of(&root.children, src, warnings)?;
    if keyed || non_comment.is_empty() {
        Ok(Value::Map(items))
    } else {
        Ok(Value::Seq(items))
    }
}

/// The neutral value of one node (container or scalar). Aborts on a YAML opaque
/// node (read-only and not a comment); records the node's style warning.
fn node_value(
    node: &Node,
    src: DocFormat,
    warnings: &mut Vec<String>,
) -> Result<Value, ConvertAbort> {
    if node.read_only && !is_comment(node) {
        return Err(ConvertAbort(
            "file contains unsupported YAML constructs (anchors, aliases, merge keys, or tags)"
                .into(),
        ));
    }
    if let Some(note) = style_note(node) {
        warnings.push(note.to_string());
    }
    match &node.kind {
        NodeKind::Table | NodeKind::InlineTable => {
            Ok(Value::Map(items_of(&node.children, src, warnings)?))
        }
        NodeKind::Array | NodeKind::ArrayOfTables => {
            Ok(Value::Seq(items_of(&node.children, src, warnings)?))
        }
        NodeKind::Scalar(_) => Ok(decode_scalar(src, node)),
        NodeKind::Root => root_to_value(node, src, warnings),
        NodeKind::Comment(_) => Ok(Value::Str(String::new())), // unreachable via items_of
    }
}

/// Project a node's children into ordered [`Item`]s (comments interleaved).
fn items_of(
    children: &[Node],
    src: DocFormat,
    warnings: &mut Vec<String>,
) -> Result<Vec<Item>, ConvertAbort> {
    let mut out = Vec::with_capacity(children.len());
    for child in children {
        if is_comment(child) {
            let raw = child.value.as_deref().unwrap_or(&child.key);
            out.push(Item::Comment(strip_markers(raw)));
            continue;
        }
        let value = node_value(child, src, warnings)?;
        let key = match child.path.last() {
            Some(Seg::Key(k)) => Some(k.clone()),
            _ => None,
        };
        let trailing = child.trailing_comment.as_deref().map(strip_markers);
        out.push(Item::Node {
            key,
            value,
            trailing,
        });
    }
    Ok(out)
}

/// A node's writing style that the default-style render will drop, as a warning
/// (or `None` when the style is already the target-neutral default).
fn style_note(node: &Node) -> Option<&'static str> {
    if matches!(node.kind, NodeKind::ArrayOfTables) {
        return Some("array-of-tables normalized to the target's default array style");
    }
    match node.format {
        Format::Hex | Format::Octal | Format::Binary => {
            Some("non-decimal integer notation normalized to decimal")
        }
        Format::MultilineBasic | Format::Literal | Format::MultilineLiteral => {
            Some("multiline/literal string style normalized")
        }
        Format::SingleQuoted | Format::DoubleQuoted => {
            Some("quoted scalar style normalized to plain where possible")
        }
        Format::LiteralBlock | Format::Folded => Some("block scalar (| / >) style normalized"),
        Format::Exponent => Some("exponent float notation normalized"),
        Format::Dotted => Some("dotted-key table normalized to a standard table"),
        Format::Inline if matches!(node.kind, NodeKind::InlineTable) => {
            Some("inline table / flow mapping normalized to a standard table")
        }
        _ => None,
    }
}

/// Strip comment markers (`#`, `//`, `/* */`) and one leading space per line.
fn strip_markers(text: &str) -> String {
    let t = text.trim();
    if t.starts_with("/*") {
        let inner = t.trim_start_matches("/*").trim_end_matches("*/");
        return inner.trim().to_string();
    }
    text.lines()
        .map(|l| {
            let l = l.trim_start();
            let l = l
                .strip_prefix("//")
                .or_else(|| l.strip_prefix('#'))
                .unwrap_or(l);
            l.strip_prefix(' ').unwrap_or(l).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── scalar decoders (per source format) ─────────────────────────────────────────

fn decode_scalar(src: DocFormat, node: &Node) -> Value {
    match src {
        DocFormat::Toml => decode_toml(node),
        DocFormat::Json => decode_json(node),
        DocFormat::Yaml => decode_yaml(node),
    }
}

fn decode_toml(node: &Node) -> Value {
    let raw = node.value.as_deref().unwrap_or("").trim();
    match node.kind {
        NodeKind::Scalar(ScalarType::Bool) => Value::Bool(raw == "true"),
        NodeKind::Scalar(ScalarType::Integer) => decode_int(raw, node.format),
        NodeKind::Scalar(ScalarType::Float) => decode_float(raw, node.format),
        NodeKind::Scalar(ScalarType::String) => Value::Str(decode_toml_string(raw, node.format)),
        NodeKind::Scalar(ScalarType::Null) => Value::Null,
        NodeKind::Scalar(
            ScalarType::OffsetDatetime
            | ScalarType::LocalDatetime
            | ScalarType::LocalDate
            | ScalarType::LocalTime,
        ) => Value::Datetime(raw.to_string()),
        _ => Value::Str(raw.to_string()),
    }
}

fn decode_json(node: &Node) -> Value {
    let raw = node.value.as_deref().unwrap_or("").trim();
    match node.kind {
        NodeKind::Scalar(ScalarType::Bool) => Value::Bool(raw == "true"),
        NodeKind::Scalar(ScalarType::Integer) => raw
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or(Value::Str(raw.to_string())),
        NodeKind::Scalar(ScalarType::Float) => raw
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or(Value::Str(raw.to_string())),
        NodeKind::Scalar(ScalarType::String) => Value::Str(unescape_json(strip_quotes(raw, '"'))),
        NodeKind::Scalar(ScalarType::Null) => Value::Null,
        _ => Value::Str(raw.to_string()),
    }
}

fn decode_yaml(node: &Node) -> Value {
    let raw = node.value.as_deref().unwrap_or("");
    match node.kind {
        NodeKind::Scalar(ScalarType::Null) => Value::Null,
        NodeKind::Scalar(ScalarType::Bool) => {
            Value::Bool(matches!(raw.trim(), "true" | "True" | "TRUE"))
        }
        NodeKind::Scalar(ScalarType::Integer) => decode_int(raw.trim(), node.format),
        NodeKind::Scalar(ScalarType::Float) => decode_float(raw.trim(), node.format),
        NodeKind::Scalar(ScalarType::String) => Value::Str(decode_yaml_string(raw, node.format)),
        _ => Value::Str(raw.trim().to_string()),
    }
}

/// Shared integer decode (TOML/YAML radix prefixes, `_` group separators).
fn decode_int(raw: &str, fmt: Format) -> Value {
    let neg = raw.starts_with('-');
    let body = raw.trim_start_matches(['+', '-']).replace('_', "");
    let parsed = match fmt {
        Format::Hex => i64::from_str_radix(strip_radix(&body, "0x", "0X"), 16),
        Format::Octal => i64::from_str_radix(strip_radix(&body, "0o", "0O"), 8),
        Format::Binary => i64::from_str_radix(strip_radix(&body, "0b", "0B"), 2),
        _ => body.parse::<i64>(),
    };
    match parsed {
        Ok(v) => Value::Int(if neg { -v } else { v }),
        Err(_) => Value::Str(raw.to_string()),
    }
}

fn strip_radix<'a>(s: &'a str, lo: &str, hi: &str) -> &'a str {
    s.strip_prefix(lo)
        .or_else(|| s.strip_prefix(hi))
        .unwrap_or(s)
}

/// Shared float decode (handles inf/nan formats; `_` group separators).
fn decode_float(raw: &str, fmt: Format) -> Value {
    match fmt {
        Format::Inf => Value::Float(if raw.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }),
        Format::Nan => Value::Float(f64::NAN),
        _ => {
            let body = raw.replace('_', "");
            body.parse::<f64>()
                .map(Value::Float)
                .unwrap_or(Value::Str(raw.to_string()))
        }
    }
}

fn strip_quotes(s: &str, q: char) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn decode_toml_string(raw: &str, fmt: Format) -> String {
    match fmt {
        Format::Literal => strip_quotes(raw, '\'').to_string(),
        Format::MultilineLiteral => {
            let inner = raw.trim();
            let inner = inner.strip_prefix("'''").unwrap_or(inner);
            let inner = inner.strip_suffix("'''").unwrap_or(inner);
            inner.strip_prefix('\n').unwrap_or(inner).to_string()
        }
        Format::MultilineBasic => {
            let inner = raw.trim();
            let inner = inner.strip_prefix("\"\"\"").unwrap_or(inner);
            let inner = inner.strip_suffix("\"\"\"").unwrap_or(inner);
            let inner = inner.strip_prefix('\n').unwrap_or(inner);
            unescape_basic(inner)
        }
        // BasicString and any fallback.
        _ => unescape_basic(strip_quotes(raw, '"')),
    }
}

fn decode_yaml_string(raw: &str, fmt: Format) -> String {
    match fmt {
        Format::SingleQuoted => strip_quotes(raw, '\'').replace("''", "'"),
        Format::DoubleQuoted => crate::model::yaml::edit::decode_double(strip_quotes(raw, '"')),
        Format::LiteralBlock | Format::Folded => decode_block(raw),
        _ => raw.trim().to_string(),
    }
}

/// De-indent a YAML block scalar's raw text (header line dropped, common indent
/// stripped). Mirrors `yaml::edit::decode_block_scalar` from text.
fn decode_block(raw: &str) -> String {
    let mut lines = raw.split('\n');
    let _ = lines.next(); // header (`|`, `>`, indicators)
    let mut body: Vec<&str> = lines.collect();
    while matches!(body.last(), Some(l) if l.trim().is_empty()) {
        body.pop();
    }
    if body.is_empty() {
        return String::new();
    }
    let indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    body.iter()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decode common backslash escapes for a TOML/JSON basic string body.
fn unescape_basic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => push_hex(&mut out, &mut chars, 4),
            Some('U') => push_hex(&mut out, &mut chars, 8),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// JSON string unescape (same set as basic; surrogate pairs handled by `\u`).
fn unescape_json(s: &str) -> String {
    unescape_basic(s)
}

fn push_hex(out: &mut String, chars: &mut std::str::Chars, n: usize) {
    let hex: String = chars.by_ref().take(n).collect();
    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
        out.push(c);
    } else {
        out.push('\\');
        out.push(if n == 4 { 'u' } else { 'U' });
        out.push_str(&hex);
    }
}

// ── target loss policy ───────────────────────────────────────────────────────

fn analyze(
    value: &Value,
    target: DocFormat,
    warnings: &mut Vec<String>,
) -> Result<(), ConvertAbort> {
    if target == DocFormat::Toml && value.has_null() {
        let mut paths = Vec::new();
        collect_null_paths(value, String::new(), &mut paths);
        return Err(ConvertAbort(format!(
            "cannot convert to TOML: TOML has no null; null at: {}",
            paths.join(", ")
        )));
    }
    if target != DocFormat::Toml && value.has_datetime() {
        warnings.push("TOML datetime values converted to quoted strings".into());
    }
    if target == DocFormat::Json && has_nonfinite(value) {
        warnings
            .push("non-finite floats (inf/nan) converted to strings (JSON has no inf/nan)".into());
    }
    Ok(())
}

fn collect_null_paths(value: &Value, prefix: String, out: &mut Vec<String>) {
    match value {
        Value::Null => out.push(if prefix.is_empty() {
            "<root>".into()
        } else {
            prefix
        }),
        Value::Map(items) => {
            for it in items {
                if let Item::Node { key, value, .. } = it {
                    let k = key.as_deref().unwrap_or("?");
                    let p = if prefix.is_empty() {
                        k.to_string()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    collect_null_paths(value, p, out);
                }
            }
        }
        Value::Seq(items) => {
            let mut i = 0;
            for it in items {
                if let Item::Node { value, .. } = it {
                    collect_null_paths(value, format!("{prefix}[{i}]"), out);
                    i += 1;
                }
            }
        }
        _ => {}
    }
}

fn has_nonfinite(value: &Value) -> bool {
    match value {
        Value::Float(f) => !f.is_finite(),
        Value::Map(items) | Value::Seq(items) => items.iter().any(|it| match it {
            Item::Node { value, .. } => has_nonfinite(value),
            Item::Comment(_) => false,
        }),
        _ => false,
    }
}

// ── reparse safety net ───────────────────────────────────────────────────────

fn reparse_check(text: &str, target: DocFormat) -> Result<(), ConvertAbort> {
    let suffix = match target {
        DocFormat::Toml => ".toml",
        DocFormat::Json => ".json",
        DocFormat::Yaml => ".yaml",
    };
    let f = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .map_err(|e| ConvertAbort(format!("internal: temp file: {e}")))?;
    std::fs::write(f.path(), text).map_err(|e| ConvertAbort(format!("internal: write: {e}")))?;
    AnyDocument::load_as(f.path(), target)
        .map_err(|e| ConvertAbort(format!("internal: converted output did not re-parse: {e}")))?;
    Ok(())
}

// ── renderers (default style) ──────────────────────────────────────────────────

// JSON ----------------------------------------------------------------------------

fn render_json(value: &Value) -> String {
    let mut s = String::new();
    render_json_value(value, 0, &mut s);
    s.push('\n');
    s
}

fn render_json_value(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => out.push_str(&json_float(*f)),
        Value::Str(s) => {
            out.push('"');
            out.push_str(&json_escape(s));
            out.push('"');
        }
        Value::Datetime(d) => {
            out.push('"');
            out.push_str(&json_escape(d));
            out.push('"');
        }
        Value::Map(items) | Value::Seq(items) => {
            let is_map = matches!(value, Value::Map(_));
            let (open, close) = if is_map { ('{', '}') } else { ('[', ']') };
            let total_nodes = items
                .iter()
                .filter(|i| matches!(i, Item::Node { .. }))
                .count();
            if total_nodes == 0 && items.is_empty() {
                out.push(open);
                out.push(close);
                return;
            }
            out.push(open);
            out.push('\n');
            let pad = "  ".repeat(indent + 1);
            let mut seen = 0;
            for it in items {
                match it {
                    Item::Comment(text) => {
                        for line in text.split('\n') {
                            out.push_str(&pad);
                            out.push_str("// ");
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                    Item::Node {
                        key,
                        value,
                        trailing,
                    } => {
                        out.push_str(&pad);
                        if is_map {
                            out.push('"');
                            out.push_str(&json_escape(key.as_deref().unwrap_or("")));
                            out.push_str("\": ");
                        }
                        render_json_value(value, indent + 1, out);
                        seen += 1;
                        if seen < total_nodes {
                            out.push(',');
                        }
                        if let Some(t) = trailing {
                            out.push_str(" // ");
                            out.push_str(t);
                        }
                        out.push('\n');
                    }
                }
            }
            out.push_str(&"  ".repeat(indent));
            out.push(close);
        }
    }
}

fn json_float(f: f64) -> String {
    if f.is_finite() {
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    } else if f.is_nan() {
        "\"nan\"".into()
    } else if f < 0.0 {
        "\"-inf\"".into()
    } else {
        "\"inf\"".into()
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

// YAML ----------------------------------------------------------------------------

fn render_yaml(value: &Value) -> String {
    let mut s = String::new();
    match value {
        Value::Map(items) => render_yaml_map(items, 0, &mut s),
        Value::Seq(items) => render_yaml_seq(items, 0, &mut s),
        scalar => {
            s.push_str(&yaml_scalar(scalar));
            s.push('\n');
        }
    }
    if s.is_empty() {
        s.push_str("{}\n");
    }
    s
}

fn render_yaml_map(items: &[Item], indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    for it in items {
        match it {
            Item::Comment(text) => {
                for line in text.split('\n') {
                    out.push_str(&pad);
                    out.push_str("# ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Item::Node {
                key,
                value,
                trailing,
            } => {
                let key = yaml_key(key.as_deref().unwrap_or(""));
                yaml_entry(
                    &pad,
                    &format!("{key}:"),
                    value,
                    trailing.as_deref(),
                    indent,
                    out,
                );
            }
        }
    }
}

fn render_yaml_seq(items: &[Item], indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    for it in items {
        match it {
            Item::Comment(text) => {
                for line in text.split('\n') {
                    out.push_str(&pad);
                    out.push_str("# ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Item::Node {
                value, trailing, ..
            } => match value {
                Value::Map(inner) if has_node(inner) => {
                    // Compact block: first entry on the dash line.
                    render_yaml_seq_map(inner, indent, out);
                }
                Value::Seq(inner) if has_node(inner) => {
                    out.push_str(&pad);
                    out.push_str("-\n");
                    render_yaml_seq(inner, indent + 1, out);
                }
                _ => {
                    out.push_str(&pad);
                    out.push_str("- ");
                    out.push_str(&yaml_scalar_or_empty(value));
                    if let Some(t) = trailing {
                        out.push_str("  # ");
                        out.push_str(t);
                    }
                    out.push('\n');
                }
            },
        }
    }
}

/// A map as a sequence element: first entry on the `- ` line, rest aligned under it.
fn render_yaml_seq_map(items: &[Item], indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let cont = "  ".repeat(indent + 1); // aligned past "- "
    let mut first = true;
    for it in items {
        match it {
            Item::Comment(text) => {
                for line in text.split('\n') {
                    out.push_str(&cont);
                    out.push_str("# ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Item::Node {
                key,
                value,
                trailing,
            } => {
                let key = yaml_key(key.as_deref().unwrap_or(""));
                let prefix = if first {
                    format!("{pad}- ")
                } else {
                    cont.clone()
                };
                first = false;
                yaml_entry(
                    &prefix,
                    &format!("{key}:"),
                    value,
                    trailing.as_deref(),
                    indent + 1,
                    out,
                );
            }
        }
    }
}

/// Emit one `key:`-style entry (`prefix` already holds indent + any `- `).
fn yaml_entry(
    prefix: &str,
    label: &str,
    value: &Value,
    trailing: Option<&str>,
    indent: usize,
    out: &mut String,
) {
    match value {
        Value::Map(inner) if has_node(inner) => {
            out.push_str(prefix);
            out.push_str(label);
            out.push('\n');
            render_yaml_map(inner, indent + 1, out);
        }
        Value::Seq(inner) if has_node(inner) => {
            out.push_str(prefix);
            out.push_str(label);
            out.push('\n');
            render_yaml_seq(inner, indent + 1, out);
        }
        _ => {
            out.push_str(prefix);
            out.push_str(label);
            out.push(' ');
            out.push_str(&yaml_scalar_or_empty(value));
            if let Some(t) = trailing {
                out.push_str("  # ");
                out.push_str(t);
            }
            out.push('\n');
        }
    }
}

fn yaml_scalar_or_empty(value: &Value) -> String {
    match value {
        Value::Map(_) => "{}".into(),
        Value::Seq(_) => "[]".into(),
        other => yaml_scalar(other),
    }
}

fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => yaml_float(*f),
        Value::Str(s) => yaml_string(s),
        Value::Datetime(d) => yaml_string(d),
        Value::Map(_) => "{}".into(),
        Value::Seq(_) => "[]".into(),
    }
}

fn yaml_float(f: f64) -> String {
    if f.is_nan() {
        ".nan".into()
    } else if f.is_infinite() {
        if f < 0.0 {
            "-.inf".into()
        } else {
            ".inf".into()
        }
    } else {
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

fn yaml_string(s: &str) -> String {
    if yaml_plain_safe(s) {
        s.to_string()
    } else {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }
}

fn yaml_key(s: &str) -> String {
    yaml_string(s)
}

/// Conservative plain-scalar safety: no quoting needed and re-parses as a string.
fn yaml_plain_safe(s: &str) -> bool {
    if s.is_empty() || s != s.trim() || s.contains('\n') {
        return false;
    }
    if let Some(first) = s.chars().next() {
        if matches!(
            first,
            '-' | '?'
                | ':'
                | ','
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
        ) {
            return false;
        }
    }
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') {
        return false;
    }
    // Would re-parse as a non-string scalar (number/bool/null) → must quote.
    !looks_like_nonstring(s)
}

fn looks_like_nonstring(s: &str) -> bool {
    matches!(
        s,
        "null" | "Null" | "NULL" | "~" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
    ) || s.parse::<i64>().is_ok()
        || s.parse::<f64>().is_ok()
}

fn has_node(items: &[Item]) -> bool {
    items.iter().any(|i| matches!(i, Item::Node { .. }))
}

// TOML ----------------------------------------------------------------------------

fn render_toml(value: &Value) -> Result<String, ConvertAbort> {
    let items = match value {
        Value::Map(items) => items,
        _ => {
            return Err(ConvertAbort(
                "cannot convert to TOML: the document root is not a table".into(),
            ))
        }
    };
    let mut out = String::new();
    render_toml_table(items, &[], &mut out);
    if out.is_empty() {
        // an empty document is valid TOML
        return Ok(out);
    }
    Ok(out)
}

/// Render the body of a table at `prefix`: scalar/inline entries first, then
/// `[sub]` / `[[aot]]` sections (TOML requires keys before sub-table headers).
fn render_toml_table(items: &[Item], prefix: &[String], out: &mut String) {
    // Phase A: comments + inline (scalar/array/empty) entries.
    for it in items {
        match it {
            Item::Comment(text) => {
                for line in text.split('\n') {
                    out.push_str("# ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Item::Node {
                key,
                value,
                trailing,
            } => {
                if toml_is_section(value) {
                    continue;
                }
                let k = toml_key(key.as_deref().unwrap_or(""));
                out.push_str(&k);
                out.push_str(" = ");
                out.push_str(&toml_inline_value(value));
                if let Some(t) = trailing {
                    out.push_str("  # ");
                    out.push_str(t);
                }
                out.push('\n');
            }
        }
    }
    // Phase B: sections.
    for it in items {
        let Item::Node { key, value, .. } = it else {
            continue;
        };
        if !toml_is_section(value) {
            continue;
        }
        let key = key.as_deref().unwrap_or("");
        let mut path = prefix.to_vec();
        path.push(key.to_string());
        match value {
            Value::Map(inner) => {
                out.push('\n');
                out.push('[');
                out.push_str(&toml_path(&path));
                out.push_str("]\n");
                render_toml_table(inner, &path, out);
            }
            Value::Seq(inner) => {
                for el in inner {
                    match el {
                        Item::Comment(text) => {
                            for line in text.split('\n') {
                                out.push_str("# ");
                                out.push_str(line);
                                out.push('\n');
                            }
                        }
                        Item::Node {
                            value: Value::Map(body),
                            ..
                        } => {
                            out.push('\n');
                            out.push_str("[[");
                            out.push_str(&toml_path(&path));
                            out.push_str("]]\n");
                            render_toml_table(body, &path, out);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// A value rendered as a `[table]` / `[[aot]]` section rather than inline:
/// a non-empty map, or a non-empty sequence whose every element is a map.
fn toml_is_section(value: &Value) -> bool {
    match value {
        Value::Map(items) => has_node(items),
        Value::Seq(items) => {
            let nodes: Vec<&Value> = items
                .iter()
                .filter_map(|i| match i {
                    Item::Node { value, .. } => Some(value),
                    Item::Comment(_) => None,
                })
                .collect();
            !nodes.is_empty() && nodes.iter().all(|v| matches!(v, Value::Map(_)))
        }
        _ => false,
    }
}

fn toml_inline_value(value: &Value) -> String {
    match value {
        Value::Null => "\"\"".into(), // unreachable (null→TOML aborts); defensive
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => toml_float(*f),
        Value::Str(s) => toml_string(s),
        Value::Datetime(d) => d.clone(),
        Value::Seq(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|i| match i {
                    Item::Node { value, .. } => Some(toml_inline_value(value)),
                    Item::Comment(_) => None,
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|i| match i {
                    Item::Node { key, value, .. } => Some(format!(
                        "{} = {}",
                        toml_key(key.as_deref().unwrap_or("")),
                        toml_inline_value(value)
                    )),
                    Item::Comment(_) => None,
                })
                .collect();
            if parts.is_empty() {
                "{}".into()
            } else {
                format!("{{ {} }}", parts.join(", "))
            }
        }
    }
}

fn toml_float(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f < 0.0 {
            "-inf".into()
        } else {
            "inf".into()
        }
    } else {
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A single key segment: bare if it matches `[A-Za-z0-9_-]+`, else quoted.
fn toml_key(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        s.to_string()
    } else {
        toml_string(s)
    }
}

fn toml_path(path: &[String]) -> String {
    path.iter()
        .map(|s| toml_key(s))
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;

    fn load(src: &str, fmt: DocFormat) -> AnyDocument {
        let suffix = match fmt {
            DocFormat::Toml => ".toml",
            DocFormat::Json => ".json",
            DocFormat::Yaml => ".yaml",
        };
        let f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        std::fs::write(f.path(), src).unwrap();
        AnyDocument::load_as(f.path(), fmt).unwrap()
    }

    fn convert_str(src: &str, from: DocFormat, to: DocFormat) -> ConvertResult {
        convert(&load(src, from), to).unwrap()
    }

    #[test]
    fn toml_to_json_basic() {
        let r = convert_str(
            "a = 1\nb = \"x\"\nc = true\n",
            DocFormat::Toml,
            DocFormat::Json,
        );
        assert_eq!(
            r.text,
            "{\n  \"a\": 1,\n  \"b\": \"x\",\n  \"c\": true\n}\n"
        );
    }

    #[test]
    fn toml_to_yaml_nested_table() {
        let r = convert_str(
            "[server]\nhost = \"localhost\"\nport = 8080\n",
            DocFormat::Toml,
            DocFormat::Yaml,
        );
        assert_eq!(r.text, "server:\n  host: localhost\n  port: 8080\n");
    }

    #[test]
    fn json_to_toml_scope_tables() {
        let r = convert_str(
            "{ \"x\": 1, \"o\": { \"y\": 2 } }\n",
            DocFormat::Json,
            DocFormat::Toml,
        );
        assert_eq!(r.text, "x = 1\n\n[o]\ny = 2\n");
    }

    #[test]
    fn json_array_of_objects_to_toml_aot() {
        let r = convert_str(
            "{ \"items\": [ { \"n\": 1 }, { \"n\": 2 } ] }\n",
            DocFormat::Json,
            DocFormat::Toml,
        );
        assert_eq!(r.text, "\n[[items]]\nn = 1\n\n[[items]]\nn = 2\n");
    }

    #[test]
    fn comments_carry_toml_to_json() {
        let r = convert_str("# hi\na = 1\n", DocFormat::Toml, DocFormat::Json);
        assert_eq!(r.text, "{\n  // hi\n  \"a\": 1\n}\n");
    }

    #[test]
    fn trailing_comment_carries_to_yaml() {
        let r = convert_str("a = 1 # note\n", DocFormat::Toml, DocFormat::Yaml);
        assert_eq!(r.text, "a: 1  # note\n");
    }

    #[test]
    fn null_to_toml_aborts_with_path() {
        let err = convert(
            &load("{ \"a\": { \"b\": null } }\n", DocFormat::Json),
            DocFormat::Toml,
        )
        .unwrap_err();
        assert!(err.0.contains("null"));
        assert!(err.0.contains("a.b"));
    }

    #[test]
    fn null_survives_json_to_yaml() {
        let r = convert_str("{ \"a\": null }\n", DocFormat::Json, DocFormat::Yaml);
        assert_eq!(r.text, "a: null\n");
    }

    #[test]
    fn datetime_to_json_warns_and_stringifies() {
        let r = convert_str(
            "when = 2021-01-01T00:00:00Z\n",
            DocFormat::Toml,
            DocFormat::Json,
        );
        assert_eq!(r.text, "{\n  \"when\": \"2021-01-01T00:00:00Z\"\n}\n");
        assert!(r.warnings.iter().any(|w| w.contains("datetime")));
    }

    #[test]
    fn radix_int_normalized_and_warned() {
        let r = convert_str("n = 0xFF\n", DocFormat::Toml, DocFormat::Json);
        assert_eq!(r.text, "{\n  \"n\": 255\n}\n");
        assert!(r.warnings.iter().any(|w| w.contains("non-decimal")));
    }

    #[test]
    fn yaml_opaque_aborts() {
        let err = convert(
            &load("a: &anchor 1\nb: *anchor\n", DocFormat::Yaml),
            DocFormat::Json,
        )
        .unwrap_err();
        assert!(err.0.contains("unsupported YAML"));
    }

    #[test]
    fn json_to_yaml_array_of_objects() {
        let r = convert_str(
            "{ \"xs\": [ { \"a\": 1, \"b\": 2 } ] }\n",
            DocFormat::Json,
            DocFormat::Yaml,
        );
        assert_eq!(r.text, "xs:\n  - a: 1\n    b: 2\n");
    }

    #[test]
    fn roundtrip_value_equality_json_yaml() {
        // JSON → YAML → JSON preserves the decoded Value tree.
        let src = "{ \"a\": 1, \"b\": [true, \"hi\"], \"c\": { \"d\": 2.5 } }\n";
        let (v0, _) = load(src, DocFormat::Json).to_value().unwrap();
        let yaml = convert_str(src, DocFormat::Json, DocFormat::Yaml).text;
        let (v1, _) = load(&yaml, DocFormat::Yaml).to_value().unwrap();
        assert_eq!(v0, v1);
    }

    #[test]
    fn yaml_string_that_looks_numeric_is_quoted() {
        let r = convert_str("{ \"v\": \"123\" }\n", DocFormat::Json, DocFormat::Yaml);
        assert_eq!(r.text, "v: \"123\"\n");
    }
}
