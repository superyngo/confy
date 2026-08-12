// Cross-platform runner for web/*.spec.mjs (npm's "test" script previously
// used a bash `for` loop, which `npm run` invokes via cmd.exe on Windows —
// "f was unexpected at this time." — failing before any spec ran).
import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const dir = fileURLToPath(new URL(".", import.meta.url));
const specs = readdirSync(dir)
  .filter((f) => f.endsWith(".spec.mjs"))
  .sort();

for (const f of specs) {
  const result = spawnSync(process.execPath, [f], { stdio: "inherit", cwd: dir });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
