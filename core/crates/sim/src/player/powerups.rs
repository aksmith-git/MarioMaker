//! The power-ups of SMM2's 2.0 / 3.0 updates, player side: the second style power-up (part 81:
//! SMB2 Mushroom, Frog Suit, Power Balloon, Super Acorn, Boomerang Flower), the SMB1 Master
//! Sword (Link) and the 3D World wearables (parts 127–131). Written from the v3.0.3 specs
//! (research/specs/objects/81-style-powerup-2.md, 127-headdress-items.md); every number is the
//! spec's [obs] value unless marked [inf] (chosen where the binary did not settle it).
//!
//! The player's own states live in `Player::pu`. Two hooks in `Player::step` call in here:
//! `pu_before` (after the input edges; it may take the whole frame, returning None) and
//! `pu_after` (after the normal ground / air physics, before the move). The world reads the
//! requests (`arrow`, `bomb`, `boomerang`, `cannon`, …) and the attack boxes each frame
//! (world/powerups.rs).

use super::*;
use crate::fmath::DetMath;

// ------------------------------------------------------------------ Link (Master Sword)

/// SwordAttack: the 8-frame "attack" animation; the tip probes the blocks on state frames 3–7
/// at (x + 20·dir, y + 8) [obs 0x71015cdc50].
pub const SLASH_FRAMES: i32 = 8;
pub const SWORD_REACH: f32 = 20.0;
/// SwordDash: "attack_dash_start" (12 frames) with the 60-frame wind-up, then 3.0 px/f (1.5 in
/// water); a wall recoils with vy 3.0 (1.5 on the ground in water, 0 in the air) and −0.5 × the
/// dash speed [obs 0x71015ce130].
pub const DASH_WINDUP: i32 = 60;
pub const DASH_SPEED: f32 = 3.0;
/// ArrowShoot: the 24-frame "charge_start" draw; moving faster than 1.2 px/f brakes [obs].
pub const BOW_DRAW: i32 = 24;
pub const BOW_SPEED: f32 = 1.2;
/// HipAttack for Link: hangs for "attack_under_start" (12 frames), a 5-frame pause, then drops
/// from −6.0 (−4.2 in water) with gravity −0.34, steering ±0.3; landing waits 1 + 10 frames
/// [obs 0x71015c6540 / 0x71015c6790 / 0x71015c6ca0].
pub const THRUST_HANG: i32 = 12;
pub const THRUST_PAUSE: i32 = 5;
pub const THRUST_VY: f32 = -6.0;

/// Link's moves (states 137–140, 19/20).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    None,
    /// SwordAttack: frames into the slash.
    Slash(i32),
    /// SwordDash: phase 0 the opening slash (`t` frames in), 1 the wind-up (`t` frames left),
    /// 2 the dash.
    Dash {
        phase: u8,
        t: i32,
    },
    /// ShieldGuard (Down on the ground).
    Shield,
    /// ArrowShoot: frames drawn; the button was let go before the draw finished.
    Bow {
        t: i32,
        released: bool,
    },
    /// HipAttack: phase 0 the hang, 1 the pause (`t` frames in / left), 2 the drop, 3 the
    /// landing wait (`t` frames left).
    Thrust {
        phase: u8,
        t: i32,
    },
}

/// What a sword box hits with (the enemy picks its reaction column by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blade {
    Slash,
    Dash,
    Under,
}

// ------------------------------------------------------------------ SMB2 Mushroom

/// Crouch charge: 60 frames in SMB1 (30 in the other styles) → a crouch jump of vy 4.1 with the
/// held-jump gravity for the whole rise [obs 0x71015a1040, 0x71015e4db0].
pub const CHARGE_FRAMES: i32 = 60;
pub const CHARGE_VY: f32 = 4.1;
/// LiftUp: 3 frames before the object comes up (23 for big ones), then the carry height eases
/// from the start offset to 25 (15 crouching) over 12 frames; crouching is locked for 10 frames
/// after [obs 0x710159af60 / 0x710159b3a0].
pub const LIFT_DELAY: i32 = 3;
pub const LIFT_DELAY_BIG: i32 = 23;
pub const LIFT_RISE: i32 = 12;
pub const CARRY_HEIGHT: f32 = 25.0;
pub const CARRY_HEIGHT_CROUCH: f32 = 15.0;

// ------------------------------------------------------------------ Frog Suit

/// Every Frog ground jump takes the top speed bonus: 3.628 + 0.3 [obs 0x71015c8330].
pub const FROG_JUMP: f32 = 3.928;
/// FrogWalk: a 10-frame wait, then 20-frame hops [obs 0x71015c3e00 / 0x71015c3ef0].
pub const FROG_WAIT: i32 = 10;
pub const FROG_HOP: i32 = 20;
/// FrogSwim keeps the frog this far under the surface [obs 0x71015c42a0].
pub const FROG_SURFACE: f32 = 17.0;

// ------------------------------------------------------------------ Power Balloon

/// Sensor pose 8: feet ±4 at y 0, head ±4 at 15.9, walls ±7.9 for y 4…12 [obs 0x7101592480].
pub const BALLOON: Sensors = Sensors {
    foot_x: 4.0,
    head_x: 4.0,
    head_y: 15.9,
    wall_y0: 4.0,
    wall_y1: 12.0,
    wall_x: 7.9,
};
/// The jet lockout after a turned-away bounce, a head bump or a bounce off something [obs].
pub const BALLOON_LOCK: i32 = 40;

// ------------------------------------------------------------------ Super Acorn

/// Flying (the boost): 15 frames if faster than 2.0 px/f, else 20 [obs 0x71015c25b0].
pub const BOOST_FRAMES: (i32, i32) = (15, 20);
/// FlyingWallStick holds 90 frames, then the wall slide [obs 0x71015c3ae0].
pub const CLING_FRAMES: i32 = 90;
/// The squirrel's ground spin jump [obs 0x71015c23d0].
pub const SQUIRREL_SPIN_VY: f32 = 4.15;

/// The squirrel's air states (51 Flying, 52 FlyingSlowFall, 53 FlyingWallStick).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Squirrel {
    None,
    /// The boost: phase 0 rising (`t` frames of the phase timer left), 1 coasting.
    Flying {
        phase: u8,
        t: i32,
    },
    /// Mode 0 glide (jump held, boost unused), 1 flutter (boost used).
    Glide {
        mode: u8,
    },
    /// On a wall (`side` −1 left / +1 right), `t` frames left.
    Cling {
        t: i32,
        side: i32,
    },
}

// ------------------------------------------------------------------ Boomerang Flower

/// Fire state: the boomerang leaves on the first frame the 22-frame "Boomerang" animation is
/// past frame 4 [obs 0x71015c1940].
pub const BOOMERANG_ANIM: i32 = 22;

// ------------------------------------------------------------------ wearables

/// What the player wears on the head (player +0x4bc: 0 cPropel … 4 cKiller) [obs].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Headdress {
    /// Propeller Box (128).
    Propeller,
    /// Cannon Box (127).
    Cannon,
    /// Goomba Mask (129).
    Goomba,
    /// Red POW Box (131).
    RedPow,
    /// Bullet Bill Mask (130).
    Bullet,
}

impl Headdress {
    /// The kind number (+0x4bc) and the part each one comes from.
    pub fn from_kind(k: u8) -> Option<Self> {
        Some(match k {
            0 => Headdress::Propeller,
            1 => Headdress::Cannon,
            2 => Headdress::Goomba,
            3 => Headdress::RedPow,
            4 => Headdress::Bullet,
            _ => return None,
        })
    }
}

/// Cannon Box: 30 frames between shots; holding the button 60 frames charges a second shot
/// fired on release [obs 0x71015c5870].
pub const CANNON_COOLDOWN: i32 = 30;
pub const CANNON_CHARGE: i32 = 60;
/// Propeller Box: three flights per air time, each a 15-frame rise toward 2.5 px/f at +1.0
/// [obs 0x71015d5160 / 0x71015d54e0].
pub const BOX_FLIGHTS: u8 = 3;
pub const BOX_RISE: i32 = 15;
/// Bullet Bill Mask: 300 frames of dash per air time at 3.0 px/f (1.5 in water) [obs].
pub const KILLER_TIME: i32 = 300;
/// Red POW Box: three head bumps [obs 0x71015c5270].
pub const RED_POW_USES: u8 = 3;

// ------------------------------------------------------------------ sound cues

pub const CUE_SLASH: u32 = 1 << 0;
pub const CUE_DASH_WALL: u32 = 1 << 1;
pub const CUE_BOW: u32 = 1 << 2;
pub const CUE_CHARGE: u32 = 1 << 3;
pub const CUE_BALLOON_WALL: u32 = 1 << 4;
pub const CUE_BOX_SPIN: u32 = 1 << 5;
pub const CUE_KILLER_BREAK: u32 = 1 << 6;
pub const CUE_THRUST: u32 = 1 << 7;
pub const CUE_FROG_SPLASH: u32 = 1 << 8;
pub const CUE_SPIN_ONCE: u32 = 1 << 9;

/// The player's state for the update power-ups.
#[derive(Clone, Debug, PartialEq)]
pub struct PowerUps {
    /// Buttons of the last frame (this module's own edge detection).
    prev: u32,
    /// The form last frame (a change resets the form's states).
    form: Form,
    /// On the ground at the start of the previous frame / of this one.
    was_ground: bool,
    seen_ground: bool,
    /// Sensed by the world before the step: the water surface the player is in (y), a
    /// boomerang of this player is out.
    pub water_top: Option<f32>,
    pub boomerang_out: bool,
    /// Sound cues of this frame (`CUE_*`).
    pub cues: u32,

    // Link
    pub link: Link,
    /// Bow aim: 0 straight, 1 up, 2 down.
    pub aim: u8,
    /// Requests for the world this frame: fire an arrow (aim), pull a bomb out.
    pub arrow: Option<u8>,
    pub bomb: bool,
    /// The down-thrust landed this frame.
    pub thrust_land: bool,

    // SMB2 Mushroom
    pub charge: i32,
    force_held: bool,
    /// LiftUp: frames in the state (0 = not lifting), the frame the object comes up, its start
    /// offset; the carried object's height above the feet.
    pub lift: i32,
    pub lift_delay: i32,
    pub lift_from: f32,
    pub carry_h: f32,
    /// Request: pick up what is under the feet (Down + action on the ground).
    pub pick: bool,
    crouch_lock: i32,

    // Frog Suit
    frog_walk: bool,
    pub hop: bool,
    hop_t: i32,
    swim_buf: i32,
    push_up: i32,
    push_down: i32,
    leap: bool,

    // Power Balloon
    /// Tilt in degrees (0 upright, −90 = pointing right); the jet; its lockout.
    pub tilt: f32,
    pub jet: bool,
    jet_lock: i32,

    // Super Acorn
    boost: bool,
    pub sq: Squirrel,
    mag: f32,
    base: f32,
    ang: f32,
    glide_cap: f32,
    cling_lock: i32,
    recling: bool,
    spin_edge: bool,

    // Boomerang Flower
    /// Frames into the throw (0 = none); request: the boomerang leaves now.
    pub throw_t: i32,
    pub boomerang: bool,

    // wearables
    pub headdress: Option<Headdress>,
    cannon_cd: i32,
    pub cannon_charge: i32,
    /// Request: fire a Cannon Box shot (charged?).
    pub cannon: Option<bool>,
    pub flights: u8,
    /// Propeller Box state: frames in it (0 = not flying), rise frames left, falling.
    pub prop: i32,
    prop_rise: i32,
    pub prop_fall: bool,
    prop_first: bool,
    spin_once: bool,
    spin_lock: i32,
    pub killer_time: i32,
    pub killer: bool,
    pub killer_lock: i32,
    /// Request: the mask broke against a wall (the world hits the blocks in front).
    pub killer_break: bool,
    pub red_uses: u8,
}

impl Default for PowerUps {
    fn default() -> Self {
        Self {
            prev: 0,
            form: Form::Small,
            was_ground: true,
            seen_ground: true,
            water_top: None,
            boomerang_out: false,
            cues: 0,
            link: Link::None,
            aim: 0,
            arrow: None,
            bomb: false,
            thrust_land: false,
            charge: 0,
            force_held: false,
            lift: 0,
            lift_delay: 0,
            lift_from: 0.0,
            carry_h: CARRY_HEIGHT,
            pick: false,
            crouch_lock: 0,
            frog_walk: false,
            hop: false,
            hop_t: 0,
            swim_buf: 0,
            push_up: 0,
            push_down: 0,
            leap: false,
            tilt: 0.0,
            jet: false,
            jet_lock: 0,
            boost: true,
            sq: Squirrel::None,
            mag: 0.0,
            base: 0.0,
            ang: 0.0,
            glide_cap: -2.0,
            cling_lock: 0,
            recling: false,
            spin_edge: false,
            throw_t: 0,
            boomerang: false,
            headdress: None,
            cannon_cd: 0,
            cannon_charge: 0,
            cannon: None,
            flights: 0,
            prop: 0,
            prop_rise: 0,
            prop_fall: false,
            prop_first: false,
            spin_once: false,
            spin_lock: 0,
            killer_time: KILLER_TIME,
            killer: false,
            killer_lock: 0,
            killer_break: false,
            red_uses: 0,
        }
    }
}

impl PowerUps {
    /// The form changed: the old form's states end.
    fn reset_form(&mut self) {
        self.link = Link::None;
        self.charge = 0;
        self.force_held = false;
        self.lift = 0;
        self.frog_walk = false;
        self.hop = false;
        self.leap = false;
        self.tilt = 0.0;
        self.jet = false;
        self.jet_lock = 0;
        self.sq = Squirrel::None;
        self.throw_t = 0;
    }

    /// Put on a wearable (0x71015c5270): a new one replaces the old; per-kind refills.
    pub fn wear(&mut self, h: Headdress) {
        self.headdress = Some(h);
        self.prop = 0;
        self.killer = false;
        match h {
            Headdress::Propeller => self.flights = 0,
            Headdress::RedPow => self.red_uses = RED_POW_USES,
            Headdress::Bullet => self.killer_time = KILLER_TIME,
            Headdress::Cannon => {
                self.cannon_cd = 0;
                self.cannon_charge = 0;
            }
            Headdress::Goomba => {}
        }
    }

    /// Take the wearable off (no hit: the world's own removals).
    pub fn take_off(&mut self) -> Option<Headdress> {
        self.prop = 0;
        self.killer = false;
        self.cannon_charge = 0;
        self.headdress.take()
    }

    /// The attack box of Link's sword this frame, if one is out: (x0, y0, x1, y1), blade.
    pub fn sword_box(&self, x: f32, y: f32, facing: i32) -> Option<((f32, f32, f32, f32), Blade)> {
        let d = facing as f32;
        let around = |cx: f32, cy: f32, hx: f32, hy: f32| (cx - hx, cy - hy, cx + hx, cy + hy);
        match self.link {
            // the slash box: centre (±11.5, 7.5), half 11.5 × 7.5 [obs 0x71015cdc50]
            Link::Slash(_) | Link::Dash { phase: 0, .. } => {
                Some((around(x + 11.5 * d, y + 7.5, 11.5, 7.5), Blade::Slash))
            }
            Link::Dash { phase: 1, .. } => {
                Some((around(x + 11.5 * d, y + 7.5, 11.5, 7.5), Blade::Slash))
            }
            // the dash box: centre (±7.4, 7.5), half 7.4 × 7.5 [obs 0x71015ce130]
            Link::Dash { .. } => Some((around(x + 7.4 * d, y + 7.5, 7.4, 7.5), Blade::Dash)),
            // the down-thrust blade at the feet: half 5 × 8 [obs 0x71015c6790]
            Link::Thrust { phase: 2, .. } => Some((around(x, y, 5.0, 8.0), Blade::Under)),
            _ => None,
        }
    }

    /// The shield's box while guarding: centre (±4, 7), half 4 × 7 [obs 0x71015ce830].
    pub fn shield_box(&self, x: f32, y: f32, facing: i32) -> Option<(f32, f32, f32, f32)> {
        (self.link == Link::Shield).then(|| {
            let cx = x + 4.0 * facing as f32;
            (cx - 4.0, y, cx + 4.0, y + 14.0)
        })
    }

    /// The sword tip probes the blocks this frame (slash frames 3–7; every dash frame) [obs].
    pub fn sword_probe(&self) -> bool {
        match self.link {
            Link::Slash(t) => (3..=7).contains(&t),
            Link::Dash { .. } => true,
            _ => false,
        }
    }

    /// The Bullet Bill dash is on (the mask breaks instead of the player being hurt).
    pub fn dashing_bill(&self) -> bool {
        self.killer
    }
}

fn unit(dx: f32, dy: f32) -> (f32, f32) {
    let l = dx.dhypot(dy);
    if l < 1e-6 {
        (0.0, 0.0)
    } else {
        (dx / l, dy / l)
    }
}

/// Move (vx, vy) toward (tx, ty) by at most `step` (a vector step).
fn approach2(v: (f32, f32), t: (f32, f32), step: f32) -> (f32, f32) {
    let (dx, dy) = (t.0 - v.0, t.1 - v.1);
    let l = dx.dhypot(dy);
    if l <= step {
        t
    } else {
        (v.0 + dx / l * step, v.1 + dy / l * step)
    }
}

/// Turn angle `a` toward `t` (degrees) by at most `step`, the short way round.
fn turn_toward(a: f32, t: f32, step: f32) -> f32 {
    let mut d = (t - a) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    let mut r = a + d.clamp(-step, step);
    if r > 180.0 {
        r -= 360.0;
    } else if r <= -180.0 {
        r += 360.0;
    }
    r
}

impl Player {
    /// Hook before the movement (Player::step): the update power-ups' input and the states
    /// that take the whole frame (they move the player themselves and return None). Otherwise
    /// the (possibly changed) input for the normal movement.
    pub(super) fn pu_before(
        &mut self,
        inp: Input,
        pressed: bool,
        spin_pressed: bool,
        level: &Level,
    ) -> Option<Input> {
        let b = inp.buttons;
        let edge = b & !self.pu.prev;
        self.pu.prev = b;
        self.pu.cues = 0;
        self.pu.arrow = None;
        self.pu.bomb = false;
        self.pu.pick = false;
        self.pu.boomerang = false;
        self.pu.cannon = None;
        self.pu.thrust_land = false;
        self.pu.killer_break = false;
        self.pu.spin_edge = spin_pressed;
        self.pu.was_ground = std::mem::replace(&mut self.pu.seen_ground, self.on_ground);
        if self.form != self.pu.form {
            self.pu.reset_form();
            self.pu.form = self.form;
        }
        // the air-time reset (0x710158ed30): on the ground, under water, on a vine [obs]
        if self.on_ground || self.in_water || self.climbing.is_some() {
            self.pu.boost = true;
            self.pu.flights = 0;
            self.pu.killer_time = KILLER_TIME;
            self.pu.recling = false;
            self.pu.spin_once = false;
        }
        for t in [
            &mut self.pu.crouch_lock,
            &mut self.pu.jet_lock,
            &mut self.pu.cling_lock,
            &mut self.pu.cannon_cd,
            &mut self.pu.spin_lock,
            &mut self.pu.killer_lock,
        ] {
            if *t > 0 {
                *t -= 1;
            }
        }
        // a headdress turns off the form projectiles (Fire / FireSwim, throw gate 0x71015c0ca0)
        if self.pu.headdress.is_some() {
            self.throw = false;
        }
        let run_pressed = edge & input::RUN != 0;
        let inp = if self.style == Style::W3 && self.pu.headdress.is_some() {
            self.headdress_before(inp, pressed, spin_pressed, run_pressed, level)?
        } else {
            inp
        };
        self.form_before(inp, pressed, spin_pressed, run_pressed, edge, level)
    }

    fn form_before(
        &mut self,
        inp: Input,
        pressed: bool,
        spin_pressed: bool,
        run_pressed: bool,
        edge: u32,
        level: &Level,
    ) -> Option<Input> {
        match self.form {
            Form::Link => self.link_before(inp, pressed, spin_pressed, run_pressed, edge, level),
            Form::Frog => self.frog_before(inp, pressed, level),
            Form::Balloon => {
                self.balloon(inp, level);
                None
            }
            Form::Squirrel => self.squirrel_before(inp, pressed, spin_pressed, level),
            Form::Smb2 => self.usa_before(inp, run_pressed, level),
            Form::Boomerang => {
                self.boomerang_before(run_pressed);
                Some(inp)
            }
            _ => Some(inp),
        }
    }

    /// Hook on a ground jump's launch speed (`vy` as the normal rules give it).
    pub(super) fn pu_launch(&mut self, vy: f32) -> f32 {
        match self.form {
            Form::Frog => FROG_JUMP,
            Form::Smb2 if self.crouch && self.pu.charge >= CHARGE_FRAMES => {
                self.pu.charge = 0;
                self.pu.force_held = true;
                CHARGE_VY
            }
            _ => vy,
        }
    }

    /// Hook on a bounce (enemy stomp, spring): the balloon only drifts — the given speed is
    /// clamped to ±2.0 (±1.5 in liquid), the jet ends and is locked 40 frames
    /// (0x71015c7a00) [obs]. True when it replaced the bounce.
    pub(super) fn pu_bounce(&mut self, vy: f32) -> bool {
        if self.form != Form::Balloon {
            return false;
        }
        let cap = if self.in_water { 1.5 } else { 2.0 };
        self.vy = vy.clamp(-cap, cap);
        self.pu.jet = false;
        self.pu.jet_lock = BALLOON_LOCK;
        true
    }

    /// Hook on a hit, after the helmet: a wearable absorbs it (FUN_71015a4990 →
    /// HeadDressOff) [obs]. A dashing Bullet Bill Mask breaks instead (KillerBreak:
    /// invincibility 127) [obs 0x71015c55f0].
    pub(super) fn pu_absorb(&mut self) -> bool {
        let Some(h) = self.pu.take_off() else {
            return false;
        };
        if h == Headdress::Bullet && self.pu.killer {
            self.pu.cues |= CUE_KILLER_BREAK;
        }
        // no invincibility is written when a wearable is lost to a hit (the shoe path of the
        // same function writes 127) [obs 0x71015a4990]; the frames after rely on the enemy's own
        // contact cooldown [?] — the engine has none, so the Bullet Bill break's 127 is used for
        // every loss [inf]
        self.invuln = INVULN_DAMAGE;
        true
    }

    /// Terrain sensors of the update forms (None: the usual ones).
    pub(super) fn pu_sensors(&self) -> Option<&'static Sensors> {
        (self.form == Form::Balloon).then_some(&BALLOON)
    }

    /// Hook after the normal movement physics, before the move.
    pub(super) fn pu_after(
        &mut self,
        inp: Input,
        jump: bool,
        vy_before: f32,
        vx_before: f32,
        level: &Level,
    ) {
        let _ = level;
        let dir = inp.dir_x();
        match self.form {
            Form::Frog => self.frog_after(inp, vx_before),
            Form::Link => {
                if let Link::Bow { .. } = self.pu.link {
                    self.bow_movement(inp, vx_before);
                }
            }
            Form::Smb2 => {
                // the charged jump keeps the held-jump gravity while rising
                if self.pu.force_held {
                    if self.on_ground || self.vy <= 0.0 || self.in_water {
                        self.pu.force_held = false;
                    } else if !jump && vy_before > 0.0 {
                        let g = GRAVITY_HELD[vy_band(vy_before)];
                        self.vy = (vy_before + g).max(FALL_CAP);
                    }
                }
            }
            // the squirrel's ground spin jumps higher
            Form::Squirrel
                if self.pu.spin_edge && self.pu.seen_ground && self.spinning && self.vy > 2.0 =>
            {
                self.vy += SQUIRREL_SPIN_VY - 3.228;
            }
            _ => {}
        }
        // SMB1 keeps the take-off facing in the air; Link and the SMB2 Mushroom turn freely
        // (0x71015d4670) [obs]
        if matches!(self.form, Form::Link | Form::Smb2) && !self.on_ground && dir != 0 {
            self.facing = dir;
        }
    }

    /// The end of a frame a power-up state took: move, collide, the frame's bookkeeping.
    fn pu_finish(&mut self, inp: Input, level: &Level) {
        let was_ground = self.on_ground;
        self.was_in_water = self.in_water;
        self.prev_down = inp.held(input::DOWN);
        self.x += self.wind;
        let s = self.sensors();
        self.move_and_collide(level, s);
        if self.on_ground && !was_ground {
            self.cues |= CUE_LAND;
        }
        self.ceiling = self.head_hit.is_some() || self.head_solid.is_some();
        if self.on_ground {
            self.stomp_chain = 0;
            if self.vy <= 0.0 {
                self.spinning = false;
            }
        }
        if self.y < -2.0 * TILE {
            self.kill();
        }
    }

    /// Gravity of the normal air bands (§3.2).
    fn band_gravity(&self, jump: bool) -> f32 {
        if jump {
            GRAVITY_HELD[vy_band(self.vy)]
        } else {
            GRAVITY_RELEASED[vy_band(self.vy)]
        }
    }

    /// The standard stop deceleration (the crouch row; ice row on ice) [obs 0x71023d55c0].
    fn stop_decel(&self) -> f32 {
        let t = self.thresholds();
        let table = if self.on_ice {
            &ICE_CROUCH
        } else {
            &GROUND_CROUCH
        };
        table[band(self.vx, self.facing, t)]
    }

    // -------------------------------------------------------------- Link

    #[allow(clippy::too_many_arguments)]
    fn link_before(
        &mut self,
        inp: Input,
        pressed: bool,
        spin_pressed: bool,
        run_pressed: bool,
        edge: u32,
        level: &Level,
    ) -> Option<Input> {
        let dir = inp.dir_x();
        let down = inp.held(input::DOWN);
        let up = inp.held(input::UP);
        let free = self.held.is_none() && self.shoe.is_none();
        // starts: the attack handler (0x71015e57a0), the bomb pull (0x7101598090), the bow
        // (0x71015e5c10), the shield (0x71015a0840), the down-thrust (0x71015e51b0)
        if matches!(self.pu.link, Link::None | Link::Shield) {
            if run_pressed && self.held.is_none() && up && dir == 0 && self.on_ground {
                if self.vy <= 0.0 && free {
                    self.pu.bomb = true;
                    self.pu.link = Link::None;
                }
            } else if run_pressed && free {
                if self.on_ground && down && !pressed {
                    self.pu.link = Link::Dash { phase: 0, t: 0 };
                } else {
                    self.pu.link = Link::Slash(0);
                }
                self.pu.cues |= CUE_SLASH;
            } else if spin_pressed && free {
                self.pu.link = Link::Bow {
                    t: 0,
                    released: false,
                };
            } else if self.pu.link == Link::None && self.on_ground && down && free && !self.in_water
            {
                self.pu.link = Link::Shield;
            } else if self.pu.link == Link::None
                && !self.on_ground
                && edge & input::DOWN != 0
                && free
                && self.yoshi.is_none()
            {
                self.pu.link = Link::Thrust { phase: 0, t: 0 };
                self.pu.cues |= CUE_THRUST;
            }
        }
        match self.pu.link {
            Link::None => Some(inp),
            Link::Slash(t) => {
                // normal control throughout; ends with the animation
                self.pu.link = if t + 1 >= SLASH_FRAMES {
                    Link::None
                } else {
                    Link::Slash(t + 1)
                };
                Some(inp)
            }
            Link::Dash { phase: 0, t } => {
                if t + 1 >= SLASH_FRAMES {
                    // still holding the button: the charged dash winds up
                    self.pu.link = if inp.held(input::RUN) {
                        Link::Dash {
                            phase: 1,
                            t: DASH_WINDUP,
                        }
                    } else {
                        Link::None
                    };
                } else {
                    self.pu.link = Link::Dash { phase: 0, t: t + 1 };
                }
                Some(inp)
            }
            Link::Dash { phase, t } => {
                if !inp.held(input::RUN) {
                    self.pu.link = Link::None;
                    return Some(inp);
                }
                let speed = if self.in_water {
                    DASH_SPEED / 2.0
                } else {
                    DASH_SPEED
                };
                if phase == 1 {
                    let a = self.stop_decel();
                    self.vx = approach(self.vx, 0.0, a);
                    self.pu.link = if t <= 1 {
                        self.vx = speed * self.facing as f32;
                        Link::Dash { phase: 2, t: 0 }
                    } else {
                        Link::Dash { phase: 1, t: t - 1 }
                    };
                } else {
                    if dir == -self.facing {
                        self.pu.link = Link::None;
                        return Some(inp);
                    }
                    self.vx = speed * self.facing as f32;
                }
                self.vy = if self.on_ground {
                    GRAVITY_GROUND
                } else {
                    (self.vy + self.band_gravity(inp.held(input::JUMP))).max(FALL_CAP)
                };
                self.pose = Pose::Walk;
                let want = self.vx;
                self.pu_finish(inp, level);
                if phase == 2 && want != 0.0 && self.vx == 0.0 {
                    // the wall: "p_DashSwoadHit" and a recoil jump [obs]
                    self.pu.cues |= CUE_DASH_WALL;
                    self.vy = match (self.on_ground, self.in_water) {
                        (true, false) => 3.0,
                        (true, true) => 1.5,
                        _ => 0.0,
                    };
                    self.vx = -0.5 * speed * self.facing as f32;
                    self.on_ground = false;
                    self.pu.link = Link::None;
                }
                None
            }
            Link::Shield => {
                if !down || !self.on_ground || pressed || self.held.is_some() {
                    self.pu.link = Link::None;
                    return Some(inp);
                }
                let a = self.stop_decel();
                self.vx = approach(self.vx, 0.0, a);
                self.vy = GRAVITY_GROUND;
                self.pose = Pose::Crouch;
                self.pu_finish(inp, level);
                None
            }
            Link::Bow { t, released } => {
                let released = released || !inp.held(input::SPIN);
                self.pu.aim = if up {
                    1
                } else if down {
                    2
                } else {
                    0
                };
                if t + 1 >= BOW_DRAW && released {
                    self.pu.arrow = Some(self.pu.aim);
                    self.pu.cues |= CUE_BOW;
                    self.pu.link = Link::None;
                } else {
                    self.pu.link = Link::Bow { t: t + 1, released };
                }
                Some(inp)
            }
            Link::Thrust { phase, t } => self.link_thrust(inp, phase, t, level),
        }
    }

    /// While drawing the bow: on the ground faster than 1.2 px/f the stop table brakes; in the
    /// air the target is ±1.2 [obs 0x71015ceb90; inf: the air's accel row is the normal one].
    fn bow_movement(&mut self, inp: Input, vx_before: f32) {
        let dir = inp.dir_x();
        if self.on_ground {
            if vx_before.abs() > BOW_SPEED {
                self.vx = vx_before;
                let a = self.stop_decel();
                self.vx = approach(vx_before, 0.0, a);
            }
        } else if dir != 0 && self.vx.abs() > BOW_SPEED {
            self.vx = approach(vx_before, BOW_SPEED * dir as f32, AIR_DIR[0]);
        }
    }

    fn link_thrust(&mut self, inp: Input, phase: u8, t: i32, level: &Level) -> Option<Input> {
        match phase {
            0 | 1 => {
                // hangs still with no gravity through the start animation and the pause
                self.vx = 0.0;
                self.vy = 0.0;
                self.pu.link = match (phase, t + 1) {
                    (0, n) if n >= THRUST_HANG => Link::Thrust { phase: 1, t: 0 },
                    (1, n) if n >= THRUST_PAUSE => {
                        self.vy = if self.in_water { -4.2 } else { THRUST_VY };
                        Link::Thrust { phase: 2, t: 0 }
                    }
                    (p, n) => Link::Thrust { phase: p, t: n },
                };
                self.pose = Pose::Jump;
                self.pu_finish(inp, level);
                if self.on_ground && phase == 1 {
                    self.pu.link = Link::None;
                }
                None
            }
            2 => {
                if inp.held(input::UP) {
                    // Up cancels the thrust [obs]
                    self.pu.link = Link::None;
                    return Some(inp);
                }
                let cap = if self.in_water { -4.2 } else { THRUST_VY };
                self.vy = (self.vy - 0.34).max(cap);
                let dir = inp.dir_x() as f32;
                self.vx = approach(self.vx, 0.3 * dir, 0.1);
                self.pose = Pose::Jump;
                self.pu_finish(inp, level);
                if self.on_ground {
                    self.pu.thrust_land = true;
                    self.pu.link = Link::Thrust { phase: 3, t: 11 };
                }
                None
            }
            _ => {
                if !self.on_ground {
                    self.pu.link = Link::None;
                    return Some(inp);
                }
                self.vx = 0.0;
                self.vy = GRAVITY_GROUND;
                self.pu.link = if t <= 1 {
                    Link::None
                } else {
                    Link::Thrust { phase: 3, t: t - 1 }
                };
                self.pose = Pose::Wait;
                self.pu_finish(inp, level);
                None
            }
        }
    }

    // -------------------------------------------------------------- SMB2 Mushroom

    fn usa_before(&mut self, inp: Input, run_pressed: bool, level: &Level) -> Option<Input> {
        if self.pu.lift > 0 {
            // LiftUp: no speed; the object comes up after the delay and rises 12 frames
            self.pu.lift += 1;
            self.vx = 0.0;
            self.vy = if self.on_ground {
                GRAVITY_GROUND
            } else {
                (self.vy - 0.34).max(FALL_CAP)
            };
            let up = self.pu.lift - self.pu.lift_delay;
            if up >= 0 {
                let k = (up as f32 / LIFT_RISE as f32).min(1.0);
                self.pu.carry_h = self.pu.lift_from + (CARRY_HEIGHT - self.pu.lift_from) * k;
            }
            if up >= LIFT_RISE {
                self.pu.lift = 0;
                self.pu.crouch_lock = 10;
                self.pu.carry_h = CARRY_HEIGHT;
            }
            self.pose = Pose::Wait;
            self.pu_finish(inp, level);
            return None;
        }
        if run_pressed && inp.held(input::DOWN) && self.on_ground && self.held.is_none() {
            self.pu.pick = true;
        }
        // the crouch charge (Crouch / CrouchWalk), reset when crouch is let go
        if self.crouch && self.on_ground && inp.held(input::DOWN) {
            let before = self.pu.charge;
            self.pu.charge = (self.pu.charge + 1).min(CHARGE_FRAMES);
            if before < CHARGE_FRAMES && self.pu.charge == CHARGE_FRAMES {
                self.pu.cues |= CUE_CHARGE;
            }
        } else if self.on_ground {
            self.pu.charge = 0;
        }
        if self.held.is_some() {
            self.pu.carry_h = if self.crouch {
                CARRY_HEIGHT_CROUCH
            } else {
                CARRY_HEIGHT
            };
        }
        if self.pu.crouch_lock > 0 {
            return Some(Input {
                buttons: inp.buttons & !input::DOWN,
            });
        }
        Some(inp)
    }

    // -------------------------------------------------------------- Frog Suit

    fn frog_before(&mut self, inp: Input, pressed: bool, level: &Level) -> Option<Input> {
        if self.in_water {
            self.frog_swim(inp, pressed, level);
            return None;
        }
        self.pu.leap = false;
        self.pu.swim_buf = 0;
        // jump predicate 1: the Frog may still jump on the frame its ground flag drops [obs
        // 0x71015e4830; inf: coyote-like]
        if !self.on_ground && self.pu.was_ground && pressed && self.vy <= 0.0 {
            self.on_ground = true;
        }
        let dir = inp.dir_x();
        if self.on_ground && self.held.is_none() && !self.crouch {
            if !self.pu.frog_walk {
                // FrogWalk begins: a landing that keeps the stride starts mid-hop [obs]
                self.pu.frog_walk = true;
                if !self.pu.was_ground && dir != 0 && self.vx * (dir as f32) > 0.0 {
                    self.pu.hop = true;
                    self.pu.hop_t = 2;
                } else {
                    self.pu.hop = false;
                    self.pu.hop_t = FROG_WAIT;
                }
            } else if self.pu.hop {
                self.pu.hop_t -= 1;
                if self.pu.hop_t <= 0 {
                    self.pu.hop = false;
                    self.pu.hop_t = FROG_WAIT;
                }
            } else {
                self.pu.hop_t = (self.pu.hop_t - 1).max(0);
                if self.pu.hop_t == 0 && dir != 0 {
                    self.pu.hop = true;
                    self.pu.hop_t = FROG_HOP;
                    self.walk_phase = 0.0;
                }
            }
        } else {
            self.pu.frog_walk = false;
            self.pu.hop = false;
        }
        Some(inp)
    }

    /// FrogWalk's speeds (after the normal ground routine): the hop pushes toward the normal
    /// target with 3× the table's accel; the wait brakes at 0.5 (above 1.5 px/f) or 0.15; ice
    /// changes nothing; a slope slide becomes a stop [obs 0x71015c3ef0, 0x71015d2840,
    /// 0x71015e8640].
    fn frog_after(&mut self, inp: Input, vx_before: f32) {
        if self.sliding {
            self.sliding = false;
            if inp.dir_x() == 0 {
                self.vx = 0.0;
            }
        }
        if !(self.pu.frog_walk && self.on_ground && self.held.is_none()) || self.crouch {
            return;
        }
        self.skid = SkidPhase::None;
        let dir = inp.dir_x();
        let run = inp.held(input::RUN);
        if self.pu.hop {
            let t = self.thresholds();
            let star = self.star > 0;
            if dir != 0 {
                self.facing = dir;
            }
            let (target, table) = if dir == 0 {
                (0.0, &GROUND_IDLE)
            } else {
                let table = match (run, star) {
                    (false, false) => &GROUND_WALK,
                    (false, true) => &GROUND_WALK_STAR,
                    (true, false) => &GROUND_RUN,
                    (true, true) => &GROUND_RUN_STAR,
                };
                (dir as f32 * if run { self.run_max() } else { t[1] }, table)
            };
            let a = 3.0 * table[band(vx_before, self.facing, t)];
            self.vx = approach(vx_before, target, a);
            self.pose = Pose::Walk;
        } else {
            let a = if vx_before.abs() > 1.5 { 0.5 } else { 0.15 };
            self.vx = approach(vx_before, 0.0, a);
            self.pose = Pose::Wait;
        }
    }

    /// FrogSwim (state 49): free 8-way swimming with no gravity [obs 0x71015c4060 /
    /// 0x71015c42a0].
    fn frog_swim(&mut self, inp: Input, pressed: bool, level: &Level) {
        let top = self.pu.water_top;
        if !self.was_in_water {
            // entering: no sinking momentum; from above, placed at the surface clamp
            self.vy = self.vy.max(0.0);
            self.pu.leap = false;
            self.pu.push_up = 0;
            self.pu.push_down = 0;
            if let Some(t) = top
                && self.y > t - FROG_SURFACE
            {
                self.y = t - FROG_SURFACE;
                self.pu.cues |= CUE_FROG_SPLASH;
            }
        }
        if pressed {
            self.pu.swim_buf = 20;
        } else if self.pu.swim_buf > 0 {
            self.pu.swim_buf -= 1;
        }
        for t in [&mut self.pu.push_up, &mut self.pu.push_down] {
            if *t > 0 {
                *t -= 1;
            }
        }
        let clamp = top.map(|t| t - FROG_SURFACE);
        // jumping out: only from the surface clamp, with a jump pressed in the last 20 frames;
        // the launch is the Frog's jump speed [obs 0x71015e9e60]
        if let Some(c) = clamp
            && self.pu.swim_buf > 0
            && self.y >= c - 1.0
            && !self.pu.leap
        {
            self.vy = FROG_JUMP;
            self.pu.leap = true;
            self.pu.swim_buf = 0;
            self.pose = Pose::Jump;
            self.pu_finish(inp, level);
            return;
        }
        if self.pu.leap {
            // on the way out: ordinary gravity until the water is left
            self.vy = (self.vy + self.band_gravity(inp.held(input::JUMP))).max(FALL_CAP);
            self.pose = Pose::Jump;
            self.pu_finish(inp, level);
            return;
        }
        let dx = inp.dir_x() as f32;
        let dy = if inp.held(input::UP) {
            1.0
        } else if inp.held(input::DOWN) {
            -1.0
        } else {
            0.0
        };
        let run = inp.held(input::RUN);
        let (mut target, step) = if dx == 0.0 && dy == 0.0 {
            ((0.0, 0.0), 0.05)
        } else {
            // per-axis targets, not normalised: diagonals are √2 faster [obs]
            let s = if run { 2.0 } else { 1.0 };
            ((dx * s, dy * s), if run { 0.8 } else { 0.5 })
        };
        if self.pu.push_up > 0 {
            target.1 = target.1.max(2.0 * self.pu.push_up as f32 / 30.0);
        }
        if self.pu.push_down > 0 {
            target.1 = target.1.min(-2.0 * self.pu.push_down as f32 / 30.0);
        }
        let (vx, vy) = approach2((self.vx, self.vy), target, step);
        self.vx = vx;
        self.vy = vy.min(2.0);
        if dx != 0.0 {
            self.facing = dx as i32;
        }
        self.pose = Pose::Swim;
        self.walk_phase += if run { 1.0 } else { 0.5 };
        let vy0 = self.vy;
        self.pu_finish(inp, level);
        if vy0 > 0.0 && (self.head_hit.is_some() || self.head_solid.is_some()) {
            // a head bump pushes the frog down for 30 frames [obs]
            self.vy = -2.0;
            self.pu.push_down = 30;
        }
        if let Some(c) = clamp
            && self.y > c
        {
            self.y = c;
            self.vy = 0.0;
        }
    }

    // -------------------------------------------------------------- Power Balloon

    /// BalloonFloat (state 50): the balloon's only state [obs 0x7101590d00].
    fn balloon(&mut self, inp: Input, level: &Level) {
        let dx = inp.dir_x() as f32;
        let dy = if inp.held(input::UP) {
            1.0
        } else if inp.held(input::DOWN) {
            -1.0
        } else {
            0.0
        };
        let has_dir = dx != 0.0 || dy != 0.0;
        let u = unit(dx, dy);
        let liquid = self.in_water;
        let cap = if liquid { 1.5 } else { 2.0 };
        let speed = self.vx.dhypot(self.vy);
        let want_tilt = dy.datan2(dx).to_degrees() - 90.0;
        if !self.pu.jet {
            let mut t = (u.0 * 0.5, u.1 * 0.5);
            let step = if speed > 0.5 {
                0.04
            } else if has_dir {
                0.05
            } else {
                0.02
            };
            // contact clamps: the ground keeps it from sinking, a ceiling from rising, walls
            // from pressing in
            if self.on_ground {
                t.1 = t.1.max(0.0);
                self.vy = self.vy.max(0.0);
            }
            if self.ceiling {
                t.1 = t.1.min(0.0);
            }
            if self.wall_at(level, -1) {
                t.0 = t.0.max(0.0);
            }
            if self.wall_at(level, 1) {
                t.0 = t.0.min(0.0);
            }
            (self.vx, self.vy) = approach2((self.vx, self.vy), t, step);
            if has_dir {
                self.pu.tilt = turn_toward(self.pu.tilt, want_tilt, 6.0);
            }
            let wedged = self.on_ground && self.wall_at(level, -1) && self.wall_at(level, 1);
            if inp.held(input::RUN) && self.pu.jet_lock == 0 && !wedged {
                self.pu.jet = true;
            }
        }
        if self.pu.jet {
            if has_dir {
                self.pu.tilt = turn_toward(self.pu.tilt, want_tilt, 5.0);
            }
            let star = self.star > 0;
            let (s, step) = match (self.low_gravity, star) {
                (true, false) => (3.5, 0.06),
                (true, true) => (4.0, 0.09),
                (false, false) => (if liquid { 1.5 } else { 2.0 }, 0.07),
                (false, true) => (if liquid { 2.0 } else { 2.5 }, 0.07),
            };
            let (sn, cs) = self.pu.tilt.to_radians().dsin_cos();
            (self.vx, self.vy) = approach2((self.vx, self.vy), (-sn * s, cs * s), step);
            if !inp.held(input::RUN) {
                self.pu.jet = false;
            }
        }
        // facing: left while tilted left (> 0), right while tilted right
        if self.pu.tilt > 1.0 && self.pu.tilt < 179.0 {
            self.facing = -1;
        } else if self.pu.tilt < -1.0 && self.pu.tilt > -179.0 {
            self.facing = 1;
        }
        self.pose = Pose::Swim;
        self.walk_phase += 1.0;
        let (vx0, vy0) = (self.vx, self.vy);
        let fast = vx0.dhypot(vy0) > 0.50001 || self.pu.jet;
        self.pu_finish(inp, level);
        let mut bounced = false;
        if vx0 != 0.0 && self.vx == 0.0 && fast {
            // a wall reflects it (at least 0.5 px/f) [obs]
            self.vx = -vx0.signum() * vx0.abs().max(0.5);
            self.vy = vy0;
            bounced = true;
        }
        if vy0 > 0.0 && (self.head_hit.is_some() || self.head_solid.is_some()) {
            // a head bump: down at 2.0, the jet ends [obs]
            self.vy = if liquid { -1.5 } else { -2.0 };
            self.pu.jet = false;
            self.pu.jet_lock = BALLOON_LOCK;
            bounced = true;
        } else if vy0 < 0.0 && self.on_ground && fast {
            self.vy = (-vy0).max(0.5);
            self.vx = vx0;
            self.on_ground = false;
            bounced = true;
        }
        if bounced {
            self.pu.cues |= CUE_BALLOON_WALL;
            let s = self.vx.dhypot(self.vy);
            if s > cap {
                self.vx *= cap / s;
                self.vy *= cap / s;
            }
            if self.pu.jet {
                // a jet turned away by more than ≈26° ends (dot < 0.9)
                let (sn, cs) = self.pu.tilt.to_radians().dsin_cos();
                let v = unit(self.vx, self.vy);
                if v.0 * -sn + v.1 * cs < 0.9 {
                    self.pu.jet = false;
                    self.pu.jet_lock = BALLOON_LOCK;
                }
            }
        }
    }

    // -------------------------------------------------------------- Super Acorn

    fn squirrel_before(
        &mut self,
        inp: Input,
        pressed: bool,
        spin_pressed: bool,
        level: &Level,
    ) -> Option<Input> {
        let jump = inp.held(input::JUMP);
        let dir = inp.dir_x();
        if self.in_water || self.on_ground || self.climbing.is_some() || self.yoshi.is_some() {
            self.pu.sq = Squirrel::None;
            return Some(inp);
        }
        let free = self.held.is_none() && self.shoe.is_none();
        // the boost: the spin button in the air, once per air time (0x71015c23d0) [obs]
        if spin_pressed
            && free
            && self.pu.boost
            && !matches!(self.pu.sq, Squirrel::Flying { .. } | Squirrel::Cling { .. })
        {
            self.squirrel_boost(dir);
        }
        if let Squirrel::None = self.pu.sq {
            // wall cling (0x71015e6ce0 → 0x71015c3940): pressing into a wall while falling,
            // both 14-px probes on it
            if dir != 0
                && self.vy < 0.0
                && free
                && self.pu.cling_lock == 0
                && !self.pu.recling
                && self.cling_probe(level, dir)
            {
                // FlyingWallStick.begin: no speed, facing away from the wall; the 90-frame
                // timer counts from the next frame
                self.vx = 0.0;
                self.vy = 0.0;
                self.facing = -dir;
                self.pu.sq = Squirrel::Cling {
                    t: CLING_FRAMES,
                    side: dir,
                };
                self.pose = Pose::Climb;
                self.pu_finish(inp, level);
                self.vx = 0.0;
                return None;
            } else if self.vy < 0.0 && free && self.wall_slide == 0 {
                // glide / flutter (0x71015c2f60) from Fall / Jump
                if self.pu.boost && jump {
                    self.pu.sq = Squirrel::Glide { mode: 0 };
                } else if !self.pu.boost {
                    self.pu.sq = Squirrel::Glide { mode: 1 };
                }
                if matches!(self.pu.sq, Squirrel::Glide { .. }) {
                    self.pu.glide_cap = if self.vy < -2.0 { -3.5 } else { -2.0 };
                }
            }
        }
        match self.pu.sq {
            Squirrel::None => Some(inp),
            Squirrel::Flying { phase, t } => {
                self.squirrel_fly(inp, phase, t, level);
                None
            }
            Squirrel::Glide { mode } => {
                if mode == 0 && !jump {
                    // the glide ends when jump is let go [obs]
                    self.pu.sq = Squirrel::None;
                    return Some(inp);
                }
                self.squirrel_glide(inp, mode, level);
                None
            }
            Squirrel::Cling { t, side } => {
                if pressed {
                    // the wall jump [inf: the shared wall kick]
                    self.vy = 3.5;
                    self.vx = -side as f32 * 2.25;
                    self.facing = -side;
                    self.pu.sq = Squirrel::None;
                    self.pose = Pose::Jump;
                    self.pu_finish(inp, level);
                    return None;
                }
                if dir != side || !self.cling_probe(level, side) {
                    self.pu.sq = Squirrel::None;
                    self.pu.cling_lock = 30;
                    return Some(inp);
                }
                // holding on: 1 px into the wall a frame, no speed, no gravity
                self.vx = side as f32;
                self.vy = 0.0;
                self.facing = -side;
                if t <= 1 {
                    // then the ordinary wall slide, once per air time
                    self.pu.sq = Squirrel::None;
                    self.pu.recling = true;
                    self.wall_slide = side;
                    self.wall_release = 0;
                } else {
                    self.pu.sq = Squirrel::Cling { t: t - 1, side };
                }
                self.pose = Pose::Climb;
                self.pu_finish(inp, level);
                self.vx = 0.0;
                None
            }
        }
    }

    /// The two 14-px probes toward the wall, near the top and the bottom of the body.
    fn cling_probe(&self, level: &Level, dir: i32) -> bool {
        let x = self.x + dir as f32 * 14.0;
        let top = self.sensors().head_y - 2.0;
        level.cell_at(x, self.y + 2.0).is_solid() && level.cell_at(x, self.y + top).is_solid()
    }

    /// Flying.begin (0x71015c25b0).
    fn squirrel_boost(&mut self, dir: i32) {
        self.pu.boost = false;
        if dir != 0 {
            self.facing = dir;
        }
        let t = if self.vx.abs() > 2.0 {
            BOOST_FRAMES.0
        } else {
            BOOST_FRAMES.1
        };
        self.vy = if self.low_gravity { 1.4 } else { 2.0 };
        if self.vx * (self.facing as f32) < 0.0 {
            self.vx = 0.0;
        }
        let ax = self.vx.abs();
        self.pu.mag = self.vy.dhypot(ax);
        self.pu.base = (ax / 3.0).max(0.8);
        self.pu.ang = self.vy.datan2(ax).to_degrees();
        self.pu.sq = Squirrel::Flying { phase: 0, t };
        self.spinning = false;
    }

    /// Flying.exec (0x71015c2910): the rise turns toward 67.5° while the speed grows to 4.2 ×
    /// the base, then a coast at gravity −0.12 into the flutter.
    fn squirrel_fly(&mut self, inp: Input, phase: u8, t: i32, level: &Level) {
        let f = self.facing as f32;
        let t = (t - 1).max(0);
        if phase == 0 {
            let k = if self.low_gravity { 2.94 } else { 4.2 };
            self.pu.mag = approach(self.pu.mag, k * self.pu.base, 0.1);
            self.pu.ang = approach(self.pu.ang, 67.5, 4.21875);
            let (sn, cs) = self.pu.ang.to_radians().dsin_cos();
            let vx_t = f * (self.pu.mag * cs).max(0.5);
            let vy_t = self.pu.mag * sn;
            self.vx = approach(self.vx, vx_t, 0.3);
            self.vy = if self.vy < vy_t {
                (self.vy + 0.3).min(vy_t)
            } else {
                vy_t
            };
            self.pu.sq = if t == 0 && (self.pu.ang - 67.5).abs() <= 5.625 {
                Squirrel::Flying { phase: 1, t: 0 }
            } else {
                Squirrel::Flying { phase: 0, t }
            };
        } else {
            let g = if self.low_gravity { -0.168 } else { -0.12 };
            self.vy = (self.vy + g).max(-3.0);
            self.vx = approach(self.vx, f * 1.5, 0.05);
            if self.vy <= 0.0 {
                self.pu.sq = Squirrel::Glide { mode: 1 };
                self.pu.glide_cap = if self.vy < -2.0 { -3.5 } else { -2.0 };
            }
        }
        self.pose = Pose::Jump;
        self.pu_finish(inp, level);
        if self.on_ground {
            self.pu.sq = Squirrel::None;
        }
    }

    /// FlyingSlowFall.exec (0x71015c3390): glide (mode 0) or flutter (mode 1).
    fn squirrel_glide(&mut self, inp: Input, mode: u8, level: &Level) {
        let dir = inp.dir_x();
        if dir != 0 {
            self.facing = dir;
            let t = if mode == 1 {
                1.5
            } else if inp.held(input::RUN) {
                3.0
            } else {
                2.0
            };
            let a = if self.vx * (dir as f32) >= 0.0 {
                0.06
            } else {
                0.09
            };
            self.vx = approach(self.vx, t * dir as f32, a);
        } else if self.vx * (self.facing as f32) < 0.0 || self.vx.abs() > 2.0 {
            self.vx = approach(self.vx, 0.0, 0.03);
        }
        // the fall cap steps 0.1 a frame toward −1.5 (boost used) or −2.0 … −0.65 by speed
        let want = if !self.pu.boost {
            -1.5
        } else {
            let k = (self.vx.abs() / 2.0).clamp(0.0, 1.0);
            k * -0.65 + (1.0 - k) * -2.0
        };
        self.pu.glide_cap = approach(self.pu.glide_cap, want, 0.1);
        let g = self.band_gravity(inp.held(input::JUMP));
        self.vy = (self.vy + g).max(self.pu.glide_cap);
        self.pose = Pose::Jump;
        self.pu_finish(inp, level);
        if self.on_ground {
            self.pu.sq = Squirrel::None;
        }
    }

    // -------------------------------------------------------------- Boomerang Flower

    fn boomerang_before(&mut self, run_pressed: bool) {
        if self.pu.throw_t > 0 {
            self.pu.throw_t += 1;
            if self.pu.throw_t == 6 {
                self.pu.boomerang = true;
            }
            if self.pu.throw_t > BOOMERANG_ANIM {
                self.pu.throw_t = 0;
            }
            self.fire_pose = self.fire_pose.max(2);
            return;
        }
        // the throw gate (0x71015c0ca0): not carrying, no headdress, no boomerang out
        if run_pressed
            && self.held.is_none()
            && self.pu.headdress.is_none()
            && !self.pu.boomerang_out
            && !self.crouch
            && self.shoe.is_none()
        {
            self.pu.throw_t = 1;
            self.fire_pose = 8;
        }
    }

    // -------------------------------------------------------------- wearables

    fn headdress_before(
        &mut self,
        inp: Input,
        pressed: bool,
        spin_pressed: bool,
        run_pressed: bool,
        level: &Level,
    ) -> Option<Input> {
        let _ = pressed;
        let free = self.held.is_none() && self.yoshi.is_none() && self.shoe.is_none();
        match self.pu.headdress {
            Some(Headdress::Cannon) => {
                // p_HoudaiCharge (0x71015c5870)
                if !free {
                    self.pu.cannon_charge = 0;
                } else if inp.held(input::RUN) {
                    if run_pressed && self.pu.cannon_cd == 0 && self.pu.cannon_charge == 0 {
                        self.pu.cannon = Some(false);
                        self.pu.cannon_cd = CANNON_COOLDOWN;
                    }
                    self.pu.cannon_charge = (self.pu.cannon_charge + 1).min(CANNON_CHARGE);
                } else {
                    if self.pu.cannon_charge >= CANNON_CHARGE {
                        self.pu.cannon = Some(true);
                        self.pu.cannon_cd = CANNON_COOLDOWN;
                    }
                    self.pu.cannon_charge = 0;
                }
                Some(inp)
            }
            Some(Headdress::Propeller) => {
                if self.pu.prop > 0 {
                    self.box_propeller(inp, spin_pressed, level);
                    return None;
                }
                if spin_pressed && !self.in_water && free && self.pu.flights < BOX_FLIGHTS {
                    self.box_flight();
                    self.box_propeller(inp, false, level);
                    return None;
                }
                Some(inp)
            }
            Some(Headdress::Bullet) => {
                let jump = inp.held(input::JUMP);
                if !self.pu.killer
                    && !self.on_ground
                    && jump
                    && (self.vy < -0.5 || self.in_water)
                    && self.pu.killer_time > 0
                    && self.pu.killer_lock == 0
                    && free
                {
                    self.pu.killer = true;
                    self.spinning = false;
                }
                if self.pu.killer {
                    if !jump || self.pu.killer_time == 0 || self.on_ground && self.vy > 0.0 {
                        self.pu.killer = false;
                        return Some(inp);
                    }
                    self.killer_move(inp, level);
                    return None;
                }
                Some(inp)
            }
            _ => Some(inp),
        }
    }

    /// Propeller.begin with the box (0x71015d5160): one of the three flights.
    fn box_flight(&mut self) {
        self.pu.flights += 1;
        self.pu.prop = 1;
        self.pu.prop_rise = BOX_RISE;
        self.pu.prop_fall = false;
        self.on_ground = false;
        self.spinning = false;
        self.pu.cues |= CUE_BOX_SPIN;
        if self.low_gravity {
            self.vy *= 0.7;
        }
    }

    /// Propeller (70) / PropellerFall (71) with the box (0x71015d54e0 / 0x71015d5cd0).
    fn box_propeller(&mut self, inp: Input, spin_pressed: bool, level: &Level) {
        if spin_pressed && self.pu.prop_fall {
            if self.pu.flights < BOX_FLIGHTS {
                self.box_flight();
            } else if self.pu.spin_lock == 0 {
                // out of flights: "spin once" (max fall −1.0, 20-frame lockout)
                self.pu.spin_once = true;
                self.pu.spin_lock = 20;
                self.pu.cues |= CUE_SPIN_ONCE;
            }
        }
        self.pu.prop += 1;
        let night = if self.low_gravity { 0.7 } else { 1.0 };
        let rising = self.pu.prop_rise > 0 || !self.pu.prop_fall;
        if self.pu.prop_rise > 0 {
            self.pu.prop_rise -= 1;
            self.vy = (self.vy + 1.0 * night).min(2.5 * night);
        } else if !self.pu.prop_fall {
            // the box's mode 2: gravity −0.05 until falling
            self.vy -= 0.05;
            if self.vy < 0.0 {
                self.pu.prop_fall = true;
                self.pu.prop_first = true;
            }
        } else {
            let g = if self.pu.prop_first { -0.03 } else { -0.1 };
            self.pu.prop_first = false;
            let cap = if self.pu.spin_once { -1.0 } else { -1.25 };
            self.vy = (self.vy + g).max(cap);
        }
        let dir = inp.dir_x();
        let (target, a) = if dir == 0 {
            (0.0, 0.05)
        } else {
            self.facing = dir;
            let back = self.vx * (dir as f32) < 0.0;
            let a = match (rising, back) {
                (true, true) => 0.08,
                (true, false) => 0.03,
                (false, true) => 0.11,
                (false, false) => 0.07,
            };
            (1.5 * dir as f32, a)
        };
        self.vx = approach(self.vx, target, a);
        self.pose = Pose::Spin;
        self.walk_phase += 1.0;
        let vy0 = self.vy;
        self.pu_finish(inp, level);
        if vy0 > 0.0 && (self.head_hit.is_some() || self.head_solid.is_some()) {
            self.vy = -1.5;
        }
        if self.on_ground || self.in_water {
            self.pu.prop = 0;
        }
    }

    /// KillerMove (state 141, 0x71015c6000): a straight dash in the start direction.
    fn killer_move(&mut self, inp: Input, level: &Level) {
        if self.wall_at(level, self.facing) {
            // KillerBreak (0x71015c56a0): the mask breaks, 127 frames of invincibility
            self.pu.take_off();
            self.pu.killer_break = true;
            self.pu.cues |= CUE_KILLER_BREAK;
            self.invuln = INVULN_DAMAGE;
            self.vx = 0.0;
            self.pu_finish(inp, level);
            return;
        }
        self.pu.killer_time -= 1;
        if self.in_water {
            self.pu.killer_time = KILLER_TIME;
        }
        self.vy = approach(self.vy, 0.0, 0.4);
        let s = if self.in_water { 1.5 } else { 3.0 };
        self.vx = approach(self.vx, s * self.facing as f32, 0.1);
        self.pose = Pose::Jump;
        self.pu_finish(inp, level);
    }
}
