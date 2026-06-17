pub mod dispatch;
pub mod host;
pub mod insertion;
pub mod intent;
pub mod search;
pub mod selection;
#[allow(clippy::module_inception)]
pub mod session;
pub mod state;
pub mod type_filter;
pub mod view;

pub use host::{EditTextOutcome, Host};
pub use insertion::resolve_target;
pub use intent::Intent;
pub use search::{fuzzy_indices, fuzzy_match, haystack};
pub use selection::{normalize, Selection};
pub use session::{format_label, node_type_label, node_type_label_str, Session};
pub use state::{
    Clipboard, ConvertState, ConvertStep, EditField, EditKind, EditState, FilterLayer, History,
    KindSwitchState, Mode, PasteSlot, PendingComment, PendingCommit, PendingExternalEdit,
    PromptKind,
};
pub use type_filter::{
    classify, layout, nav_rows, Cell, CheckState, Group, LayoutRow, TypeFilter, TypeToken,
};
pub use view::{
    ConvertView, EditView, ExternalEdit, ExternalEditKind, KindOptionView, ModeView, PromptView,
    SessionSnapshot, TypeFilterCellView, TypeFilterRow, TypeFilterView, Update, ViewRow,
};
