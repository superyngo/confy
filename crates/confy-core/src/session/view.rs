use crate::model::node::{Format, Path, ScalarType};
use serde::{Deserialize, Serialize};

/// One visible row in the tree — the view model both the TUI and Web UI render.
/// The host adds presentation-only fields (type_tag fixed-pitch label, column padding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewRow {
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
    pub key: String,
    /// Rendered scalar value string; `None` for branches and comments.
    pub value: Option<String>,
    pub scalar_type: Option<ScalarType>,
    pub format: Format,
    pub trailing_comment: Option<String>,
    /// True for YAML opaque nodes and JSON block comments (read-only in the UI).
    pub read_only: bool,
    /// True when this row's path is in the session's live selection.
    pub selected: bool,
    /// True when this row's path matches `session.cursor`.
    pub is_cursor: bool,
}

/// What the session changed after a [`super::intent::Intent`] was dispatched.
/// The UI uses this to decide what to re-render.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Update {
    /// `visible_rows()` changed — re-pull and redraw the tree.
    pub rows_dirty: bool,
    /// A status message to show in the footer (replaces any previous).
    pub status: Option<String>,
    /// An error message to show in the footer.
    pub error: Option<String>,
    /// The user confirmed quit — the host event loop should exit.
    pub quit: bool,
    /// The core needs an external edit: the host should call `Host::edit_text`
    /// with this initial text, then re-dispatch `Intent::ExternalEditDone(text)`
    /// (or nothing on cancellation).
    pub external_edit: Option<String>,
    /// The core needs the host to write a converted file (fs-free: host does the I/O).
    /// Contains `(output_path, text)`.
    pub convert_write: Option<(String, String)>,
}

impl Update {
    pub fn dirty() -> Self {
        Update {
            rows_dirty: true,
            ..Default::default()
        }
    }
    pub fn with_status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn with_error(mut self, e: impl Into<String>) -> Self {
        self.error = Some(e.into());
        self
    }
}
