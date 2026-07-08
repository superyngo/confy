use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::sync::LazyLock;

/// One shared matcher — `fuzzy_match`/`fuzzy_indices` are called per node per
/// filter keystroke (and per rendered row while a filter is active), so
/// rebuilding `SkimMatcherV2::default()` each call was pure churn.
static MATCHER: LazyLock<SkimMatcherV2> = LazyLock::new(SkimMatcherV2::default);

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
    MATCHER.fuzzy_match(haystack, needle).is_some()
}

pub fn fuzzy_indices(haystack: &str, needle: &str) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return None;
    }
    MATCHER.fuzzy_indices(haystack, needle).map(|(_, idx)| idx)
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
