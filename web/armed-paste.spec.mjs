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

// ---- dnd endDrag → onDragEnd: the armed-paste cue restore hook ----
// dnd's `clearOver()` unconditionally wipes `.drag-over-into` + `#dropLine`,
// which double as the armed-paste cue (ADR 0004 §1), whenever ANY drag
// gesture ends — even one unrelated to the armed clipboard. `endDrag()` must
// give the owner a hook to redraw the cue, AFTER the wipe. Runs the real
// shipped `installDnd` against a minimal DOM shim (no jsdom, no new dep).
console.log("\n-- dnd endDrag fires the cue-restore hook after clearOver --");
const dndTs = readFileSync(path.join(here, "dnd.ts"), "utf8");
async function bundleTs(entry) {
  const built = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  return import(modUrl);
}
{
  const ops = [];
  const classList = () => ({
    add: (c) => ops.push(`add ${c}`),
    remove: (c) => ops.push(`remove ${c}`),
  });
  const armedRow = { classList: classList() }; // the paste-Into row clearOver strips
  const srcRow = { classList: classList() };
  const dropLine = {
    style: new Proxy({}, { set: (t, k, v) => (ops.push(`style.${k}=${v}`), (t[k] = v), true) }),
  };
  const listeners = {};
  const treeEl = {
    addEventListener: (t, fn) => (listeners[t] ??= []).push(fn),
    querySelectorAll: (sel) => (sel === ".drag-over-into" ? [armedRow] : sel === ".drag-src" ? [srcRow] : []),
    querySelector: () => null,
  };
  globalThis.document = {
    getElementById: (id) =>
      id === "dropLine" ? dropLine : { getBoundingClientRect: () => ({ top: 0 }), scrollTop: 0 },
  };
  globalThis.CSS = { escape: (s) => s };
  const { installDnd: install } = await bundleTs("dnd.ts");
  install(treeEl, () => null, () => {}, () => undefined, () => ops.push("onDragEnd"));
  listeners.dragend[0]({}); // Esc / dropped outside the tree
  check("dragend fires the onDragEnd hook", ops.includes("onDragEnd"));
  check(
    "hook runs AFTER clearOver's wipe (wipe → restore order)",
    ops.indexOf("onDragEnd") > ops.indexOf("remove drag-over-into") &&
      ops.indexOf("onDragEnd") > ops.indexOf("style.display=none"),
  );
  listeners.drop[0]({}); // drop with no resolvable target (early-return endDrag)
  check("no-target drop also fires the hook", ops.filter((o) => o === "onDragEnd").length === 2);
}
check(
  "installDnd keeps onDragEnd optional and LAST (after send)",
  /send: \(i: Intent\) => void,[\s\S]*?onDragEnd\?: \(\) => void,\s*\n\): void \{/.test(dndTs),
);

// ---- renderPasteSlotCue alone restores BOTH halves of the cue ----
// Extracted verbatim from ui.ts (same technique as armedPasteTarget above —
// ui.ts can't run under Node: wasm + top-level DOM wiring). After a wipe,
// one call must redraw the Into row class AND the After dropLine.
console.log("\n-- renderPasteSlotCue redraws the Into class + After line --");
{
  const cueMatch = uiTs.match(/^function renderPasteSlotCue\([\s\S]*?\n\}/m);
  check("ui.ts defines renderPasteSlotCue()", !!cueMatch);
  let cue = () => {};
  let setEnv = () => {};
  if (cueMatch) {
    const built = await esbuild.build({
      stdin: {
        contents: `let $, tree, rawView, CSS;
export function setEnv(e) { $ = e.$; tree = e.tree; rawView = e.rawView; CSS = e.CSS; }
export ${cueMatch[0]}\n`,
        resolveDir: here,
        loader: "ts",
      },
      write: false,
      format: "esm",
      target: "es2022",
    });
    const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
    ({ renderPasteSlotCue: cue, setEnv } = await import(modUrl));
  }
  const cueOps = [];
  const cueDropLine = { style: {} };
  const intoRow = { classList: { add: (c) => cueOps.push(`add ${c}`) } };
  const afterRow = {
    getBoundingClientRect: () => ({ bottom: 40 }),
    querySelector: () => ({ offsetWidth: 12 }),
  };
  const cueTree = {
    querySelector: (sel) => (sel.includes('"Key":"b"') ? intoRow : sel.includes('"Key":"c"') ? afterRow : null),
    querySelectorAll: () => [],
  };
  setEnv({
    $: (id) => (id === "dropLine" ? cueDropLine : { getBoundingClientRect: () => ({ top: 5 }), scrollTop: 3 }),
    tree: cueTree,
    rawView: false,
    CSS: { escape: (s) => s },
  });
  cue({ paste_slot: { Into: [{ Key: "b" }] } });
  check("Into slot re-adds the row's drag-over-into class", cueOps.includes("add drag-over-into"));
  check("Into slot keeps the dropLine hidden", cueDropLine.style.display === "none");
  cue({ paste_slot: { After: [{ Key: "c" }] } });
  check("After slot shows the dropLine", cueDropLine.style.display === "block");
  check("After line at row bottom - wrap top + scrollTop", cueDropLine.style.top === "38px", JSON.stringify(cueDropLine.style));
  check("After line at row indent + 8px", cueDropLine.style.left === "20px", JSON.stringify(cueDropLine.style));
  cue({ paste_slot: undefined });
  check("unarmed snapshot hides the line", cueDropLine.style.display === "none");
}

// ---- ui.ts wiring: installDnd's 4th argument re-invokes the cue ----
check(
  "installDnd call site passes the cue-restore callback",
  /installDnd\(tree, \(\) => snap, send, \(p, r\) => session!\.pointerSlot\(p, r\), \(\) => \{\s*\n\s*if \(snap\) renderPasteSlotCue\(snap\);\s*\n\s*\}\);/.test(uiTs),
);

// ---- call-site wiring: both armed branches route through the helper ----
console.log("\n-- ui.ts armed-branch wiring --");
const focusRowBlock = uiTs.match(/^function focusRow\([\s\S]*?\n\}/m)?.[0] ?? "";
check("focusRow's armed branch calls armedPasteTarget", /clipboard_count \?\? 0\) > 0\) return send\(armedPasteTarget\(path, ev\)\)/.test(focusRowBlock));
check("focusRow armed branch no longer sends a bare SetCursor", !/clipboard_count[^;\n]*\)\s*> 0[^\n]*SetCursor/.test(focusRowBlock));

const onTreeClickBlock = uiTs.match(/^function onTreeClick\([\s\S]*?\n\}/m)?.[0] ?? "";
const armedBranch = onTreeClickBlock.match(/if \(\(snap\.clipboard_count \?\? 0\) > 0\) \{\s*\n\s*return send\(armedPasteTarget\(path, ev\)\);\s*\n\s*\}/)?.[0] ?? "";
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
