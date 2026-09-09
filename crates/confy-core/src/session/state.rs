use crate::model::document::{DocFormat, KindTarget, Target};
use crate::model::node::{Path, ScalarType};
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
    /// The Add-type picker is open — `AddNode`/`AddChild`/`AddSibling` route
    /// here instead of inserting directly.
    AddPicker(AddPickerState),
    /// The Action menu is open (design doc `docs/spec/2026-08-30-action-menu-design.md`
    /// §2, ADR 0009). No sub-state needed — the menu re-derives
    /// `items`/`target_count`/`target_label` from `selected_paths()` on every
    /// `mode_view()` call, so `Escape`/Commit never need to restore anything
    /// beyond `resting_mode()`.
    ActionMenu {
        cursor: usize,
    },
    /// The schema-enum picker popup is open (spec §3: reuses the `K`
    /// kind-switch popup's shape on every host).
    SchemaEnum(SchemaEnumState),
    /// The `C` document-conversion flow is open.
    Convert(ConvertState),
    Detail,
    Help(HelpTab),
    Edit(EditState),
}

/// Which tab of the shared Help/About panel (`?`) is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelpTab {
    Help,
    About,
}

/// Static About-tab text: author/version/license/repo, shown alongside Help.
pub const ABOUT_TEXT: &str = concat!(
    "confy ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "A cross-platform TUI/Web UI for editing structured configuration files.\n",
    "\n",
    "Author:    wen\n",
    "License:   MIT\n",
    "Copyright: (c) 2026 wen\n",
    "GitHub:    https://github.com/superyngo/confy\n",
    "Live demo: https://confy.turkeyang.net/\n",
    "VS Code:   https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode\n",
    "Open VSX:  https://open-vsx.org/extension/wenanlin/confy-vscode\n",
    "MS Store:  https://apps.microsoft.com/detail/9PLCJGQ3C654\n",
    "\n",
    "Privacy: confy runs entirely offline. It collects no telemetry and transmits no data. Files stay on your device/browser; the only network activity is the optional Open-from-URL feature fetching a URL you supply. Language/theme preferences are stored locally only.\n",
);

/// Static About-tab text, zh-TW body (Phase 4). Kept as its own const —
/// mirrors `ABOUT_TEXT`'s structure/spacing rather than routing through the
/// `i18n.rs` catalog, since `about_text(lang)` already expects a per-lang
/// `&'static str` here and the body isn't parameterized.
pub const ABOUT_TEXT_ZH_TW: &str = concat!(
    "confy ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "跨平台 TUI／Web UI，用於編輯結構化設定檔。\n",
    "\n",
    "作者：     wen\n",
    "授權：     MIT\n",
    "版權：     (c) 2026 wen\n",
    "GitHub：   https://github.com/superyngo/confy\n",
    "即時展示： https://confy.turkeyang.net/\n",
    "VS Code： https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode\n",
    "Open VSX： https://open-vsx.org/extension/wenanlin/confy-vscode\n",
    "MS Store： https://apps.microsoft.com/detail/9PLCJGQ3C654\n",
    "\n",
    "隱私權：confy 完全離線運作，不蒐集任何遙測資料，也不會傳輸任何資料。檔案僅保留在您的裝置／瀏覽器中；唯一的網路活動是選用的「從網址開啟」功能，會擷取您所提供的網址。語言／主題偏好設定僅儲存在本機。\n",
);

/// About-tab text for `lang`.
pub fn about_text(lang: crate::session::i18n::Lang) -> &'static str {
    match lang {
        crate::session::i18n::Lang::En => ABOUT_TEXT,
        crate::session::i18n::Lang::ZhTw => ABOUT_TEXT_ZH_TW,
    }
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

/// One selectable entry in the Add-type picker (`Mode::AddPicker`) — a
/// notation-independent node kind. `Scalar` carries which scalar type; every
/// other variant is a container or comment. Never carries a `Format`/notation
/// choice (that stays `K`'s job).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddKind {
    Scalar(ScalarType),
    Table,
    ArrayOfTables,
    InlineTable,
    Array,
    Comment,
}

/// In-flight Add-type picker popup state. `options` are `(display_label, kind)`
/// pairs — same order convention as `KindSwitchState::options`.
pub struct AddPickerState {
    pub target: Target,
    pub options: Vec<(String, AddKind)>,
    pub cursor: usize,
}

/// State for the schema-enum picker popup (spec §3: reuses the `K`
/// kind-switch popup's shape on every host). `options` are `(display_label,
/// value_repr)` pairs — `value_repr` is the document-format scalar text
/// `Session::schema_enum_commit` splices in directly via
/// `ConfigDocument::scalar_fragment`.
///
/// `from_schema` distinguishes two of the three ways this mode is entered: a
/// real schema `enum`/`const`/`oneOf`-of-`const` constraint (`true`), or the
/// schema-independent boolean fallback — a `bool` scalar always offers its
/// own two-option `true`/`false` picker (`false`). Hosts use it only to title
/// the popup ("Schema value" vs a neutral "Value"); every other behaviour
/// (move/jump/commit/cancel) is identical.
///
/// `from_kind_switch` marks the third: `K` on a TOML datetime, which is a
/// *type* pick wearing this widget (ADR 0012). It is not a styling hint but a
/// **widget** one — those options are kind options, so a host that renders
/// kind options in a dedicated list (the web UI's popover / overlay list) must
/// render these there too, not in whatever surface it uses for picking a
/// *value*. Without it the desktop web UI drew this one picker as an inline
/// `<select>` in the value cell while every other kind list was a list box.
#[derive(Clone, Debug)]
pub struct SchemaEnumState {
    pub path: Path,
    pub key: String,
    pub is_element: bool,
    pub created_on_add: bool,
    pub options: Vec<(String, String)>,
    pub cursor: usize,
    pub from_schema: bool,
    pub from_kind_switch: bool,
}

pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    TypeChange {
        from: String,
        to: String,
        /// Extra translated prose about what the change costs, appended to the
        /// question. Today: a TOML datetime retype's dropped/filled components
        /// (ADR 0012) — disclosed here, at the last moment before the value is
        /// rewritten, rather than inside the picker's option labels.
        note: Option<String>,
    },
    ArrayUpgrade {
        target: Target,
        on_collision: crate::model::document::OnCollision,
    },
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteSlot {
    Into(Path),
    After(Path),
}

pub struct Clipboard {
    pub fragments: Vec<String>,
    pub cut: bool,
    pub sources: Vec<Path>,
}

/// Undoable-entry cap — a ring buffer, not an unbounded stack. Fixed, not
/// configurable: no host exposes undo depth as a setting, and none should
/// (ADR 0003).
const MAX_HISTORY: usize = 200;

/// Second cap, on *bytes* rather than entries. An entry is a full serialized
/// snapshot, so the entry cap alone makes memory linear in document size:
/// measured (F9, 2026-09-09) at a steady **~201× the document** for a full
/// stack — 0.7 MB for a 3.5 KB file, but **216 MB for a 1 MB one**. The
/// entry cap is what a user perceives (200 `z` presses) and the byte cap is
/// what the process pays, so both apply and the tighter one wins: every
/// document up to ~80 KB keeps the full 200 steps, and past that the depth
/// shrinks instead of the footprint growing. At least one undo step always
/// survives, however large the document.
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;

pub struct History {
    past: std::collections::VecDeque<String>,
    current: String,
    future: Vec<String>,
}

impl History {
    pub fn new(initial: String) -> Self {
        History {
            past: std::collections::VecDeque::new(),
            current: initial,
            future: Vec::new(),
        }
    }
    /// An undo step is a *change*. A snapshot identical to `current` is
    /// therefore dropped, not stacked: several mutations legitimately re-author
    /// what is already there (committing an Add-picker seed at its default
    /// value, Enter on an unedited value, a `K` switch to the notation the node
    /// already uses), and each one used to cost the user an extra `z` that
    /// visibly did nothing. Doing it here rather than per-call-site keeps this
    /// the single definition of "an undoable entry" — and keeps `depth()`, which
    /// the VS Code host diffs to mirror the undo stack, honest. The redo
    /// `future` survives: with the text unchanged there is nothing to diverge
    /// from, so a no-op mutation after an undo must not strand the redo.
    pub fn push(&mut self, snapshot: String) {
        if snapshot == self.current {
            return;
        }
        self.past
            .push_back(std::mem::replace(&mut self.current, snapshot));
        self.future.clear();
        // Both caps, tighter wins; the byte total is summed rather than
        // tracked incrementally because it is at most 200 additions on a
        // path that has just serialized the whole document anyway.
        while self.past.len() > MAX_HISTORY
            || (self.past.len() > 1 && self.bytes() > MAX_HISTORY_BYTES)
        {
            self.past.pop_front();
        }
    }
    /// Bytes held by the undo stack (`past` + the redo `future`; `current` is
    /// the live document, not a snapshot of it).
    fn bytes(&self) -> usize {
        self.past.iter().chain(&self.future).map(|s| s.len()).sum()
    }
    pub fn undo(&mut self) -> Option<String> {
        let prev = self.past.pop_back()?;
        self.future
            .push(std::mem::replace(&mut self.current, prev.clone()));
        Some(prev)
    }
    pub fn redo(&mut self) -> Option<String> {
        let next = self.future.pop()?;
        self.past
            .push_back(std::mem::replace(&mut self.current, next.clone()));
        Some(next)
    }
    pub fn cancel_last(&mut self) -> Option<String> {
        let prev = self.past.pop_back()?;
        self.current = prev.clone();
        Some(prev)
    }
    pub fn current(&self) -> &str {
        &self.current
    }
    /// Undoable-entry count (`past.len()`). Hosts that mirror the undo stack
    /// (VS Code) diff this across dispatches: it grows on a history push and
    /// shrinks when `cancel_last` rolls the newest entry back (add→Esc).
    pub fn depth(&self) -> usize {
        self.past.len()
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
    fn small_documents_keep_the_full_entry_depth() {
        // 40 KB × 200 = 8 MB, half the byte budget: the entry cap is what bites.
        let mut h = History::new("x".repeat(40 * 1024));
        for i in 0..400 {
            h.push(format!("{i}{}", "x".repeat(40 * 1024)));
        }
        assert_eq!(h.depth(), MAX_HISTORY);
    }

    #[test]
    fn a_large_document_loses_depth_instead_of_growing_without_bound() {
        let mut h = History::new("x".repeat(1024 * 1024));
        for i in 0..400 {
            h.push(format!("{i}{}", "x".repeat(1024 * 1024)));
        }
        assert!(h.depth() < MAX_HISTORY, "depth {} not capped", h.depth());
        assert!(h.bytes() <= MAX_HISTORY_BYTES, "{} bytes", h.bytes());
        // …but undo never becomes unavailable.
        assert!(h.depth() >= 1);
        assert!(h.undo().is_some());
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

    #[test]
    fn push_of_identical_snapshot_is_not_an_undo_step() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string());
        h.push("v1".to_string());
        assert_eq!(h.depth(), 1, "re-authoring the same text is not a step");
        assert_eq!(h.undo(), Some("v0".to_string()));
        assert_eq!(h.undo(), None);
    }

    #[test]
    fn identical_push_keeps_redo_future() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string());
        assert_eq!(h.undo(), Some("v0".to_string()));
        h.push("v0".to_string());
        assert_eq!(
            h.redo(),
            Some("v1".to_string()),
            "no-op must not strand redo"
        );
    }
}
