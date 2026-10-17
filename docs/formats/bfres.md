# BFRES (Switch, version 0.9) — subset read by the engine

SMM2 stores every model as a BFRES inside a SARC (`Model/*.zs`, `Pack/<style>_Model.pack` and,
for 3D World blocks, coins and pipes, `Pack/StaticModel.pack`). The 2D styles use BFRES only as
sprite containers: each animation frame is its own texture (embedded BNTX, named
`<anim>.<frame>`) drawn on a quad mesh, and texture-pattern animations switch textures. NSMBU
draws enemies, items and objects the same way, with pre-rendered frames (`walk_Alb.000` …) on a
quad, but its players, Yoshi and bosses are skinned meshes; 3D World is meshes throughout,
including the terrain (one model per field-atlas cell). Reader:
[`core/crates/formats/src/bfres.rs`](../../core/crates/formats/src/bfres.rs).

Little-endian. Offsets are absolute file offsets (u64). Strings are a u16 length followed by the
bytes. Resource dictionaries: u32 (zero) + u32 count + (count + 1) 16-byte entries, entry 0 is the
root; the name offset is at +8 of each entry. Structure arrays are contiguous.

## Header
| Off | Type | Field |
|---|---|---|
| 0x00 | char[8] | `FRES    ` |
| 0x08 | u32 | version `0x00090000` (0.9.0.0) |
| 0x0C | u16 | BOM `0xFEFF` |
| 0x20 | off | name |
| 0x28 / 0x30 | off | models (FMDL) / dict |
| 0x58 / 0x60 | off | skeletal animations (FSKA) / dict |
| 0x68 / 0x70 | off | material animations (FMAA) / dict |
| 0x78 / 0x80 | off | bone visibility animations (FBVS) / dict |
| 0xB0 | off | buffer info: u32, u32 size, u64 offset of the GPU data block (vertex/index buffers) |

## Model `FMDL` (0x78)
+0x08 name · +0x18 skeleton · +0x20 vertex buffers · +0x28 shapes · +0x38 materials ·
+0x68 / +0x6A / +0x6C u16 vertex buffer / shape / material counts.

## Skeleton `FSKL`, bones (0x60 each)
Skeleton: +0x04 u32 flags (bits 8–9 scaling mode: 1 standard, 2 Maya; bit 12 Euler rotations),
+0x08 bone dict, +0x10 bone array, +0x18 matrix palette (u16 bone index per entry: the smooth
matrices, then the rigid ones), +0x20 inverse model matrices (f32 3×4 row-major, one per smooth
matrix), +0x38 / +0x3A / +0x3C u16 bone / smooth / rigid counts.

Bone: +0x00 name, +0x28 u16 index, +0x2A i16 parent (−1 root), +0x2C / +0x2E i16 smooth / rigid
palette index (−1 none), +0x34 u32 flags (0x1 visible; 0x1000: rotation is Euler XYZ, else a
quaternion; 0x0080_0000 segment scale compensation), +0x38 f32×3 scale, +0x44 f32×4 rotation,
+0x54 f32×3 translation. Rest transform = T · Rz · Ry · Rx · S, parent first; with segment scale
compensation T · S_parent⁻¹ · R · S. Checked on the owner's files: rest-pose model matrix ×
stored inverse = identity for every smooth matrix.

## Material `FMAT` (0xA8)
+0x08 name · +0x30 texture name offsets (one per sampler) · +0x40 samplers (0x20 each; bytes
0–2 = wrap u/v/w: 0 repeat, 1 mirrored repeat, 2 clamp) · +0x48 sampler dict (slot names such as
`_a0`) · +0x9C u8 texture count · +0x9D u8 sampler count.

## Shape `FSHP` (0x60)
+0x04 u32 flags · +0x08 name · +0x10 vertex buffer · +0x18 meshes (one per LOD) · +0x20 skin
bone list (u16) · +0x50 u16 index · +0x52 u16 material · +0x54 u16 bone (rigid shapes follow it;
also the bone whose visibility shows the shape) · +0x56 u16 vertex buffer · +0x58 u16 skin bone
count · +0x5A u8 skin count · +0x5B u8 mesh (LOD) count · +0x5C u8 shape-key count.

Skinning: skin count 0 = vertices in the shape bone's space; 1 = each vertex in the space of one
bone, `_i0` (u8) names its palette entry (a rigid one); 2–4 = smooth, vertices in model space
(bind pose), `_i0` (u8×2 / u8×4) palette entries and `_w0` (unorm8) weights summing to 1.
`_i0` indexes the skeleton's matrix palette, not the shape's skin bone list.

## Mesh (LOD, 0x38)
+0x00 sub-mesh array · +0x08 memory pool · +0x10 buffer · +0x18 buffer size (u32 byte size first)
· +0x20 u32 index data offset in the GPU data block · +0x24 u32 primitive (3 = triangle list, the
only value seen) · +0x28 u32 index format (0 u8, 1 u16, 2 u32) · +0x2C u32 index count · +0x30
u32 first vertex (added to every index; LODs share one vertex buffer) · +0x34 u16 sub-mesh count.
The GPU data block holds all index buffers first, then the vertex buffers.

## Vertex buffer `FVTX` (0x58)
+0x08 attributes (0x10 each: +0 name, +8 u16 format, +0xC u16 byte offset, +0xE u16 buffer) ·
+0x30 buffer sizes and +0x38 strides (16-byte entries, first u32) · +0x48 u32 offset of the first
buffer in the GPU data block (later buffers follow, 8-byte aligned) · +0x4C / +0x4D u8 attribute /
buffer count · +0x50 u32 vertex count.

Attribute format: high byte = layout (0x02 8, 0x09 8_8, 0x0B 8_8_8_8, 0x0E 10_10_10_2,
0x12 16_16, 0x15 16_16_16_16, 0x17 32_32, 0x18 32_32_32), low byte = 1 unorm, 2 snorm, 3 uint,
4 sint, 5 float. Names: `_p0` position, `_u0` / `_u1` UV sets, `_n0` normal, `_t0` tangent,
`_c0` vertex colour, `_i0` bone indices, `_w0` weights. UV (0, 0) is the top-left of the texture.

## Material animation `FMAA` (0x70)
+0x04 u32 flags (bit 2 = looping) · +0x08 name · +0x28 per-material data · +0x38 texture name
offsets · +0x58 i32 frame count · +0x62 u16 per-material count · +0x6C u16 texture count.

Per-material data (0x40): +0x00 material name, +0x10 pattern infos, +0x18 curves, +0x20
constants, +0x32 / +0x34 / +0x36 / +0x38 u16 counts of parameter tracks / patterns / constants /
curves. Pattern info (0x10): sampler name, +8 u16 curve index into this material's curves
(0xFFFF = none), +0xA i16 constant index. Constant: u32 target, i32 value.

Curve (0x30): +0x00 frames, +0x08 keys, +0x10 u16 flags (bits 0–1 frame type: 0 f32, 1 s16/32,
2 u8; bits 2–3 key type: 0 s32, 1 s16, 2 s8; bits 4–6 curve type, 4 = integer step),
+0x12 u16 key count, +0x18 / +0x1C f32 start / end frame, +0x24 s32 offset. Texture index =
offset + key, held until the next key. Looping curves may end with a key at frame == frame count.

## Skeletal animation `FSKA` (0x50)
+0x04 u32 flags (bit 2 looping; bits 8–9 scaling mode; bit 12 Euler rotations, else quaternions)
· +0x08 name · +0x10 path · +0x18 bound skeleton · +0x20 bind indices (u16 per bone animation,
0xFFFF = by name) · +0x28 bone animations · +0x40 i32 frame count · +0x44 i32 curve count ·
+0x48 u32 baked size · +0x4C u16 bone animation count.

Bone animation (0x38): +0x00 name (matched to the skeleton by name), +0x08 curves, +0x10 base
values, +0x28 u32 flags, +0x2E u8 curve count. Flags 0x08 / 0x10 / 0x20: scale (f32×3) /
rotation (f32×4) / translation (f32×3) base values present, stored in that order; they equal the
curves' frame-0 values (Euler angles may differ by a full turn). Curve targets are byte offsets
into the bone data: 0x04/0x08/0x0C scale x/y/z, 0x10/0x14/0x18 translation, 0x20…0x2C rotation
x/y/z/w. The player animations live in files of their own (`WU_Player_CommonAnm`,
`3W_Player_Animation_Mario`, …) and bind to the player skeletons by bone name.

## Curves (0x30, all animation kinds)
+0x00 frames, +0x08 keys, +0x10 u16 flags (bits 0–1 frame type: 0 f32, 1 s16 / 32, 2 u8; bits
2–3 key type: 0 f32, 1 s16, 2 s8; bits 4–6 curve type: 0 cubic, 1 linear, 2 baked float, 4 step
int, 5 baked int, 6 step bool, 7 baked bool; bits 8–9 / 12–13 pre / post wrap: 0 clamp,
1 repeat, 2 mirror, 3 relative repeat), +0x12 u16 key count, +0x14 u32 target, +0x18 / +0x1C f32
start / end frame, +0x20 f32 scale, +0x24 offset (f32, or s32 for integer curves), +0x28 f32
delta (end − start value).

Cubic keys are 4 coefficients per key, linear 2, others 1. A value is stored × scale, plus the
offset on the first coefficient only; per segment v(t) = c0 + c1·t + c2·t² + c3·t³ with t = 0…1
from one key frame to the next. Verified on the owner's files: every segment ends where the next
key starts. Bool keys are a bit array (u32 words, LSB first).

## Bone visibility animation `FBVS` (0x60)
+0x04 u32 flags (bit 2 looping) · +0x08 name · +0x10 path · +0x18 bound model · +0x20 bind
indices · +0x28 curves (target = index into the names) · +0x30 base values (bit array) · +0x38
bone names (offset array) · +0x50 i32 frame count · +0x54 baked size · +0x58 u16 name count ·
+0x5A u16 curve count. 3D World faces, eyes and hands are sets of alternative shapes on sibling
bones (`EyeOpen` / `EyeHalfClose` / `EyeClose`, `Face00`…`Face08`) that these tracks switch.
