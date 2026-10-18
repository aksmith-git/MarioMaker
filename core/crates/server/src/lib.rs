//! A small self-hostable course server (Phase 10): our own open protocol, not Nintendo's.
//!
//! Courses live in a directory: `courses/<code>.bin` (raw course data, validated on upload) and
//! `index.json` (metadata and play statistics). HTTP API (JSON unless noted):
//!
//! | method | path | does |
//! |---|---|---|
//! | POST | `/api/courses` (body: course data; header `X-Maker`) | upload → `{code, …}` |
//! | GET | `/api/courses?q=&style=&sort=new\|popular\|hard&limit=&offset=` | search |
//! | GET | `/api/courses/<code>` | metadata |
//! | GET | `/api/courses/<code>/data` | course data (bytes) |
//! | POST | `/api/courses/<code>/play`, `/clear` | statistics |
//! | GET | `/api/health` | liveness: counts and the replay engine version |
//! | GET | `/api/pool?min=&max=&count=` | random courses by difficulty value (fail rate 0–100) |
//! | POST | `/api/accounts` (body `{"name"}`) | new account → `{account, token}` (token shown once) |
//! | GET | `/api/me` | the caller's account |
//! | GET | `/api/makers/<maker id>` | a maker's profile and courses |
//! | POST | `/api/courses/<code>/like`, `/boo` | one vote per account (replaces its earlier vote) |
//! | POST | `/api/courses/<code>/clear` (body `{"time_ms"}`, optional) | clear; first clear / record |
//! | POST | `/api/courses/<code>/replay` (body: replay, `mmk_modes::replay`) | clear proven by a replay: re-simulated, its time can set a verified record |
//! | GET | `/api/courses/<code>/replay` | the verified record's replay (for ghosts) |
//! | GET, POST | `/api/courses/<code>/comments` (POST body `{"text"}`) | comments |
//! | DELETE | `/api/courses/<code>` | remove (the uploader, or the admin) |
//! | POST | `/api/admin/ban/<account id>`, `/api/admin/unban/<id>` | moderation (admin token) |
//! | POST | `/api/endless/<easy\|normal\|expert\|super_expert>` (body `{"cleared"}`, or `{"ruleset": "hundred"}` for a completed 100 Mario run) | a run the client held itself; keeps the best (100 Mario: counts the completion) |
//! | GET | `/api/endless` | Endless: the four runs (`RunStatus`), best clears, lifetime skips, last difficulty |
//! | POST | `/api/endless/<d>/start` (body optional `{"ruleset": "smm2"\|"hundred"}`) | New Game on `<d>` (wipes that run): SMM2's rules (5 lives), or SMM1's 100 Mario Challenge (100 lives, done after 8 / 16 / 16 / 6 clears, SMM1's fail-rate bands, free skips) → `{difficulty, run}` |
//! | GET | `/api/endless/<d>/next?count=` | the run's next courses (≤ 10, by clear-rate band) → `{courses, widened, run}` |
//! | POST | `/api/endless/<d>/result` (body `Report`: `{code, result, pending, coins, score, frames_left, during_miss}`) | a course result → `{lives, clears, passed, game_over, run_complete, best}` (100 Mario: `best` = completed runs) |
//! | POST | `/api/endless/<d>/suspend`, `/finish` | quit (run kept) → `{run}`; game over → `{clears, best, new_high_score}` |
//! | GET | `/api/endless/<d>/history` | the run's last 48 courses |
//! | GET | `/api/rankings/<maker_points\|clears\|first_clears\|records\|endless_<difficulty>\|hundred_<difficulty>>?limit=` | leaderboards; `me`: the signed-in caller's row |
//!
//! The Endless Challenge routes (`endless` module) need a signed-in account; the server holds
//! the run as SMM2's does.
//!
//! Accounts (`accounts` module) are a name and a secret token sent as `Authorization: Bearer
//! <token>`; uploads, votes and comments from an account carry its name and maker ID. Anonymous
//! uploads stay possible (header `X-Maker`). The admin token comes from `Store::with_admin`.
//!
//! The difficulty value is 100 − clear rate once a course has been played, as SMM1's 100 Mario
//! bands use it. Courses are identified by SMM2-style codes (XXX-XXX-XXX).

pub mod limits;
pub mod ws;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mmk_formats::course_id::smm2_data_id_to_code;
use mmk_formats::smm2::Course;
use serde_json::{Value, json};

pub mod accounts;
pub mod endless;

use accounts::Accounts;

/// Longest comment (characters).
pub const MAX_COMMENT: usize = 75;

/// Largest upload accepted (a course is 0x5BFC0 bytes raw).
pub const MAX_UPLOAD: usize = 0x60000;

#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub code: String,
    pub data_id: u32,
    pub name: String,
    pub description: String,
    pub maker: String,
    pub style: String,
    pub theme: u8,
    pub uploaded: u64,
    pub plays: u64,
    pub clears: u64,
    pub attempts: u64,
    pub likes: u64,
    pub boos: u64,
    /// Uploader's account (0 anonymous) and maker ID.
    pub owner: u32,
    pub maker_id: String,
    /// Best clear time (ms, 0 none) and its holder's maker ID; the first clearer's.
    pub record_ms: u64,
    pub record_holder: String,
    /// The record was set by a replay the server re-simulated (its run is at `…/replay`).
    pub record_verified: bool,
    pub first_clear: String,
}

impl Meta {
    /// Difficulty value 0–100 (fail rate); unplayed courses count as 50.
    pub fn difficulty(&self) -> u8 {
        (self.clears * 100)
            .checked_div(self.attempts)
            .map_or(50, |rate| (100 - rate.min(100)) as u8)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code, "name": self.name, "description": self.description,
            "maker": self.maker, "style": self.style, "theme": self.theme,
            "uploaded": self.uploaded, "plays": self.plays, "clears": self.clears,
            "attempts": self.attempts, "likes": self.likes, "difficulty": self.difficulty(),
            "boos": self.boos, "maker_id": self.maker_id, "record_ms": self.record_ms,
            "record_holder": self.record_holder, "first_clear": self.first_clear,
            "record_verified": self.record_verified,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        Some(Self {
            code: v["code"].as_str()?.to_string(),
            data_id: n("data_id") as u32,
            name: s("name"),
            description: s("description"),
            maker: s("maker"),
            style: s("style"),
            theme: n("theme") as u8,
            uploaded: n("uploaded"),
            plays: n("plays"),
            clears: n("clears"),
            attempts: n("attempts"),
            likes: n("likes"),
            boos: n("boos"),
            owner: n("owner") as u32,
            maker_id: s("maker_id"),
            record_ms: n("record_ms"),
            record_holder: s("record_holder"),
            record_verified: v["record_verified"].as_bool().unwrap_or(false),
            first_clear: s("first_clear"),
        })
    }
}

/// The course store (thread-safe; one per server).
pub struct Store {
    dir: PathBuf,
    inner: Mutex<Inner>,
    /// Moderation token (None: no admin).
    admin: Option<String>,
    /// Records only from verified replays (typed-in times count as clears only).
    verified_records: bool,
}

struct Inner {
    courses: Vec<Meta>,
    by_code: HashMap<String, usize>,
    next_id: u32,
    rng: u64,
    accounts: Accounts,
    /// code → account id → +1 like / −1 boo.
    votes: HashMap<String, HashMap<u32, i8>>,
    /// code → comments.
    comments: HashMap<String, Vec<Comment>>,
    /// maker ID → codes of the courses they cleared.
    cleared: HashMap<String, Vec<String>>,
    /// Endless Challenge: difficulty → maker ID → best run (courses cleared).
    endless: HashMap<String, HashMap<String, u64>>,
    /// Endless Challenge runs: maker ID → the four run records (`endless.json`).
    endless_runs: HashMap<String, mmk_modes::endless::EndlessSave>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub maker_id: String,
    pub name: String,
    pub text: String,
    pub posted: u64,
}

impl Comment {
    fn to_json(&self) -> Value {
        json!({ "maker_id": self.maker_id, "name": self.name, "text": self.text,
                "posted": self.posted })
    }
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            maker_id: v["maker_id"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            text: v["text"].as_str()?.to_string(),
            posted: v["posted"].as_u64().unwrap_or(0),
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum StoreError {
    NotACourse(String),
    TooLarge,
    NotFound,
    Io(String),
    /// No (valid) account token, or the account may not do this.
    Forbidden,
    Bad(String),
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(dir.join("courses")).map_err(|e| StoreError::Io(e.to_string()))?;
        let courses: Vec<Meta> = fs::read(dir.join("index.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v["courses"].as_array().cloned())
            .map(|a| a.iter().filter_map(Meta::from_json).collect())
            .unwrap_or_default();
        let next_id = courses.iter().map(|m| m.data_id).max().unwrap_or(0x10_0000) + 1;
        let by_code = courses
            .iter()
            .enumerate()
            .map(|(i, m)| (m.code.clone(), i))
            .collect();
        let read = |name: &str| {
            fs::read(dir.join(name))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .unwrap_or(Value::Null)
        };
        let accounts = Accounts::from_json(&read("accounts.json"));
        let social = read("social.json");
        let votes = social["votes"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(code, v)| {
                        let m = v
                            .as_object()
                            .map(|m| {
                                m.iter()
                                    .filter_map(|(id, x)| {
                                        Some((id.parse().ok()?, x.as_i64()? as i8))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (code.clone(), m)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let comments = social["comments"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(code, v)| {
                        let list = v
                            .as_array()
                            .map(|a| a.iter().filter_map(Comment::from_json).collect())
                            .unwrap_or_default();
                        (code.clone(), list)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cleared = social["cleared"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(who, v)| {
                        let codes = v
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|c| Some(c.as_str()?.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (who.clone(), codes)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let endless = social["endless"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(d, v)| {
                        let best = v
                            .as_object()
                            .map(|m| {
                                m.iter()
                                    .filter_map(|(who, n)| Some((who.clone(), n.as_u64()?)))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (d.clone(), best)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let endless_runs = endless::load(&dir);
        Ok(Self {
            dir,
            inner: Mutex::new(Inner {
                courses,
                by_code,
                next_id,
                rng: now() | 1,
                accounts,
                votes,
                comments,
                cleared,
                endless,
                endless_runs,
            }),
            admin: None,
            verified_records: false,
        })
    }

    /// (courses, accounts) held.
    pub fn counts(&self) -> (usize, usize) {
        let g = self.inner.lock().unwrap();
        (g.courses.len(), g.accounts.list.len())
    }

    /// Enable moderation with this admin token.
    /// Accept record times only from replays the server re-simulates.
    pub fn with_verified_records(mut self, on: bool) -> Self {
        self.verified_records = on;
        self
    }

    pub fn with_admin(mut self, token: Option<String>) -> Self {
        self.admin = token.filter(|t| !t.is_empty());
        self
    }

    fn write_json(&self, name: &str, v: &Value) -> Result<(), StoreError> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        fs::write(&tmp, serde_json::to_vec(v).unwrap())
            .and_then(|_| fs::rename(&tmp, self.dir.join(name)))
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    fn save_social(&self, g: &Inner) -> Result<(), StoreError> {
        let votes: serde_json::Map<String, Value> = g
            .votes
            .iter()
            .map(|(code, m)| {
                let o: serde_json::Map<String, Value> =
                    m.iter().map(|(id, x)| (id.to_string(), json!(x))).collect();
                (code.clone(), Value::Object(o))
            })
            .collect();
        let comments: serde_json::Map<String, Value> = g
            .comments
            .iter()
            .map(|(code, list)| {
                (
                    code.clone(),
                    Value::Array(list.iter().map(Comment::to_json).collect()),
                )
            })
            .collect();
        self.write_json(
            "social.json",
            &json!({ "votes": votes, "comments": comments, "cleared": g.cleared,
                     "endless": g.endless }),
        )
    }

    /// A new account: (public account JSON, token).
    pub fn register(&self, name: &str) -> Result<(Value, String), StoreError> {
        let mut g = self.inner.lock().unwrap();
        let (a, token) = g.accounts.register(name, now()).map_err(StoreError::Bad)?;
        self.write_json("accounts.json", &g.accounts.to_json())?;
        Ok((a.public_json(), token))
    }

    /// The account of a bearer token.
    pub fn account(&self, token: &str) -> Option<accounts::Account> {
        self.inner.lock().unwrap().accounts.by_token(token).cloned()
    }

    /// A maker's profile and courses.
    pub fn maker(&self, maker_id: &str) -> Option<Value> {
        let g = self.inner.lock().unwrap();
        let a = g.accounts.by_maker_id(maker_id)?;
        let courses: Vec<Value> = g
            .courses
            .iter()
            .filter(|m| m.owner == a.id)
            .map(Meta::to_json)
            .collect();
        Some(json!({ "maker": a.public_json(), "courses": courses }))
    }

    /// Like (+1) or boo (−1) as `account`; a second vote replaces the first.
    pub fn vote(&self, code: &str, account: u32, v: i8) -> Result<Meta, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let i = *g.by_code.get(code).ok_or(StoreError::NotFound)?;
        let old = g
            .votes
            .entry(code.to_string())
            .or_default()
            .insert(account, v);
        let m = &mut g.courses[i];
        match old {
            Some(1) => m.likes -= 1,
            Some(-1) => m.boos -= 1,
            _ => {}
        }
        if v > 0 {
            m.likes += 1;
        } else {
            m.boos += 1;
        }
        let out = m.clone();
        self.save_index(&g)?;
        self.save_social(&g)?;
        Ok(out)
    }

    /// A clear by `who` (maker ID, if signed in) in `time_ms` (0 unknown). The time can set the
    /// record unless the server takes records only from replays.
    pub fn clear(&self, code: &str, who: Option<&str>, time_ms: u64) -> Result<Meta, StoreError> {
        let time = if self.verified_records { 0 } else { time_ms };
        self.clear_inner(code, who, time, None)
    }

    /// A clear by `who` proven by a replay (`mmk_modes::replay`): the server plays it on the
    /// course and the clear time is the replay's own. Errors when it doesn't clear.
    pub fn clear_replay(&self, code: &str, who: &str, replay: &[u8]) -> Result<Meta, StoreError> {
        use mmk_modes::replay::{Replay, SIM_VERSION};
        let r = Replay::decode(replay).ok_or_else(|| StoreError::Bad("not a replay".into()))?;
        if r.sim_version != SIM_VERSION {
            return Err(StoreError::Bad(format!(
                "replay from engine version {} (this server plays {SIM_VERSION})",
                r.sim_version
            )));
        }
        let course = mmk_formats::smm2::Course::parse(&self.data(code)?)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let time = r
            .run(&course)
            .time_ms()
            .ok_or_else(|| StoreError::Bad("the replay doesn't clear the course".into()))?;
        self.clear_inner(code, Some(who), time, Some(replay))
    }

    /// The record holder's replay, when the record was verified.
    pub fn record_replay(&self, code: &str) -> Option<Vec<u8>> {
        self.get(code).filter(|m| m.record_verified)?;
        fs::read(self.dir.join(format!("replays/{code}.mmkr"))).ok()
    }

    fn clear_inner(
        &self,
        code: &str,
        who: Option<&str>,
        time_ms: u64,
        replay: Option<&[u8]>,
    ) -> Result<Meta, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let i = *g.by_code.get(code).ok_or(StoreError::NotFound)?;
        let m = &mut g.courses[i];
        m.clears += 1;
        if let Some(who) = who {
            let mine = g.cleared.entry(who.to_string()).or_default();
            if !mine.iter().any(|c| c == code) {
                mine.push(code.to_string());
            }
            let m = &mut g.courses[i];
            if m.first_clear.is_empty() {
                m.first_clear = who.to_string();
            }
            if time_ms > 0 && (m.record_ms == 0 || time_ms < m.record_ms) {
                m.record_ms = time_ms;
                m.record_holder = who.to_string();
                m.record_verified = replay.is_some();
                let path = self.dir.join(format!("replays/{code}.mmkr"));
                match replay {
                    Some(r) => fs::create_dir_all(self.dir.join("replays"))
                        .and_then(|_| fs::write(&path, r))
                        .map_err(|e| StoreError::Io(e.to_string()))?,
                    None => {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        let out = g.courses[i].clone();
        self.save_index(&g)?;
        if who.is_some() {
            self.save_social(&g)?;
        }
        Ok(out)
    }

    pub fn comments(&self, code: &str) -> Vec<Comment> {
        let g = self.inner.lock().unwrap();
        g.comments.get(code).cloned().unwrap_or_default()
    }

    pub fn comment(
        &self,
        code: &str,
        a: &accounts::Account,
        text: &str,
    ) -> Result<Comment, StoreError> {
        let text: String = text.trim().chars().take(MAX_COMMENT).collect();
        if text.is_empty() || text.chars().any(|c| c.is_control() && c != '\n') {
            return Err(StoreError::Bad("bad comment".into()));
        }
        let mut g = self.inner.lock().unwrap();
        if !g.by_code.contains_key(code) {
            return Err(StoreError::NotFound);
        }
        let c = Comment {
            maker_id: a.maker_id.clone(),
            name: a.name.clone(),
            text,
            posted: now(),
        };
        g.comments
            .entry(code.to_string())
            .or_default()
            .push(c.clone());
        self.save_social(&g)?;
        Ok(c)
    }

    /// Remove a course (its uploader or the admin).
    pub fn delete(&self, code: &str, by: Option<u32>, admin: bool) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        let i = *g.by_code.get(code).ok_or(StoreError::NotFound)?;
        let owner = g.courses[i].owner;
        if !(admin || (owner != 0 && Some(owner) == by)) {
            return Err(StoreError::Forbidden);
        }
        g.courses.remove(i);
        g.by_code = g
            .courses
            .iter()
            .enumerate()
            .map(|(i, m)| (m.code.clone(), i))
            .collect();
        g.votes.remove(code);
        g.comments.remove(code);
        let _ = fs::remove_file(self.dir.join(format!("courses/{code}.bin")));
        self.save_index(&g)?;
        self.save_social(&g)
    }

    pub fn set_banned(&self, id: u32, banned: bool) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        if !g.accounts.set_banned(id, banned) {
            return Err(StoreError::NotFound);
        }
        self.write_json("accounts.json", &g.accounts.to_json())
    }

    pub fn is_admin(&self, token: &str) -> bool {
        // constant time, so response timing doesn't leak the token
        self.admin.as_deref().is_some_and(|a| {
            a.len() == token.len()
                && a.bytes()
                    .zip(token.bytes())
                    .fold(0u8, |d, (x, y)| d | (x ^ y))
                    == 0
        })
    }

    fn save_index(&self, inner: &Inner) -> Result<(), StoreError> {
        let list: Vec<Value> = inner
            .courses
            .iter()
            .map(|m| {
                let mut v = m.to_json();
                v["data_id"] = json!(m.data_id);
                v["owner"] = json!(m.owner);
                v
            })
            .collect();
        let tmp = self.dir.join("index.json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec(&json!({ "courses": list })).unwrap(),
        )
        .and_then(|_| fs::rename(&tmp, self.dir.join("index.json")))
        .map_err(|e| StoreError::Io(e.to_string()))
    }

    /// Validate and store a course; returns its metadata.
    pub fn upload(&self, data: &[u8], maker: &str) -> Result<Meta, StoreError> {
        self.upload_as(data, maker, None)
    }

    /// Upload as `account` (its name and maker ID are recorded), or anonymously as `maker`.
    pub fn upload_as(
        &self,
        data: &[u8],
        maker: &str,
        account: Option<&accounts::Account>,
    ) -> Result<Meta, StoreError> {
        if data.len() > MAX_UPLOAD {
            return Err(StoreError::TooLarge);
        }
        let course = Course::parse(data).map_err(|e| StoreError::NotACourse(e.to_string()))?;
        let mut g = self.inner.lock().unwrap();
        let data_id = g.next_id;
        g.next_id += 1;
        let code = smm2_data_id_to_code(data_id, false);
        let meta = Meta {
            code: code.clone(),
            data_id,
            name: course.header.name.to_string_lossy(),
            description: course.header.description.to_string_lossy(),
            maker: account
                .map(|a| a.name.clone())
                .unwrap_or_else(|| maker.chars().take(32).collect()),
            style: course.header.game_style.as_str_lossy(),
            theme: course.main.header.theme,
            uploaded: now(),
            plays: 0,
            clears: 0,
            attempts: 0,
            likes: 0,
            boos: 0,
            owner: account.map_or(0, |a| a.id),
            maker_id: account.map(|a| a.maker_id.clone()).unwrap_or_default(),
            record_ms: 0,
            record_holder: String::new(),
            record_verified: false,
            first_clear: String::new(),
        };
        fs::write(self.dir.join(format!("courses/{code}.bin")), data)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let i = g.courses.len();
        g.courses.push(meta.clone());
        g.by_code.insert(code, i);
        self.save_index(&g)?;
        Ok(meta)
    }

    pub fn get(&self, code: &str) -> Option<Meta> {
        let g = self.inner.lock().unwrap();
        g.by_code.get(code).map(|&i| g.courses[i].clone())
    }

    pub fn data(&self, code: &str) -> Result<Vec<u8>, StoreError> {
        if self.get(code).is_none() {
            return Err(StoreError::NotFound);
        }
        fs::read(self.dir.join(format!("courses/{code}.bin")))
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    /// An Endless Challenge run on `difficulty` that cleared `cleared` courses; the account's best
    /// on that difficulty is kept. Returns the best.
    pub fn endless(&self, difficulty: &str, who: &str, cleared: u64) -> Result<u64, StoreError> {
        if !ENDLESS.contains(&difficulty) {
            return Err(StoreError::NotFound);
        }
        let mut g = self.inner.lock().unwrap();
        let best = g
            .endless
            .entry(difficulty.to_string())
            .or_default()
            .entry(who.to_string())
            .or_insert(0);
        *best = (*best).max(cleared);
        let out = *best;
        self.save_social(&g)?;
        Ok(out)
    }

    /// A 100 Mario run on `difficulty` completed by a client that ran it itself; the account's
    /// completed runs on that difficulty.
    pub fn hundred_complete(&self, difficulty: &str, who: &str) -> Result<u64, StoreError> {
        if !ENDLESS.contains(&difficulty) {
            return Err(StoreError::NotFound);
        }
        let mut g = self.inner.lock().unwrap();
        let n = g
            .endless
            .entry(format!("hundred_{difficulty}"))
            .or_default()
            .entry(who.to_string())
            .or_insert(0);
        *n += 1;
        let out = *n;
        self.save_social(&g)?;
        Ok(out)
    }

    /// Leaderboard `kind`: maker_points (likes − boos on their courses [inf: SMM2 adds points
    /// for likes and takes them for boos; the exact weights weren't decoded]), clears (courses
    /// cleared), first_clears, records (world records held), endless_<d> (Endless best clears),
    /// hundred_<d> (completed 100 Mario runs). Banned accounts are left out.
    pub fn rankings(&self, kind: &str, limit: usize) -> Option<Vec<Value>> {
        let g = self.inner.lock().unwrap();
        let score = |a: &accounts::Account| -> i64 {
            let count =
                |f: &dyn Fn(&Meta) -> bool| g.courses.iter().filter(|m| f(m)).count() as i64;
            match kind {
                "maker_points" => g
                    .courses
                    .iter()
                    .filter(|m| m.owner == a.id)
                    .map(|m| m.likes as i64 - m.boos as i64)
                    .sum::<i64>()
                    .max(0),
                "clears" => g.cleared.get(&a.maker_id).map_or(0, |c| c.len() as i64),
                "first_clears" => count(&|m| m.first_clear == a.maker_id),
                "records" => count(&|m| m.record_holder == a.maker_id),
                k if k
                    .strip_prefix("endless_")
                    .is_some_and(|d| ENDLESS.contains(&d)) =>
                {
                    g.endless
                        .get(&k["endless_".len()..])
                        .and_then(|m| m.get(&a.maker_id))
                        .map_or(0, |&n| n as i64)
                }
                // completed 100 Mario runs
                k if k
                    .strip_prefix("hundred_")
                    .is_some_and(|d| ENDLESS.contains(&d)) =>
                {
                    g.endless
                        .get(k)
                        .and_then(|m| m.get(&a.maker_id))
                        .map_or(0, |&n| n as i64)
                }
                _ => -1,
            }
        };
        if score(&accounts::Account::default()) < 0 {
            return None;
        }
        let mut rows: Vec<(i64, &accounts::Account)> = g
            .accounts
            .list
            .iter()
            .filter(|a| !a.banned)
            .map(|a| (score(a), a))
            .filter(|(s, _)| *s > 0)
            .collect();
        rows.sort_by_key(|(s, a)| (std::cmp::Reverse(*s), a.id));
        Some(
            rows.iter()
                .take(limit)
                .enumerate()
                .map(|(i, (s, a))| {
                    json!({ "rank": i + 1, "score": s, "maker_id": a.maker_id, "name": a.name })
                })
                .collect(),
        )
    }

    /// `maker_id`'s row on leaderboard `kind` (None: not ranked, or no such leaderboard).
    pub fn rank_of(&self, kind: &str, maker_id: &str) -> Option<Value> {
        self.rankings(kind, usize::MAX)?
            .into_iter()
            .find(|r| r["maker_id"] == maker_id)
    }

    /// Record a play / anonymous clear.
    pub fn record(&self, code: &str, what: &str) -> Result<Meta, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let i = *g.by_code.get(code).ok_or(StoreError::NotFound)?;
        let m = &mut g.courses[i];
        match what {
            "play" => {
                m.plays += 1;
                m.attempts += 1;
            }
            "clear" => m.clears += 1,
            _ => return Err(StoreError::NotFound),
        }
        let out = m.clone();
        self.save_index(&g)?;
        Ok(out)
    }

    /// Search: text in name/maker/description, optional style, sort, paging.
    pub fn search(
        &self,
        q: &str,
        style: Option<&str>,
        sort: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<Meta> {
        let g = self.inner.lock().unwrap();
        let q = q.to_lowercase();
        let mut v: Vec<Meta> = g
            .courses
            .iter()
            .filter(|m| style.is_none_or(|s| m.style == s))
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.maker.to_lowercase().contains(&q)
                    || m.description.to_lowercase().contains(&q)
                    || m.code.eq_ignore_ascii_case(&q)
            })
            .cloned()
            .collect();
        match sort {
            "popular" => v.sort_by_key(|m| std::cmp::Reverse((m.likes, m.plays))),
            "hard" => v.sort_by_key(|m| std::cmp::Reverse(m.difficulty())),
            _ => v.sort_by_key(|m| std::cmp::Reverse(m.uploaded)),
        }
        v.into_iter().skip(offset).take(limit.min(100)).collect()
    }

    /// Up to `count` random courses whose difficulty value is in min..=max (100 Mario pools).
    pub fn pool(&self, min: u8, max: u8, count: usize) -> Vec<Meta> {
        let mut g = self.inner.lock().unwrap();
        let mut v: Vec<Meta> = g
            .courses
            .iter()
            .filter(|m| (min..=max).contains(&m.difficulty()))
            .cloned()
            .collect();
        for i in 0..v.len() {
            g.rng ^= g.rng << 13;
            g.rng ^= g.rng >> 7;
            g.rng ^= g.rng << 17;
            let j = i + (g.rng % (v.len() - i) as u64) as usize;
            v.swap(i, j);
        }
        v.truncate(count.min(100));
        v
    }
}

fn query(url: &str) -> HashMap<String, String> {
    url.split_once('?')
        .map(|(_, q)| {
            q.split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), decode(v)))
                .collect()
        })
        .unwrap_or_default()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Response of one API request: status, content type, body.
pub fn handle(
    store: &Store,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    let path = url.split('?').next().unwrap_or("");
    let q = query(url);
    let json =
        |status: u16, v: Value| (status, "application/json", serde_json::to_vec(&v).unwrap());
    let err = |status: u16, msg: &str| json(status, json!({ "error": msg }));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let token = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, v)| v.strip_prefix("Bearer "))
        .map(str::trim);
    let account = token.and_then(|t| store.account(t));
    let admin = token.is_some_and(|t| store.is_admin(t));
    let body_json = || serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let store_err = |e: StoreError| match e {
        StoreError::NotFound => err(404, "not found"),
        StoreError::Forbidden => err(403, "forbidden"),
        StoreError::Bad(m) | StoreError::NotACourse(m) => err(400, &m),
        StoreError::TooLarge => err(413, "too large"),
        StoreError::Io(m) => err(500, &m),
    };
    // signed-in actions need an account that isn't banned
    let signed_in = || account.as_ref().filter(|a| !a.banned);
    let who = signed_in().map(|a| a.maker_id.as_str());
    if let Some(r) = endless::route(store, method, &parts, &q, who, body) {
        return r;
    }
    match (method, parts.as_slice()) {
        ("POST", ["api", "accounts"]) => {
            let name = body_json()["name"].as_str().unwrap_or("").to_string();
            match store.register(&name) {
                Ok((a, token)) => json(201, json!({ "account": a, "token": token })),
                Err(e) => store_err(e),
            }
        }
        ("GET", ["api", "me"]) => match &account {
            Some(a) => json(200, a.public_json()),
            None => err(401, "sign in"),
        },
        ("GET", ["api", "makers", id]) => match store.maker(id) {
            Some(v) => json(200, v),
            None => err(404, "no such maker"),
        },
        ("POST", ["api", "courses", code, vote @ ("like" | "boo")]) => match signed_in() {
            Some(a) => match store.vote(code, a.id, if *vote == "like" { 1 } else { -1 }) {
                Ok(m) => json(200, m.to_json()),
                Err(e) => store_err(e),
            },
            None => err(401, "sign in"),
        },
        ("POST", ["api", "courses", code, "clear"]) => {
            let time = body_json()["time_ms"].as_u64().unwrap_or(0);
            let who = signed_in().map(|a| a.maker_id.clone());
            match store.clear(code, who.as_deref(), time) {
                Ok(m) => json(200, m.to_json()),
                Err(e) => store_err(e),
            }
        }
        ("POST", ["api", "courses", code, "replay"]) => match signed_in() {
            Some(a) => match store.clear_replay(code, &a.maker_id, body) {
                Ok(m) => json(200, m.to_json()),
                Err(e) => store_err(e),
            },
            None => err(401, "sign in"),
        },
        ("GET", ["api", "courses", code, "replay"]) => match store.record_replay(code) {
            Some(b) => (200, "application/octet-stream", b),
            None => err(404, "no verified record"),
        },
        ("GET", ["api", "courses", code, "comments"]) => json(
            200,
            json!({ "comments": store.comments(code).iter().map(Comment::to_json)
                .collect::<Vec<_>>() }),
        ),
        ("POST", ["api", "courses", code, "comments"]) => match signed_in() {
            Some(a) => {
                let text = body_json()["text"].as_str().unwrap_or("").to_string();
                match store.comment(code, a, &text) {
                    Ok(c) => json(201, c.to_json()),
                    Err(e) => store_err(e),
                }
            }
            None => err(401, "sign in"),
        },
        ("DELETE", ["api", "courses", code]) => {
            match store.delete(code, account.as_ref().map(|a| a.id), admin) {
                Ok(()) => json(200, json!({ "deleted": code })),
                Err(e) => store_err(e),
            }
        }
        ("POST", ["api", "admin", what @ ("ban" | "unban"), id]) if admin => {
            match id
                .parse()
                .map_err(|_| StoreError::NotFound)
                .and_then(|id| store.set_banned(id, *what == "ban"))
            {
                Ok(()) => json(200, json!({ "ok": true })),
                Err(e) => store_err(e),
            }
        }
        ("POST", ["api", "endless", difficulty]) => match signed_in() {
            Some(a) => {
                let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                let n = v["cleared"].as_u64().unwrap_or(0);
                let r = if v["ruleset"] == "hundred" {
                    store.hundred_complete(difficulty, &a.maker_id)
                } else {
                    store.endless(difficulty, &a.maker_id, n)
                };
                match r {
                    Ok(best) => json(200, json!({ "difficulty": difficulty, "best": best })),
                    Err(e) => store_err(e),
                }
            }
            None => err(401, "sign in"),
        },
        ("GET", ["api", "rankings", kind]) => {
            let limit = q
                .get("limit")
                .and_then(|l| l.parse().ok())
                .unwrap_or(100)
                .min(500);
            match store.rankings(kind, limit) {
                Some(rows) => {
                    // the caller's own row, wherever it is (null: not ranked)
                    let me = who.and_then(|w| store.rank_of(kind, w));
                    json(200, json!({ "kind": kind, "rankings": rows, "me": me }))
                }
                None => err(404, "no such ranking"),
            }
        }
        ("GET", ["api", "health"]) => {
            let (courses, accounts) = store.counts();
            json(
                200,
                json!({ "ok": true, "courses": courses, "accounts": accounts,
                        "sim_version": mmk_modes::replay::SIM_VERSION }),
            )
        }
        ("GET", ["api"]) | ("GET", [""]) => {
            json(200, json!({ "server": "mmk-server", "version": 1 }))
        }
        ("POST", ["api", "courses"]) => {
            let maker = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("x-maker"))
                .map(|(_, v)| v.as_str())
                .unwrap_or("anonymous");
            if account.as_ref().is_some_and(|a| a.banned) {
                return err(403, "banned");
            }
            match store.upload_as(body, maker, account.as_ref()) {
                Ok(m) => json(201, m.to_json()),
                Err(StoreError::TooLarge) => err(413, "too large"),
                Err(StoreError::NotACourse(e)) => err(400, &format!("not a course: {e}")),
                Err(e) => err(500, &format!("{e:?}")),
            }
        }
        ("GET", ["api", "courses"]) => {
            let n = |k: &str, d: usize| q.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
            let list = store.search(
                q.get("q").map(String::as_str).unwrap_or(""),
                q.get("style").map(String::as_str),
                q.get("sort").map(String::as_str).unwrap_or("new"),
                n("offset", 0),
                n("limit", 30),
            );
            json(
                200,
                json!({ "courses": list.iter().map(Meta::to_json).collect::<Vec<_>>() }),
            )
        }
        ("GET", ["api", "courses", code]) => match store.get(code) {
            Some(m) => json(200, m.to_json()),
            None => err(404, "no such course"),
        },
        ("GET", ["api", "courses", code, "data"]) => match store.data(code) {
            Ok(b) => (200, "application/octet-stream", b),
            Err(_) => err(404, "no such course"),
        },
        ("POST", ["api", "courses", code, what]) => match store.record(code, what) {
            Ok(m) => json(200, m.to_json()),
            Err(_) => err(404, "no such course or action"),
        },
        ("GET", ["api", "pool"]) => {
            let n = |k: &str, d: u64| q.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
            let list = store.pool(
                n("min", 0) as u8,
                n("max", 100) as u8,
                n("count", 50) as usize,
            );
            json(
                200,
                json!({ "courses": list.iter().map(Meta::to_json).collect::<Vec<_>>() }),
            )
        }
        _ => err(404, "not found"),
    }
}

/// Serve the API on `addr` (e.g. "0.0.0.0:8640") until the process ends.
/// Start the online-play relay: UDP on `udp_addr` (native players) and WebSocket on `ws_addr`
/// (browsers), each in its own thread.
pub fn start_relay(
    udp_addr: &str,
    ws_addr: &str,
) -> Result<std::sync::Arc<mmk_netplay::relay::Hub>, String> {
    let udp = std::net::UdpSocket::bind(udp_addr).map_err(|e| format!("udp {udp_addr}: {e}"))?;
    let tcp = std::net::TcpListener::bind(ws_addr).map_err(|e| format!("tcp {ws_addr}: {e}"))?;
    let hub = mmk_netplay::relay::Hub::new(udp);
    let h = hub.clone();
    std::thread::spawn(move || h.run_udp());
    let h = hub.clone();
    std::thread::spawn(move || ws::serve(h, tcp));
    Ok(hub)
}

/// Endless Challenge difficulties.
pub const ENDLESS: &[&str] = &["easy", "normal", "expert", "super_expert"];

/// How `serve` runs.
#[derive(Clone, Copy, Debug)]
pub struct ServeOptions {
    /// Request threads (replay verification and big uploads don't hold up other requests).
    pub threads: usize,
    pub limits: limits::Limits,
    /// Take the client address from `X-Forwarded-For` (only behind a reverse proxy you run).
    pub trust_proxy: bool,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            threads: 8,
            limits: limits::Limits::default(),
            trust_proxy: false,
        }
    }
}

/// CORS headers for browsers (any origin: the API is public and uses bearer tokens, not
/// cookies).
fn cors(
    resp: tiny_http::Response<std::io::Cursor<Vec<u8>>>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let h = |k: &str, v: &str| tiny_http::Header::from_bytes(k, v).unwrap();
    resp.with_header(h("Access-Control-Allow-Origin", "*"))
        .with_header(h(
            "Access-Control-Allow-Methods",
            "GET, POST, DELETE, OPTIONS",
        ))
        .with_header(h(
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type, X-Maker",
        ))
        .with_header(h("Access-Control-Max-Age", "86400"))
}

pub fn serve(store: Store, addr: &str) -> Result<(), String> {
    serve_with(store, addr, ServeOptions::default())
}

pub fn serve_with(store: Store, addr: &str, opts: ServeOptions) -> Result<(), String> {
    let server = std::sync::Arc::new(tiny_http::Server::http(addr).map_err(|e| e.to_string())?);
    let store = std::sync::Arc::new(store);
    let gate = std::sync::Arc::new(limits::Gate::new(opts.limits));
    let workers: Vec<_> = (0..opts.threads.max(1))
        .map(|_| {
            let (server, store, gate) = (server.clone(), store.clone(), gate.clone());
            std::thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    respond(&store, &gate, opts.trust_proxy, req);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

fn respond(store: &Store, gate: &limits::Gate, trust_proxy: bool, mut req: tiny_http::Request) {
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|h| {
            (
                h.field.as_str().as_str().to_string(),
                h.value.as_str().to_string(),
            )
        })
        .collect();
    let reply = |req: tiny_http::Request, status: u16, ctype: &str, out: Vec<u8>| {
        let resp = tiny_http::Response::from_data(out)
            .with_status_code(status)
            .with_header(tiny_http::Header::from_bytes("Content-Type", ctype).unwrap());
        let _ = req.respond(cors(resp));
    };
    if method == "OPTIONS" {
        // CORS preflight
        return reply(req, 204, "text/plain", Vec::new());
    }
    let forwarded = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for"))
        .and_then(|(_, v)| v.split(',').next()?.trim().parse().ok());
    let ip = match (trust_proxy, forwarded) {
        (true, Some(ip)) => Some(ip),
        _ => req.remote_addr().map(|a| a.ip()),
    };
    let path = url.split('?').next().unwrap_or("");
    // the admin (seeding a server, moderating) isn't limited
    let admin = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, v)| v.strip_prefix("Bearer "))
        .is_some_and(|t| store.is_admin(t.trim()));
    if !admin
        && let (Some(ip), Some(kind)) = (ip, limits::kind_of(&method, path))
        && !gate.allow(ip, kind, std::time::Instant::now())
    {
        let body = serde_json::to_vec(&json!({ "error": "too many requests, slow down" })).unwrap();
        return reply(req, 429, "application/json", body);
    }
    let mut body = Vec::new();
    let _ = std::io::Read::read_to_end(
        &mut std::io::Read::take(req.as_reader(), MAX_UPLOAD as u64 + 1),
        &mut body,
    );
    let (status, ctype, out) = handle(store, &method, &url, &headers, &body);
    reply(req, status, ctype, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mmk-server-test-{name}-{}-{}",
            std::process::id(),
            now()
        ));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn upload_search_download_stats_and_pools() {
        let dir = tmp("courses");
        let store = Store::open(&dir).unwrap();
        let mut c = Course::blank("M3");
        c.header.name.set("Sky Fortress");
        let data = c.to_bytes().unwrap();
        let (st, _, body) = handle(
            &store,
            "POST",
            "/api/courses",
            &[("X-Maker".into(), "ada".into())],
            &data,
        );
        assert_eq!(st, 201);
        let meta: Value = serde_json::from_slice(&body).unwrap();
        let code = meta["code"].as_str().unwrap().to_string();
        assert_eq!(code.len(), 11, "XXX-XXX-XXX");
        assert_eq!(meta["maker"], "ada");
        // not a course
        assert_eq!(handle(&store, "POST", "/api/courses", &[], b"hello").0, 400);
        // search by text and style
        let (_, _, b) = handle(&store, "GET", "/api/courses?q=sky+fort&style=M3", &[], &[]);
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["courses"].as_array().unwrap().len(), 1);
        // data round trip
        let (st, ct, b) = handle(
            &store,
            "GET",
            &format!("/api/courses/{code}/data"),
            &[],
            &[],
        );
        assert_eq!((st, ct), (200, "application/octet-stream"));
        assert_eq!(b, data);
        // stats → difficulty: 4 plays, 1 clear = 75
        for _ in 0..4 {
            handle(
                &store,
                "POST",
                &format!("/api/courses/{code}/play"),
                &[],
                &[],
            );
        }
        let (_, _, b) = handle(
            &store,
            "POST",
            &format!("/api/courses/{code}/clear"),
            &[],
            &[],
        );
        let m: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(m["difficulty"], 75);
        let (_, _, b) = handle(&store, "GET", "/api/pool?min=75&max=95", &[], &[]);
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["courses"].as_array().unwrap().len(), 1);
        // persisted
        let again = Store::open(&dir).unwrap();
        assert_eq!(again.get(&code).unwrap().clears, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn accounts_votes_records_comments_and_moderation() {
        let dir = tmp("accounts");
        let store = Store::open(&dir).unwrap().with_admin(Some("root".into()));
        let call = |method: &str, url: &str, token: Option<&str>, body: &[u8]| {
            let headers: Vec<(String, String)> = token
                .map(|t| vec![("Authorization".into(), format!("Bearer {t}"))])
                .unwrap_or_default();
            let (st, _, b) = handle(&store, method, url, &headers, body);
            (
                st,
                serde_json::from_slice::<Value>(&b).unwrap_or(Value::Null),
            )
        };
        let (st, v) = call("POST", "/api/accounts", None, br#"{"name":"Ada"}"#);
        assert_eq!(st, 201);
        let token = v["token"].as_str().unwrap().to_string();
        let maker_id = v["account"]["maker_id"].as_str().unwrap().to_string();
        let (_, me) = call("GET", "/api/me", Some(&token), b"");
        assert_eq!(me["name"], "Ada");
        // upload as the account
        let data = Course::blank("M1").to_bytes().unwrap();
        let (st, m) = call("POST", "/api/courses", Some(&token), &data);
        assert_eq!(st, 201);
        assert_eq!(
            (m["maker"].as_str(), m["maker_id"].as_str()),
            (Some("Ada"), Some(maker_id.as_str()))
        );
        let code = m["code"].as_str().unwrap().to_string();
        // votes: one per account, a boo replaces the like
        assert_eq!(
            call("POST", &format!("/api/courses/{code}/like"), None, b"").0,
            401
        );
        call(
            "POST",
            &format!("/api/courses/{code}/like"),
            Some(&token),
            b"",
        );
        let (_, m) = call(
            "POST",
            &format!("/api/courses/{code}/like"),
            Some(&token),
            b"",
        );
        assert_eq!(m["likes"], 1);
        let (_, m) = call(
            "POST",
            &format!("/api/courses/{code}/boo"),
            Some(&token),
            b"",
        );
        assert_eq!(
            (m["likes"].as_u64(), m["boos"].as_u64()),
            (Some(0), Some(1))
        );
        // clears: first clear and record
        let (_, m) = call(
            "POST",
            &format!("/api/courses/{code}/clear"),
            Some(&token),
            br#"{"time_ms":51000}"#,
        );
        assert_eq!(
            (m["first_clear"].as_str(), m["record_ms"].as_u64()),
            (Some(maker_id.as_str()), Some(51000))
        );
        let (_, m) = call(
            "POST",
            &format!("/api/courses/{code}/clear"),
            Some(&token),
            br#"{"time_ms":60000}"#,
        );
        assert_eq!(m["record_ms"], 51000, "a slower clear keeps the record");
        // comments
        call(
            "POST",
            &format!("/api/courses/{code}/comments"),
            Some(&token),
            br#"{"text":"nice jumps"}"#,
        );
        let (_, c) = call("GET", &format!("/api/courses/{code}/comments"), None, b"");
        assert_eq!(c["comments"][0]["text"], "nice jumps");
        // profile
        let (_, p) = call("GET", &format!("/api/makers/{maker_id}"), None, b"");
        assert_eq!(p["courses"][0]["code"].as_str(), Some(code.as_str()));
        // leaderboards (the course was booed, so no maker points)
        for kind in ["clears", "first_clears", "records"] {
            let (st, r) = call("GET", &format!("/api/rankings/{kind}"), None, b"");
            assert_eq!(st, 200);
            assert_eq!(
                r["rankings"][0]["maker_id"].as_str(),
                Some(maker_id.as_str())
            );
            assert_eq!(r["rankings"][0]["score"], 1, "{kind}");
        }
        assert_eq!(
            call("POST", "/api/endless/expert", None, br#"{"cleared":3}"#).0,
            401
        );
        for n in [7, 4] {
            let (st, e) = call(
                "POST",
                "/api/endless/expert",
                Some(&token),
                format!(r#"{{"cleared":{n}}}"#).as_bytes(),
            );
            assert_eq!((st, e["best"].as_u64()), (200, Some(7)));
        }
        let (_, r) = call("GET", "/api/rankings/endless_expert", None, b"");
        assert_eq!(r["rankings"][0]["score"], 7);
        assert_eq!(
            call("POST", "/api/endless/silly", Some(&token), b"{}").0,
            404
        );
        let (_, r) = call("GET", "/api/rankings/maker_points", None, b"");
        assert_eq!(r["rankings"].as_array().map(Vec::len), Some(0));
        assert_eq!(call("GET", "/api/rankings/nope", None, b"").0, 404);
        // persisted across a restart
        let store2 = Store::open(&dir).unwrap();
        assert!(
            store2.account(&token).is_some(),
            "{:?}",
            fs::read_to_string(dir.join("accounts.json"))
        );
        assert_eq!(store2.comments(&code).len(), 1);
        assert_eq!(store2.rankings("clears", 10).unwrap().len(), 1);
        // moderation: a ban blocks votes; the admin can delete
        let id = me["id"].as_u64().unwrap();
        assert_eq!(
            call("POST", &format!("/api/admin/ban/{id}"), Some(&token), b"").0,
            404
        );
        assert_eq!(
            call("POST", &format!("/api/admin/ban/{id}"), Some("root"), b"").0,
            200
        );
        assert_eq!(
            call(
                "POST",
                &format!("/api/courses/{code}/like"),
                Some(&token),
                b""
            )
            .0,
            401
        );
        assert_eq!(
            call("DELETE", &format!("/api/courses/{code}"), Some("root"), b"").0,
            200
        );
        assert!(store.get(&code).is_none());
    }

    #[test]
    fn replays_prove_record_times() {
        use mmk_modes::replay::Replay;
        use mmk_sim::{Input, World, input};
        let dir = tmp("replays");
        let store = Store::open(&dir).unwrap().with_verified_records(true);
        // a flat course and a run that clears it
        let mut ed = mmk_editor::Editor::new("M1");
        for tx in 0..ed.size().0 {
            let _ = ed.paint_ground(tx, 0);
        }
        let data = ed.save().unwrap();
        let course = mmk_formats::smm2::Course::parse(&data).unwrap();
        let mut w = World::from_course(&course);
        let mut r = Replay::new();
        let mut touch = 0;
        while !w.cleared && r.inputs.len() < 20_000 {
            let jump = if r.inputs.len() % 90 >= 70 {
                input::JUMP
            } else {
                0
            };
            let i = Input {
                buttons: input::RIGHT | input::RUN | jump,
            };
            r.push(i);
            w.step(i);
            if touch == 0 && w.goal_phase != mmk_sim::world::GoalPhase::None {
                touch = r.inputs.len();
            }
        }
        assert!(w.cleared);
        let (st, _, v) = handle(&store, "POST", "/api/accounts", &[], br#"{"name":"Rex"}"#);
        assert_eq!(st, 201);
        let v: Value = serde_json::from_slice(&v).unwrap();
        let token = v["token"].as_str().unwrap().to_string();
        let auth = [("Authorization".to_string(), format!("Bearer {token}"))];
        let m = store.upload(&data, "rex").unwrap();
        let url = format!("/api/courses/{}/replay", m.code);
        // typed-in times don't make records on this server
        let (_, _, b) = handle(
            &store,
            "POST",
            &format!("/api/courses/{}/clear", m.code),
            &auth,
            br#"{"time_ms":1}"#,
        );
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["record_ms"], 0);
        // a replay that stops short is refused
        let mut short = r.clone();
        short.inputs.truncate(r.inputs.len() - 1);
        assert_eq!(handle(&store, "POST", &url, &auth, &short.encode()).0, 400);
        assert_eq!(handle(&store, "POST", &url, &[], &r.encode()).0, 401);
        let (st, _, b) = handle(&store, "POST", &url, &auth, &r.encode());
        assert_eq!(st, 200);
        let v: Value = serde_json::from_slice(&b).unwrap();
        // timed at the goal touch, as the game's clear screen shows
        let ms = (touch as u64 * 1000).div_ceil(60);
        assert_eq!(
            (v["record_ms"].as_u64(), v["record_verified"].as_bool()),
            (Some(ms), Some(true))
        );
        let (st, _, ghost) = handle(&store, "GET", &url, &[], b"");
        assert_eq!((st, Replay::decode(&ghost)), (200, Some(r)));
        let _ = fs::remove_dir_all(&dir);
    }
}
