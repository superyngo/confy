//! Integration tests for `confy <url>` (open-from-URL CLI entry point).
//!
//! Only the non-TTY abort path is exercised here: `assert_cmd` spawns the
//! binary with piped (non-terminal) stdin, which deterministically hits
//! `open_url`'s TTY guard before any prompt, network call, or filesystem
//! write happens — network-free and hermetic, mirroring `convert_cli.rs`'s
//! coverage of the analogous non-interactive-confirm path. The real
//! interactive prompt → fetch → write → TUI-launch flow has no PTY test
//! harness in this repo (same ceiling as `create_missing_file`'s and
//! `run_convert`'s y/N prompts) and is covered by manual smoke test instead.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::TempDir;

fn confy() -> Command {
    Command::cargo_bin("confy").unwrap()
}

#[test]
fn url_open_without_a_tty_aborts_without_writing_or_fetching() {
    let dir = TempDir::new().unwrap();

    // A URL that would fail to resolve/connect if a network call were ever
    // attempted — proves the TTY guard runs first and short-circuits before
    // any `ureq::get` call.
    confy()
        .current_dir(dir.path())
        .arg("https://url-open-cli-test.invalid/does-not-matter.toml")
        .assert()
        .failure()
        .stderr(contains("terminal"));

    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        0,
        "no file should be written on a non-interactive abort"
    );
}

#[test]
fn non_url_paths_are_unaffected_by_url_detection() {
    // Regression guard: a local path containing no `http(s)://` prefix must
    // still take the ordinary existing-file path, not the URL branch.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("plain.toml");
    fs::write(&file, "a = 1\n").unwrap();

    // Non-interactive stdin also can't drive the ratatui event loop, so this
    // just asserts the process doesn't take the URL-prompt code path (which
    // would fail with a "terminal" message); a normal existing-file open
    // proceeds into `tui::run`, which fails differently (no PTY) — the point
    // here is only that it's NOT the URL branch's error message.
    let output = confy().current_dir(dir.path()).arg(&file).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Save") && !stderr.contains("prompt for a save path"),
        "a local path must never enter the URL-open prompt branch, got: {stderr}"
    );
}
