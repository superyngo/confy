// Plain-Node test pinning `fab.ts`'s pure add/paste decision logic and markup,
// shared between the desktop (`ui.ts`) and touch (`touch/app.ts`) FAB. Follows
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

const { fabAddAction, fabHTML } = await bundle("fab.ts");

// A minimal ViewRow; tests override specific fields.
function makeRow(overrides = {}) {
  return {
    path: [],
    depth: 0,
    is_branch: false,
    key: "k",
    value: undefined,
    kind_label: "string",
    read_only: false,
    trailing_comment: undefined,
    violations: undefined,
    selected: false,
    is_cursor: false,
    has_descendant_violation: false,
    ...overrides,
  };
}

function makeSnap(overrides = {}) {
  return { clipboard_count: 0, rows: [], ...overrides };
}

console.log("-- fabAddAction() --");
{
  check("null snapshot -> null", fabAddAction(null) === null);

  const locked = fabAddAction(makeSnap({ clipboard_count: 1, rows: [] }));
  check("armed clipboard -> locked", locked?.kind === "locked", JSON.stringify(locked));

  const noCursor = fabAddAction(makeSnap({ rows: [makeRow({ is_cursor: false })] }));
  check(
    "no cursor row -> AddNode",
    noCursor?.kind === "add" && noCursor.intent === "AddNode" && noCursor.noticeKey === "web.host.add.node",
    JSON.stringify(noCursor)
  );

  const expandedBranch = fabAddAction(
    makeSnap({
      rows: [
        makeRow({ is_branch: true, is_cursor: true, depth: 0 }),
        makeRow({ depth: 1 }), // deeper successor => expanded
      ],
    })
  );
  check(
    "cursor on expanded branch -> AddChild",
    expandedBranch?.kind === "add" &&
      expandedBranch.intent === "AddChild" &&
      expandedBranch.noticeKey === "web.host.add.child",
    JSON.stringify(expandedBranch)
  );

  const collapsedBranch = fabAddAction(
    makeSnap({
      rows: [
        makeRow({ is_branch: true, is_cursor: true, depth: 0 }),
        makeRow({ depth: 0 }), // same-depth successor => collapsed
      ],
    })
  );
  check(
    "cursor on collapsed branch -> AddSibling",
    collapsedBranch?.kind === "add" &&
      collapsedBranch.intent === "AddSibling" &&
      collapsedBranch.noticeKey === "web.host.add.sibling",
    JSON.stringify(collapsedBranch)
  );

  const leaf = fabAddAction(makeSnap({ rows: [makeRow({ is_branch: false, is_cursor: true })] }));
  check(
    "cursor on leaf -> AddSibling",
    leaf?.kind === "add" && leaf.intent === "AddSibling" && leaf.noticeKey === "web.host.add.sibling",
    JSON.stringify(leaf)
  );
}

console.log("\n-- fabHTML() --");
{
  const html = fabHTML();
  check("has data-act=add", html.includes('data-act="add"'));
  check("has data-act=pastecancel", html.includes('data-act="pastecancel"'));
  check("no ids by default", !html.includes("id="), html);

  const withIds = fabHTML({ fab: "fab", clear: "fabClear" });
  check("id=fab present", withIds.includes('id="fab"'));
  check("id=fabClear present", withIds.includes('id="fabClear"'));
}

console.log(failures === 0 ? "\nALL FAB CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
