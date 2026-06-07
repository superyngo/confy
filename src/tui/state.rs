use crate::model::node::Path;

pub enum Mode {
    Normal,
    MovePending { sources: Vec<Path> },
    Prompt(PromptKind),
    Filter,
    Detail,
    Help,
    Edit(EditState),
}

pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    MoveCollision {
        key: String,
    },
    /// Inline-edit commit changed the scalar's type; confirm before writing.
    TypeChange {
        from: String,
        to: String,
    },
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
    pub buffer: String,
    pub cursor: usize,
    pub scroll: usize,
    pub other_buffer: String,
    pub other_cursor: usize,
    pub other_scroll: usize,
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
