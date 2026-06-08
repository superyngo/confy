//! Phase 3 of the CST migration: apply a [`Mutation`] to the rowan tree by
//! splicing green nodes/tokens (the mutable `clone_for_update` API).
//!
//! Resolution uses the same `path → element` index that `cst_project::walk`
//! produces, so the resolver can never disagree with the projection. Each `apply`
//! works on a `clone_for_update` of the document and returns the new tree only on
//! success, so a failed multi-step edit (e.g. `Move`) rolls back for free — the
//! caller keeps the original tree untouched.
//!
//! Implemented so far: `Replace` (whole-document on the empty path; a scalar value
//! inline edit) and `EditComment`. The remaining variants return `Unsupported`
//! until ported.

use crate::model::cst_project::{walk, CstIndex, Target};
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
    let new_tok = scalar_token(&new_value)
        .ok_or_else(|| MutateError::Fragment("fragment value is not a scalar".into()))?;

    // Swap only the scalar token inside the target VALUE (keeps EOL comment/indent).
    let old_tok = scalar_token(&value).ok_or(MutateError::Unsupported)?;
    let i = old_tok.index();
    new_tok.detach();
    value.splice_children(i..i + 1, vec![NodeOrToken::Token(new_tok)]);
    Ok(())
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
            // Inline-table members carry a `,` separator (deferred); handle the
            // document/table scope where an entry occupies its own line.
            if parent.kind() != SyntaxKind::ROOT {
                return Err(MutateError::Unsupported);
            }
            let i = entry.index();
            let end = extend_over_newline(&parent, i + 1);
            parent.splice_children(i..end, vec![]);
            Ok(())
        }
        _ => Err(MutateError::Unsupported),
    }
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
    let parse = taplo::parser::parse(&frag_text);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_key = fragment_first_key(&frag)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;

    let (proj, idx) = walk(tree, "");
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let depth = target.parent.len();
    let collides = parent
        .children
        .iter()
        .filter(|c| !matches!(c.kind, crate::model::node::NodeKind::Comment(_)))
        .any(|c| c.path.get(depth) == Some(&Seg::Key(new_key.clone())));
    if collides {
        return match on_collision {
            OnCollision::Cancel => Err(MutateError::Collision(new_key)),
            // Overwrite / Rename deferred.
            _ => Err(MutateError::Unsupported),
        };
    }

    let at = resolve_insert_at(tree, &proj.root, &idx, target)?;
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
        _ => Err(MutateError::Unsupported),
    }
}

/// Move `sources` to `target`, atomically (the caller commits the clone only on
/// success). Comments are independent CST nodes, so a move repositions only the
/// named nodes — adjacent comments stay put with no special handling. Entry sources
/// at document/table scope are supported; table/AoT sources are deferred.
fn move_nodes(
    tree: &SyntaxNode,
    sources: &[Vec<Seg>],
    target: &InsTarget,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");

    // Capture each source's text (its `key = value` line) before any removal.
    let mut frags: Vec<String> = Vec::new();
    for p in sources {
        let t = idx
            .iter()
            .find(|(ip, _)| ip == p)
            .map(|(_, t)| t.clone())
            .ok_or(MutateError::NotFound)?;
        match t {
            Target::Entry(n) => frags.push(n.to_string()),
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

/// The scalar value token of a `VALUE` node (skips trivia and a trailing comment).
fn scalar_token(value: &SyntaxNode) -> Option<SyntaxToken> {
    value.children_with_tokens().find_map(|c| match c {
        NodeOrToken::Token(t) if is_scalar_kind(t.kind()) => Some(t),
        _ => None,
    })
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
