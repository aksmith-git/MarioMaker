# SMCraft on Windows

SMCraft runs on Windows 10 and 11 (64-bit) as `mmk-play.exe`, the portable player: the SMM2
menus, Course World, 100 Mario / Endless Challenge, online play, and the Course Maker. `mmk.exe`
is the companion tool: it imports your game data, runs a course server and more.

> **Status:** the Windows build compiles and links (cross-built and checked from macOS); it has not
> yet been run on Windows hardware by the developers. Reports are welcome.

The **SMM2 Recomp** (the native port by static recompilation, in `recomp/`) gets its own Windows
section here once its macOS bring-up is done.

## What you need

- Windows 10 or 11, 64-bit, and a graphics card with DirectX 12 or Vulkan.
- **Your own copy of Super Mario Maker 2**, dumped from your own Switch, with its RomFS and ExeFS
  extracted to folders. Version 3.0.3 (the base game plus the 3.0.3 update) is recommended. SMCraft
  contains no game files and never downloads any; nothing you import leaves your PC.
- Optional: your own Super Mario Maker (Wii U) dump for the 100 Mario screens in their original
  style and the Mystery Mushroom costumes.

## Get SMCraft

Either use a prebuilt `SMCraft-windows-x64.zip` (unzip anywhere), or build it:

### Build on Windows

1. Install [Rust](https://rustup.rs) (the default MSVC toolchain), and the Visual Studio 2022
   Build Tools with the "Desktop development with C++" workload.
2. In a terminal:
   ```powershell
   git clone https://github.com/aksmith-git/MarioMaker
   cd MarioMaker\core
   cargo build --release -p mmk-shell -p mmk-cli
   ```
   The programs are `core\target\release\mmk-play.exe` and `core\target\release\mmk.exe`.

### Build from macOS or Linux (cross-compile)

```sh
brew install mingw-w64            # macOS; on Linux: your distribution's mingw-w64 package
rustup target add x86_64-pc-windows-gnu
scripts/build-windows.sh          # → dist/SMCraft-windows-x64.zip
```

## First launch: import your game data

SMCraft keeps everything in `%LOCALAPPDATA%\SMCraft` (set `MMK_HOME` to use another folder):
imports in `assets\`, your course library in `courses\`, saves in `play\`.

1. **Game data** (art, sounds, models, course keys) from your extracted dump:
   ```powershell
   mmk.exe import --romfs D:\dump\romfs --exefs D:\dump\exefs
   ```
   If the update is extracted separately, add it and SMCraft lays it over the base game (using
   links, so nothing is copied when both are on the same drive):
   ```powershell
   mmk.exe import --romfs D:\dump\base\romfs --exefs D:\dump\base\exefs `
                  --update-romfs D:\dump\update\romfs --update-exefs D:\dump\update\exefs
   ```
   This writes `%LOCALAPPDATA%\SMCraft\assets\smm2-<version>` (about 2 GB; a few minutes).
2. **Menus** (the SMM2 title, Course World, course screens) from the same RomFS (with the update
   applied):
   ```powershell
   mmk.exe import-smm2-ui D:\dump\romfs "$env:LOCALAPPDATA\SMCraft\assets\smm2-ui-3.0.3" --exefs D:\dump\exefs
   ```
3. Optional, from your Wii U dump: the original 100 Mario screens and the costumes
   ```powershell
   mmk.exe import-smm1 D:\wiiu\base\content D:\wiiu\update\content "$env:LOCALAPPDATA\SMCraft\assets\smm1-v208"
   ```

Without step 1 SMCraft still runs, with placeholder shapes instead of the game's art.

## Play

- `mmk-play.exe` starts on the SMM2 menus (when the menu import from step 2 is there).
- Put course files (`.bcd`, or raw / zlib course data) in `%LOCALAPPDATA%\SMCraft\courses` to see
  them in Course World and use them for 100 Mario.
- `mmk-play.exe path\to\course.bcd` plays one course directly; `mmk-play.exe --edit new.bin` opens
  the Course Maker.
- Controls: arrows / WASD move, Z / Space jump, X / Shift run, Esc pause; Xbox, PlayStation and
  Switch Pro controllers work (d-pad / stick, A / B jump, X / Y run, Start pause).
- Online: `mmk-play.exe --server http://your-server:8640` for Course World on an open-source
  course server, `--net-relay your-server:8640 --net-room NAME --net-player 0` for online co-op.
  To host a server yourself: `mmk.exe server --dir D:\smcraft-server` (see self-hosting.md).

`mmk-play.exe` opens a console window next to the game, where it reports what it found and any
problems; leave it open while playing.

## Troubleshooting

- **"assets: none, drawing placeholder boxes"** in the console: the import didn't run or went to
  another folder. Pass `--assets "%LOCALAPPDATA%\SMCraft\assets\smm2-3.0.3"` or set `MMK_ASSETS`.
- **No menus, straight into a demo course:** the menu import (step 2) is missing; or pass
  `--ui-cache <folder>`.
- **Black window / graphics errors:** update your graphics driver. `--cpu-textures` decodes
  textures on the CPU if the GPU rejects compressed textures.
