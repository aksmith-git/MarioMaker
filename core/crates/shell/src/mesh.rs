//! NSMBU / 3D World models in the portable shell: the frame's mesh draws (`mmk_host::mesh`) are
//! posed and skinned on the CPU into one vertex / index list, so every wgpu backend can draw them
//! (WebGL2 has no storage buffers in vertex shaders for GPU skinning).
//!
//! Depth works as in the Swift app's `MeshRenderer`: consecutive draws of one layer form a depth
//! group with real depth inside it, and each later group sits entirely in front of the earlier
//! ones, so meshes and sprites still stack in layer order. Sprites never write depth.

use std::ops::Range;

use mmk_assets::Cache;
use mmk_host::mesh::{MeshStore, MmkMeshDraw, TEXTURE_NONE};

/// One skinned vertex in world pixels: x, y, depth (0 near … 1 far), uv, normal, flags (1 = unlit
/// card: cut out by alpha, no lighting).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub flags: f32,
}

/// Indices `first..first + count` drawn with one texture (None: plain grey) and wrap mode
/// (bits 0–1 u, 2–3 v: 0 repeat, 1 mirror, 2 clamp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub first: u32,
    pub count: u32,
    pub texture: Option<u32>,
    pub wrap: u32,
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

fn apply(m: &[f32], p: [f32; 3], w: f32) -> [f32; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3] * w,
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7] * w,
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11] * w,
    ]
}

/// Pose and skin `draws` into `out`.
pub fn build(store: &mut MeshStore, cache: &Cache, draws: &[MmkMeshDraw], out: &mut MeshFrame) {
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
        let center = 1.0 - (g as f32 + 1.0) * slice;
        let first_piece = out.pieces.len();
        for (si, sh) in m.shapes.iter().enumerate().take(shapes) {
            if visible.get(si) != Some(&1) || sh.index_count == 0 {
                continue;
            }
            let mat = m.materials.get(sh.material);
            let unlit = mat.is_some_and(|x| x.unlit);
            let wrap = mat.map(|x| x.wrap).unwrap_or([0, 0]);
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
                    remap[i] = out.vertices.len() as u32;
                    out.vertices.push(MeshVertex {
                        pos: [wp[0], wp[1], center - wp[2] * slice / 400.0],
                        uv: v.uv,
                        normal: wn,
                        flags: unlit as u32 as f32,
                    });
                }
                out.indices.push(remap[i]);
            }
            let tex = textures.get(si).copied().unwrap_or(TEXTURE_NONE);
            out.pieces.push(Piece {
                first,
                count: out.indices.len() as u32 - first,
                texture: (tex != TEXTURE_NONE).then_some(tex),
                wrap: (wrap[0] as u32 & 3) | (wrap[1] as u32 & 3) << 2,
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
