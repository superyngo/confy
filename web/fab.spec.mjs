// Plain-Node test pinning `fab.ts`'s pure FAB markup helpers, shared between
// the desktop (`ui.ts`) and touch (`touch/app.ts`) FAB. Follows
// `render.spec.mjs`'s convention: no test framework, just `node:assert` + a
// `check()` tally, bundled with esbuild since `fab.ts` imports `kind-labels.ts`.
import path from "node:path";
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

async function bundle(entry) {
  const result = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const code = result.outputFiles[0].text;
  const modUrl = "data:text/javascript;base64," + Buffer.from(code).toString("base64");
  return import(modUrl);
}

const { fabHTML } = await bundle("fab.ts");

console.log("\n-- fabHTML() --");
{
  const html = fabHTML();
  check("has data-act=actions", html.includes('data-act="actions"'));
  check("has data-act=pastecancel", html.includes('data-act="pastecancel"'));
  check("no ids by default", !html.includes("id="), html);

  const withIds = fabHTML({ fab: "fab", clear: "fabClear" });
  check("id=fab present", withIds.includes('id="fab"'));
  check("id=fabClear present", withIds.includes('id="fabClear"'));
}

console.log(failures === 0 ? "\nALL FAB CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
