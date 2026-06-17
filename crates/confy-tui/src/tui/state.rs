use crate::model::node::Path;

pub enum Mode {
    Normal,
    Prompt(PromptKind),
    /// Typing a filter query (the inline `/` input field is shown).
    Filter,
    /// Browsing/selecting within the locked-in filtered result list. Behaves like
    /// `Normal` for navigation and edits, but the filter stays applied; `/` reopens
    /// the input (prefilled) to refine, and Esc clears the filter.
    FilterResults,
    /// The `f` type-filter checkbox popup is open. Arrows move the cursor, Space
    /// toggles the focused cell, Enter applies (locks into `FilterResults`/`Normal`),
    /// Esc peels the type filter off. The tree filters live in the background.
    TypeFilter,
    /// The `K` kind-switch popup is open: a single-select list of the kinds the
    /// cursor node can convert to. Up/Down (or j/k) move, Enter applies
    /// (`Mutation::ConvertKind`), Esc cancels.
    KindSwitch(KindSwitchState),
    /// The `C` document-conversion flow is open (Root node only): pick a target
    /// format, type an output path, then confirm past the lossy-warning list.
    /// The open document is never modified — a successful conversion writes a
    /// brand-new file.
    Convert(ConvertState),
    Detail,
    Help,
    Edit(EditState),
}

/// In-flight `C` conversion flow state.
pub struct ConvertState {
    pub step: ConvertStep,
    /// Selectable target formats (the current format excluded).
    pub options: Vec<crate::model::document::DocFormat>,
    /// Cursor into `options` during [`ConvertStep::Format`].
    pub cursor: usize,
    /// Chosen target (valid once past [`ConvertStep::Format`]).
    pub target: crate::model::document::DocFormat,
    /// Output path being typed (caret field) in [`ConvertStep::Path`].
    pub path: String,
    pub path_cursor: usize,
    /// Lossy-normalization warnings shown in [`ConvertStep::Confirm`].
    pub warnings: Vec<String>,
    /// Rendered output text, held between the confirm prompt and the write.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertStep {
    /// Choosing the target format (single-select list).
    Format,
    /// Typing the output file path.
    Path,
    /// Confirming a lossy conversion (warning list + y/n).
    Confirm,
}

/// In-flight `K` kind-switch popup state.
pub struct KindSwitchState {
    pub path: Path,
    /// `(label, target)` choices for the node — the current kind is excluded.
    pub options: Vec<(String, crate::model::document::KindTarget)>,
    pub cursor: usize,
}

pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    /// Inline-edit commit changed the scalar's type; confirm before writing.
    TypeChange {
        from: String,
        to: String,
    },
    /// Pasting a comment into a single-line array: confirm the reformat to
    /// multiline before re-issuing the paste with the upgrade allowed.
    ArrayUpgrade {
        target: crate::model::document::Target,
        on_collision: crate::model::document::OnCollision,
    },
    /// A comment-introducing op on a pure `.json` file: confirm the JSONC upgrade
    /// before applying the deferred operation.
    JsoncUpgrade {
        pending: PendingComment,
    },
}

/// A comment-introducing operation deferred behind the JSONC-upgrade confirmation.
pub enum PendingComment {
    Remark { path: Path },
}

/// Which column the inline editor is currently editing. `Tab` toggles between
/// them (disabled for array elements, which have no name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditField {
    Value,
    Name,
}

/// In-flight inline editor state (§inline edit). The editor edits one field at a
/// time: `buffer`/`cursor`/`scroll` are the *active* field's working set, while
/// `other_*` hold the inactive field saved across a `Tab` swap. `key` is the
/// node's original key (for rename detection). `scroll` is the horizontal viewport
/// offset (first visible char), persistent so moving left after the right edge
/// walks the cursor back through the window before the text scrolls.
pub struct EditState {
    pub path: Path,
    pub key: String,
    pub field: EditField,
    /// Array element: no name field, so `Tab` is a no-op.
    pub is_element: bool,
    /// Comment node: the buffer is the raw `#`-prefixed text, committed via
    /// `EditComment` (no name field, no type check, so `Tab` is a no-op).
    pub is_comment: bool,
    /// F2 rename-only mode: only the Name field is active; Tab and value Replace
    /// are skipped. Supports all node types including [T/D] synthetic tables,
    /// [T/S] scope tables, and [A/T] groups.
    pub rename_only: bool,
    pub buffer: String,
    pub cursor: usize,
    pub scroll: usize,
    pub other_buffer: String,
    pub other_cursor: usize,
    pub other_scroll: usize,
    /// The node's trailing inline comment at edit start (`# bind` / `// bind`),
    /// seeded into the Value buffer after the value. On commit the buffer is split
    /// back into value + comment; a change from this baseline drives
    /// `Mutation::SetTrailingComment`. `None` when the node had no trailing comment.
    pub orig_trailing: Option<String>,
    /// Set when this edit session was opened by `a` (add) on a freshly inserted
    /// seed node. Esc (`edit_cancel`) then rolls the insert back so the add
    /// leaves no trace; a normal edit of an existing node leaves this `false`.
    pub created_on_add: bool,
}

/// Where a paste lands in the tree, addressed by the *node path* of a visible
/// row (§3: was a visible-row `usize`). In paste mode `↑/↓` step through a merged
/// sequence of these slots: an `After(path)` reads as a green line *below* that
/// row (insert as a sibling after it, into that row's container); an `Into(path)`
/// reads as the whole branch row turning green (append as its **last** child,
/// regardless of open/closed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteSlot {
    /// Append as the last child of the branch at this path.
    Into(Path),
    /// Insert as a sibling immediately after the node at this path.
    After(Path),
}

/// Clipboard holding serialized TOML fragments for copy/cut/paste (§6 x/c/v).
/// Cut defers deletion until paste succeeds (wenv-style), so the document is
/// only mutated on `v`, not on `x`.
pub struct Clipboard {
    pub fragments: Vec<String>,
    pub cut: bool,
    /// Source paths for cut — deleted after successful paste.
    pub sources: Vec<Path>,
}

/// Multi-step undo/redo over full serialized-document snapshots.
/// One snapshot per user action (§6 z/y). UI-state changes never push.
pub struct History {
    past: Vec<String>,
    current: String,
    future: Vec<String>,
}

impl History {
    pub fn new(initial: String) -> Self {
        History {
            past: Vec::new(),
            current: initial,
            future: Vec::new(),
        }
    }
    pub fn push(&mut self, snapshot: String) {
        self.past
            .push(std::mem::replace(&mut self.current, snapshot));
        self.future.clear();
    }
    pub fn undo(&mut self) -> Option<String> {
        let prev = self.past.pop()?;
        self.future
            .push(std::mem::replace(&mut self.current, prev.clone()));
        Some(prev)
    }
    pub fn redo(&mut self) -> Option<String> {
        let next = self.future.pop()?;
        self.past
            .push(std::mem::replace(&mut self.current, next.clone()));
        Some(next)
    }
    /// Discard the most recent `push` as if it never happened: revert `current`
    /// to the prior snapshot and drop it from `past`, leaving the redo `future`
    /// untouched (unlike `undo`, which records a redo step). Returns the restored
    /// snapshot, or `None` when there is nothing to roll back. Used to cancel a
    /// freshly-added node via Esc so the add leaves no undo/redo trace.
    pub fn cancel_last(&mut self) -> Option<String> {
        let prev = self.past.pop()?;
        self.current = prev.clone();
        Some(prev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_restores_snapshots() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string()); // snapshot AFTER an action
        h.push("v2".to_string());
        assert_eq!(h.undo(), Some("v1".to_string()));
        assert_eq!(h.undo(), Some("v0".to_string()));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some("v1".to_string()));
    }

    #[test]
    fn push_clears_redo_future() {
        // After undoing, a new action (push) must discard the redo stack:
        // you cannot redo into a branch that no longer exists.
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string());
        assert_eq!(h.undo(), Some("v0".to_string())); // future now holds v1
        h.push("v2".to_string()); // new action from v0 -> v2; v1 future discarded
        assert_eq!(h.redo(), None, "redo stack must be cleared by push");
        assert_eq!(h.undo(), Some("v0".to_string())); // v2 -> v0
    }
}
