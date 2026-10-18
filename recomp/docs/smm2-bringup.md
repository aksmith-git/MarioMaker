# SMM2 v3.0.3 bring-up: findings

What Super Mario Maker 2 v3.0.3 needed from the runtime forked from LAS Recomp, in the order it came up, and how the
title screen was checked against Ryujinx. Names and behaviour only: no game code, disassembly or assets.

## Toolchain

- Both modules recompile with LAS's generator (`sdk` ~34,000 functions, `main` ~105,000). Two functions in `main`
  are giant `switch` dispatchers (~40k instructions each) that took over 20 minutes each at `-O1`; functions whose
  all-label switch exceeds 16k instructions are now emitted `optnone` (`tools/gen.py`, `HUGE_SWITCH_FN`).
- The game code builds into its own library (`smm2game.dylib`, entry `recomp_game_tables`) next to the runtime
  executable, as LAS does.

## Kernel

- Per-core scheduling. SMM2 pins its worker threads to cores (SetThreadCoreMask 1, 2, 4) and relies on threads of
  one core never running at the same time (job queues without locks). LAS ran every guest thread freely on host
  threads; random crashes and hangs went away once each guest core runs one thread at a time (`runtime/kernel.c`:
  `core_acquire` / `core_release` at each SVC; `RECOMP_NO_CORES=1` turns it off).
- GetInfo(RandomEntropy) is random, but the game's generators seed themselves from the system tick
  (`sead::Random::init()`); see "Repeatable boots" below.

## Services and system data

- `nifm` (offline: no network, as Ryujinx answers), `friend`, `mii` (empty database), `acc` (nickname from
  `RECOMP_USER_NAME`, default "Player"), `am` GetDisplayVersion (`GAME_DISPLAY_VERSION`, 3.0.3: shown on the title
  screen).
- Shared fonts (`pl:u` RequestSharedFontLoad and the font data, from the firmware's font archives; the `.bfttf`
  header is big-endian) and system data archives opened by data id (MiiModel 0802, NgWord 0806 / 0823, TimeZone
  080e). `games/smm2/sysdata.py` extracts them from the player's firmware with their keys, read in place.
- RomFS is served from a folder through an in-memory index (`runtime/romfs_build.c`): the update's files over the
  base game's, as links, nothing copied.

## NVN

- API version: SMM2 checks NVN 53.311 (device integer 1 = 311) and dereferences a null global otherwise.
- Command lists: nested recording in shared command memory and CopyCommands. Lists are recorded into host
  buffers and copied out at EndRecording (the handle is the host copy); the guest's command-memory use is estimated
  so a list's lifetime follows the memory the game reuses.
- Virtual memory pools: a 112 MB virtual pool mapped whole onto a physical pool aliases its storage.
- Textures sharing storage alias one Metal texture.
- Display pacing at 60 Hz from presents (the game would run at 120 fps).
- **Conditional rendering.** SMM2 clears a render target on its first bind each frame by guarding ClearColor with
  SetRenderEnableConditional on a per-target report, then resets the report (ReportCounter / ReportValue /
  ClearBuffer) so later binds skip the clear. LAS ignored the condition, so every later bind cleared the layout
  layers drawn before it: every frame was black. SetRenderEnable(Conditional), ReportValue and ClearBuffer now run
  in submission order; the condition compares the two 64-bit counters at the address and address + 16 (mode 0:
  render if equal, 1: if not equal), as Ryujinx does.
- **3D render targets.** The colour-correction LUT (an 8x8x8 3D texture) is filled with eight colour targets, one
  slice each (a view's layer is a depth plane in Metal).
- **First-use shader compiles block.** LAS compiled programs in the background and skipped their draws until ready;
  SMM2 renders the LUT and light probes once, so a skipped draw stays missing (black scene). Blocking compiles plus
  the warm-up cache; `LARECOMP_ASYNC_SHADERS=1` restores LAS's behaviour.
- **Narrow vertex strides.** Metal needs vertex strides in multiples of 4; streams with a 1- or 2-byte stride
  (SMM2's terrain stores small per-vertex indices that way) are repacked into padded records. Before that, 3D World
  terrain was drawn as screen-sized polygons.
- **UpdateUniformBuffer order.** The guest GPU runs it in command order; we apply it on the CPU while encoding, so
  it now first waits for encoded work that reads the same memory in place (vertex, index and storage buffers,
  compute uniforms; tracked in 64 KB pages).
- Cube-array views of 2D arrays (72-layer character reflection maps).
- Culling both faces (NVN Face FRONT_AND_BACK) draws no polygons; Metal has no such mode and the renderer had
  fallen back to no culling. The 3D World background switches a clear pipe's opaque yellow stand-in off that way.
- A texture rendered to before it was ever sampled first gets its memory's contents, as on the GPU. Each 2D style's
  tile sheet is loaded from a file and the animated tiles are then drawn into that same texture every frame; only
  the animated cells existed, so ground, blocks and castles were missing.
- Geometry shaders (Metal has none). The vertex shader becomes a `vs_main()` function and the geometry shader a
  code fragment of one combined vertex function; each input point is an instance drawing the emitted triangle
  strip, the n-th EmitVertex chosen by the vertex id (`maxwell/shader.h`). Point input with one strip per
  invocation, which is what SMM2's 2D tiles and some effects use.

## Maxwell shader translator

- `TLD4` (gather, with AOFFI offsets), `TMML` (level-of-detail query, 8.8 fixed point like Ryujinx's), and a fix
  for `LDC.64` whose address register is also its first destination (the second word was read from the address the
  first load had just overwritten: the layout vertex shader fetches each corner's texture coordinates that way, and
  the title logo, copyright line and coin icon were invisible).
- Compute results are flushed before draws that read them; textures sampled with `TLD4` in compute work.
- `LDC` offsets are signed when added to the register (as in Ryujinx's translator). SMM2's skinning shaders read
  bone matrix rows at `c[6][Rx - 0x10]`; taken as +0xfff0 that read past the bone palette and threw every skinned
  mesh off screen (Mario's body, the Koopa Car), leaving only rigid parts.

## Repeatable boots

The title screen's course comes from the game's global `sead::Random`, which seeds itself from the system tick at
start-up: slot `(n * random) >> 32` of `Course/title_course_data_NN` with n = 8, or 10 once the save unlocks the
last two. `SMM2_SEED=<n>` (`games/smm2/seed.c`) fixes that seed: frame 2400 is then bit-identical from boot to
boot. Seed -> slot: 4 -> 00 (3D World), 2 -> 01 (SMB1), 1 -> 02 (SMB1), 6 -> 05 (SMB3), 3 -> 06 (SMB3),
10 -> 07 (NSMBU); 03, 08 and 09 did not come up in 22 seeds with this save.

For Ryujinx the course is chosen with a LayeredFS mod making all ten slots one course (`titlemod.py --course N`).
The same mod in the recomp crashes while loading the title scene when the course is not the slot's own (open issue
below). An exefs patch seeding Ryujinx's generator instead made Ryujinx exit at start-up every time.

## Title screen against Ryujinx (2026-10-19)

Current numbers (after the second round of fixes; window-server captures of both windows, 1280x720 game area, the
recomp at render scale 1 like Ryujinx's docked 1080p, the best-matching recomp frame of 2300..2520 against one
Ryujinx grab; % of pixels differing by more than 24 / 48 / 96 in some channel):

| Course (style) | Recomp frame | > 24 | > 48 | > 96 | at frame 2400 (> 24) |
|---|---|---|---|---|---|
| 00 (3D World) | 2376 | 2.50 % | 0.66 % | 0.13 % | 2.72 % |
| 01 (SMB1) | 2460 | 4.17 % | 2.25 % | 0.99 % | 6.12 % |
| 02 (SMB1, underground) | 2480 | 4.80 % | 2.47 % | 1.05 % | 5.37 % |
| 05 (SMB3, desert) | 2488 | 4.71 % | 2.98 % | 1.15 % | 4.79 % |
| 07 (NSMBU, snow) | 2304 | 2.99 % | 0.94 % | 0.27 % | 3.58 % |

What is left in the difference maps: edges of pixel-art tiles and text (texel-boundary sampling / scaling, a pixel
here and there), the "Press L+R" button's blink and the moving characters (animation phase). Ryujinx against
itself, five boots of the same course: 0.3-1.8 %.

Animation phase cannot be lined up exactly: Ryujinx's grabs are timed by the wall clock and its boots differ in
length (the title timer read 476-481 across five identical boots at the same grab time), and the reference build
exposes no frame count. The recomp is frame-exact, so the nearest frame is found by searching a window of recomp
frames for the best match (`LARECOMP_CG_SHOT=<first>:<last>:<step>`).

Earlier (R1, first round):

- Reference: Ryujinx 1.3.2, the same dump (v1.0.0 + v3.0.3 update), private data root, window grabbed 48 s after
  launch on five boots (identical course; boot-to-boot difference 0.3-1.8 % of pixels by more than 24).
- Ours: `SMM2_SEED=4`, frame 2400, window captured the same way (`LARECOMP_CG_SHOT=2400`, window 1280x720): both go
  through the window server's colour management. (Comparing our raw frame with Ryujinx's window grab shows a
  uniform colour shift: 26.6 % of pixels by more than 24.)
- Result (1280x720, `imgcmp.py`): 6.96 % of pixels differ by more than 24 in some channel (3.80 % by more than 48,
  1.30 % by more than 96), mean difference 7.6.
- Remaining differences then: Mario's model drawn wrong (a blue block), the Koopa Car missing (a grey pole in its
  place), animation phase; all but the phase fixed since (above).
- Boot sequence: the yellow "SUPER MARIO MAKER" / "2" logo screen with the running SMB1 Mario, then the title, as on
  Ryujinx.

## Open issues

- A title course swapped into another slot (all-ten-slots mod, or two slots swapped) crashes the recomp while the
  title scene loads (a null pointer in the scene setup); Ryujinx runs the all-slots mod.
- Geometry shaders with other input primitives, several strips, or textures are not handled (draws skipped).
