// Task 13 touch toast() migration tests. Structural checks against the real
// source (same pattern as touch-modal-lock.spec.mjs — app.ts isn't easily
// unit-testable in isolation without a full DOM/wasm harness, so these assert
// against the actual TypeScript text rather than executing it).
//
// Verifies:
// 1. Zero raw toast(authoredText) call sites remain anywhere in touch/app.ts.
// 2. Every clipboard-guard block that dispatches SetHostNotice on the armed
//    path still reaches its real unblocked-path action after the guard (the
//    exact regression class found in review: 8 sites had their unblocked
//    action silently deleted during migration).
// 3. Every "core.clipboard.action-locked" guard block dispatches SetHostNotice
//    (no site silently no-ops with neither a core Intent nor a host notice).
// 4. Every web.host.* key referenced in touch/app.ts exists in both i18n
//    catalogs with matching keys (parity).
// 5. toast() was converted to a severity-driven renderNotice(notice) function
//    wired into the render loop.

import path from "node:path";
import { readFileSync } from "node:fs";
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

const appTs = readFileSync(path.join(here, "touch/app.ts"), "utf8");
const enCatalog = JSON.parse(readFileSync(path.join(here, "../i18n/en.json"), "utf8"));
const zhCatalog = JSON.parse(readFileSync(path.join(here, "../i18n/zh-TW.json"), "utf8"));

console.log("-- 1. Zero raw toast(authoredText) call sites remain --");
{
  // The only `toast` identifiers left should be: the toastT timer variable,
  // the toastEl DOM reference, and renderNotice's own internals — never a
  // call site passing literal or interpolated author text to a toast-shaped
  // function.
  const rawCallSites = [...appTs.matchAll(/\btoast\(\s*(t\(|"|`)/g)];
  check(
    "no toast(\"...\") / toast(t(...)) / toast(`...`) call sites remain",
    rawCallSites.length === 0,
    JSON.stringify(rawCallSites.map((m) => m[0])),
  );
}

console.log("\n-- 2. Every SetHostNotice-guarded clipboard-lock site still reaches its unblocked action --");
{
  // Extract every `if ((snap?.clipboard_count ?? 0) > 0) { ... SetHostNotice ...
  // return; }` guard block, then confirm there is a non-`break`/non-`}` real
  // statement immediately after the guard's closing brace, up to the next
  // `case`/closing brace of the enclosing function — i.e. the guard doesn't
  // swallow the entire remaining body of its case/branch.
  const guardPattern =
    /if \(\(snap\??\.clipboard_count \?\? 0\) > 0\) \{\s*send\(\{ SetHostNotice: \{ key: "core\.clipboard\.action-locked"[^}]*\}\s*\}\);\s*return;\s*\}\n(\s*)([^\n]+)/g;
  const matches = [...appTs.matchAll(guardPattern)];
  check("at least 8 SetHostNotice clipboard guards found", matches.length >= 8, `found ${matches.length}`);
  for (const m of matches) {
    const nextLine = m[2].trim();
    const isBareBreak = nextLine === "break;";
    const isCloseBrace = nextLine === "}";
    check(
      `guard followed by a real statement, not bare break/closing-brace: "${nextLine.slice(0, 40)}"`,
      !isBareBreak && !isCloseBrace,
      `full match tail: ${JSON.stringify(m[0].slice(-120))}`,
    );
  }
}

console.log("\n-- 3. No clipboard-armed guard silently no-ops (neither SetHostNotice nor a reachable core Intent) --");
{
  // Every `if ((snap?.clipboard_count ?? 0) > 0) { ... return; }` guard block
  // (or `if (... > 0) { ...; return; }` for the searchInput variant) must
  // contain SetHostNotice somewhere in its body.
  const allGuards = [...appTs.matchAll(/if \(\(snap\??\.clipboard_count \?\? 0\) > 0\) \{([\s\S]*?)\n(\s*)\}/g)];
  check("at least 8 clipboard_count guard blocks found", allGuards.length >= 8, `found ${allGuards.length}`);
  for (const g of allGuards) {
    const body = g[1];
    check(
      `guard body dispatches SetHostNotice (or a core Intent) — body: ${JSON.stringify(body.trim().slice(0, 60))}`,
      /SetHostNotice/.test(body) || /send\(/.test(body),
    );
  }
}

console.log("\n-- 4. Every web.host.* key referenced in touch/app.ts exists in both catalogs --");
{
  const keys = new Set([...appTs.matchAll(/"web\.host\.[a-zA-Z0-9_.-]+"/g)].map((m) => m[0].slice(1, -1)));
  check("at least one web.host.* key found", keys.size > 0, `found ${keys.size}`);
  for (const key of keys) {
    check(`"${key}" present in i18n/en.json`, Object.prototype.hasOwnProperty.call(enCatalog, key));
    check(`"${key}" present in i18n/zh-TW.json`, Object.prototype.hasOwnProperty.call(zhCatalog, key));
  }
}

console.log("\n-- 5. toast() converted to a severity-driven renderNotice(notice) function --");
{
  check(
    "renderNotice(notice: Notice | undefined) function exists",
    /function renderNotice\(notice: Notice \| undefined\)/.test(appTs),
  );
  check(
    "renderNotice is wired into the render loop via snap.notice",
    /renderNotice\(snap\.notice\)/.test(appTs),
  );
  check(
    "renderNotice branches on notice.severity",
    /notice\.severity/.test(appTs),
  );
}

if (failures > 0) {
  console.error(`\n${failures} test(s) failed`);
  process.exit(1);
}

console.log("\nALL TOAST MIGRATION CHECKS PASSED");
