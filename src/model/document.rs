use crate::model::node::{NodeTree, Path};
use std::path::Path as FsPath;

pub trait ConfigDocument: Sized {
    fn load(path: &FsPath) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError>;

    /// Serialize the node at `path` as a standalone fragment (for the clipboard and
    /// `$EDITOR`), starting at the node's own header/value line — an adjacent
    /// standalone comment is an independent node and is never part of the fragment.
    /// The empty path returns the whole document.
    fn serialize_fragment(&self, path: &[crate::model::node::Seg]) -> String;

    /// Like [`serialize_fragment`](Self::serialize_fragment) but **scope-relative**:
    /// a node copied out of a `[T/D]` dotted table has its leading dotted-ancestor
    /// key segments dropped (`dotted.test.bool_true` → `bool_true`). Used by
    /// copy/cut so a paste re-prefixes only for the new destination.
    fn serialize_fragment_relative(&self, path: &[crate::model::node::Seg]) -> String;

    /// The config syntax this document speaks (title bar, help text, comment
    /// validation).
    fn format(&self) -> DocFormat;
    /// Line-comment leader for this format ("#" / "//").
    fn comment_prefix(&self) -> &'static str;
    /// Whether authored comments are currently legal in this document
    /// (false only for a pure `.json` before the JSONC upgrade, Phase 2).
    fn supports_comments(&self) -> bool;

    /// The kinds/notations the node at `path` can convert to via
    /// [`Mutation::ConvertKind`], as `(label, target)` pairs — the current
    /// notation excluded. Empty when the node's kind cannot be switched.
    /// Labels are format-specific notation names rendered verbatim in the
    /// `K` popup. Positional legality (capture rules…) is still checked by
    /// `apply`; this lists only what is legal *by kind*.
    fn kind_options(&self, path: &[crate::model::node::Seg]) -> Vec<(String, KindTarget)>;
}

/// Which config syntax a document speaks. Backends report it via
/// [`ConfigDocument::format`]; the TUI uses it for the title bar, help text
/// and comment validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Toml,
    Json,
    Yaml,
}

/// Where an insert/move lands: insert as a child of `parent` at `index`.
#[derive(Clone, Debug)]
pub struct Target {
    pub parent: Path,
    pub index: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum OnCollision {
    Overwrite,
    Rename, // append _2, _3, ...
    Cancel,
}

#[derive(Clone, Debug)]
pub enum Mutation {
    Delete {
        path: Path,
    },
    Insert {
        target: Target,
        fragment: String,
        on_collision: OnCollision,
    },
    Replace {
        path: Path,
        fragment: String,
    },
    /// Rename the key at `path` to `new_key`, preserving its position and decor.
    Rename {
        path: Path,
        new_key: String,
    },
    Remark {
        path: Path,
    },
    /// Replace the text of the (multi-line) comment node at `path` with `text`,
    /// rewriting it in place within the owning decor slot.
    EditComment {
        path: Path,
        text: String,
    },
    Move {
        sources: Vec<Path>,
        target: Target,
        on_collision: OnCollision,
    },
    /// Insert a standalone comment block (`# …` lines) into `target.parent`'s
    /// decor at the projected `target.index`. Comments live in decor — no key,
    /// no collision.
    InsertComment {
        target: Target,
        text: String,
    },
    /// Convert the node at `path` to another notation of the same kind in place
    /// (the `K` kind-switch): a scalar to another notation of its type (string
    /// basic/literal/multiline forms, integer radix, float plain ↔ exponent), an
    /// array between inline, multiline and `[[…]]` group form, a table between
    /// `[T/I]`/`[T/D]`/`[T/S]` writing styles. Illegal conversions (a value the
    /// target notation can't represent, a position that would break the
    /// table-capture rule, comments that can't survive the target form) reject
    /// with the document untouched.
    ConvertKind {
        path: Path,
        target: KindTarget,
    },
}

/// The target of a [`Mutation::ConvertKind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindTarget {
    StringBasic,
    StringLiteral,
    StringMultiline,
    StringMultilineLiteral,
    IntDecimal,
    IntHex,
    IntOctal,
    IntBinary,
    FloatPlain,
    FloatExponent,
    ArrayInline,
    ArrayMultiline,
    ArrayOfTables,
    TableInline,
    TableDotted,
    TableScope,
    /// A JSON object spread over multiple lines (`[T/M]`). TOML's scope table
    /// stays `[T/S]`; this is the JSON multiline-object form.
    TableMultiline,
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("path not found")]
    NotFound,
    #[error("key collision: {0}")]
    Collision(String),
    #[error("invalid fragment: {0}")]
    Fragment(String),
    /// The node type is incompatible with the destination container, or the
    /// position would break TOML's table-capture rule (a scalar after a `[table]`
    /// header, or a `[table]` before the keys it would capture). Source-order /
    /// semantic legality — see the cross-layer-ops plan (D1/D5).
    #[error("{0}")]
    Illegal(String),
    #[error("operation not supported by this format")]
    Unsupported,
}
