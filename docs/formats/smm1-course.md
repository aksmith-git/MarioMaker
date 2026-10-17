# SMM1 course format

Super Mario Maker (Wii U) stores a course as a folder of files. The two that matter are
`course_data.cdt` (main area) and `course_data_sub.cdt` (sub area): same format, 0x15000 bytes
each, **big-endian**, not encrypted. Built-in courses ship each `.cdt` inside a Yaz0-compressed
SARC. Reference implementation: [`smmfmt/smm1/`](../../smmfmt/smm1/).

**Verified** below means: checked by `tests/test_corpus.py` against all 152 built-in `.cdt` files
of game v16 and the 4 replaced in update v208, with byte-exact round trips.

## 1. Header

| Off | Type | Field | Notes |
|---|---|---|---|
| 0x00 | u64 | version | always 0xB — **verified** |
| 0x08 | u32 | CRC32 | zlib CRC32 of bytes 0x10–0x14FFF — **verified** |
| 0x0C | 4 | padding | |
| 0x10 | u16 | year | creation or last-modified (sources differ) |
| 0x12–0x15 | u8 × 4 | month, day, hour, minute | |
| 0x16 | u8 | unknown | |
| 0x17 | u8 | official maker | 0 none, 1–8 named Nintendo makers, 0x1D event |
| 0x18 | 8 | course id area | last 12 hex digits of the course id at 0x1A (per one source) |
| 0x20 | u8 | is downloaded | |
| 0x21 | u8 | previously removed from server | |
| 0x27 | u8 | physics version | 0 = 1.00 … 7 = 1.47 |
| 0x28 | UTF-16BE × 0x21 | name | |
| 0x6A | char[2] | game style | `M1`, `M3`, `MW`, `WU` — **verified** |
| 0x6D | u8 | theme | 0 ground, 1 underground, 2 castle, 3 airship, 4 water, 5 ghost house |
| 0x6E | u8 | uploaded | |
| 0x6F | u8 | clear-check status | |
| 0x70 | u16 | timer | |
| 0x72 | u8 | autoscroll | 0 none, 1 slow, 2 medium, 3 fast |
| 0x74 | u32 | width | 1/16-tile units |
| 0x78 | 0x60 | Mii data | preserved raw |
| 0xD8 | s32 | Mii country | |
| 0xEC | u32 | object count | |
| 0xF0 | 2600 × 0x20 | objects | one source says 0x108; 0xF0 is **verified** |
| 0x145F0 | 300 × 8 | sound effects | empty slot = `FF FF 00 FF FF 00 00 00` |
| 0x14F50 | 0xB0 | padding | |

## 2. Object (0x20 bytes)

| Off | Type | Field | Notes |
|---|---|---|---|
| 0x00 | u32 | x | tile × 160 + 80 |
| 0x04 | u32 | z | draw layer |
| 0x08 | s16 | y | tile × 160 + 80 |
| 0x0A | s8 | width | tiles |
| 0x0B | s8 | height | tiles |
| 0x0C | u32 | flags | default 0x06000840 |
| 0x10 | u32 | child flags | |
| 0x14 | u32 | extended data | fire bar rotation, etc. |
| 0x18 | s8 | type | object id |
| 0x19 | s8 | child type | −1 none |
| 0x1A | s16 | link id | |
| 0x1C | s16 | effect index | −1 none |
| 0x1E | s8 | transform id | |
| 0x1F | s8 | child transform id | Mystery Mushroom costume |

Flags: 0x2 wings, 0x4 alternate form, 0x100 Boo → Stretch; pipe direction `flags & 0x60` as in SMM2.

## 3. SMM1 → SMM2 differences (input to the Phase 6 import design)

- Object ids 0–31, 33–43, 45–49, 52–64, 66–69 mean the same thing in both games.
- Different: 32 (SMM1 unused vine block / SMM2 Banzai Bill), 44 (weird mushroom / style power-up),
  50 (Mystery Mushroom / Hop-Chops), 51 (Big Mushroom / Skipsqueak), 65 (costume Mario / Stingby).
- No sub-area theme list beyond 6 themes; no slopes, clear pipes, tracks-as-arrays, snake paths, etc.
- Default object flags differ (0x06000840 vs 0x06000040).
- Mystery Mushroom costumes, the weird mushroom and Big Mushroom have no SMM2 equivalent: lossy.
- Positions use the same 1/160-tile units, but SMM1 y is s16 and SMM1 has an explicit z layer.

## 4. Course codes

`CCCC-0000-HHHH-HHHH`: the last eight hex digits are the data id; `CCCC` is a checksum keyed
with the old game server's access key (not implemented here). Parsing in
[`smmfmt/course_id.py`](../../smmfmt/course_id.py).

## Sources

| Source | Licence | Used for |
|---|---|---|
| Treeki/MarioUnmaker `FormatNotes.md` (2015) | none — facts only | header, object layout, flags |
| kinnay/Nintendo-File-Formats `smm/level.md` (2026-10) | none — facts only | header fields, enums |
| aboood40091/PointlessMaker, RoadrunnerWMC/Metamaker, leomaurodesenv/smm-course-viewer | GPL-3.0 | cross-checks (no code used) |
| Tarnadas/smmdb-lib | MIT | cross-check |
| kinnay/NintendoClients wiki | MIT | course code format |
