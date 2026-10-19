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

### SMCraft (the clean-room engine, Phases 4–11) — archived 2026-10-19
At the owner's call the recomp is now the only version. SMCraft (Rust engine, Swift app, web
build, SMM2 front end, Course Maker, its own 100 Mario) is kept whole on the `smcraft-archive`
branch (tag `smcraft-final`), where it builds and its tests pass as they did; its roadmap and
evidence are in that branch's PROGRESS.md. Main keeps the parts the recomp uses: the course
server and the crates it needs (`core/`: formats, sim, editor, modes, netplay, server, online,
and `mmk`, now just the server tool).

### Course server (core/) ✅ in use by the recomp's mods
- ✅ `mmk server`: accounts (name + secret token), course upload / search / download, likes,
  comments, leaderboards, world records (replays verified by SMCraft's engine), Endless runs held
  on the server, online-play relay (UDP + WebSocket). Self-hosting: docs/self-hosting.md
- ✅ 100 Mario ruleset for Endless runs (`{"ruleset":"hundred"}` on `/start`: 100 lives capped at
  100, done after 8 / 16 / 16 / 6 clears with `run_complete`, SMM1 fail-rate bands, free skips),
  `hundred_<d>` leaderboards, the caller's own rank row (`me`)
- ⬜ Replay verification for recomp runs: the server re-simulates replays with SMCraft's engine,
  which isn't the game; recomp records need their own proof [?]

### Recomp — native SMM2 v3.0.3 by static recompilation 🔄 (owner request 2026-10-18; the only version from 2026-10-19)
In `recomp/`; approach in DECISIONS.md (base: LAS Recomp, MIT; Wonder
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
- ✅ Windows / Linux ports (2026-10-19): `make -C recomp TARGET=windows-x86_64 | linux-x86_64 | linux-aarch64 GAME=smm2
  DEPS=<deps> all package`, cross-built from macOS with zig; the SMM2 fixes ported to the Vulkan backend and the GLSL
  translator (conditional rendering, report values, buffer clears, cull-both skip, 3D target slices, first-render
  uploads, storage aliases, cube-array views, in-order uniform updates, compute result flush, blocking compiles, real
  geometry shaders, TLD4 / TMML); player packages recompile the dump on first launch. Evidence in the table;
  `recomp/private/smm2/evidence/linux/` (private). Left: Mario's model on Vulkan; Windows and x64 untested
- 🔄 iOS / iPadOS (2026-10-19): `make -C recomp TARGET=ios-arm64 | iossim-arm64 GAME=smm2 app` (AOT: the game code is compiled on the Mac and linked into the app; no JIT, no game code library); Documents holds the game files and the log; touches are the console touch screen (macOS: the left mouse button), controllers through SDL; BC textures decoded on the CPU where the GPU lacks them. Evidence: the Simulator boots to the title; an iPad Pro 13 M5 (iOS 27.0, installed over Tailscale, App ID with extended virtual addressing + increased memory limit: `recomp/tools/ios_profile.sh`) boots through the intro to the title screen and its menu at a steady 60 fps (MetalFX to 2752x2064, the 3.25 GB heap buffer taken whole), no faults (`recomp/private/smm2/evidence/ios/`, private: screenshots, boot log, the address-space log below). The guest window stays at 0x7000000000 on iOS: with the entitlement iOS 27 gives an app up to 0x8000000000 and keeps 0x2ee000000-0x7000000000 itself (measured; `GUEST_ORIGIN` moves it where that differs). On-screen controller for touch-only devices (stick, ABXY, L/R/ZL/ZR, +/-, menu, hide), other touches stay the touch screen. Left: play a course on the device; the on-screen controller is drawn but not yet tried by hand
- ✅ R2 (macOS): plays courses by input injection, and input replays stay in sync with Ryujinx frame for frame
  (2026-10-19). The game now gets input (`nn::hid::GetNpadStates` answered by the runtime; before, it saw none,
  keyboard included); `RECOMP_PAD_SCRIPT` plays the Ryujinx trace tooling's scripts on the game's own clock (one frame
  per player update); `RECOMP_TRACE_OUT` records the player at the same instruction as the Ryujinx trace build;
  `tools/replay_sync.py` runs both and compares. What it took and how: `recomp/docs/smm2-bringup.md` ("Input replays")
  - Replays: 31 sessions, 456 tests (flat, wall, slope, water, items and power-up forms in all five styles, plus
    two runs on the game's own title course with enemies): 221,189 of 221,189 player updates identical in position,
    speed, facing and state, bit for bit; the whole 12 KB player object identical apart from one word holding a heap
    address. No divergence, so no recomp bug to fix
  - Plays: the game's own title course from the start to the goal (Koopa Car, enemies, coins; 1,911 updates, identical
    in both runtimes); Course Maker test play reached through the menus (L + R, Make, −) on the presented-frame clock
  Evidence: `recomp/private/smm2/evidence/r2/` (per-course tables `results/*.md`, traces of both runtimes, sessions,
  screenshots; private, not in git)
- 🔄 Mods (owner's plan, 2026-10-19: a perfect SMM2 v3.0.3 recomp; SMM1's 100 Mario exactly as SMM1;
  each SMM1 feature behind its own setting; never Nintendo's servers)
  - 🔄 **100 Mario Challenge exactly as in SMM1** (not SMM2's Endless screens reskinned): SMM1's own
    screens from the owner's SMM1 dump, drawn by the recomp host: difficulty tabs, the world map with
    the Goombas carrying Peach and the castle at the end, the dot walk, the start card with 100 little
    Marios, pause, clear, game over, credits (`Nwk_StaffCredit_00/01/03`) and `T_ThankYou_10/20/30`;
    the rescue / ending scene recovered from the Wii U binary (code, not a layout), emulator footage
    of the owner's SMM1 dump as the reference. Courses play in SMM2's own course play.
    - ✅ Server: `hundred` ruleset (100 lives capped at 100, runs of Easy 8 / Normal 16 / Expert 16 /
      Super Expert 6 then `run_complete`, SMM1 fail-rate bands, free skips), `hundred_<d>` boards
    - ✅ Game side over our server (`recomp/docs/mods.md`): pretend-online (nifm, acc NSA, mii, enl
      session, Course World's login answered locally), Course World opens against our server; the
      Endless requests answered from `/api/endless`; courses download (the game's own encryption, its
      CRC / validity checks pass) and play in SMM2's own play, ×100 on the course card and HUD; clear /
      miss / skip / suspend / finish reported. Evidence [obs]: a full **Super Expert run won against a
      local server** - 6 clears, 3 misses (100 -> 97), skips, the win -> ending -> run finished; the
      server's `hundred_super_expert` board shows the completion (`recomp/private/smm2/evidence/mods/
      se_run/`: run log, server JSON, screenshots; `course_in_card_100.png`, `course_hud_100.png`)
    - ✅ The bridge to the SMM1 screens (`games/smm2/mods/hundred.h`, `bridge.c`): a host call starts a
      server course in SMM2's play (the Endless menu stays hidden and is driven), the outcome comes back
      as `hundred_course_result`; the win plays the ending and finishes the run (not a game over).
      ✅ Continue (the menu offers it; a saved Easy run continued at 100 lives / 1 clear and cleared on),
      ✅ SMM2's pause held closed and routed to the flow (resume, Start Over = a life, Save & Quit), ✅ the
      next course named before the card, `form` at the goal, the difficulty picked with L / R (New Game
      on Super Expert with a saved Easy run started)
    - ✅ SMM1 client rules (`games/smm2/mods/hooks.c`, 22 hook points, only in a 100 Mario run): lives
      cap 100 (HUD and in-course clamps), free skip during the miss demo, a miss rolls back the
      attempt's coins and score, a multi-life award banks one 1-Up; unlocks Expert after a won Normal /
      Super Expert after a won Expert (server boards). Seen in runs: free skips, 100 -> 99 per miss,
      1-Ups banked 0 -> 1 -> 2 -> 3 on the HUD (`evidence/mods/pending_1ups_*`); the coin / score rollback
      runs on every retry (logged) but no recorded attempt collected coins before dying yet; no SMM2
      award worth several lives found in a test course (the cap is untested in play)
    - 🔄 SMM1 screens and the ending, host-drawn (`games/smm2/mods/smm1/`: SMCraft's SMM1 layout player
      ported to C, a CPU rasterizer drawn through the overlay on Metal and Vulkan; everything from the
      player's SMM1 dump, generated into `mods-generated/smm1-ui-v4/`):
      - ✅ difficulty select (tabs by unlock, Continue only with a record, OutR/OutL slides), world map
        demos as Block.rpx plays them (MapIn, Telop, the Goombas carrying Peach, AnimB, Mario's jump /
        walk / Go, the castle at the end, lives and n/total counters, A skips to Go), the start card (one
        little Mario per life, AnimA/AnimB by case, held until the course starts, Out over it), pause,
        clear, hold-to-skip countdown, the card after a miss, game over
      - ✅ the ending from the binary: the rescue scene in the cleared course's style (SMB1, SMB3, SMW
        sprites; NSMBU's 3-D Mario posed with its walk), Peach (Toad on Easy), the Thank-you line by
        difficulty, credits `Nwk_StaffCredit_01/00/03` with each course and maker, TO BE CONTINUED... /
        END / THE END; nothing skips (as SMM1); a hook for the Mystery Mushroom reward
      - ✅ SMM1's music and sounds (Course World stream per difficulty with the beat bounce, the map
        streams, the ending and staff-roll music, the screens' sound banks), the game's sound faded under
      - ✅ a whole Super Expert run against the local server, unattended: select → map → card → course →
        clear … (skips, misses) → the ending with the run's courses and makers in the credits → back on
        the select (`RECOMP_HUNDRED_AUTO`, `RECOMP_HUNDRED_SHOTS`; evidence in
        `recomp/private/smm2/evidence/e2e5_sx/`, private); pause, Save & Quit and Continue through the run side
      - ⬜ a New Game straight after an ending (SMM2 isn't back on its Endless menu after a win: run side),
        per-frame sound triggers (SLink database not decoded: sounds are placed by name [inf]), the
        NSMBU ending's perspective camera, Cemu side-by-side (no Cemu installed)
  - 🔄 **Settings, one toggle per SMM1 feature** (overlay menu's Mods tab, kept in `smm2mods.ini`):
    ✅ "100 Mario Challenge (SMM1)" (on screen in the Mods tab: `evidence/mods/mods_tab.png`); ⬜ Mystery Mushroom costumes (model, sounds, physics
    swaps), mid-air shell jumps, overlapping (overlay) pipes, and more from research/specs/smm1-extras*
  - 🔄 **Our open-source server everywhere the game would reach Nintendo** (`recomp/docs/mods.md`,
    `games/smm2/mods/world.c`): ✅ Course World's course lists (Hot, Popular, New, detailed search, a
    maker's courses) from `/api/courses`, the course page (likes, plays, clear rate, world record, maker
    name and Mii), download and play in SMM2's own play, the play / clear posted with its time
    (`evidence/mods/world/`); ✅ leaderboards list the server's makers with their maker points
    (`leaderboard_makers.png`); ✅ SMM2's own Endless under the `smm2` ruleset (5 lives, endless, its
    own screens and HUD; an Easy run against the server: `evidence/mods/smm2_endless/`); 🔄 built, not
    yet seen working: likes / boos (the clear screen's hearts took no test input), comments (text),
    uploads from the Course Maker, the ID search, maker pages; ⬜ thumbnails, Super Worlds, Ninji;
    ✅ courses=tgrcode setting (built-in server, awaiting core/crates/embed);
    ⬜ online multiplayer: plan in `DECISIONS.md` (local play over our relay first, Network Play at
    the enl layer second)

| Platform | Builds | Boots to title | Plays courses | Frames match Ryujinx | Input replays in sync |
|---|---|---|---|---|---|
| macOS (Apple Silicon, Metal) | ✅ | ✅ title rendered, 60 fps | ✅ title course to the goal, Course Maker test play | 🔄 title, 5 courses / 4 styles: 2.5-4.8 % px >24, 0.13-1.15 % >96 | ✅ 221,189 / 221,189 updates identical (31 sessions) |
| Windows (x64, Vulkan) | ✅ cross-built with zig: exe 2.4 MB + game DLL 186 MB, needs SDL3.dll, libiconv-2.dll, libshaderc_shared.dll, vulkan-1.dll (driver); player zip 85 MB | ⬜ not run (no Windows machine) | ⬜ | ⬜ | ⬜ |
| Linux arm64 (Vulkan) | ✅ cross-built with zig: exe 11.9 MB + game .so 688 MB, needs libSDL3.so.0, libvulkan.so.1, libshaderc.so.1, glibc 2.36; player zip 71 MB | ✅ title rendered (Debian 13 VM, Mesa lavapipe, ~7 fps); package first launch recompiled the game code from the dump (29 min) and booted | ⬜ | 🔄 seed 4 frame 2386 vs macOS same frame: 0.6 % px >24, 0.17 % >96 (Mario model missing on Vulkan); colour-managed vs Ryujinx 2.99 % >24, 0.24 % >96 (macOS 2.35 / 0.11) | ⬜ |
| Linux x64 (Vulkan) | ✅ cross-built with zig: exe 11.3 MB + game .so 856 MB (generated/smm2-c, x86-64-v3), same libraries; player zip 74 MB | ⬜ not run | ⬜ | ⬜ | ⬜ |
| iOS / iPadOS (arm64, Metal, AOT) | ✅ game code built on the Mac from the dump and linked into the app (exe 141 MB; SDL3 static); device app signed with an App ID carrying extended virtual addressing + increased memory limit (`tools/ios_profile.sh`) | ✅ device: iPad Pro 13 M5 (iOS 27.0) through the intro to the title screen and menu, steady 60 fps, no faults; Simulator (iPad Pro 13 M4): title at 60 fps, lighting wrong there (the Simulator's Metal limits: 32-byte colour targets, no cube arrays) | ⬜ on-screen controller drawn, not yet tried by hand | ⬜ | ⬜ |
| Web (wasm + WebGPU) | ⬜ feasibility decided after the native ports | | | | |

---

## Phase 1 detail

### Done
- Repo initialised. Public home since 2026-10-19: https://github.com/smm2recomp/smm2recomp (published as
  snapshots; the full history, SMCraft included, is at https://github.com/aksmith-git/MarioMaker). Earlier:
  https://github.com/smcraft-git/smcraft (snapshots;
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
