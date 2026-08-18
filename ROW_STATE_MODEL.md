# ROW_STATE_MODEL.md — row cursor/selection/clipboard state, unified across TUI/desktop/touch

The decision record is ADR 0005 (`docs/adr/0005-row-cursor-selection-clipboard-state-model.md`).
This document is the detail: per-state core mapping, per-platform entry-gesture tables,
the visual design spec, the keybinding table, the cut/copy-mode redesign, and the phased
implementation task list. Node-kind and per-format mutation mechanics are not repeated
here — see `CONTEXT.md`'s "Insert / move legality" table and `BEHAVIOR_MATRIX.md`. TUI
mechanics beyond row state live in `TUI.md`; web/desktop/touch architecture beyond row
state lives in `WEBUI.md`.

## 1. The five states

Layered, not parallel — each later state is entered independently and sits on top of
the ones before it; a row can be in several at once.

| # | Canonical name | 中文 | Core field | Who can enter it |
|---|---|---|---|---|
| 1 | Cursor | 提示定位 | `Session.cursor: Path` (`session.rs:23`) | TUI keyboard, desktop keyboard. Desktop mouse **hover** is a separate, core-invisible signal — see §1a. Touch has no equivalent. |
| 2 | Focal row | 選取 | Derived: `selected_paths()`'s target for single-row mutating ops (`session.rs:1258`) | Always equals `cursor`, or the last/focal member of a non-empty `Selection` (`set_selection` keeps the clicked/typed path last). |
| 3 | Locked selection | 鎖定選取 | `Session.selection: Selection` non-empty (`session.rs:25`, `selection/selection.rs:25-30`) | TUI: `s` (`ToggleSelect`) / Shift+↑↓ (`ExtendSelectUp/Down`). Desktop: Ctrl/Shift+click, marquee (`web/select.ts`). Touch: **none** — `selectOnly()` (`web/touch/app.ts:495-498`) only ever sets a 1-path selection. |
| 4 | Clipboard-armed (cut/copy mode) | 剪下複製模式 | `Session.clipboard.is_some()` (`session.rs:33`, `state.rs:205-209`) | `c`/`x`/Copy/Cut on any surface. Freezes state #3 (four guards: `session.rs:1196-1199, 1208-1211, 1222-1225, 1240-1243`) — entering #4 does not require #3 to be non-empty first; a bare cursor with an empty `Selection` can still be copied/cut via the fallback in `selected_paths()`. |
| 5 | Clipboard source | cut/copy source | `Session.clipboard.sources: Vec<Path>`, colored by `clipboard.cut: bool` (`state.rs:207-208`) | Only meaningful while #4 is active. |

### 1a. Hover is not a core state

Desktop mouse hover (`.row:hover`, `web/style.css:166`) never calls `dispatch` and never
touches `Session.cursor` — it is pure CSS. It is visually identical to state #1
(same fill color, §3) but can sit on a different row than the keyboard cursor
simultaneously; that's intentional, not a bug, since it carries no side effects.

### 1b. `Selection` is one struct regardless of member count

A desktop plain click and a TUI/desktop multi-select gesture both write the same
`Selection` struct — the only difference is how many paths end up in it. State #3's
marker (§3) therefore applies uniformly starting at one member; there is no "N ≥ 2"
threshold anywhere in this model. This is also why the plain-click case, not a
dedicated flag, is what explains the ESC asymmetry in §4.

## 2. Escape ladder (unchanged — recorded, not redesigned)

`Session::escape()` (`session.rs:1594-1636`) peels exactly one layer per press, shared
by every host:

1. If `clipboard.is_some()` → clear it (status `core.clipboard.cleared` if a selection
   remains).
2. Else if `selection` is non-empty → `selection.clear()` (status `core.selection.cleared`).

The previously-assumed "TUI = 1 press for a bare cursor, mouse = always 2 presses" is
this mechanism's direct consequence, not a platform-specific rule:

- TUI arrow-key navigation never calls `SetSelection` — a bare cursor move leaves
  `Selection` empty, so if the clipboard was armed, one Esc clears layer 1 and there is
  nothing left for layer 2 to do (visually: one press fully clears).
- A desktop plain click always calls `SetSelection([path])` (`web/ui.ts:1096`) — even a
  "single selection" is a real one-entry `Selection`. So on desktop there is always
  something for layer 2 to clear after layer 1, hence the consistently-observed two
  presses.

No code changes to `escape()` under this model; §1b already explains the asymmetry.

## 3. Visual design

Background fill is exclusive to exactly three states — a row shows **at most one** of
these fills at a time:

| Fill | State | Current TUI | Current desktop | Current touch | Target |
|---|---|---|---|---|---|
| Cursor | #1 (incl. hover, §1a) | Blue bg (`tui/ui.rs:405-408`) | 3px left accent bar (`web/style.css:168`) — **not a fill today** | none | Full-row fill, one color, shared by TUI keyboard cursor / desktop keyboard cursor / desktop hover. Desktop's bar treatment is retired in favor of the fill. |
| Cut source | #5, `cut=true` | Green bg (`tui/ui.rs:381-385`) | Purple dashed outline (`--t-date`, `web/style.css:567-568`) | FAB-only color (`touch/style.css:526`), no per-row style | Full-row fill, one color, same across all three (TUI/desktop currently disagree on which of green/blue is cut — this is the actual bug to fix, not just formalize). |
| Copy source | #5, `cut=false` | Blue bg (`tui/ui.rs:381-385`) — **collides with cursor's blue** | Green dashed outline (`--t-string`, `web/style.css:566`) | FAB-only color, no per-row style | Full-row fill, one color, distinct from cut and from cursor. |

Locked selection (#3) and its focal row (#2) never use a fill — they use a marker:

| Marker | State | Current | Target |
|---|---|---|---|
| TUI leading glyph | #3 | **Already exists**, redundantly stacked on top of a full-row grey bg: `sel_marker = "●"` is already prefixed onto the NAME cell whenever `app.session.selection.contains(&row.path)` (`tui/ui.rs:328-333`) — the grey fill (`tui/ui.rs:385-386`) is the *only* part that needs to go; the glyph itself needs no new code. |  Drop the grey fill; keep the existing `●` glyph as the sole marker, one member or many. |
| Desktop/touch leading bar | #3 | Fill + inset ring (`.row.selected`, `web/style.css:167`) | Drop the fill+ring; repurpose the `::before` left-bar treatment (freed up by retiring `.row.cursor`'s bar, above) as this marker, in a tone distinct from all three fills. |
| Cursor fill (already covered) | #2 (focal row) | — | No separate treatment: the focal row is whichever row also has the cursor fill (#1). A row can show cursor-fill *and* selection-marker simultaneously — that combination is exactly how a user reads "this row is part of my locked set, **and** it's the one my next edit-value/key/comment keystroke will hit." |

Exact hues/glyphs are an implementation choice — the requirement is one consistent
assignment shared by TUI/desktop/touch, not a specific palette. `Selection`'s marker
must never be a background fill (that's reserved for #1/#5) so it composes cleanly with
all three fills without a rendering conflict.

Touch gains new per-row `clip-cut`/`clip-copy` classes (`touch/render.ts:57-65`
currently emits neither); the pre-existing dead `.row.cut` rule (`touch/style.css:119`,
never emitted by any code path) is removed, not repurposed — the live class names stay
`clip-cut`/`clip-copy` to match desktop's.

## 4. Keybindings

| Key | Current TUI | Current desktop | Target |
|---|---|---|---|
| `Space` | `ToggleExpand` (`tui/keys.rs:56`) | `ToggleDetail` (`key-intent.ts`) | `ToggleExpand` (desktop changes) |
| `Enter` | `ToggleExpand` (`tui/keys.rs:56`) | `ToggleExpand` ("toggle-branches", `key-intent.ts`) | `ToggleDetail` (**both platforms reverse**) |
| `i` | `ToggleDetail` (`tui/keys.rs:63`) | `ToggleDetail` (`key-intent.ts`) | `ToggleDetail` (unchanged — stays the alt binding) |

Touch has no physical Enter/Space; its existing double-tap-to-open-detail gesture needs
no change.

## 5. Cut/copy mode is a full cross-platform modal lock

While state #4 is active, every function except `ToggleExpand` is disabled, on all
three surfaces:

- Move/reorder — including touch's reorder-grip drag (`web/touch/app.ts:955-1069`).
  It is itself a paste-equivalent operation and conflicts with mid-target-selection.
- Context menu, kind-switch, convert.
- Inline edit of value/key/comment/remark (all surfaces' equivalents: TUI `e`/`E`/`r`/
  F2, desktop click-to-edit/context-menu items, touch tap-to-edit/edit sheet).

A disabled affordance shows a transient toast/status message (e.g. reusing the
existing `status`/i18n pattern ADR 0004 §6 already established for paste
collision/error text) rather than silently doing nothing — this is new modal behavior
users have not seen before and needs to be legible the first time it's hit.

## 6. Cut/copy-mode target positioning

TUI is unchanged: `PasteSlot` arrow-key stepping already exists and already works
(`session.rs:474-505`).

### 6a. Desktop — new hover preview, click/commit unchanged

- Hovering a candidate row while armed computes `session.pointerSlot(path, relY)`
  client-side and paints a **local-only** preview cue (no `dispatch`, no re-render) —
  the same "compute from a DOM rect on the fly, no core round-trip" idiom `onTreeHover`
  already uses for schema tooltips (`web/ui.ts:1055-1066`).
- Clicking still calls the existing `armedPasteTarget()` → `SetPasteSlot`
  (`web/ui.ts:1075-1096`), unchanged.
- Commit is still the separate `v` key / menu Paste action, unchanged.

### 6b. Touch — body-drag continuously repositions the target; FAB still commits

Reuses the existing reorder-drag machinery (`web/touch/app.ts:971-1039`,
`onReorderMove` — already does live `pointer_slot()` classification and repaints the
same `.reorder-line`/`.drop-into` cues `renderPasteSlotCue` uses) instead of
inventing a new gesture:

- While armed, a pointerdown/pointermove/pointerup drag anywhere on the **row body**
  (not requiring the grip handle — the grip itself is disabled per §5) continuously
  repositions the target as the finger moves, mirroring `onReorderMove`'s live
  hit-test-and-classify loop.
- Release only sets/refines the target — **no auto-commit**, matching desktop's
  set-then-separately-commit flow (§6a) rather than reorder-drag's own
  commit-on-release behavior. The FAB (`web/touch/app.ts:1345-1349`) still performs the
  actual `Paste`.
- Caret disambiguation must move earlier: today it only resolves at tap time
  (`handleTap`, `web/touch/app.ts:1197, 1210-1222`); a pointerdown-level
  `closest('.caret')` bail is required so a caret press that never moves still falls
  through to the existing `act === "caret"` branch (`SetCursor` + `ToggleExpand`),
  mirroring the existing `closest('.drag-handle')` gate (`web/touch/app.ts:1076`) that
  already keeps reorder-drag and tap mutually exclusive today.

### 6c. Deferred, not part of this implementation

**Auto-scroll on edge-drag** — no such mechanism exists anywhere in the repo (touch or
desktop; desktop's native HTML5 drag gets it for free from the browser, which is why
nobody hand-rolled it — `web/dnd.ts` has none). Building it also has to teach the
existing scroll-position-restore-on-every-render latch (`web/touch/app.ts:424-427`)
about an in-flight drag, or the two will fight. Recorded here as a known follow-up;
explicitly not scheduled by the task list below.

## 7. Worked example: bug 3 as a regression case for this model

Symptom (fixed): while armed, tapping/clicking any branch's caret toggled the
**clipboard source** node's expand state, never the clicked one.

Root cause under this model: `ToggleExpand` is defined against state #1 (`cursor`,
`dispatch.rs:67-78`). The armed-click path only ever sent `SetPasteSlot` (state #6's
target, unrelated to state #1) and never moved `cursor` — so `ToggleExpand` kept firing
against wherever `cursor` had been frozen since the clipboard was armed, which visually
read as "it always hits the source row" (the source row is usually where cursor was
last sitting when `c`/`x` was pressed).

Fixed by sending an explicit `SetCursor` before `ToggleExpand` in both hosts
(`web/ui.ts:1161-1165`, `web/touch/app.ts:1213-1223`).

**Formal invariant this model adds**: any `Intent` defined against state #1 (`cursor`)
must resolve against the row the user actually invoked it on, even while state #4 is
active and state #6 (paste target) is being set by the same gesture. `ToggleExpand` is
the only such intent surfaced through the armed-click path today; any future intent
added to that path must uphold the same invariant, checked by the same kind of
regression test as the existing fix (`web/*.spec.mjs`, `touch-pointer-slot`/
`touch-paste-cue`).

## 8. Implementation task list

Each phase is independently testable/shippable; ordering follows ADR 0005's
Consequences.

- [x] **Phase 1 — Visual language (§3)**
  - [x] TUI: replace `Selection`'s solid grey fill with a leading NAME-cell glyph;
        unify cursor/cut/copy fill colors so cut and copy no longer collide with
        cursor's blue.
  - [x] Desktop: retire `.row.cursor`'s left bar in favor of a full-row fill shared
        with `.row:hover`; retire `.row.selected`'s fill+ring in favor of a leading
        `::before` bar marker; swap `.clip-cut`/`.clip-copy` colors to match the
        unified cut/copy assignment; remove dead `.row.cut` rule.
  - [x] Touch: add `clip-cut`/`clip-copy` row classes (currently absent) to
        `touch/render.ts`; add a resting-cursor style (currently absent); add the
        selection leading-bar marker; remove dead `.row.cut` rule.
- [x] **Phase 2 — Keybinding reversal (§4)**
  - [x] Desktop `key-intent.ts`: `Space` → `ToggleExpand`, `Enter` → `ToggleDetail`.
  - [x] TUI `tui/keys.rs`: `Enter` → `ToggleDetail` (drop its `ToggleExpand` binding;
        `Space` keeps `ToggleExpand`).
- [x] **Phase 3 — Cut/copy modal lock (§5)**
  - [x] Disable reorder-grip drag, context menu, kind-switch, convert, and inline
        edit affordances while `clipboard.is_some()`, on all three surfaces.
  - [x] Add the toast/status message on a blocked attempt.
- [ ] **Phase 4 — Desktop hover preview (§6a)**
  - [ ] Add client-only hover-preview cue computed from `pointerSlot()`, no dispatch.
- [ ] **Phase 5 — Touch drag-to-target (§6b)**
  - [ ] Body-drag while armed continuously repositions the target, reusing
        `onReorderMove`'s live classify-and-repaint loop; release sets/refines only.
  - [ ] Move caret disambiguation to pointerdown (`closest('.caret')` bail, mirroring
        the existing `.drag-handle` gate).
- [ ] **Docs sync** (do in the same change as the phase that ships it, not after —
      per ADR 0004's own lesson about docstring drift):
  - [x] `TUI.md`'s "Multi-select"/"Clipboard / paste" render-cue prose, once Phase 1
        ships (completed in Phase 1 final review).
  - [ ] `WEBUI.md`'s row-anatomy/paste-mode prose (Phase 1 visual language & Phase 2
        keybinding prose synced; hover-preview prose pending Phase 4).

## 9. Out of scope

- Auto-scroll on edge-drag (§6c) — recorded, not scheduled.
- Any change to node-kind/format mutation mechanics, `PasteSlot`/`Into`/`After`
  targeting semantics, or the AoT atomic-move behavior — all owned by ADR 0004,
  `CONTEXT.md`, `BEHAVIOR_MATRIX.md`, untouched here.
- TUI `type_col_cell`'s fill-skip (`tui/ui.rs:48-56`, `has_fill` param) doesn't cover
  the paste-slot `Into` target row's own green `bg(Green).fg(Black).BOLD` fill
  (`tui/ui.rs:397-405`) — call site only passes `is_cursor || in_clipboard_source`
  (`tui/ui.rs:414`), so a Green "string" KIND tag can render on that green fill,
  the same legibility collision Phase 1 fixed for cursor/clip-source but in a path
  Phase 1 didn't touch (ADR 0004's paste-target state, not one of ROW_STATE_MODEL's
  five). Recorded, not scheduled — pick up alongside Phase 4 (§6a) if desktop's
  equivalent paste-target legibility needs the same audit.
