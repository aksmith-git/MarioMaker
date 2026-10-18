//! SMM1 sprite import (Phase 11): the art and sounds of the SMM1 extras — the Mystery Mushroom
//! costumes, Lanky Mario, the Weird and Mystery Mushroom items — from the owner's Wii U dump →
//! an asset cache the engine merges next to the SMM2 one.
//!
//! Sources (the update's files replace the base game's):
//! - the costume table, `Mush/CharaMarioTable.byaml` in `Pack/Static.pack` (one entry per
//!   costume: resource name, list number, rarity, …), then the amiibo costumes of
//!   `BossNfp/Boss<NNN>.pack`. Costume ids are the game's: a table costume's id is its position
//!   in the costume name list of the owner's executable (`code/Block.rpx`, the comma-separated
//!   list starting with `Invalid`), a `Boss<NNN>` costume's id is 100 + NNN;
//! - art: `Model/M1_Player_Chara_<res>.szs` (a Wii U BFRES holding only textures, one per pose
//!   frame: `wait.0`, `walk.0`…, `jump.0`, …), or `Boss/M1_Player_Chara_<res>.szs` in the boss
//!   pack;
//! - sounds: `Sound/CharaMario/CharaMario_<res>.bars` and `…<res>Static.bars` (SARCs of BFWAV:
//!   jump, goal, down, slip, appeal; powerup), or `Boss/CharaMario_<res>[Static].bars` in the
//!   boss packs;
//! - English names: `Cmn_CharaMarioName.msbt` in `Message/CommonMsg_<lang>.szs` (labels = resource
//!   names), or `Boss/CharaMario_<res>_<lang>.szs` in the boss pack.
//!
//! - the player and item models of `Pack/StaticSkin.pack`: `M1_Player_SlimMarioMdl` (Lanky
//!   Mario), `M1_Item_kinokofunny` (Weird Mushroom), `M1_Item_CharaKinoko` (Mystery Mushroom).
//!
//! Output (`<out>/sprites/`, a regular asset cache, see `mmk_assets`):
//!
//! ```text
//! manifest.json               kind "smm1-sprites"; "costumes": [{ id, res, name, model, rare,
//!                             list_no, sounds: { event: sound id } }]; "models": [...]
//! textures/index.json         { "SMM1_Costume_<id>": { "<frame>": { w, h } }, "M1_Player_…": … }
//! textures/<model>/<frame>.mtex
//! sprites.json                each frame's quad (costumes: 24 × 24 standing on the origin)
//! audio/index.json, audio/costume_<id>_<event>.bwav   (BFWAV converted to BWAV)
//! ```
//! The engine names a costume's model `SMM1_Costume_<id>` (three digits) and knows nothing else
//! about it.

use std::collections::BTreeMap;
use std::path::Path;

use mmk_formats::archive::{self, Sarc};
use mmk_formats::audio::Fwav;
use mmk_formats::bfres_wiiu;
use mmk_formats::msbt::Msbt;
use serde_json::{Map, Value, json};

use crate::smm1::{Overlay, to_mtex};
use crate::{Progress, Result, fmt_err, read, write};

/// Engine model name of costume `id`.
pub fn costume_model(id: usize) -> String {
    format!("SMM1_Costume_{id:03}")
}

/// Where a 24 × 24 costume frame is drawn: the costume rig `M1_Player_CharaMdl` is one quad
/// x −12 … 12, y 0 … 24 with the origin at the feet [obs].
pub const COSTUME_QUAD: [f32; 4] = [-12.0, 0.0, 12.0, 24.0];

/// The extra models: (file stem in StaticSkin.pack, quad). Lanky Mario uses Super Mario's
/// 16 × 32 quad, the Weird Mushroom the Super Mushroom's 16 × 16 one, the Mystery Mushroom a
/// 24 × 24 one standing on the origin [inf].
const EXTRA_MODELS: [(&str, [f32; 4]); 3] = [
    ("M1_Player_SlimMarioMdl", [-8.0, 0.0, 8.0, 32.0]),
    ("M1_Item_kinokofunny", [-8.0, 0.0, 8.0, 16.0]),
    ("M1_Item_CharaKinoko", [-12.0, 0.0, 12.0, 24.0]),
];

#[derive(Clone, Debug, Default)]
pub struct CostumeReport {
    pub costumes: usize,
    pub textures: usize,
    pub sounds: usize,
    pub skipped: Vec<String>,
}

/// One costume's files.
struct Source {
    id: usize,
    res: String,
    entry: Map<String, Value>,
    /// The art archive (`.szs`) bytes.
    art: Option<Vec<u8>>,
    /// Sound archives (SARC of BFWAV), main then static.
    sounds: Vec<Vec<u8>>,
    name: Option<String>,
}

fn sarc_of(data: Vec<u8>, ctx: &Path) -> Result<Sarc> {
    Sarc::parse(archive::unwrap(data).map_err(fmt_err(ctx))?).map_err(fmt_err(ctx))
}

/// A member of a pack, Yaz0 removed.
fn member(sarc: &Sarc, name: &str) -> Option<Vec<u8>> {
    archive::unwrap(sarc.get(name)?.to_vec()).ok()
}

fn first_message(msbt_archive: Vec<u8>, ctx: &Path) -> Option<String> {
    let sarc = sarc_of(msbt_archive, ctx).ok()?;
    let (_, data) = sarc.files().find(|(n, _)| n.ends_with(".msbt"))?;
    let m = Msbt::parse(data).ok()?;
    m.messages.first().map(|msg| msg.plain())
}

fn collect(ov: &Overlay, language: &str, report: &mut CostumeReport) -> Result<Vec<Source>> {
    let mut out = Vec::new();
    // names of the regular costumes
    let names: BTreeMap<String, String> = ov
        .file(&format!("Message/CommonMsg_{language}.szs"))
        .and_then(|p| {
            let sarc = sarc_of(read(&p).ok()?, &p).ok()?;
            let m = Msbt::parse(sarc.get("Cmn_CharaMarioName.msbt")?).ok()?;
            Some(
                m.messages
                    .iter()
                    .map(|msg| (msg.label.clone(), msg.plain()))
                    .collect(),
            )
        })
        .unwrap_or_default();
    // the table
    let table = ov
        .file("Pack/Static.pack")
        .map(|p| -> Result<Option<Value>> {
            let sarc = sarc_of(read(&p)?, &p)?;
            Ok(member(&sarc, "Mush/CharaMarioTable.byaml")
                .and_then(|b| mmk_formats::byml::parse(&b).ok()))
        })
        .transpose()?
        .flatten();
    // the game's costume order: the name list in the executable
    let order = costume_order(ov, table.as_ref());
    if order.is_none() {
        report
            .skipped
            .push("code/Block.rpx: no costume name list; ids follow the table".into());
    }
    match table {
        Some(Value::Array(entries)) => {
            for (index, e) in entries.into_iter().enumerate() {
                let Value::Object(entry) = e else { continue };
                let Some(res) = entry.get("ResName").and_then(Value::as_str) else {
                    continue;
                };
                let res = res.to_string();
                // a few costumes have regional art (`<res>_USEU`): the language's region first
                let regional = || {
                    let prefix = format!("M1_Player_Chara_{res}_");
                    let mut found: Vec<_> = ov
                        .list("Model", ".szs")
                        .into_iter()
                        .filter(|(f, _)| f.starts_with(&prefix))
                        .collect();
                    let region = &language[..2.min(language.len())];
                    found.sort_by_key(|(f, _)| !f[prefix.len()..].contains(region));
                    found.into_iter().next().map(|(_, p)| p)
                };
                let art = ov
                    .file(&format!("Model/M1_Player_Chara_{res}.szs"))
                    .or_else(regional)
                    .map(|p| read(&p))
                    .transpose()?;
                let mut sounds = Vec::new();
                for suffix in ["", "Static"] {
                    if let Some(p) =
                        ov.file(&format!("Sound/CharaMario/CharaMario_{res}{suffix}.bars"))
                    {
                        sounds.push(read(&p)?);
                    }
                }
                let id = order
                    .as_ref()
                    .and_then(|o| o.iter().position(|n| *n == res))
                    .unwrap_or(index);
                out.push(Source {
                    id,
                    name: names.get(&res).cloned(),
                    res,
                    entry,
                    art,
                    sounds,
                });
            }
        }
        _ => report
            .skipped
            .push("Pack/Static.pack: no Mush/CharaMarioTable.byaml".into()),
    }
    // event costumes: BossNfp/Boss<NNN>.pack (+ BossStatic<NNN>.pack)
    for (file, path) in ov.list("BossNfp", ".pack") {
        let Some(num) = file
            .strip_prefix("Boss")
            .and_then(|s| s.strip_suffix(".pack"))
            .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let res = format!("Boss{num}");
        let Ok(n) = num.parse::<usize>() else {
            continue;
        };
        let sarc = sarc_of(read(&path)?, &path)?;
        let mut sounds: Vec<Vec<u8>> = member(&sarc, &format!("Boss/CharaMario_{res}.bars"))
            .into_iter()
            .collect();
        if let Some(p) = ov.file(&format!("BossNfp/BossStatic{num}.pack")) {
            let st = sarc_of(read(&p)?, &p)?;
            sounds.extend(member(&st, &format!("Boss/CharaMario_{res}Static.bars")));
        }
        let name = member(&sarc, &format!("Boss/CharaMario_{res}_{language}.szs"))
            .and_then(|b| first_message(b, &path));
        let mut entry = Map::new();
        entry.insert("ResName".into(), json!(res));
        out.push(Source {
            id: 100 + n,
            art: member(&sarc, &format!("Boss/M1_Player_Chara_{res}.szs")),
            res,
            entry,
            sounds,
            name,
        });
    }
    out.sort_by_key(|s| s.id);
    out.dedup_by_key(|s| s.id);
    Ok(out)
}

/// The costume names in id order, from the executable (`code/Block.rpx`): the comma-separated
/// list that starts with `Invalid` and names the table's costumes.
fn costume_order(ov: &Overlay, table: Option<&Value>) -> Option<Vec<String>> {
    let names: Vec<&str> = table?
        .as_array()?
        .iter()
        .filter_map(|e| e.get("ResName")?.as_str())
        .collect();
    let rpx = read(&ov.code_file("Block.rpx")?).ok()?;
    let sections = mmk_formats::rpx::sections(&rpx).ok()?;
    for sec in &sections {
        for list in mmk_formats::rpx::strings_with_prefix(sec, b"Invalid, ") {
            let order: Vec<String> = list.split(", ").skip(1).map(str::to_string).collect();
            if names.iter().all(|n| order.iter().any(|o| o == n)) {
                return Some(order);
            }
        }
    }
    None
}

/// Import the costumes into `<out>/costumes/`. A dump without the table or the art imports
/// what it has and lists the rest in `skipped`.
pub(crate) fn import_costumes(
    ov: &Overlay,
    out: &Path,
    language: &str,
    progress: Progress,
) -> Result<CostumeReport> {
    let mut report = CostumeReport::default();
    let sources = collect(ov, language, &mut report)?;
    let dir = out.join("sprites");
    let mut index = Map::new();
    let mut quads = Map::new();
    let mut audio = Map::new();
    let mut list = Vec::new();
    for (k, src) in sources.iter().enumerate() {
        progress("costumes", k, sources.len());
        let id = src.id;
        let model = costume_model(id);
        let ctx = Path::new(&src.res);
        let mut texs = Map::new();
        let mut model_quads = Map::new();
        match &src.art {
            None => report.skipped.push(format!("{}: no art", src.res)),
            Some(szs) => {
                let sarc = sarc_of(szs.clone(), ctx)?;
                for (member, data) in sarc.files() {
                    if !data.starts_with(b"FRES") {
                        continue;
                    }
                    let textures = match bfres_wiiu::textures(data) {
                        Ok(t) => t,
                        Err(e) => {
                            report.skipped.push(format!("{}/{member}: {e}", src.res));
                            continue;
                        }
                    };
                    for t in textures {
                        let img = match t.decode() {
                            Ok(i) => i,
                            Err(e) => {
                                report.skipped.push(format!("{}/{}: {e}", src.res, t.name));
                                continue;
                            }
                        };
                        let m = to_mtex(&img);
                        write(
                            &dir.join(format!("textures/{model}/{}.mtex", t.name)),
                            &m.to_bytes(),
                        )?;
                        texs.insert(t.name.clone(), json!({ "w": m.width, "h": m.height }));
                        // a frame of another size keeps the 24-px frame's footing
                        let (w, h) = (m.width as f32, m.height as f32);
                        let q = [-w / 2.0, COSTUME_QUAD[1], w / 2.0, COSTUME_QUAD[1] + h];
                        model_quads.insert(
                            t.name.clone(),
                            json!({ "rect": q, "uv": [0.0, 0.0, 1.0, 1.0], "source": "costume" }),
                        );
                        report.textures += 1;
                    }
                }
            }
        }
        let mut sounds = Map::new();
        for archive_bytes in &src.sounds {
            let Ok(sarc) = sarc_of(archive_bytes.clone(), ctx) else {
                report
                    .skipped
                    .push(format!("{}: unreadable sound archive", src.res));
                continue;
            };
            for (member, data) in sarc.files() {
                let Some(event) = member.strip_suffix(".bfwav") else {
                    continue;
                };
                let event = event.rsplit('/').next().unwrap_or(event);
                let wave = match Fwav::parse(data) {
                    Ok(w) => w,
                    Err(e) => {
                        report.skipped.push(format!("{}/{member}: {e}", src.res));
                        continue;
                    }
                };
                let sid = format!("costume_{id:03}_{event}");
                write(&dir.join(format!("audio/{sid}.bwav")), &wave.to_bwav())?;
                audio.insert(
                    sid.clone(),
                    json!({ "file": format!("{sid}.bwav"), "gain": 1.0 }),
                );
                sounds.insert(event.to_string(), json!(sid));
                report.sounds += 1;
            }
        }
        if !texs.is_empty() {
            index.insert(model.clone(), Value::Object(texs));
            quads.insert(
                model.clone(),
                json!({ "quads": Value::Object(model_quads) }),
            );
        }
        let e = &src.entry;
        list.push(json!({
            "id": id,
            "res": src.res,
            "name": src.name.clone().unwrap_or_else(|| src.res.clone()),
            "model": model,
            "art": index.contains_key(&model),
            "rare": e.get("Rare"),
            "list_no": e.get("ListNo"),
            "sounds": Value::Object(sounds),
        }));
        report.costumes += 1;
    }
    progress("costumes", sources.len(), sources.len());
    // Lanky Mario and the two items
    let mut models = Vec::new();
    for (stem, q) in EXTRA_MODELS {
        let rel = format!("Model/{stem}.szs");
        let Some(szs) = ov.pack_member("Pack/StaticSkin.pack", &rel) else {
            report
                .skipped
                .push(format!("StaticSkin.pack/{rel}: not found"));
            continue;
        };
        let mut texs = Map::new();
        let mut model_quads = Map::new();
        let sarc = sarc_of(szs, Path::new(stem))?;
        for (member, data) in sarc.files() {
            if !data.starts_with(b"FRES") {
                continue;
            }
            for t in bfres_wiiu::textures(data).unwrap_or_default() {
                let Ok(img) = t.decode() else {
                    report.skipped.push(format!("{stem}/{member}/{}", t.name));
                    continue;
                };
                let m = to_mtex(&img);
                write(
                    &dir.join(format!("textures/{stem}/{}.mtex", t.name)),
                    &m.to_bytes(),
                )?;
                texs.insert(t.name.clone(), json!({ "w": m.width, "h": m.height }));
                model_quads.insert(
                    t.name.clone(),
                    json!({ "rect": q, "uv": [0.0, 0.0, 1.0, 1.0], "source": "smm1" }),
                );
                report.textures += 1;
            }
        }
        if !texs.is_empty() {
            index.insert(stem.to_string(), Value::Object(texs));
            quads.insert(
                stem.to_string(),
                json!({ "quads": Value::Object(model_quads) }),
            );
            models.push(stem);
        }
    }
    write(
        &dir.join("textures/index.json"),
        serde_json::to_string(&Value::Object(index))
            .unwrap()
            .as_bytes(),
    )?;
    write(
        &dir.join("sprites.json"),
        serde_json::to_string(&json!({ "version": 1, "models": Value::Object(quads) }))
            .unwrap()
            .as_bytes(),
    )?;
    write(
        &dir.join("audio/index.json"),
        serde_json::to_string(&Value::Object(audio))
            .unwrap()
            .as_bytes(),
    )?;
    write(
        &dir.join("manifest.json"),
        serde_json::to_string_pretty(&json!({
            "game": "SMM1",
            "kind": "smm1-sprites",
            "models": models,
            "costumes": list,
            "textures": report.textures,
            "sounds": report.sounds,
            "skipped": report.skipped,
        }))
        .unwrap()
        .as_bytes(),
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::smm1::Smm1Config;
    use crate::smm1::tests::sarc;

    /// An event costume in a boss pack: art → textures + quads, names, the manifest list.
    #[test]
    fn imports_a_synthetic_boss_costume() {
        let tmp = std::env::temp_dir().join(format!("mmk-costume-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let base = tmp.join("content");
        fs::create_dir_all(base.join("BossNfp")).unwrap();
        let px = vec![200u8; 24 * 24 * 4];
        let fres =
            bfres_wiiu::build_rgba(&[("wait.0", 24, 24, px.clone()), ("jump.0", 24, 24, px)]);
        let art = sarc(&[("Output.bfres", &fres)]);
        let pack = sarc(&[("Boss/M1_Player_Chara_Boss007.szs", &art)]);
        fs::write(base.join("BossNfp/Boss007.pack"), &pack).unwrap();
        let ov = Overlay::new(&Smm1Config {
            base: base.clone(),
            update: None,
            out: tmp.join("out"),
            language: "USen".into(),
        });
        let r = import_costumes(&ov, &tmp.join("out"), "USen", &mut |_, _, _| {}).unwrap();
        assert_eq!((r.costumes, r.textures), (1, 2));
        let dir = tmp.join("out/sprites");
        let m: Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m["kind"], "smm1-sprites");
        assert_eq!(m["costumes"][0]["id"], 107);
        assert_eq!(m["costumes"][0]["res"], "Boss007");
        assert_eq!(m["costumes"][0]["model"], "SMM1_Costume_107");
        let mut cache = mmk_assets::Cache::open(&dir).unwrap();
        assert_eq!(cache.textures.len(), 2);
        let q = cache.sprite_quad("SMM1_Costume_107", "wait.0").unwrap();
        assert_eq!((q.x0, q.y0, q.x1, q.y1), (-12.0, 0.0, 12.0, 24.0));
        let i = cache.texture_index("SMM1_Costume_107", "jump.0").unwrap();
        assert_eq!(cache.texture(i).unwrap().data[..4], [200, 200, 200, 200]);
        cache.merged.clear();
        let _ = fs::remove_dir_all(&tmp);
    }
}
