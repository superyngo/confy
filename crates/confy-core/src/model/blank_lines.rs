//! The one blank-line rule shared by all three backends. A node's trailing
//! blank lines are pure inter-node trivia in every format, so the surgery is
//! format-neutral text work on the serialized document; the only per-backend
//! knowledge is the anchor — the byte offset just past the node's contiguous
//! extent — which each backend derives from the same span logic its `Delete`
//! uses. `Mutation::SetTrailingBlankLines`.

/// Normalize a node-extent offset to the **line boundary** `count_after` and
/// `splice` require: if `at` already sits just past a `\n` (or at the file
/// start) it is returned unchanged, otherwise it advances through the rest of
/// its line. Backends differ on whether a node's `text_range` includes its
/// terminating newline — YAML's `MAP_ENTRY` does, TOML's entries and JSON's
/// members do not — and advancing unconditionally would skip a whole line for
/// the former, putting the blank run after the *next* node.
pub(crate) fn line_boundary_at(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    if at == 0 || text.as_bytes()[at - 1] == b'\n' {
        return at;
    }
    match text[at..].find('\n') {
        Some(i) => at + i + 1,
        None => text.len(),
    }
}

/// How many blank lines follow the anchor `end` (a maximal run of lines that
/// are empty or whitespace-only). `end` must be a line boundary — the offset
/// just past a node's terminating newline.
pub(crate) fn count_after(text: &str, end: usize) -> usize {
    let (n, _) = measure(text, end);
    n
}

/// `(blank line count, byte length of the run)` starting at `end`.
fn measure(text: &str, end: usize) -> (usize, usize) {
    let rest = &text[end.min(text.len())..];
    let mut n = 0usize;
    let mut consumed = 0usize;
    for line in rest.split_inclusive('\n') {
        // A final fragment with no `\n` is the file's last, unterminated line;
        // it is content-or-nothing, never a blank *line*.
        if !line.ends_with('\n') || !line.trim().is_empty() {
            break;
        }
        n += 1;
        consumed += line.len();
    }
    (n, consumed)
}

/// Rewrite the blank run after `end` to exactly `n` clean blank lines.
/// Whitespace-only lines are normalized to empty ones. When `end` is EOF on an
/// unterminated last line, a terminating newline is emitted first so `n`
/// really is a count of *blank lines* and not of line terminators.
pub(crate) fn splice(text: &str, end: usize, n: usize) -> String {
    let end = end.min(text.len());
    let (_, consumed) = measure(text, end);
    let head = &text[..end];
    let tail = &text[end + consumed..];
    let mut out = String::with_capacity(text.len() + n);
    out.push_str(head);
    if n > 0 && !head.is_empty() && !head.ends_with('\n') {
        out.push('\n');
    }
    for _ in 0..n {
        out.push('\n');
    }
    out.push_str(tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // `end` is the offset just past a node's extent, i.e. just past its
    // terminating newline — the anchor every backend hands us.
    const SRC: &str = "a = 1\n\n\nb = 2\n";

    #[test]
    fn counts_the_blank_run_after_the_anchor() {
        let end = SRC.find("\n\n").unwrap() + 1; // just past `a = 1\n`
        assert_eq!(count_after(SRC, end), 2);
        assert_eq!(count_after(SRC, SRC.len()), 0, "EOF has no blank run");
        assert_eq!(count_after("a = 1\nb = 2\n", 6), 0);
    }

    #[test]
    fn counts_a_whitespace_only_line_as_blank() {
        assert_eq!(count_after("a = 1\n   \n\t\nb = 2\n", 6), 2);
    }

    #[test]
    fn splice_sets_grows_and_clears_the_run() {
        let end = 6;
        assert_eq!(splice(SRC, end, 2), SRC, "no-op is byte-identical");
        assert_eq!(splice(SRC, end, 0), "a = 1\nb = 2\n");
        assert_eq!(splice(SRC, end, 1), "a = 1\n\nb = 2\n");
        assert_eq!(splice(SRC, end, 4), "a = 1\n\n\n\n\nb = 2\n");
    }

    #[test]
    fn splice_replaces_whitespace_only_lines_with_clean_blanks() {
        assert_eq!(splice("a = 1\n   \nb = 2\n", 6, 1), "a = 1\n\nb = 2\n");
    }

    #[test]
    fn splice_at_eof_terminates_the_last_line_first() {
        // No trailing newline: adding a blank line means terminating `a = 1`
        // and then emitting the blanks.
        assert_eq!(splice("a = 1", 5, 1), "a = 1\n\n");
        assert_eq!(splice("a = 1", 5, 0), "a = 1");
    }
}
