// Build the web UI: copy the wasm-pack output into ./pkg (so web/ is
// self-contained and the dev server can serve it), then bundle the TS.
//
// NOTE: this script does NOT run wasm-pack — it only *copies* whatever
// `crates/confy-ffi/pkg/` already holds. A core (Rust) fix therefore does not
// reach the browser until `wasm-pack build --target web` is re-run in
// `crates/confy-ffi`, which silently shipped a stale core once. The staleness
// check below warns instead of failing, so a TS-only rebuild still works
// without a Rust toolchain.
import { cp, rm, mkdir, readFile, stat, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import esbuild from "esbuild";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const SRC_PKG = new URL("../crates/confy-ffi/pkg/", import.meta.url);
const DST_PKG = new URL("./pkg/", import.meta.url);

async function newestMtime(dir) {
  let newest = 0;
  for (const e of await readdir(dir, { withFileTypes: true, recursive: true })) {
    if (!e.isFile() || !e.name.endsWith(".rs")) continue;
    const { mtimeMs } = await stat(new URL(`${e.parentPath}/${e.name}`, import.meta.url));
    if (mtimeMs > newest) newest = mtimeMs;
  }
  return newest;
}

try {
  const wasm = await stat(new URL("confy_ffi_bg.wasm", SRC_PKG));
  const src = Math.max(
    await newestMtime(new URL("../crates/confy-core/src/", import.meta.url)),
    await newestMtime(new URL("../crates/confy-ffi/src/", import.meta.url)),
  );
  if (src > wasm.mtimeMs) {
    console.warn(
      "WARNING: crates/confy-ffi/pkg/ is OLDER than the Rust sources — this build\n" +
        "         ships a stale core. Run:  cd crates/confy-ffi && wasm-pack build --target web",
    );
  }
} catch {
  // no pkg/ yet (or unreadable) — the copy below reports the real problem
}

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
