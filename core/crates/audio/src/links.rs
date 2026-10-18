//! The gameplay sound database (`<cache>/audio/sound.json`, written by the importer from the
//! game's own sound links): per sound user (a course object of one game style, the player, the
//! course music) its event keys, each an asset or a tree of containers that pick assets by the
//! emitter's properties, plus the sound groups (voice limits, ducking), distance attenuation
//! sets, per-theme effects and the wave files.
//!
//! [`SoundDb::resolve`] walks a key's tree for one play: a switch takes the first child whose
//! condition on the watched property holds (an unconditioned child is the fallback), a random
//! container one child by weight, a blend every child, a sequence the next child in turn.

use std::collections::HashMap;

use serde_json::Value;

/// A property value an emitter (or the course) reports.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Str(String),
    Num(f32),
}

impl From<&str> for PropValue {
    fn from(s: &str) -> Self {
        PropValue::Str(s.to_string())
    }
}

impl From<f32> for PropValue {
    fn from(v: f32) -> Self {
        PropValue::Num(v)
    }
}

impl From<i32> for PropValue {
    fn from(v: i32) -> Self {
        PropValue::Num(v as f32)
    }
}

/// Emitter and course properties, looked up by name (`@style`, `@theme`, `@hour`, `@ground`,
/// `@scene` for the course-wide ones the importer names by role; the game's names otherwise).
pub trait Props {
    fn get(&self, name: &str) -> Option<PropValue>;
}

/// A property list.
#[derive(Clone, Debug, Default)]
pub struct PropList(pub Vec<(String, PropValue)>);

impl PropList {
    pub fn with(mut self, name: &str, v: impl Into<PropValue>) -> Self {
        self.set(name, v);
        self
    }
    pub fn set(&mut self, name: &str, v: impl Into<PropValue>) {
        let v = v.into();
        match self.0.iter_mut().find(|(n, _)| n == name) {
            Some(e) => e.1 = v,
            None => self.0.push((name.to_string(), v)),
        }
    }
}

impl Props for PropList {
    fn get(&self, name: &str) -> Option<PropValue> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }
}

/// Local properties first, then the course's.
pub struct Layered<'a>(pub &'a dyn Props, pub &'a dyn Props);

impl Props for Layered<'_> {
    fn get(&self, name: &str) -> Option<PropValue> {
        self.0.get(name).or_else(|| self.1.get(name))
    }
}

/// A number parameter: fixed, random in a range each play, or a curve over a property.
#[derive(Clone, Debug, PartialEq)]
pub enum Num {
    Const(f32),
    Range(f32, f32),
    /// min, max, and the power the uniform draw is raised to.
    RangePow(f32, f32, f32),
    Curve {
        property: String,
        points: Vec<(f32, f32)>,
    },
}

impl Num {
    fn parse(v: &Value) -> Option<Self> {
        if let Some(f) = v.as_f64() {
            return Some(Num::Const(f as f32));
        }
        if let (Some(a), Some(b)) = (v["min"].as_f64(), v["max"].as_f64()) {
            return Some(match v["pow"].as_f64() {
                Some(p) => Num::RangePow(a as f32, b as f32, p as f32),
                None => Num::Range(a as f32, b as f32),
            });
        }
        let property = v["curve"].as_str()?.to_string();
        let mut points: Vec<(f32, f32)> = v["points"]
            .as_array()?
            .iter()
            .filter_map(|p| Some((p[0].as_f64()? as f32, p[1].as_f64()? as f32)))
            .collect();
        // the game's curves end with a stray point past their range; keep x ascending
        let mut last = f32::NEG_INFINITY;
        points.retain(|&(x, _)| {
            let keep = x >= last;
            last = last.max(x);
            keep
        });
        Some(Num::Curve { property, points })
    }

    fn eval(&self, props: &dyn Props, rng: &mut Rng) -> f32 {
        match self {
            Num::Const(v) => *v,
            Num::Range(a, b) => a + (b - a) * rng.unit(),
            Num::RangePow(a, b, p) => a + (b - a) * rng.unit().powf(*p),
            Num::Curve { property, points } => {
                let x = match props.get(property) {
                    Some(PropValue::Num(v)) => v,
                    _ => 0.0,
                };
                curve(points, x)
            }
        }
    }
}

/// Piecewise linear, clamped at the ends.
fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(&(x0, y0)) = points.first() else {
        return 1.0;
    };
    if x <= x0 {
        return y0;
    }
    for w in points.windows(2) {
        let ((a, ya), (b, yb)) = (w[0], w[1]);
        if x <= b {
            return if b > a {
                ya + (yb - ya) * (x - a) / (b - a)
            } else {
                yb
            };
        }
    }
    points.last().map(|p| p.1).unwrap_or(1.0)
}

/// Deterministic random numbers (xorshift32) for random containers and ranges.
#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u32);

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
    Ne,
}

#[derive(Clone, Debug, PartialEq)]
enum Cond {
    None,
    If(Cmp, PropValue),
    Weight(f32),
}

impl Cond {
    /// Whether a switch child's condition holds for `v` (None: the property is unknown).
    fn holds(&self, v: Option<&PropValue>) -> bool {
        let Cond::If(cmp, want) = self else {
            return false;
        };
        let Some(v) = v else {
            return false;
        };
        let ord = match (v, want) {
            (PropValue::Num(a), PropValue::Num(b)) => a.partial_cmp(b),
            (PropValue::Str(a), PropValue::Str(b)) => Some(a.cmp(b)),
            // a number compared with an enum name (or the reverse) never matches
            _ => return *cmp == Cmp::Ne,
        };
        let Some(o) = ord else { return false };
        use std::cmp::Ordering::*;
        match cmp {
            Cmp::Eq => o == Equal,
            Cmp::Ne => o != Equal,
            Cmp::Gt => o == Greater,
            Cmp::Ge => o != Less,
            Cmp::Lt => o == Less,
            Cmp::Le => o != Greater,
        }
    }
}

/// One asset of a key's tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub wave: Option<String>,
    pub volume: Option<Num>,
    pub pitch: Option<Num>,
    /// Frames before it starts.
    pub delay: Option<Num>,
    pub priority: Option<Num>,
    /// Fade-out frames when stopped [inf].
    pub stop: Option<Num>,
    pub fade_in: Option<Num>,
    pub lpf: Option<Num>,
    /// Frames it plays before stopping (silences that hold a channel; ambient loops).
    pub duration: Option<Num>,
    pub group: Option<String>,
    pub distance: Option<String>,
    /// Ignores the underwater filter.
    pub dry: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Asset(Box<Asset>),
    Switch { watch: String },
    Random,
    Blend,
    Sequence,
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    key: String,
    parent: Option<usize>,
    cond: Cond,
    children: Option<(usize, usize)>,
    kind: Kind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct User {
    nodes: Vec<Node>,
    /// Top-level keys → node index.
    keys: HashMap<String, usize>,
    pub distance: Option<String>,
    pub priority: f32,
}

impl User {
    pub fn has(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(String::as_str)
    }
}

/// A sound group (the game's group tree): voice limit, volume, groups it ducks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: String,
    pub parent: Option<u16>,
    pub volume: f32,
    pub limit: Option<u16>,
    pub limit_type: u8,
    /// (group, volume, fade-in s, fade-out s)
    pub duck: Vec<(u16, f32, f32, f32)>,
}

/// Distance attenuation: linear from `near_gain` at `min` to `far_gain` at `max`; sounds
/// farther than `cull` don't start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attenuation {
    pub min: f32,
    pub max: f32,
    pub near_gain: f32,
    pub far_gain: f32,
    pub cull: Option<f32>,
}

impl Attenuation {
    pub fn gain(&self, d: f32) -> f32 {
        if self.max <= self.min || d <= self.min {
            return self.near_gain;
        }
        let t = ((d - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        self.near_gain + (self.far_gain - self.near_gain) * t
    }
}

/// A theme's effect parameters (delay and/or reverb), as the game's effect files give them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeFx {
    pub delay: Option<HashMap<String, f32>>,
    pub reverb: Option<HashMap<String, f32>>,
}

/// One resolved asset to play.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub wave: String,
    pub volume: f32,
    pub pitch: f32,
    pub delay: f32,
    pub priority: f32,
    pub stop: f32,
    pub fade_in: f32,
    pub lpf: f32,
    pub duration: f32,
    pub group: Option<u16>,
    pub distance: Option<String>,
    pub dry: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SoundDb {
    pub users: HashMap<String, User>,
    pub groups: Vec<Group>,
    group_ids: HashMap<String, u16>,
    pub attenuation: HashMap<String, Attenuation>,
    pub effects: HashMap<String, Vec<ThemeFx>>,
    /// Wave name → file relative to `audio/`.
    pub waves: HashMap<String, String>,
    /// Ground material per style, terrain part and theme value (`@ground` values).
    pub ground: HashMap<String, HashMap<String, HashMap<String, String>>>,
}

/// Sequence containers' next child, per (user, node).
pub type Turns = HashMap<(String, usize), usize>;

fn fx(v: &Value) -> Option<HashMap<String, f32>> {
    let p = v["params"].as_object()?;
    Some(
        p.iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_f64()? as f32)))
            .collect(),
    )
}

impl SoundDb {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let doc: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if doc["version"].as_u64() != Some(1) {
            return Err("unsupported sound database version".into());
        }
        let mut db = SoundDb::default();
        for (i, g) in doc["groups"].as_array().into_iter().flatten().enumerate() {
            db.group_ids
                .insert(g["name"].as_str().unwrap_or_default().to_string(), i as u16);
        }
        for g in doc["groups"].as_array().into_iter().flatten() {
            let id = |v: &Value| v.as_str().and_then(|n| db.group_ids.get(n).copied());
            db.groups.push(Group {
                name: g["name"].as_str().unwrap_or_default().to_string(),
                parent: id(&g["parent"]),
                volume: g["volume"].as_f64().unwrap_or(1.0) as f32,
                limit: g["limit"].as_u64().map(|v| v.min(u16::MAX as u64) as u16),
                limit_type: g["limit_type"].as_u64().unwrap_or(0) as u8,
                duck: g["duck"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| {
                        Some((
                            id(&d["group"])?,
                            d["volume"].as_f64()? as f32,
                            d["fade_in"].as_f64().unwrap_or(0.0) as f32,
                            d["fade_out"].as_f64().unwrap_or(0.0) as f32,
                        ))
                    })
                    .collect(),
            });
        }
        for (name, a) in doc["attenuation"].as_object().into_iter().flatten() {
            let f = |k: &str, d: f32| a[k].as_f64().map(|v| v as f32).unwrap_or(d);
            db.attenuation.insert(
                name.clone(),
                Attenuation {
                    min: f("min", 0.0),
                    max: f("max", 0.0),
                    near_gain: f("near_gain", 1.0),
                    far_gain: f("far_gain", 1.0),
                    cull: a["cull"].as_f64().map(|v| v as f32),
                },
            );
        }
        for (style, row) in doc["effects"].as_object().into_iter().flatten() {
            let list = row
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| ThemeFx {
                    delay: fx(&t["delay"]),
                    reverb: fx(&t["reverb"]),
                })
                .collect();
            db.effects.insert(style.clone(), list);
        }
        for (style, parts) in doc["ground"].as_object().into_iter().flatten() {
            let mut m = HashMap::new();
            for (part, themes) in parts.as_object().into_iter().flatten() {
                let t: HashMap<String, String> = themes
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect();
                m.insert(part.clone(), t);
            }
            db.ground.insert(style.clone(), m);
        }
        for (w, f) in doc["waves"].as_object().into_iter().flatten() {
            if let Some(f) = f.as_str() {
                db.waves.insert(w.clone(), f.to_string());
            }
        }
        for (name, u) in doc["users"].as_object().into_iter().flatten() {
            db.users.insert(name.clone(), Self::parse_user(u));
        }
        Ok(db)
    }

    fn parse_user(u: &Value) -> User {
        let mut user = User {
            distance: u["distance"].as_str().map(String::from),
            priority: u["priority"].as_f64().unwrap_or(0.5) as f32,
            ..User::default()
        };
        for (i, c) in u["calls"].as_array().into_iter().flatten().enumerate() {
            let parent = c["parent"].as_u64().map(|p| p as usize);
            let cond = if let Some(w) = c["weight"].as_f64() {
                Cond::Weight(w as f32)
            } else if let Some(arr) = c["if"].as_array() {
                let cmp = match arr.first().and_then(|v| v.as_str()) {
                    Some(">") => Cmp::Gt,
                    Some(">=") => Cmp::Ge,
                    Some("<") => Cmp::Lt,
                    Some("<=") => Cmp::Le,
                    Some("!=") => Cmp::Ne,
                    _ => Cmp::Eq,
                };
                let value = match arr.get(1) {
                    Some(Value::String(s)) => PropValue::Str(s.clone()),
                    Some(v) => PropValue::Num(v.as_f64().unwrap_or(0.0) as f32),
                    None => PropValue::Num(0.0),
                };
                Cond::If(cmp, value)
            } else {
                Cond::None
            };
            let children = c["children"]
                .as_array()
                .and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize)));
            let n = |k: &str| Num::parse(&c[k]);
            let s = |k: &str| c[k].as_str().map(String::from);
            let kind = match c["kind"].as_str() {
                Some("switch") => Kind::Switch {
                    watch: s("watch").unwrap_or_default(),
                },
                Some("random") => Kind::Random,
                Some("blend") => Kind::Blend,
                Some("sequence") => Kind::Sequence,
                _ => Kind::Asset(Box::new(Asset {
                    wave: s("wave"),
                    volume: n("volume"),
                    pitch: n("pitch"),
                    delay: n("delay"),
                    priority: n("priority"),
                    stop: n("stop"),
                    fade_in: n("fade_in"),
                    lpf: n("lpf"),
                    duration: n("duration"),
                    group: s("group"),
                    distance: s("distance"),
                    dry: c["dry"].as_bool().unwrap_or(false),
                })),
            };
            let key = c["key"].as_str().unwrap_or_default().to_string();
            if parent.is_none() {
                user.keys.entry(key.clone()).or_insert(i);
            }
            user.nodes.push(Node {
                key,
                parent,
                cond,
                children,
                kind,
            });
        }
        user
    }

    pub fn user(&self, name: &str) -> Option<&User> {
        self.users.get(name)
    }

    pub fn group_id(&self, name: &str) -> Option<u16> {
        self.group_ids.get(name).copied()
    }

    /// The assets user `user`'s `key` plays for one event with `props` (appended to `out`).
    /// False if there is no such user or key.
    pub fn resolve(
        &self,
        user: &str,
        key: &str,
        props: &dyn Props,
        rng: &mut Rng,
        turns: &mut Turns,
        out: &mut Vec<Play>,
    ) -> bool {
        let Some(u) = self.users.get(user) else {
            return false;
        };
        let Some(&root) = u.keys.get(key) else {
            return false;
        };
        let start = out.len();
        self.walk(user, u, root, props, rng, turns, out, 0);
        for p in &mut out[start..] {
            if p.distance.is_none() {
                p.distance = u.distance.clone();
            }
        }
        true
    }

    /// Every wave `user`'s `key` can play under any properties.
    pub fn waves_of(&self, user: &str, key: &str, out: &mut Vec<String>) {
        let Some(u) = self.users.get(user) else {
            return;
        };
        let Some(&root) = u.keys.get(key) else {
            return;
        };
        let mut stack = vec![(root, 0)];
        while let Some((i, depth)) = stack.pop() {
            let Some(n) = u.nodes.get(i) else { continue };
            match &n.kind {
                Kind::Asset(a) => {
                    if let Some(w) = &a.wave
                        && !out.contains(w)
                    {
                        out.push(w.clone());
                    }
                }
                _ if depth < 16 => {
                    if let Some((a, b)) = n.children {
                        stack.extend(
                            (a..=b.min(u.nodes.len().saturating_sub(1))).map(|j| (j, depth + 1)),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        user: &str,
        u: &User,
        i: usize,
        props: &dyn Props,
        rng: &mut Rng,
        turns: &mut Turns,
        out: &mut Vec<Play>,
        depth: usize,
    ) {
        if depth > 16 {
            return;
        }
        let Some(n) = u.nodes.get(i) else { return };
        let kids = || {
            n.children
                .map(|(a, b)| a..b.min(u.nodes.len().saturating_sub(1)) + 1)
                .unwrap_or(0..0)
        };
        match &n.kind {
            Kind::Asset(a) => {
                let Some(wave) = &a.wave else { return };
                let f = |v: &Option<Num>, d: f32, rng: &mut Rng| {
                    v.as_ref().map(|n| n.eval(props, rng)).unwrap_or(d)
                };
                let play = Play {
                    wave: wave.clone(),
                    volume: f(&a.volume, 1.0, rng),
                    pitch: f(&a.pitch, 1.0, rng),
                    delay: f(&a.delay, 0.0, rng),
                    priority: f(&a.priority, u.priority, rng),
                    stop: f(&a.stop, 0.0, rng),
                    fade_in: f(&a.fade_in, 0.0, rng),
                    lpf: f(&a.lpf, 0.0, rng),
                    duration: f(&a.duration, 0.0, rng),
                    group: a.group.as_deref().and_then(|g| self.group_id(g)),
                    distance: a.distance.clone(),
                    dry: a.dry,
                };
                out.push(play);
            }
            Kind::Switch { watch } => {
                let v = props.get(watch);
                let mut fallback = None;
                for j in kids() {
                    let c = &u.nodes[j];
                    if c.cond == Cond::None {
                        fallback.get_or_insert(j);
                    } else if c.cond.holds(v.as_ref()) {
                        self.walk(user, u, j, props, rng, turns, out, depth + 1);
                        return;
                    }
                }
                // an unknown property (the engine does not model it) takes the first child, the
                // normal case in the game's data (e.g. the local player before a remote one)
                if v.is_none() && fallback.is_none() {
                    fallback = kids().next();
                }
                if let Some(j) = fallback {
                    self.walk(user, u, j, props, rng, turns, out, depth + 1);
                }
            }
            Kind::Random => {
                let weights: Vec<(usize, f32)> = kids()
                    .map(|j| {
                        let w = match u.nodes[j].cond {
                            Cond::Weight(w) => w.max(0.0),
                            _ => 1.0,
                        };
                        (j, w)
                    })
                    .collect();
                let total: f32 = weights.iter().map(|w| w.1).sum();
                if weights.is_empty() || total <= 0.0 {
                    return;
                }
                let mut r = rng.unit() * total;
                let mut pick = weights[weights.len() - 1].0;
                for (j, w) in &weights {
                    if r < *w {
                        pick = *j;
                        break;
                    }
                    r -= w;
                }
                self.walk(user, u, pick, props, rng, turns, out, depth + 1);
            }
            Kind::Blend => {
                for j in kids() {
                    self.walk(user, u, j, props, rng, turns, out, depth + 1);
                }
            }
            Kind::Sequence => {
                let k = kids();
                if k.is_empty() {
                    return;
                }
                let len = k.len();
                let t = turns.entry((user.to_string(), i)).or_insert(0);
                let j = k.start + *t % len;
                *t = (*t + 1) % len;
                self.walk(user, u, j, props, rng, turns, out, depth + 1);
            }
        }
    }

    /// The key's node name at the top of a resolved play (for diagnostics).
    pub fn has_key(&self, user: &str, key: &str) -> bool {
        self.users.get(user).is_some_and(|u| u.has(key))
    }

    /// Key names of a node (diagnostics and tests).
    pub fn node_key(&self, user: &str, i: usize) -> Option<&str> {
        self.users.get(user)?.nodes.get(i).map(|n| n.key.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> SoundDb {
        let doc = json!({
            "version": 1,
            "groups": [
                {"name": "Root", "parent": null, "volume": 1.0},
                {"name": "SE_Sqr", "parent": "Root", "volume": 1.0, "limit": 1, "limit_type": 2,
                 "duck": [{"group": "Root", "volume": 0.5, "fade_in": 0.1, "fade_out": 0.2}]}
            ],
            "attenuation": {"Mid": {"min": 0.0, "max": 70.0, "near_gain": 1.0, "far_gain": 0.6, "cull": 290.0}},
            "effects": {"WU": [{"delay": {"type": "Delay", "params": {"Delay": 0.16}}, "reverb": null}]},
            "waves": {"J0": "se/J0.bwav"},
            "users": {
                "M1_Player": {"distance": "Mid", "calls": [
                    {"key": "jump", "kind": "switch", "watch": "PlayerMode", "children": [1, 3]},
                    {"key": "jump_0", "parent": 0, "if": ["==", "Normal"], "wave": "J0", "volume": 0.21, "group": "SE_Sqr"},
                    {"key": "jump_1", "parent": 0, "if": ["==", "Super"], "wave": "J1"},
                    {"key": "jump_d", "parent": 0, "wave": "JD", "pitch": {"min": 0.9, "max": 1.1}},
                    {"key": "combo", "kind": "switch", "watch": "DownComboCount", "children": [5, 7]},
                    {"key": "c1", "parent": 4, "if": ["<=", 1], "wave": "C1"},
                    {"key": "c2", "parent": 4, "if": ["==", 2], "wave": "C2"},
                    {"key": "c8", "parent": 4, "if": [">=", 8], "wave": "C8"},
                    {"key": "kick", "kind": "random", "children": [9, 10]},
                    {"key": "ka", "parent": 8, "weight": 1.0, "wave": "KA"},
                    {"key": "kb", "parent": 8, "weight": 1.0, "wave": "KB"},
                    {"key": "step", "kind": "sequence", "children": [12, 13]},
                    {"key": "l", "parent": 11, "wave": "L"},
                    {"key": "r", "parent": 11, "wave": "R", "delay": 3.0},
                    {"key": "both", "kind": "blend", "children": [15, 16]},
                    {"key": "b1", "parent": 14, "wave": "B1"},
                    {"key": "b2", "parent": 14, "wave": "B2"},
                    {"key": "note", "wave": "N", "pitch": {"curve": "MusicalScale", "points": [[0.0, 0.5], [12.0, 1.0], [24.0, 2.0], [3.0, 9.0]]}}
                ]}
            }
        });
        SoundDb::parse(&serde_json::to_vec(&doc).unwrap()).unwrap()
    }

    fn waves(
        db: &SoundDb,
        key: &str,
        props: &PropList,
        rng: &mut Rng,
        turns: &mut Turns,
    ) -> Vec<String> {
        let mut out = Vec::new();
        assert!(db.resolve("M1_Player", key, props, rng, turns, &mut out));
        out.into_iter().map(|p| p.wave).collect()
    }

    #[test]
    fn switch_picks_condition_then_fallback() {
        let db = db();
        let (mut rng, mut t) = (Rng(1), Turns::new());
        let p = PropList::default().with("PlayerMode", "Super");
        assert_eq!(waves(&db, "jump", &p, &mut rng, &mut t), ["J1"]);
        let p = PropList::default().with("PlayerMode", "Normal");
        let mut out = Vec::new();
        db.resolve("M1_Player", "jump", &p, &mut rng, &mut t, &mut out);
        assert_eq!(out[0].volume, 0.21);
        assert_eq!(out[0].group, db.group_id("SE_Sqr"));
        assert_eq!(
            out[0].distance.as_deref(),
            Some("Mid"),
            "user default distance set"
        );
        // unknown mode and missing property: the unconditioned fallback
        let p = PropList::default().with("PlayerMode", "Frog");
        assert_eq!(waves(&db, "jump", &p, &mut rng, &mut t), ["JD"]);
        assert_eq!(
            waves(&db, "jump", &PropList::default(), &mut rng, &mut t),
            ["JD"]
        );
    }

    #[test]
    fn numeric_conditions_random_sequence_blend() {
        let db = db();
        let (mut rng, mut t) = (Rng(7), Turns::new());
        for (n, w) in [(1, "C1"), (0, "C1"), (2, "C2"), (9, "C8")] {
            let p = PropList::default().with("DownComboCount", n);
            assert_eq!(waves(&db, "combo", &p, &mut rng, &mut t), [w], "combo {n}");
        }
        let none = PropList::default();
        let picks: Vec<String> = (0..64)
            .flat_map(|_| waves(&db, "kick", &none, &mut rng, &mut t))
            .collect();
        assert!(picks.iter().any(|w| w == "KA") && picks.iter().any(|w| w == "KB"));
        let seq: Vec<String> = (0..4)
            .flat_map(|_| waves(&db, "step", &none, &mut rng, &mut t))
            .collect();
        assert_eq!(seq, ["L", "R", "L", "R"]);
        assert_eq!(waves(&db, "both", &none, &mut rng, &mut t), ["B1", "B2"]);
    }

    #[test]
    fn ranges_and_curves() {
        let db = db();
        let (mut rng, mut t) = (Rng(3), Turns::new());
        let mut out = Vec::new();
        for _ in 0..32 {
            db.resolve(
                "M1_Player",
                "jump",
                &PropList::default(),
                &mut rng,
                &mut t,
                &mut out,
            );
        }
        assert!(out.iter().all(|p| (0.9..=1.1).contains(&p.pitch)));
        assert!(out.iter().any(|p| p.pitch != out[0].pitch));
        for (x, want) in [(0, 0.5), (6, 0.75), (12, 1.0), (24, 2.0), (30, 2.0)] {
            let p = PropList::default().with("MusicalScale", x);
            out.clear();
            db.resolve("M1_Player", "note", &p, &mut rng, &mut t, &mut out);
            assert!(
                (out[0].pitch - want).abs() < 1e-5,
                "scale {x}: {}",
                out[0].pitch
            );
        }
    }

    #[test]
    fn groups_attenuation_effects_waves() {
        let db = db();
        let g = &db.groups[db.group_id("SE_Sqr").unwrap() as usize];
        assert_eq!(g.limit, Some(1));
        assert_eq!(g.parent, Some(0));
        assert_eq!(g.duck, vec![(0, 0.5, 0.1, 0.2)]);
        let a = db.attenuation["Mid"];
        assert_eq!(a.gain(0.0), 1.0);
        assert!((a.gain(35.0) - 0.8).abs() < 1e-6);
        assert_eq!(a.gain(500.0), 0.6);
        assert_eq!(db.effects["WU"][0].delay.as_ref().unwrap()["Delay"], 0.16);
        assert_eq!(db.waves["J0"], "se/J0.bwav");
        let mut w = Vec::new();
        db.waves_of("M1_Player", "jump", &mut w);
        w.sort();
        assert_eq!(w, ["J0", "J1", "JD"]);
        let mut out = Vec::new();
        assert!(!db.resolve(
            "M1_Player",
            "nope",
            &PropList::default(),
            &mut Rng(1),
            &mut Turns::new(),
            &mut out
        ));
        assert!(!db.resolve(
            "X",
            "jump",
            &PropList::default(),
            &mut Rng(1),
            &mut Turns::new(),
            &mut out
        ));
    }
}
