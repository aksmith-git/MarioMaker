#!/bin/sh
# Run the same gates as CI before committing: Rust fmt/clippy/tests, proprietary-content guard.
#   scripts/check.sh          Rust + guard
#   scripts/check.sh --app    also build the FFI library (release) and the Swift app
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root/core"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --quiet
cd "$root"
files=$(git ls-files -m -o --exclude-standard; git diff --cached --name-only --diff-filter=AM)
if [ -n "$files" ]; then python3 scripts/check_no_proprietary.py $files; fi
if [ "$1" = "--app" ]; then
  (cd "$root/core" && cargo build --release -p mmk-ffi)
  rm -f "$root/app/.build/release/MMKApp"   # SwiftPM does not relink when only the .a changes
  (cd "$root/app" && swift build -c release)
fi
echo "check: all gates passed"
