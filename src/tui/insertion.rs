use crate::model::document::Target;
use crate::tui::app::RowSnapshot;

/// Resolve where a newly inserted node should land relative to the cursor (§6.1).
///
/// - Root (empty path), or an expanded Branch → insert as first child
///   (`parent = cursor.path`, `index = 0`).
/// - Anything else (a leaf, or a collapsed branch) → insert as a sibling
///   immediately after the cursor (`parent = cursor.path` minus its last segment,
///   `index = sibling_index + 1`).
pub fn resolve_target(cursor: &RowSnapshot, expanded: bool, sibling_index: usize) -> Target {
    let is_root = cursor.path.is_empty();
    if is_root || (cursor.is_branch && expanded) {
        Target {
            parent: cursor.path.clone(),
            index: 0,
        }
    } else {
        let mut parent = cursor.path.clone();
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

    fn row(key: &str, p: Path, is_branch: bool, depth: usize) -> RowSnapshot {
        RowSnapshot {
            key: key.to_string(),
            path: p,
            depth,
            is_branch,
            value: None,
            scalar_type: None,
            type_label: String::new(),
            format: crate::model::node::Format::Plain,
            trailing_comment: None,
        }
    }

    #[test]
    fn leaf_inserts_after_in_parent() {
        // cursor on server.port (leaf) -> parent=server, index=after port
        let cursor = row("port", path(&["server", "port"]), false, 2);
        let target = resolve_target(&cursor, false, 1);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 2);
    }

    #[test]
    fn expanded_branch_inserts_as_first_child() {
        let cursor = row("server", path(&["server"]), true, 1);
        let target = resolve_target(&cursor, true, 0);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 0);
    }

    #[test]
    fn collapsed_branch_inserts_after_sibling() {
        let cursor = row("server", path(&["server"]), true, 1);
        let target = resolve_target(&cursor, false, 3);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 4);
    }

    #[test]
    fn root_inserts_as_first_top_level() {
        // expanded flag is irrelevant for Root — always its own first child
        let cursor = row("f.toml", path(&[]), true, 0);
        let target = resolve_target(&cursor, true, 0);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 0);
    }
}
