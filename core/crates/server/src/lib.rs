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
//! | POST | `/api/courses/<code>/play`, `/clear`, `/like` | statistics |
//! | GET | `/api/pool?min=&max=&count=` | random courses by difficulty value (fail rate 0–100) |
//!
//! The difficulty value is 100 − clear rate once a course has been played, as SMM1's 100 Mario
//! bands use it. Courses are identified by SMM2-style codes (XXX-XXX-XXX).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mmk_formats::course_id::smm2_data_id_to_code;
use mmk_formats::smm2::Course;
use serde_json::{Value, json};

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
        })
    }
}

/// The course store (thread-safe; one per server).
pub struct Store {
    dir: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    courses: Vec<Meta>,
    by_code: HashMap<String, usize>,
    next_id: u32,
    rng: u64,
}

#[derive(Debug, PartialEq)]
pub enum StoreError {
    NotACourse(String),
    TooLarge,
    NotFound,
    Io(String),
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
        Ok(Self {
            dir,
            inner: Mutex::new(Inner {
                courses,
                by_code,
                next_id,
                rng: now() | 1,
            }),
        })
    }

    fn save_index(&self, inner: &Inner) -> Result<(), StoreError> {
        let list: Vec<Value> = inner
            .courses
            .iter()
            .map(|m| {
                let mut v = m.to_json();
                v["data_id"] = json!(m.data_id);
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
            maker: maker.chars().take(32).collect(),
            style: course.header.game_style.as_str_lossy(),
            theme: course.main.header.theme,
            uploaded: now(),
            plays: 0,
            clears: 0,
            attempts: 0,
            likes: 0,
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

    /// Record a play / attempt result / like.
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
            "like" => m.likes += 1,
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
    match (method, parts.as_slice()) {
        ("GET", ["api"]) | ("GET", [""]) => {
            json(200, json!({ "server": "mmk-server", "version": 1 }))
        }
        ("POST", ["api", "courses"]) => {
            let maker = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("x-maker"))
                .map(|(_, v)| v.as_str())
                .unwrap_or("anonymous");
            match store.upload(body, maker) {
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
pub fn serve(store: Store, addr: &str) -> Result<(), String> {
    let server = tiny_http::Server::http(addr).map_err(|e| e.to_string())?;
    for mut req in server.incoming_requests() {
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
        let mut body = Vec::new();
        let _ = std::io::Read::read_to_end(
            &mut std::io::Read::take(req.as_reader(), MAX_UPLOAD as u64 + 1),
            &mut body,
        );
        let (status, ctype, out) = handle(&store, &method, &url, &headers, &body);
        let resp = tiny_http::Response::from_data(out)
            .with_status_code(status)
            .with_header(tiny_http::Header::from_bytes("Content-Type", ctype).unwrap())
            .with_header(
                tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
            );
        let _ = req.respond(resp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("mmk-server-test-{}-{}", std::process::id(), now()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn upload_search_download_stats_and_pools() {
        let dir = tmp();
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
}
