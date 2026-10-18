//! What the front end needs from the platform: the course engine (load, step, read the HUD),
//! the course library and server, and a little persistent storage. The host implements
//! [`Host`]; the front end never touches files, sockets or the sim directly.

use serde_json::{Value, json};

/// A course file of the library (or a built-in course from the owner's game data).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CourseEntry {
    /// Where the host finds it (a path).
    pub id: String,
    /// The course's own name (may be empty), else a name the host made up.
    pub name: String,
    pub description: String,
    /// Game style "M1", "M3", "MW", "WU", "3W".
    pub style: String,
    /// Main-area theme (0 ground … 9 forest; 2 castle).
    pub theme: u8,
    /// The engine can play it.
    pub playable: bool,
    /// From the owner's game data (Story Mode, lessons, title courses), not the library folder.
    pub built_in: bool,
    /// "Story Mode", "Lesson", "Title" for built-in courses.
    pub collection: Option<String>,
    /// Save-data key (content hash, hex).
    pub key: String,
    /// Last-modified time of the file (seconds since 1970), when no index gives an upload date.
    pub modified: Option<i64>,
    pub meta: CourseMeta,
}

/// What a course list knows about a course beyond its file: the public dataset's `index.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CourseMeta {
    pub clear_rate: Option<f64>,
    /// SMM2 difficulty bucket 0 easy … 3 super expert.
    pub bucket: Option<u8>,
    pub maker: Option<String>,
    /// Course code ("XXX-XXX-XXX"), from the server data id.
    pub code: Option<String>,
    pub world_record_ms: Option<i64>,
    pub plays: Option<i64>,
    pub likes: Option<i64>,
    pub clears: Option<i64>,
    pub attempts: Option<i64>,
    /// Upload date, seconds since 1970.
    pub uploaded: Option<i64>,
    /// SMM2 tag ids (1 Standard … 15 Link).
    pub tags: Vec<u32>,
}

impl CourseMeta {
    /// 100 Mario difficulty value 0–100: the fail rate (100 − clear %), else a value inside the
    /// SMM2 bucket, else an easy value for unknown courses.
    pub fn difficulty_value(&self) -> u8 {
        if let Some(r) = self.clear_rate {
            return (100.0 - r).round().clamp(0.0, 100.0) as u8;
        }
        if let Some(b) = self.bucket {
            return [20, 50, 85, 98][(b as usize).min(3)];
        }
        30
    }

    /// The entries of a dataset `index.json` (`{"courses": [{"file", "clear_rate", …}]}`):
    /// (file path relative to the index, metadata). `code` turns a data id into a course code.
    pub fn parse_index(text: &str, code: impl Fn(u32) -> String) -> Vec<(String, CourseMeta)> {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for c in v["courses"].as_array().into_iter().flatten() {
            let Some(file) = c["file"].as_str() else {
                continue;
            };
            let n = |k: &str| c[k].as_i64();
            out.push((
                file.to_string(),
                CourseMeta {
                    clear_rate: c["clear_rate"].as_f64(),
                    bucket: n("difficulty").map(|d| d.clamp(0, 3) as u8),
                    maker: c["maker"].as_str().map(str::to_string),
                    code: c["data_id"].as_u64().map(|id| code(id as u32)),
                    world_record_ms: n("world_record"),
                    plays: n("plays"),
                    likes: n("likes"),
                    clears: n("clears"),
                    attempts: n("attempts"),
                    uploaded: n("uploaded"),
                    tags: [n("tag1"), n("tag2")]
                        .into_iter()
                        .flatten()
                        .filter(|&t| t > 0)
                        .map(|t| t as u32)
                        .collect(),
                },
            ));
        }
        out
    }
}

/// A course offered by Course World: a library file or a server listing.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldCourse {
    pub id: String,
    pub source: Source,
    pub name: String,
    pub maker: Option<String>,
    pub style: String,
    pub description: String,
    pub plays: Option<i64>,
    pub likes: Option<i64>,
    pub clears: Option<i64>,
    pub attempts: Option<i64>,
    pub clear_rate: Option<f64>,
    pub code: Option<String>,
    /// Seconds since 1970.
    pub date: Option<i64>,
    /// Seconds to add to `date` for the local time shown.
    pub date_offset: i64,
    pub cleared: bool,
    pub best_ticks: Option<i64>,
    pub world_record_ms: Option<i64>,
    pub playable: bool,
    pub tags: Vec<u32>,
    /// Save-data key, when known before playing.
    pub key: Option<String>,
    /// Server courses: the uploader's maker ID (None: anonymous), the world-record holder's and
    /// the first clearer's maker IDs, and whether the record was proven by a replay (a ghost to
    /// race).
    pub maker_id: Option<String>,
    pub record_holder: Option<String>,
    pub first_clear: Option<String>,
    pub record_verified: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A library file (its [`CourseEntry::id`]).
    Library(String),
    /// A course on the course server, by code.
    Server(String),
}

impl WorldCourse {
    pub fn library(e: &CourseEntry, r: &CourseRecord) -> Self {
        let m = &e.meta;
        Self {
            id: e.id.clone(),
            source: Source::Library(e.id.clone()),
            name: e.name.clone(),
            maker: m.maker.clone(),
            style: e.style.clone(),
            description: e.description.clone(),
            // the dataset's statistics when the index has them, else this player's own
            plays: m.plays.or(Some(r.attempts)),
            likes: m.likes,
            clears: m.clears.or(Some(r.clears)),
            attempts: m.attempts.or(Some(r.attempts)),
            clear_rate: m.clear_rate,
            code: m.code.clone(),
            date: m.uploaded.or(e.modified),
            date_offset: 0,
            cleared: r.cleared,
            best_ticks: r.best_ticks,
            world_record_ms: m.world_record_ms,
            playable: e.playable,
            tags: m.tags.clone(),
            key: Some(e.key.clone()),
            maker_id: None,
            record_holder: None,
            first_clear: None,
            record_verified: false,
        }
    }

    /// A server search result (`mmk_online::listing_json`: code, name, maker, style, plays,
    /// clears, attempts, likes, description, uploaded, maker_id, record_ms, record_holder,
    /// first_clear, record_verified; missing keys are unknown).
    pub fn server(j: &Value) -> Option<Self> {
        let code = j["code"].as_str()?.to_string();
        let n = |k: &str| j[k].as_i64();
        let id = |k: &str| j[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        let plays = n("plays");
        let clears = n("clears");
        // attempts when the server counts them, else plays
        let attempts = n("attempts").filter(|&a| a > 0).or(plays);
        Some(Self {
            id: format!("server:{code}"),
            source: Source::Server(code.clone()),
            name: j["name"].as_str().unwrap_or(&code).to_string(),
            maker: id("maker"),
            style: j["style"].as_str().unwrap_or("M1").to_string(),
            description: j["description"].as_str().unwrap_or_default().to_string(),
            plays,
            likes: n("likes"),
            clears,
            attempts,
            clear_rate: match (attempts, clears) {
                (Some(p), Some(c)) if p > 0 => Some((c as f64 * 100.0 / p as f64).min(100.0)),
                _ => None,
            },
            code: Some(code),
            date: n("uploaded").filter(|&d| d > 0),
            date_offset: 0,
            cleared: false,
            best_ticks: None,
            world_record_ms: n("record_ms").filter(|&r| r > 0),
            playable: true,
            tags: Vec::new(),
            key: None,
            maker_id: id("maker_id"),
            record_holder: id("record_holder"),
            first_clear: id("first_clear"),
            record_verified: j["record_verified"].as_bool().unwrap_or(false),
        })
    }

    pub fn server_code(&self) -> Option<&str> {
        match &self.source {
            Source::Server(c) => Some(c),
            Source::Library(_) => None,
        }
    }

    pub fn is_server(&self) -> bool {
        matches!(self.source, Source::Server(_))
    }
}

/// Per-course progress, keyed by the course's content hash (the Swift app's `CourseRecord`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CourseRecord {
    pub cleared: bool,
    /// Fastest clear in 60 Hz ticks.
    pub best_ticks: Option<i64>,
    pub attempts: i64,
    pub clears: i64,
}

/// The save store: `{"version": 1, "courses": {"<key>": record}}`, kept through
/// [`Host::store_value`] under "saves".
#[derive(Clone, Debug, Default)]
pub struct Saves {
    courses: serde_json::Map<String, Value>,
}

impl Saves {
    pub fn load(host: &mut dyn Host) -> Self {
        let courses = host
            .load_value("saves")
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["courses"].as_object().cloned())
            .unwrap_or_default();
        Self { courses }
    }

    pub fn record(&self, key: &str) -> CourseRecord {
        let Some(r) = self.courses.get(key) else {
            return CourseRecord::default();
        };
        CourseRecord {
            cleared: r["cleared"].as_bool().unwrap_or(false),
            best_ticks: r["bestTicks"].as_i64(),
            attempts: r["attempts"].as_i64().unwrap_or(0),
            clears: r["clears"].as_i64().unwrap_or(0),
        }
    }

    fn put(&mut self, host: &mut dyn Host, key: &str, r: &CourseRecord) {
        let mut o = json!({"cleared": r.cleared, "attempts": r.attempts, "clears": r.clears});
        if let Some(b) = r.best_ticks {
            o["bestTicks"] = json!(b);
        }
        self.courses.insert(key.to_string(), o);
        let doc = json!({"version": 1, "courses": Value::Object(self.courses.clone())});
        host.store_value("saves", &doc.to_string());
    }

    pub fn add_attempt(&mut self, host: &mut dyn Host, key: &str) {
        let mut r = self.record(key);
        r.attempts += 1;
        self.put(host, key, &r);
    }

    /// True if this is a new best time.
    pub fn add_clear(&mut self, host: &mut dyn Host, key: &str, ticks: i64) -> bool {
        let mut r = self.record(key);
        r.cleared = true;
        r.clears += 1;
        let best = r.best_ticks.is_none_or(|b| ticks < b);
        if best {
            r.best_ticks = Some(ticks);
        }
        self.put(host, key, &r);
        best
    }
}

/// The course's clear condition and its progress (as `mmk_engine_clear_condition`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Condition {
    /// 0 none, 1 defeat N (target = enemy class), 2 collect N 1-Ups, 3 collect N coins (target =
    /// coin value), 4 POW blocks, 5 P Switches, 6 star, 7 Super, 8 Fire, 9 no damage, 10 no
    /// landing, 11 shoe, … (see `mmk_ffi::condition`).
    pub kind: u32,
    pub target: u32,
    pub count: u32,
    pub needed: u32,
    pub met: bool,
    pub violated: bool,
}

/// What the screens read from the engine after each tick (the Swift app's `MmkHud` +
/// `MmkPlayStatus` + `MmkCondition`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CourseStatus {
    pub score: u32,
    pub coins: u32,
    pub lives: u32,
    /// Seconds left, −1 = no timer.
    pub time: i32,
    pub cleared: bool,
    pub dying: bool,
    /// The player touched the goal this tick (`Event::Goal`).
    pub goal_event: bool,
    /// Extra lives earned since the course was loaded.
    pub lives_gained: u32,
    pub keys: u32,
    pub pink_coins: u32,
    pub pink_total: u32,
    /// 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU, 4 3D World.
    pub style: u32,
    pub character: u32,
    pub checkpoint: bool,
    /// Dying because the time ran out.
    pub time_up: bool,
    /// Player 1 in the 384 × 216 view: px from its bottom-left (feet).
    pub player_x: f32,
    pub player_y: f32,
    /// Frames until the death sequence restarts the course; −1 while alive.
    pub restart_in: i32,
    /// The sky colour (0xRRGGBBAA), for the title logo.
    pub clear_color: u32,
    pub condition: Option<Condition>,
}

/// A course loaded into the engine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadedCourse {
    pub name: String,
    pub key: Option<String>,
    pub theme: u8,
}

/// The platform side of the front end.
pub trait Host {
    /// Library courses (the course folder, then the built-in courses of the owner's data).
    fn library(&mut self) -> Vec<CourseEntry>;
    /// Start fetching the course server's list (false: no server configured).
    fn request_server_courses(&mut self) -> bool;
    /// The server's courses once they arrived (empty if it couldn't be reached).
    fn poll_server_courses(&mut self) -> Option<Vec<WorldCourse>>;
    /// Load a course into the engine (a library file, or a server course: download it).
    fn load_course(&mut self, source: &Source) -> Result<LoadedCourse, String>;
    /// How many title demo courses there are; load one (false: unreadable).
    fn title_courses(&mut self) -> usize;
    fn load_title_course(&mut self, index: usize) -> bool;
    /// No course any more (back to menus).
    fn unload_course(&mut self);
    /// One engine tick: `scripted` replaces the player's input (title demo), else the player's
    /// own input (START masked: the front end handles pausing).
    fn step_course(&mut self, scripted: Option<u32>);
    fn course_status(&self) -> CourseStatus;
    /// Start the loaded course over.
    fn restart_course(&mut self);
    /// Player 1's character: 0 Mario, 1 Luigi, 2 Toad, 3 Toadette.
    fn set_character(&mut self, character: u32);
    /// Start the loaded course from a 100 Mario run's counters.
    fn set_run_counters(&mut self, score: u32, coins: u32, lives: u32);
    /// Local time minus UTC (seconds) at `secs` since 1970, for showing dates.
    fn local_offset(&self, _secs: i64) -> i64 {
        0
    }
    /// The play is paused (music down) or not.
    fn set_paused(&mut self, _paused: bool) {}
    /// Tell the server about a play / clear of a server course (best effort).
    fn report(&mut self, _code: &str, _what: &str) {}
    /// A course was cleared (e.g. send the replay); `ticks` its time.
    fn cleared(&mut self, _ticks: i64) {}
    /// Start a call to the course server: the JSON actions of `mmk_online::call` (`register
    /// {name}`, `me`, `info {code}`, `vote {code, like}`, `clear {code, time_ms}`, `comments
    /// {code}`, `comment {code, text}`, `maker {maker_id}`, `rankings {kind, limit}`, `endless
    /// {difficulty, cleared}`), plus two the host carries out with its engine:
    /// `submit_replay {code}` (the replay of the run just cleared) and `ghost {code}` (download
    /// the record's replay and race it from the next start of that course). `token`: the
    /// signed-in account's. The answer comes back from [`Host::poll_online`] under `id`.
    /// False: no server configured (nothing will come back).
    fn online(&mut self, _id: u32, _action: &str, _args: &Value, _token: Option<&str>) -> bool {
        false
    }
    /// The next answer of an earlier [`Host::online`] call, once it arrived.
    fn poll_online(&mut self) -> Option<(u32, Result<Value, String>)> {
        None
    }
    /// The engine's play options change play (SMM1 extras, `mmk_sim::Options::affects_play`):
    /// replays are then neither sent nor raced.
    fn play_options_affect_play(&self) -> bool {
        false
    }
    /// Small persistent values (save data, settings) by key.
    fn load_value(&mut self, key: &str) -> Option<String>;
    fn store_value(&mut self, key: &str, value: &str);
    /// A random number (opening choice, title course, 100 Mario seed).
    fn random(&mut self) -> u64;
}

/// Engine buttons (`mmk_sim::input`) for scripted demo input.
pub mod buttons {
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const UP: u32 = 4;
    pub const DOWN: u32 = 8;
    pub const JUMP: u32 = 16;
    pub const RUN: u32 = 32;
}
