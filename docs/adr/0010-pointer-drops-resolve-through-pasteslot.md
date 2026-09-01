# Pointer drops resolve through `PasteSlot` end to end, and inline containers keep their `Into` band

Status: accepted and implemented (2026-09-01)
Supersedes: [ADR 0004](0004-unified-clipboard-move-targeting.md) §1's `format != Format::Inline`
clause, and completes §1's "`web/dnd.ts` and `web/touch/app.ts` stop each independently hand-rolling
the … threshold" for the *whole* destination rather than only its into/not-into half.

## Context

ADR 0004 made `PasteSlot{Into(Path), After(Path)}` the single canonical target representation and
introduced `Session::pointer_slot(path, rel_y)` as the one classifier every pointer surface calls.
Three of the four surfaces ended up on it:

| Surface | Destination decided by |
|---|---|
| TUI keyboard | `move_paste_slot` → `slot_target` |
| Web/touch keyboard | the same core code (arrows → `CursorUp`/`CursorDown`) |
| Web/touch **armed-clipboard** click/tap/hover/body-drag | `pointer_slot` → `SetPasteSlot` → `Paste` → `slot_target` |
| Web mouse **grip drag** (`dnd.ts`) / touch **grip reorder** (`touch/app.ts`) | `pointer_slot` for `Into`-eligibility only; `parentOf(path)` + `siblingIndex(rows, path) ± 1` with a local `rel < 0.5` split for everything else, sent as `MoveSelectionTo { target, index }` — never through `slot_target` |

The drag path's hand-rolled half disagreed with core on three separate axes. Measured against the
real wasm core (`web/pkg`) on `a = 1 / [b] c = 2 d = 3 / [e] f = 4`, fully expanded, dragging `a`:

1. **Level.** `resolve_target` (`session/insertion.rs`) lands `After(<expanded branch>)` as that
   branch's **first child**, not as a sibling one level up. The drag read the same gap as
   "root-level sibling after `[b]`". Hovering `[b]`'s bottom band therefore sent
   `target: [], index: 2`, and because a bare TOML key cannot legally follow a `[table]` header at
   root, core rejected it: notice `paste error: a key here would be captured by the table above it`,
   document byte-identical, nothing moved. An armed cut+paste released at the *same pixel* correctly
   produced `[b] / a = 1 / c = 2 / d = 3`. The user-visible symptom was "the gap between an expanded
   branch and its first child inserts at the branch's own sibling level, which is deeply
   counter-intuitive" — and in TOML it usually just failed.
2. **Level, again, upward.** Hovering `[e]`'s top band: core says `After(b.d)` (inside `[b]`, after
   its last child — the flattened predecessor slot); the drag said "root, before `[e]`". Two adjacent
   hover zones expressing two different *levels* for one visual gap, with no design covering which
   wins — the "間隙定位有層次差異，但設計不完善" report.
3. **Threshold.** Core's leaf before/after boundary is `0.75` (below it, the *preceding* flattened
   slot); the drag used `0.5`. Every `rel ∈ [0.5, 0.75)` band classified the two gestures opposite
   ways, so the armed-paste dashed line and the drag line drew on opposite edges of the same row.

A fourth, separate mismatch sat in `pointer_slot` itself: `into_eligible = is_branch() && format !=
Format::Inline`. `paste_slots()` emits `Into` for **every** branch, so the keyboard could always aim
at a single-line container, and core accepts it (`t = { x = 1 }` + `Into(t)` → `t = { x = 1, k = 9 }`,
no error). The pointer was the only surface that could not express a legal, keyboard-reachable,
core-supported target — for TOML inline tables/arrays and, since YAML flow collections reuse
`Format::Inline`, for YAML flow maps/sequences too. The guard was also self-inconsistent: a
*collapsed* multi-line `[table]` — equally one row with equally invisible children — kept its `Into`
band.

Finally, the *visual* half: the TUI indents its green insertion line one level deeper for exactly
the `After(<expanded branch>)` case (`paste_line_row`'s `row.depth + 1`). The web `#dropLine` /
`#pasteTargetLine` and the touch `.reorder-line` used the hovered row's own indent unconditionally
(touch's line had no horizontal position at all), so even the *keyboard-driven* cue pointed a level
too shallow at the one gap where the level is ambiguous.

## Decision

**One destination pipeline for every surface: the pointer produces a `PasteSlot`, core resolves it.**

### 1. `Intent::MoveSelectionTo` carries a `PasteSlot`, not a parent/index

```rust
MoveSelectionTo { sources: Vec<Path>, slot: PasteSlot, #[serde(default)] cut: bool }
```

`Session::move_selection_to(sources, slot, cut)` resolves it with `self.slot_target(slot)` — the same
call `paste()` makes — then proceeds into the existing one-shot cut/copy→`do_paste` body unchanged.
A slot whose row is no longer visible is ignored silently, mirroring `set_paste_slot`/`set_cursor`'s
guard; the self-drop / self-subtree rejection now tests the *resolved* parent.

This deliberately replaces the old payload rather than adding a second variant: two parallel
targeting primitives is precisely the condition ADR 0004 set out to remove, and the intent's only
callers are the two in-repo hosts, versioned together with core.

**Consequence, accepted:** the pointer loses the one target its hand-rolled math could express that
the slot model cannot — "root level, after an expanded branch's whole subtree" (e.g. between `[b]`'s
last descendant and `[e]`). That is the *same* expressiveness the TUI has always had; the gap-level
dimension was deliberately excluded when the TUI slot model was designed, to keep stepping simple.
Reaching it now means collapsing the branch first, exactly as in the TUI. Chosen over adding a level
dimension to `PasteSlot` (see Considered options).

### 2. `pointer_slot` offers `Into` on any branch

`into_eligible` is now just `is_branch()`. Mid-band (`0.25 < rel_y < 0.75`) on any container —
including `Format::Inline` — is `Into`; `rel_y >= 0.75` is `After`; below that, the preceding slot in
`paste_slots()`'s flattened order. The now-redundant `(is_branch && rel_y > 0.25)` fallback clause is
gone. Pointer and keyboard therefore reach exactly the same slot set.

### 3. One insertion-line indent rule, shared

New `web/slot-line.ts` exports `slotLineIndentPx(rowEl, rowIndentPx)`: one `--indent` step deeper
when the row is an expanded branch (`branch` + `open` classes, which both renderers already emit),
otherwise unchanged. Used by all three line-painting sites — web `#dropLine` (drag *and* armed
hover), web `#pasteTargetLine` (committed cue), touch `.reorder-line` — so every surface now says
visually what core does functionally, matching the TUI. The web drag line is additionally drawn under
**the slot's** row rather than the hovered one, since the two differ whenever the top band resolves
to a flattened predecessor.

### 4. Hosts keep no target state but the slot

`web/dnd.ts` holds a single `slot: PasteSlot | null` (no `{mode: "into"|"before"|"after"}`);
`web/touch/app.ts` holds `reSlot: PasteSlot | null` (no `reMode`/`reTarget`). `parentOf`,
`siblingIndex`, `child_count`, and the `0.5` split are gone from both drop paths. Touch's
outside-any-row fallback clamps `rel` to `0`/`1` against the nearest row and still asks
`pointer_slot`, rather than inventing a mode. An unclassifiable hover is not a drop target: nothing
is drawn and nothing is sent.

## Consequences

- Verified against the rebuilt real wasm core: for all 5 rows × 5 bands of the fixture above, a
  `MoveSelectionTo` with `pointer_slot`'s slot and an armed cut+paste at the same position now
  produce byte-identical documents and identical notices (7 mismatches before). Pinned by
  `move_selection_to_and_paste_agree_for_every_pointer_band` (`tests/session_headless.rs`), which
  fails if any surface ever re-derives a target.
- The reported bug is gone: dropping into the gap under an expanded `[b]` lands as its first child
  with no notice, instead of erroring with the document untouched
  (`move_selection_to_after_an_expanded_branch_lands_as_its_first_child`).
- Inline/flow containers are drop-into targets on mouse and touch;
  `pointer_slot_withholds_into_for_a_single_line_inline_container` is replaced by
  `pointer_slot_offers_into_for_an_inline_container` plus a paste-through test. ADR 0004 §1's
  parenthetical justification for the guard ("a single-line container has no meaningful 'insert into'
  drop zone") is withdrawn: the drop zone is the row's mid band, identical to a collapsed branch's.
- Wire-shape change (`MoveSelectionTo`), not additive. `web/types.ts`, both hosts, and every test
  carrying the intent moved in the same change; `cut`'s serde default still preserves the
  pre-ADR-0004 omit-means-move behavior.
- Drag-copy (`⌥`/`Ctrl` → `cut: false`) is untouched and still threaded from the `drop` event.
- Touch still has no drag-copy — ADR 0004's recorded scope boundary stands.

## Considered options

- **Add a level dimension to the slot model for every surface** (e.g. `PasteSlot::Before(Path)` or
  `After { path, depth }`, with the TUI stepping levels on `←`/`→` — bindings that are in fact free
  while a clipboard is armed, since `DecValue`/`IncValue` are locked out). Strictly more expressive
  and would preserve today's mouse-only reach. Rejected for now: it changes core's enum, the TUI's
  interaction model, all three hosts, and every slot test, to buy back one target the TUI has never
  offered — a much larger blast radius than the bug requires. Recorded here as the upgrade path if
  cross-level insertion is ever wanted deliberately.
- **Keep the drag's own before/after model but resolve it through core** (a mouse-only X-axis level
  choice, Workflowy-style). Rejected: it re-introduces a per-surface targeting model — the mouse
  would reach positions the keyboard cannot — which is the drift ADR 0004 exists to prevent.
- **Export `slot_target` to wasm and let hosts resolve the slot themselves.** Rejected: no drift
  (the resolution would still be core's), but it leaves two ways to spell a move across the wire and
  keeps hosts holding a derived `Target` they have no business owning.
- **Fix only the `Format::Inline` guard** (the smaller of the two reports). Rejected: the level and
  threshold mismatches are the same root cause — a host deriving what core already decides.
