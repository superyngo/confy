//! SPIKE (spec §3.3 gate): lossless YAML-subset lexer/parser onto `rowan`.
//! Proves the gate — parse the subset structurally, round-trip byte-identically,
//! and fence out-of-subset constructs as `OPAQUE` (read-only) spans — before the
//! full Phase 3 backend (`doc`/`project`/`edit`) is planned and built.

pub mod parse;
pub mod syntax;
