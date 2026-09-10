# Row cursor/selection/clipboard state, unified across TUI/desktop/touch

The decision record is ADR 0005 (`docs/adr/0005-row-cursor-selection-clipboard-state-model.md`).
This document is the detail: per-state core mapping, per-platform entry-gesture tables,
the visual design spec, the keybinding table, the cut/copy-mode redesign, and the
implementation history. Node-kind and per-format mutation mechanics are not repeated
here — see `MUTATIONS.md`'s "Insert / move legality" table and `BEHAVIOR_MATRIX.md`. TUI
mechanics beyond row state live in `TUI.md`; web/desktop/touch architecture beyond row
state lives in `WEBUI.md`.

## 1. The five states

Layered, not parallel — each later state is entered independently and sits on top of
the ones before it; a row can be in several at once.

| # | Canonical name | 中文 | Core field | Who can enter it |
|---|---|---|---|---|
| 1 | Cursor | 提示定位 | `Session.cursor: Path` (`session.rs`) | TUI keyboard, desktop keyboard. Desktop mouse **hover** is a separate, core-invisible signal — see §1a. Touch has no equivalent. |
| 2 | Focal row | 選取 | Derived: `selected_paths()`'s target for single-row mutating ops — edit value/key/comment (`session.rs`) | Always equals `cursor`, or the last/focal member of a non-empty `Selection` (`set_selection` keeps the clicked/typed path last). Remark, delete, and copy/cut are **not** in this group — they consume the whole `Selection` (§1c). |
| 3 | Locked selection | 鎖定選取 | `Session.selection: Selection` non-empty (`session.rs`, `selection/selection.rs`) | TUI: `s` (`ToggleSelect`) / Shift+↑↓ (`ExtendSelectUp/Down`). Desktop: Ctrl/Shift+click, marquee (`web/select.ts`). Touch: single-tap `selectOnly()` writes a 1-path `Selection`; modifier taps go through `resolveClick` (range/toggle); post-paste re-selects the landed batch (§6d). All surfaces show the leading-bar marker. |
| 4 | Clipboard-armed (cut/copy mode) | 剪下複製模式 | `Session.clipboard.is_some()` (`session.rs`, `state.rs`) | `c`/`x`/Copy/Cut on any surface. Freezes state #3 (four guards in `session.rs`: `toggle_select`, `set_selection`, `extend_select_up`, `extend_select_down`) — entering #4 does not require #3 to be non-empty first; a bare cursor with an empty `Selection` can still be copied/cut via the fallback in `selected_paths()`. |
| 5 | Clipboard source | cut/copy source | `Session.clipboard.sources: Vec<Path>`, colored by `clipboard.cut: bool` (`state.rs`) | Only meaningful while #4 is active. |

### 1a. Hover is not a core state

Desktop mouse hover (`.row:hover`, `web/style.css`) never calls `dispatch` and never
touches `Session.cursor` — it is pure CSS. It is visually identical to state #1
(same fill color, §3) but can sit on a different row than the keyboard cursor
simultaneously; that's intentional, not a bug, since it carries no side effects.

### 1b. `Selection` is one struct regardless of member count

A desktop plain click and a TUI/desktop multi-select gesture both write the same
`Selection` struct — the only difference is how many paths end up in it. State #3's
marker (§3) therefore applies uniformly starting at one member; there is no "N ≥ 2"
threshold anywhere in this model. This is also why the plain-click case, not a
dedicated flag, is what explains the ESC asymmetry in §2.

### 1c. Multi-selection semantics — which ops consume it, and how it stays valid

This is the single source of truth for how state #3 (Locked selection) interacts with
mutating operations. The governing contract is `selected_paths()` (`session.rs`): an
active `Selection` **outranks the cursor** for every selection-aware op; when empty, the
op falls back to the cursor singleton. `normalize()` drops any selected path that is a
descendant of another selected one (§6.2 of the selection module), so an op never
processes both a container and its own child.

Selection-aware ops, and what each leaves behind (the post-op guarantee):

| Op | Reads | Post-op selection | Post-op cursor |
|---|---|---|---|
| Delete | `selected_paths()` | **Drops dead paths** — deleted paths are removed; co-selected paths that still resolve are kept. Positional paths shifted by a deleted sibling are dropped as unresolvable (under-select, never mis-select). | **Snaps to the deletion point** — the row that took the deleted rows' place, clamped to the last row when the tail was deleted. Live immediately; no host `compute_rows()` needed (calling it anyway is a no-op, since it only re-snaps an *unresolvable* cursor). Superseded the earlier vanish contract, where `cursor_row()` returned `None` until the host snapped and the fallback dumped the cursor on row 0. |
| Copy / Cut | `selected_paths()` | Untouched — entering Clipboard-armed (state #4) froze it (§5/§1). | Unchanged. |
| Remark (`r`) | `selected_paths()` (deepest-first order is irrelevant; **top-down row order** is what matters, see below) | **Remapped onto the remark's post-image** — see the three shapes below. With an empty `Selection`, the cursor fallback applies and `do_remark`'s own row re-anchor suffices. | Anchored to the topmost remapped row when a selection was active; `do_remark`'s index re-anchor otherwise. |
| Rename | `remap_prefix` | **Remapped** — any selected path under the renamed prefix follows it (`selection.rs`). | Remapped at each rename call site, mirroring the selection. |
| Paste / Move | — | **Cleared unconditionally** (§6d — a stale post-paste selection was the `e6f4965`/`27f1b50` bug). | Placed on the first pasted/moved node. |

**Remark's three post-image shapes** (the invariant added by the multi-select remap):
remarking a row can (a) swap its kind in place (Node ↔ Comment — row count unchanged, but
the addressing changes Key ↔ positional), (b) merge it into an adjacent comment block
(row count shrinks), or (c) split a selected block back into 1+d live rows when
un-remarking (row count grows). `Session::remark` therefore anchors each selected path to
its visible row index **before** the mutation and remaps **after**, producing:

- in-place swap → the selection follows the swapped address;
- merge → the selection collapses onto the merged block. Round trip works: select a,b →
  remark → the merged block is selected → remark → both rows restored and selected;
- split → the selection **expands** onto every restored row.

Processing order is **top-down by row**: remarking an earlier row before a later one means
each remark can only merge *upward* into a block above it, so an already-recorded
post-image can never be invalidated by a later merge. (`normalize()` has already removed
ancestor/descendant pairs, so depth order is irrelevant.) Regression tests:
`remark_selection_remaps_to_merged_block_and_back`,
`remark_selection_expands_when_unremarking_merged_block`,
`remark_selection_json_remaps_through_collapse`,
`remark_selection_tracks_scattered_rows`, `delete_selected_drops_stale_paths`
(`crates/confy-core/tests/session_headless.rs`).

## 2. Escape ladder (unchanged — recorded, not redesigned)

`Session::escape()` (`session.rs`) peels exactly one layer per press, shared
by every host:

1. If `clipboard.is_some()` → clear it (status `core.clipboard.cleared` if a selection
   remains).
2. Else if `selection` is non-empty → `selection.clear()` (status `core.selection.cleared`).

The previously-assumed "TUI = 1 press for a bare cursor, mouse = always 2 presses" is
this mechanism's direct consequence, not a platform-specific rule:

- TUI arrow-key navigation never calls `SetSelection` — a bare cursor move leaves
  `Selection` empty, so if the clipboard was armed, one Esc clears layer 1 and there is
  nothing left for layer 2 to do (visually: one press fully clears).
- A desktop plain click always calls `SetSelection([path])` (`web/ui.ts`) — even a
  "single selection" is a real one-entry `Selection`. So on desktop there is always
  something for layer 2 to clear after layer 1, hence the consistently-observed two
  presses.

No code changes to `escape()` under this model; §1b already explains the asymmetry.

## 3. Visual design

Background fill is exclusive to exactly three states — a row shows **at most one** of
these fills at a time:

| Fill | State | TUI (`tui/ui.rs`) | Desktop (`web/style.css`) | Touch (`web/touch/style.css`) |
|---|---|---|---|---|
| Cursor | #1 (incl. hover, §1a) | Full-row blue background (`Color::Blue`, bold, white fg; suppressed when armed unless active slot row) | Full-row `--cursor-bg` fill (`body:not(.paste-mode) .row.cursor`, shared with `body:not(.paste-mode) .row:hover`) | Full-row `--cursor-bg` fill on `.row-main` (`.app:not(.paste-mode) .row.cursor > .row-main`) |
| Cut source | #5, `cut=true` | Full-row green background (`Color::Green`, white fg) | Full-row `--cut-bg` fill (`.row.clip-cut`) | Full-row `--cut-bg` fill on `.row-main` (`.row.clip-cut > .row-main`) |
| Copy source | #5, `cut=false` | Full-row magenta background (`Color::Magenta`, white fg) | Full-row `--copy-bg` fill (`.row.clip-copy`) | Full-row `--copy-bg` fill on `.row-main` (`.row.clip-copy > .row-main`) |

Locked selection (#3) and its focal row (#2) never use a fill — they use a marker:

| Marker | State | TUI (`tui/ui.rs`) | Desktop (`web/style.css`) | Touch (`web/touch/style.css`) |
|---|---|---|---|---|
| Locked selection | #3 | Leading `●` glyph prefixed onto the NAME cell (`sel_marker = "●"`), no background fill | 3px left accent bar via `.row.selected::before` (`background: var(--sel-edge)`), no fill or ring | 3px left accent bar via `.row.selected > .row-main::before` (`background: var(--sel-edge)`), no fill or ring |
| Focal row | #2 | Cursor fill (#1) on the focal row; composes with the `●` glyph | Cursor fill (#1) on the focal row; composes with the `::before` bar | Cursor fill (#1) on the focal row; composes with the `::before` bar |

The focal row (#2) is whichever row also has the cursor fill (#1). A row can show cursor fill *and* selection marker simultaneously — that combination is exactly how a user reads "this row is part of my locked set, **and** it's the one my next edit-value/key/comment keystroke will hit."

Exact hues/glyphs are an implementation choice — the requirement is one consistent
assignment shared by TUI/desktop/touch, not a specific palette. `Selection`'s marker
must never be a background fill (that's reserved for #1/#5) so it composes cleanly with
all three fills without a rendering conflict.

Touch emits per-row `clip-cut`/`clip-copy` classes (`web/touch/render.ts`), matching
desktop (`web/render.ts`), styled with `.row.clip-cut > .row-main` and
.row.clip-copy > .row-main` in `web/touch/style.css`.

### 3a. While armed, the target cue outranks the plain Cursor/hover fill

`PasteSlot` targeting (§6) and state #1 (Cursor) both want the same full-row fill
slot on the same row whenever the target happens to sit under the cursor or the
pointer — so armed mode (state #4) suppresses the plain Cursor/hover fill
everywhere except the row(s) actually carrying `Into`/`After`, leaving the green
target cue as the only full-row highlight while `clipboard.is_some()`. TUI already
had this precedence (`tui/ui.rs`'s `active_slot.is_some() => base` arm predates
this model); desktop/touch were brought up to match it — `body:not(.paste-mode)
.row.cursor`/`.row:hover` (`web/style.css`) and `.app:not(.paste-mode) .row.cursor`
(`web/touch/style.css`) gate the plain fill off entirely once armed, rather than
letting it collide with or dim under the target cue. Cut/copy source (#5) and the
Locked-selection marker (#3) are unaffected — only the plain Cursor/hover fill is
suppressed.

## 4. Keybindings

Phase 2 shipped this reversal on both platforms; the table below is the current,
not a target, state.

| Key | TUI | Desktop |
|---|---|---|
| `Space` | `KeyAction::ToggleExpand` (`tui/keys.rs`) → `Intent::ToggleExpand` | `ToggleExpand` (`key-intent.ts`) |
| `Enter` | `KeyAction::Info` (`tui/keys.rs`) → `Intent::ToggleDetail` | `ToggleDetail` (`key-intent.ts`) |
| `i` | `KeyAction::Info` (`tui/keys.rs`) → `Intent::ToggleDetail`, unchanged alt binding | `ToggleDetail` (`key-intent.ts`), unchanged alt binding |

Touch has no physical Enter/Space; its existing double-tap-to-open-detail gesture needs
no change.

## 5. Cut/copy mode is a full cross-platform modal lock

While state #4 is active, every function except `ToggleExpand` is disabled, on all
three surfaces:

- Move/reorder — including touch's reorder-grip drag (`web/touch/app.ts`).
  It is itself a paste-equivalent operation and conflicts with mid-target-selection.
- Action menu, kind-switch, convert.
- Inline edit of value/key/comment/remark (all surfaces' equivalents: TUI `e`/`E`/`r`/
  F2, desktop click-to-edit and the Action menu's Edit item, touch tap-to-edit/edit sheet).
- Desktop marquee selection (`installMarquee` in `web/ui.ts`, `web/select.ts`):
  bails on mousedown while the clipboard is armed.

A disabled affordance shows a transient toast/status message (e.g. reusing the
existing `status`/i18n pattern ADR 0004 §6 already established for paste
collision/error text) rather than silently doing nothing — this is modal behavior
that needs to be legible when hit. Both desktop and touch guard notice display with a
`lastNoticeKey` fingerprint (`renderNotice` in `web/ui.ts`, `web/touch/app.ts`), so
navigation intents while armed do not re-trigger toast entrance animations for a
retained notice while the status bar repaints.

## 6. Cut/copy-mode target positioning

TUI is unchanged: `PasteSlot` arrow-key stepping already exists and already works
(`session.rs`).

### 6a. Desktop — new hover preview, click/commit unchanged

- Hovering a candidate row while armed computes `session.pointerSlot(path, relY)`
  client-side and paints a **local-only** preview cue (no `dispatch`, no re-render) —
  the same "compute from a DOM rect on the fly, no core round-trip" idiom `onTreeHover`
  already uses for schema tooltips (`web/ui.ts`).
- Clicking still calls the existing `armedPasteTarget()` → `SetPasteSlot`
  (`web/ui.ts`), unchanged.
- Commit is still the separate `v` key / menu Paste action, unchanged.
- The confirmed target (`snap.paste_slot`, painted solid via `.paste-target`/
  `#pasteTargetLine`) and the hover preview (painted dashed/muted via
  `.drag-over-into`/`#dropLine` while `body.paste-mode`) are two independent layers
  (`renderConfirmedPasteCue`/`renderHoverCue`, `web/ui.ts`), not one function reused for
  both as originally shipped in Phase 4. Without this split, moving the pointer off the
  confirmed target visually overwrote it with nothing, so confirming a target required
  moving the mouse fully off the tree (or onto the paste button) to see it — the hover
  layer now clears to nothing on `mouseleave` instead of falling back to redrawing the
  committed slot, since the confirmed layer already shows it independently.
- **The root row's two slots have no row to paint.** Neither web host draws the root row
  (`treeHTML`, `web/render.ts` / `web/touch/render.ts`), yet both of its slots are legal and
  reachable — the keyboard steps onto them (`paste_slots()` emits them first) and a pointer
  in the *first* drawn row's top band classifies as `After(root)`. Both are therefore drawn
  as insertion lines on a stand-in row edge (`rootSlotLine`, `web/slot-line.ts`, shared by
  desktop and touch): `After(root)` — root index 0, the document's top — at the **first**
  row's top edge, `Into(root)` — `children.len()`, an append at the document's end — at the
  **last** row's bottom edge. Before that, stepping to the top of paste mode showed no cue
  at all for two steps, and a drag aimed at the very top drew its line under the hovered
  row, indistinguishable from `After(<first row>)`.
- **Stepping *up* onto the root row's `Into` slot is corrected web-side only.**
  `paste_slots()` lists each row's `Into` before its `After`, so the root row's `Into` is
  index 0 — above everything in the stepping order, while `slot_target` resolves it to
  `children.len()`, an append at the document's *end*. The TUI draws the root row, so
  landing there highlights a real visible row and reads correctly; **core's order is
  therefore unchanged**. In the web hosts, where that row isn't drawn, `↑`/`k`/PageUp/`Home`
  from the top of the tree instead threw the insertion point to the far end of the document
  and clamped there (index 0 can't step further) — a move in the opposite direction, onto a
  slot the user was not aiming at. `overshotUndrawnRootSlot()` (`web/path-utils.ts`, the
  paste-mode sibling of `drawnCursorFallback`) detects exactly that combination — upward
  intent + resulting slot `Into(root)` — and both `navSelect`/`touchNavSelect` step one slot
  back down onto `After(root)`, the document top. Downward navigation and `End` are left
  alone: reaching the append slot from below is correct, and it is drawn at the last row's
  bottom edge.

### 6b. Touch — body-drag continuously repositions the target; FAB still commits

Reuses the existing reorder-drag machinery (`web/touch/app.ts`,
`onReorderMove` — already does live `pointer_slot()` classification and repaints the
same `.reorder-line`/`.drop-into` cues `renderPasteSlotCue` uses) instead of
inventing a new gesture:

- While armed, a pointerdown/pointermove/pointerup drag anywhere on the **row body**
  (not requiring the grip handle — the grip itself is disabled per §5) continuously
  repositions the target as the finger moves, mirroring `onReorderMove`'s live
  hit-test-and-classify loop.
- Release only sets/refines the target — **no auto-commit**, matching desktop's
  set-then-separately-commit flow (§6a) rather than reorder-drag's own
  commit-on-release behavior. The FAB (`web/touch/app.ts`) still performs the
  actual `Paste`.
- Caret disambiguation must move earlier: today it only resolves at tap time
  (`handleTap`, `web/touch/app.ts`); a pointerdown-level
  `closest('.caret')` bail is required so a caret press that never moves still falls
  through to the existing `act === "caret"` branch (`SetCursor` + `ToggleExpand`),
  mirroring the existing `closest('.drag-handle')` gate (`web/touch/app.ts`) that
  already keeps reorder-drag and tap mutually exclusive today.

### 6c. Edge auto-scroll — touch only, implemented; desktop/TUI need no equivalent

**Auto-scroll on edge-drag** (touch's armed-paste body-drag, §6b, and its
reorder-grip drag) shares one `requestAnimationFrame` loop (`web/touch/app.ts` —
`edgeScrollY`/`edgeScrollRAF`/`edgeAutoScrollStep`/`kickEdgeAutoScroll`): while
either drag is active, the loop nudges `.tree-pane`'s `scrollTop` toward whichever
edge the pointer sits near (speed ramps up closer to the edge) and re-runs that
drag's own hit-test (`onPasteDragMove`/`onReorderMove`) each tick against the same
pointer position, since content shifts under an otherwise-stationary finger; it
self-terminates once neither drag is active. It does **not** fight the existing
scroll-position-restore-on-render latch (`web/touch/app.ts`) because
neither drag's hit-test dispatches mid-gesture — only release does, and `render()`
only runs after a dispatch.

Desktop and TUI were deliberately never given an equivalent, not because of an
oversight but because each already solves "the target might be off-screen"
differently, appropriately to its own input model:

- **Desktop** grip-reorder uses native HTML5 drag-and-drop (`web/dnd.ts`), which
  gets edge auto-scroll for free from the browser over a scrollable container —
  building a hand-rolled version would duplicate what the platform already does.
  Desktop's armed-paste targeting (§6a) is hover-driven, not drag-driven, so there
  is no in-flight gesture to auto-scroll during in the first place — the pointer
  simply isn't a candidate row yet if it's off-screen, exactly like clicking
  anything off-screen.
- **TUI** has no pointer drag at all — targeting is keyboard-only (`PasteSlot`
  arrow-key stepping, or the tree cursor), and the TUI's viewport already
  auto-follows the cursor/paste-slot on every navigation step (a pre-existing,
  unrelated mechanism, not part of this model) — an off-screen target becomes
  on-screen the moment a key press moves onto it, so there is no drag-scroll gap
  to fill.

### 6d. Post-paste highlight — desktop-only, new

After a `Paste` lands, core's `do_paste` (`clipboard.rs`) uniformly
expands every collapsed ancestor of the destination and places `cursor` on the
first pasted/moved node — but deliberately does **not** select the pasted set
(`self.selection.clear()` runs unconditionally on every paste/move). This is the
fix for the `e6f4965`/`27f1b50` bug (ADR 0004's Consequences section): a real, persistent,
core-level `Selection` covering the pasted nodes previously survived plain
cursor-only arrow-key navigation with nothing to clear it, so a later cut/copy/
rename could silently operate on a stale set. Ancestor-expand and cursor placement
are core-level and already uniform across all three hosts; the Selection-clear is
too — none of that is host-specific and none of it changed by what follows.

**Desktop** (`web/ui.ts`'s `send()`) additionally re-selects the just-pasted set as
a purely client-side, purely ephemeral compensating layer: after a dispatch whose
`clipboard_count` just dropped to 0 with no error and `mode === "Normal"`, it reads
the landing siblings via `session.children(parent)` and issues one extra
`SetSelection`, painting the Locked-selection marker (§3) around every pasted node
so the just-landed batch stays visible. This is safe *only* because desktop's
  keyboard/click navigation (`navSelect`, `web/ui.ts`; `onTreeClick`'s plain
click path) unconditionally re-issues a fresh one-path `SetSelection` on every
subsequent nav step or click — so this extra Selection never outlives the single
gesture that follows it, unlike the reverted bug. It is a client-side echo of
`clipboard_count`, not a new core field or `Intent`.

**Touch and TUI do not have this.** The two hosts are not symmetric, though:

- **Touch** now has this too — `web/touch/app.ts`'s `send()` mirrors desktop's
  compensator verbatim, safe for the identical reason: touch's own tap handling
  (`selectOnly()` in `web/touch/app.ts`) already collapses `Selection` to a
  single path on every tap, the same self-clearing guarantee desktop relies on.
- **TUI cannot adopt the identical pattern safely.** `cursor_down`/`cursor_up`
  (`session.rs`) never touch `Selection` at all — a Locked selection set
  via `s` is *meant* to persist across arrow-key navigation until the user
  explicitly toggles it off or presses Esc (that persistence is how the TUI's
  own select-a-range-then-`x`/`c` workflow works). Reusing the desktop compensator
  verbatim would reintroduce exactly the `e6f4965`/`27f1b50` failure mode inside
  the TUI: a post-paste Selection with no code path that ever clears it on plain
  nav. Giving TUI equivalent visual feedback, if wanted, needs a TUI-native
  mechanism that does not reuse the real `Selection` field (e.g. a host-local,
  frame-limited flash independent of core state) — a materially different,
  bigger change than "call the same compensator," not a gap in this phase.

## 7. Worked example: bug 3 as a regression case for this model

Symptom (fixed): while armed, tapping/clicking any branch's caret toggled the
**clipboard source** node's expand state, never the clicked one.

Root cause under this model: `ToggleExpand` is defined against state #1 (`cursor`,
`dispatch.rs`). The armed-click path only ever sent `SetPasteSlot` (state #6's
target, unrelated to state #1) and never moved `cursor` — so `ToggleExpand` kept firing
against wherever `cursor` had been frozen since the clipboard was armed, which visually
read as "it always hits the source row" (the source row is usually where cursor was
last sitting when `c`/`x` was pressed).

Fixed by sending an explicit `SetCursor` before `ToggleExpand` in both hosts
(`navSelect` in `web/ui.ts`; touch `handleTap`'s caret branch in `web/touch/app.ts`).

**Formal invariant this model adds**: any `Intent` defined against state #1 (`cursor`)
must resolve against the row the user actually invoked it on, even while state #4 is
active and state #6 (paste target) is being set by the same gesture. `ToggleExpand` is
the only such intent surfaced through the armed-click path today; any future intent
added to that path must uphold the same invariant, checked by the same kind of
regression test as the existing fix (`web/*.spec.mjs`, `touch-pointer-slot`/
`touch-paste-cue`).

## 8. Implementation history

The row state model shipped across five sequential phases followed by a targeted
ad-hoc round. Detailed execution records, review checkpoints, and migration steps
are preserved in the frozen plans:

- Phase 1 (Visual language): `../plan/2026-08-18-row-state-visual-language-phase1.md`
- Phase 2 (Keybinding reversal): `../plan/2026-08-18-row-state-visual-language-phase2.md`
- Phase 3 (Cut/copy modal lock): `../plan/2026-08-18-row-state-visual-language-phase3.md`
- Phase 4 (Desktop hover preview): `../plan/2026-08-18-row-state-visual-language-phase4.md`
- Phase 5 (Touch drag-to-target): `../plan/2026-08-18-row-state-visual-language-phase5.md`

The governing architectural decision is recorded in ADR 0005 (`../adr/0005-row-cursor-selection-clipboard-state-model.md`).

## 9. Out of scope

- Auto-scroll on edge-drag (§6c) — **implemented for touch**; desktop/TUI need no
  equivalent (§6c explains why). No longer an open item.
- §6d's post-paste highlight is now on both desktop and touch; TUI stays a
  documented, deliberate asymmetry — not revisited unless real TUI users report
  losing track of a multi-node paste (§6d explains why porting it verbatim would
  be unsafe there).
- Desktop's marquee (`web/select.ts`/`web/ui.ts`'s `installMarquee`) now guards
  `clipboard_count`/`paste-mode` like every other affordance §5 disables while
  armed — found and fixed via the integration audit
  (`docs/audit/2026-08-19-clipboard-row-state-integration-audit.md`).
- Any change to node-kind/format mutation mechanics, `PasteSlot`/`Into`/`After`
  targeting semantics, or the AoT atomic-move behavior — all owned by ADR 0004
  (and, for pointer-driven targeting, ADR 0010), `glossary.md`,
  `BEHAVIOR_MATRIX.md`, untouched here.
- ~~TUI `type_col_cell`'s fill-skip doesn't cover the paste-slot `Into` target
  row's green fill~~ — **fixed** (`tui/ui.rs`, `type_col_cell` call site now
  passes `is_cursor || in_clipboard_source || into_here`). Correction to the
  original note: `Into` slots are only ever offered on branch rows
  (`Session::paste_slots`/`pointer_slot` both gate on `is_branch()` — and, since
  ADR 0010, on nothing else: an `Inline` single-line container gets its `Into`
  band from the pointer too), and a
  branch's `type_label` never carries a KIND colour, so the collision was not
  reachable through normal keyboard/pointer paste-slot cycling — it was
  reachable only through the WASM `Intent::SetPasteSlot` boundary, which does
  not re-validate `is_branch`. Fixed defensively regardless, with a regression
  test (`paste_target_into_fill_suppresses_kind_tag_color`) that drives the
  state directly to pin the render-layer contract.
