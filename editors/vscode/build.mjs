// Bundle the extension host and stage the webview assets. Run this from a
// scratchpad copy of the repo — esbuild deadlocks bundling from the
// /Volumes/Home volume path (see the plan's Global Constraints).
import { cp, rm, readdir, stat } from "node:fs/promises";
import esbuild from "esbuild";

// `media/` is a straight copy of `web/dist`, which this script does NOT build.
// A checkout whose `web/dist` predates `web/*.ts` therefore stages a stale
// webview into the extension and the UI silently runs the previous bundle
// (hit 2026-09-16: a rebuilt-elsewhere copy showed pre-change Action menu
// wording). Same warn-don't-fail shape as web/build.mjs's stale-`pkg/` check.
async function newestMtime(dir, exts) {
  let newest = 0;
  for (const e of await readdir(dir, { withFileTypes: true, recursive: true })) {
    // `dist/`, `pkg/` and `node_modules/` are build *output* — comparing them
    // against themselves would make every build look fresh.
    if (/(^|\/)(dist|pkg|node_modules)(\/|$)/.test(e.parentPath)) continue;
    if (!e.isFile() || !exts.some((x) => e.name.endsWith(x))) continue;
    const { mtimeMs } = await stat(new URL(`${e.parentPath}/${e.name}`, import.meta.url));
    if (mtimeMs > newest) newest = mtimeMs;
  }
  return newest;
}

try {
  const dist = await stat(new URL("../../web/dist/ui.js", import.meta.url));
  const src = await newestMtime(new URL("../../web/", import.meta.url), [".ts", ".html", ".css"]);
  if (src > dist.mtimeMs) {
    console.warn(
      "WARNING: web/dist/ is OLDER than web/'s sources — this stages a stale\n" +
        "         webview into media/. Run:  cd web && node build.mjs",
    );
  }
} catch {
  // no web/dist yet (or unreadable) — the copy below reports the real problem
}

await esbuild.build({
  entryPoints: ["src/extension.ts"],
  outfile: "dist/extension.js",
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["vscode"],
  sourcemap: true,
});

// The webview loads the same web/dist bundle the browser and Tauri hosts use.
const MEDIA = new URL("./media/", import.meta.url);
await rm(MEDIA, { recursive: true, force: true });
await cp(new URL("../../web/dist/", import.meta.url), MEDIA, { recursive: true });

console.log("built: dist/extension.js + media/");
