//! `confy-tui` — the ratatui terminal UI and CLI for confy. Consumes the headless
//! [`confy_core`] crate. The `model` re-export below lets the UI modules keep
//! their `crate::model::…` paths against the core crate (see `PORTING.md`).

pub use confy_core::model;

pub mod cli;
pub mod tui;
