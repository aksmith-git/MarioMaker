//! C ABI for 3D meshes (NSMBU / 3D World styles): mesh data, animation lookup and posing, and the
//! engine's per-frame mesh draw list. Declared in `app/Sources/CMmk/mmk.h` (section "meshes").
//!
//! Meshes and animations load lazily from the asset cache (`meshes/`, `anims/`) the first time
//! they are asked for and stay loaded with the assets handle, so pointers into mesh data stay
//! valid until `mmk_assets_free`. Pose buffers (`mmk_mesh_pose`) are valid until the next pose
//! call on the same assets handle. Meant for one render thread.
//!
//! The meshes themselves (loading, posing, sprite → mesh plans) live in `mmk_host::mesh`.

use std::ffi::{CStr, c_char};

pub use mmk_host::mesh::*;

use mmk_sim::Sprite;

use crate::{MmkAssets, guard, set_error};

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
            colors: if m.colors.is_empty() {
                std::ptr::null()
            } else {
                m.colors.as_ptr() as *const u8
            },
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
        let look = shape_look(m.materials.get(sh.material));
        *out = MmkMeshShape {
            first_index: sh.first_index,
            index_count: sh.index_count,
            material: sh.material as u32,
            bone: sh.bone as u32,
            flags: look.flags,
            alpha_ref: look.alpha_ref,
            srt: look.srt,
            vertex_color_ratio: look.vertex_color_ratio,
            extra_texture: TEXTURE_NONE,
            emission: look.emission,
            tint: look.tint,
        };
        // the extra texture is resolved against the cache (named in the model file's folder)
        drop(s);
        if look.extra.is_some()
            && let (Some(a), Some(mut s)) = (unsafe { assets.as_ref() }, store(assets))
        {
            out.extra_texture = s.extra_texture(&a.cache, mesh, &look);
        }
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

/// The 3D scene of the current frame (filled by `mmk_engine_snapshot`): matrices column-major,
/// world px → clip with depth 0 … 1; light uniforms as `mmk_host::scene3d::Env::uniform`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkScene3d {
    pub field_view_proj: [f32; 16],
    pub background_view_proj: [f32; 16],
    pub field_light: [f32; 120],
    pub background_light: [f32; 120],
    pub has_background: bool,
}

/// This frame's 3D scene; false for the 2D styles (and without meshes).
///
/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_scene3d(
    engine: *const crate::MmkEngine,
    out: *mut MmkScene3d,
) -> bool {
    guard(false, || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return false;
        };
        let Some(sc) = &e.planner.scene else {
            return false;
        };
        let flat = |rows: &[[f32; 4]; mmk_host::scene3d::LIGHT_ROWS]| {
            let mut v = [0.0f32; 120];
            for (i, r) in rows.iter().enumerate() {
                v[i * 4..i * 4 + 4].copy_from_slice(r);
            }
            v
        };
        *out = MmkScene3d {
            field_view_proj: sc.field.view_proj,
            background_view_proj: sc.background.map_or([0.0; 16], |c| c.view_proj),
            field_light: flat(&sc.field_light),
            background_light: flat(&sc.background_light),
            has_background: sc.background.is_some() && !e.planner.background.is_empty(),
        };
        true
    })
}

/// This frame's background-scene draws (3D styles; drawn with the scene's background camera,
/// behind everything). Valid until the next snapshot.
///
/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_background(
    engine: *const crate::MmkEngine,
    out: *mut *const MmkMeshDraw,
) -> usize {
    guard(0, || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return 0;
        };
        *out = e.planner.background.as_ptr();
        e.planner.background.len()
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::MMK_TEXTURE_NONE;
    use mmk_assets::Cache;
    use mmk_assets::mesh::MeshFile;
    use mmk_formats::bfres::IDENTITY;
    use std::path::PathBuf;

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
        assert_eq!(size_of::<MmkMeshModel>(), 80);
        assert_eq!(size_of::<MmkMeshShape>(), 68);
        assert_eq!(size_of::<MmkScene3d>(), 1092);
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
