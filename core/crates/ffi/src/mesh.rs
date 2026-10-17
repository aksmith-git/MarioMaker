//! C ABI for 3D meshes (NSMBU / 3D World styles): mesh data, animation lookup and posing, and the
//! engine's per-frame mesh draw list. Declared in `app/Sources/CMmk/mmk.h` (section "meshes").
//!
//! Meshes and animations load lazily from the asset cache (`meshes/`, `anims/`) the first time
//! they are asked for and stay loaded with the assets handle, so pointers into mesh data stay
//! valid until `mmk_assets_free`. Pose buffers (`mmk_mesh_pose`) are valid until the next pose
//! call on the same assets handle. Meant for one render thread.
//!
//! How sim sprites become meshes (DECISIONS.md "3D styles: sim sprites → meshes"): the sim names
//! SMB1 models and frames (`M1_Enemy_kuribo`, `walk.1`). For NSMBU / 3D World the model resolves
//! to the style's file (`WU_Enemy_kuribo`, players and blocks through tables below), the frame
//! stem picks an animation (`walk` → `walk` / `Walk`, synonyms in `ANIM_SYNONYMS`), and the
//! animation time is the world tick [inf: the sim keeps no per-actor animation clock].

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

use mmk_assets::Cache;
use mmk_assets::mesh::{
    AnimInfo, Binding, MeshFile, Pose, SkelAnim, anim_file_name, find_ci, read_anim_index,
};
use mmk_formats::bfres::{Affine, IDENTITY, mul};
use mmk_sim::Sprite;

use crate::{MMK_TEXTURE_NONE, MmkAssets, guard, set_error};

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
}

/// Shape flag: draw without lighting (pre-rendered sprite cards).
pub const MMK_MESH_UNLIT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkMeshShape {
    pub first_index: u32,
    pub index_count: u32,
    pub material: u32,
    /// Bone that switches the shape's visibility.
    pub bone: u32,
    /// bit 0 `MMK_MESH_UNLIT`; bits 8–9 wrap u, bits 10–11 wrap v (0 repeat, 1 mirror, 2 clamp).
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkMeshPose {
    /// `bone_count` skinning matrices, 12 floats each (row-major 3×4): multiply bind-pose
    /// vertices.
    pub skin: *const f32,
    /// Per shape: texture index (`mmk_assets_texture`) or `MMK_TEXTURE_NONE`.
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

struct LoadedMesh {
    name: String,
    file: MeshFile,
    names: Vec<CString>,
    /// Per model and bone: shown when no animation says otherwise (alternative parts such as
    /// faces `Face01…Face08` are hidden, `…00` shown) [inf].
    default_visible: Vec<Vec<bool>>,
}

/// Lazily loaded meshes and animations of one asset cache.
#[derive(Default)]
pub struct MeshStore {
    root: PathBuf,
    /// Never removed: the vertex / index / name buffers handed to C stay where they are.
    meshes: Vec<LoadedMesh>,
    by_name: HashMap<String, i32>,
    anims: Vec<SkelAnim>,
    anim_ids: HashMap<(String, String), i32>,
    anim_index: HashMap<String, Vec<AnimInfo>>,
    bindings: HashMap<(i32, usize, i32), Binding>,
    textures: HashMap<(String, String), u32>,
    any_texture: Option<HashMap<String, u32>>,
    pose: Pose,
    skin: Vec<f32>,
    shape_texture: Vec<u32>,
    shape_visible: Vec<u8>,
    preview: Vec<MmkMeshDraw>,
}

fn default_visible(file: &MeshFile) -> Vec<Vec<bool>> {
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

    fn mesh(&self, id: i32) -> Option<&LoadedMesh> {
        usize::try_from(id).ok().and_then(|i| self.meshes.get(i))
    }

    /// Animations listed for an animation folder (`anims/<folder>/index.json`).
    fn index(&mut self, folder: &str) -> &[AnimInfo] {
        if !self.anim_index.contains_key(folder) {
            let path = self.root.join(format!("anims/{folder}/index.json"));
            let list = read_anim_index(&path);
            self.anim_index.insert(folder.to_string(), list);
        }
        &self.anim_index[folder]
    }

    /// Animation id of `name` in `folder` (exact name), loading it on first use.
    fn load_anim(&mut self, folder: &str, name: &str) -> i32 {
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
    fn texture(&mut self, cache: &Cache, mesh: &str, tex: &str, variant: u32) -> u32 {
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
            .unwrap_or(MMK_TEXTURE_NONE);
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
                None => MMK_TEXTURE_NONE,
            };
            self.shape_texture.push(t);
        }
        Some((bones, self.shape_texture.len()))
    }

    /// Model-space world matrix of `bone` after the last `pose` call.
    fn posed_bone(&self, bone: usize) -> Option<Affine> {
        self.pose.world.get(bone).copied()
    }
}

// --- C API -------------------------------------------------------------------------------------

fn store(assets: *const MmkAssets) -> Option<std::sync::MutexGuard<'static, MeshStore>> {
    let a: &'static MmkAssets = unsafe { assets.as_ref() }?;
    Some(a.meshes.lock().unwrap_or_else(|p| p.into_inner()))
}

unsafe fn str_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        set_error("string argument is NULL");
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// Mesh id of a model file (e.g. `3W_Enemy_kuribo`), −1 if the cache has no mesh for it.
///
/// # Safety
/// `assets` must be a live handle and `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_find(assets: *const MmkAssets, name: *const c_char) -> i32 {
    guard(-1, || {
        let (Some(mut s), Some(name)) = (store(assets), unsafe { str_arg(name) }) else {
            return -1;
        };
        s.find(name)
    })
}

/// Number of models in a mesh file (0 for a bad id).
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_model_count(assets: *const MmkAssets, mesh: i32) -> usize {
    guard(0, || {
        store(assets)
            .and_then(|s| s.mesh(mesh).map(|m| m.file.models.len()))
            .unwrap_or(0)
    })
}

/// Describe model `model` of a mesh. Pointers stay valid while the assets handle lives.
///
/// # Safety
/// `assets` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_model(
    assets: *const MmkAssets,
    mesh: i32,
    model: usize,
    out: *mut MmkMeshModel,
) -> bool {
    guard(false, || {
        let (Some(s), Some(out)) = (store(assets), unsafe { out.as_mut() }) else {
            return false;
        };
        let Some(lm) = s.mesh(mesh) else {
            return false;
        };
        let Some(m) = lm.file.models.get(model) else {
            return false;
        };
        *out = MmkMeshModel {
            name: lm.names[model].as_ptr(),
            vertices: m.vertices.as_ptr(),
            vertex_count: m.vertices.len(),
            indices: m.indices.as_ptr(),
            index_count: m.indices.len(),
            bone_count: m.bones.len() as u32,
            shape_count: m.shapes.len() as u32,
            bounds: m.bounds,
        };
        true
    })
}

/// Index of the model named `name` in a mesh file, −1 if none.
///
/// # Safety
/// `assets` must be a live handle and `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_model_index(
    assets: *const MmkAssets,
    mesh: i32,
    name: *const c_char,
) -> i32 {
    guard(-1, || {
        let (Some(s), Some(name)) = (store(assets), unsafe { str_arg(name) }) else {
            return -1;
        };
        s.mesh(mesh)
            .and_then(|m| m.file.models.iter().position(|x| x.name == name))
            .map_or(-1, |i| i as i32)
    })
}

/// Describe shape `shape` of a model.
///
/// # Safety
/// `assets` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_shape(
    assets: *const MmkAssets,
    mesh: i32,
    model: usize,
    shape: usize,
    out: *mut MmkMeshShape,
) -> bool {
    guard(false, || {
        let (Some(s), Some(out)) = (store(assets), unsafe { out.as_mut() }) else {
            return false;
        };
        let Some(m) = s.mesh(mesh).and_then(|m| m.file.models.get(model)) else {
            return false;
        };
        let Some(sh) = m.shapes.get(shape) else {
            return false;
        };
        let mat = m.materials.get(sh.material);
        let wrap = mat.map(|x| x.wrap).unwrap_or([0, 0]);
        *out = MmkMeshShape {
            first_index: sh.first_index,
            index_count: sh.index_count,
            material: sh.material as u32,
            bone: sh.bone as u32,
            flags: (mat.is_some_and(|x| x.unlit) as u32 * MMK_MESH_UNLIT)
                | (wrap[0] as u32 & 3) << 8
                | (wrap[1] as u32 & 3) << 10,
        };
        true
    })
}

/// The mesh's own animation folder plus shared ones (players), see `anim_folders`.
fn folders_for(mesh: &str) -> Vec<String> {
    anim_folders(mesh)
}

/// Animation id for a mesh by name (case-insensitive; the mesh's own animations, then shared
/// player animation files). −1 if none.
///
/// # Safety
/// `assets` must be a live handle and `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_anim(
    assets: *const MmkAssets,
    mesh: i32,
    name: *const c_char,
) -> i32 {
    guard(-1, || {
        let (Some(mut s), Some(name)) = (store(assets), unsafe { str_arg(name) }) else {
            return -1;
        };
        let Some(mname) = s.mesh(mesh).map(|m| m.name.clone()) else {
            return -1;
        };
        s.find_anim(&folders_for(&mname), &[""], &[name])
    })
}

/// Number of animations available to a mesh (own + shared folders); names via
/// `mmk_mesh_anim_name`.
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_anim_count(assets: *const MmkAssets, mesh: i32) -> usize {
    guard(0, || {
        let Some(mut s) = store(assets) else { return 0 };
        let Some(mname) = s.mesh(mesh).map(|m| m.name.clone()) else {
            return 0;
        };
        folders_for(&mname).iter().map(|f| s.index(f).len()).sum()
    })
}

/// Name of animation `index` (see `mmk_mesh_anim_count`) into `buf` (NUL-terminated, truncated
/// to `cap`); returns the full length, 0 if out of range.
///
/// # Safety
/// `assets` must be a live handle; `buf` must hold `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_anim_name(
    assets: *const MmkAssets,
    mesh: i32,
    index: usize,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let Some(mut s) = store(assets) else { return 0 };
        let Some(mname) = s.mesh(mesh).map(|m| m.name.clone()) else {
            return 0;
        };
        let mut i = index;
        for f in folders_for(&mname) {
            let list = s.index(&f);
            if i < list.len() {
                let name = list[i].name.as_bytes();
                if !buf.is_null() && cap > 0 {
                    let n = name.len().min(cap - 1);
                    unsafe {
                        std::ptr::copy_nonoverlapping(name.as_ptr(), buf as *mut u8, n);
                        *buf.add(n) = 0;
                    }
                }
                return name.len();
            }
            i -= list.len();
        }
        0
    })
}

/// Length of an animation in frames (−1 for a bad id).
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_anim_frames(assets: *const MmkAssets, anim: i32) -> f32 {
    guard(-1.0, || {
        store(assets)
            .and_then(|s| s.anim(anim).map(|a| a.frames as f32))
            .unwrap_or(-1.0)
    })
}

/// Pose a model: `anim` −1 = rest pose; `time` = frames since the animation started (looping
/// animations wrap, others hold the last frame); `variant` 1 picks the `.1` textures. Buffers
/// stay valid until the next pose call on this assets handle.
///
/// # Safety
/// `assets` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_pose(
    assets: *const MmkAssets,
    mesh: i32,
    model: usize,
    anim: i32,
    time: f32,
    variant: u32,
    out: *mut MmkMeshPose,
) -> bool {
    guard(false, || {
        let Some(a) = (unsafe { assets.as_ref() }) else {
            return false;
        };
        let (Some(mut s), Some(out)) = (store(assets), unsafe { out.as_mut() }) else {
            return false;
        };
        let Some((bones, shapes)) = s.pose(&a.cache, mesh, model, anim, time, variant) else {
            return false;
        };
        *out = MmkMeshPose {
            skin: s.skin.as_ptr(),
            shape_texture: s.shape_texture.as_ptr(),
            shape_visible: s.shape_visible.as_ptr(),
            bone_count: bones as u32,
            shape_count: shapes as u32,
        };
        true
    })
}

// --- sim sprites → mesh draws ------------------------------------------------------------------

/// Turn toward the walking direction for 3D actors in the side view: 70° from facing the
/// camera [inf: picked to show the face, not measured from the game].
pub const MESH_TURN: f32 = 70.0 * std::f32::consts::PI / 180.0;

/// Frame stem → animation names to try (case-insensitive), after the stem itself.
const ANIM_SYNONYMS: &[(&str, &[&str])] = &[
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
struct Part {
    mesh: i32,
    model: u32,
    anim: i32,
    /// Index of an earlier part and its bone this part rides on.
    attach: Option<(usize, usize)>,
}

/// How a (model, frame) of the sim is drawn as meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPlan {
    parts: Vec<Part>,
    /// Size of the SMB1 texture the sim sized the sprite by (for the scale).
    ref_size: (f32, f32),
    /// The SMB1 art faces right (players) or left (everything else) when not flipped.
    art_faces_right: bool,
    /// Turn toward the moving direction (actors), or mirror (flat cards).
    turn: bool,
    card: bool,
    variant: u32,
}

/// (model file, model name or "" for the first, attach to (part index, bone)).
type PlayerPart = (&'static str, &'static str, Option<(usize, &'static str)>);

/// Player model files and parts per style (forms the styles have no model for use the closest).
fn player_parts(style: &str, model: &str) -> Option<(Vec<PlayerPart>, u32)> {
    let small = model.starts_with("M1_Player_MarioMdl");
    let fire = model.starts_with("M1_Player_FireMarioMdl");
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
fn pick_model(file: &MeshFile, mesh_name: &str, stem: &str) -> usize {
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
        let ref_size = cache
            .texture_index(model, frame)
            .map(|i| {
                let t = &cache.textures[i];
                (t.width as f32, t.height as f32)
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
            (s.w / plan.ref_size.0.max(1.0), s.x + s.w / 2.0, s.y)
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

/// This frame's mesh draws (filled by `mmk_engine_snapshot`; empty for the 2D styles). Each one
/// goes before the snapshot sprite at index `order`. Valid until the next snapshot.
///
/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_meshes(
    engine: *const crate::MmkEngine,
    out: *mut *const MmkMeshDraw,
) -> usize {
    guard(0, || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return 0;
        };
        *out = e.meshes.as_ptr();
        e.meshes.len()
    })
}

/// Draw the NSMBU / 3D World models as meshes (default) or always as sprites.
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_set_meshes(engine: *mut crate::MmkEngine, enabled: bool) {
    guard((), || {
        if let Some(e) = unsafe { engine.as_mut() } {
            e.mesh_mode = enabled;
        }
    })
}

/// Debug / model viewer: the mesh draws the engine would make for a sim sprite of `model` showing
/// `frame` in `style` ("WU" or "3W"), with its origin (feet centre) at (`x`, `y`) world px,
/// `scale` × the SMB1 size, facing right unless `flip_x`, `tick` frames into the animation.
/// Returns the count (0 = drawn as a sprite); valid until the next call.
///
/// # Safety
/// `assets` must be a live handle; strings NUL-terminated; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_mesh_sprite_draws(
    assets: *const MmkAssets,
    style: *const c_char,
    model: *const c_char,
    frame: *const c_char,
    x: f32,
    y: f32,
    scale: f32,
    flip_x: bool,
    tick: f32,
    out: *mut *const MmkMeshDraw,
) -> usize {
    guard(0, || {
        let Some(a) = (unsafe { assets.as_ref() }) else {
            return 0;
        };
        let (Some(style), Some(model), Some(frame), Some(out)) = (
            unsafe { str_arg(style) },
            unsafe { str_arg(model) },
            unsafe { str_arg(frame) },
            unsafe { out.as_mut() },
        ) else {
            return 0;
        };
        let Some(mut s) = store(assets) else { return 0 };
        let Some(plan) = s.plan_model(&a.cache, style, model, frame) else {
            return 0;
        };
        // the sim's names are 'static; this sprite only lives for the call
        let sprite = Sprite {
            image: mmk_sim::render::SpriteImage {
                model: "",
                texture: "",
                rect: None,
            },
            x: x - plan.ref_size.0 * scale / 2.0,
            y,
            w: plan.ref_size.0 * scale,
            h: plan.ref_size.1 * scale,
            // the sim flips the SMB1 art relative to its own facing
            flip_x: if plan.art_faces_right {
                flip_x
            } else {
                !flip_x
            },
            flip_y: false,
            layer: 0,
        };
        let mut draws = std::mem::take(&mut s.preview);
        draws.clear();
        s.emit(&a.cache, &plan, &sprite, false, tick, 0, &mut draws);
        s.preview = draws;
        *out = s.preview.as_ptr();
        s.preview.len()
    })
}

/// Whether a style draws (some of) its objects as meshes.
pub fn mesh_style(style: &str) -> bool {
    matches!(style, "WU" | "3W")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_names_map_to_animations() {
        assert_eq!(anim_candidates("walk.1")[..2], ["walk", "Walk"]);
        assert!(anim_candidates("fall.0").contains(&"jump2"));
        assert_eq!(*anim_candidates("blink.0").last().unwrap(), "Wait");
        assert_eq!(
            anim_folders("WU_Player_MarioMdl"),
            vec![
                "WU_Player_MarioMdl",
                "WU_Player_MarioAnm",
                "WU_Player_CommonAnm"
            ]
        );
        assert_eq!(
            anim_folders("3W_Player_MarioMini")[1..],
            ["3W_Player_Animation_MarioMini", "3W_Player_Animation_Mario"]
        );
        assert_eq!(
            anim_folders("3W_Player_MarioHandL")[1],
            "3W_Player_Animation_HandL"
        );
        assert_eq!(anim_folders("3W_Enemy_kuribo"), vec!["3W_Enemy_kuribo"]);
    }

    #[test]
    fn alternative_parts_hidden_by_default() {
        use mmk_assets::mesh::{MeshBone, MeshModel};
        let bone = |n: &str| MeshBone {
            name: n.into(),
            parent: None,
            scale: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            euler: true,
            translation: [0.0; 3],
            ssc: false,
            visible: true,
            inverse_bind: IDENTITY,
        };
        let f = MeshFile {
            models: vec![MeshModel {
                bones: vec![
                    bone("MarioFace"),
                    bone("Face00"),
                    bone("Face01"),
                    bone("Hand10"),
                ],
                ..Default::default()
            }],
        };
        assert_eq!(default_visible(&f), vec![vec![true, true, false, true]]);
    }

    #[test]
    fn picks_models_of_multi_model_files() {
        use mmk_assets::mesh::MeshModel;
        let f = MeshFile {
            models: ["blockbreak_model", "block_renga_model", "shell_model"]
                .iter()
                .map(|n| MeshModel {
                    name: n.to_string(),
                    ..Default::default()
                })
                .collect(),
        };
        assert_eq!(pick_model(&f, "3W_Object_block_renga", "wait"), 1);
        assert_eq!(pick_model(&f, "WU_Enemy_nokonokoA", "shell"), 2);
        assert_eq!(pick_model(&f, "WU_Enemy_x", "walk"), 1);
    }

    /// The header declares every function of this module.
    #[test]
    fn header_declares_exports() {
        let header = include_str!("../../../../app/Sources/CMmk/mmk.h");
        for line in include_str!("mesh.rs").lines() {
            let Some(rest) = line.strip_prefix("pub unsafe extern \"C\" fn ") else {
                continue;
            };
            let name = rest.split('(').next().unwrap();
            assert!(
                header.contains(&format!(" {name}(")),
                "{name} missing from mmk.h"
            );
        }
    }

    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<MmkMeshVertex>(), 56);
        assert_eq!(size_of::<MmkMeshModel>(), 72);
        assert_eq!(size_of::<MmkMeshShape>(), 20);
        assert_eq!(size_of::<MmkMeshPose>(), 32);
        assert_eq!(size_of::<MmkMeshDraw>(), 80);
    }

    /// The real cache (`MMK_ASSETS`, else ~/smm-work/cache/smm2-3.0.3) if it has meshes.
    fn owner_cache() -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        [
            std::env::var_os("MMK_ASSETS").map(PathBuf::from),
            Some(home.join("smm-work/cache/smm2-3.0.3")),
            Some(home.join("smm-work/cache-mesh/smm2-3.0.3")),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.join("meshes").is_dir())
    }

    #[test]
    fn owner_cache_plans_and_poses() {
        let Some(root) = owner_cache() else {
            eprintln!("skipped: no mesh cache");
            return;
        };
        // only the tables needed here: open the cache's texture index lazily via Cache::open
        let cache = Cache::open(&root).unwrap();
        let mut s = MeshStore::new(root);
        let goomba = s
            .plan_model(&cache, "3W", "M1_Enemy_kuribo", "walk.0")
            .unwrap();
        assert_eq!(goomba.parts.len(), 1);
        let a = s.anim(goomba.parts[0].anim).unwrap();
        assert_eq!(a.name, "Walk");
        let (bones, shapes) = s
            .pose(
                &cache,
                goomba.parts[0].mesh,
                0,
                goomba.parts[0].anim,
                12.0,
                0,
            )
            .unwrap();
        assert_eq!((bones, shapes), (21, 7));
        assert_eq!(s.skin.len(), 21 * 12);
        assert!(s.shape_texture.iter().any(|&t| t != MMK_TEXTURE_NONE));
        // the eyes show one variant at a time
        assert!(s.shape_visible.iter().filter(|&&v| v == 0).count() >= 2);
        let wu = s
            .plan_model(&cache, "WU", "M1_Enemy_kuribo", "walk.1")
            .unwrap();
        assert!(wu.card);
        let mario = s
            .plan_model(&cache, "3W", "M1_Player_SpMarioMdl", "walk.0")
            .unwrap();
        assert_eq!(mario.parts.len(), 5);
        assert!(mario.parts.iter().all(|p| p.anim >= 0), "{mario:?}");
        let small = s
            .plan_model(&cache, "WU", "M1_Player_MarioMdl", "wait.0")
            .unwrap();
        assert_eq!(small.parts[1].attach.map(|a| a.0), Some(0));
        assert!(small.parts[0].anim >= 0);
        let brick = s.plan_tile("3W_Field_plain", 1).unwrap();
        let ground = s.plan_tile("3W_Field_plain", 184).unwrap();
        assert_ne!(brick.parts[0].mesh, ground.parts[0].mesh);
        let mut out = Vec::new();
        let sprite = Sprite {
            image: mmk_sim::render::SpriteImage {
                model: "M1_Player_SpMarioMdl",
                texture: "walk.0",
                rect: None,
            },
            x: 92.0,
            y: 32.0,
            w: 16.0,
            h: 32.0,
            flip_x: false,
            flip_y: false,
            layer: 50,
        };
        s.emit(&cache, &mario, &sprite, false, 30.0, 3, &mut out);
        assert_eq!(out.len(), 5);
        assert_eq!((out[0].transform[3], out[0].transform[7]), (100.0, 32.0));
        assert!(
            out[1].transform[7] > 50.0,
            "head above the feet: {:?}",
            out[1].transform
        );
    }
}
