//! Bosses: Bowser (62-bowser.md), Boom Boom and Pom Pom (77-boom-boom.md) and Bowser Jr.
//! (98-bowser-jr.md). Facts are [obs] unless marked; the specs cite the binary for each one and
//! say why the few remaining [inf] / [?] choices could not be read from it.
//!
//! State layout on the shared actor fields: HP in `hops`; `phase` is the state (0 = not set up
//! yet); `timer` is the actor timer (counted down at the start of every boss step, states wait
//! for it to reach 0); `pause` is a second timer (Bowser: the fire timer; Boom Boom: the action
//! timer; Bowser Jr.: the shot / Clown Car attack timer). `chain` holds small counters, one per
//! byte (see each boss). Bowser walks along `heading` (±1) while facing `dir`. `kick_lock`
//! (counted down by the generic step) is the hurt flash, and for a boss in a Clown Car also its
//! invulnerability.

use super::{Actor, Kind, MAX_FALL, Mode, SHELL_WAKE};
use crate::fmath::DetMath;
use crate::level::{Cell, Level, TILE};
use crate::world::{VIEW_H, VIEW_W};

/// The attacks the engine can deliver to a boss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossAttack {
    Stomp,
    /// A small player fireball.
    Fire,
    /// A sliding shell.
    Shell,
    /// A Bob-omb explosion.
    Blast,
    /// A block bumped from below under the boss.
    BlockHit,
}

/// What a hit did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossHit {
    /// No effect. A stomp on Bowser is "Invalid": the touch then hurts the player.
    Ignored,
    /// Stunned or spinning: a stomp bounces off harmlessly.
    Guarded,
    /// Taken without losing HP (a fireball drained the counter; a block bumped Bowser).
    Absorbed,
    /// Lost HP.
    Hurt,
    /// HP ran out: knocked off (DieFall).
    Defeated,
}

/// The shared enemy gravity and fall cap that the bosses' states switch back to.
const G: f32 = -0.1875;
const CAP: f32 = -4.0;

/// Style index of the level: 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU (3D World uses the NSMBU rules).
fn style_index(level: &Level) -> usize {
    match level.style.as_str() {
        "M1" => 0,
        "M3" => 1,
        "MW" => 2,
        _ => 3,
    }
}

// ---------------------------------------------------------------- Bowser constants

/// HP (helper +0x18): 20, big 40.
const BOWSER_HP: u8 = 20;
const BOWSER_PATROL: f32 = 64.0;
const BOWSER_WALK: f32 = 0.3;
/// Chase-mode walk speed: SMB1 0.7, the other styles 0.9.
const BOWSER_CHASE: [f32; 2] = [0.7, 0.9];
const BOWSER_GRAVITY: f32 = -0.1;
/// Background-collision half-width per style (the "player behind" sensor), ×2 when big.
const BOWSER_BG_HALF: [f32; 4] = [13.0, 12.0, 14.0, 15.9];
/// Where a flame leaves, per style: ahead of / above Bowser's origin (×2 big).
const BOWSER_MOUTH: [(f32, f32); 4] = [(28.0, 21.0), (24.0, 24.0), (24.0, 21.0), (20.0, 24.0)];
/// The fire animation: its length, and the frame at which the flame leaves (SMB1: at its end).
/// NSMBU's length is not in the 2D sprite data [?]; one frame past its fire frame is used.
const BOWSER_BREATH: [(u8, u8); 4] = [(35, 35), (48, 36), (48, 10), (49, 48)];
/// The flame's own speed (BulletKoopaFire +0x183c); Bowser's forward speed is added.
const BOWSER_FIRE_SPEED: f32 = 1.5;
/// A flame's lane: 17 px (big 34) above or below the mouth, or level.
const FIRE_LANE: f32 = 17.0;
/// The Turn animation (non-SMB1) before he turns round: SMB3 30 frames, SMW 45, NSMBU [?] 30.
const BOWSER_TURN: [i32; 4] = [0, 30, 45, 30];

/// Bowser states (numbering ours; names from the state table).
mod bw {
    pub const FALL: u8 = 1;
    pub const LAND_ON: u8 = 2;
    pub const WALK: u8 = 3;
    pub const LITTLE_JUMP: u8 = 4;
    pub const MIDDLE_JUMP: u8 = 5;
    pub const BIG_JUMP: u8 = 6;
    pub const HIP_DROP: u8 = 7;
    pub const LOOK_AROUND: u8 = 8;
    pub const TURN: u8 = 9;
    pub const FLY: u8 = 10;
    pub const HIT_BLOCK: u8 = 11;
    pub const CLOWN: u8 = 12;
}

/// Bowser's `chain` byte 3 flags.
mod bwf {
    pub const MODE: u8 = 0x03;
    pub const SUB: u8 = 0x0c;
    pub const BREATHING: u8 = 0x10;
    pub const FIRED: u8 = 0x20;
    pub const FELL_OFF: u8 = 0x40;
    /// Fly entered from a big jump (else from a middle jump).
    pub const FROM_BIG: u8 = 0x80;
}

// ---------------------------------------------------------------- Boom Boom constants

/// Walk speed by hits taken (0, 1, 2): table indexed by 3 − HP.
const BB_SPEED: [f32; 3] = [0.75, 0.75, 1.5];
/// HP 3 when the action timer runs out: a 2.0 dash that brakes in Defend.
const BB_CHARGE: f32 = 2.0;
const BB_SLOW: f32 = 0.35;
/// The slow walk after Defend.
const BB_SLOW_FRAMES: i32 = 60;
/// Jump: 30-frame wind-up, then vy 5.5 at 1.7 px/f, 30 frames on landing.
const BB_JUMP: f32 = 5.5;
const BB_JUMP_VX: f32 = 1.7;
const BB_WINDUP: i32 = 30;
/// WallJump: vy 3.75, the third in a row 5.0.
const BB_WALL_JUMP: f32 = 3.75;
const BB_WALL_JUMP_HIGH: f32 = 5.0;
const BB_WALL_BUMPS: u8 = 3;
const BB_DAMAGE: i32 = 70;
const BB_DEFEND_HOP: f32 = 2.75;
/// Defend brakes by 0.05/frame, waits 30 frames on the ground, then hops at 1.0 px/f.
const BB_DEFEND_BRAKE: f32 = 0.05;
const BB_DEFEND_WAIT: i32 = 30;
/// Squat and stand-up animations (12 frames each): guarded from squat frame 7 to stand-up 9.
const BB_ANIM: u8 = 12;
/// The boss forgets a player beyond 432 px.
const BB_RANGE: f32 = 432.0;
/// Winged: cruise toward a point 1.2 tiles ahead of the player and 3.5 tiles above the view
/// centre at ±2.0 px/f (0.05 px/f²), vertically ±1.0 (0.075 steps).
const BB_FLY_SPEED: f32 = 2.0;
const BB_SWOOP_ACCEL: f32 = 0.025;

mod bb {
    pub const DEFEND: u8 = 1;
    pub const WALK: u8 = 2;
    pub const JUMP: u8 = 3;
    pub const WALL_JUMP: u8 = 4;
    pub const DAMAGE_DOWN: u8 = 5;
    // winged (Wing action)
    pub const W_WAIT: u8 = 6;
    pub const W_FLY: u8 = 7;
    pub const W_ATTACK: u8 = 8;
    pub const W_DAMAGE: u8 = 9;
    pub const W_DEFEND: u8 = 10;
    pub const CLOWN: u8 = 11;
    // Pom Pom (3D World alternate)
    pub const POM_WAIT: u8 = 20;
    pub const POM_JUMP: u8 = 21;
    pub const POM_THROW_SIGN: u8 = 22;
    pub const POM_THROW: u8 = 23;
    pub const POM_LAPSE: u8 = 24;
    pub const POM_DAMAGE: u8 = 25;
}

// ---------------------------------------------------------------- Bowser Jr. constants

const JR_WAIT: i32 = 20;
const JR_WALK_FRAMES: i32 = 120;
/// Walk speed: 1.25 px/f, 0.8 with wings, in 0.1 steps; stops 24 px from home.
const JR_WALK: f32 = 1.25;
const JR_WALK_WINGS: f32 = 0.8;
const JR_WALK_STEP: f32 = 0.1;
const JR_TURN_RANGE: f32 = 24.0;
/// Wait's look-ahead before a hop (80 in water).
const JR_PROBE: f32 = 30.0;
/// Shift jump: straight up at 5.5 (big 6.0); at vy ≤ 0.4 the hip drop starts (winged: Hover).
const JR_SHIFT_JUMP: f32 = 5.5;
const JR_SHIFT_JUMP_BIG: f32 = 6.0;
const JR_HOVER_AT: f32 = 0.4;
const JR_HIP_GRAVITY: f32 = -0.75;
const JR_HIP_CAP: f32 = -8.0;
const JR_HIP_LAND: i32 = 50;
/// DamageDown (non-NSMBU): bounce vy 3.0, 15 frames.
const JR_DAMAGE: i32 = 15;
/// The Shell context: 20 frames still, then the slide (1.5 px/f) [?: its length].
const JR_SHELL_WAIT: i32 = 20;
const JR_SHELL_SPEED: f32 = 1.5;
const JR_SHELL_FRAMES: i32 = 150;
const JR_SHELL_OUT: i32 = 20;
/// Fire: 3 shots, each one 26 frames into a 60-frame animation.
const JR_FIRE_ANIM: i32 = 60;
const JR_FIRE_AT: i32 = 26;
/// Hammers (winged): 4 thrown 36, 52, 68 and 84 frames into a 110-frame animation.
const JR_THROW_ANIM: i32 = 110;
const JR_THROW_AT: [i32; 4] = [36, 52, 68, 84];
/// A fireball is aimed at the player but stays within 25° of level.
const JR_AIM: f32 = 25.0;

mod jr {
    pub const WAIT: u8 = 1;
    pub const FALL: u8 = 2;
    pub const HOP: u8 = 3;
    pub const WALK: u8 = 4;
    pub const SHOT: u8 = 5;
    pub const SHIFT_JUMP: u8 = 6;
    pub const HOVER: u8 = 7;
    pub const PREV_HIP_DROP: u8 = 8;
    pub const HIP_DROP: u8 = 9;
    pub const HIP_LAND: u8 = 10;
    pub const DAMAGE_DOWN: u8 = 11;
    pub const SHELL_WAIT: u8 = 12;
    pub const SHELL: u8 = 13;
    pub const SHELL_OUT: u8 = 14;
    // Clown Car machine
    pub const CLOWN: u8 = 15;
    pub const C_PRE_THROW: u8 = 16;
    pub const C_THROW: u8 = 17;
    pub const C_POST_THROW: u8 = 18;
    pub const C_SHOT: u8 = 19;
}

/// Bowser Jr. in a Clown Car: an attack every 150 frames while the player is near.
const JR_CLOWN_ATTACK: i32 = 150;
/// Clown Car steering for riders [?: the car's Chase state is not decoded].
const CLOWN_SPEED: f32 = 1.0;
const CLOWN_ACCEL: f32 = 0.03;

impl Kind {
    pub fn is_boss(self) -> bool {
        matches!(
            self,
            Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
        )
    }

    /// Rides a Koopa Clown Car (flag 0x200).
    pub fn boss_clown(self) -> bool {
        match self {
            Kind::Bowser { clown, .. }
            | Kind::BoomBoom { clown, .. }
            | Kind::BowserJr { clown, .. } => clown,
            _ => false,
        }
    }
}

impl Actor {
    fn byte(&self, n: u32) -> u8 {
        (self.chain >> (8 * n)) as u8
    }

    fn set_byte(&mut self, n: u32, v: u8) {
        self.chain = (self.chain & !(0xff << (8 * n))) | ((v as u32) << (8 * n));
    }

    fn toward_target(&self) -> i32 {
        if self.target.0 < self.x { -1 } else { 1 }
    }

    fn scale(&self) -> f32 {
        if self.big { 2.0 } else { 1.0 }
    }

    fn fall(&mut self, level: &Level, g: f32) {
        self.fall_capped(level, g, MAX_FALL);
    }

    fn fall_capped(&mut self, level: &Level, g: f32, cap: f32) {
        self.vy = (self.vy + g).max(cap);
        self.collide(level);
    }

    /// Standing at a drop: the floor ends just ahead in direction `dir`.
    fn ledge_ahead(&self, level: &Level, dir: f32) -> bool {
        if !self.on_ground {
            return false;
        }
        let px = self.x + dir * (self.half_w() + 0.1);
        let slope = level.slope_at(px).is_some_and(|(s, _)| s >= self.y - 8.0);
        level.cell_at(px, self.y - 1.0) == Cell::Empty && !slope
    }

    /// The boss's box overlaps the camera view (actor flag 0x510 bit 3).
    fn in_view(&self) -> bool {
        let (x0, y0, x1, y1) = self.bbox();
        let (cx, cy) = self.camera;
        x1 > cx && x0 < cx + VIEW_W && y1 > cy && y0 < cy + VIEW_H
    }

    fn view_centre(&self) -> (f32, f32) {
        (self.camera.0 + VIEW_W / 2.0, self.camera.1 + VIEW_H / 2.0)
    }

    /// Moves `v` toward `target` by at most `step`.
    fn approach(v: f32, target: f32, step: f32) -> f32 {
        v + (target - v).clamp(-step, step)
    }

    /// Hit points left (0 before the first step).
    pub fn boss_hp(&self) -> u8 {
        self.hops
    }

    /// The fireball counter of Boom Boom / Bowser Jr.
    pub fn boss_fire_counter(&self) -> u8 {
        self.byte(0)
    }

    /// Removes the held Key (if any) so it drops once; true if there was one.
    pub fn take_boss_key(&mut self) -> bool {
        match &mut self.kind {
            Kind::Bowser { key, .. } | Kind::BoomBoom { key, .. } | Kind::BowserJr { key, .. } => {
                std::mem::take(key)
            }
            _ => false,
        }
    }

    /// Pom Pom has vanished (Lapse): no contact at all.
    pub fn boss_hidden(&self) -> bool {
        matches!(self.kind, Kind::BoomBoom { pom: true, .. }) && self.phase == bb::POM_LAPSE
    }

    /// Immune; stomps bounce off. Boom Boom: from DamageDown (or the start of Defend) until 9
    /// frames into the stand-up hop. Bowser Jr.: DamageDown and the shell [inf: the Shell
    /// context's own hit rules are not decoded].
    pub fn boss_guarding(&self) -> bool {
        if self.kind.boss_clown() && self.kick_lock > 0 {
            return true;
        }
        match self.kind {
            Kind::BoomBoom { pom: false, .. } => {
                matches!(self.phase, bb::DAMAGE_DOWN | bb::W_DAMAGE) || self.bb_guard()
            }
            Kind::BoomBoom { pom: true, .. } => self.phase == bb::POM_DAMAGE,
            Kind::BowserJr { .. } => matches!(
                self.phase,
                jr::DAMAGE_DOWN | jr::SHELL_WAIT | jr::SHELL | jr::SHELL_OUT
            ),
            _ => false,
        }
    }

    /// Bowser Jr. tucked into his shell (drawn as the shell).
    pub fn boss_in_shell(&self) -> bool {
        matches!(self.kind, Kind::BowserJr { .. })
            && matches!(self.phase, jr::SHELL_WAIT | jr::SHELL | jr::SHELL_OUT)
    }

    /// Bowser is breathing fire.
    pub fn boss_breathing(&self) -> bool {
        matches!(self.kind, Kind::Bowser { .. }) && self.byte(2) > 0
    }

    /// Animation hints for the renderer: the state number (see the state modules here).
    pub fn boss_state(&self) -> u8 {
        self.phase
    }

    fn boss_setup(&mut self, level: &Level) {
        let st = style_index(level);
        match self.kind {
            Kind::Bowser { clown, .. } => {
                self.hops = if self.big { 2 * BOWSER_HP } else { BOWSER_HP };
                self.home_x = self.x;
                self.dir = self.toward_target();
                self.heading = self.dir as f32;
                self.chain = 0;
                self.pause = self.bowser_fire_interval();
                self.bowser_reset_big();
                self.bowser_reset_jump(st);
                if clown {
                    self.phase = bw::CLOWN;
                } else if self.target.0 <= self.x && !self.in_view() {
                    // waits off screen (mode 3) until it scrolls into view
                    self.bowser_set_mode(3, st);
                } else if !self.on_ground {
                    self.bowser_enter(bw::FALL, st);
                } else if self.bowser_player_ahead_right(st) {
                    self.bowser_chase_dispatch(level, st);
                } else {
                    self.bowser_enter(bw::LAND_ON, st);
                }
            }
            Kind::BoomBoom { clown, pom, .. } => {
                self.hops = 3;
                self.set_byte(0, if self.big { 10 } else { 5 });
                self.pause = 0;
                if pom {
                    self.phase = bb::POM_WAIT;
                    self.timer = 60;
                } else if clown {
                    self.phase = bb::CLOWN;
                } else if self.wings {
                    self.phase = bb::W_WAIT;
                } else {
                    // a fresh Boom Boom (3 HP) starts in Defend
                    self.bb_enter_defend(false);
                }
            }
            Kind::BowserJr { clown, .. } => {
                self.hops = 3;
                self.set_byte(0, if self.big { 10 } else { 5 });
                self.home_x = self.x;
                self.pause = 0;
                self.jr_rearm_cooldown();
                if clown {
                    self.phase = jr::CLOWN;
                } else {
                    self.jr_enter_wait_or_fall();
                }
            }
            _ => {}
        }
    }

    /// A hit from `attack` (coming from `from_x`), through the boss HP rules.
    ///
    /// Bowser (helper 0x71006b4c20): fire 1, shell 3, Bob-omb blast [?] 3; a block hit only
    /// bumps him (HitBlock); stomps are Invalid. Boom Boom / Bowser Jr. (boss helper
    /// 0x71006116d0): a fireball drains the counter (5, big 10) and costs 1 HP when it empties
    /// (then refills); every other hit costs 1 HP; none while guarded. Star, POW quake and lava
    /// kill outright and are handled by the generic paths.
    pub fn boss_hit(&mut self, attack: BossAttack, from_x: f32) -> BossHit {
        if !self.alive() || !self.kind.is_boss() || self.boss_hidden() {
            return BossHit::Ignored;
        }
        if self.phase == 0 {
            // hit before its first step: set up with the SMB1 rules
            let mut lvl = Level::flat(1);
            lvl.style = "M1".into();
            self.boss_setup(&lvl);
        }
        if let Kind::Bowser { .. } = self.kind {
            let dmg: u8 = match attack {
                BossAttack::Stomp => return BossHit::Ignored,
                BossAttack::Fire => 1,
                BossAttack::Shell | BossAttack::Blast => 3,
                BossAttack::BlockHit => {
                    if self.phase != bw::CLOWN {
                        self.bowser_enter_hit_block();
                    }
                    return BossHit::Absorbed;
                }
            };
            self.hops = self.hops.saturating_sub(dmg);
            if self.hops == 0 {
                self.die_fall(from_x);
                return BossHit::Defeated;
            }
            self.kick_lock = 16;
            return BossHit::Hurt;
        }
        if self.boss_guarding() {
            return if attack == BossAttack::Stomp {
                BossHit::Guarded
            } else {
                BossHit::Ignored
            };
        }
        if attack == BossAttack::Fire {
            let n = self.byte(0).saturating_sub(1);
            if n > 0 {
                self.set_byte(0, n);
                self.kick_lock = 8;
                return BossHit::Absorbed;
            }
            self.set_byte(0, if self.big { 10 } else { 5 });
        }
        self.hops = self.hops.saturating_sub(1);
        if self.hops == 0 {
            self.die_fall(from_x);
            return BossHit::Defeated;
        }
        self.boss_damage_state();
        BossHit::Hurt
    }

    /// After losing HP: Boom Boom DamageDown (winged: PataDamageDown, keeping its wings), Bowser
    /// Jr. DamageDown. A Clown Car rider only flashes and stays in the car [?: the riders'
    /// Clown-context damage states are not decoded].
    fn boss_damage_state(&mut self) {
        if self.kind.boss_clown() {
            self.kick_lock = 60;
            return;
        }
        self.vx = 0.0;
        match self.kind {
            Kind::BoomBoom { pom: true, .. } => {
                self.phase = bb::POM_DAMAGE;
                self.vy = 2.5;
                self.on_ground = false;
                self.timer = 30;
            }
            Kind::BoomBoom { .. } => {
                self.phase = if self.wings {
                    bb::W_DAMAGE
                } else {
                    bb::DAMAGE_DOWN
                };
                self.vy = 0.0;
                self.timer = BB_DAMAGE;
            }
            Kind::BowserJr { .. } => {
                self.phase = jr::DAMAGE_DOWN;
                self.timer = JR_DAMAGE;
                self.vy = 3.0;
                self.on_ground = false;
            }
            _ => {}
        }
    }

    /// Koopa Clown Car ride [?: the car's Chase steering (0x71006dd810) is not decoded]: no
    /// gravity; eases toward the player's x and bobs ±8 px around its spawn height (3°/frame),
    /// facing the player.
    fn step_clown(&mut self, level: &Level) {
        let (px, _) = self.target;
        let want = ((px - self.x) / 48.0).clamp(-CLOWN_SPEED, CLOWN_SPEED);
        self.vx = Self::approach(self.vx, want, CLOWN_ACCEL);
        let ty = self.home_y + 8.0 * (self.age as f32 * 3.0).to_radians().dsin();
        self.vy = (ty - self.y).clamp(-1.0, 1.0);
        self.collide(level);
        self.dir = self.toward_target();
    }

    /// Flames and shuriken: straight flight through terrain. A Bowser flame with a lane
    /// (`home_y`) climbs or dives at half its forward speed until it is within half a step of
    /// the lane, then flies level (BulletKoopaFire ThreeWayFire 0x71003b2380).
    pub(super) fn step_boss_shot(&mut self) {
        if self.kind == Kind::BowserFire
            && !self.home_y.is_nan()
            && (self.y - self.home_y).abs() <= (self.vy * 0.5).abs()
        {
            self.vy = 0.0;
        }
        self.x += self.vx;
        self.y += self.vy;
        self.timer -= 1;
        if self.timer <= 0 {
            self.mode = Mode::Dead;
        }
    }

    // ------------------------------------------------------------ Bowser

    fn bowser_mode(&self) -> u8 {
        self.byte(3) & bwf::MODE
    }

    fn bowser_flag(&self, f: u8) -> bool {
        self.byte(3) & f != 0
    }

    fn bowser_set_flag(&mut self, f: u8, on: bool) {
        let b = self.byte(3);
        self.set_byte(3, if on { b | f } else { b & !f });
    }

    fn bowser_sub(&self) -> u8 {
        (self.byte(3) & bwf::SUB) >> 2
    }

    fn bowser_set_sub(&mut self, s: u8) {
        let b = self.byte(3);
        self.set_byte(3, (b & !bwf::SUB) | ((s << 2) & bwf::SUB));
    }

    fn bowser_bg_half(&self, st: usize) -> f32 {
        BOWSER_BG_HALF[st] * self.scale()
    }

    /// Fire interval: 40 + random(0…39) frames; chase mode 60 + random(0…39); off-screen
    /// mode 40 + random(0…239).
    fn bowser_fire_interval(&mut self) -> i32 {
        match self.bowser_mode() {
            3 => 40 + self.rand(240) as i32,
            2 => 60 + self.rand(40) as i32,
            _ => 40 + self.rand(40) as i32,
        }
    }

    /// Re-arms the fire timer and forgets any breath (helper +0x8 / +0x10 / +0x12).
    fn bowser_rearm_fire(&mut self) {
        self.pause = self.bowser_fire_interval();
        self.bowser_set_flag(bwf::FIRED | bwf::BREATHING, false);
    }

    /// Jump counter: SMB1 3 + random(0…1) decisions, other styles 1 + random(0…2); in the
    /// chase / off-screen modes 4 + the same random part.
    fn bowser_reset_jump(&mut self, st: usize) {
        let (base, span) = if st == 0 { (3, 2) } else { (1, 3) };
        let base = if self.bowser_mode() != 0 { 4 } else { base };
        let j = base + self.rand(span) as u8;
        self.set_byte(0, j);
    }

    /// Big-jump counter: 5 or 6 decisions (chase / off-screen modes 9 or 10).
    fn bowser_reset_big(&mut self) {
        let base = if self.bowser_mode() != 0 { 9 } else { 5 };
        let b = base + self.rand(2) as u8;
        self.set_byte(1, b);
    }

    /// The walk-speed setter (0x71006b1d20): 0.3 px/f along `heading`; chase mode 0.7 (SMB1)
    /// / 0.9; off-screen mode 0.
    fn bowser_walk_speed(&mut self, st: usize) {
        self.vx = match self.bowser_mode() {
            2 => BOWSER_CHASE[(st != 0) as usize] * self.heading,
            3 => 0.0,
            _ => BOWSER_WALK * self.heading,
        };
    }

    /// The player is behind him: past the back edge of his background box.
    fn bowser_player_behind(&self, st: usize) -> bool {
        let w = self.bowser_bg_half(st);
        if self.dir > 0 {
            self.target.0 < self.x - w
        } else {
            self.x + w < self.target.0
        }
    }

    /// Facing right with the player past his front edge (the cue to start chasing).
    fn bowser_player_ahead_right(&self, st: usize) -> bool {
        self.dir > 0 && self.on_ground && self.x + self.bowser_bg_half(st) < self.target.0
    }

    /// Animation-replacing state entries end a breath in progress; the fire timer then stays
    /// at 0 until something re-arms it (non-SMB1 Turn, LookAround, BigJump, HipDrop).
    fn bowser_cancel_breath(&mut self) {
        self.set_byte(2, 0);
    }

    fn bowser_enter(&mut self, state: u8, st: usize) {
        let m1 = st == 0;
        self.phase = state;
        match state {
            bw::FALL => {
                if self.bowser_mode() == 3 {
                    let b = self.byte(3) & !bwf::MODE;
                    self.set_byte(3, b);
                }
            }
            bw::LAND_ON => {
                self.vx = 0.0;
                self.timer = if m1 {
                    0
                } else if matches!(self.bowser_mode(), 1 | 2) {
                    10
                } else {
                    15
                };
            }
            bw::WALK => {
                self.bowser_walk_speed(st);
                let r = 20 + self.rand(20) as i32;
                self.timer = if self.bowser_mode() == 2 && self.toward_target() != self.dir {
                    20
                } else {
                    r
                };
            }
            bw::LITTLE_JUMP => {
                // the low hop 0.7 is for a ceiling found by the jump-height search, which
                // only reacts to one collider kind (36) the engine has no source of
                self.vy = 1.1;
                self.on_ground = false;
                self.bowser_walk_speed(st);
                self.timer = 3;
            }
            bw::MIDDLE_JUMP => {
                self.vy = if self.wings { 3.4 } else { 3.0 };
                self.on_ground = false;
                self.bowser_walk_speed(st);
                self.timer = 3;
            }
            bw::BIG_JUMP => {
                self.vx = 0.0;
                self.timer = 25;
                self.bowser_set_sub(0);
                self.bowser_cancel_breath();
            }
            bw::HIP_DROP => {
                self.vx = 0.0;
                self.vy = 3.0;
                self.timer = 0;
                self.bowser_set_sub(0);
                self.bowser_cancel_breath();
            }
            bw::LOOK_AROUND => {
                self.timer = 40;
                self.speed = -1.0;
                self.bowser_cancel_breath();
            }
            bw::TURN => {
                self.vx = 0.0;
                self.speed = 0.0;
                if !m1 {
                    self.bowser_cancel_breath();
                }
            }
            bw::FLY => {
                self.bowser_set_sub(0);
            }
            _ => {}
        }
    }

    /// HitBlock (a block bumped under him): hop vy 2.2 at +0.3 px/f (−0.3 in modes 1 and 3,
    /// the sign as stored by 0x71006a6b10), fire re-armed.
    fn bowser_enter_hit_block(&mut self) {
        self.phase = bw::HIT_BLOCK;
        self.vy = 2.2;
        self.on_ground = false;
        self.timer = 2;
        self.vx = if matches!(self.bowser_mode(), 1 | 3) {
            -0.3
        } else {
            0.3
        };
        self.bowser_rearm_fire();
        self.bowser_cancel_breath();
    }

    /// Mode change (0x71006b18c0): new mode, jump counter and fire timer for it, heading
    /// right, LandOn.
    fn bowser_set_mode(&mut self, mode: u8, st: usize) {
        let b = (self.byte(3) & !bwf::MODE) | (mode & bwf::MODE);
        self.set_byte(3, b);
        self.bowser_reset_jump(st);
        self.bowser_rearm_fire();
        self.heading = 1.0;
        self.bowser_enter(bw::LAND_ON, st);
    }

    /// Bowser (62-bowser.md, shared Koopa action). Walk (SMB1) or little hops (other styles)
    /// between decisions; a decision keeps him within ±64 px of home, turns him to face the
    /// player, and counts down to a middle jump (vy 3.0; winged 3.4 then a flutter) or, outside
    /// SMB1, a big jump (25-frame wind-up, vy 5.9; SMB3 6.2 aimed at the player, then a hip
    /// drop). Once the player gets past him he chases (0.7 / 0.9 px/f). Off screen with the
    /// player on his left he waits in place. The fire timer runs alongside in the ground and
    /// jump states: every 40–79 frames a breath (35 frames in SMB1), then a flame.
    pub(super) fn step_bowser(&mut self, level: &Level) -> Option<Actor> {
        if self.phase == 0 {
            self.boss_setup(level);
        }
        let st = style_index(level);
        if self.timer > 0 {
            self.timer -= 1;
        }
        // the fire animation plays on whatever the state does
        let b = self.byte(2);
        if b > 0 && b < u8::MAX {
            self.set_byte(2, b + 1);
        }
        if self.phase == bw::CLOWN {
            self.step_clown(level);
            return self.bowser_fire(st);
        }
        let state = self.phase;
        self.bowser_move(level, st);
        // the fire helper runs in Fall, LandOn, Walk, LittleJump, MiddleJump and a Fly that
        // came from a middle jump (0x71006b1de0)
        let fire_state = match state {
            bw::FALL | bw::LAND_ON | bw::WALK | bw::LITTLE_JUMP | bw::MIDDLE_JUMP => true,
            bw::FLY => !self.bowser_flag(bwf::FROM_BIG),
            _ => false,
        };
        if fire_state {
            self.bowser_fire(st)
        } else {
            self.bowser_set_flag(bwf::BREATHING, false);
            None
        }
    }

    /// Leaving a ground state when the floor is gone (walked off a ledge).
    fn bowser_ground_lost(&mut self, st: usize) -> bool {
        if self.on_ground {
            return false;
        }
        self.bowser_set_flag(bwf::FELL_OFF, true);
        self.bowser_enter(bw::FALL, st);
        true
    }

    fn bowser_move(&mut self, level: &Level, st: usize) {
        let m1 = st == 0;
        let m3 = st == 1;
        let breathing = self.bowser_flag(bwf::BREATHING);
        match self.phase {
            bw::FALL => {
                self.fall_capped(level, G, CAP);
                if self.bowser_flag(bwf::FELL_OFF) && self.timer == 0 && self.wall {
                    self.heading = -self.heading;
                    self.vx = self.vx.abs() * self.heading;
                    self.timer = 8;
                }
                if self.on_ground {
                    self.bowser_set_flag(bwf::FELL_OFF, false);
                    self.home_x = self.x;
                    if self.bowser_player_ahead_right(st) {
                        self.bowser_chase_dispatch(level, st);
                    } else {
                        self.bowser_enter(bw::LAND_ON, st);
                    }
                }
            }
            bw::LAND_ON => {
                self.fall(level, BOWSER_GRAVITY);
                if !self.bowser_ground_lost(st) && self.timer == 0 {
                    self.bowser_decide(level, st);
                }
            }
            bw::WALK => {
                self.bowser_walk_speed(st);
                if breathing {
                    self.vx = 0.0;
                }
                self.fall(level, BOWSER_GRAVITY);
                if !self.bowser_ground_lost(st) && self.timer == 0 {
                    self.bowser_decide(level, st);
                }
            }
            bw::LITTLE_JUMP => {
                self.fall(level, -0.07);
                if self.timer == 0 && self.on_ground {
                    self.bowser_enter(bw::LAND_ON, st);
                }
            }
            bw::MIDDLE_JUMP => {
                if m1 {
                    self.bowser_walk_speed(st);
                    if breathing {
                        self.vx = 0.0;
                    }
                }
                self.fall(level, BOWSER_GRAVITY);
                if self.wings {
                    self.bowser_set_flag(bwf::FROM_BIG, false);
                    self.bowser_enter(bw::FLY, st);
                } else if self.timer == 0 && self.on_ground {
                    self.bowser_enter(bw::LAND_ON, st);
                }
            }
            bw::BIG_JUMP => {
                self.fall_capped(level, G, CAP);
                match self.bowser_sub() {
                    0 => {
                        self.vx = 0.0;
                        if self.timer == 0 {
                            self.bowser_launch_big_jump(st);
                            if self.wings {
                                self.bowser_set_flag(bwf::FROM_BIG, true);
                                self.bowser_enter(bw::FLY, st);
                            } else {
                                self.bowser_set_sub(if m3 { 2 } else { 1 });
                            }
                        }
                    }
                    1 => {
                        if self.on_ground {
                            self.bowser_enter(bw::LAND_ON, st);
                        }
                    }
                    _ => {
                        if self.on_ground {
                            self.bowser_enter(bw::LAND_ON, st);
                        } else if self.vy <= 0.0 {
                            self.bowser_enter(bw::HIP_DROP, st);
                        }
                    }
                }
            }
            bw::HIP_DROP => {
                if self.timer == 0 && self.bowser_flag(bwf::SUB) {
                    // landed and the 40 frames are over
                    self.fall_capped(level, G, CAP);
                    if !self.bowser_ground_lost(st) {
                        self.bowser_set_sub(0);
                        self.bowser_enter(bw::LOOK_AROUND, st);
                    }
                    return;
                }
                if self.bowser_sub() == 0 {
                    self.fall_capped(level, -0.4, -8.0);
                    if self.on_ground {
                        // quake: players on the ground in view are stunned and the camera
                        // shakes when he is in view [the stun has no hook in the engine yet]
                        self.bowser_set_sub(1);
                        self.timer = 40;
                    }
                } else {
                    self.fall_capped(level, BOWSER_GRAVITY, CAP);
                }
            }
            bw::LOOK_AROUND => {
                self.vx = 0.0;
                self.fall(level, BOWSER_GRAVITY);
                if self.bowser_ground_lost(st) {
                    return;
                }
                if self.timer == 1 {
                    self.dir = if self.target.0 <= self.x { -1 } else { 1 };
                    self.speed = 0.0;
                } else if self.speed >= 0.0 {
                    // the turn animation; 10 frames in he moves on
                    self.speed += 1.0;
                    if self.speed >= 10.0 {
                        if self.bowser_player_ahead_right(st) {
                            self.bowser_chase_dispatch(level, st);
                        } else {
                            self.bowser_enter(bw::LAND_ON, st);
                        }
                    }
                }
            }
            bw::TURN => {
                self.fall(level, BOWSER_GRAVITY);
                self.speed += 1.0;
                if self.speed < BOWSER_TURN[st] as f32 {
                    return;
                }
                self.dir = -self.dir;
                if self.bowser_ground_lost(st) {
                    return;
                }
                if self.bowser_player_ahead_right(st) {
                    self.bowser_chase_dispatch(level, st);
                } else {
                    self.bowser_decide(level, st);
                }
            }
            bw::FLY => self.bowser_fly(level, st),
            bw::HIT_BLOCK => {
                self.fall_capped(level, G, CAP);
                if self.timer == 0 && self.on_ground {
                    self.bowser_enter(bw::LAND_ON, st);
                }
            }
            _ => {}
        }
    }

    /// Big-jump take-off: vy 5.9 (SMB3 6.2); horizontal ±2.0 along the heading in chase mode,
    /// aimed at the player in SMB3, otherwise random in [−2, 2) (winged ±1.5).
    fn bowser_launch_big_jump(&mut self, st: usize) {
        let base = if self.wings { 1.5 } else { 2.0 };
        self.vy = if st == 1 { 6.2 } else { 5.9 };
        self.on_ground = false;
        self.vx = if self.bowser_mode() == 2 {
            base * self.heading
        } else if st == 1 {
            self.bowser_aim_big_jump(base)
        } else {
            let span = if self.wings { 3.0 } else { 4.0 };
            span * self.rand(1 << 16) as f32 / 65536.0 - base
        };
    }

    /// SMB3 aim (0x71006ac720): within 128 px and with the player not to his right, half the
    /// time a random [−2, 2); otherwise the speed that reaches the player at the apex,
    /// |dx|·0.1875 / 5.9, at most 5 (halved with wings), toward the player.
    fn bowser_aim_big_jump(&mut self, base: f32) -> f32 {
        let dx = self.target.0 - self.x;
        if dx.abs() < 128.0 && dx <= 0.0 && self.rand(2) == 0 {
            return 2.0 * base * self.rand(1 << 16) as f32 / 65536.0 - base;
        }
        let mut v = (dx.abs() * -G / 5.9).min(5.0);
        if self.wings {
            v *= 0.5;
        }
        if dx < 0.0 { -v } else { v }
    }

    /// Winged Bowser's flutter (Fly 0x71006b1430): falls until vy ≤ −1.0, then gravity +0.05
    /// lifts him until vy ≥ 1.5 (or a ceiling), then −0.1875 to the ground. After an SMB3 big
    /// jump the descent ends in a hip drop (at once when he is within 3 px of the player).
    fn bowser_fly(&mut self, level: &Level, st: usize) {
        let from_big = st == 1 && self.bowser_flag(bwf::FROM_BIG);
        if from_big && (self.target.0 - self.x).abs() < 3.0 {
            self.bowser_enter(bw::HIP_DROP, st);
            return;
        }
        let sub = self.bowser_sub();
        let g = match sub {
            1 => 0.05,
            _ => G,
        };
        let before = self.vy;
        self.fall_capped(level, g, CAP);
        let ceiling = before > 0.0 && self.vy == 0.0 && !self.on_ground;
        match sub {
            0 => {
                if self.on_ground || self.vy <= -1.0 {
                    self.bowser_set_sub(1);
                }
            }
            1 => {
                if from_big {
                    let w = self.bowser_bg_half(st);
                    if (self.target.0 - self.x + w).abs() < 48.0 {
                        self.bowser_set_sub(2);
                        return;
                    }
                }
                if ceiling || self.vy >= 1.5 {
                    self.bowser_set_sub(if from_big { 3 } else { 2 });
                }
            }
            2 => {
                if self.on_ground {
                    self.bowser_enter(bw::LAND_ON, st);
                }
            }
            _ => {
                if self.vy <= 0.0 {
                    self.bowser_enter(bw::HIP_DROP, st);
                }
            }
        }
    }

    /// Decision (vtable slot 0x318, 0x71006a9690): patrol bounds, turn to the player, jump
    /// counters. Chase and off-screen modes have their own (below).
    fn bowser_decide(&mut self, level: &Level, st: usize) {
        match self.bowser_mode() {
            1 | 2 => return self.bowser_chase_dispatch(level, st),
            3 => return self.bowser_idle_decide(level, st),
            _ => {}
        }
        if self.x <= self.home_x - BOWSER_PATROL {
            self.heading = 1.0;
        } else if self.x >= self.home_x + BOWSER_PATROL {
            self.heading = -1.0;
        } else if self.wall
            || self.touching_wall(level, self.heading as i32)
            || self.ledge_ahead(level, self.heading)
        {
            self.heading = -self.heading;
        }
        if self.on_ground && self.bowser_player_behind(st) {
            self.bowser_enter(bw::TURN, st);
            return;
        }
        self.bowser_counters(st);
    }

    /// The jump-counter tail of a decision: both counters count down; at 0 the big jump (if
    /// allowed: not SMB1, not while breathing) or the middle jump, otherwise a walk (SMB1) or
    /// little hop. The jump-height search (0x71006ab020) only looks for one collider kind
    /// overhead, which the engine never has, so every jump goes ahead.
    fn bowser_counters(&mut self, st: usize) {
        let j = self.byte(0).saturating_sub(1);
        let b = self.byte(1).saturating_sub(1);
        self.set_byte(0, j);
        self.set_byte(1, b);
        let big_ok = st != 0 && !self.bowser_flag(bwf::BREATHING);
        if big_ok && b == 0 {
            self.bowser_reset_big();
            self.bowser_rearm_fire();
            self.bowser_enter(bw::BIG_JUMP, st);
        } else if j == 0 {
            self.bowser_reset_jump(st);
            self.bowser_enter(bw::MIDDLE_JUMP, st);
        } else {
            self.bowser_walk(st);
        }
    }

    fn bowser_walk(&mut self, st: usize) {
        if st == 0 {
            self.bowser_enter(bw::WALK, st);
        } else {
            self.bowser_enter(bw::LITTLE_JUMP, st);
        }
    }

    /// Off-screen mode 3 (0x71006a9970): in view → home here, mode 0, decide; otherwise stand
    /// (walk speed 0) and middle-jump when the jump counter runs out.
    fn bowser_idle_decide(&mut self, level: &Level, st: usize) {
        if self.in_view() {
            self.home_x = self.x;
            let b = self.byte(3) & !bwf::MODE;
            self.set_byte(3, b);
            return self.bowser_decide(level, st);
        }
        let j = self.byte(0);
        if j != 0 {
            self.set_byte(0, j - 1);
            if j - 1 != 0 {
                return self.bowser_walk(st);
            }
        }
        self.bowser_reset_jump(st);
        self.bowser_enter(bw::MIDDLE_JUMP, st);
    }

    /// Which chase mode (0x71006abe10): small Bowser outside SMB3 always chases fast (mode 2).
    /// Big or SMB3: fast within 640 px of the area's right end (not SMB3) or when he has
    /// fallen 32 px behind the view's left edge, otherwise at walking pace (mode 1).
    fn bowser_chase_dispatch(&mut self, level: &Level, st: usize) {
        let m3 = st == 1;
        if !m3 && !self.big {
            return self.bowser_chase(level, 2, st);
        }
        let cam_l = self.camera.0;
        let fast = if !m3 && self.x > level.right - 640.0 {
            true
        } else {
            match self.bowser_mode() {
                1 => self.x < cam_l - 32.0,
                2 => self.x <= cam_l + 32.0,
                _ => false,
            }
        };
        self.bowser_chase(level, if fast { 2 } else { 1 }, st);
    }

    /// Chase decision (0x71006ab370): the first time, switch mode. While the player stays
    /// past his front edge he heads that way (turning round at a ledge); otherwise home moves
    /// here and he goes back to patrolling. The obstacle and gap jumps it can also choose are
    /// not decoded [?].
    fn bowser_chase(&mut self, level: &Level, mode: u8, st: usize) {
        if self.bowser_mode() != mode {
            return self.bowser_set_mode(mode, st);
        }
        if self.on_ground && self.x + self.bowser_bg_half(st) < self.target.0 {
            self.heading = self.dir as f32;
            if self.bowser_player_behind(st) {
                return self.bowser_enter(bw::TURN, st);
            }
            if self.ledge_ahead(level, self.heading) {
                self.heading = -self.heading;
            }
            return self.bowser_counters(st);
        }
        self.home_x = self.x;
        let b = self.byte(3) & !bwf::MODE;
        self.set_byte(3, b);
        self.bowser_rearm_fire();
        self.bowser_decide(level, st);
    }

    /// The fire helper (0x71006a2710): counts down; at 1 the breath animation starts; the
    /// flame leaves at its fire frame (SMB1: when it ends), then the timer is re-armed.
    fn bowser_fire(&mut self, st: usize) -> Option<Actor> {
        if self.pause > 0 {
            self.pause -= 1;
            if self.pause == 1 {
                self.set_byte(2, 1);
            }
        }
        let frame = self.byte(2);
        let mut out = None;
        if frame > 0 {
            let (len, at) = BOWSER_BREATH[st];
            let f = frame - 1;
            if st != 0 && !self.bowser_flag(bwf::FIRED) && f == at {
                self.bowser_set_flag(bwf::FIRED, true);
                out = Some(self.bowser_flame(st));
            }
            if f >= len {
                if st == 0 {
                    out = Some(self.bowser_flame(st));
                }
                self.bowser_set_flag(bwf::FIRED, false);
                self.pause = self.bowser_fire_interval();
                self.set_byte(2, 0);
            }
        }
        self.bowser_set_flag(bwf::BREATHING, self.byte(2) > 0);
        out
    }

    /// A BulletKoopaFire (0x71006a2d40): at the mouth, 1.5 px/f plus Bowser's own forward
    /// speed [inf: the game averages a short speed history], one of three lanes at random.
    fn bowser_flame(&mut self, st: usize) -> Actor {
        let s = self.scale();
        let d = self.dir as f32;
        let (mx, my) = BOWSER_MOUTH[st];
        let mut f = Actor::new(Kind::BowserFire, self.x + d * mx * s, self.y + my * s);
        f.dir = self.dir;
        let carry = if self.vx * d > 0.0 { self.vx } else { 0.0 };
        f.vx = BOWSER_FIRE_SPEED * d + carry;
        let lane = FIRE_LANE * s;
        let (ty, vy) = match self.rand(3) {
            0 => (f.y, 0.0),
            1 => (f.y - lane, -f.vx.abs() * 0.5),
            _ => (f.y + lane, f.vx.abs() * 0.5),
        };
        f.home_y = ty;
        f.vy = vy;
        f.timer = 600;
        f
    }

    // ------------------------------------------------------------ Boom Boom

    fn bb_sub(&self) -> u8 {
        self.byte(3) & 0x03
    }

    fn bb_frame(&self) -> u8 {
        self.byte(3) >> 2
    }

    fn bb_set_sub(&mut self, s: u8) {
        self.set_byte(3, s & 0x03);
    }

    fn bb_tick_frame(&mut self) {
        let f = (self.bb_frame() + 1).min(63);
        self.set_byte(3, (self.byte(3) & 0x03) | (f << 2));
    }

    /// Boom Boom's flags (byte 2): guard, carried-over damage immunity, repeat swoop.
    fn bb_flag(&self, f: u8) -> bool {
        self.byte(2) & f != 0
    }

    fn bb_set_flag(&mut self, f: u8, on: bool) {
        let b = self.byte(2);
        self.set_byte(2, if on { b | f } else { b & !f });
    }

    fn bb_guard(&self) -> bool {
        self.bb_flag(BBF_GUARD) || self.bb_flag(BBF_IMMUNE)
    }

    /// Wall bumps on the right (dir > 0) / left side (byte 1, a nibble each).
    fn bb_bumps(&self, dir: i32) -> u8 {
        let b = self.byte(1);
        if dir > 0 { b & 0x0f } else { b >> 4 }
    }

    fn bb_set_bumps(&mut self, dir: i32, n: u8) {
        let b = self.byte(1);
        let n = n.min(15);
        self.set_byte(
            1,
            if dir > 0 {
                (b & 0xf0) | n
            } else {
                (b & 0x0f) | (n << 4)
            },
        );
    }

    /// The action timer after a jump or Defend: 240 + random(0…59) (30 + random at 2 HP).
    fn bb_rearm(&mut self) {
        let base = if self.hops == 2 { 30 } else { 240 };
        self.pause = base + self.rand(60) as i32;
    }

    fn bb_slow(&self) -> i32 {
        self.speed as i32
    }

    fn bb_target_in_range(&self) -> bool {
        let (px, py) = self.target;
        ((px - self.x).powi(2) + (py - self.y).powi(2)).sqrt() <= BB_RANGE
    }

    /// Boom Boom (BunbunM3, also SMB1/SMW). Starts curled up (Defend), then walks at 0.75 /
    /// 0.75 / 1.5 px/f by hits taken, re-aiming every 40 frames; a wall costs a WallJump (the
    /// 3rd in a row high), the 4th bump turns it. When the action timer (240–299) runs out:
    /// 3 HP → a 2.0 dash that brakes into Defend; 2 HP → Jump (5.5); 1 HP → keep running. A
    /// hit → DamageDown (70 frames, floating) → Defend (brake, 30 frames, a 2.75 hop at the
    /// player) → 60 frames slow → Walk. Winged: hover near the player, swoop at it.
    pub(super) fn step_boom_boom(&mut self, level: &Level) -> Option<Actor> {
        if self.phase == 0 {
            self.boss_setup(level);
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if let Kind::BoomBoom { pom: true, .. } = self.kind {
            return self.step_pom_pom(level);
        }
        match self.phase {
            bb::WALK => self.bb_walk(level),
            bb::JUMP | bb::WALL_JUMP => self.bb_jump(level),
            bb::DAMAGE_DOWN | bb::W_DAMAGE => {
                // floats: no gravity, no motion
                self.vx = 0.0;
                self.vy = 0.0;
                self.collide(level);
                if self.timer == 0 {
                    let wings = self.phase == bb::W_DAMAGE;
                    self.bb_enter_defend(true);
                    if wings {
                        self.phase = bb::W_DEFEND;
                    }
                }
            }
            bb::DEFEND | bb::W_DEFEND => self.bb_defend(level),
            bb::W_WAIT => {
                self.vx = Self::approach(self.vx, 0.0, 0.5);
                self.vy = Self::approach(self.vy, 0.0, 0.5);
                self.collide(level);
                // a player is always targeted here
                self.bb_enter_fly();
            }
            bb::W_FLY => self.bb_fly(level),
            bb::W_ATTACK => self.bb_attack(level),
            _ => {
                // Clown Car: the Clown context only rides (no attack)
                self.step_clown(level);
            }
        }
        None
    }

    fn bb_enter_walk(&mut self, from_defend: bool) {
        self.phase = bb::WALK;
        if from_defend {
            self.speed = BB_SLOW_FRAMES as f32;
        } else {
            self.dir = self.toward_target();
        }
    }

    fn bb_walk(&mut self, level: &Level) {
        let toward = self.toward_target();
        if self.pause < 1 {
            match self.hops {
                1 => self.pause = 240 + self.rand(60) as i32,
                2 => return self.bb_enter_jump(),
                _ => {
                    self.vx = BB_CHARGE * self.dir as f32;
                    return self.bb_enter_defend(false);
                }
            }
        }
        let lost = 3u8.saturating_sub(self.hops).min(2) as usize;
        let mut slow = self.bb_slow();
        if slow != 0 {
            slow -= 1;
            self.speed = slow as f32;
        }
        if self.pause != 0 {
            self.pause -= 1;
        }
        if !self.bb_target_in_range() {
            return self.bb_enter_defend(false);
        }
        if slow < 1 {
            if self.pause % 40 == 0 {
                self.dir = toward;
            }
            self.vx = BB_SPEED[lost] * self.dir as f32;
        } else {
            self.bb_rearm();
            self.vx = BB_SLOW * self.dir as f32;
        }
        self.fall(level, G);
        if self.wall {
            let side = self.dir;
            let n = self.bb_bumps(side);
            self.bb_set_bumps(side, n + 1);
            if n < BB_WALL_BUMPS {
                return self.bb_enter_wall_jump();
            }
            self.bb_set_bumps(side, 0);
            self.dir = -self.dir;
            self.vx = -self.vx;
        }
    }

    fn bb_enter_jump(&mut self) {
        self.phase = bb::JUMP;
        self.timer = BB_WINDUP;
        self.vx = 0.0;
        self.bb_set_sub(0);
    }

    fn bb_enter_wall_jump(&mut self) {
        self.phase = bb::WALL_JUMP;
        self.bb_set_sub(1);
        self.vy = if self.bb_bumps(self.dir) == 3 {
            BB_WALL_JUMP_HIGH
        } else {
            BB_WALL_JUMP
        };
        self.vx = BB_JUMP_VX * self.dir as f32;
        self.on_ground = false;
    }

    /// Jump / WallJump (0x71006361a0): a wall met in the air turns it while that side has no
    /// bumps; a landing pauses 30 frames after a Jump, 1 after a WallJump.
    fn bb_jump(&mut self, level: &Level) {
        if self.phase == bb::WALL_JUMP && self.pause != 0 {
            self.pause -= 1;
        }
        let vx = self.vx;
        self.fall(level, G);
        if self.wall && self.bb_bumps(self.dir) == 0 {
            self.dir = -self.dir;
            self.vx = vx.abs() * self.dir as f32;
        }
        match self.bb_sub() {
            0 => {
                self.bb_rearm();
                self.set_byte(1, 0);
                self.vx = 0.0;
                if self.timer == 0 {
                    self.dir = self.toward_target();
                    self.vx = BB_JUMP_VX * self.dir as f32;
                    self.vy = BB_JUMP;
                    self.on_ground = false;
                    self.bb_set_sub(1);
                }
            }
            1 => {
                if self.vy <= 0.0 && self.on_ground {
                    self.timer = if self.phase == bb::JUMP { 30 } else { 1 };
                    self.vx = 0.0;
                    self.vy = 0.0;
                    self.bb_set_sub(2);
                }
            }
            _ => {
                self.vx = 0.0;
                if self.timer == 0 {
                    if self.phase == bb::WALL_JUMP {
                        // WallJump exit: a cleared wall takes a bump back
                        let side = self.dir;
                        let n = self.bb_bumps(side);
                        if n > BB_WALL_BUMPS {
                            self.set_byte(1, 0);
                        } else if !self.touching_wall(level, side) {
                            self.bb_set_bumps(side, n.saturating_sub(1));
                        }
                    }
                    self.bb_enter_walk(false);
                }
            }
        }
    }

    fn touching_wall(&self, level: &Level, dir: i32) -> bool {
        let wx = self.x + dir as f32 * (self.half_w() + 0.5);
        [self.y + 2.0, self.y + self.height() - 2.0]
            .iter()
            .any(|&wy| level.cell_at(wx, wy).is_solid())
    }

    fn bb_enter_defend(&mut self, from_damage: bool) {
        self.phase = bb::DEFEND;
        self.bb_set_sub(0);
        self.bb_set_flag(BBF_GUARD, false);
        self.bb_set_flag(BBF_IMMUNE, from_damage);
        self.set_byte(1, 0);
    }

    /// Defend (0x7100635b20): squat until on the ground, then 30 frames while braking by 0.05;
    /// a hop at the player (vy 2.75, 1.0 px/f) with the stand-up animation; on landing the
    /// action timer is re-armed → Walk (slow). Winged (0x7100632cc0): the hop hovers at its
    /// apex and 12 frames after the animation it flies again.
    fn bb_defend(&mut self, level: &Level) {
        let winged = self.phase == bb::W_DEFEND;
        self.bb_tick_frame();
        let frame = self.bb_frame();
        // the squat: spiky (guarded) from its 7th frame, which also ends the damage immunity
        if self.bb_sub() < 2 && frame > 6 {
            self.bb_set_flag(BBF_IMMUNE, false);
            self.bb_set_flag(BBF_GUARD, true);
        }
        match self.bb_sub() {
            0 => {
                self.fall(level, G);
                if self.on_ground {
                    self.vy = 0.0;
                    self.timer = BB_DEFEND_WAIT;
                    self.set_byte(3, (self.byte(3) & 0xfc) | 1);
                }
            }
            1 => {
                if self.vx != 0.0 {
                    let v = self.vx - BB_DEFEND_BRAKE * self.vx.signum();
                    if v * self.vx > 0.0 {
                        self.vx = v;
                        self.fall(level, G);
                        return;
                    }
                    self.vx = 0.0;
                }
                self.fall(level, G);
                if self.timer != 0 {
                    return;
                }
                self.dir = self.toward_target();
                self.vx = self.dir as f32;
                self.vy = BB_DEFEND_HOP;
                self.on_ground = false;
                self.set_byte(3, 2);
            }
            2 => {
                if frame > 9 {
                    self.bb_set_flag(BBF_GUARD, false);
                }
                if winged && self.vy <= 0.0 {
                    self.vy = 0.0;
                    self.collide(level);
                } else {
                    self.fall(level, G);
                }
                if frame >= BB_ANIM {
                    if winged {
                        self.timer = 12;
                    }
                    self.bb_set_sub(3);
                }
            }
            _ => {
                if winged {
                    if self.vy <= 0.0 {
                        self.vy = 0.0;
                    }
                    self.collide(level);
                    if self.timer == 0 {
                        self.phase = bb::W_FLY;
                        self.bb_enter_fly();
                    }
                } else {
                    self.fall(level, G);
                    if self.on_ground {
                        self.bb_rearm();
                        self.bb_enter_walk(true);
                    }
                }
            }
        }
    }

    fn bb_enter_fly(&mut self) {
        self.phase = bb::W_FLY;
        self.timer = 240 + self.rand(60) as i32;
        self.bb_set_flag(BBF_REPEAT, self.hops == 1);
    }

    /// The point a winged Boom Boom hovers around (Wing slot 6, 0x7100631a80).
    fn bb_fly_home(&self) -> (f32, f32) {
        let (cx, cy) = self.view_centre();
        let hx = (self.target.0 + 1.2 * TILE * self.dir as f32).clamp(cx - 128.0, cx + 128.0);
        (hx, cy + 3.5 * TILE)
    }

    /// Fly (0x71006321e0): ±2.0 px/f toward the hover point (0.05 px/f²), ±1.0 vertically in
    /// 0.075 steps; after 240–299 frames, with the player in view, Attack.
    fn bb_fly(&mut self, level: &Level) {
        let (hx, hy) = self.bb_fly_home();
        self.dir = if hx - self.x <= 0.0 { -1 } else { 1 };
        self.vx = Self::approach(self.vx, BB_FLY_SPEED * self.dir as f32, 0.05);
        let ty = if hy - self.y > 0.0 { 1.0 } else { -1.0 };
        self.vy = Self::approach(self.vy, ty, 0.075);
        self.collide(level);
        if self.timer == 0 {
            self.bb_enter_attack();
        }
    }

    fn bb_enter_attack(&mut self) {
        self.phase = bb::W_ATTACK;
        self.vx = 0.0;
        self.vy = 0.0;
        self.timer = 60;
        self.bb_set_sub(0);
    }

    /// Attack (0x71006324a0): 60 frames still, then a straight dive at where the player was
    /// (over 60 frames at 3 HP, else 40) while the horizontal speed builds to 2.0 (0.025 px/f²);
    /// below the target it pulls up at the dive's speed (steps 0.5, 0.15 at 3 HP, 0.04 once
    /// rising) and climbs back to its start height, then flies (at 1 HP it swoops twice).
    fn bb_attack(&mut self, level: &Level) {
        match self.bb_sub() {
            0 => {
                self.vx = 0.0;
                self.vy = 0.0;
                if self.timer == 0 {
                    let (px, py) = self.target;
                    let n = if self.hops == 3 { 60.0 } else { 40.0 };
                    self.trail = vec![(px, py), (self.x, self.y)];
                    self.dir = if px - self.x < 0.0 { -1 } else { 1 };
                    self.vx = (px - self.x) / n;
                    self.vy = (py - self.y) / n;
                    self.bb_set_sub(1);
                }
            }
            1 => {
                let (tx, ty) = self.trail.first().copied().unwrap_or(self.target);
                self.dir = if tx - self.x <= 0.0 { -1 } else { 1 };
                self.vx = Self::approach(self.vx, BB_FLY_SPEED * self.dir as f32, BB_SWOOP_ACCEL);
                if self.y < ty + 4.0 {
                    self.speed = -self.vy;
                    self.bb_set_sub(2);
                }
            }
            _ => {
                let step = if self.vy >= 0.0 {
                    0.04
                } else if self.hops == 3 {
                    0.15
                } else {
                    0.5
                };
                let target = self.speed.max(-1.2);
                self.vy = Self::approach(self.vy, target, step);
                let (hx, _) = self.bb_fly_home();
                self.dir = if hx - self.x <= 0.0 { -1 } else { 1 };
                self.vx = Self::approach(self.vx, BB_FLY_SPEED * self.dir as f32, 0.05);
                let start_y = self.trail.get(1).map_or(self.y, |p| p.1);
                if self.y >= start_y {
                    self.collide(level);
                    if self.bb_flag(BBF_REPEAT) {
                        self.bb_set_flag(BBF_REPEAT, false);
                        self.bb_enter_attack();
                    } else {
                        self.phase = bb::W_FLY;
                        self.timer = 240 + self.rand(60) as i32;
                    }
                    return;
                }
            }
        }
        self.collide(level);
    }

    /// Pom Pom (Punpun, 3D World), partly decoded: waits 60 frames facing the player; on foot
    /// she then throws (ThrowSign → Throw), winged she first jumps (vy 6.0 for 15 frames, then
    /// hovers bobbing 0.4·sin at 8°/frame) and throws from the air. A hit: a 2.5 hop, then she
    /// vanishes for 120 frames (Lapse) and takes 60 frames to reappear. [?] the throw timing,
    /// the shuriken's flight and where she reappears are not decoded (shown as before: three
    /// aimed stars 12 frames apart at 2 px/f, reappearing 64 px on the player's far side).
    fn step_pom_pom(&mut self, level: &Level) -> Option<Actor> {
        let toward = self.toward_target();
        let mut shot = None;
        match self.phase {
            bb::POM_WAIT => {
                self.vx = 0.0;
                self.dir = toward;
                self.fall(level, G);
                if self.timer == 0 {
                    if self.wings {
                        self.phase = bb::POM_JUMP;
                        self.vy = 6.0;
                        self.on_ground = false;
                        self.timer = 15;
                        self.heading = 0.0;
                    } else {
                        self.phase = bb::POM_THROW_SIGN;
                        self.pause = 20;
                    }
                }
            }
            bb::POM_JUMP => {
                if self.timer > 0 {
                    let before = self.vy;
                    self.collide(level);
                    if before > 0.0 && self.vy == 0.0 {
                        self.timer = 0;
                    }
                } else {
                    self.heading += 8.0;
                    self.vy = 0.4 * self.heading.to_radians().dsin();
                    self.collide(level);
                    if self.heading >= 45.0 * 8.0 {
                        self.phase = bb::POM_THROW_SIGN;
                        self.pause = 20;
                    }
                }
            }
            bb::POM_THROW_SIGN => {
                self.vx = 0.0;
                if self.wings {
                    self.heading += 8.0;
                    self.vy = 0.4 * self.heading.to_radians().dsin();
                    self.collide(level);
                } else {
                    self.fall(level, G);
                }
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = bb::POM_THROW;
                    self.pause = 36;
                }
            }
            bb::POM_THROW => {
                if self.wings {
                    self.vy = 0.0;
                    self.collide(level);
                } else {
                    self.fall(level, G);
                }
                if self.pause % 12 == 0 {
                    let (px, py) = self.target;
                    let (sx, sy) = (self.x, self.y + 16.0 * self.scale());
                    let (dx, dy) = (px - sx, py + 8.0 - sy);
                    let d = (dx * dx + dy * dy).sqrt().max(1.0);
                    let mut s = Actor::new(Kind::Shuriken, sx, sy - 4.0);
                    s.vx = 2.0 * dx / d;
                    s.vy = 2.0 * dy / d;
                    s.timer = 300;
                    shot = Some(s);
                }
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = bb::POM_WAIT;
                    self.timer = 60;
                }
            }
            bb::POM_DAMAGE => {
                self.vx = 0.0;
                self.fall(level, G);
                if self.timer == 0 && self.on_ground {
                    self.phase = bb::POM_LAPSE;
                    self.pause = 120 + 60;
                }
            }
            bb::POM_LAPSE => {
                self.vx = 0.0;
                self.vy = 0.0;
                self.pause -= 1;
                if self.pause <= 0 {
                    let (px, py) = self.target;
                    self.x = px - toward as f32 * 64.0;
                    self.y = py;
                    self.phase = bb::POM_WAIT;
                    self.timer = 60;
                }
            }
            _ => {
                self.phase = bb::POM_WAIT;
                self.timer = 60;
            }
        }
        shot
    }

    // ------------------------------------------------------------ Bowser Jr.

    fn jr_rearm_cooldown(&mut self) {
        let c = 90 + self.rand(45) as u8;
        self.set_byte(2, c);
    }

    fn jr_enter_wait_or_fall(&mut self) {
        if self.on_ground {
            self.jr_enter_wait();
        } else {
            self.phase = jr::FALL;
        }
    }

    fn jr_enter_wait(&mut self) {
        self.phase = jr::WAIT;
        self.timer = JR_WAIT;
    }

    fn jr_sub(&self) -> u8 {
        self.byte(3)
    }

    fn jr_set_sub(&mut self, s: u8) {
        self.set_byte(3, s);
    }

    /// Bowser Jr. (KoopaJr Basic action). Wait 20 frames facing the player, then: after a
    /// 90–134-frame cooldown (counted in Wait) a shift jump; otherwise (unless he has just
    /// shot) 70 % fire — winged 75 % hammers — when the player is in view; else a computed hop
    /// to the ground 30 px ahead, or a walk (1.25 px/f, winged 0.8) that stops 24 px from home.
    /// Shift jump: straight up 5.5 → at vy ≤ 0.4 PrevHipDrop (winged: Hover 50–74 frames
    /// first) 20 frames → HipDrop (g −0.75, cap −8) → 50 frames → Wait. A hit: DamageDown
    /// (bounce 3.0, 15 frames) → the shell → Wait with home where he stands.
    pub(super) fn step_bowser_jr(&mut self, level: &Level) -> Option<Actor> {
        if self.phase == 0 {
            self.boss_setup(level);
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        let st = style_index(level);
        let mut out = None;
        match self.phase {
            jr::WAIT => self.jr_wait(level),
            jr::FALL => {
                self.fall_capped(level, G, CAP);
                if self.on_ground {
                    self.jr_enter_wait();
                }
            }
            jr::HOP => {
                self.fall_capped(level, G, CAP);
                if self.on_ground && self.vy <= 0.0 {
                    self.vx = 0.0;
                    self.jr_enter_wait();
                }
            }
            jr::WALK => self.jr_walk(level),
            jr::SHOT => {
                if self.on_ground {
                    self.vx = Self::approach(self.vx, 0.0, 0.5);
                }
                self.fall_capped(level, G, CAP);
                self.pause += 1;
                out = if self.wings {
                    self.jr_hammers()
                } else {
                    self.jr_fire_cycle()
                };
            }
            jr::SHIFT_JUMP => {
                self.fall_capped(level, G, CAP);
                if self.vy <= JR_HOVER_AT {
                    self.jr_rearm_cooldown();
                    if self.wings {
                        self.phase = jr::HOVER;
                        self.timer = 50 + self.rand(25) as i32;
                        self.heading = 0.0;
                    } else {
                        self.jr_enter_prev_hip_drop(st);
                    }
                }
            }
            jr::HOVER => {
                self.heading += 8.0;
                self.vy = 0.4 * self.heading.to_radians().dsin();
                self.vx = 0.0;
                self.collide(level);
                self.dir = self.toward_target();
                if self.timer == 0 {
                    self.jr_enter_prev_hip_drop(st);
                }
            }
            jr::PREV_HIP_DROP => {
                self.vx = 0.0;
                self.vy = 0.0;
                if self.timer == 0 {
                    self.phase = jr::HIP_DROP;
                }
            }
            jr::HIP_DROP => {
                self.vx = 0.0;
                self.fall_capped(level, JR_HIP_GRAVITY, JR_HIP_CAP);
                if self.on_ground {
                    // quake: players on the ground in view are stunned [no engine hook yet]
                    self.phase = jr::HIP_LAND;
                    self.timer = JR_HIP_LAND;
                    self.vy = self.vy.max(-1.5);
                }
            }
            jr::HIP_LAND => {
                self.fall_capped(level, G, CAP);
                if self.timer == 0 {
                    self.jr_rearm_cooldown();
                    self.jr_enter_wait();
                }
            }
            jr::DAMAGE_DOWN => {
                self.vx = 0.0;
                self.fall_capped(level, G, CAP);
                if self.timer == 0 {
                    // ShellIn: outside NSMBU the shell context starts at once
                    self.phase = jr::SHELL_WAIT;
                    self.timer = JR_SHELL_WAIT;
                }
            }
            jr::SHELL_WAIT => {
                self.vx = 0.0;
                self.fall_capped(level, G, CAP);
                if self.timer == 0 {
                    self.phase = jr::SHELL;
                    self.timer = JR_SHELL_FRAMES;
                    self.vx = JR_SHELL_SPEED * self.toward_target() as f32;
                }
            }
            jr::SHELL => {
                let vx = self.vx;
                self.fall_capped(level, G, CAP);
                if self.wall {
                    self.vx = -vx;
                }
                if self.timer == 0 {
                    self.phase = jr::SHELL_OUT;
                    self.timer = JR_SHELL_OUT;
                }
            }
            jr::SHELL_OUT => {
                self.vx = Self::approach(self.vx, 0.0, 0.2);
                self.fall_capped(level, G, CAP);
                if self.timer == 0 {
                    // the Basic action restarts: home is here (0x71006e9a80)
                    self.home_x = self.x;
                    self.jr_rearm_cooldown();
                    self.jr_enter_wait_or_fall();
                }
            }
            _ => out = self.jr_clown(level),
        }
        out
    }

    fn jr_enter_prev_hip_drop(&mut self, st: usize) {
        self.phase = jr::PREV_HIP_DROP;
        self.vx = 0.0;
        self.vy = 0.0;
        self.timer = if st == 3 { 10 } else { 20 };
    }

    /// Wait (0x71006ea960).
    fn jr_wait(&mut self, level: &Level) {
        if self.on_ground {
            self.vx = 0.0;
        }
        self.dir = self.toward_target();
        let cooldown = self.byte(2);
        let cooldown = if cooldown > 0 {
            self.set_byte(2, cooldown - 1);
            cooldown - 1
        } else {
            0
        };
        self.fall_capped(level, G, CAP);
        if self.timer != 0 {
            return;
        }
        let just_shot = self.jr_sub() == 1;
        if cooldown == 0 && self.on_ground {
            self.jr_set_sub(0);
            self.phase = jr::SHIFT_JUMP;
            self.vx = 0.0;
            self.vy = if self.big {
                JR_SHIFT_JUMP_BIG
            } else {
                JR_SHIFT_JUMP
            };
            self.on_ground = false;
            return;
        }
        if !(self.on_ground && just_shot) && self.player_in_view() {
            let r = self.rand(100);
            if r < if self.wings { 75 } else { 70 } {
                self.phase = jr::SHOT;
                self.pause = 0;
                self.jr_set_sub(1);
                self.set_byte(1, 0);
                return;
            }
        }
        self.jr_set_sub(0);
        if !self.on_ground {
            return;
        }
        // which way: toward home, or at random within 24 px of it
        let heading = if (self.x - self.home_x).abs() <= JR_TURN_RANGE {
            if self.rand(2) == 0 { 1 } else { -1 }
        } else if self.home_x < self.x {
            -1
        } else {
            1
        };
        let ceiling = level
            .cell_at(self.x, self.y + self.height() + 16.0)
            .is_solid();
        if !ceiling {
            for d in [heading, -heading] {
                // SMB1 jumps at once; the other styles first finish a wind-up animation whose
                // length is not in the 2D sprite data [?] (taken as 0)
                if let Some(v) = self.jr_hop_probe(level, d) {
                    self.phase = jr::HOP;
                    self.vx = v.0 * d as f32;
                    self.vy = v.1;
                    self.on_ground = false;
                    return;
                }
            }
        }
        self.dir = if self.rand(2) == 0 { 1 } else { -1 };
        self.phase = jr::WALK;
        self.timer = JR_WALK_FRAMES;
    }

    /// The player is inside the view [inf: 0x710035f7f0 is a view test on the player's box;
    /// its margins were not read].
    fn player_in_view(&self) -> bool {
        let (px, py) = self.target;
        let (cx, cy) = self.camera;
        px > cx && px < cx + VIEW_W && py > cy - 32.0 && py < cy + VIEW_H
    }

    /// The hop probe: 30 px ahead at feet + 4 and feet + 20; unless both are walls, look down
    /// from there (128 px when heading toward the player, else 16) for ground, and compute the
    /// jump that lands on it.
    fn jr_hop_probe(&self, level: &Level, d: i32) -> Option<(f32, f32)> {
        let df = d as f32;
        let ex = self.x + df * JR_PROBE;
        let wall_at = |y: f32| {
            let mut s = 1.0;
            while s <= JR_PROBE {
                if level.cell_at(self.x + df * s, y).is_solid() {
                    return true;
                }
                s += 4.0;
            }
            false
        };
        let low = wall_at(self.y + 4.0);
        let high = wall_at(self.y + 20.0);
        if low && high {
            return None;
        }
        let start = if low { self.y + 20.0 } else { self.y + 4.0 };
        let depth = if d == self.dir { 128.0 } else { 16.0 };
        let mut y = start;
        let mut land = None;
        while y >= self.y - depth {
            if level.cell_at(ex, y).is_solid() {
                land = Some(((y / TILE).floor() + 1.0) * TILE);
                break;
            }
            y -= 1.0;
        }
        let ly = land?;
        let v = jr_jump_velocity(JR_PROBE, ly - self.y, self.wings)?;
        // the arc must be clear of terrain (0x7100619f50) [inf: sampled at the feet and head
        // each frame rather than with the game's background sensors]
        let (mut x, mut yy, mut vy) = (self.x, self.y, v.1);
        for _ in 0..180 {
            vy = (vy + G).max(CAP);
            x += v.0 * df;
            yy += vy;
            if vy < 0.0 && yy <= ly {
                break;
            }
            let h = self.height();
            if level.cell_at(x, yy + 1.0).is_solid() || level.cell_at(x, yy + h - 1.0).is_solid() {
                return None;
            }
        }
        Some(v)
    }

    /// Walk (0x71006eb460): toward 1.25 (winged 0.8) in 0.1 steps; stops (→ Wait) 24 px from
    /// home, at a ledge or a wall, or when the 120 frames are up.
    fn jr_walk(&mut self, level: &Level) {
        let speed = if self.wings { JR_WALK_WINGS } else { JR_WALK };
        self.vx = Self::approach(self.vx, speed * self.dir as f32, JR_WALK_STEP);
        if (self.vx < 0.0 && self.x < self.home_x - JR_TURN_RANGE)
            || (self.vx > 0.0 && self.x > self.home_x + JR_TURN_RANGE)
        {
            self.vx = 0.0;
            self.fall_capped(level, G, CAP);
            return self.jr_enter_wait();
        }
        self.fall_capped(level, G, CAP);
        let ahead = self.x + self.dir as f32 * 4.0;
        let depth = if self.dir == self.toward_target() {
            128
        } else {
            16
        };
        let floor = (0..=depth)
            .step_by(4)
            .any(|dy| level.cell_at(ahead, self.y - 1.0 - dy as f32).is_solid());
        if !floor || self.wall {
            self.vx = 0.0;
            return self.jr_enter_wait();
        }
        if self.timer == 0 {
            self.jr_enter_wait();
        }
    }

    /// Fire (0x71006e5e20): three cycles of the 60-frame fire animation, a fireball 26
    /// frames into each.
    fn jr_fire_cycle(&mut self) -> Option<Actor> {
        let t = self.pause;
        let out = if t % JR_FIRE_ANIM == JR_FIRE_AT && t / JR_FIRE_ANIM < 3 {
            Some(self.jr_fireball())
        } else {
            None
        };
        if t >= 3 * JR_FIRE_ANIM {
            self.jr_enter_wait();
        }
        out
    }

    /// Hammers (winged, 0x71006e5790): four thrown 36, 52, 68 and 84 frames into the
    /// 110-frame throw animation.
    fn jr_hammers(&mut self) -> Option<Actor> {
        let t = self.pause;
        let out = JR_THROW_AT.contains(&t).then(|| self.make_hammer());
        if t >= JR_THROW_ANIM {
            self.jr_enter_wait();
        }
        out
    }

    /// A Bowser Jr. fireball: from 12 px ahead / 12 up (big 24 / 18), flying straight at the
    /// player's chest but within 25° of level, at 1.5 px/f plus his forward speed × cos²θ.
    fn jr_fireball(&mut self) -> Actor {
        let d = self.dir as f32;
        let (ox, oy) = if self.big { (24.0, 18.0) } else { (12.0, 12.0) };
        let (sx, sy) = (self.x + d * ox, self.y + oy);
        let (px, py) = self.target;
        let mut a = (py + 12.0 - sy).datan2((px - sx) * d).to_degrees();
        a = a.clamp(-JR_AIM, JR_AIM);
        let (sn, cs) = a.to_radians().dsin_cos();
        let carry = if self.vx * d > 0.0 {
            self.vx.abs()
        } else {
            0.0
        };
        let speed = BOWSER_FIRE_SPEED + carry * cs * cs;
        let mut f = Actor::new(Kind::BowserFire, sx, sy);
        f.dir = self.dir;
        f.vx = cs * speed * d;
        f.vy = sn * speed;
        f.home_y = f32::NAN;
        f.timer = 600;
        f
    }

    /// The Clown Car machine (init 0x71006e81c0): every 150 frames that the player is within
    /// 80 px or he is well inside the view, he ducks (10 frames), throws two shells (one each
    /// way) after 30 frames, comes back up (10 frames) and at 1 HP also fires (or, winged,
    /// throws hammers). The thrown shells' own launch is not decoded [?].
    fn jr_clown(&mut self, level: &Level) -> Option<Actor> {
        self.step_clown(level);
        let mut out = None;
        match self.phase {
            jr::CLOWN => {
                let near = (self.target.0 - self.x).abs() < 80.0
                    || (self.x >= self.camera.0 + 32.0 && self.x <= self.camera.0 + VIEW_W - 32.0);
                if near {
                    if self.pause > 0 {
                        self.pause -= 1;
                    }
                    if self.pause == 0 {
                        self.phase = jr::C_PRE_THROW;
                        self.timer = 10;
                    }
                }
            }
            jr::C_PRE_THROW => {
                if self.timer == 0 {
                    self.phase = jr::C_THROW;
                    self.timer = 30;
                    self.jr_set_sub(0);
                }
            }
            jr::C_THROW => {
                if self.timer == 0 {
                    // two shells, left and right (one per frame: the engine spawns one actor
                    // per step)
                    let k = self.jr_sub();
                    let d = if k == 0 { -1 } else { 1 };
                    let mut s = Actor::new(
                        Kind::Shell { red: false },
                        self.x + d as f32 * 8.0 * self.scale(),
                        self.y + 16.0 * self.scale(),
                    );
                    s.mode = Mode::ShellWait;
                    s.timer = SHELL_WAKE;
                    s.vx = d as f32;
                    s.vy = 2.0;
                    out = Some(s);
                    if k == 0 {
                        self.jr_set_sub(1);
                    } else {
                        self.phase = jr::C_POST_THROW;
                        self.timer = 10;
                    }
                }
            }
            jr::C_POST_THROW => {
                if self.timer == 0 {
                    if self.hops == 1 {
                        self.phase = jr::C_SHOT;
                        self.pause = 0;
                    } else {
                        self.phase = jr::CLOWN;
                        self.pause = JR_CLOWN_ATTACK;
                    }
                }
            }
            _ => {
                self.pause += 1;
                out = if self.wings {
                    self.jr_hammers()
                } else {
                    self.jr_fire_cycle()
                };
                if self.phase == jr::WAIT {
                    self.phase = jr::CLOWN;
                    self.pause = JR_CLOWN_ATTACK;
                }
            }
        }
        out
    }
}

/// Boom Boom flag bits (chain byte 2).
const BBF_GUARD: u8 = 0x01;
const BBF_IMMUNE: u8 = 0x02;
const BBF_REPEAT: u8 = 0x04;

/// The boss jump solver (0x7100619690): the launch (vx ≥ 0, vy) that lands `dy` above (or
/// below) a point `dx` away under gravity −0.1875 / fall cap −4, clearing 16 px (32 winged)
/// above the higher end; a steeper arc is tried while the speed would exceed 3 px/f.
fn jr_jump_velocity(dx: f32, dy: f32, winged: bool) -> Option<(f32, f32)> {
    let mut k = if winged { 2.0 } else { 1.0 };
    for _ in 0..8 {
        let h = if dy <= 0.0 { 16.0 * k } else { dy + 16.0 * k };
        let mut vy = (2.0 * -G * h).sqrt();
        for _ in 0..5 {
            let (mut v, mut rise) = (vy, 0.0);
            for _ in 0..180 {
                let n = v + G;
                if n <= 0.0 {
                    break;
                }
                v = n.max(CAP);
                rise += v;
            }
            if (h - rise).abs() <= 0.3 || rise <= 0.0 {
                break;
            }
            vy *= (h / rise).sqrt();
        }
        vy = vy.min(10.0);
        let (mut v, mut y, mut n) = (vy, 0.0, 0);
        loop {
            v = (v + G).max(CAP);
            y += v;
            if (v < 0.0 && y <= dy) || n > 400 {
                break;
            }
            n += 1;
        }
        if n > 0 {
            let vx = dx.abs() / n as f32;
            if vx <= 3.0 {
                return Some((vx, vy));
            }
        }
        k += 1.0;
    }
    None
}

/// A sprite choice: model, texture, width, height (texture size, ×2 when big), mirror.
pub type BossLook = (&'static str, &'static str, f32, f32, bool);

impl Actor {
    /// Model and texture for a boss or boss projectile; None while invisible (vanished, or the
    /// off-beat of the hurt flash). Every M1 boss texture faces left.
    pub fn boss_look(&self) -> Option<BossLook> {
        let flash = self.kick_lock > 0 && (self.kick_lock / 2) % 2 == 0;
        if flash || self.boss_hidden() {
            return None;
        }
        let cycle =
            |frames: &[&'static str], rate: u32| frames[(self.age / rate) as usize % frames.len()];
        let face = self.dir > 0;
        Some(match self.kind {
            Kind::Bowser { .. } => {
                let tex = if self.boss_breathing() {
                    "fire.0"
                } else if !self.on_ground && self.phase != bw::CLOWN {
                    cycle(&["fall.0", "fall.1"], 5)
                } else {
                    cycle(&["wait.0", "wait.1"], 20)
                };
                ("M1_Enemy_koopa", tex, 48.0, 48.0, face)
            }
            Kind::BowserFire => (
                "M1_Enemy_koopa",
                cycle(&["effect.0", "effect.1"], 2),
                24.0,
                16.0,
                self.vx > 0.0,
            ),
            Kind::BoomBoom { .. } => {
                const WALK: [&str; 6] =
                    ["walk.0", "walk.1", "walk.2", "walk.3", "walk.4", "walk.5"];
                const FLY: [&str; 6] = ["fly.0", "fly.1", "fly.2", "fly.3", "fly.4", "fly.5"];
                let f = (self.bb_frame() as usize / 4).min(2);
                let tex = match self.phase {
                    bb::W_DAMAGE => cycle(&["damage_fly.0", "damage_fly.1"], 5),
                    bb::DAMAGE_DOWN | bb::POM_DAMAGE => cycle(&["damage.0", "damage.1"], 5),
                    bb::DEFEND | bb::W_DEFEND if self.bb_sub() < 2 => {
                        ["squat.0", "squat.1", "squat.2"][f]
                    }
                    bb::DEFEND | bb::W_DEFEND => ["standup.0", "standup.1", "standup.2"][f],
                    bb::JUMP if self.bb_sub() == 0 => "jump_st.0",
                    bb::POM_THROW_SIGN => "jump_st.0",
                    bb::JUMP | bb::WALL_JUMP | bb::POM_JUMP => {
                        if self.vy > 0.0 {
                            "jump_ed.0"
                        } else {
                            "jump_ed.1"
                        }
                    }
                    bb::W_WAIT | bb::W_FLY | bb::W_ATTACK | bb::CLOWN => cycle(&FLY, 5),
                    // walk_ftp (24 frames, uneven keys); at 1 HP walk2_ftp (18, every 3)
                    _ if self.hops == 1 => cycle(&WALK, 3),
                    _ => {
                        const KEYS: [u32; 6] = [0, 6, 9, 12, 18, 21];
                        let t = self.age % 24;
                        WALK[KEYS.iter().rposition(|&k| k <= t).unwrap_or(0)]
                    }
                };
                ("M1_Enemy_bunbun", tex, 32.0, 32.0, face)
            }
            // [inf] placeholder art: Pom Pom's shuriken uses Magikoopa's magic sparkle (the
            // 3D World art is not a 2D sprite)
            Kind::Shuriken => (
                "M1_Enemy_kameck",
                cycle(&["effect.0", "effect.1", "effect.2"], 4),
                16.0,
                16.0,
                false,
            ),
            Kind::BowserJr { .. } => {
                if self.boss_in_shell() {
                    let tex = if self.phase == jr::SHELL {
                        cycle(&["shell.0", "shell.1", "shell.2", "shell.3"], 4)
                    } else {
                        "shell.0"
                    };
                    return Some(("M1_Enemy_koopaJr", tex, 16.0, 16.0, face));
                }
                let tex = match self.phase {
                    jr::SHOT | jr::C_SHOT if self.wings => {
                        let t = self.pause % JR_THROW_ANIM;
                        if t < 30 || (t / 10) % 2 == 0 {
                            "throw.0"
                        } else {
                            "throw.1"
                        }
                    }
                    jr::SHOT | jr::C_SHOT if self.pause % JR_FIRE_ANIM < 25 => "fire.0",
                    jr::SHOT | jr::C_SHOT => "fire.1",
                    jr::PREV_HIP_DROP | jr::HIP_DROP => "before.0",
                    jr::CLOWN | jr::C_PRE_THROW | jr::C_THROW | jr::C_POST_THROW => "wait.0",
                    _ if !self.on_ground => cycle(&["fall.0", "fall.1"], 5),
                    _ => cycle(&["wait.0", "wait.1"], 10),
                };
                ("M1_Enemy_koopaJr", tex, 32.0, 32.0, face)
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boss(kind: Kind, x: f32) -> (Level, Actor) {
        let level = Level::flat(80);
        let mut a = Actor::new(kind, x, 32.0);
        a.on_ground = true;
        a.dir = -1;
        a.target = (x - 100.0, 32.0);
        // the camera shows the boss
        a.camera = (x - VIEW_W / 2.0, 0.0);
        (level, a)
    }

    const BOWSER: Kind = Kind::Bowser {
        key: false,
        clown: false,
    };
    const BOOM_BOOM: Kind = Kind::BoomBoom {
        key: false,
        clown: false,
        pom: false,
    };
    const BOWSER_JR: Kind = Kind::BowserJr {
        key: false,
        clown: false,
    };

    #[test]
    fn bowser_paces_within_64_px_of_home_at_0_3() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        let home = a.home_x;
        let (mut lo, mut hi) = (home, home);
        let mut walk_speeds = Vec::new();
        for _ in 0..4000 {
            // the player stays on his left, so he never chases
            a.target.0 = home - 150.0;
            let _ = a.step_spawning(&level);
            lo = lo.min(a.x);
            hi = hi.max(a.x);
            if a.phase == bw::WALK && a.on_ground && a.vx != 0.0 {
                walk_speeds.push(a.vx.abs());
            }
        }
        // a decision comes at most 39 frames (≤ 12 px) after crossing a bound
        assert!(
            lo >= home - 64.0 - 12.0 && hi <= home + 64.0 + 12.0,
            "{lo}..{hi}"
        );
        assert!(hi - lo > 64.0, "actually paces: {lo}..{hi}");
        assert!(walk_speeds.iter().all(|&v| (v - 0.3).abs() < 1e-6));
    }

    #[test]
    fn smb1_bowser_breathes_35_frames_then_fires_every_40_to_79_more() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let mut shots = Vec::new();
        let mut breath_len = 0;
        let mut breathing = 0;
        for f in 0..3000 {
            if let Some(fire) = a.step_spawning(&level) {
                assert_eq!(fire.kind, Kind::BowserFire);
                // 1.5 px/f plus his own forward speed (0 or the 0.3 walk)
                let carry = fire.vx.abs() - BOWSER_FIRE_SPEED;
                assert!((-1e-6..=0.3 + 1e-6).contains(&carry), "{}", fire.vx);
                assert_eq!(fire.vx.signum() as i32, a.dir);
                // three lanes: level, or 17 px up / down
                let lane = fire.home_y - fire.y;
                assert!([-17.0, 0.0, 17.0].iter().any(|l| (lane - l).abs() < 1e-3));
                shots.push(f);
                breath_len = breath_len.max(breathing);
            }
            if a.boss_breathing() {
                breathing += 1;
            } else {
                breathing = 0;
            }
        }
        assert!(shots.len() > 20);
        assert_eq!(breath_len, 35, "the SMB1 fire animation is 35 frames");
        for w in shots.windows(2) {
            let gap = w[1] - w[0];
            assert!((39 + 35..=79 + 35).contains(&gap), "gap {gap}");
        }
    }

    #[test]
    fn bowser_stands_still_while_breathing_in_smb1() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let mut was = (0, false, 0);
        let mut checked = 0;
        for _ in 0..2000 {
            let _ = a.step_spawning(&level);
            // the walk reads the breath flag the fire helper left on the previous frame (its
            // entry, also on a decision that walks again, still sets the walking speed)
            let same_walk = was.0 == bw::WALK && a.timer < was.2;
            if a.phase == bw::WALK && same_walk && was.1 && a.boss_breathing() {
                assert_eq!(a.vx, 0.0);
                checked += 1;
            }
            was = (a.phase, a.boss_breathing(), a.timer);
        }
        assert!(checked > 0);
    }

    #[test]
    fn bowser_flame_climbs_to_its_lane_then_flies_level() {
        let mut f = Actor::new(Kind::BowserFire, 100.0, 52.0);
        f.vx = -1.5;
        f.vy = 0.75;
        f.home_y = 69.0;
        f.timer = 600;
        for _ in 0..40 {
            f.step_boss_shot();
        }
        assert!((f.y - 69.25).abs() < 1e-3, "{}", f.y);
        assert_eq!(f.vy, 0.0);
        assert!((f.x - 40.0).abs() < 1e-3);
    }

    #[test]
    fn smb1_bowser_middle_jumps_3_0_and_never_big_jumps() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let mut jumps = 0;
        for _ in 0..3000 {
            let was = a.phase;
            let _ = a.step_spawning(&level);
            if a.phase == bw::MIDDLE_JUMP && was != a.phase {
                jumps += 1;
                assert!((a.vy - 3.0).abs() < 1e-6);
            }
            assert!(!matches!(a.phase, bw::BIG_JUMP | bw::LITTLE_JUMP));
        }
        assert!(jumps >= 5, "{jumps} middle jumps");
    }

    #[test]
    fn smb1_bowser_turns_on_the_spot_when_the_player_gets_behind() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        assert_eq!(a.dir, -1);
        // the player goes past his back edge (13 px)
        a.target.0 = a.x + 14.0;
        let mut turned = None;
        for f in 0..200 {
            let _ = a.step_spawning(&level);
            if a.dir > 0 {
                turned = Some(f);
                break;
            }
        }
        assert!(turned.is_some(), "turned round");
    }

    #[test]
    fn small_bowser_chases_the_player_who_got_past_him() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        let mut top: f32 = 0.0;
        for _ in 0..600 {
            a.target.0 = a.x + 100.0;
            let _ = a.step_spawning(&level);
            if a.phase == bw::WALK && !a.boss_breathing() {
                top = top.max(a.vx);
            }
        }
        assert_eq!(a.bowser_mode(), 2, "chase mode");
        assert!((top - 0.7).abs() < 1e-6, "SMB1 chase speed {top}");
    }

    #[test]
    fn off_screen_bowser_waits_until_in_view() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        a.camera = (0.0, 0.0); // the view is far to his left
        let x0 = a.x;
        for _ in 0..600 {
            let _ = a.step_spawning(&level);
        }
        assert_eq!(a.bowser_mode(), 3);
        assert!((a.x - x0).abs() < 1e-3, "stands in place");
        a.camera = (a.x - VIEW_W / 2.0, 0.0);
        for _ in 0..200 {
            let _ = a.step_spawning(&level);
        }
        assert_eq!(a.bowser_mode(), 0, "patrols once on screen");
    }

    #[test]
    fn smb3_bowser_big_jumps_then_hip_drops_and_looks_around() {
        let (mut level, mut a) = boss(BOWSER, 30.0 * TILE);
        level.style = "M3".into();
        let mut top_vy: f32 = 0.0;
        let mut hip = false;
        let mut looked = false;
        for _ in 0..3000 {
            let _ = a.step_spawning(&level);
            if a.phase == bw::BIG_JUMP {
                top_vy = top_vy.max(a.vy);
            }
            hip |= a.phase == bw::HIP_DROP;
            looked |= hip && a.phase == bw::LOOK_AROUND;
        }
        assert!((top_vy - 6.2).abs() < 1e-4, "{top_vy}");
        assert!(hip && looked);
    }

    #[test]
    fn a_block_bump_hops_bowser_without_damage() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        assert_eq!(a.boss_hit(BossAttack::BlockHit, a.x), BossHit::Absorbed);
        assert_eq!(a.phase, bw::HIT_BLOCK);
        assert!((a.vy - 2.2).abs() < 1e-6);
        assert_eq!(a.boss_hp(), 20);
    }

    #[test]
    fn boom_boom_starts_curled_up_then_walks_slowly() {
        let (level, mut a) = boss(BOOM_BOOM, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        assert_eq!(a.phase, bb::DEFEND);
        let mut hop = None;
        for _ in 0..200 {
            let _ = a.step_spawning(&level);
            if a.vy > 2.0 && hop.is_none() {
                hop = Some(a.vy);
            }
            if a.phase == bb::WALK {
                break;
            }
        }
        assert!((hop.expect("hopped") - BB_DEFEND_HOP).abs() < 0.2);
        assert_eq!(a.phase, bb::WALK);
        let _ = a.step_spawning(&level);
        assert!((a.vx.abs() - BB_SLOW).abs() < 1e-6, "slow walk {}", a.vx);
    }

    #[test]
    fn boom_boom_wall_jumps_three_times_the_third_high_then_turns() {
        let (mut level, mut a) = boss(BOOM_BOOM, 30.0 * TILE);
        // a wall 6 tiles high just right of it, the player beyond it
        for y in 2..8 {
            level.set_solid(33, y);
        }
        a.target = (34.0 * TILE, 32.0);
        a.dir = 1;
        let mut wall_jumps = Vec::new();
        for _ in 0..2000 {
            let was = a.phase;
            let _ = a.step_spawning(&level);
            a.target = (a.x + 40.0, 32.0);
            if a.phase == bb::WALL_JUMP && was != bb::WALL_JUMP {
                wall_jumps.push(a.vy);
            }
            if a.dir < 0 && a.phase == bb::WALK {
                break;
            }
        }
        assert_eq!(wall_jumps.len(), 3, "{wall_jumps:?}");
        assert!((wall_jumps[0] - BB_WALL_JUMP).abs() < 1e-6);
        assert!((wall_jumps[2] - BB_WALL_JUMP_HIGH).abs() < 1e-6);
        assert_eq!(a.dir, -1, "turned after the 4th bump");
    }

    #[test]
    fn boom_boom_floats_while_stunned() {
        let (level, mut a) = boss(BOOM_BOOM, 30.0 * TILE);
        let _ = a.step_spawning(&level);
        a.y += 20.0;
        a.on_ground = false;
        assert_eq!(a.boss_hit(BossAttack::Shell, a.x), BossHit::Hurt);
        let y = a.y;
        for _ in 0..60 {
            let _ = a.step_spawning(&level);
        }
        assert_eq!(a.phase, bb::DAMAGE_DOWN);
        assert_eq!(a.y, y, "no gravity in DamageDown");
        assert!(a.boss_guarding());
    }

    #[test]
    fn winged_boom_boom_hovers_then_swoops_and_keeps_its_wings() {
        let (level, mut a) = boss(BOOM_BOOM, 30.0 * TILE);
        a.wings = true;
        a.y = 120.0;
        a.on_ground = false;
        a.target = (a.x - 60.0, 32.0);
        let mut attack = None;
        for f in 0..600 {
            let _ = a.step_spawning(&level);
            if a.phase == bb::W_ATTACK && attack.is_none() {
                attack = Some(f);
            }
            assert!(a.vx.abs() <= BB_FLY_SPEED + 1e-6);
        }
        let f = attack.expect("swooped");
        assert!((239..=302).contains(&f), "attack at {f}");
        // a stomp costs 1 HP but not the wings
        let mut b = a.clone();
        b.phase = bb::W_FLY;
        assert_eq!(b.boss_hit(BossAttack::Stomp, b.x), BossHit::Hurt);
        assert!(b.wings);
        assert_eq!(b.phase, bb::W_DAMAGE);
        assert_eq!(b.boss_hp(), 2);
    }

    #[test]
    fn bowser_jr_shift_jumps_straight_up_and_hip_drops_to_minus_8() {
        let (level, mut a) = boss(BOWSER_JR, 30.0 * TILE);
        let mut jumped = false;
        let mut min_vy: f32 = 0.0;
        for _ in 0..1500 {
            let was = a.phase;
            let _ = a.step_spawning(&level);
            if a.phase == jr::SHIFT_JUMP && was != jr::SHIFT_JUMP {
                jumped = true;
                assert_eq!(a.vx, 0.0);
                assert!((a.vy - JR_SHIFT_JUMP).abs() < 1e-6);
            }
            if a.phase == jr::HIP_DROP {
                min_vy = min_vy.min(a.vy);
            }
            assert_ne!(a.phase, jr::HOVER, "only winged Bowser Jr. hovers");
        }
        assert!(jumped);
        assert!(min_vy <= JR_HIP_CAP + 1e-6, "{min_vy}");
    }

    #[test]
    fn winged_bowser_jr_hovers_50_to_74_frames() {
        let (level, mut a) = boss(BOWSER_JR, 30.0 * TILE);
        a.wings = true;
        let mut hovers = Vec::new();
        let mut len = 0;
        for _ in 0..4000 {
            let _ = a.step_spawning(&level);
            if a.phase == jr::HOVER {
                len += 1;
            } else if len > 0 {
                hovers.push(len);
                len = 0;
            }
        }
        assert!(!hovers.is_empty());
        assert!(hovers.iter().all(|h| (49..=74).contains(h)), "{hovers:?}");
    }

    #[test]
    fn bowser_jr_fires_three_aimed_shots_per_volley() {
        let (level, mut a) = boss(BOWSER_JR, 30.0 * TILE);
        a.target = (a.x - 120.0, 32.0);
        let mut volleys: Vec<Vec<f32>> = Vec::new();
        let mut was = 0;
        for _ in 0..3000 {
            let out = a.step_spawning(&level);
            if a.phase == jr::SHOT && was != jr::SHOT {
                volleys.push(Vec::new());
            }
            if let Some(f) = out {
                assert_eq!(f.kind, Kind::BowserFire);
                let speed = (f.vx * f.vx + f.vy * f.vy).sqrt();
                assert!((speed - 1.5).abs() < 1e-4);
                assert!(f.vy.datan2(-f.vx).to_degrees().abs() <= JR_AIM + 1e-3);
                volleys.last_mut().expect("in a volley").push(f.x);
            }
            was = a.phase;
        }
        volleys.pop(); // the last one may be cut off
        assert!(!volleys.is_empty());
        assert!(volleys.iter().all(|v| v.len() == 3), "{volleys:?}");
    }

    #[test]
    fn bowser_jr_hops_onto_a_step_ahead() {
        let (mut level, mut a) = boss(BOWSER_JR, 30.0 * TILE);
        // a one-tile step 1–2 tiles ahead in every direction he might pick
        for x in [27, 28, 32, 33] {
            level.set_solid(x, 2);
        }
        let mut hop = false;
        for _ in 0..600 {
            let _ = a.step_spawning(&level);
            hop |= a.phase == jr::HOP && a.vy > 0.0;
        }
        assert!(hop, "hopped");
    }

    #[test]
    fn pom_pom_hops_vanishes_after_a_hit_and_reappears() {
        let (level, mut a) = boss(
            Kind::BoomBoom {
                key: false,
                clown: false,
                pom: true,
            },
            30.0 * TILE,
        );
        let _ = a.step_spawning(&level);
        assert_eq!(a.boss_hit(BossAttack::Stomp, a.x), BossHit::Hurt);
        assert!((a.vy - 2.5).abs() < 1e-6, "hop 2.5");
        let mut hidden = 0;
        for _ in 0..400 {
            let _ = a.step_spawning(&level);
            if a.boss_hidden() {
                hidden += 1;
            } else if hidden > 0 {
                break;
            }
        }
        assert!(hidden >= 120, "{hidden}");
        assert!(!a.boss_hidden());
    }

    #[test]
    fn jump_solver_lands_on_target() {
        let (vx, vy) = jr_jump_velocity(30.0, 16.0, false).expect("solution");
        // simulate
        let (mut x, mut y, mut v) = (0.0f32, 0.0f32, vy);
        loop {
            v = (v + G).max(CAP);
            x += vx;
            y += v;
            if v < 0.0 && y <= 16.0 {
                break;
            }
        }
        assert!((x - 30.0).abs() <= vx + 1e-3, "{x}");
        assert!(vx <= 3.0);
    }
}
