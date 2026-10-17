//! Player movement and terrain collision, SMB1 style.
//!
//! Implemented from research/specs/player-physics.md (behavioural spec): §1.3 integration, §2.2
//! ground, §2.3 skid, §2.4 air, §3.1 jump launch, §3.2 gravity, §6.1 terrain sensors.
//! Units: px, px/frame, 60 Hz, y up; position = feet centre.

use crate::input::{self, Input};
use crate::level::{Cell, Level, Slope, TILE};

/// Speed thresholds T0..T3 (§2.1).
const T: [f32; 4] = [0.5, 1.5, 2.25, 3.0];
pub const WALK_MAX: f32 = T[1];
pub const RUN_MAX: f32 = T[3];
pub const FALL_CAP: f32 = -4.0;

// Acceleration tables, 6 bands: b0 = reversing, b1..b4 = |v| ≤ T0..T3 in the facing direction,
// b5 = above T3 (§2.2, §2.4).
const GROUND_WALK: [f32; 6] = [0.1, 0.1, 0.03, 0.03, 0.035, 0.75];
const GROUND_RUN: [f32; 6] = [0.1, 0.1, 0.06, 0.029, 0.035, 0.75];
const GROUND_IDLE: [f32; 6] = [0.07, 0.05, 0.05, 0.035, 0.035, 0.75];
const AIR_DIR: [f32; 6] = [0.09, 0.08, 0.03, 0.029, 0.021, 0.021];
const AIR_DIR_RUN: [f32; 6] = [0.09, 0.08, 0.06, 0.029, 0.021, 0.021];

// Slopes, by steepness index 0..4 (§5.2)
const SLOPE_TARGET_DOWN: [f32; 5] = [1.0, 1.25, 1.25, 1.44, 1.62];
const SLOPE_TARGET_UP: [f32; 5] = [1.0, 0.6875, 0.6875, 0.5, 0.3];
const SLOPE_ACCEL_DOWN: [f32; 5] = [0.033, 0.044, 0.055, 0.077, 0.187];
const SLOPE_ACCEL_UP: [f32; 5] = [0.027, 0.027, 0.04, 0.108, 0.153];
const SLOPE_DECEL_DOWN: [f32; 5] = [0.035, 0.0315, 0.028, 0.021, 0.014];
const SLOPE_DECEL_UP: [f32; 5] = [0.035, 0.046, 0.06, 0.123, 0.175];
/// How far the feet may snap up/down to follow a slope surface while grounded (px). [inf]
const SLOPE_SNAP: f32 = 6.0;

const SKID_ENTER: f32 = 2.5;
const SKID_DECEL: f32 = 0.12;
const SKID_REACCEL: f32 = 0.25;

const JUMP_BASE: f32 = 3.628;
const GRAVITY_GROUND: f32 = -0.34;
// vy band boundaries and gravities (§3.2)
const VY_BANDS: [f32; 5] = [2.5, 1.5, 0.3, -0.12, -3.0];
const GRAVITY_HELD: [f32; 6] = [-0.06, -0.25, -0.34, -0.08, -0.31, -0.34];
const GRAVITY_RELEASED: [f32; 6] = [-0.34, -0.34, -0.34, -0.25, -0.34, -0.34];

/// Terrain sensors, small Mario in M1/M3 (§6.1): feet ±4.9 at y 0, head ±2 at y 12, walls ±7.9
/// for y 4…8.5.
pub struct Sensors {
    pub foot_x: f32,
    pub head_x: f32,
    pub head_y: f32,
    pub wall_y0: f32,
    pub wall_y1: f32,
    pub wall_x: f32,
}
pub const SMALL_M1: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 12.0,
    wall_y0: 4.0,
    wall_y1: 8.5,
    wall_x: 7.9,
};

fn jump_bonus(speed: f32) -> f32 {
    if speed < 0.7 {
        0.0
    } else if speed < 1.5 {
        0.18
    } else if speed < 2.8 {
        0.24
    } else {
        0.3
    }
}

fn vy_band(vy: f32) -> usize {
    VY_BANDS.iter().position(|&b| vy > b).unwrap_or(5)
}

/// Band for an acceleration table given speed along x and the facing direction.
fn band(speed_f: f32, facing: i32) -> usize {
    let s = speed_f.abs();
    if speed_f != 0.0 && speed_f.signum() as i32 != facing {
        return if s > T[3] { 5 } else { 0 };
    }
    T.iter().position(|&t| s <= t).map(|i| i + 1).unwrap_or(5)
}

/// Move `v` toward `target` by at most `a` (§1.3 step 4).
fn approach(v: f32, target: f32, a: f32) -> f32 {
    if v < target {
        (v + a).min(target)
    } else {
        (v - a).max(target)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Wait,
    Walk,
    Skid,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkidPhase {
    None,
    Braking,
    Reaccel,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// +1 right, −1 left
    pub facing: i32,
    pub on_ground: bool,
    pub pose: Pose,
    /// Distance walked (px), drives the walk animation.
    pub walk_phase: f32,
    pub dead: bool,
    /// Slope under the feet while grounded: (steepness index, +1 if the surface rises to the
    /// right / −1 if it falls).
    pub slope: Option<(usize, i32)>,
    skid: SkidPhase,
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
            pose: Pose::Wait,
            walk_phase: 0.0,
            dead: false,
            slope: None,
            skid: SkidPhase::None,
            prev_jump: false,
        }
    }

    fn ground(&mut self, inp: Input) {
        let dir = inp.dir_x();
        let run = inp.held(input::RUN);

        // skid (§2.3): reversing at |v| ≥ 2.5 brakes at 0.12, then re-accelerates at 0.25
        if self.skid == SkidPhase::None
            && dir != 0
            && self.vx != 0.0
            && dir != self.vx.signum() as i32
            && self.vx.abs() >= SKID_ENTER
        {
            self.skid = SkidPhase::Braking;
        }
        match self.skid {
            SkidPhase::Braking => {
                self.facing = if dir != 0 { dir } else { self.facing };
                self.vx = approach(self.vx, 0.0, SKID_DECEL);
                self.pose = Pose::Skid;
                if self.vx == 0.0 {
                    self.skid = if dir != 0 {
                        SkidPhase::Reaccel
                    } else {
                        SkidPhase::None
                    };
                }
                return;
            }
            SkidPhase::Reaccel => {
                // [?] end of the second phase is not in the spec: it ends when the stick lets go
                // or walk speed is reached in the new direction
                let target = if dir == 0 {
                    0.0
                } else {
                    dir as f32 * if run { RUN_MAX } else { WALK_MAX }
                };
                self.vx = approach(self.vx, target, SKID_REACCEL);
                if dir == 0 || self.vx.abs() >= WALK_MAX {
                    self.skid = SkidPhase::None;
                }
                self.pose = Pose::Walk;
                return;
            }
            SkidPhase::None => {}
        }

        if let Some((steep, rises)) = self.slope {
            // §5.2: rows chosen by whether the motion is downhill or uphill
            let moving = if dir != 0 {
                dir
            } else {
                self.vx.signum() as i32
            };
            let downhill = moving != 0 && moving != rises;
            let (mult, acc, dec) = if downhill {
                (
                    SLOPE_TARGET_DOWN[steep],
                    SLOPE_ACCEL_DOWN[steep],
                    SLOPE_DECEL_DOWN[steep],
                )
            } else {
                (
                    SLOPE_TARGET_UP[steep],
                    SLOPE_ACCEL_UP[steep],
                    SLOPE_DECEL_UP[steep],
                )
            };
            if dir == 0 {
                self.vx = approach(self.vx, 0.0, dec);
            } else {
                self.facing = dir;
                let target = dir as f32 * if run { RUN_MAX } else { WALK_MAX } * mult;
                let reversing = self.vx != 0.0 && self.vx.signum() as i32 != dir;
                let a = if reversing {
                    // reversing: max(1.1 × the other row's no-input value, the input accel)
                    let other = if downhill {
                        SLOPE_DECEL_UP[steep]
                    } else {
                        SLOPE_DECEL_DOWN[steep]
                    };
                    acc.max(1.1 * other)
                } else {
                    acc
                };
                self.vx = approach(self.vx, target, a);
            }
            self.pose = if self.vx == 0.0 && dir == 0 {
                Pose::Wait
            } else {
                Pose::Walk
            };
            return;
        }

        let (target, table) = if dir == 0 {
            (0.0, &GROUND_IDLE)
        } else {
            self.facing = dir;
            (
                dir as f32 * if run { RUN_MAX } else { WALK_MAX },
                if run { &GROUND_RUN } else { &GROUND_WALK },
            )
        };
        self.vx = approach(self.vx, target, table[band(self.vx, self.facing)]);
        self.pose = if self.vx == 0.0 && dir == 0 {
            Pose::Wait
        } else {
            Pose::Walk
        };
    }

    fn air(&mut self, inp: Input) {
        let dir = inp.dir_x();
        if dir == 0 {
            // no direction: accel 0 → momentum preserved
        } else {
            let run = inp.held(input::RUN);
            let s = self.vx.abs();
            let target = if run {
                RUN_MAX
            } else if s > WALK_MAX && self.vx.signum() as i32 == dir {
                s
            } else {
                WALK_MAX
            } * dir as f32;
            // SMB1 keeps the take-off facing in the air (§2.6); bands use the movement facing
            let table = if run { &AIR_DIR_RUN } else { &AIR_DIR };
            let face = if self.vx == 0.0 {
                dir
            } else {
                self.vx.signum() as i32
            };
            let b = if dir != face { 0 } else { band(self.vx, face) };
            self.vx = approach(self.vx, target, table[b]);
        }
        self.vx = self.vx.clamp(-RUN_MAX, RUN_MAX); // hard air cap
    }

    /// One 60 Hz tick.
    pub fn step(&mut self, inp: Input, level: &Level) {
        if self.dead {
            return;
        }
        let jump = inp.held(input::JUMP);
        let pressed = jump && !self.prev_jump;
        self.prev_jump = jump;

        if self.on_ground {
            self.ground(inp);
            if pressed {
                self.vy = JUMP_BASE + jump_bonus(self.vx.abs());
                self.on_ground = false;
                self.skid = SkidPhase::None;
            }
        } else {
            self.air(inp);
        }

        // gravity (§3.2): ground uses the fixed −0.34; airborne picks a band each frame
        let g = if self.on_ground {
            GRAVITY_GROUND
        } else if jump {
            GRAVITY_HELD[vy_band(self.vy)]
        } else {
            GRAVITY_RELEASED[vy_band(self.vy)]
        };
        self.vy = (self.vy + g).max(FALL_CAP);

        self.move_and_collide(level, &SMALL_M1);

        if !self.on_ground {
            self.pose = Pose::Jump;
        } else if self.pose == Pose::Walk {
            self.walk_phase += self.vx.abs();
        }
        if self.y < -3.0 * TILE {
            self.dead = true; // fell into a pit
        }
    }

    fn solid(level: &Level, px: f32, py: f32) -> bool {
        level.cell_at(px, py) == Cell::Solid
    }

    /// Axis-separated movement against the tile grid using the spec's sensor points.
    fn move_and_collide(&mut self, level: &Level, s: &Sensors) {
        // horizontal: on a slope, ground speed is along the surface (x scaled by |cos θ|, §1.3)
        let cos = match self.slope {
            Some((steep, _)) if self.on_ground => {
                if steep >= 3 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    0.894_427_2
                }
            }
            _ => 1.0,
        };
        let was_on_ground = self.on_ground;
        self.x += self.vx * cos;
        for dir in [-1.0f32, 1.0] {
            let wx = self.x + dir * s.wall_x;
            let hit = [s.wall_y0, s.wall_y1]
                .iter()
                .any(|&wy| Self::solid(level, wx, self.y + wy));
            if hit {
                let tile_edge = if dir > 0.0 {
                    (wx / TILE).floor() * TILE
                } else {
                    ((wx / TILE).floor() + 1.0) * TILE
                };
                self.x = tile_edge - dir * (s.wall_x + 0.001);
                if self.vx * dir > 0.0 {
                    self.vx = 0.0;
                }
            }
        }
        self.x = self.x.clamp(level.left + s.wall_x, level.right - s.wall_x);

        // vertical
        let prev_y = self.y;
        self.y += self.vy;
        if self.vy > 0.0 {
            let hy = self.y + s.head_y;
            if [-s.head_x, s.head_x]
                .iter()
                .any(|&hx| Self::solid(level, self.x + hx, hy))
            {
                self.y = (hy / TILE).floor() * TILE - s.head_y - 0.001;
                self.vy = 0.0;
            }
            self.on_ground = false;
            self.slope = None;
        } else {
            // feet: land on solid cells, or on one-way tops we were above last frame
            let mut landed = None;
            for fx in [-s.foot_x, s.foot_x] {
                let (px, py) = (self.x + fx, self.y);
                let top = ((py / TILE).floor() + 1.0) * TILE;
                let cell = level.cell_at(px, py);
                let ok = cell == Cell::Solid || (cell == Cell::SemiTop && prev_y >= top - 0.01);
                if ok {
                    landed = Some(landed.map_or(top, |t: f32| t.max(top)));
                }
            }
            // slope surfaces under either foot (and the centre, for the slope state)
            let mut on_slope: Option<&Slope> = None;
            for fx in [-s.foot_x, 0.0, s.foot_x] {
                if let Some((surf, slope)) = level.slope_at(self.x + fx) {
                    let reach = if was_on_ground { SLOPE_SNAP } else { 0.0 };
                    let can_land = self.y <= surf + reach && prev_y >= surf - SLOPE_SNAP;
                    if can_land && landed.is_none_or(|t| surf >= t) {
                        landed = Some(surf);
                        if fx == 0.0 || on_slope.is_none() {
                            on_slope = Some(slope);
                        }
                    }
                }
            }
            match landed {
                Some(top) => {
                    self.y = top;
                    self.vy = 0.0;
                    self.on_ground = true;
                    self.slope =
                        on_slope.map(|sl| (sl.steepness(), if sl.rising { 1 } else { -1 }));
                }
                None => {
                    self.on_ground = false;
                    self.slope = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(run_frames: usize, run: bool) -> f32 {
        let level = Level::flat(400);
        let mut p = Player::new(56.0, 32.0);
        let mv = input::RIGHT | if run { input::RUN } else { 0 };
        for _ in 0..run_frames {
            p.step(Input { buttons: mv }, &level);
        }
        let base = p.y;
        let mut top: f32 = 0.0;
        for _ in 0..120 {
            p.step(
                Input {
                    buttons: mv | input::JUMP,
                },
                &level,
            );
            top = top.max(p.y - base);
        }
        top
    }

    /// Spec (player-physics.md §0): held jump peaks ≈ 67.6 px standing, ≈ 86.3 px at full run.
    #[test]
    fn jump_heights_match_spec() {
        let standing = peak(0, false);
        assert!((standing - 67.6).abs() < 0.6, "standing peak {standing}");
        let running = peak(150, true);
        assert!((running - 86.3).abs() < 0.6, "running peak {running}");
    }

    #[test]
    fn walk_run_caps_and_friction() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..120 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
        }
        assert_eq!(p.vx, WALK_MAX);
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | input::RUN,
                },
                &level,
            );
        }
        assert_eq!(p.vx, RUN_MAX);
        for _ in 0..200 {
            p.step(Input::default(), &level);
        }
        assert_eq!(p.vx, 0.0);
    }

    #[test]
    fn skid_brakes_at_0_12() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | input::RUN,
                },
                &level,
            );
        }
        p.step(
            Input {
                buttons: input::LEFT | input::RUN,
            },
            &level,
        );
        assert_eq!(p.pose, Pose::Skid);
        assert!((p.vx - (RUN_MAX - SKID_DECEL)).abs() < 1e-6);
    }

    #[test]
    fn stands_on_ground_and_falls_into_pit() {
        let level = Level::flat(10); // ground ends at x = 160 px
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..10 {
            p.step(Input::default(), &level);
        }
        assert!(p.on_ground && p.y == 32.0);
        for _ in 0..400 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
        }
        assert!(p.x <= level.right - SMALL_M1.wall_x + 0.01);
    }

    #[test]
    fn walks_up_and_down_a_slope() {
        // flat ground with a gentle "/" slope from tile 10 (2 wide, 1 high) onto a raised plateau
        let mut level = Level::flat(60);
        level.add_test_slope(
            Slope {
                left: 10,
                bottom: 2,
                width: 4,
                height: 2,
                rising: true,
                steep: false,
            },
            14,
            4,
        );
        let mut p = Player::new(56.0, 32.0);
        let mut max_y: f32 = 0.0;
        for _ in 0..400 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
            max_y = max_y.max(p.y);
            assert!(!p.dead);
        }
        assert!(
            p.x > 15.0 * TILE,
            "should have walked past the slope, x = {}",
            p.x
        );
        assert!(
            (max_y - 4.0 * TILE).abs() < 0.01,
            "should end up on the plateau, y = {max_y}"
        );
        assert!(p.on_ground);
    }

    #[test]
    fn slope_speed_rules() {
        // walking (not running) down a steep slope targets 1.5 × 1.44; uphill 1.5 × 0.5
        let mut level = Level::flat(80);
        level.add_test_slope(
            Slope {
                left: 10,
                bottom: 2,
                width: 9,
                height: 8,
                rising: false,
                steep: true,
            },
            99,
            0,
        );
        let mut p = Player::new(10.0 * TILE + 8.0, 10.0 * TILE);
        let mut fastest: f32 = 0.0;
        for _ in 0..90 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
            if p.slope.is_some() {
                fastest = fastest.max(p.vx);
            }
        }
        assert!(
            fastest > WALK_MAX + 0.1,
            "downhill should exceed walk speed, got {fastest}"
        );
        assert!(
            fastest <= WALK_MAX * 1.44 + 1e-4,
            "downhill cap is 1.5 x 1.44, got {fastest}"
        );
    }

    #[test]
    fn deterministic() {
        let level = Level::flat(400);
        let script: Vec<u32> = (0..900)
            .map(|i| {
                if i % 97 < 50 {
                    input::RIGHT | input::RUN
                } else {
                    input::JUMP | input::LEFT
                }
            })
            .collect();
        let run = || {
            let mut p = Player::new(56.0, 32.0);
            for &b in &script {
                p.step(Input { buttons: b }, &level);
            }
            (p.x.to_bits(), p.y.to_bits(), p.vx.to_bits(), p.vy.to_bits())
        };
        assert_eq!(run(), run());
    }
}
