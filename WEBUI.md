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
                  │            Web UI (DOM)           │   tree render +
                  │   index.html / ui.ts / style.css  │   keyboard → Intent
                  └──────────────────────────────────┘
```

One command channel: the UI serializes an `Intent`, calls `ConfySession.dispatch`,
and re-renders from the returned `SessionSnapshot`. No editor logic lives in the UI.

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

- **`Intent`** — every key-mapped action (navigation, selection, filter, type-filter,
  kind-switch, convert, edit, mutations, undo/redo, lifecycle). The UI maps a DOM
  keyboard event to one `Intent`.
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
- **`ViewRow`** — one visible tree row (`path`, `depth`, `isBranch`, `key`, `value`,
  `scalarType`, `format`, `trailingComment`, `readOnly`, `selected`, `isCursor`).
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
  `SessionSnapshot` and renders the DOM from it. Every interaction is `dispatch`.
- **Keyboard → Intent.** A single `keydown` handler maps keys to `Intent` (mirrors the
  TUI `keys.rs` map: `j/k` cursor, `Enter` toggle/edit, `e` edit, `a/d/c/x/v/r`,
  `z/y`, `/`, `f`, `K`, `C`, `?`, Esc). The current `ModeView` decides which keys are
  live (e.g. in `Filter`, printable chars become `FilterKey`).
- **Modal surfaces.** Each `ModeView` variant renders its surface: a tree (Normal),
  a filter input (Filter), a checkbox grid (TypeFilter), a kind list (KindSwitch), a
  convert wizard (Convert), a detail popup (Detail), a help overlay (Help), an inline
  editor row (Edit), a yes/no prompt (Prompt).
- **External edit modal.** When `snapshot.externalEdit` is set, a `<textarea>` modal
  opens with `initial`; on submit the UI dispatches `ApplyReplace`/`ApplyEditComment`
  with the path from the request and the edited text.
- **Type-filter facet grid.** `f` opens the `TypeFilter` popup; the UI renders the
  projected grid (`TypeFilterView`) directly — headers + tri-state cells with the
  cursor highlighted. `←/→/↑/↓` move, `Space` toggles, `Enter` applies, `Esc` cancels.
  The facet set itself is authoritative in core (`session/type_filter::layout`); the
  host only lays out the cells it is handed.
- **File I/O — File System Access API with download fallback.** All file I/O is
  host-owned (`web/fs.ts`); core `Intent::Save` only clears the dirty flag. Save
  precedence: (1) write in place to the open `FileSystemFileHandle`; (2) if the API is
  available but no handle is held, `showSaveFilePicker` Save-As (and the handle is
  kept so subsequent saves are in place); (3) download (Firefox/Safari/older
  browsers). `Ctrl-o` / Open opens a real file via `showOpenFilePicker`; the Load
  button (paste-into-textarea) is the always-available fallback. The "Open…" button is
  hidden on browsers without the API. Convert output routes through Save-As when
  available, else download. The capability is detected once at boot and isolated
  behind `web/fs.ts`; no editor logic depends on it.
- **Theme.** A dark/light toggle (titlebar `☾`/`☀`) flips `:root[data-theme]`; CSS
  variables carry both palettes and the choice persists in `localStorage`.

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
