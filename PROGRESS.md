# Progress

## Roadmap
✅ done · 🔄 in progress · ⬜ not started · ⏳ waiting on owner

### Phase 1 — Environment and extraction ✅
- ✅ Dumps gitignored; provenance checked; toolchain pinned; CI + pre-commit guard
- ✅ SMM1 decrypted (base v16 + update v208); SMM2 v1.0.0 extracted; inventories; 550-course sample
- ⏳ SMM2 update v3.0.3 as `.nsp` (current file is an incomplete 7z) — needed for Phase 7

### Phase 2 — Course formats 🔄
- 🔄 Gather community format docs (SMM1, SMM2)
- ✅ Confirmed: SMM2 `.bcd` = 0x10 header + AES-CBC body + 0x30 trailer (CMAC); SMM1 `.cdt` = 0x15000 BE, CRC32 @0x08 over [0x10:]
- ⬜ Format docs in `docs/formats/`
- ⬜ SMM2 decrypted-course parser/writer
- ⬜ SMM2 `.bcd` decrypt/encrypt (key table read at runtime from the owner's executable)
- ⬜ SMM1 `.cdt` parser/writer + CRC
- ⬜ Course ID ↔ code conversion (both games)
- ⬜ Byte-exact round-trip tests: 550 dataset courses + all built-in courses
- ⏳ Owner's own saved courses (optional)

### Phase 3 — Binary analysis ⬜
- ⬜ Course-loading pipeline · player physics per style · game-parameter file mapping
- ⬜ Object behaviours (most common first) · style and theme differences
- ⬜ SMM1 100 Mario Challenge logic (from `Block.rpx` v208)
- ⬜ Behavioural specs in `research/` (private)

### Phase 4 — Engine architecture ⬜
- ⬜ 2–3 native options (all native Apple Silicon `.app`, Metal-backed) with trade-offs
- ⬜ First-launch asset-import design + placeholder-art fallback
- ⬜ Owner chooses → DECISIONS.md

### Phase 5 — Vertical slice (SMB1, ground) ⬜
- ⬜ Engine skeleton, rendering, input, placeholder art
- ⬜ SMB1 ground asset extraction
- ⬜ Load and render a real course
- ⬜ Player physics (tested) · core objects
- ⬜ Frame-by-frame tuning against Ryujinx

### Phase 5b — Native app (must be done before Phase 6) ⬜
- ⬜ Signed `.app` bundle with icon
- ⬜ First-launch import screen (point at dump → extract; placeholder fallback)
- ⬜ Course library screen: browse and play local courses
- ⬜ Pause / restart / exit
- ⬜ Settings: keyboard + controller mapping, window/fullscreen, volume
- ⬜ Persistent save data (clears, settings)
- ⬜ Per-course "playable / not yet supported" marking

### Phase 6 — 100 Mario Challenge ⬜
- ⬜ Local library + pluggable remote source (SmmServer/SMMDB-style API)
- ⬜ Difficulty tiers (Nintendo's rating + clear rates)
- ⬜ Mode: 100 lives, fixed course count, skip, completion/unlock tracking
- ⬜ Pool limited to courses marked playable; grows with Phase 7
- ⬜ SMM1 → SMM2 import design + documented losses

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

### Open
- SMM2 update (v3.0.x) not dumped. Courses made after Dec 2019 can use 2.0/3.0 items that v1.0.0 has
  no assets or code for. Needed before Phase 7 and for a realistic 100 Mario Challenge pool.
- Owner's own saved courses: not yet exported from the Switch; the dataset sample covers Phase 2 for now.
