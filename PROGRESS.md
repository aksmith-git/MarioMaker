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
  MW 77) · Yoshi, cape dive flight, NSMBU/3D World (3D mesh renderer) ⬜
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

### Phase 8 — Every platform: macOS, iOS, Windows, Linux, web ⬜ (owner request 2026-10-18)
Approach (DECISIONS.md 2026-10-18, "Cross-platform shell"): the Rust core stays platform-neutral;
a portable Rust shell runs the game everywhere, the Swift app stays as the Apple-native shell.
- ⬜ `shell` crate: winit window + wgpu renderer (sprites, layouts) + gilrs gamepads + cpal audio,
  feature parity with the Swift renderer (same FFI-free core calls)
- ⬜ Windows and Linux builds of the shell; CI matrix (macOS, Windows, Ubuntu)
- ⬜ Web: wasm32 build of core + shell (WebGPU, WebGL2 fallback), WebAudio, Gamepad API; asset import
  from the owner's files through the browser's file picker into OPFS/IndexedDB (nothing uploaded)
- ⬜ iOS/iPadOS: the Swift app on UIKit/SwiftUI + Metal (shared renderer code), touch controls,
  Files-app import of the owner's dump; installs over Tailscale with the existing tool
- ⬜ Engine-drawn menus everywhere (layout player), so every platform shows the same screens

### Phase 9 — SMM2 single-player like-for-like ⬜ (owner request 2026-10-18)
- ⬜ SMM2 layouts: Switch BFLYT/BFLAN + BNTX textures through the same layout player; title
  screen, main menu, Course World, Story Mode map, pause, clear/death screens, HUD
- ⬜ All five styles (SMB1, SMB3, SMW, NSMBU, 3D World) and all themes (day/night) — physics
  feature switches per style already specced
- ⬜ Every part (0–132, incl. 2.0/3.0 items from the update dump), Yoshi, shoes, clown cars,
  power-ups per style, Koopalings, Master Sword, etc.
- ⬜ Course Maker (editor): placement, erase, copy, sub-areas, pipes/doors links, paths, tracks,
  settings, test play, save/load to the library
- ⬜ Story Mode (Peach's castle jobs, built-in courses from the owner's dump), Coursebot, Lessons
- ⬜ Endless Challenge (SMM2's successor to 100 Mario) alongside the SMM1-style 100 Mario Challenge
- ⬜ Ninji speedruns / event courses (offline with local data), records and medals

### Phase 10 — Online with an open-source server ⬜ (owner request 2026-10-18)
Not Nintendo's network: the engine talks to an open-source server the owner can run or join.
- ⬜ Pick / write the server (proposal: our own MIT/Apache Rust server — course upload/download,
  search, likes, makers, Endless pools, rankings; optional SMMDB import) ⏳ owner choice
- ⬜ Course World client: browse, play, like/boo, comment, upload from the editor
- ⬜ Real-time multiplayer (co-op / versus, up to 4): rollback netcode on the deterministic sim
  over WebRTC (open-source `matchbox` signalling) so native and web players can meet
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
