// Bundle the extension host and stage the webview assets. Run this from a
// scratchpad copy of the repo — esbuild deadlocks bundling from the
// /Volumes/Home volume path (see the plan's Global Constraints).
import { cp, rm } from "node:fs/promises";
import esbuild from "esbuild";

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
