// Tests for the detail panel's unified Schema section (`panelHTML`, `web/panel.ts`):
// non-widget schema info (`schemaInfo`, e.g. type/description), a constraint
// description (`schemaHintText(editHint)`), and/or per-row violation messages
// (`row.violations`) render into a "Schema" block after Kind/Path/Sign but
// before the Actions row-btns (so Actions stays the panel's fixed trailing
// element), mirroring the TUI Detail popup's `Schema:` section — present
// only when there's something to say, absent otherwise.
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
    stdin: {
      contents,
      resolveDir: here,
      sourcefile: "test-entry.ts",
      loader: "ts",
    },
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

const { panelHTML } = await bundleSource(`export { panelHTML } from "./panel.ts";`);

function baseRow(overrides = {}) {
  return {
    path: [{ Key: "port" }],
    depth: 0,
    is_branch: false,
    key: "port",
    type_label: "integer",
    format: "Plain",
    value: "8080",
    key_sign: "Bare",
    read_only: false,
    violations: undefined,
    selected: false,
    is_cursor: true,
    has_descendant_violation: false,
    child_count: 0,
    trailing_comment: undefined,
    ...overrides,
  };
}

console.log("\n-- panelHTML Schema section --");

// No editHint, no violations → no Schema section at all.
const plain = panelHTML(baseRow(), false, "None");
check("no Schema field-label when nothing to say", !plain.includes(">Schema<"));
check("no schema-info block when nothing to say", !plain.includes("schema-info"));

// A constraint (editHint) with no violation → hint text renders, no violation line.
const enumHint = { Enum: [["dev", "dev"], ["prod", "prod"]] };
const constrained = panelHTML(baseRow({ value: "dev" }), false, enumHint);
check("Schema field-label present for a constraint with no violation", constrained.includes(">Schema<"));
check("hint text rendered in schema-hint-msg", constrained.includes("schema-hint-msg") && constrained.includes("Valid values"));
check("no violation message rendered", !constrained.includes("schema-violation-msg"));
check("no has-violation class without a violation", !constrained.includes("has-violation"));

// A violation with no resolvable editHint → violation text renders, no hint line.
const violating = panelHTML(baseRow({ violations: ["'8080' is not of type 'string'"] }), false, "None");
check("Schema field-label present for a violation with no hint", violating.includes(">Schema<"));
check("violation message rendered in schema-violation-msg", violating.includes("schema-violation-msg") && violating.includes("is not of type"));
check("no hint text rendered", !violating.includes("schema-hint-msg"));
check("has-violation class present when the row violates", violating.includes(`class="schema-info has-violation"`));

// Both present → both render.
const both = panelHTML(baseRow({ value: "staging", violations: ["value must be one of the enum values"] }), false, enumHint);
check("both hint and violation render together", both.includes("schema-hint-msg") && both.includes("schema-violation-msg"));

// schemaInfo (type/description) alone, no editHint/violations → still renders.
const infoOnly = panelHTML(baseRow(), false, "None", undefined, "Bind address\nType: string");
check("Schema field-label present for schemaInfo alone", infoOnly.includes(">Schema<"));
check(
  "schemaInfo lines each render in schema-hint-msg",
  (infoOnly.match(/schema-hint-msg/g) ?? []).length === 2,
  infoOnly,
);
check("schemaInfo description line rendered", infoOnly.includes("Bind address"));
check("schemaInfo type line rendered", infoOnly.includes("Type: string"));

// schemaInfo + hint + violation all present → all three render, info first.
const all = panelHTML(
  baseRow({ value: "staging", violations: ["value must be one of the enum values"] }),
  false,
  enumHint,
  undefined,
  "Deployment environment",
);
check(
  "schemaInfo, hint, and violation all render together",
  all.includes("Deployment environment") && all.includes("Valid values") && all.includes("schema-violation-msg"),
);
check(
  "schemaInfo renders before the hint text",
  all.indexOf("Deployment environment") < all.indexOf("Valid values"),
);

// Schema block renders after Kind (locked field order), before the row-btns
// actions (Actions stays the panel's fixed trailing element).
const orderIdx = {
  kind: both.indexOf('data-act="kindswitch"'),
  rowBtns: both.indexOf("row-btns"),
  schema: both.indexOf(">Schema<"),
};
check(
  "Schema block appears after Kind and before the row-btns actions",
  orderIdx.kind !== -1 && orderIdx.rowBtns !== -1 && orderIdx.schema > orderIdx.kind && orderIdx.schema < orderIdx.rowBtns,
  JSON.stringify(orderIdx),
);

process.exit(failures === 0 ? 0 : 1);
