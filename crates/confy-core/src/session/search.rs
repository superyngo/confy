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

/// Smart case (a lowercase needle is case-insensitive, an uppercase character
/// makes that position case-sensitive) and smart Unicode normalization —
/// the closest equivalent of `SkimMatcherV2::default()`'s behavior.
fn pattern(needle: &str) -> Pattern {
    Pattern::new(
        needle,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    )
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

pub fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    MATCHER.with_borrow_mut(|m| {
        let mut buf = Vec::new();
        pattern(needle)
            .score(Utf32Str::new(haystack, &mut buf), m)
            .is_some()
    })
}

/// Matched **char** positions, ascending and deduplicated. Char, not byte:
/// the TUI slices `text.chars()` and the web mirror indexes `Array.from(text)`,
/// and `nucleo` reports positions into its own UTF-32 view, which is the same
/// thing. It does not promise them sorted or unique, so both are enforced here
/// rather than in each of the three call sites.
pub fn fuzzy_indices(haystack: &str, needle: &str) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return None;
    }
    MATCHER.with_borrow_mut(|m| {
        let mut buf = Vec::new();
        let mut idx = Vec::new();
        pattern(needle).indices(Utf32Str::new(haystack, &mut buf), m, &mut idx)?;
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
        assert!(fuzzy_match("server.port 8080", "srvport"));
        assert!(!fuzzy_match("server.host", "zzz"));
    }

    #[test]
    fn fuzzy_indices_returns_matched_positions() {
        assert_eq!(fuzzy_indices("axbycz", "abc"), Some(vec![0, 2, 4]));
        assert_eq!(fuzzy_indices("server", "zzz"), None);
        assert_eq!(fuzzy_indices("server", ""), None);
    }
}
