use crate::model::node::{NodeTree, Path};
use std::path::Path as FsPath;

pub trait ConfigDocument: Sized {
    fn load(path: &FsPath) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError>;

    /// Serialize the node at `path` as a standalone fragment (for the clipboard and
    /// `$EDITOR`). The empty path returns the whole document. When `carry_comment`
    /// is set, an adjacent leading comment is carried into the fragment ($EDITOR);
    /// when clear it is not (clipboard copy).
    fn serialize_fragment(&self, path: &[crate::model::node::Seg], carry_comment: bool) -> String;
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
        toml: String,
        on_collision: OnCollision,
    },
    Replace {
        path: Path,
        toml: String,
        /// When `true` the `toml` is a full node fragment (from `$EDITOR`) whose key
        /// decor — including any adjacent leading comment — is authoritative and is
        /// synced back to the document. When `false` (inline value-only edits) the
        /// existing key decor is left untouched, so an inline edit never disturbs the
        /// node's comment.
        sync_decor: bool,
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
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("path not found")]
    NotFound,
    #[error("key collision: {0}")]
    Collision(String),
    #[error("invalid TOML fragment: {0}")]
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
