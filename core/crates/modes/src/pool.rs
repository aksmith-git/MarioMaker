//! Course pools for a run.
//!
//! The SMM1 client asks the server for 50 courses in the difficulty band, drops the player's own
//! courses, shuffles (entry 0 is never moved: for i = n−1 down to 2 swap i with a random index in
//! [1, i]) and plays the pool in order; when it runs out it searches again. The run's last course
//! comes from a separate "castle" search, falling back to the normal one. A first search with
//! fewer than 32 results (or any with 0) reports "not enough courses".
//!
//! Here the server is a [`CourseSource`]: the local library, or (later) a remote course service.
//! Only courses the engine can fully play are offered.

use crate::Rng;
use crate::hundred::Difficulty;

/// A course as the run manager sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseRef {
    /// Source-specific id (file path, course code…).
    pub id: String,
    pub name: String,
    pub maker: String,
    /// Difficulty value 0–100 (fail rate: 100 − clear %), as the bands use it.
    pub difficulty: u8,
}

pub const POOL_REQUEST: usize = 50;
pub const NOT_ENOUGH: usize = 32;

/// Where run courses come from.
pub trait CourseSource {
    /// Up to `max` playable courses whose difficulty value lies in `band` (inclusive). `castle` =
    /// the search for a run's last course.
    fn search(&mut self, band: (u8, u8), max: usize, castle: bool, rng: &mut Rng)
    -> Vec<CourseRef>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolError {
    NotEnoughCourses,
}

/// SMM1's shuffle: entry 0 stays first.
pub fn smm1_shuffle<T>(v: &mut [T], rng: &mut Rng) {
    let n = v.len();
    if n < 3 {
        return;
    }
    for i in (2..n).rev() {
        let j = 1 + rng.below(i as u32) as usize;
        v.swap(i, j);
    }
}

#[derive(Clone, Debug)]
pub struct Pool {
    pub difficulty: Difficulty,
    entries: Vec<CourseRef>,
    cursor: usize,
    first_search: bool,
    /// Fewest results the first search may return ([`NOT_ENOUGH`] for the server).
    pub min_first: usize,
}

impl Pool {
    pub fn new(difficulty: Difficulty) -> Self {
        Self {
            difficulty,
            entries: Vec::new(),
            cursor: 0,
            first_search: true,
            min_first: NOT_ENOUGH,
        }
    }

    /// A pool whose first search may return as few as `min_first` courses (a local library).
    pub fn with_minimum(difficulty: Difficulty, min_first: usize) -> Self {
        Self {
            min_first: min_first.max(1),
            ..Self::new(difficulty)
        }
    }

    fn search(
        &mut self,
        src: &mut dyn CourseSource,
        castle: bool,
        rng: &mut Rng,
    ) -> Result<(), PoolError> {
        let band = self.difficulty.band();
        let mut found = src.search(band, POOL_REQUEST, castle, rng);
        if castle && found.is_empty() {
            found = src.search(band, POOL_REQUEST, false, rng);
        }
        let too_few = found.is_empty() || (self.first_search && found.len() < self.min_first);
        self.first_search = false;
        if too_few {
            return Err(PoolError::NotEnoughCourses);
        }
        smm1_shuffle(&mut found, rng);
        self.entries = found;
        self.cursor = 0;
        Ok(())
    }

    /// The course to play next. `last` = it is the run's final course (castle search).
    pub fn current(
        &mut self,
        src: &mut dyn CourseSource,
        last: bool,
        rng: &mut Rng,
    ) -> Result<CourseRef, PoolError> {
        if last && self.cursor == 0 && !self.entries.is_empty() {
            // the final course always comes from a fresh castle search
            self.cursor = self.entries.len();
        }
        if self.cursor >= self.entries.len() {
            self.search(src, last, rng)?;
        }
        Ok(self.entries[self.cursor].clone())
    }

    /// Clear, skip or unloadable course: move on (a death does not).
    pub fn advance(&mut self) {
        self.cursor += 1;
    }

    /// Before the last course, force the castle search.
    pub fn prepare_last(&mut self) {
        self.cursor = self.entries.len();
    }
}

/// A source over a fixed list (the local library): random picks within the band.
pub struct ListSource {
    pub courses: Vec<CourseRef>,
}

impl CourseSource for ListSource {
    fn search(
        &mut self,
        band: (u8, u8),
        max: usize,
        _castle: bool,
        rng: &mut Rng,
    ) -> Vec<CourseRef> {
        let mut c: Vec<CourseRef> = self
            .courses
            .iter()
            .filter(|c| c.difficulty >= band.0 && c.difficulty <= band.1)
            .cloned()
            .collect();
        // partial Fisher–Yates for a random subset
        let n = c.len();
        for i in 0..n.min(max) {
            let j = i + rng.below((n - i) as u32) as usize;
            c.swap(i, j);
        }
        c.truncate(max);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(n: usize) -> ListSource {
        ListSource {
            courses: (0..n)
                .map(|i| CourseRef {
                    id: format!("c{i}"),
                    name: format!("Course {i}"),
                    maker: "m".into(),
                    difficulty: (i % 101) as u8,
                })
                .collect(),
        }
    }

    #[test]
    fn shuffle_keeps_entry_zero() {
        let mut rng = Rng::new(7);
        let mut v: Vec<u32> = (0..20).collect();
        smm1_shuffle(&mut v, &mut rng);
        assert_eq!(v[0], 0);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn pool_draws_from_the_band() {
        let mut src = lib(1000);
        let mut rng = Rng::new(1);
        let mut p = Pool::new(Difficulty::Expert);
        for _ in 0..60 {
            let c = p.current(&mut src, false, &mut rng).unwrap();
            assert!((75..=95).contains(&c.difficulty));
            p.advance();
        }
    }

    #[test]
    fn not_enough_courses_on_first_search() {
        let mut src = lib(101); // one course per difficulty value: only 5 Super Expert
        let mut rng = Rng::new(3);
        let mut p = Pool::new(Difficulty::SuperExpert);
        assert_eq!(
            p.current(&mut src, false, &mut rng),
            Err(PoolError::NotEnoughCourses)
        );
    }
}
