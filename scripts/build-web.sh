#!/bin/sh
# Build the portable shell for the web: wasm32 + wasm-bindgen JS glue + the page.
#   scripts/build-web.sh [out dir]     (default: core/target/web)
# Needs: rustup target add wasm32-unknown-unknown; wasm-bindgen-cli matching the wasm-bindgen
# version in core/Cargo.lock (cargo install wasm-bindgen-cli --version <v> --locked).
# Serve the output over HTTP (e.g. python3 -m http.server -d core/target/web) and open it.
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/core/target/web}
cd "$root/core"
cargo build --release -p mmk-shell --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  target/wasm32-unknown-unknown/release/mmk-play.wasm
cp crates/shell/web/*.html crates/shell/web/*.js "$out/"
echo "web build: $out"
