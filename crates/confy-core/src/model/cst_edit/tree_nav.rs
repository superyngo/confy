//! Low-level projected-tree / CST-index navigation helpers shared across
//! the `cst_edit` submodules — split out of `cst_edit.rs` (Task 15,
//! 2026-08-11 audit remediation).

use crate::model::cst_project::{CstIndex, Target};
use crate::model::document::{MutateError, Target as InsTarget};
use crate::model::node::{Node, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// The parent node of the node at `path`, plus the node itself, in the projection.
pub(crate) fn find_parent<'a>(root: &'a Node, path: &[Seg]) -> Option<(&'a Node, &'a Node)> {
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
pub(crate) fn inline_raw_member_index(idx: &CstIndex, parent: &Node, proj_index: usize) -> usize {
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
/// **decoded** — quotes stripped, basic-string escapes resolved — so the computed
/// path matches `cst_project::key_segments` exactly. Collision detection compares
/// these against projected `Seg::Key`s, so the two decoders must agree; taplo
/// lexes a quoted key as an `IDENT` that keeps its quotes, hence that arm decodes
/// too. A bare key yields one segment; a dotted key yields the chain.
pub(crate) fn fragment_key_segs(root: &SyntaxNode) -> Vec<String> {
    let Some(key) = root.descendants().find(|n| n.kind() == SyntaxKind::KEY) else {
        return Vec::new();
    };
    key.children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::IDENT
                | SyntaxKind::IDENT_WITH_GLOB
                | SyntaxKind::STRING
                | SyntaxKind::STRING_LITERAL => Some(crate::model::cst_project::unquote(t.text())),
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
pub(crate) fn resolve_insert_at(
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
///
/// An `Entry`'s own physical span (its whole `key = value` line) already contains
/// everything nested inside its value — including any inline-table members
/// `project_inline` also indexes. Those nested members' own `Target::Entry` index
/// is relative to *their* immediate CST parent (the inline table), not the ROOT,
/// so descending past an Entry and treating that as a ROOT-child index is wrong
/// (it can return an index past the ROOT's actual child count). Short-circuit on
/// `Entry` exactly like `node_start_root_index` already does; only a `Header`/
/// `AotEntry`/synthetic (headerless) container's members are genuinely separate
/// ROOT-level elements worth descending into.
pub(crate) fn node_last_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
    if let Some(Target::Entry(n)) = idx.iter().find(|(p, _)| p == &node.path).map(|(_, t)| t) {
        return Some(n.index());
    }
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
pub(crate) fn node_start_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
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
pub(crate) fn element_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
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
pub(crate) fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut cur = root;
    for i in 0..path.len() {
        cur = cur.children.iter().find(|c| c.path == path[..=i])?;
    }
    Some(cur)
}

/// True when the element at `at` is a table or array-of-tables header — i.e. a
/// comment spliced at `at` would sit immediately before a header with only a
/// single newline between them (the projection would then read it as that
/// header's *leading* comment rather than as a member of the current scope).
pub(crate) fn next_is_header(parent: &SyntaxNode, at: usize) -> bool {
    let els: Vec<_> = parent.children_with_tokens().collect();
    matches!(
        els.get(at),
        Some(NodeOrToken::Node(n))
            if matches!(n.kind(), SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER)
    )
}

/// If the element at `at` is a `NEWLINE`, return `at + 1` (so a splice consumes it),
/// else `at`.
pub(crate) fn extend_over_newline(parent: &SyntaxNode, at: usize) -> usize {
    let els: Vec<_> = parent.children_with_tokens().collect();
    if matches!(els.get(at), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        at + 1
    } else {
        at
    }
}

/// The `[start, end)` child-index range of the comment block beginning at `first`
/// within `parent`: consecutive `COMMENT` tokens separated by single newlines.
pub(crate) fn comment_block_range(parent: &SyntaxNode, first: &SyntaxToken) -> (usize, usize) {
    let els: Vec<_> = parent.children_with_tokens().collect();
    let start = first.index();
    let mut end = start + 1; // one past the first COMMENT
    let mut i = end;
    while i + 1 < els.len() {
        let sep_is_single_nl = matches!(&els[i], NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::NEWLINE && t.text().matches('\n').count() == 1);
        // Inside an array the next comment line is indented, so a WHITESPACE
        // token sits between the NEWLINE and the COMMENT — step over it. (At
        // root/table scope there is no such whitespace, so this is a no-op there.)
        let after_sep = if matches!(&els.get(i + 1), Some(NodeOrToken::Token(t))
            if t.kind() == SyntaxKind::WHITESPACE)
        {
            i + 2
        } else {
            i + 1
        };
        let next_is_comment = matches!(els.get(after_sep), Some(NodeOrToken::Token(t))
            if t.kind() == SyntaxKind::COMMENT);
        if sep_is_single_nl && next_is_comment {
            end = after_sep + 1;
            i = after_sep + 1;
        } else {
            break;
        }
    }
    (start, end)
}

pub(crate) fn is_scalar_kind(k: SyntaxKind) -> bool {
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
