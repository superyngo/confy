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
  edit buffer, Enter/blur → `CommitEdit`, **sized to its content** — `editWidthCh` seeds a
  `width:…ch` and an `input` listener grows it while typing, CSS min/max-width clamping);
  a key → a rename input; the kind badge → a
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
- **Confirm prompts are buttons** (shared `web/prompt.ts`). `Mode::Prompt` renders per-kind
  answer buttons (`data-pk` → `PromptKey`): Yes/Cancel pairs for TypeChange / ArrayUpgrade /
  JsoncUpgrade / ConfirmQuit, and Overwrite / Rename / Cancel for a paste Collision. The
  question line is `snap.status ?? snap.error` with the TUI's trailing key legend stripped
  (`promptQuestion`), with per-kind fallbacks for prompts core raises without a status. Desktop
  keeps the keyboard path (y/n/Enter/Esc, plus o/r for Collision) alongside the buttons; the
  **touch UI renders a prompt bottom sheet** (`.prompt-sheet`) whose scrim/×/grab dismissal
  answers `n` — a prompt is always *answered*, never just hidden (peel-on-dismiss). The desktop
  detail aside stays open underneath a prompt (`renderDetailPanel` leaves `.open` untouched on
  `Prompt`), and the core returns to `Mode::Detail` when a panel-origin prompt resolves.
- **Tree | Raw view.** A segmented toggle flips the main pane between the interactive tree
  and a **read-only** `<pre>` of `session.serialize()` — the live document (unsaved edits
  included), re-serialized on every render so it never drifts. Read-only first: no in-Raw
  editing, so Save still serializes from the Session (always valid); an editable Raw tab +
  save-time format guard is a later step.
- **Paste mode.** While the clipboard holds a cut/copy the selection is frozen, so a row
  click moves the **cursor** (= the `After(cursor)` paste target) via `SetCursor`, and a
  `body.paste-mode` class marks the cursor row as a visible "▸ paste here" target.
- **Pointer value gestures.** A **double-click on a row _toggles_ the Detail panel** for it
  (`SetCursor` + `ToggleDetail`); it no longer toggles branch-expand/boolean-value (expand stays
  on the caret + Enter). **Mouse-wheel over the value cell** (`[data-edit="val"]`) adjusts it in
  place: a `Bool` toggles true↔false, an `Integer`/`Float` nudges ±1 (`Nudge`, wheel up = +1) —
  `preventDefault` fires only over an adjustable value so other rows scroll normally. The keyboard
  `+`/`-` and `←`/`→` Nudge keys are unchanged. The **same wheel-adjust works on the shared
  panel's value field** (`web/panel.ts`), so it applies in the desktop Detail aside and the touch
  edit sheet too. The shared panel's actions are **Copy / Cut / Delete**; on success each fires the
  panel's optional `afterMutation` callback so the host **confirms (message) and dismisses** the
  panel (desktop `ExitDetail`, touch `closeSheets`). Panel **key/value edits are one-shot
  commits** (`CommitEdit`): success and failure both resolve back to the Detail panel (core
  `commit_edit` epilogue — no dangling `Mode::Edit`/tree editor; a **branch node's rename is
  rename-only**, skipping the value-replace step a branch has no scalar value for), and **Esc
  cancels** any panel input (original text restored, blur-commit swallowed — the browser's own
  no-change-if-value-unchanged behavior means blur doesn't re-fire a commit, so no extra
  bookkeeping is needed). Both **Enter and Escape `stopPropagation()`** on a panel input: without
  it, committing on Enter can synchronously open a confirm prompt (type change, paste collision)
  whose y/n the host's global keydown handler reads straight off that *same* bubbling Enter,
  auto-answering before the prompt is ever visible. A **multi-line value/comment renders as a
  button** that opens the host popup editor (`BeginEdit` → external edit) instead of a one-line input,
  and its one-line preview is **truncated to the cell** (ellipsis). The **Kind button shows
  `type · «notation»`** (a short glyph, dropped when it would just repeat the label). The panel's
  commit handlers read the input value **before** dispatching `SetCursor` (which rebuilds the panel
  DOM and detaches the input), and the separate **trailing-comment cell sends raw text** —
  `Session::set_trailing_comment` prepends the backend's marker (`#`/`//`) when missing.
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
- **`?url=` deep-link.** Appending `?url=<encoded-url>` to the page URL opens that
  remote config at boot (priority: Tauri startup file → `?url=` → built-in sample).
  `fetchUrlFile` in `web/fs.ts` fetches the URL, derives a display name from the last
  path segment, and infers the format from the filename extension (falling back to the
  HTTP `Content-Type` header, then defaulting to TOML). The file opens with no on-disk
  handle, so Save falls back to Save As / download — identical to the file-input path.
  No CORS proxy is included; the remote server must send permissive CORS headers.
  An explicit **"Open from URL"** entry point feeds the same `openFromUrl`: the desktop ⋯ More
  menu opens a `#url-modal`, the touch More-actions sheet opens a `.url-sheet`. The local-file
  Open button keeps its meaning (host file picker only).
- **Theme.** A dark/light toggle (titlebar `☾`/`☀`) flips `:root[data-theme]`; CSS
  variables carry both palettes and the choice persists in `localStorage`.
- **Responsive toolbar.** The toolbar holds a single right-side action button (**Save**,
  opening the Save / Convert panel — the separate Convert button is gone). As the window
  narrows, secondary controls fold into a `⋯ More` popup one group at a time, right→left, via
  staged media queries (Tree/Raw ≤600px, Expand/Collapse ≤500px, Undo/Redo/Theme ≤440px);
  the More popup lists the folded secondary actions but **not** Save / Convert (that lives only on
  the always-visible Save button, so it is never duplicated). The search box has `min-width:96px` (well
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
like desktop). Beyond the core (`confy.ts`, `types.ts`, `fs.ts`, the Intent contract), the two
UIs now share several **single-source UI modules** so look & behavior can't drift: `web/panel.ts`
(node edit/detail panel), `web/convert-dialog.ts` (the Save / Convert form), and
`web/typefilter.ts` (the type-filter grid). `convert-dialog.ts` is **container-agnostic** — it
operates over a host-supplied `ConvertSurface` (`isOpen/open/close/onCancel`), so desktop hosts the
form in a native `<dialog>` while touch hosts the **same form in a bottom `.sheet`** (all touch
panels share one mechanism). Each emits desktop's class names; the CSS that styles
them lives per-page (desktop's verbatim block; touch's app-only appendix). The touch chrome
(header + search bar) was rebuilt to **mimic desktop's** toolbar/filterbar after the bespoke
app-bar was judged worse — so the surfaces converge while the tree-body gestures stay touch-first.

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
  An **app-only appendix** (below the verbatim block) carries the converged chrome styling: the
  desktop-shaped toolbar/filterbar, the shared `<dialog>`/`.tf-*` rules, and the one-line
  `.detail .kindbtn` fix. The prototype rules it superseded (`.appbar`/`.searchbar`/`.tabs`/`.tab`/
  `.tapbtn`/`.filter-btn`/`.brand .doc`) were removed from the verbatim block as dead code.
- `touch/render.ts` — pure `SessionSnapshot → HTML`. Ports the prototype's row anatomy (caret /
  key / `=` / typed value / count / kind badge / comment / grip) but every row is a real
  `ViewRow`; flat list (the snapshot is the visible-row projection, so collapsed branches omit
  descendants — no `.children` nesting), root row skipped, `data-path` attribute-safe. The
  prototype's right-side branch `>` chevron is dropped. Each non-read-only row carries a hidden
  `.row-del` button behind `.row-main`, revealed by a **left-swipe-to-delete** gesture (see below).
- `touch/app.ts` — orchestrator: boots the Session (`load` + `Session.fromText`), generates the
  shell (ported `appHTML`), renders snapshots, and re-points every gesture to an Intent.

**Gesture → Intent map** (all through the stateless dispatch loop):
- caret tap → `SetCursor` + `ToggleExpand`. **Single row tap = select only** (`SetCursor` +
  `SetSelection`, no sheet); **double tap (same path within ~300 ms) = open the edit panel**. The
  kind badge tap behaves as a normal row tap (select) — kind switching happens only inside the
  panel. A **tap on empty tree space** (the `.tree-pane` padding below the last row — outside
  the pointer-gesture rows' own bounds, so it needs its own plain `click` listener on
  `.tree-pane`) clears the multi-select and any error banner, mirroring desktop
  `onTreeClick`'s empty-area branch.
- grip drag (ported pointer geometry, before/after/into + `.reorder-line`/`.drop-into`) →
  `MoveSelectionTo {sources,target,index}` (sibling index = visible position, as in `dnd.ts`;
  the dragged row's own subtree is excluded from drop candidates by path-prefix). Swipe is gone;
  reorder is grip-only.
- **Edit panel** (bottom sheet `<600px`, persistent side pane `≥600px` via `@container`): rendered
  by the shared `web/panel.ts` (`panelHTML` + `wirePanel`) — the same module the desktop detail
  aside uses, so both UIs show one locked field order **Key / Value / Trailing comment / Kind /
  Path / Children / Sign** (Path is the human dotted/bracketed form, e.g. `servers[1].port`; Sign
  from `ViewRow.key_sign`). Key → `CommitEdit {name}`, value → `CommitEdit {value}`, trailing →
  `SetTrailing`, comment node → `ApplyEditComment`, kind button → kind sheet, **Delete** →
  `SetCursor`+`SetSelection`+`DeleteSelected`, **Copy** → `…`+`CopySelected`, **Cut** →
  `…`+`CutSelected` (Copy/Cut arm the clipboard; the FAB pastes — see below). After each
  dispatch `wirePanel` surfaces `snapshot.error` via the host toast (the panel buttons were dead in
  the first cut — never wired — so failures are now reported, not silent).
- on `≥600px` the persistent side pane has a **draggable splitter** between the tree and detail
  panes: it sets a `--detail-w` flex-basis on `.app` (clamped ~240–520 px) persisted to
  `localStorage` (`confy-detail-w`); hidden `<600px` and in Raw view.
- **Responsive chrome collapse + dynamic menu.** The `.app` is a `container-type:inline-size`
  container; toolbar/filter buttons stay single-line (`nowrap`) and **fold into the `⋯` menu
  right→left** via `@container` breakpoints (≤720 viewtabs → ≤620 expand/collapse `.nav-grp` →
  ≤520 undo/redo/theme `.edit-grp`); the `⋯` button is hidden until the first fold. The **menu
  sheet is built dynamically** (`MENU_CANDIDATES` + `isFolded` = `offsetParent === null`) from
  whichever controls are currently folded — not a hardcoded list — so it always mirrors the
  breakpoints. Open/Save stay visible (never in the menu).
- **Type filter & Save/Convert use the shared modules** (`web/typefilter.ts` / `web/convert-dialog.ts`),
  the same code + markup desktop uses: the type-filter grid renders into the filter sheet via
  `typeFilterHTML`+`wireTypeFilter` (no "Done" button — the grid toggles live + has a `Clear`
  button, and
  the sheet closes via grab/scrim/header-×); Save/Convert renders the shared form into a
  **bottom `.sheet`** (not a `<dialog>`) via a sheet-backed `ConvertSurface`. Both are driven by
  `snapshot.mode` (`TypeFilterView` / `ConvertView`); the `convert_write` snapshot field is written
  via `fs.ts`. `dismissSheets` peels each mode on close (TypeFilter→`CommitTypeFilter`,
  Convert→`ExitConvert`, external-edit→`Escape`) so the next render doesn't re-open it.
- **FAB is context-aware** (like the TUI `a`): when the cursor row is an expanded branch → `AddChild`;
  otherwise (scalar or collapsed branch) → `AddSibling` (falls back to parameterless `AddNode` with
  no cursor row). When the **clipboard is armed** (`clipboard_count > 0`, after panel Copy/Cut) the
  FAB switches to a **paste glyph tinted by copy vs cut** (`clipboard_cut`) and a tap dispatches
  `Paste` at the cursor; tapping the status-bar clipboard badge clears it (`Escape`).
- **Swipe-to-delete.** A left-swipe on a row's `.row-main` slides it open to reveal a single
  red Delete action (`.row-del`); one row is open at a time. The pointer flow **locks the axis**
  (horizontal >8px & > vertical → swipe; vertical → scroll/tap-cancel) so it coexists with grip-drag
  reorder and list scroll; read-only rows opt out (no `.row-del`). The open row's transform is reset
  on the next full re-render (the tree `innerHTML` is rebuilt), so a Delete (or any tap) closes it.
- the **Save button** (single plain `.tbtn`, not a split-button pill — see Mobile section below
  for why that design was tried and reverted) always opens a small **save-choice sheet**
  (`openSaveSheet`, same anatomy as the language/menu sheets) offering "Save" (→ `doQuickSave`,
  writing in place to the open handle) and "Save As / Convert…" (→ the shared Save/Convert dialog
  via `SetCursor []`→`OpenConvert`→seed `SetConvertPath`, gated by `canSaveAs()` same as desktop).
  There is no separate direct-save toolbar action — both choices live behind the one button/sheet.
  The **format pill** cycles the built-in sample's dialect TOML→JSON→YAML while in sample mode
  (frozen once a real file is opened/saved), matching desktop — it no longer opens convert.
- search input → debounced `SetFilter`; a single **Tree/Raw toggle button** (`.viewtoggle`, label =
  the view it switches to) flips the view (`session.serialize()`) and folds into the `⋯` menu.
- **Read-only / opaque rows** (`ViewRow.read_only`) render without grip/kind and reject edits —
  mirroring core. Multi-line value/comment edits route to an external-edit **bottom sheet** (in
  `.app`, standard sheet chrome) via `ApplyReplace`/`ApplyEditComment` — the same handshake the
  desktop uses. Dismissing it (scrim/grab/×/Cancel) sends `Escape` to peel core's pending edit, so
  the sheet can't re-pop on the next render.
- the initial sample document is the **same welcome sample as the desktop UI** (shared, build-stamped).

`web/build.mjs` emits both bundles: `ui.ts → ui.js` (desktop, unchanged) and `touch/app.ts →
touch/app.js`.

**Shared edit/detail panel — `web/panel.ts`.** A framework-free module (`panelHTML(row)` +
`wirePanel(container,row,send,openKind,onError,afterMutation?)`) that renders the node edit/detail panel for
**both** UIs from a `ViewRow`, guaranteeing the field set + order can't drift between touch and
desktop. On the desktop side the detail `<aside>` (toggled with `i`/Space) now renders this panel
**reactively** — it tracks the cursor row on every snapshot and is fully editable — instead of the
old static `detail_text` `<pre>` dump (that flat string is now only the empty-doc fallback). To
feed the panel's Sign field, core's `ViewRow` gained a `key_sign` field (`"bare"|"quoted"|"dotted"
|"none"`, the same mapping the TUI detail text uses).

## Language / i18n (Web)

`web/i18n.ts` imports both root catalog files (`../i18n/en.json`, `../i18n/zh-TW.json` —
esbuild bundles JSON imports natively) and exposes `t(key)`/`tArgs(key, args)` with the same
en-fallback chain as core's `tr`/`tr_args`, plus `getLang()`/`setLang()` persisted in
`localStorage["confy-lang"]` (mirrors the `confy-theme` pattern). First-run default sniffs
`navigator.language` (`zh*` → `zh-TW`, else `en`). After session load and on every selector
change, the host sends `{ SetLang: lang }` so core-produced `SessionSnapshot` strings (status,
errors, detail fields) match; a selector change also re-runs `applyStaticI18n()` to refresh
`data-i18n`-tagged static DOM strings in `index.html`. The selector lives next to `btnTheme` in
the desktop toolbar and in the touch ⋯ menu (same shared-module rule as the rest of the
touch UI — see *Touch UI* above). `web/help-content.ts`'s `HELP_TEXT`/`KIND_LEGEND` cheatsheet
and `helpBodyHTML`'s About body both branch on `getLang()`; the About body appends
`web.about.language` and a `web.about.storage` line noting the preference lives in the
browser's local storage (or the desktop app's WebView persistent storage) rather than a
filesystem path — unlike the TUI, which discloses a config-file path (see `TUI.md` §*Language
/ i18n (TUI)*).

## Desktop menu (Tauri)

`web/menu.ts` builds a native File/Edit/View/Help menu bar for the Tauri desktop shell via
`window.__TAURI__.menu`/`window.__TAURI__.webview` (`withGlobalTauri: true` in
`tauri.conf.json`, so no `@tauri-apps/api` npm dependency — minimal ambient types follow the
`fs.ts` `TauriCore` pattern). `setupAppMenu(deps)` is a no-op on the pure web build
(`isTauri()` guard) and is called from the top of `ui.ts`'s `main()`, **before** `await
load(wasmUrl)` and **not awaited** — menu construction is several async IPC round-trips and
must not delay the wasm boot; this also means the menu is visible during the startup gap, and
Quit/About use `PredefinedMenuItem` so they still work if wasm init fails. `rebuildMenu()`
rebuilds and reinstalls it (`setAsAppMenu()`) on language change and after every recent-files
mutation, re-reading labels via `t()`, the recent list, and `getLang()` each time; an in-flight
flag drops concurrent rebuilds.

**Structure:** File (New `CmdOrCtrl+N` — discards the current doc and loads the default toml
sample, i.e. `loadSample("toml", openSample)`, the same fallback `main()` takes with no
startup file/URL; no confirmation, matching a browser refresh / Open `CmdOrCtrl+O` / Open
Recent ▸ dynamic submenu / Save `CmdOrCtrl+S`),
Edit (native `Predefined` Cut/Copy/Paste/Undo/Redo/SelectAll acting on focused text fields,
plus node-op items Undo/Redo/Copy/Cut/Paste Node), View (Toggle Theme / Zoom In-Out-Reset /
Language ▸ one `CheckMenuItem` per `availableLangs()`, checked = `getLang()`), Help (Help /
About — both send `EnterHelp`, About additionally sends `ToggleHelpTab` to flip onto the About
tab, mirroring `enter_help`/`toggle_help_tab` in `session.rs`). macOS gets a rebuilt app
submenu ("About confy"/Hide/HideOthers/ShowAll/Quit) since `setAsAppMenu()` replaces the
entire default menu bar including Cmd+Q; "About confy" is a custom `MenuItem` (not
`Predefined`) using the same `EnterHelp`+`ToggleHelpTab` handler as the Help menu's About, so
it opens the in-app About overlay instead of macOS's native About panel — one consistent
About surface across platforms. Windows has no app submenu, so a `Predefined` Quit sits at
the bottom of File instead (`navigator.platform`/`userAgentData` check).

**`PredefinedMenuItem.item` gotcha:** every predefined kind is a plain Rust unit variant
serialized as a bare string (`"Quit"`, `"Hide"`, …) — **except** `About`, which the Rust side
models as a newtype variant carrying `Option<AboutMetadata>` and must be sent as
`{ item: { About: null } }`; a bare `"About"` string fails IPC deserialization
(`invalid type: unit variant, expected newtype variant`). This is moot now that the app
submenu's About is a custom item rather than `Predefined`, but the gotcha applies to any
future `PredefinedMenuItem.new({ item: "About" })` call.

**Accelerator policy** (the one dangerous design point): node-op items get **no accelerator
at all** — the plain-key hint (`c`/`x`/`v`/`z`/`y`) is a label suffix only, e.g. `Copy Node
(c)`; actual handling stays in `ui.ts`'s `onKey`. Binding `CmdOrCtrl+C/X/V/Z/Y` to a menu item
would intercept the key **before** the webview sees it, breaking native copy/cut/paste/undo
inside every text input (inline edit, panel fields, search box). Zoom items also get no
accelerator — `zoomHotkeysEnabled` (`tauri.conf.json`) already owns Cmd+/−/0; the JS-tracked
zoom factor (`menu.ts`'s module-local `zoom`, `±0.1` steps clamped to `[0.3, 3]`) is a known,
accepted, not-synced duplicate of that built-in path. `getCurrentWebview().setZoom()` needs
`core:webview:allow-set-webview-zoom` explicitly in `capabilities/default.json` —
`core:webview:default` does not include it.

**GC-retention gotcha:** `buildAndSet()` keeps the built root `Menu` in the module-level
`installedMenu` variable and never lets it go out of scope. Every `Menu`/`Submenu`/`MenuItem`
JS wrapper is backed by a Tauri resource (including the click-action channel); if nothing in
JS references the tree after `setAsAppMenu()` returns, V8 is free to garbage-collect it at any
later point, tearing down those resources while the native OS menu bar keeps showing the —
now silently unresponsive — items. A large allocation spike (e.g. opening a file and swapping
in a fresh wasm `Session`) is a classic GC trigger, which is how this first surfaced. Children
don't need their own persistent JS references — they stay alive via the Rust-side tree the
root `Menu` resource owns.

**Recent files:** `localStorage["confy-recent"]` (Tauri-only — paths are only meaningful
there), most-recent-first, cap 8, deduped by path. `fs.ts`'s `OpenedFile`/`FsHandle` both grew
an optional `path` field (populated only on the Tauri branches of `tauriStartupFile`,
`pickOpenFile`, and `tauriHandle` — so `pickSaveFile`'s returned handle carries it too);
`ui.ts` calls `recentAdd` + `rebuildMenu()` wherever a Tauri path becomes newly known (startup
file, Open, Save As), and `openTauriPath(path)` (new `fs.ts` export, `read_file_text` via
`invoke`) backs the menu's `openRecentPath` handler — a missing/unreadable file calls
`recentRemove` + `rebuildMenu()` + an error status instead of opening.

## Mobile (Tauri Android)

Android reuses the touch UI verbatim (same `web/touch/` module, same `confy.ts`/`Intent`
contract) — the mobile-specific surface is entirely in host I/O (`web/fs.ts`) and a couple of
platform guards, not a separate UI.

**Picker + file-association I/O.** `fs.ts::isTauriAndroid()` (UA-sniffed, no `tauri-plugin-os`
dependency) forks `pickOpenFile()` to call the first-party `plugin:confy-picker|pick_writable`
command instead of `dialog.open()` — stock `tauri-plugin-dialog`'s Android picker uses
`ACTION_GET_CONTENT`, which never grants write access at all. Opening a file via the OS's "Open
with" chooser instead arrives through `tauri.android.conf.json`'s `fileAssociations` (Rust-side
`opened_urls`/`"opened"` event) and reads through the same `openTauriPath`-style path — no plugin
needed there, since a file-association launch intent's own grant covers the receiving activity's
lifetime. `menu.ts`'s native menu bar no-ops on Tauri mobile (same `isTauriMobile()` guard as
`canSaveAs()`) — there's no menu bar on Android.

**`canSaveAs()` gating.** False on Tauri mobile: picking a *new* save destination (Save As, first
Save after File-New-equivalent, Convert's output path) isn't supported in M1, so those paths show
a translated hint (`web.mobile.saveAsUnavailable`) instead of opening a picker. Writing in place
to an already-open handle is unaffected by this flag — `doQuickSave` only consults it on the
no-handle-yet (first save) branch.

**The split-button lesson (why Save is one plain button, not a pill).** An earlier iteration
tried merging the Save button and a "Save As / Convert…" chevron into one visually-glued
`.split-btn` pill. It rendered as two buttons stacked top-to-bottom on a real device with no
visible CSS explanation — root cause: **`web/touch/` has its own separate stylesheet
(`touch/style.css`), not the shared desktop `web/style.css`**, and the `.split-btn` CSS rule (and,
separately, the `env(safe-area-inset-top)` toolbar padding fix) had only been added to the
desktop file. Any style fix aimed at touch must land in `touch/style.css`, not `style.css` — the
two are not the same cascade and nothing here shares rules between them by default. Once fixed and
seen live, the pill design itself was dropped in favor of the plain single-button-opens-a-sheet
design described in the Touch UI section above — simpler, and immune to this whole class of bug.

**Debugging technique — live CDP against the on-device WebView.** Android's WebView exposes a
Chrome DevTools Protocol endpoint when the app is debuggable: `adb forward tcp:PORT
localabstract:webview_devtools_remote_<pid>` (find `<pid>` via `adb shell ps -A | grep
<package>`), then `curl http://localhost:PORT/json` for the page's `webSocketDebuggerUrl`. A
plain WebSocket client can then send `Runtime.evaluate` (and other CDP methods) directly — no
`chrome://inspect` UI needed. One gotcha: the devtools server 403s a connection whose `Origin`
header doesn't match an allowlist, so connect with `suppress_origin=True` (Python
`websocket-client`) or an equivalent that omits the header. Combined with `adb shell input
tap`/`screencap` to drive the actual system UI (document pickers, "Open with" choosers, the home
screen), this lets bugs get root-caused and fixes verified end-to-end on real hardware without a
human re-testing every iteration.

## Deployment

The hosted site is **<https://confy.turkeyang.net/>**, deployed via **Cloudflare
Workers Builds** Git integration (config lives in the CF dashboard, not in a
GitHub Actions workflow). The repo carries two deploy files:

- `web/cf-build.sh` — the CF **build command** (`bash web/cf-build.sh`): installs
  Rust/wasm-pack if absent, runs `wasm-pack build --target web` + `npm install &&
  node build.mjs`, then assembles a clean runtime-only `web/dist` (html/css/js/map
  + `pkg/`, no `node_modules`/sources). `web/dist` is gitignored.
- `wrangler.toml` — the CF **deploy command** (`npx wrangler deploy`) reads it:
  an assets-only Worker named `confy` serving `web/dist`.

Production branch is `main`; every push to `main` rebuilds and deploys (Git
integration can't be tag-gated). The custom domain is set in the Worker's
Settings → Domains & Routes.

## PWA (installable + offline)

The site is an installable PWA: `web/manifest.webmanifest` (standalone display,
`web/icons/icon-192.png`/`icon-512.png` derived from `crates/confy-tauri/icons/icon.png`)
plus `web/sw.js`, registered from both `index.html` and `touch.html` **on https only** —
the dev server (`serve.mjs`) stays SW-free so its deliberate `no-store` caching keeps
working, and `sw.js` never interferes with local wasm rebuilds.

`sw.js` is **network-first with cache fallback** for every same-origin GET: a fresh
deploy is always picked up immediately (matching the push-to-`main` → CF flow, no
version-stamped cache busting needed), each successful response is copied into the
`confy-shell-v1` cache, and the cache is served only when the network fails. The app
shell (both HTML entries, both CSS/JS bundles, `pkg/confy_ffi.js` + the wasm, the
manifest) is precached on install, so the app works offline after the very first visit.
Navigation requests match the cache with `ignoreSearch` (the entry-router query strings
`?ui=` / `?url=` are volatile). `cf-build.sh` copies `manifest.webmanifest`, `sw.js`,
and `icons/` into `dist`; installed-app launches hit `start_url: "./"` and the normal
coarse-pointer router bounces to the touch UI.

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

## VS Code (webview host)

`editors/vscode/` is a third host shell (M1, sideload `.vsix` only — no Marketplace
listing). A `CustomEditorProvider` owns the VS Code document lifecycle and file I/O;
the webview runs the unmodified `web/dist` bundle plus `web/vscode.ts`'s adapter. Every
behavior difference from the browser/Tauri hosts is gated in `ui.ts` on `VSHOST`
(`isVsCode()` — true only when `acquireVsCodeApi` exists), so the pure-browser and Tauri
builds are byte-identical when it is absent.

**Chrome trimming.** `document.body.classList.add("host-vscode")` on boot; `style.css`
hides `#btnOpen`/`#btnSaveAs`/`#btnTheme` under `body.host-vscode` — the document is
tab-bound (VS Code owns Open), destination picks are native save dialogs, and the theme
follows VS Code's own theme instead of confy's toggle.

**Theme.** No `theme` protocol message — `web/vscode.ts`'s `trackVsCodeTheme()` instead
runs a `MutationObserver` on `document.body`'s class list, mapping VS Code's
`vscode-dark`/`vscode-light`/`vscode-high-contrast(-light)` stamps onto confy's own
`:root[data-theme]`. Same visible behavior as a message would give, no protocol needed
(a documented refinement over the original spec).

**Message protocol** (`web/vscode-protocol.ts`, single source of truth for both sides):

| Direction | Message | Purpose |
|---|---|---|
| host→webview | `init` | Initial text/name/format/lang; VS Code's display language is authoritative here (same principle as theme) |
| host→webview | `undo` / `redo` | The *only* way an undo/redo reaches the Session (single-owner rule) |
| host→webview | `save-request { id }` | Host asks for `session.serialize()` (save/save-as/backup) |
| host→webview | `save-ok { id }` | Sent only after `workspace.fs.writeFile` succeeded; the webview marks the session clean only on this ack |
| host→webview | `revert { text }` | File > Revert: rebuild the Session from on-disk text |
| webview→host | `ready` | Boot handshake |
| webview→host | `edited { dirty, text }` | A user-initiated mutation: host pushes one VS Code edit entry + refreshes the raw preview |
| webview→host | `synced { dirty, text }` | A host-initiated change (undo/redo/revert/save-ok) landed: mirror only, no new edit entry (an addition over the original spec table — the suppression half of the undo rule) |
| webview→host | `edit-cancelled { dirty, text }` | The Session rolled back its newest history entry *without* a host undo (add→Esc via `History::cancel_last`, detected as a `history_len` decrease): mirror + neuter the newest live VS Code edit entry |
| webview→host | `save-response { id, text }` | Answers a `save-request` |
| webview→host | `request-undo` / `request-redo` | Webview keyboard/toolbar undo forwards to the host so VS Code's stack stays the sole entry point |
| webview→host | `request-save` | Webview Save / ⌘S → workbench save |
| webview→host | `convert-save { suggestedName, text }` | Convert (or same-format save-a-copy) output: host shows a native save dialog |
| webview→host | `parse-error { message }` | Initial text failed to parse: host offers the default text editor instead of a white screen |

**The `history_len` / `edit-cancelled` depth rule.** `SessionSnapshot.history_len`
(`History::depth()`, i.e. `past.len()`) is the one additive core change M1 needed. The
webview diffs it across every dispatch outside a batch: depth **grew** → a real edit
(`edited`), depth **shrank** → `History::cancel_last` rolled the newest entry back
(the two callsites are scalar add→Esc and comment add→Esc) and the host must neuter its
matching VS Code entry (`edit-cancelled`), depth **flat** → mirror-only (`synced`).
Documented residual wart: VS Code's edit-stack API can't *remove* an entry, so the
neutered add→Esc entry still counts toward the dirty dot until it is popped by one
no-op ⌘Z at the tail of the stack.

**Revert / hot-exit restore resets view state — by design.** Both rebuild the Session
via `openText`, so expansion/cursor/selection/filter state resets to the fresh-load
default. This is intended: a view reset alongside an explicit destructive action, not a
bug to fix.

**Boot-path localStorage guards.** `host-io.ts`'s `initTheme`/`toggleTheme` and
`i18n.ts`'s `getLang`/`setLang` all wrap `localStorage` access in `try/catch` — a
sandboxed webview may throw on any access, and these run on the boot path before `ready`
is even posted, so an unguarded throw would white-screen before the host ever hears from
the webview. The guards are behavior-neutral for the browser/Tauri hosts and are **not**
`VSHOST`-gated. Persistence unreliability in webviews is accepted for M1 — theme comes
from the VS Code observer regardless, and lang re-arrives on every `init`.

See `editors/vscode/README.md` for build/install/use, and CLAUDE.md's module map for
the extension-host-side file layout.
