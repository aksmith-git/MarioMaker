//! 100 Mario save data: one Continue record per difficulty, unlocks, statistics (JSON).
//!
//! Rules (smm1-100mario-code.md §5–§7): Expert unlocks after one Normal clear (Easy clears don't
//! count), Super Expert after one Expert clear. Continue is offered when 0 < cleared < run length
//! and the saved lives are not 0. New Game wipes that difficulty's record (lives 100). After every
//! clear the record gets the course, score, coins and lives.

use serde_json::{Value, json};

use crate::hundred::{Counters, Difficulty, Mode, Run};
use crate::pool::CourseRef;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContinueRecord {
    pub counters: Counters,
    pub cleared: Vec<CourseRef>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveData {
    /// Continue records by difficulty (index = Difficulty as usize).
    pub records: [Option<ContinueRecord>; 4],
    /// Completed runs per difficulty.
    pub clears: [u32; 4],
    pub runs_started: [u32; 4],
    pub skips: [u32; 4],
    pub lives_lost: u64,
    /// "Unlocked" messages already shown (Expert, Super Expert).
    pub unlock_seen: [bool; 2],
}

fn idx(d: Difficulty) -> usize {
    d as usize
}

impl SaveData {
    pub fn unlocked(&self, d: Difficulty) -> bool {
        match d {
            Difficulty::Easy | Difficulty::Normal => true,
            Difficulty::Expert => self.clears[idx(Difficulty::Normal)] >= 1,
            Difficulty::SuperExpert => self.clears[idx(Difficulty::Expert)] >= 1,
        }
    }

    pub fn can_continue(&self, d: Difficulty) -> bool {
        self.records[idx(d)].as_ref().is_some_and(|r| {
            !r.cleared.is_empty() && r.cleared.len() < d.run_length() && r.counters.lives != 0
        })
    }

    /// New Game: wipe the record and start a run.
    pub fn new_game(&mut self, d: Difficulty) -> Run {
        self.runs_started[idx(d)] += 1;
        let run = Run::new(Mode::HundredMario(d));
        self.records[idx(d)] = Some(ContinueRecord {
            counters: run.current,
            cleared: Vec::new(),
        });
        run
    }

    pub fn continue_run(&self, d: Difficulty) -> Option<Run> {
        let r = self.records[idx(d)].as_ref()?;
        Some(Run::resume(
            Mode::HundredMario(d),
            r.counters,
            r.cleared.clone(),
        ))
    }

    /// Autosave after a course result: the run's course-start state. While a course is being
    /// played the record holds the pre-charged lives (one less), so switching off costs a life.
    pub fn autosave(&mut self, run: &Run, mid_course: bool) {
        let Mode::HundredMario(d) = run.mode else {
            return;
        };
        let mut counters = run.snapshot;
        if mid_course {
            counters.lives = run.saved_lives();
        }
        self.records[idx(d)] = Some(ContinueRecord {
            counters,
            cleared: run.cleared.clone(),
        });
    }

    /// A completed run: count the clear and drop the Continue record.
    pub fn complete(&mut self, d: Difficulty) {
        self.clears[idx(d)] += 1;
        self.records[idx(d)] = None;
    }

    pub fn to_json(&self) -> Value {
        let rec = |r: &Option<ContinueRecord>| match r {
            None => Value::Null,
            Some(r) => json!({
                "coins": r.counters.coins,
                "score": r.counters.score,
                "lives": r.counters.lives,
                "cleared": r.cleared.iter().map(|c| json!({
                    "id": c.id, "name": c.name, "maker": c.maker, "difficulty": c.difficulty,
                })).collect::<Vec<_>>(),
            }),
        };
        json!({
            "version": 1,
            "records": Difficulty::ALL.iter().map(|&d| (d.name().to_string(), rec(&self.records[idx(d)])))
                .collect::<serde_json::Map<String, Value>>(),
            "clears": self.clears,
            "runs_started": self.runs_started,
            "skips": self.skips,
            "lives_lost": self.lives_lost,
            "unlock_seen": self.unlock_seen,
        })
    }

    pub fn from_json(v: &Value) -> Self {
        let mut s = SaveData::default();
        let arr4 = |key: &str| -> [u32; 4] {
            let mut out = [0; 4];
            if let Some(a) = v.get(key).and_then(Value::as_array) {
                for (o, x) in out.iter_mut().zip(a) {
                    *o = x.as_u64().unwrap_or(0) as u32;
                }
            }
            out
        };
        s.clears = arr4("clears");
        s.runs_started = arr4("runs_started");
        s.skips = arr4("skips");
        s.lives_lost = v.get("lives_lost").and_then(Value::as_u64).unwrap_or(0);
        if let Some(a) = v.get("unlock_seen").and_then(Value::as_array) {
            for (o, x) in s.unlock_seen.iter_mut().zip(a) {
                *o = x.as_bool().unwrap_or(false);
            }
        }
        if let Some(recs) = v.get("records").and_then(Value::as_object) {
            for (name, r) in recs {
                let (Some(d), Some(r)) = (Difficulty::from_name(name), r.as_object()) else {
                    continue;
                };
                let n = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
                let cleared = r
                    .get("cleared")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|c| CourseRef {
                                id: c["id"].as_str().unwrap_or_default().to_string(),
                                name: c["name"].as_str().unwrap_or_default().to_string(),
                                maker: c["maker"].as_str().unwrap_or_default().to_string(),
                                difficulty: c["difficulty"].as_u64().unwrap_or(0) as u8,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                s.records[idx(d)] = Some(ContinueRecord {
                    counters: Counters {
                        coins: n("coins"),
                        score: n("score"),
                        lives: n("lives"),
                    },
                    cleared,
                });
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hundred::Outcome;

    #[test]
    fn unlock_rules() {
        let mut s = SaveData::default();
        assert!(s.unlocked(Difficulty::Easy) && s.unlocked(Difficulty::Normal));
        assert!(!s.unlocked(Difficulty::Expert));
        s.complete(Difficulty::Easy);
        assert!(!s.unlocked(Difficulty::Expert), "Easy clears don't count");
        s.complete(Difficulty::Normal);
        assert!(s.unlocked(Difficulty::Expert));
        assert!(!s.unlocked(Difficulty::SuperExpert));
        s.complete(Difficulty::Expert);
        assert!(s.unlocked(Difficulty::SuperExpert));
    }

    #[test]
    fn continue_round_trip() {
        let mut s = SaveData::default();
        let mut run = s.new_game(Difficulty::Normal);
        assert!(!s.can_continue(Difficulty::Normal));
        let c = CourseRef {
            id: "a".into(),
            name: "Alpha".into(),
            maker: "m".into(),
            difficulty: 40,
        };
        run.finish(Outcome::Cleared, Some(&c));
        s.autosave(&run, false);
        assert!(s.can_continue(Difficulty::Normal));
        let back = SaveData::from_json(&s.to_json());
        assert_eq!(back, s);
        let resumed = back.continue_run(Difficulty::Normal).unwrap();
        assert_eq!(resumed.course_index, 1);
        assert_eq!(resumed.cleared[0].name, "Alpha");
    }

    #[test]
    fn mid_course_save_pre_charges_a_life() {
        let mut s = SaveData::default();
        let run = s.new_game(Difficulty::Easy);
        s.autosave(&run, true);
        assert_eq!(
            s.records[0].as_ref().unwrap().counters.lives,
            99,
            "switching off mid-course costs a life"
        );
    }
}
