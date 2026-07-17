// Message protocol between the VS Code extension host and the confy webview.
// Imported by web/vscode.ts (webview side) and editors/vscode/src/* (host
// side) so protocol drift is a compile error, not a runtime surprise.
// Design: docs/superpowers/specs/2026-07-15-vscode-extension-design.md
// (M1.5: the TextDocument is the single source of truth — see the plan
// docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md).

// The single definition of ConfigFormat — web/host-io.ts re-exports this.
// `.yml` folds to "yaml" and `.jsonc` to "json"; the wire never carries "yml".
export type ConfigFormat = "toml" | "json" | "yaml";

export type HostToWebview =
  // `dirty` rides along because the TextDocument may already be dirty when the
  // confy editor opens (toggle from an unsaved text editor).
  | { type: "init"; text: string; name: string; format: ConfigFormat; lang: string; dirty: boolean }
  // The document changed under us (side-by-side typing, undo/redo, revert,
  // git). The webview reloads its Session from this text; echoes of the
  // webview's own `edit` are filtered host-side and never arrive here.
  | { type: "text-changed"; text: string; dirty: boolean }
  // The document was saved (any save path) — webview clears its dirty pill.
  | { type: "saved" };

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
  | { type: "parse-error"; message: string };
