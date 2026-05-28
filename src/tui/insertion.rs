use crate::model::document::Target;
use crate::model::node::{NodeKind, Path};

/// Minimal snapshot of the cursor row needed to resolve an insertion target.
#[derive(Clone, Debug)]
pub struct RowSnapshot {
    pub path: Path,
    pub kind: NodeKind,
}

/// Resolve where a newly inserted node should land relative to the cursor.
///
/// Rules (§6.1):
/// - Root node, or an expanded Branch → insert as first child
///   (`parent = cursor.path`, `index = 0`).
/// - Anything else (leaf, or a collapsed branch) → insert as sibling
///   immediately after the cursor (`parent = cursor.path minus last seg`,
///   `index = sibling_index + 1`).
pub fn resolve_target(cursor: &RowSnapshot, expanded: bool, sibling_index: usize) -> Target {
    let is_root = matches!(cursor.kind, NodeKind::Root);
    let is_branch = matches!(
        cursor.kind,
        NodeKind::Root | NodeKind::Table | NodeKind::ArrayOfTables
            | NodeKind::Array | NodeKind::InlineTable
    );

    if is_root || (is_branch && expanded) {
        Target { parent: cursor.path.clone(), index: 0 }
    } else {
        let mut parent = cursor.path.clone();
        parent.pop();
        Target { parent, index: sibling_index + 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{ScalarType, Seg};

    fn path(keys: &[&str]) -> Path {
        keys.iter().map(|k| Seg::Key(k.to_string())).collect()
    }

    #[test]
    fn leaf_inserts_after_in_parent() {
        let cursor = RowSnapshot {
            path: path(&["server", "port"]),
            kind: NodeKind::Scalar(ScalarType::Integer),
        };
        let target = resolve_target(&cursor, false, 2);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 3);
    }

    #[test]
    fn expanded_branch_inserts_as_first_child() {
        let cursor = RowSnapshot {
            path: path(&["server"]),
            kind: NodeKind::Table,
        };
        let target = resolve_target(&cursor, true, 1);
        assert_eq!(target.parent, path(&["server"]));
        assert_eq!(target.index, 0);
    }

    #[test]
    fn collapsed_branch_inserts_after_sibling() {
        let cursor = RowSnapshot {
            path: path(&["server"]),
            kind: NodeKind::Table,
        };
        let target = resolve_target(&cursor, false, 0);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 1);
    }

    #[test]
    fn root_inserts_as_first_top_level() {
        let cursor = RowSnapshot {
            path: path(&[]),
            kind: NodeKind::Root,
        };
        // expanded=false doesn't matter for Root — always first child
        let target = resolve_target(&cursor, false, 0);
        assert_eq!(target.parent, path(&[]));
        assert_eq!(target.index, 0);
    }
}
