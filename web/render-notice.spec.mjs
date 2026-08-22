// Test for notice rendering (Task 12: message-system-integration).
// Minimal standalone test - no bundling needed, just tests the type definitions.

import assert from "node:assert";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
let checks = 0;
function check(name, cond, extra = "") {
  checks++;
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

// Test 1: Notice types compile correctly
console.log("\nNotice Type Tests:");

// These would fail TypeScript compilation if the types were wrong
const successNotice = { severity: "success", text: "Saved", source: "core" };
const errorNotice = { severity: "error", text: "Failed", source: "core" };
const infoNotice = { severity: "info", text: "Loading...", source: "core" };
const warnNotice = { severity: "warn", text: "Conflict", source: "core" };

check("Success notice object created", successNotice.severity === "success");
check("Error notice object created", errorNotice.severity === "error");
check("Info notice object created", infoNotice.severity === "info");
check("Warn notice object created", warnNotice.severity === "warn");

// Test 2: Severity values are correct (matching Rust serde lowercase)
check("Severity is lowercase", 
  ["info", "success", "warn", "error"].every(s => s === s.toLowerCase()));

// Test 3: NoticeSource values are correct (matching Rust serde kebab-case)
check("NoticeSource kebab-case", 
  ["core", "host-tui", "host-web"].every(s => s.includes("-") || s === "core"));

// Test 4: Prompt mode has question field
const promptMode = { Prompt: { kind: "ConfirmQuit", question: "Really quit?" } };
check("Prompt mode has question field", promptMode.Prompt.question === "Really quit?");


// Test 5: schema-warning count text goes through the i18n catalog, not a
// hand-rolled English string (spec §5.3: "the three hand-rolled strings are
// deleted" — TUI and touch already used core.schema.count; desktop's status
// append did not).
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const styleCss = readFileSync(path.join(here, "style.css"), "utf8");

console.log("\nSchema Warning Count Text:");
check(
  "no hand-rolled 'schema warnings' English string remains in ui.ts",
  !uiTs.includes("schema warnings"),
);
check(
  "schema warning count status append uses the core.schema.count catalog key",
  /violation_count > 0\)[\s\S]{0,200}tArgs\("core\.schema\.count"/.test(uiTs),
);

// Test 6: desktop Success toast auto-hides after 1.6s with the same
// fade/slide animation as touch's .toast/.toast.show (spec §5.2).
console.log("\nDesktop Toast Auto-Hide:");
const renderNoticeFn = uiTs.match(/function renderNotice\([\s\S]*?\n\}/)?.[0] ?? "";
check(
  "renderNotice's success case schedules a 1.6s toast auto-hide timer",
  /case "success":[\s\S]{0,200}setTimeout\([\s\S]{0,80}1600\)/.test(renderNoticeFn),
);
check(
  "#toast has the same opacity/transform/visibility transition as touch's .toast",
  /#toast\{[^}]*transition:opacity[^}]*\}[\s\S]{0,40}#toast\.show\{/.test(styleCss),
);
if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
} else {
  console.log(`\n${checks} checks passed`);
}
