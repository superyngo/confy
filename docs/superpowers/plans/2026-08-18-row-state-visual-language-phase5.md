# Row-State Visual Language (Phase 5) — Implementation Plan

**For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
(recommended) or superpowers:executing-plans to implement this task-by-task.
Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `ROW_STATE_MODEL.md` §8 Phase 5 — Touch drag-to-target (§6b).
The decision record is ADR 0005 (`docs/adr/0005-row-cursor-selection-clipboard-state-model.md`).
This phase only changes `web/touch/app.ts` (+ a new `web/*.spec.mjs`); no TUI,
desktop-web, or confy-core changes. Re-read `ROW_STATE_MODEL.md` §6b/§8 items
and tick them off in that file as part of this plan's completion.

## Architecture

Two independent gaps in `web/touch/app.ts`, both scoped to the armed-clipboard
case (`clipboard_count > 0`):

1. **Live target preview during a body-drag.** Today, a body-drag that moves
   past the tap-vs-scroll threshold (`Math.abs(dy) > 8` -> `moved = true`)
   does nothing paste-related on release — only a *stationary* tap goes
   through `handleTap`'s `armedTarget()` -> `SetPasteSlot`/`SetCursor`. This
   phase adds a live classify-and-repaint loop during the drag itself
   (mirroring `onReorderMove`'s existing hit-test loop, but painting the
   *paste*-target cue via `renderPasteSlotCue`'s existing `.drop-into`/
   `.reorder-line` elements, client-only, no `dispatch`) and, on release,
   commits the *set* (not a `Paste`) via `SetPasteSlot`/`SetCursor` for
   wherever the drag ended — release only sets/refines the target, matching
   desktop's separate set-then-commit flow (§6a); the FAB still performs the
   actual `Paste`.
2. **Caret disambiguation.** A pointerdown that lands on `.caret` must not
   engage the new drag-preview loop, so a stationary caret press still falls
   through unchanged to `handleTap`'s existing `act === "caret"` branch
   (`SetCursor` + `ToggleExpand`, itself already paste-target-aware) —
   mirroring the existing `.drag-handle` gate that keeps reorder-drag and tap
   mutually exclusive today.

**Reused, unchanged:** `session.pointerSlot(path, relY)` classification (ADR
0004 §1) is the single source of truth for what a given row+relY resolves to;
this phase adds no new classification logic, only a new place that calls it
(the live drag loop) and a new place that acts on it (drag-release).

## Out of scope (confirmed, not part of this phase)

- Auto-scroll on edge-drag (§6c) — recorded in `ROW_STATE_MODEL.md`, not
  scheduled.
- Any change to `session.pointerSlot`, `PasteSlot`/`Into`/`After` semantics,
  or `confy-core` — all owned by ADR 0004.
- TUI, desktop web (`web/ui.ts`), CONTEXT.md, BEHAVIOR_MATRIX.md — untouched.

## Task 1: Touch drag-to-target + regression test

### Step 1: Write the failing test first

Create `web/touch-paste-drag.spec.mjs`, following `web/touch-pointer-slot.spec.mjs`'s
established convention exactly (no test framework, `check()` tally; extract
the real function bodies verbatim from `web/touch/app.ts` via the same
regex-extraction + esbuild-type-strip wrapper pattern already used there,
supplying the module-level state they close over — `session`, `snap`,
`treeEl`, the drag-tracking variables, `send`/`renderPasteSlotCue` stubs).
Cover:

- **Wiring**: pointerdown sets `pasteDragActive` from `armed && !closest(".caret")`;
  pointermove's new branch calls `onPasteDragMove` only when
  `pasteDragActive && dragging`; pointerup's new branch fires
  `finishPasteDrag` only when `pasteDragActive && pasteDragMoved` (checked
  before the existing `!moved` tap branch, since `moved` itself is never set
  once the new pointermove branch takes over and returns early).
- **Live preview, no dispatch**: dragging across two different rows repaints
  `renderPasteSlotCue` with each row's classified slot and never calls `send`.
- **Dead zone**: movement under the 6px threshold does not yet mark
  `pasteDragMoved` or repaint.
- **Sweep**: `renderPasteSlotCue`'s new `.drop-into` sweep removes a
  *previous* row's stale class before applying the newly classified row's,
  mirroring `web/ui.ts`'s Phase 4 sweep (same collision this phase would
  otherwise reintroduce on touch, now that the function is called
  repeatedly mid-gesture instead of once per full render).
- **Release commits the set, not a paste**: ending a drag past the threshold
  sends exactly one `SetPasteSlot` (or `SetCursor` when `pointerSlot`
  declines), never `Paste`.
- **Caret bail**: a pointerdown on `.caret` leaves `pasteDragActive` false
  even while armed; a stationary press-and-release on it still reaches
  `handleTap`'s `act === "caret"` branch (both `SetCursor` and
  `ToggleExpand` fire, matching today's behavior) rather than the new
  drag-release path.
- **Regression — existing stationary-tap behavior unchanged**: a plain
  stationary tap on an armed row body still resolves through `handleTap`'s
  `armedTarget()` (not the new drag path), since `pasteDragMoved` never
  flips true without crossing the dead zone.
- **Disarmed**: with no clipboard armed, pointerdown never sets
  `pasteDragActive`, and body-drags behave exactly as before (swipe / plain
  scroll-cancel), unaffected by any of this phase's new code paths.

Run it before touching `app.ts` — it must fail (the new state/functions
don't exist yet). This is the RED step.

### Step 2: Implement

In `web/touch/app.ts`:

- [ ] Add `PasteSlot` to the `import type { ... } from "../types.js"` block
      (not yet imported in this file).
- [ ] Add module-level drag-preview state next to the existing tap/drag
      state (`sx`/`sy`/`dragRow`/`dragging`/`moved`, ~line 935): `pasteDragActive`,
      `pasteDragStartY`, `pasteDragMoved`, `pasteDragRow`.
- [ ] `renderPasteSlotCue(snap, slotOverride?: PasteSlot)`: add the
      `slotOverride` param: `const slot = slotOverride ?? snap.paste_slot;`
      replacing the current `const slot = snap.paste_slot;`. Add a
      `.drop-into` sweep at the top (`treeEl.querySelectorAll(".drop-into").forEach(...)`),
      mirroring `web/ui.ts`'s Phase 4 `renderPasteSlotCue` sweep — required
      once this function is called repeatedly mid-gesture without an
      intervening full re-render (previously it only ran once per render or
      once per `endReorder` cleanup, where at most one stale row ever
      existed). Update both existing call sites' doc comments only if they
      now read misleadingly; do not change their call arguments (they
      correctly rely on the `slotOverride` default).
- [ ] New `onPasteDragMove(y: number)`: mirrors `onReorderMove`'s hit-test
      loop (iterate visible `.row` elements, prefer a row whose rect
      contains `y`, else nearest by edge distance) but without
      `onReorderMove`'s source-subtree exclusion (irrelevant here — armed
      clipboard rows are ordinary rows, not a row mid-drag; trust
      `session.pointerSlot` the same way `onArmedPasteHover` does on
      desktop, adding no client-side filtering). Below the 6px dead zone
      (mirrors `onReorderMove`'s own `reStartY` threshold), do nothing. Past
      it, set `pasteDragMoved = true`, resolve `pasteDragRow`, compute
      `relY` from its rect, classify via `session.pointerSlot(path, relY)`,
      and repaint via `renderPasteSlotCue(snap, slot ?? snap.paste_slot ?? undefined)` —
      same fallback-to-committed behavior as desktop's `onArmedPasteHover`
      when the hit resolves to nothing classifiable.
- [ ] New `finishPasteDrag(y: number)`: using `pasteDragRow` (the last
      classified row) and the release `y`, recompute `relY` and
      `session.pointerSlot`, then `send(slot ? { SetPasteSlot: slot } : { SetCursor: path })` —
      never `Paste`.
- [ ] `pointerdown` handler: after the existing grip check, compute
      `armed = (snap?.clipboard_count ?? 0) > 0` and
      `pasteDragActive = armed && !(e.target as HTMLElement).closest(".caret")`;
      reset `pasteDragStartY = e.clientY`, `pasteDragMoved = false`,
      `pasteDragRow = null`.
- [ ] `pointermove` handler: add a branch after the existing `if (reordering)`
      check — `if (pasteDragActive && dragging) { e.preventDefault(); onPasteDragMove(e.clientY); return; }` —
      before the existing swipe/scroll tracking (which must not also run for
      this gesture).
- [ ] `pointerup` handler: insert an `else if (pasteDragActive && pasteDragMoved) { finishPasteDrag(e.clientY); }`
      branch between the existing `swiping` branch and the existing
      `dragging && dragRow && !moved` (`handleTap`) branch — ordering
      matters, since `pasteDragMoved` must win over the (still-false)
      `!moved` check once a drag has actually happened. Reset
      `pasteDragActive = false; pasteDragMoved = false; pasteDragRow = null;`
      alongside the existing `dragging`/`dragRow`/`swiping`/`swipeMain`
      resets at the end of the handler.
- [ ] `pointercancel` handler: reset the same three `pasteDrag*` fields, and
      if any preview was painted (`pasteDragMoved`), restore the committed
      cue via `if (snap) renderPasteSlotCue(snap);` — mirrors `endReorder`'s
      identical collateral-restore call.

### Step 3: Make the test pass (GREEN)

Re-run `web/touch-paste-drag.spec.mjs` until every check passes. Fix the
implementation, not the test, unless a test assertion is itself wrong (as
happened once in Phase 4 — note it if so).

### Step 4: Update ROW_STATE_MODEL.md and CHANGELOG.md

- [ ] Tick both Phase 5 checklist items in `ROW_STATE_MODEL.md` §8.
- [ ] Add a `CHANGELOG.md` `[Unreleased]` entry under `### Added` (or
      `### Fixed`, matching Phase 1-4's convention) describing the touch
      drag-to-target behavior.

### Step 5: Full verification

- [ ] `node web/run-tests.mjs` — all specs green (existing suites +
      `touch-paste-drag.spec.mjs`), no regressions in
      `touch-pointer-slot.spec.mjs`, `touch-paste-cue.spec.mjs`,
      `touch-render.spec.mjs`, `touch-clip-source.spec.mjs`,
      `touch-modal-lock.spec.mjs`.
- [ ] `cargo test -p confy-tui -p confy-core` — untouched by this phase, must
      stay green (no Rust files change).

### Step 6: Commit

One commit for this task (docs ticks + changelog may be folded into the same
commit or a trailing `docs(row-state)` commit, matching Phase 1-4's pattern).

## Final integration check

- [ ] Run `cargo test -p confy-tui -p confy-core` and `node web/run-tests.mjs`
      together one more time (be green with all three tasks' — here, one
      task's — changes present simultaneously; trivially true for a
      single-task phase, but keep the step for consistency with prior
      phases' final review gate).
- [ ] Re-read `ROW_STATE_MODEL.md` §8 Phase 5 checklist items and confirm
      both are ticked and match this plan's completion.
