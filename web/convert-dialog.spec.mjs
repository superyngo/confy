// Plain-Node test for `convert-dialog.ts`'s JSONC pseudo-format support (item
// 1: Save As/Convert gains a "JSONC" option alongside "JSON" — same core
// `DocFormat::Json` target, `.jsonc` extension instead of `.json`; core stays
// extension-blind, so the distinction lives entirely in this module). Follows
// `render.spec.mjs`'s convention: no jsdom, minimal manual DOM shims, esbuild
// bundling since this file pulls in `render.ts`/`i18n.ts`.
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

const result = await esbuild.build({
  entryPoints: [path.join(here, "convert-dialog.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const modUrl = "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const { extForTag, renderConvertDialog } = await import(modUrl);

// ---- extForTag: the "Jsonc" pseudo-tag maps to .jsonc, not double-mapped ----
console.log("-- extForTag() --");
check('extForTag("Json") -> ".json"', extForTag("Json") === ".json");
check('extForTag("Jsonc") -> ".jsonc"', extForTag("Jsonc") === ".jsonc");
check('extForTag("Yaml") -> ".yaml"', extForTag("Yaml") === ".yaml");
check('extForTag("Toml") -> ".toml"', extForTag("Toml") === ".toml");

// ---- minimal ConvertRefs shim (no jsdom, matches render.spec.mjs's convention) ----
function mkSelect() {
  return { innerHTML: "", value: "", options: [] };
}
function mkRefs() {
  return {
    surface: { isOpen: () => false, open: () => {}, close: () => {}, onCancel: () => {} },
    fmt: mkSelect(),
    path: { value: "" },
    warns: { innerHTML: "", classList: { toggle: () => {} } },
    run: { textContent: "" },
    cancel: {},
  };
}
function mkSnap(docFormat) {
  return { doc_format: docFormat, mode: "Normal" };
}
function mkConvertView(overrides = {}) {
  return { step: "Format", cursor: 0, options: ["Toml", "Yaml"], target: "Json", path: "out.json", path_cursor: 8, warnings: [], ...overrides };
}

// ---- renderConvertDialog: option list carries both JSON and JSONC ----
console.log("\n-- renderConvertDialog(): JSONC option list --");
{
  const refs = mkRefs();
  const cv = mkConvertView();
  renderConvertDialog(refs, cv, mkSnap("Json"));
  check("option list includes JSON", refs.fmt.innerHTML.includes('value="Json"'));
  check("option list includes JSONC", refs.fmt.innerHTML.includes('value="Jsonc"'));
  check("JSONC option label reads JSONC", refs.fmt.innerHTML.includes(">JSONC<"));
  check("initial select value defaults to Json (not Jsonc)", refs.fmt.value === "Json", refs.fmt.value);
}

// ---- renderConvertDialog: picking a .jsonc path selects the Jsonc pseudo-tag ----
console.log("\n-- renderConvertDialog(): .jsonc path selects JSONC --");
{
  const refs = mkRefs();
  const cv = mkConvertView({ path: "out.jsonc" });
  renderConvertDialog(refs, cv, mkSnap("Json"));
  check('cv.path ending ".jsonc" selects the Jsonc option', refs.fmt.value === "Jsonc", refs.fmt.value);
}

// ---- renderConvertDialog: re-render (surface already open) must not clobber the Jsonc pick ----
console.log("\n-- renderConvertDialog(): re-render preserves JSONC pick --");
{
  const refs = mkRefs();
  const cv = mkConvertView({ path: "out.jsonc" });
  refs.surface.isOpen = () => true; // simulate an already-open dialog
  globalThis.document = { activeElement: null }; // re-render branch reads document.activeElement
  refs.fmt.value = "Jsonc"; // as if the user just picked it
  renderConvertDialog(refs, cv, mkSnap("Json"));
  check(
    "re-render with the same .jsonc path keeps Jsonc selected (not reset to Json)",
    refs.fmt.value === "Jsonc",
    refs.fmt.value,
  );
}

console.log(failures === 0 ? "\nALL CONVERT-DIALOG CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
