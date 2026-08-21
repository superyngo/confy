import * as path from "node:path";

/** Resolve a schema hint's `Local` source against the directory of the
 * document that referenced it — mirrors `web/fs.ts`'s `readSiblingFile`
 * resolution rule (bare/relative paths resolve against the open file's
 * directory; absolute paths pass through). Uses Node's own `path.isAbsolute`
 * (authoritative for the extension host's OS) rather than reimplementing
 * `web/fs.ts`'s manual POSIX/Windows/UNC regex — the extension host has
 * direct, unsandboxed `fs` access and does not need the webview's
 * `read-schema-file` message round trip (design §"Coexistence"/§"Shared
 * sync schema steps"). */
export function resolveLocalSchemaPath(currentFilePath: string, relativeOrAbsolute: string): string {
  if (path.isAbsolute(relativeOrAbsolute)) return relativeOrAbsolute;
  return path.resolve(path.dirname(currentFilePath), relativeOrAbsolute);
}
