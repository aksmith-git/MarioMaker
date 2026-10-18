//! Replays: a run as the inputs of every frame since the course started. The sim is
//! deterministic, so a replay played on the same course reproduces the run exactly: the server
//! re-simulates submitted clears to verify record times, and players can race a record holder's
//! run as a ghost.
//!
//! Encoding (`MMKR`, version 1): frame count (u32 LE), then runs of identical inputs as
//! (frames, buttons) LEB128 pairs. A five-minute run is typically a few hundred bytes.
//!
//! A replay stays valid only as long as the sim behaves the same, so it records the
//! [`SIM_VERSION`] it was made with; bump that when a change alters how courses play.

use mmk_formats::smm2::Course;
use mmk_sim::{Input, World};

const MAGIC: &[u8; 4] = b"MMKR";
const FORMAT: u8 = 1;
/// Bumped whenever a sim change can alter a recorded run's outcome.
pub const SIM_VERSION: u32 = 1;
/// Longest replay accepted (10 minutes at 60 frames per second; course timers stop at 500 s).
pub const MAX_FRAMES: usize = 60 * 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    pub sim_version: u32,
    /// One input (button bits) per frame, from the first frame of the course.
    pub inputs: Vec<u32>,
}

/// What playing a replay did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Frames played (all of the replay, or up to the clear).
    pub frames: usize,
    /// The frame the course was cleared on (1-based count of frames played), if it was.
    pub cleared_at: Option<usize>,
}

impl Outcome {
    /// The clear time in milliseconds (60 frames per second).
    pub fn time_ms(&self) -> Option<u64> {
        self.cleared_at.map(|f| (f as u64 * 1000).div_ceil(60))
    }
}

fn put_leb(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_leb(b: &[u8], at: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for shift in (0..35).step_by(7) {
        let x = *b.get(*at)?;
        *at += 1;
        v |= ((x & 0x7F) as u32).checked_shl(shift)?;
        if x & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

impl Default for Replay {
    /// An empty replay for this engine version.
    fn default() -> Self {
        Self::new()
    }
}

impl Replay {
    pub fn new() -> Self {
        Self {
            sim_version: SIM_VERSION,
            inputs: Vec::new(),
        }
    }

    pub fn push(&mut self, input: Input) {
        self.inputs.push(input.buttons);
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(FORMAT);
        out.extend(self.sim_version.to_le_bytes());
        out.extend((self.inputs.len() as u32).to_le_bytes());
        let mut i = 0;
        while i < self.inputs.len() {
            let b = self.inputs[i];
            let n = self.inputs[i..].iter().take_while(|&&x| x == b).count();
            put_leb(&mut out, n as u32);
            put_leb(&mut out, b);
            i += n;
        }
        out
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.get(..4)? != MAGIC || *b.get(4)? != FORMAT {
            return None;
        }
        let sim_version = u32::from_le_bytes(b.get(5..9)?.try_into().ok()?);
        let frames = u32::from_le_bytes(b.get(9..13)?.try_into().ok()?) as usize;
        if frames > MAX_FRAMES {
            return None;
        }
        let mut inputs = Vec::with_capacity(frames);
        let mut at = 13;
        while inputs.len() < frames {
            let n = get_leb(b, &mut at)? as usize;
            let buttons = get_leb(b, &mut at)?;
            if n == 0 || inputs.len() + n > frames {
                return None;
            }
            inputs.extend(std::iter::repeat_n(buttons, n));
        }
        (at == b.len()).then_some(Self {
            sim_version,
            inputs,
        })
    }

    /// Play the replay on `course` from its start (one player), stopping at the clear. The
    /// clear time is the goal touch (when the goal sequence starts), as the game's clear screen
    /// and records count it; the replay must still play through to the finished clear.
    pub fn run(&self, course: &Course) -> Outcome {
        let mut w = World::from_course(course);
        let mut touched = None;
        for (k, &b) in self.inputs.iter().enumerate() {
            w.step(Input { buttons: b });
            if touched.is_none() && w.goal_phase != mmk_sim::world::GoalPhase::None {
                touched = Some(k + 1);
            }
            if w.cleared {
                return Outcome {
                    frames: k + 1,
                    cleared_at: Some(touched.unwrap_or(k + 1)),
                };
            }
        }
        Outcome {
            frames: self.inputs.len(),
            cleared_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_sim::input;

    #[test]
    fn encoding_round_trips_and_is_small() {
        let mut r = Replay::new();
        for f in 0..3000 {
            r.push(Input {
                buttons: if f % 200 < 150 {
                    input::RIGHT | input::RUN
                } else {
                    input::RIGHT | input::JUMP
                },
            });
        }
        let b = r.encode();
        assert!(b.len() < 120, "{} bytes", b.len());
        assert_eq!(Replay::decode(&b), Some(r));
        assert_eq!(Replay::decode(&b[..b.len() - 1]), None);
        let mut long = b.clone();
        long.push(0);
        assert_eq!(Replay::decode(&long), None, "trailing bytes");
    }

    #[test]
    fn replaying_the_demo_clears_it_at_the_same_frame() {
        // a blank course with ground all the way to the goal
        let mut ed = mmk_editor::Editor::new("M1");
        let (width, _) = ed.size();
        for tx in 0..width {
            for ty in 0..1 {
                let _ = ed.paint_ground(tx, ty);
            }
        }
        let course = Course::parse(&ed.save().unwrap()).unwrap();
        let mut w = World::from_course(&course);
        let mut r = Replay::new();
        let mut cleared = None;
        let mut touch = None;
        for f in 0..MAX_FRAMES {
            let i = Input {
                buttons: if f % 90 < 70 {
                    input::RIGHT | input::RUN
                } else {
                    input::RIGHT | input::RUN | input::JUMP
                },
            };
            r.push(i);
            w.step(i);
            if touch.is_none() && w.goal_phase != mmk_sim::world::GoalPhase::None {
                touch = Some(f + 1);
            }
            if w.cleared {
                cleared = Some(f + 1);
                break;
            }
        }
        assert!(cleared.is_some(), "the scripted run should reach the goal");
        let back = Replay::decode(&r.encode()).unwrap();
        let out = back.run(&course);
        assert_eq!(out.cleared_at, touch, "timed at the goal touch");
        assert!(touch < cleared, "the goal sequence runs after the touch");
        // one frame short doesn't clear
        let mut short = back.clone();
        short.inputs.pop();
        assert_eq!(short.run(&course).cleared_at, None);
    }
}
