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

Requirements: Xcode command line tools (clang), Homebrew `sdl3` and `ffmpeg`, Python 3 (3.12+). About 0.7 GB of
disk for the generated C and 1 GB for the build.

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
| `LARECOMP_ASYNC_SHADERS=1` | compile shaders in the background and skip their draws meanwhile (LAS's behaviour; SMM2 then misses one-shot renders) |
| `RECOMP_USER_NAME=<name>` | the account nickname (default "Player") |
| `LARECOMP_DEBUG_FRAME=<n>` | log every draw of frame n and save each render pass to `build/pass_NNN.png`; `LARECOMP_DEBUG_EACH_DRAW=1` saves the target after every draw |
| `LARECOMP_TRACE_CALLS=<n>` | log the first n command-buffer recording / call / submit / render-target / clear calls with their guest return address |
| `RECOMP_NO_CORES=1` | run guest threads without per-core exclusivity (LAS's behaviour; SMM2 races then) |
| `SMM2_SEED_ALL=1` | with `SMM2_SEED`: every later `sead::Random` gets the same seed too (what the Ryujinx trace build's seed patch does; replays with enemies) |
| `RECOMP_PAD_SCRIPT=<file>` | play an input script instead of the keyboard / controllers (which are left out meanwhile): `N BUTTONS` per line, the Ryujinx trace tooling's format (A B X Y L R ZL ZR PLUS MINUS UP DOWN LEFT RIGHT LSTICK RSTICK, and JUMP=A RUN=Y SPIN=ZR; `-` = nothing) |
| `RECOMP_PAD_CLOCK=game\|frame` | one script frame per update of the player (`game`, the default for SMM2: the same clock as the Ryujinx trace build) or per presented frame (`frame`: menus, where no player exists); `RECOMP_PAD_START=<n>` starts n ticks late; `RECOMP_PAD_EXIT=1` quits at the script's end |
| `RECOMP_TRACE_OUT=<file>` | one record per player update in the Ryujinx TraceHook format (the player object, `RECOMP_TRACE_LEN` bytes, default 0x3000, and the blocks `RECOMP_TRACE_DEREF="off,len ..."` points to, default the input block 0x550,0x200), from update `RECOMP_TRACE_START` (default 0) until one record after the script ends (`RECOMP_TRACE_FRAMES=<n>` without a script); `RECOMP_TRACE_EXIT=1` quits then. Records are game memory: keep them in `private/` |

Input replays against Ryujinx (both runtimes play the same script from the first player update and are compared
update for update): `tools/replay_sync.py`, findings in [docs/smm2-bringup.md](docs/smm2-bringup.md) ("Input replays").

Status and evidence: `../PROGRESS.md` ("Recomp"); what the bring-up needed: [docs/smm2-bringup.md](docs/smm2-bringup.md).
