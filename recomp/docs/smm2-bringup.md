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

## Maxwell shader translator

- `TLD4` (gather, with AOFFI offsets), `TMML` (level-of-detail query, 8.8 fixed point like Ryujinx's), and a fix
  for `LDC.64` whose address register is also its first destination (the second word was read from the address the
  first load had just overwritten: the layout vertex shader fetches each corner's texture coordinates that way, and
  the title logo, copyright line and coin icon were invisible).
- Compute results are flushed before draws that read them; textures sampled with `TLD4` in compute work.

## Repeatable boots

The title screen's course comes from the game's global `sead::Random`, which seeds itself from the system tick at
start-up. `SMM2_SEED=<n>` (`games/smm2/seed.c`) fixes that seed: frame 2400 is then bit-identical from boot to boot.
Ryujinx showed the same course on every boot; `SMM2_SEED=4` is that course.

## Title screen against Ryujinx (2026-10-19)

- Reference: Ryujinx 1.3.2, the same dump (v1.0.0 + v3.0.3 update), private data root, window grabbed 48 s after
  launch on five boots (identical course; boot-to-boot difference 0.3-1.8 % of pixels by more than 24).
- Ours: `SMM2_SEED=4`, frame 2400, window captured the same way (`LARECOMP_CG_SHOT=2400`, window 1280x720): both go
  through the window server's colour management. (Comparing our raw frame with Ryujinx's window grab shows a
  uniform colour shift: 26.6 % of pixels by more than 24.)
- Result (1280x720, `imgcmp.py`): 6.96 % of pixels differ by more than 24 in some channel (3.80 % by more than 48,
  1.30 % by more than 96), mean difference 7.6.
- Remaining differences: Mario's model is drawn wrong (a blue block), the Koopa Car in the middle is missing (a grey
  pole stands there instead), and the course's animations are at a different phase.
- Boot sequence: the yellow "SUPER MARIO MAKER" / "2" logo screen with the running SMB1 Mario, then the title, as on
  Ryujinx.

## Input replays against Ryujinx (R2, 2026-10-19)

### Input

- [obs] SMM2 reads its controllers with `nn::hid::GetNpadStates` (16 entries, FullKey for player 1). The SDK's own
  version reads hid shared memory, which the runtime never filled, so up to R1 the game saw no input at all (keyboard
  included). The runtime now answers `GetNpadStates` itself (`runtime/hle/input.c`): each call is one new sample, the
  older entries repeat it with earlier sampling numbers.
- Scripted pad (`runtime/padscript.h`, `RECOMP_PAD_SCRIPT`): the Ryujinx trace tooling's scripts as they are
  (`N BUTTONS` per line; JUMP=A RUN=Y SPIN=ZR), one script frame per tick of a game clock, never wall time; keyboard,
  mouse and controllers are left out while it runs; sticks at rest, the d-pad stays the d-pad (Ryujinx's override
  does the same).
- The clock and the trace point are the player's per-frame update. `games/smm2/patches.py` patches its first
  instruction after the prologue (main+0x158a5c4, `x19 = x0`) into a call to `smm2_player_hook()`
  (`games/smm2/trace.c`) followed by the original move: the same instruction the Ryujinx trace build turns into
  `svc #0x7E`. At each update the hook writes a record (TraceHook's format: the 0x3000-byte player object and the
  0x200-byte input block it points to at +0x550), then applies the next script frame.
- [obs] Latency matches Ryujinx's: the pad state set at update k is read by update k + 1, so script frame j shows from
  record j + 2 on, in both runtimes.
- Menus have no player: `RECOMP_PAD_CLOCK=frame` ticks per presented frame instead. On the title screen L + R opens
  the Make / Play menu; Make, then − (after the first-run dialogs) starts a test play of the Course Maker course
  [obs, screenshots in the evidence folder].

### Method

- Same start in both runtimes: the test course is installed as all ten title courses (recomp `RECOMP_MOD_DIR`,
  Ryujinx LayeredFS), and the script starts at the first update of the title-course player (Ryujinx: the `run`
  command is armed before the player exists). One script per course plays every test of that course kind with a
  reset between tests (run left into the course's edge, settle, walk right to about x = 100, settle); the resets are
  sized once from a recomp pre-run and are then the same frames in both runtimes, so record k is the same game frame
  in both. `tools/replay_sync.py` builds the sessions, runs both runtimes and compares.
- Compared per update: x, y, vx, vy, facing, speedF, state id and frames in state (bit for bit), plus the whole
  0x3000-byte player object and the input block, with pointers compared by what they point into (the two runtimes'
  heaps are at different addresses).
- Randomness: the title screen picks one of ten courses and the game makes several `sead::Random` generators, each
  seeded from the system tick. The test-course runs need only the recomp's `SMM2_SEED=4` (all ten slots hold the same
  course). For the game's own title course, `SEEDALL=1` seeds every generator with 4 in both runtimes
  (`SMM2_SEED_ALL=1`; on Ryujinx an IPS mod turns the tick read in `sead::Random::init()` into the constant).
  Without that, Ryujinx picked a different title course and nothing could be compared.

### Results

| session | tests | updates compared | identical | test frames | identical | first divergence |
|---|---|---|---|---|---|---|
| M1 / M3 / MW / WU / 3W `_flat` | 34 each | 14,665 / 14,665 / 14,956 / 14,628 / 14,730 | all | 4,096 each | all | none |
| `_wall` (5 styles) | 4 each | 3,153-3,207 | all | 1,586 each | all | none |
| `_slope` (5 styles) | 4 each | 2,749-2,770 | all | 1,195 each | all | none |
| `_water` (5 styles) | 2 each | 1,066 each | all | 263 each | all | none |
| `_items` (5 styles, as Super Mario) | 34 each | 15,245-15,339 | all | 4,336 each | all | none |
| `_form` (M3 raccoon, MW cape, WU propeller, 3W cat) | 16 each | 7,646-7,671 | all | 2,281 each | all | none |
| stock title course to the goal (Koopa Car, enemies) | 1 | 1,911 | all | 1,909 | all | none |
| stock title course, varied inputs, ends in a death | 1 | 3,537 | all | 3,534 | all | none |
| **all 31 sessions** | **456** | **221,189** | **221,189** | **71,947** | **71,947** | **none** |

Per-session tables (each test, frames compared, identical, first divergence): `private/smm2/evidence/r2/results/`.

- [obs] No divergence in position, speed or state anywhere. The whole player object differs only in one 4-byte word
  (+0x644) during 4 updates of the title-course runs: it holds the low half of a heap address (0x0af69540 in Ryujinx,
  0x02a65540 here, next to two floats with the same values in both), so it differs by design [inf].
- [obs] The recomp repeats itself: a second run of the M1_wall session is identical record for record.
- [obs] Reaching the goal ends a trace in both: the title course starts again with a new player object and the
  script, which follows the first one, stops (stock_clear: after 1,911 updates in both). After a death on the title
  course (stock_varied) the same player object keeps being updated in its death state to the script's end.
- [obs] Before R2 the recomp had never been given input, so none of this had been exercised: no lifter, FP/SIMD or
  scheduling bug showed up in 221,189 compared updates, and none needed fixing for R2.
- [obs] Swapping the title course for another of the game's own (`titlemod.py --course 3` or `6`) crashes on loading
  in both runtimes alike (recomp: a null read; Ryujinx: the emulator exits at the same point), so the title screen
  apparently expects each slot's own course [inf]; SMCraft's test courses load in any slot.
