# SMM2 course format

Super Mario Maker 2 (Switch) stores each course as one encrypted file (`.bcd`, 0x5C000 bytes) that
wraps the plain course data (0x5BFC0 bytes, little-endian). Reference implementation:
[`smmfmt/smm2/`](../../smmfmt/smm2/).

This page builds on community documentation (see [Sources](#sources)) and records what this
project verified against real data. **Verified** below means: checked by `tests/test_corpus.py`
against 550 player courses (public dataset, June–July 2019 uploads) and all 180 built-in courses
of both game v1.0.0 and v3.0.3, with byte-exact round trips.

## 1. Encrypted wrapper (`.bcd`)

| Offset | Size | Contents |
|---|---|---|
| 0x00000 | 4 | u32 format version, `1` in every built-in course |
| 0x00004 | 2 | u16, `0x10` in built-ins |
| 0x00006 | 2 | u16, `0` in built-ins |
| 0x00008 | 4 | u32 CRC32 (zlib) of the **decrypted** body — **verified** |
| 0x0000C | 4 | `SCDL` |
| 0x00010 | 0x5BFC0 | body, AES-128-CBC, no padding |
| 0x5BFD0 | 16 | AES IV |
| 0x5BFE0 | 16 | random state, 4 × u32 LE |
| 0x5BFF0 | 16 | AES-CMAC of the decrypted body — **verified** |

### Key derivation (verified)

1. Seed a xorshift128 generator with the four random-state words. Each step:
   `t = s0 ^ (s0 << 11); t ^= t >> 8; t ^= s3 ^ (s3 >> 19)`, then shift the state left and
   append `t`; the output is `t`.
2. Each 16-byte key is four little-endian u32 words. Each word is built from four bytes, most
   significant first; each byte takes two generator outputs: `index = out1 >> 26` (0–63) and
   `shift = (out2 >> 27) & 24`, giving `(table[index] >> shift) & 0xFF`.
3. The first key is the AES key, the next is the CMAC key (same generator, continued).
4. Decrypt with AES-128-CBC and the stored IV; check the CRC32 and CMAC against the plaintext.

`table` is 64 u32 words embedded in the game's executable. **This project does not contain it.**
[`smmfmt/smm2/keyfind.py`](../../smmfmt/smm2/keyfind.py) finds it in the user's own executable
by testing every 256-byte window against one of the user's own `.bcd` files: decrypt only the AES
block holding the game-style code at body offset 0xF1, then confirm with the full CRC32 and CMAC.
It takes about two seconds. The same table works for v1.0.0 and v3.0.3.

Thumbnails (`.btl`), replays and `save.dat` use the same scheme with their own tables (not yet needed).

Off-console copies (the public dataset, community servers) are the plain body, zlib/gzip-compressed.

## 2. Course body layout

| Range | Contents |
|---|---|
| 0x00000–0x001FF | course header |
| 0x00200–0x2E0DF | main area (0x2DEE0 bytes) |
| 0x2E0E0–0x5BFBF | sub area (0x2DEE0 bytes) |

### 2.1 Course header

| Off | Type | Field | Notes |
|---|---|---|---|
| 0x00 | u8 | start_y | tiles |
| 0x01 | u8 | goal_y | tiles |
| 0x02 | s16 | goal_x | tenths of a tile |
| 0x04 | s16 | timer | seconds — **verified** against dataset metadata |
| 0x06 | s16 | clear_condition_magnitude | **verified** |
| 0x08 | s16 | year | creation or last-save time (sources differ) |
| 0x0A–0x0D | s8 × 4 | month, day, hour, minute | |
| 0x0E | u8 | custom autoscroll speed | 0/1/2 = ×1/×2/×3 |
| 0x0F | u8 | clear condition category | 0 none, 1 parts, 2 status, 3 actions |
| 0x10 | u32 | clear condition | CRC32 of the condition's internal name; 0 = none |
| 0x14 | u32 | game version bitfield | bit n = game version n (0 = 1.0.0 … 5 = 3.0.1) |
| 0x18 | u32 | management flags | bit0 must be set; bit1 cleared; bit3 lesson/story; bit4 not uploadable; bit5 has clear-condition amount; bit6 has chosen sub-area orientation |
| 0x1C | u32 | clear-check attempts | |
| 0x20 | u32 | clear-check time | 0xFFFFFFFF if not cleared |
| 0x24 | u32 | creation id | random per course |
| 0x28 | u64 | upload id | non-zero = uploaded |
| 0x30 | u32 | clear-check game version | |
| 0x34–0xEF | — | reserved | zero in built-ins; preserved as-is |
| 0xF0 | u8 | unknown | usually 0xFF |
| 0xF1 | char[2] + NUL | game style | `M1` SMB1, `M3` SMB3, `MW` SMW, `WU` NSMBU, `3W` SM3DW — **verified** |
| 0xF4 | UTF-16LE × 0x21 | name | 32 chars + NUL — **verified** |
| 0x136 | UTF-16LE × 0x65 | description | UI limit 75 chars |

### 2.2 Area header (area + 0x00, 0x48 bytes)

| Off | Type | Field | Notes |
|---|---|---|---|
| 0x00 | u8 | theme | 0 ground, 1 underground, 2 castle, 3 airship, 4 underwater, 5 ghost house, 6 snow, 7 desert, 8 sky, 9 forest — main area **verified** |
| 0x01 | u8 | autoscroll type | 0 none, 1 slow, 2 normal, 3 fast, 4 custom |
| 0x02 | u8 | boundary type | 0 built above line, 1 below (sources differ) |
| 0x03 | u8 | orientation | 0 horizontal, 1 vertical |
| 0x04 | u8 | liquid end height | |
| 0x05 | u8 | liquid mode | 0 static, 1 rising or falling, 2 rising and falling |
| 0x06 | u8 | liquid speed | 0 none, 1–3 slow…fast |
| 0x07 | u8 | liquid start height | |
| 0x08 | s32 | boundary right | 0x300 in vertical areas |
| 0x0C | s32 | boundary top | 0x1B0 in horizontal areas |
| 0x10 | s32 | boundary left | 0 |
| 0x14 | s32 | boundary bottom | usually 0 |
| 0x18 | u32 | area flags | values seen: 0, 2, 3. **Night mode is bit 1 (value 2)** — confirmed in the game's code (Phase 3); one community source says bit 0, which is wrong. Bit 0's meaning is unknown (only seen together with bit 1). SM3DW has no night mode. |
| 0x1C–0x44 | s32 × 11 | counts | objects, sound effects, snake blocks, clear pipes, piranha creepers, ! blocks, track blocks, (reserved), ground, tracks, icicles |

### 2.3 Entity arrays (area-relative, fixed capacity, always present)

| Offset | Array | Capacity | Record | Addressing |
|---|---|---|---|---|
| 0x00048 | objects | 2600 | 0x20 | packed, first *count* |
| 0x14548 | sound effects | 300 | 0x04 | packed |
| 0x149F8 | snake blocks | 5 | 0x3C4 | **slot-indexed** |
| 0x15CCC | clear pipes | 200 | 0x124 | **slot-indexed** |
| 0x240EC | piranha creepers | 10 | 0x54 | **slot-indexed** |
| 0x24434 | ! blocks | 10 | 0x2C | **slot-indexed** |
| 0x245EC | track blocks | 10 | 0x2C | **slot-indexed** |
| 0x247A4 | ground tiles | 4000 | 0x04 | packed |
| 0x28624 | tracks | 1500 | 0x0C | packed |
| 0x2CC74 | icicles | 300 | 0x04 | packed |
| 0x2D124 | reserved | — | 0xDBC | |

Two sources list the sound-effect array at 0x14584; that is a digit swap. 0x14548 is correct
(0x48 + 2600 × 0x20) and **verified** by round trip.

**Slot-indexed path arrays (project finding, verified):** entry *i* lives in slot *i* where *i* is
its `index` field; slots may be sparse (e.g. clear pipes in slots 0, 2, 3) and the area count is the
number of live slots. 16 of the 730 checked courses have sparse slots, so a parser that reads only
the first *count* records misses entries. Empty slots are all-zero.

### 2.4 Records

**Object (0x20):** s32 x, s32 y (1/160-tile units, centre-anchored, y up; tile = (v − 80) / 160),
u16 unused, u8 width, u8 height (tiles), u32 flags (default 0x06000040), u32 child flags,
u32 extended data, s16 id, s16 child id (−1 none), s16 link id (−1 none), s16 sound-effect id.

`extended data`: fire bar angle (5° units); snake block path index in the low nibble (bit 0x10 =
start from the other end); piranha creeper / ! block / track block path index.

Flag bits interpreted at spawn (confirmed in the game's code, Phase 3):

| Bit(s) | Meaning |
|---|---|
| 0x1 | emerges from a pipe (link id = the pipe) |
| 0x2 | wings |
| 0x4 | alternate form |
| 0x60 | direction (pipes: right / left / up / down) |
| 0x200 | rides a Koopa Clown Car |
| 0x400 | on a track |
| 0x1000 | part of a stack |
| 0x6000 | size field: `(flags & 0x6000) == 0x4000` = big (doubled size) |
| 0x8000 | parachute |
| 0x10000, 0x20000 | rides a Lakitu cloud |
| 0x8000000 | held by a swinging claw |
| `(flags >> 24) & 7` | facing (0 left, 2 right, 4 down, 6 up) |

Pipes also use `(flags >> 18) & 3` colour and `(flags >> 20) & 0xF` warp id. Blocks and coins only
become live actors when they have wings or are on a track; otherwise they are tiles.
Per-object meanings of the remaining bits are listed in `smmfmt/smm2/enums.py` (ids).

**Sound effect (4):** u8 id, u8 x, u8 y, u8 pad.
**Ground tile (4):** u8 x, u8 y, u8 tile id, u8 background id. Viewers usually auto-tile instead.
**Track (0x0C):** u16 unknown, u8 flags, u8 x, u8 y, u8 type (0 horizontal, 1 vertical, 2–3 diagonal,
4–7 curve, 8–15 junction), u16 link id, u16 × 2 endpoint data. 255 = −1 for x/y.
A piece is centred on object tile (x + 1, y + 1) and joins two points one tile from its centre:
horizontal, vertical, type 2 diagonal down to the right, type 3 up to the right; curves 4–7 join
the same corner pairs as the diagonals. Adjacent pieces share end points. The two end words are
field-atlas tile indices written at the ends (0x104 = nothing written). Verified on the corpus:
433 of 438 on-track objects sit within 24 px of a piece built this way (project finding).
**Icicle (4):** u8 x, u8 y, u8 type (0 falling, 1 fixed), u8 pad.
**Snake block (0x3C4):** u8 index, u8 node count, u8 in-use (1), pad, then 120 × {u16 index,
u16 direction, u16 0x100, pad}.
**Clear pipe (0x124):** u8 index, u8 node count, u8 in-use (1), pad, then 36 × {u8 type, index, x,
y, width, height, 1, direction}.
**Piranha creeper (0x54), ! block / track block (0x2C):** u8 in-use (1), u8 index, u8 node count,
pad, then 20 (creeper) or 10 × {u8 1, u8 direction, u16 pad}. Note `index` is at +1 here.

Path directions: 1 left, 2 right, 3 down, 4 up, 5–12 corners (left→down, down→left, left→up,
up→left, right→down, down→right, right→up, up→right), 13–16 ends (right, left, up, down).

### 2.5 Links, warps and paths (project findings, verified on the 550-course sample)

- **Link ids** tie a rider to its carrier. Every pipe, Lakitu's Cloud, Koopa Clown Car and swinging
  claw has one (2955 of 2955 pipes); a part in a pipe (flag 0x1), in a clown car (0x200), on a cloud
  (0x10000) or held by a claw (0x8000000) has the same id. Track pieces use the same numbers, stored
  + 1; a part on a track (0x400) has the id of the piece it sits on, and that piece's byte +2 is 1
  (657 of 657). Ids are small and mostly dense from 0 per area.
- **Rider positions:** in a pipe, the mouth: base + (L − 1) tiles along the pipe, then ±8 px on both
  axes (up: (+8, +8), down: (−8, −8), right: (+8, −8), left: (−8, +8)). In a clown car 24 px above it
  (32 for big riders), on a cloud 8 px (16 big).
- **Pipes:** the stored position is the base tile's centre: bottom-left for an up pipe, top-right
  for down, bottom-right for left, top-left for right; width 2, height = length (2–27 seen).
  Warp id k ≠ 0 joins the main-area pipe k with the sub-area pipe k (843 of 843 pairs cross areas).
- **Doors / warp boxes:** ids 2k and 2k + 1 in the same area are a pair; both doors have the same
  kind ((flags >> 18) & 3: 0 normal, 1 P, 2 key) and both boxes the same key flag (0x4).
- **Big parts** store their doubled size (a big Goomba is 2×2, x on the tile edge).
- **Track end codes:** a free end gets a cap, closed 0x70–0x77 or open 0x80–0x87 (horizontal: k0
  right 0x71, k1 left 0x70; vertical: k0 top 0x72, k1 bottom 0x73; type 2: 0x75 / 0x74; type 3:
  0x77 / 0x76; open = + 0x10). A shared end gets a joint code from one piece and 0x104 from the
  other: straight runs 0x90 / 0x91 / 0x92 / 0x93 written by the k0 end; mixed joints use 0x94–0xA7.
- **Paths** (snake, ! block, track block, creeper): one node per step, code = direction (1 left,
  2 right, 3 down, 4 up), corner (5–12, old → new direction) or the closing end (13–16, last
  direction). The part's extended data is slot + 1 (snake: low nibble). Snake heads are
  {slot, count, 1, 0} with nodes {number, code, 0x100}; the others {1, slot, count, 0} with nodes
  {1, code}.
- **Areas:** horizontal areas are 27 tiles tall and 24–240 wide; vertical sub-areas are 48 wide and
  28–168 tall in steps of 7. Unused sub-areas are 84 tiles wide. Management flag bit 6 is set in every
  course whose sub-area holds anything. goal_x = (right − 152 px) × 10 / 16.
- **Start / goal:** no ground tile and almost no part sits in columns 0–6 below start_y + 3 or on
  the goal ground (below goal_y, the last 10 columns).

## 3. Object ids

0–132, from Goomba (0) to ON/OFF trampoline (132). Full list in
[`smmfmt/smm2/enums.py`](../../smmfmt/smm2/enums.py). v1.0.0 accepts ids 0–119 (the ! block, 119,
is already there) but 74, 78–81 and 111 are empty; later versions fill those and add 120–132
(Koopalings, masks, boxes, ON/OFF trampoline). v1.0.0 silently drops ids above 119. In the 730-course sample, 102 distinct
ids appear; the most common are hard block, coin, brick, spike trap, dotted-line block, ice block.

## 4. Course codes

9 characters from `0123456789BCDFGHJKLMNPQRSTVWXY`. Read the code reversed as a base-30 number
*n*. Bits: 0–11 high part of the id, 12 always 1, 13 maker flag, 14–33 low 20 bits, 34–39 check
(`(data_id − 31) % 64`), 40–43 always 8. `data_id = ((n & 0xFFF) << 20 | (n >> 14) & 0xFFFFF) ^ 0x1680E07C`.
Implemented and tested in [`smmfmt/course_id.py`](../../smmfmt/course_id.py).

## 5. Open questions

- ~~Night-mode bit~~ resolved: bit 1. Area-flag bit 0: meaning unknown.
- Clear condition values are the CRC32 of the game's internal condition names; all 29 distinct
  values in the 550-course sample match a known name.
- ~~"Big" object flag~~ resolved: size field 0x6000, big = 0x4000; 0x400 = on a track.
- Header 0xF0, track bytes 0/8/10, the path "in-use" bytes: meaning unknown, preserved verbatim.
- Ground tile ids vs auto-tiling: does the game use the stored ids? (Phase 3.)

## Sources

| Source | Licence | Used for |
|---|---|---|
| TheGreatRambler/toost `level.ksy` (2022-03) | MIT | full body layout, enums, clear conditions |
| liamadvance/smm2-documentation (2022-04) | none — facts only | header, area, flags, encryption overview |
| Treeki/SMM2Reversing `LevelFormat.md` (2019-07, v1.0.0) | none — facts only | layout cross-check, field semantics |
| kinnay/Nintendo-File-Formats `smm2/level.md`, `encryption.md` (2026-10) | none — facts only | header, wrapper, key derivation |
| kinnay/NintendoClients wiki (2026-10) | MIT | course code algorithm, key generation |
| mm2srv/smm2_parsing (2025-09) | MIT | encryption cross-check |
| simontime/SMM2CourseDecryptor | MIT | encryption cross-check |
| Viewers: JiXiaomai/SMM2LevelViewer, wizulus/smm2viewer, jonbarrow/smm2-level-viewer | none / ISC | flag-bit interpretation |
| HF dataset TheGreatRambler/mm2_level | CC BY-NC-SA 4.0 | test corpus (not redistributed) |
