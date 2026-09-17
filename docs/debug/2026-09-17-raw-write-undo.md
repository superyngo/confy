# Raw write mode: undo/redo routing — investigation findings
Status: Resolved (2026-09-17)

Date: 2026-09-17. Trigger: user question — (1) in "pane edit mode", wire undo/redo to the
`<textarea>`'s native text undo instead of the current locked tree-node history; (2) would that
also fix VS Code's missing Ctrl+Z/Y in that mode?

Terminology: the term is **Raw write mode** (`rawState: "write"`) — the **Raw pane**'s editable
state, holding a **document buffer** (`docs/reference/glossary.md`). "pane edit mode" is not a
repo term.

## Measured today (browser, real bundle on http://127.0.0.1:8080/?ui=desktop, Chromium)

| Gesture in Raw write mode | Observed |
|---|---|
| Physical ⌘Z (CDP `Input.dispatchKeyEvent` with `commands:["undo"]`) | **native textarea undo already works** — typed `ZZTOP` removed; nothing in `ui.ts` intercepts it (`onRawEditKey` handles only ⌘↩/⌘S/Esc; the `document.body` delegation early-returns on a writable TEXTAREA, `ui.ts:2606`) |
| `document.execCommand("undo")` with `#rawEdit` focused | returns `true`, undoes the typing (`QQMARK` removed) — a viable route for a re-wired button |
| Toolbar `#btnUndo` click | buffer unchanged; status = "action disabled while the whole file is open for editing — apply or cancel that edit first" (`core.document.edit-locked`) |

Note: synthetic `page.keyboard` ⌘Z does **not** undo (Chromium needs the native editing
command); that is a harness artifact, not app behavior.

Cause of the button behavior: `uiUndo`/`uiRedo` (`web/ui.ts:1640-1655`) always dispatch core
`Undo`/`Redo`, and core refuses both while an empty-path `pending_external_edit` is in flight
(`guard_document_edit_locked`, `session.rs:970`; `undo_redo.rs:11,44`) — spec R24, deliberate.

Affordances that therefore misbehave in write mode: desktop `#btnUndo`/`#btnRedo`, the `⋯`
overflow rows (`TOOLBAR_ENTRIES`, `ui.ts:2400`), Tauri's native Edit-menu Undo/Redo
(`web/menu.ts:352`), touch's `data-act="undo"/"redo"` while the external-edit sheet is open
(`web/touch/app.ts:2086`).

## Boundary that makes native undo safe

Every programmatic `#rawEdit.value` write (`enterRawWrite` 415, `applyRawEdit` 476,
`revertRawEdit` 785) clears the browser's native undo stack, so native undo can never cross an
Apply — i.e. it cannot swap committed document text under the open buffer, which is exactly what
R24's lock exists to prevent. Consequence to document: right after an Apply, ↶ is a no-op.

## VS Code (question 2): the change would be a no-op there

Evidence from the bundled real app (`editors/vscode/.vscode-test/vscode-darwin-arm64-1.137.0`):

1. `web/style.css:798,815`: `host-vscode` hides `header.toolbar` **and** `#histGroup` → there
   are no Undo/Redo buttons (and no reachable `⋯` rows) in that host to re-wire.
2. `…/webview/browser/pre/index.html:595,658`: the webview preload `preventDefault()`s
   ⌘/Ctrl+Z and ⌘/Ctrl+Y (keyCode 90/89) and forwards `did-keydown` to the workbench.
3. `workbench.desktop.main.js` @9182830: `UndoCommand`/`RedoCommand` get a **priority-100
   "webview"** implementation — if `IWebviewService.activeWebview?.isFocused`, it calls
   `webview.undo()` → `_send("execCommand","undo")` → preload runs
   `contentDocument.execCommand("undo")` (@19306870). The "editor" implementation is priority 90.
   ⇒ with the webview focused, ⌘Z in write mode **already performs a native textarea undo**, not
   a `TextDocument` undo. (Hypothesis; needs one manual check in a real window.)
4. Default keybindings (@404864/@405212): `undo` primary `2104` = ⌘/Ctrl+Z; `redo` primary
   `2103` = Ctrl+Y with `mac.primary 3128` = ⇧⌘Z. ⇒ on macOS **⌘Y is unbound** and the preload
   swallows it → dead key. On Windows/Linux Ctrl+Y → redo → same webview route.

Follow-on finding to verify: in VS Code **tree** mode the same priority-100 route likely makes
⌘Z a silent no-op (execCommand on a non-editable focus), so `docs/reference/VSCODE.md:30`
("keyboard z / y / ⌘S already forward to the workbench") holds only for the bare `z`/`y` keys.

## What shipped (`41eb0fb`, docs `65bf689`)

The VS Code hypothesis above was **refuted by the user's own test**: ⌘X/C/V work in that host
(Electron's native clipboard roles) while ⌘Z/⌘Y did nothing, so the priority-100 "webview"
`undo` implementation never reaches the webview's textarea. The fix therefore had to be
webview-side and host-neutral, not an affordance re-wiring alone:

- `web/ui.ts`: `rawBufferHistory(dir)` drives `#rawEdit`'s own history via
  `document.execCommand`; `onRawEditKey` handles ⌘Z / ⇧⌘Z / ⌘Y and calls `stopPropagation`
  (acting *before* VS Code's preload can forward the chord); `uiUndo`/`uiRedo` take the same
  branch, so the toolbar ↶/↷ and the ⋯ rows follow.
- `web/menu.ts`: `MenuDeps` gains `undo`/`redo` (Tauri's native Edit menu no longer calls
  `send("Undo")` directly).
- `web/touch/app.ts`: `touchHistory(dir)` — same rule for the external-edit sheet, keeping the
  armed-clipboard guard; toolbar buttons, menu-sheet rows and external-keyboard `z`/`y` route
  through it. `web/touch-modal-lock.spec.mjs` updated to assert that routing.
- Docs: `KEYMAP.md` (a Raw-write row), `WEBUI.md`, `VSCODE.md` (new section; its "⌘Z forwards
  to the workbench" claim corrected to the bare keys only), `CHANGELOG.md`.

No `editors/vscode/package.json` keybinding was needed: confy handles ⌘Y itself before the
preload's `preventDefault`, which also closes the macOS dead key without touching VS Code's
own bindings. R24 stays intact (a programmatic `.value` write clears the native stack, so
buffer undo cannot cross an Apply).

Verification: real web bundle in Chromium — ⌘Z undoes, ⇧⌘Z redoes, ↶/↷ operate the buffer
with no `core.document.edit-locked` notice, bare `z`/`y` still type, tree mode still reports
"nothing to undo"; plus a real VS Code window (user-confirmed 2026-09-17): the chords edit the
buffer and leave the side-by-side text and its dirty state alone.
