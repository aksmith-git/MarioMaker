# BFRES (Switch, version 0.9) — subset read by the engine

SMM2 stores every model as a BFRES inside a SARC (`Model/*.zs` and `Pack/<style>_Model.pack`).
The 2D styles use BFRES only as sprite containers: each animation frame is its own texture
(embedded BNTX, named `<anim>.<frame>`) drawn on a quad mesh, and texture-pattern animations
switch textures. Reader: [`core/crates/formats/src/bfres.rs`](../../core/crates/formats/src/bfres.rs).

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
| 0xB0 | off | buffer info: u32, u32 size, u64 offset of the GPU data block (vertex/index buffers) |

## Model `FMDL` (0x78)
+0x08 name · +0x18 skeleton · +0x20 vertex buffers · +0x28 shapes · +0x38 materials ·
+0x68 / +0x6A / +0x6C u16 vertex buffer / shape / material counts.

## Skeleton `FSKL`, bones (0x60 each)
Skeleton: +0x08 bone dict, +0x10 bone array. Bone: +0x00 name, +0x2A i16 parent (−1 root),
+0x34 u32 flags (0x1000: rotation is Euler XYZ, else quaternion), +0x38 f32×3 scale,
+0x44 f32×4 rotation, +0x54 f32×3 translation. Rest transform = T · Rz · Ry · Rx · S, parent first.

## Material `FMAT` (0xA8)
+0x08 name · +0x30 texture name offsets (one per sampler) · +0x40 samplers (0x20 each; bytes
0–2 = wrap u/v/w: 0 repeat, 1 mirrored repeat, 2 clamp) · +0x48 sampler dict (slot names such as
`_a0`) · +0x9C u8 texture count · +0x9D u8 sampler count.

## Shape `FSHP` (0x60)
+0x08 name · +0x52 u16 material · +0x54 u16 bone · +0x56 u16 vertex buffer · +0x5A u8 skin
count (0 = rigid: the shape follows its bone).

## Vertex buffer `FVTX` (0x58)
+0x08 attributes (0x10 each: +0 name, +8 u16 format, +0xC u16 byte offset, +0xE u16 buffer) ·
+0x30 buffer sizes and +0x38 strides (16-byte entries, first u32) · +0x48 u32 offset of the first
buffer in the GPU data block (later buffers follow, 8-byte aligned) · +0x4C / +0x4D u8 attribute /
buffer count · +0x50 u32 vertex count.

Attribute format: high byte = layout (0x02 8, 0x09 8_8, 0x0B 8_8_8_8, 0x0E 10_10_10_2,
0x12 16_16, 0x15 16_16_16_16, 0x17 32_32, 0x18 32_32_32), low byte = 1 unorm, 2 snorm, 3 uint,
4 sint, 5 float. Names: `_p0` position, `_u0` UV, `_n0` normal.

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
