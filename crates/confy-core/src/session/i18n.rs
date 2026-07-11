//! Translation catalog (Phase 1 of the i18n plan). Pure data, embedded at
//! compile time via `include_str!` — no filesystem access at runtime, so
//! `confy-core` stays fs-free (`tests/no_fs_gate.rs`).
//!
//! The catalog lives at the repo root (`i18n/en.json`, `i18n/zh-TW.json`), a
//! flat `key -> message` map shared with the TypeScript side (Phase 3).
//! `en` is canonical: `tr`/`tr_args` fall back to the `en` entry, then to the
//! raw key, so a missing translation can never panic or blank the UI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::OnceLock;

/// A UI language. `En` is the default and the canonical source of truth for
/// every catalog key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-TW")]
    ZhTw,
}

impl Lang {
    /// The wire-format code (`"en"` / `"zh-TW"`), mirroring the serde rename.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhTw => "zh-TW",
        }
    }
}

impl FromStr for Lang {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "en" => Ok(Lang::En),
            "zh-TW" => Ok(Lang::ZhTw),
            _ => Err(()),
        }
    }
}

fn en_catalog() -> &'static HashMap<String, String> {
    static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../i18n/en.json")).unwrap_or_default()
    })
}

fn zh_tw_catalog() -> &'static HashMap<String, String> {
    static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../i18n/zh-TW.json")).unwrap_or_default()
    })
}

fn catalog(lang: Lang) -> &'static HashMap<String, String> {
    match lang {
        Lang::En => en_catalog(),
        Lang::ZhTw => zh_tw_catalog(),
    }
}

/// Look up `key` in `lang`'s catalog, falling back to `en`, then to the raw
/// key string. Never panics.
pub fn tr(lang: Lang, key: &str) -> &'static str {
    if let Some(v) = catalog(lang).get(key) {
        return v.as_str();
    }
    if let Some(v) = en_catalog().get(key) {
        return v.as_str();
    }
    // Missing key (should not happen for a real `core.*` key since `en.json`
    // is canonical) — leak the raw key so the return type can stay
    // `&'static str`; this path is a bug signal, never hit in normal use.
    Box::leak(key.to_string().into_boxed_str())
}

/// Same lookup as `tr`, substituting positional `{0}`, `{1}`, … placeholders
/// with `args` in order.
pub fn tr_args(lang: Lang, key: &str, args: &[&str]) -> String {
    let template = tr(lang, key);
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = template[i + 1..].find('}') {
                let inner = &template[i + 1..i + 1 + end];
                if let Ok(idx) = inner.parse::<usize>() {
                    if let Some(arg) = args.get(idx) {
                        out.push_str(arg);
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
            }
        }
        // Fall back: copy one char.
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholder_count(s: &str) -> usize {
        let mut n = 0;
        let mut i = 0;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(end) = s[i + 1..].find('}') {
                    if s[i + 1..i + 1 + end].parse::<usize>().is_ok() {
                        n += 1;
                    }
                    i += end + 2;
                    continue;
                }
            }
            i += 1;
        }
        n
    }

    #[test]
    fn zh_tw_keys_exist_in_en_and_placeholders_match() {
        let en = en_catalog();
        let zh = zh_tw_catalog();
        for (k, zh_v) in zh.iter() {
            let en_v = en
                .get(k)
                .unwrap_or_else(|| panic!("zh-TW key '{k}' missing from en.json (canonical)"));
            assert_eq!(
                placeholder_count(en_v),
                placeholder_count(zh_v),
                "placeholder count mismatch for key '{k}'"
            );
        }
    }

    #[test]
    fn tr_never_panics_on_missing_key() {
        assert_eq!(tr(Lang::En, "does.not.exist"), "does.not.exist");
        assert_eq!(tr(Lang::ZhTw, "does.not.exist"), "does.not.exist");
    }

    #[test]
    fn tr_args_never_panics_on_missing_key() {
        assert_eq!(
            tr_args(Lang::En, "does.not.exist", &["a", "b"]),
            "does.not.exist"
        );
    }

    #[test]
    fn tr_falls_back_to_en_for_missing_zh_tw_entry() {
        // A key present in en but not (yet) in zh-TW must resolve to the en text.
        assert_eq!(
            tr(Lang::ZhTw, "core.delete.error"),
            tr(Lang::En, "core.delete.error")
        );
    }

    #[test]
    fn tr_args_substitutes_positional_placeholders() {
        assert_eq!(
            tr_args(Lang::En, "core.kind-switch.converted", &["table"]),
            "converted to table"
        );
    }

    #[test]
    fn zh_tw_has_at_least_one_real_entry() {
        assert!(!zh_tw_catalog().is_empty());
    }
}
