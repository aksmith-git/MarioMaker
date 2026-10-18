//! Effects stage: the particle effects the course objects play, from the user's RomFS into
//! `<cache>/effects/` and `<cache>/textures/Effect/`.
//!
//! - `ELink2/ELink2DB.szs` (`mmk_formats::xlink`, `xlink_triggers`): per effect user (the
//!   course objects of each game style, the player, the common users) its event keys → asset
//!   trees (switch / random / blend / sequence containers with their conditions; assets naming
//!   an emitter set with scale, offset, colour, emission and life scales), and the action
//!   triggers (an animation or state name → keys played while it lasts). User names are kept
//!   in the database's own name table, so every user is found by name.
//! - `Effect/Static.Nin_NX_NVN.szs` (`mmk_formats::vfxb`): the emitter sets those assets name,
//!   with their emitters' parameters, and their textures (BNTX) → `textures/Effect/<name>.mtex`.
//!
//! Output `effects/effects.json` (read by `mmk_host::effects`):
//!
//! ```text
//! { "version": 1,
//!   "users": { "<user>": { "calls": [ <call>, … ], "actions": { "<action>": ["<key>", …] } } },
//!   "sets": { "<set>": [ <emitter>, … ] } }
//! call: { "key", "parent"?, "if"?: [op, value] | "weight"?,
//!         container: "kind": "switch"|"random"|"blend"|"sequence", "children": [a, b], "watch"
//!         asset: "set", and when not default "scale", "pos": [x, y, z], "rot": [x, y, z],
//!                "color": [r, g, b, a], "rate", "life", "dir_vel", "delay", "duration" }
//! emitter: the decoded `EmitterParams` (texture ids replaced by texture names), "anims":
//!          [{ "kind", "loop", "step", "keys" }], "children": [ <emitter>, … ]
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mmk_assets::MtexVariant;
use mmk_formats::archive::{self, Sarc};
use mmk_formats::bntx;
use mmk_formats::vfxb::{ColorMode, Emitter, EmitterParams, KeyTable, Vfxb};
use mmk_formats::xlink::{Call, Compare, CondValue, Condition, ContainerKind, Param, Xlnk};
use mmk_formats::xlink_triggers;
use serde_json::{Map, Value, json};

use crate::{ImportError, Progress, Result, fmt_err, read, to_mtex, write};

pub const EFFECTS_VERSION: u32 = 1;
/// Texture "model" of the effect textures in the cache.
pub const TEXTURE_MODEL: &str = "Effect";

#[derive(Clone, Copy, Debug, Default)]
pub struct EffectCounts {
    pub users: usize,
    pub sets: usize,
    pub textures: usize,
}

/// The single member of a Yaz0-wrapped SARC.
fn sarc_member(path: &Path) -> Result<Vec<u8>> {
    let data = archive::unwrap(read(path)?).map_err(fmt_err(path))?;
    let sarc = Sarc::parse(data).map_err(fmt_err(path))?;
    let (_, member) = sarc
        .files()
        .next()
        .ok_or_else(|| ImportError::Invalid(format!("{}: empty archive", path.display())))?;
    Ok(member.to_vec())
}

/// Users of the game's courses: not the editor's, the menus', the story hub's or the online
/// screens' (whose names start with these).
fn course_user(name: &str) -> bool {
    const SKIP: &[&str] = &[
        "Edit",
        "UI",
        "Quest",
        "MyWorld",
        "Layout",
        "Mii",
        "SoundEditor",
        "Lyt",
    ];
    let base = match name.split_once('_') {
        Some((p, rest)) if ["M1", "M3", "MW", "WU", "3W"].contains(&p) => rest,
        _ => name,
    };
    !SKIP
        .iter()
        .any(|s| base.starts_with(s) || name.starts_with(s))
}

/// Every name in the database's name table that is a user.
fn user_names(db: &[u8], x: &Xlnk) -> Vec<String> {
    let name_pos = u32::from_le_bytes([db[0x44], db[0x45], db[0x46], db[0x47]]) as usize;
    let mut out = BTreeSet::new();
    for s in db.get(name_pos..).unwrap_or_default().split(|&b| b == 0) {
        if s.is_empty() {
            continue;
        }
        if let Ok(n) = std::str::from_utf8(s)
            && x.has_user(n)
        {
            out.insert(n.to_string());
        }
    }
    out.into_iter().collect()
}

fn num(p: Option<&Param>) -> Option<f32> {
    p.and_then(Param::as_f32)
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

/// One call as JSON; None for an asset that plays nothing (`@Blank`).
fn call_json(c: &Call) -> Option<Value> {
    let mut m = Map::new();
    m.insert("key".into(), json!(c.key));
    if let Some(p) = c.parent {
        m.insert("parent".into(), json!(p));
    }
    if let Some((k, v)) = c.condition.as_ref().and_then(cond_json) {
        m.insert(k.into(), v);
    }
    if c.container {
        let kind = match c.kind {
            Some(ContainerKind::Switch) => "switch",
            Some(ContainerKind::Random | ContainerKind::Random2) => "random",
            Some(ContainerKind::Blend) => "blend",
            Some(ContainerKind::Sequence) => "sequence",
            _ => "blend",
        };
        m.insert("kind".into(), json!(kind));
        if let Some((a, b)) = c.children {
            m.insert("children".into(), json!([a, b]));
        }
        if let Some((w, _)) = &c.watch {
            m.insert("watch".into(), json!(w));
        }
        return Some(Value::Object(m));
    }
    let set = c.params.get("RuntimeAssetName").and_then(Param::as_str)?;
    if set.is_empty() || set.starts_with('@') {
        return None;
    }
    m.insert("set".into(), json!(set));
    let p = |k: &str| num(c.params.get(k));
    if let Some(s) = p("Scale").filter(|&s| s != 1.0) {
        m.insert("scale".into(), json!(s));
    }
    let v3 = |a: &str, b: &str, c: &str| [p(a), p(b), p(c)];
    let pos = v3("PositionX", "PositionY", "PositionZ");
    if pos.iter().any(Option::is_some) {
        m.insert("pos".into(), json!(pos.map(|v| v.unwrap_or(0.0))));
    }
    let rot = v3("RotationX", "RotationY", "RotationZ");
    if rot.iter().any(Option::is_some) {
        m.insert("rot".into(), json!(rot.map(|v| v.unwrap_or(0.0))));
    }
    let col = [p("Red"), p("Green"), p("Blue"), p("Alpha")];
    if col.iter().any(Option::is_some) {
        m.insert("color".into(), json!(col.map(|v| v.unwrap_or(1.0))));
    }
    for (k, name) in [
        ("rate", "EmissionRate"),
        ("life", "LifeScale"),
        ("dir_vel", "DirectionalVel"),
        ("delay", "Delay"),
        ("duration", "Duration"),
        ("interval", "EmissionInterval"),
        ("emit_scale", "EmissionScale"),
    ] {
        if let Some(v) = p(name) {
            m.insert(k.into(), json!(v));
        }
    }
    Some(Value::Object(m))
}

/// Engine names for watched properties, by the values their conditions compare against (the
/// game style, the course theme, day / night, the object's size, the liquid it touches, how a
/// hit landed, the player's form and character). Other properties keep the game's name.
fn property_role(calls: &[Call], container: &Call) -> Option<&'static str> {
    const ROLES: &[(&str, &[&str])] = &[
        ("style", &["M1", "M3", "MW", "WU", "3W"]),
        (
            "theme",
            &[
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
            ],
        ),
        ("time", &["cDay", "cNight"]),
        ("size", &["Big", "Normal", "Middle"]),
        ("liquid", &["Water", "Lava", "Poison"]),
        ("reaction", &["Invalid", "Damage", "Die", "SmallDamage"]),
        (
            "player_mode",
            &[
                "Normal", "Link", "Balloon", "Flying", "Neko", "Sippo", "Mant", "Mega",
            ],
        ),
        ("player", &["Mario", "Luigi", "Kinopio", "Kinopico"]),
    ];
    let (a, b) = container.children?;
    let values: Vec<&str> = calls
        .get(a..=b)?
        .iter()
        .filter_map(|c| match &c.condition {
            Some(Condition::Switch {
                value: CondValue::Enum(s),
                ..
            }) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        return None;
    }
    ROLES
        .iter()
        .find(|(_, set)| values.iter().all(|v| set.contains(v)))
        .map(|(role, _)| *role)
}

/// The user's calls with blank assets removed (and the containers left without children),
/// parent and child indices renumbered.
fn user_calls(calls: &[Call]) -> Vec<Value> {
    let mut keep: Vec<Option<Value>> = calls
        .iter()
        .map(|c| {
            let mut v = call_json(c)?;
            if let Some(role) = c.watch.as_ref().and_then(|_| property_role(calls, c)) {
                v["watch"] = json!(role);
            }
            Some(v)
        })
        .collect();
    // drop containers whose children were all dropped (repeat: nested containers)
    loop {
        let mut changed = false;
        for (i, c) in calls.iter().enumerate() {
            if !c.container || keep[i].is_none() {
                continue;
            }
            let alive = c
                .children
                .is_some_and(|(a, b)| (a..=b).any(|k| keep.get(k).is_some_and(Option::is_some)));
            if !alive {
                keep[i] = None;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut new_index = vec![usize::MAX; calls.len()];
    let mut n = 0;
    for (i, k) in keep.iter().enumerate() {
        if k.is_some() {
            new_index[i] = n;
            n += 1;
        }
    }
    let mut out = Vec::with_capacity(n);
    for (i, k) in keep.into_iter().enumerate() {
        let Some(mut v) = k else { continue };
        if let Some(p) = calls[i].parent {
            v["parent"] = json!(new_index[p]);
        }
        if let Some((a, b)) = calls[i].children {
            let kids: Vec<usize> = (a..=b)
                .filter(|&k| new_index.get(k).is_some_and(|&x| x != usize::MAX))
                .map(|k| new_index[k])
                .collect();
            if let (Some(&lo), Some(&hi)) = (kids.first(), kids.last()) {
                v["children"] = json!([lo, hi]);
            }
        }
        out.push(v);
    }
    out
}

fn keys_json(t: &KeyTable) -> Value {
    let mut m = Map::new();
    m.insert("keys".into(), json!(t.keys));
    if t.loop_frames > 0 {
        m.insert("loop".into(), json!(t.loop_frames));
        m.insert("loop_random".into(), json!(t.loop_random));
    }
    Value::Object(m)
}

fn mode(m: ColorMode) -> &'static str {
    match m {
        ColorMode::Fixed => "fixed",
        ColorMode::Anim => "anim",
        ColorMode::Random => "random",
    }
}

fn emitter_json(e: &Emitter, v: &Vfxb, textures: &mut BTreeSet<u64>) -> Value {
    let p: EmitterParams = e.params();
    let samplers: Vec<Value> = p
        .samplers
        .iter()
        .map(|s| {
            let tex = s.texture.and_then(|id| {
                let name = v.texture_name(id)?;
                textures.insert(id);
                Some(name.to_string())
            });
            let pt = &s.pattern;
            json!({
                "texture": tex,
                "wrap": s.wrap,
                "filter": s.filter,
                "repeat": s.repeat,
                "pattern": pt.mode,
                "div": [pt.div_u, pt.div_v],
                "table": pt.table,
                "period": pt.period,
                "random_cells": pt.random_cells,
                "scroll": pt.scroll,
                "uv_scale": pt.uv_scale,
            })
        })
        .collect();
    let anims: Vec<Value> = e
        .anims()
        .iter()
        .map(|a| {
            json!({
                "kind": String::from_utf8_lossy(&a.kind),
                "loop": a.looped,
                "step": a.step,
                "keys": a.keys,
            })
        })
        .collect();
    let children: Vec<Value> = e
        .children
        .iter()
        .map(|c| emitter_json(c, v, textures))
        .collect();
    let mut m = Map::new();
    m.insert("name".into(), json!(p.name));
    m.insert("calc".into(), json!(p.calc_type));
    m.insert("follow".into(), json!(p.follow));
    m.insert("translate".into(), json!(p.translate));
    m.insert("rotate".into(), json!(p.rotate));
    m.insert("scale".into(), json!(p.scale));
    m.insert("color0".into(), json!(p.color0));
    m.insert("alpha0".into(), json!(p.alpha0));
    m.insert("color1".into(), json!(p.color1));
    m.insert("alpha1".into(), json!(p.alpha1));
    m.insert("one_time".into(), json!(p.one_time));
    m.insert("start".into(), json!(p.start));
    m.insert("duration".into(), json!(p.duration));
    m.insert("rate".into(), json!(p.rate));
    m.insert("rate_random".into(), json!(p.rate_random));
    m.insert("interval".into(), json!(p.interval));
    m.insert("interval_random".into(), json!(p.interval_random));
    m.insert("fade_out".into(), json!(p.fade_out));
    m.insert("fade_in".into(), json!(p.fade_in));
    m.insert("shape".into(), json!(p.shape));
    m.insert("shape_uniform".into(), json!(p.shape_uniform));
    m.insert("sweep_start".into(), json!(p.sweep_start));
    m.insert("sweep".into(), json!(p.sweep));
    m.insert("latitude".into(), json!(p.latitude));
    m.insert("fill".into(), json!(p.fill));
    m.insert("extent".into(), json!(p.extent));
    m.insert("shape_scale".into(), json!(p.shape_scale));
    m.insert("line_length".into(), json!(p.line_length));
    m.insert("line_center".into(), json!(p.line_center));
    m.insert("division".into(), json!(p.division));
    m.insert("life".into(), json!(p.life));
    m.insert("life_random".into(), json!(p.life_random));
    m.insert("infinite_life".into(), json!(p.infinite_life));
    m.insert("omni".into(), json!(p.omni));
    m.insert("dir_speed".into(), json!(p.dir_speed));
    m.insert("dir".into(), json!(p.dir));
    m.insert("spread".into(), json!(p.spread));
    m.insert("xz_diffusion".into(), json!(p.xz_diffusion));
    m.insert("random_vel".into(), json!(p.random_vel));
    m.insert("speed_random".into(), json!(p.speed_random));
    m.insert("inherit_vel".into(), json!(p.inherit_vel));
    m.insert("position_random".into(), json!(p.position_random));
    m.insert("air".into(), json!(p.air));
    m.insert("gravity".into(), json!(p.gravity));
    m.insert("gravity_dir".into(), json!(p.gravity_dir));
    m.insert("gravity_local".into(), json!(p.gravity_local));
    m.insert("size".into(), json!(p.size));
    m.insert("size_random".into(), json!(p.size_random));
    m.insert("rot_axes".into(), json!(p.rot_axes));
    m.insert("rot_init".into(), json!(p.rot_init));
    m.insert("rot_init_random".into(), json!(p.rot_init_random));
    m.insert("rot_vel".into(), json!(p.rot_vel));
    m.insert("rot_vel_random".into(), json!(p.rot_vel_random));
    m.insert("particle_type".into(), json!(p.particle_type));
    m.insert("blend".into(), json!(p.blend));
    m.insert("depth_test".into(), json!(p.depth_test));
    m.insert("color0_mode".into(), json!(mode(p.color0_mode)));
    m.insert("color0_fixed".into(), json!(p.color0_fixed));
    m.insert("color0_keys".into(), json!(keys_json(&p.color0_keys)));
    m.insert("alpha0_mode".into(), json!(mode(p.alpha0_mode)));
    m.insert("alpha0_fixed".into(), json!(p.alpha0_fixed));
    m.insert("alpha0_keys".into(), json!(keys_json(&p.alpha0_keys)));
    m.insert("color1_mode".into(), json!(mode(p.color1_mode)));
    m.insert("color1_fixed".into(), json!(p.color1_fixed));
    m.insert("color1_keys".into(), json!(keys_json(&p.color1_keys)));
    m.insert("alpha1_mode".into(), json!(mode(p.alpha1_mode)));
    m.insert("alpha1_fixed".into(), json!(p.alpha1_fixed));
    m.insert("alpha1_keys".into(), json!(keys_json(&p.alpha1_keys)));
    m.insert("scale_keys".into(), json!(keys_json(&p.scale_keys)));
    m.insert("samplers".into(), json!(samplers));
    m.insert("primitive".into(), json!(p.primitive.is_some()));
    m.insert("anims".into(), json!(anims));
    m.insert("children".into(), json!(children));
    Value::Object(m)
}

/// Import the effects of the courses into `out` (an asset cache folder whose texture stage
/// has run: the effect textures are added to its `textures/index.json`).
pub fn import_effects(romfs: &Path, out: &Path, progress: Progress) -> Result<EffectCounts> {
    let link_path = romfs.join("ELink2/ELink2DB.szs");
    let ptcl_path = romfs.join("Effect/Static.Nin_NX_NVN.szs");
    let db = sarc_member(&link_path)?;
    let x = Xlnk::parse(&db).map_err(fmt_err(&link_path))?;
    let names = user_names(&db, &x);
    let mut users = Map::new();
    let mut wanted_sets = BTreeSet::new();
    for (i, name) in names.iter().enumerate() {
        progress("effect links", i, names.len());
        if !course_user(name) {
            continue;
        }
        let Some(calls) = x.user_calls(name).map_err(fmt_err(&link_path))? else {
            continue;
        };
        let calls = user_calls(&calls);
        for c in &calls {
            if let Some(s) = c["set"].as_str() {
                wanted_sets.insert(s.to_string());
            }
        }
        let mut actions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let live: BTreeSet<&str> = calls
            .iter()
            .filter(|c| c.get("parent").is_none())
            .filter_map(|c| c["key"].as_str())
            .collect();
        for t in xlink_triggers::action_triggers(&db, name)
            .map_err(fmt_err(&link_path))?
            .unwrap_or_default()
        {
            if live.contains(t.key.as_str()) {
                let keys = actions.entry(t.action).or_default();
                if !keys.contains(&t.key) {
                    keys.push(t.key);
                }
            }
        }
        if calls.is_empty() {
            continue;
        }
        let mut u = Map::new();
        u.insert("calls".into(), Value::Array(calls));
        if !actions.is_empty() {
            u.insert("actions".into(), json!(actions));
        }
        users.insert(name.clone(), Value::Object(u));
    }
    progress("effect links", names.len(), names.len());

    let ptcl = sarc_member(&ptcl_path)?;
    let v = Vfxb::parse(&ptcl).map_err(fmt_err(&ptcl_path))?;
    let mut sets = Map::new();
    let mut texture_ids = BTreeSet::new();
    for (i, s) in v.sets.iter().enumerate() {
        progress("effect sets", i, v.sets.len());
        if !wanted_sets.contains(&s.name) {
            continue;
        }
        let emitters: Vec<Value> = s
            .emitters
            .iter()
            .map(|e| emitter_json(e, &v, &mut texture_ids))
            .collect();
        sets.insert(s.name.clone(), Value::Array(emitters));
    }
    progress("effect sets", v.sets.len(), v.sets.len());

    // textures: the ones the kept emitters sample
    let wanted_tex: BTreeSet<&str> = texture_ids
        .iter()
        .filter_map(|&id| v.texture_name(id))
        .collect();
    let mut tex_entries = Map::new();
    if let Some(b) = v.bntx() {
        let textures = bntx::parse(b).map_err(fmt_err(&ptcl_path))?;
        for (i, t) in textures.iter().enumerate() {
            progress("effect textures", i, textures.len());
            if !wanted_tex.contains(t.name.as_str()) {
                continue;
            }
            let Some(m) = to_mtex(t) else { continue };
            write(
                &out.join(format!("textures/{TEXTURE_MODEL}/{}.mtex", t.name)),
                &m.to_bytes(),
            )?;
            tex_entries.insert(
                t.name.clone(),
                json!({ "w": m.width, "h": m.height, "format": m.format.name(),
                        "srgb": m.variant == MtexVariant::Srgb }),
            );
        }
    }
    let index_path = out.join("textures/index.json");
    let mut index: Value = match read(&index_path) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    let counts = EffectCounts {
        users: users.len(),
        sets: sets.len(),
        textures: tex_entries.len(),
    };
    if let Some(obj) = index.as_object_mut() {
        obj.insert(TEXTURE_MODEL.into(), Value::Object(tex_entries));
    }
    write(
        &index_path,
        serde_json::to_string(&index).unwrap().as_bytes(),
    )?;
    write(
        &out.join("effects/effects.json"),
        serde_json::to_string(&json!({
            "version": EFFECTS_VERSION,
            "users": users,
            "sets": sets,
        }))
        .unwrap()
        .as_bytes(),
    )?;
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn course_users_skip_editor_and_menus() {
        assert!(course_user("M1_Kuribo"));
        assert!(course_user("PlayerMario"));
        assert!(!course_user("WU_EditKuribo"));
        assert!(!course_user("EditPlayer"));
        assert!(!course_user("UIEditMain"));
        assert!(!course_user("Quest_Player"));
    }

    #[test]
    fn blank_assets_and_empty_containers_are_dropped() {
        let asset = |key: &str, set: &str, parent: Option<usize>| Call {
            key: key.into(),
            params: [("RuntimeAssetName".to_string(), Param::Str(set.to_string()))]
                .into_iter()
                .collect(),
            container: false,
            parent,
            conditional: false,
            kind: None,
            children: None,
            watch: None,
            condition: None,
        };
        let container = |key: &str, a: usize, b: usize| Call {
            key: key.into(),
            params: BTreeMap::new(),
            container: true,
            parent: None,
            conditional: false,
            kind: Some(ContainerKind::Switch),
            children: Some((a, b)),
            watch: Some(("skin".into(), true)),
            condition: None,
        };
        let calls = vec![
            container("Empty", 2, 2),
            container("Hit", 3, 4),
            asset("Empty_M1", "@Blank", Some(0)),
            asset("Hit_M1", "@Blank", Some(1)),
            asset("Hit_WU", "Cmn_Hit", Some(1)),
        ];
        let out = user_calls(&calls);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["key"], "Hit");
        assert_eq!(out[0]["children"], json!([1, 1]));
        assert_eq!(out[1]["set"], "Cmn_Hit");
        assert_eq!(out[1]["parent"], 0);
    }
}
