#!/bin/sh
# Run the same gates as CI before committing: Rust fmt/clippy/tests, proprietary-content guard.
#   scripts/check.sh          Rust + guard
#   scripts/check.sh --app    also build the FFI library (release) and the Swift app, and check
#                             the portable shell for wasm32 (CI runs those jobs only on demand)
#   scripts/check.sh --ios    also build the iOS app for the Simulator (scripts/make-ios-app.sh)
#   (flags combine: scripts/check.sh --app --ios)
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
app=false; ios=false
for arg in "$@"; do
  case "$arg" in
    --app) app=true ;;
    --ios) ios=true ;;
    *) echo "usage: scripts/check.sh [--app] [--ios]" >&2; exit 2 ;;
  esac
done
cd "$root/core"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --quiet
cd "$root"
files=$(git ls-files -m -o --exclude-standard; git diff --cached --name-only --diff-filter=AM)
if [ -n "$files" ]; then python3 scripts/check_no_proprietary.py $files; fi
if $app; then
  (cd "$root/core" && cargo build --release -p mmk-ffi)
  rm -f "$root/app/.build/release/MMKApp"   # SwiftPM does not relink when only the .a changes
  (cd "$root/app" && swift build -c release)
  if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    (cd "$root/core" && cargo check -q -p mmk-shell --target wasm32-unknown-unknown)
  fi
fi
if $ios; then
  "$root/scripts/make-ios-app.sh"
fi
echo "check: all gates passed"
