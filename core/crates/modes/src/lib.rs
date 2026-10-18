//! Game modes around the course engine.
//!
//! - [`hundred`]: the 100 Mario Challenge run manager (and 10 Mario), written from the private
//!   behaviour specs `research/specs/100-mario-challenge.md` and `smm1-100mario-code.md`: lives,
//!   banked extra lives, skipping, results, run completion.
//! - [`pool`]: picking courses for a run by difficulty band from a pluggable course source.
//! - [`save`]: Continue records per difficulty, unlocks and statistics (JSON).
//! - [`replay`]: runs as per-frame inputs, re-simulated to verify clear times and drive ghosts.
//! - [`challenge`]: one 100 Mario session over a local course library, tying the three together
//!   for the app (the FFI is a thin wrapper over it).
//! - [`endless`]: SMM2's Endless Challenge (`research/specs/endless-challenge.md`): lives, pending
//!   1-Ups, skip, score, the run record our server keeps, the local course-pick rule and an
//!   offline session.
//! - [`coursebot`]: the Coursebot's 120 My Courses + 60 Downloaded slots (slot table, metadata,
//!   Downloaded stats), the course-number space and the scene-change reasons of its screens;
//!   [`coursebot::CoursebotStore`] keeps the files over a pluggable blob store.
//! - [`dojo`]: Yamamura's Dojo lesson list (from the owner's imported `lessons/index.json`),
//!   lesson starts and the saved completion marks / Maker Basics / Nina flags.
//!
//! Deterministic: randomness comes from a caller-seeded generator so runs can be replayed.

pub mod challenge;
pub mod coursebot;
pub mod dojo;
pub mod endless;
pub mod hundred;
pub mod pool;
pub mod replay;
pub mod save;

/// Small deterministic generator (xorshift64*), seeded by the caller.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u32
    }
    /// Uniform in [0, n).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            ((self.next_u32() as u64 * n as u64) >> 32) as u32
        }
    }
}
