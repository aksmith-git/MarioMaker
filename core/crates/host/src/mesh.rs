//! 3D meshes for the NSMBU / 3D World styles, shared by the front ends (the Swift app through
//! `mmk-ffi`, the portable shell directly): lazily loaded meshes and animations, posing, and the
//! plans that turn the sim's sprites into mesh draws.
//!
//! How sim sprites become meshes (DECISIONS.md "3D styles: sim sprites → meshes"): the sim names
//! SMB1 models and frames (`M1_Enemy_kuribo`, `walk.1`). For NSMBU / 3D World the model resolves
//! to the style's file (`WU_Enemy_kuribo`, players and blocks through tables below), the frame
//! stem picks an animation (`walk` → `walk` / `Walk`, synonyms in `ANIM_SYNONYMS`), and the
//! animation time is the world tick [inf: the sim keeps no per-actor animation clock].

use std::collections::HashMap;
use std::ffi::{CString, c_char};
use std::path::PathBuf;

use mmk_assets::Cache;
use mmk_assets::mesh::{
    AnimInfo, Binding, MeshFile, Pose, SkelAnim, anim_file_name, find_ci, read_anim_index,
};
use mmk_formats::bfres::{Affine, IDENTITY, mul};
use mmk_sim::Sprite;

/// "No texture" in mesh pose output (same value as the C API's `MMK_TEXTURE_NONE`).
pub const TEXTURE_NONE: u32 = u32::MAX;

/// One vertex (56 bytes): position, normal, uv, 4 bone indices, 4 weights.
pub type MmkMeshVertex = mmk_assets::mesh::Vertex;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkMeshModel {
    pub name: *const c_char,
    pub vertices: *const MmkMeshVertex,
    pub vertex_count: usize,
    pub indices: *const u32,
    pub index_count: usize,
    pub bone_count: u32,
    pub shape_count: u32,
    /// Rest-pose bounds: min x, y, z, max x, y, z (model units = world px).
    pub bounds: [f32; 6],
    /// Vertex colours, RGBA8 per vertex (NULL if the model has none).
    pub colors: *const u8,
}

/// Shape flag: pre-rendered sprite card (cut out by alpha, no lighting).
pub const MMK_MESH_UNLIT: u32 = 1;
/// Shape flag: self-lit material (no lighting: sky, clouds, glowing parts).
pub const MMK_MESH_SELF_LIT: u32 = 2;
/// Shape flag: the environment's fogs apply.
pub const MMK_MESH_FOG: u32 = 4;
/// Shape flag: alpha-tested (discard below `alpha_ref`).
pub const MMK_MESH_ALPHA_TEST: u32 = 8;
/// Shape flags bits 4–5: blending (0 opaque, 1 alpha blended, 2 additive).
pub const MMK_MESH_BLEND_SHIFT: u32 = 4;
/// Shape flags bits 6–7: faces drawn (0 front, 1 back, 2 both, 3 none).
pub const MMK_MESH_FACES_SHIFT: u32 = 6;
/// Shape flags bits 12–13: what the extra texture is (0 none, 1 emission added, 2 alpha mask,
/// 3 sky base under the albedo's cloud layer).
pub const MMK_MESH_EXTRA_SHIFT: u32 = 12;
/// Shape flag: the second distance fog applies (`enable_fog1`).
pub const MMK_MESH_FOG1: u32 = 1 << 14;
/// Shape flag: the height fog applies (`enable_fog_y`).
pub const MMK_MESH_FOG_Y: u32 = 1 << 15;
/// Shape flag: texture coordinates from the world position (u = x / 64, v = −y / 64, then the
/// SRT) instead of the model's (`tex_coord0_calc` 5 with `enable_tex_coord0_mtx_cancel`).
pub const MMK_MESH_WORLD_UV: u32 = 1 << 16;
/// World pixels one repeat of a world-mapped texture covers [inf: measured on the castle walls'
/// bricks in a reference frame, 8 rows of ~8.1 px].
pub const WORLD_UV_SPAN: f32 = 64.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkMeshShape {
    pub first_index: u32,
    pub index_count: u32,
    pub material: u32,
    /// Bone that switches the shape's visibility.
    pub bone: u32,
    /// `MMK_MESH_*` bits 0–7 (see [`ShapeLook`]); bits 8–9 wrap u, bits 10–11 wrap v (0 repeat,
    /// 1 mirror, 2 clamp).
    pub flags: u32,
    /// Alpha-test reference (with `MMK_MESH_ALPHA_TEST`).
    pub alpha_ref: f32,
    /// Texture SRT of the albedo coordinates: scale u, scale v, rotate (radians), translate u,
    /// translate v.
    pub srt: [f32; 5],
    /// How much the vertex colours tint the albedo (0 none … 1 fully).
    pub vertex_color_ratio: f32,
    /// The extra texture (`MMK_MESH_EXTRA_SHIFT`): texture index or `TEXTURE_NONE`.
    pub extra_texture: u32,
    /// Emission strength (extra mode 1).
    pub emission: f32,
    /// Colour the albedo is multiplied by (TEV stage 0's constant colour).
    pub tint: [f32; 3],
}

/// How a shape's material is drawn, worked out once for both renderers.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeLook {
    /// `MMK_MESH_*` bits 0–7 and 12–15, and the wrap bits 8–11 of [`MmkMeshShape::flags`].
    pub flags: u32,
    pub alpha_ref: f32,
    pub srt: [f32; 5],
    pub vertex_color_ratio: f32,
    /// Name of the extra texture (in the model file's texture folder), if any.
    pub extra: Option<String>,
    pub emission: f32,
    /// Colour the albedo is multiplied by.
    pub tint: [f32; 3],
}

impl ShapeLook {
    pub fn blend(&self) -> u32 {
        (self.flags >> MMK_MESH_BLEND_SHIFT) & 3
    }
    pub fn faces(&self) -> u32 {
        (self.flags >> MMK_MESH_FACES_SHIFT) & 3
    }
    pub fn extra_mode(&self) -> u32 {
        (self.flags >> MMK_MESH_EXTRA_SHIFT) & 3
    }
}

/// The look of a material (None: a plain lit opaque shape). The uber shader lights a material
/// only with its `enable_diffuse` option (sky, clouds and glowing parts are self-lit) and fogs it
/// with `enable_fog`; caches without shader options light everything.
pub fn shape_look(mat: Option<&mmk_assets::mesh::MeshMaterial>) -> ShapeLook {
    let Some(m) = mat else {
        return ShapeLook {
            flags: 2 << MMK_MESH_FACES_SHIFT,
            alpha_ref: 0.5,
            srt: [1.0, 1.0, 0.0, 0.0, 0.0],
            vertex_color_ratio: 1.0,
            extra: None,
            emission: 0.0,
            tint: [1.0; 3],
        };
    };
    let self_lit = !m.unlit && !m.options.is_empty() && m.option("enable_diffuse") != Some("1");
    let fog = m.option("enable_fog") == Some("1");
    let on = |k: &str| m.option(k) == Some("1");
    // the extra texture: the sky's base (HDRI) under its cloud layer, an emission map, or the
    // alpha mask the uber shader reads through `gsys_user0`
    let (extra_mode, extra) = if on("enable_sky") && m.texture("_s0").is_some() {
        (3, m.texture("_s0"))
    } else if on("enable_emission") && m.texture("_e0").is_some() {
        (1, m.texture("_e0"))
    } else if m.texture("gsys_user0").is_some() {
        (2, m.texture("gsys_user0"))
    } else {
        (0, None)
    };
    let blend = match m.blend {
        2 => 1,
        3 => 2,
        _ => 0,
    };
    let mut flags = (m.unlit as u32 * MMK_MESH_UNLIT)
        | (self_lit as u32 * MMK_MESH_SELF_LIT)
        | (fog as u32 * MMK_MESH_FOG)
        | ((m.blend == 1) as u32 * MMK_MESH_ALPHA_TEST)
        | blend << MMK_MESH_BLEND_SHIFT
        | (m.cull as u32 & 3) << MMK_MESH_FACES_SHIFT
        | (m.wrap[0] as u32 & 3) << 8
        | (m.wrap[1] as u32 & 3) << 10
        | extra_mode << MMK_MESH_EXTRA_SHIFT
        | (on("enable_fog1") as u32 * MMK_MESH_FOG1)
        | (on("enable_fog_y") as u32 * MMK_MESH_FOG_Y)
        | ((m.option("tex_coord0_calc") == Some("5") && on("enable_tex_coord0_mtx_cancel")) as u32
            * MMK_MESH_WORLD_UV);
    if m.unlit {
        // cards are seen from both sides (mirrored for the walking direction)
        flags = (flags & !(3 << MMK_MESH_FACES_SHIFT)) | 2 << MMK_MESH_FACES_SHIFT;
    }
    // tex_srt0: [mode, scale u, scale v, rotate, translate u, translate v]
    let srt = match m.param("tex_srt0") {
        Some(p) if p.len() >= 6 => [p[1], p[2], p[3], p[4], p[5]],
        _ => [1.0, 1.0, 0.0, 0.0, 0.0],
    };
    ShapeLook {
        flags,
        alpha_ref: m.alpha_ref,
        srt,
        vertex_color_ratio: m
            .param("vertex_color_ratio")
            .and_then(|v| v.first().copied())
            .unwrap_or(1.0),
        extra: extra.map(String::from),
        emission: m
            .param("emission_intensity")
            .and_then(|v| v.first().copied())
            .unwrap_or(1.0),
        // TEV stage 0 with colour source 200 multiplies the albedo by constant colour 0 [inf:
        // from the options of the materials that use it, e.g. the castle walls' warm tint;
        // characters enable the stage with source 0 and a black constant]
        tint: match m.param("const_color0") {
            Some(c)
                if on("enable_tev0_color")
                    && m.option("tev_tev0_color_color") == Some("200")
                    && c.len() >= 3 =>
            {
                [c[0], c[1], c[2]]
            }
            _ => [1.0; 3],
        },
    }
}

/// Texture coordinates through a shape's SRT [inf: scale, then rotation about the origin, then
/// translation].
pub fn apply_srt(uv: [f32; 2], srt: &[f32; 5]) -> [f32; 2] {
    let (u, v) = (uv[0] * srt[0], uv[1] * srt[1]);
    let (s, c) = srt[2].sin_cos();
    [c * u - s * v + srt[3], s * u + c * v + srt[4]]
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkMeshPose {
    /// `bone_count` skinning matrices, 12 floats each (row-major 3×4): multiply bind-pose
    /// vertices.
    pub skin: *const f32,
    /// Per shape: texture index (`mmk_assets_texture`) or `TEXTURE_NONE`.
    pub shape_texture: *const u32,
    /// Per shape: 1 visible, 0 hidden.
    pub shape_visible: *const u8,
    pub bone_count: u32,
    pub shape_count: u32,
}

/// One mesh to draw this frame (`mmk_engine_meshes`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MmkMeshDraw {
    pub mesh: i32,
    pub model: u32,
    /// Animation id (`mmk_mesh_anim`) or −1 for the rest pose.
    pub anim: i32,
    /// Frames since the animation started (`mmk_mesh_pose` wraps / clamps it).
    pub time: f32,
    /// Model → world pixels (y up, z toward the viewer), row-major 3×4.
    pub transform: [f32; 12],
    pub layer: u32,
    /// Number of snapshot sprites drawn before this mesh (sprites are sorted by layer).
    pub order: u32,
    /// bit 0: flat card (pre-rendered sprite), bit 1: terrain or block tile.
    pub flags: u32,
    /// Texture variant passed to `mmk_mesh_pose` (1 = the `.1` textures, e.g. Fire Mario).
    pub variant: u32,
}

pub const MMK_MESH_DRAW_CARD: u32 = 1;
pub const MMK_MESH_DRAW_TILE: u32 = 2;

// ---------------------------------------------------------------------------------------------

pub struct LoadedMesh {
    pub name: String,
    pub file: MeshFile,
    pub names: Vec<CString>,
    /// Per model and bone: shown when no animation says otherwise (alternative parts such as
    /// faces `Face01…Face08` are hidden, `…00` shown) [inf].
    pub default_visible: Vec<Vec<bool>>,
}

/// Lazily loaded meshes and animations of one asset cache.
#[derive(Default)]
pub struct MeshStore {
    pub root: PathBuf,
    /// Never removed: the vertex / index / name buffers handed to C stay where they are.
    pub meshes: Vec<LoadedMesh>,
    pub by_name: HashMap<String, i32>,
    pub anims: Vec<SkelAnim>,
    pub anim_ids: HashMap<(String, String), i32>,
    pub anim_index: HashMap<String, Vec<AnimInfo>>,
    pub bindings: HashMap<(i32, usize, i32), Binding>,
    pub textures: HashMap<(String, String), u32>,
    pub any_texture: Option<HashMap<String, u32>>,
    pub pose: Pose,
    pub skin: Vec<f32>,
    pub shape_texture: Vec<u32>,
    pub shape_visible: Vec<u8>,
    pub preview: Vec<MmkMeshDraw>,
}

pub fn default_visible(file: &MeshFile) -> Vec<Vec<bool>> {
    file.models
        .iter()
        .map(|m| {
            let names: Vec<&str> = m.bones.iter().map(|b| b.name.as_str()).collect();
            m.bones
                .iter()
                .map(|b| {
                    let n = &b.name;
                    let variant = n.len() > 2
                        && n[n.len() - 2..].bytes().all(|c| c.is_ascii_digit())
                        && &n[n.len() - 2..] != "00";
                    let first = format!("{}00", &n[..n.len().saturating_sub(2)]);
                    b.visible && !(variant && names.contains(&first.as_str()))
                })
                .collect()
        })
        .collect()
}

impl MeshStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// Whether the cache has a mesh stage at all.
    pub fn available(&self) -> bool {
        !self.root.as_os_str().is_empty() && self.root.join("meshes").is_dir()
    }

    /// Mesh id of a model file, loading it on first use; −1 if the cache has none.
    pub fn find(&mut self, name: &str) -> i32 {
        if let Some(&id) = self.by_name.get(name) {
            return id;
        }
        let path = self.root.join(format!("meshes/{name}.mmesh"));
        let id = match std::fs::read(&path)
            .ok()
            .and_then(|d| MeshFile::parse(&d).ok())
        {
            Some(file) if !self.root.as_os_str().is_empty() => {
                let names = file
                    .models
                    .iter()
                    .map(|m| CString::new(m.name.clone()).unwrap_or_default())
                    .collect();
                let default_visible = default_visible(&file);
                self.meshes.push(LoadedMesh {
                    name: name.to_string(),
                    file,
                    names,
                    default_visible,
                });
                self.meshes.len() as i32 - 1
            }
            _ => -1,
        };
        self.by_name.insert(name.to_string(), id);
        id
    }

    pub fn mesh(&self, id: i32) -> Option<&LoadedMesh> {
        usize::try_from(id).ok().and_then(|i| self.meshes.get(i))
    }

    /// Animations listed for an animation folder (`anims/<folder>/index.json`).
    pub fn index(&mut self, folder: &str) -> &[AnimInfo] {
        if !self.anim_index.contains_key(folder) {
            let path = self.root.join(format!("anims/{folder}/index.json"));
            let list = read_anim_index(&path);
            self.anim_index.insert(folder.to_string(), list);
        }
        &self.anim_index[folder]
    }

    /// Animation id of `name` in `folder` (exact name), loading it on first use.
    pub fn load_anim(&mut self, folder: &str, name: &str) -> i32 {
        let key = (folder.to_string(), name.to_string());
        if let Some(&id) = self.anim_ids.get(&key) {
            return id;
        }
        let path = self
            .root
            .join(format!("anims/{folder}/{}.mskel", anim_file_name(name)));
        let id = match std::fs::read(&path)
            .ok()
            .and_then(|d| SkelAnim::parse(&d).ok())
        {
            Some(a) => {
                self.anims.push(a);
                self.anims.len() as i32 - 1
            }
            None => -1,
        };
        self.anim_ids.insert(key, id);
        id
    }

    /// First animation found: each candidate name (case-insensitive) with each prefix, in each
    /// folder, in that order.
    pub fn find_anim(&mut self, folders: &[String], prefixes: &[&str], candidates: &[&str]) -> i32 {
        for cand in candidates {
            for prefix in prefixes {
                let want = format!("{prefix}{cand}");
                for folder in folders {
                    let found = {
                        let list = self.index(folder);
                        find_ci(list.iter().map(|a| a.name.as_str()), &want).map(String::from)
                    };
                    if let Some(name) = found {
                        let id = self.load_anim(folder, &name);
                        if id >= 0 {
                            return id;
                        }
                    }
                }
            }
        }
        -1
    }

    pub fn anim(&self, id: i32) -> Option<&SkelAnim> {
        usize::try_from(id).ok().and_then(|i| self.anims.get(i))
    }

    /// Texture index of `tex` for a mesh file: its own folder first, then any model's texture of
    /// that name.
    pub fn texture(&mut self, cache: &Cache, mesh: &str, tex: &str, variant: u32) -> u32 {
        let tex = if variant == 1 {
            // Fire forms: the `.1` texture next to a `.0` one
            match tex.strip_suffix(".0") {
                Some(stem) => format!("{stem}.1"),
                None => tex.to_string(),
            }
        } else {
            tex.to_string()
        };
        let key = (mesh.to_string(), tex.clone());
        if let Some(&i) = self.textures.get(&key) {
            return i;
        }
        let i = cache
            .texture_index(mesh, &tex)
            .map(|i| i as u32)
            .or_else(|| {
                let any = self.any_texture.get_or_insert_with(|| {
                    let mut m = HashMap::new();
                    for (i, t) in cache.textures.iter().enumerate() {
                        m.entry(t.name.clone()).or_insert(i as u32);
                    }
                    m
                });
                any.get(&tex).copied()
            })
            .unwrap_or(TEXTURE_NONE);
        self.textures.insert(key, i);
        i
    }

    /// Pose a model (`anim` −1 = rest) `time` frames into the animation; fills the output
    /// buffers. Returns (bone count, shape count), None for a bad id.
    pub fn pose(
        &mut self,
        cache: &Cache,
        mesh: i32,
        model: usize,
        anim: i32,
        time: f32,
        variant: u32,
    ) -> Option<(usize, usize)> {
        let lm: &LoadedMesh = self.meshes.get(usize::try_from(mesh).ok()?)?;
        let m = lm.file.models.get(model)?;
        let a: Option<&SkelAnim> = usize::try_from(anim).ok().and_then(|i| self.anims.get(i));
        let bound = match a {
            Some(a) => Some((
                a,
                &*self
                    .bindings
                    .entry((mesh, model, anim))
                    .or_insert_with(|| Binding::new(m, a)),
            )),
            None => None,
        };
        let frame = a.map(|a| a.frame_at(time)).unwrap_or(0.0);
        let pose = &mut self.pose;
        m.pose(bound, frame, pose);
        // bones the animation does not mention keep the default (alternative parts hidden)
        let defaults = &lm.default_visible[model];
        for (i, b) in m.bones.iter().enumerate() {
            let tracked = a.is_some_and(|a| a.visibility.iter().any(|t| t.bone == b.name));
            if !tracked {
                pose.visible[i] = defaults[i];
            }
        }
        self.skin.clear();
        for sk in &pose.skin {
            self.skin.extend(sk.iter().flatten());
        }
        self.shape_visible.clear();
        let mut names = Vec::with_capacity(m.shapes.len());
        for sh in &m.shapes {
            self.shape_visible
                .push(pose.visible.get(sh.bone).copied().unwrap_or(true) as u8);
            names.push(pose.textures.get(sh.material).cloned().flatten());
        }
        let (bones, mesh_name) = (m.bones.len(), lm.name.clone());
        self.shape_texture.clear();
        for n in names {
            let t = match n {
                Some(t) => self.texture(cache, &mesh_name, &t, variant),
                None => TEXTURE_NONE,
            };
            self.shape_texture.push(t);
        }
        Some((bones, self.shape_texture.len()))
    }

    /// Texture index of a shape look's extra texture in mesh file `mesh` (`TEXTURE_NONE` if
    /// none).
    pub fn extra_texture(&mut self, cache: &Cache, mesh: i32, look: &ShapeLook) -> u32 {
        let (Some(name), Some(lm)) = (look.extra.as_deref(), self.mesh(mesh)) else {
            return TEXTURE_NONE;
        };
        let file = lm.name.clone();
        self.texture(cache, &file, name, 0)
    }

    /// Model-space world matrix of `bone` after the last `pose` call.
    pub fn posed_bone(&self, bone: usize) -> Option<Affine> {
        self.pose.world.get(bone).copied()
    }
}

// --- sim sprites → mesh draws ------------------------------------------------------------------

/// Turn toward the walking direction for 3D actors in the side view: 70° from facing the
/// camera [inf: picked to show the face, not measured from the game].
pub const MESH_TURN: f32 = 70.0 * std::f32::consts::PI / 180.0;

/// Frame stem → animation names to try (case-insensitive), after the stem itself.
pub const ANIM_SYNONYMS: &[(&str, &[&str])] = &[
    ("walk", &["Walk", "run", "Run", "Move"]),
    ("walk_angry", &["walk", "Walk", "Run"]),
    ("walk_carry", &["carry_walk", "CarryFront", "walk", "Walk"]),
    ("carry_walk", &["CarryFront", "walk", "Walk"]),
    ("carry_wait", &["CarryUp", "wait", "Wait"]),
    ("carry_jump", &["jump", "Jump"]),
    ("wait", &["Wait", "Move", "wait"]),
    ("wait2", &["wait", "Wait"]),
    ("nokey_wait", &["wait", "Wait"]),
    ("wait_body", &["wait", "Wait"]),
    ("look", &["wait", "Wait"]),
    ("jump", &["Jump"]),
    ("jump_st", &["jump", "Jump"]),
    ("jump_swing", &["jump", "Jump"]),
    ("fall", &["jump2", "FallLoop", "Fall", "Jump2"]),
    ("jump_ed", &["jumped", "Land"]),
    ("turn", &["Turn", "Brake"]),
    ("stoop", &["SquatWait", "squat"]),
    ("squat", &["stoop", "SquatWait"]),
    ("dead", &["DieOver", "dead_pose", "BlowDown", "damage"]),
    ("down", &["dead", "BlowDown", "PressDown", "damage"]),
    ("damage", &["PressDown", "BlowDown", "damB"]),
    ("damage_fly", &["BlowDown", "damage"]),
    ("shell", &["Move", "Wait"]),
    ("revival", &["RecoverSign", "RecoverShell"]),
    ("swim", &["Swim", "SwimWalk", "swim_walk"]),
    ("climb", &["TreeClimb"]),
    ("fly", &["parawait", "ParaWait", "WaitFly"]),
    ("spin", &["spin_jump", "SpinJumpL"]),
    ("slip", &["SlopeSlide"]),
    ("throw", &["carry_throw", "CarryUpThrow", "attack"]),
    ("fire_attack", &["fire_at", "Fire"]),
    ("attack", &["Attack", "fire_at"]),
];

/// Animation names to try for a sim frame name (`walk.1` → `walk`, its synonyms, then a plain
/// wait / walk).
pub fn anim_candidates(frame: &str) -> Vec<&str> {
    let stem = frame.split('.').next().unwrap_or(frame);
    let mut out = vec![stem];
    if let Some((_, syn)) = ANIM_SYNONYMS.iter().find(|(s, _)| *s == stem) {
        out.extend_from_slice(syn);
    }
    out.extend_from_slice(&["wait", "Wait"]);
    out
}

/// Animation folders a mesh file draws its animations from: its own, then shared ones
/// (players keep theirs in separate files) [inf from the file names].
pub fn anim_folders(mesh: &str) -> Vec<String> {
    let mut out = vec![mesh.to_string()];
    if mesh.starts_with("WU_Player_") && mesh.ends_with("Mdl") {
        let who = mesh
            .trim_start_matches("WU_Player_")
            .trim_end_matches("Mdl");
        out.push(format!("WU_Player_{who}Anm"));
        out.push("WU_Player_CommonAnm".into());
    }
    if let Some(rest) = mesh.strip_prefix("3W_Player_") {
        for part in ["Face", "Eye", "HandL", "HandR"] {
            if rest.ends_with(part) {
                out.push(format!("3W_Player_Animation_{part}"));
                return out;
            }
        }
        out.push(format!("3W_Player_Animation_{rest}"));
        if let Some(base) = rest.strip_suffix("Mini") {
            out.push(format!("3W_Player_Animation_{base}"));
        }
        out.push("3W_Player_Animation_Mario".into());
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|f| seen.insert(f.clone()));
    out
}

/// One model drawn for a sprite.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub mesh: i32,
    pub model: u32,
    pub anim: i32,
    /// Index of an earlier part and its bone this part rides on.
    pub attach: Option<(usize, usize)>,
}

/// How a (model, frame) of the sim is drawn as meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPlan {
    pub parts: Vec<Part>,
    /// Size of the SMB1 texture the sim sized the sprite by (for the scale).
    pub ref_size: (f32, f32),
    /// The SMB1 art faces right (players) or left (everything else) when not flipped.
    pub art_faces_right: bool,
    /// Turn toward the moving direction (actors), or mirror (flat cards).
    pub turn: bool,
    pub card: bool,
    pub variant: u32,
}

/// (model file, model name or "" for the first, attach to (part index, bone)).
pub type PlayerPart = (&'static str, &'static str, Option<(usize, &'static str)>);

/// Player model files and parts per style (forms the styles have no model for use the closest).
pub fn player_parts(style: &str, model: &str) -> Option<(Vec<PlayerPart>, u32)> {
    let small = model.starts_with("M1_Player_MarioMdl");
    let fire = model.starts_with("M1_Player_FireMarioMdl");
    let cat = model.starts_with("M1_Player_NekoMarioMdl");
    let builder = model.starts_with("M1_Player_BuilderMarioMdl");
    // the Boomerang Flower's outfit (3D World; the sim's update forms, world/powerups.rs)
    let boomerang = model.starts_with("M1_Player_BoomerangMarioMdl");
    // the Super Acorn's suit (NSMBU): the "MM" body and head models [inf: Musasabi Mario]
    let squirrel = model.starts_with("M1_Player_SquirrelMarioMdl");
    let player = model.starts_with("M1_Player_") && model.contains("MarioMdl");
    if !player {
        return None;
    }
    let variant = fire as u32;
    Some(match (style, small) {
        // NSMBU: body + head models in one file; "S" = small, plain = super [inf from sizes];
        // the head rides the body's `face_1` bone
        ("WU", true) => (
            vec![
                ("WU_Player_MarioMdl", "SMB_model", None),
                ("WU_Player_MarioMdl", "SMH_model", Some((0, "face_1"))),
            ],
            variant,
        ),
        ("WU", false) if squirrel => (
            vec![
                ("WU_Player_MarioMdl", "MMB_model", None),
                ("WU_Player_MarioMdl", "MMH_model", Some((0, "face_1"))),
            ],
            variant,
        ),
        ("WU", false) => (
            vec![
                ("WU_Player_MarioMdl", "MB_model", None),
                ("WU_Player_MarioMdl", "MH_model", Some((0, "face_1"))),
            ],
            variant,
        ),
        // 3D World: body + face, eyes and hands as separate files on the body's bones
        ("3W", true) => (
            vec![
                ("3W_Player_MarioMini", "", None),
                ("3W_Player_MarioMiniFace", "", Some((0, "Head"))),
                ("3W_Player_MarioMiniEye", "", Some((0, "Head"))),
                ("3W_Player_MarioMiniHandL", "", Some((0, "HandL"))),
                ("3W_Player_MarioMiniHandR", "", Some((0, "HandR"))),
            ],
            variant,
        ),
        // the cat suit: its own body, hands and tail on Mario's skeleton (animations "Climb…")
        ("3W", false) if cat => (
            vec![
                ("3W_Player_MarioClimb", "", None),
                ("3W_Player_MarioFace", "", Some((0, "Head"))),
                ("3W_Player_MarioEye", "", Some((0, "Head"))),
                ("3W_Player_MarioClimbHandL", "", Some((0, "HandL"))),
                ("3W_Player_MarioClimbHandR", "", Some((0, "HandR"))),
                ("3W_Player_MarioClimbTail", "", Some((0, "Hip"))),
            ],
            variant,
        ),
        // Builder Mario: the outfit body on Mario's skeleton; the hammer rides the right hand
        ("3W", false) if builder => (
            vec![
                ("3W_Player_MarioBuilder", "", None),
                ("3W_Player_MarioFace", "", Some((0, "Head"))),
                ("3W_Player_MarioEye", "", Some((0, "Head"))),
                ("3W_Player_MarioHandL", "", Some((0, "HandL"))),
                ("3W_Player_MarioHandR", "", Some((0, "HandR"))),
                ("3W_Player_Hammer", "", Some((0, "HandR"))),
            ],
            variant,
        ),
        // Boomerang Mario: the outfit body on Mario's skeleton (animations "Boomerang…")
        ("3W", false) if boomerang => (
            vec![
                ("3W_Player_MarioBoomerang", "", None),
                ("3W_Player_MarioFace", "", Some((0, "Head"))),
                ("3W_Player_MarioEye", "", Some((0, "Head"))),
                ("3W_Player_MarioHandL", "", Some((0, "HandL"))),
                ("3W_Player_MarioHandR", "", Some((0, "HandR"))),
            ],
            variant,
        ),
        ("3W", false) => (
            vec![
                ("3W_Player_Mario", "", None),
                ("3W_Player_MarioFace", "", Some((0, "Head"))),
                ("3W_Player_MarioEye", "", Some((0, "Head"))),
                ("3W_Player_MarioHandL", "", Some((0, "HandL"))),
                ("3W_Player_MarioHandR", "", Some((0, "HandR"))),
            ],
            variant,
        ),
        _ => return None,
    })
}

/// 3D World blocks: SMB1 field-atlas index → model file (the 3D style draws blocks as models).
pub fn block_model(atlas: u32) -> Option<&'static str> {
    Some(match atlas {
        1 => "3W_Object_block_renga",
        2 => "3W_Object_block_hatena",
        5 => "3W_Object_block_kara",
        6 => "3W_Object_block_katai",
        64 => "3W_Object_block_chikuwa",
        102 => "3W_Object_block_kumo",
        120 => "3W_Object_block_kori",
        _ => return None,
    })
}

/// Model of a multi-model file to draw for frame `stem`: one named after the stem (`shell` →
/// `shell_model`), else one named after the file (`block_renga_model`), else the first one that
/// is not a break-apart model.
pub fn pick_model(file: &MeshFile, mesh_name: &str, stem: &str) -> usize {
    if file.models.len() <= 1 {
        return 0;
    }
    let lower = |s: &str| s.to_ascii_lowercase();
    let stem = lower(stem);
    if let Some(i) = file
        .models
        .iter()
        .position(|m| lower(&m.name).starts_with(&format!("{stem}_")))
    {
        return i;
    }
    let tail = mesh_name.split_once('_').map(|x| x.1).unwrap_or(mesh_name);
    let tail = lower(tail.split_once('_').map(|x| x.1).unwrap_or(tail));
    if let Some(i) = file
        .models
        .iter()
        .position(|m| lower(&m.name) == format!("{tail}_model"))
    {
        return i;
    }
    file.models
        .iter()
        .position(|m| !lower(&m.name).contains("break"))
        .unwrap_or(0)
}

impl MeshStore {
    /// Plan for a model sprite (`rect` None) of the sim in a 3D style; None = draw the sprite.
    pub fn plan_model(
        &mut self,
        cache: &Cache,
        style: &str,
        model: &str,
        frame: &str,
    ) -> Option<MeshPlan> {
        let stem = frame.split('.').next().unwrap_or(frame);
        // the SMB1 texture the sim sized the sprite by: this frame's, else another frame of the
        // same animation, else any texture of the model (frames of an actor share a size)
        let size = |t: &mmk_assets::cache::TextureEntry| (t.width as f32, t.height as f32);
        let ref_size = cache
            .texture_index(model, frame)
            .map(|i| size(&cache.textures[i]))
            .or_else(|| {
                let of_model = || cache.textures.iter().filter(|t| t.model == model);
                of_model()
                    .find(|t| t.name.split('.').next() == Some(stem))
                    .or_else(|| of_model().next())
                    .map(size)
            })
            .unwrap_or((16.0, 16.0));
        let candidates = anim_candidates(frame);
        let mut parts = Vec::new();
        let mut variant = 0;
        let art_faces_right = model.contains("_Player_");
        if let Some((list, v)) = player_parts(style, model) {
            variant = v;
            let small_prefix = if model.starts_with("M1_Player_MarioMdl") {
                "SMB_"
            } else if model.starts_with("M1_Player_NekoMarioMdl") {
                "Climb"
            } else {
                ""
            };
            for (file, model_name, attach) in list {
                let mesh = self.find(file);
                if mesh < 0 {
                    if parts.is_empty() {
                        return None;
                    }
                    continue;
                }
                let lm = self.mesh(mesh)?;
                let mi = if model_name.is_empty() {
                    0
                } else {
                    lm.file.models.iter().position(|m| m.name == model_name)?
                };
                let attach = match attach {
                    Some((p, bone)) => {
                        let parent: &Part = parts.get(p)?;
                        let pm = &self.mesh(parent.mesh)?.file.models[parent.model as usize];
                        Some((p, pm.bones.iter().position(|b| b.name == bone)?))
                    }
                    None => None,
                };
                let head = model_name.ends_with("H_model");
                let prefixes: Vec<&str> = if head {
                    vec!["PH_"]
                } else {
                    vec![small_prefix, ""]
                };
                let anim = self.find_anim(&anim_folders(file), &prefixes, &candidates);
                parts.push(Part {
                    mesh,
                    model: mi as u32,
                    anim,
                    attach,
                });
            }
        } else {
            let rest = model.strip_prefix("M1_")?;
            let styled = format!("{style}_{rest}");
            // a model of its own for this frame (3D World Koopa shells: `…_shell`)
            let own = format!("{styled}_{stem}");
            let mut mesh = self.find(&own);
            if mesh < 0 && matches!(stem, "shell" | "revival") {
                mesh = self.find(&format!("{styled}_shell"));
            }
            if mesh < 0 {
                mesh = self.find(&styled);
            }
            if mesh < 0 {
                return None;
            }
            let lm = self.mesh(mesh)?;
            let name = lm.name.clone();
            let mi = pick_model(&lm.file, &name, stem);
            let anim = self.find_anim(&anim_folders(&name), &[""], &candidates);
            parts.push(Part {
                mesh,
                model: mi as u32,
                anim,
                attach: None,
            });
        }
        let first = self.mesh(parts[0].mesh)?;
        let m0 = &first.file.models[parts[0].model as usize];
        let card = !m0.materials.is_empty() && m0.materials.iter().all(|m| m.unlit);
        Some(MeshPlan {
            parts,
            ref_size,
            art_faces_right,
            turn: model.contains("_Enemy_") || model.contains("_Player_"),
            card,
            variant,
        })
    }

    /// Plan for a 3D World terrain or block tile at field-atlas index `atlas`.
    pub fn plan_tile(&mut self, field: &str, atlas: u32) -> Option<MeshPlan> {
        let (mesh, model) = if let Some(file) = block_model(atlas) {
            let mesh = self.find(file);
            let lm = self.mesh(mesh)?;
            (mesh, pick_model(&lm.file, file, "wait") as u32)
        } else {
            let mesh = self.find(field);
            let lm = self.mesh(mesh)?;
            let theme = field.rsplit('_').next().unwrap_or("plain");
            let exact = format!("{theme}_model_{atlas:x}");
            let prefix = format!("{exact}_");
            let i = lm
                .file
                .models
                .iter()
                .position(|m| m.name == exact)
                .or_else(|| {
                    lm.file
                        .models
                        .iter()
                        .position(|m| m.name.starts_with(&prefix))
                })?;
            (mesh, i as u32)
        };
        Some(MeshPlan {
            parts: vec![Part {
                mesh,
                model,
                anim: -1,
                attach: None,
            }],
            ref_size: (16.0, 16.0),
            art_faces_right: false,
            turn: false,
            card: false,
            variant: 0,
        })
    }

    /// Mesh draws for one sprite.
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        cache: &Cache,
        plan: &MeshPlan,
        s: &Sprite,
        tile: bool,
        tick: f32,
        order: u32,
        out: &mut Vec<MmkMeshDraw>,
    ) {
        let (scale, ox, oy) = if tile {
            (1.0, s.x + s.w / 2.0, s.y)
        } else {
            // the sim's box over the SMB1 picture's size; the smaller ratio, since some boxes
            // are wider than the picture (a big Sledge Bro: 64 × 64 for a 24 × 32 picture)
            let k = (s.w / plan.ref_size.0.max(1.0)).min(s.h / plan.ref_size.1.max(1.0));
            (k, s.x + s.w / 2.0, s.y)
        };
        let facing_right = if plan.art_faces_right {
            !s.flip_x
        } else {
            s.flip_x
        };
        let mut base: Affine = IDENTITY;
        if plan.card {
            // pre-rendered art: mirror like the 2D sprite
            if s.flip_x {
                base[0][0] = -1.0;
            }
        } else if plan.turn {
            let a = if facing_right { MESH_TURN } else { -MESH_TURN };
            let (sn, c) = a.sin_cos();
            base = [[c, 0.0, sn, 0.0], [0.0, 1.0, 0.0, 0.0], [-sn, 0.0, c, 0.0]];
        }
        if s.flip_y {
            for v in base[1].iter_mut() {
                *v = -*v;
            }
        }
        let place: Affine = [
            [scale, 0.0, 0.0, ox],
            [0.0, scale, 0.0, oy],
            [0.0, 0.0, scale, 0.0],
        ];
        let root = mul(&place, &base);
        let mut transforms: Vec<Affine> = Vec::with_capacity(plan.parts.len());
        for p in &plan.parts {
            let t = match p.attach {
                Some((parent, bone)) => {
                    let pp = &plan.parts[parent];
                    let pw = transforms[parent];
                    match self
                        .pose(
                            cache,
                            pp.mesh,
                            pp.model as usize,
                            pp.anim,
                            tick,
                            plan.variant,
                        )
                        .and_then(|_| self.posed_bone(bone))
                    {
                        Some(b) => mul(&pw, &b),
                        None => pw,
                    }
                }
                None => root,
            };
            transforms.push(t);
            out.push(MmkMeshDraw {
                mesh: p.mesh,
                model: p.model,
                anim: p.anim,
                time: tick,
                transform: [
                    t[0][0], t[0][1], t[0][2], t[0][3], t[1][0], t[1][1], t[1][2], t[1][3],
                    t[2][0], t[2][1], t[2][2], t[2][3],
                ],
                layer: s.layer,
                order,
                flags: (plan.card as u32 * MMK_MESH_DRAW_CARD) | (tile as u32 * MMK_MESH_DRAW_TILE),
                variant: plan.variant,
            });
        }
    }
}

/// The styles drawn with 3D meshes.
pub fn mesh_style(style: &str) -> bool {
    matches!(style, "WU" | "3W")
}

// --- one frame -----------------------------------------------------------------------------------

/// The 3D scene of one frame: cameras and lighting of the play field and the background.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene3dFrame {
    pub field: crate::scene3d::Camera,
    /// The background scene's camera (None when the theme has no background model).
    pub background: Option<crate::scene3d::Camera>,
    pub field_light: [[f32; 4]; crate::scene3d::LIGHT_ROWS],
    pub background_light: [[f32; 4]; crate::scene3d::LIGHT_ROWS],
}

/// What a course's theme needs, worked out once: its SceneDB row, environments and background
/// model.
#[derive(Clone, Debug, Default)]
struct ThemeScene {
    key: (String, u8, bool, bool),
    row: Option<crate::scene3d::SceneRow>,
    field_env: crate::scene3d::Env,
    background_env: crate::scene3d::Env,
    /// (mesh, model, animation) of the background scene.
    background: Option<(i32, u32, i32)>,
}

/// Turns a world's sprites into this frame's draw lists: the sprites that stay 2D (placed, in
/// draw order) and, for the 3D styles, the mesh draws, the background scene's draws and the 3D
/// scene (cameras, lights). Remembers each sprite name's plan; `reset` when the course or cache
/// changes.
#[derive(Default)]
pub struct MeshPlanner {
    plans: HashMap<(&'static str, &'static str), Option<MeshPlan>>,
    tiles: HashMap<(&'static str, u32), Option<MeshPlan>>,
    theme: Option<ThemeScene>,
    /// This frame's 3D scene (3D styles with a mesh stage only).
    pub scene: Option<Scene3dFrame>,
    /// This frame's background-scene draws (drawn with `scene.background`).
    pub background: Vec<MmkMeshDraw>,
}

impl MeshPlanner {
    pub fn reset(&mut self) {
        self.plans.clear();
        self.tiles.clear();
        self.theme = None;
    }

    /// The theme's scene, (re)built when the course's style / theme / night / orientation
    /// changes.
    fn theme_scene(
        &mut self,
        world: &mmk_sim::World,
        cache: &Cache,
        store: &mut MeshStore,
    ) -> &ThemeScene {
        let l = &world.level;
        let key = (l.style.clone(), l.theme, l.night, l.vertical);
        if self.theme.as_ref().is_none_or(|t| t.key != key) {
            let row = crate::scene3d::scene_row(cache, &l.style, l.theme, l.night, l.vertical)
                // vertical areas fall back to the horizontal scene when there is no `_V` model
                .filter(|r| store.find(&r.dv_model) >= 0)
                .or_else(|| crate::scene3d::scene_row(cache, &l.style, l.theme, l.night, false));
            let env = |name: Option<&str>| {
                name.and_then(|n| crate::scene3d::load_env(&cache.root, n))
                    .unwrap_or_default()
            };
            // the background model's sky material: its constant colours colour the sky
            let sky = row.as_ref().and_then(|r| {
                let id = store.find(&r.dv_model);
                let lm = store.mesh(id)?;
                let mat = lm
                    .file
                    .models
                    .iter()
                    .flat_map(|m| &m.materials)
                    .find(|m| m.option("enable_sky") == Some("1"))?;
                let mut c = [[1.0f32; 4]; 6];
                for (i, slot) in c.iter_mut().enumerate() {
                    let v = mat.param(&format!("const_color{i}"))?;
                    slot[..v.len().min(4)].copy_from_slice(&v[..v.len().min(4)]);
                }
                Some(c)
            });
            let background = row.as_ref().and_then(|r| {
                let mesh = store.find(&r.dv_model);
                let lm = store.mesh(mesh)?;
                let model = lm
                    .file
                    .models
                    .iter()
                    .position(|m| m.name == r.dv_model)
                    .unwrap_or(0) as u32;
                let anim = store.find_anim(
                    std::slice::from_ref(&r.dv_model),
                    &[""],
                    &[r.dv_model.as_str()],
                );
                Some((mesh, model, anim))
            });
            self.theme = Some(ThemeScene {
                key,
                field_env: env(row.as_ref().map(|r| r.env_model.as_str())),
                background_env: crate::scene3d::Env {
                    sky,
                    ..env(row.as_ref().map(|r| r.env_dv.as_str()))
                },
                row,
                background,
            });
        }
        self.theme.as_ref().expect("set above")
    }

    /// `store`: the cache's meshes, or None to draw everything as sprites. Each mesh draw's
    /// `order` is the number of 2D sprites drawn before it.
    pub fn frame(
        &mut self,
        world: &mmk_sim::World,
        cache: &Cache,
        store: Option<&mut MeshStore>,
        resolver: &mut crate::SpriteResolver,
        sprites: &mut Vec<crate::DrawSprite>,
        meshes: &mut Vec<MmkMeshDraw>,
    ) {
        let mut list = world.sprites();
        list.sort_by_key(|s| s.layer);
        let style = world.level.style.as_str();
        sprites.clear();
        meshes.clear();
        self.background.clear();
        self.scene = None;
        let mut store = store.filter(|s| mesh_style(style) && s.available());
        let tick = world.tick as f32;
        let mut has_background = false;
        if let Some(store) = store.as_deref_mut() {
            let cam = (world.camera_x, world.camera_y);
            let t = self.theme_scene(world, cache, store).clone();
            let field = crate::scene3d::field_camera(style, cam);
            let background = t
                .row
                .as_ref()
                .filter(|_| t.background.is_some())
                .map(|r| crate::scene3d::background_camera(r, cam));
            if let (Some((mesh, model, anim)), Some(_)) = (t.background, &background) {
                has_background = true;
                self.background.push(MmkMeshDraw {
                    mesh,
                    model,
                    anim,
                    time: tick,
                    transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    layer: 0,
                    order: 0,
                    flags: 0,
                    variant: 0,
                });
            }
            self.scene = Some(Scene3dFrame {
                field_light: t.field_env.uniform(field.eye),
                background_light: t
                    .background_env
                    .uniform(background.map_or(field.eye, |c| c.eye)),
                field,
                background,
            });
        }
        for s in list {
            let key = (s.image.model, s.image.texture);
            // the 2D backdrop gives way to the background scene
            if has_background && key.0.contains("_DV_") {
                continue;
            }
            if let Some(store) = store.as_deref_mut() {
                let plan = match s.image.rect {
                    // 3D World terrain and blocks are models per field-atlas cell
                    Some((rx, ry, _, _)) if style == "3W" && key.0.starts_with("3W_Field_") => {
                        let idx = (ry / 16.0) as u32 * 16 + (rx / 16.0) as u32;
                        self.tiles
                            .entry((key.0, idx))
                            .or_insert_with(|| store.plan_tile(key.0, idx))
                            .as_ref()
                    }
                    Some(_) => None,
                    None => self
                        .plans
                        .entry(key)
                        .or_insert_with(|| store.plan_model(cache, style, key.0, key.1))
                        .as_ref(),
                };
                if let Some(plan) = plan {
                    let order = sprites.len() as u32;
                    let tile = s.image.rect.is_some();
                    store.emit(cache, plan, &s, tile, tick, order, meshes);
                    continue;
                }
            }
            sprites.push(resolver.resolve_one(style, &s, cache));
        }
    }
}
