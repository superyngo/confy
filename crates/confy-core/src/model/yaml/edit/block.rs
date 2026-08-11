//! Block-style map/seq structural edits: `Mutation::Replace`/`Delete`/
//! `Insert` and the container/fragment-adaptation machinery they share —
//! split out of `yaml/edit.rs` (Task 15, 2026-08-11 audit remediation).

use crate::model::document::{MutateError, OnCollision, Target as MutTarget};
use crate::model::node::Seg;
use crate::model::yaml::project::{entry_key_name, Target, YamlIndex};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};
use super::flow::{delete_flow_member, delete_flow_seq_element, flow_seq_element_node, insert_flow, replace_flow_entry, replace_flow_seq_element};
use super::mutations::comment_block_bounds;
use super::resolve::{reindent, resolve_in};

/// Replace the value at `path` with `fragment`.
///
/// Three cases:
///   (a) Empty path → whole-document replace: reparse fragment as a full YAML
///       doc and splice its ROOT children over the old ROOT children.
///   (b) Path → MapEntry: the fragment may be `key: value` (reuse whole entry)
///       or a bare value (replace just the value child).
///   (c) Path → Element (seq entry): replace the value child of the SEQ_ENTRY.
pub(crate) fn replace(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
    fragment: &str,
) -> Result<(), MutateError> {
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

    match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(entry) => {
            // An entry whose value is an opaque (out-of-subset) construct is
            // read-only — like Delete, reject before touching the tree.
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            // A flow-map member (`{x: 1}`) is an inline `FLOW_ENTRY`; rebuild it
            // inline rather than splicing a multi-line block MAP_ENTRY in its place.
            if entry.kind() == SyntaxKind::FLOW_ENTRY {
                return replace_flow_entry(tree, &entry, fragment);
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
                    // Edge case: re-parse at correct indent failed (shouldn't happen
                    // for simple entries); fall back to a whole-document span splice.
                    splice_node_span(tree, &entry, &new_text)?;
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
            // A flow-seq element shares the whole FLOW_SEQ as its target; the
            // ordinal is the path's trailing index. Rebuild `[…]` inline rather
            // than replacing the whole sequence node.
            if entry.kind() == SyntaxKind::FLOW_SEQ {
                let ord = match path.last() {
                    Some(Seg::Index(i)) => *i,
                    _ => return Err(MutateError::NotFound),
                };
                return replace_flow_seq_element(tree, &entry, ord, fragment);
            }
            // A fragment that is itself a `- …` seq element (what `$EDITOR` shows for
            // a block-map/seq element via `serialize_fragment`) replaces the WHOLE
            // entry: reindent it to the entry's own depth and splice its byte span.
            // Without this, `parse_value_fragment("- name: c")` would nest it as a
            // sub-sequence and double the dash.
            let is_element_fragment = fragment
                .lines()
                .find(|l| !l.trim().is_empty())
                .map(|l| {
                    let t = l.trim_start();
                    t == "-" || t.starts_with("- ")
                })
                .unwrap_or(false);
            if is_element_fragment {
                let entry_indent = fragment_indent(&entry.text().to_string());
                let new_text = reindent(
                    &ensure_newline(fragment),
                    fragment_indent(fragment),
                    entry_indent,
                );
                // The SEQ_ENTRY span includes its own trailing newline, so `new_text`
                // (exactly one trailing `\n` via `ensure_newline`) substitutes 1:1.
                return splice_node_span(tree, &entry, &new_text);
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

/// Replace `node`'s exact byte span in the document with `new_text`, then
/// reparse the whole tree. `new_text` must already carry the span's original
/// trailing-newline state (a MAP_ENTRY/SEQ_ENTRY span includes its newline).
/// Used where an in-place subtree splice can't express a layout change — an
/// entry replacement whose re-indent shifts following lines.
pub(crate) fn splice_node_span(
    tree: &SyntaxNode,
    node: &SyntaxNode,
    new_text: &str,
) -> Result<(), MutateError> {
    let whole = tree.to_string();
    let start: usize = node.text_range().start().into();
    let end: usize = node.text_range().end().into();
    let new_doc = format!("{}{}{}", &whole[..start], new_text, &whole[end..]);
    commit_reparse(tree, &new_doc, MutateError::Fragment)
}

/// Reparse a rebuilt whole-document string and replace `tree`'s children
/// wholesale — the shared tail of every byte-splice mutation below. `on_err`
/// maps a parse failure (an invalid *fragment* rebuild vs. an *illegal* edit).
pub(crate) fn commit_reparse(
    tree: &SyntaxNode,
    new_doc: &str,
    on_err: fn(String) -> MutateError,
) -> Result<(), MutateError> {
    let green = crate::model::yaml::parse::parse(new_doc).map_err(on_err)?;
    let new_root = SyntaxNode::new_root(green).clone_for_update();
    let n = tree.children_with_tokens().count();
    let children: Vec<_> = new_root.children_with_tokens().collect();
    tree.splice_children(0..n, children);
    Ok(())
}

/// Replace a SyntaxNode in-place within its parent.
pub(crate) fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has a parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}

/// Returns the indent depth of a MAP_ENTRY or SEQ_ENTRY node (spaces before content).
pub(crate) fn entry_indent_depth(entry: &SyntaxNode) -> usize {
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
pub(crate) fn ensure_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Extract the key text from a MAP_ENTRY's KEY child.
pub(crate) fn entry_key_text(entry: &SyntaxNode) -> String {
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

/// Find the key/value colon at quote-depth 0 on a single line.
///
/// Returns the byte index of a `:` that is either followed by whitespace
/// (`key: value`) or is the last non-blank char (`flags:` block value) — but
/// only when it falls **outside** a single- or double-quoted span. This is what
/// tells a quoted key holding `: ` (`"a: b": v`) and a bare quoted scalar
/// holding `: ` (`"a: b"`) apart from a real `key: value`, which a plain
/// `contains(": ")` cannot. Double-quote backslash escapes are honored; YAML's
/// single-quote `''` doubling round-trips through the toggle harmlessly.
pub(crate) fn key_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let trimmed_end = line.trim_end().len();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if in_double => escaped = true,
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b':' if !in_single && !in_double => {
                let next = bytes.get(i + 1);
                if matches!(next, Some(b' ') | Some(b'\t')) || i + 1 == trimmed_end {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Does a fragment's first line look like a keyed map entry (`key: value` or a
/// block `key:`)? Depth-aware via [`key_colon`].
pub(crate) fn is_keyed_line(fragment: &str) -> bool {
    let first = fragment.lines().next().unwrap_or("");
    key_colon(first).is_some()
}

/// Parse `fragment` as a `key: value` map entry.
/// Returns `None` if the fragment doesn't contain a COLON at the top level.
pub(crate) fn parse_map_entry_fragment(fragment: &str) -> Option<SyntaxNode> {
    // Must look keyed: the first content line is either `key: value` (inline) or
    // `key:` with the value on following indented lines (a block map/seq value).
    // Checking the first line (not the whole fragment) is what lets a block
    // collection — `flags:\n  - a\n  - b` — be recognized; its first line ends
    // with `:` and contains no `: `, so a whole-fragment check would miss it.
    if !is_keyed_line(fragment) {
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
pub(crate) fn parse_value_fragment(fragment: &str) -> Result<SyntaxNode, MutateError> {
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

/// Delete a map entry, sequence element, or standalone comment block.
/// Each MAP_ENTRY / SEQ_ENTRY node already includes its own NEWLINE token, so
/// removing the node from its parent MAPPING / SEQUENCE is all we need.
/// Comment tokens (COMMENT + NEWLINE) are free children of their container.
pub(crate) fn delete(tree: &SyntaxNode, idx: &YamlIndex, path: &[Seg]) -> Result<(), MutateError> {
    match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(entry) => {
            // If the entry's value is an opaque node, block mutation.
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            // A flow-map member: rebuild the `{…}` without it (a plain node
            // removal would leave a dangling `, ` separator).
            if entry.kind() == SyntaxKind::FLOW_ENTRY {
                return delete_flow_member(tree, &entry);
            }
            delete_node(&entry);
            Ok(())
        }
        Target::Element(entry) => {
            if entry_has_opaque_value(&entry) {
                return Err(MutateError::Unsupported);
            }
            // A flow-seq element's target is the whole FLOW_SEQ; the element
            // ordinal is the path's trailing index. Rebuild `[…]` without it so
            // sibling elements survive (deleting the node would drop the seq).
            if entry.kind() == SyntaxKind::FLOW_SEQ {
                let ord = match path.last() {
                    Some(Seg::Index(i)) => *i,
                    _ => return Err(MutateError::NotFound),
                };
                return delete_flow_seq_element(tree, &entry, ord);
            }
            delete_node(&entry);
            Ok(())
        }
        Target::Comment(tok) => delete_comment_block(tree, &tok),
        Target::Opaque(node) => {
            // Root-level opaque nodes: block.
            let _ = node;
            Err(MutateError::Unsupported)
        }
    }
}

/// Returns true if the entry (MAP_ENTRY or SEQ_ENTRY) contains an OPAQUE value child
/// at any depth, indicating the entry is read-only.
pub(crate) fn entry_has_opaque_value(entry: &SyntaxNode) -> bool {
    entry.descendants().any(|n| n.kind() == SyntaxKind::OPAQUE)
}

/// Remove a MAP_ENTRY or SEQ_ENTRY node from its parent.
/// The node already contains its own trailing NEWLINE, so the splice is clean.
pub(crate) fn delete_node(node: &SyntaxNode) {
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

/// Delete the **whole** standalone comment block beginning at `first` (a merged
/// multi-line `#` block projects as ONE Comment node, so deleting it must remove
/// every consecutive `#` line, not just the first). Uses `comment_block_bounds`
/// (the same span `splice_comment_block` edits) and a whole-document reparse so a
/// ROOT-level comment beside the top MAPPING/SEQUENCE is removed without dropping
/// the body.
pub(crate) fn delete_comment_block(
    tree: &SyntaxNode,
    first: &crate::model::yaml::syntax::SyntaxToken,
) -> Result<(), MutateError> {
    let (start, end, _) = comment_block_bounds(first);
    let full = tree.to_string();
    let new_doc = format!("{}{}", &full[..start], &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Find the MAPPING or SEQUENCE container that is the child collection for `parent_path`.
/// For root level, returns the top-level MAPPING or SEQUENCE child of ROOT.
/// For deeper levels, walks the path to find the innermost container.
pub(crate) fn find_container(tree: &SyntaxNode, parent_path: &[Seg]) -> Result<SyntaxNode, MutateError> {
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
                // A keyed member: a block MAP_ENTRY or a flow FLOW_ENTRY.
                let entry = container
                    .children()
                    .filter(|n| matches!(n.kind(), SyntaxKind::MAP_ENTRY | SyntaxKind::FLOW_ENTRY))
                    .find(|e| entry_key_name(e) == k.as_str())
                    .ok_or(MutateError::NotFound)?;
                child_collection(&entry).ok_or(MutateError::NotFound)?
            }
            Seg::Index(i) => {
                // A flow seq holds its elements directly (no SEQ_ENTRY wrapper): a
                // nested `{…}`/`[…]` element IS the collection to descend into.
                if container.kind() == SyntaxKind::FLOW_SEQ {
                    flow_seq_element_node(&container, *i).ok_or(MutateError::NotFound)?
                } else {
                    // Block seq: a positional SEQ_ENTRY, then its value collection.
                    let entry = container
                        .children()
                        .filter(|n| n.kind() == SyntaxKind::SEQ_ENTRY)
                        .nth(*i)
                        .ok_or(MutateError::NotFound)?;
                    child_collection(&entry).ok_or(MutateError::NotFound)?
                }
            }
        };
    }
    Ok(container)
}

/// The collection node holding an entry's children: a block MAPPING/SEQUENCE, or
/// a flow FLOW_MAP/FLOW_SEQ (possibly VALUE-wrapped).
pub(crate) fn child_collection(entry: &SyntaxNode) -> Option<SyntaxNode> {
    entry.children().find_map(|c| match c.kind() {
        SyntaxKind::MAPPING
        | SyntaxKind::SEQUENCE
        | SyntaxKind::FLOW_MAP
        | SyntaxKind::FLOW_SEQ => Some(c.clone()),
        SyntaxKind::VALUE => c
            .children()
            .find(|v| matches!(v.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ)),
        _ => None,
    })
}

/// Offset between a *projection* slot index for `parent_path` and the edit
/// container's own slot space. For a **top-level** mapping/sequence the projection
/// flattens the container's children up into ROOT *and* lists ROOT-level comment
/// blocks (leading `#` lines, before the container) as root children — but those
/// comments live outside the container (`find_container` returns the inner
/// MAPPING/SEQUENCE), so the container's slot list (`collect_items`) excludes them.
/// The returned count is how many leading ROOT-level comment **blocks** precede the
/// container; a caller subtracts it to turn a projection index into a container
/// index. Always 0 for a non-root container (its projected children are its slots).
pub(crate) fn root_prefix_offset(tree: &SyntaxNode, parent_path: &[Seg]) -> usize {
    if !parent_path.is_empty() {
        return 0;
    }
    // Count merged comment blocks among ROOT's direct children that appear before
    // the first MAPPING/SEQUENCE node (mirrors the projection's `#`-block merging:
    // consecutive `#` lines are one block; a blank line — 2 newlines — splits).
    let mut blocks = 0usize;
    let mut in_block = false;
    let mut newlines = 0u32;
    for child in tree.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n)
                if matches!(n.kind(), SyntaxKind::MAPPING | SyntaxKind::SEQUENCE) =>
            {
                break;
            }
            rowan::NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::COMMENT => {
                    if !in_block {
                        blocks += 1;
                        in_block = true;
                    }
                    newlines = 0;
                }
                SyntaxKind::NEWLINE => {
                    newlines += 1;
                    if newlines >= 2 {
                        in_block = false;
                    }
                }
                SyntaxKind::WHITESPACE | SyntaxKind::INDENT => {}
                _ => in_block = false,
            },
            _ => {}
        }
    }
    blocks
}

/// The ordered "slot elements" of a MAPPING or SEQUENCE: each MAP_ENTRY/SEQ_ENTRY
/// node and each standalone COMMENT token, in document order. Single source of
/// truth for what counts as an item — `collect_items` and the index lookups all
/// build on it so their positions can never drift.
pub(crate) fn slot_elements(
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
pub(crate) fn collect_items(container: &SyntaxNode) -> Vec<String> {
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
pub(crate) fn item_index_of_node(container: &SyntaxNode, node: &SyntaxNode) -> Option<usize> {
    slot_elements(container)
        .iter()
        .position(|el| matches!(el, rowan::NodeOrToken::Node(n) if n == node))
}

/// Detect the indentation depth of a container's entries (number of leading spaces).
/// Returns 0 for root-level containers.
pub(crate) fn container_indent(container: &SyntaxNode) -> usize {
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
pub(crate) fn item_key_name(item: &str) -> Option<String> {
    // Strip leading indent, then find the key/value colon at quote-depth 0 so a
    // quoted key holding `: ` (`"a: b": v`) keys on the whole quoted span.
    let t = item.trim_start();
    let colon = key_colon(t)?;
    Some(t[..colon].trim_matches('\'').trim_matches('"').to_string())
}

/// Collect existing map key names from the container.
pub(crate) fn existing_map_keys(container: &SyntaxNode) -> Vec<String> {
    container
        .children()
        .filter(|n| n.kind() == SyntaxKind::MAP_ENTRY)
        .map(|e| entry_key_name(&e))
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
pub(crate) fn adapt_fragment(
    fragment: &str,
    is_mapping: bool,
    dest_indent: usize,
) -> Result<(String, Option<String>), MutateError> {
    let frag = fragment.trim_end_matches('\n');

    // An already-`- ` sequence-element fragment (e.g. captured from a moved seq
    // element). Into a SEQUENCE it is reindented and passed through as-is; into
    // a MAPPING its `- ` is stripped and the inner value re-adapted.
    if frag.trim_start().starts_with("- ") {
        if is_mapping {
            // Strip exactly one `- ` level (a nested-seq element `- - x` keeps
            // its inner `- x`), then re-adapt as a mapping member.
            let trimmed = frag.trim_start();
            let inner = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            return adapt_fragment(inner, true, dest_indent);
        }
        let reindented = reindent(&format!("{frag}\n"), fragment_indent(frag), dest_indent);
        return Ok((reindented, None));
    }

    // Detect if fragment is keyed by checking for the colon at depth 0 — so a
    // bare quoted scalar holding `: ` (`"a: b"`) is not misread as keyed.
    let trimmed = frag.trim_start();
    let is_keyed = is_keyed_line(trimmed);

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
pub(crate) fn fragment_indent(fragment: &str) -> usize {
    for line in fragment.lines() {
        if !line.trim().is_empty() {
            return line.len() - line.trim_start().len();
        }
    }
    0
}

/// Build the complete document text with the container's content replaced by `new_content`.
/// Uses the container's text_range to do a string-level splice on the full document text.
pub(crate) fn rebuild_and_splice(
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
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Insert a new member/element into the container at `target`.
pub(crate) fn insert(
    tree: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    // Find the container MAPPING or SEQUENCE.
    let container = find_container(tree, &target.parent)?;
    // Inserting into an inline flow collection: rebuild the `{…}`/`[…]` inline.
    if matches!(
        container.kind(),
        SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ
    ) {
        return insert_flow(tree, &container, target, fragment, on_collision);
    }
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
                            let candidate = format!("{key}_{n}");
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

    // Translate the projection index to container-local (drops leading ROOT-level
    // comment blocks the projection lists but the container excludes), then clamp.
    let offset = root_prefix_offset(tree, &target.parent);
    let idx = target.index.saturating_sub(offset).min(items.len());
    items.insert(idx, final_item);

    // Rebuild and splice.
    rebuild_and_splice(tree, &container, &items)
}
