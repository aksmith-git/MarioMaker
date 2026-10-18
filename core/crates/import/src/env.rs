//! Environment stage: the per-theme lights and fogs of the 3D styles (`Env/Cmn.szs` for the
//! play field, `Env/Cmn_DV.szs` for the background scenes) → `env/<name>.json`.
//!
//! Each `.baglenv` (AAMP) holds directional, hemisphere and ambient lights and fogs. The names
//! are the files' base names: `plain`, `castle_n` (NSMBU field), `plain3w` (3D World field),
//! `dv_plain`, `dv_plain_d` (NSMBU background, day / night), `dv_plain3w` (3D World background);
//! the style's SceneDB names the set a theme uses (`Env_Model`, `Env_DV`).

use std::path::Path;

use mmk_formats::aamp::{self, List, Object};
use mmk_formats::archive::{self, Sarc};
use serde_json::{Value, json};

use crate::{Result, read, write};

const ARCHIVES: &[&str] = &["Env/Cmn.szs", "Env/Cmn_DV.szs"];

fn rgba(o: &Object, k: &str) -> Value {
    json!(o.vec(k).map(|v| v.to_vec()).unwrap_or_else(|| vec![0.0; 4]))
}

fn objects<'a>(root: &'a List, list: &str) -> impl Iterator<Item = &'a Object> {
    root.list(list).into_iter().flat_map(|l| l.objects.iter())
}

/// One environment file → JSON (lights and fogs).
pub fn env_json(root: &List) -> Value {
    let common = |o: &Object| {
        json!({
            "name": o.str("name").unwrap_or(""),
            "enable": o.bool("enable").unwrap_or(true),
        })
    };
    let with = |mut base: Value, extra: Value| {
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        base
    };
    let directional: Vec<Value> = objects(root, "DirectionalLight")
        .map(|o| {
            with(
                common(o),
                json!({
                    "diffuse": rgba(o, "DiffuseColor"),
                    "specular": rgba(o, "SpecularColor"),
                    "backside": rgba(o, "BacksideColor"),
                    "intensity": o.f32("Intensity").unwrap_or(1.0),
                    "direction": o.vec("Direction").map(|v| v.to_vec()).unwrap_or(vec![0.0, -1.0, 0.0]),
                    "view": o.bool("ViewCoordinate").unwrap_or(false),
                }),
            )
        })
        .collect();
    let hemisphere: Vec<Value> = objects(root, "HemisphereLight")
        .map(|o| {
            with(
                common(o),
                json!({
                    "sky": rgba(o, "SkyColor"),
                    "ground": rgba(o, "GroundColor"),
                    "intensity": o.f32("Intensity").unwrap_or(1.0),
                    "direction": o.vec("Direction").map(|v| v.to_vec()).unwrap_or(vec![0.0, 1.0, 0.0]),
                }),
            )
        })
        .collect();
    let ambient: Vec<Value> = objects(root, "AmbientLight")
        .map(|o| {
            with(
                common(o),
                json!({
                    "color": rgba(o, "Color"),
                    "intensity": o.f32("Intensity").unwrap_or(1.0),
                }),
            )
        })
        .collect();
    let fog: Vec<Value> = objects(root, "Fog")
        .map(|o| {
            with(
                common(o),
                json!({
                    "start": o.f32("Start").unwrap_or(0.0),
                    "end": o.f32("End").unwrap_or(1.0),
                    "damp": o.f32("Damp").unwrap_or(1.0),
                    "color": rgba(o, "Color"),
                    "direction": o.vec("Direction").map(|v| v.to_vec()).unwrap_or(vec![0.0, 0.0, -1.0]),
                }),
            )
        })
        .collect();
    json!({
        "directional": directional,
        "hemisphere": hemisphere,
        "ambient": ambient,
        "fog": fog,
    })
}

/// Write every `.baglenv` of the environment archives; returns how many.
pub fn export(romfs: &Path, out: &Path) -> Result<usize> {
    let mut count = 0;
    for rel in ARCHIVES {
        let path = romfs.join(rel);
        let Ok(data) = read(&path) else { continue };
        let outer = Sarc::parse(archive::unwrap(data).map_err(crate::fmt_err(&path))?)
            .map_err(crate::fmt_err(&path))?;
        for (_, genvb) in outer.files() {
            let Ok(inner) = Sarc::parse(genvb.to_vec()) else {
                continue;
            };
            for (name, file) in inner.files() {
                let Some(base) = name.strip_suffix(".baglenv") else {
                    continue;
                };
                let base = base.rsplit('/').next().unwrap_or(base);
                let Ok((_, root)) = aamp::parse(file) else {
                    continue;
                };
                write(
                    &out.join(format!("env/{base}.json")),
                    serde_json::to_string(&env_json(&root)).unwrap().as_bytes(),
                )?;
                count += 1;
            }
        }
    }
    Ok(count)
}
