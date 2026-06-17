//! `confy-core` — the headless core: lossless CST model, the closed `Mutation`
//! set, and cross-format conversion. Pure and UI-agnostic (no terminal, no
//! rendering). The TUI and future WASM/web hosts consume this crate. See
//! `PORTING.md` at the repo root for the extraction plan.

pub mod model;
pub mod session;
