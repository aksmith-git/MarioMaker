# Engine architecture

Chosen: **Option B — Rust core + thin Swift shell** (2026-10-18; options and trade-offs in
[architecture-options.md](architecture-options.md)). This page is the working design; it changes
as the code does.

## 1. Repository layout

```
core/                       Cargo workspace (Rust, toolchain pinned in rust-toolchain.toml)
  crates/
    formats/                course (SMM2 + SMM1), sarc, yaz0, zstd, byml, aamp, msbt,
                            bfres, bntx (+ Tegra unswizzle), bwav/bars (DSP-ADPCM), nso (LZ4),
                            Wii U UI: lyt (BFLYT/BFLAN + JSON form), bflim + gx2 (AddrLib
                            untiling), bffnt, bcn (software BCn decode for tools)
    import/                 first-launch importer: dump → asset cache (also used by the CLI);
                            `smm1` stage: Wii U content folders → SMM1 UI cache
    ui/                     layout player: SMM1 layouts + animations → draw list of quads
    assets/                 runtime asset-cache format: reader, writer, placeholder-pack builder
    sim/                    deterministic simulation (world, player, actors, camera, rules)
    audio/                  mixer: voices, loops, effects → interleaved f32 PCM
    modes/                  normal play, 100 Mario run manager, CourseSource trait + adapters
    ffi/                    C ABI static library for the Swift app (cbindgen header)
    cli/                    `mmk` tool: import, inspect, headless run/replay, golden dumps
app/                        Xcode project (Swift 6, SwiftUI + Metal + GameController + AVAudioEngine)
  Shell/  Render/  Input/  AudioOut/  Bridge/ (module map for the core library)
assets/placeholder/         original placeholder art + manifest (built into the runtime format)
smmfmt/  tests/             Python reference implementation of the course formats (parity oracle)
```

## 2. Runtime structure

```
 ┌───────────────────────── Swift app ─────────────────────────┐
 │ SwiftUI shell: library, settings, saves, mode select         │
 │ Metal renderer ◄── render list (POD arrays)                  │
 │ GameController/keyboard ──► input record per tick            │
 │ AVAudioEngine source node ◄── mixer PCM (audio thread pull)  │
 └───────────────▲──────────────┬───────────────────────────────┘
                 │ C ABI (ffi)  │
 ┌───────────────┴──────────────▼──────── Rust core ───────────┐
 │ modes ─► sim (60 Hz fixed step) ─► render list + audio events│
 │ assets (runtime cache)   import (first launch)   formats     │
 └──────────────────────────────────────────────────────────────┘
```

### Threads
- **Sim thread** (Rust-owned): fixed 60 Hz steps from an accumulator driven by the host clock.
  Publishes a double-buffered snapshot (render list + HUD state) after each step.
- **Main/render thread** (Swift): `CAMetalDisplayLink`/`MTKView` at display rate; reads the latest
  snapshot; never mutates sim state. No interpolation by default (SMB1 style is pixel-exact).
- **Audio thread** (AVAudioEngine render callback): pulls PCM from the mixer through a lock-free
  ring; sim posts audio events (play SE, change BGM, tempo for hurry-up) into a queue.

### Determinism rules (sim crate)
- Pure step function: `state' = step(state, input)`. No clock, I/O, threads, hash-map iteration
  order or allocation-dependent behaviour inside a step.
- `f32` arithmetic like the original; Rust does not contract `a*b+c` into FMA, keep it that way
  (no `mul_add` unless the spec says the game uses it). Trig via our own lookup table built from
  first principles to the game's resolution (objects spec notes a 256-entry base+slope table).
- Input is a compact per-tick record (buttons + stick), so every run is a replay. Replays drive
  regression tests and frame-by-frame comparison with Ryujinx captures.

## 3. FFI boundary (`ffi` crate)

- Opaque handles: `MmkEngine`, `MmkAssets`, `MmkImporter`. Everything else is `#[repr(C)]` POD.
- Calls (shape, not final):
  - `mmk_assets_open(path) → MmkAssets*`; `mmk_assets_textures/meshes(...)` enumerate GPU resources
    so Swift uploads them once (textures are already Metal pixel formats).
  - `mmk_engine_new(assets, config) → MmkEngine*`; `mmk_engine_load_course(engine, bytes, len)`.
  - `mmk_engine_set_input(engine, const MmkInput*)`; the sim thread consumes it each tick.
  - `mmk_engine_snapshot(engine, MmkSnapshot* out)` → pointers + lengths into the published
    buffer (draw items: mesh/material ids, 3×4 transform, animation frame, layer, tint; light masks;
    HUD), valid until the next call.
  - `mmk_audio_render(engine, float* out, frames)` (audio thread).
  - `mmk_import_run(romfs, exefs, update_romfs?, update_exefs?, out_root, progress_cb, user)` (implemented; blocking, the app runs it on a background thread; cancel not yet). `mmk_course_info(assets?, bytes, len, out)` for the library; `mmk_engine_restart`; `mmk_assets_placeholder()` for running without a dump.
  - Layout player (`ffi/src/ui.rs`): `mmk_ui_open(dir) → MmkUi*` (an imported SMM1 UI folder),
    `mmk_ui_layout(ui, name) → MmkUiLayout*`, `mmk_ui_play(layout, "[Part/…/]Anim", frame)`,
    `mmk_ui_stop`, `mmk_ui_anim_length`, `mmk_ui_set_text`, `mmk_ui_layout_size`,
    `mmk_ui_draw_list(layout, &quads) → count` (`MmkUiQuad`, 132 bytes: 4 corners, 2 UV sets,
    vertex colours, material black/white, 2 texture ids, wrap/filter/combine/blend flags), and
    `mmk_ui_texture_count/mmk_ui_texture` (grows as layouts open; upload new entries).
  - `mmk_last_error()` for messages; every entry point catches panics and returns an error code.
- Header generated by cbindgen in `ffi/build.rs`; Swift sees it through `app/Bridge/module.modulemap`.

## 4. Asset pipeline

### Importer input (v1)
A folder with the owner's extracted **RomFS + ExeFS**, as written by Ryujinx "Extract Data" or by
our CLI from their dump, plus optional update RomFS/ExeFS. (v2, later: read the NSP directly with
the owner's keys.)

### Steps
1. **Identify** the build: NSO module id of `exefs/main` → known build table (ids only, no
   Nintendo data); detects base v1.0.0 vs v3.0.3 and whether the update is merged.
2. **Overlay** update RomFS on base (if not already merged).
3. **Course key table**: decompress `main` (NSO, LZ4 segments), find the table as
   `smmfmt.smm2.keyfind` does, store it in the cache's private `keys/` folder.
4. **Tables**: BYML/AAMP → engine tables (EnemyDB, EditDB, SceneDB per style, PlayerAnmTable,
   BGM list, light masks, env params).
5. **Textures**: BNTX → unswizzle (Tegra block-linear) → store as Metal-ready blobs (RGBA8 sRGB,
   BC1/4/5/7) with mip chains.
6. **Models/animations**: BFRES → engine mesh, material (texture refs, blend, sampler),
   skeleton, skeletal / texture-pattern / SRT animations.
7. **Audio**: BWAV/BARS → kept as DSP-ADPCM + loop points; decoded in the mixer.
8. **Manifest**: importer version, source file hashes, contents → enables incremental re-import.

Output: `~/Library/Application Support/<app>/assets/<build-id>/`. Nothing is ever written back to
the repo.

### SMM1 UI import (`mmk import-smm1 <base content> <update content> <out>`)
Input: the owner's decrypted Wii U `content/` folders (base disc + update; update files replace
base files with the same path). Output `<assets>/smm1-v<title_version>/`:

```
manifest.json                         build, counts, layout / font / message lists
ui/layouts/<Layout>.json              parsed BFLYT + its BFLAN animations + texture list
ui/textures/<Layout>/<tex>.mtex       BFLIM → untiled; BC1-5 kept compressed, L/LA/565 → RGBA8,
                                      luminance/alpha formats via the channel-select swizzle
ui/fonts/<Font>.json, <Font>/sheet_n.mtex   BFFNT metrics + character map, glyph sheets
ui/messages/<LayoutMsg|CommonMsg>/<group>/<file>.json   MSBT (text + control tags)
```
Layout archives are found as loose `Layout/*.szs` and as `Layout/*.szs` members of every
`Pack/*.pack`. Texture conversion untiles GX2 surfaces with a clean implementation of the
AddrLib (R6xx/R7xx) address functions; stored sizes match the computed padding for every texture.

### Layout player (`ui` crate)
`UiCache` loads layouts on demand (with their part layouts, textures, fonts and messages);
`LayoutPlayer` holds bound animations (per part instance too) and text overrides. Each draw
starts from the base pane/material values, applies the bound curves (pane SRT/size, visibility,
vertex colours + pane alpha, material colours, texture SRT, texture pattern; hermite and step
keys), walks the pane tree parents-first (transform `T·Rz·Ry·Rx·S` with the parent-anchor
offset; alpha passed on by "influenced alpha" panes) and emits quads for pictures, window frames
(1/2/4/8-frame kinds) and text glyphs (BFFNT metrics, text position and line alignment, shadow
pass, per-character fallback font). Deterministic; unit-tested on synthetic layouts. Message
labels resolve in the layout's own MSBT, and part-property text ids in the enclosing screen's. The **placeholder pack** uses exactly the same format and is built from
`assets/placeholder/` (original art), so the engine always has something to draw.

## 5. Rendering (Swift, Metal)

- Resources uploaded once from the asset cache (`MTLTexture`, vertex/index buffers).
- Per frame: draw items sorted by layer (background → terrain → actors → effects → UI);
  orthographic camera for 2D styles, nearest sampling for M1/M3/MW; light-mask pass for night
  darkness; perspective path for SM3DW later.
- Shaders (MSL): unlit textured (2D styles), simple lit (NSMBU/SM3DW, later), light mask, UI.
- UI (`app/Sources/MMKApp/UILayer.swift`): draws `MmkUiQuad` lists — t = combine(tex0, tex1),
  c = mix(black, white, t) · vertex colour; one pipeline per blend mode, one sampler per
  wrap/filter combination; layout units mapped to the viewport (origin centre, y up). Debug view:
  `MMK_UI_LAYOUT=<name> [MMK_UI_ANIM=a,b~,c@30] [MMK_UI_TEXT=pane=text;…]
  [MMK_SCREENSHOT=<png> MMK_SCREENSHOT_TICKS=<n>]` (`UIPreview.swift`).

## 6. Testing

| Layer | How |
|---|---|
| formats | Rust unit tests; **parity tests with the Python `smmfmt`** on synthetic fixtures (committed) and on the owner's corpus (local, skipped in CI) — byte-exact round trips and identical parsed values |
| import | fixtures built from synthetic containers; full import tested locally |
| sim | spec tests (e.g. held-jump peak 67.6 px standing / 86.3 px at full run; acceleration curves); replay golden traces; property tests (determinism: same input → identical state hash) |
| ffi | smoke tests from Rust and from a Swift test target |
| app | Swift build in CI; manual play testing against Ryujinx |
| guard | `scripts/check_no_proprietary.py` (unchanged) |

CI: `cargo test` on Linux and macOS, Swift build on macOS, guard.

## 7. Dependencies (proposed; mainstream crates, versions pinned in Cargo.lock)

| Crate | Use | Licence |
|---|---|---|
| zstd | `.zs` archives | MIT/Apache-2.0 (+ libzstd BSD) |
| flate2 | zlib (dataset courses) | MIT/Apache-2.0 |
| lz4_flex | NSO segments | MIT |
| aes, cbc, cmac | `.bcd` crypto (RustCrypto) | MIT/Apache-2.0 |
| crc32fast | CRC32 | MIT/Apache-2.0 |
| serde, serde_json | tables, manifests, saves | MIT/Apache-2.0 |
| thiserror | errors | MIT/Apache-2.0 |
| clap | CLI | MIT/Apache-2.0 |
| cbindgen (build) | C header | MPL-2.0 (build tool only) |
| ureq (later) | HTTP course sources | MIT/Apache-2.0 |

The Swift app uses only Apple frameworks.

## 8. Order of work for Phase 5 (vertical slice: SMB1 style, ground theme)

1. Workspace + CI skeleton; `formats` course parser with Python parity tests.
2. Containers + tables (SARC, Yaz0, zstd, BYML) and the importer's identify / key-table / tables steps.
3. BNTX unswizzle + BFRES subset needed for M1 ground terrain, Mario, Goomba, Koopa, blocks, coin, pipe.
4. Swift app skeleton: window, Metal renderer drawing a test render list; input; audio out.
5. Sim: tiles/ground + slopes collision, camera, player (SMB1 feature set) from player-physics.md.
6. Core objects in object-frequency order (coin, pipe, ? block, brick, mushroom, hard block,
   Goomba, Koopa, …) from the object specs; goal and timer.
7. Load a real SMB1 ground course from the sample, play start → goal; tune against Ryujinx.
