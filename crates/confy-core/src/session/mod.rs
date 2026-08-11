pub mod clipboard;
pub mod dispatch;
pub mod host;
pub mod i18n;
pub mod inline_edit;
pub mod insertion;
pub mod intent;
pub mod schema_hint;
pub mod search;
pub mod selection;
#[allow(clippy::module_inception)]
pub mod session;
pub mod state;
pub mod status_fmt;
pub mod type_filter;
pub mod undo_redo;
pub mod view;

pub use host::{EditTextOutcome, Host};
pub use i18n::{tr, tr_args, Lang};
pub use insertion::resolve_target;
pub use intent::Intent;
pub use search::{fuzzy_indices, fuzzy_match, haystack};
pub use selection::{normalize, Selection};
pub use session::Session;
pub use status_fmt::{format_label, node_type_label, node_type_label_str};
pub use state::{
    Clipboard, ConvertState, ConvertStep, EditField, EditKind, EditState, FilterLayer, HelpTab,
    History, KindSwitchState, Mode, PasteSlot, PendingComment, PendingCommit, PendingExternalEdit,
    PromptKind,
};
pub use type_filter::{
    classify, layout, nav_rows, Cell, CheckState, Group, LayoutRow, TypeFilter, TypeToken,
};
pub use view::{
    ChildView, ConvertView, EditView, ExternalEdit, ExternalEditKind, KindOptionView, ModeView,
    PromptView, SessionSnapshot, TypeFilterCellView, TypeFilterRow, TypeFilterView, ViewRow,
};
