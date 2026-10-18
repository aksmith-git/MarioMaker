#!/bin/sh
# Build SMCraft for Windows x64 and pack it into dist/SMCraft-windows-x64.zip.
#   On Windows (PowerShell / cmd), use the native build instead: see docs/windows.md.
#   From macOS or Linux this cross-compiles with the MinGW-w64 toolchain:
#     macOS:  brew install mingw-w64        Linux: apt install mingw-w64 (or your distro's)
#     rustup target add x86_64-pc-windows-gnu
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
target=x86_64-pc-windows-gnu
cd "$root/core"
cargo build --release --target "$target" -p mmk-shell -p mmk-cli
out="$root/dist/SMCraft-windows-x64"
rm -rf "$out" && mkdir -p "$out"
for exe in mmk-play mmk; do
    cp "target/$target/release/$exe.exe" "$out/"
    # drop debug info (the release profile keeps it for profiling)
    if command -v x86_64-w64-mingw32-strip >/dev/null 2>&1; then
        x86_64-w64-mingw32-strip "$out/$exe.exe"
    fi
done
cp "$root/docs/windows.md" "$out/README-windows.md"
cp "$root/LICENSE" "$out/" 2>/dev/null || true
(cd "$root/dist" && rm -f SMCraft-windows-x64.zip && zip -qr SMCraft-windows-x64.zip SMCraft-windows-x64)
echo "built $root/dist/SMCraft-windows-x64.zip"
ls -la "$out"
