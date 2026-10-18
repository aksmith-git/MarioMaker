//! Sprite placement and frame timing for the 2D styles, from each model's BFRES.
//!
//! The 2D styles draw every animation frame as its own texture on a quad mesh. For each texture
//! this stage finds the quad it is drawn on (model space, rest pose; 1 unit = 1 px, origin = the
//! actor's origin, feet centre for walkers) and the texture coordinates at the quad's corners.
//! For each texture-pattern animation it records the frame keys (texture switches).
//!
//! Which quad a texture belongs to: the shape whose material shows it by default; otherwise a
//! shape whose material a texture-pattern animation targets (several models in one file can share
//! a material name, e.g. a Koopa's walk and shell models: the quad with the texture's own size
//! wins); a file with a single shape takes all remaining textures.

use std::collections::{BTreeMap, HashMap};

use mmk_formats::bfres::{Bfres, PatternTrack, Wrap};
use serde_json::{Value, json};

/// Placement of one texture.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteQuad {
    /// Model-space rectangle (x0, y0) bottom-left … (x1, y1) top-right, y up.
    pub rect: [f32; 4],
    /// Texture coordinates at the corners (u at x0, v at y1 = top, u at x1, v at y0 = bottom),
    /// already folded into 0…1 for mirrored/repeated samplers. None if the texture is tiled
    /// across the quad or not mapped linearly.
    pub uv: Option<[f32; 4]>,
    pub z: f32,
    pub model: String,
    pub shape: String,
    pub bone: String,
    /// `material`, `anim` or `only-shape`: how the texture was matched to the quad.
    pub source: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteAnim {
    pub frames: u32,
    pub looping: bool,
    pub material: String,
    /// (frame, texture) in frame order; the texture holds until the next key.
    pub keys: Vec<(u32, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelSprites {
    pub quads: BTreeMap<String, SpriteQuad>,
    pub anims: BTreeMap<String, SpriteAnim>,
}

struct Candidate {
    material: String,
    default_texture: Option<String>,
    quad: SpriteQuad,
}

/// Linear fit t = a·x + b through the extreme vertices; None if the vertices disagree.
fn fit(xs: &[f32], ts: &[f32]) -> Option<(f32, f32)> {
    let (imin, imax) = (0..xs.len()).fold((0, 0), |(lo, hi), i| {
        (
            if xs[i] < xs[lo] { i } else { lo },
            if xs[i] > xs[hi] { i } else { hi },
        )
    });
    let dx = xs[imax] - xs[imin];
    if dx.abs() < 1e-6 {
        return None;
    }
    let a = (ts[imax] - ts[imin]) / dx;
    let b = ts[imin] - a * xs[imin];
    xs.iter()
        .zip(ts)
        .all(|(x, t)| (a * x + b - t).abs() < 0.02)
        .then_some((a, b))
}

/// Map the texture-coordinate interval (t0, t1) into 0…1 for the sampler's wrap mode.
fn fold(t0: f32, t1: f32, wrap: Wrap) -> Option<(f32, f32)> {
    const EPS: f32 = 1e-3;
    let lo = t0.min(t1);
    let hi = t0.max(t1);
    let k = (lo + EPS).floor();
    if hi > k + 1.0 + EPS {
        return None; // repeats across the quad
    }
    Some(match wrap {
        Wrap::Mirror if (k as i64).rem_euclid(2) == 1 => (1.0 - (t0 - k), 1.0 - (t1 - k)),
        Wrap::Clamp => (t0.clamp(0.0, 1.0), t1.clamp(0.0, 1.0)),
        _ => (t0 - k, t1 - k),
    })
}

fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn candidates(bfres: &Bfres) -> Vec<Candidate> {
    let mut out = Vec::new();
    for m in &bfres.models {
        for s in &m.shapes {
            let pos = m.shape_positions(s);
            let Some(uv) = s.attribute("_u0") else {
                continue;
            };
            if pos.is_empty() || uv.values.len() != pos.len() {
                continue;
            }
            let xs: Vec<f32> = pos.iter().map(|p| p[0]).collect();
            let ys: Vec<f32> = pos.iter().map(|p| p[1]).collect();
            let us: Vec<f32> = uv.values.iter().map(|v| v[0]).collect();
            let vs: Vec<f32> = uv.values.iter().map(|v| v[1]).collect();
            let min = |v: &[f32]| v.iter().copied().fold(f32::INFINITY, f32::min);
            let max = |v: &[f32]| v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let rect = [min(&xs), min(&ys), max(&xs), max(&ys)];
            if rect[2] - rect[0] < 1e-3 || rect[3] - rect[1] < 1e-3 {
                continue; // not facing the camera
            }
            let mat = &m.materials[s.material];
            let slot = mat
                .samplers
                .iter()
                .position(|x| x.name == "_a0")
                .unwrap_or(0);
            let wrap = mat
                .samplers
                .get(slot)
                .map(|x| x.wrap)
                .unwrap_or([Wrap::Clamp; 3]);
            let uv = fit(&xs, &us)
                .zip(fit(&ys, &vs))
                .and_then(|((au, bu), (av, bv))| {
                    let (u0, u1) = fold(au * rect[0] + bu, au * rect[2] + bu, wrap[0])?;
                    let (v0, v1) = fold(av * rect[3] + bv, av * rect[1] + bv, wrap[1])?;
                    Some([u0, v0, u1, v1].map(round3))
                });
            let z = pos.iter().map(|p| p[2]).sum::<f32>() / pos.len() as f32;
            out.push(Candidate {
                material: mat.name.clone(),
                default_texture: mat.textures.get(slot).cloned(),
                quad: SpriteQuad {
                    rect: rect.map(round3),
                    uv,
                    z: round3(z),
                    model: m.name.clone(),
                    shape: s.name.clone(),
                    bone: m
                        .bones
                        .get(s.bone)
                        .map(|b| b.name.clone())
                        .unwrap_or_default(),
                    source: "material",
                },
            });
        }
    }
    out
}

/// Sprite data of one model file. `sizes`: texture name → (width, height) of the textures
/// embedded in the same file.
pub fn model_sprites(bfres: &Bfres, sizes: &HashMap<String, (u32, u32)>) -> ModelSprites {
    let cands = candidates(bfres);
    let size_matches = |c: &Candidate, tex: &str| {
        sizes.get(tex).is_some_and(|&(w, h)| {
            let r = c.quad.rect;
            ((r[2] - r[0]) - w as f32).abs() < 0.5 && ((r[3] - r[1]) - h as f32).abs() < 0.5
        })
    };
    // pick among candidate indices: own size first, then the first one
    let pick = |idx: &[usize], tex: &str| {
        idx.iter()
            .copied()
            .find(|&i| size_matches(&cands[i], tex))
            .or(idx.first().copied())
    };

    let mut out = ModelSprites::default();
    // textures shown by default
    let mut by_default: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, c) in cands.iter().enumerate() {
        if let Some(t) = &c.default_texture {
            by_default.entry(t.clone()).or_default().push(i);
        }
    }
    for (tex, idx) in &by_default {
        if let Some(i) = pick(idx, tex) {
            out.quads.insert(tex.clone(), cands[i].quad.clone());
        }
    }

    // animations, and the textures they switch to
    let mut by_anim: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for a in &bfres.material_anims {
        let Some((material, pattern)) = a.first_pattern() else {
            continue;
        };
        let raw: Vec<(f32, i32)> = match &pattern.track {
            PatternTrack::Constant(v) => vec![(0.0, *v)],
            PatternTrack::Steps(k) => k.clone(),
            PatternTrack::Unsupported(_) => continue,
        };
        let keys: Vec<(u32, String)> = raw
            .iter()
            .filter_map(|&(f, v)| {
                let t = a.textures.get(usize::try_from(v).ok()?)?;
                Some((f.max(0.0).round() as u32, t.clone()))
            })
            .collect();
        for t in a.targets.iter() {
            for p in &t.patterns {
                let used: Vec<i32> = match &p.track {
                    PatternTrack::Constant(v) => vec![*v],
                    PatternTrack::Steps(k) => k.iter().map(|x| x.1).collect(),
                    PatternTrack::Unsupported(_) => vec![],
                };
                for v in used {
                    let Some(tex) = usize::try_from(v).ok().and_then(|v| a.textures.get(v)) else {
                        continue;
                    };
                    let e = by_anim.entry(tex.clone()).or_default();
                    for (i, c) in cands.iter().enumerate() {
                        if c.material == t.material && !e.contains(&i) {
                            e.push(i);
                        }
                    }
                }
            }
        }
        out.anims.insert(
            a.name.clone(),
            SpriteAnim {
                frames: a.frame_count,
                looping: a.looping,
                material: material.to_string(),
                keys,
            },
        );
    }
    for (tex, idx) in &by_anim {
        if out.quads.contains_key(tex) {
            continue;
        }
        if let Some(i) = pick(idx, tex) {
            let mut q = cands[i].quad.clone();
            q.source = "anim";
            out.quads.insert(tex.clone(), q);
        }
    }

    // a single-shape file draws everything on that shape
    if cands.len() == 1 {
        for tex in sizes.keys() {
            if !out.quads.contains_key(tex) {
                let mut q = cands[0].quad.clone();
                q.source = "only-shape";
                out.quads.insert(tex.clone(), q);
            }
        }
    }
    out
}

impl ModelSprites {
    pub fn to_json(&self) -> Value {
        let quads: serde_json::Map<String, Value> = self
            .quads
            .iter()
            .map(|(t, q)| {
                (
                    t.clone(),
                    json!({
                        "rect": q.rect,
                        "uv": q.uv,
                        "z": q.z,
                        "model": q.model,
                        "shape": q.shape,
                        "bone": q.bone,
                        "source": q.source,
                    }),
                )
            })
            .collect();
        let anims: serde_json::Map<String, Value> = self
            .anims
            .iter()
            .map(|(n, a)| {
                (
                    n.clone(),
                    json!({
                        "frames": a.frames,
                        "loop": a.looping,
                        "material": a.material,
                        "keys": a.keys.iter().map(|(f, t)| json!([f, t])).collect::<Vec<_>>(),
                    }),
                )
            })
            .collect();
        json!({ "quads": quads, "anims": anims })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_formats::bfres::{
        Attribute, Bone, MaterialAnim, MaterialAnimTarget, Model, Sampler, Shape, TexturePattern,
    };

    fn bone(name: &str, y: f32) -> Bone {
        Bone {
            name: name.into(),
            parent: None,
            flags: mmk_formats::bfres::BONE_EULER,
            scale: [1.0; 3],
            rotation: [0.0; 4],
            translation: [0.0, y, 0.0],
            ..Default::default()
        }
    }

    fn quad_shape(name: &str, material: usize, rect: [f32; 4], uv: [f32; 4]) -> Shape {
        let [x0, y0, x1, y1] = rect;
        let [u0, v_top, u1, v_bottom] = uv;
        Shape {
            name: name.into(),
            material,
            bone: 0,
            skin_count: 0,
            vertex_count: 4,
            attributes: vec![
                Attribute {
                    name: "_p0".into(),
                    format: 0x1505,
                    values: vec![
                        [x0, y0, 0.0, 1.0],
                        [x1, y0, 0.0, 1.0],
                        [x0, y1, 0.0, 1.0],
                        [x1, y1, 0.0, 1.0],
                    ],
                },
                Attribute {
                    name: "_u0".into(),
                    format: 0x1205,
                    values: vec![
                        [u0, v_bottom, 0.0, 0.0],
                        [u1, v_bottom, 0.0, 0.0],
                        [u0, v_top, 0.0, 0.0],
                        [u1, v_top, 0.0, 0.0],
                    ],
                },
            ],
            ..Default::default()
        }
    }

    fn material(name: &str, tex: &str, wrap: Wrap) -> mmk_formats::bfres::Material {
        mmk_formats::bfres::Material {
            name: name.into(),
            textures: vec![tex.into()],
            samplers: vec![Sampler {
                name: "_a0".into(),
                wrap: [wrap, wrap, Wrap::Clamp],
            }],
            ..Default::default()
        }
    }

    fn steps(
        name: &str,
        frames: u32,
        material: &str,
        keys: &[(f32, i32)],
        tex: &[&str],
    ) -> MaterialAnim {
        MaterialAnim {
            name: name.into(),
            frame_count: frames,
            looping: true,
            textures: tex.iter().map(|t| t.to_string()).collect(),
            targets: vec![MaterialAnimTarget {
                material: material.into(),
                patterns: vec![TexturePattern {
                    sampler: "_a0".into(),
                    track: PatternTrack::Steps(keys.to_vec()),
                }],
                param_tracks: 0,
            }],
        }
    }

    /// Koopa-like file: a 16×32 walk model and a 16×16 shell model sharing the material name.
    fn koopa() -> Bfres {
        let walk = Model {
            name: "walk_model".into(),
            bones: vec![bone("body", -1.0)],
            materials: vec![material("M_body", "walk.0", Wrap::Clamp)],
            shapes: vec![quad_shape(
                "body",
                0,
                [-8.0, 0.0, 8.0, 32.0],
                [0.0, 0.0, 1.0, 1.0],
            )],
            ..Default::default()
        };
        let shell = Model {
            name: "shell_model".into(),
            bones: vec![bone("body", -1.0)],
            materials: vec![material("M_body", "shell.0", Wrap::Clamp)],
            shapes: vec![quad_shape(
                "body",
                0,
                [-8.0, 0.0, 8.0, 16.0],
                [0.0, 0.0, 1.0, 1.0],
            )],
            ..Default::default()
        };
        Bfres {
            name: "output".into(),
            models: vec![walk, shell],
            material_anims: vec![
                steps(
                    "walk_ftp",
                    20,
                    "M_body",
                    &[(0.0, 0), (10.0, 1)],
                    &["walk.0", "walk.1"],
                ),
                steps(
                    "shell_ftp",
                    16,
                    "M_body",
                    &[(0.0, 0), (4.0, 1)],
                    &["shell.0", "shell.1"],
                ),
            ],
            ..Default::default()
        }
    }

    fn sizes(list: &[(&str, u32, u32)]) -> HashMap<String, (u32, u32)> {
        list.iter()
            .map(|&(n, w, h)| (n.to_string(), (w, h)))
            .collect()
    }

    #[test]
    fn quads_follow_bones_and_materials() {
        let s = model_sprites(
            &koopa(),
            &sizes(&[
                ("walk.0", 16, 32),
                ("walk.1", 16, 32),
                ("shell.0", 16, 16),
                ("shell.1", 16, 16),
            ]),
        );
        let w = &s.quads["walk.0"];
        assert_eq!(w.rect, [-8.0, -1.0, 8.0, 31.0]); // bone moves the quad 1 px down
        assert_eq!(w.uv, Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!((w.model.as_str(), w.source), ("walk_model", "material"));
        // animated textures go to the quad of their own size
        assert_eq!(s.quads["walk.1"].model, "walk_model");
        assert_eq!(s.quads["walk.1"].source, "anim");
        assert_eq!(s.quads["shell.1"].model, "shell_model");
        assert_eq!(s.quads["shell.1"].rect, [-8.0, -1.0, 8.0, 15.0]);
        let a = &s.anims["walk_ftp"];
        assert_eq!(a.frames, 20);
        assert!(a.looping);
        assert_eq!(a.keys, vec![(0, "walk.0".into()), (10, "walk.1".into())]);
    }

    #[test]
    fn mirrored_sampler_flips_coordinates() {
        let f = Bfres {
            name: "output".into(),
            models: vec![Model {
                name: "flag_model".into(),
                bones: vec![bone("flag", 0.0)],
                materials: vec![material("flag1", "flag", Wrap::Mirror)],
                shapes: vec![quad_shape(
                    "flag",
                    0,
                    [-8.0, 0.0, 8.0, 16.0],
                    [1.0, 0.0, 2.0, 1.0],
                )],
                ..Default::default()
            }],
            material_anims: vec![],
            ..Default::default()
        };
        let s = model_sprites(&f, &sizes(&[("flag", 16, 16), ("other", 16, 16)]));
        assert_eq!(s.quads["flag"].uv, Some([1.0, 0.0, 0.0, 1.0]));
        // the only shape takes unreferenced textures too
        assert_eq!(s.quads["other"].source, "only-shape");
    }

    /// Checks against the owner's own dump (`MMK_ROMFS`, default ~/smm-work/extracted/
    /// smm2-v303-romfs). Skipped when it is not there (CI has no game data).
    #[test]
    fn owner_dump_m1_sprites() {
        let romfs = std::env::var_os("MMK_ROMFS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join("smm-work/extracted/smm2-v303-romfs")
            });
        if !romfs.join("Pack/M1_Model.pack").is_file() {
            eprintln!("skipped: no RomFS at {}", romfs.display());
            return;
        }
        let want = [
            "M1_Enemy_kuribo",
            "M1_Enemy_nokonokoA",
            "M1_Player_MarioMdl",
            "M1_Object_goalpole",
            "M1_Object_tower",
        ];
        let mut got = HashMap::new();
        for (name, data) in crate::collect_models(&romfs).unwrap() {
            if !want.contains(&name.as_str()) {
                continue;
            }
            let sarc =
                mmk_formats::archive::Sarc::parse(mmk_formats::archive::unwrap(data).unwrap())
                    .unwrap();
            let (_, bfres) = sarc.files().find(|(_, d)| d.starts_with(b"FRES")).unwrap();
            let f = mmk_formats::bfres::parse(bfres).unwrap();
            let sizes = mmk_formats::bntx::parse(mmk_formats::bntx::find_embedded(bfres).unwrap())
                .unwrap()
                .into_iter()
                .map(|t| (t.name, (t.width as u32, t.height as u32)))
                .collect();
            got.insert(name, model_sprites(&f, &sizes));
        }
        let goomba = &got["M1_Enemy_kuribo"];
        assert_eq!(goomba.quads["walk.0"].rect, [-8.0, -16.0, 8.0, 16.0]);
        assert_eq!(goomba.quads["walk.1"].rect, [-8.0, -16.0, 8.0, 16.0]);
        assert_eq!(
            goomba.anims["walk_ftp"].keys,
            vec![(0, "walk.0".to_string()), (10, "walk.1".to_string())]
        );
        assert_eq!(goomba.anims["walk_ftp"].frames, 20);
        let koopa = &got["M1_Enemy_nokonokoA"];
        assert_eq!(koopa.quads["walk.1"].rect, [-8.0, -1.0, 8.0, 31.0]);
        assert_eq!(koopa.quads["shell.2"].rect, [-8.0, -1.0, 8.0, 15.0]);
        assert_eq!(
            got["M1_Player_MarioMdl"].quads["walk.2"].rect,
            [-8.0, 0.0, 8.0, 16.0]
        );
        let goal = &got["M1_Object_goalpole"];
        assert_eq!(goal.quads["goalpoal"].rect, [-8.0, 0.0, 8.0, 176.0]);
        let flag_uv = goal.quads["flag"].uv.unwrap();
        assert!(
            flag_uv[0] > flag_uv[2],
            "goal flag is mirrored: {flag_uv:?}"
        );
        let tower = &got["M1_Object_tower"];
        assert_eq!(tower.quads["tower"].rect, [-40.0, 0.0, 40.0, 80.0]);
        assert_eq!(tower.quads["flag"].rect, [-8.0, 80.0, 8.0, 96.0]);
    }

    #[test]
    fn tiled_or_nonlinear_mapping_has_no_uv() {
        assert_eq!(fold(0.0, 3.0, Wrap::Repeat), None);
        assert_eq!(fold(2.0, 3.0, Wrap::Repeat), Some((0.0, 1.0)));
        assert_eq!(fold(-0.5, 0.5, Wrap::Repeat), None);
        assert_eq!(fit(&[0.0, 1.0, 2.0], &[0.0, 0.5, 0.0]), None);
        // piecewise mesh with a linear mapping (goal pole: base, pole, ball)
        assert!(fit(&[0.0, 16.0, 160.0, 176.0], &[1.0, 0.909, 0.091, 0.0]).is_some());
    }
}
