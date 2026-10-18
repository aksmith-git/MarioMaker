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

## Windows and Linux: the Vulkan backend (2026-10-19)

The ports are built on macOS with zig 0.17 (`make TARGET=windows-x86_64 | linux-x86_64 | linux-aarch64`). The
generated C needs no changes: arm64 shares `generated/smm2` with macOS, and x86-64 uses the portable-C lowering
(`generated/smm2-c`).

**Build fixes:**
- SDL 3.2 has no `SDL_LoadPNG` or `SDL_SavePNG`. A minimal PNG writer takes over when SDL is older than 3.4.
- `__builtin_cpu_supports` needs libgcc's `__cpu_model`. The AVX2 check now reads CPUID directly
  (`platform/x86_cpu.h`).
- zig's lld-link has no `--export-all-symbols`. Linker directives (`platform/windows_exports.c`) export the eight
  runtime symbols the game code calls, and `zig dlltool` makes the exe's import library from `windows_exports.def`.
- zig wants `X.lib` import-library names, not MinGW's `libX.dll.a`.
- SMM2 has no movie library, so FFmpeg is not linked (`GAME_MOVIES := 0`).

**What the Metal side had, ported to Vulkan (`runtime/nvn/vulkan`) and to the GLSL output of the translator:**
- Conditional rendering (`SetRenderEnableConditional`), `ReportValue` and `ClearBuffer`.
- Polygons culled on both faces are skipped.
- 3D colour targets: a depth slice is drawn through a 2D view of a `2D_ARRAY_COMPATIBLE` image.
- First-render uploads.
- Storage aliases: texture objects over the same memory share one image.
- Cube-array views of 2D arrays.
- In-order `UpdateUniformBuffer`: a flush when guest memory under recorded draws would change.
- A flush after storing compute dispatches.
- Blocking shader and pipeline builds.
- TLD4 (`textureGather`) and TMML (`textureQueryLod`).
- Geometry shaders as real Vulkan geometry stages, in descriptor set 2. They have point input, and the output topology
  and vertex count come from the shader header.

**Linux arm64 in a VM (Mesa lavapipe):**
- The build boots to the title screen.
- Its frames match Metal's except for one thing: the Mario model is missing in the 3D World title course.
  - His draws are issued, with the same programs, vertex counts and textures as on Metal.
  - His texture is uploaded and the material alpha uniform is 1.
  - The translated GLSL matches the MSL statement for statement (Metal's `LARECOMP_SAVE_GLSL=1` writes both). The
    signed LDC offsets and the bone-matrix reads are the same code.
  - [obs] He *is* drawn. In a pass with two targets he is drawn into a character layer (1600x900 RG11B10F): pass
    images (`LARECOMP_DEBUG_PASS_NCOLOR=2`) show him there.
  - The scene's composite shader then copies him into the scene where target 1 (RG16F) is not exactly 1.0. It
    discards wherever that target reads 1.0, its cleared value.
  - [obs] A texel probe (`LARECOMP_DEBUG_PROBE=310,555`) reads target 1 as 1.0 (0x3c00) at Mario's pixels after his
    pass. His fragment shader's second output never lands there.
  - So the composite discards him. Not the cause: copy aliases (`LARECOMP_VK_COPY_OPT=0`) and storage aliases
    (`RECOMP_NO_TEX_ALIAS=1`).
  - Next: that target's write mask, attachment or clear order in the two-target pass.
- Comparing captures needs care with colour:
  - Vulkan read-backs are raw sRGB; the Ryujinx references are window-server grabs.
  - Raw against grab is about 23 % of pixels apart on colour alone, for Metal too.
  - Compare raw with raw, or map the raw capture through the colour transform measured from a Metal raw / grab pair
    of the same frame.

## Open issues

- ~~A title course swapped into another slot crashes the recomp; Ryujinx runs the all-slots mod.~~ Not a recomp bug
  (R2, [obs]): Ryujinx does the same once it is made to pick the same slot. With the game's course 00 in every slot,
  Ryujinx with every `sead::Random` seeded 4 (slot 00, the course's own) runs, and seeded 1 (slot 02, like the
  recomp's `SMM2_SEED=1`) exits while the title scene loads, as the recomp crashes there; courses 03 and 06 in slot 00
  end both runtimes too. SMCraft's test courses load in any slot in both. Ryujinx usually picks slot 00 on its own
  (its boot tick), which is why the all-slots mod seemed to work there.
- Geometry shaders with other input primitives, several strips, or textures are not handled (draws skipped).

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

- Test courses: SMCraft's physics test courses (`mmk trace-courses`, in the full CLI on the `smcraft-archive`
  branch), `<style>_<kind>` for the five styles and flat / wall / slope / water / items / form.
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
- [obs] A game title course in another slot than its own crashes on loading in both runtimes alike (recomp: a null
  read; Ryujinx: the emulator exits at the same point; see "Open issues"), so the title screen apparently expects
  each slot's own course [inf]; SMCraft's test courses load in any slot.
