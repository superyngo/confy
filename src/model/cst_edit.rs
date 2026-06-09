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
use crate::model::node::{Node, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Apply `m` to a copy of `syntax`, returning the new tree. The original is never
/// mutated, so the caller commits only on `Ok`.
pub(crate) fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, toml, .. } => {
            if path.is_empty() {
                return reparse_document(&toml);
            }
            replace_value(&tree, &path, &toml)?;
            Ok(tree)
        }
        Mutation::EditComment { path, text } => {
            edit_comment(&tree, &path, &text)?;
            Ok(tree)
        }
        Mutation::Delete { path } => {
            delete(&tree, &path)?;
            Ok(tree)
        }
        Mutation::InsertComment { target, text } => {
            insert_comment(&tree, &target, &text)?;
            Ok(tree)
        }
        Mutation::Insert {
            target,
            toml,
            on_collision,
        } => {
            insert(&tree, &target, &toml, on_collision)?;
            Ok(tree)
        }
        Mutation::Rename { path, new_key } => {
            rename(&tree, &path, &new_key)?;
            Ok(tree)
        }
        Mutation::Remark { path } => {
            remark(&tree, &path)?;
            Ok(tree)
        }
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => {
            move_nodes(&tree, &sources, &target, on_collision)?;
            Ok(tree)
        }
    }
}

/// Serialize the node at `path` as a standalone fragment (clipboard / `$EDITOR`).
/// In the CST a fragment is just the node's source text — comments are independent
/// nodes, so a node never carries an adjacent comment (`carry_comment` is moot).
pub(crate) fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    let (proj, idx) = walk(syntax, "");
    // A comment node: its raw `# …` text.
    if let Some(node) = node_at(&proj.root, path) {
        if let crate::model::node::NodeKind::Comment(t) = &node.kind {
            return t.clone();
        }
    }
    let target = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t) {
        Some(t) => t,
        None => return String::new(),
    };
    match target {
        Target::Entry(n) | Target::ArrayElement(n) => {
            let s = n.to_string();
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
fn replace_value(tree: &SyntaxNode, path: &[Seg], toml: &str) -> Result<(), MutateError> {
    let (_, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
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
    let (_, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
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
    let arr = match idx.iter().find(|(p, _)| p == array_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::ARRAY)
            .ok_or(MutateError::Unsupported)?,
        _ => return Err(MutateError::Unsupported),
    };
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
    match parent.kind {
        NodeKind::Root | NodeKind::Table => {} // decor slot — handled below
        // A multiline array can hold a standalone comment line; a single-line array
        // can't (a `#` would comment out the closing bracket). Inline tables / AoT
        // groups never hold comments.
        NodeKind::Array if parent.value.is_none() => {
            return array_insert_comment(&idx, &target.parent, target.index, text);
        }
        NodeKind::Array => {
            return Err(MutateError::Illegal(
                "cannot add a comment to a single-line array".into(),
            ));
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

    // D1 simple adaptation across container types:
    //  - into an ARRAY we need a bare VALUE: a keyed fragment's key is dropped
    //    (`key↓`, by `array_insert` reading the VALUE), a bare element parses only
    //    once wrapped; a `[table]`/`[[aot]]` fragment is rejected (a hard coerce).
    //  - into a TABLE/root we need a keyed entry: a bare element gets a synthesized
    //    `placeholder` key (`key+`).
    let (frag, synthesized_key) = parse_fragment_adapted(&frag_text, parent_is_array)?;

    if parent_is_array {
        // Bare element into an array (`a` on an array, or `key↓` paste): no collision.
        return array_insert(&idx, &target.parent, target.index, &frag);
    }

    let new_key = fragment_first_key(&frag)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    // A synthesized `placeholder` key is auto-renamed on collision — the user never
    // chose it, so a clash shouldn't surface as a prompt/error.
    let on_collision = if synthesized_key {
        OnCollision::Rename
    } else {
        on_collision
    };
    let depth = target.parent.len();
    let is_collision = |k: &str| {
        parent
            .children
            .iter()
            .filter(|c| !matches!(c.kind, crate::model::node::NodeKind::Comment(_)))
            .any(|c| c.path.get(depth) == Some(&Seg::Key(k.to_string())))
    };

    if is_collision(&new_key) {
        match on_collision {
            OnCollision::Cancel => return Err(MutateError::Collision(new_key)),
            OnCollision::Overwrite => {
                // Replace the colliding entry's element in place (keeps position).
                let victim = parent
                    .children
                    .iter()
                    .find(|c| c.path.get(depth) == Some(&Seg::Key(new_key.clone())))
                    .ok_or(MutateError::NotFound)?;
                let velem = match idx.iter().find(|(p, _)| p == &victim.path).map(|(_, t)| t) {
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
                // Append _2, _3, … to the key until free, rewriting the fragment.
                let mut n = 2;
                while is_collision(&format!("{new_key}_{n}")) {
                    n += 1;
                }
                rewrite_first_key(&frag, &format!("{new_key}_{n}"))?;
            }
        }
    }

    check_partition(parent, &frag, target.index)?;
    let at = resolve_insert_at(tree, &proj.root, &idx, target)?;
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
/// becomes a keyed entry — the key is then either kept (table dest, `key+`) or
/// dropped by `array_insert` (array dest, `key↓`). A `[table]`/`[[aot]]` section
/// cannot become an array element (a hard coerce), so it is rejected for an array.
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
    let split = parent
        .children
        .iter()
        .position(|c| matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables))
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

/// Rewrite the first key-segment token of a node fragment to `new_key`.
fn rewrite_first_key(frag: &SyntaxNode, new_key: &str) -> Result<(), MutateError> {
    let key = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    let first = key
        .children_with_tokens()
        .find_map(key_seg_token)
        .ok_or_else(|| MutateError::Fragment("fragment key has no segment".into()))?;
    let parse = taplo::parser::parse(&format!("{new_key} = 0\n"));
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
    let i = first.index();
    key.splice_children(i..i + 1, vec![NodeOrToken::Token(nk)]);
    Ok(())
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
/// named nodes — adjacent comments stay put with no special handling. Entry and
/// `[table]` sources are supported; AoT-entry sources are deferred (they would need
/// append-not-collide insert semantics for `[[x]]`).
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
        let t = idx
            .iter()
            .find(|(ip, _)| ip == p)
            .map(|(_, t)| t.clone())
            .ok_or(MutateError::NotFound)?;
        match t {
            Target::Entry(n) => frags.push(n.to_string()),
            Target::Header(h) => frags.push(section_text(tree, p, h.index(), false)),
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
    let nk_tok = nk_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .and_then(|k| k.children_with_tokens().find_map(key_seg_token))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;

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

    // Replace the last key-segment token (the displayed/renamed segment).
    let last_tok = key_node
        .children_with_tokens()
        .filter_map(|c| c.into_token().filter(|t| is_key_seg(t.kind())))
        .last()
        .ok_or(MutateError::NotFound)?;
    let i = last_tok.index();
    nk_tok.detach();
    key_node.splice_children(i..i + 1, vec![NodeOrToken::Token(nk_tok)]);
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

/// The first key of a node fragment — the first segment of its first `KEY` node
/// (the child key it would occupy under its parent scope).
fn fragment_first_key(root: &SyntaxNode) -> Option<String> {
    let key = root.descendants().find(|n| n.kind() == SyntaxKind::KEY)?;
    key.children_with_tokens().find_map(|c| match c {
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
        // Insert before the child currently at `index`.
        let anchor = &parent.children[target.index];
        return element_root_index(idx, anchor).ok_or(MutateError::Unsupported);
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
    let mut last = header_pos.ok_or(MutateError::Unsupported)?;
    for child in &parent.children {
        if let Some(p) = element_root_index(idx, child) {
            last = last.max(p);
        }
    }
    Ok(extend_over_newline(tree, last + 1))
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
            sync_decor: false,
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
            sync_decor: false,
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
            sync_decor: false,
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
            sync_decor: false,
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
            sync_decor: true,
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
                sync_decor: true,
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
            sync_decor: true,
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
            sync_decor: true,
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
            sync_decor: true,
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
            toml: "__e__ = 2\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, 3]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 99,
            },
            toml: "__e__ = 4\n".into(),
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
            toml: "__e__ = 7\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(e.serialize(), "xs = [7]\n");
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
    fn insert_keyed_into_array_drops_key() {
        // C1 / key↓: a keyed fragment pasted into an array keeps only its value.
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
        assert_eq!(d.serialize(), "arr = [1, 2, 99]\n");
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
    fn insert_comment_into_single_line_array_is_rejected() {
        let mut d = doc("arr = [1, 2]\n");
        let err = d
            .apply(Mutation::InsertComment {
                target: InsTarget {
                    parent: vec![Seg::Key("arr".into())],
                    index: 0,
                },
                text: "# x".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "arr = [1, 2]\n");
    }

    #[test]
    fn replace_scalar_with_array_and_back() {
        // #1: a scalar↔structured type change round-trips through Replace.
        let mut d = doc("x = 5\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            toml: "x = [1, 2]\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            toml: "x = 9\n".into(),
            sync_decor: false,
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
            sync_decor: false,
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
            sync_decor: false,
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
            sync_decor: false,
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
        let frag = d.serialize_fragment(&[Seg::Key("p".into())], false);
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
            sync_decor: true,
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
}
