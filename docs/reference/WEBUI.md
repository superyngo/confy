# WEBUI.md — confy Web UI & WASM FFI contract

The Web UI is the second host of the headless core (`confy-core`), alongside the
ratatui TUI. It compiles the same `Session` state machine to WebAssembly and drives
it from TypeScript. This file documents the FFI boundary and the UI architecture; the
shared model glossary lives in `CONTEXT.md`, nested behavior in `BEHAVIOR_MATRIX.md`,
TUI mechanics in `TUI.md`, the cross-platform row cursor/selection/clipboard state
model in `ROW_STATE_MODEL.md`. Two native shells embed this same `web/` bundle and get
their own docs: the Tauri desktop/Android app in `TAURI.md`, the VS Code extension in
`VSCODE.md`. The port design record is `PORTING.md` (§8 records the Stage-2 transport
decisions). Keyboard bindings live in `KEYMAP.md`, the TUI ↔ Web single source of truth,
whose table is machine-checked against `resolveKeyIntent` by `web/keymap-parity.spec.mjs`.

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

One `wasm-bindgen` class, `ConfySession`, wraps a `confy_core::session::Session`, plus a
single free function (`fuzzy_indices`, below — the only export that isn't a session method).
All cross-boundary values use `serde-wasm-bindgen` so the Rust `Serialize`/`
Deserialize` derives (Phase E + Stage 2 §8.1) are the wire contract — there is no
hand-maintained field-by-field marshalling.

**Two naming layers — mind the difference.** `wasm-bindgen` exports the methods under their
**Rust snake_case** names (`schema_hint`, `schema_violations`, `had_comments_at_open`); the
generated `pkg/confy_ffi.js` glue is the proof. `web/confy.ts` then wraps that raw class in a
`Session` class exposing **camelCase** (`schemaHint`, `hadCommentsAtOpen`), which is what the
table below and the rest of the web code use. The VS Code extension deliberately bypasses the
wrapper and types the raw class directly (`editors/vscode/src/wasmSession.ts`), so it calls the
**snake_case** names — renaming an FFI method means updating both spellings.

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
| `children` | `(path: Seg[]) => ChildView[]` | immediate children of the node at `path` as `ChildView[]` (`{ key, path, type_label, is_branch }`), independent of expansion state; feeds the breadcrumb mini-tree's lazy expansion. |
| `externalEdit` | `() => { initial, kind } \| undefined` | the current external-edit request, if any (§8.2). |
| `diagLog` | `() => DiagEvent[]` | full event list from the Session's bounded 256-event ring buffer; feeds `?diag=1` console drain. |
| `setStrictJson` | `(v: boolean) => void` | host-supplied: true iff the open file's real extension is plain `.json` (not `.jsonc`) — core is extension-blind, so only the host knows. Drives `ViewRow.comment_advisory`. Called once right after `fromText`, before the first `snapshot()`. |
| `hadCommentsAtOpen` | `() => boolean` | whether the document already contained a comment when loaded — drives the one-shot "file already had comments" toast. `false` for non-JSON. Replaced the deleted `supportsComments()` write-gate binding. |
| `aboutText` | `() => string` | About-tab body for the session's current language, from core's `about_text` — the host never hand-mirrors it. |
| `schemaHint` | `(path: Seg[]) => EditHint` | schema-driven editing hint (enum/const options or numeric bounds; `None` when unconstrained). Read-only, does not enter edit mode. |
| `schemaInfo` | `(path: Seg[]) => SchemaInfo \| undefined` | `description`/`type`/`format`/`pattern` from the resolved subschema. Orthogonal to `schemaHint`: covers the plain-typed field that hint leaves at `None`. |
| `schemaViolations` | `() => Violation[]` | current violations with resolved `text_range`s — the native-editor Diagnostics data source. |
| `outline` | `() => OutlineNode[]` | read-only symbol tree for editor Outline/breadcrumb integrations, independent of cursor/expansion state. |
| `pointerSlot` | `(path: Seg[], relY: number) => PasteSlot \| undefined` | pointer-drop classification (row + relative vertical position → the `PasteSlot` it represents). Every pointer surface calls this instead of hand-rolling it (ADR 0004 §1). |

`external_edit` in the snapshot is the async handshake (§8.2): the UI opens its
multi-line modal with `initial`, awaits the result, and re-dispatches
`ApplyReplace { path, text }` / `ApplyEditComment { path, text }`.

**One free export: `fuzzy_indices(haystack: string, needle: string) => Uint32Array |
undefined`.** Not a `ConfySession` method — it is pure and stateless, so hosts call it per
rendered cell while a filter is active to mark the matched characters. It wraps
`confy_core::session::search::fuzzy_indices`, the *same* `SkimMatcherV2` the TUI's
`highlight_spans` uses, so the two surfaces cannot drift apart; a reimplemented TS scorer
would have. Returns `undefined` on no match or an empty needle. The indices are **char**
offsets (declared `Vec<u32>` because that is what marshals to a `Uint32Array`), so JS must
walk the text with `Array.from` — indexing by UTF-16 code unit misplaces every mark after
an astral char. Consumed by `web/highlight.ts` via `setFuzzyMatcher` (§*Web UI architecture*).

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
  plumbing: `SetCursor(Path)`, `SetSelection {paths}`, `MoveSelectionTo {sources,slot,cut}`
  (grip drag-reparent — a one-shot cut/copy→paste reusing `Mutation::Move`, its `PasteSlot`
  resolved by the same `slot_target` a keyboard `Paste` uses, ADR 0010),
  `CommitEdit {value?,name?}`, `CommitKind {path,target}`, `SetFilter(String)`,
  `SetConvertFormat(DocFormat)`, `SetConvertPath(String)`.
- **`SessionSnapshot`** — full renderable state: `mode: ModeView`, `rows: ViewRow[]`,
  `cursor: Seg[]`, `notice: Notice | undefined`, `detail_text`, `external_edit`, `convert_write`,
  `clipboard_count`, `clipboard_cut`, `clipboard_paths`, `paste_slot`, `quit`, `doc_format`,
  `is_dirty`, `filter` (the live text-filter query — present in **every** mode, unlike
  `ModeView::Filter`'s `text`, which exists only while the search input is focused; pointer
  hosts need it to keep drawing match highlights after focus leaves the box).
- **`ModeView`** — a serializable projection of `Mode` + the modal edit surfaces:
  `Normal | Prompt | Filter {text,cursor} | FilterResults | TypeFilter {…grid…} |
  KindSwitch {cursor,options} | ActionMenu {cursor,items,target_count,target_label} |
  Convert {…} | Detail | Help | Edit {field,buffer,cursor,…} |
  SchemaEnum {options,cursor,from_schema}`. `SchemaEnum`'s `from_schema` is `false` when the
  picker is the schema-independent `bool` `true`/`false` fallback rather than a schema
  `enum`/`const` constraint — hosts use it only to title the popup ("Value" vs "Schema value").
  This is the UI's only view of internal state; heavy
  internals (`History`, `Clipboard` — except its `clipboard_count`) never cross.
- **`TypeFilterView`** — the `f` popup grid, projected from core so the host never
  re-derives the per-format facet set: `rows: (Header | Cells[…])[]`, `cursor_row`,
  `cursor_col`, `active`. Each cell carries `label`, tri-state `state`
  (`On`/`Partial`/`Off`), and `is_cursor`.
- **`ViewRow`** — one visible tree row (`path`, `path_display`, `depth`, `is_branch`, `key`,
  `key_literal`, `key_sign`, `value`, `scalar_type`, `format`, `type_label`, `child_count`,
  `trailing_comment`, `comment_advisory`, `read_only`, `selected`, `is_cursor`, `violations`,
  `has_descendant_violation`). `type_label` (the core node-kind label) and `child_count` let
  the web render the true container kind + item count instead of guessing from `is_branch`.
  `key` is the **decoded** key while `key_literal` is the key **as authored** (quotes intact,
  `null` when bare) — render and seed edit buffers from `key_literal ?? key`, never from `key`
  alone, or a quoted key silently restyles to bare on commit. `path_display` is the ready-made
  Path string (see CONTEXT.md *Path line*); prefer it over joining `path` client-side.
  `comment_advisory` is a per-row projection, not a Notice — see MESSAGES.md §7.1.
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
  web-native row: rotating disclosure caret, key (or faint `[i]` for positional
  elements) / `=` / value (value-type colored) or item count, a per-row **kind badge**
  (friendly label + notation suffix + chevron — `table·scope`/`table·dotted`/`array·multi`,
  YAML `·block`/`·flow`, scalar `·"…"`/`·0x`/`·1e`/…), comment/trailing decoration, and
  hover action buttons flush right — a **drag grip only**: node operations live in the
  centralized **Action menu**, opened by right-clicking a row, the `m` key, or the floating
  Action button below (ADR 0009 — the old per-row `⋮` menu is gone; there is no separate
  per-row add button either). Each row
  carries `data-path` (attribute-safe JSON) so the
  pointer layer maps a click back to a node without re-deriving structure.
- **Floating action/paste button (FAB).** A bottom-right floating button, shared with touch via
  `web/fab.ts` (glyphs + markup — `data-act="actions"`). When the clipboard is unarmed it opens
  the centralized **Action menu** (desktop popover, `openActionMenuAt`); while armed it becomes
  the **Paste** button (tap = `Paste`, with a cancel affordance above). Desktop wires it in
  `web/ui.ts`; behavior is identical on touch (see the Touch UI section). Hidden in Raw view
  (`body.raw-view`).
- **Pointer selection (`select.ts`).** Pure logic resolving a click into the next full
  selection set → `SetSelection`: plain click = that row; ⇧-click = contiguous range from
  an anchor, **unioned onto a base snapshot** so earlier segments survive (segmented
  multi-select); ⌘/Ctrl-click = toggle without clearing (and re-anchor). A marquee
  rubber-band selects every row it intersects. The clicked end is kept last so core's
  cursor follows it. Plain `j/k`/arrow nav collapses the selection onto the new cursor.
  A **double-click on a row's empty area** (detected manually by timing two plain body
  clicks on the same path — native `dblclick` is unreliable after the first click
  re-renders) toggles the **Detail panel** (`SetCursor` + `ToggleDetail`). Only
  empty-space clicks reach it (key→rename, value→edit, caret→expand all return first).
  With a **multi-selection**, `Space` toggles every selected branch independently (cursor-walks
  the selected branch rows dispatching `ToggleExpand`, then restores the selection); a single
  selection keeps the plain cursor toggle.
  Navigation keys (`←→↑↓`, Home/End, PageUp/PageDown, Space) `preventDefault` so the browser's native
  arrow-scroll can't drag the off-canvas detail panel into view (`.main` is also
  `overflow:hidden` as a backstop).
- **Drag-reparent (`dnd.ts`).** HTML5 grip drag → `MoveSelectionTo`. `dragover` asks core
  `pointerSlot(path, relY)` for the destination and keeps that `PasteSlot` verbatim — a
  branch mid-band is `Into` (`.drag-over-into` outline), anything else is `After(p)`
  (horizontal `#dropLine` under `p`'s row, one indent step deeper when `p` is an expanded
  branch, since that slot inserts as its first child — `slot-line.ts`). `drop` sends the slot
  as-is; core resolves it with `slot_target`, the same call an armed keyboard `Paste` makes,
  so a drag and a paste released at the same pixel always land together (ADR 0010). The host
  derives no parent/index and no band threshold of its own; a self-subtree drop, a collision
  and an illegal destination are rejected by `do_paste` with the document untouched.
- **Inline edit / kind / context.** Clicking a value → a live `<input>` (seeded from the
  edit buffer, Enter/blur → `CommitEdit`, **sized to its content** — `editWidthCh` seeds a
  `width:…ch` and an `input` listener grows it while typing, CSS min/max-width clamping);
  a key → a rename input; the kind badge → a
  popover built from `kindOptions(path)` → `CommitKind`; right-click on a row → the
  centralized **Action menu** (`openActionMenuAt`). All popovers share one synchronous closer (a single outside-click
  listener) and are scoped per popover so they don't open/close together. **Every menu
  button toggles** — a second click on the `⋯` More button (tracked by `.open`) or the
  Action button closes
  the menu, matching the already-toggling type-filter button and kind badge. **Every popup
  closes on Esc** — the click-menus via `anyClickMenuOpen`, `#tfPop`/overlay/`#convDlg`/
  inline editor/external-edit modal each in their own path, and the load-modal via its own
  keydown handler (it early-returns from `onKey`, so it needs one). A **comment row's
  click target is the text only** (`.comment-row .comment` is `flex:0 1 auto` — no grow — so
  the empty space past the text no longer opens the editor; shrink is retained for the
  narrow-width ellipsis). **"Append sibling" on a comment** (Action menu / `AddSibling`)
  inserts a *separate* single-line comment and opens it in the inline editor; **Esc removes
  it** — the core `add_comment_sibling` path (blank-separated node + `created_on_add`).
- **Native modal widgets replace the keyboard overlay.** The always-visible **search box**
  owns the filter text (debounced `input` → `SetFilter`, `/` focuses it — no `Mode::Filter`
  is ever entered; search now matches **scalar values**, not just keys/paths/comments).
  **Matched chars are marked in the tree**, same as the TUI: `web/highlight.ts` wraps them in
  `<mark class="fz">` (amber wash, `mark.fz` in both stylesheets) inside the key, value and
  comment-row cells on **desktop and touch**. It calls the very matcher the TUI highlights with —
  `fuzzy_indices` is a free `#[wasm_bindgen]` export (`crates/confy-ffi/src/lib.rs`) over
  `confy_core::session::search`, so the two surfaces can't drift apart. The matcher is *registered*
  by `confy.ts`'s `load()` (`setFuzzyMatcher`) rather than imported by `render.ts`, keeping the
  render modules free of the wasm glue and bundleable by the plain-Node spec harness; because
  `ui.ts` *and* `touch/app.ts` both boot through that `load()`, **every host shipping those two
  bundles inherits the highlight** — browser, Tauri, and the VS Code webview alike (the VS Code
  *extension* host's raw `wasmSession.ts` path renders no tree, so it needs nothing). Before
  registration it matches nothing and degrades to plain escaped text. Indices are **char** offsets,
  so the text is walked with `Array.from` — indexing by UTF-16 code unit would misplace every mark
  after an astral char. The query comes from **`SessionSnapshot.filter`**, which carries it in every
  mode (`ModeView::Filter` only has it while the input is focused and `FilterResults` drops it), the
  way the TUI reads `Session.filter` directly. Desktop's `renderTree` row-reuse cache compares the
  full rendered HTML, so a query change invalidates it for free. `f`
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
  ConfirmQuit, and Overwrite / Rename / Cancel for a paste Collision. The question line is
  `snap.mode.Prompt.question` verbatim — core composes and localizes it (`prompt_question`),
  and the old host-side status-stripping `promptQuestion` helper is deleted. Desktop
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
- **Paste mode.** While the clipboard holds a cut/copy the selection is frozen
  (`Session::set_selection` is a no-op), so a row click positions the paste target
  instead: `armedPasteTarget()` reads the click's row-relative Y and calls
  `session.pointerSlot(path, relY)` → `SetPasteSlot` (`Into`/`After`), falling back to
  `SetCursor` only when no slot resolves (ADR 0004 §1). The committed target is the **only**
  full-row highlight while armed — both the plain cursor style and the plain `:hover` style
  are suppressed via `body:not(.paste-mode) .row.cursor`/`.row:hover` (`web/style.css`) so
  neither competes with it, mirroring the TUI's `active_slot` precedence (no blue
  cursor/hover while a paste slot is in play). The committed target and the live hover
  preview are two visually distinct layers, split into `renderConfirmedPasteCue` and
  `renderHoverCue` (`web/ui.ts`) so a user can tell "the pointer happens to be over this
  row" apart from "this is where Paste will actually land" without moving the mouse off
  the tree to check: the confirmed layer (solid green `.paste-target`/`#pasteTargetLine`)
  always reflects `snap.paste_slot` and is untouched by pointer movement; the hover layer
  (dashed/muted `.drag-over-into`/`#dropLine`, de-emphasized while `body.paste-mode`) is a
  **client-only** preview computed by `onArmedPasteHover` (a delegated `mousemove` on
  `treeWrap`) re-running the same `pointerSlot()` classification and repainting the hover
  cue elements — no `dispatch`, no re-render — clearing to nothing the moment
  `pointerSlot` declines to classify the hovered row or the pointer leaves the tree
  entirely (`mouseleave`), rather than falling back to redrawing the committed target
  (that's already always visible via the confirmed layer). See `ROW_STATE_MODEL.md` §6a
  for the full cross-platform row-state model this participates in.
- **Pointer value gestures.** A **double-click on a row _toggles_ the Detail panel** for it
  (`SetCursor` + `ToggleDetail`); it no longer toggles branch-expand/boolean-value (expand stays
  on the caret + Space). **Mouse-wheel value nudge** is gated to inline-edit mode: hovering a
  value and scrolling does nothing; only once the value field is focused (the tree's inline
  editor or the shared panel's value field, `web/panel.ts`) does the wheel adjust it — and once
  armed, *every* wheel tick anywhere on the page nudges the focused `Integer`/`Float` ±1 (wheel
  up = +1), not just ticks over the input. No `Intent` is dispatched per tick: the nudged text
  is written straight into the focused `<input>` via the stateless `nudge_repr` core query, and
  commits exactly once via the normal Enter/blur `CommitEdit` path (one undo entry per nudge
  session). On touch, the value field likewise supports **swipe-to-nudge only while focused**: a
  horizontal swipe starting anywhere (24px per step, 8px dead zone, `Integer`/`Float` only)
  writes through the same `nudge_repr` path; when the field is unfocused, native scroll and
  text selection are untouched. A `Bool` has **no** wheel/swipe/arrow-key nudge affordance at
  all — its value is edited exclusively through the dedicated true/false picker (`Mode::SchemaEnum`).
  The shared panel is **editing-and-information only** (ADR 0009): it holds no
  Copy/Cut/Delete buttons — node operations live in the Action menu, and the panel's former
  `afterMutation` dismiss callback is gone. Panel **key/value edits are one-shot
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
- **Schema hints on the web hosts.** An in-document `$schema` URL hint (e.g.
  `http://json-schema.org/draft-07/schema#`) is resolved by `web/host-io.ts`'s
  `resolveSchemaFetchRequest` (shared with touch). Browser fetches upgrade an
  `http://` hint to `https://` first — an https page blocks an `http://` fetch as
  mixed content before the server's 301 redirect can run ("Failed to fetch") —
  while the https endpoints of schema hosts send permissive CORS headers. The
  Tauri and VS Code branches fetch natively and are unaffected.
- **Theme.** A dark/light toggle (titlebar `☾`/`☀`) flips `:root[data-theme]`; CSS
  variables carry both palettes and the choice persists in `localStorage`.
- **Responsive toolbar.** The toolbar holds a single right-side action button (**Save**,
  opening the Save / Convert panel — the separate Convert button is gone). The full button
  inventory, row/group layout, per-button fold breakpoint ladder, and VS Code/Tauri desktop
  trimming rules are documented once in **`CHROME.md`** (shared with the touch UI) — not
  restated here. The More popup lists the folded secondary actions but **not** Save / Convert
  (that lives only on the always-visible Save button, so it is never duplicated). The search
  box has `min-width:96px` (well below its content size) so it yields space to those buttons
  before they collapse.
  **Rows stay single-line at every width:** they never wrap or hide cells — long key/value/
  comment compress with an ellipsis (`min-width:0` lets `text-overflow:ellipsis` fire inside
  the flex row). The **value compresses first**; the **key keeps its full width**
  (`.key{flex-shrink:0}`, truncating only past its `max-width:38vw` cap). Full text remains in
  the detail panel (`i`).
- **Notices & toast rendering (`renderNotice`).** Replaces the legacy two-bucket
  status/error model with unified severity-driven message surfaces. On desktop (`web/ui.ts`),
  `Success` notices display in a transient `#toast` alongside the status bar (`#status.sev-success`);
  `Error` notices render in a persistent red `#error` box with click-to-clear; `Info` and
  `Warn` notices update `#status` (`.sev-info` / `.sev-warn`). When idle (no notice), `#status`
  falls back to dynamic schema violation counts or node hover hints. `render()` calls
  `renderNotice(snap.notice)` on every dispatch, including pure navigation intents (cursor
  move, `ToggleExpand`, `SetPasteSlot`, ...) that core's Notice lifecycle deliberately
  leaves untouched (`MESSAGES.md` §1.1) — both desktop and touch guard against replaying a
  stale notice's toast entrance animation/timer on every such intent with a `lastNoticeKey`
  fingerprint (`${severity}|${text}`), re-triggering the toast display only when the key
  actually changes. On touch (`web/touch/app.ts`), all notices display via a unified
  `#toast` styled by severity class (`.sev-info`, `.sev-success`, `.sev-warn`,
  `.sev-error`) with longer duration for errors/warnings (3000ms vs 1600ms).

## Touch UI (dedicated `web/touch/` module)

The touch experience is **not** the desktop UI with gestures bolted on — that was tried and
rejected as low-fidelity. Instead `web/touch/` is a **separate, prototype-faithful UI** that
ports `docs/superpowers/specs/2026-06-26-web-respons-migrate-to-touch-ready.html` verbatim in
look & gesture, but drives the **same `confy-core` Session** through the shared
`confy.ts`/`Intent` contract — exactly how the desktop UI relates to the core. The prototype's
only discarded part is its fake `TREE`/DOM-as-state model; everything mutating goes through
`session.dispatch(Intent)` + a full re-render from the returned `SessionSnapshot` (stateless,
like desktop; ADR 0003 documents the one exception — the TUI calls `Session` methods directly
for its ~40 mutating calls instead of routing through `dispatch`). Beyond the core (`confy.ts`,
`types.ts`, `fs.ts`, the Intent contract), the two UIs now share several **single-source UI
modules** so look & behavior can't drift: `web/panel.ts`
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
  prototype's right-side branch `>` chevron is dropped. A comment/trailing-comment span gets the
  `comment-advisory` class (wavy underline, matching desktop) when `ViewRow.comment_advisory` is
  set. Each non-read-only row carries two hidden buttons behind `.row-main`: `.row-del` (revealed
  by a **left-swipe-to-delete**) and `.row-remark` (revealed by a **right-swipe-to-remark**, toggle
  node ↔ comment) — see below.
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
- **Ctrl/⌘-tap and Shift-tap multi-select** reuse desktop's `resolveClick` gesture resolution
  (`web/select.ts`, previously desktop-only, now shared): Ctrl/⌘-tap toggles the tapped row
  into/out of the selection; Shift-tap ranges from the last plain/Ctrl-tap anchor — matching
  desktop's `onTreeClick` exactly. Only reachable on touch+keyboard hybrids (iPad+trackpad/
  keyboard, Surface, Chromebook, touchscreen laptops), since `PointerEvent.ctrlKey/shiftKey/
  metaKey` reflect real held keys and are unset on pure touch — a plain tap is unaffected (still
  a single-row `SetSelection`, and resets the anchor).
- grip drag (`pointerSlot`-classified, `Into`/`After` + `.reorder-line`/`.drop-into`) →
  `MoveSelectionTo {sources,slot}` — the same core-resolved slot desktop's `dnd.ts` sends, with
  the same `slot-line.ts` indent rule for the line (the dragged row's own subtree is excluded
  from drop candidates by path-prefix). Swipe is gone; reorder is grip-only.
- **Edit panel** (bottom sheet `<600px`, persistent side pane `≥600px` via `@container`): rendered
  by the shared `web/panel.ts` (`panelHTML` + `wirePanel`) — the same module the desktop detail
  aside uses, so both UIs show one locked field order **Key / Value / Trailing comment / Kind /
  Path / Children / Sign** (Path is the human dotted/bracketed form, e.g. `servers[1].port`; Sign
  from `ViewRow.key_sign`). Key → `CommitEdit {name}`, value → `CommitEdit {value}`, trailing →
  `SetTrailing`, comment node → `ApplyEditComment`, kind button → kind sheet. The panel has no
  Delete/Copy/Cut buttons — node operations live in the Action menu (ADR 0009). After each
  dispatch `wirePanel` surfaces `snapshot.error` via the host toast (failures are reported,
  not silent).
- on `≥600px` the persistent side pane has a **draggable splitter** between the tree and detail
  panes: it sets a `--detail-w` flex-basis on `.app` (clamped ~240–520 px) persisted to
  `localStorage` (`confy-detail-w`); hidden `<600px` and in Raw view.
- **Responsive chrome collapse + dynamic menu.** The `.app` is a `container-type:inline-size`
  container; toolbar/filter buttons stay single-line (`nowrap`) and fold into the `⋯` menu
  right→left, one at a time, via `@container` breakpoints. The full button inventory,
  row/group layout, and fold breakpoint ladder (shared with desktop) live in **`CHROME.md`**
  — not restated here. The **menu sheet is built dynamically**
  (`MENU_CANDIDATES` + `isFolded` = `offsetParent === null`) from whichever controls are
  currently folded — not a hardcoded list — so it always mirrors the breakpoints. Open/Save
  stay visible (never in the menu).
- **Type filter & Save/Convert use the shared modules** (`web/typefilter.ts` / `web/convert-dialog.ts`),
  the same code + markup desktop uses: the type-filter grid renders into the filter sheet via
  `typeFilterHTML`+`wireTypeFilter` (no "Done" button — the grid toggles live + has a `Clear`
  button, and
  the sheet closes via grab/scrim/header-×); Save/Convert renders the shared form into a
  **bottom `.sheet`** (not a `<dialog>`) via a sheet-backed `ConvertSurface`. Both are driven by
  `snapshot.mode` (`TypeFilterView` / `ConvertView`); the `convert_write` snapshot field is written
  via `fs.ts`. `dismissSheets` peels each mode on close (TypeFilter→`CommitTypeFilter`,
  Convert→`ExitConvert`, external-edit→`Escape`) so the next render doesn't re-open it.
- **FAB opens the Action menu** (shared with desktop via `web/fab.ts`, `data-act="actions"`):
  a tap opens the touch **Action menu bottom sheet** (`openActionMenuSheet`, driven by
  `snap.mode`'s `ActionMenu` view like the TypeFilter/Convert/Prompt sheets — ADR 0009; the old
  context-aware add heuristic is gone, `Add child`/`Add sibling` are Action-menu items). When the
  **clipboard is armed** (`clipboard_count > 0`) the FAB switches to a **paste glyph tinted by
  copy vs cut** (`clipboard_cut`) and a tap dispatches `Paste` at the cursor; tapping the
  status-bar clipboard badge clears it (`Escape`).
- **External-keyboard shortcut parity.** A `document.body` `keydown` listener (`onKey`,
  `web/touch/app.ts`) resolves every key through the same `web/key-intent.ts`
  `resolveKeyIntent` desktop uses (shared verbatim, no touch-only fork) — navigation
  (j/k/g/G, arrows, Home/End, PageUp/PageDown, Shift+↑/↓ range-select), edit actions (a/d/c/x/v/r/s),
  `e` edit / `E` force-popup-editor (`BeginEditExternal`, mirrors the TUI's `E`), `F2` rename,
  expand/collapse (1/2/0/9), Nudge (+/-), `/` focus-search, `f`/`C` TypeFilter/Convert,
  `m` Action menu, `?` Help, Ctrl+S/Ctrl+O save/open, z/y undo/redo, and Space multi-branch toggle all work
  from an external/Bluetooth keyboard on a touch device. Guarded against focused
  `INPUT`/`TEXTAREA`/`SELECT` fields and the URL/external-edit sheets so typing in a form
  field is never hijacked. Every resolved key ends in `scrollFocusIntoView()` — since
  `render()` re-applies the tree pane's captured `scrollTop` verbatim across a re-render (so a
  tap never snaps the pane to the top), keyboard nav needs its own scroll-follow. Minimal
  ("sticky cursor") scrolling against `treePane.scrollTop` directly, never
  `Element.scrollIntoView` (would also scroll the `position:absolute` app shell out from under
  its bottom-anchored sheets). The anchor is `.row.cursor` normally; in paste mode, where arrows
  move the insertion slot and not the cursor, it's the `.reorder-line` for an `After` slot
  (drawn at the target row's bottom edge — scrolling only the row can still clip the line) or
  the target row for `Into`. `Home`/`g` (and `k` from the first row) can leave the cursor on the
  document's undrawn root row — neither web host draws it, so a shared `drawnCursorFallback()`
  (`web/path-utils.ts`) re-targets the first drawn row after every keyboard nav dispatch, in
  both `touchNavSelect` here and desktop `ui.ts`'s `navSelect`. The paste-mode analogue,
  `Into(root)`, is drawn as an insertion line at the very top of the tree instead.
- **Swipe actions.** A left-swipe on a row's `.row-main` slides it open to reveal a red Delete
  action (`.row-del`); a right-swipe slides it the other way to reveal a neutral Remark action
  (`.row-remark`, toggles the node to/from a comment — desktop's `r` key). One row is open at a
  time, and either side auto-closes when the other opens. The pointer flow **locks the axis**
  (horizontal >8px & > vertical → swipe; vertical → scroll/tap-cancel) so it coexists with grip-drag
  reorder and list scroll; a swipe direction is only offered when the row carries the matching
  action (read-only rows opt out of both). The open row's transform is reset on the next full
  re-render (the tree `innerHTML` is rebuilt), so a Delete/Remark tap (or any tap) closes it.
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
- **Keyboard shortcuts** (external/Bluetooth keyboard on a touch device): a `document.body`
  `keydown` listener (`onKey`) reuses desktop's `resolveKeyIntent` (`web/key-intent.ts`) verbatim,
  so the key→Intent map can't drift between surfaces. Guarded against a focused `INPUT`/
  `TEXTAREA`/`SELECT` and the URL/external-edit sheets. Most resolved intents `send()` straight
  through, since touch already renders every core sub-mode they can produce (TypeFilter/Convert/
  Prompt/SchemaEnum/Help all reactively open/close their sheet). Three are host-specific because
  touch's own editing/kind-switch surfaces bypass the core sub-modes those intents drive on
  desktop (`Mode::Edit`, `Mode::KindSwitch` — touch renders neither): `e`/`BeginEdit` and
  `K`/`OpenKindSwitch` open touch's existing panel/kind sheets instead of dispatching the raw
  intent, and `i`/Enter (`ToggleDetail`) toggles the host-local detail sheet directly (no core
  mode backs it here, unlike desktop's `Mode::Detail`) — `Escape` closes that sheet first if open.
  `q`/`QuitRequested` is suppressed (`vshost: true` passed to `resolveKeyIntent`) — a web/touch
  surface has no "quit" concept.

### Breadcrumb bar + mini-tree (`web/breadcrumb.ts`)

A VS Code-style symbol path for the cursor node, in the `#crumbs` nav between
the filter row and the tree (all hosts — in the VS Code webview it supplies the
symbol segments the workbench's native breadcrumb can't show for custom
editors). One glyph-tagged segment per cursor-path `Seg` (⌂ root first; `Index`
segs render `[i]`; glyphs are VS Code-style text tags colored by the `--t-*`
value hues). Clicking a **segment** Reveals it directly — `RevealPath` expands
every ancestor, sets the cursor, and selects the target (plain `SetCursor`
rejects non-visible paths; in paste mode the frozen selection is left alone) —
and `ui.ts` then smooth-scrolls the revealed row to the viewport center
(clamped at the edges). Clicking a **`›` separator** (including the trailing
one after the current node — the only mini-tree entry when the cursor is on
the root) opens the mini-tree popup: a lazy mini document tree fed by the ffi
`children(path)` query, pre-expanded along the cursor path, highlighted at the
segment left of the separator; carets expand/collapse freely (expand state is
ephemeral per open), and clicking a row Reveals it the same way (select +
center-scroll included). If an active filter still hides the
target, the expansion sticks, the cursor stays, and the status line reports it.
The mini-tree shows the same node set as the main tree (comments and read-only
nodes included and jumpable). The popup is the module's only state — re-render,
outside pointerdown, or a capture-phase Escape closes it (the Escape is
swallowed so it doesn't also peel filter state). Hidden in Raw view. Not in the
touch UI (deliberate — touch is sheet-driven with a weak cursor concept).

`web/build.mjs` emits both bundles: `ui.ts → ui.js` (desktop, unchanged) and `touch/app.ts →
touch/app.js`.

**Shared edit/detail panel — `web/panel.ts`.** A framework-free module (`panelHTML(row)` +
`wirePanel(container,row,send,openKind,onError,afterMutation?)`) that renders the node edit/detail panel for
**both** UIs from a `ViewRow`, guaranteeing the field set + order can't drift between touch and
desktop. On the desktop side the detail `<aside>` (toggled with `i`/Enter) now renders this panel
**reactively** — it tracks the cursor row on every snapshot and is fully editable — instead of the
old static `detail_text` `<pre>` dump (that flat string is now only the empty-doc fallback). To
feed the panel's Sign field, core's `ViewRow` gained a `key_sign` field (`"bare"|"quoted"|"dotted"
|"none"`, the same mapping the TUI detail text uses) — a coarse display facet only, never usable
to reconstruct how a key was written.

**Boolean value picker.** A `bool` scalar's value field is not a text input on either web host: it
renders as a `data-act="editvalue"` trigger that dispatches `BeginEdit`, and core answers with
`Mode::SchemaEnum` (`from_schema: false`) — so the tree draws its usual `select[data-schema-enum]`
and the panel its `data-field="value-enum"` select, both offering exactly `true`/`false` in the
node's authored casing. The `bool` case (like the enum one) is predicted **host-side** in
`panelHTML` off `ViewRow.scalar_type`, because this panel is touch's only value-edit surface and
`Intent::CommitEdit` deliberately never re-enters the picker — without the prediction the picker
would be unreachable on touch, which has neither the `←/→` Nudge keys nor a mouse wheel. Touch
renders the option list as the shared bottom sheet, titled "Value" (vs "Schema value" when
`from_schema`). A schema `enum` on the same node outranks the fallback; the "Editor" button — and
its keyboard equivalent `E` — still forces the free-form popup editor (`BeginEditExternal`).

**Key spelling — `ViewRow.key_literal`.** `ViewRow.key` is the **decoded** key (semantic
identity: paths, collisions, schema lookup); `key_literal` is the key **exactly as authored**,
quote characters and escapes intact, or `undefined` for keyless rows (array elements, comments,
root). The tree row label, the panel's editable **Key** field and the rename `<input>` all read
`key_literal ?? key`, so a single-quoted YAML key shows `'a b'`, a double-quoted one `"a b"`, and
a quoted TOML key its own single set of quotes. No quote character is ever synthesized in the web
layer — the earlier `displayKey`/`isQuotedYamlKey` helpers (which hardcoded `"` and carried
per-format exceptions) are gone. JSON rows carry no `key_literal`: their keys are unconditionally
quoted, so it would be redundant on every row. `ViewRow.path_display` is quoted from the same
source, per segment.

Every one of those surfaces is **committed verbatim**, so seeding any of them from the decoded
`key` is a correctness bug, not a cosmetic one: an untouched commit would restyle a quoted key to
bare. The inverse holds in core — a rename writes the literal but re-anchors the cursor on the
**decoded** segments from `ConfigDocument::rename_key_segs()`, since a projected path never
carries quotes.

A **Schema** field renders after Meta (Path/Children/Sign), before the Actions row (Copy/Cut/
Delete/External-edit stay the panel's fixed trailing element), as a bordered card (mirrors
the panel's `.preview` box, not bare text — every other field is a bordered element too) when
the row carries any of three independent sources: `session.schemaInfo(path)` (non-widget
`description`/`type`/`format`/`pattern` info read straight off the resolved subschema — the
common plain-typed case `schemaHint`/`EditHint` doesn't model, e.g. a bare `{"type":"string"}`
field, so touch/desktop weren't showing anything for it outside a violation), the constraint
description (`schemaHintText(editHint)`, e.g. "Valid values: …"), and the row's own violation
message(s) — same underlying data used elsewhere (hover tooltip, status-line hint, enum picker),
just also given a persistent home in the panel. The card's border tints `--warn` (`.has-violation`)
when a violation is present, reusing the tree row's own `.row.schema-violation` warn signal
instead of inventing a second one. Omitted entirely only when none of the three apply.
Independent of the Notice system (`MESSAGES.md`) — mirrors the TUI Detail popup's `Schema:`
section exactly (`TUI.md` § Status & diagnostics).

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

## Diagnostics (`?diag=1`)

See `MESSAGES.md` for the full cross-platform message-system reference
(severity classification, per-host `Notice` rendering table, unified design).
This section covers the web-specific diagnostics export mechanics only.

The Web UI surfaces `confy-core`'s in-Session 256-event diagnostic ring (`DiagEvent`) through the
WASM FFI method `ConfySession.diagLog()` (`diag_log()` in `crates/confy-ffi`).

When the page URL includes `?diag=1`, `drainDiagIfEnabled()` in `web/ui.ts` runs after every
`dispatch` cycle, diffing returned events against a module-level `lastSeenSeq` counter and printing
only newly-recorded events to `console.debug` with a `[confy-diag] [LEVEL] KIND DETAIL` prefix.
Because events are filtered by monotonic `seq`, successive user interactions emit only their own
trace deltas without re-logging historical events.

## Desktop + Mobile (Tauri)

Moved to **`TAURI.md`** — the native menu bar (`web/menu.ts`), recent-files, the
`PredefinedMenuItem`/GC-retention/accelerator gotchas, and Android (picker I/O,
`canSaveAs()` gating, the split-button/touch-stylesheet lesson, on-device CDP debugging).

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

Moved to **`VSCODE.md`** — the `CustomTextEditorProvider` model, chrome trimming, theme
tracking, the full message protocol table, echo suppression, edit-mode gating,
stale-tree pause, expansion/cursor restore, the 0.2.1 title-bar tab-swap fix, and the
boot-path localStorage guards.
