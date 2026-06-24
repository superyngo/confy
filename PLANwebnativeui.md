# Web-native UI/UX redesign for confy

## Context

`web/` was ported as a near-literal transcription of the ratatui TUI: a monospace `<pre>`
tree (`ui.ts:renderTree`), vim keybindings (`onKey`), and modal overlays driven *directly*
by the core's keyboard state machine — the search box types char-by-char into `Mode::Filter`
via `FilterChar`, kind-switch/type-filter/convert are arrow-navigated, editing pushes to a
`<textarea>`. It works but the interaction model is "TUI on a web page": no mouse-first
affordances, no inline widgets, no real selection gestures.

Goal: make the web UI **web-native** (clickable inline-editing tree, in-row `<input>`/`<select>`
widgets, mouse multi-select, context menu) while keeping the headless `confy-core` `Session`
as the single source of truth. The Rust engine, the `Intent`/`SessionSnapshot` contract, the
FFI marshalling, and the TUI are all preserved. We change only the **presentation +
interaction** layer and add a handful of **batch `Intent`s** so native widgets dispatch one
committed action instead of replaying keystrokes through the modal machine.

Architecture invariant (WEBUI.md / PORTING §8.3): the UI is stateless w.r.t. editing — it holds
the latest `SessionSnapshot`, renders DOM from it, and every interaction is one
`dispatch(Intent)` → new snapshot → re-render. The new intents are **purely additive**; nothing
in core or the TUI is removed, so `session_headless.rs`, the TUI crate, and `serde_roundtrip.rs`
stay green.

```
 pointer / keyboard
        │
        ▼
  web/select.ts ─ gesture math vs snap.rows ─┐
  web/keys.ts   ─ accelerators ──────────────┤
  web/menu.ts   ─ context-menu / hover ───────┤
  in-row widgets (input/select) ─────────────┤
        │ build Intent                        │
        ▼                                     │
  web/ui.ts  ── session.dispatch(Intent) ─────┘
        │  (confy.ts → wasm → confy-core Session)
        ▼
  SessionSnapshot ── web/render.ts ──► DOM (re-render whole tree)
```

The only Rust change is new `Intent` variants + thin `Session` methods that **reuse existing
machinery**; `serde-wasm-bindgen` already marshals any new variant (FFI is field-agnostic —
`crates/confy-ffi/src/lib.rs:dispatch` just `from_value::<Intent>`), so there is **no new FFI
plumbing**. `KindTarget`/`DocFormat`/`Path` already round-trip (used today by `KindOption`/
`ConvertView`).

## Design reference & delta (`design_index_model.html`)

The visual + interaction target is `design_index_model.html` (a standalone mockup). **Port its
look and UX, not its code:** the mockup carries a *second, fake* in-memory model (`App.roots`,
`clone`/`reid`, `convertKind`, `addChild`, `kindOptions`, `serializeDoc`/`jsonOf`, `inferType`,
`encodeInt`) that never touches `confy-core`. All of that is **discarded** — every mutation
becomes a `dispatch(Intent)`, and the rendered tree comes from `SessionSnapshot`. The mockup's
`NOTATION`/`CONTAINER`/`KIND_LABEL` tables are **hardcoded TOML** and wrong for JSON/YAML; the
per-node convertible list must come from `session.kind_options(path)` (already exposed) instead.
Likewise `serializeNode`/`jsonOf`/`inferType`/`encodeInt` are deleted — serialization, type
inference and radix encoding live in core and differ per backend.

Reuse verbatim as the **presentation/UX spec**: the `:root[data-theme]` token system + all CSS
(`.row/.key/.val/.kind/.tf-*/.pop/dialog`), toolbar/filterbar/footer layout, inline `<input>`
edit cells, the per-row **kind popover** + hover action buttons, the **right-click context menu**,
the **sliding detail side-panel**, **marquee + shift-range + ⌘-toggle** selection, **drag-and-drop
reparent** (grip handle → into / before / after), and the convert `<dialog>`.

Beyond the intents tabled below, the full design needs these additional wire pieces:

| Design feature (mockup fn) | Wire | Reuse in core |
|---|---|---|
| drag-and-drop reparent (`drop`, into/before/after) | **new** `MoveSelectionTo { target: Path, index: usize }` | builds a `Target{parent,index}` and applies `Mutation::Move{ sources: selection, target, OnCollision::Cancel }`, reusing the **exact** error handling of `do_paste` (`session.rs:2016`) — Collision → `Mode::Prompt`, Illegal / YAML-opaque / AoT-move-out → `error`. JS computes target+index vs `snap.rows`; **core decides legality** |
| move up / down (⌥↑/⌥↓ ctx items, `moveSibling`) | `MoveSelectionTo` with the ±1 sibling index | reuse `Mutation::Move` (not `Nudge` — that is value-nudge) |
| inline note add/edit on a live node (`editNote`) | edit-existing via `BeginEdit` (trailing-comment inline path); **"no note yet"** → confirm `Mutation::InsertComment` (`session.rs:2123`) covers a trailing insert, else add a thin entry point | verify before Phase 4 |

> **Reorder vs reparent:** `Paste` of a *cut* clipboard already lowers to `Mutation::Move` to the
> cursor-relative paste slot (`session.rs:2012`). Drag needs a *positioned* target (e.g. drop
> before element[2]) the slot model can't express — hence the dedicated `MoveSelectionTo`. It is
> the one genuinely new mutation entry point; everything else reuses existing machinery. Core's
> `Mutation::Move` already enforces the per-format legality the mockup fakes (the mockup only
> blocks self-descendant), so the UI must surface core's rejection and never pre-judge in JS.

## Format restrictions & kind conversion (the correctness crux)

The UI **never decides** that a conversion, insert, or move is legal — it asks core and reflects
the result. Concretely:

- **Per-row kind popover / `<select>`** is populated *only* from `session.kind_options(path)`
  (backend-computed: TOML radixes + `"""`/`'''`; JSON has none of those + no AoT; YAML block/flow
  + 5 string styles), and is **disabled when the list is empty**. The chosen `target` string is
  opaque and sent back via `CommitKind{path,target}` — no JS interpretation.
- **Node position** follows the backend: array elements are keyless (`Seg::Index`, rendered "—");
  AoT entries vs array elements differ; JSON has no dotted keys / AoT / datetime; YAML opaque
  nodes reject all mutation. `AddNode`/`Paste`/`MoveSelectionTo` all defer to core, which knows the
  parent's format+kind — the host supplies only the gesture (which node, what position).
- **Context-menu / hover-action enablement** derives from snapshot state (Paste only when
  `clipboard_count`; kind item only when `kind_options` non-empty; read-only/opaque rows expose no
  mutation), not from JS guesses.

## New batch Intents (confy-core + types.ts mirror)

Add variants to `crates/confy-core/src/session/intent.rs`, route them in
`crates/confy-core/src/session/dispatch.rs` (`Session::dispatch` match), implement the thin
methods in `crates/confy-core/src/session/session.rs`, and mirror the TS shapes in
`web/types.ts` (`Intent` union).

| Intent | New `Session` method — reuse | Notes |
|---|---|---|
| `SetCursor(Path)` | `set_cursor(path)`: if `self.visible_paths().contains(&path)` set `self.cursor = path` | path-addressed analogue of `select_row(i)` (`session.rs:2428`) |
| `SetSelection(Vec<Path>)` | `set_selection(paths)`: `self.selection.clear()` then `for p in selection::normalize(paths) { self.selection.toggle(p) }` | `toggle` on empty inserts; `normalize` already exists (`selection.rs:4`). JS owns gesture math vs `snap.rows` order |
| `SetFilter(String)` | `set_filter(text)`: set `self.filter` + `self.filter_cursor`, `recompute_filter()`; `mode = FilterResults` if non-empty else `exit_filter()`; snap cursor into filtered set | one `<input>` instead of `FilterChar` replay. `recompute_filter` (`session.rs:514`) already nulls `filtered_paths` when empty |
| `CommitEdit { value: String, name: Option<String> }` | `commit_edit(value, name)`: `begin_inline_edit()`, then overwrite the `Mode::Edit` buffers (`e.buffer = value`; if `Some(n)`, `e.other_buffer = n`), then `edit_commit()` | **Preserves** type-change / collision / element-wrap / trailing-comment prompts — they fire inside `edit_commit` (`session.rs:1302`). Inline path only |
| `CommitKind { path: Path, target: KindTarget }` | `commit_kind(path, target)`: body of `kind_switch_commit` (`session.rs:647`) with explicit args — `Mutation::ConvertKind` + `on_mutation_success()` | no popup-mode dance |
| `SetConvertFormat(DocFormat)` | `set_convert_format(fmt)`: in `Mode::Convert`, set `st.target`, seed `st.path = out.<ext>`, `st.step = Path` | mirrors `convert_pick_format` (`session.rs:717`) but format chosen by value not cursor |
| `SetConvertPath(String)` | `set_convert_path(p)`: in `Mode::Convert`, set `st.path` + `st.path_cursor` | replaces `ConvertPathChar` replay; existing `ConvertRun`/`ConvertConfirm` finish |
| `MoveSelectionTo { target: Path, index: usize }` | `move_selection_to(parent, index)`: `Mutation::Move{ sources: selection-or-cursor, target: Target{parent,index}, OnCollision::Cancel }` then the `do_paste` error handling (Collision→Prompt, else `error`) + `on_mutation_success()` | drag-reparent / move-up-down. The **only** new mutation entry point |

Convert dialog flow stays `OpenConvert` (root node) → `SetConvertFormat` → `SetConvertPath` →
`ConvertRun` (→ `ConvertConfirm` if warnings). Type-filter cells keep existing
`TypeFilterMove`(JS computes the delta to the clicked cell) + `TypeFilterToggle`; add
`TypeFilterToggleAt(usize, usize)` **only if** click-to-toggle via move+toggle proves clunky.

**Tests (add alongside existing):**
- `crates/confy-core/tests/session_headless.rs` — one `dispatch(...)`-style case per new intent
  (the `dispatch_*` tests start at line 241; follow that pattern: build a `Session`, dispatch,
  assert on the returned `SessionSnapshot`). Cover at least: `SetCursor` moves cursor;
  `SetSelection` sets+normalizes (descendant dropped); `SetFilter` filters rows then clears;
  `CommitEdit` value-only and value+name; `CommitEdit` that triggers a `TypeChange` prompt
  still reaches `Mode::Prompt`; `CommitKind` converts; convert `SetConvertFormat`/`SetConvertPath`
  → `ConvertRun` yields `convert_write`; `MoveSelectionTo` reparents a node, and an **illegal**
  move (e.g. a TOML table under an array, or onto a YAML opaque node) leaves the doc untouched and
  sets `error` (assert source unchanged).
- `crates/confy-core/tests/serde_roundtrip.rs` — append each new variant to the `variants` vec in
  `intent_roundtrips` (line 38).
- `crates/confy-ffi/functional_smoke.mjs` — add checks driving the new intents through the wasm
  `ConfySession.dispatch` (bump the "N checks" count).

## Web UI rewrite (`web/`)

Rebuild `index.html`, `style.css`, and split the monolithic `ui.ts` into focused modules. Keep
`confy.ts`, `fs.ts`, `build.mjs`, `serve.mjs` unchanged; **extend** `types.ts` (Intent union only).
`build.mjs` bundles a single `entryPoints: ["ui.ts"]` via esbuild, so the new modules just need to
be imported from `ui.ts` — no build-config change.

New module split:
- `web/render.ts` — pure `SessionSnapshot → DOM`: tree rows, inline widgets, value-type color
  classes (lift `valueTypeClass` from `ui.ts:173`), branch carets, badges. Each row carries
  `data-path` (JSON-encoded `Path`) + `data-index`.
- `web/select.ts` — mouse selection gesture engine (marquee drag / Shift-range / Ctrl·Cmd-toggle),
  all math client-side against `snap.rows` order; emits `SetCursor` + `SetSelection`.
- `web/dnd.ts` — drag-and-drop reparent: grip-handle `dragstart` (drags the current selection),
  `dragover` computes into / before / after vs the row rect + draws the drop-line (mockup
  `design_index_model.html` lines 1025-1076 are the UX reference), `drop` resolves the target to a
  `{parent, index}` and emits one `MoveSelectionTo`. Self-descendant is the *only* client guard;
  all other legality is core's (rejection shows in the footer `error`).
- `web/keys.ts` — keyboard **accelerators** reusing today's `onKey` mapping (`ui.ts:308`) as a
  *secondary* path; printable-char modal replay (`FilterChar`/`EditChar`/`ConvertPathChar`) is
  gone — widgets own text entry.
- `web/menu.ts` — right-click context menu + per-row hover action buttons.
- `web/ui.ts` — orchestrator: boot/load (keep `main`/`openText`/theme/`fs.ts` calls verbatim),
  hold `snap`, `send(Intent)` (keep `ui.ts:421`), dialogs, external-edit handshake
  (`openExternalEdit`, `ui.ts:428` — keep as-is for multiline/opaque/comment).

### Layout (the agreed inline-editing tree)
```
┌ toolbar:  title  [format]  *dirty   Open  Save  Convert  ↺↻  ☾ ┐
│ 🔍 search […]                            ( type-filter ▾ )       │
├──────────────────────────────────────────────────────────────────┤
│ ▾ server                                          [＋ add] [⋮]    │
│     host  =  [ localhost          ] (String ▾)            [⋮]     │
│     port  =  [ 8080 ] (Integer ▾)                        [⋮]     │
│ ▸ plugins                                                 [⋮]     │
│     # a multiline note                                    [⋮]     │
│     notes =  multi…line  [✎ edit]                         [⋮]     │
├──────────────────────────────────────────────────────────────────┤
│ status / error                        3 selected · clipboard: 2   │
└──────────────────────────────────────────────────────────────────┘
```

- **Tree rows** render from `snap.rows`; the `<pre>` becomes a `<div id="tree">` of row `<div>`s.
  Depth = CSS indent (drop the literal two-space indent string). Branch caret click →
  `SetCursor(path)` then `ToggleExpand` (the existing `isExpanded` heuristic at `ui.ts:209` is
  reused, since rows carry no explicit expanded flag).
- **Inline edit:** click a value (or its `✎`) swaps the cell for an `<input>`; Enter/blur →
  `CommitEdit{ value }`; Esc cancels locally (no dispatch). Click a key → rename `<input>` →
  `CommitEdit{ value: <current value>, name }`. Type/format/kind = a `<select>` populated by
  `session.kindOptions(path)` (`confy.ts:68`) → on change → `CommitKind{ path, target }`.
  Multiline / literal / folded / opaque / read-only values keep the `<textarea>` modal via the
  unchanged `external_edit` handshake (snapshot already routes these to `ExternalEdit` → host
  `ApplyReplace`/`ApplyEditComment`).
- **Mouse selection (`web/select.ts`):** plain click → `SetCursor(path)` + `SetSelection([])`;
  Shift-click → contiguous range over `snap.rows` between cursor row and clicked row →
  `SetSelection(range)`; Ctrl/Cmd-click → toggle clicked path in current selected set →
  `SetSelection(next)`; marquee drag on row gutter/empty space → rows whose rect intersects form
  the set (Shift = union, Ctrl/Cmd = toggle), live `.selected` class during drag, `mouseup` →
  `SetSelection(final)`. Selected count → footer badge (reuse `renderFooter`, `ui.ts:299`).
- **Drag-and-drop reparent (`web/dnd.ts`):** drag from the row grip handle moves the current
  selection (single row if the dragged row isn't selected). `dragover` classifies the hovered row
  into *into* (branch, middle band) / *before* / *after* and shows the drop-line; `drop` resolves a
  `{parent: Path, index: usize}` and emits one `MoveSelectionTo`. The doc re-renders from the new
  snapshot — an illegal target leaves the tree unchanged and surfaces core's `error`. Move-up/down
  context items issue `MoveSelectionTo` at the ±1 sibling index.
- **Detail side-panel:** the sliding `<aside>` is driven by `ToggleDetail` + `snap.detail_text`
  (reuse the existing overlay content path; only the chrome changes from overlay to side-panel).
- **Search bar:** always-visible `<input>`; `input` event → `SetFilter(value)` (debounced ~80 ms);
  clear button → `SetFilter("")`. Rows already reflect the filter (core's `filtered_paths`).
- **Type filter:** toolbar popover with checkbox / tri-state buttons rendered from `TypeFilterView`
  (`renderTypeFilter`, `ui.ts:272` is the starting point); click a cell → `TypeFilterMove` to it +
  `TypeFilterToggle`; Apply → `CommitTypeFilter`. No arrow-key nav needed.
- **Context menu + hover actions:** Add/Delete/Copy/Cut/Paste/Remark/Convert/Detail/Undo/Redo →
  existing intents (`AddNode`, `DeleteSelected`, `CopySelected`, `CutSelected`, `Paste`, `Remark`,
  `OpenConvert`, `ToggleDetail`, `Undo`, `Redo`); Move-up/down → `MoveSelectionTo`; per-row kind
  popover → `kind_options(path)` + `CommitKind`; inline note → `BeginEdit`/InsertComment path.
  Items enable/disable from snapshot state (Paste only when `clipboard_count`; kind item only when
  `kind_options` non-empty; read-only/opaque rows expose no mutation items).
- **Convert:** a real `<dialog>` — format `<select>` (`SetConvertFormat`), output-path `<input>`
  (`SetConvertPath`), warnings list from `ConvertView.warnings`, Convert button (`ConvertRun`) →
  confirm if warnings (`ConvertConfirm`). Host writes via the existing `doConvertWrite` path
  (`ui.ts:508`).
- **Toolbar:** Open / Save (keep `doSave`/`doOpenFs` + `updateSaveLabel` verbatim), Undo/Redo,
  theme toggle (keep `initTheme`/`toggleTheme`), format + dirty badges.
- **Keyboard accelerators (`web/keys.ts`):** keep `j/k/↑/↓`, Enter/Space toggle, `e` edit,
  `a/d/c/x/v/r`, `z/y`, `/` focuses search, `f` opens type-filter popover, `C` opens convert
  dialog, `i` detail, `?` help, `Ctrl-s`/`Ctrl-o`, Esc. These call the **same handlers** the
  pointer UI uses — accelerators, not the only path.

### Theme / CSS
Keep the `:root[data-theme]` dark/light variable system (`style.css`); add palette tokens for the
new surfaces: inputs, selects, selection highlight, marquee rectangle, context menu, `<dialog>`,
hover action buttons.

## Files

- **Edit (Rust):** `crates/confy-core/src/session/intent.rs`, `…/dispatch.rs`, `…/session.rs`;
  `crates/confy-core/tests/session_headless.rs`, `…/serde_roundtrip.rs`;
  `crates/confy-ffi/functional_smoke.mjs`.
- **Edit (web):** `web/index.html`, `web/style.css`, `web/ui.ts` (slimmed to orchestrator),
  `web/types.ts` (Intent union only).
- **New (web):** `web/render.ts`, `web/select.ts`, `web/dnd.ts`, `web/keys.ts`, `web/menu.ts`.
- **Docs:** `WEBUI.md` (new architecture + intent table), `CHANGELOG.md`, `CLAUDE.md` (module map
  for the new `web/` files + the batch intents).
- **Unchanged:** all `model/`/`edit`/backend code, the `confy-tui` crate, `web/confy.ts`,
  `web/fs.ts`, `web/build.mjs`, `web/serve.mjs`.

## Phased delivery

1. **Shell prototype (stop for review).** New `index.html` + `style.css` + `web/render.ts` +
   slimmed `web/ui.ts`: render the inline tree from snapshots, click-to-focus (`SetCursor` —
   ship this intent in phase 1 since the shell needs it), caret expand/collapse,
   toolbar/search-box/footer chrome. No editing/selection gestures yet. **Review checkpoint.**
2. **Mouse selection + drag-reparent.** Add `SetSelection` + `MoveSelectionTo` (core+tests);
   build `web/select.ts` (marquee + shift + ctrl) with live highlight and `web/dnd.ts`
   (grip-drag → `MoveSelectionTo`, illegal-move rejection surfaced from core). The detail
   side-panel chrome also lands here (reuses `ToggleDetail`/`detail_text`).
3. **Inline editing.** Add `CommitEdit` + `CommitKind` (core+tests); inline `<input>`/`<select>`
   cells; keep multiline/opaque/comment on the `external_edit` modal.
4. **Search, type-filter, actions, convert.** Add `SetFilter`, `SetConvertFormat`,
   `SetConvertPath` (core+tests); checkbox type-filter popover; `web/menu.ts` context menu +
   hover actions; convert `<dialog>`.
5. **Accelerators + polish + docs.** `web/keys.ts`; help overlay; theme palette; update
   `WEBUI.md`, `CHANGELOG.md`, `CLAUDE.md`.

(Intents land with the phase that first needs them; `serde_roundtrip` + `functional_smoke` are
extended in the same phase so the wire contract never drifts.)

## Verification

- **Core:** `cargo test -p confy-core` (new `dispatch_*` + serde-roundtrip cases),
  `cargo clippy -- -D warnings`, `cargo fmt --check`. TUI untouched → `cargo test -p confy-tui`
  stays green.
- **FFI:** in `crates/confy-ffi`, `wasm-pack build --target web`, then `node functional_smoke.mjs`
  (extended; all checks pass).
- **Web:** `node web/build.mjs` (esbuild) clean; `node web/serve.mjs` and **manually** verify per
  phase (per project memory, the user does manual TUI/web testing — no pty/long-lived bg): open a
  TOML/JSON/YAML sample; click / marquee / shift / ctrl select; inline-edit a scalar; change type
  via the `<select>`; live-search; type-filter; context-menu add/delete/copy/paste/undo; convert
  with warnings; save in place. Confirm the multiline/opaque path still opens the `<textarea>`
  modal. **Drag-reparent:** legal moves re-render; an illegal move (TOML table → array, drop onto a
  YAML opaque node) leaves the tree unchanged and shows core's `error`. **Per-format kind:** the
  kind popover offers only `kind_options(path)` — TOML radix/`"""`, none for a JSON object, YAML
  block/flow — and is disabled when empty.
- **Round-trip safety:** editing then Save reproduces a byte-identical untouched file region (the
  core's atomic-commit guarantee is unchanged — `MoveSelectionTo` reuses `Mutation::Move`; every
  other intent is a new entry point to existing machinery, no new mutation logic).
- **Open question to resolve in Phase 4:** confirm whether `Mutation::InsertComment` supports a
  *trailing*-comment insert on a live node (inline "add note"); if not, add a thin entry point.
