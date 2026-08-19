//! `Mutation::ConvertKind` (the `K` kind/notation-switch family) for scalars,
//! arrays, and tables — split out of `cst_edit.rs` (Task 15, 2026-08-11 audit
//! remediation).

use super::aot_group::aot_group_span;
use super::dotted_table::{
    dotted_member_entries, is_headerless_table, replace_dotted_table, strip_key_prefix,
};
use super::escape::{encode_basic_string, encode_multiline_basic, string_inner, unescape_basic};
use super::rename::is_key_seg;
use super::replace_delete::MemberSpan;
use super::replace_delete::{
    array_make_multiline, entry_array, entry_key_seg_count, path_key_display, replace_value,
    section_end, table_member_spans,
};
use super::tree_nav::{extend_over_newline, is_scalar_kind, node_at};
use crate::model::cst_project::{header_path, walk, CstIndex, Target};
use crate::model::document::MutateError;
use crate::model::node::{Node, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// `Mutation::ConvertKind` — rewrite the node at `path` in another kind/notation,
/// in place. Scalars re-render their literal (lossless conversions only — a
/// non-integral float to integer, a non-`true`/`false` string to bool, … reject
/// as `Illegal`); arrays toggle inline ↔ multiline; tables convert between
/// `[T/I]`, `[T/D]` and `[T/S]` writing styles, with `[T/S]` conversions checked
/// against the table-capture rule (D5) and inline targets rejecting comments
/// (a `{ … }` holds none).
pub(crate) fn convert_kind(
    tree: &SyntaxNode,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let (proj, idx) = walk(tree, "");
    let node = node_at(&proj.root, path).ok_or(MutateError::NotFound)?;
    match target {
        KT::StringBasic
        | KT::StringLiteral
        | KT::StringMultiline
        | KT::StringMultilineLiteral
        | KT::IntDecimal
        | KT::IntHex
        | KT::IntOctal
        | KT::IntBinary
        | KT::FloatPlain
        | KT::FloatExponent => convert_scalar(tree, &idx, path, target),
        KT::ArrayInline | KT::ArrayMultiline
            if matches!(node.kind, crate::model::node::NodeKind::ArrayOfTables) =>
        {
            convert_aot_to_array(tree, path, matches!(target, KT::ArrayMultiline))
        }
        KT::ArrayInline | KT::ArrayMultiline => convert_array(tree, &idx, path, target),
        KT::ArrayOfTables => convert_array_to_aot(tree, &idx, path),
        KT::TableInline | KT::TableDotted | KT::TableScope => {
            convert_table(tree, &proj.root, &idx, path, target)
        }
        KT::TableMultiline => Err(MutateError::Unsupported),
        // YAML-only targets — not reachable from the TOML backend.
        _ => Err(MutateError::Unsupported),
    }
}

/// The scalar token of the VALUE backing `path` (a keyed entry or array element).
pub(crate) fn scalar_token_at(idx: &CstIndex, path: &[Seg]) -> Result<SyntaxToken, MutateError> {
    let value = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t) {
        Some(Target::Entry(e)) => e
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::Unsupported)?,
        Some(Target::ArrayElement(v)) => v.clone(),
        _ => return Err(MutateError::Unsupported),
    };
    value
        .children_with_tokens()
        .find_map(|c| c.into_token().filter(|t| is_scalar_kind(t.kind())))
        .ok_or(MutateError::Unsupported)
}

/// `K` on a scalar: re-render its value in another **notation of the same
/// type** — string basic/literal/multiline forms, integer radix, float plain ↔
/// exponent. A value the target notation can't represent (a `'` in a literal
/// form, a real newline in a single-line literal, a negative integer in a
/// prefixed radix) rejects as `Illegal`; bools and datetimes have one notation.
pub(crate) fn convert_scalar(
    tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    use SyntaxKind as K;
    let tok = scalar_token_at(idx, path)?;
    let raw = tok.text().to_string();
    let lit = match target {
        KT::StringBasic | KT::StringLiteral | KT::StringMultiline | KT::StringMultilineLiteral => {
            let content = match tok.kind() {
                K::STRING => unescape_basic(&string_inner(&raw, 1), false)?,
                K::MULTI_LINE_STRING => unescape_basic(&string_inner(&raw, 3), true)?,
                K::STRING_LITERAL => string_inner(&raw, 1),
                K::MULTI_LINE_STRING_LITERAL => string_inner(&raw, 3),
                _ => {
                    return Err(MutateError::Illegal(
                        "only a string converts between string notations".into(),
                    ));
                }
            };
            match target {
                KT::StringBasic => encode_basic_string(&content),
                KT::StringMultiline => encode_multiline_basic(&content),
                KT::StringLiteral => {
                    if content.contains('\'') {
                        return Err(MutateError::Illegal(
                            "the value holds a `'` — a literal string can't".into(),
                        ));
                    }
                    if content.contains('\n') || content.contains('\r') {
                        return Err(MutateError::Illegal(
                            "a multi-line value cannot live in a single-line literal".into(),
                        ));
                    }
                    format!("'{content}'")
                }
                KT::StringMultilineLiteral => {
                    if content.contains("'''") {
                        return Err(MutateError::Illegal(
                            "the value holds `'''` — a multiline literal can't".into(),
                        ));
                    }
                    let lead = if content.starts_with('\n') || content.starts_with("\r\n") {
                        "\n"
                    } else {
                        ""
                    };
                    format!("'''{lead}{content}'''")
                }
                _ => unreachable!(),
            }
        }
        KT::IntDecimal | KT::IntHex | KT::IntOctal | KT::IntBinary => {
            if !matches!(
                tok.kind(),
                K::INTEGER | K::INTEGER_HEX | K::INTEGER_OCT | K::INTEGER_BIN
            ) {
                return Err(MutateError::Illegal(
                    "only an integer converts between radices".into(),
                ));
            }
            let cleaned = raw.replace('_', "");
            let (neg, body) = match cleaned.strip_prefix('-') {
                Some(b) => (true, b),
                None => (false, cleaned.strip_prefix('+').unwrap_or(&cleaned)),
            };
            let v = if let Some(h) = body.strip_prefix("0x") {
                i64::from_str_radix(h, 16)
            } else if let Some(o) = body.strip_prefix("0o") {
                i64::from_str_radix(o, 8)
            } else if let Some(b) = body.strip_prefix("0b") {
                i64::from_str_radix(b, 2)
            } else {
                body.parse()
            }
            .map_err(|_| MutateError::Illegal(format!("cannot parse `{raw}` as an integer")))?;
            let v: i64 = if neg { -v } else { v };
            match target {
                KT::IntDecimal => v.to_string(),
                _ if v < 0 => {
                    return Err(MutateError::Illegal(
                        "a negative integer has no hex/octal/binary form".into(),
                    ));
                }
                KT::IntHex => format!("0x{v:x}"),
                KT::IntOctal => format!("0o{v:o}"),
                KT::IntBinary => format!("0b{v:b}"),
                _ => unreachable!(),
            }
        }
        KT::FloatPlain | KT::FloatExponent => {
            if tok.kind() != K::FLOAT {
                return Err(MutateError::Illegal(
                    "only a float converts between notations".into(),
                ));
            }
            let f: f64 = raw
                .replace('_', "")
                .parse()
                .map_err(|_| MutateError::Illegal(format!("cannot parse `{raw}` as a float")))?;
            if !f.is_finite() {
                return Err(MutateError::Illegal(
                    "inf/nan have a single notation".into(),
                ));
            }
            match target {
                KT::FloatExponent => format!("{f:e}"),
                KT::FloatPlain => {
                    let mut s = format!("{f}");
                    if !s.contains('.') {
                        s.push_str(".0");
                    }
                    s
                }
                _ => unreachable!(),
            }
        }
        _ => return Err(MutateError::Unsupported),
    };
    let built = format!("__k__ = {lit}\n");
    let parse = taplo::parser::parse(&built);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    replace_value(tree, path, &built)
}

pub(crate) fn convert_array(
    tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;
    let arr = entry_array(idx, path)?;
    let is_multiline = arr.to_string().contains('\n');
    match target {
        KT::ArrayMultiline => {
            if is_multiline {
                return Err(MutateError::Illegal("array is already multiline".into()));
            }
            array_make_multiline(&arr)
        }
        KT::ArrayInline => {
            if !is_multiline {
                return Err(MutateError::Illegal("array is already inline".into()));
            }
            // Comments can't survive a single line; nested multi-line elements
            // can't either.
            if arr
                .descendants_with_tokens()
                .any(|c| matches!(&c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT))
            {
                return Err(MutateError::Illegal(
                    "the array holds comments — remove them first".into(),
                ));
            }
            let elems: Vec<String> = arr
                .children()
                .filter(|c| c.kind() == SyntaxKind::VALUE)
                .map(|v| v.to_string().trim().to_string())
                .collect();
            if elems.iter().any(|e| e.contains('\n')) {
                return Err(MutateError::Illegal(
                    "a multi-line element cannot be collapsed".into(),
                ));
            }
            replace_value(tree, path, &format!("__k__ = [{}]\n", elems.join(", ")))
        }
        _ => unreachable!(),
    }
}

/// `K` on an `[A/T]` group: rewrite the whole group as a keyed array of inline
/// tables (`key = [{ … }, …]`, inline or multiline) — the two container kinds
/// are equivalent. Requires a contiguous group span whose entries hold only
/// plain single-line `ENTRY` lines (no sub-sections) and no comments. The
/// replacement entry lands at the first `[[header]]`'s slot, legal only when
/// the nearest preceding header is the parent scope's own `[table]` (or none,
/// at root) — the same capture rule as the `[T/S]` conversions.
pub(crate) fn convert_aot_to_array(
    tree: &SyntaxNode,
    path: &[Seg],
    multiline: bool,
) -> Result<(), MutateError> {
    let (start, end) = aot_group_span(tree, path).ok_or(MutateError::Unsupported)?;
    // A sub-section anywhere under the group belongs to one of its entries and
    // has no place in an inline-table element.
    if tree.children().any(|n| {
        n.kind() == SyntaxKind::TABLE_HEADER && {
            let p = header_path(&n);
            p.len() > path.len() && p.starts_with(path)
        }
    }) {
        return Err(MutateError::Illegal(
            "an entry holds a sub-section — flatten it first".into(),
        ));
    }
    let els: Vec<_> = tree.children_with_tokens().collect();
    // Gather each entry's member texts, rejecting content an inline table
    // can't keep.
    let mut entries: Vec<Vec<String>> = Vec::new();
    for el in &els[start..end] {
        match el {
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::TABLE_ARRAY_HEADER => entries.push(Vec::new()),
                SyntaxKind::ENTRY => {
                    if n.descendants_with_tokens().any(
                        |c| matches!(&c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT),
                    ) {
                        return Err(MutateError::Illegal(
                            "the group holds comments — remove them first".into(),
                        ));
                    }
                    let t = n.to_string().trim().to_string();
                    if t.contains('\n') {
                        return Err(MutateError::Illegal(
                            "a multi-line member cannot live in an inline table".into(),
                        ));
                    }
                    entries.last_mut().ok_or(MutateError::Unsupported)?.push(t);
                }
                _ => {}
            },
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT => {
                return Err(MutateError::Illegal(
                    "the group holds comments — remove them first".into(),
                ));
            }
            _ => {}
        }
    }
    let preceding = els[..start].iter().rev().find_map(|el| match el {
        NodeOrToken::Node(n)
            if matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            ) =>
        {
            Some(n.clone())
        }
        _ => None,
    });
    let parent_path = &path[..path.len() - 1];
    let capture_ok = match &preceding {
        None => parent_path.is_empty(),
        Some(p) => header_path(p) == parent_path && p.kind() == SyntaxKind::TABLE_HEADER,
    };
    if !capture_ok {
        return Err(MutateError::Illegal(
            "the entry written here would be captured by the preceding table".into(),
        ));
    }
    let key = path_key_display(&path[parent_path.len()..]);
    let elems: Vec<String> = entries
        .iter()
        .map(|ms| {
            if ms.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", ms.join(", "))
            }
        })
        .collect();
    let text = if multiline {
        let mut s = format!("{key} = [\n");
        for e in &elems {
            s.push_str("  ");
            s.push_str(e);
            s.push_str(",\n");
        }
        s.push_str("]\n");
        s
    } else {
        format!("{key} = [{}]\n", elems.join(", "))
    };
    let parse = taplo::parser::parse(&text);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_els: Vec<_> = frag.children_with_tokens().collect();
    for e in &new_els {
        e.detach();
    }
    tree.splice_children(start..end, new_els);
    Ok(())
}

/// `K` on a keyed array whose elements are **all inline tables**: rewrite it as
/// an `[A/T]` group — one `[[full.path]]` section per element, members one per
/// line. Flat-ROOT keyed entries only; rejected when an entry follows before
/// the next header (the `[[…]]` sections would capture it — D5).
pub(crate) fn convert_array_to_aot(
    tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
) -> Result<(), MutateError> {
    let Some(Target::Entry(entry)) = idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
    else {
        return Err(MutateError::Unsupported);
    };
    if entry.parent().map(|p| p.kind()) != Some(SyntaxKind::ROOT) {
        return Err(MutateError::Unsupported);
    }
    let arr = entry
        .children()
        .find(|c| c.kind() == SyntaxKind::VALUE)
        .and_then(|v| struct_node(&v))
        .filter(|n| n.kind() == SyntaxKind::ARRAY)
        .ok_or(MutateError::Unsupported)?;
    if arr
        .descendants_with_tokens()
        .any(|c| matches!(&c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT))
    {
        return Err(MutateError::Illegal(
            "the array holds comments — remove them first".into(),
        ));
    }
    let values: Vec<SyntaxNode> = arr
        .children()
        .filter(|c| c.kind() == SyntaxKind::VALUE)
        .collect();
    if values.is_empty() {
        return Err(MutateError::Illegal(
            "an empty array has no elements to convert".into(),
        ));
    }
    let mut tables = Vec::new();
    for v in &values {
        let it = struct_node(v)
            .filter(|n| n.kind() == SyntaxKind::INLINE_TABLE)
            .ok_or_else(|| {
                MutateError::Illegal(
                    "only an array of inline tables can become an array of tables".into(),
                )
            })?;
        tables.push(it);
    }
    if entry_follows_before_next_header(tree, entry.index()) {
        return Err(MutateError::Illegal(
            "the [[entries]] written here would capture the keys below them".into(),
        ));
    }
    let header = path_key_display(path);
    let mut text = String::new();
    for it in &tables {
        text.push_str(&format!("[[{header}]]\n"));
        for e in it.children().filter(|c| c.kind() == SyntaxKind::ENTRY) {
            text.push_str(&format!("{}\n", e.to_string().trim()));
        }
    }
    let parse = taplo::parser::parse(&text);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_els: Vec<_> = frag.children_with_tokens().collect();
    for e in &new_els {
        e.detach();
    }
    let i = entry.index();
    let end = extend_over_newline(tree, i + 1);
    tree.splice_children(i..end, new_els);
    Ok(())
}

/// True when, scanning the flat ROOT from child `from` (exclusive), an `ENTRY`
/// appears before the next `[…]`/`[[…]]` header — i.e. a header spliced at/before
/// `from` would capture it (D5).
pub(crate) fn entry_follows_before_next_header(tree: &SyntaxNode, from: usize) -> bool {
    for el in tree.children_with_tokens().skip(from + 1) {
        if let NodeOrToken::Node(n) = el {
            match n.kind() {
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER => return false,
                SyntaxKind::ENTRY => return true,
                _ => {}
            }
        }
    }
    false
}

/// The first `keep` key segments of `entry`'s KEY (with their separators), as
/// written — the complement of [`strip_key_prefix`].
pub(crate) fn key_prefix_text(entry: &SyntaxNode, keep: usize) -> String {
    let Some(key) = entry.children().find(|c| c.kind() == SyntaxKind::KEY) else {
        return String::new();
    };
    let mut out = String::new();
    let mut seen = 0usize;
    for c in key.children_with_tokens() {
        if let NodeOrToken::Token(t) = &c {
            if is_key_seg(t.kind()) {
                seen += 1;
                if seen > keep {
                    break;
                }
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(t.text());
            }
        }
    }
    out
}

pub(crate) fn convert_table(
    tree: &SyntaxNode,
    root: &Node,
    idx: &CstIndex,
    path: &[Seg],
    target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    use crate::model::document::KindTarget as KT;

    // ---- current form: [T/I] — a keyed inline-table entry on the flat ROOT ----
    if let Some(Target::Entry(entry)) = idx.iter().find(|(p, _)| p == path).map(|(_, t)| t) {
        let it = entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::INLINE_TABLE)
            .ok_or(MutateError::Unsupported)?;
        if entry.parent().map(|p| p.kind()) != Some(SyntaxKind::ROOT) {
            return Err(MutateError::Unsupported);
        }
        let members: Vec<String> = it
            .children()
            .filter(|c| c.kind() == SyntaxKind::ENTRY)
            .map(|e| e.to_string().trim().to_string())
            .collect();
        if members.is_empty() {
            return Err(MutateError::Illegal(
                "an empty inline table has no members to convert".into(),
            ));
        }
        let key_text = entry
            .children()
            .find(|c| c.kind() == SyntaxKind::KEY)
            .map(|k| k.to_string().trim().to_string())
            .ok_or(MutateError::Unsupported)?;
        let text = match target {
            KT::TableInline => return Err(MutateError::Illegal("table is already inline".into())),
            KT::TableDotted => members
                .iter()
                .map(|m| format!("{key_text}.{m}\n"))
                .collect::<String>(),
            KT::TableScope => {
                if entry_follows_before_next_header(tree, entry.index()) {
                    return Err(MutateError::Illegal(
                        "a [table] here would capture the keys below it".into(),
                    ));
                }
                format!(
                    "[{}]\n{}",
                    path_key_display(path),
                    members.iter().map(|m| format!("{m}\n")).collect::<String>()
                )
            }
            _ => unreachable!(),
        };
        let parse = taplo::parser::parse(&text);
        if let Some(e) = parse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let frag = parse.into_syntax().clone_for_update();
        let els: Vec<_> = frag.children_with_tokens().collect();
        for e in &els {
            e.detach();
        }
        let i = entry.index();
        let end = extend_over_newline(tree, i + 1);
        tree.splice_children(i..end, els);
        return Ok(());
    }

    // ---- current form: [T/D] — flat dotted members, no own header ----
    if is_headerless_table(idx, root, path) {
        let members = dotted_member_entries(idx, path);
        if members.is_empty() {
            return Err(MutateError::Unsupported);
        }
        let first = members.first().unwrap().clone();
        let depth_below = |e: &SyntaxNode| {
            idx.iter()
                .find(|(_, t)| matches!(t, Target::Entry(n) if n == e))
                .map(|(p, _)| p.len() - path.len())
                .unwrap_or(1)
        };
        let below = |e: &SyntaxNode| {
            let strip = entry_key_seg_count(e).saturating_sub(depth_below(e));
            strip_key_prefix(e, strip).trim().to_string()
        };
        let text = match target {
            KT::TableDotted => return Err(MutateError::Illegal("table is already dotted".into())),
            KT::TableInline => {
                let ms: Vec<String> = members.iter().map(&below).collect();
                if ms.iter().any(|m| m.contains('\n')) {
                    return Err(MutateError::Illegal(
                        "a multi-line member cannot live in an inline table".into(),
                    ));
                }
                // The entry key: the member key's leading segments down to the
                // table (keeps any headerless-ancestor prefix, e.g. `a.b` for a
                // nested `[T/D]`).
                let keep = entry_key_seg_count(&first).saturating_sub(depth_below(&first));
                let key = key_prefix_text(&first, keep);
                key_text_sanity(&key)?;
                format!("{key} = {{ {} }}\n", ms.join(", "))
            }
            KT::TableScope => {
                if entry_follows_foreign(tree, &members) {
                    return Err(MutateError::Illegal(
                        "a [table] here would capture the keys below it".into(),
                    ));
                }
                let mut t = format!("[{}]\n", path_key_display(path));
                for m in &members {
                    t.push_str(&format!("{}\n", below(m)));
                }
                t
            }
            _ => unreachable!(),
        };
        return replace_dotted_table(tree, idx, path, &text);
    }

    // ---- current form: [T/S] — own [header] section ----
    let Some(Target::Header(h)) = idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
    else {
        return Err(MutateError::Unsupported);
    };
    let spans = table_member_spans(tree, idx, path);
    if spans.iter().any(|s| match s {
        MemberSpan::Section(sh) => header_path(sh) != *path,
        MemberSpan::Entry(_) => true,
    }) {
        return Err(MutateError::Illegal(
            "only a self-contained [table] (no sub-tables or dotted members) can convert".into(),
        ));
    }
    // The lines written in place of the section land in whatever scope precedes
    // them: legal only when the nearest preceding header is the parent scope's
    // own (or none, for a root-level table).
    let preceding = tree
        .children_with_tokens()
        .take(h.index())
        .filter_map(|el| match el {
            NodeOrToken::Node(n)
                if matches!(
                    n.kind(),
                    SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
                ) =>
            {
                Some(n)
            }
            _ => None,
        })
        .last();
    let parent_path = &path[..path.len() - 1];
    let capture_ok = match &preceding {
        None => parent_path.is_empty(),
        Some(p) => header_path(p) == parent_path && p.kind() == SyntaxKind::TABLE_HEADER,
    };
    if !capture_ok {
        return Err(MutateError::Illegal(
            "the section's lines would be captured by the preceding table".into(),
        ));
    }
    let i = h.index();
    let end = section_end(tree, path, i);
    let els: Vec<_> = tree.children_with_tokens().collect();
    // Skip the newline that terminates the header line — it belongs to the
    // header, not the body.
    let mut body = &els[i + 1..end];
    if matches!(body.first(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        body = &body[1..];
    }
    let entries: Vec<SyntaxNode> = body
        .iter()
        .filter_map(|el| match el {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ENTRY => Some(n.clone()),
            _ => None,
        })
        .collect();
    // The key prefix relative to the capturing scope (own key for a nested
    // table, the full path at root).
    let rel_prefix = path_key_display(&path[parent_path.len()..]);
    let text = match target {
        KT::TableScope => return Err(MutateError::Illegal("table is already a [scope]".into())),
        KT::TableDotted => {
            if entries.is_empty() {
                return Err(MutateError::Illegal(
                    "an empty [table] has no members to convert".into(),
                ));
            }
            // Keep the body verbatim, prefixing each entry line; comments and
            // blank lines survive in place.
            body.iter()
                .map(|el| match el {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::ENTRY => {
                        format!("{rel_prefix}.{}", n.to_string().trim_start())
                    }
                    NodeOrToken::Node(n) => n.to_string(),
                    NodeOrToken::Token(t) => t.text().to_string(),
                })
                .collect::<String>()
        }
        KT::TableInline => {
            if body
                .iter()
                .any(|el| matches!(el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT))
            {
                return Err(MutateError::Illegal(
                    "the table holds comments — an inline table can't keep them".into(),
                ));
            }
            if entries.is_empty() {
                return Err(MutateError::Illegal(
                    "an empty [table] has no members to convert".into(),
                ));
            }
            let ms: Vec<String> = entries
                .iter()
                .map(|e| e.to_string().trim().to_string())
                .collect();
            if ms.iter().any(|m| m.contains('\n')) {
                return Err(MutateError::Illegal(
                    "a multi-line member cannot live in an inline table".into(),
                ));
            }
            format!("{rel_prefix} = {{ {} }}\n", ms.join(", "))
        }
        _ => unreachable!(),
    };
    let parse = taplo::parser::parse(&text);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_els: Vec<_> = frag.children_with_tokens().collect();
    for e in &new_els {
        e.detach();
    }
    tree.splice_children(i..end, new_els);
    Ok(())
}

/// Validate a rebuilt dotted key parses (`k = 0`); returns the key unchanged.
pub(crate) fn key_text_sanity(key: &str) -> Result<(), MutateError> {
    let parse = taplo::parser::parse(&format!("{key} = 0\n"));
    match parse.errors.first() {
        Some(e) => Err(MutateError::Fragment(e.to_string())),
        None => Ok(()),
    }
}

/// True when an ENTRY that is **not** one of `members` sits between the first
/// member and the next section header — a `[table]` consolidated at the first
/// member would capture it.
pub(crate) fn entry_follows_foreign(tree: &SyntaxNode, members: &[SyntaxNode]) -> bool {
    let Some(first) = members.first() else {
        return false;
    };
    for el in tree.children_with_tokens().skip(first.index() + 1) {
        if let NodeOrToken::Node(n) = el {
            match n.kind() {
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER => return false,
                SyntaxKind::ENTRY if !members.contains(&n) => return true,
                _ => {}
            }
        }
    }
    false
}

/// The `ARRAY` / `INLINE_TABLE` child node of a `VALUE`, if any.
pub(crate) fn struct_node(value: &SyntaxNode) -> Option<SyntaxNode> {
    value
        .children()
        .find(|n| matches!(n.kind(), SyntaxKind::ARRAY | SyntaxKind::INLINE_TABLE))
}
