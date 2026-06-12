//! Phase 3 of the CST migration: apply a [`Mutation`] to the rowan tree by
//! splicing green nodes/tokens (the mutable `clone_for_update` API).
//!
//! Resolution uses the same `path → element` index that `cst_project::walk`
//! produces, so the resolver can never disagree with the projection. Each `apply`
//! works on a `clone_for_update` of the document and returns the new tree only on
//! success, so a failed multi-step edit (e.g. `Move`) rolls back for free — the
//! caller keeps the original tree untouched.
//!
//! All eight `Mutation` variants are ported: `Replace` (whole-document, scalar
//! value, structured array/inline-table, and whole `[table]` section), `Insert`
//! (keyed into a table/root with Cancel/Overwrite/Rename collisions, and bare array
//! elements), `Delete` (entry, comment, array element, `[table]` section, `[[aot]]`
//! entry), `Rename`, `Remark`, `EditComment`, `InsertComment`, and `Move` (atomic;
//! comments stay put because they are independent nodes). Deferred long-tail edges:
//! inline-table member delete, AoT entry move/remark, whole-AoT delete/replace, and
//! byte-perfect multiline-array element insert/delete spacing.

use crate::model::cst_project::{header_path, walk, CstIndex, Target};
use crate::model::document::{MutateError, Mutation, OnCollision, Target as InsTarget};
use crate::model::node::{Format, Node, NodeKind, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Apply `m` to a copy of `syntax`, returning the new tree. The original is never
/// mutated, so the caller commits only on `Ok`.
pub(crate) fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    let result = match m {
        Mutation::Replace { path, toml, .. } => {
            if path.is_empty() {
                reparse_document(&toml)?
            } else {
                replace_value(&tree, &path, &toml)?;
                tree
            }
        }
        Mutation::EditComment { path, text } => {
            edit_comment(&tree, &path, &text)?;
            tree
        }
        Mutation::Delete { path } => {
            delete(&tree, &path)?;
            tree
        }
        Mutation::InsertComment { target, text } => {
            insert_comment(&tree, &target, &text)?;
            tree
        }
        Mutation::Insert {
            target,
            toml,
            on_collision,
        } => {
            insert(&tree, &target, &toml, on_collision)?;
            tree
        }
        Mutation::Rename { path, new_key } => {
            rename(&tree, &path, &new_key)?;
            tree
        }
        Mutation::Remark { path } => {
            remark(&tree, &path)?;
            tree
        }
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => {
            move_nodes(&tree, &sources, &target, on_collision)?;
            tree
        }
    };
    validate_semantics(&result)?;
    Ok(result)
}

/// Semantic backstop run on every successful mutation before commit: taplo's
/// parser is syntax-only (a duplicate `[a]` section or re-defined key parses
/// clean), so the result is checked with taplo's DOM validation, which rejects
/// conflicting keys / table redefinitions while accepting every legal layout
/// (scattered `[a] … [a.sub]`, dotted siblings, AoT re-openings, the
/// `fruit.apple` mixed pattern). Catches duplicates the targeted pre-checks
/// can't see — e.g. a whole-document or block `$EDITOR` rewrite that introduces
/// a duplicate section.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let dom = taplo::parser::parse(&tree.to_string()).into_dom();
    if let Err(errors) = dom.validate() {
        if let Some(e) = errors.into_iter().next() {
            return Err(match &e {
                taplo::dom::Error::ConflictingKeys { key, .. } => {
                    MutateError::Collision(key.value().to_string())
                }
                other => MutateError::Illegal(other.to_string()),
            });
        }
    }
    Ok(())
}

/// Serialize the node at `path` as a standalone fragment (clipboard / `$EDITOR`).
/// In the CST a fragment is just the node's source text — comments are independent
/// nodes, so a node never carries an adjacent comment (`carry_comment` is moot).
pub(crate) fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    serialize_fragment_impl(syntax, path, false)
}

/// Like [`serialize_fragment`] but **scope-relative**: a node copied out of a
/// `[T/D]` dotted table has its leading dotted-ancestor key segments dropped
/// (`dotted.test.bool_true` → `bool_true`; the `test` subtable's members →
/// `test.bool_true`). Used by copy/cut so a paste re-prefixes only for the new
/// destination instead of stacking the source's prefix. The plain
/// `serialize_fragment` (used by the `$EDITOR` block edit, which must keep full
/// keys for `replace_dotted_table`) is unaffected.
pub(crate) fn serialize_fragment_relative(syntax: &SyntaxNode, path: &[Seg]) -> String {
    serialize_fragment_impl(syntax, path, true)
}

fn serialize_fragment_impl(syntax: &SyntaxNode, path: &[Seg], relative: bool) -> String {
    let (proj, idx) = walk(syntax, "");
    // A comment node: its raw `# …` text.
    if let Some(node) = node_at(&proj.root, path) {
        if let NodeKind::Comment(t) = &node.kind {
            return t.clone();
        }
        // A table is an *open set* of member spans (dotted entries and/or
        // `[…]` sections, possibly scattered) — capture all of them.
        if matches!(node.kind, NodeKind::Table) && matches!(path.last(), Some(Seg::Key(_))) {
            if let Some(text) = table_fragment(syntax, &idx, &proj.root, path, relative) {
                return text;
            }
            // A synthetic `[T/D]` table *inside an inline table*: its members are
            // `x.y = 1` entries in the `{ … }`. Verbatim keys for the `$EDITOR`
            // block edit; relative drops the segments between the inline table and
            // the node (keeping the node's own key, like the flat capture).
            if let Some(inline_len) = inline_ancestor_len(&proj.root, path) {
                let members = inline_member_entries(&idx, path);
                if !members.is_empty() {
                    let strip = if relative {
                        path.len() - 1 - inline_len
                    } else {
                        0
                    };
                    return members
                        .iter()
                        .map(|m| format!("{}\n", strip_key_prefix(m, strip).trim()))
                        .collect();
                }
            }
        }
    }
    let target = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t) {
        Some(t) => t,
        None => return String::new(),
    };
    match target {
        Target::Entry(n) | Target::ArrayElement(n) => {
            let strip = if relative {
                dotted_ancestor_prefix_len(&idx, &proj.root, path)
            } else {
                0
            };
            let s = strip_key_prefix(n, strip);
            if s.ends_with('\n') {
                s
            } else {
                format!("{s}\n")
            }
        }
        // A table / AoT entry: the section's source text (header + its lines).
        Target::Header(h) => section_text(syntax, path, h.index(), false),
        Target::AotEntry(h) => section_text(syntax, &[], h.index(), true),
        // The whole `[[x]]` group: all of its entries, in order.
        Target::AotGroup => match aot_group_span(syntax, path) {
            Some((start, end)) => {
                let els: Vec<_> = syntax.children_with_tokens().collect();
                els[start..end]
                    .iter()
                    .map(|e| match e {
                        NodeOrToken::Node(n) => n.to_string(),
                        NodeOrToken::Token(t) => t.text().to_string(),
                    })
                    .collect()
            }
            None => String::new(),
        },
        Target::Comment(_) => String::new(),
    }
}

/// The contiguous root-child span `[start, end)` covering every `[[x]]` entry of
/// the AoT group at `path`. `None` if the group's entries are interleaved with
/// other sections (so a single splice would touch foreign content) — the
/// whole-group serialize/replace then bails rather than corrupt.
fn aot_group_span(tree: &SyntaxNode, path: &[Seg]) -> Option<(usize, usize)> {
    let mut starts: Vec<usize> = tree
        .children_with_tokens()
        .enumerate()
        .filter_map(|(k, e)| match e {
            NodeOrToken::Node(n)
                if n.kind() == SyntaxKind::TABLE_ARRAY_HEADER && header_path(&n) == path =>
            {
                Some(k)
            }
            _ => None,
        })
        .collect();
    starts.sort_unstable();
    let first = *starts.first()?;
    // Contiguity: each entry's strict end must be exactly the next entry's start.
    for w in starts.windows(2) {
        if section_end_strict(tree, w[0]) != w[1] {
            return None;
        }
    }
    let end = section_end_strict(tree, *starts.last()?);
    Some((first, end))
}

/// The source text of a `[table]` / `[[aot]]` section starting at `header_idx`,
/// trimmed of a leading blank separator.
fn section_text(syntax: &SyntaxNode, t_path: &[Seg], header_idx: usize, strict: bool) -> String {
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
fn reparse_document(toml: &str) -> Result<SyntaxNode, MutateError> {
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    Ok(parse.into_syntax().clone_for_update())
}

/// Replace a scalar value in place (inline value edit). `toml` is a `key = <value>`
/// fragment (array elements use a synthetic `__elem__ = <value>`); only the scalar
/// token is swapped, so a trailing EOL comment and any surrounding array indent are
/// preserved.
/// Every flat-ROOT `ENTRY` element belonging to the dotted table at `path` (paths
/// strictly under it), in document order. Shared by the `[T/D]` block edit, delete
/// fan-out, and fragment serialization.
fn dotted_member_entries(idx: &CstIndex, path: &[Seg]) -> Vec<SyntaxNode> {
    let mut v: Vec<(usize, SyntaxNode)> = idx
        .iter()
        .filter_map(|(p, t)| match t {
            // Only *flat-root* entries: an entry nested inside an inline-table (or
            // array) value belongs to that value, not to the dotted table — skip it
            // so a `new_field = {x=1}` member never has its inner `x=1` pulled out.
            Target::Entry(n)
                if p.len() > path.len()
                    && p[..path.len()] == *path
                    && !n.ancestors().skip(1).any(|a| {
                        matches!(a.kind(), SyntaxKind::INLINE_TABLE | SyntaxKind::ARRAY)
                    }) =>
            {
                Some((n.index(), n.clone()))
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, n)| n).collect()
}

/// The prefix length of the **nearest inline-table ancestor** of `path` (the
/// largest `i < path.len()` whose node is an `InlineTable`), if any. A synthetic
/// `[T/D]` table whose members are dotted keys *inside* a `{ … }` has one — the
/// flat-ROOT machinery must not reach through it, so such paths route to the
/// inline-table helpers instead.
fn inline_ancestor_len(root: &Node, path: &[Seg]) -> Option<usize> {
    (1..path.len()).rev().find(|&i| {
        node_at(root, &path[..i]).is_some_and(|n| matches!(n.kind, NodeKind::InlineTable))
    })
}

/// The member `ENTRY`s of a synthetic `[T/D]` table nested inside an inline table:
/// every indexed entry strictly under `path`, in source order, skipping entries
/// that live inside another member's *value* (they belong to that member).
fn inline_member_entries(idx: &CstIndex, path: &[Seg]) -> Vec<SyntaxNode> {
    let mut v: Vec<SyntaxNode> = idx
        .iter()
        .filter_map(|(p, t)| match t {
            Target::Entry(n) if p.len() > path.len() && p[..path.len()] == *path => Some(n.clone()),
            _ => None,
        })
        .collect();
    v.sort_by_key(|n| n.text_range().start());
    let mut out: Vec<SyntaxNode> = Vec::new();
    for n in v {
        if !out.iter().any(|m| n.ancestors().skip(1).any(|a| &a == m)) {
            out.push(n);
        }
    }
    out
}

/// Whether the projected node at `path` has its own `[…]` header in the source.
/// A *headerless* table — a `[T/D]` dotted table, an implicit scope (only
/// `[a.sub]` was written), or the dotted side of a mixed table — opens no scope
/// of its own; its key segments live in its member lines instead.
fn has_own_header(idx: &CstIndex, path: &[Seg]) -> bool {
    idx.iter()
        .any(|(p, t)| p == path && matches!(t, Target::Header(_)))
}

/// Whether the node at `path` is a **headerless table**: a real `Table` projection
/// node, keyed (not an AoT entry), with no own `[…]` header. Such a table's key
/// prefix is carried by its member entries, so captures strip it and inserts
/// re-add it.
fn is_headerless_table(idx: &CstIndex, root: &Node, path: &[Seg]) -> bool {
    matches!(path.last(), Some(Seg::Key(_)))
        && node_at(root, path).is_some_and(|n| matches!(n.kind, NodeKind::Table))
        && !has_own_header(idx, path)
}

/// The number of contiguous **headerless-table proper ancestors** above the node
/// at `path` (counted from the deepest up, stopping at the first real scope).
/// This is exactly the count of leading key segments a copied fragment must drop
/// to become scope-relative: a `dotted.test.bool_true` leaf yields `2`, the
/// `test` subtable yields `1`.
fn dotted_ancestor_prefix_len(idx: &CstIndex, root: &Node, path: &[Seg]) -> usize {
    let mut count = 0;
    for k in (1..path.len()).rev() {
        if is_headerless_table(idx, root, &path[..k]) {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// The source text of `entry` with the first `strip` key segments (and the dots
/// that separate them) dropped from its `KEY` — so `dotted.test.bool_true = true`
/// with `strip = 2` renders `bool_true = true`. `strip == 0` is the entry verbatim.
fn strip_key_prefix(entry: &SyntaxNode, strip: usize) -> String {
    let full = entry.to_string();
    if strip == 0 {
        return full;
    }
    let key = match entry.children().find(|c| c.kind() == SyntaxKind::KEY) {
        Some(k) => k,
        None => return full,
    };
    let old_key = key.to_string();
    // The ENTRY begins with its KEY token text, so the new key text plus the rest of
    // the entry (the ` = value …` tail) reproduces a scope-relative line.
    let mut new_key = String::new();
    let mut seen_segs = 0usize;
    let mut started = false;
    for c in key.children_with_tokens() {
        if let NodeOrToken::Token(t) = &c {
            if is_key_seg(t.kind()) {
                seen_segs += 1;
                if seen_segs > strip {
                    started = true;
                }
            }
            if started {
                new_key.push_str(t.text());
            }
        }
    }
    format!("{new_key}{}", &full[old_key.len()..])
}

/// Detach an `ENTRY` together with its trailing `NEWLINE` (removing the whole line).
fn detach_entry_line(entry: &SyntaxNode) {
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
enum MemberSpan {
    /// A flat-ROOT dotted member entry (one line).
    Entry(SyntaxNode),
    /// The header of a member section, covering header..next header (strict).
    Section(SyntaxNode),
}

impl MemberSpan {
    fn start(&self) -> usize {
        match self {
            MemberSpan::Entry(n) | MemberSpan::Section(n) => n.index(),
        }
    }
}

/// The member spans of the table at `path`, in document order. Empty when `path`
/// addresses no root-level table content (e.g. a sub-table of an AoT entry,
/// whose path contains a `Seg::Index`).
fn table_member_spans(tree: &SyntaxNode, idx: &CstIndex, path: &[Seg]) -> Vec<MemberSpan> {
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
fn section_span_text(tree: &SyntaxNode, header: &SyntaxNode) -> String {
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
fn path_key_display(path: &[Seg]) -> String {
    path.iter()
        .filter_map(|s| match s {
            Seg::Key(k) => Some(quote_key_seg(k)),
            Seg::Index(_) => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The number of key segments written in an entry's own `KEY` text.
fn entry_key_seg_count(entry: &SyntaxNode) -> usize {
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
fn strip_section_header_prefix(frag: &SyntaxNode, strip: usize) {
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
fn table_fragment(
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
fn replace_table_spans(
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

/// Block-rewrite a `[T/D]` dotted table (`$EDITOR` on the table): remove all of its
/// member entries and splice the edited block in at the **first** member's position
/// (the consolidation the user opted into; the table projects at its first
/// definition). Scattered members are gathered; any standalone comments between
/// them stay put.
fn replace_dotted_table(
    tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
    toml: &str,
) -> Result<(), MutateError> {
    let members = dotted_member_entries(idx, path);
    let first = members.first().ok_or(MutateError::NotFound)?.clone();
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    // Remove the non-first members (whole lines); `detach` is position-independent.
    for m in &members[1..] {
        detach_entry_line(m);
    }
    // Replace the first member's slot (line) with the edited block.
    let i = first.index();
    let end = match first.next_sibling_or_token() {
        Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE => i + 2,
        _ => i + 1,
    };
    tree.splice_children(i..end, els);
    Ok(())
}

/// Block-rewrite a synthetic `[T/D]` table *inside an inline table*: remove every
/// member entry from the `{ … }` and splice the edited block's entries in at the
/// first member's slot — the inline mirror of `replace_dotted_table`. The block
/// keeps verbatim member keys (`x.y = 1`), must hold only single-line entries
/// (no `[…]` sections), and may drop or add members freely.
fn replace_inline_dotted_table(
    tree: &SyntaxNode,
    idx: &CstIndex,
    root: &Node,
    path: &[Seg],
    toml: &str,
) -> Result<(), MutateError> {
    let inline_len = inline_ancestor_len(root, path).ok_or(MutateError::NotFound)?;
    let members = inline_member_entries(idx, path);
    let first = members.first().ok_or(MutateError::NotFound)?.clone();
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax();
    if frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    }) {
        return Err(MutateError::Illegal(
            "a table cannot live inside an inline table".into(),
        ));
    }
    let new_entries: Vec<String> = frag
        .children()
        .filter(|n| n.kind() == SyntaxKind::ENTRY)
        .map(|n| n.to_string().trim().to_string())
        .collect();
    if new_entries.iter().any(|e| e.contains('\n')) {
        return Err(MutateError::Fragment(
            "inline-table members must be single-line".into(),
        ));
    }
    // The landing slot: the first member's position among the `{ … }`'s entries.
    let table = first
        .parent()
        .filter(|p| p.kind() == SyntaxKind::INLINE_TABLE)
        .ok_or(MutateError::Unsupported)?;
    let base = table
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .position(|c| c == first)
        .ok_or(MutateError::Unsupported)?;
    for m in members.iter().rev() {
        if let Some(parent) = m.parent() {
            delete_seq_element(&parent, m.index());
        }
    }
    let inline_path = &path[..inline_len];
    for (k, etext) in new_entries.iter().enumerate() {
        let eparse = taplo::parser::parse(etext);
        if let Some(e) = eparse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let efrag = eparse.into_syntax().clone_for_update();
        let (_, idx2) = walk(tree, "");
        inline_table_insert(&idx2, inline_path, base + k, &efrag)?;
    }
    Ok(())
}

fn replace_value(tree: &SyntaxNode, path: &[Seg], toml: &str) -> Result<(), MutateError> {
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

/// The `ARRAY` / `INLINE_TABLE` child node of a `VALUE`, if any.
fn struct_node(value: &SyntaxNode) -> Option<SyntaxNode> {
    value
        .children()
        .find(|n| matches!(n.kind(), SyntaxKind::ARRAY | SyntaxKind::INLINE_TABLE))
}

/// Replace the text of the standalone comment block at `path`. The block is the run
/// of `COMMENT` tokens (separated by single newlines) starting at the indexed
/// token; it is spliced with `text`'s lines, each validated to start with `#`.
fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
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
fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
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
        // Delete one `[[aot]]` entry: its header + entries up to the next header of
        // any kind (the next entry / table starts a new section).
        Target::AotEntry(header) => {
            let i = header.index();
            let end = section_end_strict(tree, i);
            tree.splice_children(i..end, vec![]);
            Ok(())
        }
    }
}

/// Like [`section_end`] but stops at the *next header of any kind* — used for a
/// single array-of-tables entry, where the following `[[x]]` is a separate entry.
fn section_end_strict(tree: &SyntaxNode, header_idx: usize) -> usize {
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
fn section_end(tree: &SyntaxNode, t_path: &[Seg], header_idx: usize) -> usize {
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
fn delete_seq_element(arr: &SyntaxNode, vi: usize) {
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
fn array_insert_comment(
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
fn entry_array(idx: &CstIndex, array_path: &[Seg]) -> Result<SyntaxNode, MutateError> {
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
fn array_make_multiline(arr: &SyntaxNode) -> Result<(), MutateError> {
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
fn comment_line_elements(
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
fn insert_comment(tree: &SyntaxNode, target: &InsTarget, text: &str) -> Result<(), MutateError> {
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
    let at = resolve_insert_at(tree, &proj.root, &idx, target)?;
    // `# …\n` per line, so each comment lands on its own line before the anchor.
    let frag_text = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
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

/// Insert a keyed node fragment (`key = val`, `[table]…`) at the projected
/// `target`. The fragment's first key is collision-checked against the parent
/// scope's existing keys. (Overwrite/Rename collision modes and bare array-element
/// inserts are deferred; `Cancel` and the no-collision path are handled.)
fn insert(
    tree: &SyntaxNode,
    target: &InsTarget,
    toml: &str,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let frag_text = if toml.ends_with('\n') {
        toml.to_string()
    } else {
        format!("{toml}\n")
    };

    let (proj, idx) = walk(tree, "");
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let parent_is_array = matches!(parent.kind, crate::model::node::NodeKind::Array);
    let parent_is_inline = matches!(parent.kind, crate::model::node::NodeKind::InlineTable);

    // Member spans of a *table* parent (empty for root / arrays / inline tables):
    // they drive the headerless-table insert rules below.
    let parent_spans = if matches!(parent.kind, NodeKind::Table)
        && matches!(target.parent.last(), Some(Seg::Key(_)))
    {
        table_member_spans(tree, &idx, &target.parent)
    } else {
        Vec::new()
    };
    let parent_entry_members = parent_spans
        .iter()
        .any(|s| matches!(s, MemberSpan::Entry(_)));
    let parent_section_members = parent_spans
        .iter()
        .any(|s| matches!(s, MemberSpan::Section(_)));
    let parent_headerless =
        !target.parent.is_empty() && is_headerless_table(&idx, &proj.root, &target.parent);
    // An *implicit* scope table (only `[a.sub]` sections were written, no dotted
    // members): an entry child gets the table's own `[a]` section synthesized at
    // its first definition instead of a dotted prefix.
    let implicit_scope_parent =
        parent_headerless && parent_section_members && !parent_entry_members;

    // Inserting into a headerless table (`[T/D]` dotted, or the dotted side of a
    // mixed table): the new entry has no header to live under, so it is written as
    // a dotted entry whose key carries the ancestor prefix (`x = v` into `a.b`
    // becomes `a.b.x = v`), placed next to its dotted siblings. The prefix is the
    // trailing run of headerless-table ancestors of the parent (down from the
    // nearest real scope / root); empty for a normal table.
    let dotted_prefix: Vec<String> = if implicit_scope_parent {
        Vec::new()
    } else {
        let mut segs = Vec::new();
        for i in (0..target.parent.len()).rev() {
            let anc_path = &target.parent[..=i];
            node_at(&proj.root, anc_path).ok_or(MutateError::NotFound)?;
            if !is_headerless_table(&idx, &proj.root, anc_path) {
                break;
            }
            if let Seg::Key(k) = &target.parent[i] {
                segs.push(k.clone());
            }
        }
        segs.reverse();
        segs
    };

    // D1 simple adaptation across container types:
    //  - into an ARRAY: a *keyless* bare value becomes the element as-is; a *keyed*
    //    fragment is wrapped as a `{ key = value }` inline-table element so the key is
    //    preserved (`key→{}`, below); a `[table]`/`[[aot]]` fragment is rejected.
    //  - into a TABLE/root we need a keyed entry: a bare element gets a synthesized
    //    `placeholder` key (`key+`).
    let (frag, synthesized_key) = parse_fragment_adapted(&frag_text, parent_is_array)?;

    // A `[table]`/`[[aot]]` **section** fragment is legal only into a real scope/root.
    // It cannot live inside an inline table, nor be nested under a synthetic `[T/D]`
    // dotted table (which opens no scope) — both surface a clear `Illegal`. Into a real
    // sub-scope its headers are re-prefixed with the destination path, so a `[T/S]`
    // table moved into another scope nests: `[a]` into `[b]` → `[b.a]`.
    let has_header = frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    });
    if has_header {
        if parent_is_inline {
            return Err(MutateError::Illegal(
                "a table cannot be inserted into an inline table".into(),
            ));
        }
        // A *pure* dotted table opens no scope a section could live in. A mixed
        // table (dotted members + existing sub-sections) does accept further
        // sub-sections — the TOML-spec `[fruit.apple.texture]` pattern.
        if parent_entry_members && !parent_section_members {
            return Err(MutateError::Illegal(
                "a scope table cannot be nested under a dotted table".into(),
            ));
        }
        if !target.parent.is_empty() {
            let prefix: Vec<String> = target
                .parent
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            prefix_section_headers(&frag, &prefix)?;
        }
    }

    if parent_is_array {
        // Into an array (no collision). A *keyless* bare value keeps its element form;
        // a *keyed* node is wrapped as a `{ key = value }` inline-table element so the
        // key is preserved (a keyed inline table becomes a nested inline table).
        // `[T/S]`/`[A/T]` headers are already rejected by `parse_fragment_adapted`.
        let element = if synthesized_key {
            frag
        } else {
            wrap_keyed_as_inline_element(&frag_text)?
        };
        return array_insert(&idx, &target.parent, target.index, &element);
    }

    // A header-less **multi-entry** fragment (a copied `[T/D]` table block, whose
    // members are several flat dotted entries) is inserted one entry at a time, so the
    // dotted prefix and the per-leaf collision check apply to each member — a single
    // splice would only re-key the first (and an inline-table destination would drop
    // every member but the first). A `[table]`/`[[aot]]` section keeps its entries
    // together (they belong under the header) and is never split. The landing slot is
    // held by a stable **anchor path** (the first non-comment child at/after the
    // target index): inserted dotted members can merge into one projected child, so a
    // plain `index + k` would drift past later siblings after the first insert.
    let top_entries: Vec<SyntaxNode> = frag
        .children()
        .filter(|n| n.kind() == SyntaxKind::ENTRY)
        .collect();
    if !has_header && top_entries.len() > 1 {
        let anchor_path: Option<Vec<Seg>> = parent
            .children
            .iter()
            .skip(target.index)
            .find(|c| !matches!(c.kind, NodeKind::Comment(_)))
            .map(|c| c.path.clone());
        for e in &top_entries {
            let entry_text = format!("{}\n", e.to_string().trim());
            let index = {
                let (proj2, _) = walk(tree, "");
                let parent2 = node_at(&proj2.root, &target.parent).ok_or(MutateError::NotFound)?;
                match &anchor_path {
                    Some(ap) => parent2
                        .children
                        .iter()
                        .position(|c| &c.path == ap)
                        .unwrap_or(parent2.children.len()),
                    None => parent2.children.len(),
                }
            };
            insert(
                tree,
                &InsTarget {
                    parent: target.parent.clone(),
                    index,
                },
                &entry_text,
                on_collision,
            )?;
        }
        return Ok(());
    }

    // An inline-table destination: the parent is the `{ … }` itself, or a synthetic
    // `[T/D]` table nested inside one (its members are `x.y = 1` dotted keys). The
    // flat-ROOT splice machinery below must not reach through a `{ … }`, so both
    // route to `inline_table_insert` — the synthetic case with the key re-prefixed
    // scope-relative (`q = 9` into `t.x` becomes the member `x.q = 9`). Collision is
    // exact full path (like the flat path below): a dotted member sharing only a
    // prefix with an existing `[T/D]` chain merges instead of colliding.
    let inline_len = if parent_is_inline {
        Some(target.parent.len())
    } else if matches!(parent.kind, NodeKind::Table) {
        inline_ancestor_len(&proj.root, &target.parent)
    } else {
        None
    };
    if let Some(inline_len) = inline_len {
        if has_header {
            return Err(MutateError::Illegal(
                "a table cannot be inserted into an inline table".into(),
            ));
        }
        let prefix: Vec<String> = target.parent[inline_len..]
            .iter()
            .filter_map(|s| match s {
                Seg::Key(k) => Some(k.clone()),
                Seg::Index(_) => None,
            })
            .collect();
        if !prefix.is_empty() {
            prefix_entry_key(&frag, &prefix)?;
        }
        let new_segs = fragment_key_segs(&frag);
        if new_segs.is_empty() {
            return Err(MutateError::Fragment("fragment has no key".into()));
        }
        let mut full = target.parent[..inline_len].to_vec();
        full.extend(new_segs.iter().cloned().map(Seg::Key));
        if node_at(&proj.root, &full).is_some() {
            return Err(MutateError::Collision(new_segs.join(".")));
        }
        let raw_index = inline_raw_member_index(&idx, parent, target.index);
        return inline_table_insert(&idx, &target.parent[..inline_len], raw_index, &frag);
    }

    let frag_segs = fragment_key_segs(&frag);
    if frag_segs.is_empty() {
        return Err(MutateError::Fragment("fragment has no key".into()));
    }
    // A synthesized `placeholder` key is auto-renamed on collision — the user never
    // chose it, so a clash shouldn't surface as a prompt/error.
    let on_collision = if synthesized_key {
        OnCollision::Rename
    } else {
        on_collision
    };
    // Within a table the entry run and the sub-section run partition the layout
    // (D5). Targeting *this table* means the position can always be honored at the
    // nearest legal slot, so clamp instead of rejecting: an entry-like fragment
    // lands no further than the partition split (the end of the entry run — for a
    // headerless table, after the last dotted member; never inside a section), a
    // header-like one no earlier than it. The Root keeps explicit-position
    // semantics (out-of-partition inserts there still surface `Illegal`).
    let split = parent
        .children
        .iter()
        .position(|c| {
            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                && c.format != Format::Dotted
        })
        .unwrap_or(parent.children.len());
    let parent_is_table =
        matches!(parent.kind, NodeKind::Table) && matches!(target.parent.last(), Some(Seg::Key(_)));
    let eff_index = if parent_is_table && !has_header {
        target.index.min(split)
    } else if parent_is_table && has_header {
        target.index.max(split)
    } else {
        target.index
    };
    // Carry the dotted ancestor prefix onto the key *before* the collision check, so
    // an Overwrite/splice keeps the destination prefix. Collision is decided on
    // `frag_segs` (the key relative to the parent), which equals the leaf's projected
    // path tail regardless of how the key is written.
    check_partition(parent, &frag, eff_index)?;
    if !dotted_prefix.is_empty() {
        prefix_entry_key(&frag, &dotted_prefix)?;
    }
    // Collision is **exact full path** (`target.parent ++ frag_segs`): dotted siblings
    // that merely share a prefix (`a.x` beside `a.y`) merge into one `[T/D]` table
    // instead of colliding — only an identical full key clashes. A header fragment
    // bound for a sub-scope was already re-prefixed with the destination path
    // (`prefix_section_headers`), so its key segments are absolute from the root —
    // prepending `target.parent` again would check a phantom `b.b.a` path and let a
    // duplicate `[b.a]` through.
    let header_is_absolute = has_header && !target.parent.is_empty();
    let full_path = |segs: &[String]| -> Vec<Seg> {
        let mut p = if header_is_absolute {
            Vec::new()
        } else {
            target.parent.clone()
        };
        p.extend(segs.iter().cloned().map(Seg::Key));
        p
    };
    if node_at(&proj.root, &full_path(&frag_segs)).is_some() {
        match on_collision {
            OnCollision::Cancel => return Err(MutateError::Collision(frag_segs.join("."))),
            OnCollision::Overwrite => {
                // Replace the colliding leaf's element in place (keeps position).
                let victim_path = full_path(&frag_segs);
                let velem = match idx.iter().find(|(p, _)| p == &victim_path).map(|(_, t)| t) {
                    Some(Target::Entry(n)) => n.clone(),
                    _ => return Err(MutateError::Unsupported),
                };
                let vparent = velem.parent().ok_or(MutateError::NotFound)?;
                let mut new_els: Vec<_> = frag.children_with_tokens().collect();
                while matches!(new_els.last(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE)
                {
                    new_els.pop();
                }
                for e in &new_els {
                    e.detach();
                }
                let i = velem.index();
                vparent.splice_children(i..i + 1, new_els);
                return Ok(());
            }
            OnCollision::Rename => {
                // Append _2, _3, … to the **last** segment until the full path is free.
                let base = frag_segs.last().cloned().unwrap_or_default();
                let mut segs = frag_segs.clone();
                let mut n = 2;
                loop {
                    let last = segs.len() - 1;
                    segs[last] = format!("{base}_{n}");
                    if node_at(&proj.root, &full_path(&segs)).is_none() {
                        break;
                    }
                    n += 1;
                }
                rewrite_last_key(&frag, segs.last().unwrap())?;
            }
        }
    }

    let at = if implicit_scope_parent && !has_header {
        // Synthesize the table's own `[…]` section at its first definition and
        // put the entry right under the new header.
        let parsed = taplo::parser::parse(&format!("[{}]\n", path_key_display(&target.parent)));
        if let Some(e) = parsed.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let header = parsed.into_syntax().clone_for_update();
        let hdr_els: Vec<_> = header.children_with_tokens().collect();
        for e in &hdr_els {
            e.detach();
        }
        let n = hdr_els.len();
        let at = parent_spans.first().map(|s| s.start()).unwrap_or(0);
        tree.splice_children(at..at, hdr_els);
        at + n
    } else if parent_headerless && !has_header && parent_section_members && eff_index >= split {
        // Mixed table, append: the entry stays with the dotted-member run (after
        // its last line), never inside a member section.
        let last = parent_spans
            .iter()
            .filter_map(|s| match s {
                MemberSpan::Entry(e) => Some(e.clone()),
                MemberSpan::Section(_) => None,
            })
            .next_back()
            .ok_or(MutateError::Unsupported)?;
        extend_over_newline(tree, last.index() + 1)
    } else {
        resolve_insert_at(
            tree,
            &proj.root,
            &idx,
            &InsTarget {
                parent: target.parent.clone(),
                index: eff_index,
            },
        )?
    };
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    tree.splice_children(at..at, els);
    Ok(())
}

/// The synthesized key for a bare element pasted into a table (`key+`, D1).
const PLACEHOLDER_KEY: &str = "placeholder";

/// Parse a fragment for insertion into a table (`into_array == false`) or an array
/// (`true`), adapting across container types (D1 simple adaptation). Returns the
/// parsed fragment and whether a `placeholder` key was synthesized.
///
/// A fragment that parses as a TOML document is used as-is (a keyed entry, or a
/// `[table]`/`[[aot]]` section). A fragment that does not (a **bare array-element
/// value** like `42` or `{ a = 1 }`) is wrapped as `placeholder = <value>` so it
/// becomes a keyed entry — for a table dest the key is kept (`key+`); for an array
/// dest the synthesized key marks the value as keyless, so it stays a bare element
/// (a *real* keyed fragment is instead wrapped as `{ key = value }` by the caller to
/// preserve its key). A `[table]`/`[[aot]]` section cannot become an array element
/// (a hard coerce), so it is rejected for an array.
fn parse_fragment_adapted(
    frag_text: &str,
    into_array: bool,
) -> Result<(SyntaxNode, bool), MutateError> {
    let parse = taplo::parser::parse(frag_text);
    if parse.errors.is_empty() {
        let node = parse.into_syntax().clone_for_update();
        if into_array
            && node.descendants().any(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
                )
            })
        {
            return Err(MutateError::Illegal(
                "a table cannot be pasted as an array element".into(),
            ));
        }
        return Ok((node, false));
    }
    // Not a standalone document — try treating it as a bare value with a key.
    let wrapped = format!("{PLACEHOLDER_KEY} = {}\n", frag_text.trim_end());
    let parse2 = taplo::parser::parse(&wrapped);
    match parse2.errors.first() {
        Some(e) => Err(MutateError::Fragment(e.to_string())),
        None => Ok((parse2.into_syntax().clone_for_update(), true)),
    }
}

/// Wrap a keyed entry fragment (`k = v`) as a bare inline-table value (`__w = { k = v }`)
/// so inserting it into an array preserves the key as a `{ k = v }` element (a keyed
/// inline-table value becomes a nested inline table). `array_insert` extracts the first
/// VALUE descendant, which is the wrapping inline table. A multi-line value (multiline
/// string/array) can't live on the inline table's single line, so it surfaces as a
/// `Fragment` error.
fn wrap_keyed_as_inline_element(frag_text: &str) -> Result<SyntaxNode, MutateError> {
    let entry = frag_text.trim();
    let parse = taplo::parser::parse(&format!("__w = {{ {entry} }}\n"));
    match parse.errors.first() {
        Some(e) => Err(MutateError::Fragment(e.to_string())),
        None => Ok(parse.into_syntax().clone_for_update()),
    }
}

/// If `value_text` is a **single-entry** inline table (`{ k = v }`), return its inner
/// `k = v`; else `None`. The inverse of `wrap_keyed_as_inline_element`: moving such an
/// element out of an array into a table unwraps it back to a keyed entry. A multi-key
/// inline table (or any other value) returns `None` and gets a synthesized key instead.
fn unwrap_single_key_inline(value_text: &str) -> Option<String> {
    let parse = taplo::parser::parse(&format!("__w = {}\n", value_text.trim()));
    if !parse.errors.is_empty() {
        return None;
    }
    let it = parse
        .into_syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::INLINE_TABLE)?;
    let mut entries = it.children().filter(|c| c.kind() == SyntaxKind::ENTRY);
    let first = entries.next()?;
    if entries.next().is_some() {
        return None; // multi-key
    }
    Some(first.to_string().trim().to_string())
}

/// D5 (TOML table-capture): within a table/root the legal layout is partitioned —
/// a leading region (scalars / arrays / inline tables) then a header region
/// (sub-`[table]` / `[[aot]]`). A `[table]`/`[[aot]]` header before the keys above
/// it would capture them; a plain key after a header would be re-keyed into that
/// section. So a header-like fragment may only land at index `>= split`, a leaf-like
/// one only at index `<= split`, where `split` is the parent's first sub-table/AoT
/// child index (or `len` when it has none).
fn check_partition(parent: &Node, frag: &SyntaxNode, index: usize) -> Result<(), MutateError> {
    use crate::model::node::NodeKind;
    let len = parent.children.len();
    // Clamp the append sentinel (callers pass an out-of-range index to mean "end").
    let index = index.min(len);
    // A `[T/D]` dotted table is not a capturing header (it opens no scope), so it
    // is not a partition boundary — a scalar may sit after it.
    let split = parent
        .children
        .iter()
        .position(|c| {
            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                && c.format != Format::Dotted
        })
        .unwrap_or(len);
    let header_like = frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    });
    if header_like {
        if index < split {
            return Err(MutateError::Illegal(
                "a table here would capture the keys above it".into(),
            ));
        }
    } else if index > split {
        return Err(MutateError::Illegal(
            "a key here would be captured by the table above it".into(),
        ));
    }
    Ok(())
}

/// Insert a bare value into the array at `array_path`, at element `index` (or
/// appended). Uses single-line `, ` separators; multiline-array spacing is rough.
fn array_insert(
    idx: &CstIndex,
    array_path: &[Seg],
    index: usize,
    frag: &SyntaxNode,
) -> Result<(), MutateError> {
    let arr = match idx.iter().find(|(p, _)| p == array_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::ARRAY)
            .ok_or(MutateError::Unsupported)?,
        _ => return Err(MutateError::Unsupported),
    };
    let new_val = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))?;
    new_val.detach();

    let els: Vec<_> = arr.children_with_tokens().collect();
    let value_pos: Vec<usize> = els
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, NodeOrToken::Node(n) if n.kind() == SyntaxKind::VALUE))
        .map(|(i, _)| i)
        .collect();

    if index < value_pos.len() {
        let at = value_pos[index];
        let (comma, space) = array_sep();
        arr.splice_children(at..at, vec![NodeOrToken::Node(new_val), comma, space]);
    } else if let Some(&last) = value_pos.last() {
        let (comma, space) = array_sep();
        arr.splice_children(
            last + 1..last + 1,
            vec![comma, space, NodeOrToken::Node(new_val)],
        );
    } else {
        // Empty array: insert before the closing bracket.
        let be = els
            .iter()
            .position(|e| matches!(e, NodeOrToken::Token(t) if t.kind() == SyntaxKind::BRACKET_END))
            .ok_or(MutateError::Unsupported)?;
        arr.splice_children(be..be, vec![NodeOrToken::Node(new_val)]);
    }
    Ok(())
}

/// Insert a keyed `ENTRY` into the inline table at `table_path`, at member `index`
/// (or appended). taplo bakes the closing `}`'s leading whitespace into the last
/// entry, so token surgery is brittle — instead the table is rebuilt from its
/// members' verbatim source with normalized `, ` separators (`{ … }` padding), each
/// existing member kept byte-for-byte. An empty `{}` becomes `{ k = v }`.
fn inline_table_insert(
    idx: &CstIndex,
    table_path: &[Seg],
    index: usize,
    frag: &SyntaxNode,
) -> Result<(), MutateError> {
    let it = match idx.iter().find(|(p, _)| p == table_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::INLINE_TABLE)
            .ok_or(MutateError::Unsupported)?,
        _ => return Err(MutateError::Unsupported),
    };
    let new_entry = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ENTRY)
        .ok_or_else(|| MutateError::Fragment("fragment has no entry".into()))?;

    let mut texts: Vec<String> = it
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .map(|e| e.to_string().trim().to_string())
        .collect();
    let new_text = new_entry.to_string().trim().to_string();
    if index < texts.len() {
        texts.insert(index, new_text);
    } else {
        texts.push(new_text);
    }
    let built = format!("__v__ = {{ {} }}\n", texts.join(", "));
    let parse = taplo::parser::parse(&built);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let new_it = parse
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::INLINE_TABLE)
        .ok_or(MutateError::Unsupported)?;
    new_it.detach();
    let value = it.parent().ok_or(MutateError::Unsupported)?;
    let i = it.index();
    value.splice_children(i..i + 1, vec![NodeOrToken::Node(new_it)]);
    Ok(())
}

/// A fresh detached `,` + ` ` pair for array separators (parsed from a sample).
fn array_sep() -> (taplo::syntax::SyntaxElement, taplo::syntax::SyntaxElement) {
    let frag = taplo::parser::parse("x = [0, 0]\n")
        .into_syntax()
        .clone_for_update();
    let arr = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARRAY)
        .expect("sample array");
    let comma = arr
        .children_with_tokens()
        .find(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA))
        .expect("comma");
    let space = arr
        .children_with_tokens()
        .find(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE))
        .expect("space");
    comma.detach();
    space.detach();
    (comma, space)
}

/// Swap the **last** key-segment token of a node fragment to `new_seg` (`a.b` →
/// `a.b_2`), used to de-collide an entry on `OnCollision::Rename` (for a bare key the
/// last segment is the only one).
fn rewrite_last_key(frag: &SyntaxNode, new_seg: &str) -> Result<(), MutateError> {
    let key = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    let last = key
        .children_with_tokens()
        .filter_map(key_seg_token)
        .last()
        .ok_or_else(|| MutateError::Fragment("fragment key has no segment".into()))?;
    let parse = taplo::parser::parse(&format!("{new_seg} = 0\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let nk = parse
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .and_then(|k| k.children_with_tokens().find_map(key_seg_token))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    nk.detach();
    let i = last.index();
    key.splice_children(i..i + 1, vec![NodeOrToken::Token(nk)]);
    Ok(())
}

/// Prefix the fragment's (single-segment) key with a dotted ancestor path, so an
/// insert into a synthetic `[T/D]` table is written as a dotted entry: `x = v`
/// with prefix `[a, b]` becomes `a.b.x = v`. Replaces the whole `KEY` node,
/// preserving the original final segment's source (quoting intact); non-bare
/// prefix segments are re-quoted.
fn prefix_entry_key(frag: &SyntaxNode, prefix: &[String]) -> Result<(), MutateError> {
    let key = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    let joined = prefix
        .iter()
        .map(|s| quote_key_seg(s))
        .collect::<Vec<_>>()
        .join(".");
    // Borrow correctly-tokenized `<prefix>.` segments (idents + dots) from a
    // throwaway parse, then splice them in front of the original key — preserving
    // the original final segment's tokens (and the entry's spacing) verbatim.
    let parsed = taplo::parser::parse(&format!("{joined}.__seg__ = 0\n"));
    if let Some(e) = parsed.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let pkey = parsed
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    let toks: Vec<_> = pkey.children_with_tokens().collect();
    let last = toks
        .iter()
        .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    let prefix_tokens = &toks[..last];
    for e in prefix_tokens {
        e.detach();
    }
    key.splice_children(0..0, prefix_tokens.to_vec());
    Ok(())
}

/// Prefix every `[table]`/`[[aot]]` header in a section fragment with `prefix`, so a
/// `[T/S]` scope table moved into another scope nests: `[a]` (with a nested `[a.sub]`)
/// dropped under `[b]` becomes `[b.a]` (and `[b.a.sub]`). Mirrors `prefix_entry_key`'s
/// front-splice, applied to each header's `KEY` (a fresh token copy per header, since a
/// token can only live in one tree).
fn prefix_section_headers(frag: &SyntaxNode, prefix: &[String]) -> Result<(), MutateError> {
    if prefix.is_empty() {
        return Ok(());
    }
    let joined = prefix
        .iter()
        .map(|s| quote_key_seg(s))
        .collect::<Vec<_>>()
        .join(".");
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
        let key = h
            .children()
            .find(|c| c.kind() == SyntaxKind::KEY)
            .ok_or_else(|| MutateError::Fragment("header has no key".into()))?;
        let parsed = taplo::parser::parse(&format!("{joined}.__seg__ = 0\n"));
        if let Some(e) = parsed.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let pkey = parsed
            .into_syntax()
            .clone_for_update()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::KEY)
            .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
        let toks: Vec<_> = pkey.children_with_tokens().collect();
        let last = toks
            .iter()
            .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
            .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
        let prefix_tokens: Vec<_> = toks[..last].to_vec();
        for e in &prefix_tokens {
            e.detach();
        }
        key.splice_children(0..0, prefix_tokens);
    }
    Ok(())
}

/// A key segment as written in source: bare if it is a legal bare key, else a
/// basic-quoted string.
fn quote_key_seg(s: &str) -> String {
    let bare = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Toggle the node at `path` between live and commented-out. A live entry becomes a
/// `# …` comment of its source line; a comment is uncommented by stripping the `#`
/// and reparsing as live TOML. (Table/AoT subtree remark is deferred.)
fn remark(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
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

/// Whether a header node is a `[[aot]]` entry (vs a `[table]`).
fn idx_target_is_aot(header: &SyntaxNode) -> bool {
    header.kind() == SyntaxKind::TABLE_ARRAY_HEADER
}

/// Move `sources` to `target`, atomically (the caller commits the clone only on
/// success). Comments are independent CST nodes, so a move repositions only the
/// named nodes — adjacent comments stay put with no special handling. Entry,
/// `[table]` and **array-element** sources are supported; AoT-entry sources are
/// deferred (they would need append-not-collide insert semantics for `[[x]]`).
fn move_nodes(
    tree: &SyntaxNode,
    sources: &[Vec<Seg>],
    target: &InsTarget,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");

    // Capture each source's source text before any removal.
    let mut frags: Vec<String> = Vec::new();
    for p in sources {
        // A table — `[T/D]`, `[T/S]` (scattered or not), implicit, or mixed — is an
        // open set of member spans: capture them all, scope-relative (entry keys
        // drop the headerless-ancestor prefix, headers drop the ancestor path), so
        // the re-insert re-prefixes only for the destination. A pure `[T/D]` table
        // fans out to one fragment per member line so the per-leaf collision check
        // applies; a sectioned capture stays one fragment (its entries belong under
        // their headers). The source side is removed by `delete`, which fans out
        // over the same spans.
        if node_at(&proj.root, p).is_some_and(|n| matches!(n.kind, NodeKind::Table))
            && matches!(p.last(), Some(Seg::Key(_)))
        {
            let spans = table_member_spans(tree, &idx, p);
            if spans.iter().any(|s| matches!(s, MemberSpan::Section(_))) {
                if let Some(text) = table_fragment(tree, &idx, &proj.root, p, true) {
                    frags.push(text);
                    continue;
                }
            } else if !spans.is_empty() {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                for s in &spans {
                    if let MemberSpan::Entry(m) = s {
                        frags.push(strip_key_prefix(m, strip));
                    }
                }
                continue;
            } else if let Some(inline_len) = inline_ancestor_len(&proj.root, p) {
                // A synthetic `[T/D]` table *inside an inline table* fans out to
                // its `{ … }` member entries, captured scope-relative (drop the
                // segments between the inline table and the node, keep its own
                // key) — the source side is removed by `delete`'s inline fan-out.
                let members = inline_member_entries(&idx, p);
                if !members.is_empty() {
                    let strip = p.len() - 1 - inline_len;
                    for m in &members {
                        frags.push(format!("{}\n", strip_key_prefix(m, strip).trim()));
                    }
                    continue;
                }
            }
        }
        let t = match idx.iter().find(|(ip, _)| ip == p).map(|(_, t)| t.clone()) {
            Some(t) => t,
            None => return Err(MutateError::NotFound),
        };
        match t {
            // Scope-relative capture: drop the source's dotted-ancestor prefix so the
            // re-insert re-prefixes only for the destination (matching copy/paste).
            Target::Entry(n) => {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                frags.push(strip_key_prefix(&n, strip));
            }
            Target::Header(h) => frags.push(section_text(tree, p, h.index(), false)),
            // Moving an array element out: into another array it stays a bare element;
            // into a table/root a single-key inline table `{ k = v }` unwraps to `k = v`
            // (key preserved), anything else gets a synthesized `placeholder` key on
            // insert. The destination format is then applied by `insert` (dotted prefix,
            // inline-table splice, …).
            Target::ArrayElement(value) => {
                let text = value.to_string();
                let dest_is_array = node_at(&proj.root, &target.parent)
                    .map(|n| matches!(n.kind, crate::model::node::NodeKind::Array))
                    .unwrap_or(false);
                let frag = match (dest_is_array, unwrap_single_key_inline(&text)) {
                    (false, Some(kv)) => format!("{kv}\n"),
                    _ => format!("{}\n", text.trim()),
                };
                frags.push(frag);
            }
            _ => return Err(MutateError::Unsupported),
        }
    }

    // Resolve a stable anchor — the first child at/after the target index that is
    // not itself a source — to insert before (its keyed path is stable across the
    // source removals); `None` means append.
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let anchor_path: Option<Vec<Seg>> = parent
        .children
        .iter()
        .skip(target.index)
        .find(|c| {
            !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                && !sources.contains(&c.path)
        })
        .map(|c| c.path.clone());

    // Delete sources (longest path first keeps shallower paths valid).
    let mut ordered: Vec<&Vec<Seg>> = sources.iter().collect();
    ordered.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for p in ordered {
        delete(tree, p)?;
    }

    // Re-insert before the anchor's current position (or append), in order.
    for frag in frags {
        let index = {
            let (proj2, _) = walk(tree, "");
            let parent2 = node_at(&proj2.root, &target.parent).ok_or(MutateError::NotFound)?;
            match &anchor_path {
                Some(ap) => parent2
                    .children
                    .iter()
                    .position(|c| &c.path == ap)
                    .unwrap_or(parent2.children.len()),
                None => parent2.children.len(),
            }
        };
        insert(
            tree,
            &InsTarget {
                parent: target.parent.clone(),
                index,
            },
            &frag,
            on_collision,
        )?;
    }
    Ok(())
}

/// Build a single `COMMENT` token from `text` (a `# …` line).
fn first_comment_token(text: &str) -> Result<SyntaxToken, MutateError> {
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

/// Rename the key at `path` to `new_key`, swapping the last key-segment token in
/// place (position/decor preserved). Collides if a sibling already has the resulting
/// display key.
fn rename(tree: &SyntaxNode, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    // Build the replacement key token from a validated fragment.
    let parse = taplo::parser::parse(&format!("{new_key} = 0\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let nk_root = parse.into_syntax().clone_for_update();
    // All tokens of the new key (segments + dots), so a multi-segment new key turns
    // a plain key dotted in place (`foo` → `foo.x`, making a `[T/D]` table). Trim any
    // trailing whitespace taplo lexes into the KEY so the swap stays tight.
    let mut nk_tokens: Vec<_> = nk_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?
        .children_with_tokens()
        .collect();
    let last_seg = nk_tokens
        .iter()
        .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    nk_tokens.truncate(last_seg + 1);

    let (proj, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    let key_node = match &target {
        Target::Entry(n) | Target::Header(n) | Target::AotEntry(n) => n
            .children()
            .find(|c| c.kind() == SyntaxKind::KEY)
            .ok_or(MutateError::NotFound)?,
        _ => return Err(MutateError::Unsupported),
    };

    // Collision: compute the resulting display key and compare against siblings.
    if let Some((parent, node)) = find_parent(&proj.root, path) {
        let mut segs: Vec<&str> = node.key.split('.').collect();
        if let Some(last) = segs.last_mut() {
            *last = new_key;
        }
        let new_display = segs.join(".");
        if parent.children.iter().any(|c| {
            !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                && c.path != *path
                && c.key == new_display
        }) {
            return Err(MutateError::Collision(new_key.to_string()));
        }
    }

    // Replace the last key-segment token (the displayed/renamed segment) with the
    // new key's tokens — one segment for a plain rename, several to introduce dots.
    let last_tok = key_node
        .children_with_tokens()
        .filter_map(|c| c.into_token().filter(|t| is_key_seg(t.kind())))
        .last()
        .ok_or(MutateError::NotFound)?;
    let i = last_tok.index();
    for t in &nk_tokens {
        t.detach();
    }
    key_node.splice_children(i..i + 1, nk_tokens);
    Ok(())
}

fn is_key_seg(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::IDENT
            | SyntaxKind::IDENT_WITH_GLOB
            | SyntaxKind::STRING
            | SyntaxKind::STRING_LITERAL
    )
}

fn key_seg_token(c: taplo::syntax::SyntaxElement) -> Option<SyntaxToken> {
    c.into_token().filter(|t| is_key_seg(t.kind()))
}

/// The parent node of the node at `path`, plus the node itself, in the projection.
fn find_parent<'a>(root: &'a Node, path: &[Seg]) -> Option<(&'a Node, &'a Node)> {
    fn rec<'a>(n: &'a Node, path: &[Seg]) -> Option<(&'a Node, &'a Node)> {
        for c in &n.children {
            if c.path == path {
                return Some((n, c));
            }
            if let Some(r) = rec(c, path) {
                return Some(r);
            }
        }
        None
    }
    rec(root, path)
}

/// Translate a **projected child index** of an inline table into a **raw `{ … }`
/// member (`ENTRY`) index** for `inline_table_insert`. With dotted members
/// decomposed into synthetic `[T/D]` chains, a projected child can cover several
/// raw members (and they need not be contiguous) — anchor on its earliest one.
/// Out of range (or no resolvable member) means append.
fn inline_raw_member_index(idx: &CstIndex, parent: &Node, proj_index: usize) -> usize {
    fn earliest_entry(idx: &CstIndex, n: &Node) -> Option<SyntaxNode> {
        let own = idx.iter().find_map(|(p, t)| match t {
            Target::Entry(e) if p == &n.path => Some(e.clone()),
            _ => None,
        });
        let kids = n.children.iter().filter_map(|c| earliest_entry(idx, c));
        own.into_iter().chain(kids).min_by_key(|e| e.index())
    }
    let Some(child) = parent.children.get(proj_index) else {
        return usize::MAX;
    };
    let Some(entry) = earliest_entry(idx, child) else {
        return usize::MAX;
    };
    let Some(table) = entry
        .parent()
        .filter(|p| p.kind() == SyntaxKind::INLINE_TABLE)
    else {
        return usize::MAX;
    };
    table
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .position(|c| c == entry)
        .unwrap_or(usize::MAX)
}

/// All key segments of the fragment's first `KEY` (`a.b.c = v` → `["a","b","c"]`),
/// quotes stripped. A bare key yields one segment; a dotted key yields the chain —
/// used to compute the inserted leaf's full projected path for collision detection.
fn fragment_key_segs(root: &SyntaxNode) -> Vec<String> {
    let Some(key) = root.descendants().find(|n| n.kind() == SyntaxKind::KEY) else {
        return Vec::new();
    };
    key.children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::IDENT | SyntaxKind::IDENT_WITH_GLOB => Some(t.text().to_string()),
                SyntaxKind::STRING | SyntaxKind::STRING_LITERAL => {
                    let s = t.text().trim();
                    Some(s[1..s.len().saturating_sub(1)].to_string())
                }
                _ => None,
            },
            NodeOrToken::Node(_) => None,
        })
        .collect()
}

/// Map a projected insertion `target` (`parent` path + child `index`) to a splice
/// position among the flat ROOT's children. Handles inserting *before* the child
/// currently at `index`, and appending at the end of the document or a simple table
/// scope. (Appending into a table that contains sub-tables is deferred.)
fn resolve_insert_at(
    tree: &SyntaxNode,
    root: &Node,
    idx: &CstIndex,
    target: &InsTarget,
) -> Result<usize, MutateError> {
    let parent = node_at(root, &target.parent).ok_or(MutateError::NotFound)?;
    if target.index < parent.children.len() {
        // Insert before the child currently at `index`. A synthetic `[T/D]` table
        // has no backing element of its own, so anchor on the physical start of its
        // subtree (its first dotted member line) — otherwise inserting *before* a
        // `[T/D]` table would fail as `Unsupported`.
        let anchor = &parent.children[target.index];
        return node_start_root_index(idx, anchor).ok_or(MutateError::Unsupported);
    }
    // Append at the end of the parent's scope.
    if target.parent.is_empty() {
        return Ok(tree.children_with_tokens().count());
    }
    // A table scope: after the last element belonging to it (header + children),
    // consuming the following newline so the insert starts on a fresh line.
    let header_pos = idx
        .iter()
        .find(|(p, t)| p == &target.parent && matches!(t, Target::Header(_)))
        .and_then(|(_, t)| match t {
            Target::Header(n) => Some(n.index()),
            _ => None,
        });
    // A synthetic `[T/D]` dotted table has no header — anchor on its children
    // (their dotted entries), which always exist. `node_last_root_index` descends
    // into any synthetic-table child so appending lands after its *last* member
    // (not before it).
    let mut last = match header_pos {
        Some(h) => h,
        None => parent
            .children
            .iter()
            .filter_map(|c| node_last_root_index(idx, c))
            .max()
            .ok_or(MutateError::Unsupported)?,
    };
    for child in &parent.children {
        if let Some(p) = node_last_root_index(idx, child) {
            last = last.max(p);
        }
    }
    Ok(extend_over_newline(tree, last + 1))
}

/// The ROOT-child index where `node`'s physical source *ends*: the largest start
/// index among its own element and all descendants. The dual of
/// [`node_start_root_index`] — used to append *after* a node whose subtree may include
/// a synthetic `[T/D]` table with no element of its own.
fn node_last_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
    let own = element_root_index(idx, node);
    let deepest = node
        .children
        .iter()
        .filter_map(|c| node_last_root_index(idx, c))
        .max();
    own.into_iter().chain(deepest).max()
}

/// The ROOT-child index where `node`'s physical source *begins*: its own backing
/// element if it has one, else (for a synthetic `[T/D]` table, which has none) the
/// smallest start index among its descendants — i.e. its first member line. Used to
/// anchor an "insert before this node" against a node that may be synthetic.
fn node_start_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
    if let Some(i) = element_root_index(idx, node) {
        return Some(i);
    }
    node.children
        .iter()
        .filter_map(|c| node_start_root_index(idx, c))
        .min()
}

/// The ROOT-child index of the syntax element backing `node` (an entry, header, AoT
/// entry, or comment — all flat ROOT children).
fn element_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
    let t = idx.iter().find(|(p, _)| p == &node.path).map(|(_, t)| t)?;
    match t {
        Target::Entry(n) | Target::Header(n) | Target::AotEntry(n) => Some(n.index()),
        Target::Comment(tok) => Some(tok.index()),
        // An AoT group has no single element; anchor on its first entry.
        Target::AotGroup => node
            .children
            .first()
            .and_then(|first| element_root_index(idx, first)),
        Target::ArrayElement(_) => None,
    }
}

/// Navigate the projected tree to the node at `path`.
fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut cur = root;
    for i in 0..path.len() {
        cur = cur.children.iter().find(|c| c.path == path[..=i])?;
    }
    Some(cur)
}

/// If the element at `at` is a `NEWLINE`, return `at + 1` (so a splice consumes it),
/// else `at`.
fn extend_over_newline(parent: &SyntaxNode, at: usize) -> usize {
    let els: Vec<_> = parent.children_with_tokens().collect();
    if matches!(els.get(at), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        at + 1
    } else {
        at
    }
}

/// The `[start, end)` child-index range of the comment block beginning at `first`
/// within `parent`: consecutive `COMMENT` tokens separated by single newlines.
fn comment_block_range(parent: &SyntaxNode, first: &SyntaxToken) -> (usize, usize) {
    let els: Vec<_> = parent.children_with_tokens().collect();
    let start = first.index();
    let mut end = start + 1; // one past the first COMMENT
    let mut i = end;
    while i + 1 < els.len() {
        let sep_is_single_nl = matches!(&els[i], NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::NEWLINE && t.text().matches('\n').count() == 1);
        let next_is_comment = matches!(&els[i + 1], NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::COMMENT);
        if sep_is_single_nl && next_is_comment {
            end = i + 2;
            i += 2;
        } else {
            break;
        }
    }
    (start, end)
}

fn is_scalar_kind(k: SyntaxKind) -> bool {
    use SyntaxKind as K;
    matches!(
        k,
        K::STRING
            | K::MULTI_LINE_STRING
            | K::STRING_LITERAL
            | K::MULTI_LINE_STRING_LITERAL
            | K::INTEGER
            | K::INTEGER_HEX
            | K::INTEGER_OCT
            | K::INTEGER_BIN
            | K::FLOAT
            | K::BOOL
            | K::DATE_TIME_OFFSET
            | K::DATE_TIME_LOCAL
            | K::DATE
            | K::TIME
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use std::io::Write;

    fn doc(src: &str) -> crate::model::cst_doc::CstDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        crate::model::cst_doc::CstDocument::load(f.path()).unwrap()
    }

    #[test]
    fn replace_scalar_value_keeps_everything_else() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("b".into())],
            toml: "b = 42\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 42\n");
    }

    #[test]
    fn replace_scalar_preserves_trailing_comment() {
        let mut d = doc("port = 8080  # http\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 9090  # http\n");
    }

    #[test]
    fn replace_array_element_in_place() {
        let mut d = doc("arr = [0x1, 0o2, 3] # tail\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            toml: "__elem__ = 99\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [0x1, 99, 3] # tail\n");
    }

    #[test]
    fn replace_in_table_scope() {
        let mut d = doc("[server]\nport = 8080\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
            toml: "port = 1\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[server]\nport = 1\n");
    }

    #[test]
    fn replace_empty_path_reparses_document() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![],
            toml: "a = 10\nc = 3\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 10\nc = 3\n");
    }

    #[test]
    fn replace_empty_path_rejects_invalid_and_leaves_doc_intact() {
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Replace {
                path: vec![],
                toml: "a = = bad".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn edit_single_line_comment() {
        let mut d = doc("# old\na = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "# new".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# new\na = 1\n");
    }

    #[test]
    fn edit_multiline_comment_block() {
        let mut d = doc("# one\n# two\na = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "# uno\n# dos\n# tres".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# uno\n# dos\n# tres\na = 1\n");
    }

    #[test]
    fn edit_comment_rejects_non_comment_text() {
        let mut d = doc("# old\na = 1\n");
        let err = d
            .apply(Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "not a comment".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "# old\na = 1\n");
    }

    #[test]
    fn delete_leaf_entry() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nc = 3\n");
    }

    #[test]
    fn delete_entry_leaves_adjacent_comment_behind() {
        // The migration's payoff: a comment is an independent node, so deleting the
        // entry below it does not remove the comment.
        let mut d = doc("# keep me\nb = 2\nc = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# keep me\nc = 3\n");
    }

    #[test]
    fn delete_single_and_multiline_comment() {
        let mut d = doc("# gone\na = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n");

        let mut d = doc("# one\n# two\na = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn replace_whole_array_value() {
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into())],
            toml: "arr = [9, 8, 7]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [9, 8, 7]\n");
    }

    #[test]
    fn replace_inline_table_value_keeps_trailing_comment() {
        let mut d = doc("pt = { x = 1 }  # p\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("pt".into())],
            toml: "pt = { x = 2, y = 3 }\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "pt = { x = 2, y = 3 }  # p\n");
    }

    #[test]
    fn delete_array_element_middle_and_last() {
        let mut d = doc("arr = [1, 2, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1]\n");
    }

    #[test]
    fn delete_first_array_element() {
        let mut d = doc("arr = [1, 2, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [2, 3]\n");
    }

    #[test]
    fn delete_whole_table_keeps_siblings() {
        let mut d = doc("[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\nz = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n\n[c]\nz = 3\n");
    }

    #[test]
    fn delete_table_takes_nested_subtable() {
        let mut d = doc("[a]\nx = 1\n[a.sub]\nk = 1\n[b]\ny = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n");
    }

    #[test]
    fn replace_whole_table_section() {
        let mut d = doc("[s]\nport = 1\n[d]\nz = 9\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("s".into())],
            toml: "[s]\nport = 2\nhost = \"x\"\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nport = 2\nhost = \"x\"\n[d]\nz = 9\n");
    }

    #[test]
    fn delete_aot_entry() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n[[p]]\nn = 3\n");
        // Delete the middle entry (child-position index 1 under `p`).
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("p".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[p]]\nn = 1\n[[p]]\nn = 3\n");
    }

    #[test]
    fn array_insert_middle_end_and_empty() {
        let mut d = doc("arr = [1, 3]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            toml: "2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, 3]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 99,
            },
            toml: "4\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, 3, 4]\n");

        let mut e = doc("xs = []\n");
        e.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("xs".into())],
                index: 0,
            },
            toml: "7\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(e.serialize(), "xs = [7]\n");
    }

    #[test]
    fn block_edit_dotted_table_consolidates_at_first_position() {
        // `$EDITOR` on a `[T/D]` table: members scattered around `x` get rewritten
        // and land where the first member was.
        let mut d = doc("a.b = 1\nx = 0\na.c = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            toml: "a.b = 10\na.c = 20\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 10\na.c = 20\nx = 0\n");
    }

    #[test]
    fn block_edit_contiguous_dotted_table() {
        let mut d = doc("a.b = 1\na.c = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            toml: "a.b = 1\na.c = 2\na.d = 3\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 1\na.c = 2\na.d = 3\n");
    }

    #[test]
    fn rename_plain_key_to_dotted_makes_table() {
        // `foo` → `foo.x` rewrites the key in place, projecting as a `[T/D]` table.
        let mut d = doc("foo = 1\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("foo".into())],
            new_key: "foo.x".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "foo.x = 1\n");
    }

    #[test]
    fn rename_dotted_leaf_swaps_last_segment() {
        let mut d = doc("a.b.c = 1\n");
        d.apply(Mutation::Rename {
            path: vec![
                Seg::Key("a".into()),
                Seg::Key("b".into()),
                Seg::Key("c".into()),
            ],
            new_key: "z".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b.z = 1\n");
    }

    #[test]
    fn delete_dotted_table_removes_all_members() {
        // Delete on a `[T/D]` table fans out to every member (plain cascade).
        let mut d = doc("a.b = 1\nx = 0\na.c = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = 0\n");
    }

    #[test]
    fn delete_last_dotted_leaf_drops_the_table() {
        // Deleting the only remaining member removes the implicit table too.
        let mut d = doc("a.b = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into()), Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "");
    }

    #[test]
    fn rename_whole_synthetic_dotted_table_is_rejected() {
        // Renaming a whole `[T/D]` table has no source element → doc untouched.
        let mut d = doc("a.b.c = 1\n");
        assert!(d
            .apply(Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "x".into(),
            })
            .is_err());
        assert_eq!(d.serialize(), "a.b.c = 1\n");
    }

    #[test]
    fn insert_into_dotted_table_writes_dotted_entry() {
        // Inserting a child into a synthetic `[T/D]` table writes a dotted entry
        // next to its siblings — no header.
        let mut d = doc("a.b = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 1,
            },
            toml: "x = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 1\na.x = 2\n");
    }

    #[test]
    fn insert_into_nested_dotted_table() {
        let mut d = doc("a.b.c = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into()), Seg::Key("b".into())],
                index: 1,
            },
            toml: "d = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b.c = 1\na.b.d = 2\n");
    }

    #[test]
    fn insert_into_dotted_table_under_scope_is_scope_relative() {
        // A dotted table nested in a real `[scope]` prefixes only the dotted run.
        let mut d = doc("[server]\nhost.name = \"h\"\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("server".into()), Seg::Key("host".into())],
                index: 1,
            },
            toml: "port = 80\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[server]\nhost.name = \"h\"\nhost.port = 80\n"
        );
    }

    #[test]
    fn delete_inline_table_member() {
        let mut d = doc("pt = { x = 1, y = 2, z = 3 }\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("pt".into()), Seg::Key("y".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "pt = { x = 1, z = 3 }\n");
    }

    #[test]
    fn delete_whole_aot_group() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n[q]\nz = 9\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("p".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[q]\nz = 9\n");
    }

    #[test]
    fn remark_comments_out_and_back_a_table() {
        let mut d = doc("[s]\nport = 1\nhost = \"x\"\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("s".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# [s]\n# port = 1\n# host = \"x\"\n");
        // Uncomment the block back to a live table.
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nport = 1\nhost = \"x\"\n");
    }

    #[test]
    fn remark_comments_out_an_aot_entry() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("p".into()), Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# [[p]]\n# n = 1\n[[p]]\nn = 2\n");
    }

    #[test]
    fn delete_entry_in_table_scope() {
        let mut d = doc("[s]\nx = 1\ny = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("s".into()), Seg::Key("x".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\ny = 2\n");
    }

    #[test]
    fn insert_comment_before_entry() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            text: "# note".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# note\nb = 2\n");
    }

    #[test]
    fn insert_comment_at_document_end() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# tail\n");
    }

    #[test]
    fn insert_multiline_comment_before_entry() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 0,
            },
            text: "# one\n# two".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# one\n# two\na = 1\n");
    }

    #[test]
    fn insert_comment_in_table_scope() {
        let mut d = doc("[s]\nx = 1\ny = 2\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 1,
            },
            text: "# between".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\n# between\ny = 2\n");
    }

    #[test]
    fn insert_node_below_a_comment() {
        // Phase 4: with comments as real ordered nodes, inserting a node right after
        // a comment row (cursor on the comment at index 0 → target index 1) places it
        // directly below the comment — the originally-requested capability.
        let mut d = doc("# section\na = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            toml: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "# section\nb = 2\na = 1\n");
    }

    #[test]
    fn insert_comment_rejects_non_comment() {
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::InsertComment {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                text: "nope".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn insert_leaf_before_anchor() {
        let mut d = doc("a = 1\nc = 3\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            toml: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\nc = 3\n");
    }

    #[test]
    fn insert_leaf_at_end() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            toml: "z = 9\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nz = 9\n");
    }

    #[test]
    fn insert_collision_cancel_errors() {
        let mut d = doc("a = 1\nb = 2\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                toml: "b = 9\n".into(),
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(k) if k == "b"));
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn insert_collision_overwrite_replaces_in_place() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            toml: "b = 99\n".into(),
            on_collision: OnCollision::Overwrite,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 99\nc = 3\n");
    }

    #[test]
    fn insert_collision_rename_suffixes_key() {
        let mut d = doc("b = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            toml: "b = 9\n".into(),
            on_collision: OnCollision::Rename,
        })
        .unwrap();
        assert_eq!(d.serialize(), "b = 2\nb_2 = 9\n");
    }

    #[test]
    fn insert_section_into_scope_collides_with_existing_subtable() {
        // The header was re-prefixed to `[b.a]` before the collision check; the
        // check must use the absolute path (a phantom `b.b.a` lookup used to let
        // the duplicate through).
        let mut d = doc("[b]\nx = 1\n\n[b.a]\ny = 2\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![Seg::Key("b".into())],
                    index: 9,
                },
                toml: "[a]\nz = 3\n".into(),
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), "[b]\nx = 1\n\n[b.a]\ny = 2\n");
    }

    #[test]
    fn replace_document_rejects_duplicate_sections() {
        // taplo's parser is syntax-only; the semantic backstop must reject a
        // whole-document rewrite that introduces a duplicate `[a]`.
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Replace {
                path: vec![],
                toml: "[a]\nx = 1\n[c]\ny = 2\n[a]\nz = 3\n".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn replace_section_rejects_resulting_duplicate() {
        // A block edit that renames `[a]` to an already-existing `[b]` would leave
        // two `[b]` sections — the backstop rejects it, doc untouched.
        let src = "[a]\nx = 1\n\n[b]\ny = 2\n";
        let mut d = doc(src);
        let err = d
            .apply(Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                toml: "[b]\nz = 3\n".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), src);
    }

    #[test]
    fn insert_into_table_scope() {
        let mut d = doc("[s]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            toml: "y = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\ny = 2\n");
    }

    #[test]
    fn insert_scalar_after_table_is_rejected() {
        // D5: a key appended after `[t]` would be re-keyed into `[t]` — reject,
        // leave the document untouched.
        let mut d = doc("a = 1\n[t]\nx = 1\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 9, // append at root end (past [t])
                },
                toml: "z = 9\n".into(),
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n[t]\nx = 1\n");
    }

    #[test]
    fn insert_scalar_before_table_is_ok() {
        // The split slot (index == first-header index) accepts a leaf: it lands in
        // the leading region, before the header.
        let mut d = doc("a = 1\n[t]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1, // between `a` and `[t]`
            },
            toml: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\n[t]\nx = 1\n");
    }

    #[test]
    fn insert_table_before_scalar_is_rejected() {
        // D5 inverse: a `[t]` placed before `a` would capture `a` — reject.
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                toml: "[t]\ny = 1\n".into(),
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn insert_keyed_into_array_wraps_as_inline_table() {
        // A keyed fragment pasted into an array is wrapped as `{ k = v }` so the key
        // is preserved (was: key dropped).
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            toml: "x = 99\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, { x = 99 }]\n");
    }

    #[test]
    fn insert_keyed_inline_table_into_array_nests() {
        // A keyed inline-table value becomes a nested inline table element.
        let mut d = doc("arr = [1]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            toml: "foo = { a = 1 }\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, { foo = { a = 1 } }]\n");
    }

    #[test]
    fn insert_bare_inline_table_into_array_stays_bare() {
        // A keyless inline-table value keeps its element form (no wrapping).
        let mut d = doc("arr = [1]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            toml: "{ a = 1 }\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, { a = 1 }]\n");
    }

    #[test]
    fn insert_bare_value_into_table_synthesizes_key() {
        // C2 / key+: a bare element value pasted into a table gets a `placeholder` key.
        let mut d = doc("a = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            toml: "42\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nplaceholder = 42\n");
    }

    #[test]
    fn insert_synthesized_key_auto_renames_on_collision() {
        // key+ never prompts: a `placeholder` clash auto-suffixes even under Cancel.
        let mut d = doc("placeholder = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            toml: "42\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "placeholder = 1\nplaceholder_2 = 42\n");
    }

    #[test]
    fn edit_array_interior_comment() {
        // #6b: a standalone comment inside a multiline array edits in place.
        let mut d = doc("arr = [\n  1,\n  # c\n  2,\n]\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            text: "# changed".into(),
        })
        .unwrap();
        let s = d.serialize();
        assert!(s.contains("# changed") && !s.contains("# c\n"), "s: {s:?}");
    }

    #[test]
    fn delete_array_interior_comment() {
        // #6c: deleting a standalone array comment removes it (and its line).
        let mut d = doc("arr = [\n  1,\n  # c\n  2,\n]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        let s = d.serialize();
        assert!(!s.contains("# c"), "comment removed: {s:?}");
        assert!(s.contains("1,") && s.contains("2,"), "elements kept: {s:?}");
    }

    #[test]
    fn insert_comment_into_multiline_array() {
        // #6d: a comment lands on its own indented line before the slot element.
        let mut d = doc("arr = [\n  1,\n  2,\n]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            text: "# note".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  # note\n  2,\n]\n");
    }

    #[test]
    fn insert_comment_appends_at_array_end() {
        let mut d = doc("arr = [\n  1,\n  2,\n]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  2,\n  # tail\n]\n");
    }

    #[test]
    fn insert_comment_into_single_line_array_upgrades_to_multiline() {
        // Reconstruct increment 3: instead of rejecting, the array is reformatted
        // to one element per line and the comment lands at the requested slot.
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# x".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # x\n  1,\n  2,\n]\n");
    }

    #[test]
    fn comment_upgrade_inserts_mid_and_tail() {
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            text: "# mid".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  # mid\n  2,\n]\n");

        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  2,\n  # tail\n]\n");
    }

    #[test]
    fn comment_upgrade_empty_array_and_trailing_comment() {
        // An empty array upgrades to hold just the comment.
        let mut d = doc("arr = []\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# todo".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # todo\n]\n");

        // A trailing comment on the entry line is outside the ARRAY and stays put.
        let mut d = doc("arr = [1] # eol\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# in".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # in\n  1,\n] # eol\n");
    }

    #[test]
    fn replace_scalar_with_array_and_back() {
        // #1: a scalar↔structured type change round-trips through Replace.
        let mut d = doc("x = 5\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            toml: "x = [1, 2]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            toml: "x = 9\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = 9\n");
    }

    #[test]
    fn replace_scalar_with_inline_table() {
        let mut d = doc("x = 5\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            toml: "x = { a = 1 }\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = { a = 1 }\n");
    }

    #[test]
    fn replace_structured_array_element() {
        // #2 write-back: a structured array element (array-of-arrays) swaps in place.
        let mut d = doc("arr = [[1, 2]]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
            toml: "x = [9]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [[9]]\n");
    }

    #[test]
    fn replace_single_line_array_value_swaps_it() {
        // #7 write-back: inline-editing a single-line array commits a structured
        // Replace that swaps the whole array.
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into())],
            toml: "arr = [9]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [9]\n");
    }

    #[test]
    fn insert_table_into_array_is_rejected() {
        // D1 ✗ cell: a `[table]` cannot become an array element (hard coerce).
        let mut d = doc("arr = [1]\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![Seg::Key("arr".into())],
                    index: 9,
                },
                toml: "[t]\nx = 1\n".into(),
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "arr = [1]\n");
    }

    #[test]
    fn serialize_whole_aot_group_returns_all_entries() {
        // Regression: editing an AoT *group* node showed blank ($EDITOR got "").
        let d = doc("[[p]]\nx = 1\n\n[[p]]\nx = 2\n");
        let frag = d.serialize_fragment(&[Seg::Key("p".into())]);
        assert!(
            frag.contains("[[p]]") && frag.contains("x = 1") && frag.contains("x = 2"),
            "frag: {frag:?}"
        );
    }

    #[test]
    fn replace_whole_aot_group_swaps_all_entries() {
        let mut d = doc("[[p]]\nx = 1\n\n[[p]]\nx = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("p".into())],
            toml: "[[p]]\nx = 9\n".into(),
        })
        .unwrap();
        let s = d.serialize();
        assert!(s.contains("x = 9"), "s: {s:?}");
        assert!(!s.contains("x = 1") && !s.contains("x = 2"), "s: {s:?}");
    }

    #[test]
    fn rename_leaf_key_preserves_value_and_position() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("b".into())],
            new_key: "bee".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nbee = 2\nc = 3\n");
    }

    #[test]
    fn rename_table_header() {
        let mut d = doc("[server]\nport = 8080\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("server".into())],
            new_key: "srv".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[srv]\nport = 8080\n");
    }

    #[test]
    fn rename_preserves_trailing_comment() {
        let mut d = doc("a = 1  # keep\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "aa".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "aa = 1  # keep\n");
    }

    #[test]
    fn rename_collision_errors() {
        let mut d = doc("a = 1\nb = 2\n");
        let err = d
            .apply(Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "b".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(k) if k == "b"));
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn remark_comments_out_a_leaf() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# b = 2\n");
    }

    #[test]
    fn remark_uncomments_back_to_live() {
        let mut d = doc("a = 1\n# b = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn remark_roundtrips() {
        let mut d = doc("port = 8080\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("port".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# port = 8080\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 8080\n");
    }

    #[test]
    fn move_reorders_within_scope() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        // Move `a` to the end (after c).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "b = 2\nc = 3\na = 1\n");
    }

    #[test]
    fn move_leaves_comment_behind() {
        // The whole point of the migration: a move repositions only the node; the
        // comment above it is an independent node and stays put.
        let mut d = doc("# header\nx = 1\ny = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("x".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "# header\ny = 2\nx = 1\n");
    }

    #[test]
    fn move_table_reorders_at_top_level() {
        let mut d = doc("[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\nz = 3\n");
        // Move `[a]` to the end (after c).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let s = d.serialize();
        // `[a]` and its body now come after `[c]`; one of each table remains.
        assert!(s.find("[a]").unwrap() > s.find("[c]").unwrap(), "got:\n{s}");
        assert_eq!(s.matches("[a]").count(), 1);
        assert!(s.contains("x = 1") && s.contains("z = 3"));
    }

    #[test]
    fn move_into_table_scope() {
        let mut d = doc("a = 1\n[dest]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\nx = 1\na = 1\n");
    }

    #[test]
    fn edit_comment_inside_table_scope() {
        let mut d = doc("[s]\n# explain\nport = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Key("s".into()), Seg::Index(0)],
            text: "# clarify".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\n# clarify\nport = 1\n");
    }

    // Issue 1: a `{ … }` value member of a `[T/D]` table must not have its interior
    // entries pulled out by the block edit — only the flat dotted entries are members.
    #[test]
    fn replace_dotted_table_keeps_inline_table_value_intact() {
        let mut d = doc("dotted.a = 1\ndotted.t = {x=1}\n");
        // Re-emit the same block: the inline table's inner `x=1` must not surface.
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("dotted".into())],
            toml: "dotted.a = 1\ndotted.t = {x=1}\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "dotted.a = 1\ndotted.t = {x=1}\n");
    }

    #[test]
    fn fragment_of_inline_value_member_is_not_a_separate_line() {
        let d = doc("dotted.a = 1\ndotted.t = {x=1}\n");
        // The block fragment for the whole `[T/D]` table lists exactly two members.
        let frag = d.serialize_fragment(&[Seg::Key("dotted".into())]);
        assert_eq!(frag, "dotted.a = 1\ndotted.t = {x=1}\n");
    }

    // Issue 2: copy/cut out of a `[T/D]` table drops the dotted-ancestor prefix.
    #[test]
    fn relative_fragment_strips_dotted_prefix_of_leaf() {
        let d = doc("dotted.test.bool_true = true\n");
        let frag = d.serialize_fragment_relative(&[
            Seg::Key("dotted".into()),
            Seg::Key("test".into()),
            Seg::Key("bool_true".into()),
        ]);
        assert_eq!(frag, "bool_true = true\n");
    }

    #[test]
    fn relative_fragment_strips_one_level_for_subtable() {
        let d = doc("dotted.test.a = 1\ndotted.test.b = 2\n");
        // Copying the `test` subtable strips only the `dotted` ancestor.
        let frag =
            d.serialize_fragment_relative(&[Seg::Key("dotted".into()), Seg::Key("test".into())]);
        assert_eq!(frag, "test.a = 1\ntest.b = 2\n");
    }

    #[test]
    fn plain_fragment_keeps_full_dotted_key() {
        // The `$EDITOR` path (non-relative) must keep full keys for the block rewrite.
        let d = doc("dotted.test.bool_true = true\n");
        let frag = d.serialize_fragment(&[
            Seg::Key("dotted".into()),
            Seg::Key("test".into()),
            Seg::Key("bool_true".into()),
        ]);
        assert_eq!(frag, "dotted.test.bool_true = true\n");
    }

    #[test]
    fn cut_out_of_dotted_table_drops_prefix() {
        let mut d = doc("dotted.test.flag = true\n[dest]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![
                Seg::Key("dotted".into()),
                Seg::Key("test".into()),
                Seg::Key("flag".into()),
            ]],
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\nx = 1\nflag = true\n");
    }

    // Regression: inserting into the slot *before* a `[T/D]` synthetic table (which
    // has no backing element) must anchor on the table's first member line, not fail
    // as `Unsupported`. Mirrors "cut a scalar, paste after a multiline array that is
    // immediately followed by a `[T/D]` table".
    #[test]
    fn insert_before_dotted_table_anchors_on_first_member() {
        let mut d = doc("arr = [\n  1,\n]\ndotted.x = 1\n");
        // Insert `gg = 5` at root index 1 — the slot occupied by the `dotted` table.
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            toml: "gg = 5\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n]\ngg = 5\ndotted.x = 1\n");
    }

    #[test]
    fn move_before_dotted_table_succeeds() {
        let mut d = doc("gg = 5\narr = [\n  1,\n]\ndotted.x = 1\n");
        // Move `gg` into the slot before the `dotted` table (after the array).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("gg".into())]],
            target: InsTarget {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n]\ngg = 5\ndotted.x = 1\n");
    }

    // Move an array element out: into a table a single-key inline table unwraps to a
    // keyed entry, a multi-key one / bare value gets a synthesized placeholder; into
    // another array it stays a bare element.
    fn move_elem(initial: &str, src: Vec<Seg>, dst: Vec<Seg>) -> String {
        let mut d = doc(initial);
        d.apply(Mutation::Move {
            sources: vec![src],
            target: InsTarget {
                parent: dst,
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        d.serialize()
    }

    #[test]
    fn move_single_key_element_into_table_unwraps() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = []\n[dest]\nz = 0\nfoo = 1\n");
    }

    #[test]
    fn move_multikey_element_into_table_gets_placeholder() {
        let s = move_elem(
            "arr = [{ a = 1, b = 2 }]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(
            s,
            "arr = []\n[dest]\nz = 0\nplaceholder = { a = 1, b = 2 }\n"
        );
    }

    #[test]
    fn move_bare_element_into_table_gets_placeholder() {
        let s = move_elem(
            "arr = [42]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = []\n[dest]\nz = 0\nplaceholder = 42\n");
    }

    #[test]
    fn move_element_into_array_stays_bare() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\nbrr = [9]\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("brr".into())],
        );
        assert_eq!(s, "arr = []\nbrr = [9, { foo = 1 }]\n");
    }

    #[test]
    fn move_single_key_element_into_dotted_table_prefixes() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\n[d]\ndd.x = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("d".into()), Seg::Key("dd".into())],
        );
        assert_eq!(s, "arr = []\n[d]\ndd.x = 0\ndd.foo = 1\n");
    }

    // Phase 2: a whole synthetic `[T/D]` table moves by fanning out its members,
    // each re-prefixed for the destination.
    #[test]
    fn move_whole_dotted_table_into_scope() {
        let s = move_elem(
            "a.x = 1\na.y = 2\n[dest]\nz = 0\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "[dest]\nz = 0\na.x = 1\na.y = 2\n");
    }

    #[test]
    fn move_whole_dotted_table_into_dotted_adds_prefix() {
        let s = move_elem(
            "a.x = 1\nb.y = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        );
        assert_eq!(s, "b.y = 2\nb.a.x = 1\n");
    }

    #[test]
    fn move_dotted_subtable_out_to_root_drops_prefix() {
        let s = move_elem(
            "dotted.test.p = 1\ndotted.test.q = 2\ndotted.keep = 9\n",
            vec![Seg::Key("dotted".into()), Seg::Key("test".into())],
            vec![],
        );
        assert_eq!(s, "dotted.keep = 9\ntest.p = 1\ntest.q = 2\n");
    }

    // Collision is exact full-path: a dotted entry sharing only a prefix merges into
    // the same `[T/D]` table instead of colliding.
    #[test]
    fn insert_dotted_sibling_merges_not_collides() {
        let mut d = doc("a.x = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 99,
            },
            toml: "a.y = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.x = 1\na.y = 2\n");
    }

    #[test]
    fn insert_identical_dotted_key_still_collides() {
        let mut d = doc("a.x = 1\n");
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 99,
            },
            toml: "a.x = 9\n".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(r, Err(MutateError::Collision(k)) if k == "a.x"));
    }

    #[test]
    fn copy_dotted_block_into_dotted_prefixes_every_member() {
        // Copy path: a multi-member [T/D] block inserted into a dotted dest re-prefixes
        // EVERY member (was: second member dropped).
        let mut d = doc("a.x = 1\na.y = 2\nb.k = 9\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("b".into())],
                index: 99,
            },
            toml: "a.x = 1\na.y = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.x = 1\na.y = 2\nb.k = 9\nb.a.x = 1\nb.a.y = 2\n"
        );
    }

    fn move_try(
        initial: &str,
        src: Vec<Seg>,
        dst: Vec<Seg>,
    ) -> Result<String, crate::model::document::MutateError> {
        let mut d = doc(initial);
        d.apply(Mutation::Move {
            sources: vec![src],
            target: InsTarget {
                parent: dst,
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })?;
        Ok(d.serialize())
    }

    // Phase 3: cross-type table moves.
    #[test]
    fn move_dotted_table_into_inline_table_flattens() {
        let s = move_try(
            "a.x = 1\na.y = 2\nt = { k = 0 }\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("t".into())],
        )
        .unwrap();
        assert_eq!(s, "t = { k = 0, a.x = 1, a.y = 2 }\n");
    }

    #[test]
    fn move_scope_table_into_scope_nests_header() {
        let s = move_try(
            "[a]\nx = 1\n[b]\ny = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n[b.a]\nx = 1\n");
    }

    #[test]
    fn move_scope_table_with_subtable_into_scope_nests_all_headers() {
        let s = move_try(
            "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n[b.a]\nx = 1\n[b.a.sub]\nz = 3\n");
    }

    #[test]
    fn move_scope_table_into_dotted_is_illegal() {
        // `b` must be a *top-level* dotted table, so it precedes the `[a]` header
        // (entries after `[a]` would belong to `a`).
        let r = move_try(
            "b.k = 9\n[a]\nx = 1\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        );
        assert!(
            matches!(&r, Err(MutateError::Illegal(m)) if m.contains("dotted")),
            "got {r:?}"
        );
    }

    #[test]
    fn move_scope_table_into_inline_is_illegal() {
        let r = move_try(
            "t = { k = 0 }\n[a]\nx = 1\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("t".into())],
        );
        assert!(
            matches!(&r, Err(MutateError::Illegal(m)) if m.contains("inline")),
            "got {r:?}"
        );
    }

    // An entry insert targeting a scope table clamps to its entry run instead of
    // being rejected when the index points past sub-section children (the paste
    // "Into" slot appends at `children.len()`).
    #[test]
    fn move_dotted_table_into_scope_with_subtables_clamps_to_entry_run() {
        let s = move_try(
            "d.x = 1\nd.y = 2\n[pt]\n[pt.a]\nname = \"H\"\n",
            vec![Seg::Key("d".into())],
            vec![Seg::Key("pt".into())],
        )
        .unwrap();
        assert_eq!(s, "[pt]\nd.x = 1\nd.y = 2\n[pt.a]\nname = \"H\"\n");
    }

    // The dual clamp: a header-like fragment targeted before the destination's
    // entries lands at the section run instead of "would capture" Illegal.
    #[test]
    fn move_scope_table_into_scope_at_front_clamps_past_entries() {
        let mut d = doc("[a]\nx = 1\n[b]\ny = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("b".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n[a.b]\ny = 2\n");
    }

    // A copied [T/D] fragment (multi-entry, no header) keeps *all* members when
    // pasted into an inline table — the per-entry split must run before the
    // inline-table branch, which splices only the first entry.
    #[test]
    fn copy_dotted_table_into_inline_keeps_all_members() {
        let mut d = doc("a.x = 1\na.y = 2\na.gg = 3\nt = { k = 0 }\n");
        let frag = d.serialize_fragment_relative(&[Seg::Key("a".into())]);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            toml: frag,
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.x = 1\na.y = 2\na.gg = 3\nt = { k = 0, a.x = 1, a.y = 2, a.gg = 3 }\n"
        );
    }

    // Multi-entry insert holds its slot with a stable anchor: the first members
    // merge into one projected child, so a drifting `index + k` would push later
    // members past the destination's own entries.
    #[test]
    fn copy_dotted_table_into_scope_lands_contiguously() {
        let mut d = doc("a.t.p = 1\na.t.q = 2\na.gg = 3\n\n[s]\nk = 0\n");
        let frag = d.serialize_fragment_relative(&[Seg::Key("a".into())]);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 0,
            },
            toml: frag,
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.t.p = 1\na.t.q = 2\na.gg = 3\n\n[s]\na.t.p = 1\na.t.q = 2\na.gg = 3\nk = 0\n"
        );
    }

    // ---- Synthetic [T/D] tables *inside* an inline table (decomposed dotted keys) ----

    const INLINE_DOTTED: &str = "t = { x.y = 1, x.z = 2, w = 3 }\n";

    #[test]
    fn insert_into_inline_dotted_table_prefixes_member() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into()), Seg::Key("x".into())],
                index: 99,
            },
            toml: "q = 9\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.y = 1, x.z = 2, w = 3, x.q = 9 }\n");
    }

    #[test]
    fn insert_exact_member_into_inline_collides_but_prefix_merges() {
        let mut d = doc(INLINE_DOTTED);
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            toml: "x.y = 7\n".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(r, Err(MutateError::Collision(k)) if k == "x.y"));
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            toml: "x.q = 7\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.q = 7, x.y = 1, x.z = 2, w = 3 }\n");
    }

    #[test]
    fn delete_inline_dotted_table_removes_all_members() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("t".into()), Seg::Key("x".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { w = 3 }\n");
    }

    #[test]
    fn fragment_of_inline_dotted_table_keeps_own_key() {
        let d = doc(INLINE_DOTTED);
        let frag = d.serialize_fragment_relative(&[Seg::Key("t".into()), Seg::Key("x".into())]);
        assert_eq!(frag, "x.y = 1\nx.z = 2\n");
    }

    #[test]
    fn move_inline_dotted_table_out_to_root() {
        let s = move_try(
            INLINE_DOTTED,
            vec![Seg::Key("t".into()), Seg::Key("x".into())],
            vec![],
        )
        .unwrap();
        assert_eq!(s, "t = { w = 3 }\nx.y = 1\nx.z = 2\n");
    }

    #[test]
    fn move_inline_dotted_table_into_scope() {
        let s = move_try(
            "t = { x.y = 1, x.z = 2, w = 3 }\n[s]\nk = 0\n",
            vec![Seg::Key("t".into()), Seg::Key("x".into())],
            vec![Seg::Key("s".into())],
        )
        .unwrap();
        assert_eq!(s, "t = { w = 3 }\n[s]\nk = 0\nx.y = 1\nx.z = 2\n");
    }

    #[test]
    fn replace_inline_dotted_table_consolidates_at_first_member() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("t".into()), Seg::Key("x".into())],
            toml: "x.y = 5\nx.q = 6\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.y = 5, x.q = 6, w = 3 }\n");
    }

    #[test]
    fn comment_into_inline_dotted_table_is_illegal() {
        let mut d = doc(INLINE_DOTTED);
        let r = d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("t".into()), Seg::Key("x".into())],
                index: 0,
            },
            text: "# hi".into(),
        });
        assert!(matches!(r, Err(MutateError::Illegal(_))), "got {r:?}");
    }

    // Issue 3: inserting a keyed entry into an inline table splices it inside `{ … }`.
    #[test]
    fn insert_into_inline_table() {
        let mut d = doc("t = { a = 1 }\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            toml: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { a = 1, b = 2 }\n");
    }

    #[test]
    fn insert_into_inline_table_at_front() {
        let mut d = doc("t = { a = 1 }\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            toml: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { b = 2, a = 1 }\n");
    }

    #[test]
    fn insert_into_empty_inline_table() {
        let mut d = doc("t = {}\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            toml: "a = 1\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { a = 1 }\n");
    }

    #[test]
    fn insert_into_inline_table_collision_rejected() {
        let mut d = doc("t = { a = 1 }\n");
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            toml: "a = 2\n".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }

    // ── `[T/S]` discretization: a table's member set is its scattered sections ──

    /// `[a]`'s subtree is defined in two places, split by `[b]`.
    const SCATTERED: &str = "[a]\nx = 1\n\n[b]\ny = 2\n\n[a.sub]\nz = 3\n";

    #[test]
    fn delete_scattered_scope_table_takes_all_sections() {
        let mut d = doc(SCATTERED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n\n");
    }

    #[test]
    fn serialize_scattered_scope_table_includes_all_sections() {
        let d = doc(SCATTERED);
        let frag = d.serialize_fragment(&[Seg::Key("a".into())]);
        assert_eq!(frag, "[a]\nx = 1\n\n[a.sub]\nz = 3\n");
    }

    #[test]
    fn move_scattered_scope_table_into_scope_nests_all_sections() {
        let s = move_try(
            SCATTERED,
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n\n[b.a]\nx = 1\n\n[b.a.sub]\nz = 3\n");
    }

    #[test]
    fn move_nested_scope_table_out_strips_source_prefix() {
        // `[a.sub]` moved into `[b]` must become `[b.sub]`, not `[b.a.sub]`.
        let s = move_try(
            "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n",
            vec![Seg::Key("a".into()), Seg::Key("sub".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[a]\nx = 1\n[b]\ny = 2\n[b.sub]\nz = 3\n");
    }

    #[test]
    fn block_edit_scattered_scope_table_consolidates_at_first_definition() {
        let mut d = doc(SCATTERED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            toml: "[a]\nx = 9\n[a.sub]\nz = 9\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 9\n[a.sub]\nz = 9\n[b]\ny = 2\n\n");
    }

    #[test]
    fn block_edit_scope_table_rejects_out_of_subtree_header() {
        let mut d = doc(SCATTERED);
        let r = d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            toml: "[a]\nx = 9\n[c]\nq = 1\n".into(),
        });
        assert!(matches!(r, Err(MutateError::Illegal(_))), "got {r:?}");
        assert_eq!(d.serialize(), SCATTERED);
    }

    // ── Implicit scope table (`[a]` never written, only `[a.sub]`) ──

    #[test]
    fn serialize_implicit_scope_table_collects_sections() {
        let d = doc("[a.sub]\nz = 3\n[a.other]\nw = 4\n");
        let frag = d.serialize_fragment(&[Seg::Key("a".into())]);
        assert_eq!(frag, "[a.sub]\nz = 3\n[a.other]\nw = 4\n");
    }

    #[test]
    fn delete_implicit_scope_table_removes_all_sections() {
        let mut d = doc("[a.sub]\nz = 3\n[b]\ny = 2\n[a.other]\nw = 4\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n");
    }

    #[test]
    fn insert_entry_into_implicit_scope_table_creates_header() {
        // An entry child needs an `[a]` section to live in — created at the
        // table's first definition.
        let mut d = doc("[a.sub]\nz = 3\n[b]\ny = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 99,
            },
            toml: "x = 1\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n");
    }

    // ── Mixed table: dotted members + header-defined sub-sections ──

    /// `fruit.apple` is defined by a dotted key under `[fruit]` *and* a
    /// `[fruit.apple.texture]` sub-section (the TOML-spec `fruit.apple` pattern).
    const MIXED: &str =
        "[fruit]\nname = \"f\"\napple.color = \"red\"\n\n[fruit.apple.texture]\nsmooth = true\n";

    #[test]
    fn serialize_mixed_table_canonicalizes_to_scope_form() {
        let d = doc(MIXED);
        let frag = d.serialize_fragment(&[Seg::Key("fruit".into()), Seg::Key("apple".into())]);
        assert_eq!(
            frag,
            "[fruit.apple]\ncolor = \"red\"\n[fruit.apple.texture]\nsmooth = true\n"
        );
    }

    #[test]
    fn block_edit_mixed_table_consolidates_to_scope_form() {
        let mut d = doc(MIXED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
            toml: "[fruit.apple]\ncolor = \"green\"\n[fruit.apple.texture]\nsmooth = false\n"
                .into(),
        })
        .unwrap();
        // The removed member line takes its trailing newline token with it (which
        // also held the blank line), as any deleted entry line does.
        assert_eq!(
            d.serialize(),
            "[fruit]\nname = \"f\"\n[fruit.apple]\ncolor = \"green\"\n[fruit.apple.texture]\nsmooth = false\n"
        );
    }

    #[test]
    fn delete_mixed_table_removes_members_and_sections() {
        let mut d = doc(MIXED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[fruit]\nname = \"f\"\n");
    }

    #[test]
    fn insert_entry_into_mixed_table_writes_dotted_member() {
        // No `[fruit.apple]` header may be created while dotted definitions
        // remain (spec-invalid) — the new entry joins the dotted members.
        let mut d = doc(MIXED);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
                index: 99,
            },
            toml: "size = 3\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[fruit]\nname = \"f\"\napple.color = \"red\"\n\napple.size = 3\n[fruit.apple.texture]\nsmooth = true\n"
        );
    }

    // ── AoT sub-groups travel with their table ──

    #[test]
    fn delete_scope_table_takes_scattered_aot_subgroup() {
        let mut d = doc("[a]\nx = 1\n\n[[a.list]]\nv = 1\n\n[b]\ny = 2\n\n[[a.list]]\nv = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n\n");
    }
}
