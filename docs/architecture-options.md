# Engine architecture — options (Phase 4)

Status: **proposal, awaiting the owner's choice.** No engine code is written until one option is
picked. Chosen option and reasons will be recorded in DECISIONS.md.

## 1. What the engine has to do (from Phases 1–3)

| Need | Detail that drives the design |
|---|---|
| Native macOS app, Apple Silicon | `.app` bundle, menus, settings, controller support, save data (Phase 5b) |
| Metal rendering | All styles ship as model files (BFRES) with 2D or 3D meshes. 2D styles are textured quads with texture-pattern / UV / bone animations; SM3DW is real 3D. Textures are BNTX: ~98 % RGBA8 sRGB, the rest BC1/BC4/BC5/BC7 — all native on Apple Silicon GPUs, but stored Tegra block-linear (must be unswizzled at import). |
| Faithful physics | One shared player model with style feature switches; per-frame constants in px/frame at a fixed 60 Hz (research/specs/player-physics.md). Needs a deterministic fixed-step simulation and replayable input. |
| Audio | BWAV (DSP-ADPCM in every file sampled) for music and SFX, BARS banks; loop points; per-theme reverb/delay. |
| Asset import at first launch | From the owner's own dump: RomFS + ExeFS (+ update). Containers: SARC, Yaz0, zstd; data: BYML, AAMP, MSBT. Course key table found in the owner's executable (NSO → LZ4 decompress). |
| Runs without a dump | Original placeholder art pack in the same runtime format. |
| Course formats | Port of `smmfmt` (SMM2 + SMM1); parity tests against the Python reference. |
| 100 Mario Challenge | Local library + pluggable HTTP sources (research/specs/100-mario-challenge.md §c). |

## 2. Architecture that is the same for every option

```
 App shell ── menus, course library, settings, saves, mode selection
   │
 Modes ────── NormalPlay · HundredMario (run manager, CourseSource adapters)
   │
 Sim (pure, deterministic, 60 Hz fixed step)
   ├─ World: areas, tiles/ground, slopes, liquids, camera, autoscroll, timer, clear conditions
   ├─ Player: shared movement model + style feature switches + power-up forms
   ├─ Actors: core behaviour + 18 "contexts" (wing, parachute, pipe, carried, track…)
   │          EnemyDB attack→reaction matrix loaded as data
   └─ Input → Sim is a per-frame button/stick record  (replays, tests, frame comparison)
   │
 Render (reads a per-frame snapshot of Sim; never writes to it)
   ├─ lite model renderer: meshes, materials, skeletal + texture-pattern + SRT animation
   └─ 2D layers, light masks (night), UI
 Audio (event queue from Sim → mixer; DSP-ADPCM decode; loops)
 Assets: runtime cache  ← Importer (first launch) | Placeholder pack (in repo)
 Formats: course (SMM2/SMM1), SARC, Yaz0, zstd, BYML, AAMP, BFRES, BNTX, BWAV/BARS, MSBT
```

Key rules, whichever language:
- **Simulation is a pure function of (state, input) per 60 Hz tick.** No wall-clock time, no
  rendering state, no I/O inside it. Render runs at display rate (60/120 Hz) from the latest
  snapshot. This gives deterministic replays, headless tests and frame-by-frame comparison with
  Ryujinx recordings.
- **Behaviour comes from the specs, data from the importer.** Tables that are data in the game
  (EnemyDB, EditDB, SceneDB, player animation table) are imported as data, not hard-coded.
- **Importer output is the only thing the engine reads** (`~/Library/Application Support/<app>/assets/<build>/`):
  unswizzled textures (Metal-ready), meshes + animations in an engine format, audio, text, tables.
  The placeholder pack uses the same format and ships in the repo.
- **Importer input**, v1: an extracted RomFS + ExeFS folder (Ryujinx "Extract Data" or our CLI),
  optional update RomFS/ExeFS. v2 (later): read the NSP directly with the user's keys.

## 3. Options

### Option A — Swift everywhere (Metal, AppKit/SwiftUI, GameController, AVAudioEngine)

- Language/build: Swift 6, SwiftPM packages (Formats, Importer, Sim, Render, Audio) + an Xcode app
  target. zstd vendored as a C target (BSD licence); everything else written in Swift.
- Render: Metal via MetalKit (`MTKView`), shaders in MSL. Audio: AVAudioEngine with a custom
  source node (mixer + ADPCM decode). Input: GameController framework (Switch Pro, Joy-Con,
  DualSense, Xbox; keyboard via GCKeyboard). UI: SwiftUI for menus/settings, AppKit where needed.
- Tests: Swift Testing; headless `simtest` CLI runs courses/replays without a window.

| For | Against |
|---|---|
| Most native result: menus, settings, controllers, notarised `.app` all first-party | macOS (and iOS/iPadOS) only — no Windows/Linux port without a rewrite |
| One language and one toolchain (Xcode) | Fewer game-dev libraries; we write format parsers ourselves (we would anyway) |
| Direct Metal: no abstraction layer, Apple's GPU tools (frame capture) work fully | Swift generics/ARC need care in the 60 Hz inner loop (value types, no per-frame allocation) |
| Easiest path to Phase 5b (native app) | Smaller open-source contributor pool for Swift game code |

### Option B — Rust core + thin Swift shell

- `core` (Rust, `cargo`): formats, importer (also as a CLI), deterministic simulation, audio mixing.
  Exposed as a static library with a C ABI (`cbindgen`) — one call per tick:
  `step(input) → snapshot`, plus render-list and audio-event queries.
- Swift app: window, Metal renderer consuming the render list, GameController input, CoreAudio
  output, SwiftUI menus — same as Option A's shell.

| For | Against |
|---|---|
| Core is portable and testable on its own (CI on Linux, headless replays, fuzzing the parsers) | Two languages, two build systems (cargo inside an Xcode build phase), FFI boundary to maintain |
| Rust's safety and speed for parsers and the simulation | Every new Sim feature that the renderer needs crosses the FFI (snapshot schema must be designed carefully) |
| Native shell still gives the Option A app quality | Most setup work before the first frame on screen |
| A later Windows/Linux port only needs a new shell + renderer | Contributors need both Rust and Swift for full-stack changes |

### Option C — C++20 + SDL3 (SDL_GPU on Metal) + Dear ImGui

- CMake; SDL3 for window, GPU (SDL_GPU → Metal), audio and gamepads; Dear ImGui for menus and
  debug tools; shaders authored once and cross-compiled (SDL_shadercross).
- All formats/sim in C++; zstd/zlib as libraries.

| For | Against |
|---|---|
| Portable to Windows/Linux almost for free | Least "native Mac" feel: ImGui menus, no SwiftUI/AppKit settings, more work for a polished Phase 5b |
| Huge game-dev ecosystem; SDL's controller database is excellent | No memory safety; parsers of untrusted files (downloaded courses) need extra care/fuzzing |
| Closest structure to how the original is built; fast | SDL_GPU is young; shader cross-compile step adds build complexity |
| Dear ImGui makes great in-engine debug/inspection tools | Apple GPU tooling works, but through an abstraction |

## 4. Comparison

| | A Swift | B Rust + Swift | C C++ + SDL3 |
|---|---|---|---|
| Native macOS app quality | ★★★ | ★★★ | ★★ |
| Time to first playable slice | ★★★ | ★★ | ★★ |
| Portability (Win/Linux later) | ★ | ★★★ (core) | ★★★ |
| Safety (parsing downloaded courses) | ★★★ | ★★★ | ★ |
| Toolchain simplicity | ★★★ | ★ | ★★ |
| Headless testing / CI | ★★ (macOS CI) | ★★★ | ★★★ |
| Debug tooling | ★★ (Xcode, Metal capture) | ★★ | ★★★ (ImGui) |

## 5. Recommendation

**Option A (Swift)** if macOS (and maybe iPad) is the target: fewest moving parts, best native
app, fastest to the vertical slice, and the Sim stays a pure, testable Swift package.

**Option B** if a Windows/Linux version is a real goal: pay the FFI/build cost now rather than
rewriting later.

Option C is the strongest choice only if portability matters more than a native Mac feel.

## 6. Smaller decisions that come with any option (proposed defaults)

| Topic | Proposal |
|---|---|
| Minimum OS | macOS 14 (Apple Silicon only) |
| Frame timing | 60 Hz fixed sim; render at display rate; optional interpolation off by default (pixel-exact SMB1) |
| Rendering of 2D styles | Lite BFRES renderer (quads + animations), orthographic camera, nearest-neighbour sampling for M1/M3/MW |
| App name | Neutral, non-Nintendo name for the open-source app (trademark); the repo can keep its name |
| Licence | Still open (owner's call) |
| Importer v1 input | Extracted RomFS/ExeFS folders; NSP + keys support later |
| Saves | JSON in Application Support; course library as plain folders of course files |
