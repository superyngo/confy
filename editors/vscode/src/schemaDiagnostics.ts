import type { ViolationView } from "./wasmSession.js";

export interface DiagnosticDescriptor {
  startByte: number;
  endByte: number;
  message: string;
}

/** Build the Problems-panel entries for one document, as plain byte-range
 * descriptors (no `vscode` import — kept pure and testable under plain
 * `node`; the caller converts each descriptor to a `vscode.Diagnostic` with
 * `DiagnosticSeverity.Warning`, never `Error` — Violations are a documented
 * Soft constraint, CONTEXT.md § Schema). A violation with no resolvable
 * `text_range` is dropped rather than guessed at. `loadError` becomes one
 * additional line-0 descriptor — the Problems-panel piece of `load_error`
 * UI for this host (web/touch/TUI instead surface it as a `Warn`-severity
 * host notice — `web.host.schema.load-error`/`tui.host.schema-load-error`
 * — via their own toast/status-line mechanisms). */
export function buildSchemaDiagnostics(
  violations: ViolationView[],
  loadError: string | undefined,
): DiagnosticDescriptor[] {
  const out: DiagnosticDescriptor[] = [];
  for (const v of violations) {
    if (!v.text_range) continue;
    out.push({ startByte: v.text_range[0], endByte: v.text_range[1], message: v.message });
  }
  if (loadError) out.push({ startByte: 0, endByte: 0, message: loadError });
  return out;
}
