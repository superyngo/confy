/// The one mid-operation callback the core issues to its host environment.
///
/// TUI host → spawns `$EDITOR`. Web/VSCode host → opens an in-app multi-line
/// modal. The core never touches the terminal or filesystem directly.
pub trait Host {
    /// Open `initial` in an external/multi-line editor; return the outcome.
    fn edit_text(&self, initial: String) -> EditTextOutcome;
}

/// Result of a [`Host::edit_text`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditTextOutcome {
    /// The user edited and saved; this is the new content.
    Edited(String),
    /// The user cancelled (editor exited non-zero, or the host modal was dismissed).
    Cancelled,
}
