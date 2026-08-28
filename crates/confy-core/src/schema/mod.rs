//! JSON Schema detection, validation, and constrained-editing support.
//! See `docs/superpowers/specs/2026-08-10-json-schema-support-design.md`.

pub mod dirty_check;
pub mod hints;
pub mod hints_edit;
pub mod types;
pub mod validate;
pub mod value_bridge;

pub use types::{
    Category, EditHint, SchemaSource, SchemaState, SchemaStatus, Violation, ViolationView,
};
pub use value_bridge::PointerMap;
