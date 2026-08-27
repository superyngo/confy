✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# Row-State Visual Language (Phase 4) — Implementation Plan

**For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
(recommended) or superpowers:executing-plans to implement this task-by-task. Steps use
checkbox (`- [ ]`) syntax for tracking, so this is not a formality — re-read
`ROW_STATE_MODEL.md` §6a and tick them off as this plan's completion is verified.

**Goal:** Desktop-only client-side paste-target hover preview while the clipboard is
armed — hovering a candidate row paints the same `.drag-over-into`/`#dropLine` cue the
committed `paste_slot` already uses, computed live from `session.pointerSlot()`, with no
`dispatch`/no re-render. Click-to-commit and the separate Paste action are unchanged.

## Context

`ROW_STATE_MODEL.md` §6a (spec) and §8 (task list, "Phase 4 — Desktop hover preview").
Single surface (desktop), single file (`web/ui.ts` + its spec test); no TUI/touch/core
changes. Phases 1–3 are merged to `main` (`ADR 0005`).

### Existing mechanism this reuses (do not duplicate)

- `renderPasteSlotCue(snap)` (`web/ui.ts:311-339`) paints the *committed*
  `snap.paste_slot`: `Into` → `.drag-over-into` class on the target row; `After` →
  positions `#dropLine` at the target row's bottom edge; neither → hides both cues.
  Called from `render()` (`web/ui.ts:372`, after every snapshot-driven full tree
  rebuild) and from `installDnd`'s `onDragEnd` hook (`web/ui.ts:1758-1760`, after a
  completed/aborted native drag's `clearOver()` wipe).
- `armedPasteTarget(path, ev)` (`web/ui.ts:1087-1096`) computes `relY` from the clicked
  row's `getBoundingClientRect()` and calls `session.pointerSlot(path, relY)` — the
  exact per-pixel classification logic the hover preview must reuse (same inputs, same
  WASM call), just without dispatching.
- `onTreeHover(ev)` (`web/ui.ts:1067-1078`), wired via
  `$("treeWrap").addEventListener("mouseover", onTreeHover)` (`web/ui.ts:1751`), is the
  existing precedent for a **lazy, client-only, no-dispatch** per-cell computation
  triggered by a delegated mouse listener on `treeWrap` — same idiom, different trigger
  event (schema hint only needs the cell entered once; the paste-target band depends on
  vertical position *within* the row, so the preview needs continuous `mousemove`, not
  one-shot `mouseover`).
- `dnd.ts`'s `clearOver()` (`web/dnd.ts:45-48`) is the existing precedent for
  unconditionally sweeping `.drag-over-into` off *every* row before repainting — the
  same sweep is required here since a hovered-but-not-yet-repainted row's stale
  `.drag-over-into` class would otherwise survive a `mousemove` to a different row (no
  full DOM rebuild happens between preview repaints, unlike the two existing call
  sites which only ever run right after one).

## Constraints

- No `dispatch`/`send()` call anywhere in the new code path — this is strictly a local
  DOM repaint, per §6a ("no dispatch, no re-render"). Click-to-commit
  (`armedPasteTarget` → `SetPasteSlot`) and the separate Paste action are unchanged.
- Reuse `renderPasteSlotCue`'s existing DOM elements/classes (`.drag-over-into`,
  `#dropLine`) rather than inventing a visually distinct preview style — §6a does not
  call for one, and native grip-drag (the only other consumer of these same elements)
  is already unconditionally disabled while armed (`dnd.ts:59-63`, `paste-mode` guard
  from Phase 3), so there is no visual collision to disambiguate.
- When the pointer is over a row `pointerSlot()` declines to classify (returns
  `undefined` — e.g. a self/descendant/collision target), the preview must fall back to
  showing the **actual committed** `snap.paste_slot`, not blank — clicking there
  wouldn't change the committed target either (`armedPasteTarget` falls back to
  `SetCursor`, leaving `paste_slot` untouched), so the preview must stay truthful to
  that outcome.
- Only run the pointer-slot computation while armed (`(snap.clipboard_count ?? 0) > 0`)
  — early-return otherwise so ordinary mouse movement over the tree costs nothing extra.
- Do not touch TUI, touch, or `confy-core` files — this phase is desktop-visual-only.
- Do not run project-wide formatters/linters/test suites mid-task; run them once at the
  end via the standard `npm run build`, `node web/run-tests.mjs`,
  `node touch/run-tests.mjs`, `cargo test -p confy-tui -p confy-core` verification pass.

## Task 1: Hover-preview cue + regression test

### Step 1: Write the failing test (RED)

Add a new `web/paste-hover.spec.mjs` (same `check()`-tally, no-jsdom, esbuild-extract
convention as `armed-paste.spec.mjs`/`dnd-into.spec.mjs`) that:

- Extracts `renderPasteSlotCue` and the new hover handler verbatim from `ui.ts` via
  regex + esbuild type-strip (mirroring `armed-paste.spec.mjs`'s `fnMatch` pattern).
- Static checks: the new handler computes `relY` from `getBoundingClientRect()` (same
  pattern `armedPasteTarget` uses), calls `session.pointerSlot(path, relY)`, and calls
  `renderPasteSlotCue` with the computed slot — **never** calls `send`/`dispatch`.
- `renderPasteSlotCue` itself sweeps `.drag-over-into` off every row before applying a
  new one (mirroring `dnd.ts`'s `clearOver()` sweep pattern) — add a DOM-shim test with
  two rows where hovering row A then row B leaves only B outlined (this is the
  regression `dnd-into.spec.mjs` already covers for the drag path; cover it here for
  the hover-preview path since it's a new call site with no prior full-rebuild between
  calls).
- Behavioral cases against a minimal DOM/session shim (same `rowAt`/`evOn`/`sessionStub`
  helpers as `armed-paste.spec.mjs`):
  - Hovering a row that classifies `Into` paints `.drag-over-into` on that row and hides
    `#dropLine`, without calling `send`.
  - Hovering a row that classifies `After` positions `#dropLine` at that row's bottom
    edge, without calling `send`.
  - Hovering a row where `pointerSlot` declines (`undefined`) falls back to redrawing
    the **committed** `snap.paste_slot` (test both a case where one is set and a case
    where none is set — the latter clears both cues).
  - Hovering outside any `.row` (event target has no `.row` ancestor) also falls back to
    the committed `paste_slot`.
  - Not armed (`clipboard_count` 0 or missing) is a no-op: no cue class changes, no
    `pointerSlot` call.
  - Leaving the tree (`mouseleave` on `treeWrap`) restores the committed `paste_slot`
    cue, clearing whatever the last hover preview painted.

Run it — confirm every new check reports `✗` (RED) before touching `ui.ts`.

### Step 2: Implement in `web/ui.ts`

- `renderPasteSlotCue(snap: SessionSnapshot, slotOverride?: PasteSlot)`: add the
  `.drag-over-into` sweep (`tree.querySelectorAll(".drag-over-into").forEach(el =>
  el.classList.remove("drag-over-into"))`) as the function's first statement; change
  `const slot = snap.paste_slot;` to `const slot = slotOverride ?? snap.paste_slot;`.
  Existing call sites (`render()`, `installDnd`'s `onDragEnd` hook) pass no second
  argument and are unaffected.
- Import `PasteSlot` alongside the other `./types.js` type imports (`web/ui.ts:62-72`).
- New function, placed directly after `renderPasteSlotCue` (before the schema-hover
  section it's siblings with): a doc comment explaining the reuse (cite
  `armedPasteTarget`'s relY math, `renderPasteSlotCue`'s cue elements, and the
  fallback-to-committed rule for a declined `pointerSlot`), then:
  ```ts
  function onArmedPasteHover(ev: MouseEvent) {
    if (!snap || !session || (snap.clipboard_count ?? 0) === 0) return;
    const rowEl = (ev.target as HTMLElement).closest?.(".row") as HTMLElement | null;
    let slot: PasteSlot | undefined;
    if (rowEl?.dataset.path) {
      const path = JSON.parse(rowEl.dataset.path) as Path;
      const r = rowEl.getBoundingClientRect();
      const relY = (ev.clientY - r.top) / (r.height || 1);
      slot = session.pointerSlot(path, relY);
    }
    renderPasteSlotCue(snap, slot ?? snap.paste_slot ?? undefined);
  }
  ```
- Wire it: `$("treeWrap").addEventListener("mousemove", onArmedPasteHover);` next to the
  existing `mouseover` wiring (`web/ui.ts:1751`), plus
  `$("treeWrap").addEventListener("mouseleave", () => { if (snap) renderPasteSlotCue(snap); });`
  to restore the committed cue when the pointer leaves the tree entirely (no further
  `mousemove` events will fire to do it otherwise).

### Step 3: Confirm GREEN

Re-run `paste-hover.spec.mjs` (all checks pass) and the full existing web spec suite
(`node web/run-tests.mjs`) — in particular re-confirm `armed-paste.spec.mjs`,
`dnd-into.spec.mjs`, and `dnd-copy.spec.mjs` are still green (the `renderPasteSlotCue`
signature change and new sweep must not regress the committed-cue paths those already
cover).

### Step 4: Docs sync

`ROW_STATE_MODEL.md` §8: tick the two Phase 4 checklist items. §8's `WEBUI.md` docs-sync
row: update `WEBUI.md`'s pointer-selection/paste-mode section with a short paragraph on
the hover-preview cue (client-only, no dispatch, falls back to the committed target when
`pointerSlot` declines or the pointer leaves the tree) and tick that checklist item.

### Step 5: Changelog

Add a `CHANGELOG.md` `### Added` entry under `[Unreleased]` (this is new behavior, not a
fix): desktop now previews the armed paste target under the pointer before commit,
citing ADR 0005 §6a / `ROW_STATE_MODEL.md` §6a.

## Final integration check

- [ ] Run `npm run build` (`web/`) then `node web/run-tests.mjs` — full web spec suite
      green, no regressions in the pre-existing paste-slot/dnd specs.
- [ ] Run `node touch/run-tests.mjs` — untouched surface, confirm still green (no
      accidental cross-contamination).
- [ ] Run `cargo test -p confy-tui -p confy-core` — untouched surfaces, confirm still
      green.
- [ ] Re-read `ROW_STATE_MODEL.md` §8 Phase 4 items and tick them to match this plan's
      completion.
