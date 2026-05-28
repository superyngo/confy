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
    },
    Remark {
        path: Path,
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
