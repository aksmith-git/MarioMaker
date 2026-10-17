//! Player movement and terrain collision, SMB1 style.
//!
//! Implemented from research/specs/player-physics.md (behavioural spec): §1.3 integration, §2.2
//! ground, §2.3 skid, §2.4 air, §3.1 jump launch, §3.2 gravity, §6.1 terrain sensors.
//! Units: px, px/frame, 60 Hz, y up; position = feet centre.

use crate::input::{self, Input};
use crate::level::{Cell, Level, Slope, TILE};

/// Speed thresholds T0..T3 (§2.1); row 1 while the star runs.
const T: [f32; 4] = [0.5, 1.5, 2.25, 3.0];
const T_STAR: [f32; 4] = [0.5, 2.0, 3.25, 4.0];
pub const WALK_MAX: f32 = T[1];
pub const RUN_MAX: f32 = T[3];
pub const FALL_CAP: f32 = -4.0;

// Acceleration tables, 6 bands: b0 = reversing, b1..b4 = |v| ≤ T0..T3 in the facing direction,
// b5 = above T3 (§2.2, §2.4).
const GROUND_WALK: [f32; 6] = [0.1, 0.1, 0.03, 0.03, 0.035, 0.75];
const GROUND_RUN: [f32; 6] = [0.1, 0.1, 0.06, 0.029, 0.035, 0.75];
const GROUND_IDLE: [f32; 6] = [0.07, 0.05, 0.05, 0.035, 0.035, 0.75];
const GROUND_CROUCH: [f32; 6] = [0.07, 0.06, 0.06, 0.05, 0.05, 0.75];
const GROUND_WALK_STAR: [f32; 6] = [0.13, 0.13, 0.04, 0.04, 0.035, 0.75];
const GROUND_RUN_STAR: [f32; 6] = [0.13, 0.13, 0.078, 0.039, 0.035, 0.75];
const ICE_WALK: [f32; 6] = [0.02, 0.02, 0.04, 0.04, 0.013, 0.024];
const ICE_WALK_STAR: [f32; 6] = [0.027, 0.06, 0.04, 0.04, 0.013, 0.024];
const ICE_RUN: [f32; 6] = [0.02, 0.02, 0.06, 0.028, 0.013, 0.024];
const ICE_RUN_STAR: [f32; 6] = [0.027, 0.06, 0.078, 0.039, 0.013, 0.024];
const ICE_IDLE: [f32; 6] = [0.018, 0.015, 0.015, 0.013, 0.013, 0.024];
const ICE_CROUCH: [f32; 6] = [0.018, 0.018, 0.018, 0.015, 0.015, 0.024];
const AIR_DIR: [f32; 6] = [0.09, 0.08, 0.03, 0.029, 0.021, 0.021];
const AIR_DIR_RUN: [f32; 6] = [0.09, 0.08, 0.06, 0.029, 0.021, 0.021];
const AIR_DIR_STAR: [f32; 6] = [0.12, 0.11, 0.04, 0.039, 0.027, 0.027];
const AIR_DIR_RUN_STAR: [f32; 6] = [0.12, 0.11, 0.078, 0.039, 0.027, 0.027];

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
const SKID_DECEL_ICE: f32 = 0.028;
const SKID_DECEL_ICE_STAR: f32 = 0.038;
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
/// Super/Fire standing (§6.1 row 0) and crouching (row 1).
pub const SUPER: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 27.0,
    wall_y0: 4.0,
    wall_y1: 19.0,
    wall_x: 7.9,
};
pub const SUPER_CROUCH: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 13.9,
    wall_y0: 4.0,
    wall_y1: 11.0,
    wall_x: 7.9,
};

/// Power-up form (player+0x498: 0 small, 1 Super, 2 Fire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Small,
    Super,
    Fire,
}

/// Invincibility after taking damage (§7).
pub const INVULN_DAMAGE: i32 = 127;
/// Star power (§5.4).
pub const STAR_FRAMES: i32 = 660;
/// Grow/shrink animation; the world is frozen meanwhile. [?] Length not traced (change state 3).
pub const CHANGE_FRAMES: i32 = 48;
// Death (DownPose 0x71009e13e0 / DownFall 0x71009e1ea0): 45-frame pose, then a hop at 3.2 with
// gravity −0.16; the restart request comes 140 frames into the fall.
pub const DOWN_POSE_FRAMES: i32 = 45;
pub const DOWN_VY: f32 = 3.2;
pub const DOWN_GRAVITY: f32 = -0.16;
pub const DOWN_FALL_FRAMES: i32 = 140;
/// Enemy stomp bounce: [?] not located in the binary (player-physics.md §9 q1); the
/// player-vs-player pair is used meanwhile.
pub const STOMP_BOUNCE_HELD: f32 = 4.128;
pub const STOMP_BOUNCE: f32 = 3.628;

/// Who is steering the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Normal,
    /// Growing or shrinking; `from` is the old form (drawn alternately).
    Change {
        timer: i32,
        from: Form,
    },
    /// Dead: pose (timer counts the pose), then the fall.
    DownPose {
        timer: i32,
    },
    DownFall {
        timer: i32,
    },
    /// Scripted by the goal demo (World moves the player).
    Demo,
}

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

/// Band for an acceleration table given speed along x, the facing direction and thresholds.
fn band(speed_f: f32, facing: i32, t: &[f32; 4]) -> usize {
    let s = speed_f.abs();
    if speed_f != 0.0 && speed_f.signum() as i32 != facing {
        return if s > t[3] { 5 } else { 0 };
    }
    t.iter().position(|&x| s <= x).map(|i| i + 1).unwrap_or(5)
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
    Crouch,
    Dead,
    Pole,
    Fire,
    Swim,
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
    /// Slope under the feet while grounded: (steepness index, +1 if the surface rises to the
    /// right / −1 if it falls).
    pub slope: Option<(usize, i32)>,
    pub form: Form,
    pub control: Control,
    pub invuln: i32,
    pub star: i32,
    pub crouch: bool,
    /// Tile the head hit this frame (for block bumps).
    pub head_hit: Option<(i32, i32)>,
    /// Fireball throw requested this frame (World spawns it if the limits allow).
    pub throw: bool,
    pub fire_pose: i32,
    /// Hidden from view (inside the castle at the goal).
    pub hidden: bool,
    /// Stomp chain without touching the ground.
    pub stomp_chain: u32,
    /// Standing on ice (set by the world from the ground under the feet).
    pub on_ice: bool,
    /// Actor id of the moving solid the player stands on.
    pub ride: Option<u32>,
    /// Touched a spike trap this frame while moving into it (the world applies the damage).
    pub spiked: bool,
    /// Below a water surface (set by the world each frame): swim physics.
    pub in_water: bool,
    was_in_water: bool,
    /// Jump pressed in the last 20 frames (a jump out of the water when leaving it).
    exit_window: i32,
    skid: SkidPhase,
    prev_jump: bool,
    prev_run: bool,
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
            slope: None,
            form: Form::Small,
            control: Control::Normal,
            invuln: 0,
            star: 0,
            crouch: false,
            head_hit: None,
            throw: false,
            fire_pose: 0,
            hidden: false,
            stomp_chain: 0,
            on_ice: false,
            ride: None,
            spiked: false,
            in_water: false,
            was_in_water: false,
            exit_window: 0,
            skid: SkidPhase::None,
            prev_jump: false,
            prev_run: false,
        }
    }

    fn thresholds(&self) -> &'static [f32; 4] {
        if self.star > 0 { &T_STAR } else { &T }
    }

    pub fn big(&self) -> bool {
        self.form != Form::Small
    }

    pub fn sensors(&self) -> &'static Sensors {
        match (self.big(), self.crouch) {
            (false, _) => &SMALL_M1,
            (true, false) => &SUPER,
            (true, true) => &SUPER_CROUCH,
        }
    }

    /// Body box A (§6.2), the hurt box: (x0, y0, x1, y1).
    pub fn hurt_box(&self) -> (f32, f32, f32, f32) {
        let (hw, h) = match (self.big(), self.crouch) {
            (false, false) => (4.0, 14.0),
            (false, true) => (4.0, 7.0),
            (true, false) => (6.0, 24.0),
            (true, true) => (6.0, 14.0),
        };
        (self.x - hw, self.y, self.x + hw, self.y + h)
    }

    /// Body box B (§6.2), the wider contact box used for items and coins.
    pub fn contact_box(&self) -> (f32, f32, f32, f32) {
        let (hw, h) = match (self.big(), self.crouch) {
            (false, false) => (6.0, 18.0),
            (false, true) => (6.0, 10.0),
            (true, false) => (6.0, 28.0),
            (true, true) => (6.0, 16.0),
        };
        (self.x - hw, self.y, self.x + hw, self.y + h)
    }

    pub fn alive(&self) -> bool {
        !matches!(
            self.control,
            Control::DownPose { .. } | Control::DownFall { .. }
        )
    }

    /// Take a hit. Fire → Super, Super → Small (0x71009d45c0), Small → death. Ignored while
    /// invincible or starred. Returns true if it had an effect.
    pub fn hurt(&mut self) -> bool {
        if self.invuln > 0 || self.star > 0 || !matches!(self.control, Control::Normal) {
            return false;
        }
        match self.form {
            Form::Small => self.kill(),
            f => {
                self.control = Control::Change {
                    timer: CHANGE_FRAMES,
                    from: f,
                };
                self.form = if f == Form::Super {
                    Form::Small
                } else {
                    Form::Super
                };
                self.crouch = false;
                self.invuln = INVULN_DAMAGE;
            }
        }
        true
    }

    /// Lose a life (pits, time up, damage as small).
    pub fn kill(&mut self) {
        if !self.alive() {
            return;
        }
        self.control = Control::DownPose {
            timer: DOWN_POSE_FRAMES,
        };
        self.vx = 0.0;
        self.vy = 0.0;
        self.star = 0;
        self.pose = Pose::Dead;
    }

    /// Collect a power-up (20-super-mushroom.md: form changes only upward).
    pub fn power_up(&mut self, to: Form) -> bool {
        let up = matches!(
            (self.form, to),
            (Form::Small, Form::Super | Form::Fire) | (Form::Super, Form::Fire)
        );
        if up && self.control == Control::Normal {
            // small + Fire Flower goes straight to Fire (the block swap to a mushroom happens at
            // spawn time)
            self.control = Control::Change {
                timer: CHANGE_FRAMES,
                from: self.form,
            };
            self.form = to;
        }
        up
    }

    /// Bounce off an enemy.
    pub fn bounce(&mut self, jump_held: bool) {
        self.vy = if jump_held {
            STOMP_BOUNCE_HELD
        } else {
            STOMP_BOUNCE
        };
        self.on_ground = false;
        self.slope = None;
    }

    /// Advance the non-physics controls; returns true if normal physics should run.
    fn step_control(&mut self) -> bool {
        match self.control {
            Control::Normal => true,
            Control::Demo => false,
            Control::Change { timer, .. } => {
                self.control = if timer <= 1 {
                    Control::Normal
                } else {
                    Control::Change {
                        timer: timer - 1,
                        from: match self.control {
                            Control::Change { from, .. } => from,
                            _ => self.form,
                        },
                    }
                };
                false
            }
            Control::DownPose { timer } => {
                self.control = if timer <= 1 {
                    self.vy = DOWN_VY;
                    Control::DownFall {
                        timer: DOWN_FALL_FRAMES,
                    }
                } else {
                    Control::DownPose { timer: timer - 1 }
                };
                false
            }
            Control::DownFall { timer } => {
                self.vy = (self.vy + DOWN_GRAVITY).max(FALL_CAP);
                self.y += self.vy;
                if timer > 0 {
                    self.control = Control::DownFall { timer: timer - 1 };
                }
                false
            }
        }
    }

    /// The death sequence has finished: the course should restart.
    pub fn restart_due(&self) -> bool {
        matches!(self.control, Control::DownFall { timer: 0 })
    }

    fn ground(&mut self, inp: Input) {
        let dir = inp.dir_x();
        let run = inp.held(input::RUN);
        let t = self.thresholds();
        let star = self.star > 0;
        let (walk_max, run_max) = (t[1], t[3]);

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
                let decel = match (self.on_ice, star) {
                    (false, _) => SKID_DECEL,
                    (true, false) => SKID_DECEL_ICE,
                    (true, true) => SKID_DECEL_ICE_STAR,
                };
                self.vx = approach(self.vx, 0.0, decel);
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
                    dir as f32 * if run { run_max } else { walk_max }
                };
                self.vx = approach(self.vx, target, SKID_REACCEL);
                if dir == 0 || self.vx.abs() >= walk_max {
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
                let target = dir as f32 * if run { run_max } else { walk_max } * mult;
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

        let ice = self.on_ice;
        let (target, table) = if dir == 0 {
            let table = match (self.crouch, ice) {
                (false, false) => &GROUND_IDLE,
                (true, false) => &GROUND_CROUCH,
                (false, true) => &ICE_IDLE,
                (true, true) => &ICE_CROUCH,
            };
            (0.0, table)
        } else {
            self.facing = dir;
            let table = match (run, star, ice) {
                (false, false, false) => &GROUND_WALK,
                (false, true, false) => &GROUND_WALK_STAR,
                (true, false, false) => &GROUND_RUN,
                (true, true, false) => &GROUND_RUN_STAR,
                (false, false, true) => &ICE_WALK,
                (false, true, true) => &ICE_WALK_STAR,
                (true, false, true) => &ICE_RUN,
                (true, true, true) => &ICE_RUN_STAR,
            };
            (dir as f32 * if run { run_max } else { walk_max }, table)
        };
        self.vx = approach(self.vx, target, table[band(self.vx, self.facing, t)]);
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
            let t = self.thresholds();
            let star = self.star > 0;
            let s = self.vx.abs();
            let target = if run {
                t[3]
            } else if s > t[1] && self.vx.signum() as i32 == dir {
                s
            } else {
                t[1]
            } * dir as f32;
            // SMB1 keeps the take-off facing in the air (§2.6); bands use the movement facing
            let table = match (run, star) {
                (false, false) => &AIR_DIR,
                (true, false) => &AIR_DIR_RUN,
                (false, true) => &AIR_DIR_STAR,
                (true, true) => &AIR_DIR_RUN_STAR,
            };
            let face = if self.vx == 0.0 {
                dir
            } else {
                self.vx.signum() as i32
            };
            let b = if dir != face {
                0
            } else {
                band(self.vx, face, t)
            };
            self.vx = approach(self.vx, target, table[b]);
        }
        let cap = self.thresholds()[3];
        self.vx = self.vx.clamp(-cap, cap); // hard air cap
    }

    /// One 60 Hz tick.
    pub fn step(&mut self, inp: Input, level: &Level) {
        self.head_hit = None;
        self.throw = false;
        if !self.step_control() {
            return;
        }
        if self.invuln > 0 {
            self.invuln -= 1;
        }
        if self.star > 0 {
            self.star -= 1;
        }
        if self.fire_pose > 0 {
            self.fire_pose -= 1;
        }
        let jump = inp.held(input::JUMP);
        let pressed = jump && !self.prev_jump;
        self.prev_jump = jump;
        let run = inp.held(input::RUN);
        if run && !self.prev_run && self.form == Form::Fire && !self.crouch {
            self.throw = true;
        }
        self.prev_run = run;

        // crouch: big forms on the ground with Down held (kept through a crouch jump)
        if self.on_ground {
            let want = self.big() && inp.held(input::DOWN);
            if want != self.crouch && (want || self.can_stand(level)) {
                self.crouch = want;
            }
        }
        let inp = if self.crouch && self.on_ground {
            Input {
                buttons: inp.buttons & !(input::LEFT | input::RIGHT),
            }
        } else {
            inp
        };

        if self.exit_window > 0 {
            self.exit_window -= 1;
        }
        if self.in_water {
            self.swim(inp, pressed);
        } else {
            if self.was_in_water && self.exit_window > 0 && self.vy > 0.0 {
                // leaving the water within 20 frames of a stroke: jump out (vy 3.5)
                self.vy = 3.5;
            }
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
        }
        self.was_in_water = self.in_water;

        let sensors = self.sensors();
        self.move_and_collide(level, sensors);

        if self.on_ground {
            self.stomp_chain = 0;
        }
        if self.crouch {
            self.pose = Pose::Crouch;
        } else if self.in_water && !self.on_ground {
            self.pose = Pose::Swim;
            self.walk_phase += 1.0;
        } else if !self.on_ground {
            self.pose = Pose::Jump;
        } else if self.pose == Pose::Walk {
            self.walk_phase += self.vx.abs();
        }
        if self.throw {
            self.fire_pose = 8;
        }
        if self.fire_pose > 0 && !self.crouch {
            self.pose = Pose::Fire;
        }
        if self.y < -2.0 * TILE {
            self.kill(); // fell into a pit
        }
    }

    /// Swimming (§5.1): strokes add 1.0 (1.25 from the bottom), the player sinks toward −1.5
    /// (−0.043/frame while rising, −0.027 while sinking), upward cap 1.8; horizontal speeds from
    /// the swim table (row 0 swimming, row 3 walking on the bottom).
    fn swim(&mut self, inp: Input, pressed: bool) {
        if !self.was_in_water {
            self.vy = self.vy.max(-1.5);
            self.vx = self.vx.clamp(-1.125, 1.125);
        }
        // vertical
        if self.vy >= 0.0 {
            self.vy -= 0.043;
        } else if self.vy > -1.5 {
            self.vy = (self.vy - 0.027).max(-1.5);
        } else {
            self.vy = (self.vy + 0.094).min(-1.5);
        }
        self.vy = self.vy.min(1.8);
        if pressed {
            if self.on_ground {
                self.vy = 1.25;
                self.on_ground = false;
            } else {
                self.vy += 1.0;
            }
            self.exit_window = 20;
            self.walk_phase = 0.0;
        }
        // horizontal: (target, reverse, accel ≤ target, decel > target, idle ≤, idle >, cap, above cap)
        let row = if self.on_ground {
            (0.5625, 0.05, 0.025, 0.05, 0.004, 0.04, 1.0, 0.05)
        } else {
            (1.125, 0.03, 0.025, 0.025, 0.004, 0.02, 1.5, 0.03)
        };
        let dir = inp.dir_x();
        let v = self.vx;
        if v.abs() > row.6 {
            self.vx = approach(v, row.6 * v.signum(), row.7);
        } else if dir == 0 {
            let a = if v.abs() <= row.0 { row.4 } else { row.5 };
            self.vx = approach(v, 0.0, a);
        } else {
            self.facing = dir;
            let target = row.0 * dir as f32;
            let a = if v != 0.0 && v.signum() as i32 != dir {
                row.1
            } else if v.abs() <= row.0 {
                row.2
            } else {
                row.3
            };
            self.vx = approach(v, target, a);
        }
        if self.on_ground {
            self.pose = if self.vx == 0.0 {
                Pose::Wait
            } else {
                Pose::Walk
            };
        }
    }

    /// Room to stand up from a crouch.
    fn can_stand(&self, level: &Level) -> bool {
        let hy = self.y + SUPER.head_y;
        ![-SUPER.head_x, SUPER.head_x]
            .iter()
            .any(|&hx| Self::solid(level, self.x + hx, hy))
    }

    fn solid(level: &Level, px: f32, py: f32) -> bool {
        level.cell_at(px, py).is_solid()
    }

    fn spike(level: &Level, px: f32, py: f32) -> bool {
        level.cell_at(px, py) == Cell::Spike
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
        self.spiked = false;
        self.x += self.vx * cos;
        for dir in [-1.0f32, 1.0] {
            let wx = self.x + dir * s.wall_x;
            let prev_wx = wx - self.vx * cos;
            let cell_x = (wx / TILE).floor() * TILE;
            let hit = [s.wall_y0, s.wall_y1].iter().any(|&wy| {
                let c = level.cell_at(wx, self.y + wy);
                c.is_solid()
                    // one-way walls block only motion into them from their blocking side
                    || (dir < 0.0 && c == Cell::PassRight && prev_wx >= cell_x + TILE - 0.01)
                    || (dir > 0.0 && c == Cell::PassLeft && prev_wx <= cell_x + 0.01)
            });
            // spikes hurt only when moving into them along that axis
            if self.vx * dir > 0.0
                && [s.wall_y0, s.wall_y1]
                    .iter()
                    .any(|&wy| Self::spike(level, wx, self.y + wy))
            {
                self.spiked = true;
            }
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
        // moving solids: sides (full solids only)
        for b in level.solids.iter().filter(|b| !b.top_only) {
            let (y0, y1) = (self.y + s.wall_y0, self.y + s.wall_y1);
            if y1 <= b.y0 || y0 >= b.y1 {
                continue;
            }
            let (l, r) = (self.x - s.wall_x, self.x + s.wall_x);
            if r > b.x0 && l < b.x1 {
                if self.x < (b.x0 + b.x1) / 2.0 {
                    self.x = b.x0 - s.wall_x - 0.001;
                    self.vx = self.vx.min(0.0);
                } else {
                    self.x = b.x1 + s.wall_x + 0.001;
                    self.vx = self.vx.max(0.0);
                }
            }
        }
        self.x = self.x.clamp(level.left + s.wall_x, level.right - s.wall_x);

        // vertical
        let prev_y = self.y;
        self.y += self.vy;
        self.ride = None;
        if self.vy > 0.0 {
            let hy = self.y + s.head_y;
            if [-s.head_x, s.head_x]
                .iter()
                .any(|&hx| Self::spike(level, self.x + hx, hy))
            {
                self.spiked = true;
            }
            for b in level.solids.iter().filter(|b| !b.top_only) {
                if self.x + s.head_x > b.x0
                    && self.x - s.head_x < b.x1
                    && hy > b.y0
                    && prev_y + s.head_y <= b.y0 + 0.01
                {
                    self.y = b.y0 - s.head_y - 0.001;
                    self.vy = 0.0;
                }
            }
            let hy = self.y + s.head_y;
            let prev_head = prev_y + s.head_y;
            let hits: Vec<f32> = [-s.head_x, s.head_x]
                .into_iter()
                .filter(|&hx| {
                    let c = level.cell_at(self.x + hx, hy);
                    c.is_solid()
                        || (c == Cell::PassDown && prev_head <= (hy / TILE).floor() * TILE + 0.01)
                })
                .collect();
            if !hits.is_empty() {
                // the block that gets hit: the one under the head centre if both probes hit
                let cx = if hits.len() == 2 {
                    self.x
                } else {
                    self.x + hits[0]
                };
                self.head_hit = Some(((cx / TILE).floor() as i32, (hy / TILE).floor() as i32));
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
                let ok = cell.is_solid() || (cell == Cell::SemiTop && prev_y >= top - 0.01);
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
            if [-s.foot_x, s.foot_x]
                .iter()
                .any(|&fx| Self::spike(level, self.x + fx, self.y))
                && self.vy < 0.0
            {
                self.spiked = true;
            }
            // moving solids: land on their tops if the feet were above last frame
            let mut ride = None;
            for b in &level.solids {
                let over = self.x + s.foot_x > b.x0 && self.x - s.foot_x < b.x1;
                let reach = if was_on_ground { 2.0 } else { 0.0 };
                if over
                    && self.y <= b.y1 + reach
                    && prev_y >= b.y1 - b.dy.abs() - 1.0
                    && landed.is_none_or(|t| b.y1 >= t)
                {
                    landed = Some(b.y1);
                    ride = Some(b.owner);
                    on_slope = None;
                }
            }
            self.ride = ride;
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
            assert!(p.alive());
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
