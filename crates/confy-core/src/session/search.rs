use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::cell::RefCell;

// One shared matcher — `fuzzy_match`/`fuzzy_indices` are called per node per
// filter keystroke (and per rendered row while a filter is active), so
// rebuilding it each call is pure churn.
//
// `nucleo_matcher::Matcher` holds reusable scratch buffers and so needs
// `&mut`, which is why this is a `thread_local!` `RefCell` rather than the
// `LazyLock` the old `SkimMatcherV2` allowed. Cheap either way: no lock, and
// the wasm host is single-threaded regardless. (Swapped from the unmaintained
// `fuzzy-matcher 0.3` — F11.)
thread_local! {
    static MATCHER: RefCell<Matcher> = RefCell::new(Matcher::new(Config::DEFAULT));
}

/// How the text filter matches a query term. Session-scoped, never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// Characters in order, gaps allowed (`srvport` matches `server.port`).
    #[default]
    Fuzzy,
    /// Each term must appear as a contiguous run (`nucleo`'s `Substring`, not its
    /// whole-haystack `Exact`). Case, normalization and term splitting stay as in `Fuzzy`.
    Exact,
}

/// Smart case (a lowercase needle is case-insensitive, an uppercase character
/// makes that position case-sensitive) and smart Unicode normalization —
/// the closest equivalent of `SkimMatcherV2::default()`'s behavior.
fn pattern(needle: &str, mode: MatchMode) -> Pattern {
    let kind = match mode {
        MatchMode::Fuzzy => AtomKind::Fuzzy,
        MatchMode::Exact => AtomKind::Substring,
    };
    Pattern::new(needle, CaseMatching::Smart, Normalization::Smart, kind)
}

pub fn haystack(path_keys: &[&str], leaf_value: Option<&str>, comment: Option<&str>) -> String {
    let mut s = path_keys.join(".");
    if let Some(v) = leaf_value {
        s.push(' ');
        s.push_str(v);
    }
    if let Some(c) = comment {
        s.push(' ');
        s.push_str(c);
    }
    s
}

pub fn fuzzy_match(haystack: &str, needle: &str, mode: MatchMode) -> bool {
    if needle.is_empty() {
        return true;
    }
    MATCHER.with_borrow_mut(|m| {
        let mut buf = Vec::new();
        pattern(needle, mode)
            .score(Utf32Str::new(haystack, &mut buf), m)
            .is_some()
    })
}

/// Matched **char** positions, ascending and deduplicated. Char, not byte:
/// the TUI slices `text.chars()` and the web mirror indexes `Array.from(text)`,
/// and `nucleo` reports positions into its own UTF-32 view, which is the same
/// thing. It does not promise them sorted or unique, so both are enforced here
/// rather than in each of the three call sites.
pub fn fuzzy_indices(haystack: &str, needle: &str, mode: MatchMode) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return None;
    }
    MATCHER.with_borrow_mut(|m| {
        let mut buf = Vec::new();
        let mut idx = Vec::new();
        pattern(needle, mode).indices(Utf32Str::new(haystack, &mut buf), m, &mut idx)?;
        idx.sort_unstable();
        idx.dedup();
        Some(idx.into_iter().map(|i| i as usize).collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haystack_includes_path_and_value() {
        let h = haystack(&["server", "port"], Some("8080"), None);
        assert!(h.contains("server.port"));
        assert!(h.contains("8080"));
    }

    #[test]
    fn matches_filter() {
        assert!(fuzzy_match("server.port 8080", "srvport", MatchMode::Fuzzy));
        assert!(!fuzzy_match("server.host", "zzz", MatchMode::Fuzzy));
    }

    #[test]
    fn fuzzy_indices_returns_matched_positions() {
        let f = MatchMode::Fuzzy;
        assert_eq!(fuzzy_indices("axbycz", "abc", f), Some(vec![0, 2, 4]));
        assert_eq!(fuzzy_indices("server", "zzz", f), None);
        assert_eq!(fuzzy_indices("server", "", f), None);
    }

    #[test]
    fn exact_mode_requires_contiguous_terms() {
        let e = MatchMode::Exact;
        assert!(!fuzzy_match("server.port 8080", "srvport", e));
        assert!(fuzzy_match("server.port 8080", "r.po", e));
        assert_eq!(
            fuzzy_indices("server.port", "r.po", e),
            Some(vec![5, 6, 7, 8])
        );
        // Terms stay order-free; smart case stays.
        assert!(fuzzy_match("server.port 8080", "8080 port", e));
        assert!(fuzzy_match("Server", "server", e));
        assert!(!fuzzy_match("server", "Server", e));
        assert!(fuzzy_match("x", "", e));
    }
}
