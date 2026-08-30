//! The Action menu (design doc `docs/superpowers/specs/2026-08-30-action-menu-design.md`
//! §2, ADR 0009): one core-owned item list + open/cursor state, replacing the
//! desktop `⋮` popup, the detail panel's action row, and the FAB's add-only
//! decision. Read by every host via `ModeView::ActionMenu`.

use crate::session::i18n::{tr, tr_args};
use crate::session::notice::Notice;
use crate::session::state::Mode;
use crate::session::view::{ActionId, ActionItemView};

use super::session::Session;

impl Session {
    /// Opens the Action menu on the current selection (`m` / Action button /
    /// right-click). Refuses while the clipboard is armed, matching every
    /// other modal-open path (ADR 0005 §5).
    pub fn open_action_menu(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        self.mode = Mode::ActionMenu { cursor: 0 };
    }

    /// Builds the eight-item list against the *current* `selected_paths()` —
    /// called fresh from `mode_view()` every snapshot, so a selection change
    /// while the menu is open never goes stale.
    ///
    /// Membership rule (design doc §2): an operation belongs here when core
    /// can express it as a single intent on the target set, unless the node
    /// already carries a dedicated, always-visible control for it (the kind
    /// badge is one such control, so Kind switch is not listed here).
    pub fn action_menu_items(&self) -> Vec<ActionItemView> {
        let paths = self.selected_paths();
        let single = if paths.len() == 1 {
            self.tree.node_at(&paths[0])
        } else {
            None
        };
        let single_read_only = single.map(|n| n.read_only).unwrap_or(false);
        let single_is_branch = single.map(|n| n.is_branch()).unwrap_or(false);
        let single_has_parent = paths.len() == 1 && !paths[0].is_empty();
        let any_read_only = paths
            .iter()
            .any(|p| self.tree.node_at(p).map(|n| n.read_only).unwrap_or(false));
        let mk = |id: ActionId, key: &str, enabled: bool, separator_before: bool, danger: bool| {
            ActionItemView {
                id,
                label: tr(self.lang, key).to_string(),
                enabled,
                separator_before,
                danger,
            }
        };
        vec![
            mk(
                ActionId::Edit,
                "core.action.edit",
                paths.len() == 1 && !single_read_only,
                false,
                false,
            ),
            mk(
                ActionId::AddChild,
                "core.action.add-child",
                paths.len() == 1 && single_is_branch,
                false,
                false,
            ),
            mk(
                ActionId::AddSibling,
                "core.action.add-sibling",
                single_has_parent,
                false,
                false,
            ),
            mk(ActionId::Copy, "core.action.copy", true, false, false),
            mk(ActionId::Cut, "core.action.cut", !any_read_only, false, false),
            mk(
                ActionId::Remark,
                "core.action.remark",
                !any_read_only,
                false,
                false,
            ),
            mk(
                ActionId::Detail,
                "core.action.detail",
                paths.len() == 1,
                false,
                false,
            ),
            mk(
                ActionId::Delete,
                "core.action.delete",
                !any_read_only,
                true,
                true,
            ),
        ]
    }

    /// `target_count` + `target_label` for the menu header: a single target
    /// names the node; multiple show the localized "N nodes".
    pub fn action_menu_targets(&self) -> (usize, String) {
        let paths = self.selected_paths();
        if paths.len() == 1 {
            let label = self
                .tree
                .node_at(&paths[0])
                .map(|n| n.key.clone())
                .unwrap_or_default();
            (1, label)
        } else {
            let n = paths.len();
            let n_str = n.to_string();
            (
                n,
                tr_args(self.lang, "core.action.targets", &[n_str.as_str()]),
            )
        }
    }

    /// Moves the Action menu cursor by `delta`, wrapping and skipping
    /// disabled items (never landing the cursor on one).
    pub fn action_menu_move(&mut self, delta: i32) {
        let Mode::ActionMenu { cursor } = &self.mode else {
            return;
        };
        let cursor = *cursor;
        let items = self.action_menu_items();
        let n = items.len() as i32;
        if n == 0 {
            return;
        }
        let mut c = cursor as i32;
        for _ in 0..n {
            c = (c + delta).rem_euclid(n);
            if items[c as usize].enabled {
                break;
            }
        }
        if let Mode::ActionMenu { cursor } = &mut self.mode {
            *cursor = c as usize;
        }
    }

    /// Commits the item under the cursor (keyboard `Enter`).
    pub fn action_menu_commit(&mut self) {
        let Mode::ActionMenu { cursor } = self.mode else {
            return;
        };
        let items = self.action_menu_items();
        let Some(item) = items.get(cursor) else {
            return;
        };
        self.action_menu_apply(item.id, item.enabled);
    }

    /// Web pointer analogue of `action_menu_commit`: apply a directly-picked
    /// id without moving the cursor first.
    pub fn action_menu_pick(&mut self, id: ActionId) {
        let items = self.action_menu_items();
        let enabled = items
            .iter()
            .find(|it| it.id == id)
            .map(|it| it.enabled)
            .unwrap_or(false);
        self.action_menu_apply(id, enabled);
    }

    /// Exits the menu to `resting_mode()`, then dispatches the mapped intent
    /// if the item was enabled — a disabled pick still closes the menu and
    /// surfaces `core.action.unavailable`, rather than doing nothing.
    fn action_menu_apply(&mut self, id: ActionId, enabled: bool) {
        self.mode = self.resting_mode();
        if !enabled {
            self.set_notice(Notice::core(self.lang, "core.action.unavailable", &[]));
            return;
        }
        match id {
            ActionId::Edit => self.begin_external_edit(),
            ActionId::AddChild => self.add_child(),
            ActionId::AddSibling => self.add_sibling(),
            ActionId::Copy => self.copy_selected(),
            ActionId::Cut => self.cut_selected(),
            ActionId::Remark => self.remark(),
            ActionId::Detail => self.toggle_detail(),
            ActionId::Delete => self.delete_selected(),
        }
    }

    /// Closes the Action menu without applying anything (`Esc`).
    pub fn exit_action_menu(&mut self) {
        self.mode = self.resting_mode();
        self.notice = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{Node, NodeKind, NodeTree, ScalarType, Seg};
    use crate::session::state::Clipboard;

    fn tree_with_two_scalars_and_a_branch() -> NodeTree {
        let mut a = Node::leaf("a", NodeKind::Scalar(ScalarType::Integer));
        a.path = vec![Seg::Key("a".into())];
        let mut b = Node::leaf("b", NodeKind::Scalar(ScalarType::Integer));
        b.path = vec![Seg::Key("b".into())];
        let mut c = Node::branch("c", NodeKind::Table);
        c.path = vec![Seg::Key("c".into())];
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![a, b, c];
        NodeTree { root }
    }

    fn session_with_two_scalars() -> Session {
        let tree = tree_with_two_scalars_and_a_branch();
        let mut s = Session::from_tree(tree);
        s.cursor = vec![Seg::Key("a".into())];
        s
    }

    #[test]
    fn single_branch_selected_enables_all_eight() {
        let mut s = session_with_two_scalars();
        s.cursor = vec![Seg::Key("c".into())];
        let items = s.action_menu_items();
        assert_eq!(items.len(), 8);
        assert!(items.iter().all(|it| it.enabled), "{items:?}");
    }

    #[test]
    fn two_node_selection_dims_single_path_only_items() {
        let mut s = session_with_two_scalars();
        s.selection.set_all([
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        ]);
        let (count, _label) = s.action_menu_targets();
        assert_eq!(count, 2);
        let items = s.action_menu_items();
        let enabled: Vec<ActionId> = items
            .iter()
            .filter(|it| it.enabled)
            .map(|it| it.id)
            .collect();
        assert_eq!(
            enabled,
            vec![
                ActionId::Copy,
                ActionId::Cut,
                ActionId::Remark,
                ActionId::Delete
            ]
        );
    }

    #[test]
    fn open_action_menu_refuses_while_clipboard_armed() {
        let mut s = session_with_two_scalars();
        // Bypass `copy_selected` (needs a real `doc`, which this headless
        // fixture doesn't have) — arm the clipboard directly, matching the
        // shape `capture_selected` would have produced.
        s.clipboard = Some(Clipboard {
            fragments: vec!["1".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        s.open_action_menu();
        assert!(!matches!(s.mode, Mode::ActionMenu { .. }));
        assert!(s.notice.is_some());
    }

    #[test]
    fn commit_exits_menu_before_dispatch() {
        let mut s = session_with_two_scalars();
        s.open_action_menu();
        assert!(matches!(s.mode, Mode::ActionMenu { .. }));
        // Cursor starts on item 0 (Edit) which is enabled for a single scalar.
        s.action_menu_commit();
        assert!(!matches!(s.mode, Mode::ActionMenu { .. }));
    }

    #[test]
    fn pick_disabled_item_closes_menu_and_sets_unavailable_notice() {
        let mut s = session_with_two_scalars();
        s.open_action_menu();
        // AddChild is disabled on a scalar leaf.
        s.action_menu_pick(ActionId::AddChild);
        assert!(!matches!(s.mode, Mode::ActionMenu { .. }));
        assert!(s.notice.is_some());
    }

    #[test]
    fn every_action_label_resolves_in_both_langs() {
        use crate::session::i18n::Lang;
        let mut s = session_with_two_scalars();
        for lang in [Lang::En, Lang::ZhTw] {
            s.lang = lang;
            for it in s.action_menu_items() {
                assert!(!it.label.is_empty(), "{:?} {:?}", lang, it.id);
            }
        }
    }
}
