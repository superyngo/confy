//! String-escape helpers for basic (`"…"`/`"""…"""`) TOML strings — split out
//! of `cst_edit.rs` (Task 15, 2026-08-11 audit remediation).

use crate::model::document::MutateError;

/// The inner content of a string token's text: the delimiters dropped, a
/// multiline form's immediate leading newline trimmed.
pub(crate) fn string_inner(raw: &str, delim_len: usize) -> String {
    let inner = &raw[delim_len..raw.len().saturating_sub(delim_len)];
    if delim_len == 3 {
        inner
            .strip_prefix("\r\n")
            .or_else(|| inner.strip_prefix('\n'))
            .unwrap_or(inner)
            .to_string()
    } else {
        inner.to_string()
    }
}

/// Resolve the escapes of a basic (`"…"` / `"""…"""`) string's inner text.
pub(crate) fn unescape_basic(s: &str, multiline: bool) -> Result<String, MutateError> {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    let hex = |chars: &mut std::iter::Peekable<std::str::Chars>, n: usize| {
        let code: String = (0..n).filter_map(|_| chars.next()).collect();
        u32::from_str_radix(&code, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| MutateError::Illegal(format!("bad unicode escape `\\{code}`")))
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{8}'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('f') => out.push('\u{c}'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => out.push(hex(&mut chars, 4)?),
            Some('U') => out.push(hex(&mut chars, 8)?),
            // Line-ending backslash (multiline only): skip whitespace through
            // the next non-whitespace character.
            Some(w) if multiline && w.is_ascii_whitespace() => {
                while chars.peek().is_some_and(|p| p.is_ascii_whitespace()) {
                    chars.next();
                }
            }
            other => {
                return Err(MutateError::Illegal(format!(
                    "unsupported escape `\\{}`",
                    other.map(String::from).unwrap_or_default()
                )));
            }
        }
    }
    Ok(out)
}

/// Render `content` as a single-line basic string (`"…"`, escapes applied —
/// newlines become `\n`, so a multiline source converts losslessly).
pub(crate) fn encode_basic_string(content: &str) -> String {
    let mut out = String::from("\"");
    for c in content.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render `content` as a multiline basic string (`"""…"""`): newlines and tabs
/// stay raw, backslashes and delimiter-forming quote runs are escaped, and a
/// leading newline is doubled (the parser trims the one right after `"""`).
pub(crate) fn encode_multiline_basic(content: &str) -> String {
    let mut out = String::from("\"\"\"");
    if content.starts_with('\n') || content.starts_with("\r\n") {
        out.push('\n');
    }
    let mut quotes = 0usize;
    for c in content.chars() {
        match c {
            '"' => {
                quotes += 1;
                if quotes == 3 {
                    out.pop();
                    out.push_str("\\\"\"");
                    quotes = 0;
                } else {
                    out.push('"');
                }
                continue;
            }
            '\\' => out.push_str("\\\\"),
            '\n' | '\t' => out.push(c),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
        quotes = 0;
    }
    if out.ends_with('"') {
        out.pop();
        out.push_str("\\\"");
    }
    out.push_str("\"\"\"");
    out
}
