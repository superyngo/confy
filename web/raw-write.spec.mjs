// Plain-Node test for ui.ts's Raw pane write mode (T7 of
// docs/plan/2026-09-14-raw-write-mode.md, RS2b — R1/R2 routing, R4-R8, and the
// Switching table's scroll rule). Follows paste-hover.spec.mjs's convention:
// no test framework, just `node:assert`-free `check()` tallying; `ui.ts` can't
// be imported in Node (wasm + DOM top-level wiring), so the write-mode
// functions are extracted verbatim from source and type-stripped via esbuild
// — these checks run the real shipped function bodies, not a reimplementation.
// `rawState`/`rawWriteBaseline` are declared inside the generated bundle
// itself (the same trick every other extraction spec uses for module-level
// `let`s the functions close over) — only the truly external dependencies
// (`$`, `session`, `snap`, `send`, `t`, `askConfirm`, `setRawState`,
// `doSave`, `tree`, `renderTree`, `getEdit`) are stubbed via `setEnv`.
// `exitRawWrite` is async now (the confirmation is an in-page dialog, not the
// native `confirm()` a VS Code webview's sandbox silently answers "no") — so
// every exit below is awaited, and `flush()` drains the paths that fire it
// without returning its promise (`applyRawAndExit`, `cancelRawEdit`).
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
// Drain the microtasks an un-awaited `void exitRawWrite()` runs behind.
const flush = () => new Promise((r) => setTimeout(r, 0));

const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

const names = ["enterRawWrite", "maybeEnterRawWrite", "applyRawEdit", "rawEditSave", "exitRawWrite", "renderRawOrTree", "applyRawAndExit", "revertRawEdit", "cancelRawEdit", "saveTreeScroll", "offsetAtScrollTop", "restorePaneScroll"];
const fns = names.map((n) => uiTs.match(new RegExp(`^(?:async )?function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${names[i]} extracted verbatim`, !!s));

check(
  "renderRawOrTree re-seeds the pane only in the \"view\" branch (R8: write is the user's buffer)",
  !!fns[5] && /if \(rawState === "view"\) \{[\s\S]*editEl\.value = text;/.test(fns[5]),
);
check(
  "applyRawEdit reads doc_revision, not history_len, to detect commit (R5)",
  !!fns[2] && /doc_revision/.test(fns[2]) && !/history_len/.test(fns[2]),
);

const src = `let snap, session, rawState = "off", rawWriteBaseline = null, treeScrollTop = 0;
let $fn, tFn, sendFn, askConfirmFn, setRawStateFn, doSaveFn, renderTreeFn, getEditFn, treeEl;
function $(id) { return $fn(id); }
function t(k) { return tFn(k); }
function send(i) { return sendFn(i); }
function askConfirm(m, ok) { return askConfirmFn(m, ok); }
function setRawState(next) { return setRawStateFn(next); }
function doSave() { return doSaveFn(); }
function renderTree(...a) { return renderTreeFn(...a); }
function getEdit() { return getEditFn(); }
function applyRawChrome() {}
function renderRawControls() {}
// The pane's metrics (scrollRawToOffset uses the same two) and the
// after-layout re-assert, which runs inline here.
function getComputedStyle() { return { lineHeight: "20px", paddingTop: "8px" }; }
function requestAnimationFrame(fn) { fn(); }
const tree = { get classList() { return treeEl.classList; } };
export function setEnv(e) {
  if ("snap" in e) snap = e.snap;
  if ("session" in e) session = e.session;
  if ("$" in e) $fn = e.$;
  if ("t" in e) tFn = e.t;
  if ("send" in e) sendFn = e.send;
  if ("askConfirm" in e) askConfirmFn = e.askConfirm;
  if ("setRawState" in e) setRawStateFn = e.setRawState;
  if ("doSave" in e) doSaveFn = e.doSave;
  if ("renderTree" in e) renderTreeFn = e.renderTree;
  if ("getEdit" in e) getEditFn = e.getEdit;
  if ("tree" in e) treeEl = e.tree;
}
export function getRawState() { return rawState; }
export function reset() { rawState = "off"; rawWriteBaseline = null; }
export function getBaseline() { return rawWriteBaseline; }
export function setTreeScroll(v) { treeScrollTop = v; }
export function getTreeScroll() { return treeScrollTop; }
export ${fns[0]}
export ${fns[1]}
export ${fns[2]}
export ${fns[3]}
export ${fns[4]}
export ${fns[5]}
export ${fns[6]}
export ${fns[7]}
export ${fns[8]}
export ${fns[9]}
export ${fns[10]}
export ${fns[11]}
`;

const built = await esbuild.build({
  stdin: { contents: src, resolveDir: here, loader: "ts" },
  write: false,
  format: "esm",
  target: "es2022",
});
const mod = await import("data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64"));

// ---- fakes: a tiny classList + two textish elements (#raw pre, #rawEdit textarea) ----
function mkClassList() {
  const s = new Set();
  return {
    add: (c) => s.add(c),
    remove: (c) => s.delete(c),
    contains: (c) => s.has(c),
    // `renderRawOrTree` hides/shows both panes through `toggle(cls, on)`.
    toggle: (c, on) => (on ? s.add(c) : s.delete(c)),
    set: s,
  };
}
function mkEl() {
  return { value: "", textContent: "", scrollTop: 0, scrollLeft: 0, selectionStart: 0, selectionEnd: 0, readOnly: true, classList: mkClassList(), focus() {}, setSelectionRange(s, e) { this.selectionStart = s; this.selectionEnd = e; } };
}
let els;
function freshEls() {
  // The Raw pane is one element in both states (2026-09-14) — there is no
  // `#raw` `<pre>` to swap with any more.
  els = { rawEdit: mkEl(), btnViewToggle: mkEl(), treeWrap: mkEl() };
  return els;
}

let sentIntents;
let doSaveCalls;
let setRawStateCalls;
let confirmAnswer;
function freshEnv(session) {
  mod.reset();
  freshEls();
  sentIntents = [];
  doSaveCalls = 0;
  setRawStateCalls = [];
  confirmAnswer = true;
  mod.setEnv({
    $: (id) => els[id],
    session,
    t: (k) => k,
    send: (i) => sentIntents.push(i),
    askConfirm: (_m) => Promise.resolve(confirmAnswer),
    setRawState: (next) => setRawStateCalls.push(next),
    doSave: () => { doSaveCalls++; return Promise.resolve(); },
    renderTree: () => {},
    getEdit: () => null,
    tree: { classList: mkClassList() },
  });
}

// ---- 1. R1/R2 routing: an empty-path pending edit enters write; a per-node
//         (non-empty path) one leaves rawState alone ----
console.log("\n-- maybeEnterRawWrite(): R1 empty-path routing --");
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 0, external_edit: { initial: "a = 1\n", kind: { Value: { path: [] } } } } });
  mod.maybeEnterRawWrite();
  check("empty-path pending edit enters write mode", mod.getRawState() === "write");
  check("the textarea is seeded with the pending edit's initial text", els.rawEdit.value === "a = 1\n");
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 0, external_edit: { initial: "1", kind: { Value: { path: [{ Key: "a" }] } } } } });
  mod.maybeEnterRawWrite();
  check("a per-node pending edit does NOT enter write mode", mod.getRawState() === "off");
}
{
  // Re-render while write mode is already open (unrelated cause, e.g. a
  // theme toggle) must not re-seed and clobber the user's typing (R8).
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 0, external_edit: { initial: "a = 1\n", kind: { Value: { path: [] } } } } });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 1\nb = 2\n"; // simulated typing
  mod.maybeEnterRawWrite(); // pending edit is still present (nothing applied yet)
  check("a repeat render while write is already open never re-seeds (R8)", els.rawEdit.value === "a = 1\nb = 2\n");
}

// ---- 2. Single-element pane: the reading position survives both swaps ----
console.log("\n-- enterRawWrite() / exitRawWrite(): one element keeps its own scroll --");
{
  freshEnv({ serialize: () => "a = 1\n" });
  els.rawEdit.scrollTop = 42;
  els.rawEdit.scrollLeft = 7;
  mod.enterRawWrite("a = 1\n");
  check("entering write leaves the pane's scroll where it was", els.rawEdit.scrollTop === 42 && els.rawEdit.scrollLeft === 7);
  check("entering write seats the caret on the first visible line, not offset 0", els.rawEdit.selectionStart === 6 && els.rawEdit.selectionEnd === 6);

  els.rawEdit.scrollTop = 99;
  els.rawEdit.value = mod.getBaseline(); // clean buffer — no confirm needed
  await mod.exitRawWrite();
  check("exiting write leaves the pane's scroll untouched (no hand-off)", els.rawEdit.scrollTop === 99);
  check("exitRawWrite hands off to setRawState(\"view\")", setRawStateCalls.length === 1 && setRawStateCalls[0] === "view");
  check("exitRawWrite peels core's pending edit via Escape", sentIntents.length === 1 && sentIntents[0] === "Escape");
}
{
  // A successful Apply re-seeds `.value`, which natively resets a textarea's
  // scroll — the pane is now the scroll container, so it must be restored.
  freshEnv({ serialize: () => "a = 2\n" });
  mod.setEnv({
    snap: { doc_revision: 5 },
    send: (i) => { sentIntents.push(i); mod.setEnv({ snap: { doc_revision: 6 } }); },
  });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 2\n";
  els.rawEdit.scrollTop = 120;
  mod.applyRawEdit();
  check("a committed Apply keeps the pane's scroll position", els.rawEdit.scrollTop === 120);
}
{
  // 2026-09-16, reported on Firefox: a focused caret at offset 0 makes the
  // browser scroll a scrolled pane back to the head. Both paths that move the
  // caret must therefore put it inside the viewport — here, exactly the first
  // visible line (metrics stubbed at 20px line-height, 8px padding-top).
  const TEXT = "l0\nl1\nl2\nl3\nl4\n"; // line N starts at offset 3N
  freshEnv({ serialize: () => TEXT });
  els.rawEdit.scrollTop = 8 + 3 * 20; // line 3 is at the top of the viewport
  mod.enterRawWrite(TEXT);
  check("the caret lands on the first visible line (offset 9 = line 3)", els.rawEdit.selectionStart === 9);
  check("entering write from a scrolled pane keeps the scroll", els.rawEdit.scrollTop === 68);

  // Dirty Cancel: the reported failure. A clean one returns early (checked
  // separately below) — which is exactly why only the dirty one jumped.
  els.rawEdit.value = "# typed\n" + TEXT;
  els.rawEdit.scrollTop = 8 + 2 * 20;
  mod.revertRawEdit();
  check("a dirty Cancel reverts the buffer", els.rawEdit.value === TEXT);
  check("a dirty Cancel keeps the pane's scroll", els.rawEdit.scrollTop === 48);
  check("a dirty Cancel leaves the caret on the first visible line, not 0", els.rawEdit.selectionStart === 6);
}

// ---- 2b. The *tree's* reading position survives the Raw round trip
//          (2026-09-16). Measured defect: `#treeWrap` stays displayed while
//          only `#tree` is hidden, so the browser clamps its scrollTop to 0
//          the moment Raw opens — unlike `#rawEdit`, which is hidden itself
//          and keeps its own. ----
console.log("\n-- tree scroll across the Raw round trip --");
{
  freshEnv({ serialize: () => "a = 1\n" });
  const treeCls = mkClassList();
  mod.setEnv({ snap: { doc_revision: 0 }, tree: { classList: treeCls } });
  els.treeWrap.scrollTop = 200;
  mod.saveTreeScroll();
  check("leaving the tree records its position", mod.getTreeScroll() === 200);
  // Raw is up: the browser has clamped the live scroller.
  els.treeWrap.scrollTop = 0;
  treeCls.add("hidden");
  mod.renderRawOrTree(); // rawState is "off" again -> returning
  check("returning to the tree restores its position", els.treeWrap.scrollTop === 200);
  check("the tree is unhidden on return", !treeCls.contains("hidden"));
}
{
  // A plain tree re-render (the tree was never hidden) must NOT write the
  // stale saved value over the user's live scrolling.
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 0 }, tree: { classList: mkClassList() } });
  mod.setTreeScroll(200);
  els.treeWrap.scrollTop = 40;
  mod.renderRawOrTree();
  check("a tree-to-tree render leaves the live scroll alone", els.treeWrap.scrollTop === 40);
}
{
  // "Edit whole file" opens write mode straight from the tree.
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 0 } });
  els.treeWrap.scrollTop = 310;
  mod.enterRawWrite("a = 1\n");
  check("entering write from the tree records the tree's position", mod.getTreeScroll() === 310);
}

// ---- 3. R4/R5: Apply outcome is read off doc_revision, never the notice ----
console.log("\n-- applyRawEdit(): R4/R5 commit detection + buffer handling --");
{
  freshEnv({ serialize: () => "a = 2\n" });
  mod.setEnv({ snap: { doc_revision: 5 } });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "not valid toml [[[";
  const before = sentIntents.length;
  const committed = mod.applyRawEdit();
  check("a failed Apply (doc_revision unchanged) reports not committed", committed === false);
  check("a failed Apply leaves the buffer text exactly as typed", els.rawEdit.value === "not valid toml [[[");
  check("a failed Apply still dispatches ApplyReplace with the empty path", sentIntents.length === before + 1);
  check(
    "the dispatched intent carries the empty path and the live buffer text",
    JSON.stringify(sentIntents[0]) === JSON.stringify({ ApplyReplace: { path: [], text: "not valid toml [[[" } }),
  );
}
{
  freshEnv({ serialize: () => "a = 2\n" });
  mod.setEnv({
    snap: { doc_revision: 5 },
    send: (i) => { sentIntents.push(i); mod.setEnv({ snap: { doc_revision: 6 } }); },
  });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 2\n";
  const committed = mod.applyRawEdit();
  check("a successful Apply (doc_revision moved) reports committed", committed === true);
  check("a successful Apply re-seeds the textarea from session.serialize()", els.rawEdit.value === "a = 2\n");
  check("a successful Apply moves the dirty baseline forward", mod.getBaseline() === "a = 2\n");
}

// ---- 4. R6: ⌘S is apply-if-dirty-then-save; a failed Apply never saves ----
console.log("\n-- rawEditSave(): R6 apply-if-dirty-then-save --");
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 1 } }); // dispatch below never moves this
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "still broken [[[";
  await mod.rawEditSave();
  check("⌘S does not save when the dirty buffer's Apply failed", doSaveCalls === 0);
}
{
  freshEnv({ serialize: () => "a = 2\n" });
  mod.setEnv({
    snap: { doc_revision: 1 },
    send: (i) => { sentIntents.push(i); mod.setEnv({ snap: { doc_revision: 2 } }); },
  });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 2\n";
  await mod.rawEditSave();
  check("⌘S saves once the dirty buffer's Apply committed", doSaveCalls === 1);
  check("⌘S's Apply used the empty path", sentIntents.some((i) => i?.ApplyReplace?.path?.length === 0));
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 1 } });
  mod.enterRawWrite("a = 1\n");
  // Buffer left exactly as seeded — clean, no Apply should be attempted.
  await mod.rawEditSave();
  check("⌘S on a clean buffer saves directly, with no Apply dispatched", doSaveCalls === 1 && sentIntents.length === 0);
}

// ---- 5. R7: Escape confirm-gates on dirtiness ----
console.log("\n-- exitRawWrite(): R7 confirm gated on dirtiness --");
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 1\nb = 2\n"; // dirty
  confirmAnswer = false;
  await mod.exitRawWrite();
  check("declining the confirm on a dirty buffer stays in write mode", mod.getRawState() === "write");
  check("declining the confirm sends no Escape", sentIntents.length === 0);
  check("declining the confirm does not call setRawState", setRawStateCalls.length === 0);

  confirmAnswer = true;
  await mod.exitRawWrite();
  check("accepting the confirm on a dirty buffer exits write mode", setRawStateCalls[0] === "view");
  check("accepting the confirm still peels the pending edit via Escape", sentIntents[0] === "Escape");
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.enterRawWrite("a = 1\n");
  // Buffer untouched — clean — no confirm should even be consulted.
  let confirmCalls = 0;
  mod.setEnv({ askConfirm: () => { confirmCalls++; return Promise.resolve(false); } });
  await mod.exitRawWrite();
  check("a clean buffer exits without ever asking for confirmation", confirmCalls === 0);
  check("a clean buffer's exit still reaches setRawState(\"view\")", setRawStateCalls[0] === "view");
}
// The header's Tree/Raw button passes its own landing state: one press from
// write mode reaches Tree, no detour through Raw view (2026-09-14).
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.enterRawWrite("a = 1\n");
  await mod.exitRawWrite("off");
  check("exitRawWrite(\"off\") lands on Tree in one press", setRawStateCalls.length === 1 && setRawStateCalls[0] === "off");
  check("the one-press exit still peels the pending edit via Escape", sentIntents.length === 1 && sentIntents[0] === "Escape");
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 1\nb = 2\n"; // dirty
  confirmAnswer = false;
  await mod.exitRawWrite("off");
  check("the one-press exit takes the same R7 confirm gate", mod.getRawState() === "write" && setRawStateCalls.length === 0);
}
// ---- 6. The band's Apply/Cancel both LEAVE write mode (2026-09-15) ----
console.log("\n-- applyRawAndExit() / cancelRawEdit(): both controls exit --");
{
  freshEnv({ serialize: () => "a = 2\n" });
  mod.setEnv({
    snap: { doc_revision: 1 },
    send: (i) => { sentIntents.push(i); mod.setEnv({ snap: { doc_revision: 2 } }); },
  });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 2\n";
  mod.applyRawAndExit();
  await flush();
  check("a committed Apply leaves write mode", setRawStateCalls.length === 1 && setRawStateCalls[0] === "view");
  check("the committed Apply dispatched the empty-path Replace", sentIntents.some((i) => i?.ApplyReplace?.path?.length === 0));
  check("the exit peels the pending edit via Escape", sentIntents.includes("Escape"));
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 1 } }); // never moves ⇒ Apply failed
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "still broken [[[";
  mod.applyRawAndExit();
  await flush();
  check("a failed Apply stays in write mode (R4)", setRawStateCalls.length === 0);
  check("a failed Apply keeps the buffer verbatim", els.rawEdit.value === "still broken [[[");
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.setEnv({ snap: { doc_revision: 1 } });
  mod.enterRawWrite("a = 1\n");
  mod.applyRawAndExit();
  await flush(); // clean buffer
  check("a clean buffer's Apply exits with no ApplyReplace dispatched", setRawStateCalls[0] === "view" && !sentIntents.some((i) => i?.ApplyReplace));
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.enterRawWrite("a = 1\n");
  els.rawEdit.value = "a = 1\nb = 2\n"; // dirty
  let confirmCalls = 0;
  mod.setEnv({ askConfirm: () => { confirmCalls++; return Promise.resolve(false); } });
  mod.cancelRawEdit();
  await flush();
  check("Cancel restores the last applied text", els.rawEdit.value === "a = 1\n");
  check("Cancel leaves write mode", setRawStateCalls.length === 1 && setRawStateCalls[0] === "view");
  check("Cancel never asks for confirmation — the press IS the answer", confirmCalls === 0);
}
{
  freshEnv({ serialize: () => "a = 1\n" });
  mod.cancelRawEdit();
  await flush(); // rawState is "off"
  check("Cancel is inert outside write mode", setRawStateCalls.length === 0 && sentIntents.length === 0);
}


console.log(failures === 0 ? "\nALL RAW WRITE-MODE CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
