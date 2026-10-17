# Progress

## Roadmap
✅ done · 🔄 in progress · ⬜ not started · ⏳ waiting on owner

### Phase 1 — Environment and extraction ✅
- ✅ Dumps gitignored; provenance checked; toolchain pinned; CI + pre-commit guard
- ✅ SMM1 decrypted (base v16 + update v208); SMM2 v1.0.0 extracted; inventories; 550-course sample
- ✅ SMM2 update v3.0.3 extracted (patched RomFS + ExeFS); both executables converted to ELF

### Phase 2 — Course formats ✅ (awaiting go-ahead)
- ✅ Community format docs gathered and cross-checked (13 sources; disagreements recorded)
- ✅ Confirmed: SMM2 `.bcd` = 0x10 header + AES-CBC body + IV / random state / CMAC; SMM1 `.cdt` = 0x15000 BE, CRC32 @0x08 over [0x10:]
- ✅ Format docs: [docs/formats/smm2-course.md](docs/formats/smm2-course.md), [docs/formats/smm1-course.md](docs/formats/smm1-course.md)
- ✅ `smmfmt` reference package: SMM2 course parser/writer, SMM1 `.cdt` parser/writer (CRC), course codes, `python -m smmfmt` summary CLI
- ✅ SMM2 `.bcd` decrypt/encrypt; key table found at runtime in the owner's executable (`smmfmt.smm2.keyfind`, ~2 s; same table in v1.0.0 and v3.0.3)
- ✅ Byte-exact round trips: 550 dataset courses, 180 built-in SMM2 courses × 2 versions (decrypt → parse → write → re-encrypt), 156 built-in SMM1 `.cdt` — 30 tests, CI runs the 24 that need no game data
- ✅ Finding: path arrays (snake, clear pipe, creeper, ! block, track block) are slot-indexed and can be sparse
- ⏳ Owner's own saved courses (optional; not needed to proceed)

### Phase 3 — Binary analysis ✅ (awaiting go-ahead)
Specs are private (`research/specs/`, gitignored): behaviour in prose/tables, no decompiled code.
- ✅ Ghidra headless analysis of SMM2 v1.0.0 and v3.0.3 (≈7–8 min each), SMM1 `Block.rpx` v16 + v208
  (own RPX→ELF converter, nothing installed); query scripts in `tools/ghidra/`
- ✅ Data-file survey: physics is in code; EnemyDB attack matrix, EditDB parts, per-style SceneDB,
  263 player states are data; 108 lesson input replays found
- ✅ Course-loading pipeline: decrypt → validate → placements → actors; part→actor tables per style;
  flag bits resolved (big = 0x4000 size field, 0x400 = on track, 0x1 from pipe, …)
- ✅ Player physics: one shared model for all styles (style = feature switches); SMB1 constants
  complete (walk 1.5 / run 3.0, jump 3.628 + speed bonus, held-jump gravity bands, fall cap −4.0);
  simulated jump heights 4.2 / 5.4 tiles match the game; unchanged in v3.0.3
- ✅ Themes: night = area flag bit 1; per-theme day/night effects, wind, liquids, autoscroll, timer,
  clear conditions (CRC32 of internal names)
- ✅ Objects: 82 object specs + shared mechanics (actor base, 18 enemy contexts, generic walker,
  attack/reaction matrix, blocks); all top-25 objects in depth
- ✅ SMM1 100 Mario Challenge code: 11/14 open questions answered (requests 50 courses per band,
  lives cap 100, ≤3 banked lives per course, free unlimited skip, unlock rules, reward selection)
- ⬜ Left for later phases: enemy stomp bounce speed, activation range, 44 style power-up and 42 clown
  car specs, remaining 51 rarer/2.0+ object ids (Phase 7)

### Phase 4 — Engine architecture ✅ (awaiting go-ahead)
- ✅ Options with trade-offs: [docs/architecture-options.md](docs/architecture-options.md)
- ✅ Owner chose **B: Rust core + thin Swift shell**
- ✅ Working design: [docs/architecture.md](docs/architecture.md) — crates, threads, determinism rules,
  FFI shape, importer pipeline (RomFS/ExeFS folder in, Metal-ready cache out), placeholder pack,
  testing, dependencies, Phase 5 order of work
- ✅ Asset facts checked on the dump: textures ~98 % RGBA8 sRGB + BC1/4/5/7 (all native on Apple
  Silicon, need Tegra unswizzle); 2D styles are BFRES quads; audio DSP-ADPCM

### Phase 5 — Vertical slice (SMB1, ground) 🔄
- ✅ 1. Rust workspace (formats, assets, import, sim, ffi, cli) + Swift/Metal app; CI on macOS + Linux
- ✅ 2. Importer: identifies the game build, finds the course key table, converts tables, exports
  M1 textures (RGBA8 + BCn, Tegra unswizzle) into a Metal-ready cache outside the repo
- ✅ 3. Rendering: tiles, sprites, letterboxed integer scaling; theme backgrounds (M1_DV_*, day/night,
  parallax 0.5 [inf]) with the clear colour from the background's sky (black underground/castle); BFRES reader (0.9: skeleton,
  materials, shapes, texture-pattern animations) → importer `sprites.json` (1952 of 1963 M1
  textures placed by their model quad, all animation frame keys) → FFI draws model sprites at the
  sim's origin + quad. Hand offsets gone; Goomba walk animation fixed (it never animated) and
  timed from the data (10 frames per texture); goal flag mirrored as in the game. Player walk-rate
  scaling and the other objects' frame timings are measured but not wired in yet
- ✅ 4. Audio: BWAV/BARS parsers + DSP-ADPCM/PCM16 decode; importer `audio` stage (22 SMB1 sounds:
  jump, coin, stomp, kick, bump, break, item, power-up/down, 1-UP, fireball, pole, time count,
  death / clear / hurry fanfares, ground + hurry + star music) into `<cache>/audio/`; `mmk-audio`
  mixer (32 voices, loops, music chaining, fades, exclusive groups, linear resampling to 48 kHz,
  master/music/SFX volumes); engine maps sim events to sounds (hurry jingle → fast music, star
  music, death and clear fanfares, count-down loop); AVAudioEngine output (`MMK_MUTE=1` to
  silence); `mmk audio` CLI lists/decodes sounds to WAV · other themes' music, night music and
  object sounds beyond the slice ⬜
- ✅ 5. Terrain: ground, blocks, pipes, semisolids, slopes ✅ · ice ✅ · in-course camera ✅ (4 px
  dead-zone follow, filtered vertical tracking, six vertical speed strategies, low-camera 216 px
  ceiling, full-height scroll-stop walls, course start, frozen on death, goal); autoscroll and
  vertical-area camera come with those features
- ✅ 6. Core objects: coins; ? / brick / hidden / hard / note blocks (bump curve, contents, multi-coin,
  breaking + debris); Super Mushroom, Fire Flower + fireballs, Star, 1-Up; Goomba (+ Paragoomba),
  Koopa green/red (+ Paratroopas), shells (kick, slide, chain kills, wake-up); stomp chains, damage
  (Fire → Super → Small), invincibility, death sequence, restart, lives; course timer + hurry;
  M1 goal pole (height score, slide, walk to castle, time → score countdown, castle flag)
- 🔄 Navigation: warp pipes in all four directions (entry windows, align / 0.75 px/f travel, 30 + 30
  frame holds with fade), main ↔ sub-area with per-area state kept, doors (same-area pairs)
- 🔄 More objects: Piranha Plants (free / pipe / fire), pipe generators, big coins, checkpoints
  (respawn, big coins kept), Thwomp (+ side), lifts (static / falling), springs (normal / held high
  bounce), Bill Blasters + Bullet Bills, spike traps; moving-solid layer with rider carry; P switch
  (brick ↔ coin swap, P doors), ON/OFF switch + dotted-line blocks, falling donuts, Munchers,
  conveyor belts; Spiny, Buzzy Beetle (+ fire-proof shell), Spike Top (floor only), Hammer Bro +
  hammers, Bob-omb (fuse, kick, explosion), Dry Bones (collapse/reassemble), Boo; parachutes,
  one-way walls, Burners, Fire Bars, Banzai Bills; vertical areas; liquids (static / one-way /
  oscillating water, lava, poison; deadly kinds kill) and swimming (strokes, sink, swim table,
  jump-out window; underwater theme, castle-night air swimming); seesaws (rider torque, tilt
  limits), Bumpers, Grinders (static), airship cannons + cannonballs; Lakitu (+ Spiny eggs) and
  rideable Lakitu clouds, POW quake, keys + locked doors; snake blocks (path codes decoded from
  the corpus, multi-segment moving solids, fall-apart), Twisters, swinging claws (grab/release),
  Cheep Cheeps (swim / flop), Bloopers (pulse + sink), Skewers, Magikoopa (+ magic), Rocky Wrench
  (+ wrenches), Wiggler (segments, anger), lava lifts; preset autoscroll (push / crush); night
  effects: sky-night low gravity, desert-night wind, underground-night upside-down view + swapped
  Up/Down, snow-night ice, forest-night poison, castle-night air swimming
- 🔄 Coverage (`mmk coverage`): 33 of 150 sample SMB1 courses fully playable (was 20); biggest
  blockers now Chain Chomp, tracks, Bowser, clear conditions, swinging claw, grinder, liquids
- ⬜ 7. Play real courses start to goal; tune against Ryujinx (needs a Ryujinx build with a GDB stub)

### Phase 5b — Native app (must be done before Phase 6) 🔄
- ✅ `.app` bundle: `scripts/make-app.sh` → `build/MMK.app` (Info.plist, generated placeholder icon,
  ad-hoc signature); working name "MMK" kept in one file (`Identity.swift`)
- ✅ First-launch import screen: RomFS/ExeFS (+ optional update) pickers, importer on a background
  thread with progress (`mmk_import_run`, update RomFS overlaid via symlinks); "use placeholder art"
  fallback draws coloured boxes per sprite layer (`MMK_TEXTURE_NONE`)
- ✅ Course library (home): scans the library folder, name/style/theme/timer via `mmk_course_info`,
  search, filter, sort, add files/folders (deduplicated by content hash), double-click to play
- ✅ Pause (Esc/P/+): Continue, Restart (`mmk_engine_restart`), Exit; course clear and game over
  overlays → library; menus work with keyboard, controller and mouse
- ✅ Settings window: keyboard + controller remapping, full screen, integer scaling, volume sliders
  (stored for the audio engine: `volume.master/music/sfx`)
- ✅ Save data: per course (content hash) cleared flag, best time, clears, attempts in `saves.json`
- ✅ "Playable" / "Not yet supported" marking (style M1 for now; `unsupported` object count hook)
- ✅ Built-in courses from the owner's dump in the library: the importer exports the 120 Story Mode,
  50 lesson and 10 title courses (encrypted, decrypted with the cached key table); "Built-in" filter
- ⬜ Cancel button for a running import; incremental re-import
- 🔄 SMM1 importer + layout player started: `mmk import-smm1` converts the owner's Wii U UI
  (337 layouts, 1698 animations, 2576 textures, 9 fonts, English text) into a cache; the `ui`
  crate plays layouts + animations into quads; the app draws them (`UILayer`, debug view
  `MMK_UI_LAYOUT`) — world map, course intro, course clear, game over, title menus verified
- 🔄 Shared menu look with the SMM1 layout player (Phase 6): 100 Mario screens done

### Phase 6 — 100 Mario Challenge 🔄 (owner: SMM1 presentation like-for-like)
- 🔄 Research: SMM1 100 Mario screen sequence from Block.rpx v208 + layout data — scene → layout
  map done (private `research/specs/smm1-layouts.md`: title, main menu, Course World, difficulty
  select, world maps + Mario/course/castle parts, course intro, skip, clear, pause, game over,
  ending text, credits, costume reward); still open: the rescue scene itself (not a layout), the
  run-flow timing between screens
- 🔄 SMM1 asset import (owner's disc dump): BFLYT/BFLAN layouts ✅, GX2 textures (BFLIM, AddrLib
  1D/2D untiling) ✅, BFFNT fonts ✅, MSBT text (English) ✅, BFSTM/BARS audio ⬜
- 🔄 Layout player (panes, text, keyframed animations) — pane tree, parts, pictures, windows,
  text with font fallback, SRT/visibility/alpha/colour/texture-pattern/texture-SRT curves ✅;
  part property overrides: picture material from the enclosing layout (course-dot numbers,
  castle variants) and basic values (visibility, translate, size, scale, rotate, alpha; import
  now keeps them) ✅; "influenced alpha" fades the whole subtree incl. parts ✅; children without
  anchor bits sit on the parent's origin point [inf] ✅; caller visibility, pane query, message
  templates with value placeholders (FFI `mmk_ui_set_visible/pane/message`) ✅;
  per-character transforms (GAME OVER letters), alpha compare, indirect textures ⬜
- ✅ Mode logic (`core/crates/modes`): run lengths 8/16/16/6 (10 Mario 8), 100 lives = cap, ≤3
  banked extra lives paid on clear (+1 refund in 100 Mario), death retries from the course-start
  state, free skip (100 Mario only), Save & Quit, game over, completion; course pools by
  difficulty band with SMM1's shuffle and castle search; Continue records, unlock rules, stats ·
  costume reward selection ⬜ (category table not read)
- ✅ Run session (`modes::challenge`) + FFI (`mmk_challenge_*`, `mmk_engine_set_run_counters`):
  New Game / Continue, life pre-charge autosave, deaths retry with the course-start counters,
  1-Ups and coin rollovers banked through the sim's `lives_gained` (HUD lives stay put), skip,
  Save & Quit, unloadable-course replacement, unlock announcement once, credits list
- ✅ Course pool: local library (difficulty value = fail rate from the dataset's `index.json`
  clear rate, else the SMM2 bucket); castle-theme courses preferred for the last course; no
  repeats within a run while others remain · remote source ⬜
- ✅ Pool limited to courses marked playable; grows with Phase 7
- ✅ App flow (functional stand-in screens): difficulty select with locks and Continue, course
  intro card, run banner, pause menu Skip / Start over (−1 life) / Save & Quit, game over, run
  cleared + unlock + course credits; `hundred.json` save (kept as the fallback without the
  SMM1 import)
- ✅ SMM1 layout screens (`ChallengeScreens.swift`, plain classes stepped at 60 Hz; screenshots
  via `MMK_UI_SCREEN=challenge*`):
  - ✅ difficulty tabs `Nwk_ListTitle_00` over the selected `Nwk_WorldMap*_00` (pre-MapIn view),
    New Game / Continue, lives and n/total from the Continue record, locked tiers greyed
    (Invalid) — the game hides them instead [obs data], see DECISIONS
  - ✅ world map: MapIn, Goombas carry Peach under the banner (Telop + MoveKuribo), Peach on the
    castle (AnimB), Mario jumps out and walks (MoveMario, stops found from the dot positions),
    cleared dots (Type) with the Clear pop, Mario walks to the next dot after each clear, Out
  - ✅ intro card `Play_CourseIn100Mario_00`: name, maker, SMM2 code (from the dataset id), lives,
    n/total, one Mario per life; AnimA → AnimB → hold; after a death AnimB + OutMiss over the game
  - ✅ in-course overlays drawn by the game Renderer: `Cmn_PauseMenu_00` (mode line
    "100 Mario Challenge (n/total)", Start Over, Save & Quit, close), `Play_CourseClear_00`
    (times incl. the dataset's world record, Save & Quit / Next), skip countdown
    `Play_SlidePassIconKey_00` (hold − / Tab / controller −, 3 × 32 frames, release cancels)
  - ✅ `Cmn_GameOver_00`; ending: `Play_EndingText_00` over the finished map (Easy: another
    castle; Normal; Expert/Super Expert) then `Nwk_StaffCredit_01/00/03` with the cleared courses
  - 🔄 timings between screens, the rescue scene (not a layout), Mii/flag/Star/comment areas
    (hidden: no data), per-character GAME OVER letters, UI sounds ⬜
- ⬜ SMM1 → SMM2 course import design + documented losses
- ⬜ App screens drawn with the SMM1 layouts (world map walk, intro card, clear, game over, ending)

### Phase 7 — Expansion ⬜
- ✅ Styles: all five styles' art imported (importer v4); SMB3 and SMW courses play with their own
  art (SMB1 fallback per model/frame); carrying shells, slope slide (not SMB1), SMW spin jump (new
  Spin button: C / R / ZR); the sim owns the playable-style gate — sample 319/550 (M1 148, M3 94,
  MW 77) · Yoshi, cape dive flight ⬜
- 🔄 NSMBU / 3D World rendering (3D mesh renderer; both styles playable since the merge —
  sample NSMBU 99/100, 3D World 21/100, the rest blocked on 3D World-only parts):
  - ✅ BFRES reader: shapes with index buffers per LOD and skinning attributes, skeleton matrix
    palette + inverse bind matrices, skeletal animations (cubic / linear / step / baked curves,
    wrap modes), bone visibility animations; real files checked in tests
  - ✅ Importer v5 mesh stage: 354 `meshes/*.mmesh`, 3500 `anims/*/*.mskel` (all WU_/3W_ models,
    StaticModel.pack included) in ~12 s total import
  - ✅ FFI: mesh data, animation lookup, posing (`mmk_mesh_*`), per-frame mesh draws from the sim's
    sprites (`mmk_engine_meshes`); Metal path with GPU skinning (`MeshRenderer.swift`), model
    viewer `MMK_MESH_PREVIEW`
  - ✅ Drawn: 3D World Goomba, Koopa (+ shell), Mario (body, face, eyes, hands; small, super,
    Fire), blocks, coins, items, terrain as per-cell meshes; NSMBU Mario (body + head) and the
    pre-rendered enemy / item / object cards; NSMBU terrain stays the atlas
  - ⬜ Per-actor animation clocks and transitions (time = world tick), 3D World neighbour variants
    for terrain edges, pipes / note blocks as models, small 3D World Mario's cap, material render
    state (translucency, emission), the 3D styles' backgrounds, camera angle vs the game
- ✅ Super Leaf (Raccoon: tail swing hits enemies/blocks, P-meter 63 frames → 240-frame flight,
  flaps vy 1.5 × 18 f, slow fall ≥ −1 × 15 f) and Cape Feather (cape spin both sides, glide ≥ −1,
  simplified meter launch); goal-as-Raccoon / Cape conditions
- ✅ Clear conditions: header hash → condition; defeat / collect / coins / POW / P Switch counts
  (each placed object once), goal as Super / Fire / invincible, no damage / no landing; the goal
  lets the player pass until met; HUD shows progress · holding / riding conditions ⬜ (no carry,
  Yoshi, shoe yet) — SMB1 sample 53 → 65 playable
- ✅ Chain Chomp (chained + unchained)
- ✅ Koopa Clown Car (and Fire car): floats empty, chases with an enemy rider (riders linked by
  link id, carried on top), flown by the player (2 px/f, 0.05 accel, Fire shoots, Down+Jump exits
  [inf]); "goal in a Clown Car" condition
- ✅ Winged / tracked blocks (brick, ?, hard, ice, cloud) as moving solid actors that release
  their contents when bumped; winged lifts bob; winged Thwomps patrol and bob (12-thwomp.md)
- ✅ Fish Bone: patrol, 40-frame aim, charge (2 / 6 px/f), shatters on contact; fire-proof
- ✅ Custom autoscroll: camera centre follows the area's part-89 nodes (speed per segment,
  0.03 easing, pauses during warps/death/goal; the player is pushed at the edges)
- ✅ Conveyor slopes (steep slope + belt carry); winged/tracked coins and note blocks —
  SMB1 sample 148/150 playable (left: a Bill Blaster and a Burner on tracks)
- ✅ Theme music per style: the importer copies every style's theme / hurry / star tracks (105
  streams); the player decodes only the course's tracks into per-course mixer slots · per-style
  sound effects ✅ (style variants of SFX and fanfares, SMB1 fallback), sub-area music ⬜
- ✅ Wall slide (≥ −2.0) and wall jump (vy 3.5, 2.25 away) in every style; NSMBU/3DW jump chain
  (third jump ×1.05, 10-frame window [inf]) and ground pound (wind-up, −6.0 dive, hits blocks and
  enemies below)
- ✅ Propeller Mushroom (NSMBU style power-up): Spin rise toward 2.5 for 50 f, slow fall −1.25,
  drill −6; goal-as-Propeller (ready for when NSMBU renders)
- ✅ 3D World blinking blocks (two groups, 240-frame swap) and spike blocks (shared timer: warning
  118–177, out 178–239; ON/OFF-linked modes [inf]) — placeholder tile art until 3W models draw
- ✅ Yoshi (SMW / NSMBU, yoshi.md): egg hatches [inf: 30 frames], mount from above, riding with the
  on-foot tables, Spin dismount per style, NSMBU flutter, tongue (eat / hold & spit shells, fire,
  bones; power-ups to the rider [inf]), Red Yoshi fire (60-frame cooldown), stomps kill spiny
  enemies, hit → rider pops up and Yoshi runs off; goal-on-Yoshi condition — SMW sample 97/100
- ✅ Pink coins: counted once each across both areas; the last one gives the key
- ✅ Goomba's Shoe / Stiletto: shoe Goomba hops (50-frame pause, 4.5 / 1.5), leaves the empty
  shoe; the player wears it (spikes and Munchers underfoot safe, shoe stomps, a hit knocks it
  off); "goal in a shoe" condition · shoe ground-pound, Yoshi (SMW/NSMBU) ⬜ — SMB1 sample 93/150
- ✅ Vines: climb areas with tile art from the course data; climbing (1.0 / 1.5 px/f, 6 px
  hang, side switch, jump off with 30-frame lock) · vines sprouting from blocks ⬜

- ✅ Bowser: 20 HP (big 40; fire 1, shell 3; star / POW / lava kill), ±64 px patrol facing the
  player, middle jumps and SMB1 Fly, fire every 40–79 frames + breath with level lane flames; SMB3
  big jump + hip drop; key drop · hip-drop quake, water timers, helper modes ⬜
- ✅ Boom Boom: 3 HP + fireball counter 5 / 10, run 0.75 / 0.75 / 1.5, wall jumps (3 per side),
  charge → Defend spin, DamageDown 70, winged cruise + swoop; Pom Pom (3D World alternate) [inf]
- ✅ Bowser Jr.: 3 HP + fireball counter, wait / walk ±24 / fire / hammer / hop / shift jump →
  hover → hip drop, DamageDown → shell slide; Clown Car riders for all three [inf] — SMB1 sample
  77 → 97 playable
- ✅ Lava Bubble (hide / cosine rise / fall), Monty Mole (buried, mound, leap, chase), Angry Sun
  (corner wait, spin, half-ellipse swoop) and Moon (wipes on-screen enemies)
- ✅ Tracks: graph from track records (lines + quarter arcs), rail follower (0.75 / 1.5 px/f),
  any actor rides (lifts, seesaws, grinders, claws, items, enemies); drawn as lines · junctions,
  open ends, blocks/coins on tracks, exact track tiles ⬜ — SMB1 sample 49 → 52 playable
- ⬜ Measure object/style/theme frequency in real courses
- ⬜ Add styles, themes, objects in that order (2.0/3.0 items need the update dump)

### Phase 8 — Every platform: macOS, iOS, Windows, Linux, web 🔄 (owner request 2026-10-18)
Approach (DECISIONS.md 2026-10-18, "Cross-platform shell"): the Rust core stays platform-neutral;
a portable Rust shell runs the game everywhere, the Swift app stays as the Apple-native shell.
- 🔄 `shell` crate (`mmk-play`, docs/architecture.md §5b): winit window + fixed 60 Hz loop, wgpu
  renderer (sprites, BFRES quads, layers, flips, sky colour, upside-down view, placeholder boxes;
  BCn on the GPU or CPU-decoded incl. our own BC7 decoder), keyboard + gilrs gamepads, cpal audio
  via the shared sound director, minimal HUD (built-in 5×7 font), pause / clear / game over,
  `--screenshot` headless smoke test. Shared front-end code moved to the new `host` crate (used by
  `ffi` too). Verified on macOS (M4 Pro, Metal): offscreen frames pixel-identical to the Swift app
  below the HUD; `--check-textures` agrees with the GPU on every BC texture. Open: layouts
  (layout player), the SMM font HUD, clear-condition HUD text, menus/library
- 🔄 Windows and Linux builds of the shell; CI jobs on Windows + Ubuntu (clippy, tests, release
  binaries) and wasm32 on Ubuntu. Windows type-checks locally (clippy, MSVC target); not yet run
  on Windows or Linux hardware
- 🔄 Web: wasm32 build of core + shell runs the demo strip in a canvas (WebGPU, WebGL2 fallback;
  checked in headless Edge), `scripts/build-web.sh`, `AssetSource` trait for browser-picked files.
  Open: WebAudio, asset import through the file picker into OPFS/IndexedDB (nothing uploaded),
  Gamepad API check
- ⬜ iOS/iPadOS: the Swift app on UIKit/SwiftUI + Metal (shared renderer code), touch controls,
  Files-app import of the owner's dump; installs over Tailscale with the existing tool
- 🔄 Engine-drawn menus everywhere (layout player), so every platform shows the same screens:
  SMM2 title + Play menu and the SMM1 100 Mario screens in the Swift app; not yet in `mmk-play`

### Phase 9 — SMM2 single-player like-for-like 🔄 (owner request 2026-10-18)
- 🔄 SMM2 layouts through the same layout player (private map: `research/specs/smm2-layouts.md`):
  - ✅ formats: Switch BFLYT/BFLAN 9.0 (all 531 layouts, 2328 animations parse), BNTX textures
    (all 3249 incl. R8/RG8/RGB5A1 → RGBA8), Switch BFFNT 4.1 (BNTX sheets), BFCPX font
    combinations, BFTTF/BFOTF scalable fonts
  - ✅ import `mmk import-smm2-ui <romfs> <out> [--exefs]` → `~/smm-work/cache/smm2-ui-3.0.3/`
    (same cache form as the SMM1 UI; scalable fonts rasterised into bitmap fonts with ab_glyph;
    console button icons drawn; the title courses copied)
  - ✅ player: `<Part>-<Pane>` message labels, part clones, caller pane overrides, global pane
    rectangles, "one material for all frames" windows, colour-only part material overrides [inf]
  - ✅ title screen at app launch (when the import exists) over a scripted demo of the playable
    title courses: prompt → Make (coming soon) / Play → Story Mode (library, Built-in filter),
    Course World / Coursebot (library), 100 Mario Challenge (extra button, SMM1 flow); game
    cursor, logo colour by course; keyboard + controller; screenshots `MMK_UI_SCREEN=title*`
  - ⬜ boot scene, fades, FLCT per-character text, glyph borders; Course World (`Nwk_Title_00`),
    Story Mode map (`Qst_*`), pause, clear / death screens, HUD; title in `mmk-play`
- 🔄 All five styles (SMB1, SMB3, SMW, NSMBU, 3D World) and all themes (day/night) — all five
  playable — sample 3D World 38/100; 3D World-only parts (track block, enemies …) ⬜
- ✅ Warp Box (97-warp-box.md): pairs 2k ↔ 2k+1 in the same area, entered by touch (no button),
  player hidden at once, held items dropped, key boxes spend one key and stay open, a used pair
  vanishes (single player); blocked-exit cancel ⬜
- ✅ Koopa Troopa Car (72): drop in from above; start spin, then it runs by itself (≥ 2.0, 3.0
  with the travel direction held [inf]), steering only by a 30-frame turn; jump hops it [inf];
  Spin gets out; runs enemies over; hits damage the car (third breaks it) [inf]; "goal in a
  Koopa Troopa Car" condition. Enemy drivers ⬜ (none in the sample)
- ✅ ! Block (119): 2×2 solid with a drawn path; a hit from below bounces it (7-frame ramp),
  knocks out enemies on it and grows a chain of 2×2 blocks along the path [inf: the chain motion
  wasn't decoded], restored after 600 frames
- ✅ Tree (106): the trunk climbs like a vine up into the canopy, the canopy is a jump-through
  platform, reaching it knocks out the canopy item (vy 4.5) [inf: trigger]
- ✅ Mushroom Trampoline (113): top-only platform N tiles wide; landing launches at 6.0 (4.43 with
  jump held), a jump within 6 frames re-launches at 4.43 − 0.1/frame; enemies bounce 5.5 (6.5
  winged); unflagged ones sway as lifts [inf]
- ✅ Crate (112): solid to stand on, picked up with Run beside it, thrown (3.5 px/f [inf]) it
  breaks on a wall or an enemy and releases its contents (vy 3.5)
- ✅ Super Bell / Cat Mario (3D World): the bell walks like a mushroom; wall climb (1.0, 1.5 with
  run, ≤ 120 f / 120 px, top-out vy 4.8, then WallClimbFall −0.085 to −2.0), dive (Down in the
  air: 4.0 forward, −3.0), slide scratch (2.5 for 12 f) and air scratch (vy 2.4, gravity −0.1, once
  per air time); "goal as Cat" condition; drawn with the cat suit's meshes and Climb animations
- ✅ 3D World two-stage run: 2.25 until the dash stage (75 f, 60 as Cat), then 3.0 (star 3.6),
  ×1.05 jump while dashing
- ⬜ Every part (0–132, incl. 2.0/3.0 items from the update dump), Yoshi, shoes, clown cars,
  power-ups per style, Koopalings, Master Sword, etc.
- 🔄 Course Maker (editor): core crate `editor` ✅ (place / erase parts with default sizes, child items,
  flags, ground painting auto-tiled from a corpus-derived table, theme / night / timer / autoscroll /
  style / name / length, undo-redo, save; tests play the saved course in the sim) · maker UI, copy,
  sub-area links, pipes/doors, paths, tracks ⬜ · first maker UI ✅ in the portable shell
  (`mmk-play --edit <file>`): engine-drawn course, ghost preview, 24-part palette, place/erase with
  the mouse, scroll, undo/redo, save, Enter test-play / E back · SMM2 maker layouts ⬜
- ⬜ Story Mode (Peach's castle jobs, built-in courses from the owner's dump), Coursebot, Lessons
- ⬜ Endless Challenge (SMM2's successor to 100 Mario) alongside the SMM1-style 100 Mario Challenge
- ⬜ Ninji speedruns / event courses (offline with local data), records and medals

### Phase 10 — Online with an open-source server ⬜ (owner request 2026-10-18)
Not Nintendo's network: the engine talks to an open-source server the owner can run or join.
- 🔄 Server: our own open-source course server `mmk-server` ✅ (directory store, REST API: upload
  with validation, search/sort, download, play/clear/like stats → difficulty, 100 Mario pools by
  fail-rate band; `mmk server`) and client crate `mmk-online` ✅ (`mmk upload/search/download`,
  `RemoteSource` for 100 Mario) · accounts, makers, rankings, SMMDB import ⬜ · ⏳ owner: keep this or
  connect to another open-source server
- ⬜ Course World client: browse, play, like/boo, comment, upload from the editor
- ✅ Local multiplayer (co-op, up to 4: Mario, Luigi, Toad, Toadette as the models' _2/_3/_4
  variants): extra players run the same rules, rejoin beside player 1 [inf: bubble], any player
  clears; `mmk-play --players N` (one pad each), FFI `mmk_engine_set_players / step_players`;
  `World` is `Clone` and replays deterministically (rollback groundwork) · versus, bubbles ⬜
- 🔄 Real-time online play: rollback netcode crate `mmk-netplay` ✅ (predict, restore snapshot,
  re-simulate; redundant input packets over UDP; test: two peers with 4-frame delay and 1/3 loss end
  identical to a full replay) and `mmk-play --net-bind/--net-peer/--net-player` (2 players) ·
  4 players, NAT traversal / relay via the server, WebRTC for the web, versus ⬜
- ⬜ Accounts without Nintendo services; moderation tools for self-hosters

---

## Phase 1 detail

### Done
- Repo initialised; remote `aksmith-git/MarioMaker` (private). `.gitignore` excludes dumps, keys,
  extracted data and `research/`.
- Proprietary-content guard (`scripts/check_no_proprietary.py`) + tests + CI workflow + pre-commit hook.
- Toolchain installed and pinned outside the repo — see [docs/TOOLCHAIN.md](docs/TOOLCHAIN.md).
- `tools/inventory.py`: identifies extracted files by magic (through Yaz0/zstd), looks inside SARCs.
- **SMM1** (WUX disc image) decrypted with console common key + disc key:
  - `~/smm-work/extracted/smm1-disc/` — base game v16 (`000500001018DC00`)
  - `~/smm-work/extracted/smm1-update-v208/` — update v208 bundled on the disc
  - Detailed inventories: `research/inventory/` (private)

### SMM1 layout summary (base v16 + update v208)
| Area | Where | Format |
|---|---|---|
| Main executable | `code/Block.rpx` (v208: 9.0 MiB) | Wii U RPX (PPC, big-endian) |
| Course data | `content/Course/*.szs` (152: sample, tutorial, title, new) | Yaz0 → SARC → `.cdt` (SMM1 course) |
| Course thumbnails | `content/CourseThumbnail/*.szs` | Yaz0 → SARC → `.tnl` |
| Tilesets / scenery | `content/Model/M1_Field_*`, `M1_DV_*` etc. per style | Yaz0 → SARC → BFRES |
| Sprites / characters | `content/Model/*_Player_Chara_*`, `Pack/*.pack` | BFRES, SARC packs of Yaz0 SARCs |
| Per-style environments | `content/Pack/{M1,M3,MW,WU}_FieldEnv_{Normal,Dark,Water,Yogan}.pack` | SARC |
| Physics / object params | `Pack/*.pack` → `.params`, `.byaml` | text params, BYML (to be confirmed in Phase 3) |
| Text | `content/Message/*_USen.szs` | Yaz0 → SARC → MSBT |
| Layout / UI | `content/Layout/*.szs` | Yaz0 → SARC → SARC (BFLYT/BFLIM) |
| Music | `content/Sound/**/*.bfstm` (147) | BFSTM |
| SFX | `content/Sound/*.bars` (193), `Sound.pack` | BARS → BFWAV |

Styles: `M1` SMB1, `M3` SMB3, `MW` SMW, `WU` NSMBU. Themes: plain, underground, castle, airship,
water, haunted house.

- **SMM2** from `MarioMaker2.NSP` (owner's dump; v1.0.0, title `01009B90006DC000`, SDK 7.4.0, no update):
  - `~/smm-work/extracted/smm2-nsp/exefs/` — `main` (NSO, 17 MiB), `sdk`, `rtld`, `main.npdm`
  - `~/smm-work/extracted/smm2-romfs/` → symlink to `/Volumes/SSD/smm-work/extracted/smm2-romfs` (2.7 GiB; internal disk is full)
  - `SMM2.xci` is NOT used (it is the `[romstorage.com]` file renamed).
- **Course sample**: `tools/fetch_courses.py` pulled 550 courses into `~/smm-work/courses/mm2_level/`
  from the public `TheGreatRambler/mm2_level` dataset (CC BY-NC-SA 4.0): 25 per style x difficulty
  plus 50 extra SMB1 ground courses. All from June–July 2019 uploads, so they only use v1.0.0 content.
  All 550 decompress to 0x5BFC0 bytes and their headers (timer, style, name) match the metadata.

### SMM2 layout summary (v1.0.0)
| Area | Where | Format |
|---|---|---|
| Main executable | `exefs/main` | NSO (AArch64); Ghidra Switch Loader |
| Built-in courses | `Course/*.szs` (120 story `quest`, 50 `lesson`, 10 `title`) | Yaz0 → SARC → `.bcd` (to confirm in Phase 2 whether encrypted) |
| Online/save courses | (from dataset) | 0x5BFC0-byte decrypted course; on-console `.bcd` is encrypted |
| Tilesets / scenery | `Model/{M1,M3,MW,WU,3W}_Field_<theme>`, `_DV_` (background), `_S_DV_` | zstd → SARC → BFRES (+ BFSHA) |
| Theme animation | `Pack/<style>_FieldAnime_{Normal,Dark,Ice,Snow,Water,Yogan}.pack` | SARC |
| Sprites / objects | `Pack/<style>_Model.pack`, `Static*.pack`, `Model/` | SARC of zstd SARCs → BFRES |
| Params / physics candidates | `Pack/*.pack` → 423 BYML; `Bake/*.bkres` | BYML; SARC (to be mapped in Phase 3) |
| Text | `Message/*.szs` | Yaz0 → SARC → MSBT (3038) + MSBP |
| UI | `Layout/*.szs` | Yaz0 → SARC → SARC (BFLYT etc.) |
| Music & SFX | `Sound/Resource/**` (471 BWAV, 2.1 GiB), `Pack/<style>_bars.pack` | BWAV, BARS |
| Sound delay/env per theme | `Sound/Effect/delay_<style><theme>.byml` | BYML |

Styles: `M1` SMB1, `M3` SMB3, `MW` SMW, `WU` NSMBU, `3W` SM3DW. Themes: plain, underground, castle,
airship, water, hauntedhouse, snow, desert, athletic (sky), woods.

- **SMM2 update v3.0.3** (`Mario Maker2 Updatev3.0.3.nsp`, patch title `01009B90006DC800`, NACP display
  version 3.0.3) applied over the v1.0.0 base:
  - `~/smm-work/extracted/smm2-v303-romfs/` (3801 files, 2.8 GiB; adds `MyWorld/`, +22 models, +3 packs,
    +344 sound files, +123 layouts) and `~/smm-work/extracted/smm2-v303-exefs/` (`main` 20 MiB NSO)
  - ELFs for analysis: `/Volumes/SSD/smm-work/extracted/smm2-v100-main.elf`, `smm2-v303-main.elf`

### Open
- Owner's own saved courses: not yet exported from the Switch; the dataset sample covers Phase 2 for now.
