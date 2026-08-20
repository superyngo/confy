// Message protocol between the VS Code extension host and the confy webview.
// Imported by web/vscode.ts (webview side) and editors/vscode/src/* (host
// side) so protocol drift is a compile error, not a runtime surprise.
// Design: docs/superpowers/specs/2026-07-15-vscode-extension-design.md
// (M1.5: the TextDocument is the single source of truth — see the plan
// docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md).

// The single definition of ConfigFormat — web/host-io.ts re-exports this.
// `.yml` folds to "yaml" and `.jsonc` to "json"; the wire never carries "yml".
export type ConfigFormat = "toml" | "json" | "yaml";

// "auto" tracks VS Code's own active color theme (vscode-light/vscode-dark/
// vscode-high-contrast* body classes, see web/vscode.ts); "light"/"dark" pin
// confy's palette regardless of VS Code's theme.
export type ThemeMode = "auto" | "light" | "dark";

export type HostToWebview =
  // `dirty` rides along because the TextDocument may already be dirty when the
  // confy editor opens (toggle from an unsaved text editor).
  | { type: "init"; text: string; name: string; format: ConfigFormat; theme: ThemeMode; lang: string; dirty: boolean }
  // The document changed under us (side-by-side typing, undo/redo, revert,
  // git). The webview reloads its Session from this text; echoes of the
  // webview's own `edit` are filtered host-side and never arrive here.
  | { type: "text-changed"; text: string; dirty: boolean }
  // The document was saved (any save path) — webview clears its dirty pill.
  | { type: "saved" }
  // Host-menu-driven actions with no keyboard/toolbar entry point of their own
  // in this host (title-bar "…" menu commands): open the Save As/Convert
  // dialog, or the Help overlay on a given tab.
  | { type: "exec"; action: "save-as" | "help" | "about" }
  // Theme picked from the title-bar "…" menu's confy: Theme submenu.
  | { type: "set-theme"; theme: ThemeMode }
  // Language picked from the title-bar "…" menu's Choose Display Language command.
  | { type: "set-lang"; lang: "en" | "zh-TW" }
  // Response to a webview `read-schema-file` request (local $schema
  // resolution — the webview has no filesystem access; only the extension
  // host can read a sibling schema file, relative to `document.uri`).
  | { type: "schema-file"; text: string }
  | { type: "schema-file-error"; message: string }
  // Response to a webview `read-schema-url` request (remote $schema
  // resolution — the webview's CSP `connect-src` blocks arbitrary external
  // fetches, so the extension host, which has unsandboxed Node network
  // access, fetches it instead).
  | { type: "schema-url"; text: string }
  | { type: "schema-url-error"; message: string };

export type WebviewToHost =
  | { type: "ready" }
  // A Session mutation happened: `text` is session.serialize(). The host
  // applies it to the TextDocument as a minimal WorkspaceEdit — VS Code's
  // dirty/undo/save machinery takes over from there.
  | { type: "edit"; text: string }
  // Webview keyboard/toolbar undo/redo/save forward to the workbench, which
  // owns the text document's stacks.
  | { type: "request-undo" }
  | { type: "request-redo" }
  | { type: "request-save" }
  // Convert (and same-format save-a-copy) output: host shows a save dialog.
  | { type: "convert-save"; suggestedName: string; text: string }
  | { type: "parse-error"; message: string }
  // Ask the host to read a schema file by path relative to the open
  // document's directory (absolute paths pass through untouched — same
  // resolution rule as the Tauri host's `readSiblingFile`). Id-less: only
  // one schema fetch is ever in flight per Session at a time.
  | { type: "read-schema-file"; relativePath: string }
  // Ask the host to fetch a remote schema URL — the webview's CSP
  // `connect-src` only allows same-origin webview resources. Id-less: only
  // one schema fetch is ever in flight per Session at a time.
  | { type: "read-schema-url"; url: string };
