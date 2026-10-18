//! Parts added in SMM2's 2.0 / 3.0 updates (they exist only in the v3.0.3 executable): Spike
//! and its balls (74-spike.md), Pokey (78-pokey.md), Mechakoopa and its missile / laser
//! (111-mechakoopa.md) and the ON/OFF Trampoline (132-on-off-trampoline.md). The Koopalings are
//! in actors/koopalings.rs; the world-side rules (touching them, carrying, sprites, the P Block
//! and Dash Block) in world/v3.rs.
//!
//! Every rule follows the specs' [obs] facts (each spec cites the v3.0.3 function); choices the
//! binary does not settle are marked [inf] / [ext] with the reason. Animation-timed states use
//! the animations' own lengths from the style's model data (listed in the specs).
//!
//! State layout: `phase` is the state (0 = not set up yet), `timer` the state timer, and the rest
//! lives in `Actor::v3`.

use super::{Actor, ENEMY_GRAVITY, Kind, MAX_FALL, Mode};
use crate::fmath::DetMath;
use crate::level::{Cell, Level};

/// A sprite choice: model, texture (animation frame), width, height, mirror.
pub type V3Look = (&'static str, &'static str, f32, f32, bool);

/// One Pokey segment (index 0 = the bottom one): the extra height it still has to fall to its
/// slot after a lower segment was knocked out, its fall speed, and its sway phase (degrees).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seg {
    pub drop: f32,
    pub vy: f32,
    pub phase: f32,
}

/// Per-actor state of the 2.0 / 3.0 parts (and the Koopalings).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct V3State {
    /// A second state machine (Mechakoopa's weapon controller, a Koopaling's sub-state).
    pub sub: u8,
    /// Frames into the current animation / sub-state.
    pub anim: i32,
    /// Second and third timers (re-aim, cooldowns, frames since the last lift).
    pub clock: i32,
    pub cool: i32,
    /// Small counters (throws in a burst, hops, shots in a volley).
    pub count: u8,
    pub count2: u8,
    /// Spike: holding a ball over its head. Koopaling: in its shell.
    pub holding: bool,
    /// Owner actor id (projectiles), linked actor (a laser's robot).
    pub owner: u32,
    /// Target speed (balls), turn rate (missile, deg/frame), laser length.
    pub speed: f32,
    pub turn: f32,
    pub len: f32,
    /// Koopaling HP and fire counter; spawn x (home).
    pub hp: u8,
    pub fire: u8,
    pub home: f32,
    /// Pokey segments.
    pub segs: Vec<Seg>,
    /// Sensed by the world: the ON/OFF state (true = ON).
    pub on_off: bool,
    /// Scale pop after a hit that took no HP (Koopaling fireball hits): 0 = none.
    pub pop: f32,
    /// Requests to the world: projectiles to fire now, a ground quake (Morton, Roy), Morton's
    /// two flames.
    pub shots: u8,
    pub quake: bool,
    pub flames: bool,
    /// Flag bits (see actors/koopalings.rs).
    pub flags: u8,
    /// The course's style index (Koopalings: NSMBU flinches before the shell).
    pub style: u8,
}

/// Style index: 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU, 4 3D World.
pub(super) fn style_ix(level: &Level) -> usize {
    match level.style.as_str() {
        "M1" => 0,
        "M3" => 1,
        "MW" => 2,
        "WU" => 3,
        _ => 4,
    }
}

/// The frame of a sprite animation `keys` (start frame, texture) of `len` frames at time `t`.
pub(super) fn anim_key(
    keys: &[(i32, &'static str)],
    len: i32,
    t: i32,
    looped: bool,
) -> &'static str {
    let t = if looped {
        t.rem_euclid(len.max(1))
    } else {
        t.clamp(0, len)
    };
    keys.iter()
        .rev()
        .find(|(k, _)| *k <= t)
        .map(|(_, s)| *s)
        .unwrap_or(keys[0].1)
}

/// The shortest signed angle (degrees) from `a` to `b`.
pub(super) fn angle_diff(a: f32, b: f32) -> f32 {
    (b - a + 540.0).rem_euclid(360.0) - 180.0
}

// ---------------------------------------------------------------- Spike

mod spike {
    /// GabonWU (SMW / NSMBU / 3D World): stands and throws.
    pub const WAIT: u8 = 1;
    /// 3D World Turn3W.
    pub const TURN: u8 = 2;
    pub const LIFT: u8 = 3;
    pub const THROW: u8 = 4;
    /// Gabon3rd (SMB1 / SMB3): walks between throws.
    pub const WALK: u8 = 5;
    pub const POST: u8 = 6;
}

/// Lift and throw animations per style: (lift length, ball-appears frame, throw length, release
/// frame) [obs: 2D throw_st 35 / throw_ed 15; WU 50 / 85; 3W ThrowSign 90 / Throw 71].
const SPIKE_ANIM: [(i32, i32, i32, i32); 5] = [
    (35, 0, 15, 14),
    (35, 0, 15, 14),
    (35, 0, 15, 14),
    (50, 24, 85, 52),
    (90, 10, 71, 39),
];
/// MW / WU turn: instant, then 30 frames without throwing; 3W Turn3W: 50 frames, facing flips at
/// animation frame 8 [obs].
const SPIKE_TURN_COOL: i32 = 30;
const SPIKE_TURN_3W: i32 = 50;
const SPIKE_TURN_FLIP: i32 = 8;
/// Gabon3rd throw window (player relative to the Spike): 32…288 px ahead, −16…+47 px (big +95)
/// above [obs].
const SPIKE_RANGE: (f32, f32) = (32.0, 288.0);
/// Gabon3rd: a 35-frame stand after each throw, bursts of 2 throws, 120 frames between bursts
/// [obs].
const SPIKE_POST: i32 = 35;
const SPIKE_BURST_REST: i32 = 120;
/// The `GeneratorPosition` joint at the throw's release frame (`SPIKE_ANIM`), px forward of
/// and above the feet per style, read from the models' skeletal animations (throw_ed / Throw):
/// SMB1 (−1 → 3, 23 → 24 over throw_ed; 3, 24 at frame 14), SMB3 / SMW (3, 25), NSMBU (2, 24
/// at frame 52), 3D World (z 6.6 forward, 28.76 at frame 39). The 2D and NSMBU Spike models
/// are flat and drawn facing left with +x behind (a Koopaling's `magic` joint sits at −x, the
/// wings at +x), so their +3 / +2 lie behind the Spike; the 3D World model faces +z.
const SPIKE_HAND: [(f32, f32); 5] = [
    (-3.0, 24.0),
    (-3.0, 25.0),
    (-3.0, 25.0),
    (-2.0, 24.0),
    (6.6, 28.76),
];

mod ball {
    /// SMB1 / SMB3 release: flies level at 1.5 px/f without gravity.
    pub const FLY: u8 = 1;
    pub const ROLL: u8 = 2;
    /// At rest (CarryWait): a placed ball, or one that slowed down on flat ground.
    pub const REST: u8 = 3;
}

/// Ball rolling (0x71010f2590 family) [obs]: gravity −0.1875, fall cap −3.2, flat-ground speed
/// 1.2, the excess over 1.7 decays by 0.03 × excess per frame (×0.3 in the air), slopes add
/// 0.02 × gradient per frame up to 1.7 + 1.1 × |gradient|, slowing at most 0.3 per frame, at rest
/// below 0.5 on flat ground; landings faster than 0.4 bounce at 0.6 × the landing speed.
const BALL_FALL: f32 = -3.2;
const BALL_FLAT: f32 = 1.2;
const BALL_SOFT_CAP: f32 = 1.7;
const BALL_RELEASE_VY: f32 = 2.7;
const BALL_FLY: f32 = 1.5;
const BALL_REST: f32 = 0.5;

// ---------------------------------------------------------------- Pokey

/// Pokey [obs 78-pokey.md]: walks 0.25 px/f (winged 0.5), turns at walls and ledges with the
/// walker's 8-frame lock; segments 16 px apart (3D World 15.5), swaying 1.5 px at 5°/frame;
/// a segment knocked out lets the ones above hop (1.25 + 0.05·i) and fall back (cap −4.5).
const POKEY_WALK: f32 = 0.25;
const POKEY_WING_WALK: f32 = 0.5;
const POKEY_SWAY: f32 = 1.5;
const POKEY_SWAY_RATE: f32 = 5.0;
const POKEY_HOPS: [f32; 3] = [2.5, 2.5, 4.7];
/// `V3State::flags` bits of a Pokey: segments settling (DamageWait) last frame; touching a
/// side wall last frame.
const POKEY_SETTLING: u8 = 1;
const POKEY_WALLED: u8 = 2;
pub const POKEY_MAX: usize = 8;

// ---------------------------------------------------------------- Mechakoopa

mod mecha {
    pub const WALK: u8 = 1;
    pub const TURN: u8 = 2;
    pub const DOWN: u8 = 3;
    pub const REVIVAL: u8 = 4;
    /// Winged patrol.
    pub const FLY: u8 = 5;
    /// Night Ground hop: JumpStart, Jump, Hover, descent [obs].
    pub const NIGHT_ST: u8 = 6;
    pub const NIGHT_JUMP: u8 = 7;
    pub const NIGHT_HOVER: u8 = 8;
    pub const NIGHT_FALL: u8 = 9;
    /// Weapon controller (`v3.sub`).
    pub const READY: u8 = 0;
    pub const SIGN: u8 = 1;
    pub const CHARGE: u8 = 2;
    pub const SHOT: u8 = 3;
    pub const END: u8 = 4;
}
const MECHA_WALK: f32 = 0.5;
/// Knocked down for 480 frames; the shake warning plays below 61; revival 18 frames [obs].
pub const MECHA_DOWN: i32 = 480;
const MECHA_SIGN: i32 = 61;
const MECHA_REVIVAL: i32 = 18;
/// Weapon timing [obs]: first shot after 30 + 0…59 frames, then 180 + 0…29; fire_st 30 frames
/// (NSMBU 28); Charge 60 (Blasta) / 71 (Zappa); fire_ed 20.
const MECHA_SIGN_LEN: [i32; 2] = [30, 28];
const MECHA_CHARGE: [i32; 2] = [60, 71];
const MECHA_FIRE_WAIT: i32 = 8;
const MECHA_FIRE_ED: i32 = 20;
/// Zappa's Shot lasts until the beam leaves its Shot: the beam's Sign takes 69 frames from the
/// Charge enter (2 fewer than the Charge), its Shot 40; leaving Shot it raises the robot's
/// controller byte +0x2A, and the robot's Shot ends on that byte (or the beam gone), not on
/// the 10 fade frames [obs 0x71011cf1d0, beam Shot exit 0x7100de6330].
const ZAPPA_SHOT: i32 = 69 - 71 + 40;

/// Missile [obs]: speed 1.2 at launch; phases aim (target 0.5, accel 0.01, wanted turn ≤ 1.5°,
/// turn-rate change ≤ 0.03°/f), home (1.2, 0.01, ≤ 0.7°, 0.05°) and overshoot (0.5, 0.007,
/// ≤ 0.3°, 0.03°, 40 frames); blinks from frame 50, explodes at 300.
pub const MISSILE_LIFE: i32 = 300;
/// The laser [obs]: telegraph 69 frames, then it grows 10 px/f to 320 px for 40 frames and
/// fades for 10; half height 4 (big 8), centred 2 px (4) below the muzzle.
pub const LASER_SIGN: i32 = 69;
pub const LASER_SHOT: i32 = 40;
pub const LASER_FADE: i32 = 10;
pub const LASER_MAX: f32 = 320.0;
const LASER_GROW: f32 = 10.0;

impl Kind {
    /// One of the 2.0 / 3.0 parts of this module or actors/koopalings.rs.
    pub fn is_v3(self) -> bool {
        matches!(
            self,
            Kind::Spike
                | Kind::SpikeBall { .. }
                | Kind::Pokey { .. }
                | Kind::Mechakoopa { .. }
                | Kind::MechaMissile
                | Kind::MechaLaser
                | Kind::OnOffTrampoline { .. }
                | Kind::Koopaling { .. }
                | Kind::KoopalingShot { .. }
        )
    }

    /// The 2.0 / 3.0 parts that count as enemies (Star, POW, Moon, clear conditions).
    pub fn v3_enemy(self) -> bool {
        matches!(
            self,
            Kind::Spike
                | Kind::SpikeBall { .. }
                | Kind::Pokey { .. }
                | Kind::Mechakoopa { .. }
                | Kind::MechaMissile
                | Kind::Koopaling { .. }
        )
    }
}

impl Actor {
    /// Half width and height of the body box.
    pub(super) fn v3_size(&self) -> (f32, f32) {
        let s = self.v3_scale();
        match self.kind {
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => self.koopaling_size(),
            // a Pokey is as tall as its live segments (not scaled: it has no big actor [obs])
            Kind::Pokey { height, .. } if self.v3.segs.is_empty() => (8.0, 16.0 * height as f32),
            Kind::Pokey { .. } => (8.0, 16.0 * self.v3.segs.len() as f32),
            Kind::MechaMissile => (6.0 * s, 12.0 * s),
            Kind::MechaLaser => (4.0, 8.0),
            Kind::OnOffTrampoline { width, .. } => (width as f32 * 8.0, 16.0),
            _ => (8.0 * s, 16.0 * s),
        }
    }

    pub fn v3_scale(&self) -> f32 {
        if self.big { 2.0 } else { 1.0 }
    }

    pub(super) fn v3_toward(&self) -> i32 {
        if self.target.0 <= self.x { -1 } else { 1 }
    }

    pub(super) fn v3_fall(&mut self, g: f32, cap: f32) {
        self.vy = (self.vy + g).max(cap);
    }

    /// Standing at a ledge in direction `dir` (the walker's probe: just past the box's edge,
    /// one pixel down).
    pub(super) fn v3_ledge(&self, level: &Level, dir: i32) -> bool {
        if !self.on_ground {
            return false;
        }
        let px = self.x + dir as f32 * (self.half_w() + 0.1);
        let slope = level.slope_at(px).is_some_and(|(s, _)| s >= self.y - 8.0);
        level.cell_at(px, self.y - 1.0) == Cell::Empty && !slope
    }

    /// Actor flag 0x520 bit 3: each frame the enemy base sets it when the actor's box touches a
    /// rectangle of the camera manager's views list (+0x138, one list per area), edges
    /// included, and clears it otherwise [obs v3 0x7101294a80 = v1 0x7100792c90]. The list is
    /// rebuilt every frame from the active cameras (v1 0x7100b64340): in single player it holds
    /// just the camera's view.
    pub(super) fn v3_in_view(&self) -> bool {
        let (x0, y0, x1, y1) = self.bbox();
        let (cx, cy) = self.camera;
        x1 >= cx && x0 <= cx + crate::world::VIEW_W && y1 >= cy && y0 <= cy + crate::world::VIEW_H
    }

    /// The solid surface of a 2.0 / 3.0 platform.
    pub(super) fn v3_solid_box(&self) -> Option<(f32, f32, f32, f32, bool)> {
        match self.kind {
            Kind::OnOffTrampoline { width, .. } => {
                let hw = width as f32 * 8.0;
                Some((self.x - hw, self.y + 8.0, self.x + hw, self.y + 16.0, true))
            }
            _ => None,
        }
    }

    /// An ON/OFF Trampoline bounces while its colour's state is on: the normal one while ON,
    /// the alternate (blue) one while OFF (its flag-0x4 bit differs from the ON/OFF bit) [obs].
    pub fn v3_bouncy(&self, on: bool) -> bool {
        matches!(self.kind, Kind::OnOffTrampoline { blue, .. } if blue != on)
    }

    /// One frame of a 2.0 / 3.0 part (from `step_normal`).
    pub(super) fn step_v3(&mut self, level: &Level) -> Option<Actor> {
        self.v3.style = style_ix(level) as u8;
        match self.kind {
            Kind::Spike => self.step_spike(level),
            Kind::SpikeBall { .. } => {
                self.step_spike_ball(level);
                None
            }
            Kind::Pokey { .. } => {
                self.step_pokey(level);
                None
            }
            Kind::Mechakoopa { weapon } => self.step_mecha(level, weapon),
            Kind::MechaMissile => {
                self.step_missile();
                None
            }
            Kind::MechaLaser => {
                self.step_laser(level);
                None
            }
            Kind::OnOffTrampoline { .. } => None, // always stationary [obs]
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => self.step_koopaling(level),
            _ => None,
        }
    }

    // ------------------------------------------------------------ Spike

    /// No solid tile in the probes over the head: at x and x ± 7.5 (big ± 15) from 15 to 31 px
    /// up (big 31 to 63) [obs].
    fn spike_head_clear(&self, level: &Level) -> bool {
        let s = self.v3_scale();
        let (y0, y1) = (self.y + 15.0 * s, self.y + 31.0 * s);
        [-7.5 * s, 0.0, 7.5 * s].iter().all(|&dx| {
            let mut y = y0;
            while y <= y1 {
                if level.cell_at(self.x + dx, y).is_solid() {
                    return false;
                }
                y += 8.0;
            }
            !level.cell_at(self.x + dx, y1).is_solid()
        })
    }

    /// Spike (74-spike.md). SMB1 / SMB3 use Gabon3rd, which walks and throws when the player is
    /// in front; the other styles use GabonWU, which stands, faces the player and throws on a
    /// timer. The ball appears in its hands during the lift and leaves at the throw's release
    /// frame; a Spike that is knocked out while holding it breaks it.
    fn step_spike(&mut self, level: &Level) -> Option<Actor> {
        let st = style_ix(level);
        let walker = st <= 1;
        if self.phase == 0 {
            // first Wait: 0…59 frames; Gabon3rd's re-face timer 90 + 0…59 [obs]
            self.timer = self.rand(60) as i32;
            self.v3.clock = 90 + self.rand(60) as i32;
            self.phase = if walker { spike::WALK } else { spike::WAIT };
        }
        if self.wings {
            // the Wing context flies it (the shared flyer) and it throws from the air with the
            // *Fly animations [obs anim names; flight inf]
            self.step_generic_wings(level);
        } else {
            self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
        }
        let (lift, spawn_at, throw, release) = SPIKE_ANIM[st];
        let mut out = None;
        if self.v3.cool > 0 {
            self.v3.cool -= 1;
        }
        match self.phase {
            spike::WAIT => {
                self.vx = 0.0;
                if self.timer > 0 {
                    self.timer -= 1;
                }
                self.v3.clock += 1;
                let want = self.v3_toward();
                if want != self.dir {
                    if st == 4 && self.on_ground && !self.wings {
                        self.phase = spike::TURN;
                        self.v3.anim = 0;
                    } else {
                        self.dir = want;
                        if st != 4 {
                            self.v3.cool = SPIKE_TURN_COOL;
                        }
                    }
                }
                let ready = self.timer <= 0
                    && self.v3.cool <= 0
                    && (st != 3 || self.v3.clock >= 120)
                    && (self.on_ground || self.wings);
                if self.phase == spike::WAIT && ready && self.spike_head_clear(level) {
                    self.phase = spike::LIFT;
                    self.v3.anim = 0;
                    self.v3.clock = 0;
                }
            }
            spike::TURN => {
                self.vx = 0.0;
                if self.timer > 0 {
                    self.timer -= 1; // the Wait timer keeps running [obs]
                }
                self.v3.anim += 1;
                if self.v3.anim == SPIKE_TURN_FLIP {
                    self.dir = -self.dir;
                }
                if self.v3.anim >= SPIKE_TURN_3W {
                    self.phase = spike::WAIT;
                }
            }
            spike::WALK => {
                self.vx = MECHA_WALK * self.dir as f32; // the walker's 0.5 [obs]
                self.v3.clock -= 1;
                if self.v3.clock <= 0 {
                    self.v3.clock = 90 + self.rand(60) as i32;
                    if self.v3_toward() != self.dir {
                        self.dir = -self.dir;
                    }
                }
                let s = self.v3_scale();
                let dx = (self.target.0 - self.x) * self.dir as f32;
                let dy = self.target.1 - self.y;
                let in_range = (SPIKE_RANGE.0..=SPIKE_RANGE.1).contains(&dx)
                    && dy >= -16.0
                    && dy <= if self.big { 95.0 } else { 47.0 };
                let _ = s;
                if in_range && self.v3.cool <= 0 && self.on_ground && self.spike_head_clear(level) {
                    self.phase = spike::LIFT;
                    self.v3.anim = 0;
                    self.vx = 0.0;
                }
            }
            spike::POST => {
                self.vx = 0.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = spike::WALK;
                }
            }
            spike::LIFT => {
                self.vx = 0.0;
                if self.v3.anim == spawn_at {
                    self.v3.holding = true;
                }
                self.v3.anim += 1;
                if self.v3.anim >= lift {
                    self.phase = spike::THROW;
                    self.v3.anim = 0;
                }
            }
            spike::THROW => {
                self.vx = 0.0;
                if self.v3.anim == release && self.v3.holding {
                    self.v3.holding = false;
                    out = Some(self.spike_ball(level, st));
                }
                self.v3.anim += 1;
                if self.v3.anim >= throw {
                    if walker {
                        self.phase = spike::POST;
                        self.timer = SPIKE_POST;
                        self.v3.count += 1;
                        if self.v3.count >= 2 {
                            self.v3.count = 0;
                            self.v3.cool = SPIKE_BURST_REST;
                        }
                    } else {
                        self.phase = spike::WAIT;
                        // after a throw: 120 + 0…29 frames (SMW 160 + 0…29) [obs]
                        let base = if st == 2 { 160 } else { 120 };
                        self.timer = base + self.rand(30) as i32;
                    }
                }
            }
            _ => self.phase = spike::WAIT,
        }
        if !self.wings {
            self.collide(level);
            if walker && self.wall && self.phase == spike::WALK {
                self.dir = -self.dir; // Gabon3rd turns at walls, not at ledges [obs]
            }
        }
        out
    }

    /// Where the ball is at the throw's release frame: its centre on the model's
    /// `GeneratorPosition` joint, which the lifted ball follows every frame [obs 0x71010e7670:
    /// joint position, minus the ball's centre offset +0x2FC for its feet]. The joint at the
    /// release frame (model data, `SPIKE_HAND`), forward and up from the Spike's feet.
    pub fn spike_hand(&self, st: usize) -> (f32, f32) {
        let s = self.v3_scale();
        let (fwd, up) = SPIKE_HAND[st.min(4)];
        (self.x + fwd * s * self.dir as f32, self.y + up * s)
    }

    /// The thrown ball, leaving the `GeneratorPosition` joint at the release frame.
    fn spike_ball(&self, level: &Level, st: usize) -> Actor {
        let snow = level.theme == 6;
        let (hx, hy) = self.spike_hand(st);
        let mut b = Actor::new(Kind::SpikeBall { snow }, hx, hy);
        b.big = self.big;
        b.y -= b.height() / 2.0;
        b.dir = self.dir;
        b.v3.owner = self.id;
        b.kick_lock = 90; // the ball ignores its thrower for 90 frames [obs]
        if st <= 1 {
            // SMB1 / SMB3: Fly, level at 1.5 px/f without gravity, until it touches ground or
            // a wall [obs: gravity 0 in Fly]
            b.phase = ball::FLY;
            b.vx = BALL_FLY * self.dir as f32;
        } else {
            b.phase = ball::ROLL;
            b.v3.speed = BALL_FLAT;
            b.vx = BALL_FLAT * self.dir as f32;
            b.vy = BALL_RELEASE_VY;
        }
        b
    }

    pub fn spike_holding(&self) -> bool {
        self.kind == Kind::Spike && self.v3.holding && self.mode == Mode::Normal
    }

    /// The gradient of the slope under the ball in its moving direction: > 0 downhill (a 45°
    /// slope is ±1).
    fn ball_gradient(&self, level: &Level) -> f32 {
        let Some((surf, sl)) = level.slope_at(self.x) else {
            return 0.0;
        };
        if (self.y - surf).abs() > 2.0 || !self.on_ground {
            return 0.0;
        }
        let g = if sl.steep {
            1.0
        } else {
            sl.height as f32 / sl.width.max(1) as f32
        };
        // a rising slope ("/") goes up to the right: moving right is uphill
        let up = if sl.rising { self.dir } else { -self.dir };
        -(up as f32) * g
    }

    /// A Spike's ball (or a snowball) rolling, flying or at rest.
    fn step_spike_ball(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = ball::REST;
        }
        match self.phase {
            ball::FLY => {
                self.vy = 0.0;
                self.collide(level);
                if self.wall {
                    self.mode = Mode::Dead; // breaks on a wall [obs]
                } else if self.on_ground {
                    self.phase = ball::ROLL;
                    self.v3.speed = BALL_FLAT;
                }
            }
            ball::ROLL => {
                self.v3_fall(ENEMY_GRAVITY, BALL_FALL);
                let grad = self.ball_gradient(level);
                let mut t = self.v3.speed;
                if grad != 0.0 {
                    t += 0.02 * grad;
                    t = t.min(BALL_SOFT_CAP + 1.1 * grad.abs());
                    if t < 0.0 {
                        // rolled to a stop uphill: back down the slope
                        self.dir = -self.dir;
                        t = -t;
                    }
                } else if t > BALL_SOFT_CAP {
                    let k = if self.on_ground { 0.03 } else { 0.009 };
                    t -= k * (t - BALL_SOFT_CAP);
                }
                self.v3.speed = t;
                let cur = self.vx.abs();
                let spd = if t >= cur { t } else { (cur - 0.3).max(t) };
                self.vx = spd * self.dir as f32;
                let landing_vy = self.vy;
                let was_ground = self.on_ground;
                self.collide(level);
                if self.wall {
                    self.mode = Mode::Dead;
                    return;
                }
                if self.on_ground && !was_ground && landing_vy < -0.4 {
                    let bounce = -landing_vy * (0.6 - 0.4 * grad.clamp(0.0, 1.0));
                    if bounce >= 0.4 {
                        self.vy = bounce;
                        self.on_ground = false;
                    }
                }
                if self.on_ground && grad == 0.0 && spd < BALL_REST {
                    self.phase = ball::REST;
                    self.vx = 0.0;
                }
            }
            _ => {
                // CarryWait: at rest; a slope starts it rolling
                self.vx = 0.0;
                self.v3_fall(ENEMY_GRAVITY, BALL_FALL);
                self.collide(level);
                if self.ball_gradient(level) != 0.0 {
                    self.phase = ball::ROLL;
                    self.v3.speed = 0.0;
                } else if self.ball_gradient_either(level) {
                    self.dir = -self.dir;
                    self.phase = ball::ROLL;
                    self.v3.speed = 0.0;
                }
            }
        }
    }

    /// On a slope whatever the direction (a resting ball rolls downhill).
    fn ball_gradient_either(&self, level: &Level) -> bool {
        level
            .slope_at(self.x)
            .is_some_and(|(surf, _)| self.on_ground && (self.y - surf).abs() <= 2.0)
    }

    /// A ball at rest (it can be kicked or, a snowball, carried).
    pub fn ball_resting(&self) -> bool {
        matches!(self.kind, Kind::SpikeBall { .. }) && self.phase == ball::REST
    }

    /// Kick or throw a ball along `dir` at `speed` with `vy` (a kicked or thrown snowball, a
    /// knocked one).
    pub fn ball_launch(&mut self, dir: i32, speed: f32, vy: f32) {
        self.phase = ball::ROLL;
        self.dir = dir;
        self.v3.speed = speed;
        self.vx = speed * dir as f32;
        self.vy = vy;
        self.on_ground = false;
    }

    // ------------------------------------------------------------ Pokey

    /// Pokey (78-pokey.md): a stack of segments walking slowly, swaying.
    fn step_pokey(&mut self, level: &Level) {
        if self.phase == 0 {
            let Kind::Pokey { height, .. } = self.kind else {
                return;
            };
            let n = (height as usize).clamp(1, POKEY_MAX);
            self.v3.segs = (0..n)
                .map(|i| {
                    let r = self.rand(121) as f32;
                    Seg {
                        phase: i as f32 * (25.0 + r),
                        ..Seg::default()
                    }
                })
                .collect();
            self.phase = 1;
        }
        let mut settling = false;
        for s in &mut self.v3.segs {
            s.phase = (s.phase + POKEY_SWAY_RATE) % 360.0;
            if s.drop > 0.0 || s.vy > 0.0 {
                settling = true;
                s.vy = (s.vy + ENEMY_GRAVITY).max(-4.5);
                s.drop += s.vy;
                if s.drop <= 0.0 {
                    s.drop = 0.0;
                    s.vy = 0.0;
                }
            }
        }
        // the stack waits while segments are falling back into place [obs state 7]
        let speed = if settling {
            0.0
        } else if self.wings {
            POKEY_WING_WALK
        } else {
            POKEY_WALK
        };
        self.vx = speed * self.dir as f32;
        self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
        if !self.on_ground {
            self.vx = self.vx.clamp(-0.5, 0.5);
        }
        self.collide(level);
        // winged (flag +0x523 bit 1), in Walk: each landing hops 2.5, 2.5, 4.7, … — a counter
        // set to 2 on Walk enter and when a side-wall contact ends (+0x658 bits 2/3, the bits
        // walkers test for walls) gives 4.7 at 0 (then back to 2) and 2.5 otherwise (−1)
        // [obs Walk exe 0x7101254790, enter 0x7101254540]. `count` 0 is the game's 2.
        let walled = self.wall;
        if self.wings {
            let was_settling = self.v3.flags & POKEY_SETTLING != 0;
            let left_wall = self.v3.flags & POKEY_WALLED != 0 && !walled;
            if (was_settling && !settling) || left_wall {
                self.v3.count = 0;
            }
            if !settling && self.on_ground {
                self.vy = POKEY_HOPS[self.v3.count as usize % 3];
                self.v3.count = (self.v3.count + 1) % 3;
                self.on_ground = false;
            }
        }
        self.v3.flags =
            (if settling { POKEY_SETTLING } else { 0 }) | (if walled { POKEY_WALLED } else { 0 });
        if self.turn_lock == 0 && (self.wall || self.v3_ledge(level, self.dir)) {
            self.turn();
        }
    }

    /// Centres of the live segments (bottom first): swaying ±1.5 px, odd ones the other way.
    pub fn pokey_segments(&self) -> Vec<(f32, f32)> {
        if self.v3.segs.is_empty() {
            // not set up yet (a course map draws actors as placed): the placed stack
            let n = match self.kind {
                Kind::Pokey { height, .. } => height.clamp(1, POKEY_MAX as u8) as usize,
                _ => 0,
            };
            return (0..n)
                .map(|i| (self.x, self.y + 16.0 * i as f32 + 8.0))
                .collect();
        }
        self.v3
            .segs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let side = if i % 2 == 1 { -1.0 } else { 1.0 };
                let dx = side * POKEY_SWAY * s.phase.to_radians().dsin();
                (self.x + dx, self.y + 16.0 * i as f32 + 8.0 + s.drop)
            })
            .collect()
    }

    /// Which live segment a point at height `y` touches (clamped to the stack).
    pub fn pokey_segment_at(&self, y: f32) -> usize {
        let n = self.v3.segs.len().max(1);
        (((y - self.y) / 16.0).floor().max(0.0) as usize).min(n - 1)
    }

    /// A hit on segment `k` that knocks it out: the head takes the whole Pokey with it, a body
    /// segment goes alone and the ones above hop down into place [obs 0x71012597c0,
    /// 0x7101257df0]. True when the whole Pokey was defeated.
    pub fn pokey_hit(&mut self, k: usize, from_x: f32) -> bool {
        let n = self.v3.segs.len();
        if n == 0 || k + 1 >= n {
            self.die_fall(from_x);
            return true;
        }
        self.v3.segs.remove(k);
        for (i, s) in self.v3.segs.iter_mut().enumerate().skip(k) {
            s.drop += 16.0;
            s.vy = 1.25 + 0.05 * i as f32;
        }
        false
    }

    // ------------------------------------------------------------ Mechakoopa

    /// The weapon origin. The game asks the robot's model for a joint named "shot" (0x71011d0620
    /// via the model's joint lookup 0x71011d8820) and, when the lookup fails, uses the actor's
    /// position. No robot model has that joint (M1 / M3 / MW / NSMBU `koopamecha_model`: body,
    /// parachute, wing1L, wing1R only), so the origin is the robot's feet [obs]; the controller
    /// then adds its weapon offset (Blasta 8 px ahead, big 16, 0x71011d17b0; Zappa none,
    /// 0x71011cea30). The beam keeps that origin (its per-frame joint lookup fails too).
    pub fn mecha_muzzle(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// The player is in the weapon's window: Blasta 0…288 px ahead and −160…159 px up; Zappa
    /// 0…380 ahead and −60…65 (big 80) [obs]; measured from the feet to the player's centre.
    fn mecha_in_range(&self, weapon: u8) -> bool {
        let dx = (self.target.0 - self.x) * self.dir as f32;
        let dy = self.target.1 + 12.0 - self.y;
        if weapon == 1 {
            (0.0..=288.0).contains(&dx) && (-160.0..160.0).contains(&dy)
        } else {
            let top = if self.big { 80.0 } else { 65.0 };
            (0.0..=380.0).contains(&dx) && dy >= -60.0 && dy <= top
        }
    }

    /// Mechakoopa (111-mechakoopa.md): walks toward the player, re-aiming every 60–80 frames;
    /// Blasta fires homing missiles, Zappa a laser; a hit knocks it down for 480 frames.
    fn step_mecha(&mut self, level: &Level, weapon: u8) -> Option<Actor> {
        let st = style_ix(level);
        if self.phase == 0 {
            self.phase = if self.wings { mecha::FLY } else { mecha::WALK };
            self.v3.clock = 60 + self.rand(21) as i32;
            self.v3.cool = 30 + self.rand(60) as i32;
        }
        let mut out = None;
        match self.phase {
            mecha::DOWN => {
                self.mecha_step_down(level);
                return None;
            }
            mecha::NIGHT_ST..=mecha::NIGHT_FALL => {
                self.mecha_night(level);
                return None;
            }
            mecha::REVIVAL => {
                self.timer -= 1;
                self.vx = 0.0;
                self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
                self.collide(level);
                if self.timer <= 0 {
                    self.dir = self.v3_toward();
                    self.phase = if self.wings { mecha::FLY } else { mecha::WALK };
                    self.v3.clock = 60 + self.rand(21) as i32;
                }
                return None;
            }
            _ => {}
        }
        let firing = matches!(self.v3.sub, mecha::SIGN | mecha::CHARGE | mecha::SHOT);
        if self.phase == mecha::FLY {
            // winged: no gravity, bobbing 0.15·sin φ (2°/f), patrolling ±48 px at 0.5 (accel
            // 0.01), still firing [obs]
            self.vy = 0.15 * (self.age as f32 * 2.0).to_radians().dsin();
            let t = if firing {
                0.0
            } else {
                MECHA_WALK * self.dir as f32
            };
            self.vx += (t - self.vx).clamp(-0.01, 0.01);
            self.collide(level);
            if self.wall || (self.x - self.home_x) * self.dir as f32 >= 48.0 {
                self.dir = -self.dir;
            }
        } else {
            self.v3.clock -= 1;
            let night = level.theme == 0 && level.night;
            if night && self.v3.clock <= 0 && self.phase == mecha::WALK && !firing {
                // night Ground: a hop instead of the re-aim, every 60–80 frames [obs]
                if self.on_ground {
                    self.phase = mecha::NIGHT_ST;
                    self.timer = 40;
                }
                self.v3.clock = 60 + self.rand(21) as i32;
            } else if self.v3.clock <= 0 && self.phase == mecha::WALK && !firing {
                self.v3.clock = 60 + self.rand(21) as i32;
                let want = self.v3_toward();
                if want != self.dir {
                    self.dir = want;
                    self.vx = -self.vx;
                    self.phase = mecha::TURN;
                    self.timer = 8;
                }
            }
            if self.phase == mecha::TURN {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = mecha::WALK;
                }
            }
            self.vx = if firing {
                0.0
            } else {
                MECHA_WALK * self.dir as f32
            };
            if !self.on_ground {
                self.vx = self.vx.clamp(-0.5, 0.5);
            }
            self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
            self.collide(level);
            let ledge = weapon != 0 && self.v3_ledge(level, self.dir);
            if (self.wall || ledge) && self.phase == mecha::WALK {
                self.dir = -self.dir; // Turn: flips at once, 8 frames [obs]
                self.phase = mecha::TURN;
                self.timer = 8;
                if ledge {
                    self.vx = 0.0;
                }
            }
        }
        if weapon != 0 {
            out = self.mecha_weapon(weapon, st);
        }
        out
    }

    fn mecha_weapon(&mut self, weapon: u8, st: usize) -> Option<Actor> {
        let w = (weapon - 1) as usize;
        match self.v3.sub {
            mecha::READY => {
                if self.v3.cool > 0 && self.v3_in_view() {
                    self.v3.cool -= 1;
                }
                if self.v3.cool <= 0 && self.mecha_in_range(weapon) {
                    self.v3.sub = mecha::SIGN;
                    self.v3.anim = 0;
                }
                None
            }
            mecha::SIGN => {
                self.v3.anim += 1;
                if self.v3.anim >= MECHA_SIGN_LEN[(st == 3) as usize] {
                    self.v3.sub = mecha::CHARGE;
                    self.v3.anim = 0;
                    if weapon == 2 {
                        // the laser (its telegraph) appears as the Charge starts [obs]
                        let (mx, my) = self.mecha_muzzle();
                        let mut l = Actor::new(Kind::MechaLaser, mx, my);
                        l.dir = self.dir;
                        l.big = self.big;
                        l.v3.owner = self.id;
                        return Some(l);
                    }
                }
                None
            }
            mecha::CHARGE => {
                self.v3.anim += 1;
                if self.v3.anim >= MECHA_CHARGE[w] {
                    self.v3.sub = mecha::SHOT;
                    self.v3.anim = 0;
                    if weapon == 1 {
                        let (mx, my) = self.mecha_muzzle();
                        let s = self.v3_scale();
                        let mut m =
                            Actor::new(Kind::MechaMissile, mx + 8.0 * s * self.dir as f32, my);
                        m.big = self.big;
                        m.dir = self.dir;
                        m.speed = 1.2;
                        m.heading = if self.dir > 0 { 0.0 } else { 180.0 };
                        m.v3.owner = self.id;
                        return Some(m);
                    }
                }
                None
            }
            mecha::SHOT => {
                self.v3.anim += 1;
                let len = if weapon == 1 {
                    MECHA_FIRE_WAIT
                } else {
                    ZAPPA_SHOT
                };
                if self.v3.anim >= len {
                    self.v3.sub = mecha::END;
                    self.v3.anim = 0;
                }
                None
            }
            _ => {
                self.v3.anim += 1;
                if self.v3.anim >= MECHA_FIRE_ED {
                    self.v3.sub = mecha::READY;
                    self.v3.cool = 180 + self.rand(30) as i32;
                }
                None
            }
        }
    }

    /// The night-Ground hop (111-mechakoopa.md "Wings, parachute, night") [obs]: JumpStart 40
    /// frames; Jump vy 0.8 with vx 0.25 rising to 1.5 by 0.015/f and gravity −0.008 until
    /// vy ≤ 0.4; Hover 50 frames (still once vy reaches 0); descent with x braking 0.03 and
    /// gravity −0.03 to the ground; then it faces the player and walks on.
    fn mecha_night(&mut self, level: &Level) {
        match self.phase {
            mecha::NIGHT_ST => {
                self.vx = 0.0;
                self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = mecha::NIGHT_JUMP;
                    self.vy = 0.8;
                    self.vx = 0.25 * self.dir as f32;
                    self.on_ground = false;
                }
            }
            mecha::NIGHT_JUMP => {
                let v = (self.vx.abs() + 0.015).min(1.5);
                self.vx = v * self.dir as f32;
                self.vy -= 0.008;
                if self.vy <= 0.4 {
                    self.phase = mecha::NIGHT_HOVER;
                    self.timer = 50;
                }
            }
            mecha::NIGHT_HOVER => {
                self.vy = (self.vy - 0.008).max(0.0);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = mecha::NIGHT_FALL;
                }
            }
            _ => {
                self.vx -= self.vx.clamp(-0.03, 0.03);
                self.v3_fall(-0.03, MAX_FALL);
            }
        }
        self.collide(level);
        if self.wall {
            self.vx = 0.0;
        }
        if self.phase == mecha::NIGHT_FALL && self.on_ground {
            self.dir = self.v3_toward();
            self.phase = mecha::WALK;
        }
    }

    /// Knock a Mechakoopa down (the EnemyDB "Damage" attacks): 480 frames, sliding away from
    /// `from_x` at `speed` with `vy`, braking by `decel` per frame (0 = none) [obs reaction
    /// table 0x71011cbad0]. Any weapon sequence is cancelled.
    pub fn mecha_knock(&mut self, from_x: f32, speed: f32, vy: f32, decel: f32) {
        let dir = if self.x < from_x { -1 } else { 1 };
        self.phase = mecha::DOWN;
        self.timer = MECHA_DOWN;
        self.dir = dir;
        self.vx = speed * dir as f32;
        if vy > 0.0 {
            self.vy = vy;
            self.on_ground = false;
        }
        self.v3.speed = decel;
        self.v3.sub = mecha::READY;
        self.v3.cool = self.v3.cool.max(30);
        self.v3.count = 1; // bouncing
    }

    pub fn mecha_down(&self) -> bool {
        matches!(self.kind, Kind::Mechakoopa { .. }) && self.phase == mecha::DOWN
    }

    pub fn mecha_firing(&self) -> u8 {
        self.v3.sub
    }

    /// Knocked down: slides and bounces to a stop, shakes below 61 frames, gets up at 0 (the
    /// timer is held while carried) [obs 0x71011cd300].
    fn mecha_step_down(&mut self, level: &Level) {
        if !self.held && self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer <= 0 {
            self.phase = mecha::REVIVAL;
            self.timer = MECHA_REVIVAL;
            self.vx = 0.0;
            return;
        }
        let target = if self.on_ground {
            0.0
        } else {
            self.vx.abs().min(0.5)
        };
        let decel = self.v3.speed;
        if decel > 0.0 {
            let v = self.vx.abs();
            let nv = if v > target {
                (v - decel).max(target)
            } else {
                v
            };
            self.vx = nv * self.vx.signum();
        }
        self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
        let landing = self.vy;
        let was_ground = self.on_ground;
        let before = self.vx;
        self.collide(level);
        if self.wall && before.abs() >= 0.4 {
            self.vx = -0.4 * before;
            self.dir = -self.dir;
        } else if self.wall {
            self.vx = 0.0;
        }
        if self.on_ground && !was_ground {
            self.vx *= 0.5; // every landing halves the slide [obs]
            if self.v3.count != 0 && -landing >= 1.0 {
                self.vy = -0.4 * landing;
                self.on_ground = false;
            } else if -landing < 0.4 {
                self.v3.count = 0;
            }
        }
        if self.on_ground && decel == 0.0 {
            self.vx = 0.0; // a block hit's pop has no slide [obs: no deceleration kind]
        }
    }

    pub fn mecha_shaking(&self) -> bool {
        self.mecha_down() && self.timer < MECHA_SIGN
    }

    /// Blasta's missile (111-mechakoopa.md "Missile"): flies through terrain, homing on the
    /// player in three phases, and explodes at frame 300 (the world sets off the blast).
    fn step_missile(&mut self) {
        self.timer += 1;
        let (px, py) = (self.target.0, self.target.1 + 12.0);
        let (cx, cy) = (self.x, self.y + self.half_w());
        let want = (py - cy).datan2(px - cx).to_degrees();
        let e = angle_diff(self.heading, want);
        let d = (px - cx).dhypot(py - cy);
        let (target_speed, accel, wanted, max_change) = match self.phase {
            1 => {
                // home: turn along a circle through the target (R = d/2 / sin|e|) [obs]
                let r = (d / 2.0) / e.abs().to_radians().dsin().max(1e-3);
                let w = (self.speed / r.max(1.0)).to_degrees().min(0.7) * e.signum();
                (1.2, 0.01, w, 0.05)
            }
            2 => (0.5, 0.007, e.signum() * e.abs().min(0.3), 0.03),
            _ => (0.5, 0.01, e.signum() * e.abs().min(1.5), 0.03),
        };
        self.speed += (target_speed - self.speed).clamp(-accel, accel);
        self.v3.turn += (wanted - self.v3.turn).clamp(-max_change, max_change);
        self.heading = (self.heading + self.v3.turn).rem_euclid(360.0);
        match self.phase {
            0 if e.abs() <= 40.0 => self.phase = 1,
            1 if e.abs() > 45.0 => {
                self.phase = 2;
                self.v3.clock = 40;
            }
            2 => {
                self.v3.clock -= 1;
                if self.v3.clock <= 0 {
                    self.phase = 0;
                }
            }
            _ => {}
        }
        let h = self.heading.to_radians();
        self.vx = self.speed * h.dcos();
        self.vy = self.speed * h.dsin();
        self.x += self.vx;
        self.y += self.vy;
        self.dir = if self.vx < 0.0 { -1 } else { 1 };
    }

    pub fn missile_due(&self) -> bool {
        self.kind == Kind::MechaMissile && self.timer >= MISSILE_LIFE
    }

    pub fn missile_blinking(&self) -> bool {
        self.kind == Kind::MechaMissile && self.timer >= 50
    }

    /// Zappa's laser: a telegraph line, then the beam growing out of the muzzle to its full
    /// length or the first solid tile. The world keeps it on the robot's muzzle.
    fn step_laser(&mut self, level: &Level) {
        self.timer += 1;
        if self.timer > LASER_SIGN + LASER_SHOT + LASER_FADE {
            self.mode = Mode::Dead;
            return;
        }
        // the reach to the first solid tile along the beam (the telegraph shows it too): the
        // Shot's terrain ray runs level with the origin [obs 0x7100de6740]
        let mut reach = 0.0;
        while reach < LASER_MAX {
            let x = self.x + (reach + 4.0) * self.dir as f32;
            if level.cell_at(x, self.y).is_solid() {
                break;
            }
            reach += 4.0;
        }
        if self.timer <= LASER_SIGN {
            self.v3.len = 0.0;
            self.v3.speed = reach; // the telegraph's length
        } else if self.timer <= LASER_SIGN + LASER_SHOT {
            let start = if self.big { -6.0 } else { -3.0 };
            let grown = start + LASER_GROW * (self.timer - LASER_SIGN) as f32;
            self.v3.len = grown.min(reach).max(0.0);
        }
    }

    /// The beam's damage box while it fires (muzzle to tip, half height 4, big 8, centred 2 px,
    /// big 4, below the muzzle) [obs].
    pub fn laser_box(&self) -> Option<(f32, f32, f32, f32)> {
        if self.kind != Kind::MechaLaser
            || self.timer <= LASER_SIGN
            || self.timer > LASER_SIGN + LASER_SHOT
            || self.v3.len <= 0.0
        {
            return None;
        }
        let s = self.v3_scale();
        let cy = self.y - 2.0 * s;
        let (x0, x1) = if self.dir > 0 {
            (self.x, self.x + self.v3.len)
        } else {
            (self.x - self.v3.len, self.x)
        };
        Some((x0, cy - 4.0 * s, x1, cy + 4.0 * s))
    }

    /// A tail, cape or cat scratch hitting a 2.0 / 3.0 part at height `y` [obs reaction
    /// rows]: Spike DieFall; the iron ball turns to the hit's direction at 1.2 px/f with vy 3.0,
    /// the snowball at 0.5 with vy 3.0; a Pokey loses the segment; a Mechakoopa is knocked down
    /// (0.5, vy 5.0, braking 0.0625); a Koopaling loses 1 HP.
    pub fn v3_tail_hit(&mut self, from_x: f32, y: f32) {
        let dir = if self.x < from_x { -1 } else { 1 };
        match self.kind {
            Kind::SpikeBall { snow } => {
                let speed = if snow { 0.5 } else { 1.2 };
                self.ball_launch(dir, speed, 3.0);
            }
            Kind::Pokey { .. } => {
                let k = self.pokey_segment_at(y + 8.0);
                self.pokey_hit(k, from_x);
            }
            Kind::Mechakoopa { .. } => self.mecha_knock(from_x, 0.5, 5.0, 0.0625),
            Kind::Koopaling { .. } => {
                self.koopaling_damage(from_x, false);
            }
            _ => self.die_fall(from_x),
        }
    }

    // ------------------------------------------------------------ looks

    /// Where the model's origin sits above the actor's feet: the balls, the missile and the
    /// Koopalings' shots are centred models; SMB3's Mechakoopa model is centred too [obs model
    /// quads].
    pub fn v3_draw_dy(&self) -> f32 {
        match self.kind {
            Kind::SpikeBall { .. } | Kind::MechaMissile | Kind::KoopalingShot { .. } => {
                self.height() / 2.0
            }
            Kind::Mechakoopa { .. } if self.v3.style == 1 => 16.0 * self.v3_scale(),
            _ => 0.0,
        }
    }

    /// Model and animation frame (`M1_` names; the front ends remap them per style).
    pub fn v3_look(&self) -> Option<V3Look> {
        let face = self.dir > 0;
        let t = self.age as i32;
        Some(match self.kind {
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => return self.koopaling_look(),
            Kind::Spike => {
                let tex = match self.phase {
                    spike::LIFT => anim_key(
                        &[
                            (0, "throw_st.0"),
                            (10, "throw_st.1"),
                            (25, "throw_st.2"),
                            (35, "throw_st.3"),
                        ],
                        35,
                        self.v3.anim,
                        false,
                    ),
                    spike::THROW => "throw_ed.0",
                    _ => anim_key(&[(0, "wait.0"), (10, "wait.1")], 20, t, true),
                };
                ("M1_Enemy_gabon", tex, 16.0, 16.0, face)
            }
            Kind::SpikeBall { snow } => (
                if snow {
                    "M1_Enemy_gabonballsnow"
                } else {
                    "M1_Enemy_gabonball"
                },
                "gabonball_model",
                16.0,
                16.0,
                face,
            ),
            // the stack is drawn segment by segment (world/v3.rs)
            Kind::Pokey { .. } | Kind::MechaLaser | Kind::OnOffTrampoline { .. } => return None,
            Kind::Mechakoopa { weapon } => {
                let model = match weapon {
                    1 => "M1_Enemy_koopamecha_missile",
                    2 => "M1_Enemy_koopamecha_laser",
                    _ => "M1_Enemy_koopamecha",
                };
                let tex = match self.phase {
                    mecha::DOWN if self.mecha_shaking() => {
                        anim_key(&[(0, "damage.2"), (4, "damage.1")], 8, t, true)
                    }
                    mecha::DOWN => "damage.2",
                    mecha::REVIVAL => anim_key(
                        &[(0, "damage.2"), (10, "damage.1"), (14, "damage.0")],
                        18,
                        MECHA_REVIVAL - self.timer,
                        false,
                    ),
                    _ if weapon != 0 && self.v3.sub != mecha::READY => {
                        let n = self.v3.anim;
                        match self.v3.sub {
                            mecha::SIGN => anim_key(
                                &[(0, "fire.0"), (5, "fire.1"), (30, "fire.2")],
                                30,
                                n,
                                false,
                            ),
                            mecha::END => "fire.0",
                            _ => "fire.2",
                        }
                    }
                    mecha::FLY => anim_key(&[(0, "fly.0"), (6, "fly.1")], 12, t, true),
                    _ => anim_key(&[(0, "walk.0"), (6, "walk.1")], 12, t, true),
                };
                (model, tex, 32.0, 32.0, face)
            }
            Kind::MechaMissile => {
                if self.missile_blinking() && (t / 4) % 2 == 0 {
                    return None;
                }
                ("M1_Enemy_koopamecha_missile", "missile.0", 16.0, 16.0, face)
            }
            _ => return None,
        })
    }
}
