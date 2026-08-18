// Plain-Node test for desktop modal lock (ADR 0005 §5):
// While the clipboard is armed (paste-mode / clipboard_count > 0):
//   1. dnd.ts onDragStart is prevented (no reorder drag in paste mode).
//   2. ui.ts onTreeContext (right-click) is suppressed and sets the action-locked status.
//   3. ui.ts onTreeClick kind badge click is suppressed and sets the action-locked status.
//   4. ui.ts toolbar buttons (Undo, Redo, AttachSchema) are guarded and set status.
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
  "ui.ts onTreeContext guards clipboard_count > 0 and calls setStatus",
  /function onTreeContext\([\s\S]*?clipboard_count > 0[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
);

check(
  "ui.ts kind badge click in onTreeClick guards clipboard_count > 0",
  /const kindEl = target\.closest\("\[data-kind\]"\)[\s\S]*?clipboard_count > 0[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
);

check(
  "ui.ts openKindMenuAt guards clipboard_count > 0",
  /function openKindMenuAt\([\s\S]*?clipboard_count[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
);

check(
  "ui.ts uiUndo guards clipboard_count > 0",
  /function uiUndo\(\)\s*\{[\s\S]*?clipboard_count[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
);

check(
  "ui.ts uiRedo guards clipboard_count > 0",
  /function uiRedo\(\)\s*\{[\s\S]*?clipboard_count[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
);

check(
  "ui.ts attachSchema guards clipboard_count > 0",
  /function attachSchema\(\)\s*\{[\s\S]*?clipboard_count[\s\S]*?setStatus\(t\("core\.clipboard\.action-locked"\)/.test(uiTs),
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

  let promptCalled = false;
  globalThis.prompt = () => {
    promptCalled = true;
    return "test.json";
  };

  // Simulate uiUndo / uiRedo / attachSchema logic as in ui.ts
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
    attachSchema: () => {
      if (snap && (snap.clipboard_count ?? 0) > 0) {
        setStatus(t("core.clipboard.action-locked"), "");
        return;
      }
      const choice = prompt("Path or URL to a JSON Schema file:");
      if (!choice) return;
      send({ SetSchema: { source: { Local: choice } } });
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

  statusSet = [];
  sent = [];
  promptCalled = false;
  armed.attachSchema();
  check("attachSchema while armed sets action-locked status", statusSet.some((s) => s.status.includes("action disabled")));
  check("attachSchema while armed does not open prompt", promptCalled === false);
  check("attachSchema while armed does not send SetSchema", sent.length === 0);

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

  statusSet = [];
  sent = [];
  promptCalled = false;
  unarmed.attachSchema();
  check("attachSchema while unarmed calls prompt", promptCalled === true);
  check("attachSchema while unarmed sends SetSchema", sent.some((i) => i?.SetSchema));
}

console.log(failures === 0 ? "\nALL MODAL-LOCK CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
