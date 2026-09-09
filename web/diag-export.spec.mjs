// Tests for Task 17: Diag exports (FFI diag_log + web ?diag=1 console drain)
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
let totalChecks = 0;
function check(name, cond, extra = "") {
  totalChecks++;
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

console.log("\n-- Task 17 Structural Invariants --");

// 1. web/types.ts defines DiagLevel and DiagEvent
const typesSrc = readFileSync(path.join(here, "types.ts"), "utf8");
check("types.ts defines DiagLevel", typesSrc.includes("export type DiagLevel =") || typesSrc.includes("export type DiagLevel="));
check("types.ts DiagLevel has PascalCase variants (Debug, Info, Warn, Error)",
  typesSrc.includes('"Debug"') && typesSrc.includes('"Info"') && typesSrc.includes('"Warn"') && typesSrc.includes('"Error"'));
check("types.ts defines DiagEvent", typesSrc.includes("export interface DiagEvent") || typesSrc.includes("export type DiagEvent"));
check("types.ts DiagEvent has seq, level, kind, detail fields",
  typesSrc.includes("seq:") && typesSrc.includes("level:") && typesSrc.includes("kind:") && typesSrc.includes("detail:"));

// 2. web/confy.ts has diagLog() method on Session
const confySrc = readFileSync(path.join(here, "confy.ts"), "utf8");
check("confy.ts imports DiagEvent from ./types.js", confySrc.includes("DiagEvent"));
check("confy.ts Session class has diagLog method", confySrc.includes("diagLog(") && confySrc.includes("diag_log()"));

// 3. web/diag.ts holds the ?diag=1 drain, and BOTH orchestrators use it.
// It lived in ui.ts until F15 (2026-09-09), which made the trace desktop-only.
const diagSrc = readFileSync(path.join(here, "diag.ts"), "utf8");
check("diag.ts checks ?diag=1 in URLSearchParams", diagSrc.includes('"diag"') && diagSrc.includes('"1"'));
check("diag.ts calls diagLog()", diagSrc.includes("diagLog()"));
check("diag.ts logs with [confy-diag] prefix and console.debug", diagSrc.includes("[confy-diag]") && diagSrc.includes("console.debug"));
check("diag.ts exports a cursor reset for the session swap", diagSrc.includes("export function resetDiagCursor"));
const uiSrc = readFileSync(path.join(here, "ui.ts"), "utf8");
const touchSrc = readFileSync(path.join(here, "touch", "app.ts"), "utf8");
for (const [host, src] of [["ui.ts", uiSrc], ["touch/app.ts", touchSrc]]) {
  check(`${host} render() calls the shared diag drain`, src.includes("drainDiagIfEnabled(session)"));
  check(`${host} resets the diag cursor when the session is swapped`, src.includes("resetDiagCursor()"));
}

console.log("\n-- Task 17 Behavioral: ?diag=1 console drain logic --");

// Simulate the drain logic in isolation to test behavior
function createDiagDrainer() {
  let lastSeenSeq = -1;
  const debugLogs = [];
  const fakeConsole = {
    debug: (...args) => debugLogs.push(args.join(" ")),
  };

  function drain(searchString, mockSession) {
    if (typeof searchString !== "string") return;
    const params = new URLSearchParams(searchString);
    if (params.get("diag") !== "1") return;
    if (!mockSession) return;
    const events = mockSession.diagLog();
    for (const e of events) {
      if (e.seq <= lastSeenSeq) continue;
      fakeConsole.debug(`[confy-diag] [${e.level}] ${e.kind} ${e.detail}`);
      lastSeenSeq = e.seq;
    }
  }

  return { drain, debugLogs, getLastSeenSeq: () => lastSeenSeq };
}

// Case A: ?diag=1 active
{
  const drainer = createDiagDrainer();
  let mockEvents = [
    { seq: 0, level: "Info", kind: "notice", detail: "startup" },
    { seq: 1, level: "Debug", kind: "dispatch", detail: "intent=CursorDown" },
  ];
  const mockSession = { diagLog: () => mockEvents };

  // First drain
  drainer.drain("?diag=1", mockSession);
  check("first drain logs all initial events", drainer.debugLogs.length === 2);
  check("first log message matches format", drainer.debugLogs[0] === "[confy-diag] [Info] notice startup", drainer.debugLogs[0]);
  check("second log message matches format", drainer.debugLogs[1] === "[confy-diag] [Debug] dispatch intent=CursorDown", drainer.debugLogs[1]);

  // Second drain with no new events
  drainer.drain("?diag=1", mockSession);
  check("second drain with no new events logs nothing extra", drainer.debugLogs.length === 2);

  // Third drain with 1 new event
  mockEvents = [
    ...mockEvents,
    { seq: 2, level: "Warn", kind: "schema", detail: "missing field" },
  ];
  drainer.drain("?diag=1", mockSession);
  check("third drain logs only new event", drainer.debugLogs.length === 3);
  check("third log is the new warn event", drainer.debugLogs[2] === "[confy-diag] [Warn] schema missing field", drainer.debugLogs[2]);
}

// Case B: ?diag not set or not "1"
{
  const drainer = createDiagDrainer();
  const mockEvents = [
    { seq: 0, level: "Info", kind: "notice", detail: "startup" },
  ];
  const mockSession = { diagLog: () => mockEvents };

  drainer.drain("", mockSession);
  check("drain without ?diag=1 does not log", drainer.debugLogs.length === 0);

  drainer.drain("?diag=0", mockSession);
  check("drain with ?diag=0 does not log", drainer.debugLogs.length === 0);

  drainer.drain("?diag=true", mockSession);
  check("drain with ?diag=true does not log", drainer.debugLogs.length === 0);
}

if (failures > 0) {
  console.error(`\n${failures} check(s) failed out of ${totalChecks}`);
  process.exit(1);
} else {
  console.log(`\nALL ${totalChecks} Task 17 checks passed`);
}
