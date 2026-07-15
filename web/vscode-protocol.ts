// Message protocol between the VS Code extension host and the confy webview.
// Imported by web/vscode.ts (webview side) and editors/vscode/src/* (host
// side) so protocol drift is a compile error, not a runtime surprise.
// Design: docs/superpowers/specs/2026-07-15-vscode-extension-design.md.

// The single definition of ConfigFormat — web/host-io.ts re-exports this
// (its old local 3-value union is deleted), so the web layer and the
// extension host cannot drift. `.yml` folds to "yaml" and `.jsonc` to "json"
// at the filename→format mapping, exactly as host-io's formatFromName does;
// the wire never carries "yml".
export type ConfigFormat = "toml" | "json" | "yaml";

export type HostToWebview =
  | { type: "init"; text: string; name: string; format: ConfigFormat; lang: string }
  // VS Code edit-stack callbacks — the *only* way an undo/redo reaches the
  // Session in this host (single-owner rule, spec §Undo).
  | { type: "undo" }
  | { type: "redo" }
  // Save/Save As/backup all fetch the serialized text through this.
  | { type: "save-request"; id: number }
  // Sent only after workspace.fs.writeFile succeeded; the webview marks the
  // session clean (Intent::Save) only on this ack.
  | { type: "save-ok"; id: number }
  | { type: "revert"; text: string };

export type WebviewToHost =
  | { type: "ready" }
  // A user-initiated mutation: host pushes one VS Code edit entry + refreshes
  // the raw preview. `text` is session.serialize() (cheap token concat).
  // `dirty` is the Session's own dirty bit; the host derives dirty from its
  // edit stack and deliberately ignores it — it rides along as a diagnostic
  // (compare against the tab dot during acceptance) and for future
  // bidirectional-sync milestones. Grilling decision: keep, not dead weight.
  | { type: "edited"; dirty: boolean; text: string }
  // A host-initiated change landed (undo/redo/revert/save-ok): refresh the
  // preview/dirty mirror but do NOT push an edit entry.
  | { type: "synced"; dirty: boolean; text: string }
  // The Session rolled back its newest history entry WITHOUT a host undo
  // (add→Esc via History::cancel_last, detected as a history_len decrease):
  // mirror text like `synced` AND neuter the newest live VS Code edit entry
  // so popping it later doesn't undo an older, wrong Session edit.
  | { type: "edit-cancelled"; dirty: boolean; text: string }
  | { type: "save-response"; id: number; text: string }
  // Webview keyboard/toolbar undo forwards to the host so VS Code's stack
  // stays the single entry point.
  | { type: "request-undo" }
  | { type: "request-redo" }
  // Webview Save button / ⌘S inside the webview → workbench save.
  | { type: "request-save" }
  // Convert (and same-format save-a-copy) output: host shows a save dialog.
  | { type: "convert-save"; suggestedName: string; text: string }
  | { type: "parse-error"; message: string };
