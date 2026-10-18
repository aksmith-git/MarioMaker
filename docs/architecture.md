# Engine architecture

Chosen: **Option B — Rust core + thin Swift shell** (2026-10-18; options and trade-offs in
[architecture-options.md](architecture-options.md)), plus a **portable Rust shell** for Windows,
Linux and the web (DECISIONS.md 2026-10-18 "Cross-platform shell"; §5b). This page is the working
design; it changes as the code does.

## 1. Repository layout

```
core/                       Cargo workspace (Rust, toolchain pinned in rust-toolchain.toml)
  crates/
    formats/                course (SMM2 + SMM1), sarc, yaz0, zstd, byml, aamp, msbt,
                            bfres, bntx (+ Tegra unswizzle), bwav/bars (DSP-ADPCM), nso (LZ4),
                            UI: lyt (BFLYT/BFLAN, Wii U 5.x and Switch 9.0, + JSON form),
                            bflim + gx2 (AddrLib untiling), bffnt (Wii U 3 / Switch 4), bfcpx
                            (Switch font combinations, BFTTF/BFOTF), bcn + bc7 (software BCn decode)
    import/                 first-launch importer: dump → asset cache (also used by the CLI);
                            `smm1` stage: Wii U content folders → SMM1 UI cache; `smm2_ui` +
                            `smm2_font`: SMM2 RomFS → SMM2 UI cache (scalable fonts rasterised)
    ui/                     layout player: SMM1 and SMM2 layouts + animations → draw list of quads
    front/                  SMM2 front end on the layout player (opening, fades, title, Course World,
                            100 Mario, in-course screens, UI sounds), platform-neutral: input +
                            Host trait in, draw items + sound cues out (`mmk-play` uses it)
    assets/                 runtime asset-cache format: reader, writer, placeholder-pack builder
    sim/                    deterministic simulation (world, player, actors, camera, rules)
    audio/                  mixer: voices, loops, effects → interleaved f32 PCM
    modes/                  100 Mario / 10 Mario run manager, course pools (CourseSource trait),
                            Continue/unlock save data (implemented: hundred.rs, pool.rs, save.rs)
    host/                   front-end glue shared by both shells: course bytes (zlib/raw/.bcd),
                            sprite placement (BFRES quads, flips) and resolution, clear colour,
                            audio handle + sound director (sim events → mixer commands),
                            BCn → RGBA8 for GPUs without BC support
    ffi/                    C ABI static library for the Swift app (cbindgen header)
    shell/                  portable shell `mmk-play`: winit + wgpu + gilrs + cpal; native
                            (macOS, Windows, Linux) and wasm32 (WebGPU/WebGL2, WebAudio, OPFS);
                            web/ (page, storage glue, import worker)
    cli/                    `mmk` tool: import, inspect, headless run/replay, golden dumps
app/                        Swift package (Swift 6, SwiftUI + Metal + GameController + AVAudioEngine)
                            for macOS and iOS / iPadOS: Sources/MMKApp (one source tree,
                            AppKit / UIKit differences behind #if os(), Platform.swift),
                            Sources/CMmk (module map + mmk.h for the core library); iOS bundle
                            by scripts/make-ios-app.sh
assets/placeholder/         original placeholder art + manifest (built into the runtime format)
smmfmt/  tests/             Python reference implementation of the course formats (parity oracle)
```

## 2. Runtime structure

Two shells drive the same core: the Swift app (macOS and iOS / iPadOS) through the C ABI, and the
portable Rust shell (§5b) by calling the crates directly. Both use `host` for everything between
the sim and the screen/speakers, so placement and sound rules exist once.

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
- **Audio thread** (AVAudioEngine source node): pulls interleaved stereo f32 from the mixer
  (`mmk_audio_render`); after each tick the engine turns the sim's events into mixer commands
  (play SE, change BGM, hurry jingle → hurry BGM) sent through a bounded queue. The mixer is only
  touched by the audio thread; the queue never allocates.

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
  - `mmk_assets_open(path) → MmkAssets*`; `mmk_assets_texture(...)` enumerates GPU resources
    so Swift uploads them once (textures are already Metal pixel formats). Meshes load on
    demand (`ffi/src/mesh.rs`): `mmk_mesh_find/model/shape/anim/pose`, and the engine's
    per-frame `mmk_engine_meshes`.
  - `mmk_engine_new(assets, config) → MmkEngine*`; `mmk_engine_load_course(engine, bytes, len)`.
  - `mmk_engine_set_input(engine, const MmkInput*)`; the sim thread consumes it each tick.
  - `mmk_engine_snapshot(engine, MmkSnapshot* out)` → pointers + lengths into the published
    buffer (draw items: mesh/material ids, 3×4 transform, animation frame, layer, tint; light masks;
    HUD), valid until the next call.
  - `mmk_audio_open(cache) → MmkAudio*`; `mmk_engine_set_audio(engine, audio)`;
    `mmk_audio_render(audio, float* out, frames)` (audio thread); `mmk_audio_set_volumes`.
  - `mmk_import_run(romfs, exefs, update_romfs?, update_exefs?, out_root, progress_cb, user)` (implemented; blocking, the app runs it on a background thread; cancel not yet). `mmk_course_info(assets?, bytes, len, out)` for the library; `mmk_engine_restart`; `mmk_assets_placeholder()` for running without a dump.
  - Layout player (`ffi/src/ui.rs`): `mmk_ui_open(dir) → MmkUi*` (an imported SMM1 or SMM2 UI folder),
    `mmk_ui_layout(ui, name) → MmkUiLayout*`, `mmk_ui_play(layout, "[Part/…/]Anim", frame)`,
    `mmk_ui_stop`, `mmk_ui_anim_length`, `mmk_ui_set_text`, `mmk_ui_layout_size`,
    `mmk_ui_draw_list(layout, &quads) → count` (`MmkUiQuad`, 132 bytes: 4 corners, 2 UV sets,
    vertex colours, material black/white, 2 texture ids, wrap/filter/combine/blend flags), and
    `mmk_ui_texture_count/mmk_ui_texture` (grows as layouts open; upload new entries).
    Menus: `mmk_ui_pane_rect` (a pane's corners in layout space, global alpha, shown),
    `mmk_ui_set_pane` (force translate / size / scale / alpha over the animations),
    `mmk_ui_add_clone` (a copy of a part pane that follows the original's animations).
    SMM2 in-course screens: `mmk_ui_table_message` (shared tables such as `GoalCondition`),
    `mmk_ui_set_picture` (an import texture in a picture, e.g. a clear-condition icon),
    `mmk_ui_set_material_color`, `mmk_ui_text_width`; `mmk_engine_play_status` (keys, pink
    coins, style, character, checkpoint, time-out death, goal sequence, the player's view
    position and the frames until a death restarts the course — the death iris).
  - SMM2 front end: `mmk_engine_set_character` (player 1's character, the pause Settings);
    `mmk_online_search / download / report` (`ffi/src/online.rs`, the open-source course server
    through `mmk-online`; blocking, called off the main thread) for Course World.
  - `mmk_texture_decode_rgba(info, out, len)`: any cache or UI texture as RGBA8 (BCn decoded by
    `host::texture`), for GPUs without BC support (iPhones, iPads before M1; `MMK_CPU_TEXTURES=1`).
  - `mmk_last_error()` for messages; every entry point catches panics and returns an error code.
- Header generated by cbindgen in `ffi/build.rs`; Swift sees it through `app/Bridge/module.modulemap`.

## 4. Asset pipeline

### Importer input (v1)
A folder with the owner's extracted **RomFS + ExeFS**, as written by Ryujinx "Extract Data" or by
our CLI from their dump, plus optional update RomFS/ExeFS. (v2, later: read the NSP directly with
the owner's keys.) All file access goes through `mmk_import::vfs` (the file system natively; the
web build's worker installs its own for the browser's picked files and storage, §5b).

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
   skeleton, skeletal / texture-pattern / SRT animations. The 2D styles get `sprites.json`
   (sprite quad + UVs per texture, texture-pattern frame keys; format notes in
   [formats/bfres.md](formats/bfres.md)). NSMBU and 3D World also get `meshes/<Model>.mmesh`
   (bind-pose vertices, skeleton, materials, LOD 0) and `anims/<Model>/<anim>.mskel`
   (skeletal + bone visibility + texture-pattern tracks merged by name), see
   `mmk_assets::mesh`.
7. **Audio**: the gameplay sound database from the game's sound links (`SLink2DB`: per
   object and style, event keys → containers over waves; plus the sound group tree, distance
   attenuation sets, per-theme effects, ground materials) → `audio/sound.json`; every wave it
   names copied unchanged (BWAV, DSP-ADPCM + loop points) to `audio/se/` and `audio/music/`
   (`import::sound`). The player decodes effects per course and streams music
   (`mmk_audio::{links, mixer, stream}`, `host::sound`). `audio/index.json` keeps the fixed
   SMB1 set for caches made before the database.
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

### SMM2 UI import (`mmk import-smm2-ui <romfs> <out> [--exefs <dir>] [--language USen]`)
Input: the owner's extracted RomFS (update applied). Output `<assets>/smm2-ui-<build>/`, the same
form as the SMM1 UI import (so `ui` and the app load either), plus `courses/` (the 10 title
courses as shipped):

```
manifest.json                         game "SMM2", build, layouts, fonts (+ baked-font coverage)
ui/layouts/<Layout>.json              BFLYT 9.0 + its BFLAN animations + texture list
ui/textures/<Layout>/<name>.mtex      every texture of the archive's timg/__Combined.bntx
ui/fonts/<Font>.json, <Font>/sheet_n.mtex   bitmap fonts (BFFNT 4.1, BNTX sheets) and the
                                      scalable-font combinations rasterised (smm2_font)
ui/messages/<LayoutMsg|CommonMsg|ReferenceMsg>/<Table>.json   (ReferenceMsg: GoalCondition,
                                      PartsIconName, … shared tables)
ui/textures/GameSkin/<style>_Lyt_Logo_00.mtex   the styles' logos (`Model/<style>_Lyt_Logo`)
ui/sound/sounds.json, <sound>.wav     menu sounds: per layout its SLink user, button sound types
                                      and animations with sounds (`Pack/Static.pack`
                                      `Param/Layout/*.byml`), per user event key → sound /
                                      volume / delay (`SLink2/SLink2DB.szs`, `formats::xlink`),
                                      the sounds as 16-bit WAV (`import::smm2_ui_sound`)
ui/textures/MissionIcon/<style>_Lyt_M_<name>.mtex   clear-condition icons of the five styles
                                      (`Model/<style>_Lyt_Mission_RGBA8`, BFRES textures)
courses/title_course_data_<n>.bcd
```
Switch bitmap-font sheets are stored bottom row first (flipped at import, like the Wii U ones);
one-channel sheets are written as luminance + alpha.
Layout archives: `Layout/<Name>.Nin_NX_NVN.szs` (and `Layout/*.szs` in `Pack/Layout*.pack` when
not loose). Fonts: a layout names a combination (`MessageFont.fcpx`); `formats::bfcpx` reads it
(font files, character ranges, scale, baseline shift) and unmasks the BFTTF/BFOTF files of
`Font/ScalableFont.szs`; `smm2_font::bake` rasterises every character of the language's
messages (+ ASCII, Latin-1) at the combination's size with `ab_glyph` into the grid of glyph
cells the player draws (line height 1.5 × size [inf]); missing console fonts: a stand-in for the
system gothic, drawn controller button icons. DECISIONS.md 2026-10-18 "SMM2 UI".

### Layout player (`ui` crate)
`UiCache` loads layouts on demand (with their part layouts, textures, fonts and messages);
`LayoutPlayer` holds bound animations (per part instance too) and text overrides. Each draw
starts from the base pane/material values, applies the bound curves (pane SRT/size, visibility,
vertex colours + pane alpha, material colours, texture SRT, texture pattern; hermite and step
keys), walks the pane tree parents-first (transform `T·Rz·Ry·Rx·S`, children placed at the
parent's origin point unless anchor bits say otherwise; an "influenced alpha" pane fades its whole
subtree) and emits quads for pictures, window frames (1/2/4/8-frame kinds) and text glyphs (BFFNT
metrics, text position and line alignment, shadow pass, per-character fallback font).
Deterministic; unit-tested on synthetic layouts. Message labels resolve in the layout's own MSBT,
and part-property text ids in the enclosing screen's. Part properties also override picture
content (a material of the enclosing layout) and basic values (visibility, translate, size,
scale, rotate, alpha). Callers can force pane visibility, read a pane's animated values and get
message templates with value placeholders (U+FFFC).
SMM2 additions: part texts resolve `<PartPane>-<TextPane>` labels in the enclosing layouts'
tables (outermost first); callers can force a pane's translate / size / scale / alpha, ask
where a pane is on screen (`pane_global`, through parts and clones), and clone a part pane
(`add_clone`: same parent, the source's animated values plus an offset, its own part instance,
e.g. an extra menu button). Windows with flag bit 0 draw every frame with the first frame's
colours; a part property with material usage 1 takes only the enclosing material's colours [inf].
One evaluation pass (`pass`: states → matrices, alpha, visibility) serves drawing and queries.

### SMM2 title screen (app)
`LayoutUI` holds the opened imports (`LayoutUI.smm1` for the 100 Mario screens, `LayoutUI.smm2`
for the title; `SMM1UI` is an alias). With the SMM2 UI import the app starts on the title
(`SMM2Title.swift`): `TitleFlow` runs a `GameSession` on the playable title courses with
scripted input, no HUD, the next course on death / clear / 40 s (`onDemoEnd`, `loadDemoCourse`),
and hands the session a `sceneOverlay` — `TitleScene`, a `UIScene` over `Play_Title_00` and
`Cmn_Cursor_00` — that the session steps at 60 Hz with the menu input and the Renderer draws over
the course. Choices go back to `AppModel` (library, Built-in filter, 100 Mario Challenge); the
library has a Title button. Without a demo course the scene runs in a `UIScreenView`. Captures:
`MMK_UI_SCREEN=title|title-main|title-play`, `MMK_UI_SELECT=<n>`,
`MMK_UI_INPUT=<tick>:<button>,…`, `MMK_TITLE_COURSE=<n>`, `MMK_TITLE=off` skips the title.

### SMM2 in-course screens (app)
`SMM2Course.swift`: `CourseScreens` (a `UIScene`) holds the HUD (`Play_PlayScene_00`, plus
`Play_KeyIcon_00` per key held), the course card (`Play_CourseIn_00` over `Cmn_Fade_00`), pause
(`Cmn_PauseMenu_00`), clear (`Play_CourseClear_00`), give up (`Play_CourseRetire_00`), time up
(`Play_TimeUp_00`), game over (`Play_GameOver_00`) and the selection frame (`Cmn_Cursor_00`).
`AppModel` attaches it to a library course's `GameSession` (`attachSMM2`) when the SMM2 UI import
is there; the session steps it at 60 Hz, feeds it `mmk_engine_hud` / `mmk_engine_play_status` /
`mmk_engine_clear_condition` every frame (`CourseSnapshot`), drives the phases (`.intro`,
`.paused`, `.cleared`, `.retire`, `.gameOver`) from its menu input and the engine's death / clear
events, and waits for a button's Decide animation before acting. The Renderer draws its layers
in the overlay pass and leaves out the drawn HUD strip. `SMM2Condition` maps the engine's clear
condition to the import's icon and `GoalCondition` sentence. Without the import (or with
`MMK_UI_SMM2=off`) the session keeps the drawn HUD and the SwiftUI menus.

### SMM2 front end (app)
Following the game's scene code (private `research/specs/smm2-frontend.md`):
- `SceneFade` (`SceneFade.swift`): one `Cmn_Fade_00` shared by every screen — `run(kind) {
  switch }` plays the kind's Out animation, switches the screen, then its In; both Metal
  renderers draw `SceneFade.shared.layers` last, SwiftUI screens get a black veil.
- `BootScene` (`SMM2Boot.swift`): `Boot_BootScene_00` before the title (`AppModel.Screen.boot`).
- `CourseWorldFlow` (`SMM2CourseWorld.swift`): `CourseWorldScene` (`Nwk_Title_00`) and
  `CourseListScene` (`Nwk_CourseListGeneral_00`: a five-slot ring of `Nwk_CourseData_00` rows,
  three visible) over `WorldCourse`s from the library and the course server; plays courses in a
  `GameSession` with the SMM2 screens and no lives.
- `EndlessMenuScene` / `EndlessMapScene` (`SMM2Endless.swift`): the 100 Mario Challenge in
  `Nwk_EndlessModeMenu_00` + `Nwk_WorldMap_00` (`EndlessBoard` holds the layout and a second
  instance for the start buttons); `ChallengeFlow.presentation` chooses them or SMM1's screens.
- `MenuCursor`, `DebugInput` (`SMM2Menus.swift`): the selection frame and scripted capture
  input shared by these screens; `UISound` (`UISound.swift`): the import's menu sounds.

### 100 Mario screens (app)
`ChallengeScreens.swift` sequences SMM1 layouts in plain Swift classes (`UIScene`: 60 Hz `step`,
`layers` to draw): the menu (tabs over the world map), the map walk + intro card, game over and
the ending; `RunOverlay` holds the in-course layouts (pause, clear, skip countdown, card after a
death) that the game Renderer draws over the course. `UILayout` keeps a small animation timeline
per layout. Hosts only step and draw them, so the same classes can move to the portable shell. The **placeholder pack** uses exactly the same format and is built from
`assets/placeholder/` (original art), so the engine always has something to draw.

## 5. Rendering (Swift, Metal)

- Resources uploaded once from the asset cache (`MTLTexture`, vertex/index buffers). The same
  renderer runs on macOS and iOS (MTKView hosted through `MetalViewRepresentable`); BCn goes to
  the GPU as is where `supportsBCTextureCompression`, else decoded to RGBA8 by the core.
- Per frame: draw items sorted by layer (background → terrain → actors → effects → UI);
  orthographic camera for 2D styles, nearest sampling for M1/M3/MW; light-mask pass for night
  darkness (later).
- Meshes (NSMBU / 3D World, `MeshRenderer.swift`): the snapshot's sprites that have a mesh in
  the cache come out of `mmk_engine_meshes` instead (model → world transform, animation, time,
  the sprite index they go before). Rust poses them (`mmk_mesh_pose`: skinning matrices,
  visibility, texture per shape); the shader skins on the GPU under the same orthographic
  transform as the sprites. Depth groups per layer keep painter's order with the sprites. Model
  viewer: `MMK_MESH_PREVIEW=<mesh>[#model][@anim],<style>:<sim model>:<frame>,…
  [MMK_MESH_YAW/PITCH/TICK/FLIP] [MMK_SCREENSHOT=<png>]` (`MeshPreview.swift`).
- Shaders (MSL): unlit textured (2D styles), skinned + simple lit (NSMBU / 3D World meshes),
  light mask (later), UI.
- UI (`app/Sources/MMKApp/UILayer.swift`): draws `MmkUiQuad` lists — t = combine(tex0, tex1),
  c = mix(black, white, t) · vertex colour; one pipeline per blend mode, one sampler per
  wrap/filter combination; layout units mapped to the viewport (origin centre, y up). Debug view:
  `MMK_UI_LAYOUT=<name> [MMK_UI_ANIM=a,b~,c@30] [MMK_UI_TEXT=pane=text;…]
  [MMK_SCREENSHOT=<png> MMK_SCREENSHOT_TICKS=<n>]` (`UIPreview.swift`).

## 5b. Portable shell (`shell` crate, `mmk-play`)

Plays a course with the Rust core everywhere the Swift app can't go (Windows, Linux, browsers),
and on macOS too. No platform code outside this crate; everything below `shell` is shared.

```
 ┌──────────────── mmk-play (shell) ─────────────────┐
 │ app: winit window + loop ── session: 60 Hz fixed  │
 │      step, pause / clear / game over ── input:    │
 │      keys + gilrs pads → Input                    │
 │ render: wgpu (Metal/Vulkan/DX12/GL/WebGPU)        │
 │ hud: built-in 5×7 font   audio_out: cpal stream   │
 │ source: AssetSource (dir / memory; web: OPFS)     │
 └───────────────▲───────────────┬───────────────────┘
                 │ Rust calls    │
   host (place, resolver, clear colour, director, course bytes)
   sim · assets · audio · formats
```

- **Loop** (`app`, `session`): one redraw per display frame (presenting waits for vsync); each
  redraw adds the host time to an accumulator and runs whole 60 Hz ticks (at most 0.25 s of
  catch-up), like `GameSession.frame` in the Swift app. Start (Esc/P/Enter, pad Start) pauses;
  R restarts; clear and game over wait for jump/start. A hidden (occluded) window keeps ticking
  but draws nothing.
- **Renderer** (`render`): port of `Renderer.swift`. Letterboxed 384×216 view at a whole-number
  scale; sky = theme clear colour (`host::clear_color`); sprites from `host::SpriteResolver`
  (texture index + BFRES quad placement + flips, sorted by layer), snapped to whole pixels, one
  draw per run of sprites sharing a texture, nearest sampling, alpha blend + discard below 0.01;
  upside-down view for underground night; HUD image on top; placeholder boxes by layer when the
  art is missing (placeholder mode). Output is pixel-identical to the Swift app's `MMK_SCREENSHOT`
  frames below the HUD strip (checked on a ground course and an underground-night area).
- **Textures** (`texture`, `bc7`): RGBA8 as is; BC1–7 sampled by the GPU when the adapter has
  `TEXTURE_COMPRESSION_BC` (desktop), otherwise decoded on the CPU (`formats::bcn` for BC1–5,
  `shell::bc7` for BC7; BC6H unsupported). Channel selects are applied on the CPU (WebGPU has no
  view swizzle). `mmk-play --check-textures` compares GPU sampling with the CPU decoders for every
  BC texture of a cache (BC4/5/7 exact, BC1–3 within 3 steps of interpolation rounding).
- **Input** (`input`): physical key codes (arrows/WASD, Z/Space jump, X/Shift run) and gilrs
  gamepads by button position (south/east jump, west/north run, d-pad or left stick); in a
  browser gilrs reads the Gamepad API (standard mapping).
- **Audio** (`audio_out`): cpal default output (Core Audio, WASAPI, ALSA; WebAudio in a browser);
  the mixer (`host::audio::AudioOut`) renders at the device's rate on the callback thread, fed by
  the session's `Director`, with the sound database and its waves read from `audio/`
  (`source::music_reader`; the web build prefetches a course's files, `host::audio::
  course_files`, see below).
- **Assets** (`source`): every cache read goes through `AssetSource` (`DirSource` on disk,
  `MemorySource` for files handed over by the browser); `assets::Cache::load` and
  `audio::Bank::load_cache_with` take a read function for this. `Cache::load_deferred` reads only
  the index: `texture(i)` records a request, the host reads the file when it can and hands it
  over (`take_requests` / `provide`), and the renderer keeps the texture pending meanwhile.
- **Web** (`web`, `web_import`, wasm32; page: `web/index.html`, `mmk-web.js`,
  `mmk-import-worker.js`): the same app in `<canvas id="mmk">`; WebGPU, or WebGL2 where WebGPU
  is missing (`wgpu::util::new_instance_with_webgpu_detection`); graphics start asynchronously
  and arrive as a winit user event. The page talks to the running app through exported functions
  that send it user events (`Game`: play this; `Sound`: a mixer).
  - *Game data* lives in the origin private file system (OPFS, `mmk-data/`, about 1.3 GB),
    never uploaded. The owner picks a folder (`<input webkitdirectory>`): an asset cache made
    by `mmk import` is copied file by file (16 writers, the manifest last as the completion
    mark; `meshes/` and `anims/` skipped, the shell draws no meshes yet); a folder with
    `romfs/` + `exefs/main` is imported by `mmk-import` itself in a module worker: it reads the
    picked files synchronously (`FileReaderSync`) through `mmk_import::vfs` and posts each
    output file to the page, which stores it (workers can't wait for storage).
  - *Reading it back*: OPFS reads are asynchronous, the game is not. `open_assets` reads the
    manifest, texture index, sprites, audio index + sounds and key table (`mmkStorage.read`),
    opens the cache deferred and the bank; each frame `pump_textures` starts the reads the
    renderer asked for and holds the session (at most 30 frames) while they're on their way;
    `play_course` decodes the course (`.bcd` with the key table) and reads its music before
    the mixer needs it. Memory holds what is being played, not the cache.
  - *Sound* opens on the first pointer or key event (`unlock_audio`; browsers start WebAudio
    only after a gesture); cpal's WebAudio backend runs the callback on the main thread.
  - *Courses*: the cache's built-in courses (`courses/`) in a list, or any course file.
  - *SMM2 menus* (`open_front`, host `web_front`): when OPFS `mmk-ui/` holds an SMM2 UI import
    (copied from a picked folder with a `files.txt` list, or made by the dump import's worker
    via `import_ui`, its files posted as `@ui/<path>`), the page starts `mmk-front` through the
    same `FrontApp` as native. `mmk_ui::UiCache::from_files(MemFiles)`: all JSON read up front,
    textures `Fetch::Later` → `take_requests` / `provide` from `pump_ui` each frame (the front
    end holds ≤ 30 frames for them; `UiLayer` skips quads whose textures are still missing and
    re-uploads them on `revision` changes); UI WAVs read up front as bytes (`UiMixer`'s clip
    reader), mixed into the same WebAudio stream as the game. Library: built-in courses decoded
    in the background + opened files; `localStorage` for saves; course music read when the
    front end loads a course. Page-opened courses play through the front end (`play_course`).
  Build: `scripts/build-web.sh` (wasm-bindgen CLI matching `Cargo.lock`), serve the folder.
  Checked in headless Edge (CDP; WebGPU, BC on the GPU) and Firefox (WebDriver BiDi).
- **Smoke tests:** `mmk-play <course> --screenshot out.png --ticks N [--scale S] [--idle]
  [--cpu-textures]` runs N ticks of the Swift app's scripted input (run right, jump every 90
  ticks) and renders one frame offscreen to PNG; `--quit-after S [--scripted]` runs the real
  window for S seconds and reports frames and ticks.

## 6. Testing

| Layer | How |
|---|---|
| formats | Rust unit tests; **parity tests with the Python `smmfmt`** on synthetic fixtures (committed) and on the owner's corpus (local, skipped in CI) — byte-exact round trips and identical parsed values |
| import | fixtures built from synthetic containers; full import tested locally |
| sim | spec tests (e.g. held-jump peak 67.6 px standing / 86.3 px at full run; acceleration curves); replay golden traces; property tests (determinism: same input → identical state hash) |
| ffi | smoke tests from Rust and from a Swift test target |
| app | Swift build in CI; manual play testing against Ryujinx |
| guard | `scripts/check_no_proprietary.py` (unchanged) |

CI: `cargo test` on Linux and macOS, Swift build on macOS, guard; the portable shell is built,
clippy-checked and tested on Windows and Ubuntu (binaries uploaded), and its wasm32 build is
checked on Ubuntu.

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
| winit 0.30 | shell: window, event loop (native + web) | Apache-2.0 |
| wgpu 30 | shell: renderer (Metal, Vulkan, DX12, GL, WebGPU) | MIT/Apache-2.0 |
| gilrs 0.11 | shell: gamepads | MIT/Apache-2.0 |
| cpal 0.18 | shell: audio output (native; WebAudio backend in the browser) | Apache-2.0 |
| png, pollster, bytemuck | shell: screenshots, blocking on GPU setup, vertex bytes | MIT/Apache-2.0 (bytemuck also Zlib) |
| ab_glyph 0.2.32 (+ ttf-parser) | import: rasterise SMM2's scalable fonts into bitmap fonts | Apache-2.0 (ttf-parser MIT/Apache-2.0) |
| wasm-bindgen, web-sys, js-sys, wasm-bindgen-futures, console_error_panic_hook | shell: web build | MIT/Apache-2.0 |

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
