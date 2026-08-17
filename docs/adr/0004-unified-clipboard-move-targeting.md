---
status: proposed
---

# Unify node copy/cut/paste/move targeting across TUI, web keyboard, web mouse, and touch

## Context

`confy-core`'s `Session` already owns one shared clipboard/move pipeline —
`Clipboard{fragments, cut, sources}` → `do_paste(clipboard, target: Target{parent,
index}, on_collision, allow_upgrade)` (`session/clipboard.rs`) — used identically by
every host through the same wasm boundary. But **where a paste/move lands** (the
`Target`) is currently produced by three unrelated code paths that grew independently:

1. **TUI**: `PasteSlot{Into(Path), After(Path)}` (`session/state.rs`) — a real,
   navigable gap-cursor. When the clipboard is armed, `Session::cursor_down/up/…`
   switch from moving the tree cursor to stepping `move_paste_slot` through the
   flattened `Into`-then-`After` slot sequence (`session.rs:474-505`). Fully wired,
   fully rendered (`tui/ui.rs`'s green branch highlight / insertion line).
2. **Web/touch pointer-as-target**: `onTreeClick`/`focusRow` (`web/ui.ts`) and tap
   handling (`web/touch/app.ts`) send `SetCursor` when the clipboard is armed, which
   only ever resolves to `PasteSlot::After(cursor)` via `effective_paste_slot()`'s
   fallback. There is no way to express "paste *into* this branch" from a click or a
   tap.
3. **Web/touch drag**: `web/dnd.ts` and `web/touch/app.ts`'s `onReorderMove` each
   independently re-implement a 0.25–0.75 relative-height band test to classify a
   drop as `before`/`after`/`into`, then call `Intent::MoveSelectionTo` — a *separate*
   primitive from clipboard paste (`Session::move_selection_to`, `clipboard.rs:115-154`)
   that synthesizes a one-shot `cut:true` `Clipboard` and calls `do_paste` directly,
   bypassing `PasteSlot` entirely.

Web/touch keyboard navigation (`ArrowUp`/`ArrowDown` → `Intent::CursorDown`/`CursorUp`)
*does* already share `Session::cursor_down/up` with the TUI, so it already moves
`PasteSlot` internally — but `SessionSnapshot` never exposed a `paste_slot` field, so
no pointer-based host can render it, and no host besides the TUI has ever driven it
from anything but that shared navigation code.

Two concrete bugs trace directly to this split:

- **No "paste into" from keyboard, click, or tap** — the reported gap. Not a missing
  mechanism (the TUI's mechanism already works and is core-shared); a missing
  snapshot field, missing web/touch rendering, and a missing pointer→`PasteSlot`
  intent.
- **Moving/copying an `[[array-of-tables]]` entry into another AoT group/array loses nested
  sub-sections.** `move_nodes`'s `Target::AotEntry` arm (`model/cst_edit/move_paste.rs:919`) is
  `frags.extend(aot_entry_member_fragments(tree, &h)?)` — it always splits the whole `[[items]]`
  entry into per-member dotted-key fragments before re-inserting. When the destination is a
  table/root this is deliberate, documented, `[T/D]`-parity behavior (`CONTEXT.md`'s "Insert /
  move legality" table already records it — not a bug). But when the destination is *another*
  AoT group or array, `do_paste`/`move_nodes` conditionally **rejoins** the fragments into one
  new `[[entry]]` (`dest_packs`, `move_paste.rs:924-937`) — and that rejoin is lossy: any nested
  `[[items.sub]]` sub-section permanently flattens to a dotted key even on a same-array reorder,
  which is the concrete case a user is most likely to hit and be surprised by. The docstring two
  lines above the arm (`move_paste.rs:834`, "AoT-entry sources are deferred") is also stale — the
  code demonstrably handles it — but that wording bug is cosmetic, separate from the data-loss
  above.

A third, pre-existing fact worth recording as intentional rather than accidental:
copy/cut/paste and move are implemented **three separate times per format** —
`model/cst_edit/*` (TOML, taplo CST — the only engine with dotted-table/AoT
concepts and the D1–D5 adaptation matrix from
`docs/superpowers/plans/2026-06-09-cross-layer-ops-and-line-paste.md`),
`model/json/edit.rs` (its own `adapt_fragment`), and `model/yaml/edit/mutations.rs`
(its own `move_nodes`, gated by an `entry_has_opaque_value` check unique to YAML's
anchors/aliases/tags). These were never reconciled into one written contract, so
"does this differ per format" had no answer besides reading three files.

## Decision

Adopt `PasteSlot` as the single canonical target representation for every surface. Fix the
AoT-entry sub-section data loss on AoT/array-destination moves as part of the same change,
since it's the concrete manifestation of the split this ADR closes — but leave the deliberate,
`[T/D]`-parity table/root-destination flattening alone. Node-kind and per-format move mechanics
stay owned by `CONTEXT.md`'s "Insert / move legality" table and `BEHAVIOR_MATRIX.md` (already
the maintained, code-cross-referenced source for that); this document only records what changes
in them, not a second copy of the whole matrix.

### 1. Core: `PasteSlot` becomes the one target representation

- `SessionSnapshot` gains `paste_slot: Option<PasteSlot>` — purely additive,
  surfaces the existing `effective_paste_slot()` value so any host can render it.
- New `Intent::SetPasteSlot(PasteSlot)` — pointer hosts call this instead of
  `SetCursor` while the clipboard is armed.
- New `Session::pointer_slot(path: &Path, rel_y: f32) -> Option<PasteSlot>` — the
  one place that turns "this row, this relative vertical position" into a
  `PasteSlot`. It has access to exactly what the decision needs (`is_branch`,
  `format != Format::Inline` to withhold `Into` on single-line containers) since
  that's tree state core already owns. `web/dnd.ts` and `web/touch/app.ts` stop
  each independently hand-rolling the 0.25/0.75 threshold and call this instead.
- `Session::move_selection_to` gains a `cut: bool` parameter (today it's hardcoded
  `cut: true`). A drag-drop with the copy modifier held calls it with `cut: false`;
  a plain drag-drop keeps `cut: true`. This makes "drag with a modifier" and
  "Copy → position → Paste" the same underlying primitive
  (`Target` + `cut` → `do_paste`) instead of two parallel code paths that happen to
  both end up in `do_paste`.

### 2. Surface bindings

| Surface | Source selection | Setting the target | Into supported | Copy-via-drag |
|---|---|---|---|---|
| TUI | cursor + Shift-range/`s` | Arrow keys step `PasteSlot` while clipboard armed (unchanged) | ✅ (unchanged) | n/a (no drag) |
| Web keyboard | click / Shift / ⌘ | Arrow keys already move `PasteSlot` (shared core) — now also rendered | ✅ (new: rendering) | n/a |
| Web mouse | click / marquee | Hover/click while armed calls `pointer_slot` → `SetPasteSlot`, replacing bare `SetCursor` | ✅ (new) | ✅ ⌥/Ctrl held during drag-drop → `cut:false` |
| Touch | tap | Tap while armed calls `pointer_slot` → `SetPasteSlot`, same as mouse | ✅ (new) | ❌ **explicit non-goal** — no modifier key exists on touch; drag stays move-only, copy stays the two-step Copy → tap target → Paste flow |

Web/touch rendering gains an `Into`/`After` visual treatment mirroring the TUI's
green branch highlight / insertion line (currently `render.ts` has no equivalent —
paste-armed rows only get the generic `.paste-mode` class).

### 3. `[[array-of-tables]]` entry → AoT/array destination is atomic; table/root is unchanged

`move_nodes`'s `Target::AotEntry` handling still flattens to per-member fragments before
re-inserting (unchanged — this is the `[T/D]`-parity path, kept as-is for a table/root
destination). What changes: when `dest_packs` rejoins those fragments into one new `[[entry]]`
(`move_paste.rs:924-937`, destination is another `ArrayOfTables`/`Array`), the rejoin stops
re-flattening nested `[[items.sub]]` sub-sections into dotted keys and instead reconstructs the
entry's original section structure — a true atomic move for the same-shape case. The stale
"deferred" docstring (`move_paste.rs:834`) is corrected to describe actual behavior either way.

### 4. Node-kind contract: `CONTEXT.md` is the source, this is the delta

"What does X do to node kind Y" is `CONTEXT.md`'s "Insert / move legality" table (and
`BEHAVIOR_MATRIX.md` for the fuller cross-backend account) — not re-derived here. The only row
this ADR changes is `[[array-of-tables]]` entry → `ArrayOfTables`/`Array` destination (§3, above);
every other cell in that table is unchanged by this decision. Implementation must update
`CONTEXT.md`'s `[A/T] group` row and the "Mutation mechanics" → **Move** row's AoT-entry sentence
to match once §3 ships, in the same change — not as a follow-up — so the glossary never drifts
from code the way the stale docstring did.

### 5. Format-specific behavior: also `CONTEXT.md`'s job

TOML's D5 partition/dotted-table/AoT rules, JSON's simplified `adapt_fragment`, and YAML's
`entry_has_opaque_value` move guard are already covered by `CONTEXT.md`'s "Nested behavior
matrix" and `BEHAVIOR_MATRIX.md`. None of them change under this ADR — the three engines stay
three engines, per genuine TOML/JSON/YAML grammar differences, not implementation drift.

### 6. Error/legality UX stays one vocabulary

`MutateError::Collision` / `Illegal` / the D5 partition rejection already route
through the same `status`/`error` i18n keys (`core.paste.collision`,
`core.paste.error`, …) regardless of host — this ADR doesn't change that, just
confirms it's the existing, correct pattern and that new failure modes (e.g. an
illegal `Into` chosen via the new pointer path) must reuse it rather than invent
per-surface wording.

## Consequences

- Additive wasm/wire surface: one `SessionSnapshot` field, one `Intent` variant,
  one new `Session` method. No existing `Intent` or snapshot field changes shape.
- `move_selection_to`'s signature changes (`cut: bool` added) — internal to core
  plus its two existing web/touch call sites; not part of the public `Intent` enum
  (`MoveSelectionTo` intent can carry the modifier flag as a new optional field).
- `web/dnd.ts` and `web/touch/app.ts` drop their independent band-threshold copies
  in favor of `pointer_slot`; `render.ts`/touch render gain Into/After styling.
- Touch permanently has no drag-copy — copying on touch is always the two-step
  Copy → tap target → Paste flow. This is a recorded scope boundary, not a gap to
  close later.
- `CONTEXT.md` gains **PasteSlot** / **Into** / **After** as formal glossary terms now (they're
  real, shipped `session/state.rs` concepts this ADR promotes to cross-platform vocabulary —
  not aspirational). `CONTEXT.md`'s "Insert / move legality" table and "Mutation mechanics" →
  Move row stay as-is until §3 ships, then update in the same change (§4).
- This document is the design; it is not yet implemented. An implementation plan
  (via `writing-plans`) should phase it: (1) core primitive + snapshot field +
  `SetPasteSlot` intent, (2) AoT atomic-move fix (§3) + `CONTEXT.md` sync (§4), (3) web/touch
  pointer wiring + rendering, (4) drag copy-modifier, each phase independently testable.
- Three bugs found while grilling this ADR were **fixed** in the same debugging pass, not gating
  this ADR's implementation (all three have headless regression tests; two are confirmed against
  the real TUI binary): a `tree_nav.rs` panic on any insert appending a member to a `[T/D]` table
  whose existing member's value has 2+ levels of nested inline tables; `do_paste` leaving `self.tree`
  stale after a partial multi-fragment paste failure; and a compound bug where `do_paste` never
  expanded a collapsed `Into` target (so the pasted node was invisible) and rename never remapped
  `self.selection` (so the next copy/delete silently targeted a stale pre-rename path). Full detail
  in `docs/superpowers/audits/2026-08-16-clipboard-paste-bugs.md`. A fourth item logged during the
  same grilling session — an "unconfirmed YAML add-entry failure" — was never reproduced or
  root-caused in the follow-up and is dropped here as unsubstantiated; re-open only if it recurs
  with a concrete repro.
- The same pass also fixed `do_paste` auto-selecting its own freshly-pasted/moved node(s)
  (`self.selection` now clears on every paste/move, leaving only the cursor on the result) —
  already applied uniformly to keyboard paste and mouse drag-reorder. This is the behavior any
  `pointer_slot`/`SetPasteSlot` implementation of this ADR must preserve: no per-surface
  divergence in what a paste leaves selected.

## Considered options

- **Per-surface incremental patches** (add Web keyboard rendering, add mouse/touch
  Into targeting, fix AoT, add drag-copy — each in isolation) — rejected: keeps
  three independently-maintained targeting implementations, which is how the AoT
  docstring/code drift happened in the first place; doesn't address the explicit
  request for a root-level, consistent fix.
- **Move pointer-band classification into a per-host shared TS module instead of
  core** — rejected: the classification needs `is_branch`/`Format` facts core
  already computes from the tree; duplicating that lookup client-side re-creates
  the exact drift risk (a future core-only format like a new "single-line" kind
  would need updating in three TS files, not one Rust function).
- **Touch drag-copy via a long-press/two-finger gesture** — rejected: touch keeps
  drag as move-only; copy stays the explicit two-step flow.
