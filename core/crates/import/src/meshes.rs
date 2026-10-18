//! Mesh stage: the NSMBU (WU) and 3D World (3W) styles' BFRES models → `meshes/<Model>.mmesh`
//! and their animations → `anims/<Model>/<anim>.mskel` (format: `mmk_assets::mesh`).
//!
//! Per model: LOD 0 of every shape, vertices baked into the bind pose in model space (rigid and
//! single-bone shapes are moved there with their bone's rest transform, so every vertex skins the
//! same way), only the vertices LOD 0 uses. Animations: skeletal (FSKA), bone visibility (FBVS)
//! and texture-pattern (FMAA) tracks that share a base name (`walk`, `walk_ftp`) become one
//! animation.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use mmk_assets::mesh::{
    AnimInfo, BoneTrack, MeshBone, MeshFile, MeshMaterial, MeshModel, MeshShape, SkelAnim,
    TexTrack, Vertex, VisTrack, anim_file_name, anim_index_to_json, bfres_rest_world,
    srt_component,
};
use mmk_formats::bfres::{self, Affine, Bfres, PatternTrack, Wrap, apply, invert};

use crate::{Result, write};

/// Styles whose models get meshes.
pub const MESH_STYLES: &[&str] = &["WU", "3W"];

/// Whether a model file gets a mesh: the 3D styles' actors, objects, items, players, 3D World
/// terrain pieces and the background scenes (`_DV_`, with their animated parts `DVObj_`).
/// Shadow/collision helpers (`_S_`), layout and environment-map files and the NSMBU field
/// atlases (drawn as tiles) are skipped.
pub fn wants_mesh(model: &str) -> bool {
    let Some((style, rest)) = model.split_once('_') else {
        return false;
    };
    MESH_STYLES.contains(&style)
        && !rest.starts_with("S_")
        && !rest.starts_with("Lyt_")
        && !rest.starts_with("EnvMap")
        && !rest.starts_with("ModelSpec")
        && !(style == "WU" && rest.starts_with("Field_"))
}

fn rot(m: &Affine, n: [f32; 3]) -> [f32; 3] {
    let v = [0, 1, 2].map(|r| m[r][0] * n[0] + m[r][1] * n[1] + m[r][2] * n[2]);
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        v.map(|x| x / len)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Shader parameters kept in the cache (the uber shader has ~100; these drive our shading).
fn keep_param(name: &str) -> bool {
    name.starts_with("const_color")
        || name.starts_with("tex_srt")
        || name.starts_with("tev_")
        || name.starts_with("edge_light")
        || matches!(
            name,
            "albedo_ratio"
                | "vertex_color_ratio"
                | "emission_intensity"
                | "specular_intensity"
                | "specular_roughness"
                | "material_indirect_scale"
                | "bake_ao_ratio"
                | "gsys_bake_st0"
                | "gsys_bake_st1"
                | "bloom_intensity"
                | "fresnel_coeff"
        )
}

/// Shader options kept: those that differ from the default, without the per-stage "form"
/// combiner settings and game-effect colour slots.
fn keep_option(name: &str, value: &str) -> bool {
    !matches!(value, "0" | "-1" | "")
        && !name.starts_with("form")
        && !name.starts_with("game_effect")
}

/// A BFRES material → cache material (textures by shader sampler, render state, parameters).
pub fn convert_material(mat: &bfres::Material) -> MeshMaterial {
    // the material sampler a shader sampler reads (assignment table, else the same name)
    let tex_of = |shader_sampler: &str| -> Option<(String, [u8; 2])> {
        let ms = mat
            .sampler_assign
            .iter()
            .find(|(s, _)| s == shader_sampler)
            .map(|(_, m)| m.as_str())
            .unwrap_or(shader_sampler);
        let i = mat.samplers.iter().position(|s| s.name == ms)?;
        let w = &mat.samplers[i].wrap;
        Some((
            mat.textures.get(i)?.clone(),
            [wrap_code(w[0]), wrap_code(w[1])],
        ))
    };
    let (albedo, wrap) = match tex_of("_a0") {
        Some((t, w)) => (Some(t), w),
        None => (
            mat.textures.first().cloned(),
            mat.samplers
                .first()
                .map(|s| [wrap_code(s.wrap[0]), wrap_code(s.wrap[1])])
                .unwrap_or([0, 0]),
        ),
    };
    let mut textures = Vec::new();
    if mat.sampler_assign.is_empty() {
        for (i, smp) in mat.samplers.iter().enumerate() {
            if smp.name != "_a0"
                && let Some(t) = mat.textures.get(i)
            {
                textures.push((smp.name.clone(), t.clone()));
            }
        }
    } else {
        for (shader_sampler, _) in &mat.sampler_assign {
            if shader_sampler != "_a0"
                && let Some((t, _)) = tex_of(shader_sampler)
            {
                textures.push((shader_sampler.clone(), t));
            }
        }
    }
    let rs = |k: &str| mat.render_str(k).unwrap_or("");
    let alpha_test = rs("gsys_alpha_test_enable") == "true";
    let additive = rs("gsys_color_blend_rgb_dst_func") == "one";
    let blend = match rs("gsys_render_state_mode") {
        "opaque" => alpha_test as u8,
        "mask" => 1,
        "translucent" => 2 + additive as u8,
        _ if rs("gsys_render_state_blend_mode") == "none" => alpha_test as u8,
        _ => 2 + additive as u8,
    };
    let cull = match rs("gsys_render_state_display_face") {
        "back" => 1,
        "both" => 2,
        "none" => 3,
        _ => 0,
    };
    MeshMaterial {
        name: mat.name.clone(),
        albedo,
        wrap,
        unlit: false,
        textures,
        blend,
        alpha_ref: mat.render_f32("gsys_alpha_test_value").unwrap_or(0.5),
        cull,
        params: mat
            .params
            .iter()
            .filter(|p| {
                keep_param(&p.name) && (12..=15).contains(&p.kind) || p.name.starts_with("tex_srt")
            })
            .map(|p| (p.name.clone(), p.values.clone()))
            .collect(),
        options: mat
            .options
            .iter()
            .filter(|(n, v)| keep_option(n, v))
            .cloned()
            .collect(),
    }
}

fn wrap_code(w: Wrap) -> u8 {
    match w {
        Wrap::Repeat => 0,
        Wrap::Mirror => 1,
        Wrap::Clamp => 2,
        Wrap::Other(_) => 0,
    }
}

/// One BFRES model → engine model.
pub fn convert_model(m: &bfres::Model) -> MeshModel {
    let rest = bfres_rest_world(m);
    let sk = &m.skeleton;
    let bones: Vec<MeshBone> = m
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let stored = usize::try_from(b.smooth_index)
                .ok()
                .and_then(|k| sk.inverse_model.get(k));
            MeshBone {
                name: b.name.clone(),
                parent: b.parent,
                scale: b.scale,
                rotation: b.rotation,
                euler: b.euler(),
                translation: b.translation,
                ssc: b.segment_scale_compensate(),
                visible: b.visible(),
                inverse_bind: stored.copied().unwrap_or_else(|| invert(&rest[i])),
            }
        })
        .collect();
    let mut materials: Vec<MeshMaterial> = m.materials.iter().map(convert_material).collect();
    let palette = |i: f32| -> usize {
        sk.matrix_to_bone
            .get(i as usize)
            .map(|&b| b as usize)
            .unwrap_or(0)
    };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut shapes = Vec::new();
    let mut card_materials = Vec::new();
    // vertex colours / second UVs: kept if any shape has them (others get white / 0)
    let any_color = m.shapes.iter().any(|s| s.attribute("_c0").is_some());
    let any_uv1 = m.shapes.iter().any(|s| s.attribute("_u1").is_some());
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut uv1s: Vec<[f32; 2]> = Vec::new();
    for s in &m.shapes {
        let Some(lod) = s
            .lods
            .first()
            .filter(|l| l.primitive == bfres::PRIMITIVE_TRIANGLES)
        else {
            continue;
        };
        let (Some(pos), nrm, uv) = (s.attribute("_p0"), s.attribute("_n0"), s.attribute("_u0"))
        else {
            continue;
        };
        let idx = s.attribute("_i0");
        let wgt = s.attribute("_w0");
        let col = s.attribute("_c0");
        let uv1 = s.attribute("_u1");
        let first_index = indices.len() as u32;
        let base = vertices.len() as u32;
        let mut remap: HashMap<u32, u32> = HashMap::new();
        for &vi in &lod.indices {
            let next = base + remap.len() as u32;
            let out = *remap.entry(vi).or_insert_with(|| {
                let k = vi as usize;
                let p = pos.values.get(k).copied().unwrap_or_default();
                let n = nrm
                    .and_then(|a| a.values.get(k))
                    .copied()
                    .unwrap_or([0.0, 0.0, 1.0, 0.0]);
                let t = uv
                    .and_then(|a| a.values.get(k))
                    .copied()
                    .unwrap_or_default();
                let (p3, n3) = ([p[0], p[1], p[2]], [n[0], n[1], n[2]]);
                let mut v = Vertex {
                    uv: [t[0], t[1]],
                    ..Default::default()
                };
                let single = match s.skin_count {
                    0 => Some(s.bone),
                    1 => Some(palette(
                        idx.and_then(|a| a.values.get(k)).map_or(0.0, |x| x[0]),
                    )),
                    _ => None,
                };
                match single {
                    Some(b) => {
                        let w = rest.get(b).copied().unwrap_or(bfres::IDENTITY);
                        v.position = apply(&w, p3);
                        v.normal = rot(&w, n3);
                        v.joints = [b as u16, 0, 0, 0];
                        v.weights = [1.0, 0.0, 0.0, 0.0];
                    }
                    None => {
                        v.position = p3;
                        v.normal = rot(&bfres::IDENTITY, n3);
                        let ii = idx
                            .and_then(|a| a.values.get(k))
                            .copied()
                            .unwrap_or_default();
                        let ww = wgt
                            .and_then(|a| a.values.get(k))
                            .copied()
                            .unwrap_or([1.0, 0.0, 0.0, 0.0]);
                        let n = (s.skin_count as usize).min(4);
                        let mut inf: Vec<(u16, f32)> =
                            (0..n).map(|j| (palette(ii[j]) as u16, ww[j])).collect();
                        inf.sort_by(|a, b| b.1.total_cmp(&a.1));
                        let sum: f32 = inf.iter().map(|x| x.1).sum();
                        for (j, (b, w)) in inf.iter().enumerate() {
                            v.joints[j] = *b;
                            v.weights[j] = if sum > 0.0 {
                                w / sum
                            } else if j == 0 {
                                1.0
                            } else {
                                0.0
                            };
                        }
                    }
                }
                vertices.push(v);
                if any_color {
                    let c = col
                        .and_then(|a| a.values.get(k))
                        .copied()
                        .unwrap_or([1.0; 4]);
                    colors.push(c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8));
                }
                if any_uv1 {
                    let t = uv1
                        .and_then(|a| a.values.get(k))
                        .copied()
                        .unwrap_or_default();
                    uv1s.push([t[0], t[1]]);
                }
                next
            });
            indices.push(out);
        }
        let shape_verts = &vertices[base as usize..];
        let (zmin, zmax) = shape_verts
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                (lo.min(v.position[2]), hi.max(v.position[2]))
            });
        if shape_verts.len() <= 8 && zmax - zmin < 1e-3 {
            card_materials.push(s.material);
        }
        shapes.push(MeshShape {
            name: s.name.clone(),
            material: s.material,
            bone: s.bone,
            first_index,
            index_count: indices.len() as u32 - first_index,
        });
    }
    for mi in card_materials {
        if let Some(mat) = materials.get_mut(mi) {
            mat.unlit = true;
        }
    }
    let mut bounds = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
    for v in &vertices {
        for k in 0..3 {
            bounds[k] = bounds[k].min(v.position[k]);
            bounds[k + 3] = bounds[k + 3].max(v.position[k]);
        }
    }
    if vertices.is_empty() {
        bounds = [0.0; 6];
    }
    MeshModel {
        name: m.name.clone(),
        bones,
        materials,
        shapes,
        vertices,
        indices,
        bounds,
        colors,
        uv1: uv1s,
    }
}

/// Every model of a file; None if no model has drawable geometry.
pub fn convert_file(f: &Bfres) -> Option<MeshFile> {
    let models: Vec<MeshModel> = f.models.iter().map(convert_model).collect();
    models
        .iter()
        .any(|m| !m.indices.is_empty())
        .then_some(MeshFile { models })
}

/// Base name shared by an animation's skeletal, visibility and material parts.
fn anim_key(name: &str) -> &str {
    for suffix in ["_ftp", "_fsp", "_fcl", "_fts", "_fvs", "_fsh"] {
        if let Some(s) = name.strip_suffix(suffix) {
            return s;
        }
    }
    name
}

fn entry<'a>(out: &'a mut BTreeMap<String, SkelAnim>, name: &str) -> &'a mut SkelAnim {
    let key = anim_key(name).to_string();
    out.entry(key.clone()).or_insert_with(|| SkelAnim {
        name: key,
        ..Default::default()
    })
}

/// The file's animations, merged by base name.
pub fn convert_anims(f: &Bfres) -> Vec<SkelAnim> {
    let mut out: BTreeMap<String, SkelAnim> = BTreeMap::new();
    for a in &f.skeletal_anims {
        let e = entry(&mut out, &a.name);
        e.frames = e.frames.max(a.frame_count);
        e.looping = a.looping();
        e.euler = a.euler();
        e.bones = a
            .bone_anims
            .iter()
            .map(|b| {
                let mut base = [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
                let mut mask = 0;
                if let Some(v) = b.base_scale {
                    base[..3].copy_from_slice(&v);
                    mask |= 1;
                }
                if let Some(v) = b.base_rotate {
                    base[3..7].copy_from_slice(&v);
                    mask |= 2;
                }
                if let Some(v) = b.base_translate {
                    base[7..].copy_from_slice(&v);
                    mask |= 4;
                }
                let curves = b
                    .curves
                    .iter()
                    .filter_map(|c| {
                        let mut c = c.clone();
                        c.target = srt_component(c.target)? as u32;
                        Some(c)
                    })
                    .collect();
                BoneTrack {
                    bone: b.name.clone(),
                    base,
                    mask,
                    curves,
                }
            })
            .collect();
    }
    for v in &f.visibility_anims {
        let e = entry(&mut out, &v.name);
        if e.bones.is_empty() {
            e.looping = v.looping();
        }
        e.frames = e.frames.max(v.frame_count);
        e.visibility = v
            .bones
            .iter()
            .enumerate()
            .map(|(i, (bone, on))| VisTrack {
                bone: bone.clone(),
                base: *on,
                curve: v.curves.iter().find(|c| c.target as usize == i).cloned(),
            })
            .collect();
    }
    for a in &f.material_anims {
        let mut tracks = Vec::new();
        for t in &a.targets {
            // the albedo slot's pattern (else the first)
            let Some(p) = t
                .patterns
                .iter()
                .find(|p| p.sampler == "_a0")
                .or(t.patterns.first())
            else {
                continue;
            };
            let raw: Vec<(f32, i32)> = match &p.track {
                PatternTrack::Constant(v) => vec![(0.0, *v)],
                PatternTrack::Steps(k) => k.clone(),
                PatternTrack::Unsupported(_) => continue,
            };
            let keys: Vec<(f32, String)> = raw
                .iter()
                .filter_map(|&(fr, v)| {
                    Some((fr, a.textures.get(usize::try_from(v).ok()?)?.clone()))
                })
                .collect();
            if !keys.is_empty() {
                tracks.push(TexTrack {
                    material: t.material.clone(),
                    keys,
                });
            }
        }
        if tracks.is_empty() {
            continue;
        }
        let e = entry(&mut out, &a.name);
        if e.bones.is_empty() && e.visibility.is_empty() {
            e.looping = a.looping;
        }
        e.frames = e.frames.max(a.frame_count);
        e.textures = tracks;
    }
    out.into_values().collect()
}

/// Counts written by `export`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshCounts {
    pub meshes: usize,
    pub anims: usize,
    pub vertices: usize,
}

/// Write the mesh and animations of model file `model` into the cache at `out`.
pub fn export(f: &Bfres, model: &str, out: &Path) -> Result<MeshCounts> {
    let mut counts = MeshCounts::default();
    if let Some(mesh) = convert_file(f) {
        counts.meshes = 1;
        counts.vertices = mesh.models.iter().map(|m| m.vertices.len()).sum();
        write(&out.join(format!("meshes/{model}.mmesh")), &mesh.to_bytes())?;
    }
    let anims = convert_anims(f);
    if !anims.is_empty() {
        let dir = out.join(format!("anims/{model}"));
        let mut index = Vec::new();
        for a in &anims {
            write(
                &dir.join(format!("{}.mskel", anim_file_name(&a.name))),
                &a.to_bytes(),
            )?;
            index.push(AnimInfo {
                name: a.name.clone(),
                frames: a.frames,
                looping: a.looping,
                skeletal: !a.bones.is_empty(),
                visibility: !a.visibility.is_empty(),
                textures: !a.textures.is_empty(),
            });
        }
        write(
            &dir.join("index.json"),
            serde_json::to_string(&anim_index_to_json(&index))
                .unwrap()
                .as_bytes(),
        )?;
        counts.anims = anims.len();
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_assets::mesh::{Binding, Pose};

    /// A skinned two-bone file built in memory (same layout as the formats crate's test).
    fn sample() -> Bfres {
        use mmk_formats::bfres::*;
        let bone = |name: &str, parent: Option<usize>, ty: f32, smooth: i16| Bone {
            name: name.into(),
            parent,
            flags: BONE_VISIBLE | BONE_EULER,
            translation: [0.0, ty, 0.0],
            smooth_index: smooth,
            ..Default::default()
        };
        let attr = |name: &str, values: Vec<[f32; 4]>| Attribute {
            name: name.into(),
            format: 0x1805,
            values,
        };
        let bones = vec![bone("root", None, 0.0, 0), bone("tip", Some(0), 10.0, 1)];
        let mut inverse = vec![IDENTITY, IDENTITY];
        inverse[1][1][3] = -10.0;
        let model = Model {
            name: "arm".into(),
            bones,
            skeleton: Skeleton {
                flags: 0x1200,
                matrix_to_bone: vec![0, 1, 1],
                smooth_count: 2,
                rigid_count: 1,
                inverse_model: inverse,
            },
            materials: vec![Material {
                name: "skin".into(),
                textures: vec!["skin_Alb".into()],
                samplers: vec![Sampler {
                    name: "_a0".into(),
                    wrap: [Wrap::Repeat, Wrap::Clamp, Wrap::Clamp],
                }],
                ..Default::default()
            }],
            shapes: vec![
                // smooth: two vertices half/half, one on the root
                Shape {
                    name: "smooth".into(),
                    skin_count: 2,
                    vertex_count: 4,
                    attributes: vec![
                        attr(
                            "_p0",
                            vec![
                                [0.0, 5.0, 0.0, 1.0],
                                [1.0, 5.0, 0.0, 1.0],
                                [0.0, 0.0, 0.0, 1.0],
                                [9.0; 4],
                            ],
                        ),
                        attr(
                            "_i0",
                            vec![
                                [0.0, 1.0, 0.0, 0.0],
                                [1.0, 0.0, 0.0, 0.0],
                                [0.0, 0.0, 0.0, 0.0],
                                [0.0; 4],
                            ],
                        ),
                        attr(
                            "_w0",
                            vec![
                                [0.5, 0.5, 0.0, 0.0],
                                [0.25, 0.75, 0.0, 0.0],
                                [1.0, 0.0, 0.0, 0.0],
                                [1.0; 4],
                            ],
                        ),
                    ],
                    lods: vec![Lod {
                        primitive: PRIMITIVE_TRIANGLES,
                        indices: vec![0, 1, 2],
                    }],
                    ..Default::default()
                },
                // single-bone: the vertex is stored in "tip" space (palette entry 2 = rigid tip)
                Shape {
                    name: "rigid".into(),
                    skin_count: 1,
                    vertex_count: 3,
                    attributes: vec![
                        attr(
                            "_p0",
                            vec![
                                [0.0, 1.0, 0.0, 1.0],
                                [1.0, 1.0, 0.0, 1.0],
                                [0.0, 2.0, 0.0, 1.0],
                            ],
                        ),
                        attr("_i0", vec![[2.0, 0.0, 0.0, 0.0]; 3]),
                    ],
                    lods: vec![Lod {
                        primitive: PRIMITIVE_TRIANGLES,
                        indices: vec![0, 1, 2],
                    }],
                    ..Default::default()
                },
                // bone-bound flat card
                Shape {
                    name: "card".into(),
                    bone: 1,
                    vertex_count: 4,
                    attributes: vec![attr(
                        "_p0",
                        vec![
                            [-1.0, -1.0, 0.0, 1.0],
                            [1.0, -1.0, 0.0, 1.0],
                            [-1.0, 1.0, 0.0, 1.0],
                            [1.0, 1.0, 0.0, 1.0],
                        ],
                    )],
                    lods: vec![Lod {
                        primitive: PRIMITIVE_TRIANGLES,
                        indices: vec![0, 1, 2, 2, 1, 3],
                    }],
                    ..Default::default()
                },
            ],
        };
        let lift = SkeletalAnim {
            name: "Walk".into(),
            flags: ANIM_EULER | ANIM_LOOPING,
            frame_count: 10,
            bone_anims: vec![BoneAnim {
                name: "tip".into(),
                flags: BONE_ANIM_BASE_TRANSLATE,
                base_scale: None,
                base_rotate: None,
                base_translate: Some([0.0, 10.0, 0.0]),
                curves: vec![Curve {
                    target: 0x14,
                    kind: CurveKind::Linear,
                    start: 0.0,
                    end: 10.0,
                    pre_wrap: CurveWrap::Clamp,
                    post_wrap: CurveWrap::Clamp,
                    delta: 10.0,
                    frames: vec![0.0, 10.0],
                    keys: vec![[10.0, 10.0, 0.0, 0.0], [20.0, 0.0, 0.0, 0.0]],
                }],
            }],
        };
        let tex = MaterialAnim {
            name: "Walk_ftp".into(),
            frame_count: 10,
            looping: true,
            textures: vec!["skin_Alb".into(), "skin_Alb.1".into()],
            targets: vec![MaterialAnimTarget {
                material: "skin".into(),
                patterns: vec![TexturePattern {
                    sampler: "_a0".into(),
                    track: PatternTrack::Steps(vec![(0.0, 0), (5.0, 1)]),
                }],
                param_tracks: 0,
            }],
        };
        Bfres {
            name: "arm".into(),
            models: vec![model],
            material_anims: vec![tex],
            skeletal_anims: vec![lift],
            visibility_anims: vec![],
        }
    }

    #[test]
    fn bakes_vertices_into_the_bind_pose() {
        let f = sample();
        let mesh = convert_file(&f).unwrap();
        let m = &mesh.models[0];
        assert_eq!(m.shapes.len(), 3);
        // only the vertices LOD 0 uses (the smooth shape's fourth vertex is dropped)
        assert_eq!(m.vertices.len(), 3 + 3 + 4);
        assert_eq!(m.shapes[2].first_index, 6);
        let v = &m.vertices;
        assert_eq!(v[0].joints[..2], [0, 1]);
        assert_eq!(v[0].weights[..2], [0.5, 0.5]);
        // heavier influence first
        assert_eq!((v[1].joints[0], v[1].weights[0]), (0, 0.75));
        // rigid: tip space → model space (+10 y), bound fully to tip
        assert_eq!(v[3].position, [0.0, 11.0, 0.0]);
        assert_eq!((v[3].joints[0], v[3].weights[0]), (1, 1.0));
        // bone-bound card: moved by its bone, and unlit
        assert_eq!(v[6].position, [-1.0, 9.0, 0.0]);
        assert!(m.materials[0].unlit);
        assert_eq!(m.materials[0].albedo.as_deref(), Some("skin_Alb"));
        assert_eq!(m.materials[0].wrap, [0, 2]);
        assert_eq!(m.bounds[4], 12.0);
        // stored inverse bind matrices are used
        assert_eq!(m.bones[1].inverse_bind[1][3], -10.0);
    }

    #[test]
    fn merges_animation_parts_and_poses() {
        let f = sample();
        let anims = convert_anims(&f);
        assert_eq!(anims.len(), 1);
        let a = &anims[0];
        assert_eq!((a.name.as_str(), a.frames, a.looping), ("Walk", 10, true));
        assert_eq!(a.bones[0].mask, 4);
        assert_eq!(a.bones[0].curves[0].target, 8); // translate y
        assert_eq!(a.textures[0].keys[1], (5.0, "skin_Alb.1".to_string()));
        let mesh = convert_file(&f).unwrap();
        let m = &mesh.models[0];
        let bind = Binding::new(m, a);
        let mut p = Pose::default();
        m.pose(Some((a, &bind)), 5.0, &mut p);
        // the rigid vertex rides the tip up by 5
        let moved = apply(&p.skin[1], m.vertices[3].position);
        assert!((moved[1] - 16.0).abs() < 1e-5, "{moved:?}");
        assert_eq!(p.textures[0].as_deref(), Some("skin_Alb.1"));
    }

    #[test]
    fn mesh_file_filter() {
        assert!(wants_mesh("3W_Enemy_kuribo"));
        assert!(wants_mesh("WU_Enemy_kuribo"));
        assert!(wants_mesh("3W_Field_plain"));
        assert!(!wants_mesh("WU_Field_plain"));
        assert!(wants_mesh("3W_DV_plain"));
        assert!(wants_mesh("WU_DV_plain_D"));
        assert!(wants_mesh("3W_DVObj_water_FishA"));
        assert!(!wants_mesh("3W_S_DV_plain"));
        assert!(!wants_mesh("M1_Enemy_kuribo"));
        assert!(!wants_mesh("3W_S_Object_halfcol"));
    }

    /// The owner's dump: Goomba / Koopa / Mario / brick convert, skinned vertices stay finite
    /// and the walk animations bind to the skeletons. Skipped without game data.
    #[test]
    fn owner_dump_meshes() {
        let romfs = [
            std::env::var_os("MMK_ROMFS").map(std::path::PathBuf::from),
            Some("/Volumes/SSD/smm-work/extracted/smm2-update-v303/romfs".into()),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.join("Pack/3W_Model.pack").is_file());
        let Some(romfs) = romfs else {
            eprintln!("skipped: no RomFS");
            return;
        };
        let models = crate::collect_models(&romfs).unwrap();
        let get = |name: &str| {
            let (_, data) = models.iter().find(|(n, _)| n == name).unwrap();
            let sarc = mmk_formats::archive::Sarc::parse(
                mmk_formats::archive::unwrap(data.clone()).unwrap(),
            )
            .unwrap();
            let (_, b) = sarc.files().find(|(_, d)| d.starts_with(b"FRES")).unwrap();
            bfres::parse(b).unwrap()
        };
        for (name, anim, min_bound) in [
            ("3W_Enemy_kuribo", "Walk", 10),
            ("3W_Enemy_nokonokoA", "Walk", 10),
            ("3W_Player_Mario", "", 10),
            ("WU_Enemy_kuribo", "walk", 4),
            ("3W_Object_block_renga", "", 1),
            ("WU_Player_MarioMdl", "", 2),
        ] {
            let f = get(name);
            let mesh = convert_file(&f).unwrap();
            let m = &mesh.models[0];
            assert!(
                m.vertices
                    .iter()
                    .all(|v| v.position.iter().all(|x| x.is_finite()))
            );
            assert!(m.bounds[4] - m.bounds[1] > 8.0, "{name}: {:?}", m.bounds);
            let anims = convert_anims(&f);
            if !anim.is_empty() {
                let a = anims.iter().find(|a| a.name == anim).unwrap_or_else(|| {
                    panic!(
                        "{name}: {:?}",
                        anims.iter().map(|a| &a.name).collect::<Vec<_>>()
                    )
                });
                let bind = Binding::new(m, a);
                assert!(
                    bind.bound_bones() >= min_bound || !a.textures.is_empty(),
                    "{name}"
                );
                let mut p = Pose::default();
                m.pose(Some((a, &bind)), 7.0, &mut p);
                assert!(p.skin.iter().flatten().flatten().all(|x| x.is_finite()));
            }
        }
        // the player's animations come from a separate file and bind to its skeleton
        let mario = convert_file(&get("3W_Player_Mario")).unwrap();
        let anims = convert_anims(&get("3W_Player_Animation_Mario"));
        let wait = anims
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("wait"))
            .unwrap();
        assert!(Binding::new(&mario.models[0], wait).bound_bones() > 15);
    }
}
