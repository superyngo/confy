// Plain-Node test for ui.ts's `renderNotice` toast dedupe: `render()` calls
// `renderNotice(snap.notice)` on every dispatch, including pure navigation
// intents (cursor move, ToggleExpand, SetPasteSlot, …) that core's Notice
// lifecycle deliberately leaves untouched (MESSAGES.md §1.1) — without a
// dedupe guard, a still-armed clipboard's sticky "cut N node(s)" success
// notice replays the toast's enter animation/timer on every unrelated
// redraw. `renderNotice` fingerprints `${severity}|${text}` into a
// module-level `lastNoticeKey` (same fix as touch/app.ts) and only shows the
// toast when the fingerprint changes; the status bar text must still repaint
// unconditionally on every call. Follows armed-paste.spec.mjs's convention:
// no test framework, just `node:assert` + a `check()` tally; `ui.ts` can't
// be imported in Node (wasm + DOM top-level wiring), so `renderNotice` is
// extracted verbatim from ui.ts's source and type-stripped via esbuild — the
// behavioral checks below run the real shipped function body, not a
// reimplementation.
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

// ---- static checks against the source ----
const noticeMatch = uiTs.match(/^function renderNotice\([\s\S]*?\n\}/m);
check("ui.ts defines renderNotice()", !!noticeMatch);
check(
  "ui.ts declares a module-level lastNoticeKey guard",
  /let lastNoticeKey: string \| undefined;/.test(uiTs),
);
check(
  "renderNotice only shows the toast when the fingerprint changes",
  !!noticeMatch && /if \(key !== lastNoticeKey\)/.test(noticeMatch[0]),
);
check(
  "renderNotice resets lastNoticeKey when the notice clears",
  !!noticeMatch && /if \(!notice\)\s*\{[\s\S]{0,60}lastNoticeKey = undefined;/.test(noticeMatch[0]),
);

// ---- extract + execute the real renderNotice ----
let renderNotice = null;
let setEnv = null;
if (noticeMatch) {
  const src = `let toastEl, statusEl, errorEl;
let toastT: number | undefined;
let lastNoticeKey: string | undefined;
export function setEnv(e: any) { toastEl = e.toastEl; statusEl = e.statusEl; errorEl = e.errorEl; }
export ${noticeMatch[0]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  ({ renderNotice, setEnv } = await import(modUrl));
}
if (!renderNotice) {
  // Pre-implementation (RED): keep the tally flowing so every behavioral
  // check below reports ✗ instead of crashing on undefined imports.
  renderNotice = () => {};
  setEnv = () => {};
}

// ---- minimal DOM shim (no jsdom, no new npm dependency) ----
function mkEl() {
  const addCalls = [];
  const classes = new Set();
  return {
    textContent: "",
    classList: {
      add: (c) => {
        addCalls.push(c);
        classes.add(c);
      },
      remove: (c) => classes.delete(c),
      contains: (c) => classes.has(c),
    },
    addCalls,
    classes,
  };
}

console.log("\n-- renderNotice dedupes the toast for a repeated notice, replays for a changed one --");
{
  const toastEl = mkEl();
  const statusEl = mkEl();
  const errorEl = mkEl();
  setEnv({ toastEl, statusEl, errorEl });
  const showCount = () => toastEl.addCalls.filter((c) => c === "show").length;

  renderNotice({ severity: "success", text: "cut 1 node(s)" });
  check("first call shows the toast", showCount() === 1);
  check("first call sets the status text", statusEl.textContent === "cut 1 node(s)");

  renderNotice({ severity: "success", text: "cut 1 node(s)" });
  check("second call with the identical notice does not replay the toast", showCount() === 1);
  check("second call still repaints the status text", statusEl.textContent === "cut 1 node(s)");

  renderNotice({ severity: "success", text: "cut 2 node(s)" });
  check("a notice with different text replays the toast", showCount() === 2);
  check("status text reflects the new notice", statusEl.textContent === "cut 2 node(s)");

  renderNotice(undefined);
  check("clearing the notice resets the status text", statusEl.textContent === "");

  renderNotice({ severity: "success", text: "cut 2 node(s)" });
  check("the same notice re-shown after a clear replays the toast again", showCount() === 3);
}

console.log(failures === 0 ? "\nALL TOAST-DEDUPE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
