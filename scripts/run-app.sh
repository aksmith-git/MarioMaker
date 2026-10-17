#!/bin/sh
# Build the Rust core and run the macOS app (unbundled; scripts/make-app.sh builds the .app).
#   scripts/run-app.sh                 open the app: import screen on first launch, then the course library
#   MMK_COURSE=<file> ...              skip the menus and play this course (.course.zlib, raw data or .bcd)
#   MMK_SCREENSHOT=out.png ...         scripted run, save a frame and quit (smoke test; also skips the menus)
#   MMK_ASSETS=<cache dir> ...         use a specific asset cache; MMK_ASSETS=placeholder for no game data
#   MMK_SUPPORT_DIR=<dir> ...          use another data folder instead of ~/Library/Application Support/MMK
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
(cd "$root/core" && cargo build --release -p mmk-ffi)
# SwiftPM doesn't relink when only the static library changed
rm -f "$root/app/.build/release/MMKApp"
cd "$root/app" && swift build -c release && exec .build/release/MMKApp
