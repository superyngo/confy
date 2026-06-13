//! YAML mutation helpers (Tasks 5–6).
//!
//! Sub-task 5a: indent engine (`reindent`), path resolver (`resolve`), opaque
//! guard (`is_opaque`).
//! Sub-task 5b: atomic dispatcher (`apply`), `serialize_fragment`, opaque
//! rejection pre-check.
//!
//! Per-variant splice implementations come in Tasks 5c–6; every variant returns
//! `Err(MutateError::Unsupported)` until then.

use crate::model::document::{MutateError, Mutation, OnCollision, Target as MutTarget};
use crate::model::node::Seg;
use crate::model::yaml::project::{walk, Target};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};

// ── Indent engine ─────────────────────────────────────────────────────────────

/// Re-indent every line of `fragment` from `from` leading spaces to `to`.
/// Literal/folded block-scalar bodies shift with their header (uniform shift of
/// all lines preserves their *relative* indentation). Blank lines stay blank.
#[allow(dead_code)] // used by per-variant splice fns in later tasks
pub(crate) fn reindent(fragment: &str, from: usize, to: usize) -> String {
    let mut out = String::with_capacity(fragment.len());
    for line in fragment.split_inclusive('\n') {
        let (content, nl) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        if content.trim().is_empty() {
            out.push_str(content);
            out.push_str(nl);
            continue;
        }
        let stripped = content.strip_prefix(&" ".repeat(from)).unwrap_or(content);
        out.push_str(&" ".repeat(to));
        out.push_str(stripped);
        out.push_str(nl);
    }
    out
}

// ── Resolver ──────────────────────────────────────────────────────────────────

/// Resolve `path` to its source element using the projection's shared index.
/// Re-walks `syntax` (which may be a clone_for_update'd tree) so the returned
/// `Target` nodes are from the same tree as `syntax`.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    idx.into_iter().find(|(p, _)| p == path).map(|(_, t)| t)
}

// ── Opaque guard ──────────────────────────────────────────────────────────────

/// Returns `true` if `path` itself or any strict ancestor path resolves to an
/// `Target::Opaque` — i.e. the path is inside (or is) an opaque span.
///
/// Precondition: `path` is non-empty. The root (`[]`) is never opaque and is
/// guarded out by the caller (`apply`); an empty path here always yields `false`.
fn is_opaque(syntax: &SyntaxNode, path: &[Seg]) -> bool {
    // Check the path itself first.
    if let Some(Target::Opaque(_)) = resolve(syntax, path) {
        return true;
    }
    // Then check every strict prefix (ancestor).
    for len in 1..path.len() {
        if let Some(Target::Opaque(_)) = resolve(syntax, &path[..len]) {
            return true;
        }
    }
    false
}

// ── Per-variant implementations (5c–5e) + remaining stubs ────────────────────

// ── 5c: Replace ───────────────────────────────────────────────────────────────

/// Replace the value at `path` with `fragment`.
///
/// Three cases:
///   (a) Empty path → whole-document replace: reparse fragment as a full YAML
///       doc and splice its ROOT children over the old ROOT children.
///   (b) Path → MapEntry: the fragment may be `key: value` (reuse whole entry)
///       or a bare value (replace just the value child).
///   (c) Path → Element (seq entry): replace the value child of the SEQ_ENTRY.
fn replace(tree: &SyntaxNode, path: &[Seg], fragment: &str) -> Result<(), MutateError> {
    if path.is_empty() {
        // Whole-document replace.
        // Reject multi-doc fragments.
        let doc_markers = fragment
            .split_inclusive('\n')
            .filter(|l| l.trim_start().starts_with("---"))
            .count();
        if doc_markers > 1 {
            return Err(MutateError::Fragment(
                "multi-document YAML is not supported".into(),
            ));
        }
        let green = crate::model::yaml::parse::parse(fragment).map_err(MutateError::Fragment)?;
        let new_root_immutable = SyntaxNode::new_root(green);
        let new_root = new_root_immutable.clone_for_update();
        let n = tree.children_with_tokens().count();
        let new_children: Vec<_> = new_root.children_with_tokens().collect();
        tree.splice_children(0..n, new_children);
        return Ok(());
    }

    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(entry) => {
            // An entry whose value is an opaque (out-of-subset) construct is
            // read-only — like Delete, reject before touching the tree.
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            // Detect the entry's indent depth (the INDENT token inside it).
            let entry_indent = entry_indent_depth(&entry);
            // Normalize the fragment to indent 0 for parsing, then build a
            // correctly-indented replacement entry.
            let frag_trimmed = ensure_newline(&reindent(
                &ensure_newline(fragment),
                fragment_indent(fragment),
                0,
            ));
            // Try to parse fragment as a full `key: value` entry at indent 0.
            if let Some(new_entry_0) = parse_map_entry_fragment(&frag_trimmed) {
                // Build the final entry at the target indent by re-building
                // the entry text with the correct leading spaces.
                let new_text = reindent(&new_entry_0.text().to_string(), 0, entry_indent);
                if let Some(new_entry) = parse_map_entry_fragment(&new_text) {
                    replace_node(&entry, new_entry);
                } else {
                    // Edge case: re-parse at correct indent failed (shouldn't happen for
                    // simple entries); fall back to whole-document replace strategy.
                    let whole = tree.to_string();
                    let offset: usize = entry.text_range().start().into();
                    let end: usize = entry.text_range().end().into();
                    let new_doc = format!("{}{}{}", &whole[..offset], new_text, &whole[end..]);
                    let new_green = crate::model::yaml::parse::parse(&new_doc)
                        .map_err(MutateError::Fragment)?;
                    let new_root = SyntaxNode::new_root(new_green).clone_for_update();
                    let n = tree.children_with_tokens().count();
                    let children: Vec<_> = new_root.children_with_tokens().collect();
                    tree.splice_children(0..n, children);
                }
            } else {
                // Bare value: replace just the value child of the entry.
                let new_value = parse_value_fragment(fragment)?;
                if let Some(old_value) = entry.children().find(|c| {
                    matches!(
                        c.kind(),
                        SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE
                    )
                }) {
                    replace_node(&old_value, new_value);
                } else {
                    // Entry currently has no value child (implicit null):
                    // rebuild the entry by re-parsing the whole entry.
                    let key_text = entry_key_text(&entry);
                    let spaces = " ".repeat(entry_indent);
                    let rebuilt = format!("{spaces}{key_text}: {}\n", fragment.trim());
                    if let Some(new_entry) = parse_map_entry_fragment(&rebuilt) {
                        replace_node(&entry, new_entry);
                    } else {
                        return Err(MutateError::Fragment(
                            "could not build replacement entry".into(),
                        ));
                    }
                }
            }
            Ok(())
        }
        Target::Element(entry) => {
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            // Seq entry: replace the VALUE/MAPPING/SEQUENCE child.
            let new_value = parse_value_fragment(fragment)?;
            if let Some(old_value) = entry.children().find(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE
                )
            }) {
                replace_node(&old_value, new_value);
            } else {
                return Err(MutateError::NotFound);
            }
            Ok(())
        }
        Target::Comment(_) => Err(MutateError::Illegal(
            "use EditComment to edit a comment".into(),
        )),
        Target::Opaque(_) => Err(MutateError::Unsupported),
    }
}

/// Replace a SyntaxNode in-place within its parent.
fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has a parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}

/// Returns the indent depth of a MAP_ENTRY or SEQ_ENTRY node (spaces before content).
fn entry_indent_depth(entry: &SyntaxNode) -> usize {
    for c in entry.children_with_tokens() {
        if let rowan::NodeOrToken::Token(t) = c {
            if t.kind() == SyntaxKind::INDENT {
                return t.text().len();
            }
            // First non-trivia token — no indent.
            break;
        }
    }
    0
}

/// Ensure a string ends with a newline.
fn ensure_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Extract the key text from a MAP_ENTRY's KEY child.
fn entry_key_text(entry: &SyntaxNode) -> String {
    entry
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .and_then(|k| {
            k.children_with_tokens().find_map(|c| match c {
                rowan::NodeOrToken::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                    ) =>
                {
                    Some(t.text().to_string())
                }
                _ => None,
            })
        })
        .unwrap_or_default()
}

/// Parse `fragment` as a `key: value` map entry.
/// Returns `None` if the fragment doesn't contain a COLON at the top level.
fn parse_map_entry_fragment(fragment: &str) -> Option<SyntaxNode> {
    // Must contain `: ` to be a keyed fragment.
    if !fragment.contains(": ") && !fragment.ends_with(":\n") && !fragment.ends_with(':') {
        return None;
    }
    // Ensure it ends with a newline for the parser.
    let owned;
    let src = if fragment.ends_with('\n') {
        fragment
    } else {
        owned = format!("{fragment}\n");
        &owned
    };
    let green = crate::model::yaml::parse::parse(src).ok()?;
    let root = SyntaxNode::new_root(green);
    let mapping = root.children().find(|n| n.kind() == SyntaxKind::MAPPING)?;
    let entry = mapping
        .children()
        .find(|n| n.kind() == SyntaxKind::MAP_ENTRY)?;
    // Exactly one entry.
    if mapping
        .children()
        .filter(|n| n.kind() == SyntaxKind::MAP_ENTRY)
        .count()
        == 1
    {
        Some(entry.clone_for_update())
    } else {
        None
    }
}

/// Parse `fragment` as a bare YAML value (MAPPING, SEQUENCE, or scalar).
/// Returns the inner value SyntaxNode (MAPPING, SEQUENCE, or VALUE).
fn parse_value_fragment(fragment: &str) -> Result<SyntaxNode, MutateError> {
    // Wrap as a dummy `__v__: <fragment>` entry and extract the value child.
    let src = if fragment.trim().ends_with('\n') || fragment.trim().is_empty() {
        let mut owned = format!("__v__: {fragment}");
        if !owned.ends_with('\n') {
            owned.push('\n');
        }
        owned
    } else {
        format!("__v__: {fragment}\n")
    };
    let green = crate::model::yaml::parse::parse(&src).map_err(MutateError::Fragment)?;
    let root = SyntaxNode::new_root(green);
    let mapping = root
        .children()
        .find(|n| n.kind() == SyntaxKind::MAPPING)
        .ok_or_else(|| MutateError::Fragment("could not parse value fragment".into()))?;
    let entry = mapping
        .children()
        .find(|n| n.kind() == SyntaxKind::MAP_ENTRY)
        .ok_or_else(|| MutateError::Fragment("could not parse value fragment".into()))?;
    // The value child is MAPPING, SEQUENCE, or VALUE.
    entry
        .children()
        .find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE
            )
        })
        .map(|n| n.clone_for_update())
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))
}

// ── 5d: Delete ────────────────────────────────────────────────────────────────

/// Delete a map entry, sequence element, or standalone comment block.
/// Each MAP_ENTRY / SEQ_ENTRY node already includes its own NEWLINE token, so
/// removing the node from its parent MAPPING / SEQUENCE is all we need.
/// Comment tokens (COMMENT + NEWLINE) are free children of their container.
fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(entry) => {
            // If the entry's value is an opaque node, block mutation.
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            delete_node(&entry);
            Ok(())
        }
        Target::Element(entry) => {
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            delete_node(&entry);
            Ok(())
        }
        Target::Comment(tok) => {
            delete_comment_token(&tok);
            Ok(())
        }
        Target::Opaque(node) => {
            // Root-level opaque nodes: block.
            let _ = node;
            Err(MutateError::Unsupported)
        }
    }
}

/// Returns true if the entry (MAP_ENTRY or SEQ_ENTRY) contains an OPAQUE value child
/// at any depth, indicating the entry is read-only.
fn entry_has_opaque_value(entry: &SyntaxNode) -> bool {
    entry.descendants().any(|n| n.kind() == SyntaxKind::OPAQUE)
}

/// Remove a MAP_ENTRY or SEQ_ENTRY node from its parent.
/// The node already contains its own trailing NEWLINE, so the splice is clean.
fn delete_node(node: &SyntaxNode) {
    let parent = node.parent().expect("node has parent");
    let children: Vec<_> = parent.children_with_tokens().collect();
    let node_idx = children
        .iter()
        .position(|c| match c {
            rowan::NodeOrToken::Node(sn) => sn == node,
            _ => false,
        })
        .expect("node is child of parent");
    parent.splice_children(node_idx..node_idx + 1, vec![]);
}

/// Delete a standalone COMMENT token and its associated NEWLINE.
/// COMMENT tokens are free children at the MAPPING / SEQUENCE / ROOT level.
/// Layout: …NEWLINE? INDENT? COMMENT NEWLINE…
/// We remove: optional preceding INDENT, the COMMENT, and the following NEWLINE.
fn delete_comment_token(tok: &crate::model::yaml::syntax::SyntaxToken) {
    let parent = tok.parent().expect("parent");
    let children: Vec<_> = parent.children_with_tokens().collect();
    let n = children.len();

    // Find the comment token's index.
    let tok_idx = children
        .iter()
        .position(|c| match c {
            rowan::NodeOrToken::Token(t) => t == tok,
            _ => false,
        })
        .expect("token is child of parent");

    let mut start = tok_idx;
    let mut end = tok_idx + 1;

    // Eat the following NEWLINE.
    if end < n {
        if let rowan::NodeOrToken::Token(t) = &children[end] {
            if t.kind() == SyntaxKind::NEWLINE {
                end += 1;
            }
        }
    }

    // Eat a preceding INDENT (leading whitespace of this comment's line).
    if start > 0 {
        if let rowan::NodeOrToken::Token(t) = &children[start - 1] {
            if t.kind() == SyntaxKind::INDENT {
                start -= 1;
            }
        }
    }

    parent.splice_children(start..end, vec![]);
}

// ── 5e: Insert ────────────────────────────────────────────────────────────────

/// Find the MAPPING or SEQUENCE container that is the child collection for `parent_path`.
/// For root level, returns the top-level MAPPING or SEQUENCE child of ROOT.
/// For deeper levels, walks the path to find the innermost container.
fn find_container(tree: &SyntaxNode, parent_path: &[Seg]) -> Result<SyntaxNode, MutateError> {
    // Top-level container is the direct child of ROOT that is MAPPING or SEQUENCE.
    let top = tree
        .children()
        .find(|n| matches!(n.kind(), SyntaxKind::MAPPING | SyntaxKind::SEQUENCE))
        .ok_or(MutateError::NotFound)?;

    if parent_path.is_empty() {
        return Ok(top);
    }

    let mut container = top;
    for seg in parent_path {
        container = match seg {
            Seg::Key(k) => {
                // Find MAP_ENTRY with this key, then its value MAPPING or SEQUENCE.
                let entry = container
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::MAP_ENTRY)
                    .find(|e| entry_key_text(e) == k.as_str())
                    .ok_or(MutateError::NotFound)?;
                entry
                    .children()
                    .find(|c| matches!(c.kind(), SyntaxKind::MAPPING | SyntaxKind::SEQUENCE))
                    .ok_or(MutateError::NotFound)?
            }
            Seg::Index(i) => {
                // Find the i-th SEQ_ENTRY, then its value MAPPING or SEQUENCE.
                let entry = container
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::SEQ_ENTRY)
                    .nth(*i)
                    .ok_or(MutateError::NotFound)?;
                entry
                    .children()
                    .find(|c| matches!(c.kind(), SyntaxKind::MAPPING | SyntaxKind::SEQUENCE))
                    .ok_or(MutateError::NotFound)?
            }
        };
    }
    Ok(container)
}

/// The ordered "slot elements" of a MAPPING or SEQUENCE: each MAP_ENTRY/SEQ_ENTRY
/// node and each standalone COMMENT token, in document order. Single source of
/// truth for what counts as an item — `collect_items` and the index lookups all
/// build on it so their positions can never drift.
fn slot_elements(
    container: &SyntaxNode,
) -> Vec<rowan::NodeOrToken<SyntaxNode, crate::model::yaml::syntax::SyntaxToken>> {
    container
        .children_with_tokens()
        .filter(|c| match c {
            rowan::NodeOrToken::Node(n) => {
                matches!(n.kind(), SyntaxKind::MAP_ENTRY | SyntaxKind::SEQ_ENTRY)
            }
            rowan::NodeOrToken::Token(t) => t.kind() == SyntaxKind::COMMENT,
        })
        .collect()
}

/// Collect the slot items as verbatim text strings, newline-terminated.
/// Order matches projection order (same traversal as project.rs). A COMMENT token
/// excludes its line's trailing NEWLINE (a separate token), so re-add it to keep
/// comment items newline-terminated like entry items — else concatenation in
/// `rebuild_and_splice` would run lines together.
fn collect_items(container: &SyntaxNode) -> Vec<String> {
    slot_elements(container)
        .iter()
        .map(|el| match el {
            rowan::NodeOrToken::Node(n) => n.text().to_string(),
            rowan::NodeOrToken::Token(t) => format!("{}\n", t.text().trim_end()),
        })
        .collect()
}

/// Position of an entry `node` among the slot items, matched by node identity (not
/// text) so duplicate-valued siblings resolve correctly.
fn item_index_of_node(container: &SyntaxNode, node: &SyntaxNode) -> Option<usize> {
    slot_elements(container)
        .iter()
        .position(|el| matches!(el, rowan::NodeOrToken::Node(n) if n == node))
}

/// Position of a standalone COMMENT `tok` among the slot items, matched by token
/// identity so duplicate-text comment blocks resolve correctly.
fn item_index_of_comment(
    container: &SyntaxNode,
    tok: &crate::model::yaml::syntax::SyntaxToken,
) -> Option<usize> {
    slot_elements(container)
        .iter()
        .position(|el| matches!(el, rowan::NodeOrToken::Token(t) if t == tok))
}

/// Detect the indentation depth of a container's entries (number of leading spaces).
/// Returns 0 for root-level containers.
fn container_indent(container: &SyntaxNode) -> usize {
    // Look at the first MAP_ENTRY or SEQ_ENTRY and count its leading INDENT.
    for child in container.children() {
        if matches!(child.kind(), SyntaxKind::MAP_ENTRY | SyntaxKind::SEQ_ENTRY) {
            // The INDENT token is the first child of the entry (if the entry is indented).
            for c in child.children_with_tokens() {
                if let rowan::NodeOrToken::Token(t) = c {
                    if t.kind() == SyntaxKind::INDENT {
                        return t.text().len();
                    }
                    // If first token is not INDENT, entry is at column 0.
                    break;
                }
            }
            break;
        }
    }
    0
}

/// Extract the key name from the item text of a MAP_ENTRY (everything before `: `).
fn item_key_name(item: &str) -> Option<String> {
    // Strip leading indent.
    let t = item.trim_start();
    // Find `: ` at depth 0.
    let colon = t.find(": ")?;
    Some(t[..colon].trim_matches('\'').trim_matches('"').to_string())
}

/// Collect existing map key names from the container.
fn existing_map_keys(container: &SyntaxNode) -> Vec<String> {
    container
        .children()
        .filter(|n| n.kind() == SyntaxKind::MAP_ENTRY)
        .map(|e| entry_key_text(&e))
        .collect()
}

/// Adapt a fragment for insertion into `container`.
///
/// - keyed (`b: 2`) into MAPPING → use as-is, key = Some("b")
/// - keyed (`b: 2`) into SEQUENCE → wrap as `- b: 2` element, key = None
/// - bare value (`5`) into MAPPING → synthesize `placeholder: 5`, key = Some("placeholder")
/// - bare value (`5`) into SEQUENCE → use as `- 5`, key = None
///
/// Returns `(item_text, Option<key_name>)`.
fn adapt_fragment(
    fragment: &str,
    is_mapping: bool,
    dest_indent: usize,
) -> Result<(String, Option<String>), MutateError> {
    let frag = fragment.trim_end_matches('\n');

    // Detect if fragment is keyed by checking for `: ` at depth 0.
    let trimmed = frag.trim_start();
    let is_keyed = trimmed.contains(": ") || trimmed.ends_with(':');

    if is_keyed {
        // Re-indent the fragment to dest_indent.
        let reindented = reindent(&format!("{frag}\n"), fragment_indent(frag), dest_indent);
        let key = item_key_name(&reindented).or_else(|| item_key_name(frag));
        if is_mapping {
            Ok((reindented, key))
        } else {
            // keyed fragment into SEQUENCE → `- key: value`
            // The fragment at dest_indent becomes: "  key: value\n"
            // We need to produce: "<dest_indent_spaces>- key: value\n"
            // Strip the leading spaces from reindented then prefix with "- ".
            let stripped = reindented.trim_start().to_string();
            let spaces = " ".repeat(dest_indent);
            Ok((format!("{spaces}- {stripped}"), None))
        }
    } else {
        // Bare value.
        let val = trimmed.to_string();
        if is_mapping {
            let placeholder = format!("{}: {val}", " ".repeat(dest_indent) + "placeholder");
            // Ensure trailing newline.
            let text = if placeholder.ends_with('\n') {
                placeholder
            } else {
                format!("{placeholder}\n")
            };
            Ok((text, Some("placeholder".to_string())))
        } else {
            let spaces = " ".repeat(dest_indent);
            Ok((format!("{spaces}- {val}\n"), None))
        }
    }
}

/// Detect the leading-indent count of a fragment (first non-blank line's indent).
fn fragment_indent(fragment: &str) -> usize {
    for line in fragment.lines() {
        if !line.trim().is_empty() {
            return line.len() - line.trim_start().len();
        }
    }
    0
}

/// Build the complete document text with the container's content replaced by `new_content`.
/// Uses the container's text_range to do a string-level splice on the full document text.
fn rebuild_and_splice(
    tree: &SyntaxNode,
    container: &SyntaxNode,
    items: &[String],
) -> Result<(), MutateError> {
    let full_text = tree.to_string();
    let offset: usize = container.text_range().start().into();
    let end_offset: usize = container.text_range().end().into();

    let new_content: String = items.iter().cloned().collect();
    let new_doc = format!(
        "{}{}{}",
        &full_text[..offset],
        new_content,
        &full_text[end_offset..]
    );

    // Re-parse the rebuilt document and replace the whole ROOT.
    let new_green = crate::model::yaml::parse::parse(&new_doc).map_err(MutateError::Illegal)?;
    let new_root = SyntaxNode::new_root(new_green).clone_for_update();
    let n = tree.children_with_tokens().count();
    let new_children: Vec<_> = new_root.children_with_tokens().collect();
    tree.splice_children(0..n, new_children);
    Ok(())
}

/// Insert a new member/element into the container at `target`.
fn insert(
    tree: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    // Find the container MAPPING or SEQUENCE.
    let container = find_container(tree, &target.parent)?;
    let is_mapping = container.kind() == SyntaxKind::MAPPING;
    let dest_indent = container_indent(&container);

    // Collect existing items.
    let mut items: Vec<String> = collect_items(&container);

    // Adapt the fragment to the destination.
    let (new_item, new_key) = adapt_fragment(fragment, is_mapping, dest_indent)?;

    // Collision check for mappings.
    let mut final_item = new_item;
    if is_mapping {
        if let Some(key) = &new_key {
            let existing = existing_map_keys(&container);
            if existing.iter().any(|k| k == key) {
                match on_collision {
                    OnCollision::Cancel => {
                        return Err(MutateError::Collision(key.clone()));
                    }
                    OnCollision::Overwrite => {
                        // Remove the existing item with this key.
                        let ci = items
                            .iter()
                            .position(|it| item_key_name(it).as_deref() == Some(key.as_str()));
                        if let Some(ci) = ci {
                            items.remove(ci);
                        }
                    }
                    OnCollision::Rename => {
                        let mut n = 2usize;
                        loop {
                            let candidate = format!("{key}{n}");
                            if !existing.iter().any(|k| k == &candidate) {
                                // Rebuild item with renamed key.
                                let spaces = " ".repeat(dest_indent);
                                let trimmed_frag = fragment.trim();
                                let val_part = trimmed_frag
                                    .split_once(": ")
                                    .map(|x| x.1)
                                    .unwrap_or(trimmed_frag)
                                    .trim_end_matches('\n');
                                let renamed = format!("{spaces}{candidate}: {val_part}\n");
                                final_item = renamed;
                                break;
                            }
                            n += 1;
                        }
                    }
                }
            }
        }
    }

    // Insert at the clamped index.
    let idx = target.index.min(items.len());
    items.insert(idx, final_item);

    // Rebuild and splice.
    rebuild_and_splice(tree, &container, &items)
}

fn rename(tree: &SyntaxNode, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    let entry = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(e) => e,
        _ => return Err(MutateError::Illegal("rename requires a key".into())),
    };

    // Sibling collision check against the other MAP_ENTRY keys in the same parent.
    let parent = entry.parent().expect("entry has parent");
    for sib in parent
        .children()
        .filter(|n| n.kind() == SyntaxKind::MAP_ENTRY)
    {
        if sib == entry {
            continue;
        }
        if entry_key_text(&sib) == new_key {
            return Err(MutateError::Collision(new_key.to_string()));
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

    // Build a replacement scalar token by parsing a probe `new_key: 0`.
    let probe = format!("{new_key}: 0\n");
    let new_entry = parse_map_entry_fragment(&probe).ok_or(MutateError::Illegal(
        "new key does not parse as a map entry".into(),
    ))?;
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
fn comment_out(text: &str) -> String {
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
fn uncomment(text: &str) -> String {
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

fn remark(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
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
            let container = first_tok.parent().expect("comment has parent");
            let items = collect_items(&container);
            let block_text = comment_block_text(&first_tok);
            let block_lines: Vec<&str> = block_text.lines().collect();
            let pos = item_index_of_comment(&container, &first_tok).ok_or(MutateError::NotFound)?;

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

            let mut new_items = items.clone();
            // The block occupies `block_lines.len()` consecutive comment items.
            let span = block_lines.len();
            new_items.splice(pos..pos + span, [ensure_newline(&recovered)]);
            rebuild_and_splice(tree, &container, &new_items)
        }
        Target::Opaque(_) => Err(MutateError::Unsupported),
    }
}

fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
    // Validate: every line must start with `#` (after leading whitespace).
    for line in text.lines() {
        if !line.trim_start().starts_with('#') {
            return Err(MutateError::Fragment(
                "every line of a comment must start with #".into(),
            ));
        }
    }

    let first_tok = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Comment(t) => t,
        Target::Opaque(_) => return Err(MutateError::Unsupported),
        _ => {
            return Err(MutateError::Illegal(
                "path does not resolve to a comment".into(),
            ))
        }
    };

    let container = first_tok.parent().expect("comment has parent");
    let items = collect_items(&container);
    let block_text = comment_block_text(&first_tok);
    let block_lines: Vec<&str> = block_text.lines().collect();
    let pos = item_index_of_comment(&container, &first_tok).ok_or(MutateError::NotFound)?;

    let mut new_items = items.clone();
    new_items.splice(pos..pos + block_lines.len(), [ensure_newline(text)]);
    rebuild_and_splice(tree, &container, &new_items)
}

fn insert_comment(tree: &SyntaxNode, target: &MutTarget, text: &str) -> Result<(), MutateError> {
    // Validate: every line must start with `#` (after leading whitespace).
    for line in text.lines() {
        if !line.trim_start().starts_with('#') {
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

    let idx = target.index.min(items.len());
    items.insert(idx, reindented);
    rebuild_and_splice(tree, &container, &items)
}

fn move_nodes(
    _tree: &SyntaxNode,
    _sources: &[Vec<Seg>],
    _target: &MutTarget,
    _on_collision: OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn convert_kind(
    _tree: &SyntaxNode,
    _path: &[Seg],
    _target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

// ── Semantic validator ────────────────────────────────────────────────────────

/// Backstop after a splice: re-parse and reject duplicate mapping keys
/// (Collision) or structural breakage (Illegal). Mirrors json/edit.rs's DOM
/// check using YAML re-parse + walk-based duplicate-key detection.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    // Re-walk and check for duplicate keys at every mapping level.
    let (node_tree, _idx) = walk(&reparsed, "");
    check_duplicate_keys(&node_tree.root.children)?;
    Ok(())
}

/// Recursively check for duplicate key names among siblings at each level.
fn check_duplicate_keys(nodes: &[crate::model::node::Node]) -> Result<(), MutateError> {
    let mut seen = std::collections::HashSet::new();
    for node in nodes {
        if let crate::model::node::NodeKind::Comment(_) = &node.kind {
            // Comments use Index paths — not keyed, no collision.
        } else if let Some(Seg::Key(k)) = node.path.last() {
            if !seen.insert(k.clone()) {
                return Err(MutateError::Collision(k.clone()));
            }
        }
        check_duplicate_keys(&node.children)?;
    }
    Ok(())
}

// ── Serialize fragment ────────────────────────────────────────────────────────

/// Collect the raw text of a merged standalone `#` comment block from its first
/// COMMENT token: consecutive COMMENT tokens separated only by a single NEWLINE
/// (+ optional INDENT/WHITESPACE) join with `\n`. A second consecutive NEWLINE
/// ends the block (matches project.rs comment-merge logic).
fn comment_block_text(first: &crate::model::yaml::syntax::SyntaxToken) -> String {
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
    match resolve(syntax, path) {
        Some(Target::MapEntry(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Element(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Opaque(node)) => node.text().to_string().trim_end().to_string(),
        None => String::new(),
    }
}

// ── Atomic dispatcher ─────────────────────────────────────────────────────────

/// Extract the primary path(s) from a mutation for the opaque pre-check.
fn mutation_paths(m: &Mutation) -> Vec<&Vec<Seg>> {
    match m {
        Mutation::Delete { path } => vec![path],
        Mutation::Insert { target, .. } => vec![&target.parent],
        Mutation::Replace { path, .. } => vec![path],
        Mutation::Rename { path, .. } => vec![path],
        Mutation::Remark { path } => vec![path],
        Mutation::EditComment { path, .. } => vec![path],
        Mutation::InsertComment { target, .. } => vec![&target.parent],
        Mutation::Move {
            sources, target, ..
        } => {
            let mut paths: Vec<&Vec<Seg>> = sources.iter().collect();
            paths.push(&target.parent);
            paths
        }
        Mutation::ConvertKind { path, .. } => vec![path],
    }
}

pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    // Opaque pre-check: any target path inside (or equal to) an opaque span → Unsupported.
    for path in mutation_paths(&m) {
        if !path.is_empty() && is_opaque(syntax, path) {
            return Err(MutateError::Unsupported);
        }
    }

    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
        } => insert(&tree, &target, &fragment, on_collision)?,
        Mutation::Rename { path, new_key } => rename(&tree, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => move_nodes(&tree, &sources, &target, on_collision)?,
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &path, target)?,
    }
    validate_semantics(&tree)?;
    Ok(tree)
}

// ── Test helpers (pub(crate) so later chunk tests can import them) ────────────

#[cfg(test)]
pub(crate) fn parse_syntax(src: &str) -> SyntaxNode {
    SyntaxNode::new_root(
        crate::model::yaml::parse::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}")),
    )
}

/// Parse `src`, apply `m`, and return the serialized result.
/// Used by per-variant tests across later chunks.
#[cfg(test)]
pub(crate) fn apply_str(
    src: &str,
    m: crate::model::document::Mutation,
) -> Result<String, crate::model::document::MutateError> {
    let t = parse_syntax(src);
    apply(&t, m).map(|tree| tree.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{MutateError, Mutation};
    use crate::model::node::Seg;

    // ── Indent engine tests ───────────────────────────────────────────────────

    #[test]
    fn reindent_shifts_every_line() {
        assert_eq!(
            reindent("a: 1\nb:\n  c: 2\n", 0, 4),
            "    a: 1\n    b:\n      c: 2\n"
        );
        assert_eq!(reindent("    x: 1\n", 4, 0), "x: 1\n");
    }

    #[test]
    fn reindent_preserves_block_scalar_body_relative_indent() {
        let frag = "note: |\n  line one\n  line two\n";
        assert_eq!(
            reindent(frag, 0, 2),
            "  note: |\n    line one\n    line two\n"
        );
    }

    // ── Opaque rejection test ─────────────────────────────────────────────────

    #[test]
    fn mutations_on_opaque_are_unsupported() {
        let src = "ref: *anchor\nk: 1\n";
        let s = parse_syntax(src);
        let m = Mutation::Delete {
            path: vec![Seg::Key("ref".into())],
        };
        assert!(matches!(apply(&s, m), Err(MutateError::Unsupported)));
    }

    // ── serialize_fragment tests ─────────────────────────────────────────────

    #[test]
    fn fragment_of_map_entry() {
        let s = parse_syntax("a: 1\nb: hello\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("a".into())]), "a: 1");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("b".into())]), "b: hello");
    }

    #[test]
    fn fragment_of_seq_entry() {
        let s = parse_syntax("- 10\n- 20\n- 30\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Index(1)]), "- 20");
    }

    #[test]
    fn fragment_of_comment() {
        let s = parse_syntax("# hello\na: 1\n");
        // Comment is at index 0.
        assert_eq!(serialize_fragment(&s, &[Seg::Index(0)]), "# hello");
    }

    #[test]
    fn fragment_of_unknown_path_is_empty() {
        let s = parse_syntax("a: 1\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("nope".into())]), "");
    }

    // ── 5f: Rename ───────────────────────────────────────────────────────────

    #[test]
    fn rename_key_token_in_place() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "c".into(),
            },
        )
        .expect("rename should succeed");
        assert_eq!(out, "c: 1\n");
    }

    #[test]
    fn rename_onto_existing_sibling_is_collision() {
        let r = apply_str(
            "a: 1\nb: 2\n",
            Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "b".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Collision(_))),
            "rename onto sibling expected Collision, got {r:?}"
        );
    }

    #[test]
    fn rename_non_key_is_illegal() {
        let r = apply_str(
            "- 1\n- 2\n",
            Mutation::Rename {
                path: vec![Seg::Index(0)],
                new_key: "x".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "rename of seq element expected Illegal, got {r:?}"
        );
    }

    // ── 5g: Remark ───────────────────────────────────────────────────────────

    #[test]
    fn remark_entry_to_comment() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Remark {
                path: vec![Seg::Key("a".into())],
            },
        )
        .expect("remark entry should succeed");
        assert_eq!(out, "# a: 1\n");
    }

    #[test]
    fn remark_comment_to_entry() {
        let out = apply_str(
            "# a: 1\n",
            Mutation::Remark {
                path: vec![Seg::Index(0)],
            },
        )
        .expect("remark comment should succeed");
        assert_eq!(out, "a: 1\n");
    }

    #[test]
    fn remark_nested_entry_preserves_indent() {
        let src = "srv:\n  host: a\n  port: 80\n";
        let out = apply_str(
            src,
            Mutation::Remark {
                path: vec![Seg::Key("srv".into()), Seg::Key("host".into())],
            },
        )
        .expect("remark nested entry");
        assert_eq!(out, "srv:\n  # host: a\n  port: 80\n");
    }

    #[test]
    fn remark_duplicate_sequence_element_targets_the_right_one() {
        // Two identical elements: remarking index 2 must comment the THIRD,
        // not the first (identity-based position, not text match).
        let src = "- x\n- y\n- x\n";
        let out = apply_str(
            src,
            Mutation::Remark {
                path: vec![Seg::Index(2)],
            },
        )
        .expect("remark dup element");
        assert_eq!(out, "- x\n- y\n# - x\n");
    }

    #[test]
    fn edit_comment_duplicate_first_line_targets_the_right_block() {
        // Two comment blocks share the first line `# TODO`; a blank line breaks
        // them into two projected Comment nodes (Index 0 and 1). Editing the
        // SECOND must rewrite that block, leaving the first untouched.
        let src = "# TODO\n# a\n\n# TODO\n# b\nk: 1\n";
        let out = apply_str(
            src,
            Mutation::EditComment {
                path: vec![Seg::Index(1)],
                text: "# DONE".into(),
            },
        )
        .expect("edit second dup-first-line block");
        assert!(
            out.starts_with("# TODO\n# a\n") && out.contains("# DONE") && !out.contains("# b"),
            "expected first block intact and second→# DONE, got {out:?}"
        );
    }

    // ── 5h: EditComment ──────────────────────────────────────────────────────

    #[test]
    fn edit_comment_rewrites_block() {
        let out = apply_str(
            "# old\n",
            Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "# new".into(),
            },
        )
        .expect("edit comment should succeed");
        assert_eq!(out, "# new\n");
    }

    #[test]
    fn edit_comment_non_hash_rejected() {
        let r = apply_str(
            "# old\n",
            Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "not a comment".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "non-# text expected Fragment, got {r:?}"
        );
    }

    // ── 5i: InsertComment ────────────────────────────────────────────────────

    #[test]
    fn insert_comment_at_front() {
        use crate::model::document::Target;
        let out = apply_str(
            "a: 1\n",
            Mutation::InsertComment {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                text: "# note".into(),
            },
        )
        .expect("insert comment should succeed");
        assert_eq!(out, "# note\na: 1\n");
    }

    #[test]
    fn insert_member_after_leading_comment_is_not_mangled() {
        // A pre-existing standalone comment must keep its newline through the
        // collect_items/rebuild round-trip (regression for run-together lines).
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "# header\na: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 2,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert after leading comment");
        assert_eq!(out, "# header\na: 1\nb: 2\n");
    }

    #[test]
    fn insert_comment_non_hash_rejected() {
        use crate::model::document::Target;
        let r = apply_str(
            "a: 1\n",
            Mutation::InsertComment {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                text: "nope".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "non-# text expected Fragment, got {r:?}"
        );
    }

    // ── 5c: Replace ─────────────────────────────────────────────────────────

    #[test]
    fn replace_inline_scalar_value() {
        let out = apply_str(
            "k: 1\n",
            Mutation::Replace {
                path: vec![Seg::Key("k".into())],
                fragment: "k: 2".into(),
            },
        )
        .expect("replace should succeed");
        assert_eq!(out, "k: 2\n");
    }

    #[test]
    fn replace_block_mapping_value() {
        // Replace `host: a` inside `srv:` with `host: b`.
        let src = "srv:\n  host: a\n  port: 80\n";
        let out = apply_str(
            src,
            Mutation::Replace {
                path: vec![Seg::Key("srv".into()), Seg::Key("host".into())],
                fragment: "host: b".into(),
            },
        )
        .expect("replace should succeed");
        assert_eq!(out, "srv:\n  host: b\n  port: 80\n");
    }

    #[test]
    fn replace_whole_document_valid() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Replace {
                path: vec![],
                fragment: "b: 2\n".into(),
            },
        )
        .expect("whole-doc replace should succeed");
        assert_eq!(out, "b: 2\n");
    }

    #[test]
    fn replace_whole_document_multi_doc_rejected() {
        let r = apply_str(
            "a: 1\n",
            Mutation::Replace {
                path: vec![],
                fragment: "---\na: 1\n---\nb: 2\n".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "multi-doc replace should be Fragment error, got {r:?}"
        );
    }

    #[test]
    fn replace_over_opaque_value_is_unsupported() {
        // `ref: *anchor` projects as a read-only MapEntry (opaque value);
        // Replace must reject it, leaving the doc untouched.
        let r = apply_str(
            "ref: *anchor\nk: 1\n",
            Mutation::Replace {
                path: vec![Seg::Key("ref".into())],
                fragment: "ref: 5".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Unsupported)),
            "replace over opaque value expected Unsupported, got {r:?}"
        );
    }

    // ── 5d: Delete ─────────────────────────────────────────────────────────

    #[test]
    fn delete_middle_element_of_sequence() {
        let src = "- 10\n- 20\n- 30\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Index(1)],
            },
        )
        .expect("delete middle element should succeed");
        assert_eq!(out, "- 10\n- 30\n");
    }

    #[test]
    fn delete_map_entry_with_nested_children() {
        let src = "srv:\n  host: a\n  port: 80\nother: x\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Key("srv".into())],
            },
        )
        .expect("delete entry with nested children");
        assert_eq!(out, "other: x\n");
    }

    #[test]
    fn delete_standalone_comment() {
        let src = "# hello\na: 1\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Index(0)],
            },
        )
        .expect("delete comment should succeed");
        assert_eq!(out, "a: 1\n");
    }

    // ── 5e: Insert ─────────────────────────────────────────────────────────

    #[test]
    fn insert_member_at_end_of_mapping() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "a: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert member at end");
        assert_eq!(out, "a: 1\nb: 2\n");
    }

    #[test]
    fn insert_keyed_fragment_into_sequence() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "- 1\n- 2\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert keyed fragment into sequence");
        // Keyed fragment into a sequence → becomes a sequence element `- b: 2`
        assert!(
            out.contains("- b: 2"),
            "expected '- b: 2' in output: {out:?}"
        );
    }

    #[test]
    fn insert_bare_value_into_mapping_gets_placeholder_key() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "a: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "5".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert bare value into mapping");
        assert!(
            out.contains("placeholder"),
            "expected 'placeholder' key in output: {out:?}"
        );
    }

    #[test]
    fn insert_into_nested_block_mapping() {
        use crate::model::document::{OnCollision, Target};
        let src = "srv:\n  host: a\n";
        let out = apply_str(
            src,
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("srv".into())],
                    index: 1,
                },
                fragment: "port: 80\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into nested block mapping");
        assert_eq!(out, "srv:\n  host: a\n  port: 80\n");
    }
}
