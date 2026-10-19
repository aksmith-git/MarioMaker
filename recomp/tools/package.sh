#!/bin/bash
# usage: tools/package.sh <target> <build dir> <game id> <game hle list> [deps dir]   (make package TARGET=... does this)
# A player package for windows-x86_64, linux-x86_64 or linux-aarch64: dist/<GAME_TITLE>-<target>.zip with the runtime,
# what building the game code on the player's machine needs, and nothing from the game. On first launch the runtime
# recompiles the player's own exefs (runtime/main.c first_launch_build -> runtime/gamebuild.c): the generator
# (tools/*.py, run by a Python venv the player makes once: setup.bat / setup.sh), then the bundled compiler (zig/).
#   <game id>recomp[.exe]       the runtime
#   <game id>recomp.dll.a       (Windows) its import library: the game code DLL links against the exe
#   *.dll                       (Windows) SDL3 + libiconv, shaderc (Vulkan itself comes with the graphics driver)
#   zig/                        the compiler, for this target (tools/fetch_zig.sh)
#   gamelib-src/                the headers the game code is compiled against, its entry file, the HLE list
#   tools/                      the generator (Python) and the game's setup scripts (dump layout, system data)
#   setup.bat | setup.sh        makes tools/venv (capstone, unicorn, lz4)
#   README.txt                  setup and first launch, short; the full text is docs/windows.md / recomp/README.md
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=$1 B=$2 ID=$3 HLE=$4 DEPS=${5:-}
OS=${TARGET%%-*}
EXE=$B/${ID}recomp$([ "$OS" = windows ] && echo .exe || true)
[ -f "$EXE" ] || { echo "no $EXE: make TARGET=$TARGET all first"; exit 1; }
NAME="SMM2Recomp-$TARGET"
OUT=dist/$NAME
rm -rf "$OUT" "dist/$NAME.zip"
mkdir -p "$OUT/gamelib-src" "$OUT/tools/games/smm2"
cp "$EXE" "$OUT/"
if [ "$OS" = windows ]; then
    cp "$B/${ID}recomp.dll.a" "$OUT/"
    for d in SDL3.dll libiconv-2.dll libshaderc_shared.dll; do cp "$DEPS/bin/$d" "$OUT/"; done
fi
# the compiler for the player's machine
ZT=$([ "$TARGET" = windows-x86_64 ] && echo x86_64-windows || echo "${TARGET#*-}-linux")
[ -d "build/zig-$ZT" ] || ZIG_TARGET=$ZT ZIG_OUT="build/zig-$ZT" tools/fetch_zig.sh
cp -R "build/zig-$ZT" "$OUT/zig"
# what the game code is compiled against (the same files GAMELIB_ID hashes)
cp runtime/cpu.h runtime/simd.h runtime/simd_fp.h runtime/gamelib.h runtime/game.h tools/gamelib_entry.c "$OUT/gamelib-src/"
cp "$HLE" "$OUT/gamelib-src/hle_symbols.txt"
cp games/smm2/patches.py "$OUT/gamelib-src/patches.py"  # (the generator's per-instruction patches for main)
mkdir -p "$OUT/gamelib-src/mods" && cp games/smm2/mods/patches.py "$OUT/gamelib-src/mods/patches.py"  # (mods' hook points)
# the generator and the game's setup tools
cp tools/gen.py tools/lifter.py tools/analyze.py tools/nso.py tools/passes.py tools/patches.py tools/simd_c.py "$OUT/tools/"
cp games/smm2/setup.py games/smm2/sysdata.py "$OUT/tools/games/smm2/"
cp LICENSE-LASRecomp "$OUT/" 2>/dev/null || true
cp third_party/imgui/LICENSE.txt "$OUT/LICENSE-imgui.txt" 2>/dev/null || true
{
    echo "Third-party components in this package and their licenses:"
    echo "- LAS Recomp (the recompiler and runtime this is forked from): MIT, LICENSE-LASRecomp"
    echo "- Dear ImGui: MIT, LICENSE-imgui.txt"
    echo "- zig (zig/): MIT, zig/LICENSE; its clang / lld: Apache-2.0 with LLVM exception"
    echo "- SMAA (built in): MIT-style (third_party/smaa in the source)"
    if [ "$OS" = windows ]; then
        echo "- SDL3 (SDL3.dll): zlib license"
        echo "- shaderc, glslang, SPIRV-Tools (libshaderc_shared.dll): Apache-2.0 / BSD-3-Clause"
        echo "- libiconv (libiconv-2.dll, used by SDL3): LGPL-2.1 (unmodified MSYS2 build, replaceable)"
    else
        echo "- SDL3, the Vulkan loader and shaderc come from your distribution (not included)"
    fi
} > "$OUT/THIRD-PARTY.txt"
if [ "$OS" = windows ]; then
    printf '%s\r\n' '@echo off' \
        'rem Makes tools\venv: the Python the game code generator runs with (once; needs Python 3.12+ from python.org)' \
        'cd /d "%~dp0"' \
        'py -3 -m venv tools\venv || python -m venv tools\venv || goto :fail' \
        'tools\venv\Scripts\python -m pip install capstone unicorn lz4 || goto :fail' \
        'echo Done. Now put your game files in "extracted" (see README.txt) and start smm2recomp.exe.' \
        'pause' 'exit /b 0' ':fail' 'echo Setup failed: is Python 3.12 or newer installed (python.org, "Add to PATH")?' 'pause' 'exit /b 1' \
        > "$OUT/setup.bat"
else
    printf '%s\n' '#!/bin/sh' '# Makes tools/venv: the Python the game code generator runs with (once; needs python3 with venv)' \
        'set -e' 'cd "$(dirname "$0")"' 'python3 -m venv tools/venv' 'tools/venv/bin/python -m pip install capstone unicorn lz4' \
        'echo "Done. Now put your game files in extracted/ (see README.txt) and start ./smm2recomp."' > "$OUT/setup.sh"
    chmod +x "$OUT/setup.sh"
fi
if [ "$OS" = windows ]; then X='smm2recomp.exe' S='setup.bat' P='tools\venv\Scripts\python' D='\'; else X='./smm2recomp' S='./setup.sh' P='tools/venv/bin/python' D='/'; fi
cat > "$OUT/README.txt" <<EOF
SMM2 Recomp ($TARGET) - Super Mario Maker 2 v3.0.3 recompiled from YOUR OWN dump. No game files are included.

1. Run $S once (makes tools${D}venv for the game code generator; needs Python 3.12+).
2. Lay out your own extracted dump (base game RomFS, v3.0.3 update ExeFS + RomFS) in extracted${D}:
     $P tools${D}games${D}smm2${D}setup.py --base-romfs <base romfs> --update-exefs <update exefs> --update-romfs <update romfs> --out extracted
3. The console's shared fonts and system data, from your own firmware dump and keys (needs hactool):
     $P tools${D}games${D}smm2${D}sysdata.py --firmware <registered NCA folder> --keys <prod.keys> --data .
   Without fonts${D} the game stops at the title screen (it needs the Switch's button-glyph font).
4. Start $X. The first launch recompiles the game code from extracted${D}exefs (a few minutes to half an hour;
   progress in the console / terminal); later launches start straight away.

Keys: WASD stick, arrows d-pad, K/J/L/I = A/B/X/Y, Q/E = L/R, Enter = +, F1 menu. Full instructions and troubleshooting:
docs/windows.md (Windows) and recomp/README.md (Linux) in the source repository.
EOF
if [ "$OS" = windows ]; then for f in README.txt THIRD-PARTY.txt; do sed -i.bak 's/$/\r/' "$OUT/$f" && rm -f "$OUT/$f.bak"; done; fi
(cd dist && rm -f "$NAME.zip" && zip -qr -9 "$NAME.zip" "$NAME")
echo "dist/$NAME.zip: $(du -h "dist/$NAME.zip" | cut -f1) ($(du -sh "$OUT" | cut -f1) unpacked)"
