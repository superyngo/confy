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

/// Everything before `text`'s trailing blank lines and its final `\n`
/// terminator — the "body" half of the multiline-editor package. Whitespace-only
/// trailing lines count as blank (matching [`measure`]), so a buffer the user's
/// editor left with `"  \n"` on the end still splits cleanly.
fn trim_trailing_blank_lines(text: &str) -> &str {
    let mut end = text.len();
    while end > 0 {
        let seg_start = text[..end].rfind('\n').map_or(0, |i| i + 1);
        if !text[seg_start..end].trim().is_empty() {
            break;
        }
        // Drop this blank (possibly empty) trailing line *and* the newline that
        // terminated the line before it.
        end = seg_start.saturating_sub(1);
        if seg_start == 0 {
            break;
        }
    }
    &text[..end]
}

/// Build the multiline editor's buffer: a node's `body` fragment terminated by
/// exactly one `\n`, followed by its `n` trailing blank lines. The node and its
/// blank run are edited as **one package**, so `n` is authored as literal empty
/// lines the user can add to or delete (`Session::multiline_edit_initial`).
/// An all-blank `body` yields an empty buffer — there is nothing to package.
pub(crate) fn with_trailing_run(body: &str, n: usize) -> String {
    let core = trim_trailing_blank_lines(body);
    if core.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(core.len() + n + 1);
    out.push_str(core);
    for _ in 0..=n {
        out.push('\n');
    }
    out
}

/// The inverse of [`with_trailing_run`]: split an edited buffer back into
/// `(body, trailing blank line count)`. The body always ends in exactly one
/// `\n`, so an editor that strips the file's final newline is indistinguishable
/// from one that keeps it.
pub(crate) fn split_trailing_run(text: &str) -> (String, usize) {
    let core = trim_trailing_blank_lines(text);
    if core.is_empty() {
        return (String::new(), 0);
    }
    let newlines = text[core.len()..].matches('\n').count();
    (format!("{core}\n"), newlines.saturating_sub(1))
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

    #[test]
    fn with_trailing_run_packages_the_body_and_its_blanks() {
        assert_eq!(with_trailing_run("a = 1\n", 0), "a = 1\n");
        assert_eq!(with_trailing_run("a = 1\n", 2), "a = 1\n\n\n");
        // A fragment that already carries trailing blanks (TOML's section
        // extent swallows them) is normalized first, never doubled.
        assert_eq!(with_trailing_run("[t]\nx = 1\n\n", 1), "[t]\nx = 1\n\n");
        // A fragment with no terminator at all (JSON member, YAML element).
        assert_eq!(with_trailing_run("  - name: b", 1), "  - name: b\n\n");
        assert_eq!(with_trailing_run("", 3), "", "nothing to package");
    }

    #[test]
    fn split_trailing_run_is_the_inverse() {
        for n in [0usize, 1, 3] {
            let buf = with_trailing_run("a = 1\n", n);
            assert_eq!(split_trailing_run(&buf), ("a = 1\n".to_string(), n));
        }
        // An editor that stripped the final newline reads as zero blanks, not
        // as a missing terminator.
        assert_eq!(split_trailing_run("a = 1"), ("a = 1\n".to_string(), 0));
        // Whitespace-only trailing lines are blanks.
        assert_eq!(
            split_trailing_run("a = 1\n  \n"),
            ("a = 1\n".to_string(), 1)
        );
        assert_eq!(split_trailing_run("\n\n"), (String::new(), 0));
    }
}
