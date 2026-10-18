# SMM2 Recomp on Windows

The SMM2 Recomp (`recomp/`, see [recomp/README.md](../recomp/README.md)) is Super Mario Maker 2 v3.0.3 itself,
recompiled from **your own dump** into a native Windows program that draws with Vulkan. The download contains no
game files. On first launch it recompiles the game code from your files, on your PC.

> **Status: built, not yet run on Windows.** The Windows build (`smm2recomp.exe`, the game code DLL and the player
> package) is cross-compiled and linked on macOS. Nobody has started it on a Windows PC yet; the owner will be the
> first to test it.
>
> The same code boots to the title screen on Linux arm64, in a VM on Mesa's software Vulkan (lavapipe). That covers
> the Vulkan renderer, the shader translator and the first-launch recompile from the player package. The Windows-only
> parts have never run: the Windows platform layer, the DLL loading and the link against the exe's import library.
>
> Expect problems. Please report them with the console output.

## What you need

- **Windows and hardware:**
  - Windows 10 or 11, 64-bit.
  - A CPU with AVX2: Intel Haswell (2013) or newer, AMD Zen or Excavator or newer.
  - A graphics driver with **Vulkan 1.3**. Current NVIDIA, AMD and Intel drivers have it.
- **[Python](https://www.python.org/downloads/) 3.12 or newer.** When installing, tick "Add python.exe to PATH". The
  game code generator runs on it.
- **Your own dump of Super Mario Maker 2, extracted to folders:**
  - the base game's RomFS
  - the **3.0.3 update**'s ExeFS (`main`, `sdk`, `rtld`, `main.npdm`)
  - the 3.0.3 update's RomFS

  Any extraction tool that takes your own console's keys works, for example hactoolnet or hactool on the NSP or XCI
  and on the update NSP.
- **Your own console's firmware dump and keys,** for the shared fonts and system data (step 3 below). Step 3 needs
  [hactool](https://github.com/SciresM/hactool). The game stops at the title screen without the fonts.
- **About 4 GB of free disk** next to the program during the first launch. The generated C and the objects are
  deleted afterwards; the game code DLL stays (about 190 MB).

## Get it

Unzip `SMM2Recomp-windows-x86_64.zip` somewhere you can write to, such as Documents or the Desktop, not Program
Files. The same folder holds your saves, settings and caches.

You can also build it yourself, from macOS or Linux (see [Build it yourself](#build-it-yourself-cross-compile-from-macos-or-linux)).

The zip contains:

- `smm2recomp.exe`
- `SDL3.dll`, `libiconv-2.dll` and `libshaderc_shared.dll`. Vulkan itself comes with your graphics driver.
- `smm2recomp.dll.a`, the import library the game code DLL links against
- `zig\`, the compiler
- `tools\`, the generator (Python) and the dump setup scripts
- `gamelib-src\`, the headers the game code is compiled against
- `setup.bat`, `README.txt` and the licenses

## First launch: your dump, then the recompile

Run these steps in a PowerShell terminal opened in the unzipped folder.

1. **Install the generator's Python packages** (once):
   ```powershell
   .\setup.bat
   ```
   This makes `tools\venv` with capstone, unicorn and lz4.

2. **Lay out your game files in `extracted\`:**
   ```powershell
   tools\venv\Scripts\python tools\games\smm2\setup.py --base-romfs D:\dump\base\romfs `
       --update-exefs D:\dump\update\exefs --update-romfs D:\dump\update\romfs --out extracted
   ```
   - The update's files go over the base game's.
   - The script uses symbolic links if Windows allows them (Developer Mode). Otherwise it uses hard links on the same
     drive, and copies as a last resort.
   - It checks that the ExeFS is the 3.0.3 one.

3. **Copy the fonts and system data from your firmware:**
   ```powershell
   tools\venv\Scripts\python tools\games\smm2\sysdata.py --firmware D:\firmware\registered `
       --keys D:\keys\prod.keys --data .
   ```
   - `--firmware` is the registered NCA folder of a system dump, for example Ryujinx's
     `bis\system\Contents\registered`.
   - `--keys` is your `prod.keys`.
   - hactool must be on PATH, or pass `--hactool <path>`.

   This writes `fonts\` (the console's shared fonts) and `sysdata\` (Mii model, word filter, time zones). The keys are
   read in place, never copied, and hactool's output is not shown.

4. **Start `smm2recomp.exe`.** The first launch shows a notice, then builds the game code from your files:
   1. It reads `extracted\exefs` and generates the game code as C (`tools\gen.py`).
   2. It compiles the C with the bundled zig, using one job per CPU thread.
   3. It links `extracted\smm2game.dll` against the exe.

   This takes from a few minutes to half an hour, depending on the CPU. The console window shows the progress. On
   Linux, the same build from the package took 29 minutes in an 8-core VM.

   The game starts when the build is done, and later launches start straight away. Your saves, settings and shader
   caches are kept next to the program.

## Play

| Keyboard | Switch control |
|---|---|
| WASD | left stick |
| arrows | d-pad |
| K / J / L / I | A / B / X / Y |
| Q / E | L / R (the title screen asks for L + R) |
| 1 / 3 | ZL / ZR |
| Enter / Backspace | Plus / Minus |

Controllers work through SDL. F1 or Esc opens the overlay menu, with its Video, Audio and Controls tabs.

Useful options, as environment variables:

- `LARECOMP_WINDOW=1280x720` for a window instead of fullscreen.
- `LARECOMP_GPU=<index>` to pick a graphics card. The log lists them.
- `SMM2_SEED=4` for the same title course on every start.

## Build it yourself (cross-compile from macOS or Linux)

The release is built with zig (the compiler the package also ships) and MSYS2's UCRT64 libraries.

1. **Put the dependencies in a folder,** for example `deps/windows-x86_64`. Download these MSYS2 UCRT64 packages from
   https://repo.msys2.org/mingw/ucrt64/ and unpack the `.pkg.tar.zst` files:

   `sdl3`, `vulkan-loader`, `vulkan-headers`, `shaderc`, `vulkan-memory-allocator`, `libiconv`

   Then arrange the folder like this:

   | Folder | Contents |
   |---|---|
   | `include/` | SDL3, vulkan, vk_video, shaderc, `vk_mem_alloc.h` |
   | `lib/` | the import libraries `libSDL3.dll.a`, `libvulkan-1.dll.a` and `libshaderc_shared.dll.a`, each also copied as `SDL3.lib`, `vulkan-1.lib` and `shaderc_shared.lib` (zig looks for those names) |
   | `bin/` | `SDL3.dll`, `libiconv-2.dll`, `libshaderc_shared.dll` |

2. **Set up your own dump for the recomp** (`make -C recomp venv setup`; see recomp/README.md).

3. **Build and package:**
   ```sh
   recomp/tools/fetch_zig.sh
   make -C recomp TARGET=windows-x86_64 GAME=smm2 DEPS=deps/windows-x86_64 gen all
   make -C recomp TARGET=windows-x86_64 GAME=smm2 DEPS=deps/windows-x86_64 package
   ```
   - `gen` writes the x86-64 game code to `generated/smm2-c`. It lowers FP/SIMD to portable C (`--simd c`).
     linux-x86_64 shares this code.
   - `all` writes to `build/smm2-windows-x86_64/`. It makes these files:

     | File | Size | What |
     |---|---|---|
     | `smm2recomp.exe` | 2.4 MB | the runtime |
     | `smm2recomp.dll.a` | | its import library: the eight runtime symbols the game code calls (seven runtime functions and the player-update hook) (`runtime/platform/windows_exports.def`) |
     | `smm2game.dll` | 186 MB | the game code (plus a 440 MB `.pdb`) |

     `smm2game.dll` is made from your dump: don't share it. The package doesn't contain it.
   - `package` writes `dist/SMM2Recomp-windows-x86_64.zip` (85 MB).

## Troubleshooting

- **"needs a CPU with AVX2"**: the game code is compiled for x86-64-v3, so older CPUs can't run it.
- **The first launch says the builder isn't there**: one of these is likely:
  - `setup.bat` didn't run, or failed. Check that Python is on PATH.
  - The `zig\`, `tools\` or `gamelib-src\` folder isn't next to the exe.
- **"building the game code failed"**: the console names the failing step and the last line of its log. An ExeFS from
  another version fails in the generator. To rebuild, delete `extracted\smm2game.dll` and `extracted\smm2game.dll.id`.
- **The game stops at the title screen** with a crash in the log: the fonts are missing (step 3).
- **A black window or a Vulkan error at start**: update the graphics driver to one with Vulkan 1.3. On laptops with
  two graphics cards, set `LARECOMP_GPU=<index>`.
- **The first seconds of a new scene stutter**: shaders are translated and compiled the first time they're used, and
  cached in `cache\` afterwards.
- **Known difference from the console**, seen on Linux and so expected on Windows: in the 3D World title course, the
  Mario model is missing from the Vulkan renderer's frames. The Metal (macOS) renderer draws him, and everything else
  matches.

SMCraft (the earlier clean-room engine) and its Windows build `mmk-play.exe` are archived on the `smcraft-archive`
branch, with their instructions.
