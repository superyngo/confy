#!/usr/bin/env bash
# Cloudflare Pages build command for the confy web UI (Git-integration deploy).
# Configure in the CF Pages dashboard:
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

# 3. Build wasm core, then the web bundle.
( cd crates/confy-ffi && wasm-pack build --target web )
( cd web && npm install && node build.mjs )

# 4. Assemble a clean output dir with only the runtime files.
cd web
rm -rf dist
mkdir -p dist/touch dist/pkg dist/icons
cp index.html touch.html style.css ui.js ui.js.map manifest.webmanifest sw.js dist/
cp touch/style.css touch/app.js touch/app.js.map dist/touch/
cp icons/icon-192.png icons/icon-512.png dist/icons/
cp -r pkg/. dist/pkg/

echo "cf-build: assembled web/dist"
