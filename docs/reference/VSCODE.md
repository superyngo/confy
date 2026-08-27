# VSCODE.md — confy VS Code extension host (`editors/vscode`)

`editors/vscode/` is a third host shell (M1.5+, published to the VS Marketplace and Open
VSX — see `.github/workflows/publish-vscode.yml`) over the same `web/` bundle `WEBUI.md`
documents. A `CustomTextEditorProvider`
makes VS Code's own `TextDocument` the single source of truth for content, dirty state,
undo stack, save, revert, and hot exit; the webview runs the unmodified `web/dist` bundle
plus `web/vscode.ts`'s adapter, and the Session there is a *view* over that document.
Every behavior difference from the browser/Tauri hosts is gated in `ui.ts` on `VSHOST`
(`isVsCode()` — true only when `acquireVsCodeApi` exists), so the pure-browser and Tauri
builds are byte-identical when it is absent. See `editors/vscode/README.md` for
build/install/use, and CLAUDE.md's module map for the extension-host-side file layout.

Design record: `docs/superpowers/specs/2026-07-15-vscode-extension-design.md`. M1.5
rebased the provider from `CustomEditorProvider` onto `CustomTextEditorProvider`
(plan: `docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md`); 0.2.1
fixed the title-bar toggle to truly swap the tab in place and promoted "Open Text Editor
to the Side" to an `editor/title` icon button. M1.6 (0.3.0) hid the whole confy toolbar
header in this host and moved Save As/Convert, Help, About, and language to the editor
title's "…" More Actions menu (see Chrome trimming below).

## Chrome trimming

`document.body.classList.add("host-vscode")` on boot. The full header/filter-row trimming
and relocation rules for this host (which controls hide, which relocate, and the exact
CSS/JS mechanism for each) are documented once, alongside every other host, in
**`CHROME.md`** — not restated here. In short: the whole `header.toolbar` is hidden (the
document is tab-bound — VS Code owns Open — destination picks are native save dialogs, the
theme defaults to following VS Code's own theme, overridable via the "…" menu's Theme
submenu below), Undo/Redo get no replacement UI (keyboard z / y / ⌘S already forward to the
workbench via `request-undo`/`request-redo`/`request-save`), and the filter row
(search/type-filter/Expand-Collapse, plus the Raw/Tree toggle relocated in from the header)
stays.

Save As / Convert, Help, About, language, and theme — with no toolbar button left to click —
move to the editor title's **"…" More Actions** menu: three commands (`confy.saveAsConvert`,
`confy.help`, `confy.about`) each posting an `exec` message (below) to the active confy
webview panel (`ConfyEditorProvider.postToActive`, tracked alongside `activeDocument`), plus
two native submenus (`contributes.submenus`): **language** (id `confy.language`, two entries
`confy.langEnglish`/`confy.langZhTw`) and **theme** (id `confy.theme`, three entries
`confy.themeAuto`/`confy.themeLight`/`confy.themeDark`) — all five hidden from the command
palette, picking directly with no intermediate QuickPick. Save As / Convert also has a
keyboard shortcut, **⇧⌘S / Ctrl-Shift-S**, contributed as `contributes.keybindings` (`when:
activeCustomEditorId == 'confy.editor'`) rebinding it straight to `confy.saveAsConvert`.
This is an extension-side rebind, not a webview `keydown` intercept: the workbench's
keybinding service claims ⇧⌘S before it ever reaches the webview's DOM (confirmed in
testing — VS Code's own built-in Save As fired instead of an earlier webview-side
`onKey` intercept), so overriding the binding at the `contributes.keybindings` level is
the only place this can actually be caught. A language pick persists in
`context.globalState["confy.lang"]` and posts `set-lang`; that same key is read on the next
`ready` handshake and **overrides `vscode.env.language`** once set (VS Code's display
language is otherwise still authoritative — same principle as theme).

## Theme

`web/vscode-protocol.ts`'s `ThemeMode` (`"auto" | "light" | "dark"`) rides on `init`'s `theme`
field and the `set-theme` message. `"auto"` is the default: `web/vscode.ts`'s
`trackVsCodeTheme("auto")` runs a `MutationObserver` on `document.body`'s class list, mapping
VS Code's `vscode-dark`/`vscode-light`/`vscode-high-contrast(-light)` stamps onto confy's own
`:root[data-theme]`. Picking **Light** or **Dark** from the "…" menu's **confy: Theme**
submenu (`confy.themeAuto`/`confy.themeLight`/`confy.themeDark`, `extension.ts`) persists the
choice in `context.globalState["confy.theme"]`, posts `set-theme` to the active webview, and
pins `document.documentElement.dataset.theme` directly — the MutationObserver only runs while
the mode is `"auto"`. The persisted choice rides back on every `init` (same principle as
`set-lang`/`lang`).

## Message protocol

`web/vscode-protocol.ts`, single source of truth for both sides:

| Direction | Message | Purpose |
|---|---|---|
| host→webview | `init { text, name, format, theme, lang, dirty }` | Initial state; `theme`/`lang` ride along persisted from `context.globalState` (VS Code's own theme/display language are otherwise authoritative — see Theme above); `dirty` rides along because the TextDocument may already be dirty when the confy editor opens (toggle from an unsaved text editor) |
| host→webview | `text-changed { text, dirty }` | The document changed under us — side-by-side typing (150ms debounce), undo/redo, revert, git. Echoes of the webview's own `edit` are filtered host-side (via `webviewText`) and never arrive here |
| host→webview | `saved` | The document was saved (any save path) — webview clears its dirty pill |
| host→webview | `exec { action: "save-as" \| "help" \| "about" }` | "…" menu commands with no in-webview chrome left to click: open the Save/Convert dialog, or the Help overlay on the Help/About tab. Ignored if no session or `staleTree` |
| host→webview | `set-theme { theme }` | Theme picked from the "…" menu's confy: Theme submenu; calls the existing `trackVsCodeTheme(theme)` |
| host→webview | `set-lang { lang }` | Language picked from the "…" menu's language submenu; calls the existing `chooseLang(lang)` |
| webview→host | `ready` | Boot handshake |
| webview→host | `edit { text }` | A Session mutation happened: `text` is `session.serialize()`. The host applies it as a minimal-span `WorkspaceEdit` (common prefix/suffix trim) — VS Code's dirty/undo/save machinery takes over from there |
| webview→host | `request-undo` / `request-redo` | Webview keyboard/toolbar undo/redo forward to the workbench, which owns the text document's stacks |
| webview→host | `request-save` | Webview Save / ⌘S → workbench save |
| webview→host | `read-schema-file { relativePath }` | Local `$schema` file read: the webview has **no filesystem access**, so the host resolves the path (absolute, or relative to `document.uri`'s directory) and reads it via `vscode.workspace.fs`. Reply: `schema-file { text }` or `schema-file-error { message }` |
| webview→host | `read-schema-url { url }` | Remote `$schema: "https://…"` fetch: the webview's CSP `connect-src ${webview.cspSource}` blocks external origins, so the unsandboxed host fetches it instead (Node `fetch`, error format `HTTP {status} {statusText}` — parity with the browser/Tauri hosts). Reply: `schema-url { text }` or `schema-url-error { message }` |
| webview→host | `parse-error { message }` | Initial text failed to parse: host offers the default text editor instead of a white screen |
 
**Schema file/URL reads go through the host** (added 0.20.x, `31f86ba`): `web/fs.ts`'s
`readSiblingFile` and `web/host-io.ts`'s `resolveSchemaFetchRequest` branch to
`web/vscode.ts`'s `requestSchemaFile`/`requestSchemaUrl` when `isVsCode()` — before
this, local and remote `$schema` loading both silently failed inside the extension
(no Tauri bridge, CSP-blocked `fetch`). Parity with the other hosts only: no CSP
relaxation, no `../` traversal sandboxing, no timeout/content-type/redirect checks.
This serves the **custom editor webview**; the native text editors' diagnostics use
the separate extension-host pipeline below instead.

**Echo suppression.** The host tracks `webviewText` (last text the webview is known to
hold — set on `ready`'s `init` reply, on every received `edit`, and on every posted
`text-changed`). An `onDidChangeTextDocument` whose result equals `webviewText` is the
echo of the host's own `applyEdit` and is not posted back — this is what lets a shared
`TextDocument` avoid an infinite edit↔text-changed loop.

**Edit-mode gating eliminates the M1 add→Esc wart.** The webview's `notifyHost` defers
posting `edit` while `Mode::Edit` is active: an `a`-add's immediate Insert never reaches
the host; Esc rolls the Session back to `lastNotifyText` and nothing is posted (no dirty,
no undo entry), while a commit posts one single `edit` for the whole add. A side-by-side
text editor doesn't see in-flight inline-edit/nudge churn until commit; a save/hot-exit
during an in-flight edit stores the text *without* the transient placeholder.

**Stale-tree pause.** While side-by-side text doesn't parse, `reloadFromHost` leaves the
last-good Session in place, sets `staleTree`, and the webview dims the tree
(`body.stale-tree` CSS — browsable/copyable but visibly paused) and shows a status
message (`web.vscode.staleTree`), and stops posting `edit` (so a stale tree can never
clobber newer raw text). Tree edits made during the stale window are dropped on the next
successful reload — a rare, accepted wart. The pause clears the moment a later
`text-changed` parses.

**Expansion + cursor restore on `text-changed`.** A successful reload captures the
expanded-branch set and cursor path before rebuilding the Session, then replays them by
path afterward (`captureTreeState`/`restoreTreeState`) — parents precede children in row
order, so expanding in order always finds the child row once its parent is open. An
in-flight inline edit, modal, selection, or filter is discarded by the reload; this is
accepted (it matches revert semantics).

## Title-bar tab swap (0.2.1)

The **Open with confy** / **Reopen as Text Editor** title-bar buttons
(`confy.openWithConfy`/`confy.reopenAsText`) must truly replace the active tab, not
stack a second one beside it. VS Code tracks tabs by `(uri, viewType)` identity, so a
plain `vscode.openWith` call for a different viewType leaves the previous tab open. The
fix (`extension.ts`'s `swapEditorKind()`): open the new view **first** (so the shared
`TextDocument` keeps at least one reference), then close the old tab — this is the
closest an extension can get to the built-in "Reopen Editor With…" swap using public
API. **Known limitation:** unlike VS Code's own internal editor-replace (used by
"Reopen Editor With…" and, as of 1.132, the `breadcrumbs.showEditorType` dropdown),
`tabGroups.close()` still shows the unsaved-changes confirmation dialog on a dirty
document — its API contract has no carve-out for another tab sharing the same
document, and no public API exposes the in-place swap VS Code's core UI uses instead.
Verified live against VS Code 1.134 (2026-08): the confy title-bar buttons prompt on a
dirty document; the native `breadcrumbs.showEditorType` dropdown (opt-in, off by
default) does not. Users who want a prompt-free swap can enable
`breadcrumbs.showEditorType` as an alternative to the title-bar buttons — no change
needed on confy's side, since the extension already satisfies it via
`contributes.customEditors`. **"Open Text Editor to the
Side"** (`confy.openTextBeside`) is a separate, unaffected command — it always opens a
genuinely new tab in `ViewColumn.Beside` and is contributed as an `editor/title` icon
button next to "Reopen as Text Editor" (not a button inside the confy panel itself).

## Outline & breadcrumbs (native text editors)

`activate` also registers a `DocumentSymbolProvider` (`outlineProvider.ts`) for
`**/*.toml`, `**/*.yaml`, and `**/*.yml`: the extension host itself loads the wasm core
(the same `media/pkg/confy_ffi.js` + `confy_ffi_bg.wasm` the webview stages; passed as
raw bytes, so the `--target web` glue calls `WebAssembly.instantiate` directly instead
of `fetch()`) and maps the read-only `ConfySession.outline()` tree onto hierarchical
`DocumentSymbol`s — the core's UTF-8 byte offsets (rowan) converted to VS Code's
UTF-16 positions (`byteToPosition.ts`, unit-tested via plain `node
--experimental-strip-types`). This populates the Explorer's Outline view, ⇧⌘O
go-to-symbol, and the breadcrumb bar, with scalar values as symbol detail. A malformed
or mid-edit document never errors — the Outline simply goes empty. Scope: VS Code's
**native** text editor only; confy's own custom editor tab is a webview and gets no
Outline/breadcrumbs (spec's Platform constraint). Because a runtime-only
`registerDocumentSymbolProvider` has no declarative `contributes` equivalent,
`package.json` carries an explicit `"activationEvents": ["onStartupFinished"]`.

Range policy note (2026-08-26): the provider preserves core `Node.text_range`
anchoring semantics (ADR 0006) but widens the *editor-facing* parent
`DocumentSymbol.range` to include all descendant symbol ranges. This keeps VS Code's
breadcrumb parent-chain resolution stable for TOML scattered/nested table layouts
such as `[workspace]` + `[workspace.package]`, where strict source anchoring can
produce non-enclosing parent/child spans.

## Schema diagnostics & hover (native text editors)

For the same TOML/YAML native-editor scope, `extension.ts` wires:

- `SchemaSessionManager` (one persistent `ConfySession` per open document)
- diagnostics (`confy-schema` `DiagnosticCollection`) from `schema_violations()`
- hover provider (`schemaHoverProvider.ts`) from `schema_hint(path)`

This pipeline is deliberately independent of the custom editor webview and runs in
the extension host process.

## Wasm loader boundary (CJS host vs ESM glue)

The extension entry is CJS-bundled (`dist/extension.js`), while wasm-bindgen's
`--target web` glue (`media/pkg/confy_ffi.js`) is ESM. The loader in
`src/wasmSession.ts` therefore **must not** statically import the glue at bundle time.

Current strategy (required):

1. Resolve `media/pkg/confy_ffi.js` from `context.extensionUri`.
2. Convert to an absolute file URL (`pathToFileURL(...).href`).
3. Dynamic `import(fileUrl)` at runtime.
4. Initialize with raw wasm bytes (`ffi.default({ module_or_path: bytes })`).

This avoids CJS/ESM import-shim breakage and removed the previous runtime LinkError
(`Import "./confy_ffi_bg.js" ... requires a callable`) and build-time `import.meta`
warning.

## Build/test workflow (regression guard)

`web/build.mjs` now assembles a fresh runtime `web/dist` (including `dist/pkg/*`) on
every run, so `editors/vscode/build.mjs` always stages current artifacts into `media/`.

Recommended local verification for VS Code host changes:

1. `cd crates/confy-ffi && wasm-pack build --target web`
2. `cd web && node build.mjs`
3. `cd editors/vscode && npm run check && npm run build && npm run integration-test`

`npm run integration-test` uses `@vscode/test-electron` and asserts native-editor
DocumentSymbols/diagnostics/hover behavior, catching the exact class of silent regressions
that manual clicking can miss.

## Publishing (M2)

Publisher/namespace is **`wenanlin`** on both registries — Open VSX requires its
namespace to match `package.json`'s `publisher` field, so the two can't diverge without
maintaining separate IDs. `.github/workflows/publish-vscode.yml` (dispatched by
`publish-gate.yml` after `release.yml` succeeds on an app `v*.*.*` tag) builds, packages, and publishes to both from two repo secrets:
`VSCE_PAT` (Azure DevOps PAT, org scope "All accessible organizations", Marketplace →
Manage; the org lives at `dev.azure.com/wenanlin`) and `OVSX_PAT` (Open VSX access
token — requires the account to have signed the Eclipse Foundation Open VSX Publisher
Agreement, which itself requires an Eclipse Foundation account with GitHub linked). A
freshly published version isn't visible immediately: VS Marketplace shows "Verifying"
until its malware scan clears, and Open VSX activates asynchronously — both normally
within minutes. Release steps: `editors/vscode/README.md` § Publishing a new version.

## Boot-path localStorage guards

`host-io.ts`'s `initTheme`/`toggleTheme` and `i18n.ts`'s `getLang`/`setLang` all wrap
`localStorage` access in `try/catch` — a sandboxed webview may throw on any access, and
these run on the boot path before `ready` is even posted, so an unguarded throw would
white-screen before the host ever hears from the webview. The guards are
behavior-neutral for the browser/Tauri hosts and are **not** `VSHOST`-gated. Persistence
unreliability in webviews is accepted for M1 — theme comes from the VS Code observer
regardless, and lang re-arrives on every `init`.
