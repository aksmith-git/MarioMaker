#!/bin/sh
# Build the Rust core and run the macOS app.
#   scripts/run-app.sh                 play (keyboard: arrows/WASD, Z/Space jump, X/Shift run)
#   MMK_ASSETS=<cache dir> ...         use a specific asset cache (default: Application Support or ~/smm-work/cache)
#   MMK_SCREENSHOT=out.png ...         scripted run, save a frame and quit (smoke test)
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
(cd "$root/core" && cargo build --release -p mmk-ffi)
cd "$root/app" && swift build -c release && exec .build/release/MMKApp
