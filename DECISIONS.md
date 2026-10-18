# Decisions

Newest first. Each entry: what was decided, why, and what would change it.

## 2026-10-18 — iOS packaging: one Swift package, cross-compiled, bundled by a script
The iOS / iPadOS app is the macOS Swift package built for iOS, not a second project.
- **Build:** `swift build -c release --triple arm64-apple-ios17.0[-simulator] --sdk <sdk>`. The
  Rust core is a plain static library per target (`cargo build -p mmk-ffi --target
  aarch64-apple-ios` / `aarch64-apple-ios-sim`), passed with `-Xlinker -L <dir>`. SwiftPM
  conditions can't tell the simulator from a device, so `Package.swift` keeps its link path
  for macOS only. `scripts/make-ios-app.sh [--device]` builds both halves and writes the
  `.app` (Info.plist, icon, ad-hoc signature for the Simulator) like `make-app.sh` does on
  macOS. `scripts/check.sh --ios` runs it for the Simulator.
- **Rejected:** an XCFramework + `.binaryTarget` (it would change the macOS build and needs the
  framework rebuilt before every `swift build`); an `.xcodeproj` (hand-kept project files,
  nothing to generate them without new tools); the `AppleProductTypes` `.iOSApplication`
  product (Xcode / Playgrounds only, breaks `swift build`).
- **Signing is the owner's:** device builds are signed only when `MMK_IOS_SIGN_IDENTITY` and
  `MMK_IOS_PROFILE` are set (the profile's entitlements are applied), otherwise left unsigned.
- **Data on iOS:** the app's Documents folder holds what Application Support/MMK holds on
  macOS, and the Files app shows it. The importer stays on the Mac; the owner copies the cache
  folder in, or picks it anywhere and the app copies it (copying is simpler and safer than
  keeping security-scoped bookmarks to files the core reads lazily). In the Simulator the
  development caches are found through the Mac's home (`SIMULATOR_HOST_HOME`).
- **BC textures:** iPhones and iPads before M1 can't sample BCn. There the core decodes to
  RGBA8 (`mmk_texture_decode_rgba`, shared with the portable shell, whose BC7 decoder moved to
  `formats`).

What would change it: needing App Store distribution or extensions (then an Xcode project), or
SwiftPM gaining a simulator/device condition (then the link path moves back into the manifest).

## 2026-10-18 — 3D styles: meshes in the cache, sim sprites become mesh draws
NSMBU (WU) and 3D World (3W) are drawn from their BFRES meshes. What the data showed first:
- **NSMBU** enemies, items and objects are pre-rendered frames on a quad (`walk_Alb.000` …,
  switched by texture-pattern animations), like the 2D styles but at a finer frame rate. Its
  players (body + head models), Yoshi and the bosses are skinned meshes. Terrain is the
  `WU_Field_*` atlas, drawn as tiles as before.
- **3D World** is meshes throughout. Its terrain is one model per field-atlas cell
  (`<theme>_model_<atlas index in hex>`, the same index the sim already computes for the 2D
  atlases), with neighbour variants (`_ff`, `_fs`, `_l_f`, …) for slopes and edges. Blocks,
  coins and pipes live in `Pack/StaticModel.pack`, which the importer now reads for WU_/3W_.

Decisions:
- **Cache format:** `meshes/<Model>.mmesh` holds every model of a game file: skeleton,
  materials (albedo texture, wrap, unlit flag), shapes, LOD 0 only. Vertices are baked into the
  bind pose in model space, so rigid and single-bone shapes skin like smooth ones. Each
  animation is its own `anims/<Model>/<anim>.mskel`, plus an `index.json` per folder. It merges
  the skeletal, bone visibility and texture-pattern tracks that share a base name (`walk` +
  `walk_ftp`). Curves are kept as decoded keys and evaluated at run time, not baked per frame.
  Both files are a JSON header plus a binary blob (`mmk_assets::mesh`). Importer version 5.
- **Who poses:** Rust (`MeshModel::pose`, through `mmk_mesh_pose`) computes the skinning matrices,
  bone visibility and the texture of each shape. Metal skins on the GPU. Only the 3D styles use
  this path. `mmk_engine_set_meshes(false)` / `MMK_NO_MESH=1` turns it off.
- **Mapping a sim sprite to a mesh** (`ffi/src/mesh.rs`):
  - Model: `M1_<rest>` → `<style>_<rest>`. A model of its own for the frame wins
    (`3W_Enemy_nokonokoA_shell` for `shell.*`). In a multi-model file, the model named after the
    frame stem wins, then the one named after the file, never a `*break*` debris model.
  - Players (table): NSMBU `WU_Player_MarioMdl`, `SMB_model` + `SMH_model` for small, `MB_model`
    + `MH_model` otherwise. "S" = small is inferred from the sizes [inf]. The head rides the
    body's `face_1` bone [inf]. 3D World uses `3W_Player_Mario` / `…MarioMini` with the Face,
    Eye, HandL and HandR files on the body's `Head` / `HandL` / `HandR` bones. Fire forms use the
    `.1` textures.
  - Blocks (3W): field-atlas index → `3W_Object_block_*` (brick, ?, used, hard, donut, cloud,
    ice). Other 3W tiles take the terrain model for their atlas index. The first variant is used
    for edge and slope cells [inf]. Cells without a model (pipes, note blocks, …) fall back to
    the NSMBU atlas tile.
  - Animation: the frame stem (`walk.1` → `walk`), matched case-insensitively, then a synonym
    table (`fall` → `jump2` / `FallLoop`, `shell` → `Move`, `dead` → `DieOver`, …), then
    `wait`, else the rest pose. Players also try their shared animation files
    (`WU_Player_CommonAnm`, `3W_Player_Animation_*`). Small NSMBU Mario tries `SMB_`-prefixed
    names first; the NSMBU head uses `PH_` texture animations.
  - Time = the world tick [inf: the sim keeps no per-actor animation clock], so all actors
    animate in phase and one-shot animations start mid-way.
  - Placement: origin = the sim's model origin (x + w/2, y). Scale = sim width ÷ the SMB1
    texture width. Enemies and players turn 70° from the camera toward their walking direction
    [inf: chosen so the face shows; not measured]. Pre-rendered NSMBU cards are mirrored like
    the 2D sprites; their art is taken to face left like SMB1 [inf].
  - Bones the animation doesn't mention keep their rest visibility, except alternative parts.
    For `<name>NN` siblings only `…00` shows (3D World faces, hands, eyelids) [inf].
- **Rendering:** orthographic side view sharing the sprites' world → NDC transform. A depth
  buffer is used per depth group (consecutive mesh draws of one layer), and every later group is
  in front of all earlier ones. Meshes keep real depth among themselves inside a layer, and
  painter's order with sprites between layers. Lighting is one directional light plus ambient
  (our choice). Cards are unlit and alpha-tested. 3D albedo alpha is ignored, since the render
  state isn't read [inf]. Backgrounds (`_DV_`) aren't meshed yet.
- **Debug:** `MMK_MESH_PREVIEW=<mesh>[#model][@anim],<style>:<sim model>:<frame>,…` model viewer
  with `MMK_SCREENSHOT` (mirrors `MMK_UI_LAYOUT`); `mmk bfres <file> [--member] [--curves]`.

What would change it: the game's per-actor animation state and blend rules, the camera angle
and actor turn from Ryujinx captures, material render info (translucency, emission), and the
3D World neighbour-variant rules.
## 2026-10-18 — SMM2 UI: same cache form as SMM1, scalable fonts rasterised at import
- **One cache form for both games.** `mmk import-smm2-ui` writes `<out>/manifest.json`
  (`"game": "SMM2"`) and `ui/{layouts,textures,fonts,messages}` exactly like the SMM1 UI import,
  so `UiCache`, the FFI and the app load either; messages sit flat in `messages/<archive>/`
  (SMM2 has no groups) and the reader takes both. Location: `<assets root>/smm2-ui-<build>/`
  (development copy `~/smm-work/cache/smm2-ui-3.0.3/`), `AppPaths.findSMM2UICache()`
  (`MMK_UI2_CACHE` first); the asset-cache search skips `smm2-ui-*`.
- **Text: TrueType/OpenType → bitmap fonts at import.** SMM2's text fonts are scalable
  (BFTTF/BFOTF behind BFCPX combinations). The importer rasterises, once, every character the
  chosen language's messages use (plus ASCII and Latin-1) at each combination's authored size
  into the BFFNT-like grid the player already draws, with `ab_glyph` 0.2.32 — already pinned in
  Cargo.lock (winit uses it on Linux), built offline, licence Apache-2.0 (ttf-parser MIT/Apache).
  The player stays a bitmap-font player; no font code at run time. Would change for user text
  in scripts the import doesn't cover (Japanese course names): bake more, or rasterise at run time.
- **Metrics we chose [inf]:** line height 1.5 × size, character width = size (SMM2's text panes are
  authored at 1.5–1.57 height:width, which then scales glyphs evenly); baseline centring the main
  Latin font's em box; later combination entries win (makes every SMM2 entry reachable); the
  console's own fonts are missing: UD Kakugo stands in for the system UD gothic, and the
  controller button icons (U+E0E0… A/B/X/Y/L/R/ZL/ZR/SL/SR/+/−) are drawn as shapes with
  letters (`smm2_font::button_icon`, community code-point table [inf]). No glyph borders yet.
- **The title courses go into the UI import too** (`courses/title_course_data_*.bcd`, encrypted
  as shipped), so the title has its demo even next to an asset cache without built-in courses.

## 2026-10-18 — SMM2 title screen: the game's layouts, inferred choreography, two departures
- **Layouts and their animations are the game's** (`Play_Title_00`, `Cmn_ClapperboardBtn_00`,
  `Cmn_MainMenuBtn_00`, `Cmn_Cursor_00`); states and transitions are the title layout's own
  animations (PressKey / DecideBackBtnTop, DecidePlayBtn / DecideBackBtn) [obs]. Inferred:
  the screen starts on the "Press L + R" prompt; any menu button brings Make / Play; the cursor
  is `Cmn_Cursor_00` sized to the selected button's `N_Carsor_00` / `B_Btn_00`; logo letters
  white over a dark sky (`Type` frame 1), black otherwise; 24 ticks of the button's Decide before
  the next screen. Details in the private `research/specs/smm2-layouts.md`.
- **Behind it**: the title courses the engine can play (NSMBU / 3D World ones need the mesh
  renderer), one after another, with a scripted demo (stand 1 s, walk right, jump every 2 s;
  next course on death, clear or after 40 s). In the game the player moves Mario there.
- **Departures:** a fourth Play-menu button "100 Mario Challenge" (a copy of the Coursebot part,
  castle icon) for the SMM1-style challenge, with the list drawn at 80% so four fit between the
  logo and the copyright line; Make (Course Maker) only claps and says "coming soon". Story Mode
  opens the library on its Built-in filter, Course World and Coursebot the library.
- **Player semantics added [inf]:** window flag bit 0 "one material for all frames" gives every
  frame the first frame's colours and keeps each frame's texture (SMM2 button shadows); a part
  property with material usage 1 takes only the enclosing material's colours (Course Maker icon
  tint). Would change with a frame-by-frame comparison against the game.
## 2026-10-18 — 3D World enemies (96, 65, 51, 50, 114, 117, 86, 107): own modules, one state struct
Ant Trooper, Stingby, Skipsqueak, Hop-Chops, Porcupuffer, Bully, Charvaargh and Piranha Creeper
live in `actors/w3.rs` and `world/w3.rs`, like the bosses: the shared files only gain variants,
match arms and one-line hooks. Their extra state is one `W3State` on the actor, which also holds
what the world senses for them each frame (the liquid, the player's height, wetness and jump).
Choices the specs leave open, all [inf]:
- **Ant Trooper:** walks at 0.5 (the crawler walk isn't decoded). It crawls the contour with a
  square box: walls ahead are climbed, ledges are wrapped at 6°/frame (big 4°), and it only wraps
  fully solid blocks, so it walks off semisolids. A stomp defeats a plain one (the rows say
  ReactOnly; the game defeats it). Winged ones use the generic flight.
- **Stingby:** Turn takes 12 frames and the alert 30. The obs charge speed 0.5 is kept, though it
  looks slow.
- **Skipsqueak:** "player jumping" is read as airborne and rising. The crouch takes 6 frames and
  recovery jumps drift toward home. The third try uses 3.65, then it gives up and stays put.
  Winged ones hover in place.
- **Hop-Chops:** hops are timed by the animation windows (9 ground frames, 2 when chasing). Turn
  takes 16 frames, Lost 30, and the Find hop uses the normal gravity.
- **Porcupuffer:** it swims on any liquid and survives lava. With no liquid it rests on the
  ground. The jump window is centred 40 px above its centre. SwimTurn has a 30-frame cooldown
  and Recover lasts 20 frames.
- **Bully:** it sees 128 px either side and ±48 px. A fireball pushes it at 1.0, a stomp or spin
  at 1.75, and a ground pound or shoe stomp at 2.5 followed by 120 frames of HoldOut. Its touch
  shoves the player (2.0 px/f) without hurting them. Shells bounce off it. It turns at ledges
  only while walking. Winged ones chase for 300 frames, then fly home.
- **Charvaargh:** it isn't a generic enemy, so nothing defeats it and a Star only protects the
  player. It rests with its head 4 px under the surface. The leap is a 64-px half circle at
  2.5 px/f, and each of its 8 segments touches within 32×32 px. Only the head is drawn; the
  model's animation shows the body.
- **Piranha Creeper:** its path node codes were read off the corpus: 1–4 straight, 5–12 corners,
  13–16 end caps, one 2×2 cell per node. It waits 60 frames at the start and at each end. A stomp
  tramples the head (harmless for 10 + 130 frames); fire, shells and Stars defeat it. The stalk
  is harmless and drawn unrotated.

## 2026-10-18 — Bosses (Bowser, Boom Boom / Pom Pom, Bowser Jr.): HP through one hit path
Every boss hit goes through `Actor::boss_hit(attack)`, which applies the spec's HP rules. Bowser:
fire 1, shell 3, 20 HP (big 40), and stomps are Invalid. Boom Boom and Bowser Jr.: 3 HP; a
fireball drains a 5-point counter (big 10) and costs 1 HP when it empties. Star, POW quake and lava
use the generic kill paths. The code is in `actors/bosses.rs` and `world/bosses.rs`, so the shared
files only gain variants, match arms and one-line hooks. Choices the specs leave open, all [inf]:
- **Shared rules:**
  - A stomp on Bowser hurts the player, as on Spike Top (the other "Invalid" stamp). Chain Chomp's
    bounce is the exception there.
  - A Bob-omb blast is a Shell-strength hit (Bowser 3, others 1). A shell that hits a boss is spent
    (DieFall).
  - Boom Boom and Bowser Jr. are immune in DamageDown/Defend and in DamageDown/Shell. Stomps then
    bounce, and contact from above never hurts.
  - A held Key (child 95) appears at the boss's centre on defeat.
  - Clown Car riders (flag 0x200; the Clown context isn't decoded) hover at their spawn height,
    ease toward the player and only flash when hit. In the car, Bowser keeps his fire timer and
    Bowser Jr. fires, throws hammers and drops shells every 120 frames. Boom Boom doesn't attack.
- **Bowser:**
  - He walks along a patrol direction while turning to face the player, so he can walk backwards.
  - The middle jump always happens (the target-height search isn't decoded). Turn and LandOn
    last 8 frames, the breath 30, and the flame leaves at its end (SMB1).
  - Flames fly at the 1.5 speed field and settle at 0.5 px/f onto one of four lanes, 0–24 px above
    his feet, picked from 3 random bits.
  - The Fly hover uses gravity −0.05 while breathing. The breath pauses during a big jump.
  - The SMB3 big jump aims to reach the player's x by the apex. Other styles' big jump is
    0–1 px/f toward the player. HitBlock lasts 16 frames.
  - Not modelled: the hip-drop quake and block breaking, the ×1.5 timers in water, and the
    helper's "mode" 1–3.
- **Boom Boom:**
  - Jump 5.0 and WallJump 3.75, read from the spec's state section; the table's "big 3.75" stays
    open.
  - Charge 60 frames. Defend ends after 3 hops and at least 30 frames, at a top speed of 2.0. Then
    60 frames of slow walk.
  - Winged: cruise speed 1.0, holding the spawn height. The swoop accelerates 0.1 down for the
    first 30 frames, then up.
- **Pom Pom** only in 3D World (the loader rule). Wait 60, then alternately a jump (4.0 / 1.5) or 3
  aimed shuriken (2 px/f). After a hit she vanishes for 60 frames and reappears 64 px from the
  player. No decoys. M1 placeholder art.
- **Bowser Jr.:**
  - The Wait choice table: near (≤ 80 px) shift-jump 6/16, hop 4/16, walk; far fire 5/16, hammer
    2/16, shift-jump 2/16, walk.
  - Angry walk (1.25) at 1 HP. Hop 2.5, shift jump 5.0 with 1 px/f toward the player. The hover
    drifts over the player (≤ 1.5).
  - After a hip drop he paces around the landing point.
  - His fire is a Bowser flame aimed at the player's height (±32 px); the hammer is the Hammer
    Bro's.
  - Shell: slides at 2.0 for 150 frames, then 20 frames to come out.

What would change this: Ryujinx measurements, or decoding Bowser's target-height search, the Bowser
Jr. Wait table, the Clown contexts and the Pom Pom constants.

## 2026-10-18 — SMM1 UI: JSON + `.mtex` cache, layout player in its own crate
- **Cache form:** `mmk import-smm1` writes each layout as JSON (parsed BFLYT + its BFLAN
  animations + texture list), textures as untiled `.mtex` (same format as the SMM2 cache, so the
  Metal upload code is shared), fonts as JSON metrics + sheet `.mtex`, messages as JSON. JSON
  keeps the cache endian-neutral and inspectable, and the engine never needs the original files;
  `formats::lyt::json` round-trips every parsed field (tested). Written by hand on
  `serde_json::Value` — no serde-derive dependency added.
- **Location:** `<assets root>/smm1-v<update title_version>/` (development copy
  `~/smm-work/cache/smm1-v208/`), separate from the SMM2 cache because it is loaded lazily per
  layout (the SMM2 `Cache` loads every texture at open).
- **Everything, not a subset:** all 337 layout archives are imported (67 MB): the app menus will
  reuse them and the cost is small.
- **Fonts the game does not ship:** layouts that name the Wii U system fonts (`CafeStd_25/50`)
  get MessageFont; every font falls back per character to SpecialFont (7380 glyphs, kana/kanji),
  so user course names in Japanese render.
- **Crate split:** parsers in `formats`, import in `import`, evaluation in a new `ui` crate (no
  file I/O in the player itself; tests build layouts in memory), C ABI in `ffi/src/ui.rs`, Swift
  in `UILayer.swift` / `UIPreview.swift` — kept out of the files other work is changing.

## 2026-10-18 — Layout player semantics where the format leaves room (verify against the game)
Chosen from how the SMM1 data is authored and checked visually on the owner's dump:
- Pane-based texture projection maps the pane's origin point to the texture centre
  (uv = 0.5 + (x/w, −y/h)); the texture SRT translation is added. The world-map Goombas are only
  right this way.
- Parent anchor offset = (anchor factor − parent origin factor) × parent size (13 panes in the
  whole game use a non-centre anchor).
- Two-texture materials: TEV mode 0 takes tex1, 1 multiplies, 2 adds, 5 subtracts; no stage =
  multiply. Third textures are ignored.
- Colours: material and vertex colours are treated as authored sRGB values (linearised in the
  shader, sRGB target); BC4/BC5 masks are linear factors.
- Text ids resolve in the layout's own MSBT; part-property text ids resolve in the outermost
  (screen) layout's MSBT first — the difficulty buttons only get their labels this way.
- Window flag bit 4 = content not drawn (cursor frames are hollow).
Would change with a frame-by-frame comparison in an emulator.

## 2026-10-18 — App shell: working name "MMK", storage locations, shell-owned pause
- **Name:** the app is called "MMK" (bundle id `dev.mmk.engine`) until the owner picks a name. No
  Nintendo names or marks in the app name, icon or bundle id. Name, bundle id and version live
  only in `app/Sources/MMKApp/Identity.swift`; `scripts/make-app.sh` reads them from there and the
  Application Support folder is named after it, so renaming is a one-line change (existing data
  would then need moving from the old folder).
- **Storage:** `~/Library/Application Support/MMK/` holds `assets/<build>/` (importer output),
  `courses/` (the library; added files are copied in, so the library survives the originals
  moving) and `saves.json`. The older `…/mmk/assets` and `~/smm-work/cache` locations are still
  searched for an asset cache. Settings are UserDefaults keys (README "Settings").
- **Save key:** FNV-1a 64 of the decoded course data, so a course keeps its record whether it was
  added as `.bcd`, zlib or raw data, and wherever the file lives. Not a security hash; collisions
  only merge two records.
- **Pause is the shell's job:** the start/pause button no longer reaches the engine; the shell stops
  stepping it. Course clear = HUD flag bit 0 (set when the goal sequence ends); clear time runs
  from the attempt's start (course start, death restart or menu restart) to the Goal event.
  Game over at 0 lives is an app rule for now (SMM2 normal play has no lives); revisit for Phase 6.
- **Placeholder art:** without a dump the engine still runs; sprites whose texture isn't in the
  cache come out of the snapshot as `MMK_TEXTURE_NONE` and the renderer draws a box coloured by
  draw layer (only in placeholder mode or with `MMK_DEBUG_MISSING_ART=1`, so a real import
  with a missing texture looks as before).
- **`mmk_course_info` takes an assets handle** (may be NULL) in addition to the bytes: encrypted
  `.bcd` files need the key table from the asset cache.

## 2026-10-18 — Audio: game BWAVs in the cache, decoded at load, mixed in Rust (Phase 5 step 4)
- The importer copies each needed sound's BWAV unchanged into `<cache>/audio/` (outside the
  repo) with an `index.json`; the engine decodes DSP-ADPCM/PCM16 to PCM16 once when the audio
  handle opens (≈30 MB for the slice, mostly the two ground music streams). Simpler than decoding
  in the render path and loop points need no ADPCM history bookkeeping. Revisit (stream from
  ADPCM) when more themes' music is loaded at once.
- Engine sound ids → game wave names live in a code table (`import::audio::M1_SOUNDS`), with the
  game's per-sound volumes as gains (numbers, like the sim's physics constants). The mapping comes
  from the game's own sound-link data; evidence is in the private research notes.
- Mixer: 32 voices, 32.32 fixed-point positions, linear interpolation to 48 kHz, short
  anti-click fade on cuts, three exclusive groups modelled on the NES pulse 1 / pulse 2 / noise
  channels (a new sound in a group cuts the previous one), same sound retriggers instead of
  stacking, fixed ×2 output gain (the game's waves peak around 0.2 FS). The group behaviour and
  the kick sound's two takes alternating are inferred, not read from the game.
- Threads: the engine (game thread) only sends commands over a bounded `std::sync::mpsc` channel;
  the mixer is owned by the audio handle and locked only by `mmk_audio_render`. No new crates.
- The sim now emits `PowerUp` for every mushroom/flower pickup (the game plays the sound even
  when the form does not change).
- Only the M1 ground (day) music is imported; other themes play it too until their streams are
  added. `MMK_MUTE=1` disables audio in the app.

## 2026-10-18 — Camera choices where the spec is inferred (Phase 5 step 5)
The follow rules and constants come from the binary as-is (dead zone ±4 px; box −28/+40 px around
home, shrinking to ±16 while rising; 4 px vertical hysteresis; follower caps 5–8.5 px/frame
horizontally, 4.99 up / 7.99 down vertically; 216 px ceiling; 60-frame scroll-stop hold, 4 px/frame
release). Checked identical in v3.0.3. Inferred until the Ryujinx harness can measure them:
- Header boundary type ≠ 0 means "nothing built above the line" and gives the 216 px ceiling
  (strong corpus correlation with build height; blank courses have 1).
- The box-shrink curve is the identity (Bézier controls 0, 1/3, 2/3, 1); the multiplayer join
  weight is 1 for one player.
- The engine has no separate Fall state: leaving the ground with vy ≤ 0 counts as a walk-off (no
  hysteresis) until an upward impulse; jumps use the normal mode, as the game's Jump state does.
- Scroll-stop walls count any solid cell (ground and blocks); semisolids and slopes do not. Which
  block types the game counts is unverified.
- The descend-only start strategy is on at course start (the game enables it in a play mode that
  was not identified; it only matters in low-camera main areas).
- Death freezes the view (camera mode 3 → the empty cDemo state); the goal keeps following with
  the goal strategy (5 px/frame vertical cap).
- Player velocity for the followers is the player's vx/vy (the game adds a carrying platform's speed).

## 2026-10-18 — 2D sprites are placed by their BFRES quads; the sim passes model origins (Phase 5 step 3)
The importer reads each M1 model's BFRES (own minimal reader for version 0.9: skeleton rest pose,
materials and sampler wraps, shape vertices, texture-pattern animations) and writes
`sprites.json`: per texture the quad in model space (1 unit = 1 px, origin = actor origin) and
its texture coordinates; per animation the frame keys. Convention: for model sprites the sim's
(x + w/2, y) is the model origin and (w, h) the texture size times the scale; the FFI snapshot
draws the texture at origin + quad (mirrored around the origin for flip_x, flipped in place for
flip_y). The sim stays asset-free and no longer carries art offsets. Without quad data (old
caches) the texture fills the sim's rectangle as before.
Consequences taken from the data as-is: enemies sit 1 px into the ground (Goomba art offset and
Koopa bone offset agree; Mario does not), the goal flag is mirrored (mirror-repeat sampler),
the fireball is drawn 16×16 around its centre, the castle flag rests on top of the tower.
Texture-to-quad matching when several models share a material name (Koopa walk vs shell): the
quad of the texture's own size wins. Frame timings are exposed (`Cache::anim`); the sim uses them
only for the Goomba/Koopa walk (10 frames per texture at play rate 1.0 from EnemyModelParams).
What would change it: Ryujinx captures showing different placement, or code that offsets models
or scales animation rates (player walk-rate scaling is not known yet).

## 2026-10-18 — Core-object choices where the specs have no number (Phase 5 step 6)
Values read from the binary are used as-is (bump curve, coin pop, item emergence, walker speeds,
shell wake 511, kick 2.5 + 0.5·|v|, death 45-frame pose → vy 3.2 / g −0.16 / restart after 140,
damage Fire → Super → Small with 127 frames of invincibility, goal height score and slide speeds).
Where nothing was found, these stand-ins are used until the Ryujinx comparison harness can measure
them:
- Enemy stomp bounce: the player-vs-player pair 4.128 (jump held) / 3.628.
- Stomp test: player falling with the feet in the enemy's upper half.
- Grow/shrink freeze: 48 frames.
- Off-screen actors are removed when their activation box (32 px half-extents; sliding shells
  160 × 32) leaves the view + 32 px, and respawn when their home position re-enters the view;
  killed actors stay dead.
- Shell kick scores 400; stomp/shell chains use the score-type table 100 … 8000, then 1-UP.
- Goal pole stands in the goal ground's second column (X0 = left + 24): with the castle door at
  X0 + 112 the 80-px castle then ends flush with the goal ground's right edge.
- Pole touch uses body box B (a small player on the base block only touches the box with box A).
- Lifts placed off a track are static (the binary has an oscillating Move state, ±48 px, but
  which lift actor the course loader picks was not traced; course makers use plain lifts as
  fixed platforms). Verify in Ryujinx.
- Spring launch: 6.0 px/frame, or 4.43 with jump held (the held-jump gravity bands then give
  the high bounce) — the binary pairs the speeds with "jump profile" ids whose meaning is [inf].
- Side Thwomp charge trigger: player within 2 × its half-width vertically and 256 px horizontally
  [inf]; the normal Thwomp uses the spec's |dx| < W + 32 zone.
- P-switch duration 600 frames ([ext] ~10 s; not found in the binary). Dotted-line block colour
  from the alternate flag [?]. Conveyor art uses a placeholder tile until the belt model is found.
- Simplified for now: Spike Top walks floors only (turns at ledges) instead of crawling around
  blocks; Hammer Bro alternates a 5.5 jump with a 2.5 hop (no drop-through); Bob-omb blast radius
  1 tile (plus) for terrain and 32 px for actors/player [inf].
- Fire Bar rotation 2.8125°/frame (the shared counter's step was not located); Burner flame
  12 px wide × 48 px (texture length); one-way walls are drawn nowhere yet (no M1 texture found).
- Seesaw rider weight = 1 per px of lever arm; Grinder hurt radius 20 px; cannon diagonal
  facings (odd values of (flags >> 24) & 7) mapped as 1 down-left, 3 down-right, 5 up-left,
  7 up-right [inf]. Sprites don't rotate yet: seesaw drawn as stepped tiles, cannon barrels not
  drawn.
- Swinging claw pendulum: amplitude 60°, period 160 frames, rope 40 px (the rope simulation is
  outside the decompiled range) [inf]; Twister lift: vy toward 4.5 at +0.6/frame [inf].
- Sprite art offsets inside textures are measured from the textures (e.g. the Goomba sits in the
  top half of its 16×32 image) until BFRES quad placement is imported. (Superseded: BFRES quads,
  see the Phase 5 step 3 entry.)

## 2026-10-18 — 100 Mario Challenge uses SMM1's own presentation, like-for-like
Owner request: title/menu/world map and the before/after screens and animations as in SMM1.
Assets come from the owner's SMM1 dump at first launch (never committed). Rendered by a layout
player for Nintendo's BFLYT/BFLAN format so screens run from the original layout and animation
data. Courses themselves are SMM2 courses on the SMM2 engine. Roughly doubles Phase 6; the SMM1
importer and the layout player start in Phase 5b because the app menus reuse them.
Pending owner choice: SMM1 HUD during 100 Mario courses (default yes), languages (default EN-US).

## 2026-10-18 — For now the Swift render loop drives the 60 Hz tick
docs/architecture.md plans a Rust-owned sim thread. Until there is audio and heavier simulation,
the renderer steps the engine from an accumulator (fixed 1/60 s ticks, capped catch-up) on the
main thread. Simpler, still deterministic (ticks never depend on frame time). Revisit with audio.

## 2026-10-18 — 2D styles render as sprite quads, not a full BFRES renderer
SMB1 (and the other 2D styles) store each animation frame as its own small texture named
`<anim>.<frame>` inside the model, and terrain as one tile atlas per theme. The slice draws
textured quads; quad placement and frame timing will be read from the BFRES later for accuracy.
SM3DW will need the model renderer.

## 2026-10-18 — Engine architecture: Rust core + thin Swift shell (Option B)
Owner's choice from docs/architecture-options.md. Rust core (formats, importer, deterministic
60 Hz sim, audio mixer, modes) behind a C ABI; Swift app for window, Metal rendering,
GameController input, audio output and SwiftUI menus. Reason: native Mac app quality plus a
portable, independently testable core that leaves a Windows/Linux port open. Cost accepted: two
languages and an FFI boundary. Design: docs/architecture.md. Toolchain: Rust 1.99.0 (pinned),
Swift 6.4 / Xcode 27, macOS 14+ Apple Silicon.

## 2026-10-18 — Behavioural specs stay private; the engine is written from them
Phase 3 specs (`research/specs/`) describe behaviour in prose, tables and maths with evidence tags,
never decompiled code. They stay gitignored because they cite addresses and quote game data values
from the owner's dump. Public docs (`docs/formats/`) only carry format facts.

## 2026-10-18 — Ghidra headless directly, REA for spot checks
REA's MCP calls time out (330 s) before Ghidra finishes auto-analysing a 36 MB binary. Headless
analysis runs in ~7 min; small read-only query scripts (`tools/ghidra/`) do the rest. Parallel
agents each get a private copy of the analysed project.

## 2026-10-17 — Course key table is located at runtime, never shipped
`smmfmt.smm2.keyfind` scans the user's own executable for the 64-word table that decrypts one of
the user's own `.bcd` files (cheap one-block test, then full CRC32 + CMAC). The engine's
first-launch importer will do the same and cache the result in the user's app-data folder.
Community sources that print the table were not used for it.

## 2026-10-17 — Format code lives in a Python reference package (`smmfmt`) for now
Phase 4 chooses the engine language. Until then, `smmfmt` is the executable specification: the
engine's own parser will be tested against it (same inputs, same parsed values). Records keep
their raw bytes so unknown fields survive round trips.

## 2026-10-17 — Path arrays are slot-indexed
Snake blocks, clear pipes, piranha creepers, ! blocks and track blocks store entry *i* in slot *i*
and may be sparse; the count is the number of live slots. Found in 16 of 730 courses. Parsers that
read only the first *count* slots lose entries.

## 2026-10-17 — v3.0.3 is the behavioural target; v1.0.0 is the mapping aid
The engine targets SMM2 3.0.3 (final) behaviour and assets. Community RE notes (e.g. Treeki's
SMM2Reversing, July 2019) were written against v1.0.x, so Phase 3 maps functions in the v1.0.0
binary first and carries names across to 3.0.3 by function matching. The first-launch importer
will take the base dump plus an optional update dump.

## 2026-10-17 — Native app milestone (Phase 5b) before the 100 Mario Challenge
Owner wants a proper native macOS app before the 100 Mario Challenge work starts. Phase 5b adds the
app bundle, import screen, course library, pause menu, settings, save data and per-course
"playable" marking. Phase 6's course pool is limited to playable courses and grows with Phase 7
(alternative: do Phase 7 first; rejected for now because it delays the mode a lot).

## 2026-10-17 — Course sample from the public mm2_level dataset
TGRCode's API returned 502 and SMMDB served a self-signed certificate. TheGreatRambler's Hugging
Face dataset (26.6M courses, Feb 2022, CC BY-NC-SA 4.0) is read remotely with DuckDB range requests.
It also carries Nintendo's own difficulty bucket and clear rates, useful for Phase 6. Shard 0 is
used first: its uploads predate game v2.0, matching the v1.0.0 dump. Courses stay outside the repo.

## 2026-10-17 — SMM2 extracted with hactool rather than Ryujinx's Extract Data
Same output, but scriptable and repeatable. Keys are read in place from Ryujinx's folder.

## 2026-10-17 — SMM2 RomFS lives on the external SSD
Internal disk had ~1 GB free. `~/smm-work/extracted/smm2-romfs` is a symlink, so paths are unchanged.

## 2026-10-17 — CDecrypt patched to take the common key at runtime
Upstream CDecrypt embeds the Wii U common key. Rule: key material only comes from the owner's
console. The local build reads `$WIIU_COMMON_KEY_FILE` instead. Patch kept outside the repo.

## 2026-10-17 — SMM1 reference build is update v208, not the disc's v16
The disc ships base v16 plus update v208 (in the GI partition). Only v208 contains Super Expert
in the 100 Mario Challenge, so v208's `Block.rpx` and content are the reference for Phase 6.
Base content + v208 overlay = the effective game.

## 2026-10-17 — NSO loading via Ghidra Switch Loader built from source; nx2elf as fallback
Upstream Switch Loader release targets Ghidra 12.1.2; extensions must match Ghidra exactly, and
REA is configured against 12.1.4. Built 1.6.1 @ 5d80d195 against 12.1.4.

## 2026-10-17 — hactoolnet dropped
Its source repository is no longer on GitHub; hactool 1.4.0 + nstool v1.9.2 cover the need.

## 2026-10-17 — Proprietary-content guard checks content, not just extensions
Extension lists miss renamed files. The guard also checks magic bytes (Yaz0, SARC, zstd, BNTX,
BFRES, MSBT, BYML, NSO, XCI, WUX, RPX...), key-file shapes, and a 5 MiB size cap. Runs in CI and
as a pre-commit hook.

## Open
- Licence for the repository (owner's call).
- Engine language/framework: deferred to Phase 4 per plan.

## 2026-10-18 — 100 Mario over a local library: three deliberate departures from the server
SMM1 refuses to start when the first search returns fewer than 32 courses; a local library is
far smaller, so any non-empty band starts a run (`Pool::with_minimum`). Courses already cleared
in the run are not offered again while others remain (the server has no such memory, but with a
small library repeats would dominate). The castle search (last course) is read as "castle theme"
and falls back to any course. Difficulty value = 100 − clear rate (the same fail-rate scale the
SMM1 bands use); courses without metadata get 30, so they appear only in Easy and Normal runs.

## 2026-10-18 — Tracks as a geometric graph, not the game's per-cell rail grid
The game expands each track record into a 3×3 (5×5 for junctions) template of tile codes and walks
a cell grid. The engine builds the same paths as line and arc pieces joined at shared end points.
The geometry was fitted to the corpus: object reference points land on the pieces they link to.
Riders keep their offset from the track point and move 0.75 px/frame (1.5 winged). The pieces
are 32 px long; only free ends reverse. Still guessed: which way curves bulge, the start direction
field, and open-vs-closed ends (all ends reverse). Junctions aren't built. Tracks are drawn with
white texels from the field atlas, a placeholder for the real per-cell track tiles.

## 2026-10-18 — Cross-platform shell (owner asked for Windows, macOS, Linux, web and iOS)
The Rust core has no platform code, so it ports as is. Two ways to reach the platforms the
SwiftUI/Metal app can't (Windows, Linux, web):
- **chosen:** a portable Rust shell (winit + wgpu + gilrs + cpal; wasm32 for the web) that runs
  the same core and draws everything itself, including the menus (layout player). The Swift app stays
  for macOS/iOS.
- **rejected:** a separate native app per platform. It would mean five UIs to keep in sync.

Menus move from SwiftUI to engine-drawn layouts in both shells. That's what the like-for-like goal
needs anyway, since SMM1 and SMM2 menus are layouts. Proposal; it can be revisited before Phase 8
starts.

## 2026-10-18 — Online: an open-source server, never Nintendo's
The owner wants multiplayer through an open-source server. The engine isn't the Switch game, so it
doesn't need Nintendo's NEX protocol.
- **Proposal for course sharing:** our own open-source server (Rust, MIT/Apache), self-hostable,
  with an HTTP API. It covers courses, search, likes, makers and Endless/100 Mario pools, and can
  import from SMMDB.
- **Proposal for real-time play:** rollback netcode on the deterministic sim over WebRTC
  (open-source matchbox signalling), so desktop and browser players can join the same game.

⏳ Open for the owner: whether they already have a specific open-source server in mind.

## 2026-10-18 — SMM1 UI cache location
The app looks for the SMM1 UI import (`mmk import-smm1`) in `MMK_UI_CACHE`, then
`<Application Support>/MMK/assets/smm1-<build>/` (next to the SMM2 import), then the development
cache `~/smm-work/cache/smm1-<build>/`; newest build name first (`AppPaths.findSMM1Cache`). No
import → the SwiftUI stand-in 100 Mario screens. The in-app importer doesn't run the SMM1 import yet.

## 2026-10-18 — 100 Mario screens: SMM1 layouts, inferred choreography
The layouts and their animations are the game's; how the game strings them together is not in
the data, so these are inferred and marked [inf] in `ChallengeScreens.swift`:
- **Difficulty select = tabs over the world map.** `Nwk_ListTitle_00` and the map both have a
  300-frame MapIn whose frame 0 shows the tab bar above an un-zoomed map with New Game / Continue
  [obs]. Confirm plays MapIn (bar leaves, map zooms, curtains close) [obs keys]; left/right swap
  maps with OutL/OutR + InR/InL.
- **Locked tiers are shown greyed** (button "Invalid"), per the owner's brief. The game adds tabs as
  they unlock (`OpenType` 0/1/2 = 2/3/4 tabs) [obs], so this is a deliberate departure.
- **Selected tab = the button's Decide animation held at its end** (dark button, pointer below).
- **New Game:** MapIn; at 200 the banner (Telop) while the Goombas carry Peach (MoveKuribo); then
  Peach on the castle (AnimB); Mario jumps out of the start castle and walks to dot 1 (MoveMario);
  Out; intro card. **Continue:** MapIn with Mario on his dot, Out, card. **After a clear:** In,
  the cleared dot's Clear pop, Mario walks one dot, Out, card. **After a skip:** straight to the
  card. Mario's stops are the MoveMario frames where his pane reaches each dot's position (the
  animation authors one walk along all dots); the walk plays at the authored speed.
- **Cleared dot = `Type` frame 1** (shows the clear disc).
- **Intro card:** one small Mario per life; the formation is 100 and AnimB sends #99 forward, so the
  hidden ones are #0…#(99 − lives). After AnimA + AnimB the card holds for the length of its Out
  animation (150; Out and OutMiss only key the fade to 0, so they act as timers). After a death:
  AnimB + OutMiss (60) over the paused game. Confirm skips (A skips since 1.20 [F]).
- **Pause:** `Cmn_PauseMenu_00` Type 6 (Start Over + exit, no network buttons); the exit button
  shows the table's "Save & Quit" text. Skip is not a pause item in the layout version: SMM1 skips
  with the held button. The selection starts on the close (continue) button.
- **Course clear:** `ModeType` 0 (Save & Quit + Next, no recommendations); on the run's last course
  `ClearType` 1 (Next alone). Star, comments, recommendations, Miiverse, Mii and flags are hidden:
  there is no data for them. "World Record" is the dataset's `world_record` when the index has it.
- **Skip:** hold for 3 × 32 frames (each CountDown step), then Pass (32) and the course ends.
  Default keys − (minus) and Tab, controller Options (−); bound like the other actions.
- **Ending:** the rescue scene isn't a layout, so the ending text balloon plays over the finished
  world map (Mario at the castle). Text: `T_ThankYou_10` (Easy), `_20` (Normal), `_30` (Expert,
  Super Expert); credits `Nwk_StaffCredit_01` (8 entries, Easy), `_00` (16, Normal and Expert),
  `_03` (6, Super Expert), chosen by their entry counts.
- **Layout player:** a child without parent-anchor bits sits on its parent's origin point (fits
  the castle flags and the balloon text; the earlier "parent centre" rule floated them), and an
  "influenced alpha" pane fades all its descendants, parts included (Mario's fade-in on the map).

## 2026-10-18 — Portable shell: shared `host` crate, wgpu, our own BC7 decoder
- **`host` crate:** sprite placement (`place`, the resolver), clear colour, course bytes decoding
  and the sound director moved out of `ffi` into `core/crates/host`, so the Swift app (through
  `ffi`) and the Rust shell run the same code. `ffi` re-exports `place`, and `MmkAudio` is now an
  alias of `host::audio::AudioOut`, so the C ABI is unchanged. New sim events get their sounds in
  `host/src/audio.rs` (formerly `ffi/src/audio.rs`).
- **Stack:** winit 0.30 (latest stable; 0.31 is still beta), wgpu 30, gilrs 0.11, cpal 0.18. One
  wgpu renderer covers Metal, Vulkan, DX12, GL and WebGPU/WebGL2.
- **BCn fallback:** GPUs without BC support (WebGL2, mobile) get CPU-decoded RGBA8:
  `formats::bcn` for BC1–5, and a small BC7 decoder of our own in the shell, written from the
  Khronos format specification, instead of a new dependency. `mmk-play --check-textures` checks
  it against the GPU (every BC7 texture of the owner's cache matches exactly). BC6H is not
  decoded; the caches have none.

## 2026-10-18 — CI runs Linux only on push
GitHub Actions minutes were close to the 2000-minute limit, and macOS minutes bill at 10×.
Pushes now run only the Linux jobs: Rust fmt, clippy and tests; the Python tests; the
proprietary guard. The macOS (Rust and the Swift app), Windows and wasm shell jobs run when started
by hand (`workflow_dispatch`). A newer push cancels the older run. `scripts/check.sh --app`
still runs every gate locally, macOS app included, before each commit.

## 2026-10-18 — Textures load lazily
With all five styles imported, the cache holds 12,540 textures (729 MB). Loading and uploading
all of them took about 2 GB of memory at start-up. Now `Cache::open` reads texture sizes from
`textures/index.json`, and a texture's pixels only when first asked for (`Cache::texture`). Both
renderers upload a texture when a sprite first uses it. Measured on the same course and frame:
- portable shell: 2.0 GB → 107 MB peak, start 1.1 s → 0.5 s;
- macOS app: about 275 MB peak.

`Cache::load` through a reader function, used by the web and by tests, still loads everything.

## 2026-10-18 — Web build: game data in OPFS, streamed; both import paths in the browser
- **Storage: the origin private file system (OPFS), not IndexedDB.** It holds the cache as
  files (about 1.3 GB, 15,944 files without the unused `meshes/` and `anims/`), a picked `File`
  is written without passing through JavaScript memory, and reads come back per file. Copying
  the owner's `mmk import` cache takes about 15 s in Edge. Nothing is uploaded; **Remove game
  data** deletes it. Rejected: keeping a `showDirectoryPicker` handle to the owner's folder
  instead of a copy (Chromium only, and every visit asks for permission again).
- **The whole cache never sits in memory.** OPFS reads are asynchronous and the game loop is
  not, so `Cache::load_deferred` reads only the index and the shell streams texture files on
  first use (`take_requests` / `provide`; the session holds up to 30 frames while reads are on
  their way, then plays on and the art pops in). Course music is read before the course starts.
  `Cache::load` (everything up front) stays for tests and small caches.
- **The dump import runs in the browser too, with the real importer.** Measured natively, it
  peaks at about 1.3 GB, which fits in wasm32's 4 GB. `mmk-import` builds for wasm32; its file
  access goes through `mmk_import::vfs` (a thread-local override, so the stage functions keep
  their signatures). The page runs it in a module worker, because only workers can read picked
  files synchronously (`FileReaderSync`). The worker posts each output file to the page, which
  stores it, because workers can't wait for storage either. In headless Edge it takes about
  35 s and the Edge process peaks near 1.5 GB. The output matches the desktop import (same file
  list; 169 sampled files byte-identical). Copying a cache made on the desktop is still the
  lighter way, for machines with little memory and for mobile browsers.
- **Sound: cpal's WebAudio backend** (the `wasm-bindgen` feature; `futures-channel` was already
  in the local cargo cache) rather than our own AudioWorklet. The native and web shells share
  `audio_out`. The stream opens on the first pointer or key event, since browsers block audio
  before a gesture. It schedules buffers from the main thread, so a long main-thread stall can
  click. An AudioWorklet needs wasm threads (cross-origin isolation) to share the mixer.

