// Build the web UI: copy the wasm-pack output into ./pkg (so web/ is
// self-contained and the dev server can serve it), then bundle the TS.
import { cp, rm, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import esbuild from "esbuild";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const SRC_PKG = new URL("../crates/confy-ffi/pkg/", import.meta.url);
const DST_PKG = new URL("./pkg/", import.meta.url);

await rm(DST_PKG, { recursive: true, force: true });
await mkdir(DST_PKG, { recursive: true });
await cp(SRC_PKG, DST_PKG, { recursive: true });

await esbuild.build({
  entryPoints: ["ui.ts"],
  bundle: true,
  outfile: "ui.js",
  format: "esm",
  target: "es2022",
  sourcemap: true,
});

console.log("built: ui.js + pkg/");
