//! Bosses: Bowser (62-bowser.md), Boom Boom and Pom Pom (77-boom-boom.md) and Bowser Jr.
//! (98-bowser-jr.md). Inferred choices are marked [inf]; DECISIONS.md lists them.
//!
//! State layout on the shared actor fields: HP in `hops`; `phase` is the state (0 = not set up
//! yet); `timer` and `pause` are state timers; `chain` holds small counters, one per byte
//! (Bowser: jump / big-jump decision counters; Boom Boom and Bowser Jr.: the fireball counter in
//! byte 0; Boom Boom: right / left wall bumps in bytes 1 / 2 and Defend hops in byte 3; Pom Pom:
//! the attack alternation in byte 1). Bowser walks along `heading` (±1) while facing `dir`.
//! `kick_lock` (counted down by the generic step) is the hurt flash, and for a boss in a Clown
//! Car also its invulnerability.

use super::{Actor, Kind, MAX_FALL, Mode, SHELL_WAKE};
use crate::level::{Cell, Level};

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
    /// Taken without losing HP (a fireball drained the counter; a block shoved Bowser).
    Absorbed,
    /// Lost HP.
    Hurt,
    /// HP ran out: knocked off (DieFall).
    Defeated,
}

// ---------------------------------------------------------------- Bowser constants

/// HP (helper +0x18): 20, big 40.
const BOWSER_HP: u8 = 20;
const BOWSER_PATROL: f32 = 64.0;
const BOWSER_WALK: f32 = 0.3;
const BOWSER_GRAVITY: f32 = -0.1;
/// [inf] the "player behind" sensor half-width (scaled ×2 when big).
const BOWSER_SENSOR: f32 = 16.0;
/// [inf] frames of the breath animation before the flame leaves (SMB1 fires at its end).
const BOWSER_BREATH: i32 = 30;
/// BulletKoopaFire speed field 1.5 [obs; its use as the horizontal speed is inf].
const BOWSER_FIRE_SPEED: f32 = 1.5;
/// [inf] vertical speed at which a flame settles onto its lane.
const FIRE_SETTLE: f32 = 0.5;

/// Bowser states (numbering ours; names from the state table).
mod bw {
    pub const WALK: u8 = 1;
    pub const LITTLE_JUMP: u8 = 2;
    pub const TURN: u8 = 3;
    pub const LAND_ON: u8 = 4;
    pub const MIDDLE_JUMP: u8 = 5;
    pub const FLY: u8 = 6;
    pub const BEFORE_BIG_JUMP: u8 = 7;
    pub const BIG_JUMP: u8 = 8;
    pub const HIP_DROP: u8 = 9;
    pub const LOOK_AROUND: u8 = 10;
    pub const HIT_BLOCK: u8 = 11;
    pub const CLOWN: u8 = 12;
}

// ---------------------------------------------------------------- Boom Boom constants

/// Walk speed by hits taken (0, 1, 2): table indexed by 3 − HP.
const BB_SPEED: [f32; 3] = [0.75, 0.75, 1.5];
const BB_CHARGE: f32 = 2.0;
const BB_SLOW: f32 = 0.35;
/// [inf] Jump 5.0 and WallJump 3.75 (the spec leaves which value belongs to which open).
const BB_JUMP: f32 = 5.0;
const BB_WALL_JUMP: f32 = 3.75;
const BB_WINDUP: i32 = 30;
const BB_WALL_BUMPS: u8 = 3;
const BB_DAMAGE: i32 = 70;
const BB_DEFEND_HOP: f32 = 2.75;
const BB_DEFEND_ACCEL: f32 = 0.05;
const BB_DEFEND_MIN: i32 = 30;
/// [inf] the spin's top speed (the charge speed) and the hops before it ends.
const BB_DEFEND_MAX: f32 = 2.0;
const BB_DEFEND_HOPS: u8 = 3;
/// [inf] frames of the charge before Defend, and of the slow walk after it.
const BB_CHARGE_FRAMES: i32 = 60;
const BB_SLOW_FRAMES: i32 = 60;
/// [inf] winged cruise speed (the acceleration 0.05 and vertical step 0.075 are obs).
const BB_FLY_SPEED: f32 = 1.0;
const BB_SWOOP_FRAMES: i32 = 60;

mod bb {
    pub const WALK: u8 = 1;
    pub const JUMP_WINDUP: u8 = 2;
    pub const JUMP: u8 = 3;
    pub const WALL_WINDUP: u8 = 4;
    pub const WALL_JUMP: u8 = 5;
    pub const DAMAGE_DOWN: u8 = 6;
    pub const DEFEND: u8 = 7;
    pub const CHARGE: u8 = 8;
    pub const SLOW: u8 = 9;
    pub const FLY: u8 = 10;
    pub const SWOOP: u8 = 11;
    pub const CLOWN: u8 = 12;
    // Pom Pom [inf throughout: her constants were not extracted]
    pub const POM_WAIT: u8 = 20;
    pub const POM_JUMP: u8 = 21;
    pub const POM_THROW_SIGN: u8 = 22;
    pub const POM_THROW: u8 = 23;
    pub const POM_LAPSE: u8 = 24;
}

// ---------------------------------------------------------------- Bowser Jr. constants

const JR_GRAVITY: f32 = -0.1875;
const JR_WAIT: i32 = 20;
const JR_WALK_FRAMES: i32 = 120;
const JR_WALK: f32 = 0.8;
const JR_WALK_ANGRY: f32 = 1.25;
const JR_WALK_STEP: f32 = 0.1;
const JR_TURN_RANGE: f32 = 24.0;
/// The Wait probe's look-ahead distance (80 px) [obs]; using it as "near" is [inf].
const JR_NEAR: f32 = 80.0;
const JR_HOP_FRAMES: i32 = 25;
/// [inf] hop and shift-jump launch speeds (not decoded).
const JR_HOP: f32 = 2.5;
const JR_SHIFT_JUMP: f32 = 5.0;
const JR_HOVER_AT: f32 = 0.4;
const JR_HIP_GRAVITY: f32 = -0.75;
const JR_HIP_CAP: f32 = -8.0;
const JR_HIP_LAND: i32 = 50;
/// DamageDown 40 frames plus its 15-frame second phase.
const JR_DAMAGE: i32 = 55;
const JR_SHELL_IN: i32 = 23;
/// [inf] the Shell context: slides at 2.0 for 150 frames, then 20 frames to come out.
const JR_SHELL_SPEED: f32 = 2.0;
const JR_SHELL_FRAMES: i32 = 150;
const JR_SHELL_OUT: i32 = 20;
/// [inf] the shot animation: the projectile leaves 20 frames in, back to Wait at 40.
const JR_SHOT_AT: i32 = 20;
const JR_SHOT_END: i32 = 40;

mod jr {
    pub const WAIT: u8 = 1;
    pub const WALK: u8 = 2;
    pub const SHOT_FIRE: u8 = 3;
    pub const SHOT_HAMMER: u8 = 4;
    pub const HOP: u8 = 5;
    pub const SHIFT_JUMP: u8 = 6;
    pub const HOVER: u8 = 7;
    pub const PREV_HIP_DROP: u8 = 8;
    pub const HIP_DROP: u8 = 9;
    pub const HIP_LAND: u8 = 10;
    pub const DAMAGE_DOWN: u8 = 11;
    pub const SHELL_IN: u8 = 12;
    pub const SHELL: u8 = 13;
    pub const SHELL_OUT: u8 = 14;
    pub const CLOWN: u8 = 15;
}

/// [inf] Clown Car riders attack every 120 frames.
const CLOWN_ATTACK: i32 = 120;

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
        self.vy = (self.vy + g).max(MAX_FALL);
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

    /// Stunned or spinning: immune, stomps bounce off (the boss action's invulnerable flag) [inf
    /// which states set it].
    pub fn boss_guarding(&self) -> bool {
        if self.kind.boss_clown() && self.kick_lock > 0 {
            return true;
        }
        match self.kind {
            Kind::BoomBoom { .. } => matches!(self.phase, bb::DAMAGE_DOWN | bb::DEFEND),
            Kind::BowserJr { .. } => matches!(
                self.phase,
                jr::DAMAGE_DOWN | jr::SHELL_IN | jr::SHELL | jr::SHELL_OUT
            ),
            _ => false,
        }
    }

    /// Bowser Jr. tucked into his shell (drawn as the shell).
    pub fn boss_in_shell(&self) -> bool {
        matches!(self.kind, Kind::BowserJr { .. })
            && matches!(self.phase, jr::SHELL_IN | jr::SHELL | jr::SHELL_OUT)
    }

    /// Bowser is breathing fire.
    pub fn boss_breathing(&self) -> bool {
        matches!(self.kind, Kind::Bowser { .. }) && self.pause < 0
    }

    /// Animation hints for the renderer: the state number (see the state modules here).
    pub fn boss_state(&self) -> u8 {
        self.phase
    }

    fn boss_setup(&mut self, style: &str) {
        let m1 = style == "M1";
        match self.kind {
            Kind::Bowser { clown, .. } => {
                self.hops = if self.big { 2 * BOWSER_HP } else { BOWSER_HP };
                self.heading = self.dir as f32;
                let j = self.bowser_jump_count(m1);
                let b = 5 + self.rand(2) as u8;
                self.set_byte(0, j);
                self.set_byte(1, b);
                self.pause = self.bowser_fire_interval();
                self.timer = 20 + self.rand(20) as i32;
                self.phase = if clown { bw::CLOWN } else { bw::WALK };
            }
            Kind::BoomBoom { clown, pom, .. } => {
                self.hops = 3;
                self.set_byte(0, if self.big { 10 } else { 5 });
                self.timer = 240 + self.rand(60) as i32;
                self.phase = if pom {
                    self.timer = 60;
                    bb::POM_WAIT
                } else if clown {
                    bb::CLOWN
                } else if self.wings {
                    bb::FLY
                } else {
                    bb::WALK
                };
                self.pause = CLOWN_ATTACK;
            }
            Kind::BowserJr { clown, .. } => {
                self.hops = 3;
                self.set_byte(0, if self.big { 10 } else { 5 });
                self.timer = JR_WAIT;
                self.pause = CLOWN_ATTACK;
                self.phase = if clown { jr::CLOWN } else { jr::WAIT };
            }
            _ => {}
        }
    }

    /// A hit from `attack` (coming from `from_x`), through the boss HP rules.
    ///
    /// Bowser (helper 0x71006b4c20): fire 1, shell 3, Bob-omb blast [inf] 3; a block hit only
    /// shoves him (HitBlock, ±0.3 px/f); stomps are Invalid. Boom Boom / Bowser Jr. (boss
    /// helper 0x71006116d0): a fireball drains the counter (5, big 10) and costs 1 HP when it
    /// empties (then refills); every other hit costs 1 HP; none while stunned or spinning.
    /// Star, POW quake and lava kill outright and are handled by the generic paths.
    pub fn boss_hit(&mut self, attack: BossAttack, from_x: f32) -> BossHit {
        if !self.alive() || !self.kind.is_boss() || self.boss_hidden() {
            return BossHit::Ignored;
        }
        if self.phase == 0 {
            self.boss_setup("M1");
        }
        if let Kind::Bowser { .. } = self.kind {
            let dmg: u8 = match attack {
                BossAttack::Stomp => return BossHit::Ignored,
                BossAttack::Fire => 1,
                BossAttack::Shell | BossAttack::Blast => 3,
                BossAttack::BlockHit => {
                    if self.phase != bw::CLOWN {
                        // [inf] the shove lasts 16 frames
                        self.phase = bw::HIT_BLOCK;
                        self.timer = 16;
                        self.vx = 0.3 * if self.x < from_x { -1.0 } else { 1.0 };
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

    /// After losing HP: DamageDown (a winged Boom Boom loses its wings, PataDamageDown [inf]).
    /// A Clown Car rider only flashes and stays in the car [inf].
    fn boss_damage_state(&mut self) {
        if self.kind.boss_clown() {
            self.kick_lock = 60;
            return;
        }
        self.vx = 0.0;
        match self.kind {
            Kind::BoomBoom { .. } => {
                self.wings = false;
                self.phase = bb::DAMAGE_DOWN;
                self.pause = BB_DAMAGE;
            }
            Kind::BowserJr { .. } => {
                self.phase = jr::DAMAGE_DOWN;
                self.pause = JR_DAMAGE;
                self.vy = 3.0;
                self.on_ground = false;
            }
            _ => {}
        }
    }

    /// Koopa Clown Car ride [inf: the Clown context was not decoded]: no gravity; eases toward
    /// the player's x (≤ 1 px/f, 0.03 px/f²) and bobs ±8 px around its spawn height (3°/frame),
    /// facing the player. Returns true when its attack timer (`pause`) fires.
    fn step_clown(&mut self, level: &Level) -> bool {
        let (px, _) = self.target;
        let want = ((px - self.x) / 48.0).clamp(-1.0, 1.0);
        self.vx += (want - self.vx).clamp(-0.03, 0.03);
        let ty = self.home_y + 8.0 * (self.age as f32 * 3.0).to_radians().sin();
        self.vy = (ty - self.y).clamp(-1.0, 1.0);
        self.collide(level);
        self.dir = self.toward_target();
        self.pause -= 1;
        if self.pause <= 0 {
            self.pause = CLOWN_ATTACK;
            return true;
        }
        false
    }

    /// Flames and shuriken: straight flight through terrain; a flame eases onto its lane.
    pub(super) fn step_boss_shot(&mut self) {
        self.x += self.vx;
        if self.kind == Kind::BowserFire {
            self.y += (self.home_y - self.y).clamp(-FIRE_SETTLE, FIRE_SETTLE);
        } else {
            self.y += self.vy;
        }
        self.timer -= 1;
        if self.timer <= 0 {
            self.mode = Mode::Dead;
        }
    }

    // ------------------------------------------------------------ Bowser

    /// Jump counter: SMB1 3 + random(0…1) decisions, other styles 1 + random(0…2).
    fn bowser_jump_count(&mut self, m1: bool) -> u8 {
        if m1 {
            3 + self.rand(2) as u8
        } else {
            1 + self.rand(3) as u8
        }
    }

    /// Fire interval 40 + random(0…39) frames (the helper's "mode" variants are not used [inf]).
    fn bowser_fire_interval(&mut self) -> i32 {
        40 + self.rand(40) as i32
    }

    /// Bowser (62-bowser.md, shared Koopa action). Walk (SMB1) or little hops (other styles)
    /// for 20–39 frames between decisions; a decision keeps him within ±64 px of home, turns him
    /// to face the player, and counts down to a middle jump (vy 3.0, winged 3.4; in SMB1 a Fly
    /// hover while breathing) or, outside SMB1, a big jump (25-frame wind-up, vy 5.9; SMB3 6.2
    /// aimed at the player, then a hip drop and 40 frames of looking around). The fire timer
    /// runs alongside: every 40–79 frames a breath, then a flame.
    pub(super) fn step_bowser(&mut self, level: &Level) -> Option<Actor> {
        let m1 = level.style == "M1";
        let m3 = level.style == "M3";
        if self.phase == 0 {
            self.boss_setup(&level.style);
        }
        if self.phase == bw::CLOWN {
            self.step_clown(level);
        } else {
            self.bowser_move(level, m1, m3);
        }
        // the breath is held during a big jump and hip drop [inf]
        if matches!(
            self.phase,
            bw::BEFORE_BIG_JUMP | bw::BIG_JUMP | bw::HIP_DROP
        ) {
            return None;
        }
        self.bowser_breath()
    }

    fn bowser_move(&mut self, level: &Level, m1: bool, m3: bool) {
        match self.phase {
            bw::WALK => {
                self.vx = BOWSER_WALK * self.heading;
                self.fall(level, BOWSER_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 && self.on_ground {
                    self.bowser_decide(level, m1);
                }
            }
            bw::LITTLE_JUMP | bw::MIDDLE_JUMP | bw::FLY | bw::BIG_JUMP => {
                let g = match self.phase {
                    bw::LITTLE_JUMP => -0.07,
                    // [inf] "gravity briefly +0.05 then −0.1": a gentler −0.05 while breathing
                    bw::FLY if self.pause < 0 => -0.05,
                    _ => BOWSER_GRAVITY,
                };
                if self.phase != bw::BIG_JUMP {
                    self.vx = BOWSER_WALK * self.heading; // [inf] keeps pacing in the air
                }
                self.timer -= 1;
                self.fall(level, g);
                if self.phase == bw::BIG_JUMP && m3 && self.vy <= 0.0 {
                    // apex: hip drop (start vy 3.0, 1.0 when close to the target)
                    let close = (self.target.0 - self.x).abs() < 16.0 * self.scale();
                    self.phase = bw::HIP_DROP;
                    self.vx = 0.0;
                    self.vy = if close { 1.0 } else { 3.0 };
                } else if self.on_ground && self.timer <= 0 {
                    self.phase = bw::LAND_ON;
                    self.timer = 8; // [inf]
                    self.vx = 0.0;
                }
            }
            bw::HIP_DROP => {
                self.vx = 0.0;
                self.vy = (self.vy - 0.4).max(-8.0);
                self.collide(level);
                if self.on_ground {
                    // [inf] the big Bowser's quake and block breaking are not modelled
                    self.phase = bw::LOOK_AROUND;
                    self.timer = 40;
                }
            }
            bw::BEFORE_BIG_JUMP => {
                self.vx = 0.0;
                self.fall(level, BOWSER_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    let dx = self.target.0 - self.x;
                    self.phase = bw::BIG_JUMP;
                    self.vy = if m3 { 6.2 } else { 5.9 };
                    self.on_ground = false;
                    self.timer = 0;
                    self.vx = if m3 {
                        // [inf] reach the player's x by the apex
                        (dx / (self.vy / -BOWSER_GRAVITY)).clamp(-4.0, 4.0)
                    } else {
                        // [inf] random 0…4 (big 0…3) steps of 0.25 px/f toward the player
                        let n = if self.big { 4 } else { 5 };
                        self.rand(n) as f32 * 0.25 * dx.signum()
                    };
                }
            }
            _ => {
                // Turn, LandOn, LookAround, HitBlock (keeps its shove)
                if self.phase != bw::HIT_BLOCK {
                    self.vx = 0.0;
                }
                self.fall(level, BOWSER_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    if self.phase == bw::TURN {
                        self.dir = self.toward_target();
                    }
                    self.bowser_decide(level, m1);
                }
            }
        }
    }

    /// Decision (vtable slot 0x318): patrol bounds, turn to the player, jump counters.
    fn bowser_decide(&mut self, level: &Level, m1: bool) {
        self.vx = 0.0;
        if self.x <= self.home_x - BOWSER_PATROL {
            self.heading = 1.0;
        } else if self.x >= self.home_x + BOWSER_PATROL {
            self.heading = -1.0;
        }
        if self.wall || self.ledge_ahead(level, self.heading) {
            self.heading = -self.heading;
        }
        let behind = (self.target.0 - self.x) * (self.dir as f32) < -BOWSER_SENSOR * self.scale();
        if behind {
            self.phase = bw::TURN;
            self.timer = 8; // [inf]
            return;
        }
        let mut j = self.byte(0).saturating_sub(1);
        let mut b = self.byte(1).saturating_sub(1);
        if !m1 && b == 0 && self.pause >= 0 {
            self.pause = self.bowser_fire_interval();
            b = 5 + self.rand(2) as u8;
            self.phase = bw::BEFORE_BIG_JUMP;
            self.timer = 25;
        } else if j == 0 {
            // [inf] the reachable-height search is not decoded: the middle jump always happens
            j = self.bowser_jump_count(m1);
            self.vy = if self.wings { 3.4 } else { 3.0 };
            self.on_ground = false;
            self.phase = if m1 && self.pause < 0 {
                bw::FLY
            } else {
                bw::MIDDLE_JUMP
            };
            self.timer = 0;
        } else if m1 {
            self.phase = bw::WALK;
            self.timer = 20 + self.rand(20) as i32;
        } else {
            self.phase = bw::LITTLE_JUMP;
            self.vy = 1.1;
            self.on_ground = false;
            self.timer = 3;
        }
        self.set_byte(0, j);
        self.set_byte(1, b);
    }

    /// Fire timer (`pause`): counts down; at 0 the breath runs (`pause` negative, counting up)
    /// and the flame leaves at its end, then the timer is re-armed.
    fn bowser_breath(&mut self) -> Option<Actor> {
        if self.pause > 0 {
            self.pause -= 1;
            if self.pause == 0 {
                self.pause = -BOWSER_BREATH;
            }
            return None;
        }
        self.pause += 1;
        if self.pause < 0 {
            return None;
        }
        self.pause = self.bowser_fire_interval();
        let lane = self.rand(8); // 3 random bits [obs]
        let s = self.scale();
        // [inf] four lanes 0/8/16/24 px (×2 big) above his feet; mouth 20 px up, 8 px ahead
        let lane_y = self.y + (lane % 4) as f32 * 8.0 * s;
        Some(self.boss_flame(20.0 * s, lane_y))
    }

    fn boss_flame(&self, mouth_h: f32, lane_y: f32) -> Actor {
        let d = self.dir as f32;
        let mut f = Actor::new(
            Kind::BowserFire,
            self.x + d * (self.half_w() + 8.0),
            self.y + mouth_h,
        );
        f.dir = self.dir;
        f.vx = BOWSER_FIRE_SPEED * d;
        f.home_y = lane_y;
        f.timer = 600;
        f
    }

    // ------------------------------------------------------------ Boom Boom

    /// Boom Boom (BunbunM3, also used by SMB1/SMW). Runs at 0.75 / 0.75 / 1.5 px/f by hits
    /// taken, re-aiming at the player every 40 frames; a wall costs a WallJump (30-frame wind-up),
    /// the 4th bump on a side turns it. When the action timer (240–299) runs out: 3 HP → charge
    /// at 2.0 into Defend; 2 HP → Jump; 1 HP → keep running. A hit → DamageDown 70 frames →
    /// Defend (spiky spin: hops at 2.75, accelerating 0.05/frame toward the player) → a slow walk
    /// at 0.35 → Walk. Winged: cruise (accel 0.05, vertical step 0.075) for 240–299 frames, then a
    /// 60-frame swoop at 2.0 (+0.5 per lost HP).
    pub(super) fn step_boom_boom(&mut self, level: &Level) -> Option<Actor> {
        if self.phase == 0 {
            self.boss_setup(&level.style);
        }
        if let Kind::BoomBoom { pom: true, .. } = self.kind {
            return self.step_pom_pom(level);
        }
        let toward = self.toward_target();
        let lost = 3u8.saturating_sub(self.hops).min(2) as usize;
        let g = super::ENEMY_GRAVITY; // [inf] the shared enemy gravity
        match self.phase {
            bb::WALK | bb::SLOW => {
                if self.timer % 40 == 0 {
                    self.dir = toward;
                }
                let speed = if self.phase == bb::SLOW {
                    BB_SLOW
                } else {
                    BB_SPEED[lost]
                };
                if self.on_ground {
                    self.vx = speed * self.dir as f32;
                }
                self.fall(level, g);
                if self.wall && self.on_ground && self.turn_lock == 0 {
                    self.boom_wall();
                    return None;
                }
                self.timer -= 1;
                if self.phase == bb::SLOW {
                    self.pause -= 1;
                    if self.pause <= 0 {
                        self.phase = bb::WALK;
                    }
                } else if self.timer <= 0 {
                    match self.hops {
                        3 => {
                            self.phase = bb::CHARGE;
                            self.pause = BB_CHARGE_FRAMES;
                            self.dir = toward;
                        }
                        2 => {
                            self.phase = bb::JUMP_WINDUP;
                            self.pause = BB_WINDUP;
                        }
                        _ => self.timer = 240 + self.rand(60) as i32,
                    }
                }
            }
            bb::CHARGE => {
                self.vx = BB_CHARGE * self.dir as f32;
                self.fall(level, g);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
                self.pause -= 1;
                if self.pause <= 0 {
                    self.enter_defend();
                }
            }
            bb::JUMP_WINDUP | bb::WALL_WINDUP => {
                self.vx = 0.0;
                self.fall(level, g);
                self.pause -= 1;
                if self.pause <= 0 {
                    let wall = self.phase == bb::WALL_WINDUP;
                    self.vy = if wall { BB_WALL_JUMP } else { BB_JUMP };
                    self.vx = BB_SPEED[lost] * self.dir as f32;
                    self.on_ground = false;
                    self.phase = if wall { bb::WALL_JUMP } else { bb::JUMP };
                }
            }
            bb::JUMP | bb::WALL_JUMP => {
                self.fall(level, g);
                if self.on_ground && self.vy <= 0.0 {
                    if self.phase == bb::JUMP {
                        self.timer = 240 + self.rand(60) as i32;
                    }
                    self.phase = bb::WALK;
                }
            }
            bb::DAMAGE_DOWN => {
                self.vx = 0.0;
                self.fall(level, g);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.enter_defend();
                }
            }
            bb::DEFEND => {
                let t = BB_DEFEND_MAX * toward as f32;
                self.vx += (t - self.vx).clamp(-BB_DEFEND_ACCEL, BB_DEFEND_ACCEL);
                let vx = self.vx;
                self.fall(level, g);
                if self.wall {
                    self.vx = -vx;
                }
                self.pause += 1;
                if self.on_ground {
                    let hops = self.byte(3) + 1;
                    self.set_byte(3, hops);
                    if self.pause >= BB_DEFEND_MIN && hops >= BB_DEFEND_HOPS {
                        self.phase = bb::SLOW;
                        self.pause = BB_SLOW_FRAMES;
                        self.timer = 240 + self.rand(60) as i32;
                        self.dir = toward;
                    } else {
                        self.vy = BB_DEFEND_HOP;
                        self.on_ground = false;
                    }
                }
            }
            bb::FLY => {
                self.dir = toward;
                let tvx = BB_FLY_SPEED * toward as f32;
                self.vx += (tvx - self.vx).clamp(-0.05, 0.05);
                let tvy = ((self.home_y - self.y) / 16.0).clamp(-1.0, 1.0);
                self.vy += (tvy - self.vy).clamp(-0.075, 0.075);
                self.collide(level);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = bb::SWOOP;
                    self.pause = BB_SWOOP_FRAMES;
                    self.vx = (2.0 + 0.5 * lost as f32) * toward as f32;
                    self.vy = 0.0;
                }
            }
            bb::SWOOP => {
                // [inf] down for the first half (accel 0.1 within the 0.04–0.15 range, cap
                // −1.2), back up for the second
                let a = if self.pause > BB_SWOOP_FRAMES / 2 {
                    -0.1
                } else {
                    0.1
                };
                self.vy = (self.vy + a).clamp(-1.2, 1.2);
                let vx = self.vx;
                self.collide(level);
                if self.wall {
                    self.vx = -vx;
                }
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = bb::FLY;
                    self.timer = 240 + self.rand(60) as i32;
                }
            }
            _ => {
                // Clown Car: no attack of its own [inf]
                self.step_clown(level);
            }
        }
        None
    }

    fn boom_wall(&mut self) {
        let side = if self.dir > 0 { 1 } else { 2 };
        let n = self.byte(side);
        if n < BB_WALL_BUMPS {
            self.set_byte(side, n + 1);
            self.phase = bb::WALL_WINDUP;
            self.pause = BB_WINDUP;
            self.vx = 0.0;
        } else {
            self.set_byte(side, 0);
            self.turn();
        }
    }

    fn enter_defend(&mut self) {
        self.phase = bb::DEFEND;
        self.pause = 0;
        self.set_byte(3, 0);
        self.vy = BB_DEFEND_HOP;
        self.on_ground = false;
    }

    /// Pom Pom (Punpun) [inf throughout — states from the table, constants chosen]: waits 60
    /// frames facing the player, then alternately jumps at her (vy 4.0, 1.5 px/f) or throws:
    /// a 20-frame sign, then 3 shuriken 12 frames apart (2 px/f, aimed). After DamageDown she
    /// vanishes (Lapse, 60 frames) and reappears 64 px on the player's far side. The PunpunFake
    /// decoys are not made.
    fn step_pom_pom(&mut self, level: &Level) -> Option<Actor> {
        let toward = self.toward_target();
        let g = super::ENEMY_GRAVITY;
        let mut shot = None;
        match self.phase {
            bb::POM_WAIT => {
                self.vx = 0.0;
                self.dir = toward;
                self.fall(level, g);
                self.timer -= 1;
                if self.timer <= 0 && self.on_ground {
                    let n = self.byte(1);
                    self.set_byte(1, n.wrapping_add(1));
                    if n.is_multiple_of(2) {
                        self.phase = bb::POM_JUMP;
                        self.vy = 4.0;
                        self.vx = 1.5 * toward as f32;
                        self.on_ground = false;
                    } else {
                        self.phase = bb::POM_THROW_SIGN;
                        self.pause = 20;
                    }
                }
            }
            bb::POM_JUMP => {
                self.fall(level, g);
                if self.on_ground && self.vy <= 0.0 {
                    self.phase = bb::POM_WAIT;
                    self.timer = 60;
                }
            }
            bb::POM_THROW_SIGN => {
                self.vx = 0.0;
                self.fall(level, g);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = bb::POM_THROW;
                    self.pause = 36;
                }
            }
            bb::POM_THROW => {
                self.fall(level, g);
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
            bb::DAMAGE_DOWN => {
                self.vx = 0.0;
                self.fall(level, g);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = bb::POM_LAPSE;
                    self.pause = 60;
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
                    self.timer = 30;
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

    /// Bowser Jr. (KoopaJr Basic action). Wait 20 frames, then [inf: the choice table is not
    /// decoded] walk, shoot fire, throw a hammer, hop or shift-jump. Walk: 120 frames toward 0.8
    /// px/f (1.25 when angry: [inf] at 1 HP) in 0.1 steps, turning 24 px from home. Shift jump:
    /// once vy < 0.4 → Hover 50–74 frames → PrevHipDrop 10/20 frozen → HipDrop (g −0.75, cap −8)
    /// → 50-frame recovery, after which [inf] he paces around the landing point. A hit: DamageDown (bounce 3.0, 55 frames) → ShellIn 23 → Shell
    /// (slides) → ReturnFromShell → Wait. In a Clown Car he fires, throws hammers and drops
    /// shells in turn [inf].
    pub(super) fn step_bowser_jr(&mut self, level: &Level) -> Option<Actor> {
        if self.phase == 0 {
            self.boss_setup(&level.style);
        }
        let toward = self.toward_target();
        let brake = |v: f32, by: f32| v - v.clamp(-by, by);
        let mut out = None;
        match self.phase {
            jr::WAIT => {
                self.vx = brake(self.vx, 0.1);
                self.fall(level, JR_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 && self.on_ground {
                    self.dir = toward;
                    self.jr_choose();
                }
            }
            jr::WALK => {
                let speed = if self.hops == 1 {
                    JR_WALK_ANGRY
                } else {
                    JR_WALK
                };
                if self.x <= self.home_x - JR_TURN_RANGE {
                    self.dir = 1;
                } else if self.x >= self.home_x + JR_TURN_RANGE {
                    self.dir = -1;
                }
                if self.ledge_ahead(level, self.dir as f32) {
                    self.dir = -self.dir;
                }
                let t = speed * self.dir as f32;
                self.vx += (t - self.vx).clamp(-JR_WALK_STEP, JR_WALK_STEP);
                self.fall(level, JR_GRAVITY);
                if self.wall {
                    self.dir = -self.dir;
                }
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                }
            }
            jr::SHOT_FIRE | jr::SHOT_HAMMER => {
                self.vx = brake(self.vx, 0.5);
                self.fall(level, JR_GRAVITY);
                self.pause += 1;
                if self.pause == JR_SHOT_AT {
                    out = Some(self.jr_shot(self.phase == jr::SHOT_FIRE));
                }
                if self.pause >= JR_SHOT_END {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                }
            }
            jr::HOP => {
                self.fall(level, JR_GRAVITY);
                self.pause -= 1;
                if self.pause <= 0 && self.on_ground {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                }
            }
            jr::SHIFT_JUMP => {
                self.fall(level, JR_GRAVITY);
                if self.vy < JR_HOVER_AT {
                    self.phase = jr::HOVER;
                    self.pause = 50 + self.rand(25) as i32;
                } else if self.on_ground {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                }
            }
            jr::HOVER => {
                // [inf] drifts over the player (≤ 1.5 px/f, 0.1 px/f²)
                self.vy = 0.0;
                let t = ((self.target.0 - self.x) / 16.0).clamp(-1.5, 1.5);
                self.vx += (t - self.vx).clamp(-0.1, 0.1);
                self.collide(level);
                self.dir = toward;
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::PREV_HIP_DROP;
                    self.pause = if self.rand(2) == 0 { 10 } else { 20 };
                    self.vx = 0.0;
                }
            }
            jr::PREV_HIP_DROP => {
                self.vx = 0.0;
                self.vy = 0.0;
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::HIP_DROP;
                }
            }
            jr::HIP_DROP => {
                self.vx = 0.0;
                self.vy = (self.vy + JR_HIP_GRAVITY).max(JR_HIP_CAP);
                self.collide(level);
                if self.on_ground {
                    self.phase = jr::HIP_LAND;
                    self.pause = JR_HIP_LAND;
                }
            }
            jr::HIP_LAND => {
                self.vx = brake(self.vx.clamp(-1.5, 1.5), 0.1);
                self.fall(level, JR_GRAVITY);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                    self.home_x = self.x; // [inf] he paces where the hip drop landed
                }
            }
            jr::DAMAGE_DOWN => {
                self.vx = 0.0;
                self.fall(level, JR_GRAVITY);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::SHELL_IN;
                    self.pause = JR_SHELL_IN;
                }
            }
            jr::SHELL_IN => {
                self.vx = 0.0;
                self.fall(level, JR_GRAVITY);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::SHELL;
                    self.pause = JR_SHELL_FRAMES;
                    self.vx = JR_SHELL_SPEED * toward as f32;
                }
            }
            jr::SHELL => {
                let vx = self.vx;
                self.fall(level, JR_GRAVITY);
                if self.wall {
                    self.vx = -vx;
                }
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::SHELL_OUT;
                    self.pause = JR_SHELL_OUT;
                }
            }
            jr::SHELL_OUT => {
                self.vx = brake(self.vx, 0.2);
                self.fall(level, JR_GRAVITY);
                self.pause -= 1;
                if self.pause <= 0 {
                    self.phase = jr::WAIT;
                    self.timer = JR_WAIT;
                }
            }
            _ => {
                if self.step_clown(level) {
                    let n = self.byte(1);
                    self.set_byte(1, n.wrapping_add(1));
                    out = Some(match n % 3 {
                        0 => self.jr_shot(true),
                        1 => self.jr_shot(false),
                        _ => {
                            let mut s = Actor::new(Kind::Shell { red: false }, self.x, self.y);
                            s.mode = Mode::ShellWait;
                            s.timer = SHELL_WAKE;
                            s.vx = toward as f32;
                            s.vy = 2.0;
                            s
                        }
                    });
                }
            }
        }
        out
    }

    fn jr_choose(&mut self) {
        let toward = self.toward_target();
        let near = (self.target.0 - self.x).abs() <= JR_NEAR;
        let r = self.rand(16);
        let next = if near {
            match r {
                0..=5 => jr::SHIFT_JUMP,
                6..=9 => jr::HOP,
                _ => jr::WALK,
            }
        } else {
            match r {
                0..=4 => jr::SHOT_FIRE,
                5..=6 => jr::SHOT_HAMMER,
                7..=8 => jr::SHIFT_JUMP,
                _ => jr::WALK,
            }
        };
        match next {
            jr::WALK => self.timer = JR_WALK_FRAMES,
            jr::HOP => {
                self.vy = JR_HOP;
                self.pause = JR_HOP_FRAMES;
                self.on_ground = false;
            }
            jr::SHIFT_JUMP => {
                self.vy = JR_SHIFT_JUMP;
                self.vx = toward as f32;
                self.on_ground = false;
            }
            _ => self.pause = 0,
        }
        self.phase = next;
    }

    /// Bowser Jr.'s fire [inf: a Bowser flame aimed at the player's height, within ±32 px] or
    /// hammer (the Hammer Bro's arc).
    fn jr_shot(&mut self, fire: bool) -> Actor {
        if fire {
            let s = self.scale();
            let lane = (self.target.1 + 4.0).clamp(self.y - 32.0, self.y + 32.0);
            self.boss_flame(10.0 * s, lane)
        } else {
            self.make_hammer()
        }
    }
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
                    "fall.0"
                } else {
                    cycle(&["wait.0", "wait.1"], 16)
                };
                ("M1_Enemy_koopa", tex, 48.0, 48.0, face)
            }
            Kind::BowserFire => (
                "M1_Enemy_koopa",
                cycle(&["effect.0", "effect.1"], 4),
                24.0,
                16.0,
                self.vx > 0.0,
            ),
            Kind::BoomBoom { .. } => {
                const WALK: [&str; 6] =
                    ["walk.0", "walk.1", "walk.2", "walk.3", "walk.4", "walk.5"];
                const FLY: [&str; 6] = ["fly.0", "fly.1", "fly.2", "fly.3", "fly.4", "fly.5"];
                let tex = match self.phase {
                    bb::DAMAGE_DOWN if self.wings => cycle(&["damage_fly.0", "damage_fly.1"], 6),
                    bb::DAMAGE_DOWN => cycle(&["damage.0", "damage.1"], 6),
                    bb::DEFEND => "squat.2",
                    bb::JUMP_WINDUP | bb::WALL_WINDUP | bb::POM_THROW_SIGN => "jump_st.0",
                    bb::JUMP | bb::WALL_JUMP | bb::POM_JUMP => {
                        if self.vy > 0.0 {
                            "jump_ed.0"
                        } else {
                            "jump_ed.1"
                        }
                    }
                    bb::SLOW => {
                        let k = (BB_SLOW_FRAMES - self.pause).clamp(0, 29) / 10;
                        ["standup.0", "standup.1", "standup.2"][k as usize]
                    }
                    bb::FLY | bb::SWOOP | bb::CLOWN => cycle(&FLY, 4),
                    bb::CHARGE => cycle(&WALK, 2),
                    _ => cycle(&WALK, 4),
                };
                ("M1_Enemy_bunbun", tex, 32.0, 32.0, face)
            }
            // [inf] placeholder art: Pom Pom's shuriken uses Magikoopa's magic sparkle
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
                        cycle(&["shell.0", "shell.1", "shell.2", "shell.3"], 3)
                    } else {
                        "shell.0"
                    };
                    return Some(("M1_Enemy_koopaJr", tex, 16.0, 16.0, face));
                }
                let tex = match self.phase {
                    jr::SHOT_FIRE if self.pause < JR_SHOT_AT => "fire.0",
                    jr::SHOT_FIRE => "fire.1",
                    jr::SHOT_HAMMER if self.pause < JR_SHOT_AT => "throw.0",
                    jr::SHOT_HAMMER => "throw.1",
                    jr::PREV_HIP_DROP | jr::HIP_DROP => "before.0",
                    jr::CLOWN => "wait.0",
                    _ if !self.on_ground => cycle(&["fall.0", "fall.1"], 8),
                    _ => cycle(&["wait.0", "wait.1"], 16),
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
    use crate::level::TILE;

    fn boss(kind: Kind, x: f32) -> (Level, Actor) {
        let level = Level::flat(80);
        let mut a = Actor::new(kind, x, 32.0);
        a.on_ground = true;
        a.dir = -1;
        a.target = (x - 100.0, 32.0);
        (level, a)
    }

    const BOWSER: Kind = Kind::Bowser {
        key: false,
        clown: false,
    };

    #[test]
    fn bowser_paces_within_64_px_of_home_at_0_3() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let home = a.home_x;
        let (mut lo, mut hi) = (home, home);
        let mut walk_speeds = Vec::new();
        for f in 0..4000 {
            // the player wanders either side so he turns now and then
            a.target.0 = home + if (f / 500) % 2 == 0 { -150.0 } else { 150.0 };
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
    fn bowser_breathes_fire_every_40_to_79_frames_plus_the_breath() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let mut shots = Vec::new();
        for f in 0..3000 {
            if let Some(fire) = a.step_spawning(&level) {
                assert_eq!(fire.kind, Kind::BowserFire);
                assert!((fire.vx.abs() - BOWSER_FIRE_SPEED).abs() < 1e-6);
                assert_eq!(fire.vx.signum() as i32, a.dir);
                let lane = fire.home_y - a.y;
                assert!(
                    [0.0, 8.0, 16.0, 24.0]
                        .iter()
                        .any(|l| (lane - l).abs() < 1e-3)
                );
                shots.push(f);
            }
        }
        assert!(shots.len() > 20);
        for w in shots.windows(2) {
            let gap = w[1] - w[0];
            assert!(
                (40 + BOWSER_BREATH..80 + BOWSER_BREATH).contains(&gap),
                "gap {gap}"
            );
        }
    }

    #[test]
    fn bowser_flame_settles_onto_its_lane() {
        let mut f = Actor::new(Kind::BowserFire, 100.0, 52.0);
        f.vx = -1.5;
        f.home_y = 40.0;
        f.timer = 600;
        for _ in 0..40 {
            f.step_boss_shot();
        }
        assert_eq!(f.y, 40.0);
        assert!((f.x - 40.0).abs() < 1e-3);
    }

    #[test]
    fn smb1_bowser_middle_jumps_3_0_and_never_big_jumps() {
        let (level, mut a) = boss(BOWSER, 30.0 * TILE);
        let mut jumps = 0;
        for _ in 0..3000 {
            let was = a.phase;
            let _ = a.step_spawning(&level);
            if matches!(a.phase, bw::MIDDLE_JUMP | bw::FLY) && was != a.phase {
                jumps += 1;
            }
            assert!(!matches!(
                a.phase,
                bw::BIG_JUMP | bw::BEFORE_BIG_JUMP | bw::LITTLE_JUMP
            ));
        }
        assert!(jumps >= 5, "{jumps} middle jumps");
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
    fn boom_boom_wall_jumps_three_times_then_turns() {
        let (mut level, mut a) = boss(
            Kind::BoomBoom {
                key: false,
                clown: false,
                pom: false,
            },
            30.0 * TILE,
        );
        // a wall 6 tiles high just right of it, the player beyond it
        for y in 2..8 {
            level.set_solid(33, y);
        }
        a.target = (40.0 * TILE, 32.0);
        a.dir = 1;
        let mut wall_jumps = Vec::new();
        for _ in 0..1000 {
            let was = a.phase;
            let _ = a.step_spawning(&level);
            if a.phase == bb::WALL_JUMP && was == bb::WALL_WINDUP {
                wall_jumps.push(a.vy);
            }
            if a.dir < 0 {
                break;
            }
        }
        assert_eq!(wall_jumps.len(), 3);
        assert!(wall_jumps.iter().all(|&v| (v - BB_WALL_JUMP).abs() < 1e-6));
        assert_eq!(a.dir, -1, "turned after the 4th bump");
    }

    #[test]
    fn winged_boom_boom_cruises_then_swoops() {
        let (level, mut a) = boss(
            Kind::BoomBoom {
                key: false,
                clown: false,
                pom: false,
            },
            30.0 * TILE,
        );
        a.wings = true;
        a.y = 120.0;
        a.home_y = 120.0;
        a.on_ground = false;
        let mut swoop = None;
        for f in 0..400 {
            let _ = a.step_spawning(&level);
            if a.phase == bb::SWOOP && swoop.is_none() {
                swoop = Some(f);
                assert!((a.vx.abs() - 2.0).abs() < 1e-6);
            }
        }
        let f = swoop.expect("swooped");
        assert!((239..=300).contains(&f), "swoop at {f}");
        // a stomp takes the wings and 1 HP
        assert_eq!(a.boss_hit(BossAttack::Stomp, a.x), BossHit::Hurt);
        assert!(!a.wings);
        assert_eq!(a.boss_hp(), 2);
    }

    #[test]
    fn bowser_jr_hovers_50_to_74_frames_then_hip_drops_to_minus_8() {
        let (level, mut a) = boss(
            Kind::BowserJr {
                key: false,
                clown: false,
            },
            30.0 * TILE,
        );
        a.target = (a.x - 20.0, 32.0); // near: shift jumps are likely
        let mut hovers = Vec::new();
        let mut hover_len = 0;
        let mut min_vy: f32 = 0.0;
        for _ in 0..6000 {
            let _ = a.step_spawning(&level);
            if a.phase == jr::HOVER {
                hover_len += 1;
            } else if hover_len > 0 {
                hovers.push(hover_len);
                hover_len = 0;
            }
            if a.phase == jr::HIP_DROP {
                min_vy = min_vy.min(a.vy);
            }
        }
        assert!(!hovers.is_empty());
        assert!(hovers.iter().all(|h| (50..=74).contains(h)), "{hovers:?}");
        assert!(min_vy <= JR_HIP_CAP + 1e-6, "{min_vy}");
    }

    #[test]
    fn bowser_jr_walks_within_24_px_of_home() {
        let (level, mut a) = boss(
            Kind::BowserJr {
                key: false,
                clown: false,
            },
            30.0 * TILE,
        );
        a.target = (a.x - 300.0, 32.0); // far: walks and shoots
        let mut far: f32 = 0.0;
        let mut shots = 0;
        let mut walked = 0;
        for _ in 0..3000 {
            if a.step_spawning(&level).is_some() {
                shots += 1;
            }
            if a.phase == jr::WALK && (a.x - a.home_x).abs() <= 24.0 {
                walked += 1;
            }
            if a.phase == jr::WALK && walked > 0 {
                far = far.max((a.x - a.home_x).abs());
            }
        }
        assert!(shots > 5 && walked > 100);
        // a turn starts at ±24 and braking at 0.1 overshoots by about 3 px
        assert!(far <= 24.0 + 4.0, "{far}");
    }

    #[test]
    fn pom_pom_vanishes_after_a_hit_and_reappears_beside_the_player() {
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
        let mut hidden = false;
        for _ in 0..200 {
            let _ = a.step_spawning(&level);
            hidden |= a.boss_hidden();
            if hidden && !a.boss_hidden() {
                break;
            }
        }
        assert!(hidden && !a.boss_hidden());
        assert!(((a.x - a.target.0).abs() - 64.0).abs() < 1e-3);
    }
}
