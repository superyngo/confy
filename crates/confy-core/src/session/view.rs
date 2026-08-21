use crate::model::document::{DocFormat, KindTarget};
use crate::model::node::{Format, Path, ScalarType};
use crate::session::notice::{Notice, Severity};
use crate::session::state::{ConvertStep, EditField, HelpTab, PasteSlot};
use crate::session::type_filter::CheckState;
use serde::{Deserialize, Serialize};

/// One immediate child of a node — the Web UI breadcrumb mini-tree row
/// (returned by `Session::children_of`, exposed as ffi `children(path)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildView {
    pub key: String,
    pub path: Path,
    /// Core type label ("table"/"array"/"string"/"comment"/…), same vocabulary
    /// as `ViewRow::type_label`.
    pub type_label: String,
    pub is_branch: bool,
}

/// Read-only outline transport — deliberately separate from the internal
/// `Node`/`NodeKind` wire shape, matching the existing `ChildView`/
/// `KindOptionView` convention of small dedicated FFI-boundary types.
/// Consumed by editor Outline/breadcrumb integrations (VS Code
/// `DocumentSymbolProvider`, spec `docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineNode {
    pub key: String,
    pub path: Path,
    /// Same vocabulary as `ViewRow::type_label`/`ChildView::type_label`.
    pub type_label: String,
    /// Scalar leaves only — carried through for the editor's `detail` field.
    pub value: Option<String>,
    pub text_range: (u32, u32),
    pub key_text_range: Option<(u32, u32)>,
    pub children: Vec<OutlineNode>,
}

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
    /// Soft-constraint violation messages whose Path == this row's Path;
    /// `None` = clean. Never blocks anything (`CONTEXT.md` § Schema
    /// "Soft constraint").
    pub violations: Option<Vec<String>>,
    /// `true` when this row is a branch and some node in its subtree (at any
    /// depth) currently has a schema violation — independent of this row's
    /// own expand state; the renderer decides whether to draw a marker based
    /// on whether the row is *currently* collapsed.
    pub has_descendant_violation: bool,
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
        /// The localized question text, rendered core-side from `PromptKind`
        /// + `Session::lang` per snapshot (`core.prompt.*` keys) so every
        /// host renders identical prose.
        question: String,
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
    /// The schema-enum picker popup is open (spec §3). `options` are the
    /// display labels; the chosen value is committed core-side by
    /// `Session::schema_enum_commit`.
    SchemaEnum {
        cursor: usize,
        options: Vec<String>,
    },
    /// The `C` document-conversion flow is open.
    Convert(ConvertView),
    /// The `i` detail popup is open.
    Detail,
    /// The `?` help overlay is open, on tab `tab`.
    Help {
        tab: HelpTab,
    },
    /// The inline editor is active on one row.
    Edit(EditView),
}

/// Which yes/no prompt is open (the question text lives in
/// `ModeView::Prompt.question`).
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
    /// The single user-facing transient message slot (design spec §2/§10).
    /// `status`/`error` are the legacy dual-write projection of this field.
    pub notice: Option<Notice>,
    /// Legacy dual-write projection of `notice` (spec §10/§11 Q6): Error →
    /// `error`, Info/Success/Warn → `status`, no notice → both `None`.
    /// Computed at snapshot-build time in `Session::snapshot`.
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
    /// The armed clipboard's target — `effective_paste_slot()`, surfaced only
    /// while a clipboard is armed (mirrors `clipboard_count`'s convention).
    /// Every pointer host renders this instead of re-deriving it (ADR 0004 §1).
    pub paste_slot: Option<PasteSlot>,
    /// True while a committed type filter is narrowing the rows, whatever the
    /// current mode — lets the UI keep its filter-button state accurate after
    /// the popup closes (the `TypeFilterView.active` flag only exists while
    /// `Mode::TypeFilter` is open).
    pub type_filter_active: bool,
    /// The user confirmed quit — the host should exit.
    pub quit: bool,
    /// Active UI language code (`"en"` / `"zh-TW"`), so hosts stay in sync.
    pub lang: String,
    /// Undo-history depth (`History::depth()`, 0 before the first edit or
    /// when no document is loaded).
    pub history_len: usize,
    pub schema_status: Option<crate::schema::SchemaStatus>,
    /// Set when a detected/explicit schema source needs the host to resolve
    /// its text (local read or URL fetch) and dispatch `Intent::SchemaLoaded`
    /// back — mirrors `external_edit`/`convert_write`'s async-signal shape.
    pub schema_fetch_request: Option<crate::schema::SchemaSource>,
}

impl SessionSnapshot {
    /// The error-slot text: `Some` iff a notice is present with
    /// `Severity::Error` (design spec §10 / §12 Q7).
    pub fn error_text(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|n| n.severity == Severity::Error)
            .map(|n| n.text.as_str())
    }

    /// The status-slot text: `Some` iff a notice is present with any
    /// non-Error severity (Info/Success/Warn — design spec §10 / §12 Q7).
    pub fn status_text(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|n| n.severity != Severity::Error)
            .map(|n| n.text.as_str())
    }
}
