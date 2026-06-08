use crate::model::node::{NodeTree, Path};
use std::path::Path as FsPath;

pub trait ConfigDocument: Sized {
    fn load(path: &FsPath) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError>;
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
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("path not found")]
    NotFound,
    #[error("key collision: {0}")]
    Collision(String),
    #[error("invalid TOML fragment: {0}")]
    Fragment(String),
    #[error("operation not supported by this format")]
    Unsupported,
}
