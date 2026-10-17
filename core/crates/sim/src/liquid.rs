//! The area liquid level: water, lava or poison (research/specs/themes.md §3).
//!
//! Static, one-way or oscillating between the start and end heights. Movement: wait 60 frames,
//! accelerate toward ±vmax, brake near the target (target speed = clamp(distance × 0.05,
//! 0.1 × vmax, vmax) inside min(brake cap, ¼ of the range)), then stop (one-way) or wait 60 and
//! reverse (oscillate).

use crate::level::LiquidSpec;

const VMAX: [f32; 3] = [0.1, 0.3, 0.7];
const ACC: [f32; 3] = [0.001, 0.002, 0.01];
const BRAKE: [f32; 3] = [4.0, 8.0, 12.0];
const WAIT: i32 = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Wait(i32),
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Liquid {
    pub spec: LiquidSpec,
    /// Surface height (px).
    pub y: f32,
    state: State,
    v: f32,
    up: bool,
}

impl Liquid {
    pub fn new(spec: LiquidSpec) -> Self {
        let moving = spec.start != spec.end
            && (1..=3).contains(&spec.speed)
            && (1..=2).contains(&spec.mode)
            && !spec.full;
        Self {
            spec,
            y: spec.start,
            state: if moving {
                State::Wait(WAIT)
            } else {
                State::Idle
            },
            v: 0.0,
            up: spec.end > spec.start,
        }
    }

    pub fn deadly(&self) -> bool {
        matches!(self.spec.kind, 2 | 3)
    }

    pub fn water(&self) -> bool {
        self.spec.kind == 1
    }

    /// Is the point (px) below the surface?
    pub fn covers(&self, y: f32) -> bool {
        self.spec.full || y < self.y
    }

    pub fn step(&mut self) {
        match self.state {
            State::Idle => {}
            State::Wait(n) => {
                self.state = if n <= 1 {
                    State::Move
                } else {
                    State::Wait(n - 1)
                };
            }
            State::Move => {
                let s = (self.spec.speed as usize - 1).min(2);
                let (low, high) = (
                    self.spec.start.min(self.spec.end),
                    self.spec.start.max(self.spec.end),
                );
                let target = if self.up { high } else { low };
                let remaining = (target - self.y).abs();
                let brake = BRAKE[s].min(0.25 * (high - low));
                self.v = if remaining <= brake {
                    (remaining * 0.05).clamp(VMAX[s] * 0.1, VMAX[s])
                } else {
                    (self.v + ACC[s]).min(VMAX[s])
                };
                if remaining <= self.v {
                    self.y = target;
                    self.v = 0.0;
                    self.state = if self.spec.mode == 1 {
                        State::Idle
                    } else {
                        self.up = !self.up;
                        State::Wait(WAIT)
                    };
                } else {
                    self.y += if self.up { self.v } else { -self.v };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(start: f32, end: f32, mode: u8, speed: u8) -> LiquidSpec {
        LiquidSpec {
            kind: 2,
            start,
            end,
            mode,
            speed,
            full: false,
        }
    }

    #[test]
    fn static_when_start_equals_end() {
        let mut l = Liquid::new(spec(44.0, 44.0, 2, 2));
        for _ in 0..200 {
            l.step();
        }
        assert_eq!(l.y, 44.0);
    }

    #[test]
    fn one_way_rises_and_stops() {
        let mut l = Liquid::new(spec(12.0, 108.0, 1, 3));
        for _ in 0..60 {
            l.step();
        }
        assert_eq!(l.y, 12.0, "waits 60 frames");
        for _ in 0..1000 {
            l.step();
        }
        assert_eq!(l.y, 108.0);
    }

    #[test]
    fn oscillates() {
        let mut l = Liquid::new(spec(12.0, 60.0, 2, 3));
        let mut saw_top = false;
        let mut back = false;
        for _ in 0..2000 {
            l.step();
            saw_top |= l.y == 60.0;
            back |= saw_top && l.y == 12.0;
        }
        assert!(saw_top && back);
    }
}
