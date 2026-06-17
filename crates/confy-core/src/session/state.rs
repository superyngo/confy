use crate::model::document::{DocFormat, KindTarget, Target};
use crate::model::node::Path;
use serde::{Deserialize, Serialize};

/// The action a TypeChange confirmation (`y`) applies.
pub enum PendingCommit {
    /// Replace the node's value with this `key = value` fragment.
    Replace(String),
    /// Rename the key to `new_name` (may introduce dots), then set the value.
    Rename { new_name: String, value: String },
}

/// How `e` should edit the cursor node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditKind {
    Inline,
    External,
}

/// Which filter layer was most recently (re)applied. Esc in FilterResults peels
/// this layer first so two active filters come off one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterLayer {
    Text,
    Type,
}

/// In-flight async external edit (PORTING §8.2). Set when `dispatch` routes an
/// edit to the external path; consumed by the follow-up `ApplyReplace` /
/// `ApplyEditComment` intent. The host only ever sees the `initial` text and
/// returns edited text — this struct remembers the resolution the core needs.
#[derive(Clone, Debug)]
pub struct PendingExternalEdit {
    pub path: Path,
    /// True when the edited text is a bare value that must be re-wrapped via
    /// `scalar_fragment(None, …)` (the array-element form). Mirrors App::edit_node.
    pub wrap_element: bool,
    /// True when this is a standalone-comment edit (`apply_edit_comment`), not a
    /// value replace.
    pub is_comment: bool,
}

/// The editing mode the session is in.
pub enum Mode {
    Normal,
    Prompt(PromptKind),
    /// Typing a filter query (the inline `/` input field is shown).
    Filter,
    /// Browsing/selecting within the locked-in filtered result list.
    FilterResults,
    /// The `f` type-filter checkbox popup is open.
    TypeFilter,
    /// The `K` kind-switch popup is open.
    KindSwitch(KindSwitchState),
    /// The `C` document-conversion flow is open.
    Convert(ConvertState),
    Detail,
    Help,
    Edit(EditState),
}

/// In-flight `C` conversion flow state.
pub struct ConvertState {
    pub step: ConvertStep,
    pub options: Vec<DocFormat>,
    pub cursor: usize,
    pub target: DocFormat,
    pub path: String,
    pub path_cursor: usize,
    pub warnings: Vec<String>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConvertStep {
    Format,
    Path,
    Confirm,
}

/// In-flight `K` kind-switch popup state.
pub struct KindSwitchState {
    pub path: Path,
    pub options: Vec<(String, KindTarget)>,
    pub cursor: usize,
}

pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    TypeChange {
        from: String,
        to: String,
    },
    ArrayUpgrade {
        target: Target,
        on_collision: crate::model::document::OnCollision,
    },
    JsoncUpgrade {
        pending: PendingComment,
    },
}

pub enum PendingComment {
    Remark { path: Path },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditField {
    Value,
    Name,
}

pub struct EditState {
    pub path: Path,
    pub key: String,
    pub field: EditField,
    pub is_element: bool,
    pub is_comment: bool,
    pub rename_only: bool,
    pub buffer: String,
    pub cursor: usize,
    pub scroll: usize,
    pub other_buffer: String,
    pub other_cursor: usize,
    pub other_scroll: usize,
    pub orig_trailing: Option<String>,
    pub created_on_add: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteSlot {
    Into(Path),
    After(Path),
}

pub struct Clipboard {
    pub fragments: Vec<String>,
    pub cut: bool,
    pub sources: Vec<Path>,
}

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
    pub fn cancel_last(&mut self) -> Option<String> {
        let prev = self.past.pop()?;
        self.current = prev.clone();
        Some(prev)
    }
    pub fn current(&self) -> &str {
        &self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_restores_snapshots() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string());
        h.push("v2".to_string());
        assert_eq!(h.undo(), Some("v1".to_string()));
        assert_eq!(h.undo(), Some("v0".to_string()));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some("v1".to_string()));
    }

    #[test]
    fn push_clears_redo_future() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string());
        assert_eq!(h.undo(), Some("v0".to_string()));
        h.push("v2".to_string());
        assert_eq!(h.redo(), None, "redo stack must be cleared by push");
        assert_eq!(h.undo(), Some("v0".to_string()));
    }
}
