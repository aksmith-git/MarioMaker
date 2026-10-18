//! The imported effect data (`<cache>/effects/effects.json`, written by
//! `mmk_import::effects`): emitter sets and the effect links of every course object.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

/// An animation key: value (x, y, z) at time `t` (w).
pub type Key = [f32; 4];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyTable {
    pub keys: Vec<Key>,
    pub loop_frames: i32,
    pub loop_random: bool,
}

impl KeyTable {
    /// The value at `t` (piecewise linear; held before the first and after the last key).
    pub fn eval(&self, t: f32) -> [f32; 3] {
        eval_keys(&self.keys, t, false)
    }
}

/// Piecewise linear (or stepped) value of `keys` at `t`.
pub fn eval_keys(keys: &[Key], t: f32, step: bool) -> [f32; 3] {
    let Some(first) = keys.first() else {
        return [1.0; 3];
    };
    if keys.len() == 1 || t <= first[3] {
        return [first[0], first[1], first[2]];
    }
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t < b[3] {
            if step || b[3] <= a[3] {
                return [a[0], a[1], a[2]];
            }
            let f = (t - a[3]) / (b[3] - a[3]);
            return [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ];
        }
    }
    let last = keys[keys.len() - 1];
    [last[0], last[1], last[2]]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Fixed,
    Anim,
    Random,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampler {
    pub texture: Option<String>,
    pub wrap: [u8; 2],
    pub filter: u8,
    pub repeat: [f32; 2],
    pub pattern: u8,
    pub div: [u32; 2],
    pub table: Vec<u32>,
    pub period: f32,
    pub random_cells: u32,
    pub scroll: [f32; 2],
    pub uv_scale: [f32; 2],
}

/// An emitter animation: kind (EA**), keys over the emitter's frames.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterAnim {
    pub kind: String,
    pub looped: bool,
    pub step: bool,
    pub keys: Vec<Key>,
}

impl EmitterAnim {
    pub fn at(&self, frame: f32) -> [f32; 3] {
        let mut t = frame;
        if self.looped
            && let Some(last) = self.keys.last()
            && last[3] > 0.0
        {
            t = frame.rem_euclid(last[3]);
        }
        eval_keys(&self.keys, t, self.step)
    }
}

/// One emitter's parameters (see `mmk_formats::vfxb::EmitterParams`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmitterDef {
    pub name: String,
    pub follow: u8,
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
    pub color0: [f32; 3],
    pub alpha0: f32,
    pub one_time: bool,
    pub start: u32,
    pub duration: i32,
    pub rate: f32,
    pub rate_random: i32,
    pub interval: i32,
    pub interval_random: i32,
    pub fade_out: Option<i32>,
    pub fade_in: Option<i32>,
    pub shape: u8,
    pub shape_uniform: bool,
    pub sweep_start: f32,
    pub sweep: f32,
    pub latitude: f32,
    pub fill: f32,
    pub extent: [f32; 3],
    pub shape_scale: [f32; 3],
    pub line_length: f32,
    pub line_center: f32,
    pub division: u32,
    pub life: i32,
    pub life_random: i32,
    pub infinite_life: bool,
    pub omni: f32,
    pub dir_speed: f32,
    pub dir: [f32; 3],
    pub spread: f32,
    pub xz_diffusion: f32,
    pub random_vel: [f32; 3],
    pub speed_random: f32,
    pub inherit_vel: f32,
    pub position_random: f32,
    pub air: f32,
    pub gravity: f32,
    pub gravity_dir: [f32; 3],
    pub size: [f32; 3],
    pub size_random: [f32; 3],
    pub rot_axes: [bool; 3],
    pub rot_init: [f32; 3],
    pub rot_init_random: [f32; 3],
    pub rot_vel: [f32; 3],
    pub rot_vel_random: [f32; 3],
    pub particle_type: u8,
    pub blend: u8,
    pub color0_mode: ColorMode,
    pub color0_fixed: [f32; 3],
    pub color0_keys: KeyTable,
    pub alpha0_mode: ColorMode,
    pub alpha0_fixed: f32,
    pub alpha0_keys: KeyTable,
    pub scale_keys: KeyTable,
    pub samplers: Vec<Sampler>,
    pub anims: Vec<EmitterAnim>,
    pub children: Vec<EmitterDef>,
}

impl EmitterDef {
    pub fn anim(&self, kind: &str) -> Option<&EmitterAnim> {
        self.anims.iter().find(|a| a.kind == kind)
    }
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn fd(v: &Value, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(d)
}

fn v3(v: &Value, d: f32) -> [f32; 3] {
    [fd(&v[0], d), fd(&v[1], d), fd(&v[2], d)]
}

fn i(v: &Value) -> i32 {
    v.as_i64().unwrap_or(0) as i32
}

fn keys(v: &Value) -> Vec<Key> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|k| [f(&k[0]), f(&k[1]), f(&k[2]), f(&k[3])])
                .collect()
        })
        .unwrap_or_default()
}

fn table(v: &Value) -> KeyTable {
    KeyTable {
        keys: keys(&v["keys"]),
        loop_frames: i(&v["loop"]),
        loop_random: v["loop_random"].as_bool().unwrap_or(false),
    }
}

fn mode(v: &Value) -> ColorMode {
    match v.as_str() {
        Some("anim") => ColorMode::Anim,
        Some("random") => ColorMode::Random,
        _ => ColorMode::Fixed,
    }
}

fn opt_i(v: &Value) -> Option<i32> {
    v.as_i64().map(|x| x as i32)
}

impl EmitterDef {
    pub fn from_json(v: &Value) -> Self {
        let b = |k: &str| v[k].as_bool().unwrap_or(false);
        let axes = &v["rot_axes"];
        Self {
            name: v["name"].as_str().unwrap_or_default().to_string(),
            follow: i(&v["follow"]) as u8,
            translate: v3(&v["translate"], 0.0),
            rotate: v3(&v["rotate"], 0.0),
            scale: v3(&v["scale"], 1.0),
            color0: v3(&v["color0"], 1.0),
            alpha0: fd(&v["alpha0"], 1.0),
            one_time: b("one_time"),
            start: v["start"].as_u64().unwrap_or(0) as u32,
            duration: i(&v["duration"]),
            rate: f(&v["rate"]),
            rate_random: i(&v["rate_random"]),
            interval: i(&v["interval"]),
            interval_random: i(&v["interval_random"]),
            fade_out: opt_i(&v["fade_out"]),
            fade_in: opt_i(&v["fade_in"]),
            shape: i(&v["shape"]) as u8,
            shape_uniform: b("shape_uniform"),
            sweep_start: f(&v["sweep_start"]),
            sweep: f(&v["sweep"]),
            latitude: f(&v["latitude"]),
            fill: fd(&v["fill"], 1.0),
            extent: v3(&v["extent"], 0.0),
            shape_scale: v3(&v["shape_scale"], 1.0),
            line_length: f(&v["line_length"]),
            line_center: f(&v["line_center"]),
            division: v["division"].as_u64().unwrap_or(1) as u32,
            life: i(&v["life"]),
            life_random: i(&v["life_random"]),
            infinite_life: b("infinite_life"),
            omni: f(&v["omni"]),
            dir_speed: f(&v["dir_speed"]),
            dir: v3(&v["dir"], 0.0),
            spread: f(&v["spread"]),
            xz_diffusion: f(&v["xz_diffusion"]),
            random_vel: v3(&v["random_vel"], 0.0),
            speed_random: f(&v["speed_random"]),
            inherit_vel: f(&v["inherit_vel"]),
            position_random: f(&v["position_random"]),
            air: fd(&v["air"], 1.0),
            gravity: f(&v["gravity"]),
            gravity_dir: v3(&v["gravity_dir"], 0.0),
            size: v3(&v["size"], 1.0),
            size_random: v3(&v["size_random"], 0.0),
            rot_axes: [
                axes[0].as_bool().unwrap_or(false),
                axes[1].as_bool().unwrap_or(false),
                axes[2].as_bool().unwrap_or(false),
            ],
            rot_init: v3(&v["rot_init"], 0.0),
            rot_init_random: v3(&v["rot_init_random"], 0.0),
            rot_vel: v3(&v["rot_vel"], 0.0),
            rot_vel_random: v3(&v["rot_vel_random"], 0.0),
            particle_type: i(&v["particle_type"]) as u8,
            blend: i(&v["blend"]) as u8,
            color0_mode: mode(&v["color0_mode"]),
            color0_fixed: v3(&v["color0_fixed"], 1.0),
            color0_keys: table(&v["color0_keys"]),
            alpha0_mode: mode(&v["alpha0_mode"]),
            alpha0_fixed: fd(&v["alpha0_fixed"], 1.0),
            alpha0_keys: table(&v["alpha0_keys"]),
            scale_keys: table(&v["scale_keys"]),
            samplers: v["samplers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|s| Sampler {
                            texture: s["texture"].as_str().map(str::to_string),
                            wrap: [i(&s["wrap"][0]) as u8, i(&s["wrap"][1]) as u8],
                            filter: i(&s["filter"]) as u8,
                            repeat: [fd(&s["repeat"][0], 1.0), fd(&s["repeat"][1], 1.0)],
                            pattern: i(&s["pattern"]) as u8,
                            div: [
                                s["div"][0].as_u64().unwrap_or(1).max(1) as u32,
                                s["div"][1].as_u64().unwrap_or(1).max(1) as u32,
                            ],
                            table: s["table"]
                                .as_array()
                                .map(|t| t.iter().map(|x| x.as_u64().unwrap_or(0) as u32).collect())
                                .unwrap_or_default(),
                            period: fd(&s["period"], 1.0),
                            random_cells: s["random_cells"].as_u64().unwrap_or(0) as u32,
                            scroll: [f(&s["scroll"][0]), f(&s["scroll"][1])],
                            uv_scale: [fd(&s["uv_scale"][0], 1.0), fd(&s["uv_scale"][1], 1.0)],
                        })
                        .collect()
                })
                .unwrap_or_default(),
            anims: v["anims"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| EmitterAnim {
                            kind: x["kind"].as_str().unwrap_or_default().to_string(),
                            looped: x["loop"].as_bool().unwrap_or(false),
                            step: x["step"].as_bool().unwrap_or(false),
                            keys: keys(&x["keys"]),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            children: v["children"]
                .as_array()
                .map(|a| a.iter().map(EmitterDef::from_json).collect())
                .unwrap_or_default(),
        }
    }
}

/// How an asset is placed and tuned (the link's parameters).
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub set: String,
    pub scale: f32,
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    pub color: [f32; 4],
    pub rate: f32,
    pub life: f32,
    pub dir_vel: f32,
    pub delay: i32,
    pub duration: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    Always,
    Cmp(String, Value),
    Weight(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CallKind {
    Asset(Asset),
    Switch(String),
    Random,
    Blend,
    Sequence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallDef {
    pub key: String,
    pub cond: Cond,
    pub kind: CallKind,
    pub children: Option<(usize, usize)>,
    pub top: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct User {
    pub calls: Vec<CallDef>,
    pub keys: HashMap<String, usize>,
    pub actions: HashMap<String, Vec<String>>,
}

/// What the game asks of a property, as the conditions spell it.
pub trait Props {
    /// The property's value (`watch` as the link names it), or None if the engine doesn't know.
    fn get(&self, watch: &str) -> Option<String>;
}

impl<F: Fn(&str) -> Option<String>> Props for F {
    fn get(&self, watch: &str) -> Option<String> {
        self(watch)
    }
}

fn matches(op: &str, want: &Value, have: Option<&str>) -> bool {
    let Some(have) = have else {
        return op == "!=";
    };
    let eq = match want {
        Value::String(s) => s == have,
        Value::Number(n) => have.parse::<f64>().ok() == n.as_f64(),
        _ => false,
    };
    let num = || Some((have.parse::<f64>().ok()?, want.as_f64()?));
    match op {
        "==" => eq,
        "!=" => !eq,
        ">" => num().is_some_and(|(h, w)| h > w),
        ">=" => num().is_some_and(|(h, w)| h >= w),
        "<" => num().is_some_and(|(h, w)| h < w),
        "<=" => num().is_some_and(|(h, w)| h <= w),
        _ => false,
    }
}

impl User {
    fn from_json(v: &Value) -> Self {
        let mut calls = Vec::new();
        let mut keys = HashMap::new();
        for c in v["calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let cond = if let Some(w) = c["weight"].as_f64() {
                Cond::Weight(w as f32)
            } else if let Some(a) = c["if"].as_array() {
                Cond::Cmp(
                    a.first()
                        .and_then(Value::as_str)
                        .unwrap_or("==")
                        .to_string(),
                    a.get(1).cloned().unwrap_or(Value::Null),
                )
            } else {
                Cond::Always
            };
            let kind = match c["kind"].as_str() {
                Some("switch") => CallKind::Switch(c["watch"].as_str().unwrap_or_default().into()),
                Some("random") => CallKind::Random,
                Some("sequence") => CallKind::Sequence,
                Some(_) => CallKind::Blend,
                None => {
                    let p = |k: &str, d: f32| fd(&c[k], d);
                    let col = &c["color"];
                    CallKind::Asset(Asset {
                        set: c["set"].as_str().unwrap_or_default().to_string(),
                        scale: p("scale", 1.0),
                        pos: v3(&c["pos"], 0.0),
                        rot: v3(&c["rot"], 0.0),
                        color: [
                            fd(&col[0], 1.0),
                            fd(&col[1], 1.0),
                            fd(&col[2], 1.0),
                            fd(&col[3], 1.0),
                        ],
                        rate: p("rate", 1.0),
                        life: p("life", 1.0),
                        dir_vel: p("dir_vel", 1.0),
                        delay: p("delay", 0.0) as i32,
                        duration: p("duration", 0.0) as i32,
                    })
                }
            };
            let top = c.get("parent").is_none();
            let key = c["key"].as_str().unwrap_or_default().to_string();
            if top {
                keys.insert(key.clone(), calls.len());
            }
            calls.push(CallDef {
                key,
                cond,
                kind,
                children: c["children"].as_array().and_then(|a| {
                    Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize))
                }),
                top,
            });
        }
        let actions = v["actions"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        let keys = v
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (k.clone(), keys)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            calls,
            keys,
            actions,
        }
    }

    /// The assets call `i` plays with these properties (a random container picks with `pick`,
    /// a value in [0, 1)).
    fn resolve(&self, i: usize, props: &dyn Props, pick: f32, out: &mut Vec<Asset>) {
        let Some(c) = self.calls.get(i) else { return };
        let kids = || c.children.map(|(a, b)| a..=b).into_iter().flatten();
        match &c.kind {
            CallKind::Asset(a) => out.push(a.clone()),
            CallKind::Blend | CallKind::Sequence => {
                for k in kids() {
                    self.resolve(k, props, pick, out);
                }
            }
            CallKind::Random => {
                let all: Vec<(usize, f32)> = kids()
                    .map(|k| {
                        let w = match self.calls.get(k).map(|c| &c.cond) {
                            Some(Cond::Weight(w)) => *w,
                            _ => 1.0,
                        };
                        (k, w.max(0.0))
                    })
                    .collect();
                let total: f32 = all.iter().map(|(_, w)| w).sum();
                let mut r = pick * total;
                for (k, w) in &all {
                    if r < *w {
                        self.resolve(*k, props, pick, out);
                        return;
                    }
                    r -= w;
                }
                if let Some((k, _)) = all.last() {
                    self.resolve(*k, props, pick, out);
                }
            }
            CallKind::Switch(watch) => {
                let have = props.get(watch);
                let mut fallback = None;
                for k in kids() {
                    match self.calls.get(k).map(|c| &c.cond) {
                        Some(Cond::Cmp(op, want)) if matches(op, want, have.as_deref()) => {
                            self.resolve(k, props, pick, out);
                            return;
                        }
                        Some(Cond::Always | Cond::Weight(_)) if fallback.is_none() => {
                            fallback = Some(k);
                        }
                        _ => {}
                    }
                }
                if let Some(k) = fallback {
                    self.resolve(k, props, pick, out);
                }
            }
        }
    }
}

/// Every emitter set and effect user of an asset cache.
#[derive(Debug, Default)]
pub struct EffectLibrary {
    pub sets: HashMap<String, Arc<Vec<EmitterDef>>>,
    pub users: HashMap<String, User>,
}

impl EffectLibrary {
    pub fn from_json(v: &Value) -> Self {
        let sets = v["sets"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        let ems = v
                            .as_array()
                            .map(|a| a.iter().map(EmitterDef::from_json).collect())
                            .unwrap_or_default();
                        (k.clone(), Arc::new(ems))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let users = v["users"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), User::from_json(v)))
                    .collect()
            })
            .unwrap_or_default();
        Self { sets, users }
    }

    /// Parse `effects.json`; an empty library on error (a cache imported before effects).
    pub fn from_bytes(b: &[u8]) -> Self {
        serde_json::from_slice::<Value>(b)
            .map(|v| Self::from_json(&v))
            .unwrap_or_default()
    }

    /// `<cache root>/effects/effects.json`, if there is one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(root: &std::path::Path) -> Self {
        std::fs::read(root.join("effects/effects.json"))
            .map(|b| Self::from_bytes(&b))
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    /// The user an object of `base` name uses in `style` ("M1_Kuribo", else "Kuribo").
    pub fn user(&self, style: &str, base: &str) -> Option<&User> {
        self.users
            .get(&format!("{style}_{base}"))
            .or_else(|| self.users.get(base))
    }

    /// The assets user `base` plays for `key` in `style`.
    pub fn resolve(
        &self,
        style: &str,
        base: &str,
        key: &str,
        props: &dyn Props,
        pick: f32,
    ) -> Vec<Asset> {
        let mut out = Vec::new();
        // the style's user, else the shared one (when it lacks the key)
        let users = [
            self.users.get(&format!("{style}_{base}")),
            self.users.get(base),
        ];
        if let Some((u, &i)) = users
            .into_iter()
            .flatten()
            .find_map(|u| Some((u, u.keys.get(key)?)))
        {
            u.resolve(i, props, pick, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lib() -> EffectLibrary {
        EffectLibrary::from_json(&json!({
            "version": 1,
            "users": {
                "WU_Kuribo": { "calls": [
                    { "key": "Die", "kind": "switch", "children": [1, 2], "watch": "skin" },
                    { "key": "Die_WU", "parent": 0, "if": ["==", "WU"], "set": "Burst", "scale": 0.5 },
                    { "key": "Die_Etc", "parent": 0, "set": "Puff" }
                ], "actions": { "run": ["Die"] } }
            },
            "sets": { "Burst": [ { "name": "a", "rate": 2.0, "life": 10 } ] }
        }))
    }

    #[test]
    fn switch_picks_the_matching_child_or_the_fallback() {
        let l = lib();
        let wu = |p: &str| (p == "skin").then(|| "WU".to_string());
        let a = l.resolve("WU", "Kuribo", "Die", &wu, 0.0);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].set, "Burst");
        assert_eq!(a[0].scale, 0.5);
        let other = |_: &str| Some("M3".to_string());
        let a = l.resolve("WU", "Kuribo", "Die", &other, 0.0);
        assert_eq!(a[0].set, "Puff");
        assert!(l.resolve("M1", "Kuribo", "Die", &other, 0.0).is_empty());
        assert_eq!(l.user("WU", "Kuribo").unwrap().actions["run"], vec!["Die"]);
        assert_eq!(l.sets["Burst"][0].rate, 2.0);
    }

    #[test]
    fn keys_interpolate_and_hold() {
        let k = [[0.0, 0.0, 0.0, 0.0], [1.0, 2.0, 3.0, 0.5]];
        assert_eq!(eval_keys(&k, -1.0, false), [0.0; 3]);
        assert_eq!(eval_keys(&k, 0.25, false), [0.5, 1.0, 1.5]);
        assert_eq!(eval_keys(&k, 0.25, true), [0.0; 3]);
        assert_eq!(eval_keys(&k, 2.0, false), [1.0, 2.0, 3.0]);
    }
}
