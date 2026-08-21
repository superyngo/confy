// Tests for Task 15: SessionSnapshot legacy status/error removal and web notice exclusivity
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

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

console.log("\n-- Task 15: Structural Invariants (no legacy status/error reads) --");

// 1. web/types.ts: SessionSnapshot interface should not define status: or error:
const typesSrc = readFileSync(path.join(here, "types.ts"), "utf8");
const snapInterfaceMatch = typesSrc.match(/export interface SessionSnapshot \{([\s\S]*?)\}/);
const snapInterfaceBody = snapInterfaceMatch ? snapInterfaceMatch[1] : "";
check("web/types.ts SessionSnapshot has no status field", !/^\s*status\s*:/m.test(snapInterfaceBody));
check("web/types.ts SessionSnapshot has no error field", !/^\s*error\s*:/m.test(snapInterfaceBody));

// 2. web/ui.ts: No reads of snap.status, snap.error, snapshot.status, snapshot.error
const uiSrc = readFileSync(path.join(here, "ui.ts"), "utf8");
const uiHits = uiSrc.match(/\b(snap|snapshot)\.(status|error)\b/g) || [];
check("web/ui.ts has no snap.status / snap.error reads", uiHits.length === 0, `found: ${uiHits.join(", ")}`);

// 3. web/touch/app.ts: No reads of snap.status, snap.error, snapshot.status, snapshot.error
const touchSrc = readFileSync(path.join(here, "touch/app.ts"), "utf8");
const touchHits = touchSrc.match(/\b(snap|snapshot|after)\.(status|error)\b/g) || [];
check("web/touch/app.ts has no snap.status / snap.error / after.error reads", touchHits.length === 0, `found: ${touchHits.join(", ")}`);

// 4. web/panel.ts: No reads of snap.error, out.error
const panelSrc = readFileSync(path.join(here, "panel.ts"), "utf8");
const panelHits = panelSrc.match(/\b(snap|out|snapshot)\.(status|error)\b/g) || [];
check("web/panel.ts has no snap.error / out.error reads", panelHits.length === 0, `found: ${panelHits.join(", ")}`);

// 5. crates/confy-ffi/functional_smoke.mjs: No reads of .status / .error on snapshot objects
const smokeSrc = readFileSync(path.join(here, "..", "crates/confy-ffi/functional_smoke.mjs"), "utf8");
const smokeHits = smokeSrc.match(/\b(snap\w*|saved\w*|snb)\.(status|error)\b/g) || [];
check("crates/confy-ffi/functional_smoke.mjs has no snap.status / snap.error reads", smokeHits.length === 0, `found: ${smokeHits.join(", ")}`);

process.exit(failures === 0 ? 0 : 1);
