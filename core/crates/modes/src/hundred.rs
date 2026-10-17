//! 100 Mario Challenge (and 10 Mario) run rules.
//!
//! Run lengths and lives are client constants in SMM1 v208: 100 Mario Easy 8 / Normal 16 /
//! Expert 16 / Super Expert 6 courses with 100 lives; 10 Mario 8 courses with 10 lives. Lives never
//! exceed the starting value. Extra lives (1-Ups, 100-coin rollovers) are *banked*, at most 3 per
//! course, and paid out only when the course is cleared; in 100 Mario a clear also adds 1 (it
//! refunds the life pre-charged when the course started). A death costs one life and retries the
//! same course from its start state; a skip (100 Mario only) costs nothing and moves to the next
//! course; Save & Quit keeps the course-start lives.

use crate::pool::CourseRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Difficulty {
    Easy,
    Normal,
    Expert,
    SuperExpert,
}

impl Difficulty {
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Expert,
        Difficulty::SuperExpert,
    ];

    /// Courses to clear in one 100 Mario run.
    pub fn run_length(self) -> usize {
        match self {
            Difficulty::Easy => 8,
            Difficulty::Normal | Difficulty::Expert => 16,
            Difficulty::SuperExpert => 6,
        }
    }

    /// Difficulty band the pool is drawn from (fail-rate value 0–100, inclusive), as the v208
    /// client requests it: Easy 0–34, Normal 0–74 (Easy courses included), Expert 75–95, Super
    /// Expert 96–100.
    pub fn band(self) -> (u8, u8) {
        match self {
            Difficulty::Easy => (0, 34),
            Difficulty::Normal => (0, 74),
            Difficulty::Expert => (75, 95),
            Difficulty::SuperExpert => (96, 100),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Expert => "expert",
            Difficulty::SuperExpert => "super_expert",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    HundredMario(Difficulty),
    TenMario,
}

impl Mode {
    pub fn run_length(self) -> usize {
        match self {
            Mode::HundredMario(d) => d.run_length(),
            Mode::TenMario => 8,
        }
    }
    pub fn start_lives(self) -> u32 {
        match self {
            Mode::HundredMario(_) => 100,
            Mode::TenMario => 10,
        }
    }
    pub fn can_skip(self) -> bool {
        matches!(self, Mode::HundredMario(_))
    }
}

/// Coins, score and lives: kept as "current" and as the course-start snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub coins: u32,
    pub score: u32,
    pub lives: u32,
}

/// How a course attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Cleared,
    Died,
    Skipped,
    SaveAndQuit,
}

/// What the run does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Play this course (index into the run).
    Play,
    /// Retry the same course.
    Retry,
    GameOver,
    /// All courses cleared: ending, credits and reward.
    Complete,
    /// Back to the world map (Save & Quit).
    Quit,
}

pub const MAX_BANKED: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub mode: Mode,
    /// Courses cleared so far (= index of the current course).
    pub course_index: usize,
    pub current: Counters,
    /// State at the start of the current course (restored on death/skip).
    pub snapshot: Counters,
    /// Extra lives banked during this course (paid on clear, ≤ 3).
    pub banked: u32,
    /// Courses cleared, for the staff credits.
    pub cleared: Vec<CourseRef>,
    pub deaths: u32,
    pub skips: u32,
}

impl Run {
    pub fn new(mode: Mode) -> Self {
        let c = Counters {
            coins: 0,
            score: 0,
            lives: mode.start_lives(),
        };
        Self {
            mode,
            course_index: 0,
            current: c,
            snapshot: c,
            banked: 0,
            cleared: Vec::new(),
            deaths: 0,
            skips: 0,
        }
    }

    /// Resume from a Continue record.
    pub fn resume(mode: Mode, counters: Counters, cleared: Vec<CourseRef>) -> Self {
        let mut r = Self::new(mode);
        r.course_index = cleared.len();
        r.current = counters;
        r.snapshot = counters;
        r.cleared = cleared;
        r
    }

    pub fn total(&self) -> usize {
        self.mode.run_length()
    }

    /// "course n / total" for the HUD and intro card (1-based).
    pub fn course_number(&self) -> usize {
        self.course_index + 1
    }

    /// The last course of a run comes from a separate "castle" search.
    pub fn is_last_course(&self) -> bool {
        self.course_index + 1 >= self.total()
    }

    /// A 1-Up or a 100-coin rollover during the course: banked, never more than 3.
    pub fn extra_life(&mut self) {
        self.banked = (self.banked + 1).min(MAX_BANKED);
    }

    /// Coins collected in the course (the counter rolls over at 100 into a banked life).
    pub fn add_coins(&mut self, n: u32) {
        for _ in 0..n {
            self.current.coins += 1;
            if self.current.coins >= 100 {
                self.current.coins -= 100;
                self.extra_life();
            }
        }
    }

    pub fn add_score(&mut self, points: u32) {
        self.current.score = (self.current.score + points).min(999_999_990);
    }

    /// Apply the result of a course attempt and say what happens next.
    pub fn finish(&mut self, outcome: Outcome, course: Option<&CourseRef>) -> Next {
        let cap = self.mode.start_lives();
        match outcome {
            Outcome::Cleared => {
                let mut lives = (self.current.lives + self.banked).min(cap);
                if matches!(self.mode, Mode::HundredMario(_)) {
                    lives = (lives + 1).min(cap);
                }
                self.current.lives = lives;
                self.banked = 0;
                if let Some(c) = course {
                    self.cleared.push(c.clone());
                }
                self.course_index += 1;
                self.snapshot = self.current;
                if self.course_index >= self.total() {
                    Next::Complete
                } else {
                    Next::Play
                }
            }
            Outcome::Died => {
                self.deaths += 1;
                // coins and score of the failed attempt are discarded; banked extras stay
                let lives = self.snapshot.lives.saturating_sub(1);
                self.snapshot.lives = lives;
                self.current = self.snapshot;
                if lives == 0 {
                    Next::GameOver
                } else {
                    Next::Retry
                }
            }
            Outcome::Skipped => {
                debug_assert!(self.mode.can_skip());
                self.skips += 1;
                self.current = self.snapshot;
                self.banked = 0;
                Next::Play
            }
            Outcome::SaveAndQuit => {
                self.current = self.snapshot;
                Next::Quit
            }
        }
    }

    /// Lives the run would have if the game were switched off mid-course (the pre-charged
    /// course-start value written to the Continue record) — 100 Mario only.
    pub fn saved_lives(&self) -> u32 {
        match self.mode {
            Mode::HundredMario(_) => self.snapshot.lives.saturating_sub(1),
            Mode::TenMario => self.snapshot.lives,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course(id: u32) -> CourseRef {
        CourseRef {
            id: id.to_string(),
            name: format!("course {id}"),
            maker: "maker".into(),
            difficulty: 50,
        }
    }

    #[test]
    fn run_lengths_and_lives() {
        assert_eq!(Mode::HundredMario(Difficulty::Easy).run_length(), 8);
        assert_eq!(Mode::HundredMario(Difficulty::Normal).run_length(), 16);
        assert_eq!(Mode::HundredMario(Difficulty::Expert).run_length(), 16);
        assert_eq!(Mode::HundredMario(Difficulty::SuperExpert).run_length(), 6);
        assert_eq!(Run::new(Mode::TenMario).current.lives, 10);
        assert_eq!(
            Run::new(Mode::HundredMario(Difficulty::Easy)).current.lives,
            100
        );
    }

    #[test]
    fn banked_lives_cap_at_three_and_pay_on_clear() {
        let mut r = Run::new(Mode::HundredMario(Difficulty::Normal));
        r.finish(Outcome::Died, None);
        r.finish(Outcome::Died, None);
        assert_eq!(r.current.lives, 98);
        for _ in 0..5 {
            r.extra_life();
        }
        assert_eq!(r.banked, 3);
        assert_eq!(r.finish(Outcome::Cleared, Some(&course(1))), Next::Play);
        // 98 + 3 banked + 1 refund
        assert_eq!(r.current.lives, 100, "capped at the starting value");
        assert_eq!(r.course_index, 1);
    }

    #[test]
    fn death_restores_course_start_and_keeps_banked() {
        let mut r = Run::new(Mode::HundredMario(Difficulty::Easy));
        r.add_coins(30);
        r.add_score(5000);
        r.extra_life();
        assert_eq!(r.finish(Outcome::Died, None), Next::Retry);
        assert_eq!(r.current.coins, 0);
        assert_eq!(r.current.score, 0);
        assert_eq!(r.current.lives, 99);
        assert_eq!(r.banked, 1, "a death does not reset the bank");
    }

    #[test]
    fn skip_costs_nothing_and_does_not_advance_the_count() {
        let mut r = Run::new(Mode::HundredMario(Difficulty::Expert));
        r.finish(Outcome::Died, None);
        r.extra_life();
        assert_eq!(r.finish(Outcome::Skipped, None), Next::Play);
        assert_eq!(r.current.lives, 99, "lives lost before the skip stay lost");
        assert_eq!(r.banked, 0);
        assert_eq!(r.course_index, 0);
    }

    #[test]
    fn game_over_and_completion() {
        let mut r = Run::new(Mode::TenMario);
        for _ in 0..9 {
            assert_eq!(r.finish(Outcome::Died, None), Next::Retry);
        }
        assert_eq!(r.finish(Outcome::Died, None), Next::GameOver);
        let mut r = Run::new(Mode::HundredMario(Difficulty::SuperExpert));
        for i in 0..5 {
            assert_eq!(r.finish(Outcome::Cleared, Some(&course(i))), Next::Play);
        }
        assert!(r.is_last_course());
        assert_eq!(r.finish(Outcome::Cleared, Some(&course(9))), Next::Complete);
        assert_eq!(r.cleared.len(), 6);
    }

    #[test]
    fn coin_rollover_banks_a_life() {
        let mut r = Run::new(Mode::HundredMario(Difficulty::Normal));
        r.add_coins(150);
        assert_eq!(r.current.coins, 50);
        assert_eq!(r.banked, 1);
    }
}
