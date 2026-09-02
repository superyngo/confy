pub mod any_doc;
pub mod convert;
pub mod cst_doc;
pub mod cst_edit;
pub mod cst_project;
pub mod document;
pub mod json;
pub mod node;
pub mod text_range;
pub mod value;
pub mod yaml;

/// Maximum container nesting depth any backend will parse. All three parsers
/// are recursive descent (TOML via taplo), so an unbounded `[[[[…` overflows
/// the stack — a hard abort natively and a page-killing trap in wasm. 256 is
/// far beyond any real config file and well inside the default main-thread
/// stack for every host.
pub const MAX_NESTING_DEPTH: usize = 256;

/// The error text every backend reports when [`MAX_NESTING_DEPTH`] is exceeded.
pub(crate) fn nesting_error() -> String {
    format!("nesting deeper than {MAX_NESTING_DEPTH} levels is not supported")
}

/// Cheap pre-scan for backends whose parser we don't own (taplo): does the
/// bracket/brace nesting of `src` — ignoring `"…"`/`'…'` strings and `#`
/// comments — ever exceed [`MAX_NESTING_DEPTH`]?
pub(crate) fn bracket_depth_exceeds(src: &str) -> bool {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut in_comment = false;
    for &b in src.as_bytes() {
        if in_comment {
            in_comment = b != b'\n';
            continue;
        }
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if b == b'\\' && q == b'"' {
                escaped = true;
            } else if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'#' => in_comment = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod nesting_tests {
    use super::*;

    #[test]
    fn bracket_scan_ignores_strings_and_comments() {
        let deep = "[".repeat(MAX_NESTING_DEPTH + 1);
        assert!(!bracket_depth_exceeds(&format!("a = \"{deep}\"\n")));
        assert!(!bracket_depth_exceeds(&format!("a = '{deep}'\n")));
        assert!(!bracket_depth_exceeds(&format!("# {deep}\n")));
        assert!(bracket_depth_exceeds(&format!("a = {deep}\n")));
        assert!(!bracket_depth_exceeds(&format!(
            "a = {}{}\n",
            "[".repeat(MAX_NESTING_DEPTH),
            "]".repeat(MAX_NESTING_DEPTH)
        )));
    }
}
