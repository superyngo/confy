# Keymap — TUI ↔ Web single source of truth

This file is the **single source of truth** for confy's keyboard bindings across surfaces.
It is not prose-only documentation: the table below is **parsed by two tests** that fail the
build whenever an implementation and this file disagree.

| Consumer | Checks |
| --- | --- |
| `crates/confy-tui/src/tui/keys.rs` (`keymap_doc_*` tests) | every **TUI** cell against `map_key` |
| `web/keymap-parity.spec.mjs` | every **Web** cell against `resolveKeyIntent` |

Both tests also run a **completeness scan**: any key that produces a binding in an
implementation but is missing from the table fails. Adding, removing or re-pointing a binding
on either surface therefore *requires* editing this file in the same commit. This guard exists
because the two keymaps are independent sources of truth written in different languages — the
`E` binding was missing from the web for exactly this reason (see CHANGELOG 2026-09-02).

## Scope of the machine-checked table

The table covers the **normal mode** (tree) keymap only — the surface where both hosts must
behave identically. Modal sub-modes (Edit, Convert, TypeFilter, KindSwitch, AddPicker,
ActionMenu, SchemaEnum, Help, Prompt) diverge *structurally* by design, because the web renders
native DOM widgets where the TUI renders a drawn overlay; those differences are described in
"Implementation differences" below rather than checked cell by cell.

Modifier scope: unmodified keys plus the explicit `Ctrl+`/`Shift+` rows below. Note that the
TUI's character arms match with a modifier wildcard (`(KeyCode::Char('c'), _)`), so e.g.
`Ctrl+C` also reaches `Copy` there; that quirk is out of the scan's scope.

## Column format

- **Key** — canonical name. DOM `KeyboardEvent.key` spelling, except `Space` (DOM `" "`).
  The Rust test maps these onto `crossterm::KeyCode` (`ArrowDown` → `KeyCode::Down`,
  `Escape` → `KeyCode::Esc`, `F2` → `KeyCode::F(2)`, single chars → `KeyCode::Char`).
  Shifted characters are written as the resulting character (`E`, `K`, `C`, `G`, `?`, `+`, `~`),
  not as `Shift+e`; only the arrows need an explicit `Shift+` prefix.
- **TUI** — the `KeyAction` variant returned by `map_key`, or `—` for `KeyAction::Noop`.
- **Web** — the `KeyResolution` returned by `resolveKeyIntent(mode="Normal", …)`, encoded as:
  `intent:<Intent>` · `nav:<Intent>` · `native:<action>` · `tree-page(<dir>)` · `—` for `null`.
  A payload-carrying intent is written `Name(arg)`, e.g. `intent:Nudge(1)`.
- **Status** — `both` (bound on both surfaces), `tui-only`, or `web-only`. Derived from the two
  columns and cross-checked by both tests, so it can never silently drift out of agreement.

<!-- KEYMAP-TABLE:BEGIN -->
| Key | TUI | Web | Status | Notes |
| --- | --- | --- | --- | --- |
| `j` | `CursorDown` | `nav:CursorDown` | both | vim down |
| `k` | `CursorUp` | `nav:CursorUp` | both | vim up |
| `ArrowDown` | `CursorDown` | `nav:CursorDown` | both | |
| `ArrowUp` | `CursorUp` | `nav:CursorUp` | both | |
| `g` | `—` | `nav:CursorHome` | web-only | TUI uses `Home`; accepted divergence |
| `G` | `—` | `nav:CursorEnd` | web-only | TUI uses `End`; accepted divergence |
| `Home` | `Home` | `nav:CursorHome` | both | |
| `End` | `End` | `nav:CursorEnd` | both | |
| `PageUp` | `PageUp` | `tree-page(-1)` | both | web page size is DOM-derived (`treePageStep`) |
| `PageDown` | `PageDown` | `tree-page(1)` | both | web page size is DOM-derived (`treePageStep`) |
| `Shift+ArrowUp` | `ExtendSelectUp` | `intent:ExtendSelectUp` | both | range select |
| `Shift+ArrowDown` | `ExtendSelectDown` | `intent:ExtendSelectDown` | both | range select |
| `s` | `ToggleSelect` | `intent:ToggleSelect` | both | |
| `Space` | `ToggleExpand` | `native:toggle-branches` | both | web batches multi-branch toggle host-side |
| `0` | `CollapseAll` | `intent:CollapseAll` | both | |
| `9` | `ExpandAll` | `intent:ExpandAll` | both | |
| `1` | `ExpandLevel` | `intent:ExpandLevel` | both | |
| `2` | `CollapseLevel` | `intent:CollapseLevel` | both | |
| `Enter` | `Info` | `intent:ToggleDetail` | both | ADR 0005 §4 |
| `i` | `Info` | `intent:ToggleDetail` | both | |
| `e` | `EditNode` | `intent:BeginEdit` | both | inline or popup, per `edit_target_kind()` |
| `E` | `EditExternal` | `intent:BeginEditExternal` | both | **force** popup/`$EDITOR` on any node |
| `F2` | `Rename` | `intent:BeginRename` | both | |
| `a` | `AddNode` | `intent:AddNode` | both | |
| `d` | `Delete` | `intent:DeleteSelected` | both | |
| `Delete` | `Delete` | `intent:DeleteSelected` | both | |
| `c` | `Copy` | `intent:CopySelected` | both | |
| `x` | `Cut` | `intent:CutSelected` | both | |
| `v` | `Paste` | `intent:Paste` | both | |
| `r` | `Remark` | `intent:Remark` | both | node ↔ comment |
| `ArrowRight` | `IncValue` | `intent:Nudge(1)` | both | |
| `ArrowLeft` | `DecValue` | `intent:Nudge(-1)` | both | |
| `+` | `—` | `intent:Nudge(1)` | web-only | TUI uses `ArrowRight`; accepted divergence |
| `-` | `—` | `intent:Nudge(-1)` | web-only | TUI uses `ArrowLeft`; accepted divergence |
| `z` | `Undo` | `native:undo` | both | web host owns the stack (VS Code shares it) |
| `y` | `Redo` | `native:redo` | both | web host owns the stack (VS Code shares it) |
| `Ctrl+s` | `Save` | `native:save` | both | |
| `w` | `Save` | `—` | tui-only | vim `:w`; accepted divergence |
| `Ctrl+o` | `—` | `native:open` | web-only | TUI takes a CLI path argument |
| `/` | `Filter` | `native:focus-search` | both | web focuses the real `<input>` |
| `f` | `TypeFilter` | `intent:EnterTypeFilter` | both | |
| `K` | `KindSwitch` | `intent:OpenKindSwitch` | both | capital: `k` is vim up |
| `C` | `Convert` | `intent:OpenConvert` | both | capital: `c` is copy |
| `m` | `ActionMenu` | `intent:OpenActionMenu` | both | |
| `?` | `Help` | `intent:EnterHelp` | both | |
| `Escape` | `Escape` | `intent:Escape` | both | |
| `q` | `Quit` | `intent:QuitRequested` | both | suppressed in the VS Code/touch host (`vshost`) |
| `l` | `LangPicker` | `—` | tui-only | web uses a toolbar dropdown instead |
| `~` | `ToggleDiag` | `—` | tui-only | web has no diag-ring overlay |
<!-- KEYMAP-TABLE:END -->

## Deliberate divergences

These are **decided, not accidental**. Do not "fix" them without changing this section.

### Bound on one surface only, same capability exists on both

- **`w` (TUI only)** — a vim `:w` save alias. `Ctrl+S` is the cross-surface binding and works
  everywhere; `w` is unbound on the web.
- **`g` / `G` (web only)** — vim first/last row. The TUI reaches the same rows via `Home`/`End`.
- **`+` / `-` (web only)** — nudge. The TUI reaches the same action via `ArrowRight`/`ArrowLeft`.

### Same capability, different affordance

- **Language switch** — TUI binds `l` to a modal picker (`App::lang_picker`, a TUI-local
  construct, not a core `Mode`). The web has no such mode; it uses a toolbar dropdown, which
  the touch UI folds into its "⋯ More" menu.
- **`.json` ↔ `.jsonc` on Convert** — TUI toggles the extension with `Tab` on the Path step
  (`convert_toggle_jsonc_ext`), chosen because `Tab` is not a printable char and so cannot
  collide with typing the path. The web instead offers a `Jsonc` pseudo-tag directly in the
  format `<select>` (`web/convert-dialog.ts`, `uiTagFor`/`extForTag`); JSONC is never a real
  `DocFormat`, it is `Json` with a `.jsonc` extension seeded.
- **Quit** — `q` maps to `QuitRequested` on both, but the web passes `vshost: true` in the VS
  Code and touch hosts, which suppresses it: those surfaces have no "quit the app" concept.

### Present on one surface only

- **`Ctrl+O` open (web only)** — the TUI receives its path as a CLI argument and has no
  in-app file picker.
- **`~` diag-ring overlay (TUI only)** — the web has no diag overlay UI.

## Implementation differences (why modal keys are not cell-checked)

- **Inline edit.** The TUI drives core's edit buffer keystroke by keystroke, so it binds
  `EditCursorLeft`/`Right`/`Home`/`End` and `EditDelete`. The web renders a real
  `<input class="cell-input">` (`web/render.ts`) and calls `e.stopPropagation()` on it
  (`focusInlineEdit`, `web/ui.ts`), so the global `onKey` never sees those keystrokes and the
  **browser** provides cursor motion, selection and forward-delete natively. Commit is a single
  `CommitEdit` on Enter/blur rather than a stream of `EditChar`. Consequence:
  `resolveKeyIntent`'s `Edit` branch is effectively dead on desktop and the `EditCursor*` /
  `EditDelete` members of `web/types.ts`'s `Intent` union are declared but unused — they
  document the core protocol, they are not web dead code to delete.
- **Popup / external editor.** One intent, three presentations: the TUI suspends the alternate
  screen and spawns `$EDITOR`; desktop web opens `#ext-modal`; touch opens a `.ext-sheet`
  bottom sheet. All three are driven by the same `snap.external_edit` async handshake, and
  `E` forces it on any node while `e` only reaches it when core's `edit_target_kind()` returns
  `External` (multiline string / comment).
- **Clipboard-armed guard.** Core's `begin_external_edit` already refuses while the clipboard
  is armed, so hosts need no duplicate check. The TUI additionally raises its own
  `core.clipboard.action-locked` notice before dispatching; touch's `openExternalEdit` does the
  same. Desktop web relies on core alone.
- **Action menu.** Desktop intercepts `OpenActionMenu` to position a real popover
  (`openActionMenuFromKeyboard`); touch and the TUI let the intent through to core's
  `Mode::ActionMenu`.
- **Touch key handling.** `web/touch/app.ts` reuses `resolveKeyIntent` verbatim — no touch-only
  fork — then intercepts a few resolutions to open bottom sheets instead of core sub-modes
  (`ToggleDetail`, `BeginEdit`, `OpenKindSwitch`). `BeginEditExternal` is deliberately *not*
  intercepted: it falls through to `send()` and opens the ext sheet via the snapshot handshake.
- **Page size.** The TUI pages by `terminal_height / 2`. The web has no fixed row height, so
  `treePageStep` derives the visible row count from the scroll-container ratio and halves it —
  same convention, measured differently.

## Related documents

- `docs/reference/TUI.md` — TUI behaviour; `?` overlay text lives in `i18n/*.json` (`tui.help.*`).
- `docs/reference/WEBUI.md` — web/touch behaviour and chrome.
- `web/help-content.ts` — the web `?` overlay text (en + zh-TW, web + VS Code variants).
