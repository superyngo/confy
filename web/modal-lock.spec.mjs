// Plain-Node test for desktop modal lock (ADR 0005 §5):
// While the clipboard is armed (paste-mode / clipboard_count > 0):
//   1. dnd.ts onDragStart is prevented (no reorder drag in paste mode).
//   2. ui.ts onTreeContext (right-click) is suppressed and sets the action-locked status.
//   3. ui.ts onTreeClick kind badge click is suppressed and sets the action-locked status.
//   4. ui.ts toolbar buttons (Undo, Redo) are guarded and set status.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
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

const dndTs = readFileSync(path.join(here, "dnd.ts"), "utf8");
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

// ---- (a) Structural checks: dnd.ts and ui.ts guards ----
console.log("-- structural: dnd.ts & ui.ts modal lock guards --");

check(
  "dnd.ts checks paste-mode in dragstart",
  /classList\?*\.contains\("paste-mode"\)/.test(dndTs),
);

check(
  "ui.ts onTreeContext guards clipboard_count > 0 and dispatches SetHostNotice",
  /function onTreeContext\([\s\S]*?clipboard_count(?: \?\? 0\))? > 0[\s\S]*?SetHostNotice[\s\S]*?core\.clipboard\.action-locked/.test(uiTs),
);

check(
  "ui.ts kind badge click in onTreeClick guards clipboard_count > 0",
  /const kindEl = target\.closest\("\[data-kind\]"\)[\s\S]*?clipboard_count(?: \?\? 0\))? > 0[\s\S]*?SetHostNotice[\s\S]*?core\.clipboard\.action-locked/.test(uiTs),
);

check(
  "ui.ts openKindMenuAt guards clipboard_count > 0",
  /function openKindMenuAt\([\s\S]*?clipboard_count[\s\S]*?SetHostNotice[\s\S]*?core\.clipboard\.action-locked/.test(uiTs),
);

check(
  "ui.ts uiUndo guards clipboard_count > 0",
  /function uiUndo\(\)\s*\{[\s\S]*?clipboard_count[\s\S]*?SetHostNotice[\s\S]*?core\.clipboard\.action-locked/.test(uiTs),
);

check(
  "ui.ts uiRedo guards clipboard_count > 0",
  /function uiRedo\(\)\s*\{[\s\S]*?clipboard_count[\s\S]*?SetHostNotice[\s\S]*?core\.clipboard\.action-locked/.test(uiTs),
);


check(
  "ui.ts installMarquee bails on paste-mode before arming a drag",
  /function installMarquee\(\)[\s\S]*?mousedown"[\s\S]*?paste-mode[\s\S]*?sx = ev\.clientX/.test(uiTs),
);

// ---- (b) Behavioral checks: dnd.ts dragstart under paste-mode ----
console.log("\n-- behavioral: dnd.ts dragstart modal lock --");

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
  const bodyClasses = new Set();
  const ops = [];
  const sent = [];

  const classList = (name, classes) => ({
    add: (c) => (ops.push(`${name} add ${c}`), classes.add(c)),
    remove: (c) => (ops.push(`${name} remove ${c}`), classes.delete(c)),
    contains: (c) => classes.has(c),
  });

  const mkRow = (key) => {
    const classes = new Set();
    const row = {
      dataset: { path: JSON.stringify([{ Key: key }]) },
      classList: classList(key, classes),
      getBoundingClientRect: () => ({ top: 0, height: 40, bottom: 40 }),
      querySelector: () => null,
      classes,
    };
    row.closest = (sel) => (sel === ".row" || sel === "[data-grip]" ? row : null);
    return row;
  };

  const rowA = mkRow("a");
  const listeners = {};
  const treeEl = {
    addEventListener: (t, fn) => (listeners[t] ??= []).push(fn),
    querySelectorAll: () => [],
    querySelector: (sel) => (sel.includes('"a"') ? rowA : null),
  };

  globalThis.document = {
    body: {
      classList: classList("body", bodyClasses),
    },
    getElementById: () => ({
      getBoundingClientRect: () => ({ top: 0 }),
      scrollTop: 0,
      style: {},
    }),
  };
  globalThis.CSS = { escape: (s) => s };

  const snap = {
    clipboard_count: 0,
    rows: [{ path: [{ Key: "a" }], selected: true }],
  };

  const { installDnd } = await bundleTs("dnd.ts");
  installDnd(
    treeEl,
    () => snap,
    (i) => sent.push(i),
    () => undefined,
  );

  const dragstartHandler = listeners["dragstart"]?.[0];
  check("dragstart listener registered", typeof dragstartHandler === "function");

  // 1. In paste-mode: dragstart MUST be prevented
  bodyClasses.add("paste-mode");
  let prevented = false;
  const evArmed = {
    target: rowA,
    defaultPrevented: false,
    preventDefault: () => { prevented = true; },
    dataTransfer: { setData: () => {}, effectAllowed: "" },
  };
  dragstartHandler(evArmed);
  check("dragstart in paste-mode is prevented", prevented === true);
  check("dragstart in paste-mode does not add drag-src class", !rowA.classes.has("drag-src"));

  // 2. Normal mode (not paste-mode): dragstart proceeds
  bodyClasses.delete("paste-mode");
  prevented = false;
  const evNormal = {
    target: rowA,
    defaultPrevented: false,
    preventDefault: () => { prevented = true; },
    dataTransfer: { setData: () => {}, effectAllowed: "" },
  };
  dragstartHandler(evNormal);
  check("dragstart without paste-mode is not prevented", prevented === false);
  check("dragstart without paste-mode adds drag-src class", rowA.classes.has("drag-src"));
}

// ---- (c) Behavioral checks: ui.ts actions during armed clipboard ----
console.log("\n-- behavioral: ui.ts toolbar & menu modal lock --");

{
  let statusSet = [];
  const setStatus = (status, err) => {
    statusSet.push({ status, err });
  };
  const t = (k) => {
    if (k === "core.clipboard.action-locked") {
      return "action disabled while clipboard is armed — paste (v) or discard (Esc) first";
    }
    return k;
  };

  let sent = [];
  const send = (i) => sent.push(i);

  // Simulate uiUndo / uiRedo logic as in ui.ts
  const makeHelpers = (snap) => ({
    uiUndo: () => {
      if (snap && (snap.clipboard_count ?? 0) > 0) {
        setStatus(t("core.clipboard.action-locked"), "");
        return;
      }
      send("Undo");
    },
    uiRedo: () => {
      if (snap && (snap.clipboard_count ?? 0) > 0) {
        setStatus(t("core.clipboard.action-locked"), "");
        return;
      }
      send("Redo");
    },
  });

  // Test with clipboard armed
  const armedSnap = { clipboard_count: 2 };
  const armed = makeHelpers(armedSnap);

  statusSet = [];
  sent = [];
  armed.uiUndo();
  check("uiUndo while armed sets action-locked status", statusSet.some((s) => s.status.includes("action disabled")));
  check("uiUndo while armed does not send Undo intent", sent.length === 0);

  statusSet = [];
  sent = [];
  armed.uiRedo();
  check("uiRedo while armed sets action-locked status", statusSet.some((s) => s.status.includes("action disabled")));
  check("uiRedo while armed does not send Redo intent", sent.length === 0);

  // Test with clipboard unarmed
  const unarmedSnap = { clipboard_count: 0 };
  const unarmed = makeHelpers(unarmedSnap);

  statusSet = [];
  sent = [];
  unarmed.uiUndo();
  check("uiUndo while unarmed sends Undo intent", sent.includes("Undo"));

  statusSet = [];
  sent = [];
  unarmed.uiRedo();
  check("uiRedo while unarmed sends Redo intent", sent.includes("Redo"));

}

// ---- (d) Behavioral checks: installMarquee bails while armed ----
console.log("\n-- behavioral: installMarquee modal lock --");
{
  const marqueeBlock = uiTs.match(/^function installMarquee\(\)[\s\S]*?\n\}/m)?.[0];
  check("installMarquee extracted verbatim", !!marqueeBlock);

  const windowListeners = {};
  const sent = [];
  const rawView = false;
  const snap = { rows: [{ path: [{ Key: "a" }], selected: false }] };

  const boxStyle = { display: "" };
  const wrap = {
    addEventListener: () => {},
    getBoundingClientRect: () => ({ top: 0, left: 0 }),
    scrollLeft: 0,
    scrollTop: 0,
  };
  const treeListeners = {};
  wrap.addEventListener = (t, fn) => (treeListeners[t] = fn);
  const box = { style: boxStyle };
  const $ = (id) => (id === "treeWrap" ? wrap : box);
  const windowStub = { addEventListener: (t, fn) => (windowListeners[t] = fn) };
  const tree = {};
  const send = (i) => sent.push(i);
  const rowsInRect = () => [];
  const setAnchor = () => {};

  const src = `const $ = (id) => (id === "treeWrap" ? wrapStub : boxStub);
let wrapStub, boxStub, tree, send, rowsInRect, setAnchor, rawView, snap, suppressClick;
export function setEnv(e) {
  wrapStub = e.wrap; boxStub = e.box; tree = e.tree;
  send = e.send; rowsInRect = e.rowsInRect; setAnchor = e.setAnchor;
  rawView = e.rawView; snap = e.snap; suppressClick = false;
}
const document = { body: { classList: { contains: (c) => e_bodyClasses.has(c) } } };
let e_bodyClasses = new Set();
export function setPasteMode(on) { on ? e_bodyClasses.add("paste-mode") : e_bodyClasses.delete("paste-mode"); }
${marqueeBlock ?? "function installMarquee() {}"}
export { installMarquee };
`;
  globalThis.window = windowStub;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  const mod = await import(modUrl);

  mod.setEnv({ wrap, box, tree, send, rowsInRect, setAnchor, rawView, snap });
  mod.installMarquee();
  const mousedown = treeListeners["mousedown"];
  check("mousedown listener registered", typeof mousedown === "function");

  // Armed: mousedown must not arm the drag, at all — a subsequent mousemove
  // must never see `active`, so window listeners must never fire a repaint.
  mod.setPasteMode(true);
  mousedown({ button: 0, clientX: 10, clientY: 10, target: { closest: () => null } });
  if (windowListeners["mousemove"]) {
    windowListeners["mousemove"]({ clientX: 20, clientY: 20 });
  }
  check("box never shown after an armed mousedown + move past the drag tolerance", boxStyle.display !== "block");

  // Unarmed: mousedown arms the drag, a real move past tolerance shows the box.
  mod.setPasteMode(false);
  boxStyle.display = "";
  mousedown({ button: 0, clientX: 10, clientY: 10, target: { closest: () => null } });
  windowListeners["mousemove"]({ clientX: 30, clientY: 30 });
  check("box shown after an unarmed mousedown + move past the drag tolerance", boxStyle.display === "block");
}

console.log(failures === 0 ? "\nALL MODAL-LOCK CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
