// Plain-Node test for the shared toolbar/overflow-menu fold mechanism
// (`toolbar-fold.ts`) — follows the `crates/confy-ffi/functional_smoke.mjs`
// convention: no test framework, just `node:assert` + a `check()` tally, run
// directly via `node toolbar-fold.spec.mjs`.
//
// Two kinds of checks:
//   (a) pure-logic checks of `foldedEntries()` itself, compiled on the fly via
//       esbuild's `transform` API (already a devDependency; no jsdom, no new
//       npm dependency) since Node can't import `.ts` directly.
//   (b) a structural regression test — the actual guardrail against the bug
//       this mechanism exists to prevent: a toolbar button marked
//       `data-foldable="true"` in the markup (desktop `index.html` /
//       touch `touch/app.ts`'s `appHTML()`) that has no matching entry in the
//       corresponding registry (`TOOLBAR_ENTRIES` in `ui.ts`, `MENU_CANDIDATES`
//       in `touch/app.ts`), or vice versa.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
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

// ---- (a) pure logic: foldedEntries() ----
console.log("-- foldedEntries() --");

const tsSrc = readFileSync(path.join(here, "toolbar-fold.ts"), "utf8");
const { code } = await esbuild.transform(tsSrc, { loader: "ts", format: "esm" });
const modUrl = "data:text/javascript;base64," + Buffer.from(code).toString("base64");
const { foldedEntries } = await import(modUrl);

const fakeEntries = [
  { key: "a", labelKey: "l.a", run: () => {} },
  { key: "b", labelKey: "l.b", run: () => {} },
  { key: "c", labelKey: "l.c", run: () => {} },
];

{
  const folded = new Set(["a", "c"]);
  const result = foldedEntries(fakeEntries, (k) => folded.has(k));
  check(
    "folded keys returned in original order",
    result.map((e) => e.key).join(",") === "a,c",
    JSON.stringify(result.map((e) => e.key)),
  );
}
{
  const result = foldedEntries(fakeEntries, () => false);
  check("empty folded-set -> empty result", result.length === 0, JSON.stringify(result));
}
{
  const result = foldedEntries(fakeEntries, () => true);
  check(
    "all-folded -> full list in original order",
    result.map((e) => e.key).join(",") === "a,b,c",
    JSON.stringify(result.map((e) => e.key)),
  );
}
{
  // A key not present in the caller's folded-set is simply not folded — no throw.
  const folded = new Set(["a"]);
  const result = foldedEntries(fakeEntries, (k) => folded.has(k));
  check(
    "unknown/not-in-set key handled gracefully (excluded, no throw)",
    result.map((e) => e.key).join(",") === "a",
    JSON.stringify(result.map((e) => e.key)),
  );
}
{
  const result = foldedEntries([], (k) => k === "a");
  check("no entries -> empty result", result.length === 0);
}

// ---- (b) structural regression: markup <-> registry parity ----
console.log("\n-- structural: data-foldable markup vs registry --");

function extractButtonAttrs(html, attrName) {
  // Every <button ...>, checked for data-foldable="true"; extracts `attrName`.
  const out = [];
  const buttonRe = /<button\b[^>]*>/g;
  let m;
  while ((m = buttonRe.exec(html))) {
    const tag = m[0];
    if (!/data-foldable="true"/.test(tag)) continue;
    const attrRe = new RegExp(`\\b${attrName}="([^"]+)"`);
    const am = tag.match(attrRe);
    if (am) out.push(am[1]);
  }
  return out;
}

// -- desktop: index.html ids <-> ui.ts TOOLBAR_ENTRIES keys --
const indexHtml = readFileSync(path.join(here, "index.html"), "utf8");
const desktopFoldableIds = extractButtonAttrs(indexHtml, "id");
check("desktop: found foldable buttons in index.html", desktopFoldableIds.length > 0, desktopFoldableIds.length);

const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const toolbarEntriesBlock = uiTs.match(
  /const TOOLBAR_ENTRIES: ToolbarEntry\[\] = \[([\s\S]*?)\n\];/,
);
check("desktop: found TOOLBAR_ENTRIES block in ui.ts", !!toolbarEntriesBlock);
const desktopRegistryKeys = toolbarEntriesBlock
  ? [...toolbarEntriesBlock[1].matchAll(/key:\s*"([^"]+)"/g)].map((m) => m[1])
  : [];

for (const id of desktopFoldableIds) {
  check(
    `desktop: data-foldable id "${id}" has a TOOLBAR_ENTRIES entry`,
    desktopRegistryKeys.includes(id),
  );
}
for (const key of desktopRegistryKeys) {
  check(
    `desktop: TOOLBAR_ENTRIES key "${key}" is marked data-foldable in index.html`,
    desktopFoldableIds.includes(key),
  );
}

// -- touch: appHTML() data-act <-> touch/app.ts MENU_CANDIDATES keys --
const touchAppTs = readFileSync(path.join(here, "touch", "app.ts"), "utf8");
const appHtmlBlock = touchAppTs.match(/function appHTML\(\): string \{([\s\S]*?)\n\}/);
check("touch: found appHTML() block in touch/app.ts", !!appHtmlBlock);
const touchFoldableActs = appHtmlBlock ? extractButtonAttrs(appHtmlBlock[1], "data-act") : [];
check("touch: found foldable buttons in appHTML()", touchFoldableActs.length > 0, touchFoldableActs.length);

const menuCandidatesBlock = touchAppTs.match(
  /const MENU_CANDIDATES: ToolbarEntry\[\] = \[([\s\S]*?)\n\];/,
);
check("touch: found MENU_CANDIDATES block in touch/app.ts", !!menuCandidatesBlock);
// Registry keys are selectors like `[data-act="undo"]`; unwrap to the bare act value.
const touchRegistryActs = menuCandidatesBlock
  ? [...menuCandidatesBlock[1].matchAll(/key:\s*'\[data-act="([^"]+)"\]'/g)].map((m) => m[1])
  : [];

for (const act of touchFoldableActs) {
  check(
    `touch: data-foldable data-act "${act}" has a MENU_CANDIDATES entry`,
    touchRegistryActs.includes(act),
  );
}
for (const act of touchRegistryActs) {
  check(
    `touch: MENU_CANDIDATES key for data-act "${act}" is marked data-foldable in appHTML()`,
    touchFoldableActs.includes(act),
  );
}

console.log(failures === 0 ? "\nALL TOOLBAR-FOLD CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
