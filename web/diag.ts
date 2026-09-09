// `?diag=1` console trace — shared by both web orchestrators.
//
// The ring itself lives in core (`Session.diag`, 256 events); this is only the
// host-side drain. It was desktop-only until F15 (2026-09-09): `ui.ts` had it,
// `touch/app.ts` had nothing, even though both drive the same `ConfySession`
// and the same ring. One module rather than two copies, so the cursor rule
// below can't drift between hosts.

/** Anything with a `diagLog()` — avoids importing the Session class here. */
interface DiagSource {
  diagLog(): { seq: number; level: string; kind: string; detail: string }[];
}

let lastSeenSeq = -1;

/**
 * A new `Session` carries a new ring whose `seq` restarts at 0, so a cursor
 * from the old one is a high-water mark the replacement can never reach —
 * every post-swap event would be skipped until it caught up. Call this at the
 * one site that swaps the session.
 */
export function resetDiagCursor(): void {
  lastSeenSeq = -1;
}

/**
 * Print events recorded since the last call as
 * `[confy-diag] [LEVEL] KIND DETAIL`. A no-op without `?diag=1`, so it costs
 * nothing in normal use. Called every `render()`.
 */
export function drainDiagIfEnabled(session: DiagSource | null): void {
  if (typeof location === "undefined") return;
  if (new URLSearchParams(location.search).get("diag") !== "1") return;
  if (!session) return;
  for (const e of session.diagLog()) {
    if (e.seq <= lastSeenSeq) continue;
    console.debug(`[confy-diag] [${e.level}] ${e.kind} ${e.detail}`);
    lastSeenSeq = e.seq;
  }
}
