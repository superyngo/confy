# confy for VS Code

Structural tree editor for TOML / JSON / JSONC / YAML, embedding the confy
web UI + wasm Session in a custom editor. Design:
`docs/superpowers/specs/2026-07-15-vscode-extension-design.md`.

## Install

- Marketplace: search **confy** in VS Code's Extensions view, or install
  [wenanlin.confy-vscode](https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode).
- VSCodium / Cursor / Windsurf etc.: [Open VSX listing](https://open-vsx.org/extension/wenanlin/confy-vscode).
- Sideload a built `.vsix`: `code --install-extension confy-vscode-<version>.vsix` (see Build).

## Build

1. Build the web bundle first (repo root; esbuild must run from a scratchpad
   copy on this machine — see the plan/CLAUDE.md):
   `crates/confy-ffi: wasm-pack build --target web`, then `web: node build.mjs`
   (this now also assembles fresh `web/dist`, including `dist/pkg/*`).
2. `cd editors/vscode && npm install && npm run build` (same scratchpad rule).
3. `npm run package` → `confy-vscode-<version>.vsix`.

## Native text-editor features

Beyond the custom editor webview, the extension also enriches VS Code's native
TOML/YAML text editors with:

- Outline/breadcrumb symbols (`DocumentSymbolProvider`)
- Schema diagnostics in Problems (warning-only)
- Schema-aware hover hints

All three run in the extension host process and are backed by the same wasm core.

The **custom editor webview** also loads `$schema` references (local files and
`https://` URLs) — the webview itself has no filesystem access and its CSP blocks
external fetches, so both go through `read-schema-file`/`read-schema-url` message
round trips to the extension host (see VSCODE.md § Message protocol).

## Integration testing

Run `npm run integration-test` in `editors/vscode/` to execute extension-host tests
with `@vscode/test-electron` against fixture TOML/schema files. This is the primary
regression guard for native editor symbol/diagnostic/hover behavior.

## Publishing a new version

1. Bump `version` in `editors/vscode/package.json` (normally done together
   with the app's own release version bump in the root `chore: release
   vX.Y.Z` commit).
2. Cut the app release as usual (`git tag vX.Y.Z && git push --tags`).
3. Once `.github/workflows/release.yml` succeeds, `publish-gate.yml` pauses
   for one manual approval (`publish-gate` environment), then dispatches
   `.github/workflows/publish-vscode.yml` with that tag — it checks out the
   tag, verifies `package.json`'s version matches it, and publishes to the
   VS Marketplace + Open VSX (account/secret setup: `VSCODE.md` § Publishing).
4. A manual `gh workflow run publish-vscode.yml -f tag=vX.Y.Z -f
   dry_run=true` builds + packages without publishing — useful as a dry run.

## Use

- Open a `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` file and click the
  **Open with confy** title-bar button (tree icon) — the tab swaps to confy in
  place, carrying any unsaved edit; inside confy, **Reopen as Text Editor**
  swaps back the same way. (Right-click → "Reopen Editor With…" → **confy**
  still works.) To make confy the default for a glob, use VS Code's
  `workbench.editorAssociations` setting (e.g. `"*.toml": "confy.editor"`).
- **confy: Open Text Editor to the Side** (title-bar button next to **Reopen
  as Text Editor**, or command palette) — the real text editor, editable and
  live in both directions (shared `TextDocument`).
- Save/undo/redo/revert are native VS Code (⌘S / ⌘Z / ⌘⇧Z / File > Revert),
  backed by a shared `TextDocument` — switching editors carries unsaved
  changes, and side-by-side text editing syncs live in both directions.
- The confy toolbar header is hidden in this host — **Save As / Convert…**,
  **Help**, **About**, and the **Language** submenu live in the editor
  tab's **"…" More Actions** menu (command palette works too). ⇧⌘S
  (Ctrl-Shift-S) is the keyboard shortcut for Save As / Convert.
