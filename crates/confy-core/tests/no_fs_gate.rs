//! PORTING.md §7 boundary gate: `confy-core`'s **runtime** code must never touch
//! the filesystem, the process environment, or any terminal/UI crate. The host
//! (confy-tui / a future WASM shim) owns all I/O; the core is pure.
//!
//! This walks `confy-core/src`, skips each file's trailing `#[cfg(test)]` module
//! (unit tests legitimately read fixtures), and fails on a forbidden token in the
//! remaining runtime code. Living in the `tests/` crate, this file's own `std::fs`
//! use is outside the scanned tree.

use std::fs;
use std::path::Path;

/// Tokens that must not appear in runtime core code.
const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::process",
    "std::env",
    "tempfile",
    "crossterm",
    "ratatui",
];

#[test]
fn core_runtime_is_fs_and_terminal_free() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    scan(&src, &mut violations);
    assert!(
        violations.is_empty(),
        "confy-core runtime code must stay filesystem/terminal-free \
         (move the I/O to the host); offending lines:\n{}",
        violations.join("\n")
    );
}

fn scan(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        // Scan only runtime code: everything before the file's `#[cfg(test)]`
        // module (the project convention is one trailing test module per file).
        let runtime = match text.find("#[cfg(test)]") {
            Some(i) => &text[..i],
            None => text.as_str(),
        };
        for (n, line) in runtime.lines().enumerate() {
            for tok in FORBIDDEN {
                if line.contains(tok) {
                    out.push(format!(
                        "{}:{}: `{tok}` in `{}`",
                        path.display(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
}
