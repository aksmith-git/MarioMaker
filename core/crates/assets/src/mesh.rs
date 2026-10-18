//! 3D meshes and their animations (NSMBU / 3D World styles), written by the importer's mesh stage.
//!
//! ```text
//! meshes/<Model>.mmesh          every model of one game model file: skeleton, materials, shapes,
//!                               bind-pose vertices (LOD 0 only), triangle indices
//! anims/<Model>/index.json      { anim: { frames, loop, skeletal, visibility, textures } }
//! anims/<Model>/<anim>.mskel    one animation: bone curves, bone visibility, texture patterns
//! ```
//! Both files are a small container: magic (4 bytes), u32 version, u32 JSON length, u32 blob
//! length, the JSON header (padded to 8 bytes), then the binary blob. The JSON describes the
//! structure; vertex, index and curve arrays live in the blob as `[byte offset, count]` pairs.
//! Little-endian. Vertices are stored in the bind pose in model space (1 unit = 1 world pixel for
//! the game's models), so the skinning matrix of a bone is `world(bone) · inverse_bind(bone)`.

use std::collections::HashMap;

use mmk_formats::bfres::{
    self, Affine, Curve, CurveKind, CurveWrap, IDENTITY, compose, euler_matrix, mul, quat_matrix,
};
use serde_json::{Value, json};

pub const MESH_MAGIC: &[u8; 4] = b"MMSH";
pub const ANIM_MAGIC: &[u8; 4] = b"MSKL";
pub const MESH_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("not a {0} file")]
    Magic(&'static str),
    #[error("unsupported version {0}")]
    Version(u32),
    #[error("truncated or inconsistent file: {0}")]
    Corrupt(String),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

/// One vertex as the renderer consumes it (56 bytes, `MmkMeshVertex` in mmk.h).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Bone indices into `MeshModel::bones`.
    pub joints: [u16; 4],
    /// Weights of `joints` (sum 1).
    pub weights: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshBone {
    pub name: String,
    pub parent: Option<usize>,
    pub scale: [f32; 3],
    /// Euler XYZ radians (w unused) when `euler`, else a quaternion.
    pub rotation: [f32; 4],
    pub euler: bool,
    pub translation: [f32; 3],
    /// Segment scale compensation: the parent's scale does not carry over.
    pub ssc: bool,
    pub visible: bool,
    /// Inverse of the bone's rest-pose model matrix.
    pub inverse_bind: Affine,
}

impl MeshBone {
    /// Local transform from scale / rotation / translation (rotation as `euler` says).
    fn local(&self, scale: [f32; 3], rotation: [f32; 4], euler: bool, t: [f32; 3]) -> Affine {
        let r = if euler {
            euler_matrix(rotation[0], rotation[1], rotation[2])
        } else {
            quat_matrix(rotation)
        };
        compose(scale, r, t)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshMaterial {
    pub name: String,
    /// Albedo texture (sampler `_a0`, else the first texture), in the same model file's
    /// texture folder.
    pub albedo: Option<String>,
    /// Sampler wrap u, v: 0 repeat, 1 mirror, 2 clamp.
    pub wrap: [u8; 2],
    /// Flat cards (pre-rendered sprites on a quad): drawn without lighting.
    pub unlit: bool,
    /// The other textures by the shader sampler they feed (`_n0` normal, `_s0` specular /
    /// sky, `_e0` emission, `_b0` bake, `_a1` second albedo, …), from the material's sampler
    /// assignment.
    pub textures: Vec<(String, String)>,
    /// Render mode: 0 opaque, 1 alpha-tested ("mask"), 2 translucent, 3 additive.
    pub blend: u8,
    /// Alpha-test reference (with `blend` 1).
    pub alpha_ref: f32,
    /// Faces drawn: 0 front only (back faces culled), 1 back only, 2 both.
    pub cull: u8,
    /// Selected shader parameters (`const_color0`, `tex_srt0`, `albedo_ratio`, …).
    pub params: Vec<(String, Vec<f32>)>,
    /// Shader options that differ from the defaults (`enable_fog`, `enable_sky`, …).
    pub options: Vec<(String, String)>,
}

impl MeshMaterial {
    pub fn param(&self, name: &str) -> Option<&[f32]> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    /// Texture fed to shader sampler `sampler` (`_a0` is the albedo).
    pub fn texture(&self, sampler: &str) -> Option<&str> {
        if sampler == "_a0" {
            return self.albedo.as_deref();
        }
        self.textures
            .iter()
            .find(|(n, _)| n == sampler)
            .map(|(_, t)| t.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshShape {
    pub name: String,
    pub material: usize,
    /// Bone whose visibility shows or hides the shape.
    pub bone: usize,
    /// Range in `MeshModel::indices`.
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshModel {
    pub name: String,
    pub bones: Vec<MeshBone>,
    pub materials: Vec<MeshMaterial>,
    pub shapes: Vec<MeshShape>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Rest-pose bounds: min x, y, z, max x, y, z.
    pub bounds: [f32; 6],
    /// Vertex colours (RGBA8, one per vertex) if the model has them, else empty.
    pub colors: Vec<[u8; 4]>,
    /// Second texture-coordinate set (bake maps) if the model has one, else empty.
    pub uv1: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshFile {
    pub models: Vec<MeshModel>,
}

/// Bone curves of one bone. `base` holds scale (0–2), rotation (3–6), translation (7–9) at
/// frame 0; components without a base value fall back to the bone's rest value.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneTrack {
    pub bone: String,
    pub base: [f32; 10],
    /// bit 0 scale, bit 1 rotation, bit 2 translation present in `base`.
    pub mask: u8,
    /// Curves; `Curve::target` is the SRT component index 0–9.
    pub curves: Vec<Curve>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisTrack {
    pub bone: String,
    pub base: bool,
    pub curve: Option<Curve>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TexTrack {
    pub material: String,
    /// (frame, texture) sorted by frame; the texture holds until the next key.
    pub keys: Vec<(f32, String)>,
}

/// One animation: any combination of skeletal, visibility and texture-pattern tracks that share
/// a name in the game file (`walk`, `walk_ftp`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkelAnim {
    pub name: String,
    pub frames: u32,
    pub looping: bool,
    /// Rotations of `bones` tracks are Euler angles (else quaternions).
    pub euler: bool,
    pub bones: Vec<BoneTrack>,
    pub visibility: Vec<VisTrack>,
    pub textures: Vec<TexTrack>,
}

impl SkelAnim {
    /// Animation-local frame for a time `t` frames after the animation started.
    pub fn frame_at(&self, t: f32) -> f32 {
        let n = self.frames as f32;
        if n <= 0.0 {
            0.0
        } else if self.looping {
            t.rem_euclid(n)
        } else {
            t.clamp(0.0, n)
        }
    }
}

/// SRT component index (0–9) of a bone-animation curve target (byte offset into the game's
/// bone animation data).
pub fn srt_component(target: u32) -> Option<u8> {
    Some(match target {
        0x04 => 0,
        0x08 => 1,
        0x0C => 2,
        0x20 => 3,
        0x24 => 4,
        0x28 => 5,
        0x2C => 6,
        0x10 => 7,
        0x14 => 8,
        0x18 => 9,
        _ => return None,
    })
}

// --- posing ------------------------------------------------------------------------------------

/// Per model × animation: which track drives which bone.
#[derive(Clone, Debug, Default)]
pub struct Binding {
    bone_track: Vec<Option<usize>>,
    vis_track: Vec<Option<usize>>,
    tex_track: Vec<Option<usize>>,
}

impl Binding {
    pub fn new(model: &MeshModel, anim: &SkelAnim) -> Self {
        let bones: HashMap<&str, usize> = anim
            .bones
            .iter()
            .enumerate()
            .map(|(i, t)| (t.bone.as_str(), i))
            .collect();
        let vis: HashMap<&str, usize> = anim
            .visibility
            .iter()
            .enumerate()
            .map(|(i, t)| (t.bone.as_str(), i))
            .collect();
        let tex: HashMap<&str, usize> = anim
            .textures
            .iter()
            .enumerate()
            .map(|(i, t)| (t.material.as_str(), i))
            .collect();
        Self {
            bone_track: model
                .bones
                .iter()
                .map(|b| bones.get(b.name.as_str()).copied())
                .collect(),
            vis_track: model
                .bones
                .iter()
                .map(|b| vis.get(b.name.as_str()).copied())
                .collect(),
            tex_track: model
                .materials
                .iter()
                .map(|m| tex.get(m.name.as_str()).copied())
                .collect(),
        }
    }

    /// Bones the animation drives (0 = it does not fit this skeleton).
    pub fn bound_bones(&self) -> usize {
        self.bone_track.iter().filter(|t| t.is_some()).count()
    }
}

/// A posed model: skinning matrices, bone visibility, texture per material.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    /// Model-space bone transforms.
    pub world: Vec<Affine>,
    /// `world · inverse_bind` per bone (multiply bind-pose vertices).
    pub skin: Vec<Affine>,
    pub visible: Vec<bool>,
    /// Texture name per material (animated or the albedo).
    pub textures: Vec<Option<String>>,
}

impl MeshModel {
    /// Pose at `frame` of `anim` (already animation-local, see `SkelAnim::frame_at`); the rest
    /// pose without an animation.
    pub fn pose(&self, anim: Option<(&SkelAnim, &Binding)>, frame: f32, out: &mut Pose) {
        let n = self.bones.len();
        let mut locals: Vec<Affine> = Vec::with_capacity(n);
        let mut scales: Vec<[f32; 3]> = Vec::with_capacity(n);
        for (i, b) in self.bones.iter().enumerate() {
            let track = anim.and_then(|(a, bind)| Some((a, &a.bones[bind.bone_track[i]?])));
            let (s, r, t, euler) = match track {
                Some((a, tr)) => {
                    let mut v = [0.0f32; 10];
                    v[..3].copy_from_slice(&b.scale);
                    v[7..].copy_from_slice(&b.translation);
                    // rotation: the track's base, or the rest rotation if the modes agree
                    if tr.mask & 2 != 0 || b.euler == a.euler {
                        v[3..7].copy_from_slice(&b.rotation);
                    } else {
                        v[3..7].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
                    }
                    for (bit, range) in [(1u8, 0..3), (2, 3..7), (4, 7..10)] {
                        if tr.mask & bit != 0 {
                            v[range.clone()].copy_from_slice(&tr.base[range]);
                        }
                    }
                    for c in &tr.curves {
                        if let Some(slot) = v.get_mut(c.target as usize) {
                            *slot = c.eval(frame);
                        }
                    }
                    (
                        [v[0], v[1], v[2]],
                        [v[3], v[4], v[5], v[6]],
                        [v[7], v[8], v[9]],
                        a.euler,
                    )
                }
                None => (b.scale, b.rotation, b.translation, b.euler),
            };
            let mut m = b.local(s, r, euler, t);
            if b.ssc
                && let Some(p) = b.parent.and_then(|p| scales.get(p))
            {
                // T · S_parent⁻¹ · R · S
                for (row, ps) in m.iter_mut().zip(p) {
                    if *ps != 0.0 {
                        for cell in row.iter_mut().take(3) {
                            *cell /= ps;
                        }
                    }
                }
            }
            locals.push(m);
            scales.push(s);
        }
        out.world.clear();
        out.world.resize(n, IDENTITY);
        let mut done = vec![false; n];
        for i in 0..n {
            self.resolve_world(i, &locals, &mut out.world, &mut done, 0);
        }
        out.skin.clear();
        out.skin.extend(
            self.bones
                .iter()
                .zip(&out.world)
                .map(|(b, w)| mul(w, &b.inverse_bind)),
        );
        out.visible.clear();
        out.visible
            .extend(self.bones.iter().enumerate().map(|(i, b)| {
                match anim.and_then(|(a, bind)| Some(&a.visibility[bind.vis_track[i]?])) {
                    Some(v) => match &v.curve {
                        Some(c) => c.eval(frame) >= 0.5,
                        None => v.base,
                    },
                    None => b.visible,
                }
            }));
        out.textures.clear();
        out.textures
            .extend(self.materials.iter().enumerate().map(|(i, m)| {
                match anim.and_then(|(a, bind)| Some(&a.textures[bind.tex_track[i]?])) {
                    Some(t) => t
                        .keys
                        .iter()
                        .take_while(|(f, _)| *f <= frame)
                        .last()
                        .or(t.keys.first())
                        .map(|(_, s)| s.clone()),
                    None => m.albedo.clone(),
                }
            }));
    }

    fn resolve_world(
        &self,
        i: usize,
        locals: &[Affine],
        world: &mut [Affine],
        done: &mut [bool],
        depth: usize,
    ) {
        if done[i] {
            return;
        }
        let m = match self.bones[i].parent {
            Some(p) if p < self.bones.len() && p != i && depth < self.bones.len() => {
                self.resolve_world(p, locals, world, done, depth + 1);
                mul(&world[p], &locals[i])
            }
            _ => locals[i],
        };
        world[i] = m;
        done[i] = true;
    }

    /// Rest-pose model matrices of all bones (with segment scale compensation).
    pub fn rest_world(&self) -> Vec<Affine> {
        let mut p = Pose::default();
        self.pose(None, 0.0, &mut p);
        p.world
    }
}

// --- container ---------------------------------------------------------------------------------

#[derive(Default)]
struct Blob(Vec<u8>);

impl Blob {
    fn align(&mut self) {
        while !self.0.len().is_multiple_of(4) {
            self.0.push(0);
        }
    }
    fn f32s(&mut self, v: &[f32]) -> Value {
        self.align();
        let at = self.0.len();
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
        json!([at, v.len()])
    }
    fn u32s(&mut self, v: &[u32]) -> Value {
        self.align();
        let at = self.0.len();
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
        json!([at, v.len()])
    }
    fn vertices(&mut self, v: &[Vertex]) -> Value {
        self.align();
        let at = self.0.len();
        for x in v {
            for f in x.position.iter().chain(&x.normal).chain(&x.uv) {
                self.0.extend_from_slice(&f.to_le_bytes());
            }
            for j in x.joints {
                self.0.extend_from_slice(&j.to_le_bytes());
            }
            for w in x.weights {
                self.0.extend_from_slice(&w.to_le_bytes());
            }
        }
        json!([at, v.len()])
    }
}

fn container(magic: &[u8; 4], header: &Value, blob: &Blob) -> Vec<u8> {
    let mut j = serde_json::to_vec(header).expect("serialise header");
    while !j.len().is_multiple_of(8) {
        j.push(b' ');
    }
    let mut out = Vec::with_capacity(16 + j.len() + blob.0.len());
    out.extend_from_slice(magic);
    out.extend_from_slice(&MESH_VERSION.to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(&(blob.0.len() as u32).to_le_bytes());
    out.extend_from_slice(&j);
    out.extend_from_slice(&blob.0);
    out
}

struct Reader<'a> {
    blob: &'a [u8],
}

fn corrupt(s: impl Into<String>) -> MeshError {
    MeshError::Corrupt(s.into())
}

impl<'a> Reader<'a> {
    fn open(
        data: &'a [u8],
        magic: &[u8; 4],
        what: &'static str,
    ) -> Result<(Value, Self), MeshError> {
        if data.get(..4) != Some(magic) {
            return Err(MeshError::Magic(what));
        }
        let u = |o: usize| -> Result<usize, MeshError> {
            Ok(u32::from_le_bytes(
                data.get(o..o + 4)
                    .ok_or_else(|| corrupt("header"))?
                    .try_into()
                    .unwrap(),
            ) as usize)
        };
        let version = u(4)? as u32;
        if version != MESH_VERSION {
            return Err(MeshError::Version(version));
        }
        let (jl, bl) = (u(8)?, u(12)?);
        let json = data.get(16..16 + jl).ok_or_else(|| corrupt("json"))?;
        let blob = data
            .get(16 + jl..16 + jl + bl)
            .ok_or_else(|| corrupt("blob"))?;
        Ok((serde_json::from_slice(json)?, Self { blob }))
    }
    fn range(&self, v: &Value, elem: usize) -> Result<&'a [u8], MeshError> {
        let at = v
            .get(0)
            .and_then(Value::as_u64)
            .ok_or_else(|| corrupt("range"))? as usize;
        let n = v
            .get(1)
            .and_then(Value::as_u64)
            .ok_or_else(|| corrupt("range"))? as usize;
        self.blob
            .get(at..at + n * elem)
            .ok_or_else(|| corrupt("range outside the blob"))
    }
    fn f32s(&self, v: &Value) -> Result<Vec<f32>, MeshError> {
        Ok(self
            .range(v, 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect())
    }
    fn u32s(&self, v: &Value) -> Result<Vec<u32>, MeshError> {
        Ok(self
            .range(v, 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect())
    }
    fn vertices(&self, v: &Value) -> Result<Vec<Vertex>, MeshError> {
        Ok(self
            .range(v, 56)?
            .as_chunks::<56>()
            .0
            .iter()
            .map(|c| {
                let f = |i: usize| f32::from_le_bytes(c[4 * i..4 * i + 4].try_into().unwrap());
                let h =
                    |i: usize| u16::from_le_bytes(c[32 + 2 * i..34 + 2 * i].try_into().unwrap());
                Vertex {
                    position: [f(0), f(1), f(2)],
                    normal: [f(3), f(4), f(5)],
                    uv: [f(6), f(7)],
                    joints: [h(0), h(1), h(2), h(3)],
                    weights: [f(10), f(11), f(12), f(13)],
                }
            })
            .collect())
    }
}

fn fa<const N: usize>(v: &Value) -> [f32; N] {
    let mut out = [0.0; N];
    if let Some(a) = v.as_array() {
        for (o, x) in out.iter_mut().zip(a) {
            *o = x.as_f64().unwrap_or(0.0) as f32;
        }
    }
    out
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn affine_to_vec(m: &Affine) -> Vec<f32> {
    m.iter().flatten().copied().collect()
}

impl MeshFile {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut blob = Blob::default();
        let models: Vec<Value> = self
            .models
            .iter()
            .map(|m| {
                json!({
                    "name": m.name,
                    "bones": m.bones.iter().map(|b| json!({
                        "name": b.name, "parent": b.parent, "s": b.scale, "r": b.rotation,
                        "euler": b.euler, "t": b.translation, "ssc": b.ssc, "visible": b.visible,
                        "inv": affine_to_vec(&b.inverse_bind),
                    })).collect::<Vec<_>>(),
                    "materials": m.materials.iter().map(|x| {
                        let mut j = json!({
                            "name": x.name, "albedo": x.albedo, "wrap": x.wrap, "unlit": x.unlit,
                        });
                        if !x.textures.is_empty() {
                            j["textures"] = json!(x.textures);
                        }
                        if x.blend != 0 {
                            j["blend"] = json!(x.blend);
                            j["alpha_ref"] = json!(x.alpha_ref);
                        }
                        if x.cull != 0 {
                            j["cull"] = json!(x.cull);
                        }
                        if !x.params.is_empty() {
                            j["params"] = json!(x.params);
                        }
                        if !x.options.is_empty() {
                            j["options"] = json!(x.options);
                        }
                        j
                    }).collect::<Vec<_>>(),
                    "shapes": m.shapes.iter().map(|x| json!({
                        "name": x.name, "material": x.material, "bone": x.bone,
                        "first": x.first_index, "count": x.index_count,
                    })).collect::<Vec<_>>(),
                    "bounds": m.bounds,
                    "vertices": blob.vertices(&m.vertices),
                    "indices": blob.u32s(&m.indices),
                    "colors": (!m.colors.is_empty()).then(|| blob.u32s(
                        &m.colors.iter().map(|c| u32::from_le_bytes(*c)).collect::<Vec<_>>())),
                    "uv1": (!m.uv1.is_empty()).then(|| blob.f32s(
                        &m.uv1.iter().flatten().copied().collect::<Vec<_>>())),
                })
            })
            .collect();
        container(MESH_MAGIC, &json!({ "models": models }), &blob)
    }

    pub fn parse(data: &[u8]) -> Result<Self, MeshError> {
        let (h, r) = Reader::open(data, MESH_MAGIC, "mesh")?;
        let mut models = Vec::new();
        for m in h
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let arr = |k: &str| {
                m.get(k)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            };
            let bones = arr("bones")
                .iter()
                .map(|b| {
                    let inv = fa::<12>(&b["inv"]);
                    MeshBone {
                        name: s(b, "name"),
                        parent: b.get("parent").and_then(Value::as_u64).map(|p| p as usize),
                        scale: fa(&b["s"]),
                        rotation: fa(&b["r"]),
                        euler: self::b(b, "euler"),
                        translation: fa(&b["t"]),
                        ssc: self::b(b, "ssc"),
                        visible: self::b(b, "visible"),
                        inverse_bind: [
                            [inv[0], inv[1], inv[2], inv[3]],
                            [inv[4], inv[5], inv[6], inv[7]],
                            [inv[8], inv[9], inv[10], inv[11]],
                        ],
                    }
                })
                .collect::<Vec<_>>();
            let materials = arr("materials")
                .iter()
                .map(|x| {
                    let w = fa::<2>(&x["wrap"]);
                    let pairs = |k: &str| -> Vec<(String, String)> {
                        x.get(k)
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|p| {
                                Some((
                                    p.get(0)?.as_str()?.to_string(),
                                    p.get(1)?.as_str()?.to_string(),
                                ))
                            })
                            .collect()
                    };
                    MeshMaterial {
                        name: s(x, "name"),
                        albedo: x.get("albedo").and_then(Value::as_str).map(String::from),
                        wrap: [w[0] as u8, w[1] as u8],
                        unlit: b(x, "unlit"),
                        textures: pairs("textures"),
                        blend: u(x, "blend") as u8,
                        alpha_ref: x.get("alpha_ref").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                        cull: u(x, "cull") as u8,
                        params: x
                            .get("params")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|p| {
                                let name = p.get(0)?.as_str()?.to_string();
                                let vals = p
                                    .get(1)?
                                    .as_array()?
                                    .iter()
                                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                                    .collect();
                                Some((name, vals))
                            })
                            .collect(),
                        options: pairs("options"),
                    }
                })
                .collect::<Vec<_>>();
            let vertices = r.vertices(&m["vertices"])?;
            let indices = r.u32s(&m["indices"])?;
            let shapes = arr("shapes")
                .iter()
                .map(|x| MeshShape {
                    name: s(x, "name"),
                    material: u(x, "material") as usize,
                    bone: u(x, "bone") as usize,
                    first_index: u(x, "first") as u32,
                    index_count: u(x, "count") as u32,
                })
                .collect::<Vec<_>>();
            for sh in &shapes {
                if sh.material >= materials.len().max(1)
                    || (sh.first_index + sh.index_count) as usize > indices.len()
                {
                    return Err(corrupt(format!("shape {}", sh.name)));
                }
            }
            if indices.iter().any(|&i| i as usize >= vertices.len())
                || vertices
                    .iter()
                    .any(|v| v.joints.iter().any(|&j| j as usize >= bones.len().max(1)))
            {
                return Err(corrupt(format!(
                    "model {}: index out of range",
                    s(m, "name")
                )));
            }
            let colors = match m.get("colors").filter(|v| !v.is_null()) {
                Some(v) => r.u32s(v)?.into_iter().map(u32::to_le_bytes).collect(),
                None => Vec::new(),
            };
            let uv1 = match m.get("uv1").filter(|v| !v.is_null()) {
                Some(v) => r.f32s(v)?.as_chunks::<2>().0.to_vec(),
                None => Vec::new(),
            };
            if (!colors.is_empty() && colors.len() != vertices.len())
                || (!uv1.is_empty() && uv1.len() != vertices.len())
            {
                return Err(corrupt(format!(
                    "model {}: colour / uv1 count",
                    s(m, "name")
                )));
            }
            models.push(MeshModel {
                name: s(m, "name"),
                bones,
                materials,
                shapes,
                vertices,
                indices,
                bounds: fa(&m["bounds"]),
                colors,
                uv1,
            });
        }
        Ok(Self { models })
    }

    pub fn model(&self, name: &str) -> Option<&MeshModel> {
        self.models.iter().find(|m| m.name == name)
    }
}

fn kind_code(k: CurveKind) -> u16 {
    match k {
        CurveKind::Cubic => 0,
        CurveKind::Linear => 1,
        CurveKind::BakedFloat => 2,
        CurveKind::StepInt => 4,
        CurveKind::BakedInt => 5,
        CurveKind::StepBool => 6,
        CurveKind::BakedBool => 7,
        CurveKind::Other(v) => v,
    }
}

fn kind_from(v: u64) -> CurveKind {
    match v {
        0 => CurveKind::Cubic,
        1 => CurveKind::Linear,
        2 => CurveKind::BakedFloat,
        4 => CurveKind::StepInt,
        5 => CurveKind::BakedInt,
        6 => CurveKind::StepBool,
        7 => CurveKind::BakedBool,
        v => CurveKind::Other(v as u16),
    }
}

fn wrap_code(w: CurveWrap) -> u8 {
    match w {
        CurveWrap::Clamp => 0,
        CurveWrap::Repeat => 1,
        CurveWrap::Mirror => 2,
        CurveWrap::RelativeRepeat => 3,
    }
}

fn wrap_from(v: u64) -> CurveWrap {
    match v {
        1 => CurveWrap::Repeat,
        2 => CurveWrap::Mirror,
        3 => CurveWrap::RelativeRepeat,
        _ => CurveWrap::Clamp,
    }
}

fn curve_to_json(c: &Curve, blob: &mut Blob) -> Value {
    let keys: Vec<f32> = c.keys.iter().flatten().copied().collect();
    json!({
        "target": c.target, "kind": kind_code(c.kind), "start": c.start, "end": c.end,
        "pre": wrap_code(c.pre_wrap), "post": wrap_code(c.post_wrap), "delta": c.delta,
        "frames": blob.f32s(&c.frames), "keys": blob.f32s(&keys),
    })
}

fn curve_from_json(v: &Value, r: &Reader) -> Result<Curve, MeshError> {
    let keys = r.f32s(&v["keys"])?;
    let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Ok(Curve {
        target: u(v, "target") as u32,
        kind: kind_from(u(v, "kind")),
        start: f("start"),
        end: f("end"),
        pre_wrap: wrap_from(u(v, "pre")),
        post_wrap: wrap_from(u(v, "post")),
        delta: f("delta"),
        frames: r.f32s(&v["frames"])?,
        keys: keys.as_chunks::<4>().0.to_vec(),
    })
}

impl SkelAnim {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut blob = Blob::default();
        let bones: Vec<Value> = self
            .bones
            .iter()
            .map(|t| {
                json!({
                    "bone": t.bone, "base": t.base, "mask": t.mask,
                    "curves": t.curves.iter().map(|c| curve_to_json(c, &mut blob)).collect::<Vec<_>>(),
                })
            })
            .collect();
        let vis: Vec<Value> = self
            .visibility
            .iter()
            .map(|t| {
                json!({ "bone": t.bone, "base": t.base,
                        "curve": t.curve.as_ref().map(|c| curve_to_json(c, &mut blob)) })
            })
            .collect();
        let tex: Vec<Value> = self
            .textures
            .iter()
            .map(|t| json!({ "material": t.material, "keys": t.keys.iter().map(|(f, s)| json!([f, s])).collect::<Vec<_>>() }))
            .collect();
        let h = json!({
            "name": self.name, "frames": self.frames, "loop": self.looping, "euler": self.euler,
            "bones": bones, "visibility": vis, "textures": tex,
        });
        container(ANIM_MAGIC, &h, &blob)
    }

    pub fn parse(data: &[u8]) -> Result<Self, MeshError> {
        let (h, r) = Reader::open(data, ANIM_MAGIC, "animation")?;
        let arr = |k: &str| {
            h.get(k)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let mut bones = Vec::new();
        for t in arr("bones") {
            let curves = t
                .get("curves")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|c| curve_from_json(c, &r))
                .collect::<Result<Vec<_>, _>>()?;
            bones.push(BoneTrack {
                bone: s(&t, "bone"),
                base: fa(&t["base"]),
                mask: u(&t, "mask") as u8,
                curves,
            });
        }
        let mut visibility = Vec::new();
        for t in arr("visibility") {
            let curve = match t.get("curve") {
                Some(c) if !c.is_null() => Some(curve_from_json(c, &r)?),
                _ => None,
            };
            visibility.push(VisTrack {
                bone: s(&t, "bone"),
                base: b(&t, "base"),
                curve,
            });
        }
        let textures = arr("textures")
            .iter()
            .map(|t| TexTrack {
                material: s(t, "material"),
                keys: t
                    .get("keys")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|k| {
                        Some((k.get(0)?.as_f64()? as f32, k.get(1)?.as_str()?.to_string()))
                    })
                    .collect(),
            })
            .collect();
        Ok(Self {
            name: s(&h, "name"),
            frames: u(&h, "frames") as u32,
            looping: b(&h, "loop"),
            euler: b(&h, "euler"),
            bones,
            visibility,
            textures,
        })
    }
}

/// Summary of one animation in `anims/<Model>/index.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimInfo {
    pub name: String,
    pub frames: u32,
    pub looping: bool,
    pub skeletal: bool,
    pub visibility: bool,
    pub textures: bool,
}

pub fn anim_index_to_json(list: &[AnimInfo]) -> Value {
    Value::Object(
        list.iter()
            .map(|a| {
                (
                    a.name.clone(),
                    json!({ "frames": a.frames, "loop": a.looping, "skeletal": a.skeletal,
                            "visibility": a.visibility, "textures": a.textures }),
                )
            })
            .collect(),
    )
}

pub fn anim_index_from_json(v: &Value) -> Vec<AnimInfo> {
    let mut out: Vec<AnimInfo> = v
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, a)| AnimInfo {
            name: k.clone(),
            frames: u(a, "frames") as u32,
            looping: b(a, "loop"),
            skeletal: b(a, "skeletal"),
            visibility: b(a, "visibility"),
            textures: b(a, "textures"),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Read `anims/<model>/index.json` (empty if missing or unreadable).
pub fn read_anim_index(path: &std::path::Path) -> Vec<AnimInfo> {
    std::fs::read(path)
        .ok()
        .and_then(|d| serde_json::from_slice::<Value>(&d).ok())
        .map(|v| anim_index_from_json(&v))
        .unwrap_or_default()
}

/// File-safe name of an animation (the game's names are plain ASCII identifiers).
pub fn anim_file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Rest-pose bone matrices of a BFRES model computed the same way as `MeshModel::pose`.
pub fn bfres_rest_world(m: &bfres::Model) -> Vec<Affine> {
    let model = MeshModel {
        bones: m
            .bones
            .iter()
            .map(|b| MeshBone {
                name: b.name.clone(),
                parent: b.parent,
                scale: b.scale,
                rotation: b.rotation,
                euler: b.euler(),
                translation: b.translation,
                ssc: b.segment_scale_compensate(),
                visible: b.visible(),
                inverse_bind: IDENTITY,
            })
            .collect(),
        ..Default::default()
    };
    model.rest_world()
}

/// Case-insensitive name lookup helper for animation sets.
pub fn find_ci<'a>(names: impl IntoIterator<Item = &'a str>, want: &str) -> Option<&'a str> {
    let mut exact = None;
    for n in names {
        if n == want {
            return Some(n);
        }
        if exact.is_none() && n.eq_ignore_ascii_case(want) {
            exact = Some(n);
        }
    }
    exact
}

/// Cache of bindings keyed by (model index, animation name).
pub type BindingCache = HashMap<(usize, String), Binding>;

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_formats::bfres::apply;

    fn bone(name: &str, parent: Option<usize>, t: [f32; 3]) -> MeshBone {
        MeshBone {
            name: name.into(),
            parent,
            scale: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            euler: true,
            translation: t,
            ssc: false,
            visible: true,
            inverse_bind: IDENTITY,
        }
    }

    /// Two-bone arm: root at the origin, tip 10 up; a vertex bound half/half.
    fn arm() -> MeshModel {
        let mut m = MeshModel {
            name: "arm".into(),
            bones: vec![
                bone("root", None, [0.0; 3]),
                bone("tip", Some(0), [0.0, 10.0, 0.0]),
            ],
            materials: vec![MeshMaterial {
                name: "skin".into(),
                albedo: Some("skin_Alb".into()),
                wrap: [0, 2],
                unlit: false,
                ..Default::default()
            }],
            shapes: vec![MeshShape {
                name: "s".into(),
                material: 0,
                bone: 1,
                first_index: 0,
                index_count: 3,
            }],
            vertices: vec![
                Vertex {
                    position: [0.0, 10.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    joints: [1, 0, 0, 0],
                    weights: [1.0, 0.0, 0.0, 0.0],
                    ..Default::default()
                },
                Vertex {
                    position: [1.0, 5.0, 0.0],
                    joints: [0, 1, 0, 0],
                    weights: [0.5, 0.5, 0.0, 0.0],
                    ..Default::default()
                },
                Vertex::default(),
            ],
            indices: vec![0, 1, 2],
            bounds: [0.0, 0.0, 0.0, 1.0, 10.0, 0.0],
            colors: Vec::new(),
            uv1: Vec::new(),
        };
        let rest = m.rest_world();
        for (b, w) in m.bones.iter_mut().zip(&rest) {
            b.inverse_bind = bfres::invert(w);
        }
        m
    }

    fn linear(target: u32, v0: f32, v1: f32, frames: f32) -> Curve {
        Curve {
            target,
            kind: CurveKind::Linear,
            start: 0.0,
            end: frames,
            pre_wrap: CurveWrap::Clamp,
            post_wrap: CurveWrap::Clamp,
            delta: v1 - v0,
            frames: vec![0.0, frames],
            keys: vec![[v0, v1 - v0, 0.0, 0.0], [v1, 0.0, 0.0, 0.0]],
        }
    }

    fn lift() -> SkelAnim {
        SkelAnim {
            name: "lift".into(),
            frames: 10,
            looping: true,
            euler: true,
            bones: vec![BoneTrack {
                bone: "tip".into(),
                base: [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 0.0],
                mask: 7,
                curves: vec![linear(8, 10.0, 20.0, 10.0)],
            }],
            visibility: vec![VisTrack {
                bone: "tip".into(),
                base: true,
                curve: Some(Curve {
                    kind: CurveKind::StepBool,
                    frames: vec![0.0, 5.0],
                    keys: vec![[1.0; 4], [0.0; 4]],
                    ..linear(1, 0.0, 0.0, 10.0)
                }),
            }],
            textures: vec![TexTrack {
                material: "skin".into(),
                keys: vec![(0.0, "a".into()), (6.0, "b".into())],
            }],
        }
    }

    #[test]
    fn rest_pose_skins_to_identity() {
        let m = arm();
        let mut p = Pose::default();
        m.pose(None, 0.0, &mut p);
        for s in &p.skin {
            for (r, row) in s.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    let want = if r == c { 1.0 } else { 0.0 };
                    assert!((v - want).abs() < 1e-6);
                }
            }
        }
        assert_eq!(p.textures, vec![Some("skin_Alb".to_string())]);
        assert_eq!(p.visible, vec![true, true]);
    }

    #[test]
    fn animation_moves_bones_visibility_and_textures() {
        let m = arm();
        let a = lift();
        let bind = Binding::new(&m, &a);
        assert_eq!(bind.bound_bones(), 1);
        let mut p = Pose::default();
        m.pose(Some((&a, &bind)), 5.0, &mut p);
        // tip moved from y 10 to 15: its vertex follows fully, the shared one half way
        let tip = apply(&p.skin[1], [0.0, 10.0, 0.0]);
        assert!((tip[1] - 15.0).abs() < 1e-5);
        let half = |v: &Vertex| {
            let a = apply(&p.skin[v.joints[0] as usize], v.position);
            let b = apply(&p.skin[v.joints[1] as usize], v.position);
            [0, 1, 2].map(|k| a[k] * v.weights[0] + b[k] * v.weights[1])
        };
        assert!((half(&m.vertices[1])[1] - 7.5).abs() < 1e-5);
        assert_eq!(p.visible, vec![true, false]);
        assert_eq!(p.textures, vec![Some("a".to_string())]);
        m.pose(Some((&a, &bind)), 7.0, &mut p);
        assert_eq!(p.textures, vec![Some("b".to_string())]);
        assert_eq!(a.frame_at(23.0), 3.0);
    }

    #[test]
    fn segment_scale_compensation() {
        let mut m = arm();
        m.bones[0].scale = [2.0; 3];
        m.bones[1].ssc = true;
        let w = m.rest_world();
        // the root's scale moves the tip (translation is in the parent's space) but does not
        // scale it
        assert!((w[1][1][3] - 20.0).abs() < 1e-5);
        assert!((w[1][0][0] - 1.0).abs() < 1e-5);
        m.bones[1].ssc = false;
        assert!((m.rest_world()[1][0][0] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn mesh_and_anim_round_trip() {
        let f = MeshFile {
            models: vec![arm()],
        };
        let bytes = f.to_bytes();
        assert_eq!(&bytes[..4], MESH_MAGIC);
        assert_eq!(MeshFile::parse(&bytes).unwrap(), f);
        let a = lift();
        assert_eq!(SkelAnim::parse(&a.to_bytes()).unwrap(), a);
        assert!(MeshFile::parse(&a.to_bytes()).is_err());
        let mut bad = bytes.clone();
        bad.truncate(bytes.len() - 8);
        assert!(MeshFile::parse(&bad).is_err());
        let idx = vec![AnimInfo {
            name: "lift".into(),
            frames: 10,
            looping: true,
            skeletal: true,
            visibility: true,
            textures: false,
        }];
        assert_eq!(anim_index_from_json(&anim_index_to_json(&idx)), idx);
    }

    #[test]
    fn helpers() {
        assert_eq!(srt_component(0x14), Some(8));
        assert_eq!(srt_component(0x2C), Some(6));
        assert_eq!(srt_component(0x30), None);
        assert_eq!(find_ci(["Walk", "Wait"], "walk"), Some("Walk"));
        assert_eq!(find_ci(["walk", "Walk"], "Walk"), Some("Walk"));
        assert_eq!(anim_file_name("2jmp b/1"), "2jmp_b_1");
        assert_eq!(size_of::<Vertex>(), 56);
    }
}
