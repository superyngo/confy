use crate::model::document::Target;
use crate::model::node::Path;

/// Resolve where a newly inserted node should land relative to the cursor (§6.1).
///
/// - Root (empty path), or an expanded Branch → insert as first child
///   (`parent = path`, `index = 0`).
/// - Anything else (a leaf, or a collapsed branch) → insert as a sibling
///   immediately after the cursor (`parent = path` minus its last segment,
///   `index = sibling_index + 1`).
///
/// §5: CORE — pure §6.1 logic. Takes the cursor's `(path, is_branch)` plus its
/// expand/sibling context (was a `&RowSnapshot`, a host type), so the resolver no
/// longer depends on the TUI render row and can lift into `confy-core` verbatim.
pub fn resolve_target(
    path: &Path,
    is_branch: bool,
    expanded: bool,
    sibling_index: usize,
) -> Target {
    let is_root = path.is_empty();
    if is_root || (is_branch && expanded) {
        Target {
            parent: path.clone(),
            index: 0,
        }
    } else {
        let mut parent = path.clone();
        parent.pop();
        Target {
            parent,
            index: sibling_index + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{Path, Seg};

    fn path(keys: &[&str]) -> Path {
        keys.iter().map(|k| Seg::Key(k.to_string())).collect()
    }

    #[test]
    fn leaf_inserts_after_in_parent() {
        // cursor on server.port (leaf) -> parent=server, index=after port
        let target = resolve_target(&path(&["server", "port"]), false, false, 1);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 2);
    }

    #[test]
    fn expanded_branch_inserts_as_first_child() {
        let target = resolve_target(&path(&["server"]), true, true, 0);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 0);
    }

    #[test]
    fn collapsed_branch_inserts_after_sibling() {
        let target = resolve_target(&path(&["server"]), true, false, 3);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 4);
    }

    #[test]
    fn root_inserts_as_first_top_level() {
        // expanded flag is irrelevant for Root — always its own first child
        let target = resolve_target(&path(&[]), true, true, 0);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 0);
    }
}
