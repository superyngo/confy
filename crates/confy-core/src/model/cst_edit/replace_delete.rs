//! `Mutation::Replace`/`Delete`/`Remark`/`EditComment`/`InsertComment` and
//! the table/section/member-span machinery they share — split out of
//! `cst_edit.rs` (Task 15, 2026-08-11 audit remediation).

use crate::model::cst_project::{header_path, walk, CstIndex, Target};
use crate::model::document::{MutateError, Target as InsTarget};
use crate::model::node::{Node, NodeKind, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use super::aot_group::{aot_entry_end, aot_group_span, idx_target_is_aot};
use super::convert::{struct_node};
use super::dotted_table::{dotted_ancestor_prefix_len, dotted_member_entries, inline_ancestor_len, inline_member_entries, is_headerless_table, replace_dotted_table, replace_inline_dotted_table, strip_key_prefix};
use super::move_paste::{quote_key_seg};
use super::rename::{is_key_seg};
use super::tree_nav::{comment_block_range, extend_over_newline, is_scalar_kind, next_is_header, node_at, resolve_insert_at};

/// The source text of a `[table]` / `[[aot]]` section starting at `header_idx`,
/// trimmed of a leading blank separator.
pub(crate) fn section_text(syntax: &SyntaxNode, t_path: &[Seg], header_idx: usize, strict: bool) -> String {
    let end = if strict {
        section_end_strict(syntax, header_idx)
    } else {
        section_end(syntax, t_path, header_idx)
    };
    let els: Vec<_> = syntax.children_with_tokens().collect();
    let mut s = String::new();
    for el in &els[header_idx..end] {
        match el {
            NodeOrToken::Node(n) => s.push_str(&n.to_string()),
            NodeOrToken::Token(t) => s.push_str(t.text()),
        }
    }
    s
}

/// Empty-path `Replace`: reparse the edited text as a whole new document, rejecting
/// invalid TOML (the document is left untouched because the caller keeps the old
/// tree on `Err`).
pub(crate) fn reparse_document(toml: &str) -> Result<SyntaxNode, MutateError> {
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    Ok(parse.into_syntax().clone_for_update())
}

/// Detach an `ENTRY` together with its trailing `NEWLINE` (removing the whole line).
pub(crate) fn detach_entry_line(entry: &SyntaxNode) {
    if let Some(nl) = entry.next_sibling_or_token() {
        if matches!(&nl, NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE) {
            nl.detach();
        }
    }
    entry.detach();
}

/// One root-child piece of a table's member set, in document order. A table's
/// definition is an *open set* of lines — flat dotted member entries plus every
/// `[…]`/`[[…]]` section whose header path lies under the table (own header
/// included). `[T/D]`, `[T/S]` and mixed tables are the three compositions of
/// this one span list; serialize/delete/replace/move all fan out over it.
pub(crate) enum MemberSpan {
    /// A flat-ROOT dotted member entry (one line).
    Entry(SyntaxNode),
    /// The header of a member section, covering header..next header (strict).
    Section(SyntaxNode),
}

impl MemberSpan {
    pub(crate) fn start(&self) -> usize {
        match self {
            MemberSpan::Entry(n) | MemberSpan::Section(n) => n.index(),
        }
    }
}

/// The member spans of the table at `path`, in document order. Empty when `path`
/// addresses no root-level table content (e.g. a sub-table of an AoT entry,
/// whose path contains a `Seg::Index`).
pub(crate) fn table_member_spans(tree: &SyntaxNode, idx: &CstIndex, path: &[Seg]) -> Vec<MemberSpan> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut spans: Vec<MemberSpan> = tree
        .children()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            ) && header_path(n).starts_with(path)
        })
        .map(MemberSpan::Section)
        .collect();
    // A flat dotted member entry joins the set unless a member section already
    // covers it (an entry inside `[a.sub]` belongs to that section's span).
    let sec_ranges: Vec<(usize, usize)> = spans
        .iter()
        .map(|s| match s {
            MemberSpan::Section(h) => (h.index(), section_end_strict(tree, h.index())),
            MemberSpan::Entry(_) => unreachable!(),
        })
        .collect();
    for e in dotted_member_entries(idx, path) {
        let i = e.index();
        if !sec_ranges.iter().any(|(s, t)| (*s..*t).contains(&i)) {
            spans.push(MemberSpan::Entry(e));
        }
    }
    spans.sort_by_key(|s| s.start());
    spans
}

/// The source text of the strict section starting at `header` (header line up to
/// the next header of any kind).
pub(crate) fn section_span_text(tree: &SyntaxNode, header: &SyntaxNode) -> String {
    let i = header.index();
    let end = section_end_strict(tree, i);
    let els: Vec<_> = tree.children_with_tokens().collect();
    els[i..end]
        .iter()
        .map(|e| match e {
            NodeOrToken::Node(n) => n.to_string(),
            NodeOrToken::Token(t) => t.text().to_string(),
        })
        .collect()
}

/// The dotted source form of a key path (`[s, a]` → `s.a`, quoting as needed).
pub(crate) fn path_key_display(path: &[Seg]) -> String {
    path.iter()
        .filter_map(|s| match s {
            Seg::Key(k) => Some(quote_key_seg(k)),
            Seg::Index(_) => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The number of key segments written in an entry's own `KEY` text.
pub(crate) fn entry_key_seg_count(entry: &SyntaxNode) -> usize {
    entry
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .map(|k| {
            k.children_with_tokens()
                .filter(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
                .count()
        })
        .unwrap_or(0)
}

/// Drop the first `strip` key segments (and their dots) from every header in a
/// section fragment — the inverse of `prefix_section_headers`, used to capture a
/// nested table scope-relative (`[a.sub]` captured as table `sub` → `[sub]`).
pub(crate) fn strip_section_header_prefix(frag: &SyntaxNode, strip: usize) {
    if strip == 0 {
        return;
    }
    let headers: Vec<SyntaxNode> = frag
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            )
        })
        .collect();
    for h in headers {
        let key = match h.children().find(|c| c.kind() == SyntaxKind::KEY) {
            Some(k) => k,
            None => continue,
        };
        let els: Vec<_> = key.children_with_tokens().collect();
        let mut seen = 0usize;
        let mut keep_from = els.len();
        for (k, c) in els.iter().enumerate() {
            if let NodeOrToken::Token(t) = c {
                if is_key_seg(t.kind()) {
                    seen += 1;
                    if seen == strip + 1 {
                        keep_from = k;
                        break;
                    }
                }
            }
        }
        for c in &els[..keep_from] {
            c.detach();
        }
    }
}

/// Span-based fragment of the table at `path` (`None` when it has no member
/// spans). `relative` (clipboard capture) strips the table's ancestor key
/// segments from entries and headers so a paste re-prefixes only for the
/// destination. Non-relative (`$EDITOR` block edit) keeps full keys; a *mixed*
/// table (dotted members + sections) is canonicalized to scope form — a
/// synthesized `[full.key]` header with the dotted members folded under it,
/// followed by the member sections.
pub(crate) fn table_fragment(
    tree: &SyntaxNode,
    idx: &CstIndex,
    root: &Node,
    path: &[Seg],
    relative: bool,
) -> Option<String> {
    let spans = table_member_spans(tree, idx, path);
    if spans.is_empty() {
        return None;
    }
    let ensure_nl = |s: String| {
        if s.ends_with('\n') {
            s
        } else {
            format!("{s}\n")
        }
    };
    let entry_strip = if relative {
        dotted_ancestor_prefix_len(idx, root, path)
    } else {
        0
    };
    let has_sections = spans.iter().any(|s| matches!(s, MemberSpan::Section(_)));
    // Pure `[T/D]`: the member lines — full keys for the block edit (which
    // splices back into the same scope), scope-relative for the clipboard.
    if !has_sections {
        return Some(
            spans
                .iter()
                .map(|s| match s {
                    MemberSpan::Entry(e) => ensure_nl(strip_key_prefix(e, entry_strip)),
                    MemberSpan::Section(_) => unreachable!(),
                })
                .collect(),
        );
    }
    let has_entries = spans.iter().any(|s| matches!(s, MemberSpan::Entry(_)));
    let mut text = String::new();
    if !relative && has_entries {
        // Mixed table, block edit: canonical scope form (the only header-form
        // a re-splice can produce without leaving dotted definitions behind).
        text.push_str(&format!("[{}]\n", path_key_display(path)));
    }
    for s in &spans {
        match s {
            MemberSpan::Entry(e) => {
                let strip = if relative {
                    entry_strip
                } else {
                    // Fold under the synthesized header: keep only the
                    // segments *below* the table.
                    let depth_below = idx
                        .iter()
                        .find(|(_, t)| matches!(t, Target::Entry(n) if n == e))
                        .map(|(p, _)| p.len() - path.len())
                        .unwrap_or(1);
                    entry_key_seg_count(e).saturating_sub(depth_below)
                };
                text.push_str(&ensure_nl(strip_key_prefix(e, strip)));
            }
            MemberSpan::Section(h) => text.push_str(&section_span_text(tree, h)),
        }
    }
    if relative {
        let strip = path.iter().filter(|s| matches!(s, Seg::Key(_))).count() - 1;
        if strip > 0 {
            let parse = taplo::parser::parse(&text);
            if parse.errors.is_empty() {
                let f = parse.into_syntax().clone_for_update();
                strip_section_header_prefix(&f, strip);
                text = f.to_string();
            }
        }
    }
    Some(text)
}

/// Block-rewrite a table that has member *sections* (`$EDITOR` on a `[T/S]`,
/// scattered or not, or on a mixed table): remove every member span and splice
/// the edited block in at the first member **section**'s position. With more
/// than one span (a consolidation) the block must stay inside the table —
/// every header under the table's path, and header-led (a leading top-level
/// entry would attach to whatever section precedes the splice point).
pub(crate) fn replace_table_spans(
    tree: &SyntaxNode,
    path: &[Seg],
    spans: &[MemberSpan],
    toml: &str,
) -> Result<(), MutateError> {
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    if spans.len() > 1 {
        for h in frag.descendants().filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            )
        }) {
            if !header_path(&h).starts_with(path) {
                return Err(MutateError::Illegal(format!(
                    "the edited block defines `[{}]` outside this table",
                    path_key_display(&header_path(&h))
                )));
            }
        }
        let first_content = frag.children().find(|n| {
            matches!(
                n.kind(),
                SyntaxKind::ENTRY | SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            )
        });
        if matches!(&first_content, Some(n) if n.kind() == SyntaxKind::ENTRY) {
            return Err(MutateError::Illegal(
                "the edited block must start with a [header] line".into(),
            ));
        }
    }
    let anchor = spans
        .iter()
        .find_map(|s| match s {
            MemberSpan::Section(h) => Some(h.clone()),
            MemberSpan::Entry(_) => None,
        })
        .ok_or(MutateError::NotFound)?;
    // Remove the other spans in reverse document order (handles re-query their
    // positions, so earlier spans stay valid).
    for s in spans.iter().rev() {
        match s {
            MemberSpan::Entry(e) => detach_entry_line(e),
            MemberSpan::Section(h) if *h != anchor => {
                let i = h.index();
                let end = section_end_strict(tree, i);
                tree.splice_children(i..end, vec![]);
            }
            MemberSpan::Section(_) => {}
        }
    }
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    let i = anchor.index();
    let end = section_end_strict(tree, i);
    tree.splice_children(i..end, els);
    Ok(())
}

pub(crate) fn replace_value(tree: &SyntaxNode, path: &[Seg], toml: &str) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");
    // A table block-rewrites over its member spans: a pure `[T/D]` consolidates
    // its member lines at the first one; any table with member sections —
    // `[T/S]` (scattered or not) or mixed — consolidates at its first section.
    if node_at(&proj.root, path).is_some_and(|n| matches!(n.kind, NodeKind::Table))
        && matches!(path.last(), Some(Seg::Key(_)))
    {
        let spans = table_member_spans(tree, &idx, path);
        if spans.iter().any(|s| matches!(s, MemberSpan::Section(_))) {
            return replace_table_spans(tree, path, &spans, toml);
        }
        if !spans.is_empty() {
            return replace_dotted_table(tree, &idx, path, toml);
        }
        if inline_ancestor_len(&proj.root, path).is_some() {
            return replace_inline_dotted_table(tree, &idx, &proj.root, path, toml);
        }
    }
    let target = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone()) {
        Some(t) => t,
        None => return Err(MutateError::NotFound),
    };
    // Whole-group replace (`$EDITOR` on an AoT *group* node): swap all of its
    // `[[x]]` entries for the edited fragment.
    if let Target::AotGroup = &target {
        let (start, end) = aot_group_span(tree, path).ok_or(MutateError::Unsupported)?;
        let parse = taplo::parser::parse(toml);
        if let Some(e) = parse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let frag = parse.into_syntax().clone_for_update();
        let els: Vec<_> = frag.children_with_tokens().collect();
        for e in &els {
            e.detach();
        }
        tree.splice_children(start..end, els);
        return Ok(());
    }
    // Whole-section replace (`$EDITOR` on a `[table]` or `[[aot]]` entry): swap the
    // section's elements for the edited fragment.
    if let Target::Header(header) | Target::AotEntry(header) = &target {
        let parse = taplo::parser::parse(toml);
        if let Some(e) = parse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let frag = parse.into_syntax().clone_for_update();
        let els: Vec<_> = frag.children_with_tokens().collect();
        for e in &els {
            e.detach();
        }
        let i = header.index();
        let end = if header.kind() == SyntaxKind::TABLE_ARRAY_HEADER {
            section_end_strict(tree, i)
        } else {
            section_end(tree, path, i)
        };
        tree.splice_children(i..end, els);
        return Ok(());
    }

    let value = match target {
        Target::Entry(entry) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::NotFound)?,
        Target::ArrayElement(value) => value,
        _ => return Err(MutateError::Unsupported),
    };

    // The new scalar token from the fragment's first ENTRY's VALUE.
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_value = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))?;

    // Swap the VALUE's content element — a scalar token OR an ARRAY / INLINE_TABLE
    // node — for the fragment's, preserving the VALUE wrapper and any trailing EOL
    // comment. Works for every combination, including a scalar↔structured *type
    // change* (e.g. `5` → `[1, 2]`).
    let is_content = |c: &taplo::syntax::SyntaxElement| match c {
        NodeOrToken::Token(t) => is_scalar_kind(t.kind()),
        NodeOrToken::Node(n) => matches!(n.kind(), SyntaxKind::ARRAY | SyntaxKind::INLINE_TABLE),
    };
    let old_content = value
        .children_with_tokens()
        .find(&is_content)
        .ok_or(MutateError::Unsupported)?;
    let new_content = new_value
        .children_with_tokens()
        .find(&is_content)
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))?;
    let i = old_content.index();
    new_content.detach();
    value.splice_children(i..i + 1, vec![new_content]);
    Ok(())
}

/// `Mutation::SetTrailingComment` — set/change/clear the EOL comment of the keyed
/// scalar at `path`. The trailing `# …` is replaced textually between the value's
/// own content and the line's terminating newline (a comment can't span lines, so
/// the next `\n` is the safe right edge); the result is reparsed. Only a keyed
/// scalar entry is supported — array elements stay display-only.
pub(crate) fn set_trailing_comment(
    tree: &SyntaxNode,
    path: &[Seg],
    comment: Option<&str>,
) -> Result<SyntaxNode, MutateError> {
    let (_proj, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    // A `[section]` / `[[aot]]` header: the EOL comment sits after the closing
    // `]`, inside the header node (the NEWLINE is the header's sibling). Splice
    // there rather than through a VALUE.
    if let Target::Header(h) | Target::AotEntry(h) = &target {
        return set_header_trailing_comment(tree, h, comment);
    }
    let value = match target {
        Target::Entry(entry) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::NotFound)?,
        // A multiline-array element: the carried node is already its VALUE.
        Target::ArrayElement(value) => value,
        _ => return Err(MutateError::Unsupported),
    };
    // End of the value's *own* content (scalar token / array / inline table),
    // before any trailing whitespace + comment that the VALUE node also holds.
    let is_content = |c: &taplo::syntax::SyntaxElement| match c {
        NodeOrToken::Token(t) => is_scalar_kind(t.kind()),
        NodeOrToken::Node(n) => matches!(n.kind(), SyntaxKind::ARRAY | SyntaxKind::INLINE_TABLE),
    };
    let content = value
        .children_with_tokens()
        .find(&is_content)
        .ok_or(MutateError::Unsupported)?;
    let mut cut_start: usize = content.text_range().end().into();
    let full = tree.to_string();
    // Preserve a following separator comma (a multiline-array element is
    // `1,  # c`); a keyed entry has no comma, so this is a no-op for it.
    let rest = &full[cut_start..];
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.starts_with(',') {
        cut_start += (rest.len() - after_ws.len()) + 1;
    }
    let cut_end = full[cut_start..]
        .find('\n')
        .map(|i| cut_start + i)
        .unwrap_or(full.len());
    let tail = match comment {
        Some(c) => format!("  {}", c.trim()),
        None => String::new(),
    };
    let new_text = format!("{}{}{}", &full[..cut_start], tail, &full[cut_end..]);
    reparse_document(&new_text)
}

/// Set/change/clear the EOL comment of a `[section]` / `[[aot]]` header. The
/// comment lives after the closing bracket(s), inside the header node; the splice
/// rewrites from the last `]` to the header node's end (the NEWLINE is outside it).
pub(crate) fn set_header_trailing_comment(
    tree: &SyntaxNode,
    header: &SyntaxNode,
    comment: Option<&str>,
) -> Result<SyntaxNode, MutateError> {
    let last_bracket = header
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::BRACKET_END)
        .last()
        .ok_or(MutateError::Unsupported)?;
    let cut_start: usize = last_bracket.text_range().end().into();
    let cut_end: usize = header.text_range().end().into();
    let full = tree.to_string();
    let tail = match comment {
        Some(c) => format!("  {}", c.trim()),
        None => String::new(),
    };
    let new_text = format!("{}{}{}", &full[..cut_start], tail, &full[cut_end..]);
    reparse_document(&new_text)
}

/// Replace the text of the standalone comment block at `path`. The block is the run
/// of `COMMENT` tokens (separated by single newlines) starting at the indexed
/// token; it is spliced with `text`'s lines, each validated to start with `#`.
pub(crate) fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
    if text
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        return Err(MutateError::Fragment(
            "comment lines must start with #".into(),
        ));
    }
    let (_, idx) = walk(tree, "");
    let first = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone()) {
        Some(Target::Comment(t)) => t,
        Some(_) => return Err(MutateError::Unsupported),
        None => return Err(MutateError::NotFound),
    };
    let parent = first.parent().ok_or(MutateError::NotFound)?;
    let (start, end) = comment_block_range(&parent, &first);

    // New COMMENT/NEWLINE elements from parsing the replacement (drop a trailing
    // newline — the block's following newline stays in place).
    let frag = taplo::parser::parse(text).into_syntax().clone_for_update();
    let mut els: Vec<_> = frag.children_with_tokens().collect();
    while matches!(els.last(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        els.pop();
    }
    for e in &els {
        e.detach();
    }
    parent.splice_children(start..end, els);
    Ok(())
}

/// Delete the node at `path`. A keyed entry (leaf / array / inline table) at the
/// document or table scope is removed with its trailing newline; a comment block is
/// removed with its trailing newline. Because comments are independent nodes now,
/// deleting an entry leaves any adjacent comment in place for free.
pub(crate) fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");
    // A table's definition is an open set of member spans (dotted entries and/or
    // `[…]` sections, possibly scattered) — delete fans out over all of them, in
    // reverse document order so earlier spans stay valid.
    if node_at(&proj.root, path).is_some_and(|n| matches!(n.kind, NodeKind::Table))
        && matches!(path.last(), Some(Seg::Key(_)))
    {
        let spans = table_member_spans(tree, &idx, path);
        if !spans.is_empty() {
            for s in spans.iter().rev() {
                match s {
                    MemberSpan::Entry(e) => detach_entry_line(e),
                    MemberSpan::Section(h) => {
                        let i = h.index();
                        let end = section_end_strict(tree, i);
                        tree.splice_children(i..end, vec![]);
                    }
                }
            }
            return Ok(());
        }
        // A synthetic `[T/D]` table *inside an inline table*: fan out over its
        // member entries in the `{ … }` (reverse order keeps separators valid).
        if inline_ancestor_len(&proj.root, path).is_some() {
            let members = inline_member_entries(&idx, path);
            if !members.is_empty() {
                for m in members.iter().rev() {
                    if let Some(parent) = m.parent() {
                        delete_seq_element(&parent, m.index());
                    }
                }
                return Ok(());
            }
        }
    }
    let target = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone()) {
        Some(t) => t,
        None => return Err(MutateError::NotFound),
    };
    match target {
        Target::Comment(first) => {
            let parent = first.parent().ok_or(MutateError::NotFound)?;
            let (start, end) = comment_block_range(&parent, &first);
            let end = extend_over_newline(&parent, end);
            parent.splice_children(start..end, vec![]);
            Ok(())
        }
        Target::Entry(entry) => {
            let parent = entry.parent().ok_or(MutateError::NotFound)?;
            match parent.kind() {
                // Document / table scope: the entry occupies its own line.
                SyntaxKind::ROOT => {
                    let i = entry.index();
                    let end = extend_over_newline(&parent, i + 1);
                    parent.splice_children(i..end, vec![]);
                    Ok(())
                }
                // Inline-table member: remove the entry with its `,` separator.
                SyntaxKind::INLINE_TABLE => {
                    delete_seq_element(&parent, entry.index());
                    Ok(())
                }
                _ => Err(MutateError::Unsupported),
            }
        }
        Target::ArrayElement(value) => {
            let arr = value.parent().ok_or(MutateError::NotFound)?;
            delete_seq_element(&arr, value.index());
            Ok(())
        }
        // Delete a whole array-of-tables (`d` on the `[[x]]` group): remove every
        // section whose header path equals this one, bottom-up.
        Target::AotGroup => {
            let mut starts: Vec<usize> = tree
                .children_with_tokens()
                .enumerate()
                .filter_map(|(k, e)| match e {
                    NodeOrToken::Node(n)
                        if n.kind() == SyntaxKind::TABLE_ARRAY_HEADER
                            && header_path(&n) == path =>
                    {
                        Some(k)
                    }
                    _ => None,
                })
                .collect();
            starts.sort_unstable();
            for &i in starts.iter().rev() {
                let end = section_end_strict(tree, i);
                tree.splice_children(i..end, vec![]);
            }
            Ok(())
        }
        // Delete a whole `[table]` section (header + entries + nested sub-tables).
        Target::Header(header) => {
            let i = header.index();
            let end = section_end(tree, path, i);
            tree.splice_children(i..end, vec![]);
            Ok(())
        }
        // Delete one `[[aot]]` entry: its full extent — header + entries + its
        // own sub-sections (`[fruit.physical]`), up to the group's next entry or
        // a foreign header.
        Target::AotEntry(header) => {
            let i = header.index();
            let end = aot_entry_end(tree, &header_path(&header), i);
            tree.splice_children(i..end, vec![]);
            Ok(())
        }
    }
}

/// Like [`section_end`] but stops at the *next header of any kind* — used for a
/// single array-of-tables entry, where the following `[[x]]` is a separate entry.
pub(crate) fn section_end_strict(tree: &SyntaxNode, header_idx: usize) -> usize {
    let els: Vec<_> = tree.children_with_tokens().collect();
    for (k, el) in els.iter().enumerate().skip(header_idx + 1) {
        if let NodeOrToken::Node(n) = el {
            if matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            ) {
                return k;
            }
        }
    }
    els.len()
}

/// The end (exclusive ROOT-child index) of the `[table]` section that starts at
/// `header_idx`: everything until the next header that is *not* a descendant of
/// `t_path` (so nested sub-tables stay with their parent), or end of document.
pub(crate) fn section_end(tree: &SyntaxNode, t_path: &[Seg], header_idx: usize) -> usize {
    let els: Vec<_> = tree.children_with_tokens().collect();
    for (k, el) in els.iter().enumerate().skip(header_idx + 1) {
        if let NodeOrToken::Node(n) = el {
            if matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            ) && !header_path(n).starts_with(t_path)
            {
                return k;
            }
        }
    }
    els.len()
}

/// Remove the comma-separated element at child index `vi` from an `ARRAY` or
/// `INLINE_TABLE`, taking one `,` separator with it (the one after the element, or —
/// for the last element — the one before) plus the adjacent run of whitespace/
/// newlines, so `[1, 2, 3]` → `[1, 3]` and `{ x = 1, y = 2 }` → `{ y = 2 }`.
pub(crate) fn delete_seq_element(arr: &SyntaxNode, vi: usize) {
    let els: Vec<_> = arr.children_with_tokens().collect();
    let is_comma = |i: usize| matches!(els.get(i), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::COMMA);
    let is_trivia = |i: usize| {
        matches!(els.get(i), Some(NodeOrToken::Token(t))
            if matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE))
    };
    // Comma after the element (skipping trivia)?
    let mut j = vi + 1;
    while is_trivia(j) {
        j += 1;
    }
    if is_comma(j) {
        let mut end = j + 1;
        while is_trivia(end) {
            end += 1;
        }
        arr.splice_children(vi..end, vec![]);
        return;
    }
    // Last element: take the preceding comma + trivia.
    let mut start = vi;
    while start > 0 && is_trivia(start - 1) {
        start -= 1;
    }
    if start > 0 && is_comma(start - 1) {
        start -= 1;
    }
    arr.splice_children(start..vi + 1, vec![]);
}

/// Insert a standalone comment line into a *multiline* array at the projected
/// full-sequence `index` (counting elements + standalone comments alike). The
/// comment lands on its own line before the slot's element/comment, indented to
/// match the array's existing lines; an out-of-range index appends before `]`.
pub(crate) fn array_insert_comment(
    idx: &CstIndex,
    array_path: &[Seg],
    index: usize,
    text: &str,
) -> Result<(), MutateError> {
    let arr = entry_array(idx, array_path)?;
    let els: Vec<_> = arr.children_with_tokens().collect();

    // Indent = the whitespace before the first element/comment line, else two spaces.
    let indent = els
        .iter()
        .enumerate()
        .find_map(|(i, e)| match e {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => match els.get(i + 1) {
                Some(NodeOrToken::Node(n)) if n.kind() == SyntaxKind::VALUE => {
                    Some(t.text().to_string())
                }
                Some(NodeOrToken::Token(c)) if c.kind() == SyntaxKind::COMMENT => {
                    Some(t.text().to_string())
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| "  ".to_string());

    // Slot anchors: each VALUE node + each standalone COMMENT token (a COMMENT with a
    // NEWLINE since the last value), in order, by their `els` position — matching the
    // projection's full-sequence indexing.
    let mut slots: Vec<usize> = Vec::new();
    let mut newline_since_value = true;
    for (i, e) in els.iter().enumerate() {
        match e {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::VALUE => {
                slots.push(i);
                newline_since_value = false;
            }
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::NEWLINE => newline_since_value = true,
                SyntaxKind::COMMENT if newline_since_value => slots.push(i),
                _ => {}
            },
            _ => {}
        }
    }

    let line = comment_line_elements(&indent, text)?;
    let at = if let Some(&ci) = slots.get(index) {
        // Before the slot's line: its leading indent WS if present, else the token.
        if ci > 0
            && matches!(els.get(ci - 1), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::WHITESPACE)
        {
            ci - 1
        } else {
            ci
        }
    } else {
        // Append before the closing bracket.
        els.iter()
            .position(|e| matches!(e, NodeOrToken::Token(t) if t.kind() == SyntaxKind::BRACKET_END))
            .ok_or(MutateError::Unsupported)?
    };
    arr.splice_children(at..at, line);
    Ok(())
}

/// Resolve a keyed-array path to its `ARRAY` syntax node (via the entry's VALUE).
pub(crate) fn entry_array(idx: &CstIndex, array_path: &[Seg]) -> Result<SyntaxNode, MutateError> {
    match idx.iter().find(|(p, _)| p == array_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::ARRAY)
            .ok_or(MutateError::Unsupported),
        _ => Err(MutateError::Unsupported),
    }
}

/// Rewrite a single-line array as multiline — one element per line with a
/// trailing comma, two-space indent — so it can hold standalone comment lines.
/// Elements keep their exact source repr; a trailing comment after the array on
/// the entry line is outside the `ARRAY` node and stays put.
pub(crate) fn array_make_multiline(arr: &SyntaxNode) -> Result<(), MutateError> {
    let elems: Vec<String> = arr
        .children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::VALUE => {
                Some(n.to_string().trim().to_string())
            }
            _ => None,
        })
        .collect();
    let mut s = String::from("[\n");
    for e in &elems {
        s.push_str("  ");
        s.push_str(e);
        s.push_str(",\n");
    }
    s.push(']');
    let parse = taplo::parser::parse(&format!("x = {s}\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let root = parse.into_syntax().clone_for_update();
    let new_arr = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARRAY)
        .ok_or(MutateError::Unsupported)?;
    new_arr.detach();
    let parent = arr.parent().ok_or(MutateError::NotFound)?;
    let i = arr.index();
    parent.splice_children(i..i + 1, vec![NodeOrToken::Node(new_arr)]);
    Ok(())
}

/// Fresh `WHITESPACE COMMENT NEWLINE` elements for each line of `text`, indented.
pub(crate) fn comment_line_elements(
    indent: &str,
    text: &str,
) -> Result<Vec<taplo::syntax::SyntaxElement>, MutateError> {
    let mut s = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with('#') {
            return Err(MutateError::Fragment(
                "comment lines must start with #".into(),
            ));
        }
        s.push_str(indent);
        s.push_str(line);
        s.push('\n');
    }
    let frag = taplo::parser::parse(&s).into_syntax().clone_for_update();
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    Ok(els)
}

/// Insert a standalone comment block at the projected `target` position. Comments
/// are independent nodes — no key, no collision.
pub(crate) fn insert_comment(tree: &SyntaxNode, target: &InsTarget, text: &str) -> Result<(), MutateError> {
    if text
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        return Err(MutateError::Fragment(
            "comment lines must start with #".into(),
        ));
    }
    let (proj, idx) = walk(tree, "");
    use crate::model::node::NodeKind;
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    // A synthetic `[T/D]` table *inside an inline table* projects as `Table`, but
    // its members live in a `{ … }`, which holds no comments.
    if matches!(parent.kind, NodeKind::Table)
        && inline_ancestor_len(&proj.root, &target.parent).is_some()
    {
        return Err(MutateError::Illegal(
            "comments can only be inserted into a table, the document, or a multiline array".into(),
        ));
    }
    match parent.kind {
        NodeKind::Root | NodeKind::Table => {} // decor slot — handled below
        // A multiline array can hold a standalone comment line; a single-line array
        // can't (a `#` would comment out the closing bracket), so it is upgraded to
        // multiline first. Inline tables / AoT groups never hold comments.
        NodeKind::Array if parent.value.is_none() => {
            return array_insert_comment(&idx, &target.parent, target.index, text);
        }
        NodeKind::Array => {
            let arr = entry_array(&idx, &target.parent)?;
            array_make_multiline(&arr)?;
            // The entry in `idx` is still live; array_insert_comment re-resolves
            // the (now multiline) ARRAY through it.
            return array_insert_comment(&idx, &target.parent, target.index, text);
        }
        _ => {
            return Err(MutateError::Illegal(
                "comments can only be inserted into a table, the document, or a multiline array"
                    .into(),
            ));
        }
    }
    // A synthetic `[T/D]` table holds no comments: a comment pasted "into" it
    // lands at the scope level directly above the table's first member, as an
    // independent node.
    let dotted_anchor = if matches!(parent.kind, NodeKind::Table)
        && is_headerless_table(&idx, &proj.root, &target.parent)
    {
        dotted_member_entries(&idx, &target.parent)
            .first()
            .map(|e| e.index())
    } else {
        None
    };
    let at = match dotted_anchor {
        Some(i) => i,
        None => resolve_insert_at(tree, &proj.root, &idx, target)?,
    };
    // `# …\n` per line, so each comment lands on its own line before the anchor.
    let mut frag_text = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    // Appending into a table scope right before an outer header: add a blank line
    // so the comment trails THIS table (projection's blank-line rule) instead of
    // becoming the next section's leading comment.
    if matches!(parent.kind, NodeKind::Table) && next_is_header(tree, at) {
        frag_text.push('\n');
    }
    let frag = taplo::parser::parse(&frag_text)
        .into_syntax()
        .clone_for_update();
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    tree.splice_children(at..at, els);
    Ok(())
}

/// Toggle the node at `path` between live and commented-out. A live entry becomes a
/// `# …` comment of its source line; a comment is uncommented by stripping the `#`
/// and reparsing as live TOML. (Table/AoT subtree remark is deferred.)
pub(crate) fn remark(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    let (_, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    match target {
        // Comment out a single entry line.
        Target::Entry(entry) => {
            let parent = entry.parent().ok_or(MutateError::NotFound)?;
            if parent.kind() != SyntaxKind::ROOT {
                return Err(MutateError::Unsupported);
            }
            let comment = format!("# {entry}");
            let tok = first_comment_token(&comment)?;
            let i = entry.index();
            parent.splice_children(i..i + 1, vec![NodeOrToken::Token(tok)]);
            Ok(())
        }
        // Uncomment a comment block: strip `#` and reparse the lines as live TOML.
        Target::Comment(first) => {
            let parent = first.parent().ok_or(MutateError::NotFound)?;
            let (start, end) = comment_block_range(&parent, &first);
            let els: Vec<_> = parent.children_with_tokens().collect();
            let mut stripped = String::new();
            for e in &els[start..end] {
                if let NodeOrToken::Token(t) = e {
                    if t.kind() == SyntaxKind::COMMENT {
                        let s = t.text().trim_start();
                        let s = s.strip_prefix('#').unwrap_or(s);
                        let s = s.strip_prefix(' ').unwrap_or(s);
                        stripped.push_str(s);
                        stripped.push('\n');
                    }
                }
            }
            let parse = taplo::parser::parse(&stripped);
            if let Some(e) = parse.errors.first() {
                return Err(MutateError::Fragment(e.to_string()));
            }
            let frag = parse.into_syntax().clone_for_update();
            let mut new_els: Vec<_> = frag.children_with_tokens().collect();
            while matches!(new_els.last(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE)
            {
                new_els.pop();
            }
            for e in &new_els {
                e.detach();
            }
            parent.splice_children(start..end, new_els);
            Ok(())
        }
        // Comment out a whole `[table]` / `[[aot]]` section, line by line.
        Target::Header(header) | Target::AotEntry(header) => {
            let strict = idx_target_is_aot(&header);
            let i = header.index();
            let end = if strict {
                section_end_strict(tree, i)
            } else {
                section_end(tree, path, i)
            };
            let els: Vec<_> = tree.children_with_tokens().collect();
            let raw: String = els[i..end]
                .iter()
                .map(|e| match e {
                    NodeOrToken::Node(n) => n.to_string(),
                    NodeOrToken::Token(t) => t.text().to_string(),
                })
                .collect();
            let body = raw.strip_suffix('\n').unwrap_or(&raw);
            let commented: String = body
                .split('\n')
                .map(|l| {
                    if l.is_empty() {
                        "#".to_string()
                    } else {
                        format!("# {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let frag = taplo::parser::parse(&format!("{commented}\n"))
                .into_syntax()
                .clone_for_update();
            let new_els: Vec<_> = frag.children_with_tokens().collect();
            for e in &new_els {
                e.detach();
            }
            tree.splice_children(i..end, new_els);
            Ok(())
        }
        _ => Err(MutateError::Unsupported),
    }
}

/// Build a single `COMMENT` token from `text` (a `# …` line).
pub(crate) fn first_comment_token(text: &str) -> Result<SyntaxToken, MutateError> {
    let frag = taplo::parser::parse(&format!("{text}\n"))
        .into_syntax()
        .clone_for_update();
    let tok = frag
        .children_with_tokens()
        .find_map(|c| c.into_token().filter(|t| t.kind() == SyntaxKind::COMMENT))
        .ok_or_else(|| MutateError::Fragment("not a comment".into()))?;
    tok.detach();
    Ok(tok)
}
