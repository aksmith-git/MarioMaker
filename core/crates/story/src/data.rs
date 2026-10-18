//! The Story data the importer writes into the asset cache (never in git): the job, build and
//! flag tables, the hub map objects and the event flows.
//!
//! Cache layout (written by `mmk_import::modes`):
//!
//! ```text
//! story/index.json      {"version":1, "flows":[names], "lesson_flows":[names]}
//! story/jobs.json       [{no, course_no, course, kind, style, difficulty, days, reward, giver,
//!                         return_tag, title, client, detail, goal}]
//! story/builds.json     [{no, cost, days, label}]
//! story/flags.json      [{name, kind}]
//! story/main_field.json the hub map (MainField) as JSON: {"Objs":[…], "Rails":[…]}
//! eventflow/<name>.bfevfl  the flows, as in the game files
//! ```
//!
//! Text (job titles, dialogue) stays in the game's message files; jobs carry the labels.

use std::collections::HashMap;
use std::io;

use mmk_formats::bfevfl;
use serde_json::Value;

use crate::flow::Flows;
use crate::state::FlagKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Normal,
    Hidden,
    Key,
}

impl JobKind {
    pub fn name(self) -> &'static str {
        match self {
            JobKind::Normal => "normal",
            JobKind::Hidden => "hidden",
            JobKind::Key => "key",
        }
    }
    pub fn from_name(s: &str) -> Self {
        match s {
            "hidden" => JobKind::Hidden,
            "key" => JobKind::Key,
            _ => JobKind::Normal,
        }
    }
}

/// What a job pays on its first clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reward {
    Coins(i32),
    /// A Mii-gear item (its data name).
    Gear(String),
    /// A hub change; the flow picks which by the job number (`LAND_IN` / `LAND_OUT`).
    Land {
        inside: bool,
    },
}

impl Reward {
    /// `getRewardKind`: 0 coins, 1 gear, 2 hub change.
    pub fn kind(&self) -> i32 {
        match self {
            Reward::Coins(_) => 0,
            Reward::Gear(_) => 1,
            Reward::Land { .. } => 2,
        }
    }
    pub fn coins(&self) -> i32 {
        match self {
            Reward::Coins(n) => *n,
            _ => 0,
        }
    }
    pub fn to_json(&self) -> Value {
        match self {
            Reward::Coins(n) => serde_json::json!({ "coins": n }),
            Reward::Gear(g) => serde_json::json!({ "gear": g }),
            Reward::Land { inside } => {
                serde_json::json!({ "land": if *inside { "in" } else { "out" } })
            }
        }
    }
    pub fn from_json(v: &Value) -> Self {
        if let Some(n) = v["coins"].as_i64() {
            Reward::Coins(n as i32)
        } else if let Some(g) = v["gear"].as_str() {
            Reward::Gear(g.to_string())
        } else {
            Reward::Land {
                inside: v["land"] == "in",
            }
        }
    }
}

/// The course's clear condition as the job window states it (a copy of the course file's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Goal {
    pub category: String,
    pub target: String,
    pub count: i32,
    pub full: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// QuestNo (= index, = `quest_index` in the flows).
    pub no: u8,
    /// Course file number: `Course/quest_%03d`, also the number of its text labels.
    pub course_no: u8,
    /// Built-in course id in the cache (`quest_%03d`).
    pub course: String,
    pub kind: JobKind,
    /// List icon style as the table gives it (`M1`, `M3`, `MW`, `WU`, `3W`); the course file's
    /// own style is what is played.
    pub style: String,
    /// Job window difficulty 0 Easy … 3 Very Hard.
    pub difficulty: u8,
    pub days: i32,
    pub reward: Reward,
    /// The NPC that offers it (empty: the Taskmaster's board).
    pub giver: String,
    /// Hub tag Mario is put at after the first clear (empty: where he left).
    pub return_tag: String,
    /// Message labels in `CommonMsg/Qst_Request` (title, client, detail).
    pub title: String,
    pub client: String,
    pub detail: String,
    pub goal: Goal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub no: u8,
    pub cost: i32,
    pub days: i32,
    /// Name label in `Qst_Etc` (`Build00` …).
    pub label: String,
}

/// A hub map object: gates decide whether it is there, events what talking to it does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HubObject {
    pub id: String,
    pub unit: String,
    pub instance: String,
    pub model: String,
    pub flow: String,
    pub entry: String,
    pub approach: String,
    pub appear: Gate,
    pub disappear: Gate,
    pub invalid: Gate,
    pub pos: [f32; 3],
}

/// `{type, index}` gate of a hub object (story-mode.md §1.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gate {
    pub kind: i32,
    /// A number, a comma list (all must hold) or a flag name.
    pub index: String,
}

impl Gate {
    fn from_json(o: &Value, prefix: &str) -> Self {
        let kind = o[format!("{prefix}FlagTypeName")].as_i64().unwrap_or(0) as i32;
        let index = match &o[format!("{prefix}FlagIndexOrName")] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        Self { kind, index }
    }
    /// The numbers of a numeric gate.
    pub fn numbers(&self) -> Vec<i32> {
        self.index
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct StoryData {
    pub jobs: Vec<Job>,
    pub builds: Vec<Build>,
    pub flags: HashMap<String, FlagKind>,
    pub hub: Vec<HubObject>,
    pub flows: Flows,
    /// Lesson flow names (lessons run on the same interpreter).
    pub lesson_flows: Vec<String>,
}

#[derive(Debug)]
pub enum DataError {
    Io(String, io::Error),
    Json(String, String),
    Flow(String, mmk_formats::Error),
    Missing(&'static str),
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataError::Io(p, e) => write!(f, "{p}: {e}"),
            DataError::Json(p, e) => write!(f, "{p}: {e}"),
            DataError::Flow(p, e) => write!(f, "{p}: {e}"),
            DataError::Missing(what) => write!(f, "story data has no {what}"),
        }
    }
}

impl std::error::Error for DataError {}

/// Reads one file of the cache by its path relative to the cache root.
pub type ReadFn<'a> = &'a dyn Fn(&str) -> io::Result<Vec<u8>>;

fn read_json(read: ReadFn, path: &str) -> Result<Value, DataError> {
    let bytes = read(path).map_err(|e| DataError::Io(path.into(), e))?;
    serde_json::from_slice(&bytes).map_err(|e| DataError::Json(path.into(), e.to_string()))
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}
fn n(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}

impl Job {
    pub fn from_json(v: &Value) -> Self {
        let g = &v["goal"];
        Self {
            no: n(v, "no") as u8,
            course_no: n(v, "course_no") as u8,
            course: s(v, "course"),
            kind: JobKind::from_name(v["kind"].as_str().unwrap_or("")),
            style: s(v, "style"),
            difficulty: n(v, "difficulty") as u8,
            days: v["days"].as_i64().unwrap_or(1) as i32,
            reward: Reward::from_json(&v["reward"]),
            giver: s(v, "giver"),
            return_tag: s(v, "return_tag"),
            title: s(v, "title"),
            client: s(v, "client"),
            detail: s(v, "detail"),
            goal: Goal {
                category: s(g, "category"),
                target: s(g, "target"),
                count: n(g, "count") as i32,
                full: g["full"].as_bool().unwrap_or(false),
            },
        }
    }
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "no": self.no, "course_no": self.course_no, "course": self.course,
            "kind": self.kind.name(), "style": self.style, "difficulty": self.difficulty,
            "days": self.days, "reward": self.reward.to_json(), "giver": self.giver,
            "return_tag": self.return_tag, "title": self.title, "client": self.client,
            "detail": self.detail,
            "goal": { "category": self.goal.category, "target": self.goal.target,
                      "count": self.goal.count, "full": self.goal.full },
        })
    }
}

impl Build {
    pub fn from_json(v: &Value) -> Self {
        Self {
            no: n(v, "no") as u8,
            cost: n(v, "cost") as i32,
            days: v["days"].as_i64().unwrap_or(1) as i32,
            label: s(v, "label"),
        }
    }
    pub fn to_json(&self) -> Value {
        serde_json::json!({ "no": self.no, "cost": self.cost, "days": self.days,
                            "label": self.label })
    }
}

impl HubObject {
    pub fn from_json(o: &Value) -> Self {
        let t = &o["Translate"];
        let f = |k: &str| t[k].as_f64().unwrap_or(0.0) as f32;
        Self {
            id: s(o, "Id"),
            unit: s(o, "UnitConfigName"),
            instance: s(o, "InstanceName"),
            model: s(o, "ModelName"),
            flow: s(o, "EventFlowFileName"),
            entry: s(o, "EventEntryPointName"),
            approach: s(o, "ApproachEntryPointName"),
            appear: Gate::from_json(o, "Appear"),
            disappear: Gate::from_json(o, "Disappear"),
            invalid: Gate::from_json(o, "Invalid"),
            pos: [f("X"), f("Y"), f("Z")],
        }
    }
}

impl StoryData {
    /// Load the Story data from an asset cache (paths relative to the cache root).
    pub fn load(read: ReadFn) -> Result<Self, DataError> {
        let index = read_json(read, "story/index.json")?;
        let jobs: Vec<Job> = read_json(read, "story/jobs.json")?
            .as_array()
            .map(|a| a.iter().map(Job::from_json).collect())
            .unwrap_or_default();
        let builds: Vec<Build> = read_json(read, "story/builds.json")?
            .as_array()
            .map(|a| a.iter().map(Build::from_json).collect())
            .unwrap_or_default();
        let flags = read_json(read, "story/flags.json")?
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some((
                            f["name"].as_str()?.to_string(),
                            FlagKind::from_name(f["kind"].as_str()?)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let hub = read_json(read, "story/main_field.json")
            .ok()
            .and_then(|m| m["Objs"].as_array().cloned())
            .map(|a| a.iter().map(HubObject::from_json).collect())
            .unwrap_or_default();
        let names = |k: &str| -> Vec<String> {
            index[k]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| Some(x.as_str()?.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut flows = Flows::new();
        let lesson_flows = names("lesson_flows");
        for name in names("flows").iter().chain(&lesson_flows) {
            let path = format!("eventflow/{name}.bfevfl");
            let bytes = read(&path).map_err(|e| DataError::Io(path.clone(), e))?;
            flows.add_file(bfevfl::parse(&bytes).map_err(|e| DataError::Flow(path, e))?);
        }
        if jobs.is_empty() {
            return Err(DataError::Missing("jobs"));
        }
        Ok(Self {
            jobs,
            builds,
            flags,
            hub,
            flows,
            lesson_flows,
        })
    }

    /// Load from a cache folder on disk (`<cache>/story/…`).
    pub fn load_dir(root: &std::path::Path) -> Result<Self, DataError> {
        Self::load(&|p| std::fs::read(root.join(p)))
    }

    pub fn job(&self, q: i32) -> Option<&Job> {
        usize::try_from(q).ok().and_then(|q| self.jobs.get(q))
    }
    pub fn build(&self, b: i32) -> Option<&Build> {
        usize::try_from(b).ok().and_then(|b| self.builds.get(b))
    }
    pub fn flag_kind(&self, name: &str) -> Option<FlagKind> {
        self.flags.get(name).copied()
    }
}
