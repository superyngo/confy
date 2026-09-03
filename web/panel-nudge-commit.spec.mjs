// Regression: the detail panel must commit a **programmatically written**
// value (the wheel/swipe nudge writes `input.value` straight into the field,
// `web/panel.ts`'s `nudgeRepr` path) — the browser resets its "text as of last
// change event" baseline on a script write, so no `change` event ever fires
// for a nudged value and the old change-only commit dropped the nudge
// silently: the panel showed the stepped number while the tree/document kept
// the old one. `wirePanel` now also commits on blur whenever the field's text
// differs from what was rendered (Escape still cancels, because it restores
// the rendered text before blurring).
//
// Framework-free DOM shim: only the surface `wirePanel` actually touches
// (querySelector + addEventListener + value/style), same "no jsdom" spirit as
// the rest of this suite.
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

async function bundleSource(contents) {
  const result = await esbuild.build({
    stdin: { contents, resolveDir: here, sourcefile: "test-entry.ts", loader: "ts" },
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

// `wirePanel` installs document-level listeners for the touch swipe and the
// desktop wheel.
globalThis.document = { addEventListener() {}, removeEventListener() {} };

const { wirePanel } = await bundleSource(`export { wirePanel } from "./panel.ts";`);

class FakeInput {
  constructor(value) {
    this.value = value;
    this.disabled = false;
    this.style = {};
    this.listeners = {};
  }
  addEventListener(type, fn) {
    (this.listeners[type] ??= []).push(fn);
  }
  emit(type, ev = {}) {
    for (const fn of this.listeners[type] ?? []) fn(ev);
  }
  focus() {
    this.emit("focus");
  }
  blur() {
    this.emit("blur");
  }
}

const row = {
  path: [{ Key: "mask" }],
  depth: 0,
  is_branch: false,
  key: "mask",
  type_label: "integer",
  badge_label: "int",
  badge_note: "0x",
  format: "Hex",
  value: "0xFF",
  scalar_type: "Integer",
  key_sign: "Bare",
  read_only: false,
  selected: false,
  is_cursor: true,
  has_descendant_violation: false,
  child_count: 0,
};

// Wire a panel whose only field is the value input, and record every intent.
function wire(valueText) {
  const ve = new FakeInput(valueText);
  const intents = [];
  const container = {
    querySelector(sel) {
      return sel === '[data-field="value"]' ? ve : null;
    },
  };
  wirePanel(
    container,
    row,
    (intent) => {
      intents.push(intent);
      return { notice: undefined };
    },
    (_p, text, delta) => (text === "0xFF" && delta === 1 ? "0x104" : undefined),
    () => {},
    () => {},
  );
  return { ve, intents };
}

console.log("\n-- panel value field: programmatic (nudge) write commits on blur --");
{
  const { ve, intents } = wire("0xFF");
  ve.focus();
  // What the wheel nudge does: write the core-computed repr, no `change` event.
  ve.value = "0x104";
  ve.blur();
  const commit = intents.find((i) => typeof i === "object" && "CommitEdit" in i);
  check("a nudged value commits on blur", commit !== undefined, JSON.stringify(intents));
  check(
    "it commits the nudged text (schema-stepped hex, not the pre-nudge value)",
    commit?.CommitEdit?.value === "0x104",
    JSON.stringify(intents),
  );
  check(
    "the cursor is set to the panel's row first",
    JSON.stringify(intents[0]) === JSON.stringify({ SetCursor: row.path }),
    JSON.stringify(intents),
  );
}

console.log("\n-- panel value field: unchanged / cancelled input does not commit --");
{
  const { ve, intents } = wire("0xFF");
  ve.focus();
  ve.blur();
  check("an untouched field commits nothing on blur", intents.length === 0, JSON.stringify(intents));
}
{
  const { ve, intents } = wire("0xFF");
  ve.focus();
  ve.value = "0x104"; // nudged…
  ve.emit("keydown", { key: "Escape", stopPropagation() {} }); // …then cancelled
  check("Escape restores the rendered text", ve.value === "0xFF");
  check("a cancelled nudge commits nothing", intents.length === 0, JSON.stringify(intents));
}

console.log("\n-- panel value field: a typed edit still commits exactly once --");
{
  const { ve, intents } = wire("0xFF");
  ve.focus();
  ve.value = "0x1F";
  ve.emit("change"); // user-typed edits fire change, then blur
  ve.blur();
  const commits = intents.filter((i) => typeof i === "object" && "CommitEdit" in i);
  check("exactly one CommitEdit for change-then-blur", commits.length === 1, JSON.stringify(intents));
}

process.exit(failures === 0 ? 0 : 1);
