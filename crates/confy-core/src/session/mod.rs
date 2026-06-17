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
pub use session::{Session, format_label, node_type_label, node_type_label_str};
pub use state::{
    Clipboard, ConvertState, ConvertStep, EditField, EditKind, EditState, FilterLayer, History,
    KindSwitchState, Mode, PasteSlot, PendingComment, PendingCommit, PromptKind,
};
pub use type_filter::{
    Cell, CheckState, Group, LayoutRow, TypeFilter, TypeToken, classify, layout, nav_rows,
};
pub use view::{Update, ViewRow};
