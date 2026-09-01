# Web UI tree: PageUp/PageDown navigation

## Context

Core already supports paging the tree cursor via `Intent::PageUp(usize)`/`Intent::PageDown(usize)` (`crates/confy-core/src/session/session.rs:597-633`) — moves the cursor by `page_size` visible rows (clamped at the ends, not wrapping), or moves the paste slot by the same step when a clipboard is armed (`self.clipboard.is_some()` branch in both methods). The TUI wires this to `PageUp`/`PageDown` keys with `page_size = terminal_height / 2` (`crates/confy-tui/src/tui/mod.rs:561-562`, `keys::KeyAction::PageUp => app.page_up(terminal.size()?.height as usize / 2)`).

The web UI (`web/key-intent.ts`) has no `PageUp`/`PageDown` case in its normal-mode (tree) key switch — only its popups (TypeFilter, AddPicker, ActionMenu, SchemaEnum) handle these keys. Pressing PageUp/PageDown while the tree has focus currently does nothing but scroll the browser natively. This plan adds tree-level PageUp/PageDown to both web hosts (desktop `web/ui.ts`, touch `web/touch/app.ts`), computing a page-size step the same way the existing `typeFilterPageStep`/`touchTypeFilterPageStep` do (scroll-ratio against the real DOM, no hardcoded row height), halved to mirror the TUI's `height / 2` convention, and routes through the existing `navSelect`/`touchNavSelect` wrappers so clipboard/selection/scroll-follow behavior stays identical to every other nav key.

## Approach

1. **`web/key-intent.ts` — add a `tree-page` resolution kind.**
   - In the `KeyResolution` union (currently lines 24-38), add a new arm alongside `typefilter-page`:
     ```ts
     // PageUp/PageDown in the tree (normal mode): page size is DOM-derived
     // (`treePageStep`), so only direction is pure — same split as
     // `typefilter-page`. Host always calls `ev.preventDefault()`.
     | { kind: "tree-page"; dir: -1 | 1 }
     ```
   - In the normal-mode `switch (key)` block (lines 196-232), add two cases right after the `"g"/"Home"`/`"G"/"End"` cases (after line 200):
     ```ts
     case "PageUp": return { kind: "tree-page", dir: -1 };
     case "PageDown": return { kind: "tree-page", dir: 1 };
     ```
     These must stay below the `Escape`/modal-mode early returns (SchemaEnum/Help/etc. at lines 160-177) and below the `ctrl`/`rawView` guards (lines 180-183) exactly like the existing arrow/Home/End handling, so the mode-precedence order documented in the module's header comment (line 5) is unchanged — PageUp/PageDown in a popup mode is still resolved by that popup's own branch earlier in the function, never falls through to this tree case.
   - Add a new pure exported function next to `navRowCount` (after line 47), for both hosts to reuse:
     ```ts
     // PageUp/PageDown step for the tree, in visible-row units — mirrors the
     // TUI's `terminal_height / 2` convention (crates/confy-tui/src/tui/mod.rs)
     // without assuming a fixed row height: derive the on-screen row count from
     // the scroll-container ratio (same technique as `typeFilterPageStep`), then
     // halve it. `totalRows` is `snap.rows.length`; `clientH`/`scrollH` are the
     // tree scroll container's `clientHeight`/`scrollHeight`.
     export function treePageStep(totalRows: number, clientH: number, scrollH: number): number {
       if (totalRows === 0) return 1;
       const ratio = scrollH > 0 ? clientH / scrollH : 1;
       const visible = Math.max(1, Math.min(totalRows, Math.round(ratio * totalRows)));
       return Math.max(1, Math.floor(visible / 2));
     }
     ```
     No equivalent exists — `typeFilterPageStep`/`touchTypeFilterPageStep` are host-local (read `document.getElementById`), not exported, and don't halve.

2. **`web/ui.ts` — dispatch `tree-page` on desktop.**
   - In `onKey`'s switch (lines 804-834), add a case after `"typefilter-page"` (after line 817):
     ```ts
     case "tree-page": {
       ev.preventDefault();
       const step = treePageStep(snap.rows.length, $("treeWrap").clientHeight, tree.scrollHeight);
       return navSelect(result.dir < 0 ? { PageUp: step } : { PageDown: step });
     }
     ```
     Uses the existing `$("treeWrap")` (the scroll container, `web/index.html:121`) for `clientHeight` and the existing `tree` const (`web/ui.ts:109`, the `#tree` div, `web/index.html:122`) for `scrollHeight` — same two elements `pasteTargetLine`/`dropLine` positioning already reads (lines 380-384, 425-429). `navSelect` (line 1046) already `send()`s the intent and applies `drawnCursorFallback`; no new scroll code needed — `renderTree`'s existing `cur?.scrollIntoView({ block: "nearest" })` (`web/render.ts:315`) runs on every render and keeps the new cursor position visible.
   - Import `treePageStep` in the existing `import { resolveKeyIntent, ... } from "./key-intent.js"` at the top of `web/ui.ts` (find the exact existing import line via `grep -n 'from "./key-intent.js"' web/ui.ts` before editing — add `treePageStep` to that named-import list).

3. **`web/touch/app.ts` — dispatch `tree-page` on touch.**
   - In `handleKeyResult`'s switch (lines 1840-1900), add a case after `"typefilter-page"` (after line 1865):
     ```ts
     case "tree-page": {
       ev.preventDefault();
       const step = treePageStep(snap!.rows.length, treePane.clientHeight, treeEl.scrollHeight);
       return touchNavSelect(result.dir < 0 ? { PageUp: step } : { PageDown: step });
     }
     ```
     Uses the existing `treePane`/`treeEl` module-level elements (declared line 108-110, assigned during init — same elements `scrollFocusIntoView` (line ~488-491) and the render scrollTop-preservation logic (line 566-569) already read). `touchNavSelect` (line 1760) sends the intent and applies the same root-row fallback as desktop; `onKey`'s existing `scrollFocusIntoView()` call (line 1833, runs after every `handleKeyResult` return) keeps the new cursor on-screen — no new scroll code needed.
   - Import `treePageStep` in the existing `import { resolveKeyIntent, navRowCount, type KeyResolution } from "../key-intent.js"` (`web/touch/app.ts:85`) — change to `import { resolveKeyIntent, navRowCount, treePageStep, type KeyResolution } from "../key-intent.js"`.

4. **Clipboard-armed (paste-slot) behavior requires no extra code.** Core's `page_up`/`page_down` already redirect to `move_paste_slot` when `self.clipboard.is_some()` (session.rs:598-602, 616-620) — sending a plain `{PageUp: step}`/`{PageDown: step}` intent through `navSelect`/`touchNavSelect` is correct in both clipboard states; the intent shape does not change based on clipboard state (same pattern the existing `CursorUp`/`CursorDown`/`CursorHome`/`CursorEnd` sends already rely on).

5. **Help text — add PgUp/PgDn to the cursor-movement line.** `web/help-content.ts` has four literal template strings with the line `j/k or ↑/↓     move cursor` (or its zh-TW/VS Code variants): `HELP_TEXT` (line 7), `HELP_TEXT_ZH_TW` (line 29), `HELP_TEXT_VSCODE` (~line 52), `HELP_TEXT_VSCODE_ZH_TW` (~line 76). Re-read each exact line before editing (line numbers may drift). Change each to:
   - EN: `j/k or ↑/↓     move cursor · PgUp/PgDn page`
   - zh-TW: `j/k 或 ↑/↓     移動游標 · PgUp/PgDn 翻頁`
   Keep every other line in those four blocks unchanged.

## Critical files & anchors

- `web/key-intent.ts:24-38` (`KeyResolution` union) and `:196-232` (normal-mode switch) — add `tree-page` kind + PageUp/PageDown cases; `:40-47` (`navRowCount`) — add `treePageStep` right after it, exported.
- `web/ui.ts:804-834` (`onKey` switch) — add `"tree-page"` case; verify the exact `import { resolveKeyIntent, ... } from "./key-intent.js"` line before editing to append `treePageStep`.
- `web/touch/app.ts:85` (key-intent import), `:1839-1900` (`handleKeyResult` switch) — add `"tree-page"` case.
- `web/help-content.ts` — four `move cursor` lines (English ~line 7/52, zh-TW ~line 29/76); re-read exact current line numbers first.
- `web/key-intent.spec.mjs` — existing table-driven test file; add cases for the new resolution (see Verification).

## Verification

1. `cd web && npm run typecheck` — must pass (new `treePageStep` export, updated imports, new switch cases all type-check).
2. Add to `web/key-intent.spec.mjs` (follows the file's existing no-framework `check()` tally convention, e.g. the `PageUp`/`PageDown` TypeFilter cases at lines 192-193, 230-236):
   - Normal-mode `resolve(normalMode, "PageUp")` → `{ kind: "tree-page", dir: -1 }`; `"PageDown"` → `{ kind: "tree-page", dir: 1 }`.
   - `treePageStep(20, 400, 400)` (fully visible, no scroll) → `10` (half of all 20 rows visible).
   - `treePageStep(20, 100, 400)` (25% visible ⇒ 5 rows) → `2` (`floor(5/2)`).
   - `treePageStep(0, 100, 400)` → `1` (empty-tree guard).
   - `treePageStep(3, 100, 0)` (no scroll height, e.g. before first render) → `1` (`ratio` defaults to 1 ⇒ all 3 visible ⇒ `floor(3/2)=1`).
   - Confirm a modal mode (e.g. `typeFilterMode`) still resolves `"PageUp"` to `{ kind: "typefilter-page", dir: -1 }`, not `tree-page` — proves mode precedence is intact.
   Run `npm test` inside `web/` and confirm all pass, including the new cases.
3. Real-binary manual check (Bug-Fix Protocol — must pass on the actual built app, not just unit tests):
   - `cd web && ./cf-build.sh` (or the project's existing dev-serve script — confirm exact command via `cat web/package.json` scripts before running) to get a live build.
   - Open a document with more rows than fit on screen (e.g. the existing sample doc, expand-all with `9` if needed) in desktop web.
   - Press `PageDown`: cursor jumps forward roughly half the visible rows, tree scrolls to keep it in view, no page-native scroll happens (no jump disconnected from the highlighted row).
   - Press `PageDown` repeatedly to the end: cursor clamps on the last row, does not wrap to the top.
   - Press `PageUp` repeatedly to the top: clamps on the first row, does not wrap to the bottom.
   - Press `c` (copy) to arm the clipboard, then `PageUp`/`PageDown`: the *paste-slot* insertion line moves by the page step instead of the plain cursor (matches TUI's clipboard-armed `page_up`/`page_down` redirect) — visually confirm via the existing paste-slot cue line, not just the cursor highlight.
   - Repeat the PageDown/PageUp/clamp checks on touch host (resize viewport to the touch layout, or open `web/touch/index.html`'s build) with an external/Bluetooth-style keyboard event (can be simulated via devtools keyboard dispatch) to confirm `touchNavSelect` + `scrollFocusIntoView` keep the cursor visible identically.
   - With a popup open (e.g. press `f` for TypeFilter), confirm `PageUp`/`PageDown` still page *that popup*, not the tree underneath (mode precedence regression check).

## Assumptions & contingencies

- **Page-step formula**: no fixed row height exists in the web DOM (unlike the TUI's fixed-line terminal), so this plan reuses the project's existing scroll-ratio technique (`typeFilterPageStep`) rather than measuring a single row's pixel height, then halves the result to match the TUI's `height / 2` semantic. If manual verification (step 3) shows the halved scroll-ratio step feels too small/large compared to the TUI's actual half-screen jump (e.g. because `treeWrap`'s `clientHeight` includes non-row chrome), adjust only the divisor in `treePageStep` (currently `/2`) — do not change the overall ratio approach or add a hardcoded pixel-per-row constant, since row height varies with font size/zoom/indent depth and a hardcoded value would drift from what's actually on screen (the same reasoning already documented in `web/ui.ts:712-717` for `typeFilterPageStep`).
- **Desktop `#treeWrap` vs `#tree` for scrollHeight**: `treeWrap` is the scrolling container (`overflow` set in CSS) and `tree` is its scrolled content — confirmed by `pasteTargetLine`/`dropLine` positioning code (`web/ui.ts:380-384`) reading `wrap.scrollTop` against `wrap` (`treeWrap`) while measuring row rects relative to it. If `tree.scrollHeight` reads `0` or equal to `clientHeight` unexpectedly in manual testing (e.g. if the scrollable element is actually `treeWrap` itself, not `tree`), swap to `$("treeWrap").scrollHeight` instead — verify by inspecting computed `overflow-y` on both elements in devtools before finalizing if step-3 manual testing shows page size is always the full row count regardless of content length.
