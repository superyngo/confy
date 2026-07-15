# confy for VS Code (M1 — sideload)

Structural tree editor for TOML / JSON / JSONC / YAML, embedding the confy
web UI + wasm Session in a custom editor. Design:
`docs/superpowers/specs/2026-07-15-vscode-extension-design.md`.

## Build

1. Build the web bundle first (repo root; esbuild must run from a scratchpad
   copy on this machine — see the plan/CLAUDE.md):
   `crates/confy-ffi: wasm-pack build --target web`, then `web: node build.mjs`
   + assemble `web/dist` (cf-build.sh's copy steps).
2. `cd editors/vscode && npm install && npm run build` (same scratchpad rule).
3. `npm run package` → `confy-vscode-0.1.0.vsix`.

## Install / use

- `code --install-extension confy-vscode-0.1.0.vsix`
- Right-click a `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` tab → "Reopen Editor
  With…" → **confy**. To make confy the default for a glob, use VS Code's
  `workbench.editorAssociations` setting (e.g. `"*.toml": "confy.editor"`).
- Command palette: **confy: Open Raw Preview** — live read-only text mirror
  beside the tree.
- Save/undo/redo/revert are native VS Code (⌘S / ⌘Z / ⌘⇧Z / File > Revert).

Not in M1: Marketplace listing, watching external on-disk edits while open,
editable side-by-side text sync.
