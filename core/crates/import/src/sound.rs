//! Sound stage: the gameplay sound database and every wave it names, from the user's RomFS
//! into `<cache>/audio/`.
//!
//! - `SLink2/SLink2DB.szs` (`mmk_formats::xlink`): per sound user (the course objects of each
//!   game style, the player, the course music users) its event keys → asset trees (switch /
//!   random / blend / sequence containers with their conditions; assets with wave name, volume,
//!   pitch, delay, group, priority, distance set, …). User names are not stored in the
//!   database; the candidates are the names of the resource lists (`*.barslist`), which name
//!   every user that loads sounds.
//! - `Pack/StaticSound.pack`: the group tree with voice limits and ducking
//!   (`Sound/Group/GroupSetting.bagst`) and the distance attenuation sets
//!   (`Sound/Attenuation/Attenuation.baatarc`).
//! - `Sound/Effect/delay_*.byml`, `reverb_*.byml`: the per style and theme effects.
//! - The waves: BWAVs from the BARS archives (`Sound/Resource/*.bars`, `Pack/*.pack`) or the
//!   stream files (`Sound/Resource/Stream/`), copied unchanged to `audio/se/` (effects) and
//!   `audio/music/` (streams).
//!
//! Output `audio/sound.json` (read by `mmk_audio::links`):
//!
//! ```text
//! { "version": 1,
//!   "users": { "<user>": { "distance": "<set>", "calls": [ <call>, … ] } },
//!   "groups": [ { "name", "parent", "folder", "volume", "limit", "limit_type", "duck": [..] } ],
//!   "attenuation": { "<set>": { "max", "min", "far_gain", "near_gain", "cull" } },
//!   "effects": { "<style>": [ { "delay": {..}|null, "reverb": {..}|null } × 10 themes ] },
//!   "waves": { "<wave>": "se/<file>.bwav" | "music/<file>.bwav" } }
//! call: { "key", "parent"?, "if"?: ["==", value] | "weight"?: w,
//!         asset: "wave", "volume", "pitch", "delay", "group", "priority", "stop", "fade_in",
//!                "lpf", "distance", "duration", "dry"(IgnoreWaterFilter)
//!         container: "kind": "switch"|"random"|"blend"|"sequence", "children": [a, b],
//!                    "watch"?, "global"? }
//! number params: a number, {"min", "max"} (random each play) or {"curve": property,
//!                "global", "points": [[x, y], …]}
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use mmk_formats::archive::{self, Sarc};
use mmk_formats::audio::settings;
use mmk_formats::audio::{Bars, Bwav};
use mmk_formats::byml;
use mmk_formats::xlink::{Call, Compare, CondValue, Condition, ContainerKind, Param, Xlnk};
use serde_json::{Map, Value, json};

use crate::{ImportError, Progress, Result, fmt_err, is_file, read, read_dir, write};

pub const SOUND_DB_VERSION: u32 = 1;

/// Game styles, in the order of the game's style index.
const STYLES: [&str; 5] = ["M1", "M3", "MW", "WU", "3W"];
/// Course themes by id (0 ground … 9 forest), as the music keys and effect files spell them.
pub const THEMES: [&str; 10] = [
    "Plain",
    "UnderGround",
    "Castle",
    "Airship",
    "Water",
    "HauntedHouse",
    "Snow",
    "Desert",
    "Athletic",
    "Woods",
];

/// Users that are not per style but play during a course.
const COMMON_USERS: &[&str] = &[
    "CourseSceneBGM",
    "CourseSceneBGM_Common",
    "CourseInScene",
    "GameEnemy",
    "GameObject",
    "GamePlayer",
    "GameYoshi",
    "ObjectCommon",
    "DistantView",
    "MU_koura",
    "Scene",
];

/// Music keys of the course music users that are not played in a course (editor, title,
/// online and mode screens).
fn menu_music_key(key: &str) -> bool {
    key.contains("_Edit")
        || key.starts_with("BGM_Title")
        || key.starts_with("Battle_")
        || key.ends_with("_Clear") && !key.starts_with("Fanfare")
}

/// A user of the gameplay database: per-style course objects (not the editor's) and the
/// common course users.
fn gameplay_user(name: &str) -> bool {
    if COMMON_USERS.contains(&name) {
        return true;
    }
    if let Some(s) = name.strip_prefix("CourseSceneBGM_") {
        return STYLES.contains(&s);
    }
    let Some((prefix, base)) = name.split_once('_') else {
        return false;
    };
    (STYLES.contains(&prefix) || prefix == "MU") && !base.starts_with("Edit")
}

/// Names of every resource list (each a sound user's name).
fn user_candidates(romfs: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    for p in read_dir(&romfs.join("Sound/ResourceList")).unwrap_or_default() {
        if let Some(stem) = p
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".barslist"))
        {
            names.insert(stem.to_string());
        }
    }
    for p in read_dir(&romfs.join("Pack")).unwrap_or_default() {
        let file = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !(file.ends_with("_barslist.pack") || file == "StaticSound.pack") {
            continue;
        }
        let Ok(sarc) = open(&p) else { continue };
        for (n, _) in sarc.files() {
            if let Some(stem) = n
                .rsplit('/')
                .next()
                .and_then(|n| n.strip_suffix(".barslist"))
            {
                names.insert(stem.to_string());
            }
        }
    }
    for n in COMMON_USERS {
        names.insert(n.to_string());
    }
    for s in STYLES {
        names.insert(format!("CourseSceneBGM_{s}"));
    }
    names.into_iter().collect()
}

fn open(path: &Path) -> Result<Sarc> {
    let data = archive::unwrap(read(path)?).map_err(fmt_err(path))?;
    Sarc::parse(data).map_err(fmt_err(path))
}

fn num(p: &Param) -> Option<Value> {
    Some(match p {
        Param::Num(v) => json!(v),
        Param::Int(v) => json!(v),
        Param::Range(a, b) => json!({"min": a, "max": b}),
        Param::RangePow(a, b, p) => json!({"min": a, "max": b, "pow": p}),
        Param::Curve(c) => json!({
            "curve": c.property,
            "global": c.global,
            "points": c.points.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(),
        }),
        _ => return None,
    })
}

fn cond_json(c: &Condition) -> Option<(&'static str, Value)> {
    match c {
        Condition::Switch { cmp, value, .. } => {
            let op = match cmp {
                Compare::Eq => "==",
                Compare::Gt => ">",
                Compare::Ge => ">=",
                Compare::Lt => "<",
                Compare::Le => "<=",
                Compare::Ne => "!=",
            };
            let v = match value {
                CondValue::Enum(s) => json!(s),
                CondValue::Int(i) => json!(i),
                CondValue::Float(f) => json!(f),
            };
            Some(("if", json!([op, v])))
        }
        Condition::Random { weight } => Some(("weight", json!(weight))),
        Condition::Other(_) => None,
    }
}

/// Engine names for watched properties, by the values their conditions compare against: the
/// game style, the course theme, day/night, the ground material and the scene. Other
/// properties keep the game's name.
fn property_role(calls: &[Call], container: &Call) -> Option<&'static str> {
    const THEME_VALUES: [&str; 10] = [
        "plain",
        "underground",
        "castle",
        "airship",
        "water",
        "hauntedhouse",
        "snow",
        "desert",
        "athletic",
        "woods",
    ];
    const GROUND: [&str; 12] = [
        "cIce",
        "cSnow",
        "cRock",
        "cCloud",
        "cDirt",
        "cSand",
        "cWood",
        "cNormal",
        "cThickwood",
        "cGlass",
        "cNone",
        "cGrass",
    ];
    let (a, b) = container.children?;
    let values: Vec<&str> = calls
        .get(a..=b)?
        .iter()
        .filter_map(|c| match &c.condition {
            Some(Condition::Switch {
                value: CondValue::Enum(v),
                ..
            }) => Some(v.as_str()),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        return None;
    }
    let all = |set: &[&str]| values.iter().all(|v| set.contains(v));
    Some(if all(&STYLES) {
        "@style"
    } else if all(&THEME_VALUES) {
        "@theme"
    } else if all(&["cDay", "cNight"]) {
        "@hour"
    } else if all(&GROUND) {
        "@ground"
    } else if values.contains(&"Course") && all(&["Course", "QuestScene", "LessonMenuScene"]) {
        "@scene"
    } else {
        return None;
    })
}

/// One call as JSON; asset waves go into `waves`.
fn call_json(calls: &[Call], c: &Call, waves: &mut BTreeSet<String>) -> Value {
    let mut v = Map::new();
    v.insert("key".into(), json!(c.key));
    if let Some(p) = c.parent {
        v.insert("parent".into(), json!(p));
    }
    if let Some((k, val)) = c.condition.as_ref().and_then(cond_json) {
        v.insert(k.into(), val);
    }
    if c.container {
        let kind = match c.kind {
            Some(ContainerKind::Switch) => "switch",
            Some(ContainerKind::Random | ContainerKind::Random2) => "random",
            Some(ContainerKind::Blend) => "blend",
            Some(ContainerKind::Sequence) => "sequence",
            _ => "blend",
        };
        v.insert("kind".into(), json!(kind));
        if let Some((a, b)) = c.children {
            v.insert("children".into(), json!([a, b]));
        }
        if let Some((w, g)) = &c.watch {
            let name = property_role(calls, c)
                .map(str::to_string)
                .unwrap_or(w.clone());
            v.insert("watch".into(), json!(name));
            if *g {
                v.insert("global".into(), json!(true));
            }
        }
        return Value::Object(v);
    }
    let s = |k: &str| c.params.get(k).and_then(|p| p.as_str());
    if let Some(w) = s("RuntimeAssetName").filter(|w| !w.is_empty() && *w != "@Blank") {
        waves.insert(w.to_string());
        v.insert("wave".into(), json!(w));
    }
    // the TV volume where the asset sets one (`VolumeTV`), else `Volume`; `VolumeDRC` is the
    // handheld level [inf]
    let volume = c.params.get("VolumeTV").or_else(|| c.params.get("Volume"));
    for (out, p) in [
        ("volume", volume),
        ("pitch", c.params.get("Pitch")),
        ("delay", c.params.get("Delay")),
        ("priority", c.params.get("Priority")),
        ("stop", c.params.get("StopFrame")),
        ("fade_in", c.params.get("FadeInTime")),
        ("lpf", c.params.get("Lpf")),
        ("duration", c.params.get("Duration")),
    ] {
        if let Some(n) = p.and_then(num) {
            v.insert(out.into(), n);
        }
    }
    if let Some(g) = s("GroupName").filter(|g| !g.is_empty()) {
        v.insert("group".into(), json!(g));
    }
    if let Some(d) = s("DistanceParamSetName").filter(|d| !d.is_empty()) {
        v.insert("distance".into(), json!(d));
    }
    if c.params.contains_key("IgnoreWaterFilter") {
        v.insert("dry".into(), json!(true));
    }
    Value::Object(v)
}

/// Safe file stem for a wave name (names may contain spaces).
pub fn wave_file_stem(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// BWAVs for `wanted` names from every BARS in the RomFS (complete ones only), then from the
/// stream files.
fn find_waves(
    romfs: &Path,
    wanted: &BTreeSet<String>,
    progress: Progress,
) -> Result<HashMap<String, (bool, Vec<u8>)>> {
    let mut found: HashMap<String, (bool, Vec<u8>)> = HashMap::new();
    let mut sources: Vec<std::path::PathBuf> = read_dir(&romfs.join("Sound/Resource"))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "bars"))
        .collect();
    for p in read_dir(&romfs.join("Pack")).unwrap_or_default() {
        let f = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if f.ends_with("_bars.pack") || f.starts_with("Static") && f.ends_with("Sound.pack") {
            sources.push(p);
        }
    }
    let take = |blob: &[u8], found: &mut HashMap<String, (bool, Vec<u8>)>| {
        let Ok(bars) = Bars::parse(blob) else { return };
        for e in &bars.entries {
            if found.contains_key(&e.name) || !wanted.contains(&e.name) {
                continue;
            }
            if let Some(w) = bars.bwav(e)
                && w.is_complete()
                && let Ok(b) = w.bytes()
            {
                found.insert(e.name.clone(), (false, b.to_vec()));
            }
        }
    };
    for (k, src) in sources.iter().enumerate() {
        progress("sound waves", k, sources.len());
        if src.extension().is_some_and(|e| e == "bars") {
            take(&read(src)?, &mut found);
        } else if let Ok(sarc) = open(src) {
            for (n, d) in sarc.files() {
                if n.ends_with(".bars") {
                    take(d, &mut found);
                }
            }
        }
    }
    progress("sound waves", sources.len(), sources.len());
    // streams: music and anything the archives only hold a prefetch part of
    let dir = romfs.join("Sound/Resource/Stream");
    for w in wanted {
        if found.contains_key(w) {
            continue;
        }
        let p = dir.join(format!("{w}.bwav"));
        if is_file(&p) {
            let bytes = read(&p)?;
            if Bwav::parse(&bytes).is_ok_and(|b| b.is_complete()) {
                found.insert(w.clone(), (true, bytes));
            }
        }
    }
    Ok(found)
}

fn groups_json(romfs: &Path) -> Result<Value> {
    let path = romfs.join("Pack/StaticSound.pack");
    let sarc = open(&path)?;
    let Some(d) = sarc.get("Sound/Group/GroupSetting.bagst") else {
        return Ok(json!([]));
    };
    let groups = settings::parse_groups(d).map_err(fmt_err(&path))?;
    Ok(Value::Array(
        groups
            .iter()
            .map(|g| {
                json!({
                    "name": g.name,
                    "parent": g.parent,
                    "folder": g.folder,
                    "volume": g.volume,
                    "limit": g.limit,
                    "limit_type": g.limit_type,
                    "duck": g.duck.iter().map(|k| json!({
                        "group": k.group, "volume": k.volume,
                        "fade_in": k.fade_in, "fade_out": k.fade_out,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    ))
}

fn attenuation_json(romfs: &Path) -> Result<Value> {
    let path = romfs.join("Pack/StaticSound.pack");
    let sarc = open(&path)?;
    let Some(arc) = sarc.get("Sound/Attenuation/Attenuation.baatarc") else {
        return Ok(json!({}));
    };
    let inner = Sarc::parse(arc.to_vec()).map_err(fmt_err(&path))?;
    let mut rolloff = HashMap::new();
    let mut culling = HashMap::new();
    let mut sets = Vec::new();
    for (n, d) in inner.files() {
        let stem = n
            .rsplit('/')
            .next()
            .and_then(|f| f.rsplit_once('.'))
            .map(|(s, _)| s.to_string())
            .unwrap_or_default();
        if n.ends_with(".baroc") {
            if let Ok(r) = settings::parse_rolloff(d) {
                rolloff.insert(stem, r);
            }
        } else if n.ends_with(".baacl") {
            if let Ok(c) = settings::parse_culling(d) {
                culling.insert(stem, c);
            }
        } else if n.ends_with(".baatn")
            && let Ok(s) = settings::parse_attenuation(d)
        {
            sets.push((stem, s));
        }
    }
    let mut out = Map::new();
    for (name, s) in sets {
        let mut v = Map::new();
        if let Some(r) = rolloff.get(&s.volume).filter(|r| r.linear) {
            v.insert("max".into(), json!(r.max));
            v.insert("min".into(), json!(r.min));
            v.insert("far_gain".into(), json!(r.far_gain));
            v.insert("near_gain".into(), json!(r.near_gain));
        }
        if let Some(c) = culling.get(&s.culling).filter(|c| c.on) {
            v.insert("cull".into(), json!(c.distance));
        }
        out.insert(name, Value::Object(v));
    }
    Ok(Value::Object(out))
}

/// Theme index of an effect file's theme spelling.
fn effect_theme(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase();
    THEMES.iter().position(|t| {
        let t = t.to_ascii_lowercase();
        n == t || (t == "hauntedhouse" && n == "haunted")
    })
}

fn effects_json(romfs: &Path) -> Result<Value> {
    let dir = romfs.join("Sound/Effect");
    let mut table: BTreeMap<&str, Vec<[Value; 2]>> = STYLES
        .iter()
        .map(|s| (*s, vec![[Value::Null, Value::Null]; THEMES.len()]))
        .collect();
    for p in read_dir(&dir).unwrap_or_default() {
        let Some(stem) = p
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".byml"))
        else {
            continue;
        };
        let (slot, rest) = if let Some(r) = stem.strip_prefix("delay_") {
            (0, r)
        } else if let Some(r) = stem.strip_prefix("reverb_") {
            (1, r)
        } else {
            continue;
        };
        let Some(style) = STYLES.iter().find(|s| rest.starts_with(**s)) else {
            continue;
        };
        let Some(theme) = effect_theme(&rest[style.len()..]) else {
            continue;
        };
        let doc = byml::parse(&read(&p)?).map_err(fmt_err(&p))?;
        let Some(fx) = doc["Effect"].as_object().and_then(|m| m.values().next()) else {
            continue;
        };
        let entry = json!({"type": fx["TypeName"], "params": fx["Params"]});
        if let Some(row) = table.get_mut(style) {
            row[theme][slot] = entry;
        }
    }
    let mut out = Map::new();
    for (style, row) in table {
        out.insert(
            style.into(),
            Value::Array(
                row.into_iter()
                    .map(|[d, r]| json!({"delay": d, "reverb": r}))
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(out))
}

/// Ground materials of the 3D styles' terrain parts per theme (`Mush/<style>_BgGroundAttrTable`:
/// part → theme → material), for footstep and landing sounds.
fn ground_json(romfs: &Path) -> Result<Value> {
    let path = romfs.join("Pack/Static.pack");
    let sarc = open(&path)?;
    let mut out = Map::new();
    for style in STYLES {
        let Some(d) = sarc.get(&format!("Mush/{style}_BgGroundAttrTable.byml")) else {
            continue;
        };
        let doc = byml::parse(d).map_err(fmt_err(&path))?;
        let mut parts = Map::new();
        for row in doc.as_array().into_iter().flatten() {
            let Some(part) = row["PartsName"].as_str() else {
                continue;
            };
            let mut themes = Map::new();
            for (k, v) in row.as_object().into_iter().flatten() {
                if k != "PartsName" {
                    themes.insert(k.clone(), v.clone());
                }
            }
            parts.insert(part.to_string(), Value::Object(themes));
        }
        out.insert(style.to_string(), Value::Object(parts));
    }
    Ok(Value::Object(out))
}

#[derive(Debug, Default)]
pub struct SoundReport {
    pub users: usize,
    pub waves: usize,
    pub streams: usize,
    pub missing: Vec<String>,
}

/// Import the gameplay sound database and its waves into `out/audio/`.
pub fn import_sounds(romfs: &Path, out: &Path, progress: Progress) -> Result<SoundReport> {
    let slink_path = romfs.join("SLink2/SLink2DB.szs");
    let slink = open(&slink_path)?;
    let db = slink
        .files()
        .find(|(n, _)| n.ends_with(".bslnk"))
        .map(|(_, d)| d.to_vec())
        .ok_or_else(|| ImportError::Invalid("SLink2DB.szs has no .bslnk".into()))?;
    let x = Xlnk::parse(&db).map_err(fmt_err(&slink_path))?;
    let mut report = SoundReport::default();
    let mut waves = BTreeSet::new();
    let mut users = Map::new();
    let names = user_candidates(romfs);
    for (k, name) in names.iter().enumerate() {
        progress("sound users", k, names.len());
        if !gameplay_user(name) {
            continue;
        }
        let Some(calls) = x.user_calls(name).map_err(fmt_err(&slink_path))? else {
            continue;
        };
        let music = name.starts_with("CourseSceneBGM");
        let mut list = Vec::with_capacity(calls.len());
        // a call is kept unless it (or its top-level ancestor) is a menu music key
        let top = |mut i: usize| {
            for _ in 0..64 {
                match calls[i].parent {
                    Some(p) if p < calls.len() => i = p,
                    _ => break,
                }
            }
            i
        };
        for (i, c) in calls.iter().enumerate() {
            if music && menu_music_key(&calls[top(i)].key) {
                // keep the slot (children are addressed by index) but drop the asset
                list.push(json!({"key": c.key, "parent": c.parent, "kind": "blend"}));
                continue;
            }
            list.push(call_json(&calls, c, &mut waves));
        }
        let mut u = Map::new();
        if let Some(p) = x.user_params(name).map_err(fmt_err(&slink_path))? {
            if let Some(d) = p
                .get("DistanceParamSetName")
                .and_then(|v| v.as_str())
                .filter(|d| !d.is_empty())
            {
                u.insert("distance".into(), json!(d));
            }
            if let Some(v) = p.get("Priority").and_then(num) {
                u.insert("priority".into(), v);
            }
        }
        u.insert("calls".into(), Value::Array(list));
        // state / animation triggers (which call an object's action starts)
        let actions: Vec<Value> = x
            .user_actions(name)
            .map_err(fmt_err(&slink_path))?
            .unwrap_or_default()
            .iter()
            .map(|t| {
                let mut v = json!({"action": t.action, "call": t.call});
                if let Some(a) = &t.after {
                    v["after"] = json!(a);
                }
                if t.start_frame > 0 {
                    v["start"] = json!(t.start_frame);
                }
                if let Some(e) = t.end_frame {
                    v["end"] = json!(e);
                }
                v
            })
            .collect();
        if !actions.is_empty() {
            u.insert("actions".into(), Value::Array(actions));
        }
        users.insert(name.clone(), Value::Object(u));
    }
    progress("sound users", names.len(), names.len());
    report.users = users.len();

    let found = find_waves(romfs, &waves, progress)?;
    let mut wave_files = Map::new();
    for w in &waves {
        let Some((stream, bytes)) = found.get(w) else {
            report.missing.push(w.clone());
            continue;
        };
        let rel = format!(
            "{}/{}.bwav",
            if *stream { "music" } else { "se" },
            wave_file_stem(w)
        );
        write(&out.join("audio").join(&rel), bytes)?;
        if *stream {
            report.streams += 1;
        } else {
            report.waves += 1;
        }
        wave_files.insert(w.clone(), json!(rel));
    }

    let doc = json!({
        "version": SOUND_DB_VERSION,
        "users": users,
        "groups": groups_json(romfs)?,
        "attenuation": attenuation_json(romfs)?,
        "effects": effects_json(romfs)?,
        "ground": ground_json(romfs)?,
        "waves": wave_files,
    });
    write(
        &out.join("audio/sound.json"),
        serde_json::to_string(&doc).unwrap_or_default().as_bytes(),
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_and_key_filters() {
        assert!(gameplay_user("M1_Kuribo"));
        assert!(gameplay_user("3W_Player"));
        assert!(gameplay_user("CourseSceneBGM_WU"));
        assert!(gameplay_user("ObjectCommon"));
        assert!(!gameplay_user("M1_EditKuribo"));
        assert!(!gameplay_user("UITitle"));
        assert!(!gameplay_user("Quest_Block"));
        assert!(menu_music_key("BGM_Plain_Edit"));
        assert!(menu_music_key("BGM_Title_Day"));
        assert!(menu_music_key("Network_Clear"));
        assert!(!menu_music_key("BGM_Plain_Day_Hurry"));
        assert!(!menu_music_key("FanfareGoal"));
        assert_eq!(wave_file_stem("A b/c"), "A_b_c");
        assert_eq!(effect_theme("Haunted"), Some(5));
        assert_eq!(effect_theme("AirShip"), Some(3));
    }
}
