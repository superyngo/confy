// Drift guard: docs/reference/KEYMAP.md is the single source of truth for the
// normal-mode keymap on both surfaces. This spec checks every **Web** cell of
// that table against the real `resolveKeyIntent`, and additionally scans the
// whole key space so a *newly added* web binding that nobody documented fails
// here. The TUI half of the same table is checked by the `keymap_doc_*` tests
// in crates/confy-tui/src/tui/keys.rs.
//
// Scope: unmodified keys plus the table's explicit `Ctrl+`/`Shift+` rows —
// matching the "Scope of the machine-checked table" section of KEYMAP.md.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import esbuild from "esbuild";

const here = fileURLToPath(new URL(".", import.meta.url));
const DOC = fileURLToPath(new URL("../docs/reference/KEYMAP.md", import.meta.url));

let failures = 0;
const check = (name, cond, extra = "") => {
  if (cond) console.log(`  ✓ ${name}`);
  else {
    console.log(`  ✗ ${name}${extra ? ` — ${extra}` : ""}`);
    failures++;
  }
};

// ---- load the real resolver (same bundling path key-intent.spec.mjs uses) ----
const result = esbuild.buildSync({
  entryPoints: [here + "key-intent.ts"],
  bundle: true,
  format: "esm",
  write: false,
  target: "es2022",
});
const modUrl =
  "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const { resolveKeyIntent } = await import(modUrl);

// ---- parse the SSOT table ----
const doc = readFileSync(DOC, "utf8");
const begin = doc.indexOf("<!-- KEYMAP-TABLE:BEGIN -->");
const end = doc.indexOf("<!-- KEYMAP-TABLE:END -->");
if (begin < 0 || end < 0) {
  console.log("  ✗ KEYMAP.md is missing the KEYMAP-TABLE:BEGIN/END markers");
  process.exit(1);
}
const unwrap = (cell) => cell.trim().replace(/^`(.*)`$/, "$1").replace(/^\*\*(.*)\*\*$/, "$1");
// Detect the header/separator rows on the *raw* cells: the `-` key row unwraps
// to a bare "-" and would otherwise be swallowed by a naive dash test.
const isSeparator = (raw) => raw.every((c) => /^:?-{3,}:?$/.test(c.trim()));
const rows = doc
  .slice(begin, end)
  .split("\n")
  .filter((l) => l.trim().startsWith("|"))
  .map((l) => l.trim().slice(1, -1).split("|"))
  .filter((raw) => !isSeparator(raw) && raw[0].trim() !== "Key")
  .map((raw) => raw.map(unwrap))
  .map(([key, tui, web, status]) => ({ key, tui, web, status }));

console.log(`-- KEYMAP.md parsed: ${rows.length} rows --`);
check("table is non-empty", rows.length > 20, `got ${rows.length}`);

// ---- canonical key name -> (ev.key, mods) ----
function asEvent(name) {
  if (name.startsWith("Shift+")) return { key: name.slice(6), shift: true, ctrl: false };
  if (name.startsWith("Ctrl+")) return { key: name.slice(5), shift: false, ctrl: true };
  const key = name === "Space" ? " " : name;
  // A shifted character really does arrive with shiftKey set; pass it so the
  // table is checked under realistic modifier state.
  return { key, shift: /^[A-Z]$/.test(key), ctrl: false };
}

// ---- KeyResolution -> the table's compact encoding ----
function encode(r) {
  if (!r) return "—";
  const intent = (i) =>
    typeof i === "string" ? i : `${Object.keys(i)[0]}(${Object.values(i)[0]})`;
  switch (r.kind) {
    case "intent": return `intent:${intent(r.intent)}`;
    case "nav": return `nav:${intent(r.intent)}`;
    case "native": return `native:${r.action}`;
    case "tree-page": return `tree-page(${r.dir})`;
    case "typefilter-page": return `typefilter-page(${r.dir})`;
    default: return `?${r.kind}`;
  }
}

const resolveNamed = (name) => {
  const { key, shift, ctrl } = asEvent(name);
  return encode(resolveKeyIntent("Normal", key, { ctrl, shift }, false, false));
};

// ---- 1. every documented Web cell matches the implementation ----
console.log("-- documented Web cells match resolveKeyIntent --");
for (const row of rows) {
  const actual = resolveNamed(row.key);
  check(`${row.key} -> ${row.web}`, actual === row.web, `resolver gave ${actual}`);
}

// ---- 2. Status column agrees with the two binding columns ----
console.log("-- Status column is consistent --");
for (const row of rows) {
  const tuiBound = row.tui !== "—";
  const webBound = row.web !== "—";
  const expected = tuiBound && webBound ? "both" : tuiBound ? "tui-only" : webBound ? "web-only" : "unbound";
  check(`${row.key} status=${row.status}`, row.status === expected, `derived ${expected}`);
}

// ---- 3. completeness: no undocumented web binding ----
// Any key that resolves to something must appear in the table. This is the
// half that would have caught `E` being absent had it existed the other way.
console.log("-- completeness scan (no undocumented web binding) --");
const documented = new Set(rows.map((r) => r.key));
const named = [
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End",
  "PageUp", "PageDown", "Enter", "Escape", "Delete", "Backspace", "Tab", "Space",
  ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
];
const printable = Array.from({ length: 0x7e - 0x20 }, (_, i) => String.fromCharCode(0x21 + i));
const candidates = [
  ...named,
  ...printable,
  ...printable.filter((c) => /[a-z]/.test(c)).map((c) => `Ctrl+${c}`),
  "Shift+ArrowUp", "Shift+ArrowDown",
];

const undocumented = [];
for (const name of candidates) {
  const enc = resolveNamed(name);
  if (enc !== "—" && !documented.has(name)) undocumented.push(`${name} -> ${enc}`);
}
check(
  "every web binding is in KEYMAP.md",
  undocumented.length === 0,
  undocumented.join(", "),
);

// ---- 4. the table claims nothing the resolver refuses ----
// (covered by check 1, but assert the `—` rows really are unbound so a
// tui-only row can't quietly become bound on the web without a doc edit)
console.log("-- documented web-unbound keys really are unbound --");
for (const row of rows.filter((r) => r.web === "—")) {
  const actual = resolveNamed(row.key);
  check(`${row.key} stays unbound on the web`, actual === "—", `resolver gave ${actual}`);
}

if (failures) {
  console.log(`\n${failures} KEYMAP PARITY CHECK(S) FAILED`);
  process.exit(1);
}
console.log("\nALL KEYMAP PARITY CHECKS PASSED");
