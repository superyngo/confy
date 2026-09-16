// Guard spec: no web/ source may call a native modal (`confirm`/`alert`/
// `prompt`).
//
// Why this is a test and not a comment: VS Code builds its webview iframe with
// `sandbox="allow-scripts allow-same-origin allow-forms allow-pointer-lock
// allow-downloads"` — no `allow-modals` — so Chromium resolves `confirm()` to
// `false` without ever showing a dialog (measured 2026-09-16 against those
// exact flags, read out of VS Code 1.135's own bundle). A native confirm
// therefore reads as "the user said no" forever in that host: Raw write mode's
// Escape and Tree/Raw toggle both became silent no-ops that way. The in-page
// `#confirm-modal` (`askConfirm` in ui.ts) is the one path all three hosts
// share; this spec keeps the native call from creeping back.
import path from "node:path";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";

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

// Every .ts under web/ (desktop + touch), excluding build output.
function tsFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    if (name === "dist" || name === "pkg" || name === "node_modules") continue;
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) out.push(...tsFiles(full));
    else if (name.endsWith(".ts")) out.push(full);
  }
  return out;
}

console.log("\n-- no native modal dialogs in web/ sources --");
const files = tsFiles(here);
check("found web/*.ts sources to scan", files.length > 0, `(${files.length})`);

// `window.`-qualified or bare, and the `.confirm(` member form.
const BANNED = /(^|[^.\w$"'`])(?:window\s*\.\s*)?(confirm|alert|prompt)\s*\(/;
for (const f of files) {
  const rel = path.relative(here, f);
  const hits = readFileSync(f, "utf8")
    .split("\n")
    .map((line, i) => [i + 1, line])
    // Comments may name them (this is a documented constraint, see ui.ts).
    .filter(([, line]) => !/^\s*(\/\/|\*|\/\*)/.test(line) && BANNED.test(line));
  check(`${rel} calls no native modal`, hits.length === 0, hits.map(([n, l]) => `\n      ${n}: ${l.trim()}`).join(""));
}

// The replacement must actually exist, or the rule above is vacuous.
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
check("ui.ts defines the in-page askConfirm dialog", /function askConfirm\(/.test(uiTs));
check("exitRawWrite awaits it instead of a native confirm", /await askConfirm\(/.test(uiTs));
const html = readFileSync(path.join(here, "index.html"), "utf8");
for (const id of ["confirm-modal", "confirmMsg", "confirmOk", "confirmCancel"]) {
  check(`index.html carries #${id}`, html.includes(`id="${id}"`));
}

console.log(failures === 0 ? "\nALL NO-NATIVE-MODAL CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
