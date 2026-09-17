# Scroll-position preservation across mode switches
Status: Resolved (2026-09-16)

Measured 2026-09-16. Method: `python3 -m http.server` over `web/dist`, real headless Chromium.
Desktop entry `index.html` at 900x260/900x300 (forces a scrollable tree);
touch entry `touch.html?ui=touch` at 390x640 with a mobile UA.
Default sample document (10 root rows).

## Measurements

| # | Case | Host | Measured | Verdict |
|---|---|---|---|---|
| 1 | tree scroll across tree -> Raw -> tree | desktop | `treeWrap.scrollTop` 200 -> **0** the moment Raw opens; on return it lands on the cursor row (186 when cursor is row 8, 0 when cursor is root) | **BROKEN** |
| 2 | raw pane scroll across Raw -> tree -> Raw | desktop | 300 -> 300 (reads 0 while `display:none`, restored on re-show) | OK |
| 3 | Raw view -> write | desktop | 300 -> 300, caret seated at 0 | OK |
| 4 | Raw write -> Apply (dirty buffer) | desktop | 300 -> 300, exits to view | OK |
| 5 | Raw write -> Cancel (dirty buffer) | desktop | 250 -> 250 | OK |
| 6 | tree scroll across any unrelated re-render | desktop | manual scroll to 0 with cursor at row 8, press `Escape` -> snaps back to **186** (cursor anchor) | **BROKEN (re-anchor, not preserve)** |
| 7 | detail panel `#detailBody` scroll across re-render | desktop | 150 -> 150 across Collapse all / Expand all / nudge keys | OK (not exhaustive) |
| 8 | tree scroll across tree -> Raw -> tree | touch | 150 -> (reads 0 while hidden) -> **150** | OK |
| 9 | raw pane scroll across re-render and round trip | touch | 400 -> 400 | OK |

## Root causes

**Finding 1 (cases 1 + 6) — desktop web has no tree-scroll state at all.**

- The scroller is `#treeWrap`; Raw mode hides only its child `#tree`
  (`renderRawOrTree`, `web/ui.ts:578-579`, `.hidden { display:none }`,
  `web/style.css:607`). The scroller itself stays displayed, so its content
  height collapses to 0 and the browser *commits* `scrollTop = 0`. This is the
  opposite of `#rawEdit`, which is itself hidden (`position:absolute; inset:0`)
  and therefore keeps its own scroll — hence case 2 passing and case 1 failing.
- Nothing saves/restores `treeWrap.scrollTop`. The only vertical positioning is
  `renderTree`'s unconditional `cur?.scrollIntoView({ block: "nearest" })`
  (`web/render.ts:332-333`), so the viewport is re-derived from the cursor on
  **every** render. It looks stable only because `nearest` is a no-op while the
  cursor is visible.

Touch already solves both: `render()` captures and re-applies
`treePane.scrollTop` verbatim around the `innerHTML` rebuild
(`web/touch/app.ts:584-588`) and adds explicit minimal "sticky cursor"
scrolling for resolved keys only (`scrollFocusIntoView`, ~:492-510). Touch's
tree pane is also hidden as a whole in Raw, so the browser preserves it.

## Not defects

- Core holds no scroll state by design — `Intent::DetailScrollBy/SetScroll` and
  `HelpScrollBy/SetScroll` are explicit no-ops (`session/dispatch.rs:189-198`).
  Scroll is host-owned; any fix belongs in `web/ui.ts` / `web/render.ts`.
- `Edit{,Clamp}Scroll` is the inline editor's *horizontal* text window, unrelated.
- TUI has no Raw mode (no `raw` match in `crates/confy-tui/src/tui/`).

## Documentation state

`docs/reference/WEBUI.md` (~:337-343) already promises the single-element Raw
pane keeps "the scroll position and the reading position ... the same object
across the switch" — true for the Raw side (cases 2-5), silent about the tree
side. No row exists in `docs/plan/2026-09-09-open-follow-ups.md`.

## Follow-up 2026-09-16 (user report, post-fix `b31d353`)

Reported: (1) Raw view -> Edit still jumps to the head; (2) Cancel with a
*clean* buffer keeps the scroll, Cancel with a *dirty* buffer jumps to the head.

**Not reproduced** against `web/dist` at `b31d353`: headless Chromium (with
`Emulation.setFocusEmulationEnabled`), and a real windowed Google Chrome with
real wheel scrolling (`scrollTop` 1306), a real mouse click on `#btnRawEdit`,
and a dirty Cancel. All held their position.

**Hypothesis (matches the clean-vs-dirty asymmetry exactly).** Both paths seat
the caret at offset 0 while the pane is focused, and a focused caret at 0 makes
the browser scroll the pane to the head:
- `enterRawWrite` (`web/ui.ts`): `focus()` + `setSelectionRange(0, 0)`, with the
  scroll restored right after — a caret scroll deferred to the next layout
  would land *after* that restore.
- `revertRawEdit`: restores `scrollTop` and *then* calls
  `setSelectionRange(0, 0)` — the caret scroll is unambiguously last here.
  A clean buffer returns early (no value write, no caret move), which is
  exactly why clean Cancel is fine and dirty Cancel is not.

Open question: which host shows it (browser / Tauri desktop / VS Code webview),
since Chrome does not. Proposed fix is host-independent: stop seating the caret
at 0. Enter write with the caret on the first *visible* line; revert with the
pre-revert caret restored (clamped); restore scroll last and re-assert it in a
`requestAnimationFrame`.

### Confirmed 2026-09-16 — it is Firefox-only, and the hypothesis held

Harness: [`2026-09-16-scroll-preservation-audit/2026-09-16-raw-scroll-crossbrowser-check.mjs`](2026-09-16-scroll-preservation-audit/2026-09-16-raw-scroll-crossbrowser-check.mjs)
(Playwright, headed, real wheel + real clicks + real typing; run it from a dir
with `playwright` installed, against `web/dist` served on :8734).

Pre-fix build (`b31d353`), real Firefox:
`1) Edit keeps the scroll  480 -> 10  FAIL`, `2a) dirty Cancel  480 -> 10  FAIL`,
clean Cancel / Apply / tree round trip PASS. Exactly the reported pair, and the
clean-vs-dirty split confirms `revertRawEdit`'s early return as the discriminator.

Post-fix: **Firefox, Chromium, real Microsoft Edge 153, and WebKit — ALL PASS**
on all six checks (Chrome/Chromium never reproduced it, which is why the first
pass missed it: only Gecko scrolls a focused caret-at-0 back to the head).
