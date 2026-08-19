// Plain-Node test for touch's post-paste highlight (ROW_STATE_MODEL.md §6d):
// mirrors `web/ui.ts`'s `send()` — a Paste that just landed (clipboard_count
// drops to 0, no error, mode back to Normal) re-selects the pasted/moved batch
// via one extra `SetSelection`, so it stays visibly highlighted. Runs the real
// shipped `send()` body extracted verbatim from `touch/app.ts` (same technique
// as `touch-modal-lock.spec.mjs`), never a reimplementation.
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

const sendBlock = appTs.match(/^function send\([\s\S]*?\n\}/m)?.[0];
check("send extracted verbatim", !!sendBlock);
check(
  "send mirrors ui.ts's post-paste reselect compensator",
  !!sendBlock && /preClip > 0/.test(sendBlock) && /SetSelection/.test(sendBlock) && /session\.children\(parent\)/.test(sendBlock),
);

let mod = null;
{
  const src = `let session = null;
let snap = null;
let renderCalls = 0;
const isBatching = () => false;
function render() { renderCalls++; }
export function setEnv(e) { session = e.session; snap = e.snap; renderCalls = 0; }
export function getRenderCalls() { return renderCalls; }
${sendBlock ?? "function send(i) {}"}
export { send };
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

const P = (k) => [{ Key: k }];
const siblings = [{ path: P("a") }, { path: P("b") }, { path: P("c") }, { path: P("d") }];

function sessionStub(nextSnap, childrenList = siblings) {
  const dispatched = [];
  return {
    dispatched,
    session: {
      dispatch: (i) => {
        dispatched.push(i);
        return dispatched.length === 1 ? nextSnap : { ...nextSnap, mode: "Normal", error: null };
      },
      children: (_p) => childrenList,
    },
  };
}

console.log("\n-- paste lands: re-selects the pasted batch --");
{
  const { session, dispatched } = sessionStub({
    cursor: P("b"),
    clipboard_count: 0,
    error: null,
    mode: "Normal",
  });
  mod.setEnv({ session, snap: { clipboard_count: 2 } });
  mod.send("Paste");
  check("send issues exactly two dispatches (Paste + SetSelection)", dispatched.length === 2, JSON.stringify(dispatched));
  check(
    "second dispatch selects the pasted siblings starting at cursor",
    JSON.stringify(dispatched[1]) === JSON.stringify({ SetSelection: { paths: [P("b"), P("c")] } }),
    JSON.stringify(dispatched[1]),
  );
  check("render runs exactly once (not twice for the compensator)", mod.getRenderCalls() === 1);
}

console.log("\n-- non-paste nav: no compensator fires --");
{
  const { session, dispatched } = sessionStub({
    cursor: P("c"),
    clipboard_count: 0,
    error: null,
    mode: "Normal",
  });
  mod.setEnv({ session, snap: { clipboard_count: 0 } });
  mod.send("CursorDown");
  check("no clipboard was armed before -> single dispatch only", dispatched.length === 1, JSON.stringify(dispatched));
}

console.log("\n-- collision prompt: clipboard stays armed, no compensator --");
{
  const { session, dispatched } = sessionStub({
    cursor: P("b"),
    clipboard_count: 1,
    error: null,
    mode: "Prompt",
  });
  mod.setEnv({ session, snap: { clipboard_count: 1 } });
  mod.send("Paste");
  check("clipboard_count still > 0 -> single dispatch only", dispatched.length === 1, JSON.stringify(dispatched));
}

console.log("\n-- paste failure (error set): no compensator --");
{
  const { session, dispatched } = sessionStub({
    cursor: P("b"),
    clipboard_count: 0,
    error: "core.paste.error",
    mode: "Normal",
  });
  mod.setEnv({ session, snap: { clipboard_count: 2 } });
  mod.send("Paste");
  check("error set -> single dispatch only, no reselect", dispatched.length === 1, JSON.stringify(dispatched));
}

console.log(failures === 0 ? "\nALL TOUCH PASTE-SELECT CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
