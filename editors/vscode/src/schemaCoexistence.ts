// editors/vscode/src/schemaCoexistence.ts
export type SchemaLanguage = "toml" | "yaml";

const COEXISTING_EXTENSIONS: Record<SchemaLanguage, string> = {
  toml: "tamasfe.even-better-toml",
  yaml: "redhat.vscode-yaml",
};

/** Whether confy should defer Diagnostics registration for `language` — true
 * when that language's established schema-aware extension is *installed*.
 * Deliberately checks installed, not active: confy's own `onStartupFinished`
 * activation can race ahead of the other extension's typically-lazy
 * `onLanguage:*` activation, so `isActive` risks a false negative at the
 * moment this check runs (spec §"Coexistence"). `isInstalled` is injected so
 * this stays testable without a real `vscode.extensions` API — callers pass
 * `(id) => vscode.extensions.getExtension(id) !== undefined`. */
export function isDiagnosticsDeferred(
  language: SchemaLanguage,
  isInstalled: (extensionId: string) => boolean,
): boolean {
  return isInstalled(COEXISTING_EXTENSIONS[language]);
}
