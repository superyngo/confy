# Copy/cut/paste row-state integration audit — TUI / desktop / touch

✅ **Resolved — historical reference.** All findings below were addressed; see `CHANGELOG.md`. Kept for the development record, not as an open action list.

**Date:** 2026-08-19
**Scope:** every row-state facet touched by copy/cut/paste across the three hosts —
targeting, commit, post-commit highlight, and the armed-clipboard modal lock.
Builds on ADR 0004 (targeting unification) and ADR 0005 + `ROW_STATE_MODEL.md`
(cursor/selection/clipboard state model, status: implemented). This document does
not re-decide anything ADR 0004/0005 already settled — it re-verifies the current
code against those decisions, records what's changed since (edge auto-scroll,
desktop post-paste highlight — see `ROW_STATE_MODEL.md` §6c/§6d, updated in the
same change as this audit), and surfaces two new findings neither ADR anticipated.

## Method

Read every clipboard-armed-guarded code path in `crates/confy-core/src/session/`
(`clipboard.rs`, `session.rs`, `dispatch.rs`), `crates/confy-tui/src/tui/{app,ui}.rs`,
`web/ui.ts`, `web/touch/app.ts`, `web/dnd.ts`, `web/select.ts`; cross-referenced
against `git log` for every prior bug fix in this area (`27f1b50`, `e6f4965`,
`07345f7`, and the ADR 0004/0005 phase commits). Findings 1 and 2 (§4.1, §4.2.1)
were subsequently implemented after user review — see the Resolution section
at the end of this document.

## Consistency scorecard

| Facet | TUI | Desktop | Touch | Status |
|---|---|---|---|---|
| Ancestor auto-expand on paste destination | ✅ core (`clipboard.rs:383-409`) | ✅ same core path | ✅ same core path | Uniform — core-level, one code path |
| Selection clears on every paste/move | ✅ `clipboard.rs:411` | ✅ same | ✅ same | Uniform — the `e6f4965`/`27f1b50` fix |
| Cursor lands on first pasted/moved node | ✅ `clipboard.rs:419-420` | ✅ same | ✅ same | Uniform |
| Target cue outranks plain cursor/hover fill while armed | ✅ (`active_slot.is_some() => base`, predates the fix) | ✅ (`07345f7`) | ✅ (`07345f7`) | Uniform, now formalized as `ROW_STATE_MODEL.md` §3a |
| Modal lock: move/reorder disabled while armed | ✅ (n/a, no drag) | ✅ `dnd.ts:60` (`paste-mode` class check) | ✅ `app.ts:1054`/`1238` (`clipboard_count` check) | Uniform, two different guard mechanisms, both correct |
| Modal lock: marquee/box-select disabled while armed | n/a | ❌ **no guard anywhere** | n/a (no touch marquee gesture) | **Gap — §4.1 below** |
| Modal lock: context menu/kind-switch/convert/inline-edit | ✅ | ✅ (exhaustively grepped, see §4.1) | ✅ (exhaustively grepped) | Uniform |
| Edge auto-scroll for an off-screen drag target | n/a (keyboard-only, viewport follows cursor) | n/a (native HTML5 DnD scrolls for free) | ✅ shared RAF loop, both drags | Uniform *outcome*, three different mechanisms — deliberate, see `ROW_STATE_MODEL.md` §6c |
| Post-paste re-highlight of the landed batch | ❌ none | ✅ client-side, ephemeral (`ui.ts` `send()`) | ❌ none | **Asymmetric — §4.2 below** |

Every row above except the last two was already correct before this session; the
two "implemented"-with-caveats rows (edge auto-scroll, post-paste highlight) shipped
earlier in this session and are now formalized in `ROW_STATE_MODEL.md` §6c/§6d. The
two ❌ rows are this audit's actual findings.

## §4.1 — Finding: desktop's marquee has no clipboard-armed guard

`installMarquee` (`web/ui.ts:1914-1975`) is the only mutating/selecting gesture on
any host with **zero** `clipboard_count`/`paste-mode` check — every other guarded
affordance was grepped and confirmed present: `uiUndo`/`uiRedo`, `attachSchema`,
add/menu/kind-switch/inline-edit click routing, `openKindMenuAt`, the type-filter
toggle, `onTreeContext` (context menu), and `dnd.ts`'s `dragstart` (checks
`document.body.classList.contains("paste-mode")`). `ROW_STATE_MODEL.md` §5's own
contract — "every function except `ToggleExpand` is disabled" while armed — implies
marquee should be in that set; it was never explicitly enumerated when §5 was
written and nobody has since audited the full affordance list against it until now.

**Consequence, traced precisely:** `Session::set_selection` (`session.rs:1251-1254`)
silently no-ops while `clipboard.is_some()` — no toast, unlike the guarded pattern
used everywhere else. A plain armed click (no real drag) is unaffected, since it
never reaches the marquee's `mousemove`/`mouseup` path at all (`moved` stays
`false` under the 4px tolerance, so native `click` fires and `onTreeClick` routes
it to `armedPasteTarget()` → `SetPasteSlot` as designed). But **a click involving
more than 4px of incidental pointer movement while armed** (a common trackpad/mouse
jitter, not a deliberate drag) crosses the marquee's `moved` threshold, which then:
sets `suppressClick = true` (eating the subsequent `click` event `onTreeClick` would
otherwise have handled), draws and un-draws the rubber-band box, and fires a
`SetSelection` that core silently discards. Net effect: the click's intended
`SetPasteSlot` never fires, with **no user-visible feedback of any kind** — not a
toast (marquee's `SetSelection` path doesn't use the toast convention), not a
missing target-cue update (the hover-preview cue, `onArmedPasteHover`, already
tracks the pointer independently and looks correct throughout, then gets
authoritatively resynced by `render()`'s `renderPasteSlotCue(snap)` after the no-op
dispatch — so nothing looks obviously "wrong," the click just silently didn't land).

No data loss (nothing commits), no stale state (the resync is real), and the DOM
never shows an inconsistent target — this is a UX paper-cut, not a correctness bug.
It reproduces only on a jittery click while armed, which is why it wasn't caught by
the existing `web/*.spec.mjs` suite (all synthetic, no real pointer jitter) or by
manual smoke-testing (which drives precise programmatic clicks).

**Recommendation:** gate `installMarquee`'s `mousedown` handler on
`document.body.classList.contains("paste-mode")` (or an equivalent
`clipboard_count` check plumbed to `installMarquee`), mirroring `dnd.ts`'s existing
`paste-mode` class check exactly — same mechanism, same file area, one extra
early-return. Low risk, small diff, closes the last unguarded affordance against
§5's own stated contract. No behavior change while unarmed.

## §4.2 — Finding: post-paste highlight is desktop-only; touch can safely match it, TUI cannot

Traced in detail in the newly-added `ROW_STATE_MODEL.md` §6d (not duplicated here).
Summary of the two hosts' actual asymmetry, since it is *not* symmetric and
shouldn't be treated as one undifferentiated gap:

- **Touch** already collapses `Selection` to a single path on every tap
  (`selectOnly()`, `web/touch/app.ts:501-505`) — structurally identical to
  desktop's `navSelect`/`onTreeClick` self-clearing guarantee that makes the
  desktop compensator safe. Porting the same `send()`-level compensator to touch's
  own `send()` (`web/touch/app.ts:122-126`) is a same-shape, same-risk change.
- **TUI** cannot get the literal same compensator safely. `cursor_down`/`cursor_up`
  (`session.rs:299-332`) never touch `Selection` — a TUI Locked selection is
  designed to *persist* across arrow-key navigation until the user explicitly
  toggles it off, which is how TUI's own `s`-then-`x`/`c` multi-row workflow
  works. Calling the desktop-style compensator from TUI's `paste()`
  (`crates/confy-tui/src/tui/app.rs:651-654`) would leave a real, persistent,
  core-level `Selection` on the pasted batch with no code path that ever clears it
  on plain nav — reintroducing the exact shape of the `e6f4965`/`27f1b50` bug this
  session's own desktop feature explicitly designed around avoiding. Any TUI
  parity would need a TUI-native mechanism that doesn't reuse the real `Selection`
  field (e.g. a host-local, N-frame flash independent of core state) — a
  materially different and larger change, not a "just port it" gap.

**Recommendation:** two independent decisions, not one:

1. Extend the post-paste highlight to touch (low risk, same guarantee as desktop,
   small diff mirroring `web/ui.ts`'s existing `send()` logic in
   `web/touch/app.ts`'s `send()`).
2. TUI: recommend **leaving it as a documented, deliberate asymmetry** rather than
   building a bespoke TUI-only flash mechanism — TUI's paste already gives
   immediate, precise feedback (the cursor lands exactly on the pasted node, in a
   context the user was just keyboard-navigating), which is a weaker case for a
   *batch* highlight than desktop/touch's pointer-driven gestures where the user's
   attention was on the drag, not the destination list position. Revisit only if
   real TUI users report losing track of a multi-node paste.

## Non-findings (checked, confirmed already correct — no action)

- **Escape ladder** (`ROW_STATE_MODEL.md` §2): unchanged, one `escape()` shared by
  all hosts, asymmetry between TUI's 1-press and desktop's 2-press bare-cursor case
  is a documented, correct consequence of §1b, not a bug.
- **`SetSelection` silently no-ops while armed** (`session.rs:1251-1254`, no toast):
  confirmed intentional given how rarely it's reachable once §4.1 is fixed — every
  *other* path that could reach it while armed already redirects to
  `SetPasteSlot`/`armedPasteTarget()` before ever calling `SetSelection`. Not
  itself a finding; recorded here only because it was checked as a candidate one
  and ruled out.
- **Hover-preview cue can never go stale** — verified `render()`
  (`web/ui.ts:397`, `renderPasteSlotCue(snap)`) runs after *every* dispatch
  (`send()`), including the marquee's no-op `SetSelection` — so even the §4.1
  jittery-click case can't leave a mismatched cue on screen, only a silently
  dropped click.
- **TUI's cursor/target fill precedence** (`tui/ui.rs`'s `active_slot.is_some() =>
  base`) already matched the target-suppresses-cursor rule `07345f7` had to add to
  desktop/touch — TUI was the reference, not a laggard, on this one. Now formalized
  in `ROW_STATE_MODEL.md` §3a so it's no longer only inferable from a commit
  message.

## Recommendations summary

| # | Finding | Risk | Effort | Recommendation | Resolution |
|---|---|---|---|---|---|
| 1 | Marquee has no armed-clipboard guard (§4.1) | Low (UX paper-cut only, no data risk) | Small (one guard, one file) | Fix — closes a real gap against §5's own contract | **Fixed** — `installMarquee` mousedown now bails on `paste-mode`; regression tests in `web/modal-lock.spec.mjs` |
| 2 | Touch lacks post-paste highlight (§4.2.1) | Low (same self-clearing guarantee as desktop) | Small (port `web/ui.ts`'s pattern into `web/touch/app.ts`) | Fix if visual parity with desktop is wanted | **Fixed** — ported into `web/touch/app.ts`'s `send()`; regression tests in `web/touch-paste-select.spec.mjs` |
| 3 | TUI lacks post-paste highlight (§4.2.2) | High if done the "obvious" way (reintroduces `e6f4965`/`27f1b50`'s shape); low if left alone | N/A unless pursued | Leave as documented, deliberate asymmetry; don't port verbatim | **Left as-is**, per user decision — documented in `ROW_STATE_MODEL.md` §6d |

## Resolution (2026-08-19, same day)

User reviewed findings 1–3 and approved 1 and 2; declined 3 (documented instead).
Both approved fixes shipped in this change:

- `web/ui.ts`'s `installMarquee` now early-returns on `document.body.classList
  .contains("paste-mode")`, mirroring `dnd.ts`'s existing guard.
- `web/touch/app.ts`'s `send()` now carries the identical post-paste reselect
  compensator `web/ui.ts`'s `send()` already had.

Verified: full `node web/run-tests.mjs` (17 suites, including two new spec
additions — a behavioral block in `modal-lock.spec.mjs` and the new
`touch-paste-select.spec.mjs`) green; `cargo test -p confy-core -p confy-tui`
green (untouched by this change, confirms no collateral breakage); `tsc --noEmit`
shows only the pre-existing, unrelated `clipboard_count` nullability errors;
headless-browser smoke test confirms the marquee fix end-to-end (a real
mousedown→mousemove→mouseup→click sequence delivered across separate event-loop
turns, matching how a real pointer device dispatches, still lands the armed paste
target after the fix).
