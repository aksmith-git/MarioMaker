//! NSMBU / 3D World models in the portable shell: the frame's mesh draws (`mmk_host::mesh`) are
//! posed and skinned on the CPU into one vertex / index list, so every wgpu backend can draw them
//! (WebGL2 has no storage buffers in vertex shaders for GPU skinning).
//!
//! Vertices leave here in clip space through the camera they are drawn with
//! (`mmk_host::scene3d`): the play field's (orthographic, or 3D World's slight perspective) or
//! the background scene's. Play-field depth works as in the Swift app's `MeshRenderer`:
//! consecutive draws of one layer form a depth group with real depth inside it, and each later
//! group sits entirely in front of the earlier ones, so meshes and sprites still stack in layer
//! order. Sprites never write depth. The background scene keeps its own depth (it is drawn in a
//! pass of its own before everything else).

use std::ops::Range;

use mmk_assets::Cache;
use mmk_host::mesh::{
    MMK_MESH_ALPHA_TEST, MMK_MESH_FOG, MMK_MESH_FOG_Y, MMK_MESH_FOG1, MMK_MESH_SELF_LIT,
    MMK_MESH_UNLIT, MeshStore, MmkMeshDraw, TEXTURE_NONE, apply_srt, shape_look,
};
use mmk_host::scene3d::{Mat4, project};

/// Vertex flags.
pub const FLAG_UNLIT: u32 = 1;
pub const FLAG_FOG: u32 = 2;
/// Pre-rendered sprite card: cut out by alpha, no lighting.
pub const FLAG_CARD: u32 = 4;
/// Alpha-tested material (`alpha_ref`).
pub const FLAG_ALPHA_TEST: u32 = 8;
pub const FLAG_FOG1: u32 = 16;
pub const FLAG_FOG_Y: u32 = 32;
/// Bits 6–7: the piece's extra texture (1 emission, 2 alpha mask, 3 sky base).
pub const FLAG_EXTRA_SHIFT: u32 = 6;

/// One skinned vertex: clip position, world position (lighting, fog), normal, texture
/// coordinates (texture SRT applied), vertex colour, flags, alpha-test reference and emission
/// strength (`pad[0]`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub clip: [f32; 4],
    pub world: [f32; 3],
    pub flags: f32,
    pub normal: [f32; 3],
    pub alpha_ref: f32,
    pub uv: [f32; 2],
    pub pad: [f32; 2],
    pub color: [f32; 4],
}

/// Indices `first..first + count` drawn with one texture (None: plain grey), wrap mode (bits
/// 0–1 u, 2–3 v: 0 repeat, 1 mirror, 2 clamp), blending (0 opaque / alpha-tested, 2 alpha
/// blended, 3 additive) and face culling (0 back faces culled, 1 front, 2 none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub first: u32,
    pub count: u32,
    pub texture: Option<u32>,
    /// The extra texture (emission, mask or sky base; see `FLAG_EXTRA_SHIFT`).
    pub extra: Option<u32>,
    pub wrap: u32,
    pub blend: u8,
    pub cull: u8,
}

/// One frame's meshes: pieces grouped by the sprite index they're drawn before.
#[derive(Clone, Debug, Default)]
pub struct MeshFrame {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub pieces: Vec<Piece>,
    /// (order: number of sprites drawn before, pieces).
    pub groups: Vec<(u32, Range<usize>)>,
}

impl MeshFrame {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.pieces.clear();
        self.groups.clear();
    }
}

/// How the draws are projected.
#[derive(Clone, Copy, Debug)]
pub struct Projection<'a> {
    pub view_proj: &'a Mat4,
    /// Play field: depth squeezed into per-layer slices (see the module docs).
    pub layered: bool,
    /// Underground night: the course is drawn upside down.
    pub flip_y: bool,
}

fn apply(m: &[f32], p: [f32; 3], w: f32) -> [f32; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3] * w,
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7] * w,
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11] * w,
    ]
}

/// Pose and skin `draws` into `out`, projected through `proj`.
pub fn build(
    store: &mut MeshStore,
    cache: &Cache,
    draws: &[MmkMeshDraw],
    out: &mut MeshFrame,
    proj: &Projection,
) {
    out.clear();
    // depth groups: a new one whenever the layer changes
    let mut group_of = Vec::with_capacity(draws.len());
    let mut groups = 0usize;
    let mut last = None;
    for d in draws {
        if last != Some(d.layer) {
            groups += 1;
            last = Some(d.layer);
        }
        group_of.push(groups - 1);
    }
    let slice = 1.0 / (groups as f32 + 1.0);
    let flip = if proj.flip_y { -1.0 } else { 1.0 };
    let mut remap: Vec<u32> = Vec::new();
    for (d, &g) in draws.iter().zip(&group_of) {
        let Some((bones, shapes)) =
            store.pose(cache, d.mesh, d.model as usize, d.anim, d.time, d.variant)
        else {
            continue;
        };
        let skin = store.skin.clone();
        let textures = store.shape_texture.clone();
        let visible = store.shape_visible.clone();
        let mut extras = std::collections::HashMap::new();
        if let Some(m) = store
            .mesh(d.mesh)
            .and_then(|lm| lm.file.models.get(d.model as usize))
        {
            let looks: Vec<(usize, mmk_host::mesh::ShapeLook)> = m
                .materials
                .iter()
                .enumerate()
                .map(|(i, mat)| (i, shape_look(Some(mat))))
                .filter(|(_, l)| l.extra.is_some())
                .collect();
            for (i, l) in looks {
                extras.insert(i, store.extra_texture(cache, d.mesh, &l));
            }
        }
        let Some(m) = store
            .mesh(d.mesh)
            .and_then(|lm| lm.file.models.get(d.model as usize))
        else {
            continue;
        };
        if m.bones.len() != bones || skin.len() < bones * 12 {
            continue;
        }
        let t = &d.transform;
        // a mirroring transform turns the faces' winding around
        let det = t[0] * (t[5] * t[10] - t[6] * t[9]) - t[1] * (t[4] * t[10] - t[6] * t[8])
            + t[2] * (t[4] * t[9] - t[5] * t[8]);
        let mirrored = (det < 0.0) != proj.flip_y;
        let center = 1.0 - (g as f32 + 1.0) * slice;
        let first_piece = out.pieces.len();
        for (si, sh) in m.shapes.iter().enumerate().take(shapes) {
            if visible.get(si) != Some(&1) || sh.index_count == 0 {
                continue;
            }
            let look = shape_look(m.materials.get(sh.material));
            let extra = match look.extra {
                Some(_) => extras.get(&sh.material).copied().unwrap_or(TEXTURE_NONE),
                None => TEXTURE_NONE,
            };
            if look.faces() == 3 {
                continue; // drawn with no faces (helper geometry)
            }
            let card = look.flags & MMK_MESH_UNLIT != 0
                || d.flags & mmk_host::mesh::MMK_MESH_DRAW_CARD != 0;
            let unlit = card || look.flags & MMK_MESH_SELF_LIT != 0;
            let flags = (unlit as u32 * FLAG_UNLIT)
                | ((look.flags & MMK_MESH_FOG != 0) as u32 * FLAG_FOG)
                | (card as u32 * FLAG_CARD)
                | ((look.flags & MMK_MESH_ALPHA_TEST != 0) as u32 * FLAG_ALPHA_TEST)
                | ((look.flags & MMK_MESH_FOG1 != 0) as u32 * FLAG_FOG1)
                | ((look.flags & MMK_MESH_FOG_Y != 0) as u32 * FLAG_FOG_Y)
                | if extra != TEXTURE_NONE {
                    look.extra_mode() << FLAG_EXTRA_SHIFT
                } else {
                    0
                };
            let alpha_ref = look.alpha_ref;
            let vc_ratio = look.vertex_color_ratio;
            remap.clear();
            remap.resize(m.vertices.len(), u32::MAX);
            let first = out.indices.len() as u32;
            let range = sh.first_index as usize..(sh.first_index + sh.index_count) as usize;
            for &i in m.indices.get(range).unwrap_or(&[]) {
                let i = i as usize;
                if remap[i] == u32::MAX {
                    let v = &m.vertices[i];
                    let (mut p, mut n) = ([0.0f32; 3], [0.0f32; 3]);
                    let mut any = false;
                    for k in 0..4 {
                        let w = v.weights[k];
                        if w <= 0.0 {
                            continue;
                        }
                        let b = v.joints[k] as usize * 12;
                        let Some(s) = skin.get(b..b + 12) else {
                            continue;
                        };
                        let q = apply(s, v.position, 1.0);
                        let r = apply(s, v.normal, 0.0);
                        for c in 0..3 {
                            p[c] += w * q[c];
                            n[c] += w * r[c];
                        }
                        any = true;
                    }
                    if !any {
                        (p, n) = (v.position, v.normal);
                    }
                    let wp = apply(t, p, 1.0);
                    let wn = apply(t, n, 0.0);
                    let mut clip = project(proj.view_proj, wp);
                    if proj.layered && clip[3].abs() > 1e-6 {
                        let zn = (clip[2] / clip[3]).clamp(0.0, 1.0);
                        clip[2] = (center + (zn - 0.5) * slice * 0.98) * clip[3];
                    }
                    clip[1] *= flip;
                    let c = m.colors.get(i).copied().unwrap_or([255; 4]);
                    let lerp = |x: u8| 1.0 + (x as f32 / 255.0 - 1.0) * vc_ratio;
                    remap[i] = out.vertices.len() as u32;
                    out.vertices.push(MeshVertex {
                        clip,
                        world: wp,
                        flags: flags as f32,
                        normal: wn,
                        alpha_ref,
                        uv: apply_srt(
                            if look.flags & mmk_host::mesh::MMK_MESH_WORLD_UV != 0 {
                                let k = 1.0 / mmk_host::mesh::WORLD_UV_SPAN;
                                [wp[0] * k, -wp[1] * k]
                            } else {
                                v.uv
                            },
                            &look.srt,
                        ),
                        pad: [look.emission, 0.0],
                        color: [
                            lerp(c[0]) * look.tint[0],
                            lerp(c[1]) * look.tint[1],
                            lerp(c[2]) * look.tint[2],
                            c[3] as f32 / 255.0,
                        ],
                    });
                }
                out.indices.push(remap[i]);
            }
            let tex = textures.get(si).copied().unwrap_or(TEXTURE_NONE);
            // mirrored models / courses (upside down) see the other side of every face
            let cull = match (look.faces(), mirrored) {
                (0, false) | (1, true) => 0,
                (1, false) | (0, true) => 1,
                _ => 2,
            };
            out.pieces.push(Piece {
                first,
                count: out.indices.len() as u32 - first,
                texture: (tex != TEXTURE_NONE).then_some(tex),
                extra: (extra != TEXTURE_NONE).then_some(extra),
                wrap: (look.flags >> 8) & 0xF,
                blend: match look.blend() {
                    1 => 2,
                    2 => 3,
                    _ => 0,
                },
                // cards are drawn from both sides
                cull: if card { 2 } else { cull },
            });
        }
        if out.pieces.len() > first_piece {
            match out.groups.last_mut() {
                Some((order, r)) if *order == d.order && r.end == first_piece => {
                    r.end = out.pieces.len()
                }
                _ => out.groups.push((d.order, first_piece..out.pieces.len())),
            }
        }
    }
}
