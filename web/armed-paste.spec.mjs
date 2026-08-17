// Plain-Node test for ui.ts's clipboard-armed click routing (ADR 0004 §1):
// while the clipboard is armed, a click must position the paste target via
// `Session.pointerSlot(path, relY)` -> `Intent.SetPasteSlot` (`Into`/`After`
// from the click's row-relative Y) instead of always falling back to a bare
// `SetCursor`. Follows `toolbar-fold.spec.mjs`'s convention: no test framework,
// just `node:assert` + a `check()` tally. `ui.ts` can't be imported in Node
// (wasm + DOM top-level wiring), so the `armedPasteTarget` helper is extracted
// verbatim from ui.ts's source and type-stripped via esbuild — the behavioral
// checks below run the real shipped function body, not a reimplementation —
// and the two call sites (`focusRow`, `onTreeClick`'s plain row-body branch)
// are verified structurally against the source, same as TOOLBAR_ENTRIES.
import path from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

// ---- extract + execute the real `armedPasteTarget` from ui.ts ----
const fnMatch = uiTs.match(/^function armedPasteTarget\([\s\S]*?\n\}/m);
check("ui.ts defines armedPasteTarget()", !!fnMatch);
check(
  "armedPasteTarget computes relY from the row's bounding rect",
  !!fnMatch && /getBoundingClientRect\(\)/.test(fnMatch[0]) && /ev\.clientY - r\.top/.test(fnMatch[0]),
);
check(
  "armedPasteTarget routes through session.pointerSlot and SetPasteSlot",
  !!fnMatch && /session\.pointerSlot\(path, relY\)/.test(fnMatch[0]) && /\{ SetPasteSlot: slot \}/.test(fnMatch[0]),
);

let armedPasteTarget = null;
let setSession = null;
if (fnMatch) {
  // `session` is a module-level global in ui.ts; expose a setter so each case
  // can arm/disarm the stub. The fn text is verbatim from the source above.
  const src = `let session;
export function setSession(s) { session = s; }
export ${fnMatch[0]}\n`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  ({ armedPasteTarget, setSession } = await import(modUrl));
}

// Minimal fakes: only the surface armedPasteTarget touches — `closest(".row")`
// on the event target, `getBoundingClientRect()` on the row, `pointerSlot` on
// the session. No jsdom (no new npm dependency).
const rowAt = (top, height) => ({ getBoundingClientRect: () => ({ top, height }) });
const evOn = (row, clientY) => ({
  target: { closest: (sel) => (sel === ".row" ? row : null) },
  clientY,
});
const P = [{ Key: "a" }, { Index: 1 }];
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function sessionStub(slot, captured) {
  return { pointerSlot: (path, relY) => { captured.push({ path, relY }); return slot; } };
}
if (!armedPasteTarget) {
  // Pre-implementation (RED): keep the tally flowing so every behavioral
  // check below reports ✗ instead of crashing on undefined imports.
  armedPasteTarget = () => undefined;
  setSession = () => {};
}

console.log("\n-- armed click -> pointerSlot classification --");
{
  const captured = [];
  setSession(sessionStub({ After: P }, captured));
  const got = armedPasteTarget?.(P, evOn(rowAt(100, 40), 120)); // 50% down the row
  check("50% click returns SetPasteSlot", !!got && "SetPasteSlot" in got, JSON.stringify(got));
  check("slot passes through verbatim", eq(got, { SetPasteSlot: { After: P } }), JSON.stringify(got));
  check("relY is (clientY - rowTop) / rowHeight", captured.at(-1)?.relY === 0.5, JSON.stringify(captured.at(-1)));
  check("pointerSlot receives the clicked path", eq(captured.at(-1)?.path, P));
}
{
  const captured = [];
  setSession(sessionStub({ Into: P }, captured));
  const got = armedPasteTarget?.(P, evOn(rowAt(100, 40), 110)); // top quarter -> Into
  check("top-quarter click also classified (Into)", eq(got, { SetPasteSlot: { Into: P } }), JSON.stringify(got));
  check("relY 0.25 at a quarter down", captured.at(-1)?.relY === 0.25, JSON.stringify(captured.at(-1)));
}
{
  const captured = [];
  setSession(sessionStub({ Into: P }, captured));
  armedPasteTarget?.(P, evOn(rowAt(100, 40), 140)); // bottom edge
  check("relY 1 at the row bottom", captured.at(-1)?.relY === 1, JSON.stringify(captured.at(-1)));
}

console.log("\n-- fallbacks still land on the bare cursor --");
{
  const captured = [];
  setSession(sessionStub(undefined, captured)); // pointerSlot declines to classify
  const got = armedPasteTarget?.(P, evOn(rowAt(100, 40), 120));
  check("unclassifiable click falls back to SetCursor", eq(got, { SetCursor: P }), JSON.stringify(got));
}
{
  const captured = [];
  setSession(null); // no session yet (boot race)
  const got = armedPasteTarget?.(P, evOn(rowAt(100, 40), 120));
  check("null session falls back to SetCursor", eq(got, { SetCursor: P }), JSON.stringify(got));
}
{
  const captured = [];
  setSession(sessionStub({ Into: P }, captured));
  const got = armedPasteTarget?.(P, { target: { closest: () => null }, clientY: 120 }); // no .row ancestor
  check("click outside any .row falls back to SetCursor", eq(got, { SetCursor: P }), JSON.stringify(got));
  check("pointerSlot never consulted without a row", captured.length === 0);
}
{
  const captured = [];
  setSession(sessionStub({ Into: P }, captured));
  armedPasteTarget?.(P, evOn(rowAt(100, 0), 130)); // collapsed/zero-height row: `r.height || 1` guards div-by-zero
  check("zero-height row divides by 1, not 0", captured.at(-1)?.relY === 30, JSON.stringify(captured.at(-1)));
}

// ---- call-site wiring: both armed branches route through the helper ----
console.log("\n-- ui.ts armed-branch wiring --");
const focusRowBlock = uiTs.match(/^function focusRow\([\s\S]*?\n\}/m)?.[0] ?? "";
check("focusRow's armed branch calls armedPasteTarget", /clipboard_count \?\? 0\) > 0\) return send\(armedPasteTarget\(path, ev\)\)/.test(focusRowBlock));
check("focusRow armed branch no longer sends a bare SetCursor", !/clipboard_count[^;\n]*\)\s*> 0[^\n]*SetCursor/.test(focusRowBlock));

const onTreeClickBlock = uiTs.match(/^function onTreeClick\([\s\S]*?\n\}/m)?.[0] ?? "";
const armedBranch = onTreeClickBlock.match(/if \(\(snap\.clipboard_count \?\? 0\) > 0\) \{[\s\S]*?\n  \}/)?.[0] ?? "";
check("onTreeClick has a clipboard-armed branch", armedBranch.length > 0);
check(
  "onTreeClick's armed branch returns send(armedPasteTarget(path, ev))",
  /return send\(armedPasteTarget\(path, ev\)\)/.test(armedBranch),
);
check(
  "onTreeClick's armed branch no longer sends a bare SetCursor",
  !/SetCursor/.test(armedBranch),
);

console.log(failures === 0 ? "\nALL ARMED-PASTE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
