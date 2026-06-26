# WEBUI.md — confy Web UI & WASM FFI contract

The Web UI is the second host of the headless core (`confy-core`), alongside the
ratatui TUI. It compiles the same `Session` state machine to WebAssembly and drives
it from TypeScript. This file documents the FFI boundary and the UI architecture; the
shared model glossary lives in `CONTEXT.md`, nested behavior in `BEHAVIOR_MATRIX.md`,
TUI mechanics in `TUI.md`. The port design record is `PORTING.md` (§8 records the
Stage-2 transport decisions).

## Architecture

```
                 ┌──────────────────────────────────────────┐
                 │              confy-core (Rust)            │
                 │   model  +  session (Session, dispatch)   │
                 └─────────────────┬────────────────────────┘
                                   │  pure Rust, fs-free, no I/O
                  ┌────────────────┴─────────────────┐
                  │           confy-ffi (Rust)        │   wasm-bindgen +
                  │   ConfySession wrapper + serde    │   serde-wasm-bindgen
                  └────────────────┬─────────────────┘
                                   │  wasm32-unknown-unknown
                  ┌────────────────┴─────────────────┐
                  │        TypeScript integration     │   generated .d.ts +
                  │     (confy.ts — typed handle)     │   hand-written types
                  └────────────────┬─────────────────┘
                                   │
                  ┌────────────────┴─────────────────┐
                  │       Web UI (DOM, web-native)    │   render.ts tree +
                  │  index.html / ui.ts / render.ts / │   select.ts pointer +
                  │  select.ts / dnd.ts / style.css   │   dnd.ts drag → Intent
                  └──────────────────────────────────┘
```

One command channel: every gesture (keyboard **or pointer**) becomes one `Intent`, the
UI calls `ConfySession.dispatch`, and re-renders the whole DOM from the returned
`SessionSnapshot`. No editor logic lives in the UI — it is stateless w.r.t. editing.

The UI is a **web-native** port of `design_index_model.html` (a visual/UX mockup); the
mockup's self-contained JS model is discarded — `confy-core`'s `Session` is the single
source of truth. `style.css` is the design's `<style>` block **verbatim** plus a fenced
app-only appendix, so the visual layer cannot drift from the design.

## FFI API surface (`confy-ffi`)

A single `wasm-bindgen` class, `ConfySession`, wraps a `confy_core::session::Session`.
All cross-boundary values use `serde-wasm-bindgen` so the Rust `Serialize`/`
Deserialize` derives (Phase E + Stage 2 §8.1) are the wire contract — there is no
hand-maintained field-by-field marshalling.

| Method | Signature (TS) | Notes |
|---|---|---|
| `ConfySession.fromText` | `(text: string, format: DocFormat) => ConfySession` | constructor; parses via `AnyDocument::from_str_as`. Throws on parse error. |
| `dispatch` | `(intent: Intent) => SessionSnapshot` | the one command channel (§8.4). |
| `snapshot` | `() => SessionSnapshot` | re-pull the full renderable state without mutating. |
| `visibleRows` | `() => ViewRow[]` | convenience; subset of `snapshot`. |
| `serialize` | `() => string` | current document text (host writes/downloads). |
| `isDirty` | `() => boolean` | |
| `docFormat` | `() => DocFormat` | |
| `kindOptions` | `(path: Seg[]) => KindOption[]` | per-node convertible kinds (drives the `K` popup). |
| `externalEdit` | `() => { initial, kind } \| undefined` | the current external-edit request, if any (§8.2). |

`external_edit` in the snapshot is the async handshake (§8.2): the UI opens its
multi-line modal with `initial`, awaits the result, and re-dispatches
`ApplyReplace { path, text }` / `ApplyEditComment { path, text }`.

## TypeScript-facing data model

All types mirror the Rust serde representation exactly (snake_case field names). The
hand-written `types.ts` is the canonical reference for the UI; it is kept in sync with
the Rust derives by the `serde_roundtrip` + `dispatch` native tests (they assert the
shapes round-trip). Key types:

- **`Intent`** — every action (navigation, selection, filter, type-filter, kind-switch,
  convert, edit, mutations, undo/redo, lifecycle). The UI maps a DOM keyboard event **or
  pointer gesture** to one `Intent`. The web-native UI adds a set of purely-additive
  **batch intents** that take a whole value at once (the pointer analogue of the
  incremental keyboard intents), each reusing existing core machinery and needing no FFI
  plumbing: `SetCursor(Path)`, `SetSelection {paths}`, `MoveSelectionTo {sources,target,
  index}` (drag-reparent — a one-shot cut→paste reusing `Mutation::Move`),
  `CommitEdit {value?,name?}`, `CommitKind {path,target}`, `SetFilter(String)`,
  `SetConvertFormat(DocFormat)`, `SetConvertPath(String)`.
- **`SessionSnapshot`** — full renderable state: `mode: ModeView`, `rows: ViewRow[]`,
  `cursor: Seg[]`, `status`/`error`, `detail_text`, `external_edit`, `convert_write`,
  `clipboard_count`, `quit`, `doc_format`, `is_dirty`.
- **`ModeView`** — a serializable projection of `Mode` + the modal edit surfaces:
  `Normal | Prompt | Filter {text,cursor} | FilterResults | TypeFilter {…grid…} |
  KindSwitch {cursor,options} | Convert {…} | Detail | Help | Edit
  {field,buffer,cursor,…}`. This is the UI's only view of internal state; heavy
  internals (`History`, `Clipboard` — except its `clipboard_count`) never cross.
- **`TypeFilterView`** — the `f` popup grid, projected from core so the host never
  re-derives the per-format facet set: `rows: (Header | Cells[…])[]`, `cursor_row`,
  `cursor_col`, `active`. Each cell carries `label`, tri-state `state`
  (`On`/`Partial`/`Off`), and `is_cursor`.
- **`ViewRow`** — one visible tree row (`path`, `depth`, `is_branch`, `key`, `value`,
  `scalar_type`, `format`, `type_label`, `child_count`, `trailing_comment`, `read_only`,
  `selected`, `is_cursor`). `type_label` (the core node-kind label) and `child_count` let
  the web render the true container kind + item count instead of guessing from `is_branch`.
- **`Seg`** = `{ Key: string } | { Index: number }`; **`Path`** = `Seg[]`.

## Serialization format decisions

- **Serde + serde-wasm-bindgen**, not `wasm_bindgen` getters. One derive per type;
  the wire format is JSON-shaped JS values. Adding a Rust field is the only change
  needed; no FFI plumbing.
- **Full-state snapshot, no diff** (§8.3). Each `dispatch` returns the entire tree +
  modal state; the UI re-renders wholesale. A structured row diff is a future G2
  optimization, not present now.
- **Async editor via signal, not callback** (§8.2). The sync `Host` trait is a TUI
  concern; WASM uses `externalEdit` in the snapshot + a follow-up `ApplyReplace`/
  `ApplyEditComment` intent, so the browser modal can be `Promise`-based.

## Web UI architecture

- **Stateless render.** The UI keeps no editor state of its own; it holds the latest
  `SessionSnapshot` and renders the DOM from it. Every interaction is `dispatch`. The one
  non-editor UI-local bit of state is the **Tree|Raw view toggle** (see below).
- **Row anatomy (`render.ts`).** A pure `SessionSnapshot → DOM` function draws the design's
  web-native row: drag grip, rotating disclosure caret, key (or faint `[i]` for positional
  elements) / `=` / value (value-type colored) or item count, a per-row **kind badge**
  (friendly label + notation suffix + chevron — `table·scope`/`table·dotted`/`array·multi`,
  YAML `·block`/`·flow`, scalar `·"…"`/`·0x`/`·1e`/…), comment/trailing decoration, and
  hover `＋`/`⋮` action buttons. Each row carries `data-path` (attribute-safe JSON) so the
  pointer layer maps a click back to a node without re-deriving structure.
- **Pointer selection (`select.ts`).** Pure logic resolving a click into the next full
  selection set → `SetSelection`: plain click = that row; ⇧-click = contiguous range from
  an anchor, **unioned onto a base snapshot** so earlier segments survive (segmented
  multi-select); ⌘/Ctrl-click = toggle without clearing (and re-anchor). A marquee
  rubber-band selects every row it intersects. The clicked end is kept last so core's
  cursor follows it. Plain `j/k`/arrow nav collapses the selection onto the new cursor.
  A **double-click on a row's empty area** (detected manually by timing two plain body
  clicks on the same path — native `dblclick` is unreliable after the first click
  re-renders) toggles: a branch expands/collapses, a boolean leaf flips its value. Only
  empty-space clicks reach it (key→rename, value→edit, caret→expand all return first).
  With a **multi-selection**, `Enter` toggles every selected branch independently (cursor-walks
  the selected branch rows dispatching `ToggleExpand`, then restores the selection); a single
  selection keeps the plain cursor toggle.
  Navigation keys (`←→↑↓`, Home/End, Space) `preventDefault` so the browser's native
  arrow-scroll can't drag the off-canvas detail panel into view (`.main` is also
  `overflow:hidden` as a backstop).
- **Drag-reparent (`dnd.ts`).** HTML5 grip drag → `MoveSelectionTo`: `dragover` computes
  whether the pointer is over a branch's middle (drop **into** it, `.drag-over-into`) or a
  gap (drop before/after a sibling, shown by a horizontal `#dropLine`); a self-subtree drop
  is rejected. Sibling index is the child's visible position (== core's full-child index);
  core's `Move` self-adjusts for removed earlier siblings, so feed original indices.
- **Inline edit / kind / context.** Clicking a value → a live `<input>` (seeded from the
  edit buffer, Enter/blur → `CommitEdit`); a key → a rename input; the kind badge → a
  popover built from `kindOptions(path)` → `CommitKind`; `＋` → `AddNode`; `⋮`/right-click →
  a context menu. All popovers share one synchronous closer (a single outside-click
  listener) and are scoped per popover so they don't open/close together. **Every menu
  button toggles** — a second click on the `⋯` More button (tracked by `.open`) or the
  per-row `⋮` (tracked by `ctxMenuPath`, the ctx-menu analogue of `kindMenuPath`) closes
  the menu, matching the already-toggling type-filter button and kind badge. **Every popup
  closes on Esc** — the click-menus via `anyClickMenuOpen`, `#tfPop`/overlay/`#convDlg`/
  inline editor/external-edit modal each in their own path, and the load-modal via its own
  keydown handler (it early-returns from `onKey`, so it needs one). A **comment row's
  click target is the text only** (`.comment-row .comment` is `flex:0 1 auto` — no grow — so
  the empty space past the text no longer opens the editor; shrink is retained for the
  narrow-width ellipsis). **"Append sibling" on a comment** (context menu / `AddSibling`)
  inserts a *separate* single-line comment and opens it in the inline editor; **Esc removes
  it** — the core `add_comment_sibling` path (blank-separated node + `created_on_add`).
- **Native modal widgets replace the keyboard overlay.** The always-visible **search box**
  owns the filter text (debounced `input` → `SetFilter`, `/` focuses it — no `Mode::Filter`
  is ever entered; search now matches **scalar values**, not just keys/paths/comments). `f`
  renders the `TypeFilterView` grid into the native `#tfPop` popover (tri-state cells; cell
  click = `TypeFilterMove`+`TypeFilterToggle`; Apply/Cancel). The **Save button** (and `C`)
  opens the native `#convDlg` as one unified **Save / Convert** panel: its format `<select>`
  defaults to the current format with the filename prefilled from the open file's stem. Same
  format → a faithful "Save copy" of `serialize()`; a different format → the convert flow
  (`SetConvertFormat`/`SetConvertPath` → `ConvertRun`→`ConvertConfirm`; a lossy convert is a
  non-fatal warning + second confirm, not a failure). `⌘S` stays the instant in-place save
  (the panel is for save-as/convert). `Detail` is a slide-in
  aside. The keyboard `#overlay` now serves **only** Help / Prompt / KindSwitch. The
  body-keydown accelerator guard skips `INPUT`/`TEXTAREA`/`SELECT` so typing in a widget
  isn't routed as navigation.
- **Tree | Raw view.** A segmented toggle flips the main pane between the interactive tree
  and a **read-only** `<pre>` of `session.serialize()` — the live document (unsaved edits
  included), re-serialized on every render so it never drifts. Read-only first: no in-Raw
  editing, so Save still serializes from the Session (always valid); an editable Raw tab +
  save-time format guard is a later step.
- **Paste mode.** While the clipboard holds a cut/copy the selection is frozen, so a row
  click moves the **cursor** (= the `After(cursor)` paste target) via `SetCursor`, and a
  `body.paste-mode` class marks the cursor row as a visible "▸ paste here" target.
- **Help.** The `?` overlay appends a **per-format KIND legend** (`KIND_LEGEND`, keyed by
  `doc_format`, ported from the TUI's per-backend help) explaining each container/scalar
  label·notation for the open file's format.
- **External edit modal.** When `snapshot.externalEdit` is set, a `<textarea>` modal opens
  with `initial`; on submit the UI dispatches `ApplyReplace`/`ApplyEditComment` with the
  request's path and the edited text.
- **File I/O — File System Access API with download fallback.** All file I/O is
  host-owned (`web/fs.ts`); core `Intent::Save` only clears the dirty flag. The toolbar
  **Save** button opens the Save / Convert panel (above); **`⌘S`** is the instant
  in-place fast path with this precedence: (1) write in place to the open
  `FileSystemFileHandle`; (2) if the API is
  available but no handle is held, `showSaveFilePicker` Save-As (and the handle is
  kept so subsequent saves are in place); (3) download (Firefox/Safari/older
  browsers). `Ctrl-o` / Open opens a real file via `showOpenFilePicker`; the Load
  button (paste-into-textarea) is the always-available fallback. The "Open…" button is
  hidden on browsers without the API. Convert output routes through Save-As when
  available, else download. The capability is detected once at boot and isolated
  behind `web/fs.ts`; no editor logic depends on it.
- **Theme.** A dark/light toggle (titlebar `☾`/`☀`) flips `:root[data-theme]`; CSS
  variables carry both palettes and the choice persists in `localStorage`.
- **Responsive toolbar.** The toolbar holds a single right-side action button (**Save**,
  opening the Save / Convert panel — the separate Convert button is gone). As the window
  narrows, secondary controls fold into a `⋯ More` popup one group at a time, right→left, via
  staged media queries (Tree/Raw ≤600px, Expand/Collapse ≤500px, Undo/Redo/Theme ≤440px);
  the More popup always lists the full action set. The search box has `min-width:96px` (well
  below its content size) so it yields space to those buttons before they collapse.
  **Rows stay single-line at every width:** they never wrap or hide cells — long key/value/
  comment compress with an ellipsis (`min-width:0` lets `text-overflow:ellipsis` fire inside
  the flex row). The **value compresses first**; the **key keeps its full width**
  (`.key{flex-shrink:0}`, truncating only past its `max-width:38vw` cap). Full text remains in
  the detail panel (`i`).

## Touch UI (dedicated `web/touch/` module)

The touch experience is **not** the desktop UI with gestures bolted on — that was tried and
rejected as low-fidelity. Instead `web/touch/` is a **separate, prototype-faithful UI** that
ports `docs/superpowers/specs/2026-06-26-web-respons-migrate-to-touch-ready.html` verbatim in
look & gesture, but drives the **same `confy-core` Session** through the shared
`confy.ts`/`Intent` contract — exactly how the desktop UI relates to the core. The prototype's
only discarded part is its fake `TREE`/DOM-as-state model; everything mutating goes through
`session.dispatch(Intent)` + a full re-render from the returned `SessionSnapshot` (stateless,
like desktop). The two UIs share **only** `confy.ts`, `types.ts`, `fs.ts`, and the Intent
contract; neither compromises the other.

**Entry selection — one URL, two pages.** `index.html`'s `<head>` runs a tiny router before any
paint: `?ui=desktop` stays on desktop; `?ui=touch` or `matchMedia('(pointer:coarse)').matches`
→ `location.replace('touch.html')`. `touch.html` carries the reverse guard (fine pointer without
`?ui=touch`, or `?ui=desktop`, → back to `index.html`). A two-page redirect (not in-page
DOM-swap) is used because the desktop `body{display:flex;flex-direction:column}` + `.main{flex:1}`
assume toolbar/main/footer are direct body children — wrapping them would break layout and force
edits to the verbatim desktop CSS.

**Files** (`web/touch/`):
- `touch.html` — minimal shell: reverse-guard redirect, `<link>` to `touch/style.css`, a `#root`
  mount, the `fileInput` open-fallback, and `<script type=module src=./touch/app.js>`.
- `touch/style.css` — the prototype's `<style>` block **verbatim**, minus the showcase device-frame
  rules (`.stage`/`.frames`/`.device`/`.os-status`); two adaptations follow from dropping the
  frame: `body` fills the viewport (positioned ancestor for `.app`) and `.app` inset goes 46px→0
  (the space the fake OS status bar occupied). Mirrors the desktop "CSS = design verbatim" rule.
- `touch/render.ts` — pure `SessionSnapshot → HTML`. Ports the prototype's row anatomy (caret /
  key / `=` / typed value / count / kind badge / comment / grip / swipe actions) but every row is
  a real `ViewRow`; flat list (the snapshot is the visible-row projection, so collapsed branches
  omit descendants — no `.children` nesting), root row skipped, `data-path` attribute-safe.
- `touch/app.ts` — orchestrator: boots the Session (`load` + `Session.fromText`), generates the
  shell (ported `appHTML`), renders snapshots, and re-points every gesture to an Intent.

**Gesture → Intent map** (all through the stateless dispatch loop):
- caret tap → `SetCursor` + `ToggleExpand`; row tap → `SetCursor` + `SetSelection` + open Detail.
- kind badge tap → kind sheet from `session.kindOptions(path)` → `CommitKind {path,target}`.
- grip drag (ported pointer geometry, before/after/into + `.reorder-line`/`.drop-into`) →
  `MoveSelectionTo {sources,target,index}` (sibling index = visible position, as in `dnd.ts`;
  the dragged row's own subtree is excluded from drop candidates by path-prefix).
- left-swipe row → **Edit** (open Detail), **Dup** (`CopySelected` + `Paste` — a real duplicate;
  there is no dedicated duplicate Intent), **Delete** (`DeleteSelected`).
- Detail (bottom sheet `<600px`, persistent side pane `≥600px` via `@container`): key →
  `CommitEdit {name}`, value → `CommitEdit {value}`, trailing comment → `SetTrailing`, standalone
  comment node → `ApplyEditComment`; kind button → kind sheet.
- Menu sheet → `Open`/`OpenConvert`/`Redo`/`ExpandAll`/`CollapseAll` + theme toggle (`fs.ts` for I/O).
- Type-filter & Convert sheets are built from `snapshot.mode` (`TypeFilterView` / `ConvertView`),
  never local UI state: chips dispatch `TypeFilterMove`+`TypeFilterToggle`; convert dispatches
  `SetConvertFormat`/`SetConvertPath`/`ConvertRun`/`ConvertConfirm`, and the resulting
  `convert_write` snapshot field is written via `fs.ts`.
- FAB → `AddNode` (parameterless, like the desktop `a` key — the prototype's add-type sheet is
  dropped; the new node's kind is changed afterwards via the kind badge/detail).
- search input → debounced `SetFilter`; Tree/Raw tabs are a view toggle (`session.serialize()`).
- **Read-only / opaque rows** (`ViewRow.read_only`) render without grip/kind/swipe and reject
  edits — mirroring core. Multi-line value/comment edits route to an external-edit sheet
  (`ApplyReplace`/`ApplyEditComment`), the same handshake the desktop uses.

`web/build.mjs` emits both bundles: `ui.ts → ui.js` (desktop, unchanged) and `touch/app.ts →
touch/app.js`.

## Future structured-diff evolution

The full-snapshot transport is the G1 baseline. If re-render latency becomes
measurable on large files, G2 introduces a structured row diff without changing the
`Intent` contract:

1. Add `Update { rowsDirty, … }` (already exists, Phase E) as an optional
   `delta` field on `SessionSnapshot`, or a sibling `dispatchDelta` entry point.
2. Ship a row identity keyed by `Path` (already stable across mutations — that is
   what the §3 reshape bought) so the UI can patch only changed/added/removed rows.
3. Keep `snapshot()` as the full-state fallback for resync.

No diff scaffolding is built now; the `Path`-keyed `ViewRow` is already the identity
the diff would key on, so the upgrade is additive.
