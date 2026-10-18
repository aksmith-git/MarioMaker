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
- 🔄 Mods: 100 Mario Challenge on SMM2's Endless Challenge (100 lives, SMM1 run rules, courses from
  our server or the local library); Mystery Mushroom costumes (model, sounds, physics swaps)
  - ✅ Server side: Endless runs take a ruleset at `/start` (`smm2`, or `hundred`: 100 lives capped
    at 100, done after 8 / 16 / 16 / 6 clears with `run_complete`, SMM1's fail-rate bands, skips
    free after a death); completed runs on `hundred_<d>` leaderboards apart from SMM2's Endless
    boards; rankings carry the caller's own row (`me`) (tests: modes, server)
  - ⬜ Recomp hooks (the game's Endless NEX calls answered by our server, lives text and cap,
    skip rule); ⬜ costumes

| Platform | Builds | Boots to title | Plays courses | Frames match Ryujinx | Input replays in sync |
|---|---|---|---|---|---|
| macOS (Apple Silicon, Metal) | ✅ | ✅ title rendered, 60 fps | ✅ title course to the goal, Course Maker test play | 🔄 title, 5 courses / 4 styles: 2.5-4.8 % px >24, 0.13-1.15 % >96 | ✅ 221,189 / 221,189 updates identical (31 sessions) |
| Windows (x64, Vulkan) | ✅ cross-built with zig: exe 2.4 MB + game DLL 186 MB, needs SDL3.dll, libiconv-2.dll, libshaderc_shared.dll, vulkan-1.dll (driver); player zip 85 MB | ⬜ not run (no Windows machine) | ⬜ | ⬜ | ⬜ |
| Linux arm64 (Vulkan) | ✅ cross-built with zig: exe 11.9 MB + game .so 688 MB, needs libSDL3.so.0, libvulkan.so.1, libshaderc.so.1, glibc 2.36; player zip 71 MB | ✅ title rendered (Debian 13 VM, Mesa lavapipe, ~7 fps); package first launch recompiled the game code from the dump (29 min) and booted | ⬜ | 🔄 seed 4 frame 2386 vs macOS same frame: 0.6 % px >24, 0.17 % >96 (Mario model missing on Vulkan); colour-managed vs Ryujinx 2.99 % >24, 0.24 % >96 (macOS 2.35 / 0.11) | ⬜ |
| Linux x64 (Vulkan) | ✅ cross-built with zig: exe 11.3 MB + game .so 856 MB (generated/smm2-c, x86-64-v3), same libraries; player zip 74 MB | ⬜ not run | ⬜ | ⬜ | ⬜ |
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
