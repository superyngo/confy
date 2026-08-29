---
status: implemented
---

# Formalize the row cursor/selection/clipboard-source state model and unify its interaction and visual language across TUI, desktop, and touch

## Context

`confy-core`'s `Session` already owns four orthogonal pieces of per-row state, shared by
every host through the same `dispatch(Intent)` boundary:

1. `cursor: Path` (`session.rs:23`) — the single focus point. Always exists.
2. `selection: Selection { committed, round, anchor }` (`session.rs:25`,
   `selection/selection.rs:25-30`) — a set built by `ToggleSelect`/`ExtendSelectUp`/
   `ExtendSelectDown` (TUI: `s`, Shift+Arrow) or `SetSelection` (desktop: plain/⇧/⌘-click,
   marquee). `selected_paths()` (`session.rs:1258`) falls back to a one-row selection
   from `cursor` when `Selection` is empty — this is why TUI arrow navigation never has
   to touch `Selection` at all, while a desktop plain click always does (`SetSelection([path])`).
3. `clipboard: Option<Clipboard { fragments, cut, sources }>` (`session.rs:33`,
   `state.rs:205-209`) — `sources` are the cut/copy source rows, `cut` is the only
   cut-vs-copy discriminator.
4. `paste_slot: Option<PasteSlot { Into(Path), After(Path) }>` (`session.rs:34`,
   `state.rs:200-203`) — the paste target, formalized as cross-platform vocabulary by
   ADR 0004.

There is a fifth, **unnamed** state: while `clipboard.is_some()`, four early-return
guards (`toggle_select` `session.rs:1196-1199`, `set_selection` `1208-1211`,
`extend_select_up` `1222-1225`, `extend_select_down` `1240-1243`) freeze `Selection`
against further mutation. This is a distinct, later-entered state layered on top of
"a locked `Selection` exists" — not the same thing, even though both involve the word
"selection."

Two problems trace directly to this being informal:

- **The state model was assumed asymmetric across hosts when it isn't.** The working
  assumption going into this ADR was that multi-row locked selection ("鎖定選取") is a
  TUI-only concept and that mouse/touch have no equivalent. That's wrong at the code
  level: desktop's Ctrl/Shift-click and marquee already populate the exact same
  `Selection` struct (`web/select.ts` → `SetSelection`) — only the entry gesture
  differs. Treating it as TUI-only in documentation/design would have produced a doc
  that contradicts the shipped code on day one.
- **A concrete bug shipped from exactly this gap.** While a clipboard was armed,
  tapping/clicking any branch's expand caret toggled the *clipboard source* node
  instead of the clicked one (fixed: `web/ui.ts:1161-1165`, `web/touch/app.ts:1213-1223`
  now send an explicit `SetCursor` before `ToggleExpand`). Root cause: `ToggleExpand` is
  cursor-based (`dispatch.rs:67-78`), but the armed-click path only ever sent
  `SetPasteSlot`, never moving `cursor` — a point fix that patched the one call site
  it was reported against, without a formal model to check for siblings.
- **Visual language diverged per host with no shared source.** TUI colors copy=blue/
  cut=green (`tui/ui.rs:381-385`); desktop colors copy=green/cut=purple
  (`web/style.css:566-568`, via `--t-string`/`--t-date`) — the **inverse** assignment;
  touch has no per-row cut/copy styling at all, only FAB-button color
  (`touch/style.css:525-526`, a third scheme). Desktop's keyboard cursor renders as a
  left accent bar (`web/style.css:168`) while TUI's renders as a full background fill
  (`tui/ui.rs:405-408`) — same core field (`is_cursor`), different visual language.
  `.row.cut` exists in both `web/style.css:169` and `touch/style.css:119` but is dead
  CSS — no code path ever emits class `cut`.
- **Keybindings diverge without a documented reason.** TUI: `Enter`/`Space` both fire
  `ToggleExpand` (`tui/keys.rs:56`), `i` fires `Info`/toggle-detail (`tui/keys.rs:63`).
  Desktop: `Enter` fires `ToggleExpand` (`key-intent.ts` ~L160-190, matching TUI), but
  `Space` fires `ToggleDetail` — the opposite of what it does on TUI.

This ADR formalizes the state model as it actually exists in `confy-core`, corrects the
TUI-only assumption, and decides a single cross-platform visual/interaction language
for it. Node-kind and per-format mutation mechanics are untouched and stay owned by
`CONTEXT.md`/`BEHAVIOR_MATRIX.md`, exactly as ADR 0004 established for `PasteSlot`.
Full per-state binding tables, the visual spec, and the phased implementation task list
live in `ROW_STATE_MODEL.md`; this document records the decision, not a second copy of
the matrix.

## Decision

### 1. Five formal states, layered, not parallel

Adopt this vocabulary (English canonical name — Chinese working name — core mapping):

| # | Canonical name | Chinese | Core mapping | Scope |
|---|---|---|---|---|
| 1 | **Cursor** | 提示定位 | `Session.cursor: Path` | Always exists on TUI/desktop (keyboard-driven on both). Desktop mouse **hover** is a separate, core-invisible, CSS-only signal — it never writes `cursor`. Touch has no cursor-equivalent pointer state. |
| 2 | **Focal row** | 選取 | The row `selected_paths()` treats as the target for non-multi mutating ops (edit value/key/comment, remark) — `cursor`, or the last/focal member of a non-empty `Selection` (`set_selection` keeps the focal path last, `session.rs:1208`) | All platforms; not a separate field, a derived rule. |
| 3 | **Locked selection** | 鎖定選取 | `Session.selection: Selection` non-empty | **Cross-platform**, not TUI-only. TUI: `s` / Shift+Arrow. Desktop: Ctrl/Shift+click / marquee. Touch: none (`web/touch/app.ts` `selectOnly()` only ever sets a 1-path selection; no shift/ctrl/marquee gesture exists). Applies uniformly regardless of member count (1 row from a plain click and 3 rows from a marquee use the same struct and the same visual rule — see §2). |
| 4 | **Clipboard-armed (cut/copy mode)** | 剪下複製模式 | `Session.clipboard.is_some()` | Cross-platform. A later, independently-entered state layered on top of #3: entering it freezes whatever `Selection` currently holds (the four guards above); it does not itself require #3 non-empty (`selected_paths()`'s cursor-fallback still applies to a bare cursor with no explicit `Selection`). |
| 5 | **Clipboard source** | cut/copy source | `Session.clipboard.sources: Vec<Path>`, colored by `Session.clipboard.cut: bool` | Cross-platform; only meaningful while #4 is active. |

`CONTEXT.md` gains **Cursor**, **Locked selection**, and **Clipboard-armed** as formal
glossary terms (mirroring how ADR 0004 promoted `PasteSlot`/`Into`/`After`) — these are
shipped, existing concepts this ADR names, not new mechanism.

### 2. Visual language: background-fill states are exclusive; membership states are markers only

Background fill is reserved for exactly three **mutually-exclusive** row states, unified
across TUI/desktop/touch:

- **Cursor** (`is_cursor`) → full-row background fill, one consistent color across TUI
  keyboard cursor, desktop keyboard cursor, and desktop mouse hover (hover and cursor
  render identically and may appear on two different rows at once, since hover never
  moves `cursor` — §1).
- **Cut source** → full-row background fill.
- **Copy source** → full-row background fill (distinct color from cut).

**Locked selection** and **focal-row-within-a-lock** no longer consume a background
fill (dropping TUI's solid grey `Selection` fill at `tui/ui.rs:385-386` and desktop's
`--sel` fill + inset ring at `web/style.css:167`) — replaced by a lightweight leading
marker. TUI already has this marker and needs no new code for it: `sel_marker = "●"`
is already prefixed onto the NAME cell whenever `Selection` contains the row
(`tui/ui.rs:328-333`) — it is currently just redundantly stacked underneath the grey
fill; dropping the fill is the only TUI change. Desktop/touch repurpose the existing
left-edge `::before` bar treatment (currently `.row.cursor::before`, freed up by
folding cursor into the background-fill group) as their equivalent marker, in a
neutral tone distinct from the three fill colors. The marker applies uniformly to any
non-empty `Selection`, one row or many — no count threshold.

This makes overlap well-defined instead of accidental: a row can be cursor (fill) *and*
in the locked selection (marker) at the same time, and the two read as independent
facts rather than competing backgrounds. Exact hues are left to implementation — the
requirement is internal consistency across all three surfaces, not a specific palette
(TUI blue/desktop green/touch-FAB-orange must converge on the *same* assignment
everywhere, not any particular one).

Touch gains new per-row `clip-cut`/`clip-copy` styling (it currently has none —
`touch/render.ts:57-65` never emits the classes `web/render.ts` does), and its dead
`.row.cut` class (`touch/style.css:119`, never emitted) is removed rather than
resurrected — the real class names are `clip-cut`/`clip-copy`, matching desktop.

### 3. Keybinding unification (reverses current Enter semantics)

| Key | Current TUI | Current desktop | Target (both) |
|---|---|---|---|
| `Space` | `ToggleExpand` (`tui/keys.rs:56`) | `ToggleDetail` (`key-intent.ts`) | `ToggleExpand` |
| `Enter` | `ToggleExpand` (`tui/keys.rs:56`) | `ToggleExpand` (`key-intent.ts` "toggle-branches") | `ToggleDetail` |
| `i` | `Info`/`ToggleDetail` (`tui/keys.rs:63`) | `ToggleDetail` (`key-intent.ts`) | `ToggleDetail` (unchanged, stays the alt binding) |

This is a deliberate behavior reversal on `Enter` on both TUI and desktop (not additive)
— confirmed explicitly with the design's requester rather than inferred, since it
changes existing muscle memory. Touch has no physical Enter/Space; its double-tap-to-
open-detail gesture (`web/touch/app.ts`) is conceptually aligned already and is
unaffected.

### 4. Cut/copy mode is a cross-platform modal lock

Entering state #4 disables every function key / equivalent pointer affordance except
`ToggleExpand`, on **all three surfaces**: move/reorder (including the touch
reorder-grip drag, `web/touch/app.ts:955-1069` — a move operation is itself
paste-equivalent and conflicts with being mid-target-selection), context menu, and
inline edit of value/key/comment. This was already the original ask (TUI list: "move
bar, menu, 編輯key/value/comment"); this ADR confirms it applies identically to
desktop's mouse-driven equivalents and to touch's grip-drag, not just to TUI's literal
key bindings. A disabled affordance surfaces a transient toast/status message rather
than a silent no-op, since this is new-to-users modal behavior.

**Later revision (2026-08-22, desktop/touch row-actions consolidation):** the
reorder-grip is the one exception to "toast, not silent no-op" above. Desktop's
`+`/`⋮` row-actions already went silent-hide-only (`.paste-mode .row-actions
{display:none}`, `web/style.css`) rather than toast-on-attempt, because they are
not independently reachable once hidden — any runtime guard on them is dead code
under normal pointer input. When the grip moved into desktop's `.row-actions`
(replacing the standalone `＋`, `web/render.ts`) it inherited the same hide, and
touch's grip was brought in line with the same pattern (`.app.paste-mode
.drag-handle{visibility:hidden}`, `web/touch/style.css`) — its runtime
`clipboard_count` guard (`web/touch/app.ts: startReorder`) was removed as
unreachable rather than kept as dead defensive code. Every other locked
affordance (context menu, toolbar buttons, inline edit, swipe-to-delete, …)
keeps the original toast-on-attempt behavior; only the grip's specific
lock became hide-outright, because it is the one row-level move affordance a
pointer can already never reach once its container is hidden.

### 5. Escape ladder — documented as-is, no behavior change

`Session::escape()` (`session.rs:1594-1636`) already peels exactly one layer per press,
clipboard before selection, shared by every host. The previously-assumed "TUI: 1 press
for a bare cursor / 2 presses when locked; mouse: always 2 presses" asymmetry is not a
new rule to design — it is the direct, correct consequence of §1 #3: TUI arrow
navigation never populates `Selection` (nothing for layer 2 to clear on a bare cursor),
while a desktop plain click always does (`SetSelection([path])`, one entry, always
something for layer 2 to clear). `ROW_STATE_MODEL.md` records this mechanism
precisely; no code changes under this ADR.

### 6. Cut/copy-mode target positioning gains a genuine two-stage gesture on desktop and touch

TUI's existing `PasteSlot` arrow-key stepping is unchanged. Desktop and touch currently
only *set* the target on click/tap (`web/ui.ts:1075-1096`, `web/touch/app.ts:1188-1222`)
— there is no live preview before commit-to-target. This ADR adds one, on both:

- **Desktop**: hovering a candidate row computes `pointer_slot()` and renders a
  local-only preview cue — no `dispatch`, no re-render, purely client-side, mirroring
  how `onTreeHover` (`web/ui.ts:1055-1066`) already does client-only work for schema
  tooltips. A click still calls the existing `armedPasteTarget()` → `SetPasteSlot`
  unchanged; commit is still a separate `v` / menu action, unchanged.
- **Touch**: while armed, a **row-body** drag (pointerdown/pointermove/pointerup, not
  requiring the reorder-grip) continuously repositions the target, reusing the existing
  `onReorderMove` (`web/touch/app.ts:971-1039`) live-`pointer_slot()`-and-repaint
  pattern already built for reorder-drag — the precedent for this exists verbatim in
  the repo, it is not new mechanism. Release only sets/refines the target (mirrors
  desktop: no auto-commit); the FAB still performs the actual `Paste`. A pointerdown
  starting on the caret must still resolve to `ToggleExpand` on release (per §4/§1) —
  today's caret-vs-body disambiguation only runs at tap-resolution
  (`web/touch/app.ts:1197`, `handleTap`); a pointerdown-level `closest('.caret')` bail
  is required, mirroring the existing `closest('.drag-handle')` gate
  (`web/touch/app.ts:1076`) that already makes reorder-drag and tap mutually exclusive.
  Per §4, the reorder-grip is disabled while armed, so there is no gesture ambiguity
  between "grip-drag to reorder" and "body-drag to target."

Auto-scroll-on-edge-drag for the touch case is explicitly **out of scope** for this
ADR — no such mechanism exists anywhere in the repo today (touch or desktop; desktop's
native HTML5 drag gets edge-scroll for free from the browser, which is why nobody
hand-rolled it — `web/dnd.ts` has none either), and building it collides with the
existing scroll-position-restore-on-every-render latch
(`web/touch/app.ts:424-427`), which per-pointermove `dispatch` would fight. Recorded as
a follow-up in `ROW_STATE_MODEL.md`, not part of this ADR's implementation.

## Consequences

- Purely additive at the wire level for the new hover-preview/drag-preview cues (client
  state only, no new `Intent`/snapshot field); the `Selection`/`Cursor`/`Clipboard`
  fields themselves are unchanged in shape.
- `CONTEXT.md` gains Cursor / Locked selection / Clipboard-armed as formal glossary
  terms now (shipped concepts, not aspirational).
- Every color/background rule in `tui/ui.rs`, `web/style.css`, `web/touch/style.css`
  changes (§2); this is the one part of this ADR that is a visible behavior change to
  existing users, not just documentation.
- `Enter`/`Space` semantics invert on both TUI and desktop (§3) — a deliberate,
  explicitly-confirmed breaking change to existing keybindings.
- The bug-3 fix (`SetCursor` before `ToggleExpand` while armed) becomes a named
  invariant of this model ("`ToggleExpand` always resolves against the row it was
  actually invoked on, even while `Selection` is frozen") with a regression test,
  rather than a point patch with no model backing it — see `ROW_STATE_MODEL.md`'s
  worked-example section.
- `WEBUI.md`'s "Paste mode" paragraph, which still described the pre-ADR-0004
  `SetCursor` behavior, is corrected in this change (unrelated drift found while
  writing this ADR, fixed alongside it rather than left to compound further, matching
  ADR 0004's own precedent of fixing adjacent drift it found).
- Implementation is phased (`ROW_STATE_MODEL.md`'s task list): (1) visual language —
  color/marker changes across TUI/web/touch CSS + ratatui styles, (2) keybinding
  reversal, (3) cut/copy-mode modal lock (disable list + toast), (4) desktop hover
  preview, (5) touch drag-to-target + caret pointerdown gate, each independently
  testable and shippable. Auto-scroll is deliberately not phase 6 — it is a recorded,
  unscheduled follow-up.
- **Status: implemented (2026-08-18).** All five phases shipped and merged to `main`,
  each independently reviewed: visual language (`tui/ui.rs`, `web/style.css`,
  `web/touch/style.css`), keybinding reversal (`tui/keys.rs`, `web/key-intent.ts`),
  the cut/copy-mode modal lock (`Session::apply`, `web/dnd.ts`, `web/touch/app.ts`),
  desktop hover preview (`web/ui.ts` `onArmedPasteHover`), and touch drag-to-target
  (`web/touch/app.ts` `onPasteDragMove`/`finishPasteDrag`). See `CHANGELOG.md`
  `[Unreleased]` for the itemized entries and `ROW_STATE_MODEL.md` §8 for the
  per-phase checklist, now fully ticked. Auto-scroll-on-edge-drag (§6, out of
  scope) remains an unscheduled follow-up, not a gap in this ADR's own scope.

**Later revision (2026-08-29, multi-selection remap):** the Decision §1 table and this
document's framing of *remark* as a single-focal-row (state #2) op are superseded.
Remark, like delete/copy/cut, now consumes the whole Locked selection (`selected_paths()`)
and remaps it onto the remark's post-image: in-place kind swaps track the Key↔positional
address change, adjacent-row merges collapse the selection onto the merged block, and
un-remarking a selected block expands it onto every restored row; `delete_selected` drops
paths that no longer resolve. The full op-by-op contract lives in
`ROW_STATE_MODEL.md` §1c, the SSOT for multi-selection semantics.

## Considered options

- **Leave the visual/keybinding scheme divergent per platform (status quo)** —
  rejected: this is precisely the shape of drift that produced bug 3 (a point fix with
  no formal model to check for the same class of bug at other call sites), and the
  explicit ask this ADR responds to.
- **Introduce a dedicated `LockedSelection`/`CutCopyMode` enum distinct from `Selection`
  non-empty / `clipboard.is_some()`** — rejected: both concepts already exist and fully
  determine the described behavior; a parallel flag would duplicate state without
  changing behavior, the same "don't re-derive client-side what core already computes"
  reasoning ADR 0004 used to reject a client-side band-classifier duplicate.
- **Build touch auto-scroll and auto-commit-on-drag-release in the same pass** —
  rejected: no auto-scroll precedent exists anywhere in the repo (net-new engineering,
  and it collides with the existing render-triggered scrollTop-restore latch), and
  auto-commit removes the two-step confirmation a destructive cut/paste currently gets;
  deferred/kept as the current explicit two-step (set target → FAB commits).
