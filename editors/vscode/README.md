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
3. `npm run package` → `confy-vscode-0.3.0.vsix`.

## Install / use

- `code --install-extension confy-vscode-0.3.0.vsix`
- Open a `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` file and click the
  **Open with confy** title-bar button (tree icon) — the tab swaps to confy in
  place, carrying any unsaved edit; inside confy, **Reopen as Text Editor**
  swaps back the same way. (Right-click → "Reopen Editor With…" → **confy**
  still works.) To make confy the default for a glob, use VS Code's
  `workbench.editorAssociations` setting (e.g. `"*.toml": "confy.editor"`).
- **confy: Open Text Editor to the Side** (title-bar button next to **Reopen
  as Text Editor**, or command palette) — the real text editor, editable and
  live in both directions (shared `TextDocument`).
- Save/undo/redo/revert are native VS Code (⌘S / ⌘Z / ⌘⇧Z / File > Revert).
- The confy toolbar header is hidden in this host — **Save As / Convert…**,
  **Help**, **About**, and the **Language** submenu live in the editor
  tab's **"…" More Actions** menu (command palette works too). ⇧⌘S
  (Ctrl-Shift-S) is the keyboard shortcut for Save As / Convert.

M1.5 (shared `TextDocument`) shipped — switching carries unsaved changes;
side-by-side text editing syncs live. M1.6 moved chrome VS Code already owns
(Open/Save/theme/undo/redo) out of the confy header into native VS Code
surfaces, and added Save As / Convert, Help, About, and language commands to
the "…" menu. Still out of scope: Marketplace (M2).
