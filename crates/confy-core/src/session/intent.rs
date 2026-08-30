use serde::{Deserialize, Serialize};

/// Serde default for `Intent::MoveSelectionTo.cut` — omitting the field on
/// the wire preserves the pre-ADR-0004 cut-only behavior.
fn default_move_cut() -> bool {
    true
}

/// Every user-facing action the TUI can dispatch to the Session.
/// The event loop translates raw key events to `Intent` values; the Session
/// drives all state changes from there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
    // ---- Navigation ----
    CursorDown,
    CursorUp,
    CursorHome,
    CursorEnd,
    PageUp(usize),
    PageDown(usize),
    ToggleExpand,
    CollapseAll,
    ExpandAll,
    ExpandLevel,
    CollapseLevel,

    // ---- Pointer (Web UI) ----
    /// Place the cursor on a visible row by path (pointer analogue of the
    /// navigation intents). Ignored if the path is not currently visible.
    SetCursor(crate::model::node::Path),
    /// Pointer analogue of the TUI's arrow-key `PasteSlot` stepping (ADR 0004
    /// §1): set the armed clipboard's target directly. Built from
    /// `ConfySession::pointer_slot(path, rel_y)`; ignored if the path isn't
    /// currently visible when it lands (mirrors `SetCursor`).
    SetPasteSlot(crate::session::state::PasteSlot),
    /// **Reveal** (CONTEXT.md §Operations): expand every ancestor of `path`
    /// and place the cursor on it (Web UI breadcrumb mini-tree jump). No-op if
    /// the path doesn't exist; if an active filter still hides the row, the
    /// expansion sticks, the cursor stays put, and the status line reports it.
    RevealPath(crate::model::node::Path),
    /// One-shot inline edit commit (pointer analogue of the `BeginEdit` →
    /// type → `EditCommit` keyboard flow). `value` replaces the scalar/comment
    /// text, `name` renames the key; `None` keeps the current one. Reuses the
    /// full `edit_commit` machinery (type-change / collision / trailing prompts).
    CommitEdit {
        value: Option<String>,
        name: Option<String>,
    },
    /// One-shot kind switch (pointer analogue of `OpenKindSwitch` →
    /// `KindSwitchCommit`). `target` is a `KindTarget` from `kind_options(path)`.
    CommitKind {
        path: crate::model::node::Path,
        target: crate::model::document::KindTarget,
    },
    /// Replace the whole selection with `paths` (pointer analogue of the
    /// keyboard selection keys). The Web UI resolves a click / ⇧-range /
    /// ⌘-toggle / marquee gesture into a final set; non-visible paths are
    /// dropped and the cursor follows the focal (last) path.
    SetSelection {
        paths: Vec<crate::model::node::Path>,
    },
    /// Set, change, or clear the **trailing inline comment** of the node at
    /// `path` (Web UI: the separate comment cell + the "Append comment" menu).
    /// `Some(text)` sets/changes it (text carries its own `#`/`//` prefix),
    /// `None` clears it. Reuses `Mutation::SetTrailingComment`, so unsupported
    /// targets (inline collections, …) reject with the document untouched.
    SetTrailing {
        path: crate::model::node::Path,
        comment: Option<String>,
    },
    /// Drag-reparent (Web UI): move `sources` into `target` at child `index`.
    /// A one-shot cut→paste reusing the full collision / illegal-destination /
    /// array-upgrade machinery; a drop onto a source or into its own subtree is
    /// rejected and the document is left untouched.
    MoveSelectionTo {
        sources: Vec<crate::model::node::Path>,
        target: crate::model::node::Path,
        index: usize,
        /// Copy (`false`) vs move (`true`, the default). A drag-drop with the
        /// platform copy modifier (⌥/Ctrl) held sends `false`; a plain
        /// drag-drop omits it (ADR 0004 §1).
        #[serde(default = "default_move_cut")]
        cut: bool,
    },

    // ---- Selection ----
    ToggleSelect,
    ExtendSelectUp,
    ExtendSelectDown,

    // ---- Filter (/) ----
    EnterFilter,
    CommitFilter,
    ExitFilter,
    ExitFilterResults,
    /// Set the whole filter text at once (Web UI live-search `<input>`).
    /// Non-empty → `FilterResults`; empty drops back to the resting mode.
    SetFilter(String),
    FilterChar(char),
    FilterBackspace,
    FilterDelete,
    FilterCursorLeft,
    FilterCursorRight,
    FilterCursorHome,
    FilterCursorEnd,

    // ---- Type filter (f) ----
    EnterTypeFilter,
    CommitTypeFilter,
    ExitTypeFilter,
    TypeFilterMove(i32, i32),
    TypeFilterToggle,

    // ---- Kind switch (K) ----
    OpenKindSwitch,
    KindSwitchMove(i32),
    KindSwitchCommit,
    ExitKindSwitch,

    // ---- Action menu (m) ----
    OpenActionMenu,
    ActionMenuMove(i32),
    ActionMenuCommit,
    /// Web pointer analogue of `ActionMenuCommit`: apply a directly-chosen
    /// item without moving the cursor first.
    ActionMenuPick(crate::session::view::ActionId),
    ExitActionMenu,

    // ---- Convert (C) ----
    OpenConvert,
    ConvertMove(i32),
    ConvertPickFormat,
    /// Web UI: pick the convert target format by value (a `<select>`).
    SetConvertFormat(crate::model::document::DocFormat),
    /// Web UI: set the whole output path at once (an `<input>`).
    SetConvertPath(String),
    ConvertPathChar(char),
    ConvertPathBackspace,
    ConvertPathDelete,
    ConvertPathLeft,
    ConvertPathRight,
    ConvertPathHome,
    ConvertPathEnd,
    ConvertRun,
    ConvertConfirm,
    ExitConvert,

    // ---- Detail popup (i) ----
    ToggleDetail,
    ExitDetail,
    DetailScrollBy(i32, u16),
    DetailSetScroll(u16),

    // ---- Help (?) ----
    EnterHelp,
    ExitHelp,
    HelpScrollBy(i32, u16),
    HelpSetScroll(u16),
    /// Flip the shared Help/About panel between its two tabs (TUI `Tab` key /
    /// Web UI tab-button click), while `Mode::Help(_)` is active.
    ToggleHelpTab,

    // ---- Inline edit ----
    BeginEdit,
    /// Force the external popup editor unconditionally, bypassing the
    /// schema-aware inline/SchemaEnum-picker branch `BeginEdit` takes for
    /// scalars. Mirrors the TUI's `E` key (`App::edit_node`, always
    /// external regardless of node kind or schema).
    BeginEditExternal,
    BeginRename,
    EditToggleField,
    EditClampScroll(usize),
    EditChar(char),
    EditBackspace,
    EditDelete,
    EditCursorLeft,
    EditCursorRight,
    EditCursorHome,
    EditCursorEnd,
    EditCommit,
    EditCancel,

    // ---- External edit ($EDITOR) — dispatched by host ----
    /// Host already obtained edited text; apply it as Replace at the path.
    ApplyReplace {
        path: crate::model::node::Path,
        text: String,
    },
    /// Host already obtained edited comment text.
    ApplyEditComment {
        path: crate::model::node::Path,
        text: String,
    },

    // ---- Mutations ----
    Nudge(i64),
    AddNode,
    /// Force a child insertion regardless of the cursor's expand state (Web `+`).
    AddChild,
    /// Force a sibling insertion regardless of the cursor's expand state (Web menu).
    AddSibling,
    DeleteSelected,
    CopySelected,
    CutSelected,
    Paste,
    Remark,

    // ---- Undo / Redo ----
    Undo,
    Redo,

    // ---- Lifecycle ----
    Escape,
    PromptKey(char),
    QuitRequested,
    Save,

    // ---- i18n ----
    /// Switch the UI language. A string (not the `Lang` enum) to keep the wasm
    /// wire contract simple; an unrecognized code leaves the current language
    /// unchanged (never panics).
    SetLang(String),

    // ---- Host notices ----
    /// **Not a user action** — the internal channel hosts use to report
    /// their own errors/notices through the sole dispatch path, so
    /// `dispatch` stays the only mutation route into the Session (design
    /// spec §5, §12 Q6). `key` resolves through the shared `severity_of`
    /// table; `source` stamps the notice (`HostTui`/`HostWeb` — `Core`
    /// here is a caller bug the handler ignores). A `String` (not
    /// `&'static str`) to keep the wasm wire contract deserializable,
    /// matching `SetLang`'s precedent.
    SetHostNotice {
        key: String,
        args: Vec<String>,
        source: crate::session::notice::NoticeSource,
    },
    // Schema
    /// Re-run `detect_and_request_schema()` against the current document and
    /// stash the result into `pending_schema_fetch` — **not** an idempotent
    /// no-op: it unconditionally overwrites `pending_schema_fetch`, even
    /// with `None`, whenever called. Hosts that want to avoid redundant
    /// fetch/recompile after every edit must compare the returned
    /// `schema_fetch_request` against what they already have loaded
    /// themselves (VS Code schema-hints design).
    DetectSchema,
    SchemaLoaded {
        source: crate::schema::SchemaSource,
        text: Result<String, String>,
    },
    SchemaEnumMove(i32),
    /// Clamped jump (PageUp/PageDown/Home/End) — see `Session::schema_enum_jump`.
    SchemaEnumJump(i32),
    SchemaEnumCommit,
}
