# Upstream projects

The recomp is a fork of **LAS Recomp**, retargeted from *Link's Awakening* to *Super Mario Maker 2* v3.0.3 and
reorganised toward a shared runtime plus per-game glue. Ideas (and, where noted, files) come from
**SuperMarioWonderRecomp**. Both are MIT; their notices are kept next to this file.

| Project | Upstream | Commit forked / read | License (here) |
|---|---|---|---|
| LAS Recomp | https://github.com/peiikai/LASRecomp | `e9245915d84df3b43a8502fbc7cf5f2292335c20` (main, 2026-10) | MIT, `LICENSE-LASRecomp` |
| SuperMarioWonderRecomp ("SwitchGamesRecomp") | https://github.com/BelmanteGu/SuperMarioWonderRecomp | `010ed7a596a621c15d3a8dfa5795ca8c3668b066` (main, 2026-10) | MIT; no files taken yet (layout ideas only) |

Third-party code that came with the LAS fork keeps its own license: `third_party/imgui` (MIT, trimmed to the core and
the SDL3 / Metal / Vulkan backends) and `third_party/smaa` (MIT-style, `third_party/smaa/LICENSE.txt`). Added here:
`third_party/bcdec` (bcdec by Sergii Kudlai, MIT or Unlicense, `third_party/bcdec/LICENSE`): BC texture decoding on
the CPU for GPUs without BC support (the iOS Simulator).

## What was taken from LAS Recomp

- **Recompiler** (`tools/`): `gen.py` (whole-module AArch64 -> C, function discovery, Unicorn-resolved jump tables),
  `lifter.py`, `analyze.py` (eh_frame), `nso.py`, `simd_c.py` (portable FP/SIMD lowering), `passes.py`, the patch
  mechanism (`patches.py`, now empty: games bring theirs via `--patches`), the game-library entry and the lifter /
  SIMD / shader / ASTC tests, `fetch_zig.sh` / `zig_slim.sh` (the compiler players' machines build with).
- **Runtime** (`runtime/`): loader, dispatcher, kernel (threads, sync, memory, SVCs), HIPC/CMIF IPC, services (acc,
  am/applet, apm, lm, audio, fs, hid, set, time, pctl, aoc), HLE of nn::vi / nv / pl / swkbd / lmem heaps / hid
  input / movie, NVN on the host (`runtime/nvn`: objects, command buffers, Maxwell shader translator, ASTC, Metal and
  Vulkan backends, SMAA, output scaling), platform layers (macOS, Linux, Windows), paths / settings / game library
  loading, the in-game overlay menu (Video, Audio, Controls, Quit) and its on-screen keyboard.
- **Build** (`Makefile`): the same compile rules, now parameterised by `GAME` (games/<game>/game.mk).

## What was left out (Link's Awakening only)

- The randomizer (`third_party/LAS-Randomizer`, `runtime/rando.*`, `tools/rando`, the randomizer / tracker / spoiler /
  warp / pets / save-editor / cheats / difficulty / cosmetics UI) and every gameplay hook that names LA addresses
  (`runtime/hle/`: chamber, digs, doorjump, escape, flags_ext, hookshot, keysanity, miniboss, mirror, pets, pot_glow,
  qol, rescue, shopsanity, souls, warp, widescreen, framepace).
- LA's instruction patches (`tools/patches.py`'s widescreen / gameplay sites) and LA's texture edits
  (`runtime/nvn/texpatch.c` is now a neutral implementation of the same interface).
- The LA start screen (`runtime/ui/launcher.mm`): the game starts straight away; game files are set up by
  `make smm2` instead (a player-facing start screen can come back later).
- Packaging (`.app` / `.dmg` / Windows zip / AppImage / PyInstaller helpers), LA docs, fonts and screenshots.

## What changed in the shared parts

- `runtime/game.h`: the game's identity (`GAME_ID`, `GAME_TITLE`, `GAME_TITLE_ID`, `GAME_BUNDLE_ID`) from game.mk,
  used for the window title, data folder, settings file, game library name (`<id>game.dylib`) and the program id
  the kernel / aoc report (LAS hard-coded LA's).
- `runtime/game_hooks.c`: the call sites LAS's renderers / input / fs had into LA features stay, and resolve to
  weak, neutral defaults here (a game can override any of them).
- `runtime/mods.*` replaces `rando.*`: the same RomFS-override and save-slot hooks, driven by `RECOMP_MOD_DIR`.
- `runtime/romfs_build.*` + `runtime/services/fs.c`: the RomFS can be served from a folder through an in-memory
  index (tables in memory, file data read from the host files), so a 2.7 GB image needn't be built and an update's
  files can be links over the base game's.
- The game library's entry is `recomp_game_tables`.
- Cross builds with zig (`make TARGET=...`) and player packages (`tools/package.sh`, written here: runtime + zig +
  generator, no game files). With no start screen, the runtime runs LAS's `gamebuild.c` itself on first launch when the
  game code library is missing (`runtime/main.c`).

Environment variables keep LAS's `LARECOMP_*` names for now (they're documented in LAS's sources); new ones are
`RECOMP_*`.
