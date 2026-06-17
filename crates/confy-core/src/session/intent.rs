use serde::{Deserialize, Serialize};

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

    // ---- Selection ----
    ToggleSelect,
    ExtendSelectUp,
    ExtendSelectDown,

    // ---- Filter (/) ----
    EnterFilter,
    CommitFilter,
    ExitFilter,
    ExitFilterResults,
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

    // ---- Convert (C) ----
    OpenConvert,
    ConvertMove(i32),
    ConvertPickFormat,
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

    // ---- Inline edit ----
    BeginInlineEdit,
    BeginInlineRename,
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
}
