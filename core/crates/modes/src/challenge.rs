//! One 100 Mario Challenge session over a local course library: the run, its course pool, the
//! save data and the course source, driven by the app one course result at a time.
//!
//! Flow (smm1-100mario-code.md §4.1, §6): `start` (New Game or Continue) picks the first course;
//! `course_started` pre-charges a life in the Continue record; the course result goes to `finish`
//! (clear, death, skip, Save & Quit), which autosaves and picks the next course. A course that
//! fails to load is replaced (`unloadable`, "The next course will be loaded").
//!
//! The local source differs from the server in two deliberate ways: the first search may return
//! fewer than 32 courses (a small library still plays), and courses already cleared in this run
//! are skipped while others remain. The castle search (the run's last course) prefers courses
//! with the castle theme [inf: the server's castle request filters by course type].

use std::collections::HashSet;

use serde_json::Value;

use crate::Rng;
use crate::hundred::{Difficulty, Mode, Next, Outcome, Run};
use crate::pool::{CourseRef, CourseSource, Pool, PoolError};
use crate::save::SaveData;

/// A library course offered to the pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryCourse {
    pub course: CourseRef,
    /// Main area uses the castle theme (preferred for the run's last course).
    pub castle: bool,
}

/// The local library as a [`CourseSource`].
#[derive(Clone, Debug, Default)]
pub struct LibrarySource {
    pub courses: Vec<LibraryCourse>,
    /// Ids not to offer while others remain (cleared earlier in this run).
    pub exclude: HashSet<String>,
}

impl LibrarySource {
    pub fn in_band(&self, band: (u8, u8)) -> usize {
        self.courses
            .iter()
            .filter(|c| (band.0..=band.1).contains(&c.course.difficulty))
            .count()
    }
}

impl CourseSource for LibrarySource {
    fn search(
        &mut self,
        band: (u8, u8),
        max: usize,
        castle: bool,
        rng: &mut Rng,
    ) -> Vec<CourseRef> {
        let pick = |exclude: bool| -> Vec<CourseRef> {
            self.courses
                .iter()
                .filter(|c| (band.0..=band.1).contains(&c.course.difficulty))
                .filter(|c| !castle || c.castle)
                .filter(|c| !exclude || !self.exclude.contains(&c.course.id))
                .map(|c| c.course.clone())
                .collect()
        };
        let mut c = pick(true);
        if c.is_empty() {
            c = pick(false);
        }
        let n = c.len();
        for i in 0..n.min(max) {
            let j = i + rng.below((n - i) as u32) as usize;
            c.swap(i, j);
        }
        c.truncate(max);
        c
    }
}

/// Counters for the HUD, intro card and world map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// 1-based number of the course being played.
    pub course_number: u32,
    pub total: u32,
    pub lives: u32,
    pub coins: u32,
    pub score: u32,
    pub banked: u32,
    pub deaths: u32,
    pub skips: u32,
}

pub struct Challenge {
    pub save: SaveData,
    pub source: LibrarySource,
    rng: Rng,
    pool: Option<Pool>,
    run: Option<Run>,
    current: Option<CourseRef>,
    /// Unlock to announce once ("You've unlocked Expert mode…").
    announce: Option<Difficulty>,
}

impl Challenge {
    pub fn new(save: SaveData, seed: u64) -> Self {
        Self {
            save,
            source: LibrarySource::default(),
            rng: Rng::new(seed),
            pool: None,
            run: None,
            current: None,
            announce: None,
        }
    }

    pub fn from_json(save: Option<&str>, seed: u64) -> Self {
        let data = save
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .map(|v| SaveData::from_json(&v))
            .unwrap_or_default();
        Self::new(data, seed)
    }

    pub fn save_json(&self) -> String {
        self.save.to_json().to_string()
    }

    pub fn add_course(&mut self, course: CourseRef, castle: bool) {
        self.source.courses.push(LibraryCourse { course, castle });
    }

    /// Library courses in the difficulty's band.
    pub fn available(&self, d: Difficulty) -> usize {
        self.source.in_band(d.band())
    }

    pub fn difficulty(&self) -> Option<Difficulty> {
        match self.run.as_ref()?.mode {
            Mode::HundredMario(d) => Some(d),
            Mode::TenMario => None,
        }
    }

    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    pub fn current(&self) -> Option<&CourseRef> {
        self.current.as_ref()
    }

    /// New Game (`resume` false) or Continue. Picks the first course.
    pub fn start(&mut self, d: Difficulty, resume: bool) -> Result<(), PoolError> {
        let run = if resume {
            self.save
                .continue_run(d)
                .ok_or(PoolError::NotEnoughCourses)?
        } else {
            self.save.new_game(d)
        };
        self.source.exclude = run.cleared.iter().map(|c| c.id.clone()).collect();
        self.pool = Some(Pool::with_minimum(d, 1));
        self.run = Some(run);
        self.pick()
    }

    fn pick(&mut self) -> Result<(), PoolError> {
        let (Some(pool), Some(run)) = (self.pool.as_mut(), self.run.as_ref()) else {
            return Err(PoolError::NotEnoughCourses);
        };
        let last = run.is_last_course();
        let picked = pool.current(&mut self.source, last, &mut self.rng);
        match picked {
            Ok(c) => {
                self.current = Some(c);
                Ok(())
            }
            Err(e) => {
                self.current = None;
                Err(e)
            }
        }
    }

    /// The course is about to be played (after its intro card): the Continue record gets the
    /// pre-charged lives, so quitting the app mid-course costs one life.
    pub fn course_started(&mut self) {
        if let Some(run) = &self.run {
            self.save.autosave(run, true);
        }
    }

    /// The current course failed to load: replace it with the next one.
    pub fn unloadable(&mut self) -> Result<(), PoolError> {
        if let Some(p) = self.pool.as_mut() {
            p.advance();
        }
        self.pick()
    }

    /// Extra lives earned during the attempt (1-Ups, 100-coin rollovers): banked, ≤ 3.
    pub fn extra_lives(&mut self, n: u32) {
        if let Some(run) = self.run.as_mut() {
            for _ in 0..n {
                run.extra_life();
            }
        }
    }

    /// Apply a course result. `score`/`coins`: the engine's counters at the end of a clear
    /// (they started from the course-start snapshot). Returns what the app shows next.
    pub fn finish(&mut self, outcome: Outcome, score: u32, coins: u32) -> Next {
        let Some(run) = self.run.as_mut() else {
            return Next::Quit;
        };
        let Mode::HundredMario(d) = run.mode else {
            return Next::Quit;
        };
        if outcome == Outcome::Cleared {
            run.current.score = score.min(999_999_990);
            run.current.coins = coins % 100;
        }
        let course = self.current.clone();
        let next = run.finish(outcome, course.as_ref());
        let i = d as usize;
        match outcome {
            Outcome::Cleared => {
                if let Some(c) = &course {
                    self.source.exclude.insert(c.id.clone());
                }
                self.save.autosave(run, false);
            }
            Outcome::Died => {
                self.save.lives_lost += 1;
                // the retry is a course in progress again
                self.save.autosave(run, true);
            }
            Outcome::Skipped => self.save.skips[i] += 1,
            Outcome::SaveAndQuit => self.save.autosave(run, false),
        }
        match next {
            Next::Play => {
                if let Some(p) = self.pool.as_mut() {
                    p.advance();
                    if run.is_last_course() {
                        p.prepare_last();
                    }
                }
                if self.pick().is_err() {
                    // the library ran dry mid-run: keep the record and leave
                    return Next::Quit;
                }
            }
            Next::Complete => {
                let unlocked_before = [
                    self.save.unlocked(Difficulty::Expert),
                    self.save.unlocked(Difficulty::SuperExpert),
                ];
                self.save.complete(d);
                for (k, d) in [Difficulty::Expert, Difficulty::SuperExpert]
                    .into_iter()
                    .enumerate()
                {
                    if !unlocked_before[k] && self.save.unlocked(d) && !self.save.unlock_seen[k] {
                        self.save.unlock_seen[k] = true;
                        self.announce = Some(d);
                    }
                }
                self.current = None;
            }
            Next::GameOver | Next::Quit => self.current = None,
            Next::Retry => {}
        }
        next
    }

    /// A difficulty newly unlocked by the last completed run (shown once), then cleared.
    pub fn take_unlock(&mut self) -> Option<Difficulty> {
        self.announce.take()
    }

    /// Leave the run (after Game Over, the ending or Save & Quit).
    pub fn end(&mut self) {
        self.run = None;
        self.pool = None;
        self.current = None;
    }

    pub fn status(&self) -> Status {
        let Some(r) = &self.run else {
            return Status::default();
        };
        Status {
            course_number: r.course_number().min(r.total()) as u32,
            total: r.total() as u32,
            lives: r.current.lives,
            coins: r.current.coins,
            score: r.current.score,
            banked: r.banked,
            deaths: r.deaths,
            skips: r.skips,
        }
    }

    /// Courses cleared this run, for the staff credits.
    pub fn cleared(&self) -> &[CourseRef] {
        self.run
            .as_ref()
            .map(|r| r.cleared.as_slice())
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(n: usize, difficulty: impl Fn(usize) -> u8) -> Challenge {
        let mut c = Challenge::new(SaveData::default(), 9);
        for i in 0..n {
            c.add_course(
                CourseRef {
                    id: format!("c{i}"),
                    name: format!("Course {i}"),
                    maker: "m".into(),
                    difficulty: difficulty(i),
                },
                i % 5 == 0,
            );
        }
        c
    }

    #[test]
    fn a_full_easy_run_with_a_small_library() {
        let mut c = library(10, |_| 20);
        c.start(Difficulty::Easy, false).unwrap();
        let mut seen = HashSet::new();
        for k in 0..8 {
            let course = c.current().unwrap().clone();
            assert!(
                seen.insert(course.id.clone()),
                "no repeats while others remain"
            );
            c.course_started();
            let next = c.finish(Outcome::Cleared, 1000 * (k + 1), 5);
            assert_eq!(next, if k == 7 { Next::Complete } else { Next::Play });
        }
        // the last course came from the castle search (ids divisible by 5 are castles)
        let last = &c.cleared()[7];
        let i: usize = last.id[1..].parse().unwrap();
        assert_eq!(i % 5, 0, "{last:?}");
        assert_eq!(c.save.clears[0], 1);
        assert!(c.save.records[0].is_none());
        assert_eq!(c.take_unlock(), None, "Easy unlocks nothing");
    }

    #[test]
    fn deaths_and_skips_follow_the_run_rules() {
        let mut c = library(40, |i| (i * 2) as u8);
        c.start(Difficulty::Normal, false).unwrap();
        let first = c.current().unwrap().clone();
        c.course_started();
        assert_eq!(c.save.records[1].as_ref().unwrap().counters.lives, 99);
        assert_eq!(c.finish(Outcome::Died, 0, 0), Next::Retry);
        assert_eq!(
            c.current().unwrap(),
            &first,
            "a death retries the same course"
        );
        assert_eq!(c.status().lives, 99);
        assert_eq!(c.save.lives_lost, 1);
        assert_eq!(c.finish(Outcome::Skipped, 0, 0), Next::Play);
        assert_ne!(c.current().unwrap(), &first);
        assert_eq!(c.status().course_number, 1, "a skip doesn't count");
        assert_eq!(c.status().lives, 99);
        c.extra_lives(5);
        assert_eq!(c.finish(Outcome::Cleared, 500, 7), Next::Play);
        assert_eq!(c.status().lives, 100, "99 + 3 banked + 1, capped");
        assert_eq!(c.status().course_number, 2);
        // Save & Quit, then Continue from the record
        assert_eq!(c.finish(Outcome::SaveAndQuit, 0, 0), Next::Quit);
        c.end();
        let json = c.save_json();
        let mut back = Challenge::from_json(Some(&json), 1);
        back.source = c.source.clone();
        assert!(back.save.can_continue(Difficulty::Normal));
        back.start(Difficulty::Normal, true).unwrap();
        assert_eq!(back.status().course_number, 2);
        assert_eq!(back.status().score, 500);
        assert!(!back.source.exclude.is_empty());
    }

    #[test]
    fn completing_normal_announces_expert_once() {
        let mut c = library(20, |_| 50);
        c.start(Difficulty::Normal, false).unwrap();
        for _ in 0..16 {
            c.finish(Outcome::Cleared, 0, 0);
        }
        assert_eq!(c.take_unlock(), Some(Difficulty::Expert));
        assert_eq!(c.take_unlock(), None);
        assert!(c.save.unlocked(Difficulty::Expert));
        c.end();
        c.start(Difficulty::Normal, false).unwrap();
        for _ in 0..16 {
            c.finish(Outcome::Cleared, 0, 0);
        }
        assert_eq!(c.take_unlock(), None, "shown once");
    }

    #[test]
    fn empty_band_is_not_enough_courses() {
        let mut c = library(10, |_| 20);
        assert_eq!(c.available(Difficulty::Expert), 0);
        assert_eq!(
            c.start(Difficulty::Expert, false),
            Err(PoolError::NotEnoughCourses)
        );
    }

    #[test]
    fn game_over_leaves_no_continue() {
        let mut c = library(10, |_| 20);
        c.start(Difficulty::Easy, false).unwrap();
        c.finish(Outcome::Cleared, 0, 0);
        let mut next = Next::Retry;
        while next == Next::Retry {
            c.course_started();
            next = c.finish(Outcome::Died, 0, 0);
        }
        assert_eq!(next, Next::GameOver);
        assert!(!c.save.can_continue(Difficulty::Easy));
    }
}
