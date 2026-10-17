//! Player movement, SMB1 style. Constants and rules from research/specs/player-physics.md
//! (behavioural spec); values are px and px/frame at 60 Hz.

use crate::input::{self, Input};

pub const WALK_MAX: f32 = 1.5;
pub const RUN_MAX: f32 = 3.0;
pub const AIR_CAP: f32 = 3.0;
pub const FALL_CAP: f32 = -4.0;

/// Speed bands used to pick an acceleration: |vx| < 0.5, < 1.5, < 2.25, < 3.0.
const BANDS: [f32; 4] = [0.5, 1.5, 2.25, 3.0];
const GROUND_ACCEL_WALK: [f32; 4] = [0.1, 0.03, 0.03, 0.035];
const GROUND_ACCEL_RUN: [f32; 4] = [0.1, 0.06, 0.029, 0.035];
const OVER_CAP_DECEL: f32 = 0.75;
const FRICTION: [f32; 4] = [0.05, 0.05, 0.035, 0.035];
const TURN_DECEL: f32 = 0.1;
const SKID_SPEED: f32 = 2.5;
const SKID_DECEL: f32 = 0.12;
const AIR_ACCEL: [f32; 4] = [0.08, 0.03, 0.029, 0.021];
const AIR_ACCEL_RUN_BAND1: f32 = 0.06;
const AIR_TURN: f32 = 0.09;

const JUMP_BASE: f32 = 3.628;
/// Launch bonus by |vx|: < 0.7, < 1.5, < 2.8, otherwise.
const JUMP_BONUS: [(f32, f32); 4] = [(0.7, 0.0), (1.5, 0.18), (2.8, 0.24), (f32::INFINITY, 0.3)];

fn band(speed: f32) -> usize {
    BANDS.iter().position(|&b| speed < b).unwrap_or(3)
}

/// Gravity while jump is held, by vertical speed (rising fast → floaty, apex hang, then fall).
fn gravity_held(vy: f32) -> f32 {
    if vy > 2.5 {
        -0.06
    } else if vy > 1.5 {
        -0.25
    } else if vy > 0.3 {
        -0.34
    } else if vy > -0.12 {
        -0.08
    } else if vy > -3.0 {
        -0.31
    } else {
        -0.34
    }
}

/// Gravity with jump released: -0.34, softened to -0.25 near the top of the arc.
fn gravity_released(vy: f32) -> f32 {
    if vy.abs() < 0.3 { -0.25 } else { -0.34 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Wait,
    Walk,
    Skid,
    Jump,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// +1 right, -1 left
    pub facing: i32,
    pub on_ground: bool,
    pub jumping: bool,
    pub pose: Pose,
    /// Accumulated walk-cycle distance (px) for animation.
    pub walk_phase: f32,
    prev_jump: bool,
}

impl Player {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            facing: 1,
            on_ground: true,
            jumping: false,
            pose: Pose::Wait,
            walk_phase: 0.0,
            prev_jump: false,
        }
    }

    fn horizontal_ground(&mut self, inp: Input) {
        let dir = inp.dir_x() as f32;
        let speed = self.vx.abs();
        let moving = self.vx.signum();
        let max = if inp.held(input::RUN) {
            RUN_MAX
        } else {
            WALK_MAX
        };
        if dir == 0.0 {
            // no input: friction towards 0
            let f = FRICTION[band(speed)];
            self.vx = if speed <= f {
                0.0
            } else {
                self.vx - moving * f
            };
            self.pose = if self.vx == 0.0 {
                Pose::Wait
            } else {
                Pose::Walk
            };
        } else if self.vx != 0.0 && dir != moving {
            // reversing
            let d = if speed >= SKID_SPEED {
                SKID_DECEL
            } else {
                TURN_DECEL
            };
            self.vx += dir * d;
            self.pose = Pose::Skid;
            if self.vx.signum() == dir {
                self.facing = dir as i32;
            }
        } else {
            self.facing = dir as i32;
            if speed > max {
                self.vx = moving * (speed - OVER_CAP_DECEL).max(max);
            } else {
                let table = if inp.held(input::RUN) {
                    &GROUND_ACCEL_RUN
                } else {
                    &GROUND_ACCEL_WALK
                };
                self.vx = dir * (speed + table[band(speed)]).min(max);
            }
            self.pose = Pose::Walk;
        }
    }

    fn horizontal_air(&mut self, inp: Input) {
        let dir = inp.dir_x() as f32;
        if dir == 0.0 {
            return; // momentum kept, no drag
        }
        let speed = self.vx.abs();
        let a = if self.vx != 0.0 && dir != self.vx.signum() {
            AIR_TURN
        } else if inp.held(input::RUN) && band(speed) == 1 {
            AIR_ACCEL_RUN_BAND1
        } else {
            AIR_ACCEL[band(speed)]
        };
        let max = if inp.held(input::RUN) {
            RUN_MAX
        } else {
            WALK_MAX
        };
        let target = (self.vx + dir * a).clamp(-AIR_CAP, AIR_CAP);
        // accelerate up to the current max; never steal speed that is already above it
        self.vx = if target.abs() > max && target.abs() > speed {
            self.vx
        } else {
            target
        };
    }

    /// One tick against a flat floor at `floor_y` (feet position).
    pub fn step(&mut self, inp: Input, floor_y: f32) {
        let jump = inp.held(input::JUMP);
        let pressed = jump && !self.prev_jump;
        self.prev_jump = jump;

        if self.on_ground {
            self.horizontal_ground(inp);
            if pressed {
                let speed = self.vx.abs();
                let bonus = JUMP_BONUS
                    .iter()
                    .find(|(limit, _)| speed < *limit)
                    .map(|b| b.1)
                    .unwrap_or(0.0);
                self.vy = JUMP_BASE + bonus;
                self.on_ground = false;
                self.jumping = true;
            }
        } else {
            self.horizontal_air(inp);
        }

        if !self.on_ground {
            let g = if jump && self.jumping {
                gravity_held(self.vy)
            } else {
                gravity_released(self.vy)
            };
            self.vy = (self.vy + g).max(FALL_CAP);
            self.pose = Pose::Jump;
        }

        self.x += self.vx;
        self.y += self.vy;
        if self.y <= floor_y {
            self.y = floor_y;
            self.vy = 0.0;
            if !self.on_ground {
                self.on_ground = true;
                self.jumping = false;
                self.pose = if self.vx == 0.0 {
                    Pose::Wait
                } else {
                    Pose::Walk
                };
            }
        }
        if self.pose == Pose::Walk {
            self.walk_phase += self.vx.abs();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jump_peak(run_frames: usize, run: bool) -> f32 {
        let mut p = Player::new(0.0, 0.0);
        let held = |b| Input { buttons: b };
        let mv = input::RIGHT | if run { input::RUN } else { 0 };
        for _ in 0..run_frames {
            p.step(held(mv), 0.0);
        }
        let mut peak: f32 = 0.0;
        for _ in 0..200 {
            p.step(held(mv | input::JUMP), 0.0);
            peak = peak.max(p.y);
        }
        peak
    }

    /// Spec check (player-physics.md): held jump peaks ≈ 67.6 px standing, ≈ 86.3 px at full run.
    #[test]
    fn jump_heights_match_spec() {
        let standing = jump_peak(0, false);
        assert!((standing - 67.6).abs() < 0.5, "standing peak {standing}");
        let running = jump_peak(200, true);
        assert!((running - 86.3).abs() < 0.5, "running peak {running}");
    }

    #[test]
    fn walk_and_run_caps() {
        let mut p = Player::new(0.0, 0.0);
        for _ in 0..300 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                0.0,
            );
        }
        assert_eq!(p.vx, WALK_MAX);
        for _ in 0..300 {
            p.step(
                Input {
                    buttons: input::RIGHT | input::RUN,
                },
                0.0,
            );
        }
        assert_eq!(p.vx, RUN_MAX);
        for _ in 0..300 {
            p.step(Input::default(), 0.0);
        }
        assert_eq!(p.vx, 0.0);
    }

    #[test]
    fn deterministic() {
        let script: Vec<u32> = (0..600)
            .map(|i| {
                if i % 97 < 50 {
                    input::RIGHT | input::RUN
                } else {
                    input::JUMP | input::LEFT
                }
            })
            .collect();
        let run = || {
            let mut p = Player::new(0.0, 0.0);
            for &b in &script {
                p.step(Input { buttons: b }, 0.0);
            }
            (p.x.to_bits(), p.y.to_bits(), p.vx.to_bits(), p.vy.to_bits())
        };
        assert_eq!(run(), run());
    }
}
