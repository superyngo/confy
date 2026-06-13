//! Lossless JSON/JSONC backend (mirrors the `cst_*` TOML trio). A hand-rolled
//! lexer/parser builds a `rowan` green tree as the single source of truth, so
//! `serialize()` is plain token concatenation and an untouched file round-trips
//! byte-identically. JSONC extensions: `//` line comments (Comment nodes /
//! trailing comments), read-only `/* */` blocks, and trailing commas accepted on
//! parse but never emitted by confy's own splices.

pub mod syntax;
pub mod parse;
pub mod doc;
pub mod project;
pub mod edit;

// pub use doc::JsonDocument;  // uncommented once doc.rs defines it (later task)
