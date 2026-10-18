//! BFRES model files (Switch, version 0.9 as used by SMM2): a read-only subset.
//!
//! Models → skeleton (bone rest transforms, smooth/rigid matrix tables, inverse model matrices),
//! materials (texture names, sampler wrap modes), shapes (vertex attributes incl. skinning, the
//! bone they are bound to, index buffers per LOD); skeletal animations (scale/rotate/translate
//! curves), bone visibility animations and material animations (texture-pattern tracks). Shader
//! parameters, render info, shape keys and user data are not read.
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

/// Inverse of an affine transform (general 3×3 part); identity if singular.
pub fn invert(m: &Affine) -> Affine {
    let a = |r: usize, c: usize| m[r][c];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-12 {
        return IDENTITY;
    }
    let inv_det = 1.0 / det;
    let mut r = [[0.0f32; 4]; 3];
    r[0][0] = (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) * inv_det;
    r[0][1] = (a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2)) * inv_det;
    r[0][2] = (a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1)) * inv_det;
    r[1][0] = (a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2)) * inv_det;
    r[1][1] = (a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0)) * inv_det;
    r[1][2] = (a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2)) * inv_det;
    r[2][0] = (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)) * inv_det;
    r[2][1] = (a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)) * inv_det;
    r[2][2] = (a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)) * inv_det;
    for row in r.iter_mut() {
        row[3] = -(row[0] * m[0][3] + row[1] * m[1][3] + row[2] * m[2][3]);
    }
    r
}

/// Rotation matrix of Euler XYZ angles (radians): R = Rz · Ry · Rx.
pub fn euler_matrix(x: f32, y: f32, z: f32) -> [[f32; 3]; 3] {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

/// Rotation matrix of a quaternion (x, y, z, w); normalised first.
pub fn quat_matrix(q: [f32; 4]) -> [[f32; 3]; 3] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let [x, y, z, w] = if n > 1e-8 {
        q.map(|v| v / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
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
}

/// T · R · S as an affine transform.
pub fn compose(scale: [f32; 3], rotation: [[f32; 3]; 3], translation: [f32; 3]) -> Affine {
    let mut m = [[0.0; 4]; 3];
    for row in 0..3 {
        for col in 0..3 {
            m[row][col] = rotation[row][col] * scale[col];
        }
        m[row][3] = translation[row];
    }
    m
}

#[derive(Clone, Debug, Default)]
pub struct Bfres {
    pub name: String,
    pub models: Vec<Model>,
    pub material_anims: Vec<MaterialAnim>,
    pub skeletal_anims: Vec<SkeletalAnim>,
    pub visibility_anims: Vec<VisibilityAnim>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub name: String,
    pub bones: Vec<Bone>,
    pub skeleton: Skeleton,
    pub materials: Vec<Material>,
    pub shapes: Vec<Shape>,
}

/// Skeleton-wide data besides the bones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    /// Scaling mode (bits 8–9: 1 standard, 2 Maya) and rotation mode (bit 12: Euler).
    pub flags: u32,
    /// Matrix palette → bone: entries `0..smooth_count` are the smooth-skinning matrices, then
    /// `rigid_count` rigid ones. Vertex bone indices (`_i0`) index this list.
    pub matrix_to_bone: Vec<u16>,
    pub smooth_count: usize,
    pub rigid_count: usize,
    /// Inverse rest-pose model matrix of each smooth matrix (bind pose → bone space).
    pub inverse_model: Vec<Affine>,
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
    /// Index into the skeleton's matrix palette for smooth / rigid skinning (−1 = none).
    pub smooth_index: i16,
    pub rigid_index: i16,
}

impl Default for Bone {
    fn default() -> Self {
        Self {
            name: String::new(),
            parent: None,
            flags: BONE_VISIBLE,
            scale: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0; 3],
            smooth_index: -1,
            rigid_index: -1,
        }
    }
}

/// Bone flag: visible in the rest pose.
pub const BONE_VISIBLE: u32 = 0x1;
/// Bone flag: rotation is stored as Euler XYZ angles rather than a quaternion.
pub const BONE_EULER: u32 = 0x1000;
/// Bone flag: segment scale compensation — the parent's scale does not carry over (Maya).
pub const BONE_SEGMENT_SCALE_COMPENSATE: u32 = 0x0080_0000;

impl Bone {
    pub fn euler(&self) -> bool {
        self.flags & BONE_EULER != 0
    }

    pub fn visible(&self) -> bool {
        self.flags & BONE_VISIBLE != 0
    }

    pub fn segment_scale_compensate(&self) -> bool {
        self.flags & BONE_SEGMENT_SCALE_COMPENSATE != 0
    }

    /// Rest transform relative to the parent: T · R · S, with Euler rotation R = Rz · Ry · Rx.
    pub fn local(&self) -> Affine {
        let r = if self.euler() {
            let [x, y, z, _] = self.rotation;
            euler_matrix(x, y, z)
        } else {
            quat_matrix(self.rotation)
        };
        compose(self.scale, r, self.translation)
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
    /// Render info (`gsys_render_state_mode`, `gsys_alpha_test_value`, …).
    pub render_info: Vec<(String, RenderInfo)>,
    /// Shader parameters (uniform values) by name.
    pub params: Vec<ShaderParam>,
    /// Shader archive and shading model the material is drawn with.
    pub shader: (String, String),
    /// Static shader options (`option name → value`).
    pub options: Vec<(String, String)>,
    /// Sampler assignment: shader sampler name → material sampler name.
    pub sampler_assign: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderInfo {
    Int(Vec<i32>),
    Float(Vec<f32>),
    Str(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShaderParam {
    pub name: String,
    /// Parameter type as stored (0–11 bool / int vectors, 12–15 float1–4, 28–30 texture SRTs…).
    pub kind: u8,
    /// The raw value as 32-bit words read as floats (ints and bools keep their bits).
    pub values: Vec<f32>,
}

impl Material {
    pub fn render_str(&self, name: &str) -> Option<&str> {
        self.render_info
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| match v {
                RenderInfo::Str(s) => s.first().map(String::as_str),
                _ => None,
            })
    }
    pub fn render_f32(&self, name: &str) -> Option<f32> {
        self.render_info
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| match v {
                RenderInfo::Float(f) => f.first().copied(),
                RenderInfo::Int(i) => i.first().map(|&x| x as f32),
                _ => None,
            })
    }
    pub fn param(&self, name: &str) -> Option<&[f32]> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.values.as_slice())
    }
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub name: String,
    pub material: usize,
    /// Bone the shape is bound to (applies when `skin_count == 0`; also the bone whose
    /// visibility switches the shape on and off).
    pub bone: usize,
    /// Bone influences per vertex; 0 = rigid, the whole shape follows `bone`; 1 = each vertex
    /// follows one bone and is stored in that bone's space; 2+ = smooth skinning, vertices in
    /// model space (bind pose). `_i0` indexes `Skeleton::matrix_to_bone`, `_w0` are the weights.
    pub skin_count: u8,
    pub vertex_count: usize,
    pub attributes: Vec<Attribute>,
    /// Bones the skinned vertices use (informational; `_i0` indexes the matrix palette).
    pub skin_bones: Vec<u16>,
    /// Index buffers, most detailed first (LOD 0).
    pub lods: Vec<Lod>,
}

/// One level of detail: a triangle list into the shape's vertex buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lod {
    /// Primitive type as stored (3 = triangle list, the only kind seen in SMM2).
    pub primitive: u32,
    /// Vertex indices (the stored first-vertex offset already added).
    pub indices: Vec<u32>,
}

/// `Lod::primitive` of triangle lists.
pub const PRIMITIVE_TRIANGLES: u32 = 3;

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

/// How a curve interpolates between its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    /// Per segment v(t) = c0 + c1·t + c2·t² + c3·t³, t = 0…1 across the segment.
    Cubic,
    /// v(t) = c0 + c1·t.
    Linear,
    /// One value per frame from the start frame.
    BakedFloat,
    /// Value held until the next key (integers, e.g. texture indices).
    StepInt,
    BakedInt,
    /// Booleans (visibility), held until the next key.
    StepBool,
    BakedBool,
    Other(u16),
}

impl CurveKind {
    fn from_raw(v: u16) -> Self {
        match v {
            0 => Self::Cubic,
            1 => Self::Linear,
            2 => Self::BakedFloat,
            4 => Self::StepInt,
            5 => Self::BakedInt,
            6 => Self::StepBool,
            7 => Self::BakedBool,
            v => Self::Other(v),
        }
    }

    /// Stored coefficients per key.
    fn coefficients(self) -> usize {
        match self {
            Self::Cubic => 4,
            Self::Linear => 2,
            _ => 1,
        }
    }
}

/// Behaviour outside a curve's [start, end] frame range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveWrap {
    Clamp,
    Repeat,
    Mirror,
    RelativeRepeat,
}

impl CurveWrap {
    fn from_raw(v: u16) -> Self {
        match v {
            1 => Self::Repeat,
            2 => Self::Mirror,
            3 => Self::RelativeRepeat,
            _ => Self::Clamp,
        }
    }
}

/// An animation curve with its keys decoded: stored values × scale + offset (the offset goes to
/// the constant coefficient only).
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// What the curve drives: a byte offset into the target's animation data (bone animations:
    /// 0x04/0x08/0x0C scale, 0x10/0x14/0x18 translate, 0x20…0x2C rotate x/y/z/w) or an index
    /// (visibility and texture-pattern tracks).
    pub target: u32,
    pub kind: CurveKind,
    pub start: f32,
    pub end: f32,
    pub pre_wrap: CurveWrap,
    pub post_wrap: CurveWrap,
    /// Value change from start to end (for relative repeat).
    pub delta: f32,
    pub frames: Vec<f32>,
    /// Up to 4 coefficients per key (see `CurveKind`); booleans are 0.0 / 1.0.
    pub keys: Vec<[f32; 4]>,
}

impl Curve {
    /// Frame mapped into [start, end] by the wrap modes; also the number of whole periods
    /// skipped (for relative repeat).
    fn wrap_frame(&self, frame: f32) -> (f32, f32) {
        let len = self.end - self.start;
        if len <= 0.0 {
            return (self.start, 0.0);
        }
        let (mode, outside) = if frame < self.start {
            (self.pre_wrap, true)
        } else if frame > self.end {
            (self.post_wrap, true)
        } else {
            (CurveWrap::Clamp, false)
        };
        if !outside {
            return (frame, 0.0);
        }
        let rel = frame - self.start;
        let periods = (rel / len).floor();
        let inner = rel - periods * len;
        match mode {
            CurveWrap::Clamp => (frame.clamp(self.start, self.end), 0.0),
            CurveWrap::Repeat => (self.start + inner, 0.0),
            CurveWrap::RelativeRepeat => (self.start + inner, periods),
            CurveWrap::Mirror => {
                if (periods as i64).rem_euclid(2) == 1 {
                    (self.end - inner, 0.0)
                } else {
                    (self.start + inner, 0.0)
                }
            }
        }
    }

    /// Value at `frame`.
    pub fn eval(&self, frame: f32) -> f32 {
        if self.keys.is_empty() {
            return 0.0;
        }
        let (f, periods) = self.wrap_frame(frame);
        let v = match self.kind {
            CurveKind::BakedFloat | CurveKind::BakedInt | CurveKind::BakedBool => {
                let i = ((f - self.start).max(0.0).round() as usize).min(self.keys.len() - 1);
                self.keys[i][0]
            }
            _ => {
                // last key at or before f
                let i = self
                    .frames
                    .iter()
                    .rposition(|&k| k <= f)
                    .unwrap_or(0)
                    .min(self.keys.len() - 1);
                let c = &self.keys[i];
                match self.kind {
                    CurveKind::Cubic | CurveKind::Linear if i + 1 < self.frames.len() => {
                        let (f0, f1) = (self.frames[i], self.frames[i + 1]);
                        let t = if f1 > f0 {
                            ((f - f0) / (f1 - f0)).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ((c[3] * t + c[2]) * t + c[1]) * t + c[0]
                    }
                    _ if self.frames.first().is_some_and(|&f0| f < f0) => self.keys[0][0],
                    _ => c[0],
                }
            }
        };
        v + periods * self.delta
    }
}

/// A skeletal animation (FSKA): per bone, base values and curves.
#[derive(Clone, Debug, PartialEq)]
pub struct SkeletalAnim {
    pub name: String,
    /// bit 2 looping; bits 8–9 scaling mode; bit 12 Euler rotation (else quaternion).
    pub flags: u32,
    pub frame_count: u32,
    pub bone_anims: Vec<BoneAnim>,
}

/// Skeletal animation flag: rotations are Euler XYZ angles.
pub const ANIM_EULER: u32 = 0x1000;

impl SkeletalAnim {
    pub fn looping(&self) -> bool {
        self.flags & ANIM_LOOPING != 0
    }
    pub fn euler(&self) -> bool {
        self.flags & ANIM_EULER != 0
    }
}

/// The animation of one bone (matched to the skeleton by name).
#[derive(Clone, Debug, PartialEq)]
pub struct BoneAnim {
    pub name: String,
    pub flags: u32,
    pub base_scale: Option<[f32; 3]>,
    pub base_rotate: Option<[f32; 4]>,
    pub base_translate: Option<[f32; 3]>,
    pub curves: Vec<Curve>,
}

/// Bone animation flags: which base values are stored.
pub const BONE_ANIM_BASE_SCALE: u32 = 0x08;
pub const BONE_ANIM_BASE_ROTATE: u32 = 0x10;
pub const BONE_ANIM_BASE_TRANSLATE: u32 = 0x20;

/// A bone visibility animation (FBVS).
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityAnim {
    pub name: String,
    pub flags: u32,
    pub frame_count: u32,
    /// Bone names, with each one's value at frame 0 (when it has no curve).
    pub bones: Vec<(String, bool)>,
    /// Curves; `target` is the index into `bones`.
    pub curves: Vec<Curve>,
}

impl VisibilityAnim {
    pub fn looping(&self) -> bool {
        self.flags & ANIM_LOOPING != 0
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
const RENDER_INFO_SIZE: usize = 0x18;
const SHADER_PARAM_SIZE: usize = 0x20;
const SHAPE_SIZE: usize = 0x60;
const VERTEX_BUFFER_SIZE: usize = 0x58;
const ATTRIBUTE_SIZE: usize = 0x10;
const MATERIAL_ANIM_SIZE: usize = 0x70;
const MATERIAL_ANIM_DATA_SIZE: usize = 0x40;
const PATTERN_INFO_SIZE: usize = 0x10;
const CURVE_SIZE: usize = 0x30;
const MESH_SIZE: usize = 0x38;
const SKELETAL_ANIM_SIZE: usize = 0x50;
const BONE_ANIM_SIZE: usize = 0x38;
const VISIBILITY_ANIM_SIZE: usize = 0x60;

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

    let ska_count = r.dict_names(r.off(0x60)?)?.len();
    let ska_at = r.off(0x58)?;
    let skeletal_anims = (0..ska_count)
        .map(|i| parse_skeletal_anim(&r, ska_at + i * SKELETAL_ANIM_SIZE))
        .collect::<Result<Vec<_>>>()?;

    let vis_count = r.dict_names(r.off(0x80)?)?.len();
    let vis_at = r.off(0x78)?;
    let visibility_anims = (0..vis_count)
        .map(|i| parse_visibility_anim(&r, vis_at + i * VISIBILITY_ANIM_SIZE))
        .collect::<Result<Vec<_>>>()?;

    Ok(Bfres {
        name,
        models,
        material_anims,
        skeletal_anims,
        visibility_anims,
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

    let (bones, skel) = if skeleton != 0 {
        r.magic(skeleton, b"FSKL")?;
        let names = r.dict_names(r.off(skeleton + 0x08)?)?;
        let at = r.off(skeleton + 0x10)?;
        let bones = (0..names.len())
            .map(|i| parse_bone(r, at + i * BONE_SIZE))
            .collect::<Result<Vec<_>>>()?;
        (bones, parse_skeleton(r, skeleton)?)
    } else {
        (Vec::new(), Skeleton::default())
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
        let skin_bone_count = r.u16(s + 0x58)? as usize;
        let skin_bones_at = r.off(s + 0x20)?;
        let skin_bones = if skin_bones_at != 0 {
            (0..skin_bone_count)
                .map(|k| r.u16(skin_bones_at + 2 * k))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mesh_count = r.u8(s + 0x5B)? as usize;
        let meshes_at = r.off(s + 0x18)?;
        let lods = if meshes_at != 0 {
            (0..mesh_count)
                .map(|k| parse_mesh(r, meshes_at + k * MESH_SIZE, buffer_base, vertex_count))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let shape = Shape {
            name: r.name(s + 0x08)?,
            material: r.u16(s + 0x52)? as usize,
            bone: r.u16(s + 0x54)? as usize,
            skin_count: r.u8(s + 0x5A)?,
            vertex_count,
            attributes,
            skin_bones,
            lods,
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
        skeleton: skel,
        materials,
        shapes,
    })
}

fn parse_skeleton(r: &R, sk: usize) -> Result<Skeleton> {
    let flags = r.u32(sk + 0x04)?;
    let mtb_at = r.off(sk + 0x18)?;
    let inv_at = r.off(sk + 0x20)?;
    let smooth_count = r.u16(sk + 0x3A)? as usize;
    let rigid_count = r.u16(sk + 0x3C)? as usize;
    let matrix_to_bone = if mtb_at != 0 {
        (0..smooth_count + rigid_count)
            .map(|k| r.u16(mtb_at + 2 * k))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let inverse_model = if inv_at != 0 {
        (0..smooth_count)
            .map(|k| {
                let v = r.f32x::<12>(inv_at + 48 * k)?;
                Ok([
                    [v[0], v[1], v[2], v[3]],
                    [v[4], v[5], v[6], v[7]],
                    [v[8], v[9], v[10], v[11]],
                ])
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Ok(Skeleton {
        flags,
        matrix_to_bone,
        smooth_count,
        rigid_count,
        inverse_model,
    })
}

/// One LOD's index buffer (`face offset` is relative to the GPU data block).
fn parse_mesh(r: &R, m: usize, buffer_base: usize, vertex_count: usize) -> Result<Lod> {
    let face_offset = r.u32(m + 0x20)? as usize;
    let primitive = r.u32(m + 0x24)?;
    let format = r.u32(m + 0x28)?;
    let count = r.u32(m + 0x2C)? as usize;
    let first = r.u32(m + 0x30)?;
    let at = buffer_base + face_offset;
    let indices = (0..count)
        .map(|k| {
            let v = match format {
                0 => r.u8(at + k)? as u32,
                1 => r.u16(at + 2 * k)? as u32,
                2 => r.u32(at + 4 * k)?,
                f => return Err(bad(format!("index format {f}"))),
            };
            let v = v + first;
            if v as usize >= vertex_count {
                return Err(bad(format!("index {v} ≥ vertex count {vertex_count}")));
            }
            Ok(v)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Lod { primitive, indices })
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
        smooth_index: r.i16(b + 0x2C)?,
        rigid_index: r.i16(b + 0x2E)?,
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
    // render info: values at 0x10, count at 0x9A
    let ri_at = r.off(m + 0x10)?;
    let ri_count = r.u16(m + 0x9A)? as usize;
    let mut render_info = Vec::with_capacity(ri_count);
    for i in 0..ri_count {
        let e = ri_at + i * RENDER_INFO_SIZE;
        let name = r.name(e)?;
        let data = r.off(e + 0x08)?;
        let count = r.u16(e + 0x10)? as usize;
        let value = match r.u8(e + 0x12)? {
            0 => RenderInfo::Int(
                (0..count)
                    .map(|k| r.i32(data + 4 * k))
                    .collect::<Result<_>>()?,
            ),
            1 => RenderInfo::Float(
                (0..count)
                    .map(|k| r.f32(data + 4 * k))
                    .collect::<Result<_>>()?,
            ),
            _ => RenderInfo::Str(
                (0..count)
                    .map(|k| r.name(data + 8 * k))
                    .collect::<Result<_>>()?,
            ),
        };
        render_info.push((name, value));
    }
    // shader parameters: values at 0x50, raw data at 0x60, count at 0x9E
    let sp_at = r.off(m + 0x50)?;
    let src = r.off(m + 0x60)?;
    let sp_count = r.u16(m + 0x9E)? as usize;
    let mut params = Vec::with_capacity(sp_count);
    for i in 0..sp_count {
        let e = sp_at + i * SHADER_PARAM_SIZE;
        let kind = r.u8(e + 0x10)?;
        let size = r.u8(e + 0x11)? as usize;
        let at = src + r.u16(e + 0x12)? as usize;
        let values = (0..size / 4)
            .map(|k| r.f32(at + 4 * k))
            .collect::<Result<Vec<_>>>()?;
        params.push(ShaderParam {
            name: r.name(e + 0x08)?,
            kind,
            values,
        });
    }
    // shader assign: archive, model, attribute / sampler / option assignments
    let (mut shader, mut options, mut sampler_assign) =
        (Default::default(), Vec::new(), Vec::new());
    let sa = r.off(m + 0x20)?;
    if sa != 0 {
        shader = (r.name(sa)?, r.name(sa + 0x08)?);
        let pairs = |values: usize, dict: usize, count: usize| -> Result<Vec<(String, String)>> {
            let names = r.dict_names(dict)?;
            (0..count)
                .map(|k| {
                    Ok((
                        names.get(k).cloned().unwrap_or_default(),
                        r.name(values + 8 * k)?,
                    ))
                })
                .collect()
        };
        sampler_assign = pairs(
            r.off(sa + 0x20)?,
            r.off(sa + 0x28)?,
            r.u8(sa + 0x45)? as usize,
        )?;
        options = pairs(
            r.off(sa + 0x30)?,
            r.off(sa + 0x38)?,
            r.u16(sa + 0x46)? as usize,
        )?;
    }
    Ok(Material {
        name: r.name(m + 0x08)?,
        textures,
        samplers,
        render_info,
        params,
        shader,
        options,
        sampler_assign,
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

/// Any curve, keys decoded to floats (see `Curve`).
fn parse_curve(r: &R, c: usize) -> Result<Curve> {
    let frames_at = r.off(c)?;
    let keys_at = r.off(c + 0x08)?;
    let flags = r.u16(c + 0x10)?;
    let count = r.u16(c + 0x12)? as usize;
    let target = r.u32(c + 0x14)?;
    let start = r.f32(c + 0x18)?;
    let end = r.f32(c + 0x1C)?;
    let scale = r.f32(c + 0x20)?;
    let (frame_type, key_type) = (flags & 3, (flags >> 2) & 3);
    let kind = CurveKind::from_raw((flags >> 4) & 7);
    let pre_wrap = CurveWrap::from_raw((flags >> 8) & 3);
    let post_wrap = CurveWrap::from_raw((flags >> 12) & 3);
    let delta = r.f32(c + 0x28)?;
    let baked = matches!(
        kind,
        CurveKind::BakedFloat | CurveKind::BakedInt | CurveKind::BakedBool
    );
    let frames = if baked || frames_at == 0 {
        Vec::new()
    } else {
        (0..count)
            .map(|i| {
                Ok(match frame_type {
                    0 => r.f32(frames_at + 4 * i)?,
                    1 => r.i16(frames_at + 2 * i)? as f32 / 32.0,
                    _ => r.u8(frames_at + i)? as f32,
                })
            })
            .collect::<Result<Vec<_>>>()?
    };
    let mut keys = Vec::with_capacity(count);
    match kind {
        CurveKind::StepBool | CurveKind::BakedBool => {
            for i in 0..count {
                let word = r.u32(keys_at + 4 * (i / 32))?;
                let bit = (word >> (i % 32)) & 1;
                keys.push([bit as f32, 0.0, 0.0, 0.0]);
            }
        }
        CurveKind::StepInt | CurveKind::BakedInt => {
            let offset = r.i32(c + 0x24)?;
            for i in 0..count {
                let raw = match key_type {
                    0 => r.i32(keys_at + 4 * i)?,
                    1 => r.i16(keys_at + 2 * i)? as i32,
                    _ => r.u8(keys_at + i)? as i8 as i32,
                };
                keys.push([(raw + offset) as f32, 0.0, 0.0, 0.0]);
            }
        }
        _ => {
            let offset = r.f32(c + 0x24)?;
            let n = kind.coefficients();
            // float keys are stored as values; integer keys are quantised (× scale + offset)
            let (scale, offset) = if key_type == 0 && scale == 0.0 {
                (1.0, 0.0)
            } else {
                (scale, offset)
            };
            for i in 0..count {
                let mut k = [0.0f32; 4];
                for (j, v) in k.iter_mut().take(n).enumerate() {
                    let e = i * n + j;
                    let raw = match key_type {
                        0 => r.f32(keys_at + 4 * e)?,
                        1 => r.i16(keys_at + 2 * e)? as f32,
                        _ => r.u8(keys_at + e)? as i8 as f32,
                    };
                    *v = raw * scale + if j == 0 { offset } else { 0.0 };
                }
                keys.push(k);
            }
        }
    }
    Ok(Curve {
        target,
        kind,
        start,
        end,
        pre_wrap,
        post_wrap,
        delta,
        frames,
        keys,
    })
}

fn parse_skeletal_anim(r: &R, a: usize) -> Result<SkeletalAnim> {
    r.magic(a, b"FSKA")?;
    let flags = r.u32(a + 0x04)?;
    let name = r.name(a + 0x08)?;
    let bone_anims_at = r.off(a + 0x28)?;
    let frame_count = r.i32(a + 0x40)?.max(0) as u32;
    let bone_anim_count = r.u16(a + 0x4C)? as usize;
    let bone_anims = (0..bone_anim_count)
        .map(|i| parse_bone_anim(r, bone_anims_at + i * BONE_ANIM_SIZE))
        .collect::<Result<Vec<_>>>()?;
    Ok(SkeletalAnim {
        name,
        flags,
        frame_count,
        bone_anims,
    })
}

fn parse_bone_anim(r: &R, b: usize) -> Result<BoneAnim> {
    let name = r.name(b)?;
    let curves_at = r.off(b + 0x08)?;
    let mut base = r.off(b + 0x10)?;
    let flags = r.u32(b + 0x28)?;
    let curve_count = r.u8(b + 0x2E)? as usize;
    let mut take = |present: bool, n: usize| -> Result<Option<Vec<f32>>> {
        if !present || base == 0 {
            return Ok(None);
        }
        let v = (0..n)
            .map(|k| r.f32(base + 4 * k))
            .collect::<Result<Vec<_>>>()?;
        base += 4 * n;
        Ok(Some(v))
    };
    let base_scale = take(flags & BONE_ANIM_BASE_SCALE != 0, 3)?.map(|v| [v[0], v[1], v[2]]);
    let base_rotate =
        take(flags & BONE_ANIM_BASE_ROTATE != 0, 4)?.map(|v| [v[0], v[1], v[2], v[3]]);
    let base_translate =
        take(flags & BONE_ANIM_BASE_TRANSLATE != 0, 3)?.map(|v| [v[0], v[1], v[2]]);
    let curves = (0..curve_count)
        .map(|i| parse_curve(r, curves_at + i * CURVE_SIZE))
        .collect::<Result<Vec<_>>>()?;
    Ok(BoneAnim {
        name,
        flags,
        base_scale,
        base_rotate,
        base_translate,
        curves,
    })
}

fn parse_visibility_anim(r: &R, a: usize) -> Result<VisibilityAnim> {
    r.magic(a, b"FBVS")?;
    let flags = r.u32(a + 0x04)?;
    let name = r.name(a + 0x08)?;
    let curves_at = r.off(a + 0x28)?;
    let base_at = r.off(a + 0x30)?;
    let names_at = r.off(a + 0x38)?;
    let frame_count = r.i32(a + 0x50)?.max(0) as u32;
    let count = r.u16(a + 0x58)? as usize;
    let curve_count = r.u16(a + 0x5A)? as usize;
    let bones = (0..count)
        .map(|i| {
            let name = if names_at != 0 {
                r.name(names_at + 8 * i)?
            } else {
                String::new()
            };
            let on = base_at != 0 && (r.u8(base_at + i / 8)? >> (i % 8)) & 1 != 0;
            Ok((name, on))
        })
        .collect::<Result<Vec<_>>>()?;
    let curves = (0..curve_count)
        .map(|i| parse_curve(r, curves_at + i * CURVE_SIZE))
        .collect::<Result<Vec<_>>>()?;
    Ok(VisibilityAnim {
        name,
        flags,
        frame_count,
        bones,
        curves,
    })
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

    impl Builder {
        /// Writes `strings` and the GPU block (`gpu`, with `patches` = (u32 field, offset into
        /// gpu)) and returns the file.
        fn finish(mut self, gpu: Vec<u8>, patches: Vec<(usize, usize)>) -> Vec<u8> {
            let strings = std::mem::take(&mut self.strings);
            for (s, refs) in strings {
                self.align(2);
                let at = self.at();
                self.buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
                self.buf.extend_from_slice(s.as_bytes());
                self.buf.push(0);
                for f in refs {
                    self.put_off(f, at);
                }
            }
            self.align(8);
            let info = self.zeros(16);
            self.align(0x100);
            let gpu_at = self.at();
            self.buf.extend_from_slice(&gpu);
            self.put_u32(info + 4, gpu.len() as u32);
            self.put_off(info + 8, gpu_at);
            self.put_off(0xB0, info);
            for (field, rel) in patches {
                self.put_u32(field, rel as u32);
            }
            self.buf
        }

        /// A curve record with u8 frames and i16 keys (`coefs` per key), returns its offset.
        fn curve(
            &mut self,
            kind: u16,
            target: u32,
            frames: &[u8],
            keys: &[i16],
            scale: f32,
            offset: f32,
        ) -> usize {
            self.align(8);
            let c = self.zeros(CURVE_SIZE);
            let fr = self.zeros(frames.len().next_multiple_of(8));
            for (k, f) in frames.iter().enumerate() {
                self.put_u8(fr + k, *f);
            }
            let ks = self.zeros((2 * keys.len()).next_multiple_of(8));
            for (k, v) in keys.iter().enumerate() {
                self.put_u16(ks + 2 * k, *v as u16);
            }
            self.put_off(c, fr);
            self.put_off(c + 8, ks);
            // u8 frames (2), i16 keys (1 << 2), curve kind
            self.put_u16(c + 0x10, 2 | (1 << 2) | (kind << 4));
            self.put_u16(c + 0x12, frames.len() as u16);
            self.put_u32(c + 0x14, target);
            self.put_f32(c + 0x18, frames[0] as f32);
            self.put_f32(c + 0x1C, *frames.last().unwrap() as f32);
            self.put_f32(c + 0x20, scale);
            self.put_f32(c + 0x24, offset);
            c
        }
    }

    /// A skinned two-bone model ("root" at the origin, "tip" 10 up), one smooth-skinned shape
    /// (two triangles, u16 indices), a skeletal animation moving "tip" with a cubic curve and a
    /// linear one, and a visibility animation with a step-bool track.
    pub fn skinned_sample() -> Vec<u8> {
        let mut b = Builder::default();
        b.zeros(0xF0);
        b.buf[..8].copy_from_slice(b"FRES    ");
        b.put_u32(0x08, 0x0009_0000);
        b.put_u16(0x0C, 0xFEFF);
        b.str_ref(0x20, "skinned");
        let mut gpu = Vec::new();
        let mut patches = Vec::new();

        let mo = b.zeros(MODEL_SIZE);
        let mdict = b.dict(&["body"]);
        b.put_off(0x28, mo);
        b.put_off(0x30, mdict);
        b.put_magic(mo, b"FMDL");
        b.str_ref(mo + 0x08, "body");
        // skeleton: root (smooth 0), tip (smooth 1, rigid 2)
        b.align(8);
        let sk = b.zeros(0x48);
        b.put_magic(sk, b"FSKL");
        b.put_u32(sk + 4, 0x1200);
        let bones_at = b.zeros(BONE_SIZE * 2);
        for (i, (name, parent, ty)) in [("root", -1i16, 0.0f32), ("tip", 0, 10.0)]
            .into_iter()
            .enumerate()
        {
            let o = bones_at + i * BONE_SIZE;
            b.str_ref(o, name);
            b.put_u16(o + 0x28, i as u16);
            b.put_u16(o + 0x2A, parent as u16);
            b.put_u16(o + 0x2C, i as u16);
            b.put_u16(o + 0x2E, if i == 1 { 2 } else { 0xFFFF });
            b.put_u32(
                o + 0x34,
                BONE_VISIBLE
                    | BONE_EULER
                    | if i == 1 {
                        BONE_SEGMENT_SCALE_COMPENSATE
                    } else {
                        0
                    },
            );
            for k in 0..3 {
                b.put_f32(o + 0x38 + 4 * k, 1.0);
            }
            b.put_f32(o + 0x50, 1.0);
            b.put_f32(o + 0x58, ty);
        }
        let bdict = b.dict(&["root", "tip"]);
        let mtb = b.zeros(8);
        for (k, v) in [0u16, 1, 1].iter().enumerate() {
            b.put_u16(mtb + 2 * k, *v);
        }
        let inv = b.zeros(96);
        for k in 0..2 {
            for d in 0..3 {
                b.put_f32(inv + 48 * k + 20 * d, 1.0);
            }
        }
        b.put_f32(inv + 48 + 28, -10.0); // tip: inverse translation y
        b.put_off(sk + 0x08, bdict);
        b.put_off(sk + 0x10, bones_at);
        b.put_off(sk + 0x18, mtb);
        b.put_off(sk + 0x20, inv);
        b.put_u16(sk + 0x38, 2);
        b.put_u16(sk + 0x3A, 2);
        b.put_u16(sk + 0x3C, 1);
        b.put_off(mo + 0x18, sk);
        // material
        let mat = b.zeros(MATERIAL_SIZE);
        b.put_magic(mat, b"FMAT");
        b.str_ref(mat + 8, "skin");
        let tn = b.zeros(8);
        b.str_ref(tn, "skin_Alb");
        let sm = b.zeros(SAMPLER_SIZE);
        let sd = b.dict(&["_a0"]);
        b.put_off(mat + 0x30, tn);
        b.put_off(mat + 0x40, sm);
        b.put_off(mat + 0x48, sd);
        b.put_u8(mat + 0x9C, 1);
        b.put_u8(mat + 0x9D, 1);
        b.put_off(mo + 0x38, mat);
        // index buffer first in the GPU block, then the vertex buffers
        let indices: [u16; 6] = [0, 1, 2, 2, 1, 3];
        for i in indices {
            gpu.extend_from_slice(&i.to_le_bytes());
        }
        while gpu.len() % 8 != 0 {
            gpu.push(0);
        }
        // vertex buffer: _p0 f32×3 (buffer 0), _i0 u8×2 + _w0 unorm8×2 (buffer 1, stride 4)
        let vb = b.zeros(VERTEX_BUFFER_SIZE);
        b.put_magic(vb, b"FVTX");
        let attrs = b.zeros(ATTRIBUTE_SIZE * 3);
        b.str_ref(attrs, "_p0");
        b.put_u16(attrs + 8, 0x1805);
        b.str_ref(attrs + 0x10, "_i0");
        b.put_u16(attrs + 0x18, 0x0903);
        b.put_u16(attrs + 0x1E, 1);
        b.str_ref(attrs + 0x20, "_w0");
        b.put_u16(attrs + 0x28, 0x0901);
        b.put_u16(attrs + 0x2C, 2);
        b.put_u16(attrs + 0x2E, 1);
        let sizes = b.zeros(32);
        let strides = b.zeros(32);
        b.put_u32(sizes, 48);
        b.put_u32(sizes + 16, 16);
        b.put_u32(strides, 12);
        b.put_u32(strides + 16, 4);
        b.put_off(vb + 0x08, attrs);
        b.put_off(vb + 0x30, sizes);
        b.put_off(vb + 0x38, strides);
        patches.push((vb + 0x48, gpu.len()));
        b.put_u8(vb + 0x4C, 3);
        b.put_u8(vb + 0x4D, 2);
        b.put_u32(vb + 0x50, 4);
        for p in [
            [-4.0f32, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [-4.0, 10.0, 0.0],
            [4.0, 10.0, 0.0],
        ] {
            for v in p {
                gpu.extend_from_slice(&v.to_le_bytes());
            }
        }
        // bottom vertices follow root, top ones half root / half tip
        for iw in [
            [0u8, 1, 255, 0],
            [0, 1, 255, 0],
            [1, 0, 128, 127],
            [1, 0, 128, 127],
        ] {
            gpu.extend_from_slice(&iw);
        }
        b.put_off(mo + 0x20, vb);
        // shape with one LOD
        let sh = b.zeros(SHAPE_SIZE);
        b.put_magic(sh, b"FSHP");
        b.str_ref(sh + 8, "skin__skin");
        let mesh = b.zeros(MESH_SIZE);
        b.put_u32(mesh + 0x20, 0);
        b.put_u32(mesh + 0x24, PRIMITIVE_TRIANGLES);
        b.put_u32(mesh + 0x28, 1);
        b.put_u32(mesh + 0x2C, 6);
        let skin_bones = b.zeros(8);
        b.put_u16(skin_bones, 0);
        b.put_u16(skin_bones + 2, 1);
        b.put_off(sh + 0x18, mesh);
        b.put_off(sh + 0x20, skin_bones);
        b.put_u16(sh + 0x58, 2);
        b.put_u8(sh + 0x5A, 2);
        b.put_u8(sh + 0x5B, 1);
        b.put_off(mo + 0x28, sh);
        b.put_u16(mo + 0x68, 1);
        b.put_u16(mo + 0x6A, 1);
        b.put_u16(mo + 0x6C, 1);

        // skeletal animation: "tip" translate y = cubic 10 → 20 over 10 frames (i16 keys ×
        // 0.5, offset 10), rotate z linear 0 → 1 rad
        b.align(8);
        let ska = b.zeros(SKELETAL_ANIM_SIZE);
        let skd = b.dict(&["Walk"]);
        b.put_off(0x58, ska);
        b.put_off(0x60, skd);
        b.put_magic(ska, b"FSKA");
        b.put_u32(ska + 4, ANIM_EULER | ANIM_LOOPING);
        b.str_ref(ska + 8, "Walk");
        let ba = b.zeros(BONE_ANIM_SIZE);
        b.str_ref(ba, "tip");
        let base = b.zeros(40);
        for (k, v) in [1.0f32, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 0.0]
            .iter()
            .enumerate()
        {
            b.put_f32(base + 4 * k, *v);
        }
        // cubic: one segment 0..10 with v(t) = 10 + 10·(3t² − 2t³) (smoothstep), then hold
        let c0 = b.curve(0, 0x14, &[0, 10], &[0, 0, 60, -40, 20, 0, 0, 0], 0.5, 10.0);
        let c1 = b.curve(1, 0x28, &[0, 10], &[0, 100, 100, 0], 0.01, 0.0);
        // a bone animation's curve records are contiguous: copy them together
        let curves = b.zeros(2 * CURVE_SIZE);
        let (src0, src1) = (c0, c1);
        let rec0 = b.buf[src0..src0 + CURVE_SIZE].to_vec();
        let rec1 = b.buf[src1..src1 + CURVE_SIZE].to_vec();
        b.buf[curves..curves + CURVE_SIZE].copy_from_slice(&rec0);
        b.buf[curves + CURVE_SIZE..curves + 2 * CURVE_SIZE].copy_from_slice(&rec1);
        b.put_off(ba + 0x08, curves);
        b.put_off(ba + 0x10, base);
        b.put_u32(
            ba + 0x28,
            BONE_ANIM_BASE_SCALE | BONE_ANIM_BASE_ROTATE | BONE_ANIM_BASE_TRANSLATE,
        );
        b.put_u8(ba + 0x2E, 2);
        b.put_off(ska + 0x28, ba);
        b.put_u32(ska + 0x40, 10);
        b.put_u32(ska + 0x44, 2);
        b.put_u16(ska + 0x4C, 1);

        // visibility: root shown, tip hidden at frame 0 and shown from frame 4
        b.align(8);
        let vis = b.zeros(VISIBILITY_ANIM_SIZE);
        let vd = b.dict(&["Walk"]);
        b.put_off(0x78, vis);
        b.put_off(0x80, vd);
        b.put_magic(vis, b"FBVS");
        b.put_u32(vis + 4, ANIM_LOOPING);
        b.str_ref(vis + 8, "Walk");
        let names = b.zeros(16);
        b.str_ref(names, "root");
        b.str_ref(names + 8, "tip");
        let bits = b.zeros(8);
        b.put_u8(bits, 0b01);
        let vc = b.zeros(CURVE_SIZE);
        let vf = b.zeros(8);
        b.put_u8(vf, 0);
        b.put_u8(vf + 1, 4);
        let vk = b.zeros(8);
        b.put_u32(vk, 0b10);
        b.put_off(vc, vf);
        b.put_off(vc + 8, vk);
        b.put_u16(vc + 0x10, 2 | (6 << 4));
        b.put_u16(vc + 0x12, 2);
        b.put_u32(vc + 0x14, 1);
        b.put_f32(vc + 0x1C, 10.0);
        b.put_off(vis + 0x28, vc);
        b.put_off(vis + 0x30, bits);
        b.put_off(vis + 0x38, names);
        b.put_u32(vis + 0x50, 10);
        b.put_u16(vis + 0x58, 2);
        b.put_u16(vis + 0x5A, 1);
        b.finish(gpu, patches)
    }

    #[test]
    fn parses_skinned_shapes_and_skeleton_tables() {
        let f = parse(&skinned_sample()).unwrap();
        let m = &f.models[0];
        assert_eq!(m.skeleton.matrix_to_bone, vec![0, 1, 1]);
        assert_eq!((m.skeleton.smooth_count, m.skeleton.rigid_count), (2, 1));
        assert_eq!(m.bones[1].smooth_index, 1);
        assert_eq!(m.bones[1].rigid_index, 2);
        assert!(m.bones[1].segment_scale_compensate());
        // inverse model matrix of "tip" undoes its rest transform
        let id = mul(&m.bone_world(1), &m.skeleton.inverse_model[1]);
        assert!(close(&id, &IDENTITY, 1e-6));
        assert!(close(
            &invert(&m.bone_world(1)),
            &m.skeleton.inverse_model[1],
            1e-6
        ));
        let s = &m.shapes[0];
        assert_eq!(s.skin_count, 2);
        assert_eq!(s.skin_bones, vec![0, 1]);
        assert_eq!(s.lods.len(), 1);
        assert_eq!(s.lods[0].indices, vec![0, 1, 2, 2, 1, 3]);
        assert_eq!(s.attribute("_p0").unwrap().values[3][..3], [4.0, 10.0, 0.0]);
        assert_eq!(s.attribute("_i0").unwrap().values[2][..2], [1.0, 0.0]);
        let w = s.attribute("_w0").unwrap().values[2];
        assert!((w[0] + w[1] - 1.0).abs() < 1e-6 && (w[0] - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn parses_skeletal_and_visibility_anims() {
        let f = parse(&skinned_sample()).unwrap();
        let a = &f.skeletal_anims[0];
        assert_eq!((a.name.as_str(), a.frame_count), ("Walk", 10));
        assert!(a.looping() && a.euler());
        let tip = &a.bone_anims[0];
        assert_eq!(tip.name, "tip");
        assert_eq!(tip.base_translate, Some([0.0, 10.0, 0.0]));
        assert_eq!(tip.base_rotate, Some([0.0, 0.0, 0.0, 1.0]));
        let ty = &tip.curves[0];
        assert_eq!((ty.target, ty.kind), (0x14, CurveKind::Cubic));
        assert_eq!(ty.frames, vec![0.0, 10.0]);
        // keys × 0.5, offset on the constant term only
        assert_eq!(ty.keys[0], [10.0, 0.0, 30.0, -20.0]);
        assert_eq!(ty.eval(0.0), 10.0);
        assert!((ty.eval(5.0) - 15.0).abs() < 1e-5);
        assert_eq!(ty.eval(10.0), 20.0);
        assert_eq!(ty.eval(12.0), 20.0); // clamped after the end
        let rz = &tip.curves[1];
        assert_eq!(rz.kind, CurveKind::Linear);
        assert!((rz.eval(2.5) - 0.25).abs() < 1e-6);
        let v = &f.visibility_anims[0];
        assert_eq!(
            v.bones,
            vec![("root".to_string(), true), ("tip".to_string(), false)]
        );
        let c = &v.curves[0];
        assert_eq!((c.target, c.kind), (1, CurveKind::StepBool));
        assert_eq!(c.eval(0.0), 0.0);
        assert_eq!(c.eval(3.9), 0.0);
        assert_eq!(c.eval(4.0), 1.0);
    }

    fn curve(kind: CurveKind, frames: &[f32], keys: &[[f32; 4]]) -> Curve {
        Curve {
            target: 0,
            kind,
            start: frames.first().copied().unwrap_or(0.0),
            end: frames.last().copied().unwrap_or(keys.len() as f32 - 1.0),
            pre_wrap: CurveWrap::Clamp,
            post_wrap: CurveWrap::Clamp,
            delta: 0.0,
            frames: frames.to_vec(),
            keys: keys.to_vec(),
        }
    }

    #[test]
    fn curve_kinds_and_wrapping() {
        let step = curve(CurveKind::StepInt, &[0.0, 5.0], &[[1.0; 4], [2.0; 4]]);
        assert_eq!(
            (step.eval(4.9), step.eval(5.0), step.eval(99.0)),
            (1.0, 2.0, 2.0)
        );
        let baked = curve(CurveKind::BakedFloat, &[], &[[0.0; 4], [3.0; 4], [6.0; 4]]);
        assert_eq!(
            (baked.eval(1.0), baked.eval(1.4), baked.eval(9.0)),
            (3.0, 3.0, 6.0)
        );
        let mut lin = curve(
            CurveKind::Linear,
            &[0.0, 10.0],
            &[[0.0, 10.0, 0.0, 0.0], [10.0, 0.0, 0.0, 0.0]],
        );
        lin.post_wrap = CurveWrap::Repeat;
        assert!((lin.eval(13.0) - 3.0).abs() < 1e-5);
        lin.post_wrap = CurveWrap::RelativeRepeat;
        lin.delta = 10.0;
        assert!((lin.eval(13.0) - 13.0).abs() < 1e-5);
        lin.post_wrap = CurveWrap::Mirror;
        assert!((lin.eval(13.0) - 7.0).abs() < 1e-5);
    }

    #[test]
    fn invert_affine() {
        let b = Bone {
            rotation: [0.3, -1.1, 2.0, 0.0],
            translation: [1.0, 2.0, 3.0],
            scale: [2.0, 1.0, 0.5],
            flags: BONE_EULER,
            ..Default::default()
        };
        let m = b.local();
        assert!(close(&mul(&m, &invert(&m)), &IDENTITY, 1e-5));
        // quaternion bones: 90° about Z maps x to y
        let q = Bone {
            rotation: [
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            flags: 0,
            ..Default::default()
        };
        let p = apply(&q.local(), [1.0, 0.0, 0.0]);
        assert!(p[0].abs() < 1e-6 && (p[1] - 1.0).abs() < 1e-6);
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

    /// The owner's RomFS (`MMK_ROMFS`, else the usual extraction folders); None in CI.
    pub fn owner_romfs() -> Option<std::path::PathBuf> {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        std::env::var_os("MMK_ROMFS")
            .map(std::path::PathBuf::from)
            .into_iter()
            .chain([
                "/Volumes/SSD/smm-work/extracted/smm2-update-v303/romfs".into(),
                home.join("smm-work/extracted/smm2-v303-romfs"),
            ])
            .find(|p| p.join("Pack/3W_Model.pack").is_file())
    }

    /// BFRES of `model` from `Pack/<style>_Model.pack` or `Pack/StaticModel.pack`.
    pub fn owner_model(romfs: &std::path::Path, model: &str) -> Vec<u8> {
        use crate::archive::{Sarc, unwrap};
        let style = model.split('_').next().unwrap();
        for pack in [format!("{style}_Model.pack"), "StaticModel.pack".into()] {
            let data = std::fs::read(romfs.join("Pack").join(pack)).unwrap();
            let sarc = Sarc::parse(unwrap(data).unwrap()).unwrap();
            let found = sarc.files().find(|(n, _)| {
                n.rsplit('/').next().and_then(|f| f.split('.').next()) == Some(model)
            });
            if let Some((_, d)) = found {
                let inner = Sarc::parse(unwrap(d.to_vec()).unwrap()).unwrap();
                let (_, f) = inner.files().find(|(_, d)| d.starts_with(b"FRES")).unwrap();
                return f.to_vec();
            }
        }
        panic!("{model} not found");
    }

    fn close(a: &Affine, b: &Affine, eps: f32) -> bool {
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(x, y)| (x - y).abs() < eps)
    }

    /// Real files: skeleton matrices agree with the rest pose, skin indices stay in the matrix
    /// palette, smooth weights sum to 1, cubic curves are continuous across keys and start at
    /// the stored base value, visibility tracks name real bones.
    #[test]
    fn owner_dump_3d_models() {
        let Some(romfs) = owner_romfs() else {
            eprintln!("skipped: no RomFS");
            return;
        };
        for name in [
            "3W_Enemy_kuribo",
            "3W_Enemy_nokonokoA",
            "3W_Player_Mario",
            "WU_Player_MarioMdl",
            "WU_Enemy_koopa",
            "3W_Object_block_renga",
        ] {
            let f = parse(&owner_model(&romfs, name)).unwrap();
            for m in &f.models {
                let sk = &m.skeleton;
                assert_eq!(sk.matrix_to_bone.len(), sk.smooth_count + sk.rigid_count);
                for (k, inv) in sk.inverse_model.iter().enumerate() {
                    let b = sk.matrix_to_bone[k] as usize;
                    assert_eq!(m.bones[b].smooth_index, k as i16, "{name} bone {b}");
                    let id = mul(&m.bone_world(b), inv);
                    assert!(
                        close(&id, &IDENTITY, 2e-3),
                        "{name}/{}: {id:?}",
                        m.bones[b].name
                    );
                }
                for s in &m.shapes {
                    let lod = &s.lods[0];
                    assert_eq!(lod.primitive, PRIMITIVE_TRIANGLES, "{name}/{}", s.name);
                    assert_eq!(lod.indices.len() % 3, 0);
                    if s.skin_count > 0 {
                        let idx = s.attribute("_i0").unwrap();
                        let n = s.skin_count as usize;
                        for v in &idx.values {
                            for &i in &v[..n] {
                                assert!(
                                    (i as usize) < sk.matrix_to_bone.len(),
                                    "{name}/{}",
                                    s.name
                                );
                            }
                        }
                    }
                    if s.skin_count > 1 {
                        let w = s.attribute("_w0").unwrap();
                        for v in &w.values {
                            let sum: f32 = v[..s.skin_count as usize].iter().sum();
                            assert!((sum - 1.0).abs() < 0.02, "{name}/{}: {v:?}", s.name);
                        }
                    }
                }
            }
            for a in &f.skeletal_anims {
                for b in &a.bone_anims {
                    for c in &b.curves {
                        assert!(
                            c.frames.windows(2).all(|w| w[0] < w[1]),
                            "{name}/{}",
                            a.name
                        );
                        if c.kind == CurveKind::Cubic {
                            for i in 0..c.keys.len().saturating_sub(1) {
                                let k = c.keys[i];
                                let end = k[0] + k[1] + k[2] + k[3];
                                let next = c.keys[i + 1][0];
                                assert!(
                                    (end - next).abs() < 1e-2 * (1.0 + next.abs()),
                                    "{name}/{}/{} target {:#x}: {end} vs {next}",
                                    a.name,
                                    b.name,
                                    c.target
                                );
                            }
                        }
                        // the base value is the curve's value at frame 0
                        let base = match c.target {
                            0x04..=0x0C => b.base_scale.map(|v| v[(c.target as usize - 4) / 4]),
                            0x10..=0x18 => {
                                b.base_translate.map(|v| v[(c.target as usize - 0x10) / 4])
                            }
                            0x20..=0x2C => b.base_rotate.map(|v| v[(c.target as usize - 0x20) / 4]),
                            t => panic!("{name}: curve target {t:#x}"),
                        };
                        if let (Some(base), true) = (base, c.start == 0.0) {
                            let mut d = c.eval(0.0) - base;
                            if c.target >= 0x20 && a.euler() {
                                // equal angles modulo a full turn
                                let tau = std::f32::consts::TAU;
                                d -= (d / tau).round() * tau;
                            }
                            let v = base + d;
                            assert!(
                                d.abs() < 1e-2 * (1.0 + base.abs()),
                                "{name}/{}/{} {:#x}: base {base} vs {v}",
                                a.name,
                                b.name,
                                c.target
                            );
                        }
                    }
                }
            }
            if let Some(m) = f.models.first() {
                for v in &f.visibility_anims {
                    // tracks may also name bones of attached models (e.g. wings)
                    assert!(
                        v.bones
                            .iter()
                            .any(|(bone, _)| m.bones.iter().any(|b| &b.name == bone)),
                        "{name}/{}",
                        v.name
                    );
                    for c in &v.curves {
                        assert!((c.target as usize) < v.bones.len());
                    }
                }
            }
        }
        // the player animations live in their own files
        let anims = parse(&owner_model(&romfs, "WU_Player_CommonAnm")).unwrap();
        assert!(anims.skeletal_anims.len() > 200);
        let walk = anims
            .skeletal_anims
            .iter()
            .find(|a| a.name == "walk")
            .unwrap();
        assert!(walk.looping() && walk.frame_count > 0);
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
