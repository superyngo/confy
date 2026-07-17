# confy VS Code extension — M1 design

**Status:** SHIPPED (implemented + merged to main 2026-07-16; manual acceptance checklist deferred — see plan Task 6 Step 5)
**Milestone:** M1 — sideload `.vsix`, personal use. Marketplace publishing is M2.
**Prior art:** the Tauri shell (`crates/confy-tauri` + `web/fs.ts`'s Tauri branch) — this
extension is the third host shell over the same `web/dist` UI + `confy_ffi` wasm Session.

## Goal

Open `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` files inside VS Code with the full confy tree
editor, with native VS Code document behavior (dirty dot, Cmd+S, Cmd+Z, close-confirmation,
hot exit), plus an optional side-by-side **read-only raw preview** that live-updates as the
tree is edited.

**M1 acceptance criteria** (manual, user-verified, from a `.vsix` install — not just F5):

1. Right-click a `.toml` → "Reopen Editor With…" → confy opens the tree UI.
2. Edit a value → tab shows the dirty dot.
3. Cmd+S → file on disk updated (verify in a text editor).
4. Cmd+Z / Cmd+Shift+Z undo/redo the tree edits correctly, webview focused or not.
5. "confy: Open Raw Preview" opens a read-only text view beside the tree that updates
   live on every mutation, with the target language's syntax highlighting.
6. Close with unsaved changes → VS Code's native save/discard prompt.
7. All of the above for at least one file of each backend (TOML, JSON/JSONC, YAML).

## Decisions (from design review)

- **Activation:** custom editor registered with `priority: "option"` — plain text editor
  stays the default. Users who want confy as the default for some glob use VS Code's
  built-in `workbench.editorAssociations` setting; the extension adds no setting of its own.
- **Document ownership:** confy owns the document (approach A, `CustomEditorProvider` with
  a custom document — *not* `CustomTextEditorProvider`). The wasm Session in the webview is
  the single source of truth between open and save; VS Code text buffers are not involved.
- **Sync:** one-way, live, via a `confy-raw://` virtual read-only document
  (`TextDocumentContentProvider`) mirroring `session.serialize()`. No bidirectional text
  sync in M1.
- **Theme:** follow VS Code light/dark only — map the webview's `vscode-light` /
  `vscode-dark` / `vscode-high-contrast*` body classes onto confy's existing
  `:root[data-theme]` light/dark palettes, and react to theme-change. No `--vscode-*`
  variable remapping in M1. The UI's own theme toggle is hidden in this host.
- **i18n:** initialize the Session lang from `vscode.env.language` (`zh-tw` → `zh-TW`,
  anything else → `en`); the in-UI `L` language picker keeps working.

## Architecture

New top-level directory `editors/vscode/` — a TypeScript package parallel to `web/`, not a
cargo workspace member. Two layers, one channel:

```
VS Code extension host (Node)                     webview (browser)
┌──────────────────────────────┐   postMessage   ┌──────────────────────────────┐
│ editorProvider.ts            │◄───────────────►│ web/dist UI (ui.js verbatim) │
│   CustomEditorProvider       │                 │ + confy_ffi wasm Session     │
│   dirty/save/revert/undo/    │                 │ + web/vscode.ts host adapter │
│   backup lifecycle           │                 └──────────────────────────────┘
│ rawPreview.ts                │
│   confy-raw:// read-only     │
│   TextDocumentContentProvider│
│ extension.ts  (activate)     │
└──────────────────────────────┘
```

All file I/O lives in the extension host (`vscode.workspace.fs`); the webview never touches
the filesystem. This is the same capability boundary as the browser (FS Access API) and
Tauri (plugin invoke) hosts — the webview side hides it behind the existing `fs.ts`-style
host-adapter pattern.

### Files

```
editors/vscode/
  package.json        manifest: customEditors (confy.editor, priority "option",
                      filenamePattern *.toml/*.json/*.jsonc/*.yaml/*.yml),
                      commands (confy.openRawPreview), activation events
  src/extension.ts    activate(): register provider + preview + commands
  src/editorProvider.ts  CustomEditorProvider impl (document lifecycle ⇄ messages)
  src/rawPreview.ts   TextDocumentContentProvider for confy-raw:// + open-beside command
  src/messages.ts     host⇄webview message types (hand-written, web/types.ts style)
  media/              build-time copy of web/dist (ui.js, wasm, css, catalogs)
  build.mjs           esbuild extension bundle (cjs, node) + copy web/dist → media/
web/
  vscode.ts           NEW host adapter: detects acquireVsCodeApi(), implements the
                      postMessage protocol, theme mapping, host-capability flags
  ui.ts               boot gains a VS Code host-detection branch (same pattern as isTauri())
```

`web/` changes are deliberately minimal: one adapter module plus a boot branch. The webview
loads the same `ui.js` bundle the browser and Tauri use.

## Message protocol

| direction | message | when |
|---|---|---|
| host → webview | `init {text, name, format, theme, lang}` | editor opened (reply to `ready`) |
| host → webview | `undo` / `redo` | VS Code edit-stack callback → dispatch `Undo`/`Redo` intent |
| host → webview | `save-request {id}` | `saveCustomDocument` / `saveCustomDocumentAs` / `backupCustomDocument` |
| host → webview | `revert {text}` | Revert File → session rebuilt from text |
| host → webview | `theme {dark}` | VS Code theme changed |
| webview → host | `ready` | boot complete |
| webview → host | `edited {dirty, text}` | after every mutation → fire `onDidChangeCustomDocument` + refresh raw preview (`text` = `serialize()`; cheap — token concatenation) |
| webview → host | `save-response {id, text}` | serialize() result; host writes via `workspace.fs` |
| webview → host | `request-undo` / `request-redo` | webview undo keys forward to the host (below) |
| webview → host | `convert-save {text, suggestedName}` | Convert output → host `showSaveDialog` + write |
| webview → host | `parse-error {message}` | initial text failed to parse (below) |

### Undo: single owner

The real history lives in the Session's `History`, but the *entry point* is VS Code alone.
In this host the webview's undo/redo keys do **not** dispatch intents directly — the adapter
posts `request-undo`, the host invokes the workbench `undo` command, and VS Code's custom-
editor edit stack calls back into the provider, which posts `undo` to the webview, which
dispatches `Intent::Undo`. Every `edited` message pushes exactly one edit entry onto VS
Code's stack, so the two stacks stay 1:1 whether Cmd+Z is pressed with the webview focused
or not, and the dirty dot tracks VS Code's own edit counting.

### Save / backup / revert

`saveCustomDocument` → `save-request` → webview replies `save-response` with
`session.serialize()` → host writes the text with `workspace.fs.writeFile` → on success the
host posts `save-ok {id}` and only then does the webview dispatch `Intent::Save` (marks the
session clean); on write failure the session stays dirty (see *Error handling*). Save As
targets the new uri. `backupCustomDocument` (hot exit) runs the same request but writes to
the backup destination and never sends `save-ok`. `revertCustomDocument` reads the file and
posts `revert`.

### Raw preview

`confy.openRawPreview` opens `confy-raw://<encoded fs path>` beside the active confy editor
via `vscode.window.showTextDocument` (the document is read-only by scheme). The provider
caches the latest serialized text per document — refreshed from the `text` carried on every
`edited` message — and fires `onDidChange` so VS Code re-renders. Language
mode is set from the extension so highlighting matches the format.

## Webview integration details

- **CSP:** `script-src` needs `'wasm-unsafe-eval'` (wasm compile in webviews); all assets
  load via `asWebviewUri` with `localResourceRoots` limited to `media/`.
- **Tab hidden:** `retainContextWhenHidden: true` — the Session stays alive in memory; no
  getState/setState serialization in M1.
- **UI trimming in this host:** the webview is bound to one document, so the UI's own
  Open / New / Save-As buttons and the theme toggle are hidden (host-capability flags on the
  adapter, same mechanism as `canSaveAs()` on Tauri mobile). **Convert stays** — its output
  path routes through the host's `showSaveDialog` (`convert-save`).
- **External-edit modal** (the web UI's `$EDITOR` analogue) works unchanged inside the
  webview.

## Error handling

- **Parse failure on open:** the webview shows the parse error plus a "Open in text editor"
  button (`parse-error` → host runs `vscode.commands.executeCommand("vscode.openWith", uri,
  "default")`). The custom editor never white-screens on bad input.
- **External change on disk while open:** ignored in M1 (confy owns the document; no file
  watcher). Documented M2 item.
- **Two confy editors on the same file:** allowed by VS Code; each has its own Session;
  last save wins. Accepted for M1.
- **Save write failure:** surface `workspace.fs` errors via `window.showErrorMessage`; the
  session stays dirty (the `Intent::Save` clean-mark is only dispatched after a successful
  write — host confirms with a `save-ok {id}` ack; on failure the webview skips the
  clean-mark).

## Build & packaging

- `editors/vscode/build.mjs`: esbuild bundles `src/extension.ts` → `dist/extension.js`
  (cjs, `platform: node`, external `vscode`), then copies `web/dist` → `media/`.
  Follow the existing esbuild-on-volume lesson: bundle from a scratchpad copy, copy back.
- `vsce package` produces the sideload `.vsix`; `code --install-extension confy-*.vsix`.
- The web bundle must be built first (existing `cf-build.sh`/`build.mjs` flow); the
  extension build consumes `web/dist` as-is.

## Testing

The extension-host layer is thin glue, so M1 skips `@vscode/test-electron`. Verification:

- **Manual acceptance checklist** = the 7 criteria under *Goal*, run by the user on a real
  `.vsix` install (same model as Android M1's hardware acceptance).
- The wasm/web layers are already covered (`functional_smoke.mjs`, core tests) and are not
  modified beyond the adapter; `tsc` must stay clean across `web/` and `editors/vscode/`.
- `messages.ts` protocol types are shared by both sides at compile time, so drift is a type
  error, not a runtime surprise.

## Non-goals (M1)

- Marketplace publishing (M2: publisher account, listing assets, CI release).
- Reconciling external on-disk changes while a confy editor is open — **superseded by
  M1.5**: on-disk changes to an *open* TextDocument now flow through
  `onDidChangeTextDocument` and are handled (side-by-side typing, undo/redo, revert, git).
- Bidirectional live sync with an editable text buffer (`CustomTextEditorProvider`) —
  **SHIPPED as M1.5 (2026-07-16)** — see
  docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md; the TextDocument
  owns content/dirty/undo, the webview posts whole-serialize edits applied as minimal-span
  WorkspaceEdits, and external changes reload the Session with tree-state restore.
- Full `--vscode-*` theme-variable mapping (light/dark only).
- Extension settings UI / walkthroughs.

## Post-M1 addendum — editor title-bar toggle (2026-07-16)

Two `editor/title` navigation buttons swap the active tab in place between the built-in
text editor and confy (`vscode.openWith` on the same uri in the same group replaces the
tab, since a resource opens at most once per group):

- **Open with confy** (`confy.openWithConfy`, `$(list-tree)`) — shown on matching
  extensions when the active editor is not confy.
- **Reopen as Text Editor** (`confy.reopenAsText`, `$(go-to-file)`) — shown when
  `activeCustomEditorId == 'confy.editor'`.

Since M1.5, both directions carry dirty state seamlessly: the toggle reopens the same
shared `TextDocument` in the other view, so an unsaved edit is neither saved nor prompted
for — it simply appears in the new view, still dirty.
