# confy for VS Code (M1.5 — sideload)

Structural tree editor for TOML / JSON / JSONC / YAML, embedding the confy
web UI + wasm Session in a custom editor. Design:
`docs/superpowers/specs/2026-07-15-vscode-extension-design.md`.

## Build

1. Build the web bundle first (repo root; esbuild must run from a scratchpad
   copy on this machine — see the plan/CLAUDE.md):
   `crates/confy-ffi: wasm-pack build --target web`, then `web: node build.mjs`
   + assemble `web/dist` (cf-build.sh's copy steps).
2. `cd editors/vscode && npm install && npm run build` (same scratchpad rule).
3. `npm run package` → `confy-vscode-0.2.1.vsix`.

## Install / use

- `code --install-extension confy-vscode-0.2.1.vsix`
- Open a `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` file and click the
  **Open with confy** title-bar button (tree icon) — the tab swaps to confy in
  place, carrying any unsaved edit; inside confy, **Reopen as Text Editor**
  swaps back the same way. (Right-click → "Reopen Editor With…" → **confy**
  still works.) To make confy the default for a glob, use VS Code's
  `workbench.editorAssociations` setting (e.g. `"*.toml": "confy.editor"`).
- Command palette: **confy: Open Text Editor to the Side** — the real text
  editor, editable and live in both directions (shared `TextDocument`). Also
  available as a toolbar button inside confy (the split-rectangle icon,
  VS Code host only).
- Save/undo/redo/revert are native VS Code (⌘S / ⌘Z / ⌘⇧Z / File > Revert).

M1.5 (shared `TextDocument`) shipped — switching carries unsaved changes;
side-by-side text editing syncs live. Still out of scope: Marketplace (M2).
