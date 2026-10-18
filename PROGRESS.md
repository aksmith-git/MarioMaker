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
  object sounds beyond the slice ✅ (Phase 7 "Gameplay sound from the game's own sound links")
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
  conveyor belts; Spiny, Buzzy Beetle (+ fire-proof shell), Spike Top (crawls floors, walls, ceilings), Hammer Bro +
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
- 🔄 7. Tune against Ryujinx ✅ frame-exact loop: a locally patched Ryujinx (1.3.2 + a trace hook on
  the player's per-frame update, scripted pad state per emulated frame, private data root) runs
  the owner's SMM2 v3.0.3 on physics test courses (`mmk trace-courses`, played as title courses);
  `mmk trace` runs the same input script in the sim and the two are diffed frame by frame (tooling
  private). 454 recorded tests over all five styles (walk/run/skid, every jump height, air turns,
  jump chain, crouch/duck/crouch jump, spin and 3DW long jump, ground pound, wall slide/jump,
  ledges, slopes, swimming, Super Mario, style power-ups): 220 matched exactly before, 428 now
  (landing frame, jump-press retry, CrouchEnd, 3DW run timer, slope geometry, wall
  slide only in NSMBU/3DW, ledge fall, …) · ⬜ power-up specials (cape glide/flight, raccoon
  flight, propeller, cat attack), slide end on flat, pit-death height, enemies, real courses
  start to goal

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
  - ✅ Reference frames from the owner's SMM2 in Ryujinx (private root, LayeredFS title-course
    replacement, in-process key / grab dylib; scripts outside git) and `mmk-play --size
    --view-at` diffs: 24 dataset courses (WU all themes but water + nights, 3W all themes),
    mean MAE 61.1 → 49.4 (8× blurred 58.2 → 45.2)
  - ✅ Game cameras from the binary: 3D World field perspective (fovy 12.5°, raised 35 px,
    looking 5 px up), NSMBU orthographic; background scenes (`_DV_` models) with the DV camera
    from SceneDB (`DV_Play_Fovy`, ProjOffsetY / ProjMoveY / CamMoveY), own depth pass; the 2D
    backdrop gives way to them
  - ✅ Environment lighting / fog per theme (`env/*.json` from Env/Cmn(_DV).szs, AAMP reader),
    materials: blend / alpha test / faces, self-lit, emission, alpha masks, sky palette,
    world-space texture coordinates (castle walls), TEV tint, texture SRT, vertex colours;
    WU tiles use whole atlas cells (UVs were a quarter cell); big actors' scale
  - ⬜ Per-actor animation clocks and transitions (time = world tick), 3D World neighbour variants
    for terrain edges, pipes / note blocks as models, small 3D World Mario's cap; the game's
    light maps (runtime cube maps) and distance-fog formula (3W snow / airship too foggy), WU
    background tints, liquids (lava / water as the 2D sprite), normal / specular / bake maps,
    bloom and colour correction, the sim's start camera in normal-boundary courses (~30 px high)
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
  0.03 easing, pauses during warps/death/goal; the player is pushed at the edges); the main area's
  path starts at the start unit and ends at the goal unit, so a course without nodes still
  scrolls (binary pass 3)
- ✅ Binary pass 3 (course loading): slopes as caps + diagonal (flush with the ground), spike
  block colours (0 red / 1 blue ON/OFF, 2 timer), cannon facing + aim, goal pole over the goal
  ground's first column, angry alternate Wiggler
- ✅ Conveyor slopes (steep slope + belt carry); winged/tracked coins and note blocks —
  SMB1 sample 148/150 playable (left: a Bill Blaster and a Burner on tracks)
- ✅ Theme music per style: the importer copies every style's theme / hurry / star tracks (105
  streams); the player decodes only the course's tracks into per-course mixer slots · per-style
  sound effects ✅ (style variants of SFX and fanfares, SMB1 fallback), sub-area music ✅
- 🔄 Gameplay sound from the game's own sound links (DECISIONS.md "Gameplay sound from the
  game's own sound links"; private `research/specs/audio.md` §5–§11, `audio-triggers.md`):
  - ✅ formats: SLink2 users with containers (switch / random / blend / sequence), conditions,
    random ranges, property curves, user params, action triggers; the group tree (AGST),
    attenuation sets (AATN / AROC / AACL)
  - ✅ import v7: `audio/sound.json` (616 gameplay users, groups, attenuation, per-theme
    effects, ground materials) + 2430 effect waves (`audio/se/`) + 234 streams
    (`audio/music/`, game names); `mmk import-sound <romfs> <cache>` runs only this stage
  - ✅ mixer: per-voice volume / pitch / pan / delay / duration / fade-in / low-pass, group
    voice limits with priority stealing and folder limits, ducking, theme echo + reverb sends,
    four-channel music with a fade-in layer (Yoshi drums), DSP-ADPCM music decoded while
    playing, music position clock, final limiter
  - ✅ director (host `sound.rs`): keys per style / theme / day-night / object; player events
    (jump by form, triple-jump count, swim, spin, wall kick, power-up / item / damage, pipe,
    door, head bump, pole, checkpoint, clear condition), player states (landing, splash in /
    out, ground pound, vines, shoes, Yoshi, skid, propeller, footsteps by ground material),
    objects (coins incl. 10/30/50, blocks, ON/OFF, P switch, 1-Up incl. 100 coins), actors
    from their state changes (stomp with combo / big / wings, defeat, Koopa into shell, kick,
    shell wall hit, Thwomp landing, Bill Blaster shot, out of a block); music: area theme
    track (day / night, 3D World day only), hurry jingle → hurry track, Super Star / P switch
    tracks with the theme track resumed at its position, star end sign at 120 frames, death /
    clear / castle clear fanfares, sub-area music and effects, theme ambience (NSMBU / 3D
    World), underwater filter for sounds below the water surface, 3D World distance
    attenuation and culling, pan by screen position [inf]
  - ✅ `mmk audio-render <course> --cache DIR --out x.wav [--log] [--only sfx|music]`:
    headless scripted run through the same director and mixer; a rendered SMB1 jump matches
    the game's wave exactly (correlation 1.000)
  - ⬜ springs, note blocks (pitch by height), POW, Bob-omb, Burners, cannons, projectiles,
    bosses' actions, keys, pink coins, objects' splashes and parachutes, star / shell / spin /
    Yoshi kills, cape / tail / cat / hammer actions, 3D World voices, Yoshi's own sounds,
    costume music; checks against the running game (pan, limits, effect sends)
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
- ✅ Bosses pass 3 (from the binary): shell slide / wakeup, Clown Car steering + ride heights,
  Jr.'s clown damage and thrown Koopas, Pom Pom's throw / hide / Lapse + homing shuriken, NSMBU
  anim lengths, boss Key → key event · Bob-omb blast type, Pom Pom decoys ⬜
- ✅ Lava Bubble (hide / cosine rise / fall), Monty Mole (buried, mound, leap, chase), Angry Sun
  (corner wait, spin, half-ellipse swoop) and Moon (wipes on-screen enemies)
- ✅ Tracks: graph from track records (lines + quarter arcs), rail follower (0.75 / 1.5 px/f),
  any actor rides (lifts, seesaws, grinders, claws, items, enemies); drawn as lines · junctions,
  open ends, blocks/coins on tracks, exact track tiles ⬜ — SMB1 sample 49 → 52 playable
  - ✅ Pass 3: the game's rail grid (record templates, end words, cell words) and its follower
    (cell-centre walk, 2×2 bends at r 24, closed / open ends, start field), junctions 8–15
    switched by ON/OFF — all 657 corpus riders attach, every fall is at an open end · re-catch
    of a falling rider onto a lower track (0x7100343680) ⬜
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
  below the HUD; `--check-textures` agrees with the GPU on every BC texture. Layouts ✅: the
  layout player's quads drawn by `ui_layer` (port of `UILayer.swift`: two-texture combine, black /
  white colours, per-quad blend and sampler); `mmk-play --layout NAME --ui-cache DIR [--ui-anim]
  [--ui-text] --screenshot` matches the Swift app's MMK_UI_LAYOUT preview (SMM1 Cmn_GameOver_00,
  SMM2 Play_Title_00 under three animation specs: ≤ 4 px off by ≤ 3 levels). Open: the SMM font
  HUD, clear-condition HUD text, menus/library (presentation flow lives in Swift today)
- ✅ 3D models in the portable shell: the mesh planner moved to `mmk-host::mesh` (shared with
  the Swift app); `mmk-play` skins on the CPU (works on every wgpu backend, WebGL2 included) and
  draws meshes with a depth buffer, interleaved with sprites by layer like the Swift renderer;
  `mmk-play --map` renders whole courses, `mmk gallery` / `mmk missing-art` check every part's art
- 🔄 Windows and Linux builds of the shell; CI jobs on Windows + Ubuntu (clippy, tests, release
  binaries) and wasm32 on Ubuntu. Windows type-checks locally (clippy, MSVC target); not yet run
  on Windows or Linux hardware
- 🔄 Web: wasm32 build of core + shell in a canvas (WebGPU, WebGL2 fallback), `scripts/build-web.sh`.
  Playable with the owner's own data (docs/architecture.md §5b "Web"):
  - ✅ Import game data from a picked folder into OPFS (never uploaded, kept between visits):
    an `mmk import` cache is copied (~15 s), or the dump's `romfs/` + `exefs/` is imported in
    the page by `mmk-import` itself in a worker (~35 s, ~1.5 GB peak; output identical to the
    desktop import); textures stream from storage on first use, music per course
  - ✅ WebAudio (cpal) through the shared sound director, started on the first gesture
  - ✅ Gamepad API (gilrs) and keyboard; course picker: built-in courses of the import, or any
    `.bcd` / zlib / raw course file
  - ✅ SMM2 menus (`mmk-front`) when the SMM2 UI import is in storage: copied from an
    `mmk import-smm2-ui` folder, or made by the in-page dump import (identical to the desktop
    import, all 4528 files); the UI cache reads JSON up front (13 MB, ~0.4 s) and textures on
    request; opening, title over its demo courses, Play menu, Course World (the library: built-in
    courses + opened files), Story Mode, 100 Mario, course card / HUD / pause / give up; UI sounds,
    course music, keys, mouse clicks; saves in `localStorage`. No server listing in the browser
  - checked in headless Edge (CDP: import, courses, keys, a scripted Gamepad API pad, non-silent
    WebAudio, the menus) and headless Firefox (WebDriver BiDi; before the menus). Open: Safari
    (OPFS `createWritable`), a real controller in a browser, touch controls, mobile memory
    limits, the course server from the browser (HTTP), Firefox with the menus
- 🔄 iOS/iPadOS: the same Swift package cross-compiled (`swift build --triple`, core linked from
  `core/target/aarch64-apple-ios{,-sim}/release`), bundled by `scripts/make-ios-app.sh`
  (DECISIONS.md "iOS packaging"); `scripts/check.sh --ios` builds it for the Simulator. AppKit
  bits split behind `#if os(macOS)`; on iOS: UIKit MTKView hosts, touch controls (d-pad, Jump /
  Run / Spin, + / −; multitouch, sliding between buttons, hidden while a controller is
  connected), hardware keyboards through `GCKeyboard` with the same bindings, controllers as on
  macOS, data folder = Documents (shown in Files: the owner copies a cache made on the Mac into
  `assets/`, or picks one and the app copies it in), Files picker for courses, settings sheet,
  compact course list on iPhone. GPUs without BC support get BCn decoded by the core
  (`mmk_texture_decode_rgba`; BC7 decoder moved from `shell` to `formats`). Verified on the
  Simulator (iPad Pro 11" and iPhone 18 Pro, iOS 27): placeholder art, the real cache (found
  through the Mac's home), CPU texture path, title screen, library. Open: signing + a run on the
  owner's iPad (`ios-install-tailscale <app> --device ipad` once signed), tap-to-select in layout
  menus, iCloud placeholder files in picked folders, the in-app importer on iOS
- 🔄 Engine-drawn menus everywhere (layout player), so every platform shows the same screens:
  ✅ the SMM2 front end is Rust (`mmk-front`: opening, fades, title + Play menu, Course World over
  a library folder and the course server, Endless / SMM1-layout 100 Mario, in-course HUD / card /
  pause / settings / clear / give up / time up / game over, UI sounds) and `mmk-play` runs it by
  default (Windows, Linux, macOS); headless captures (`--front-screen`) match the Swift app's to
  within rounding on every deterministic screen. ✅ the Swift app runs `mmk-front` too, through
  the `mmk_front_*` C API (DECISIONS.md "The Swift app runs the Rust front end"): its own screen
  classes (~4 500 lines) are deleted; it keeps its renderer, audio, input, touch controls and the
  SwiftUI library / Settings. Its DebugCaptures before vs after on the 30 front-end screens of the
  comparison set (macOS, 1024×640 window): 27 pixel-identical, course-pause 0.11 % and
  course-settings 0.11 % of pixels (the selection frame's Loop phase), smm1-walk 0.35 % (the
  random course name on the map's balloon); real-time navigation (hub → list → course page →
  play) and the `MMK_SCREENSHOT` smoke test run; `scripts/check.sh --ios` builds. ✅ the web build runs
  it too (UI cache over an in-memory file source filled from OPFS, textures on request). Open:
  Settings screens, ghosts in the Apple app, a server listing in the browser (HTTP fetch)

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
    Course World (SMM2 screens, below), Coursebot (library), 100 Mario Challenge (extra button);
    game cursor, logo colour by course; keyboard + controller; screenshots `MMK_UI_SCREEN=title*`.
    From the binary (private `research/specs/smm2-frontend.md`): the layout graph, the cursor on
    Make at the prompt, on Story Mode when the Play menu opens, on Play after going back
  - ✅ SMM2 front end from the game's scene code (v3.0.3, private notes above):
    - ✅ opening `Boot_BootScene_00` at launch: the boot page's choice of opening (update
      openings first, then 66 / 8 / 8 / 2 / 8 / 4 / 4 %), InMain, then the black circle into the
      title (Settings > Game, `MMK_BOOT`, `MMK_BOOT_OPENING`)
    - ✅ scene fades `Cmn_Fade_00` (black 30 + 30 for scene changes without parameters, black
      circle from the opening; white, blocks available) over every screen (`SceneFade`)
    - ✅ Course World: hub `Nwk_Title_00` (background loops, Courses / Leaderboards / Endless
      Challenge / the player card; Network Play, Ninji Speedruns, Super Worlds say they need
      Nintendo's network), course list `Nwk_CourseListGeneral_00` (Hot / Popular / New tabs, rows
      with style logo, maker, likes, plays, tags, date, clear mark; open row with description,
      world record, clears / attempts, course ID, Play, and for server courses More info / View
      comments) over the library (dataset index statistics) and the open-source course server
      (Settings > Game; `mmk_online_*`); the online screens of Phase 10 in `mmk-front`
    - ✅ clicks / taps (`FrontInput::click`) on the title, the hub, the course list (tabs, rows,
      an open row's buttons), the online screens, the Endless menu and map, and the SMM1 100
      Mario screens; text entry (`FrontInput::text`) for names, tokens and comments
    - ✅ 100 Mario Challenge in SMM2's Endless Challenge screens (default; SMM1's kept in
      Settings > Game): `Nwk_EndlessModeMenu_00` difficulty tabs (SMM1 unlocks), New Game /
      Continue, the map `Nwk_WorldMap_00` (ChangeMap, Telop, Mario out of the castle, walks
      between dots, InClear, theme icons, Play balloon, his jump), course card and HUD with lives,
      pause Start Over / Quit, clear Quit / Next, game over, the skip countdown
      `Play_SlidePassIconKey_00`
    - ✅ library / Course World / Story Mode play without lives (owner's decision): no game over,
      the death iris, HUD Type 0, card AnimDefault
    - ✅ pause Settings `Cmn_OptionWindow_00` (offline variant): character Mario / Luigi / Toad /
      Toadette (`mmk_engine_set_character`)
    - ✅ menu sounds from the game's data (import v4: SLink2 users + `Param/Layout` sound types →
      `ui/sound/`): buttons, animations (the opening's sound sequence and title call, the course
      card jingle, the pause jingle per style, the Endless map's Telop / dots / Mario)
    - ⬜ Coursebot (`Lcl_*`), Story Mode map (`Qst_*`), Make, the main menu `Cmn_MainMenu_00`
      (the editor's), SMM2's own Endless Challenge rules (endless run, SMM2 lives), Leaderboards /
      Ninji / Super Worlds over the course server, layout scissor clipping, CJK glyphs in course
      names, the pause blur / clear capture effects, menus in `mmk-play`
  - ✅ in-course screens over library courses (`SMM2Course.swift`, DECISIONS.md "SMM2 in-course
    screens"): HUD `Play_PlayScene_00` per style skin (lives + character icon, coins, score,
    timer with the 100 s pulse, clear-condition icon + counter / failed mark, pink-coin slots;
    keys as `Play_KeyIcon_00` under the score [departure]), course card `Play_CourseIn_00` on
    black (name, maker, condition sentence + icon, lives) at the start and after each death,
    pause `Cmn_PauseMenu_00` (close = Continue, Start Over / Retry after a checkpoint, Exit
    Course; course info, condition, best time; game cursor), clear `Play_CourseClear_00` +
    `Play_ClearInfo_00` (banner, clear time, Start Over / Quit), give up `Play_CourseRetire_00`
    (Exit / Keep playing), "TIME'S UP!" `Play_TimeUp_00`, `Play_GameOver_00`; the drawn HUD and
    SwiftUI menus stay as the fallback (`MMK_UI_SMM2=off`, no import); captures
    `MMK_UI_SCREEN=course-*`
  - ✅ import v2: ReferenceMsg tables (clear-condition sentences, part names), the five styles'
    clear-condition icons (`ui/textures/MissionIcon/`); Switch bitmap-font sheets flipped
    (stored bottom row first) and read as luminance; player: caller picture textures, material
    colours, text width; FFI `mmk_ui_table_message`, `mmk_ui_set_picture`,
    `mmk_ui_set_material_color`, `mmk_ui_text_width`, `mmk_engine_play_status`
  - ⬜ capture-based effects (the clear / give-up wipes, pause blur: PF_* panes), the 3D player
    figure on the course card, P-meter / coin pop / 1-Up animations, checkpoint-flag and
    condition-met pop-ups in the course, likes / comments (network)
  - ⬜ FLCT per-character text, glyph borders; title and in-course screens in `mmk-play`
- 🔄 All five styles (SMB1, SMB3, SMW, NSMBU, 3D World) and all themes (day/night) — all five
  playable — sample 3D World 100/100
- ✅ Warp Box (97-warp-box.md): pairs 2k ↔ 2k+1 in the same area, entered by touch (no button),
  player hidden at once, held items dropped, key boxes spend one key and stay open, a used pair
  vanishes (single player); blocked-exit cancel ⬜
- ✅ Koopa Troopa Car (72): drop in from above; start spin, then it runs by itself (≥ 2.0, 3.0
  with the travel direction held [inf]), steering only by a 30-frame turn; jump hops it [inf];
  Spin gets out; runs enemies over; hits damage the car (third breaks it) [inf]; "goal in a
  Koopa Troopa Car" condition. Enemy drivers ⬜ (none in the sample)
- ✅ ! Block (119): 2×2 solid with a drawn path; a hit from below bounces it (7-frame ramp),
  knocks out enemies on it and grows a chain of 2×2 blocks along the path, restored after 600 frames
  - ✅ Pass 3: the chain as decoded — one segment per path point, a head hit slides one out
    (3 px/f), a ground-pound-type hit sends runs to each turn; snake / track-block path codes are
    steps into each node, end codes included · world wiring of `hit_all` (ground pound) ⬜
- ✅ Tree (106): the trunk climbs like a vine up into the canopy, the canopy is a jump-through
  platform, reaching it knocks out the canopy item (vy 4.5) [inf: trigger]
- ✅ Mushroom Trampoline (113): top-only platform N tiles wide; landing launches at 6.0 (4.43 with
  jump held), a jump within 6 frames re-launches at 4.43 − 0.1/frame; enemies bounce 5.5 (6.5
  winged); unflagged ones sway as lifts [inf]
- ✅ Crate (112): solid to stand on, picked up with Run beside it, thrown (3.5 px/f [inf]) it
  breaks on a wall or an enemy and releases its contents (vy 3.5)
- ✅ Track Block (85): 2×2 solid on its own path (2 tiles per node [inf]), sets off when ridden,
  0.5 px/f while ON, holds while OFF, 60-frame stop at the ends, then back (normal) or blink
  180 f, vanish and respawn at the start (alternate)
- ✅ 3D World enemies (actors/w3.rs, world/w3.rs): Ant Trooper (wall/ceiling crawl, horned),
  Stingby (patrol, alert, charge), Skipsqueak (+ Spiny; jumps with the player), Hop-Chops,
  Porcupuffer (4 HP, homing, surface patrol), Bully (push-back, ledge teeter), Charvaargh (lava leap with
  8 segments), Piranha Creeper (path creep, sleeping form) — guesses burnt down against the
  binary (2026-10-18): animation-timed states, Charvaargh's real arc, creeper pieces and node table,
  Bully push table / sight box / ledge teeter; second pass: Charvaargh's vertical and chained
  leaps, turns and spacing, creeper PreStop and trample rules, winged Bully and Ant Trooper
  flight, the horned Ant Trooper's top; what is left is listed in DECISIONS.md
  - ⬜ Hook `World::w3_charvaarghs` into `World::step` (several Charvaarghs' turns and spacing);
    Piranha Creeper stalk as solid terrain, block bumps trampling the head
- ✅ Fire Bars, Burners, airship Cannons and Bill Blasters on tracks: a carrier actor rides the
  track as the moving solid and the fixed object follows it — sample 550/550 by the coverage check
- ⚠️ The 550-course sample dates from June 2019 (game versions 1–3, before the 2.0/3.0 updates),
  so "550/550" says nothing about the update parts: 74, 78–81, 111, 120–132 and the Master Sword
  are absent from it and not yet played (🔄 enemies/blocks in progress; power-ups after Phase 11)
- ✅ Story Mode parts: the stone (75, heavy carry: thrown ≤ 0.8 px/f, never breaks), the escort
  Toad (73, follows 32 px back along the player's path; losing him fails the run [inf]), Toadette
  at the goal (115, scenery); conditions: shake N trees, goal holding the stone, goal on a Lakitu's
  Cloud, no swinging claw, no water — built-in courses 159 → 177/180
- ✅ Empty shells (the 0x4 forms of Buzzy Beetle, Spiny, Dry Bones): never wake; Buzzy / Spiny
  shells met with the head become helmets (take one hit; the Spiny one knocks out what it rises
  into), the Dry Bones Shell is ridden like a shoe [inf: mechanics not decoded]; Buzzy shells now
  drawn with their own art; helmet / Dry Bones Shell goal conditions
- ✅ Alternate forms (flag 0x4) that were silently ignored: Goombrat (turns at ledges), sideways
  Spring (flings 4.5 px/f [inf]), Bull's-Eye Blaster / Bull's-Eye Banzai (homing 1° / 0.25° per
  frame for 528 f), red Cannon (2.4 px/f balls), fast Spike Top (0.75), lit Bob-omb (walks lit),
  Boo circle (8 Boos, 52 px), Blooper Nanny (4 babies on her path), Sledge Bro (hover, hip-drop,
  landing quake stuns), Superball Flower (SMB1: ball reflects at 3 px/f, 270 f, collects coins),
  red POW (breaks on-screen bricks [ext]); the coverage check now reports any unhandled 0x4 form
- ✅ Big Mushroom / Big Mario (SMB1 part 44): the item walks like a mushroom; Big Mario has the
  class-3 sensors (feet ±9.9, wall 11.9) and body box (±12), drawn 32×32, knocks out what he
  touches and breaks bricks he walks into [inf]; "goal as Big Mario" condition
- ✅ Winged Hidden Blocks: invisible flying blocks revealed by a bump — built-ins 180/180
- ✅ Super Hammer / Builder Mario (116): item walks like a mushroom [inf]; Run swings the hammer
  (hits enemies and breaks blocks in front), air swing falls ≤ 1.5 and |vx| ≤ 1.125; "goal as
  Builder" condition; drawn with the builder outfit + hammer meshes. Crate building ⬜
- ✅ Super Bell / Cat Mario (3D World): the bell walks like a mushroom; wall climb (1.0, 1.5 with
  run, ≤ 120 f / 120 px, top-out vy 4.8, then WallClimbFall −0.085 to −2.0), dive (Down in the
  air: 4.0 forward, −3.0), slide scratch (2.5 for 12 f) and air scratch (vy 2.4, gravity −0.1, once
  per air time); "goal as Cat" condition; drawn with the cat suit's meshes and Climb animations
- ✅ 3D World two-stage run: 2.25 until the dash stage (75 f, 60 as Cat), then 3.0 (star 3.6),
  ×1.05 jump while dashing
- 🔄 Every part (0–132, incl. 2.0/3.0 items from the update dump): the 2.0/3.0 enemies, blocks
  and gizmos ✅ (below; from the v3.0.3 binary, editor-built test courses in
  `crates/editor/tests/v3_parts.rs`, `MMK_WRITE_COURSES=<dir>` writes them) · the update
  power-ups ✅ (below; `crates/editor/tests/v3_powerups.rs`) · Koopalings in Clown Cars ⬜
  - ✅ Spike (74): SMB1/SMB3 Gabon3rd walks 0.5 and throws in bursts of 2 when the player is
    32–288 px ahead (ball flies level at 1.5); SMW/NSMBU/3D World GabonWU stands, faces the player
    (3D World 50-frame Turn), waits 0–59 then 120 (SMW 160) + 0–29 frames, lifts and throws on the
    style's animation frames; balls roll 1.2 (slopes speed them up, walls break them, landings
    bounce ×0.6); snow theme: snowballs that are kicked (1.7–5.2) or carried; the 0x4 form is the
    ball itself
  - ✅ Pokey (78): 1–8 segments (default 4), 0.25 px/f, turns at walls and ledges, ±1.5 px sway;
    stomps only on the head (whole stack), fire/shells/tails/slides knock out one segment and the
    ones above hop down; Snow Pokey (ground pound defeats it, Run picks up a snowball); winged hops
  - ✅ Mechakoopa (111; kind = (flags >> 18) & 3): walks 0.5, re-aims every 60–80 frames; stomps
    and ground pounds knock it down for 480 frames (slide/bounce rules, shake below 61, 18-frame
    revival), kicked or carried and thrown while down; fire-proof; Blasta's missile (three-phase
    homing, explodes at frame 300 breaking blocks like a Bob-omb) and Zappa's laser (69-frame
    telegraph, grows 10 px/f to 320 for 40 frames); winged patrol; night-Ground hops
  - ✅ P Block (79, 3D World): solid / outline swapped while a P Switch runs (both kinds); P Switch
    time 540 frames from the binary
  - ✅ Dash Block (80, 3D World): standing on it sets a 120-frame dash at 4.0 px/f along the
    player's facing (jumps keep it; a wall or a turn ends it)
  - ✅ ON/OFF Trampoline (132, 3D World): bounces like the Mushroom Trampoline only while its
    colour's ON/OFF state is on, else a plain platform
  - ✅ Koopalings (120–126): shared action (3 HP, 5 fireballs per HP (big 10), hop between
    volleys of wand magic (1/2/3 shots at 3/2/1 HP, aimed ±10° within 60°), shell after a hit:
    65-frame wait then 1–3 shell hops (NSMBU: 60-frame flinch, 300-frame shell slide), Key drop);
    Larry (higher 1-HP jumps), Wendy (bouncing rings, 1/2/3 alive), Iggy (walks, fast magic),
    Lemmy (rides his ball, 1/3/5 bouncing balls), Morton (ground pound: 80-frame stun, two
    crawling flames), Roy (dives into the ground and comes up elsewhere), Ludwig (hovering shell,
    fans of 3/5 slow shots) · Clown Car rides, Roy's ceiling exits, Morton's winged feints, wing
    flight ⬜
  - ✅ Update power-ups (v3.0.3; `sim/src/player/powerups.rs`, `actors/powerups.rs`,
    `world/powerups.rs`): part 81 gives the style's form (SMB1 SMB2 Mushroom, SMB3 Frog Suit,
    SMW Power Balloon, NSMBU Super Acorn, 3D World Boomerang Flower); a ? block swaps it for a
    Super Mushroom for small Mario, but not the Master Sword or the wearables; every new form drops
    to Super when hit; Link and the balloon have the small body
  - ✅ SMB2 Mushroom: stands on carryable enemies [ext], Down + Run lifts them (3 / 23 frames, carry
    height to 25 over 12 frames), letting go of Run throws (2.2 px/f, up to 4.0, gravity −0.1875)
    and thrown enemies knock out what they hit; 60-frame crouch charge → vy 4.1 with the held-jump
    gravity (≈ 98 px); snowballs pulled from snow ground · cloud pulling ⬜
  - ✅ Master Sword (part 20 with flag 0x4 in SMB1) / Link: 8-frame slash (23 × 15 box, the tip
    breaks blocks on frames 3–7), Down + Run dash (60-frame wind-up, then 3.0 px/f while Run is
    held, wall recoil), shield while Down on the ground (blocks projectiles, turns enemies), bow
    on Spin (24-frame draw, arrows 5.0 / 0.9 straight, 3.5 / 4.8 up, 3.5 / −2.0 down, gravity
    −0.12, drag 0.038, bounce off terrain), down-thrust (12 + 5-frame hang, drop at −6.0, Up
    cancels, bounces off enemies), Up + Run pulls a LinkBomb (Bob-omb blast); EnemyDB sword /
    sword_under / shield / arrow reactions
  - ✅ Frog Suit: 20-frame hops / 10-frame waits on land (3× accel, ice has no effect, no slope
    slide), every jump 3.928; free 8-way swimming 1.0 (Run 2.0) with no gravity, kept 17 px under
    the surface, jump out from the surface
  - ✅ Power Balloon: floats 0.5 px/f along the stick with no gravity or timer, Run jets 2.0 (1.5
    in water; sky-night 3.5) along the tilt, which turns 6° / 5° a frame; walls, floors and
    ceilings bounce it (jet lockout 40); stomps only drift it
  - ✅ Super Acorn: Spin in the air boosts once per air time (vy 2.0, turning to 67.5° while the
    speed grows to 4.2 × base for 15 / 20 frames, then a −0.12 coast), glides with jump held (fall
    cap −2.0 … −0.65 by speed), flutters at −1.5 once the boost is used, clings to walls for 90
    frames; ground spin jump 4.15
  - ✅ Boomerang Flower: one boomerang at a time, 5.0 px/f out 160 px, 15-frame stops, back past
    the thrower 80 / 60 / 40 / 20 px, caught after 15 frames; wall rebound, items hop
    ("BoomerangReaction"), EnemyDB boomerang reactions and bound_boomerang
  - ✅ Wearables (3D World, 127–131): put on by touch (small Mario too), take one hit; turn off
    fireballs, boomerangs and the cat's moves; Cannon Box (30-frame cooldown, 3.0 / charged 5.5
    shots for 30 frames, 32 × 32 blast), Propeller Box (3 flights per air time, 15-frame rise to
    2.5), Goomba Mask (hidden from Goombas / Boom Boom), Bullet Bill Mask (300 frames of 3.0 px/f
    dash per air time while jump is held, breaks on walls), Red POW Box (3 head bumps: POW quake
    and an 80-px block-breaking ring) · the boxes' worn models drawn above the head
  - ✅ Binary pass 3: in-view flag (box touches a camera view, edges in), Spike balls leave the
    model's GeneratorPosition joint, Mechakoopa weapons start at its feet (no "shot" joint),
    Zappa's Shot ends with the beam's (38 f), winged Pokey hop counter resets, Roy one exit try
    per frame, quake stun skips invincible players; missile blasts, carried and thrown parts hit
    like shells; a Bully pushes a shell back; a plain stomp kicks a snowball
- ⬜ Yoshi variants, shoes, clown cars, etc. (rest of the "every part" list)
- 🔄 Course Maker (editor): core crate `editor` — each step tested by saving the course, re-opening
  it byte-identically and playing it in the sim (`unsupported_features` empty):
  - ✅ parts: place / erase / resize (semisolid, mushroom platform, bridge, pipe, belt, lift, slopes,
    vine, Bill Blaster, seesaw, trampolines, snake block, tree) with sizes, forms and composition from
    the game's editor part table; big parts stored doubled; child items; ground auto-tiled
  - ✅ both areas: main / sub switching, sub-area theme / night / vertical (48 wide, 28–168 tall in 7s),
    area sizes (24–240), goal follows the right edge
  - ✅ links: pipes (length / direction / colour, main↔sub warp ids, contents), door pairs (normal,
    P, key), warp box pairs, link ids shared with tracks — encoded like the sample courses
  - ✅ tracks (straight + 45°, corpus end codes, open / closed ends, parts on tracks: 0x400 + piece
    link id) · snake block / ! block / track block / Piranha Creeper paths · ⬜ laying bends and
    junctions, clear pipes, Lakitu-cloud / clown-car / claw riders (kept when editing a file)
  - ✅ selection: rectangle copy / cut / paste / move / delete (parts, ground, tracks; all or nothing)
  - ✅ settings: clear condition (category / CRC32 / magnitude), autoscroll incl. custom nodes and
    speed, liquid mode / speed / heights, timer 10–500
  - ✅ placement rules: the game's per-area part counters from v1.0.0 (100 enemies & gizmos, 100
    power-ups, 2000 plain blocks & coins, 200 terrain parts, 1 checkpoint, 5 pink / big coins, 4
    door and 4 warp-box pairs, 3 Bowsers, 10 path blocks, 5 snakes …; each reached exactly in the
    sample), the validators' size ranges, style-only parts, header ranges (timer, start / goal
    heights, area sizes, custom autoscroll main-area only); overlap layers, start / goal zones and
    containers from the sample · v3 parts' limits (120–132) and per-part flag masks ⬜
  - ✅ shell maker UI (`mmk-play --edit <file>`): engine-drawn area, ghost preview, 40-part palette,
    keys for everything above (README) · SMM2 maker layouts ⬜
- 🔄 Story Mode (Peach's castle): core ✅, screens ⬜
  - ✅ BFEVFL reader (`mmk_formats::bfevfl`, all 24 Story + 51 lesson flows parse) and a flow
    interpreter (`mmk_story::flow`: host-implemented actors, waits, sub-flow arguments, forks)
  - ✅ the game's own flows run the Story (`mmk_story::Story`): jobs activated by the Chief, the
    givers and the Taskmaster, builds enabled by the flows, NPC chains and moves (move flags
    settle when the hub is rebuilt), the three crises blocking the Chief, the kidnapping, the
    ending and staff roll, Builder Mario, post-game jobs 117–119 and the ten 1000-coin statue
    stages, the all-clear Coursebot gear; the rules behind the flows' queries / actions: payment
    on a job's first clear, collected coins on every clear, a failure pays nothing and passes no
    day, builds paid in full when ordered and counted down per cleared job in parallel, pop-up
    queue, coins / days 0–9999, hub gates, hub coins and daily flags
  - ✅ a job's course: 5 lives, TOO BAD → Try Again / Call Luigi / Give Up, Luigi's clear stamps
    the job until Mario clears it, "struggling" play type after two attempts (`JobPlay`)
  - ✅ `StoryState`: quest.dat's meaning as JSON (jobs, builds, days, coins, ordered job, flags,
    talk words, gear, Builder / Superball unlocks)
  - ✅ import stage `modes` (importer v6; also `mmk import-modes <romfs> <cache>`): flows, jobs /
    builds / flags with English fields, the hub map, the clear-condition copy
  - ✅ tests: synthetic flows for every rule; the whole Story from the opening to the post-game
    through the owner's flows (467 courses) when a cache is present
  - ⬜ screens: hub (2D list of `hub_things()` or the 3D hub), Taskmaster list / job window, Chief
    build menu, reward / progress pop-ups, TOO BAD window, ending, staff roll; castle progress %
    (map-part weights not found yet)
- 🔄 Coursebot: core ✅ (`mmk_modes::coursebot`: 120 + 60 slots behind the game's slot table, drag =
  swap of My Courses entries, files never renamed; metadata with clear check, upload code, Super
  World, Downloaded server / local stats; course-number space; scene reasons → list modes; upload
  gate; JSON; `CoursebotStore` over a pluggable blob store; starts empty) · screens ⬜
- 🔄 Lessons (Yamamura's Dojo): core ✅ (`mmk_modes::dojo`: catalogue from the imported
  `lessons/index.json`, 3 levels × 15 Maker Lessons, the 4 power-up lessons, Maker Basics, lesson
  starts with descriptors, completion bits, Maker Basics → 10 title courses, Nina's flag;
  `mmk_story::lesson` runs a lesson flow for the screen — all 50 run to `EndLesson` in the test)
  · Dojo menus, lesson screen (editor puppeteering), `SPRH` demo replays ⬜
- 🔄 Endless Challenge: its menus and map present the 100 Mario Challenge (above); SMM2's own
  rules ✅ (`mmk_modes::endless`: 5 lives [ext], pending 1-Ups max 3 per course paid at the goal,
  misses / Start Over cost a life, free unlimited skip rolled back to the course-start snapshot (a
  skip during a miss demo costs the life), 50 points per second left, quit restores the snapshot,
  skip hint on the 4th attempt, 48-entry history and map position, clear-rate pick rule, offline
  `Session`) · screens on the SMM2 rules ⬜
- ⬜ Ninji speedruns / event courses (offline with local data), records and medals
- ✅ Pass 3 (world.rs, from the binary): blast box, Clown Car exit, cape/hammer boxes, Burner
  collider, helmet hits, Bill Blaster generator, spawn profiles, escort Toad comeback, goal pole

### Phase 10 — Online with an open-source server ⬜ (owner request 2026-10-18)
Not Nintendo's network: the engine talks to an open-source server the owner can run or join.
- 🔄 Server: our own open-source course server `mmk-server` ✅ (directory store, REST API: upload
  with validation, search/sort, download, play/clear/like stats → difficulty, 100 Mario pools by
  fail-rate band; `mmk server`) and client crate `mmk-online` ✅ (`mmk upload/search/download`,
  `RemoteSource` for 100 Mario) · accounts ✅ (name + secret token, SHA-256 stored; maker IDs;
  like/boo, clears with world records and first clears, comments, maker profiles, delete own
  course; client + `mmk register/like/comment/maker`, `mmk upload --token`) · leaderboards ✅
  (maker points [inf weights], courses cleared, first clears, world records, Endless Challenge per
  difficulty; `mmk rankings`) · verified records ✅ (`mmk_modes::replay`: a clear sent as its
  per-frame inputs, re-simulated by the server, which takes the time from the replay; record runs
  served for ghosts; `--verified-records`; `mmk-play --code … --token` submits on clear) · ghosts ✅
  in the portable shell (`mmk-play --code … --ghost` races the record's run, `--ghost-file`,
  `--record-replay`; drawn half transparent, 2D styles; the 3D styles' player is a mesh ⬜) ·
  ghosts in the Swift app ⬜ · server hardening ✅ (request thread pool, CORS preflight for browsers,
  per-address limits on writes and new accounts → 429, `--trust-proxy`, `/api/health`) ·
  SMMDB import ⬜ · ⏳ owner: keep this or connect to another open-source server
- ✅ Endless runs held by the server: `/api/endless` status, `/<d>/start|next|result|suspend|
  finish|history`; batches of 10 by clear-rate band (≥ 20 attempts), no repeats in a run, never
  the player's own or banned courses, widening reported; best per difficulty feeds the
  `endless_<d>` rankings; Endless plays count in course statistics; `mmk-online` client calls
- 🔄 Course World client: browse (SMM2 course list over the server and the library) and play ✅,
  play / clear reports ✅; in the Rust front end (`mmk-front`, 2026-10-18) ✅: account (player
  card → `Nwk_PlayerInfo_00`: create with a typed name, sign in with a token, sign out; token in
  the host's store), like / boo on a server course's clear screen and pause menu (the game's
  judge buttons),
  comments (`Nwk_CommentViewer_00`) and posting (`Nwk_CommentPosting_00`, typed), course page
  (`Nwk_CourseListWindow_00` + `Nwk_CourseDetail_00`: likes, plays, first clear, world record),
  maker profiles (their uploaded courses), leaderboards (`Nwk_CourseListMaker_00`: maker points,
  clears, first clears, records, Endless per difficulty), clears sent as replays the server
  verifies (fallback: the time), the record's ghost raced from the course page, Endless runs
  reported; `mmk-play` host on threads (`--server` / `$MMK_SERVER`), tested against a local
  `mmk server` with captures `account`, `courseworld-leaderboards`, `courseworld-course-page`,
  `-comments`, `-post`, `-maker`, `-play`, `-pause`, `-clear` · comments from the pause / clear
  screens ⬜ ·
  upload from the editor ⬜ (no Course Maker in the front end yet) · the Swift app ✅ (the same
  screens through `mmk_front_*`; its host makes the calls on threads, records server-course runs
  and sends them as replays — tested against a live server in `mmk-ffi`: a verified record, a
  like, a comment; the account screen signed in from UserDefaults; macOS types from the keyboard,
  iOS through a text prompt) · ghosts in the Swift app ⬜ (the engine snapshot draws one world) ·
  the server times a replay to the end of the clear sequence, the clear screen to the goal touch
  (16.35 s vs 9.18 s in one run) ⬜
- ✅ Local multiplayer (co-op, up to 4: Mario, Luigi, Toad, Toadette as the models' _2/_3/_4
  variants): extra players run the same rules, rejoin beside player 1 [inf: bubble], any player
  clears; `mmk-play --players N` (one pad each), FFI `mmk_engine_set_players / step_players`;
  `World` is `Clone` and replays deterministically (rollback groundwork) · versus, bubbles ⬜
- 🔄 Real-time online play: rollback netcode crate `mmk-netplay` ✅ (predict, restore snapshot,
  re-simulate; redundant input packets over UDP; test: two peers with 4-frame delay and 1/3 loss end
  identical to a full replay) and `mmk-play --net-bind/--net-peer/--net-player` · up to 4 players
  ✅ · relay rooms on the course server (UDP, same port) with hole punching ✅ (direct once a peer
  answers a probe, relayed until then; `mmk-play --net-relay host:port --net-room NAME`; test: 4
  peers through a live relay end identical to a full replay) · browsers ✅ join the same rooms over a
  WebSocket (`mmk server` HTTP port + 1; page "Play online…" or `?relay=&room=&player=&players=`;
  checked with two headless browsers, one joining 3 s late: identical positions at frames 600 and
  1200) · start sync (`MAX_LEAD` wait) and acked resends ✅ · desync checks ✅ (state hashes at
  confirmed frames every second, compared across peers, logged; `mmk simulate --checksum` run in
  two processes over the 550 courses: identical) · WebRTC upgrade, versus ⬜
- ✅ Accounts without Nintendo services; moderation for self-hosters (`--admin-token`: ban/unban,
  delete any course)

### Phase 11 — SMM1 extras, each behind a setting 🔄 (owner request 2026-10-18)
From the owner's SMM1 (Wii U) dump, as engine options layered on SMM2 play (`mmk_sim::Options`,
all off = SMM2; FFI `mmk_engine_set_option`, `mmk-play` / `mmk simulate` flags). Rules from
Block.rpx v208 (private spec research/specs/smm1-extras.md).
- ✅ Mid-air shell kicks / shell jumps (`smm1_shell_kicks`): SMM1's flat 10 px falling stomp
  window, still shells kicked without a bounce, the kicker's 16 / 8 / 4 no-hit windows and the
  5-frame contact window, the 4.0 catch-up re-kick, and the mid-air wall assist (hop 3.51855, the
  shell parks a tile short of the wall) — the "shell jump"
- ✅ SMM1 draw order (`smm1_layering`): SMM1 draws the player in front of pipes in play (the
  "pipes over the player" memory is the editor, and pipe travel); the option applies SMM1's
  depth ladder: passing through a pipe the player is in front of blocks but behind the pipe,
  coins behind blocks, tracks over pipes, the vine / pole / death depths
- ✅ Lanky Mario (`smb1_mushroom=weird`, SMB1 courses' Super Mushrooms become Weird Mushrooms):
  Super's body, jump launch +0.5 (≈ 6.3-tile standing jump), float cue, SMM1 pickup/damage rules,
  the narrower item box; art imported (`M1_Player_SlimMarioMdl`, `M1_Item_kinokofunny`)
- ✅ Costumes (`smb1_mushroom=mystery`, `costume` = id or random): small Mario's body and physics,
  crouch, air turn, Up taunt, no helmets, Sonic's spin; costume ids as SMM1's; random pick from the
  costumes with art; art (115 costumes, 24 × 24 frames) and sounds (jump/jump2, power-up, taunt,
  landing, skid, death and clear jingles) imported by `mmk import-smm1` into `<cache>/sprites`
- ⬜ Swift Settings toggles and the costume picker (FFI ready; app being rewritten separately)
- ⬜ Options in replays / online play (replays and rollback sessions don't carry options yet: keep
  them off for records)
- ⬜ 100 Mario costume rewards (rules in smm1-100mario-code.md §7) and costume ownership
- ⬜ Lanky Mario's float-cue and jump sounds (`M1_HEN_JUMP*`: not imported); pickup freeze length
  for costumes [?]; Inkling squid frames while swimming; the random Weird Mushroom easter egg
  (1 in 20, pity at 100; its gates are not understood)
- ⬜ SMM2 mode fixes found on the way (fidelity log): still-shell stomps shouldn't bounce, SMM2's
  kicker windows and wall assist
- ⬜ Other SMM1-only behaviours (surveyed, research/specs/smm1-extras-misc.md §B): per-enemy pipe
  spawn intervals (Goomba 140, others 180, no pipe colours); SMM1 spawn caps; Clown Car weight
  classes; attack reactions (shells vs Lava Bubbles, Blooper stomps, Fish Bones in lava, star vs
  Munchers); helmet reactions; behaviour presets (Green Koopa, Spike Top, pipe Blooper, Dry
  Bones, Magikoopa); stacking heights; SMM1 sound effects (14 built-in + recordings); Big Mushroom
  in other styles [?]

### Recomp — native SMM2 v3.0.3 by static recompilation 🔄 (owner request 2026-10-18)
Separate from SMCraft, in `recomp/`; approach in DECISIONS.md (base: LAS Recomp, MIT; Wonder
framework ideas; Ryujinx as the reference). Nothing counts as done without the evidence below.
- ✅ Approach chosen and recorded; proprietary-content guard extended for recomp output
- ✅ Toolchain: LAS Recomp forked into `recomp/` (MIT notice, upstream commits in `recomp/UPSTREAM.md`), retargeted
  to SMM2 v3.0.3 (`main` + `sdk`), shared runtime + `games/smm2/`; one command from the player's dump:
  `make -C recomp smm2` (setup, recompile, build), `make -C recomp run-smm2`
- ✅ R1 (macOS): boots to the title screen and renders it (2026-10-19). What it took: `recomp/docs/smm2-bringup.md`
  (per-core scheduling, nifm/friend/mii/fonts/system data, NVN conditional rendering, 3D render targets, blocking
  first-use compiles, narrow vertex strides, in-order uniform updates, translator TLD4/TMML/LDC.64 fix)
- 🔄 Title screen frames vs Ryujinx, five courses in four styles (3D World, SMB1 x2, SMB3, NSMBU), same course
  and nearest frame, both windows grabbed through the window server: 2.50-4.80 % of pixels differ by more than 24,
  0.66-2.98 % by more than 48, 0.13-1.15 % by more than 96 (table in `recomp/docs/smm2-bringup.md`). Fixed in
  this round: skinned meshes (Mario, Koopa Car: signed LDC offsets), a culled stand-in mesh, geometry shaders,
  2D tile sheets (rendered textures keep their memory). Left: texel-edge and text differences, animation phase
  (not exactly alignable: Ryujinx grabs are wall-clock timed). Ryujinx against itself: 0.3-1.8 %.
  Evidence: `recomp/private/smm2/evidence/r1/final_*` (`_side_by_side.png`, `_diff.png`, `_recomp.png`; private,
  not in git)
- ⬜ R2: play a course by input injection; input-replay comparison against Ryujinx
- 🔄 Mods: 100 Mario Challenge on SMM2's Endless Challenge (100 lives, SMM1 run rules, courses from
  our server or the local library); Mystery Mushroom costumes (model, sounds, physics swaps)
  - ✅ Server side: Endless runs take a ruleset at `/start` (`smm2`, or `hundred`: 100 lives capped
    at 100, done after 8 / 16 / 16 / 6 clears with `run_complete`, SMM1's fail-rate bands, skips
    free after a death); completed runs on `hundred_<d>` leaderboards apart from SMM2's Endless
    boards; rankings carry the caller's own row (`me`). SMCraft's 100 Mario reports completions
    there too (tests: modes, server, front)
  - ⬜ Recomp hooks (the game's Endless NEX calls answered by our server, lives text and cap,
    skip rule); ⬜ costumes

| Platform | Builds | Boots to title | Plays courses | Frames match Ryujinx | Input replays in sync |
|---|---|---|---|---|---|
| macOS (Apple Silicon, Metal) | ✅ | ✅ title rendered, 60 fps | ⬜ | 🔄 title, 5 courses / 4 styles: 2.5-4.8 % px >24, 0.13-1.15 % >96 | ⬜ |
| Windows (x64, Vulkan) | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Linux (x64, Vulkan) | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| iOS / iPadOS (arm64, Metal, AOT) | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Web (wasm + WebGPU) | ⬜ feasibility decided after the native ports | | | | |

---

## Phase 1 detail

### Done
- Repo initialised. Public home: https://github.com/smcraft-git/smcraft (published as snapshots;
  2026-10-18). `.gitignore` excludes dumps, keys, extracted data and `research/`.
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
