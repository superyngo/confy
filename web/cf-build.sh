#!/usr/bin/env bash
# Cloudflare Workers Builds build command for the confy web UI (Git-integration deploy).
# Configure in the CF Workers Builds dashboard:
#   Build command:           bash web/cf-build.sh
#   Build output directory:  web/dist
# Builds the wasm core, bundles the TS, and assembles a clean runtime-only
# ./web/dist (no node_modules / *.ts / build scripts).
set -euo pipefail

cd "$(dirname "$0")/.."   # repo root

# 1. Rust toolchain (CF build image usually ships it; install if absent).
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  . "$HOME/.cargo/env"
fi

# 2. wasm-pack.
if ! command -v wasm-pack >/dev/null 2>&1; then
  curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

# 3. Build wasm core, then typecheck + test + bundle the web UI (fails fast on
#    a type error or test regression before any output is assembled).
#
#    The workspace `[profile.release]` optimizes for *speed* (opt-level = 3),
#    which is what the native TUI/desktop binaries want. The wasm bundle is the
#    one artifact where size beats speed — every visitor downloads it — so
#    size-optimize just this leg. Same env-var override idiom that
#    `.github/workflows/release.yml` uses to relax the profile for Windows.
( cd crates/confy-ffi && CARGO_PROFILE_RELEASE_OPT_LEVEL=z wasm-pack build --target web )
( cd crates/confy-ffi && node functional_smoke.mjs )
( cd web && npm ci && node build.mjs && npm run typecheck && npm test )

# 4. Assemble a clean output dir with only the runtime files.
cd web
rm -rf dist
mkdir -p dist/touch dist/pkg dist/icons
cp index.html touch.html privacy.html style.css ui.js ui.js.map manifest.webmanifest sw.js schema-sample.json dist/
cp touch/style.css touch/app.js touch/app.js.map dist/touch/
cp icons/icon-192.png icons/icon-512.png dist/icons/
cp -r pkg/. dist/pkg/

echo "cf-build: assembled web/dist"
