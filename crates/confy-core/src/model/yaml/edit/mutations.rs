//! `Mutation::Rename`/`Remark`/`EditComment`/`InsertComment`/`Move`/
//! `SetTrailingComment` — split out of `yaml/edit.rs` (Task 15, 2026-08-11
//! audit remediation).

use super::block::{
    collect_items, commit_reparse, container_indent, delete, ensure_newline,
    entry_has_opaque_value, find_container, fragment_indent, insert, item_index_of_node,
    parse_map_entry_fragment, rebuild_and_splice, root_prefix_offset,
};
use super::resolve::{reindent, resolve, resolve_in};
use crate::model::document::{MutateError, OnCollision, Target as MutTarget};
use crate::model::node::Seg;
use crate::model::yaml::project::{entry_key_name, walk, Target, YamlIndex};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};

pub(crate) fn rename(idx: &YamlIndex, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    let entry = match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(e) => e,
        _ => return Err(MutateError::Illegal("rename requires a key".into())),
    };

    // Build a replacement scalar token by parsing a probe `new_key: 0` up
    // front — both the collision check below and the token swap need it.
    // Parsing also *decodes* `new_key`, which the caller may now pass as a
    // literal quoted YAML key (e.g. `"a b"`, quotes included, straight from
    // the rename buffer) rather than a bare name, so the collision check can
    // compare it against siblings' *decoded* names on equal footing.
    let probe = format!("{new_key}: 0\n");
    let new_entry = parse_map_entry_fragment(&probe).ok_or(MutateError::Illegal(
        "new key does not parse as a map entry".into(),
    ))?;
    let decoded_new_key = entry_key_name(&new_entry);

    // Sibling collision check against the other keyed members in the same parent
    // (block MAP_ENTRY or flow FLOW_ENTRY).
    let parent = entry.parent().expect("entry has parent");
    for sib in parent
        .children()
        .filter(|n| matches!(n.kind(), SyntaxKind::MAP_ENTRY | SyntaxKind::FLOW_ENTRY))
    {
        if sib == entry {
            continue;
        }
        if entry_key_name(&sib) == decoded_new_key {
            return Err(MutateError::Collision(decoded_new_key));
        }
    }

    // Locate the KEY node, then its scalar token.
    let key_node = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or(MutateError::NotFound)?;
    let children: Vec<_> = key_node.children_with_tokens().collect();
    let tok_idx = children
        .iter()
        .position(|c| {
            matches!(c, rowan::NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE))
        })
        .ok_or(MutateError::NotFound)?;

    let new_tok = new_entry
        .children()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .and_then(|kn| {
            kn.children_with_tokens().find_map(|c| match c {
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
        })
        .ok_or(MutateError::Illegal("new key has no scalar token".into()))?;

    key_node.splice_children(tok_idx..tok_idx + 1, vec![new_tok.into()]);
    Ok(())
}

/// Prefix `# ` to each non-blank line of `text`, after that line's leading
/// whitespace (so indentation is preserved). Blank lines stay blank.
pub(crate) fn comment_out(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.trim().is_empty() {
                l.to_string()
            } else {
                let indent_len = l.len() - l.trim_start().len();
                format!("{}# {}", &l[..indent_len], &l[indent_len..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip a `# ` (or `#`) prefix from each line of `text`, after that line's
/// leading whitespace. Lines without a `#` are left unchanged.
pub(crate) fn uncomment(text: &str) -> String {
    text.lines()
        .map(|l| {
            let indent_len = l.len() - l.trim_start().len();
            let (indent, rest) = l.split_at(indent_len);
            if let Some(r) = rest.strip_prefix("# ") {
                format!("{indent}{r}")
            } else if let Some(r) = rest.strip_prefix('#') {
                format!("{indent}{r}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn remark(tree: &SyntaxNode, idx: &YamlIndex, path: &[Seg]) -> Result<(), MutateError> {
    match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(entry) | Target::Element(entry) => {
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            let container = entry.parent().expect("entry has parent");
            let items = collect_items(&container);
            let entry_text = entry.text().to_string();
            let pos = item_index_of_node(&container, &entry).ok_or(MutateError::NotFound)?;

            let commented = ensure_newline(&comment_out(entry_text.trim_end()));
            let mut new_items = items.clone();
            new_items[pos] = commented;
            rebuild_and_splice(tree, &container, &new_items)
        }
        Target::Comment(first_tok) => {
            let block_text = comment_block_text(&first_tok);
            // Recover the live text by stripping the comment leader.
            let recovered = uncomment(&block_text);
            // Validate it parses as a map entry or a `- ` sequence element.
            let recovered_nl = ensure_newline(&recovered);
            let valid = parse_map_entry_fragment(&recovered_nl).is_some()
                || recovered.trim_start().starts_with("- ");
            if !valid {
                return Err(MutateError::Fragment(
                    "comment does not parse as a map entry or sequence element".into(),
                ));
            }

            splice_comment_block(tree, &first_tok, &recovered)
        }
        Target::Opaque(_) => Err(MutateError::Unsupported),
    }
}

/// Byte range `[start, end)` covering the comment block beginning at `first`,
/// plus the block's leading-indent width. The range spans an optional leading
/// INDENT token through the NEWLINE that terminates the block's last comment
/// line, so replacing just that slice leaves every sibling — including a
/// ROOT-level MAPPING/SEQUENCE that sits beside a leading comment — untouched.
/// Mirrors the consecutive-comment grouping in `comment_block_text`.
pub(crate) fn comment_block_bounds(
    first: &crate::model::yaml::syntax::SyntaxToken,
) -> (usize, usize, usize) {
    use rowan::NodeOrToken;
    let mut start = usize::from(first.text_range().start());
    let mut indent = 0usize;
    if let Some(NodeOrToken::Token(prev)) = first.prev_sibling_or_token() {
        if prev.kind() == SyntaxKind::INDENT {
            start = usize::from(prev.text_range().start());
            indent = prev.text().chars().count();
        }
    }
    let mut end = usize::from(first.text_range().end());
    let mut sib = first.next_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::INDENT => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines == 1 {
                    end = usize::from(el.text_range().end());
                }
                if newlines >= 2 {
                    break;
                }
            }
            SyntaxKind::COMMENT if newlines == 1 => newlines = 0,
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    (start, end, indent)
}

/// Replace the comment block beginning at `first` with `text` (reindented to the
/// block's own indentation) via a whole-document reparse. Container-agnostic:
/// unlike the slot-item rebuild, this preserves a ROOT-level MAPPING/SEQUENCE
/// sibling, so editing or remarking the leading comment never drops the body.
pub(crate) fn splice_comment_block(
    tree: &SyntaxNode,
    first: &crate::model::yaml::syntax::SyntaxToken,
    text: &str,
) -> Result<(), MutateError> {
    let (start, end, indent) = comment_block_bounds(first);
    let body = ensure_newline(&reindent(
        &ensure_newline(text),
        fragment_indent(text),
        indent,
    ));
    let full = tree.to_string();
    let new_doc = format!("{}{}{}", &full[..start], body, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

pub(crate) fn edit_comment(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    text: &str,
) -> Result<(), MutateError> {
    // Validate: every line must start with `#` (after leading whitespace).
    for line in text.lines() {
        if !line.trim_start().starts_with('#') {
            return Err(MutateError::Fragment(
                "every line of a comment must start with #".into(),
            ));
        }
    }

    let first_tok = match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::Comment(t) => t,
        Target::Opaque(_) => return Err(MutateError::Unsupported),
        _ => {
            return Err(MutateError::Illegal(
                "path does not resolve to a comment".into(),
            ))
        }
    };

    splice_comment_block(tree, &first_tok, text)
}

pub(crate) fn insert_comment(
    tree: &SyntaxNode,
    target: &MutTarget,
    text: &str,
) -> Result<(), MutateError> {
    // Validate: every non-blank line must start with `#` (after leading
    // whitespace). A blank line is allowed so a fragment can carry a separator
    // (used to keep an inserted comment a distinct node, not merged into a
    // neighbour).
    for line in text.lines() {
        if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            return Err(MutateError::Fragment(
                "every line of a comment must start with #".into(),
            ));
        }
    }

    let container = find_container(tree, &target.parent)?;
    let dest_indent = container_indent(&container);
    let mut items = collect_items(&container);

    // Reindent the comment block to the container's indentation.
    let reindented = ensure_newline(&reindent(
        &ensure_newline(text),
        fragment_indent(text),
        dest_indent,
    ));

    let offset = root_prefix_offset(tree, &target.parent);
    let idx = target.index.saturating_sub(offset).min(items.len());
    items.insert(idx, reindented);
    rebuild_and_splice(tree, &container, &items)
}

pub(crate) fn move_nodes(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    sources: &[Vec<Seg>],
    target: &MutTarget,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    if sources.is_empty() {
        return Ok(());
    }

    // ── 0. Opaque source-value guard ────────────────────────────────────────
    // The `apply` dispatcher already rejects a source/target that *is* opaque;
    // additionally reject a source whose VALUE is opaque (mirrors delete/replace).
    for path in sources.iter() {
        match resolve_in(idx, path) {
            Some(Target::MapEntry(entry)) | Some(Target::Element(entry)) => {
                if entry_has_opaque_value(&entry) {
                    return Err(MutateError::Unsupported);
                }
            }
            Some(Target::Opaque(_)) => return Err(MutateError::Unsupported),
            _ => {}
        }
    }

    // ── 1. Capture (path, fragment) pairs BEFORE any deletion ──────────────
    // The source path rides along (same capture shape as the JSON backend) so
    // the re-insert loop below can derive a `<arrayKey>_<index>` suggested key
    // for a bare scalar pulled out of a keyed array.
    let captured: Vec<(Vec<Seg>, String)> = sources
        .iter()
        .map(|path| {
            let frag = fragment_of(resolve_in(idx, path));
            if frag.is_empty() {
                Err(MutateError::NotFound)
            } else {
                Ok((path.clone(), frag))
            }
        })
        .collect::<Result<_, _>>()?;

    // ── 2a. Pre-deletion shift: count same-container sources before target ───
    // `target.index` is a *pre-deletion* ordinal in the parent's *full* child
    // sequence (comments included — the space the TUI's `true_sibling_index`
    // uses). Every source in that same container at a lower ordinal shifts the
    // surviving slots up by one on deletion, so the insert index drops by that
    // many. Covers keyed *and* positional sources (a keyed node moved down past a
    // trailing comment was previously left unadjusted, overshooting the comment).
    let shift = {
        let proj = crate::model::yaml::project::project(tree, "");
        crate::model::node::NodeTree::node_at(&proj, &target.parent)
            .map(|parent| {
                sources
                    .iter()
                    .filter(|s| {
                        parent
                            .children
                            .iter()
                            .position(|c| &c.path == *s)
                            .is_some_and(|ord| ord < target.index)
                    })
                    .count()
            })
            .unwrap_or(0)
    };

    // ── 2. Delete sources back-to-front ─────────────────────────────────────
    // Later/higher-index sources first so earlier sources' indices stay valid.
    let mut delete_indices: Vec<usize> = (0..sources.len()).collect();
    delete_indices.sort_by(|&a, &b| match (sources[a].last(), sources[b].last()) {
        (Some(Seg::Index(ia)), Some(Seg::Index(ib))) => ib.cmp(ia),
        _ => b.cmp(&a),
    });
    for i in delete_indices {
        // Each delete splices the tree, so the shared pre-mutation index is
        // stale — re-walk for a fresh one per deletion.
        let (_, fresh) = walk(tree, "");
        delete(tree, &fresh, &sources[i])?;
    }

    // ── 3. Effective insertion index ────────────────────────────────────────
    let effective_index = target.index - shift.min(target.index);

    // ── 4. Insert each captured fragment at the effective target ─────────────
    for (i, (path, frag)) in captured.iter().enumerate() {
        let insert_target = MutTarget {
            parent: target.parent.clone(),
            index: effective_index + i,
        };
        // Some(`<arrayKey>_<index>`) only when this source is a bare element
        // of a keyed array — exactly the source whose keyless fragment lands
        // in a mapping and needs a key synthesized. Every other source shape
        // (entry, header, nested/unkeyed array element) gets None and keeps
        // the generic placeholder; keyed fragments ignore the suggestion.
        let suggested_key = crate::model::node::array_element_suggested_key(path);
        insert(
            tree,
            &insert_target,
            frag,
            suggested_key.as_deref(),
            on_collision,
        )?;
    }

    Ok(())
}

/// Collect the raw text of a merged standalone `#` comment block from its first
/// COMMENT token: consecutive COMMENT tokens separated only by a single NEWLINE
/// (+ optional INDENT/WHITESPACE) join with `\n`. A second consecutive NEWLINE
/// ends the block (matches project.rs comment-merge logic).
pub(crate) fn comment_block_text(first: &crate::model::yaml::syntax::SyntaxToken) -> String {
    use crate::model::yaml::syntax::SyntaxKind;
    use rowan::NodeOrToken;

    let mut out = vec![first.text().trim_end().to_string()];
    let mut sib = first.next_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::INDENT => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break;
                }
            }
            SyntaxKind::COMMENT if newlines == 1 => {
                if let NodeOrToken::Token(tok) = &el {
                    out.push(tok.text().trim_end().to_string());
                }
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    out.join("\n")
}

/// Serialize the node at `path` as a standalone fragment (for clipboard / `$EDITOR`).
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    fragment_of(resolve(syntax, path))
}

/// The fragment text of a resolved target (shared by `serialize_fragment` and
/// index-based lookups that already hold a `Target`).
pub(crate) fn fragment_of(target: Option<Target>) -> String {
    match target {
        Some(Target::MapEntry(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Element(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Opaque(node)) => node.text().to_string().trim_end().to_string(),
        None => String::new(),
    }
}

/// `Mutation::SetTrailingComment` — set/change/clear the EOL `#` comment of the
/// keyed scalar at `path`. The comment lives inside the `MAP_ENTRY`, after the
/// value child; the splice rewrites from the value's end to the line's newline
/// and reparses. Only a single-line scalar map entry is supported.
pub(crate) fn set_trailing_comment(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    comment: Option<&str>,
) -> Result<(), MutateError> {
    // A block MAP_ENTRY or a block SEQ_ENTRY (array element): both hold their value
    // as a child node and end the line the same way. Flow members/elements are
    // single-line inline collections with no per-item EOL slot — rejected upstream.
    let entry = match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(e) if e.kind() == SyntaxKind::MAP_ENTRY => e,
        Target::Element(e) if e.kind() == SyntaxKind::SEQ_ENTRY => e,
        _ => return Err(MutateError::Unsupported),
    };
    let value = entry
        .children()
        .find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::MAPPING
                    | SyntaxKind::SEQUENCE
                    | SyntaxKind::VALUE
                    | SyntaxKind::FLOW_MAP
                    | SyntaxKind::FLOW_SEQ
            )
        })
        .ok_or(MutateError::Unsupported)?;
    let value_text = value.text().to_string();
    if value_text.contains('\n') {
        // A block-collection value (`key:\n  …`) keeps its comment on the entry's
        // own first line, after `key:`, before the newline that begins the block —
        // so a branch (nested map/seq) is editable. A multi-line block *scalar*
        // (`|` / `>`) has no such slot, and a block value reached through a
        // SEQ_ENTRY (`- key: v`) shares its dash line with content, so both reject.
        if entry.kind() == SyntaxKind::MAP_ENTRY
            && matches!(value.kind(), SyntaxKind::MAPPING | SyntaxKind::SEQUENCE)
        {
            return set_block_entry_trailing(tree, &entry, comment);
        }
        return Err(MutateError::Unsupported);
    }
    // The VALUE node may swallow the spaces before an existing comment; cut at the
    // value's last non-whitespace byte so we don't accumulate separator spaces.
    let value_start: usize = value.text_range().start().into();
    let cut_start = value_start + value_text.trim_end().len();
    let full = tree.to_string();
    let cut_end = full[cut_start..]
        .find('\n')
        .map(|i| cut_start + i)
        .unwrap_or(full.len());
    let tail = match comment {
        Some(c) => format!("  {}", c.trim()),
        None => String::new(),
    };
    let new_text = format!("{}{}{}", &full[..cut_start], tail, &full[cut_end..]);
    commit_reparse(tree, &new_text, MutateError::Fragment)
}

/// Set/change/clear the EOL comment of a block-map parent entry (`key:  # c`).
/// The comment slot is between the entry's `:` and the newline that starts the
/// nested block; the splice rewrites that span and reparses in place.
pub(crate) fn set_block_entry_trailing(
    tree: &SyntaxNode,
    entry: &SyntaxNode,
    comment: Option<&str>,
) -> Result<(), MutateError> {
    let colon = entry
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::COLON)
        .ok_or(MutateError::Unsupported)?;
    let cut_start: usize = colon.text_range().end().into();
    let full = tree.to_string();
    let cut_end = full[cut_start..]
        .find('\n')
        .map(|i| cut_start + i)
        .unwrap_or(full.len());
    let tail = match comment {
        Some(c) => format!("  {}", c.trim()),
        None => String::new(),
    };
    let new_text = format!("{}{}{}", &full[..cut_start], tail, &full[cut_end..]);
    commit_reparse(tree, &new_text, MutateError::Fragment)
}
