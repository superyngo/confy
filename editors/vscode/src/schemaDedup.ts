// editors/vscode/src/schemaDedup.ts
import type { SchemaSource, SchemaStatus } from "../../../web/types.js";

function sameSchemaSource(a: SchemaSource, b: SchemaSource): boolean {
  if ("Local" in a && "Local" in b) return a.Local === b.Local;
  if ("Url" in a && "Url" in b) return a.Url === b.Url;
  return false;
}

/** Whether a freshly `DetectSchema`-detected source requires the host to
 * (re)fetch/read and dispatch `SchemaLoaded` — confy-core does not dedup
 * this itself (`Intent::DetectSchema`'s doc comment): `apply_schema_text`
 * unconditionally recompiles the validator every call. `false` only when
 * the same source is already loaded *and* that load actually succeeded —
 * a previous failure (`load_error` set) retries on every reparse rather
 * than getting stuck (ADR 0007's "host owns dedup" consequence). */
export function needsSchemaReload(
  detected: SchemaSource | undefined,
  loaded: SchemaSource | undefined,
  status: SchemaStatus | undefined,
): boolean {
  if (!detected) return false;
  if (!loaded || !sameSchemaSource(detected, loaded)) return true;
  return status?.load_error != null;
}
