# SMM2 Recomp

A native port of Super Mario Maker 2 **v3.0.3** built by **static recompilation** of the player's own
dump: the game's AArch64 code is translated to C ahead of time and compiled for the host, its
system calls run on a native runtime, and its NVN graphics run on Metal (macOS, iOS) or Vulkan
(Windows, Linux). SMM1's 100 Mario Challenge and Mystery Mushroom costumes come as mods.

This is the repository's main project (SMCraft, the earlier clean-room engine, is archived on the
`smcraft-archive` branch); `../core` holds the open-source course server its mods talk to. The approach and the projects it builds on are in `../DECISIONS.md` ("Native
SMM2 port by static recompilation"); status, with evidence per platform, is in `../PROGRESS.md`
("Recomp"). It is a fork of [LAS Recomp](https://github.com/peiikai/LASRecomp) (MIT) — what was taken,
left out and changed is in [UPSTREAM.md](UPSTREAM.md).

**Nothing from the game is in this repository.** You supply your own dump; the recompiled code,
translated shaders and extracted data are generated on your machine into `private/`, `generated/`
and `build/`, which git ignores and the repo's proprietary-content check blocks.

## Layout

| Path | What |
|---|---|
| `tools/` | the recompiler: `gen.py` (a whole NSO module -> C), `lifter.py`, `simd_c.py`, `nso.py`, tests |
| `runtime/` | the shared runtime: loader, dispatcher, kernel, IPC + services, NVN on Metal / Vulkan, platform layers |
| `games/smm2/` | the game's glue: `game.mk` (identity, dump paths), `setup.py` (lays out the dump), patches, HLE list |
| `third_party/` | Dear ImGui (overlay menu), SMAA |
| `docs/` | findings from the bring-up (names and behaviour only, no game code) |
| `private/`, `generated/`, `build/` | made from your dump (ignored; may be symlinks to another disk) |

## Building (macOS, Apple Silicon)

Requirements: Xcode command line tools (clang), Homebrew `sdl3`, Python 3 (3.12+). About 0.7 GB of disk for the
generated C and 1 GB for the build. (FFmpeg is only for games whose code plays movies; SMM2's has none:
`GAME_MOVIES := 0` in `games/smm2/game.mk`.)

You need your own extracted dump: the base game's RomFS and the v3.0.3 update's ExeFS (`main`, `sdk`, `rtld`,
`main.npdm`) and RomFS. By default `make` looks in `~/smm-work/extracted/` (`smm2-romfs`, `smm2-v303-exefs`,
`smm2-v303-romfs`); pass `DUMP_BASE_ROMFS=`, `DUMP_UPDATE_EXEFS=`, `DUMP_UPDATE_ROMFS=` for other places.

    make -C recomp venv        # once: tools/venv with capstone, unicorn, lz4
    make -C recomp smm2        # setup (checks the v3.0.3 build ids, links the RomFS), recompile, build
    make -C recomp run-smm2    # play: runs build/smm2/smm2recomp from private/smm2 (saves, settings, caches there)

`make smm2` runs three steps you can also run alone (`make -C recomp GAME=smm2 setup | gen | all`): `setup` lays
the files out in `private/smm2/extracted/` (exefs copied, romfs as links: the update's files over the base
game's, nothing duplicated), `gen` recompiles `sdk` and `main` to C in `generated/smm2/` (about a minute), and
`all` builds the runtime (`build/smm2/smm2recomp`) and the game code library (`build/smm2/smm2game.dylib`, about
1,000 C files: 10-20 minutes the first time).

`make -C recomp GAME=smm2 runtime-objs` compiles the runtime alone (no game files needed).

**System data** (all platforms): the Switch's shared fonts and a few system data archives (Mii model, word filter,
time zones), from your own firmware dump and keys, read in place by `hactool`:

    tools/venv/bin/python games/smm2/sysdata.py --firmware <registered NCA folder> --keys <prod.keys> --data private/smm2

Without the fonts (`private/smm2/fonts/`) the game stops at the title screen: its text engine needs the console's
button-glyph font (a desktop font such as DejaVu Sans or Liberation Sans is no substitute there).

## Building (Linux)

Linux runs on Vulkan: x86-64 (with AVX2: Intel Haswell / AMD Zen or newer) or arm64, a Vulkan 1.3 driver (Mesa
RADV / ANV / NVK, NVIDIA's; Mesa's software lavapipe works, slowly), SDL3 3.2 or newer, the Vulkan loader and shaderc.
The game code is portable C: on arm64 it is the same generated code as macOS (`generated/smm2`, FP/SIMD as inline
asm), on x86-64 the portable-C lowering (`generated/smm2-c`, `--simd c`).

Status (2026-10-19), from the Linux evidence:

- **Builds:** linux-aarch64 and linux-x86_64 are cross-built from macOS with zig.
- **Boots to the title screen:** linux-aarch64 does, in a Debian 13 VM on Mesa lavapipe (software Vulkan), at about
  7 fps.
- **Matches the Metal renderer:** the same frame (seed 4, frame 2386) differs from macOS's in 0.6 % of pixels by more
  than 24 and 0.17 % by more than 96. The difference is the Mario model, which the Vulkan path doesn't show yet.
  Everything else in the frame matches.
- **Package first launch works:** the player package's first-launch recompile built the game code from the dump in
  29 minutes, then booted to the same frames.
- **Not tested:** x86-64 hasn't been run. Neither has a hardware GPU.

**On Linux, from source** (as on macOS; Debian 13 / Ubuntu 25.04 package names):

    sudo apt install clang lld make python3-venv libsdl3-dev libvulkan-dev libshaderc-dev \
                     libvulkan-memory-allocator-dev mesa-vulkan-drivers
    make -C recomp venv smm2       # setup, recompile (x86-64: --simd c), build build/smm2/smm2recomp + smm2game.so
    make -C recomp run-smm2

**Cross-built from macOS** (or any host) with zig, the compiler the player packages bring along:

    recomp/tools/fetch_zig.sh                   # build/zig (or ZIG=<zig>)
    make -C recomp TARGET=linux-aarch64 GAME=smm2 DEPS=<deps> all            # generated/smm2 (as macOS)
    make -C recomp TARGET=linux-x86_64  GAME=smm2 DEPS=<deps> gen all        # generated/smm2-c
    make -C recomp TARGET=linux-aarch64 GAME=smm2 package                    # dist/SMM2Recomp-linux-aarch64.zip

`DEPS` is a folder with `include/` (SDL3, vulkan, vk_video, shaderc, `vk_mem_alloc.h`) and `lib/` (`libSDL3.so`,
`libvulkan.so`, `libshaderc.so`) for the target, e.g. copied from a Debian system's `/usr/include` and
`/usr/lib/<arch>-linux-gnu` (amd64 packages unpacked with `dpkg-deb -x` for x86-64). Outputs go to
`build/smm2-<target>/`; the executable looks for its libraries in the system and in `lib/` next to it.

**The player package** (`make ... package`): the runtime, zig, the generator (Python) and the headers - nothing from
the game. Unzip it, then:

    ./setup.sh                                  # once: tools/venv (capstone, unicorn, lz4) for the generator
    tools/venv/bin/python tools/games/smm2/setup.py --base-romfs <base romfs> --update-exefs <update exefs> \
        --update-romfs <update romfs> --out extracted
    tools/venv/bin/python tools/games/smm2/sysdata.py --firmware <registered NCA folder> --keys <prod.keys> --data .
    ./smm2recomp                                # first launch: recompiles extracted/exefs into extracted/smm2game.so

The folder with `extracted/` is the data folder (saves, settings, shader caches); without one the runtime uses
`~/.local/share/smm2recomp`. Audio needs a running PulseAudio / PipeWire / ALSA device (without one the game runs
silent).

## Building (iOS / iPadOS)

iOS runs no code it didn't ship with, and there is no compiler on the device, so the game code is built on the Mac
from your own dump and linked into the app. The app is then signed and installed on your device. The app is made from your dump: it is
for your own devices only, never to be shared. Metal renderer as on macOS (shaders still translated and compiled at
run time: `newLibraryWithSource` is allowed), SDL3 built for iOS as a static library, the same arm64 game code as
macOS (`generated/smm2`).

    # SDL3 for iOS, once: cmake -S SDL3-<ver> -B b -G Ninja -DCMAKE_SYSTEM_NAME=iOS -DCMAKE_OSX_SYSROOT=iphoneos \
    #   -DCMAKE_OSX_ARCHITECTURES=arm64 -DSDL_SHARED=OFF -DSDL_STATIC=ON -DCMAKE_INSTALL_PREFIX=<deps>/ios-device
    #   (and iphonesimulator -> <deps>/ios-sim); then cmake --build b && cmake --install b
    make -C recomp TARGET=ios-arm64 GAME=smm2 DEPS=<deps>/ios-device app        # build/smm2-ios-arm64/SMM2Recomp.app
    make -C recomp TARGET=iossim-arm64 GAME=smm2 DEPS=<deps>/ios-sim app        # the Simulator (ad-hoc signed)

Signing: `RECOMP_IOS_IDENTITY=<codesigning identity> RECOMP_IOS_PROFILE=<.mobileprovision>` (and
`RECOMP_IOS_BUNDLE_ID`, default `org.smcraft.smm2recomp`). The guest's memory window sits at 0x7000000000-0x7320000000,
far above iOS's default address space, and the game uses 3+ GB: the profile must carry
`com.apple.developer.kernel.extended-virtual-addressing` and `com.apple.developer.kernel.increased-memory-limit`.
That means an explicit App ID with both capabilities. A wildcard profile lacks them, and the app then stops at start
with a message. `tools/ios_profile.sh <bundle id> <team id>` has Xcode make that App ID and profile (automatic
signing of an empty stub app; needs the team's Apple ID in Xcode > Settings > Accounts, and xcodegen) and prints the
profile's path.

The window fits on iOS 27 (iPad Pro M5): with the entitlement the app's address space reaches 0x8000000000, and iOS
keeps 0x2ee000000-0x7000000000 for itself, so 0x7000000000 is the first free place. Where that differs, the window can
move: `make GUEST_ORIGIN=<base> ...` generates the game code for that base (`generated/smm2-<base>`) and builds the
runtime with it (runtime/rt.h; a game's patch addresses move with it, tools/patches.py). If the reservation fails, the
log lists the address space (kernel limits, what is mapped, which reservations would work). Install over Tailscale with `~/.claude/tools/ios-install-tailscale/ios-install-tailscale <app> --device ipad`,
or locally with Xcode / `xcrun devicectl device install app`.

Game files go in the app's **Documents** folder (Finder's file sharing for the device, or the Files app):
`extracted/` (exefs + romfs, laid out by `games/smm2/setup.py` on the Mac), `fonts/` and `sysdata/` (sysdata.py), and
optionally a `save/` to carry over. The log is `Documents/smm2recomp.log`. Or `RECOMP_IOS_DATA=private/smm2` puts
them inside the app (data/, linked into Documents at every launch): nothing to copy, but a 3 GB app to install.
- **Simulator:** copy them into the app's container, `xcrun simctl get_app_container <device> org.smcraft.smm2recomp data`.
  macOS privacy blocks links from there into `~/Documents` or external disks.

Input:
- An on-screen controller on touch-only devices (runtime/ui/touchpad.mm): left stick, A/B/X/Y, L/R/ZL/ZR, Plus /
  Minus, and a menu button for the overlay menu. Fingers that land on it are its own; every other touch goes to the
  touch screen. The button in the top left corner hides / shows it, and it hides while a controller is connected.
- Touches are the console's touch screen (`nn::hid::GetTouchScreenState`).
- Game controllers go through SDL (GameController), and an attached keyboard as on the desktop.
- On the desktop, the left mouse button is a finger on the touch screen (`RECOMP_NO_MOUSE_TOUCH=1` turns it off).
- A two-finger tap is L + R, which is what the title screen asks for when no controller is attached. The menus after
  it take touches themselves.
- [obs] macOS: taps in the Course Maker (`LARECOMP_HOST_INPUT` tap / click) dismiss its dialogs and place blocks.

GPUs without BC textures (the Simulator, older iPhones) get BC1-7 decoded on the CPU at upload (`runtime/nvn/bcsoft.c`,
bcdec: MIT).

The Simulator's Metal is more limited than the devices':
- It allows at most 32 bytes of colour targets per pass and has no cube arrays.
- There, colour targets past that budget are dropped and cube-array lookups read zero, so its frames show wrong
  lighting.
- iPads with M-series chips have neither limit.

## Running

Keyboard (LAS Recomp's defaults, remappable in the overlay's Controls tab): WASD left stick, arrows d-pad,
K/J/L/I = A/B/X/Y, Q/E = L/R, 1/3 = ZL/ZR, Enter / Backspace = Plus / Minus. F1 or Esc opens the overlay menu.
On the title screen the course is playable straight away; L + R (Q + E) opens the Make / Play menu.

Useful environment variables (inherited from LAS Recomp): `LARECOMP_WINDOW_SHOT=<frame>` saves the window at
that frame (`LARECOMP_SHOT_PATH=<png>`, `LARECOMP_EXIT_AFTER_SHOT=1`), `LARECOMP_SCREENSHOT_EVERY=<n>`,
`LARECOMP_TRACE_SVC=1`, `LARECOMP_LOG_FILES=1` (RomFS files as they are read). `RECOMP_MOD_DIR=<dir>` serves files
from `<dir>/romfs/` over the game's.

Added for SMM2:

| Variable | Effect |
|---|---|
| `SMM2_SEED=<n>` | fixed seed for the game's global random generator: every boot shows the same title course and frames repeat exactly (`4`: the course Ryujinx shows) |
| `LARECOMP_CG_SHOT=<frame>` or `<first>:<last>:<step>` | capture the window through the window server (colour-managed, like the Ryujinx reference grabs); with a range, `LARECOMP_SHOT_PATH` is a printf pattern (`shot_%05d.png`). For comparisons: `LARECOMP_WINDOW=1280x720 LARECOMP_SCALE=1` |
| `LARECOMP_ASYNC_SHADERS=1` | compile shaders in the background and skip their draws meanwhile (LAS's behaviour, Metal and Vulkan; SMM2 then misses one-shot renders) |
| `LARECOMP_WINDOW_SHOT=<first>:<last>:<step>` | (Vulkan) the window image every step-th frame in that range, `LARECOMP_SHOT_PATH` a printf pattern (`build/win_%05d.png`); raw sRGB, not colour-managed like `LARECOMP_CG_SHOT` |
| `RECOMP_USER_NAME=<name>` | the account nickname (default "Player") |
| `LARECOMP_DEBUG_FRAME=<n>` | log every draw of frame n and save each render pass to `build/pass_NNN.png`; `LARECOMP_DEBUG_EACH_DRAW=1` saves the target after every draw (Metal); `LARECOMP_DEBUG_NO_PASSES=1` logs without the images (on Vulkan skipped draws log their reason) |
| `LARECOMP_TRACE_CALLS=<n>` | log the first n command-buffer recording / call / submit / render-target / clear calls with their guest return address |
| `RECOMP_NO_MOUSE_TOUCH=1` | the left mouse button no longer touches the console's touch screen (desktop) |
| `LARECOMP_HOST_INPUT="<frame>:tap:<x>/<y>:<frames>"` | (tests) a finger on the touch screen at game pixel x, y (1280x720) for that many frames |
| `RECOMP_BC_SOFT=1` | (Metal) decode BC textures on the CPU, as on GPUs without BC support |
| `RECOMP_NO_CORES=1` | run guest threads without per-core exclusivity (LAS's behaviour; SMM2 races then) |
| `SMM2_SEED_ALL=1` | with `SMM2_SEED`: every later `sead::Random` gets the same seed too (what the Ryujinx trace build's seed patch does; replays with enemies) |
| `RECOMP_PAD_SCRIPT=<file>` | play an input script instead of the keyboard / controllers (which are left out meanwhile): `N BUTTONS` per line, the Ryujinx trace tooling's format (A B X Y L R ZL ZR PLUS MINUS UP DOWN LEFT RIGHT LSTICK RSTICK, and JUMP=A RUN=Y SPIN=ZR; `-` = nothing) |
| `RECOMP_PAD_CLOCK=game\|frame` | one script frame per update of the player (`game`, the default for SMM2: the same clock as the Ryujinx trace build) or per presented frame (`frame`: menus, where no player exists); `RECOMP_PAD_START=<n>` starts n ticks late; `RECOMP_PAD_EXIT=1` quits at the script's end |
| `RECOMP_TRACE_OUT=<file>` | one record per player update in the Ryujinx TraceHook format (the player object, `RECOMP_TRACE_LEN` bytes, default 0x3000, and the blocks `RECOMP_TRACE_DEREF="off,len ..."` points to, default the input block 0x550,0x200), from update `RECOMP_TRACE_START` (default 0) until one record after the script ends (`RECOMP_TRACE_FRAMES=<n>` without a script); `RECOMP_TRACE_EXIT=1` quits then. Records are game memory: keep them in `private/` |

Input replays against Ryujinx (both runtimes play the same script from the first player update and are compared
update for update): `tools/replay_sync.py`, findings in [docs/smm2-bringup.md](docs/smm2-bringup.md) ("Input replays").

Status and evidence: `../PROGRESS.md` ("Recomp"); what the bring-up needed: [docs/smm2-bringup.md](docs/smm2-bringup.md).

Mods (SMM1's 100 Mario Challenge on SMM2's Endless Challenge, against our own course server): how to turn them on,
server setup and how they work in [docs/mods.md](docs/mods.md).
