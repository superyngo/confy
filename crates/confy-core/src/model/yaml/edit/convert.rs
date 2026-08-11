//! `Mutation::ConvertKind` for YAML: container flow/block toggling and
//! scalar notation conversion — split out of `yaml/edit.rs` (Task 15,
//! 2026-08-11 audit remediation).

use crate::model::document::MutateError;
use crate::model::node::{NodeKind, ScalarType, Seg};
use crate::model::yaml::project::{Target, YamlIndex};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};
use super::block::{commit_reparse, entry_has_opaque_value, entry_indent_depth, entry_key_text};
use super::flow::{node_in_flow};
use super::resolve::{resolve_in};

pub(crate) fn convert_kind(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    match target {
        KT::Flow | KT::Block => convert_container(tree, idx, path, target),
        KT::StringPlain
        | KT::StringSingle
        | KT::StringDouble
        | KT::StringLiteralBlock
        | KT::StringFolded => convert_string(tree, idx, path, target),
        KT::IntDecimal | KT::IntHex | KT::IntOctal => convert_int(tree, idx, path, target),
        KT::FloatPlain | KT::FloatExponent => convert_float(tree, idx, path, target),
        _ => Err(MutateError::Unsupported),
    }
}

/// Resolve `path` to the MAP_ENTRY / SEQ_ENTRY node carrying the value, plus the
/// value-content child node (VALUE / MAPPING / SEQUENCE / FLOW_MAP / FLOW_SEQ /
/// BLOCK_SCALAR). Rejects opaque-valued entries (read-only).
/// Find the first PLAIN scalar token under `value` — the numeric leaf of an
/// int/float entry. `Unsupported` if there is none.
pub(crate) fn first_plain_token(
    value: &SyntaxNode,
) -> Result<crate::model::yaml::syntax::SyntaxToken, MutateError> {
    value
        .descendants_with_tokens()
        .find_map(|el| match el {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::PLAIN => Some(t),
            _ => None,
        })
        .ok_or(MutateError::Unsupported)
}

pub(crate) fn resolve_value_node(
    idx: &YamlIndex,
    path: &[Seg],
) -> Result<(SyntaxNode, SyntaxNode), MutateError> {
    let entry = match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(e) | Target::Element(e) => e,
        _ => return Err(MutateError::Unsupported),
    };
    if entry_has_opaque_value(&entry) {
        return Err(MutateError::Unsupported);
    }
    let value = entry
        .children()
        .find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::MAPPING
                    | SyntaxKind::SEQUENCE
                    | SyntaxKind::FLOW_MAP
                    | SyntaxKind::FLOW_SEQ
                    | SyntaxKind::VALUE
                    | SyntaxKind::BLOCK_SCALAR
            )
        })
        .ok_or(MutateError::NotFound)?;
    Ok((entry, value))
}

/// Splice `new_value_text` over the value node's text range in the whole document
/// and reparse atomically (same strategy as `rebuild_and_splice`).
pub(crate) fn splice_value_text(
    tree: &SyntaxNode,
    value: &SyntaxNode,
    new_value_text: &str,
) -> Result<(), MutateError> {
    let full = tree.to_string();
    let start: usize = value.text_range().start().into();
    let end: usize = value.text_range().end().into();
    // Preserve the original value's trailing-newline state: callers build the
    // new value text with a single trailing `\n`, but the replaced range may or
    // may not already include the newline.
    let old = &full[start..end];
    let trailing = if old.ends_with('\n') { "\n" } else { "" };
    let new_value_text = format!("{}{}", new_value_text.trim_end_matches('\n'), trailing);
    let new_doc = format!("{}{}{}", &full[..start], new_value_text, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

pub(crate) fn convert_container(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let (entry, value) = resolve_value_node(idx, path)?;

    // A member sitting *inside* an inline flow collection can't be block-expanded
    // (it would break the one line); reject. The flow collection as a whole is
    // converted via its own block-level entry, which is not in-flow.
    if node_in_flow(&entry) {
        return Err(MutateError::Unsupported);
    }

    // Locate the actual collection node (FLOW_MAP/FLOW_SEQ/MAPPING/SEQUENCE):
    // it may be the value itself or a VALUE-wrapped flow collection.
    let coll = if matches!(
        value.kind(),
        SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ
    ) {
        value.clone()
    } else {
        value
            .children()
            .find(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::FLOW_MAP
                        | SyntaxKind::FLOW_SEQ
                        | SyntaxKind::MAPPING
                        | SyntaxKind::SEQUENCE
                )
            })
            .ok_or(MutateError::Unsupported)?
    };

    let is_flow = matches!(coll.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ);
    let is_map = matches!(coll.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::MAPPING);

    match target {
        KT::Flow if is_flow => return Err(MutateError::Unsupported),
        KT::Block if !is_flow => return Err(MutateError::Unsupported),
        _ => {}
    }

    // Build the new entry text and splice over the whole entry — block ↔ flow
    // changes the value's *line layout*, so a value-only splice would leave a
    // dangling `key:` line or stray space.
    let indent = entry_indent_depth(&entry);
    let is_map_entry = entry.kind() == SyntaxKind::MAP_ENTRY;
    let key_prefix = if is_map_entry {
        format!("{}{}", " ".repeat(indent), entry_key_text(&entry))
    } else {
        // Seq element: the `- ` is the prefix; the collection is its value.
        format!("{}-", " ".repeat(indent))
    };

    let new_entry_text = if target == KT::Flow {
        // Block → flow: reject comments / block scalars / multi-line members.
        // Scan the whole entry — a standalone comment in the block body attaches
        // to the outer entry, not the inner collection node. Checked (and
        // reported) separately since they're distinct causes.
        if entry
            .descendants_with_tokens()
            .any(|el| el.kind() == SyntaxKind::COMMENT)
        {
            return Err(MutateError::Illegal(
                "cannot collapse container with comments to flow".into(),
            ));
        }
        if entry
            .descendants_with_tokens()
            .any(|el| el.kind() == SyntaxKind::BLOCK_SCALAR)
        {
            return Err(MutateError::Illegal(
                "cannot collapse container to flow: a literal (|) or folded (>) block scalar can't be written on one line".into(),
            ));
        }
        let members = flow_members_from_block(&coll, is_map)?;
        let inner = members.join(", ");
        let flow = if is_map {
            format!("{{{inner}}}")
        } else {
            format!("[{inner}]")
        };
        if is_map_entry {
            format!("{key_prefix}: {flow}\n")
        } else {
            format!("{key_prefix} {flow}\n")
        }
    } else {
        // Flow → block.
        let child_indent = indent + 2;
        let members = block_members_from_flow(&coll, is_map)?;
        if members.is_empty() {
            return Err(MutateError::Illegal(
                "cannot expand empty flow collection".into(),
            ));
        }
        let mut body = String::new();
        for m in &members {
            let line = if is_map {
                format!("{}{}\n", " ".repeat(child_indent), m)
            } else {
                format!("{}- {}\n", " ".repeat(child_indent), m)
            };
            body.push_str(&line);
        }
        if is_map_entry {
            format!("{key_prefix}:\n{body}")
        } else if is_map {
            // Seq element holding a map: compact `- key: v` form — the first
            // member rides the dash line, the rest align under it. (An empty
            // dash line then an indented map parses fine but reads as a stray
            // blank line, so emit the canonical compact block mapping.)
            let mut s = format!("{key_prefix} {}\n", members[0]);
            for m in &members[1..] {
                s.push_str(&format!("{}{}\n", " ".repeat(child_indent), m));
            }
            s
        } else {
            // Seq element holding a nested sequence: `-\n  - ...`.
            format!("{key_prefix}\n{body}")
        }
    };

    splice_entry_text(tree, &entry, &new_entry_text)
}

/// Splice `new_entry_text` over the entry node's full text range and reparse.
pub(crate) fn splice_entry_text(
    tree: &SyntaxNode,
    entry: &SyntaxNode,
    new_entry_text: &str,
) -> Result<(), MutateError> {
    let full = tree.to_string();
    let start: usize = entry.text_range().start().into();
    let end: usize = entry.text_range().end().into();
    let new_doc = format!("{}{}{}", &full[..start], new_entry_text, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Build single-line flow members from a block collection's entries.
/// Map members are `key: value`; sequence members are bare `value`. Rejects a
/// member whose own text spans multiple lines.
pub(crate) fn flow_members_from_block(coll: &SyntaxNode, is_map: bool) -> Result<Vec<String>, MutateError> {
    let entry_kind = if is_map {
        SyntaxKind::MAP_ENTRY
    } else {
        SyntaxKind::SEQ_ENTRY
    };
    let mut out = Vec::new();
    for entry in coll.children().filter(|n| n.kind() == entry_kind) {
        let text = entry.text().to_string();
        let trimmed = text.trim();
        if trimmed.contains('\n') {
            return Err(MutateError::Illegal(
                "cannot collapse container with multi-line members to flow".into(),
            ));
        }
        // A bare plain-style value that's unsafe inside `{…}`/`[…]` (contains a
        // flow indicator, e.g. a comma) would silently truncate on reparse and
        // spawn a bogus sibling key — YAML flow context forbids an unescaped
        // `,{}[]` in a plain scalar even though block context allows it freely.
        // Reject the whole conversion rather than silently reformatting the
        // member to a quoted style behind the user's back; they can quote it
        // themselves first (`K` on that value → single/double) and retry.
        if let Some(content) = entry_plain_value(&entry) {
            if !flow_plain_safe(&content) {
                return Err(MutateError::Illegal(if is_map {
                    format!(
                        "cannot collapse container to flow: \"{}\" is a plain string containing a flow-unsafe character (, {{ }} [ ]) — quote it first",
                        entry_key_text(&entry)
                    )
                } else {
                    format!(
                        "cannot collapse container to flow: plain element {content:?} contains a flow-unsafe character (, {{ }} [ ]) — quote it first"
                    )
                }));
            }
        }
        if is_map {
            out.push(trimmed.to_string());
        } else {
            // Strip the leading `- ` of the sequence element.
            let v = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            out.push(v.to_string());
        }
    }
    Ok(out)
}

/// Build block members (each a single-line `key: value` or bare value) from a
/// flow collection by reusing the projection, which already parses flow members.
pub(crate) fn block_members_from_flow(coll: &SyntaxNode, is_map: bool) -> Result<Vec<String>, MutateError> {
    // Re-derive the members from the flow source between the braces/brackets.
    let src = coll.text().to_string();
    let inner = src.trim();
    let inner = inner
        .strip_prefix('{')
        .or_else(|| inner.strip_prefix('['))
        .unwrap_or(inner);
    let inner = inner
        .strip_suffix('}')
        .or_else(|| inner.strip_suffix(']'))
        .unwrap_or(inner);
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    // Split on **top-level** commas (depth-aware): a nested `{…}`/`[…]` element
    // (an `[A/F]`/`[T/F]` inside this flow collection) is kept verbatim as one
    // member, so block-expanding the outer container leaves the inner flow form
    // intact — the symmetric inverse of `[A/B]`-nested-`[T/F]` → `[A/F]`.
    let members: Vec<String> = split_top_level_commas(inner)
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    // Sanity: map members must look keyed; seq members must not.
    if is_map
        && members
            .iter()
            .any(|m| !m.contains(": ") && !m.ends_with(':'))
    {
        return Err(MutateError::Unsupported);
    }
    Ok(members)
}

/// Split a flow collection's inner text on commas that sit at brace/bracket depth
/// 0 and outside quotes — so a nested `{…}`/`[…]` element (whose own commas are
/// nested) stays a single member.
pub(crate) fn split_top_level_commas(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut start = 0usize;
    for (i, c) in inner.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '{' | '[' if !in_single && !in_double => depth += 1,
            '}' | ']' if !in_single && !in_double => depth -= 1,
            ',' if depth == 0 && !in_single && !in_double => {
                out.push(inner[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].to_string());
    out
}

pub(crate) fn convert_string(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let (entry, value) = resolve_value_node(idx, path)?;
    let indent = entry_indent_depth(&entry);

    // A literal/folded block scalar is multi-line — illegal for a member inside an
    // inline flow collection.
    if node_in_flow(&entry) && matches!(target, KT::StringLiteralBlock | KT::StringFolded) {
        return Err(MutateError::Illegal(
            "cannot use a block scalar inside an inline flow collection".into(),
        ));
    }

    // Decode the current scalar content into a plain Rust string.
    let content = decode_string_value(&value)?;

    let new_value_text = match target {
        KT::StringPlain => {
            // Inside a flow collection, a flow indicator character (`,{}[]`)
            // can't survive unquoted anywhere in the scalar — stricter than
            // block context, which only forbids one when leading.
            let safe = if node_in_flow(&entry) {
                flow_plain_safe(&content)
            } else {
                plain_safe(&content)
            };
            if !safe {
                return Err(MutateError::Illegal(
                    "content cannot be represented as a plain scalar".into(),
                ));
            }
            format!("{content}\n")
        }
        KT::StringSingle => {
            if content.contains('\n') {
                return Err(MutateError::Illegal(
                    "content with newlines cannot be single-quoted".into(),
                ));
            }
            format!("'{}'\n", content.replace('\'', "''"))
        }
        KT::StringDouble => {
            format!("\"{}\"\n", encode_double(&content))
        }
        KT::StringLiteralBlock => encode_block(&content, indent, '|'),
        KT::StringFolded => encode_block(&content, indent, '>'),
        _ => unreachable!(),
    };

    splice_value_text(tree, &value, &new_value_text)
}

/// Decode the current scalar / block-scalar value node into its plain content.
pub(crate) fn decode_string_value(value: &SyntaxNode) -> Result<String, MutateError> {
    // A VALUE may wrap a SCALAR (PLAIN/SINGLE/DOUBLE) or BLOCK_SCALAR.
    if value.kind() == SyntaxKind::BLOCK_SCALAR {
        return decode_block_scalar(value);
    }
    if let Some(bs) = value
        .children()
        .find(|c| c.kind() == SyntaxKind::BLOCK_SCALAR)
    {
        return decode_block_scalar(&bs);
    }
    // Find the scalar token.
    let tok = value
        .descendants_with_tokens()
        .find_map(|el| match el {
            rowan::NodeOrToken::Token(t)
                if matches!(
                    t.kind(),
                    SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                ) =>
            {
                Some(t)
            }
            _ => None,
        })
        .ok_or(MutateError::Unsupported)?;
    let text = tok.text();
    Ok(match tok.kind() {
        SyntaxKind::SINGLE => {
            let inner = text
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(text);
            inner.replace("''", "'")
        }
        SyntaxKind::DOUBLE => {
            let inner = text
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(text);
            decode_double(inner)
        }
        _ => text.to_string(),
    })
}

/// Decode a BLOCK_SCALAR (`|` literal / `>` folded) into its content.
/// Pragmatic: strips the header line and de-indents the body by its common
/// indent. Folded-vs-literal line-folding is not round-tripped — the raw body
/// lines are joined with `\n` (literal semantics), which is exact for a
/// single-line body and a faithful superset for multi-line.
pub(crate) fn decode_block_scalar(bs: &SyntaxNode) -> Result<String, MutateError> {
    let full = bs.text().to_string();
    let mut lines = full.split('\n');
    // First line is the header (`|`, `>`, with optional indicators).
    let _ = lines.next();
    let body: Vec<&str> = lines.collect();
    // Trim a trailing empty line produced by the final newline.
    let body: Vec<&str> = {
        let mut b = body;
        while matches!(b.last(), Some(l) if l.trim().is_empty()) {
            b.pop();
        }
        b
    };
    if body.is_empty() {
        return Ok(String::new());
    }
    // Common indent = min leading-space of non-blank lines.
    let indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let content = body
        .iter()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(content)
}

/// Encode `content` as a literal (`|`) or folded (`>`) block scalar value text,
/// indented to `indent + 2`. Always uses the clip chomping default.
pub(crate) fn encode_block(content: &str, indent: usize, marker: char) -> String {
    let body_indent = " ".repeat(indent + 2);
    let mut out = format!("{marker}\n");
    for line in content.split('\n') {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&body_indent);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// A string is plain-safe if it needs no quoting: non-empty, no leading YAML
/// indicator, no `: ` / ` #`, no leading/trailing whitespace, no newline.
pub(crate) fn plain_safe(s: &str) -> bool {
    if s.is_empty() || s.contains('\n') {
        return false;
    }
    if s != s.trim() {
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
    // A bare value that re-parses as a non-string type would change the type;
    // reject so the conversion stays string→string. Re-project the candidate
    // and require it to classify as a String. Fail closed: if the candidate
    // doesn't even parse, it is not plain-safe.
    let Ok(green) = crate::model::yaml::parse::parse(&format!("__k__: {s}\n")) else {
        return false;
    };
    matches!(
        crate::model::yaml::project::project(&SyntaxNode::new_root(green), "")
            .root
            .children
            .first()
            .map(|n| &n.kind),
        Some(NodeKind::Scalar(ScalarType::String))
    )
}

/// Whether `s` is safe as an unquoted (plain) *string* scalar **inside a flow
/// collection** (`{…}`/`[…]`) — `plain_safe` (block context) plus: a flow
/// indicator character (`,{}[]`) ends a plain scalar wherever it appears, not
/// just when leading.
pub(crate) fn flow_plain_safe(s: &str) -> bool {
    plain_safe(s) && !s.contains([',', '{', '}', '[', ']'])
}

/// If `entry`'s value is a bare single-line PLAIN scalar that the core schema
/// classifies as a **string** (not already quoted, not a nested
/// collection/block scalar, and not a plain int/float/bool/null — those are
/// always flow-safe as-is and must not be turned into strings by quoting),
/// return its decoded content. Used by `flow_members_from_block` to detect a
/// block-plain string value that needs quoting once it's collapsed into a
/// flow collection.
pub(crate) fn entry_plain_value(entry: &SyntaxNode) -> Option<String> {
    let value = entry.children().find(|c| c.kind() == SyntaxKind::VALUE)?;
    if value.children().any(|c| {
        matches!(
            c.kind(),
            SyntaxKind::MAPPING
                | SyntaxKind::SEQUENCE
                | SyntaxKind::FLOW_MAP
                | SyntaxKind::FLOW_SEQ
                | SyntaxKind::BLOCK_SCALAR
        )
    }) {
        return None;
    }
    let tok = value.descendants_with_tokens().find_map(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::PLAIN => Some(t),
        _ => None,
    })?;
    let (kind, _) = crate::model::yaml::project::classify_plain_scalar(tok.text().trim());
    if !matches!(kind, NodeKind::Scalar(ScalarType::String)) {
        return None; // int/float/bool/null plain scalar — always flow-safe verbatim
    }
    decode_string_value(&value).ok()
}

/// Encode `content` for a double-quoted scalar (escape `"` and `\`, newlines→`\n`).
pub(crate) fn encode_double(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for c in content.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Decode a double-quoted scalar's inner content (handle common escapes).
pub(crate) fn decode_double(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn convert_int(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let (_, value) = resolve_value_node(idx, path)?;
    let tok = first_plain_token(&value)?;
    let text = tok.text().trim().to_string();

    // Parse the integer, honoring sign / radix prefix / `_` separators.
    let (neg, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(&text)),
    };
    let body_clean = body.replace('_', "");
    let magnitude: i128 = if let Some(h) = body_clean
        .strip_prefix("0x")
        .or_else(|| body_clean.strip_prefix("0X"))
    {
        i128::from_str_radix(h, 16).map_err(|_| MutateError::Illegal("not an integer".into()))?
    } else if let Some(o) = body_clean
        .strip_prefix("0o")
        .or_else(|| body_clean.strip_prefix("0O"))
    {
        i128::from_str_radix(o, 8).map_err(|_| MutateError::Illegal("not an integer".into()))?
    } else {
        body_clean
            .parse()
            .map_err(|_| MutateError::Illegal("not an integer".into()))?
    };

    if neg && matches!(target, KT::IntHex | KT::IntOctal) {
        return Err(MutateError::Illegal(
            "negative integers have no hex/octal form".into(),
        ));
    }

    let rendered = match target {
        KT::IntDecimal => format!("{magnitude}"),
        KT::IntHex => format!("0x{magnitude:x}"),
        KT::IntOctal => format!("0o{magnitude:o}"),
        _ => unreachable!(),
    };
    let sign = if neg { "-" } else { "" };
    splice_value_text(tree, &value, &format!("{sign}{rendered}\n"))
}

pub(crate) fn convert_float(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let (_, value) = resolve_value_node(idx, path)?;
    let tok = first_plain_token(&value)?;
    let parsed: f64 = tok
        .text()
        .trim()
        .parse()
        .map_err(|_| MutateError::Illegal("not a float".into()))?;
    let rendered = match target {
        KT::FloatExponent => format!("{parsed:e}"),
        KT::FloatPlain => {
            // Rust's Display drops a whole float's `.0` (`1500.0` → "1500"),
            // which YAML's core schema would re-read as an Integer — a type
            // change in a float→float convert. Force a float-recognizable form.
            let s = format!("{parsed}");
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        _ => unreachable!(),
    };
    splice_value_text(tree, &value, &format!("{rendered}\n"))
}
