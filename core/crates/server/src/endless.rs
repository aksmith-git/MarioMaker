//! Endless Challenge runs, held by the server as SMM2's server holds them (spec
//! `endless-challenge.md` §3.3, §3.4): lives, clears, the course order and the history live here;
//! the client keeps only the in-course counters (`mmk_modes::endless::Run`) and reports each
//! result. The rules are `mmk_modes::endless` ([`RunRecord::report`] and [`pick`]), shared with
//! the offline session.
//!
//! - Runs are per (account maker ID, difficulty), kept in `endless.json`
//!   (`{"players": {maker_id: EndlessSave}}`). Best clears stay in the leaderboard map of
//!   `social.json` that `POST /api/endless/<d>` and `GET /api/rankings/endless_<d>` already use;
//!   every clear raises it, so a suspended run counts too.
//! - Courses are drawn by the clear-rate bands of [`pick`] (only courses with ≥ 20 attempts are
//!   rated; unrated ones come last), never the player's own (`Meta::maker_id`), never one of a
//!   banned account, never twice in a run. A batch of up to 10 is queued in the run record, so
//!   `next` is idempotent and Continue replays the course that was being played.
//! - Endless plays count in the course's normal statistics: the first report of a course in a
//!   run counts a play, each miss or clear an attempt, a clear a clear (first clear and the
//!   account's cleared list too; no record time). A skip counts an attempt only when it ended a
//!   miss; a broken course counts nothing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use mmk_modes::endless::{
    Applied, BATCH, Candidate, Difficulty, EndlessSave, Finished, HistoryEntry, Outcome, Report,
    Ruleset, RunError, RunStatus, pick, pick_hundred,
};
use serde_json::{Map, Value, json};

use crate::{Inner, Meta, Store, StoreError, now};

/// Runs per maker ID, from `endless.json`.
pub(crate) fn load(dir: &Path) -> HashMap<String, EndlessSave> {
    fs::read(dir.join("endless.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["players"].as_object().cloned())
        .map(|o| {
            o.iter()
                .map(|(who, s)| (who.clone(), EndlessSave::from_json(s)))
                .collect()
        })
        .unwrap_or_default()
}

fn run_err(e: RunError) -> StoreError {
    match e {
        RunError::NoRun => StoreError::NotFound,
        RunError::NotActive => StoreError::Bad("the run is over".into()),
        RunError::NotInRun => StoreError::Bad("that course is not in this run".into()),
        RunError::NoCourses => StoreError::NotFound,
    }
}

/// A report's answer: the record's {lives, clears} (§3.1) and the best on that difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    pub applied: Applied,
    pub best: u64,
}

/// A drawn batch: the courses to play in order, whether the draw left the band, the run.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub courses: Vec<Meta>,
    pub widened: bool,
    pub run: RunStatus,
}

/// The leaderboard of a ruleset on `d`: `easy` … (Endless best clears) or `hundred_easy` …
/// (completed 100 Mario runs).
pub(crate) fn board(ruleset: Ruleset, d: Difficulty) -> String {
    match ruleset {
        Ruleset::Smm2 => d.name().to_string(),
        Ruleset::Hundred => format!("hundred_{}", d.name()),
    }
}

impl Inner {
    fn best(&self, d: Difficulty, who: &str) -> u64 {
        self.board_score(&board(Ruleset::Smm2, d), who)
    }

    fn board_score(&self, board: &str, who: &str) -> u64 {
        self.endless
            .get(board)
            .and_then(|m| m.get(who))
            .copied()
            .unwrap_or(0)
    }

    /// One more completed 100 Mario run on `d`; the new count.
    fn add_completion(&mut self, d: Difficulty, who: &str) -> u64 {
        let b = self
            .endless
            .entry(board(Ruleset::Hundred, d))
            .or_default()
            .entry(who.to_string())
            .or_insert(0);
        *b += 1;
        *b
    }

    fn raise_best(&mut self, d: Difficulty, who: &str, clears: u64) -> u64 {
        let b = self
            .endless
            .entry(d.name().to_string())
            .or_default()
            .entry(who.to_string())
            .or_insert(0);
        *b = (*b).max(clears);
        *b
    }

    fn next_seed(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}

impl Store {
    fn save_endless(&self, g: &Inner) -> Result<(), StoreError> {
        let players: Map<String, Value> = g
            .endless_runs
            .iter()
            .map(|(who, s)| (who.clone(), s.to_json()))
            .collect();
        self.write_json("endless.json", &json!({ "players": players }))
    }

    /// New Game on `d` for `who`: wipes that difficulty's run and starts one under `ruleset`
    /// with its starting lives (SMM2: 5 [ext/inf]; 100 Mario: 100).
    pub fn endless_start(
        &self,
        who: &str,
        d: Difficulty,
        ruleset: Ruleset,
    ) -> Result<RunStatus, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let best = g.board_score(&board(ruleset, d), who).min(u32::MAX as u64) as u32;
        let save = g.endless_runs.entry(who.to_string()).or_default();
        let rec = save.new_game_with(d, ruleset, now());
        rec.status.best_before = best;
        let out = rec.status;
        self.save_endless(&g)?;
        Ok(out)
    }

    /// The four runs, the best clears per difficulty, the lifetime skip count (0–3) and the last
    /// difficulty, as JSON.
    pub fn endless_status(&self, who: &str) -> Value {
        let g = self.inner.lock().unwrap();
        let save = g.endless_runs.get(who).cloned().unwrap_or_default();
        let runs: Map<String, Value> = Difficulty::ALL
            .iter()
            .map(|&d| {
                let v = save.run(d).map_or(Value::Null, |r| r.status.to_json());
                (d.name().to_string(), v)
            })
            .collect();
        let best: Map<String, Value> = Difficulty::ALL
            .iter()
            .map(|&d| (d.name().to_string(), json!(g.best(d, who))))
            .collect();
        json!({ "runs": runs, "best": best, "skips": save.skips,
                "last": save.last.map(Difficulty::name) })
    }

    /// The next courses of the run (up to `count`, at most 10): the run's queue, refilled by the
    /// pick rule when it is empty.
    pub fn endless_next(
        &self,
        who: &str,
        d: Difficulty,
        count: usize,
    ) -> Result<Batch, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let seed = g.next_seed();
        let Inner {
            courses,
            by_code,
            accounts,
            endless_runs,
            ..
        } = &mut *g;
        let save = endless_runs.get_mut(who).ok_or(StoreError::NotFound)?;
        save.last = Some(d);
        let rec = save.run_mut(d).ok_or(StoreError::NotFound)?;
        if !rec.status.can_continue() {
            return Err(run_err(RunError::NotActive));
        }
        rec.queue.retain(|c| by_code.contains_key(c));
        let mut widened = false;
        if rec.queue.is_empty() {
            let banned: HashSet<u32> = accounts
                .list
                .iter()
                .filter(|a| a.banned)
                .map(|a| a.id)
                .collect();
            let cands: Vec<Candidate> = courses
                .iter()
                .map(|m| Candidate {
                    id: m.code.clone(),
                    name: m.name.clone(),
                    maker: m.maker.clone(),
                    owner: m.maker_id.clone(),
                    style: m.style.clone(),
                    theme: m.theme,
                    attempts: m.attempts,
                    clears: m.clears,
                    hint: None,
                    broken: m.owner != 0 && banned.contains(&m.owner),
                })
                .collect();
            let pick = match rec.status.ruleset {
                Ruleset::Smm2 => pick,
                Ruleset::Hundred => pick_hundred,
            };
            let p = pick(
                &cands,
                d,
                &rec.excluded(),
                who,
                BATCH,
                &mut mmk_modes::Rng::new(seed),
            );
            widened = p.widened;
            rec.enqueue(p.indices.iter().map(|&i| cands[i].id.clone()), p.widened);
        }
        let list: Vec<Meta> = rec
            .queue
            .iter()
            .take(count.clamp(1, BATCH))
            .filter_map(|c| by_code.get(c).map(|&i| courses[i].clone()))
            .collect();
        let run = rec.status;
        self.save_endless(&g)?;
        Ok(Batch {
            courses: list,
            widened,
            run,
        })
    }

    /// Apply a course result reported by `who` (see [`Report`]); the course must be queued in
    /// the run. Updates the course's statistics and the best.
    pub fn endless_report(
        &self,
        who: &str,
        d: Difficulty,
        r: &Report,
    ) -> Result<Answer, StoreError> {
        let outcome = r.outcome.ok_or_else(|| {
            StoreError::Bad("result: clear, death, time_up, start_over, skip or broken".into())
        })?;
        let mut g = self.inner.lock().unwrap();
        let ix = g.by_code.get(&r.code).copied();
        let (style, theme) = ix.map_or((String::new(), 0), |i| {
            (g.courses[i].style.clone(), g.courses[i].theme)
        });
        let save = g.endless_runs.get_mut(who).ok_or(StoreError::NotFound)?;
        let rec = save.run_mut(d).ok_or(StoreError::NotFound)?;
        let first = rec.current() != Some(r.code.as_str()) || rec.course_reports == 0;
        let ruleset = rec.status.ruleset;
        let applied = rec.report(r, &style, theme, now()).map_err(run_err)?;
        if outcome == Outcome::Skip {
            save.note_skip();
        }
        let best = match ruleset {
            Ruleset::Smm2 if outcome == Outcome::Clear || applied.game_over => {
                g.raise_best(d, who, applied.clears as u64)
            }
            Ruleset::Smm2 => g.best(d, who),
            Ruleset::Hundred if applied.complete => g.add_completion(d, who),
            Ruleset::Hundred => g.board_score(&board(ruleset, d), who),
        };
        if let Some(i) = ix {
            let m = &mut g.courses[i];
            if first && outcome != Outcome::Broken {
                m.plays += 1;
            }
            let attempt = outcome.is_miss()
                || outcome == Outcome::Clear
                || (outcome == Outcome::Skip && r.during_miss);
            if attempt {
                m.attempts += 1;
            }
            if outcome == Outcome::Clear {
                m.clears += 1;
                if m.first_clear.is_empty() {
                    m.first_clear = who.to_string();
                }
                let mine = g.cleared.entry(who.to_string()).or_default();
                if !mine.contains(&r.code) {
                    mine.push(r.code.clone());
                }
            }
            self.save_index(&g)?;
        }
        if outcome == Outcome::Clear || applied.game_over || applied.complete {
            self.save_social(&g)?;
        }
        self.save_endless(&g)?;
        Ok(Answer { applied, best })
    }

    /// Quit to the menu: the run is kept for Continue (the lives already lost stay lost).
    pub fn endless_suspend(&self, who: &str, d: Difficulty) -> Result<RunStatus, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let rec = g
            .endless_runs
            .get_mut(who)
            .and_then(|s| s.run_mut(d))
            .ok_or(StoreError::NotFound)?;
        rec.status.updated_at = now();
        let out = rec.status;
        self.save_endless(&g)?;
        Ok(out)
    }

    /// Finish the run (after the game over): the best is updated and the run cleared.
    pub fn endless_finish(&self, who: &str, d: Difficulty) -> Result<Finished, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let save = g.endless_runs.get_mut(who).ok_or(StoreError::NotFound)?;
        let ruleset = save.run(d).map(|r| r.status.ruleset).unwrap_or_default();
        let f = save.finish(d).ok_or(StoreError::NotFound)?;
        let best = match ruleset {
            Ruleset::Smm2 => g.raise_best(d, who, f.clears as u64),
            Ruleset::Hundred => g.board_score(&board(ruleset, d), who),
        };
        self.save_social(&g)?;
        self.save_endless(&g)?;
        Ok(Finished {
            best: best.min(u32::MAX as u64) as u32,
            ..f
        })
    }

    /// The run's last 48 courses (oldest first).
    pub fn endless_history(
        &self,
        who: &str,
        d: Difficulty,
    ) -> Result<Vec<HistoryEntry>, StoreError> {
        let g = self.inner.lock().unwrap();
        let rec = g
            .endless_runs
            .get(who)
            .and_then(|s| s.run(d))
            .ok_or(StoreError::NotFound)?;
        Ok(rec.history.iter().cloned().collect())
    }
}

type Response = (u16, &'static str, Vec<u8>);

fn json_resp(status: u16, v: Value) -> Response {
    (status, "application/json", serde_json::to_vec(&v).unwrap())
}

fn store_err(e: StoreError) -> Response {
    let (st, msg) = match e {
        StoreError::NotFound => (404, "no such run".to_string()),
        StoreError::Forbidden => (403, "forbidden".to_string()),
        StoreError::Bad(m) | StoreError::NotACourse(m) => (400, m),
        StoreError::TooLarge => (413, "too large".to_string()),
        StoreError::Io(m) => (500, m),
    };
    json_resp(st, json!({ "error": msg }))
}

/// The Endless routes (see the table in the crate docs); None for any other request (the older
/// `POST /api/endless/<d>` is handled by the main router). `who`: the signed-in, unbanned
/// account's maker ID.
pub(crate) fn route(
    store: &Store,
    method: &str,
    parts: &[&str],
    q: &HashMap<String, String>,
    who: Option<&str>,
    body: &[u8],
) -> Option<Response> {
    let action = match parts {
        ["api", "endless"] if method == "GET" => None,
        ["api", "endless", _, a] => Some(*a),
        _ => return None,
    };
    let known = matches!(
        (method, action),
        ("GET", None | Some("next" | "history"))
            | ("POST", Some("start" | "result" | "suspend" | "finish"))
    );
    if !known {
        return None;
    }
    let Some(who) = who else {
        return Some(json_resp(401, json!({ "error": "sign in" })));
    };
    let Some(action) = action else {
        return Some(json_resp(200, store.endless_status(who)));
    };
    let Some(d) = Difficulty::from_name(parts[2]) else {
        return Some(json_resp(404, json!({ "error": "no such difficulty" })));
    };
    let name = d.name();
    let out = match action {
        "start" => {
            let v = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
            let ruleset = match v["ruleset"].as_str() {
                None => Ruleset::Smm2,
                Some(n) => match Ruleset::from_name(n) {
                    Some(r) => r,
                    None => {
                        return Some(json_resp(
                            400,
                            json!({ "error": "ruleset: smm2 or hundred" }),
                        ));
                    }
                },
            };
            store
                .endless_start(who, d, ruleset)
                .map(|s| json_resp(201, json!({ "difficulty": name, "run": s.to_json() })))
        }
        "next" => {
            let count = q.get("count").and_then(|c| c.parse().ok()).unwrap_or(BATCH);
            store.endless_next(who, d, count).map(|b| {
                json_resp(
                    200,
                    json!({
                        "difficulty": name,
                        "courses": b.courses.iter().map(Meta::to_json).collect::<Vec<_>>(),
                        "widened": b.widened,
                        "run": b.run.to_json(),
                    }),
                )
            })
        }
        "result" => {
            let v = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
            store.endless_report(who, d, &Report::from_json(&v)).map(|a| {
                json_resp(
                    200,
                    json!({
                        "difficulty": name, "lives": a.applied.lives, "clears": a.applied.clears,
                        "passed": a.applied.passed, "game_over": a.applied.game_over,
                        "run_complete": a.applied.complete, "best": a.best,
                    }),
                )
            })
        }
        "suspend" => store
            .endless_suspend(who, d)
            .map(|s| json_resp(200, json!({ "difficulty": name, "run": s.to_json() }))),
        "finish" => store.endless_finish(who, d).map(|f| {
            let mut v = f.to_json();
            v["difficulty"] = json!(name);
            json_resp(200, v)
        }),
        _ => store.endless_history(who, d).map(|h| {
            json_resp(
                200,
                json!({ "difficulty": name,
                        "history": h.iter().map(HistoryEntry::to_json).collect::<Vec<_>>() }),
            )
        }),
    };
    Some(out.unwrap_or_else(store_err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle;
    use mmk_formats::smm2::Course;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mmk-server-endless-{name}-{}-{}",
            std::process::id(),
            now()
        ));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn call(
        store: &Store,
        method: &str,
        url: &str,
        token: Option<&str>,
        body: &str,
    ) -> (u16, Value) {
        let headers: Vec<(String, String)> = token
            .map(|t| vec![("Authorization".into(), format!("Bearer {t}"))])
            .unwrap_or_default();
        let (st, _, b) = handle(store, method, url, &headers, body.as_bytes());
        (st, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    fn account(store: &Store, name: &str) -> (String, String) {
        let (st, v) = call(
            store,
            "POST",
            "/api/accounts",
            None,
            &format!(r#"{{"name":"{name}"}}"#),
        );
        assert_eq!(st, 201);
        (
            v["token"].as_str().unwrap().to_string(),
            v["account"]["maker_id"].as_str().unwrap().to_string(),
        )
    }

    /// Upload a blank course as `token` with these statistics.
    fn course(store: &Store, token: &str, clears: u64, attempts: u64) -> String {
        let a = store.account(token).unwrap();
        let data = Course::blank("M1").to_bytes().unwrap();
        let m = store.upload_as(&data, "", Some(&a)).unwrap();
        let mut g = store.inner.lock().unwrap();
        let i = g.by_code[&m.code];
        g.courses[i].clears = clears;
        g.courses[i].attempts = attempts;
        m.code
    }

    fn codes(v: &Value) -> Vec<String> {
        v["courses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["code"].as_str().unwrap().to_string())
            .collect()
    }

    fn report(code: &str, result: &str, extra: &str) -> String {
        format!(r#"{{"code":"{code}","result":"{result}"{extra}}}"#)
    }

    #[test]
    fn a_run_on_the_server() {
        let dir = tmp("run");
        let store = Store::open(&dir).unwrap();
        let (me, my_id) = account(&store, "Pia");
        let (maker, _) = account(&store, "Mo");
        let easy: Vec<String> = (0..12).map(|_| course(&store, &maker, 50, 100)).collect();
        let expert: Vec<String> = (0..3).map(|_| course(&store, &maker, 5, 100)).collect();
        let unrated: Vec<String> = (0..2).map(|_| course(&store, &maker, 0, 10)).collect();
        let own = course(&store, &me, 60, 100);
        let me = Some(me.as_str());

        assert_eq!(call(&store, "GET", "/api/endless", None, "").0, 401);
        let (st, s) = call(&store, "GET", "/api/endless", me, "");
        assert_eq!(st, 200);
        assert!(s["runs"]["easy"].is_null());
        assert_eq!(
            (s["best"]["easy"].as_u64(), s["skips"].as_u64()),
            (Some(0), Some(0))
        );
        assert_eq!(call(&store, "GET", "/api/endless/easy/next", me, "").0, 404);
        assert_eq!(
            call(&store, "POST", "/api/endless/silly/start", me, "").0,
            404
        );

        // New Game
        let (st, v) = call(&store, "POST", "/api/endless/easy/start", me, "");
        assert_eq!(st, 201);
        assert_eq!(
            (v["run"]["lives"].as_u64(), v["run"]["active"].as_bool()),
            (Some(5), Some(true))
        );
        let (st, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        assert_eq!(st, 200);
        let batch = codes(&b);
        assert_eq!(batch.len(), 10);
        assert!(
            batch.iter().all(|c| easy.contains(c)),
            "Easy band only, never my own"
        );
        assert!(!batch.contains(&own));
        assert_eq!(b["widened"], false);
        let (_, again) = call(&store, "GET", "/api/endless/easy/next?count=3", me, "");
        assert_eq!(codes(&again), batch[..3], "the queue is kept");

        // a miss, then a clear with 2 pending 1-Ups
        let c0 = &batch[0];
        let (st, a) = call(
            &store,
            "POST",
            "/api/endless/easy/result",
            me,
            &report(c0, "death", ""),
        );
        assert_eq!(st, 200);
        assert_eq!(
            (a["lives"].as_u64(), a["game_over"].as_bool()),
            (Some(4), Some(false))
        );
        let clear = report(
            c0,
            "clear",
            r#","pending":2,"coins":30,"score":100,"frames_left":600"#,
        );
        let (_, a) = call(&store, "POST", "/api/endless/easy/result", me, &clear);
        assert_eq!(
            (
                a["lives"].as_u64(),
                a["clears"].as_u64(),
                a["best"].as_u64()
            ),
            (Some(6), Some(1), Some(1))
        );
        let m = store.get(c0).unwrap();
        assert_eq!(
            (m.plays, m.attempts, m.clears),
            (1, 102, 51),
            "Endless plays count"
        );
        assert_eq!(m.first_clear, my_id);
        let (_, r) = call(&store, "GET", "/api/rankings/endless_easy", None, "");
        assert_eq!(r["rankings"][0]["score"], 1);
        // a course that is not in the run, or already passed; no result
        assert_eq!(
            call(&store, "POST", "/api/endless/easy/result", me, &clear).0,
            400
        );
        assert_eq!(
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(&own, "clear", "")
            )
            .0,
            400
        );
        assert_eq!(
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                r#"{"code":"x"}"#
            )
            .0,
            400
        );
        // a free skip
        let (_, a) = call(
            &store,
            "POST",
            "/api/endless/easy/result",
            me,
            &report(&batch[1], "skip", ""),
        );
        assert_eq!(
            (a["lives"].as_u64(), a["passed"].as_u64()),
            (Some(6), Some(2))
        );
        let (_, h) = call(&store, "GET", "/api/endless/easy/history", me, "");
        let h = h["history"].as_array().unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(
            (h[0]["result"].as_str(), h[0]["seq"].as_u64()),
            (Some("clear"), Some(1))
        );
        assert_eq!(
            (h[1]["result"].as_str(), h[1]["code"].as_str()),
            (Some("skip"), Some(batch[1].as_str()))
        );
        // suspend and status
        let (st, v) = call(&store, "POST", "/api/endless/easy/suspend", me, "");
        assert_eq!((st, v["run"]["active"].as_bool()), (200, Some(true)));
        let (_, s) = call(&store, "GET", "/api/endless", me, "");
        let run = RunStatus::from_json(&s["runs"]["easy"]);
        assert_eq!(
            (run.lives, run.clears, run.passed, run.coins, run.score),
            (6, 1, 2, 30, 600)
        );
        assert_eq!(
            (s["skips"].as_u64(), s["last"].as_str()),
            (Some(1), Some("easy"))
        );
        // persisted: the queue continues with the third course
        let store = Store::open(&dir).unwrap();
        let (_, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        assert_eq!(codes(&b)[0], batch[2]);
        assert_eq!(codes(&b).len(), 8);
        // clear the rest of the batch, then the band runs dry and the draw widens
        for c in &batch[2..] {
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(c, "clear", ""),
            );
        }
        let (_, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        assert_eq!(
            (codes(&b).len(), b["widened"].as_bool()),
            (2, Some(false)),
            "the last 2 Easy"
        );
        for c in codes(&b) {
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(&c, "clear", ""),
            );
        }
        let (_, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        let mut got = codes(&b);
        got.sort();
        let mut want = expert.clone();
        want.sort();
        assert_eq!(
            (got, b["widened"].as_bool()),
            (want, Some(true)),
            "Normal is empty: Expert"
        );
        assert_eq!(b["run"]["widened"], true);
        for c in codes(&b) {
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(&c, "skip", ""),
            );
        }
        let (_, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        let mut got = codes(&b);
        got.sort();
        let mut want = unrated.clone();
        want.sort();
        assert_eq!(got, want, "unrated courses come last");
        // game over
        let last = &codes(&b)[0];
        let mut a = Value::Null;
        for _ in 0..6 {
            a = call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(last, "start_over", ""),
            )
            .1;
        }
        assert_eq!(
            (a["lives"].as_u64(), a["game_over"].as_bool()),
            (Some(0), Some(true))
        );
        assert_eq!(call(&store, "GET", "/api/endless/easy/next", me, "").0, 400);
        let (st, f) = call(&store, "POST", "/api/endless/easy/finish", me, "");
        assert_eq!(st, 200);
        assert_eq!(
            (
                f["clears"].as_u64(),
                f["best"].as_u64(),
                f["new_high_score"].as_bool()
            ),
            (Some(11), Some(11), Some(true))
        );
        assert_eq!(
            call(&store, "GET", "/api/endless/easy/history", me, "").0,
            404
        );
        assert_eq!(
            call(&store, "POST", "/api/endless/easy/finish", me, "").0,
            404
        );
        // the older best-only route feeds the same best
        let (_, v) = call(&store, "POST", "/api/endless/easy", me, r#"{"cleared":20}"#);
        assert_eq!(v["best"], 20);
        let (_, s) = call(&store, "GET", "/api/endless", me, "");
        assert_eq!(s["best"]["easy"], 20);
        let (_, v) = call(&store, "POST", "/api/endless/easy/start", me, "");
        assert_eq!(v["run"]["best_before"], 20);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hundred_mario_run_on_the_server() {
        let dir = tmp("hundred");
        let store = Store::open(&dir).unwrap();
        let (me, my_id) = account(&store, "Pia");
        let (maker, _) = account(&store, "Mo");
        let easy: Vec<String> = (0..10).map(|_| course(&store, &maker, 80, 100)).collect();
        for _ in 0..3 {
            course(&store, &maker, 40, 100);
        }
        let me = Some(me.as_str());
        let start = |body: &str| call(&store, "POST", "/api/endless/easy/start", me, body);
        assert_eq!(start(r#"{"ruleset":"silly"}"#).0, 400);
        let (st, v) = start(r#"{"ruleset":"hundred"}"#);
        assert_eq!(st, 201);
        assert_eq!(
            (v["run"]["lives"].as_u64(), v["run"]["goal"].as_u64()),
            (Some(100), Some(8))
        );
        assert_eq!(v["run"]["ruleset"], "hundred");
        let (_, b) = call(&store, "GET", "/api/endless/easy/next", me, "");
        let batch = codes(&b);
        assert!(batch.iter().all(|c| easy.contains(c)), "fail rate 0–34 %");
        let post = |code: &str, result: &str, extra: &str| {
            call(
                &store,
                "POST",
                "/api/endless/easy/result",
                me,
                &report(code, result, extra),
            )
        };
        post(&batch[0], "death", "");
        let (_, a) = post(&batch[0], "skip", r#","during_miss":true"#);
        assert_eq!(a["lives"].as_u64(), Some(99), "free skip");
        for (k, c) in batch[1..9].iter().enumerate() {
            let (st, a) = post(c, "clear", "");
            assert_eq!(st, 200);
            assert_eq!(a["run_complete"].as_bool(), Some(k == 7));
        }
        let (_, a) = call(&store, "GET", "/api/endless", me, "");
        assert_eq!(a["runs"]["easy"]["complete"], true);
        assert_eq!(a["best"]["easy"].as_u64(), Some(0), "not an Endless score");
        assert_eq!(call(&store, "GET", "/api/endless/easy/next", me, "").0, 400);
        let (_, r) = call(&store, "GET", "/api/rankings/hundred_easy", me, "");
        assert_eq!(r["rankings"][0]["score"], 1);
        assert_eq!(r["me"]["maker_id"], my_id.as_str());
        assert_eq!(r["me"]["rank"], 1);
        let (_, r) = call(&store, "GET", "/api/rankings/hundred_easy", None, "");
        assert!(r["me"].is_null());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn banned_makers_and_own_courses_are_never_drawn() {
        let dir = tmp("banned");
        let store = Store::open(&dir).unwrap().with_admin(Some("root".into()));
        let (me, _) = account(&store, "Pia");
        let (bad, _) = account(&store, "Bad");
        let (good, _) = account(&store, "Good");
        for _ in 0..3 {
            course(&store, &bad, 1, 100);
        }
        course(&store, &me, 1, 100);
        let fine = course(&store, &good, 1, 100);
        let id = store.account(&bad).unwrap().id;
        assert_eq!(
            call(
                &store,
                "POST",
                &format!("/api/admin/ban/{id}"),
                Some("root"),
                ""
            )
            .0,
            200
        );
        let me = Some(me.as_str());
        call(&store, "POST", "/api/endless/super_expert/start", me, "");
        let (_, b) = call(&store, "GET", "/api/endless/super_expert/next", me, "");
        assert_eq!(codes(&b), vec![fine]);
        // a banned player can't play either
        assert_eq!(
            call(&store, "POST", "/api/endless/easy/start", Some(&bad), "").0,
            401
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
