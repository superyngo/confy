use crate::model::node::NodeTree;
use std::path::Path;

pub trait ConfigDocument: Sized {
    fn load(path: &Path) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    // `apply` (Mutation) added in Task 6.
}
