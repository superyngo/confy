// Regression test for the touch toast "stuck error keeps popping up" bug:
// render() re-invokes renderNotice(snap.notice) on every dispatched Intent,
// including pure navigation ones (cursor move, ToggleExpand, SetCursor/
// SetSelection) that the core Notice lifecycle deliberately leaves untouched
// (MESSAGES.md §1.1). Before the fix, replaying the *same* still-active
// notice on each such unrelated re-render re-triggered the toast's entrance
// animation and restarted its 3s/1.6s auto-hide timer — so tapping any other
// (valid) node, toggling expand, or dragging the move-grip after a failed
// cut/paste made the same error toast keep popping back up until an actual
// mutation (successful paste) or clipboard clear transitioned Session.notice
// away. Runs the real shipped `renderNotice()` extracted verbatim from
// `touch/app.ts` (same technique as touch-paste-select.spec.mjs), never a
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

const appTs = readFileSync(path.join(here, "touch/app.ts"), "utf8");

const fnBlock = appTs.match(
  /^let toastT: number \| undefined;\n[\s\S]*?^function renderNotice\([\s\S]*?\n\}/m,
)?.[0];
check("renderNotice (+ state) extracted verbatim", !!fnBlock);

let mod = null;
{
  const src = `type Notice = { severity: string; text: string; source: string };
export const calls: { op: string; arg?: string }[] = [];
export let timeoutCount = 0;
const toastEl = {
  textContent: "",
  classList: {
    add(c: string) { calls.push({ op: "add", arg: c }); },
    remove(c: string) { calls.push({ op: "remove", arg: c }); },
  },
};
const window = {
  setTimeout(fn: () => void, _ms: number) { timeoutCount++; return 0; },
};
function clearTimeout(_t: unknown) {}
${fnBlock ?? "function renderNotice(n: unknown) {}"}
export { renderNotice };
export function resetCalls() { calls.length = 0; timeoutCount = 0; }
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  mod = await import(modUrl);
}

const errNotice = { severity: "error", text: "core.paste.error", source: "core" };
const errNotice2 = { severity: "error", text: "core.paste.error", source: "core" }; // distinct object, same content

console.log("\n-- first error notice: shows toast, starts one timer --");
{
  mod.resetCalls();
  mod.renderNotice(errNotice);
  check("adds sev-error class", mod.calls.some((c) => c.op === "add" && c.arg === "sev-error"));
  check("adds show class", mod.calls.some((c) => c.op === "add" && c.arg === "show"));
  check("starts exactly one auto-hide timer", mod.timeoutCount === 1, `got ${mod.timeoutCount}`);
}

console.log("\n-- unrelated re-render replays the SAME notice (nav/expand-toggle/move-grip) --");
{
  mod.resetCalls();
  // Simulates render() firing again for a non-mutating Intent (ToggleExpand,
  // SetCursor, SetSelection, ...) while Session.notice is unchanged.
  mod.renderNotice(errNotice2);
  check("does NOT re-add show class", !mod.calls.some((c) => c.op === "add" && c.arg === "show"));
  check("does NOT restart the auto-hide timer", mod.timeoutCount === 0, `got ${mod.timeoutCount}`);
}

console.log("\n-- notice clears (paste completes / clipboard cleared), then reappears --");
{
  mod.resetCalls();
  mod.renderNotice(undefined);
  check("clears the show class on notice=undefined", mod.calls.some((c) => c.op === "remove" && c.arg === "show"));

  mod.resetCalls();
  mod.renderNotice(errNotice);
  check("a fresh occurrence after clearing shows the toast again", mod.calls.some((c) => c.op === "add" && c.arg === "show"));
  check("a fresh occurrence after clearing restarts the timer", mod.timeoutCount === 1, `got ${mod.timeoutCount}`);
}

console.log("\n-- genuinely different notice content still (re)shows immediately --");
{
  const warnNotice = { severity: "warn", text: "core.clipboard.action-locked", source: "core" };
  mod.resetCalls();
  mod.renderNotice(warnNotice);
  check("adds sev-warn class", mod.calls.some((c) => c.op === "add" && c.arg === "sev-warn"));
  check("adds show class", mod.calls.some((c) => c.op === "add" && c.arg === "show"));
  check("starts a new timer", mod.timeoutCount === 1, `got ${mod.timeoutCount}`);
}

console.log(failures === 0 ? "\nALL TOUCH NOTICE-TOAST CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
