// Build the web UI: copy the wasm-pack output into ./pkg (so web/ is
// self-contained and the dev server can serve it), then bundle the TS.
import { cp, rm, mkdir, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import esbuild from "esbuild";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const SRC_PKG = new URL("../crates/confy-ffi/pkg/", import.meta.url);
const DST_PKG = new URL("./pkg/", import.meta.url);

await rm(DST_PKG, { recursive: true, force: true });
await mkdir(DST_PKG, { recursive: true });
await cp(SRC_PKG, DST_PKG, { recursive: true });

// Stamp the workspace version into the bundle so the built-in sample's
// `about.version` tracks the real release rather than a hardcoded literal.
const cargoToml = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
const version = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? "dev";

const esbuildOpts = {
  bundle: true,
  format: "esm",
  target: "es2022",
  // esbuild does NOT minify by default, not even in bundle mode — the shipped
  // ui.js was going out as ~5200 lines of indented, commented source. The
  // sourcemap keeps it debuggable.
  minify: true,
  sourcemap: true,
  define: { __APP_VERSION__: JSON.stringify(version) },
};

// Desktop UI bundle.
await esbuild.build({ ...esbuildOpts, entryPoints: ["ui.ts"], outfile: "ui.js" });

// Dedicated touch UI bundle (see WEBUI.md § Touch UI).
await esbuild.build({ ...esbuildOpts, entryPoints: ["touch/app.ts"], outfile: "touch/app.js" });

// The VS Code extension consumes web/dist verbatim. Rebuild the runtime-only
// dist bundle from the fresh pkg/ output so the extension copies the current
// wasm/JS artifacts instead of an accidentally stale previous build.
await import(new URL("./assemble-dist.mjs", import.meta.url).href);

console.log("built: ui.js + touch/app.js + pkg/ + dist/");
