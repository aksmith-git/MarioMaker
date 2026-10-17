# Decisions

Newest first. Each entry: what was decided, why, and what would change it.

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
- Sprite art offsets inside textures are measured from the textures (e.g. the Goomba sits in the
  top half of its 16×32 image) until BFRES quad placement is imported.

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
