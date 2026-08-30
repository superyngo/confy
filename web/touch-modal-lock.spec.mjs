// Plain-Node test for touch modal lock (ADR 0005 §5): while the clipboard
// is armed (clipboard_count > 0), mutations, modal entries, swipe-to-delete,
// double-tap detail sheet, and toolbar buttons are disabled and dispatch
// Intent::SetHostNotice (core.clipboard.action-locked). Grip reorder instead
// hides the grip outright via CSS (`.app.paste-mode .drag-handle`) so it can
// never be tapped while armed — no runtime guard/toast for it.
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

const appTs = readFileSync(path.join(here, "touch/app.ts"), "utf8");

// ---- 1. Structural checks on touch/app.ts ----
console.log("-- structural: touch/app.ts guards while clipboard is armed --");

const startReorderBlock = appTs.match(/^function startReorder\([\s\S]*?\n\}/m)?.[0] ?? "";
check("startReorder found in source", startReorderBlock.length > 0);
check(
  "startReorder no longer guards clipboard_count (armed-state protection moved to CSS)",
  !/clipboard_count/.test(startReorderBlock),
);

const installGesturesBlock = appTs.match(/^function installTreeGestures\(\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
check("installTreeGestures found in source", installGesturesBlock.length > 0);
check(
  "installTreeGestures swipe-to-delete guards armed clipboard",
  /clipboard_count[\s\S]*?swipeMain\s*=/.test(installGesturesBlock),
);

const touchCss = readFileSync(path.join(here, "touch/style.css"), "utf8");
check(
  "touch/style.css hides the drag-handle grip while paste-mode is armed",
  /\.app\.paste-mode\s+\.drag-handle\s*\{\s*visibility:\s*hidden/.test(touchCss),
);

const openPanelBlock = appTs.match(/^function openPanel\([\s\S]*?\n\}/m)?.[0] ?? "";
check("openPanel found in source", openPanelBlock.length > 0);
check(
  "openPanel checks clipboard_count and dispatches SetHostNotice",
  /clipboard_count/.test(openPanelBlock) && /SetHostNotice/.test(openPanelBlock),
);

const shellHandlersBlock = appTs.match(/^function installShellHandlers\(\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
check("installShellHandlers found in source", shellHandlersBlock.length > 0);
check(
  "installShellHandlers undo/redo/open/save/filter/info/expand/collapse guards armed clipboard",
  /case "undo":[\s\S]*?clipboard_count/.test(shellHandlersBlock) &&
    /case "save":[\s\S]*?clipboard_count/.test(shellHandlersBlock) &&
    /case "filter":[\s\S]*?clipboard_count/.test(shellHandlersBlock),
);

const openMenuSheetBlock = appTs.match(/^function openMenuSheet\(\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
check("openMenuSheet found in source", openMenuSheetBlock.length > 0);
check(
  "openMenuSheet item clicks guard armed clipboard",
  /clipboard_count/.test(openMenuSheetBlock) && /SetHostNotice/.test(openMenuSheetBlock),
);

// ---- 2. Behavioral checks with extracted functions ----
console.log("\n-- behavioral: touch modal lock execution --");

// Extract functions for execution
const fns = ["pathOf", "startReorder", "openPanel", "installTreeGestures", "installShellHandlers"]
  .map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0])
  .map((s, i) => {
    check(`${["pathOf", "startReorder", "openPanel", "installTreeGestures", "installShellHandlers"][i]} extracted verbatim`, !!s);
    return s ?? `function ${["pathOf", "startReorder", "openPanel", "installTreeGestures", "installShellHandlers"][i]}() {}`;
  });

const H = (globalThis.__touchModalHooks = { sent: [], ops: [], toasts: [] });
let mod = null;
{
  const src = `let session = null;
let snap = null;
let reordering = false;
let reRow = null;
let reStartY = 0;
let reMoved = false;
let reTarget = null;
let reMode = "before";
let reLine = null;
let reSrcPath = null;
let rawView = false;
let sx = 0;
let sy = 0;
let dragRow = null;
let dragging = false;
let moved = false;
let pasteDragActive = false;
let pasteDragStartY = 0;
let pasteDragMoved = false;
let pasteDragRow = null;
let edgeScrollY = 0;
let edgeScrollRAF = null;
let swiping = false;
let swipeMain = null;
let swipeHasDel = false;
let swipeHasRemark = false;
let swipeBase = 0;
let swipeOff = 0;
let openSwipeMain = null;
let openSwipeOff = 0;
const SWIPE_W = 72;
const treeListeners = {};
const appListeners = {};
const treeEl = {
  addEventListener: (t, fn) => { treeListeners[t] = fn; },
  contains: (el) => el && el.__inTree,
  querySelector: () => null,
  setPointerCapture: () => {},
};
const treePane = {
  addEventListener: () => {},
};
const searchInput = {
  value: "",
  parentElement: { classList: { toggle: () => {}, remove: () => {} } },
  addEventListener: () => {},
};
const app = {
  addEventListener: (t, fn) => { appListeners[t] = fn; },
  querySelector: () => null,
};
const sheets = {
  detail: { innerHTML: "", addEventListener: () => {} },
  menu: { innerHTML: "", addEventListener: () => {} },
  lang: { innerHTML: "", addEventListener: () => {} },
  save: { innerHTML: "", addEventListener: () => {} },
  url: { innerHTML: "", classList: { contains: () => false }, addEventListener: () => {} },
  ext: { innerHTML: "", classList: { contains: () => false }, addEventListener: () => {} },
};

const H = globalThis.__touchModalHooks;
const send = (i) => H.sent.push(i);
const sendR = (i) => { H.sent.push(i); return {}; };
const selectOnly = (p) => H.ops.push("selectOnly " + JSON.stringify(p));
const toast = (m) => H.toasts.push(m);
const openSheet = (s) => H.ops.push("openSheet " + s);
const closeSheets = () => H.ops.push("closeSheets");
function t(k) { return "locked:" + k; }
const isWide = () => false;
const esc = (s) => s;
const lastKey = (p) => "k";
const parentIsInline = () => false;
const panelHTML = () => "";
const wirePanel = () => {};
const openKindRow = () => {};
const afterPanelMutation = () => {};
const IC = { close: "" };
const rowFor = (p) => snap?.rows?.find((r) => JSON.stringify(r.path) === JSON.stringify(p)) ?? null;
const setSwipeRevealed = (m, on) => H.ops.push("setSwipeRevealed " + on);
const onReorderMove = () => {};
const handleTap = () => {};
const onPasteDragMove = () => {};
const finishPasteDrag = () => {};
const renderPasteSlotCue = () => {};
const addContextual = () => H.ops.push("addContextual");
const cycleSampleFormat = () => {};
const openSample = () => {};
const openSaveSheet = () => H.ops.push("openSaveSheet");
const openLangSheet = () => H.ops.push("openLangSheet");
const openMenuSheet = () => H.ops.push("openMenuSheet");
const openOpenSheet = () => H.ops.push("openOpenSheet");
const toggleTheme = () => H.ops.push("toggleTheme");
const setRawView = (v) => { rawView = v; };
const edgeAutoScrollStep = () => {};
const kickEdgeAutoScroll = () => {};
const requestAnimationFrame = (fn) => 0;

export function setEnv(e) {
  session = e.session ?? session;
  snap = e.snap ?? snap;
  reordering = false;
  dragging = false;
  swiping = false;
  swipeMain = null;
  dragRow = null;
}
export function getReordering() { return reordering; }
export function getSwiping() { return swiping; }
export function getSwipeMain() { return swipeMain; }
export function triggerPointerDown(e) { treeListeners["pointerdown"](e); }
export function triggerPointerMove(e) { treeListeners["pointermove"](e); }
export function triggerAppClick(target) {
  const ev = { target };
  appListeners["click"](ev);
}
export ${fns[0]}
export ${fns[1]}
export ${fns[2]}
export ${fns[3]}
export ${fns[4]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  mod = await import(modUrl);
  mod.installTreeGestures();
  mod.installShellHandlers();
}

// 1. Grip drag/pointerdown while armed: since the grip is hidden outright via
// CSS (`.app.paste-mode .drag-handle`, checked structurally above) rather
// than guarded at runtime, calling straight into the handlers (bypassing the
// CSS a real pointer could never get past) now starts reorder unconditionally
// — there is no code-level lock left to assert here.
{
  H.toasts.length = 0;
  H.sent.length = 0;
  mod.setEnv({ snap: { clipboard_count: 1 } });
  const row = {
    dataset: { path: JSON.stringify([{ Key: "a" }]) },
    classList: { add: () => {}, remove: () => {} },
  };
  const gripEl = {
    closest: (sel) => (sel === ".drag-handle" ? gripEl : sel === ".row" ? row : null),
  };
  mod.triggerPointerDown({ target: gripEl, clientY: 100, pointerId: 1 });
  check(
    "grip pointerdown starts reorder even when armed (no runtime guard; CSS hides the grip instead)",
    mod.getReordering() === true,
  );
  check(
    "grip pointerdown never dispatches action-locked SetHostNotice",
    !H.sent.some((i) => i && i.SetHostNotice && i.SetHostNotice.key === "core.clipboard.action-locked"),
    JSON.stringify(H.sent),
  );
}

// 2. Grip drag when unarmed (clipboard_count = 0): starts reorder
{
  H.toasts.length = 0;
  H.sent.length = 0;
  mod.setEnv({ snap: { clipboard_count: 0 } });
  const row = {
    dataset: { path: JSON.stringify([{ Key: "a" }]) },
    classList: { add: () => {}, remove: () => {} },
  };
  mod.startReorder({ clientY: 100, pointerId: 1 }, row);
  check("startReorder when unarmed sets reordering", mod.getReordering() === true);
  check("startReorder when unarmed dispatches no SetHostNotice", !H.sent.some((i) => i && i.SetHostNotice));
}

// 4. Swipe-to-delete when armed: does not reveal delete button or start swipe
{
  H.ops.length = 0;
  mod.setEnv({ snap: { clipboard_count: 1 } });
  const row = {
    querySelector: (sel) => (sel === ".row-del" ? rowDel : sel === ".row-main" ? rowMain : null),
  };
  const rowMain = { style: {}, closest: (sel) => (sel === ".row" ? row : null) };
  const rowDel = {};
  const target = {
    closest: (sel) => (sel === ".row-main" ? rowMain : sel === ".row" ? row : null),
  };
  mod.triggerPointerDown({ target, clientX: 100, clientY: 100 });
  check("pointerdown when armed does not set swipeMain", mod.getSwipeMain() === null);

  mod.triggerPointerMove({ clientX: 50, clientY: 100, preventDefault: () => {} });
  check("pointermove when armed does not set swiping", mod.getSwiping() === false);
  check("pointermove when armed does not call setSwipeRevealed", !H.ops.some((o) => o.startsWith("setSwipeRevealed")));
}

// 5. Swipe-to-delete when unarmed: allows swipe
{
  H.ops.length = 0;
  mod.setEnv({ snap: { clipboard_count: 0 } });
  const row = {
    classList: { add: () => {}, remove: () => {} },
    querySelector: (sel) => (sel === ".row-del" ? rowDel : sel === ".row-main" ? rowMain : null),
  };
  const rowMain = { style: {}, closest: (sel) => (sel === ".row" ? row : null) };
  const rowDel = {};
  const target = {
    closest: (sel) => (sel === ".row-main" ? rowMain : sel === ".row" ? row : null),
  };
  mod.triggerPointerDown({ target, clientX: 100, clientY: 100 });
  check("pointerdown when unarmed sets swipeMain", mod.getSwipeMain() !== null);

  mod.triggerPointerMove({ clientX: 50, clientY: 100, preventDefault: () => {} });
  check("pointermove when unarmed sets swiping", mod.getSwiping() === true);
  check("pointermove when unarmed calls setSwipeRevealed", H.ops.some((o) => o.startsWith("setSwipeRevealed")));
}

// 6. Double tap (openPanel) when clipboard_count > 0: does not open detail sheet, emits toast
{
  H.ops.length = 0;
  H.toasts.length = 0;
  mod.setEnv({
    session: { schemaHint: () => null },
    snap: { clipboard_count: 1, rows: [{ path: [{ Key: "a" }], key: "a", type_label: "string" }] },
  });
  mod.openPanel([{ Key: "a" }]);
  check(
    "openPanel when armed does not open detail sheet",
    !H.ops.includes("openSheet detail"),
    JSON.stringify(H.ops),
  );
  check(
    "openPanel when armed dispatches action-locked SetHostNotice",
    H.sent.some((i) => i && i.SetHostNotice && i.SetHostNotice.key === "core.clipboard.action-locked"),
    JSON.stringify(H.sent),
  );
}

// 7. Toolbar buttons when armed: emit toast and do not perform action
const disabledActs = ["undo", "redo", "save", "open", "filter", "info", "expandall", "collapseall", "toggleview", "cyclefmt", "lang", "searchclear"];
for (const act of disabledActs) {
  H.sent.length = 0;
  H.ops.length = 0;
  H.toasts.length = 0;
  mod.setEnv({ snap: { clipboard_count: 1 } });
  const btn = {
    dataset: { act },
    closest: (sel) => (sel === "[data-act]" ? btn : null),
    __inTree: false,
  };
  mod.triggerAppClick(btn);
  check(
    `toolbar [data-act="${act}"] when armed dispatches action-locked SetHostNotice`,
    H.sent.some((i) => i && i.SetHostNotice && i.SetHostNotice.key === "core.clipboard.action-locked"),
    JSON.stringify(H.sent),
  );
  check(
    `toolbar [data-act="${act}"] when armed sends no other intent`,
    H.sent.every((i) => i && i.SetHostNotice),
    JSON.stringify(H.sent),
  );
}

// 8. FAB [data-act="actions"] when armed sends Paste (allowed invariant)
{
  H.sent.length = 0;
  mod.setEnv({ snap: { clipboard_count: 1 } });
  const btn = {
    dataset: { act: "actions" },
    closest: (sel) => (sel === "[data-act]" ? btn : null),
    __inTree: false,
  };
  mod.triggerAppClick(btn);
  check("FAB actions when armed sends Paste", H.sent.includes("Paste"), JSON.stringify(H.sent));
}

// 9. [data-act="pastecancel"] when armed sends Escape (allowed invariant)
{
  H.sent.length = 0;
  mod.setEnv({ snap: { clipboard_count: 1 } });
  const btn = {
    dataset: { act: "pastecancel" },
    closest: (sel) => (sel === "[data-act]" ? btn : null),
    __inTree: false,
  };
  mod.triggerAppClick(btn);
  check("pastecancel when armed sends Escape", H.sent.includes("Escape"), JSON.stringify(H.sent));
}

console.log(failures === 0 ? "\nALL TOUCH MODAL LOCK CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
