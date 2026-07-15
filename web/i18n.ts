// Web/touch translation catalog — the TypeScript twin of
// `crates/confy-core/src/session/i18n.rs`. Reads the SAME repo-root JSON
// files (`i18n/en.json` / `i18n/zh-TW.json`) esbuild bundles natively as JS
// modules (`resolveJsonModule` in tsconfig.json), so `core.*`/`tui.*`/`web.*`
// keys share one source of truth across Rust and JS. `en` is canonical:
// `t`/`tArgs` fall back to the `en` entry, then to the raw key, mirroring
// `tr`/`tr_args` in core exactly.
import en from "../i18n/en.json";
import zhTw from "../i18n/zh-TW.json";

export type Lang = "en" | "zh-TW";

const CATALOGS: Record<Lang, Record<string, string>> = {
  en: en as Record<string, string>,
  "zh-TW": zhTw as Record<string, string>,
};

// Display name for each language, for the language picker. Keyed the same as
// `getLang`/`setLang`/`SetLang`; add a language later by adding one entry
// here (and one catalog file) rather than touching the picker UI.
export const LANG_DISPLAY_NAMES: Record<Lang, string> = {
  en: "English",
  "zh-TW": "繁體中文",
};

// The languages offered by the picker, in display order.
export function availableLangs(): Lang[] {
  return Object.keys(LANG_DISPLAY_NAMES) as Lang[];
}

const STORAGE_KEY = "confy-lang";

let currentLang: Lang | null = null;

// First-run default (no localStorage value yet): `navigator.language`
// `zh*` prefix -> "zh-TW", else "en".
function detectDefaultLang(): Lang {
  const nl = typeof navigator !== "undefined" ? navigator.language : "en";
  return nl?.toLowerCase().startsWith("zh") ? "zh-TW" : "en";
}

export function getLang(): Lang {
  if (currentLang) return currentLang;
  let stored: string | null = null;
  try {
    stored = localStorage.getItem(STORAGE_KEY);
  } catch {
    // storage blocked (sandboxed webview) — detect from navigator instead
  }
  currentLang = stored === "zh-TW" || stored === "en" ? stored : detectDefaultLang();
  return currentLang;
}

export function setLang(lang: Lang): void {
  currentLang = lang;
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    // storage blocked — the in-memory choice still holds for this session
  }
}

// Look up `key` in the active language, falling back to `en`, then the raw
// key string. Never throws.
export function t(key: string): string {
  const lang = getLang();
  const v = CATALOGS[lang][key];
  if (v !== undefined) return v;
  const enV = CATALOGS.en[key];
  if (enV !== undefined) return enV;
  return key;
}

// Same lookup as `t`, substituting positional `{0}`, `{1}`, … placeholders
// with `args` in order (mirrors core's `tr_args`).
export function tArgs(key: string, args: string[]): string {
  const template = t(key);
  return template.replace(/\{(\d+)\}/g, (m, idx) => {
    const i = Number(idx);
    return i < args.length ? args[i] : m;
  });
}

// Sweep the DOM for `data-i18n="key"` (textContent), `data-i18n-title="key"`
// (title attribute), and `data-i18n-placeholder="key"` (placeholder
// attribute), applying `t(key)`. Call once on boot and again after every
// language change — snapshot-driven strings refresh via the dispatch
// round-trip, but these static labels don't come from a snapshot.
export function applyStaticI18n(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    const key = el.dataset.i18n;
    if (key) el.textContent = t(key);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((el) => {
    const key = el.dataset.i18nTitle;
    if (key) el.title = t(key);
  });
  root
    .querySelectorAll<HTMLInputElement>("[data-i18n-placeholder]")
    .forEach((el) => {
      const key = el.dataset.i18nPlaceholder;
      if (key) el.placeholder = t(key);
    });
}
