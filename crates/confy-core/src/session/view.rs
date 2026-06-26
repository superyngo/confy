use crate::model::document::{DocFormat, KindTarget};
use crate::model::node::{Format, Path, ScalarType};
use crate::session::state::{ConvertStep, EditField};
use crate::session::type_filter::CheckState;
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
    /// Node-kind label (`table`/`array`/`inline`/`array-of-tables`/`string`/…)
    /// so the Web UI can render the per-row kind badge without re-deriving the
    /// container kind (which `is_branch` alone can't distinguish).
    pub type_label: String,
    /// Immediate child count — drives the branch row's "N" item-count badge
    /// (meaningful for branches; 0 for scalars/comments).
    pub child_count: usize,
    pub trailing_comment: Option<String>,
    /// Key-sign label (`bare`/`quoted`/`dotted`/`none`) so a structured panel can
    /// show "Sign" without re-deriving it from the flat detail text.
    pub key_sign: String,
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

// ---- Stage-2 full-state transport (WASM / Web UI) ----
//
// `SessionSnapshot` is the full renderable state the Web UI re-renders from after
// each `dispatch`. It is the G1 full-state transport (PORTING §8.3): the entire
// visible tree + modal surfaces + signals. No structured row diff yet.

/// One convertible kind in the `K` popup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindOptionView {
    pub label: String,
    pub target: KindTarget,
}

/// The serializable projection of `Mode` + the modal edit surfaces the UI renders.
/// Heavy internals (`History`, `Clipboard`) never cross the boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModeView {
    Normal,
    Prompt {
        kind: PromptView,
    },
    /// Typing a `/` filter query.
    Filter {
        text: String,
        cursor: usize,
    },
    /// Browsing the locked-in filtered result list.
    FilterResults,
    /// The `f` type-filter popup is open. Carries the full facet grid (headers +
    /// cells with tri-state checks + the cursor cell) so the host renders the
    /// popup without duplicating `type_filter::layout` (PORTING §5 type_filter SPLIT).
    TypeFilter(TypeFilterView),
    /// The `K` kind-switch popup is open.
    KindSwitch {
        cursor: usize,
        options: Vec<KindOptionView>,
    },
    /// The `C` document-conversion flow is open.
    Convert(ConvertView),
    /// The `i` detail popup is open.
    Detail,
    /// The `?` help overlay is open.
    Help,
    /// The inline editor is active on one row.
    Edit(EditView),
}

/// Which yes/no prompt is open (the prompt's text lives in `snapshot.status`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PromptView {
    ConfirmQuit,
    Collision,
    TypeChange,
    ArrayUpgrade,
    JsoncUpgrade,
}

/// The inline-edit surface projected for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditView {
    pub field: EditField,
    pub buffer: String,
    pub cursor: usize,
    pub key: String,
    pub is_element: bool,
    pub is_comment: bool,
    pub rename_only: bool,
}

/// The `C` convert-wizard surface projected for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertView {
    pub step: ConvertStep,
    pub cursor: usize,
    pub options: Vec<DocFormat>,
    pub target: DocFormat,
    pub path: String,
    pub path_cursor: usize,
    pub warnings: Vec<String>,
}

/// One row of the `f` type-filter facet grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeFilterRow {
    Header(String),
    Cells(Vec<TypeFilterCellView>),
}

/// One facet cell: label + tri-state + whether the cursor is on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeFilterCellView {
    pub label: String,
    pub state: CheckState,
    pub is_cursor: bool,
}

/// The `f` type-filter popup surface: the per-format facet grid plus the cursor
/// cell and whether any facet is currently active (non-empty filter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeFilterView {
    pub rows: Vec<TypeFilterRow>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub active: bool,
}

/// Which kind of external edit the host's async modal should perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExternalEditKind {
    /// Replace a value fragment at `path`.
    Value { path: Path },
    /// Replace a standalone comment's text at `path`.
    Comment { path: Path },
}

/// A request for the host to open its async multi-line editor (PORTING §8.2).
/// The host returns the edited text via a follow-up `Intent::ApplyReplace` /
/// `Intent::ApplyEditComment`; on cancel it dispatches `Escape`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalEdit {
    pub initial: String,
    pub kind: ExternalEditKind,
}

/// The full renderable state. The Web UI re-renders wholesale from this each
/// `dispatch` (full-state transport, no diff — PORTING §8.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub doc_format: DocFormat,
    pub is_dirty: bool,
    pub mode: ModeView,
    pub rows: Vec<ViewRow>,
    pub cursor: Path,
    pub status: Option<String>,
    pub error: Option<String>,
    pub detail_text: Option<String>,
    /// Set when the core needs the host's async editor (§8.2).
    pub external_edit: Option<ExternalEdit>,
    /// Set when the core needs the host to write a converted file (fs-free).
    pub convert_write: Option<(String, String)>,
    /// Number of captured fragments in the live clipboard (`None` = empty).
    /// Surfaces real application state the UI shows as a "clipboard: N" hint.
    pub clipboard_count: Option<usize>,
    /// True when the clipboard holds a *cut* (move) rather than a *copy*, so the
    /// UI can style cut source rows distinctly from copied ones.
    pub clipboard_cut: bool,
    /// The source node paths captured in the clipboard, so the UI can mark those
    /// rows (distinct from the selection box).
    pub clipboard_paths: Vec<Path>,
    /// The user confirmed quit — the host should exit.
    pub quit: bool,
}
