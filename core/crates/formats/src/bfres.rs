//! BFRES model files (Switch, version 0.9 as used by SMM2): a read-only subset.
//!
//! Enough for the 2D styles, whose sprites are textured quads: models → skeleton (bone rest
//! transforms), materials (texture names, sampler wrap modes), shapes (vertex attributes and the
//! bone they are bound to) and material animations (texture-pattern tracks). Index buffers,
//! LODs, shader parameters, skeletal animations and user data are not read.
//!
//! All offsets in the file are absolute; strings are a u16 length followed by the bytes.
//! Layout notes: `docs/formats/bfres.md`.

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFRES",
        detail: detail.into(),
    }
}

/// Row-major 3×4 affine transform (rotation/scale in columns 0–2, translation in column 3).
pub type Affine = [[f32; 4]; 3];

pub const IDENTITY: Affine = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// `a · b` (apply `b` first).
pub fn mul(a: &Affine, b: &Affine) -> Affine {
    let mut out = [[0.0; 4]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
            if c == 3 {
                *cell += a[r][3];
            }
        }
    }
    out
}

pub fn apply(m: &Affine, p: [f32; 3]) -> [f32; 3] {
    let f = |r: usize| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3];
    [f(0), f(1), f(2)]
}

#[derive(Clone, Debug, Default)]
pub struct Bfres {
    pub name: String,
    pub models: Vec<Model>,
    pub material_anims: Vec<MaterialAnim>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub name: String,
    pub bones: Vec<Bone>,
    pub materials: Vec<Material>,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub flags: u32,
    pub scale: [f32; 3],
    /// Euler angles in radians (x, y, z, unused) when `euler()`, else a quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

/// Bone flag: rotation is stored as Euler XYZ angles rather than a quaternion.
pub const BONE_EULER: u32 = 0x1000;

impl Bone {
    pub fn euler(&self) -> bool {
        self.flags & BONE_EULER != 0
    }

    /// Rest transform relative to the parent: T · R · S, with Euler rotation R = Rz · Ry · Rx.
    pub fn local(&self) -> Affine {
        let r = if self.euler() {
            let [x, y, z, _] = self.rotation;
            let (sx, cx) = x.sin_cos();
            let (sy, cy) = y.sin_cos();
            let (sz, cz) = z.sin_cos();
            [
                [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
                [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
                [-sy, sx * cy, cx * cy],
            ]
        } else {
            let [x, y, z, w] = self.rotation;
            [
                [
                    1.0 - 2.0 * (y * y + z * z),
                    2.0 * (x * y - z * w),
                    2.0 * (x * z + y * w),
                ],
                [
                    2.0 * (x * y + z * w),
                    1.0 - 2.0 * (x * x + z * z),
                    2.0 * (y * z - x * w),
                ],
                [
                    2.0 * (x * z - y * w),
                    2.0 * (y * z + x * w),
                    1.0 - 2.0 * (x * x + y * y),
                ],
            ]
        };
        let mut m = [[0.0; 4]; 3];
        for row in 0..3 {
            for col in 0..3 {
                m[row][col] = r[row][col] * self.scale[col];
            }
            m[row][3] = self.translation[row];
        }
        m
    }
}

/// Sampler wrap mode as stored (0 repeat, 1 mirrored repeat, 2 clamp; others kept raw).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Repeat,
    Mirror,
    Clamp,
    Other(u8),
}

impl Wrap {
    fn from_raw(v: u8) -> Self {
        match v {
            0 => Self::Repeat,
            1 => Self::Mirror,
            2 => Self::Clamp,
            v => Self::Other(v),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sampler {
    /// Sampler slot name, e.g. `_a0` (albedo).
    pub name: String,
    /// Wrap mode for u, v, w.
    pub wrap: [Wrap; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    pub name: String,
    /// Texture bound to each sampler slot (same order as `samplers`).
    pub textures: Vec<String>,
    pub samplers: Vec<Sampler>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// `_p0` position, `_u0` first UV set, `_n0` normal, ...
    pub name: String,
    /// Format as stored: high byte = component layout, low byte = 1 unorm, 2 snorm, 3 uint,
    /// 4 sint, 5 float.
    pub format: u16,
    /// One value per vertex, missing components 0. Empty if the format is not supported.
    pub values: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub name: String,
    pub material: usize,
    /// Bone the shape is bound to (applies when `skin_count == 0`).
    pub bone: usize,
    /// Bone influences per vertex; 0 = rigid, the whole shape follows `bone`.
    pub skin_count: u8,
    pub vertex_count: usize,
    pub attributes: Vec<Attribute>,
}

impl Shape {
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }
}

impl Model {
    /// Rest-pose model-space transform of bone `i`.
    pub fn bone_world(&self, i: usize) -> Affine {
        let mut m = IDENTITY;
        let mut cur = Some(i);
        // walk up at most bones.len() parents (guards against cycles in corrupt files)
        for _ in 0..=self.bones.len() {
            let Some(b) = cur.and_then(|c| self.bones.get(c)) else {
                break;
            };
            m = mul(&b.local(), &m);
            cur = b.parent;
        }
        m
    }

    /// Model-space vertex positions of `shape` in the rest pose.
    pub fn shape_positions(&self, shape: &Shape) -> Vec<[f32; 3]> {
        let Some(p) = shape.attribute("_p0") else {
            return Vec::new();
        };
        let m = if shape.skin_count == 0 {
            self.bone_world(shape.bone)
        } else {
            IDENTITY
        };
        p.values
            .iter()
            .map(|v| apply(&m, [v[0], v[1], v[2]]))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialAnim {
    pub name: String,
    pub frame_count: u32,
    pub looping: bool,
    /// Textures the pattern tracks index into.
    pub textures: Vec<String>,
    pub targets: Vec<MaterialAnimTarget>,
}

/// Animation of one material (matched by name).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialAnimTarget {
    pub material: String,
    pub patterns: Vec<TexturePattern>,
    /// Number of shader-parameter tracks (colour, texture SRT, ...); not decoded.
    pub param_tracks: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TexturePattern {
    /// Sampler slot the pattern drives, e.g. `_a0`.
    pub sampler: String,
    pub track: PatternTrack,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternTrack {
    /// The same texture index for the whole animation.
    Constant(i32),
    /// Step curve: (frame, texture index) keys sorted by frame; the value holds until the next key.
    Steps(Vec<(f32, i32)>),
    /// A curve type this reader does not evaluate (raw curve-type code).
    Unsupported(u16),
}

impl TexturePattern {
    /// Texture index (into `MaterialAnim::textures`) at `frame`.
    pub fn index_at(&self, frame: f32) -> Option<i32> {
        match &self.track {
            PatternTrack::Constant(v) => Some(*v),
            PatternTrack::Steps(keys) => keys
                .iter()
                .take_while(|(f, _)| *f <= frame)
                .last()
                .or(keys.first())
                .map(|(_, v)| *v),
            PatternTrack::Unsupported(_) => None,
        }
    }
}

impl MaterialAnim {
    /// First texture-pattern track of the animation, with the material it targets.
    pub fn first_pattern(&self) -> Option<(&str, &TexturePattern)> {
        self.targets
            .iter()
            .find_map(|t| t.patterns.first().map(|p| (t.material.as_str(), p)))
    }
}

// ---------------------------------------------------------------------------------------------

struct R<'a>(&'a [u8]);

impl<'a> R<'a> {
    fn bytes(&self, o: usize, n: usize) -> Result<&'a [u8]> {
        o.checked_add(n)
            .and_then(|end| self.0.get(o..end))
            .ok_or_else(|| bad(format!("read of {n} bytes at {o:#x} outside the file")))
    }
    fn u8(&self, o: usize) -> Result<u8> {
        Ok(self.bytes(o, 1)?[0])
    }
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(o, 2)?.try_into().unwrap()))
    }
    fn i16(&self, o: usize) -> Result<i16> {
        Ok(self.u16(o)? as i16)
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(o, 4)?.try_into().unwrap()))
    }
    fn i32(&self, o: usize) -> Result<i32> {
        Ok(self.u32(o)? as i32)
    }
    fn f32(&self, o: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(o)?))
    }
    fn f32x<const N: usize>(&self, o: usize) -> Result<[f32; N]> {
        let mut v = [0.0; N];
        for (i, x) in v.iter_mut().enumerate() {
            *x = self.f32(o + 4 * i)?;
        }
        Ok(v)
    }
    /// 64-bit file offset field.
    fn off(&self, o: usize) -> Result<usize> {
        let v = u64::from_le_bytes(self.bytes(o, 8)?.try_into().unwrap());
        usize::try_from(v).map_err(|_| bad("offset too large"))
    }
    fn string(&self, o: usize) -> Result<String> {
        if o == 0 {
            return Ok(String::new());
        }
        let len = self.u16(o)? as usize;
        Ok(String::from_utf8_lossy(self.bytes(o + 2, len)?).into_owned())
    }
    /// String through an offset field.
    fn name(&self, field: usize) -> Result<String> {
        self.string(self.off(field)?)
    }
    /// Names of a resource dictionary (entry 0 is the root and has no name).
    fn dict_names(&self, o: usize) -> Result<Vec<String>> {
        if o == 0 {
            return Ok(Vec::new());
        }
        let count = self.u32(o + 4)? as usize;
        (1..=count).map(|i| self.name(o + 8 + 16 * i + 8)).collect()
    }
    fn magic(&self, o: usize, want: &[u8; 4]) -> Result<()> {
        if self.bytes(o, 4)? != want {
            return Err(bad(format!(
                "expected {} at {o:#x}",
                String::from_utf8_lossy(want)
            )));
        }
        Ok(())
    }
}

// Structure sizes (version 0.9).
const MODEL_SIZE: usize = 0x78;
const BONE_SIZE: usize = 0x60;
const MATERIAL_SIZE: usize = 0xA8;
const SAMPLER_SIZE: usize = 0x20;
const SHAPE_SIZE: usize = 0x60;
const VERTEX_BUFFER_SIZE: usize = 0x58;
const ATTRIBUTE_SIZE: usize = 0x10;
const MATERIAL_ANIM_SIZE: usize = 0x70;
const MATERIAL_ANIM_DATA_SIZE: usize = 0x40;
const PATTERN_INFO_SIZE: usize = 0x10;
const CURVE_SIZE: usize = 0x30;

/// Animation flag: the animation loops.
pub const ANIM_LOOPING: u32 = 1 << 2;

pub fn parse(data: &[u8]) -> Result<Bfres> {
    let r = R(data);
    if data.get(..8) != Some(b"FRES    ") {
        return Err(bad("missing FRES magic"));
    }
    let version = r.u32(0x08)?;
    if version >> 16 != 0x0009 {
        return Err(bad(format!(
            "unsupported version {version:#010x} (want 0.9)"
        )));
    }
    if r.u16(0x0C)? != 0xFEFF {
        return Err(bad("not little-endian"));
    }
    let name = r.name(0x20)?;

    // GPU buffer data (vertex/index buffers), referenced by vertex buffers relative to its start.
    let buffer_info = r.off(0xB0)?;
    let buffer_base = if buffer_info != 0 {
        r.off(buffer_info + 8)?
    } else {
        0
    };

    let model_count = r.dict_names(r.off(0x30)?)?.len();
    let models_at = r.off(0x28)?;
    let models = (0..model_count)
        .map(|i| parse_model(&r, models_at + i * MODEL_SIZE, buffer_base))
        .collect::<Result<Vec<_>>>()?;

    let anim_count = r.dict_names(r.off(0x70)?)?.len();
    let anims_at = r.off(0x68)?;
    let material_anims = (0..anim_count)
        .map(|i| parse_material_anim(&r, anims_at + i * MATERIAL_ANIM_SIZE))
        .collect::<Result<Vec<_>>>()?;

    Ok(Bfres {
        name,
        models,
        material_anims,
    })
}

fn parse_model(r: &R, m: usize, buffer_base: usize) -> Result<Model> {
    r.magic(m, b"FMDL")?;
    let name = r.name(m + 0x08)?;
    let skeleton = r.off(m + 0x18)?;
    let vertex_buffers = r.off(m + 0x20)?;
    let shapes_at = r.off(m + 0x28)?;
    let materials_at = r.off(m + 0x38)?;
    let vb_count = r.u16(m + 0x68)? as usize;
    let shape_count = r.u16(m + 0x6A)? as usize;
    let material_count = r.u16(m + 0x6C)? as usize;

    let bones = if skeleton != 0 {
        r.magic(skeleton, b"FSKL")?;
        let names = r.dict_names(r.off(skeleton + 0x08)?)?;
        let at = r.off(skeleton + 0x10)?;
        (0..names.len())
            .map(|i| parse_bone(r, at + i * BONE_SIZE))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let materials = (0..material_count)
        .map(|i| parse_material(r, materials_at + i * MATERIAL_SIZE))
        .collect::<Result<Vec<_>>>()?;

    let buffers = (0..vb_count)
        .map(|i| parse_vertex_buffer(r, vertex_buffers + i * VERTEX_BUFFER_SIZE, buffer_base))
        .collect::<Result<Vec<_>>>()?;

    let mut shapes = Vec::with_capacity(shape_count);
    for i in 0..shape_count {
        let s = shapes_at + i * SHAPE_SIZE;
        r.magic(s, b"FSHP")?;
        let vb = r.u16(s + 0x56)? as usize;
        let (vertex_count, attributes) = buffers
            .get(vb)
            .cloned()
            .ok_or_else(|| bad(format!("shape {i}: vertex buffer {vb} out of range")))?;
        let shape = Shape {
            name: r.name(s + 0x08)?,
            material: r.u16(s + 0x52)? as usize,
            bone: r.u16(s + 0x54)? as usize,
            skin_count: r.u8(s + 0x5A)?,
            vertex_count,
            attributes,
        };
        if shape.material >= materials.len() || (shape.bone >= bones.len() && !bones.is_empty()) {
            return Err(bad(format!(
                "shape {}: bad material/bone index",
                shape.name
            )));
        }
        shapes.push(shape);
    }

    Ok(Model {
        name,
        bones,
        materials,
        shapes,
    })
}

fn parse_bone(r: &R, b: usize) -> Result<Bone> {
    let parent = r.i16(b + 0x2A)?;
    Ok(Bone {
        name: r.name(b)?,
        parent: (parent >= 0).then_some(parent as usize),
        flags: r.u32(b + 0x34)?,
        scale: r.f32x::<3>(b + 0x38)?,
        rotation: r.f32x::<4>(b + 0x44)?,
        translation: r.f32x::<3>(b + 0x54)?,
    })
}

fn parse_material(r: &R, m: usize) -> Result<Material> {
    r.magic(m, b"FMAT")?;
    let tex_names = r.off(m + 0x30)?;
    let samplers_at = r.off(m + 0x40)?;
    let sampler_names = r.dict_names(r.off(m + 0x48)?)?;
    let tex_count = r.u8(m + 0x9C)? as usize;
    let sampler_count = r.u8(m + 0x9D)? as usize;
    let textures = (0..tex_count)
        .map(|i| r.name(tex_names + 8 * i))
        .collect::<Result<Vec<_>>>()?;
    let samplers = (0..sampler_count)
        .map(|i| {
            let s = samplers_at + i * SAMPLER_SIZE;
            Ok(Sampler {
                name: sampler_names.get(i).cloned().unwrap_or_default(),
                wrap: [
                    Wrap::from_raw(r.u8(s)?),
                    Wrap::from_raw(r.u8(s + 1)?),
                    Wrap::from_raw(r.u8(s + 2)?),
                ],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Material {
        name: r.name(m + 0x08)?,
        textures,
        samplers,
    })
}

fn parse_vertex_buffer(r: &R, v: usize, buffer_base: usize) -> Result<(usize, Vec<Attribute>)> {
    r.magic(v, b"FVTX")?;
    let attrs_at = r.off(v + 0x08)?;
    let sizes_at = r.off(v + 0x30)?;
    let strides_at = r.off(v + 0x38)?;
    let data_offset = r.u32(v + 0x48)? as usize;
    let attr_count = r.u8(v + 0x4C)? as usize;
    let buffer_count = r.u8(v + 0x4D)? as usize;
    let vertex_count = r.u32(v + 0x50)? as usize;

    // buffers follow each other in the GPU data block, each 8-byte aligned
    let mut buffers = Vec::with_capacity(buffer_count);
    let mut at = buffer_base + data_offset;
    for i in 0..buffer_count {
        let size = r.u32(sizes_at + 16 * i)? as usize;
        let stride = r.u32(strides_at + 16 * i)? as usize;
        buffers.push((at, stride));
        at += size.next_multiple_of(8);
    }

    let mut attributes = Vec::with_capacity(attr_count);
    for i in 0..attr_count {
        let a = attrs_at + i * ATTRIBUTE_SIZE;
        let name = r.name(a)?;
        let format = r.u16(a + 0x08)?;
        let offset = r.u16(a + 0x0C)? as usize;
        let buffer = r.u16(a + 0x0E)? as usize;
        let &(start, stride) = buffers
            .get(buffer)
            .ok_or_else(|| bad(format!("attribute {name}: buffer {buffer} out of range")))?;
        let values = if attribute_layout(format).is_some() {
            (0..vertex_count)
                .map(|k| decode_attribute(r, start + k * stride + offset, format))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        attributes.push(Attribute {
            name,
            format,
            values,
        });
    }
    Ok((vertex_count, attributes))
}

/// (component count, bytes per component) of a vertex attribute layout; `(3, 0)` marks the
/// packed 10_10_10_2 layout.
fn attribute_layout(format: u16) -> Option<(usize, usize)> {
    Some(match format >> 8 {
        0x02 => (1, 1),
        0x09 => (2, 1),
        0x0B => (4, 1),
        0x0E => (3, 0),
        0x12 => (2, 2),
        0x15 => (4, 2),
        0x17 => (2, 4),
        0x18 => (3, 4),
        _ => return None,
    })
}

fn decode_attribute(r: &R, o: usize, format: u16) -> Result<[f32; 4]> {
    let (n, size) = attribute_layout(format).expect("checked by caller");
    let kind = format & 0xFF;
    let mut out = [0.0f32; 4];
    if size == 0 {
        // 10_10_10_2 signed normalised
        let v = r.u32(o)?;
        for (i, x) in out.iter_mut().take(3).enumerate() {
            let raw = ((v >> (10 * i)) & 0x3FF) as i32;
            let signed = if raw >= 512 { raw - 1024 } else { raw };
            *x = (signed as f32 / 511.0).max(-1.0);
        }
        out[3] = (v >> 30) as f32;
        return Ok(out);
    }
    for (i, x) in out.iter_mut().take(n).enumerate() {
        let p = o + i * size;
        *x = match (size, kind) {
            (1, 1) => r.u8(p)? as f32 / 255.0,
            (1, 2) => (r.u8(p)? as i8 as f32 / 127.0).max(-1.0),
            (1, 4) => r.u8(p)? as i8 as f32,
            (1, _) => r.u8(p)? as f32,
            (2, 1) => r.u16(p)? as f32 / 65535.0,
            (2, 2) => (r.i16(p)? as f32 / 32767.0).max(-1.0),
            (2, 4) => r.i16(p)? as f32,
            (2, 5) => half_to_f32(r.u16(p)?),
            (2, _) => r.u16(p)? as f32,
            (_, 5) => r.f32(p)?,
            (_, 4) => r.i32(p)? as f32,
            _ => r.u32(p)? as f32,
        };
    }
    Ok(out)
}

/// IEEE 754 binary16 → f32.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let frac = (h & 0x3FF) as f32;
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 if frac == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

fn parse_material_anim(r: &R, a: usize) -> Result<MaterialAnim> {
    r.magic(a, b"FMAA")?;
    let flags = r.u32(a + 0x04)?;
    let name = r.name(a + 0x08)?;
    let targets_at = r.off(a + 0x28)?;
    let tex_names = r.off(a + 0x38)?;
    let frame_count = r.i32(a + 0x58)?.max(0) as u32;
    let target_count = r.u16(a + 0x62)? as usize;
    let tex_count = r.u16(a + 0x6C)? as usize;
    let textures = (0..tex_count)
        .map(|i| r.name(tex_names + 8 * i))
        .collect::<Result<Vec<_>>>()?;
    let targets = (0..target_count)
        .map(|i| parse_anim_target(r, targets_at + i * MATERIAL_ANIM_DATA_SIZE))
        .collect::<Result<Vec<_>>>()?;
    Ok(MaterialAnim {
        name,
        frame_count,
        looping: flags & ANIM_LOOPING != 0,
        textures,
        targets,
    })
}

fn parse_anim_target(r: &R, d: usize) -> Result<MaterialAnimTarget> {
    let material = r.name(d)?;
    let patterns_at = r.off(d + 0x10)?;
    let curves_at = r.off(d + 0x18)?;
    let constants_at = r.off(d + 0x20)?;
    let param_count = r.u16(d + 0x32)? as usize;
    let pattern_count = r.u16(d + 0x34)? as usize;
    let constant_count = r.u16(d + 0x36)? as usize;
    let curve_count = r.u16(d + 0x38)? as usize;
    let mut patterns = Vec::with_capacity(pattern_count);
    for i in 0..pattern_count {
        let p = patterns_at + i * PATTERN_INFO_SIZE;
        let sampler = r.name(p)?;
        let curve = r.u16(p + 0x08)?;
        let constant = r.i16(p + 0x0A)?;
        let track = if curve != 0xFFFF {
            let curve = curve as usize;
            if curve >= curve_count {
                return Err(bad(format!("{material}: curve {curve} out of range")));
            }
            parse_step_curve(r, curves_at + curve * CURVE_SIZE)?
        } else if constant >= 0 && (constant as usize) < constant_count {
            PatternTrack::Constant(r.i32(constants_at + 8 * constant as usize + 4)?)
        } else {
            return Err(bad(format!(
                "{material}: pattern {i} has no curve or constant"
            )));
        };
        patterns.push(TexturePattern { sampler, track });
    }
    Ok(MaterialAnimTarget {
        material,
        patterns,
        param_tracks: param_count,
    })
}

/// Integer step curve (texture-pattern frames). Other curve types are reported as unsupported.
fn parse_step_curve(r: &R, c: usize) -> Result<PatternTrack> {
    let frames_at = r.off(c)?;
    let keys_at = r.off(c + 0x08)?;
    let flags = r.u16(c + 0x10)?;
    let count = r.u16(c + 0x12)? as usize;
    let offset = r.i32(c + 0x24)?;
    let (frame_type, key_type, curve_type) = (flags & 3, (flags >> 2) & 3, (flags >> 4) & 7);
    const STEP_INT: u16 = 4;
    if curve_type != STEP_INT {
        return Ok(PatternTrack::Unsupported(curve_type));
    }
    let mut keys = Vec::with_capacity(count);
    for i in 0..count {
        let frame = match frame_type {
            0 => r.f32(frames_at + 4 * i)?,
            1 => r.i16(frames_at + 2 * i)? as f32 / 32.0,
            _ => r.u8(frames_at + i)? as f32,
        };
        let raw = match key_type {
            0 => r.i32(keys_at + 4 * i)?,
            1 => r.i16(keys_at + 2 * i)? as i32,
            _ => r.u8(keys_at + i)? as i8 as i32,
        };
        keys.push((frame, offset + raw));
    }
    Ok(PatternTrack::Steps(keys))
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a minimal version-0.9 BFRES image for tests (synthetic; no game data).
    #[derive(Default)]
    pub struct Builder {
        buf: Vec<u8>,
        /// (string, list of positions to patch with the string's offset)
        strings: Vec<(String, Vec<usize>)>,
    }

    pub struct TestBone {
        pub name: &'static str,
        pub parent: i16,
        pub rotation: [f32; 3],
        pub translation: [f32; 3],
    }

    pub struct TestShape {
        pub name: &'static str,
        pub bone: u16,
        pub material: u16,
        /// half-float positions (x, y)
        pub positions: Vec<[f32; 2]>,
        /// unorm8 UVs
        pub uvs: Vec<[u8; 2]>,
    }

    pub struct TestMaterial {
        pub name: &'static str,
        pub texture: &'static str,
        pub wrap: [u8; 3],
    }

    pub struct TestModel {
        pub name: &'static str,
        pub bones: Vec<TestBone>,
        pub materials: Vec<TestMaterial>,
        pub shapes: Vec<TestShape>,
    }

    pub enum TestTrack {
        Constant(i32),
        /// (frame u8, key i8) with an offset
        Steps(Vec<(u8, i8)>, i32),
    }

    pub struct TestAnim {
        pub name: &'static str,
        pub frames: i32,
        pub looping: bool,
        pub material: &'static str,
        pub textures: Vec<&'static str>,
        pub track: TestTrack,
    }

    fn f32_to_half(v: f32) -> u16 {
        // exact for the small integers used in tests
        if v == 0.0 {
            return 0;
        }
        let sign = if v < 0.0 { 0x8000 } else { 0 };
        let a = v.abs();
        let e = a.log2().floor() as i32;
        let frac = ((a / 2f32.powi(e) - 1.0) * 1024.0).round() as u16;
        sign | (((e + 15) as u16) << 10) | frac
    }

    impl Builder {
        fn at(&self) -> usize {
            self.buf.len()
        }
        fn align(&mut self, n: usize) {
            while !self.buf.len().is_multiple_of(n) {
                self.buf.push(0);
            }
        }
        fn zeros(&mut self, n: usize) -> usize {
            let at = self.at();
            self.buf.resize(at + n, 0);
            at
        }
        fn put_u8(&mut self, o: usize, v: u8) {
            self.buf[o] = v;
        }
        fn put_u16(&mut self, o: usize, v: u16) {
            self.buf[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        fn put_u32(&mut self, o: usize, v: u32) {
            self.buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn put_f32(&mut self, o: usize, v: f32) {
            self.put_u32(o, v.to_bits());
        }
        fn put_off(&mut self, o: usize, v: usize) {
            self.buf[o..o + 8].copy_from_slice(&(v as u64).to_le_bytes());
        }
        fn put_magic(&mut self, o: usize, m: &[u8; 4]) {
            self.buf[o..o + 4].copy_from_slice(m);
        }
        fn str_ref(&mut self, field: usize, s: &str) {
            match self.strings.iter_mut().find(|(x, _)| x == s) {
                Some((_, refs)) => refs.push(field),
                None => self.strings.push((s.to_string(), vec![field])),
            }
        }
        /// Dictionary with `names` (root entry first).
        fn dict(&mut self, names: &[&str]) -> usize {
            self.align(8);
            let d = self.zeros(8 + 16 * (names.len() + 1));
            self.put_u32(d + 4, names.len() as u32);
            for (i, n) in names.iter().enumerate() {
                self.str_ref(d + 8 + 16 * (i + 1) + 8, n);
            }
            d
        }

        pub fn build(models: &[TestModel], anims: &[TestAnim]) -> Vec<u8> {
            let mut b = Builder::default();
            b.zeros(0xF0);
            b.buf[..8].copy_from_slice(b"FRES    ");
            b.put_u32(0x08, 0x0009_0000);
            b.put_u16(0x0C, 0xFEFF);
            b.str_ref(0x20, "test");

            // GPU data block collected separately, placed at the end
            let mut gpu: Vec<u8> = Vec::new();

            let models_at = b.zeros(MODEL_SIZE * models.len());
            let mdict = b.dict(&models.iter().map(|m| m.name).collect::<Vec<_>>());
            b.put_off(0x28, models_at);
            b.put_off(0x30, mdict);
            let mut vertex_patches = Vec::new(); // (field, gpu offset)
            for (mi, m) in models.iter().enumerate() {
                let mo = models_at + mi * MODEL_SIZE;
                b.put_magic(mo, b"FMDL");
                b.str_ref(mo + 0x08, m.name);
                // skeleton
                b.align(8);
                let sk = b.zeros(0x40);
                b.put_magic(sk, b"FSKL");
                let bones_at = b.zeros(BONE_SIZE * m.bones.len());
                for (i, bone) in m.bones.iter().enumerate() {
                    let o = bones_at + i * BONE_SIZE;
                    b.str_ref(o, bone.name);
                    b.put_u16(o + 0x28, i as u16);
                    b.put_u16(o + 0x2A, bone.parent as u16);
                    b.put_u32(o + 0x34, BONE_EULER | 0x0100_0000);
                    for k in 0..3 {
                        b.put_f32(o + 0x38 + 4 * k, 1.0);
                        b.put_f32(o + 0x44 + 4 * k, bone.rotation[k]);
                        b.put_f32(o + 0x54 + 4 * k, bone.translation[k]);
                    }
                    b.put_f32(o + 0x50, 1.0);
                }
                let bdict = b.dict(&m.bones.iter().map(|x| x.name).collect::<Vec<_>>());
                b.put_off(sk + 0x08, bdict);
                b.put_off(sk + 0x10, bones_at);
                b.put_off(mo + 0x18, sk);
                // materials
                let mats_at = b.zeros(MATERIAL_SIZE * m.materials.len());
                for (i, mat) in m.materials.iter().enumerate() {
                    let o = mats_at + i * MATERIAL_SIZE;
                    b.put_magic(o, b"FMAT");
                    b.str_ref(o + 0x08, mat.name);
                    let tn = b.zeros(8);
                    b.str_ref(tn, mat.texture);
                    let sm = b.zeros(SAMPLER_SIZE);
                    for k in 0..3 {
                        b.put_u8(sm + k, mat.wrap[k]);
                    }
                    let sd = b.dict(&["_a0"]);
                    b.put_off(o + 0x30, tn);
                    b.put_off(o + 0x40, sm);
                    b.put_off(o + 0x48, sd);
                    b.put_u8(o + 0x9C, 1);
                    b.put_u8(o + 0x9D, 1);
                }
                b.put_off(mo + 0x38, mats_at);
                // one vertex buffer per shape: _p0 (half x4), _u0 (unorm8 x2)
                let vbs_at = b.zeros(VERTEX_BUFFER_SIZE * m.shapes.len());
                for (i, s) in m.shapes.iter().enumerate() {
                    let o = vbs_at + i * VERTEX_BUFFER_SIZE;
                    b.put_magic(o, b"FVTX");
                    let attrs = b.zeros(ATTRIBUTE_SIZE * 2);
                    b.str_ref(attrs, "_p0");
                    b.put_u16(attrs + 8, 0x1505);
                    b.str_ref(attrs + 0x10, "_u0");
                    b.put_u16(attrs + 0x18, 0x0901);
                    b.put_u16(attrs + 0x1E, 1);
                    let sizes = b.zeros(32);
                    let strides = b.zeros(32);
                    let n = s.positions.len();
                    b.put_u32(sizes, (8 * n) as u32);
                    b.put_u32(sizes + 16, (2 * n) as u32);
                    b.put_u32(strides, 8);
                    b.put_u32(strides + 16, 2);
                    b.put_off(o + 0x08, attrs);
                    b.put_off(o + 0x30, sizes);
                    b.put_off(o + 0x38, strides);
                    vertex_patches.push((o + 0x48, gpu.len()));
                    b.put_u8(o + 0x4C, 2);
                    b.put_u8(o + 0x4D, 2);
                    b.put_u32(o + 0x50, n as u32);
                    for p in &s.positions {
                        for v in [p[0], p[1], 0.0, 1.0] {
                            gpu.extend_from_slice(&f32_to_half(v).to_le_bytes());
                        }
                    }
                    for uv in &s.uvs {
                        gpu.extend_from_slice(uv);
                    }
                    while !gpu.len().is_multiple_of(8) {
                        gpu.push(0);
                    }
                }
                b.put_off(mo + 0x20, vbs_at);
                let shapes_at = b.zeros(SHAPE_SIZE * m.shapes.len());
                for (i, s) in m.shapes.iter().enumerate() {
                    let o = shapes_at + i * SHAPE_SIZE;
                    b.put_magic(o, b"FSHP");
                    b.str_ref(o + 0x08, s.name);
                    b.put_u16(o + 0x50, i as u16);
                    b.put_u16(o + 0x52, s.material);
                    b.put_u16(o + 0x54, s.bone);
                    b.put_u16(o + 0x56, i as u16);
                    b.put_u8(o + 0x5B, 1);
                }
                b.put_off(mo + 0x28, shapes_at);
                b.put_u16(mo + 0x68, m.shapes.len() as u16);
                b.put_u16(mo + 0x6A, m.shapes.len() as u16);
                b.put_u16(mo + 0x6C, m.materials.len() as u16);
            }

            // material animations
            b.align(8);
            let anims_at = b.zeros(MATERIAL_ANIM_SIZE * anims.len());
            let adict = b.dict(&anims.iter().map(|a| a.name).collect::<Vec<_>>());
            b.put_off(0x68, anims_at);
            b.put_off(0x70, adict);
            for (i, a) in anims.iter().enumerate() {
                let o = anims_at + i * MATERIAL_ANIM_SIZE;
                b.put_magic(o, b"FMAA");
                b.put_u32(o + 4, if a.looping { ANIM_LOOPING } else { 0 });
                b.str_ref(o + 0x08, a.name);
                let tn = b.zeros(8 * a.textures.len());
                for (k, t) in a.textures.iter().enumerate() {
                    b.str_ref(tn + 8 * k, t);
                }
                let d = b.zeros(MATERIAL_ANIM_DATA_SIZE);
                b.str_ref(d, a.material);
                let pi = b.zeros(PATTERN_INFO_SIZE);
                b.str_ref(pi, "_a0");
                b.put_off(d + 0x10, pi);
                b.put_u16(d + 0x34, 1);
                match &a.track {
                    TestTrack::Constant(v) => {
                        let c = b.zeros(8);
                        b.put_u32(c + 4, *v as u32);
                        b.put_off(d + 0x20, c);
                        b.put_u16(pi + 8, 0xFFFF);
                        b.put_u16(pi + 10, 0);
                        b.put_u16(d + 0x36, 1);
                    }
                    TestTrack::Steps(keys, offset) => {
                        b.align(8);
                        let c = b.zeros(CURVE_SIZE);
                        let fr = b.zeros(keys.len().next_multiple_of(8));
                        let ks = b.zeros(keys.len().next_multiple_of(8));
                        for (k, (f, v)) in keys.iter().enumerate() {
                            b.put_u8(fr + k, *f);
                            b.put_u8(ks + k, *v as u8);
                        }
                        b.put_off(c, fr);
                        b.put_off(c + 8, ks);
                        b.put_u16(c + 0x10, 0x4A); // u8 frames, i8 keys, step-int
                        b.put_u16(c + 0x12, keys.len() as u16);
                        b.put_u32(c + 0x24, *offset as u32);
                        b.put_off(d + 0x18, c);
                        b.put_u16(pi + 8, 0);
                        b.put_u16(pi + 10, 0xFFFF);
                        b.put_u16(d + 0x38, 1);
                    }
                }
                b.put_off(o + 0x28, d);
                b.put_off(o + 0x38, tn);
                b.put_u32(o + 0x58, a.frames as u32);
                b.put_u16(o + 0x62, 1);
                b.put_u16(o + 0x6C, a.textures.len() as u16);
            }

            // strings
            let strings = std::mem::take(&mut b.strings);
            for (s, refs) in strings {
                b.align(2);
                let at = b.at();
                b.buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
                b.buf.extend_from_slice(s.as_bytes());
                b.buf.push(0);
                for f in refs {
                    b.put_off(f, at);
                }
            }
            // GPU block + buffer info
            b.align(8);
            let info = b.zeros(16);
            b.align(0x100);
            let gpu_at = b.at();
            b.buf.extend_from_slice(&gpu);
            b.put_u32(info + 4, gpu.len() as u32);
            b.put_off(info + 8, gpu_at);
            b.put_off(0xB0, info);
            for (field, rel) in vertex_patches {
                b.put_u32(field, rel as u32);
            }
            b.buf
        }
    }

    pub fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> (Vec<[f32; 2]>, Vec<[u8; 2]>) {
        (
            vec![[x0, y0], [x1, y0], [x0, y1], [x1, y1]],
            vec![[0, 255], [255, 255], [0, 0], [255, 0]],
        )
    }

    /// A Goomba-like model: 16×32 quad centred on the origin, a second bone offset upwards,
    /// a walk animation alternating two textures every 10 frames, and a constant one.
    pub fn sample() -> Vec<u8> {
        let (positions, uvs) = quad(-8.0, -16.0, 8.0, 16.0);
        let (p2, u2) = quad(-8.0, -8.0, 8.0, 8.0);
        Builder::build(
            &[TestModel {
                name: "walker_model",
                bones: vec![
                    TestBone {
                        name: "body",
                        parent: -1,
                        rotation: [0.0; 3],
                        translation: [0.0; 3],
                    },
                    TestBone {
                        name: "flag",
                        parent: 0,
                        rotation: [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                        translation: [0.0, 88.0, 0.0],
                    },
                ],
                materials: vec![
                    TestMaterial {
                        name: "M_body",
                        texture: "walk.0",
                        wrap: [2, 2, 2],
                    },
                    TestMaterial {
                        name: "M_flag",
                        texture: "flag",
                        wrap: [1, 1, 2],
                    },
                ],
                shapes: vec![
                    TestShape {
                        name: "body__M_body",
                        bone: 0,
                        material: 0,
                        positions,
                        uvs,
                    },
                    TestShape {
                        name: "flag__M_flag",
                        bone: 1,
                        material: 1,
                        positions: p2,
                        uvs: u2,
                    },
                ],
            }],
            &[
                TestAnim {
                    name: "walk_ftp",
                    frames: 20,
                    looping: true,
                    material: "M_body",
                    textures: vec!["walk.0", "walk.1"],
                    track: TestTrack::Steps(vec![(0, -1), (10, 0)], 1),
                },
                TestAnim {
                    name: "damage_ftp",
                    frames: 1,
                    looping: false,
                    material: "M_body",
                    textures: vec!["damage.0"],
                    track: TestTrack::Constant(0),
                },
            ],
        )
    }

    #[test]
    fn parses_models_shapes_and_materials() {
        let f = parse(&sample()).unwrap();
        assert_eq!(f.name, "test");
        assert_eq!(f.models.len(), 1);
        let m = &f.models[0];
        assert_eq!(m.name, "walker_model");
        assert_eq!(m.bones.len(), 2);
        assert_eq!(m.bones[1].parent, Some(0));
        assert!(m.bones[1].euler());
        assert_eq!(m.materials[0].textures, vec!["walk.0".to_string()]);
        assert_eq!(m.materials[0].samplers[0].name, "_a0");
        assert_eq!(m.materials[1].samplers[0].wrap[0], Wrap::Mirror);
        assert_eq!(m.materials[0].samplers[0].wrap[1], Wrap::Clamp);
        let s = &m.shapes[0];
        assert_eq!(
            (s.name.as_str(), s.material, s.bone),
            ("body__M_body", 0, 0)
        );
        assert_eq!(s.vertex_count, 4);
        let pos = m.shape_positions(s);
        assert_eq!(pos[0], [-8.0, -16.0, 0.0]);
        assert_eq!(pos[3], [8.0, 16.0, 0.0]);
        let uv = s.attribute("_u0").unwrap();
        assert_eq!(uv.values[0][..2], [0.0, 1.0]);
        assert_eq!(uv.values[3][..2], [1.0, 0.0]);
    }

    #[test]
    fn applies_bone_rest_transform() {
        let f = parse(&sample()).unwrap();
        let m = &f.models[0];
        let pos = m.shape_positions(&m.shapes[1]);
        // rotation of 90° about X maps (x, y, 0) to (x, 0, y); then +88 on y
        for (p, want) in pos
            .iter()
            .zip([[-8.0, 88.0, -8.0], [8.0, 88.0, 8.0]].iter().cycle())
        {
            assert!((p[0] - want[0]).abs() < 1e-4, "{p:?}");
            assert!((p[1] - want[1]).abs() < 1e-4, "{p:?}");
        }
        assert!((pos[0][2] + 8.0).abs() < 1e-4);
        assert!((pos[2][2] - 8.0).abs() < 1e-4);
    }

    #[test]
    fn parses_texture_pattern_anims() {
        let f = parse(&sample()).unwrap();
        assert_eq!(f.material_anims.len(), 2);
        let walk = &f.material_anims[0];
        assert_eq!(walk.name, "walk_ftp");
        assert_eq!(walk.frame_count, 20);
        assert!(walk.looping);
        assert_eq!(walk.textures, vec!["walk.0", "walk.1"]);
        let (mat, p) = walk.first_pattern().unwrap();
        assert_eq!(mat, "M_body");
        assert_eq!(p.sampler, "_a0");
        // keys are stored relative to the curve offset
        assert_eq!(p.track, PatternTrack::Steps(vec![(0.0, 0), (10.0, 1)]));
        assert_eq!(p.index_at(0.0), Some(0));
        assert_eq!(p.index_at(9.0), Some(0));
        assert_eq!(p.index_at(10.0), Some(1));
        assert_eq!(p.index_at(19.0), Some(1));
        let damage = &f.material_anims[1];
        assert!(!damage.looping);
        assert_eq!(
            damage.first_pattern().unwrap().1.track,
            PatternTrack::Constant(0)
        );
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"FRES").is_err());
        let mut v = sample();
        v[0x0A] = 0x0A; // version 0.10
        assert!(parse(&v).is_err());
        let mut v = sample();
        v.truncate(0x200);
        assert!(parse(&v).is_err());
    }

    #[test]
    fn half_floats() {
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0xC800), -8.0);
        assert_eq!(half_to_f32(0x4C00), 16.0);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(f32_to_half(-16.0), 0xCC00);
    }
}
