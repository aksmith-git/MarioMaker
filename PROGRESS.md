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
- 🔄 3. Rendering: tiles, sprites, letterboxed integer scaling ✅ · exact BFRES quad placement and
  animation timing ⬜ (sprite offsets currently measured from the textures)
- ✅ 4. Audio: BWAV/BARS parsers + DSP-ADPCM/PCM16 decode; importer `audio` stage (22 SMB1 sounds:
  jump, coin, stomp, kick, bump, break, item, power-up/down, 1-UP, fireball, pole, time count,
  death / clear / hurry fanfares, ground + hurry + star music) into `<cache>/audio/`; `mmk-audio`
  mixer (32 voices, loops, music chaining, fades, exclusive groups, linear resampling to 48 kHz,
  master/music/SFX volumes); engine maps sim events to sounds (hurry jingle → fast music, star
  music, death and clear fanfares, count-down loop); AVAudioEngine output (`MMK_MUTE=1` to
  silence); `mmk audio` CLI lists/decodes sounds to WAV · other themes' music, night music and
  object sounds beyond the slice ⬜
- 🔄 5. Terrain: ground, blocks, pipes, semisolids, slopes ✅ · ice ✅ · exact camera rules ⬜
- ✅ 6. Core objects: coins; ? / brick / hidden / hard / note blocks (bump curve, contents, multi-coin,
  breaking + debris); Super Mushroom, Fire Flower + fireballs, Star, 1-Up; Goomba (+ Paragoomba),
  Koopa green/red (+ Paratroopas), shells (kick, slide, chain kills, wake-up); stomp chains, damage
  (Fire → Super → Small), invincibility, death sequence, restart, lives; course timer + hurry;
  M1 goal pole (height score, slide, walk to castle, time → score countdown, castle flag)
- ⬜ 7. Play real courses start to goal; tune against Ryujinx (needs a Ryujinx build with a GDB stub)

### Phase 5b — Native app (must be done before Phase 6) ⬜
- ⬜ Signed `.app` bundle with icon
- ⬜ First-launch import screen (point at dump → extract; placeholder fallback)
- ⬜ Course library screen: browse and play local courses
- ⬜ Pause / restart / exit
- ⬜ Settings: keyboard + controller mapping, window/fullscreen, volume
- ⬜ Persistent save data (clears, settings)
- ⬜ Per-course "playable / not yet supported" marking

### Phase 6 — 100 Mario Challenge ⬜ (owner: SMM1 presentation like-for-like)
- ⬜ Research: SMM1 100 Mario screen sequence from Block.rpx v208 + layout data — title/mode select,
  difficulty select, world map + Mario walking between courses, course intro card, lives / extra-life
  counting, course clear, skip, game over, final rescue scene, costume reward, Save & Quit
- ⬜ SMM1 asset import (owner's disc dump): BFLYT/BFLAN layouts, GX2 textures, BFFNT fonts,
  BFSTM/BARS audio, MSBT text (English first)
- ⬜ Layout player (panes, text, keyframed animations) — shared with the app menus
- ⬜ Mode logic per research/specs (lives cap 100, ≤3 banked lives/course, free skip, unlocks, rewards)
- ⬜ Course pool: local library + pluggable remote source (SmmServer/SMMDB-style), difficulty tiers
- ⬜ Pool limited to courses marked playable; grows with Phase 7
- ⬜ SMM1 → SMM2 course import design + documented losses

### Phase 7 — Expansion ⬜
- ⬜ Measure object/style/theme frequency in real courses
- ⬜ Add styles, themes, objects in that order (2.0/3.0 items need the update dump)

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
