// Fuzzy-match highlighting for the tree's KEY and VALUE cells — the web mirror
// of the TUI's `highlight_spans` (crates/confy-tui/src/tui/ui.rs).
//
// Both sides run the *same* matcher: `fuzzy_indices` is exported from the wasm
// core (crates/confy-ffi/src/lib.rs), so the marks drawn here are exactly the
// positions the TUI reverse-videos — no reimplemented scoring to drift apart.
//
// The matcher is *registered* by `confy.ts`'s `load()` rather than imported
// here, so `render.ts` stays free of the wasm module and the plain-Node spec
// harness can keep bundling it with esbuild. Before registration the matcher
// matches nothing, which degrades to the pre-highlight rendering (plain escaped
// text) instead of throwing.
import { escapeHtml } from "./escape.js";

/** `fuzzy_indices` from the wasm core: char offsets, or undefined for no match. */
export type FuzzyMatcher = (
  haystack: string,
  needle: string,
) => number[] | Uint32Array | undefined;

let matcher: FuzzyMatcher = () => undefined;

export function setFuzzyMatcher(m: FuzzyMatcher): void {
  matcher = m;
}

/**
 * Escaped HTML for `text`, with every char the fuzzy `needle` matched wrapped in
 * `<mark class="fz">`. Consecutive matched chars coalesce into one `<mark>`
 * (same as the TUI coalescing same-style spans), so a fully-matched cell is one
 * element, not one per letter.
 *
 * Run this against the *cell's own text*, never the filter haystack
 * (`session::search::haystack` joins path + value + comment): the marks must line
 * up with what's on screen. A row matched via its path therefore shows no marks
 * in its VALUE cell — honest, and identical to the TUI.
 *
 * Indices are *char* offsets, so the text is walked via `Array.from`; indexing
 * by UTF-16 code unit would misplace every mark after an astral char (emoji,
 * rare CJK). Escaping happens per run, so a `<` inside the matched range is
 * still encoded — the `<mark>` tags are the only raw markup produced.
 */
export function highlightHtml(text: string, needle: string): string {
  if (!needle) return escapeHtml(text);
  const idx = matcher(text, needle);
  if (!idx || idx.length === 0) return escapeHtml(text);
  const matched = new Set<number>(Array.from(idx as ArrayLike<number>));
  const chars = Array.from(text);
  let out = "";
  let buf = "";
  let bufHl = false;
  const flush = (): void => {
    if (!buf) return;
    out += bufHl ? `<mark class="fz">${escapeHtml(buf)}</mark>` : escapeHtml(buf);
    buf = "";
  };
  for (let i = 0; i < chars.length; i++) {
    const isHl = matched.has(i);
    if (isHl !== bufHl) {
      flush();
      bufHl = isHl;
    }
    buf += chars[i];
  }
  flush();
  return out;
}
