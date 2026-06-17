use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

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
    SkimMatcherV2::default()
        .fuzzy_match(haystack, needle)
        .is_some()
}

pub fn fuzzy_indices(haystack: &str, needle: &str) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return None;
    }
    SkimMatcherV2::default()
        .fuzzy_indices(haystack, needle)
        .map(|(_, idx)| idx)
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
