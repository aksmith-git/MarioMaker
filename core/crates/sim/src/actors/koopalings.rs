//! The Koopalings (120-koopalings.md): Lemmy, Morton, Larry, Wendy, Iggy, Roy and Ludwig. All
//! seven run one shared action (Fall, Wait, Turn, JumpSt, Jump, JumpEd, Damage, ShellIn,
//! RecoverFromShell, AttackReady, Attack) plus a per-Koopaling controller; each overrides a few
//! states and adds its own (Iggy walks, Lemmy rides a ball, Morton ground-pounds, Roy dives into
//! the ground, Ludwig hovers). Their projectiles are `Kind::KoopalingShot`.
//!
//! The numbers are [obs] from the v3.0.3 action / controller code cited in the spec; the few
//! simplifications are marked [inf] with the reason. The world-side rules (touching them, the
//! quake, keys) are in world/koopalings.rs.
//!
//! State layout: `phase` is the state (0 = not set up); `timer` the state timer; `v3.anim` the
//! animation frame of the state; `v3.hp` / `v3.fire` HP and the fireball counter; `v3.count`
//! the moves left before the next attack; `v3.count2` shots fired in the volley; `v3.cool` the
//! attack cooldown; `v3.home` the spawn x; `v3.turn` the current gravity; `pause` frames the
//! player has been behind; `v3.holding` = in the shell (no HP loss).

use super::v3::{V3Look, angle_diff, anim_key, style_ix};
use super::{Actor, ENEMY_GRAVITY, Kind, MAX_FALL, Mode};
use crate::fmath::DetMath;
use crate::level::{Cell, Level, TILE};

/// Who is who: `Kind::Koopaling::who`.
pub mod who {
    pub const LEMMY: u8 = 0;
    pub const MORTON: u8 = 1;
    pub const LARRY: u8 = 2;
    pub const WENDY: u8 = 3;
    pub const IGGY: u8 = 4;
    pub const ROY: u8 = 5;
    pub const LUDWIG: u8 = 6;
}

/// `Kind::KoopalingShot::what`.
pub mod shot {
    /// KoKoopaM3Magic (M1 / M3 / MW wand magic).
    pub const MAGIC: u8 = 0;
    /// LarryFire (NSMBU wand magic).
    pub const FIRE: u8 = 1;
    pub const RING: u8 = 2;
    pub const BALL: u8 = 3;
    /// MortonDossunFire.
    pub const FLAME: u8 = 4;
}

mod st {
    pub const FALL: u8 = 1;
    pub const WAIT: u8 = 2;
    pub const JUMP_ST: u8 = 3;
    pub const JUMP: u8 = 4;
    pub const JUMP_ED: u8 = 5;
    pub const READY: u8 = 6;
    pub const ATTACK: u8 = 7;
    /// NSMBU: the 60-frame flinch before the shell (or the defeat).
    pub const DAMAGE: u8 = 8;
    /// NSMBU: the shell_in hop.
    pub const SHELL_IN: u8 = 9;
    pub const SHELL_WAIT: u8 = 10;
    pub const SHELL_JUMP: u8 = 11;
    pub const SHELL_SLIDE: u8 = 12;
    pub const RECOVER: u8 = 13;
    /// Iggy (wingless) and Lemmy on foot.
    pub const WALK: u8 = 14;
    pub const CREATE_BALL: u8 = 15;
    pub const RIDE_SIGN: u8 = 16;
    pub const RIDE_WALK: u8 = 17;
    pub const RIDE_STOP: u8 = 18;
    pub const QUAKE_ST: u8 = 20;
    pub const QUAKE_JUMP: u8 = 21;
    pub const QUAKE_SIGN: u8 = 22;
    pub const QUAKE_DROP: u8 = 23;
    pub const QUAKE: u8 = 24;
    pub const PRE_DIVE: u8 = 25;
    pub const DIVE: u8 = 26;
    pub const IN_GROUND: u8 = 27;
    pub const APPEAR_HEAD: u8 = 28;
    pub const LOOK_AROUND: u8 = 29;
    pub const APPEAR_FROM: u8 = 30;
    pub const JUMP_KEEP: u8 = 31;
    pub const JUMP_KEEP_END: u8 = 32;
}

/// `v3.flags` bits.
mod fl {
    /// The held Key has been dropped.
    pub const KEY_DROPPED: u8 = 1;
    /// Lemmy rides his ball.
    pub const ON_BALL: u8 = 2;
    /// Roy is under the ground (hidden, intangible).
    pub const HIDDEN: u8 = 4;
    /// The last Wait followed an attack (10 % of those get a short random Wait).
    pub const ATTACKED: u8 = 8;
}

pub const KOOPALING_HP: u8 = 3;
/// Fireballs per HP: 5, big 10 [obs 0x7101142640].
const FIRE_COUNTER: [u8; 2] = [5, 10];
/// Return toward home once 96 px away [obs slot 91].
const HOME_RANGE: f32 = 96.0;
/// Long Wait when there is nowhere to hop, by HP 3 / 2 / 1: [100,150), [80,120), [60,90).
const LONG_WAIT: [(i32, i32); 3] = [(60, 30), (80, 40), (100, 50)];
const SHELL_WAIT_2D: i32 = 65;
const SHELL_WAIT_WU: i32 = 35;
const SHELL_SLIDE_FRAMES: i32 = 300;
const DAMAGE_FRAMES: i32 = 60;
/// Jump gravity while rising: −0.23 (SMB3 −0.1, then the saved −0.1875 from the apex) [obs].
const JUMP_G: [f32; 5] = [-0.23, -0.1, -0.23, -0.23, -0.23];
/// Ledge jumps: vy for a ledge n tiles up (SMB3 1–7, the others 1–3) [obs tables 0x71023cb560 /
/// 0x71023cb57c].
const LEDGE_VY_M3: [f32; 7] = [2.1, 3.0, 3.2, 3.9, 4.3, 4.7, 5.1];
const LEDGE_VY: [f32; 3] = [3.0, 4.0, 5.0];

/// Model base name per Koopaling.
const NAMES: [&str; 7] = ["lemmy", "morton", "larry", "wendy", "iggy", "roy", "ludwig"];
const MODELS: [&str; 7] = [
    "M1_Enemy_lemmy",
    "M1_Enemy_morton",
    "M1_Enemy_larry",
    "M1_Enemy_wendy",
    "M1_Enemy_iggy",
    "M1_Enemy_roy",
    "M1_Enemy_ludwig",
];

/// HP index: 0 at 3 HP, 1 at 2, 2 at 1.
fn hp_ix(hp: u8) -> usize {
    (3 - hp.clamp(1, 3)) as usize
}

impl Actor {
    fn kl_who(&self) -> u8 {
        match self.kind {
            Kind::Koopaling { who, .. } => who,
            _ => u8::MAX,
        }
    }

    pub fn koopaling_name(&self) -> &'static str {
        NAMES[self.kl_who().min(6) as usize]
    }

    pub(super) fn koopaling_size(&self) -> (f32, f32) {
        let s = self.v3_scale();
        match self.kind {
            Kind::KoopalingShot { what } => match what {
                // KoKoopaM3Magic: 37.5 × 37.5 around the shot and its trail (Ludwig's, without
                // a trail, 16 × 20) [obs slot 191]
                shot::MAGIC if self.v3.flags & 1 != 0 => (8.0 * s, 20.0 * s),
                shot::MAGIC => (18.75 * s, 37.5 * s),
                shot::FIRE => (8.0 * s, 16.0 * s),
                shot::RING => (11.0 * s, 22.0 * s),
                shot::FLAME => (4.0 * s, 8.0 * s),
                _ => (7.9 * s, 15.8 * s),
            },
            // body 32 × 32 (big 64), bottom-centred; Lemmy on his ball is taller by the ball's
            // diameter [obs profiles, 0x71011bba00]
            _ if self.v3.flags & fl::ON_BALL != 0 => (16.0 * s, (32.0 + 16.0 * self.v3.len) * s),
            _ => (16.0 * s, 32.0 * s),
        }
    }

    /// HP left (3 at the start).
    pub fn koopaling_hp(&self) -> u8 {
        self.v3.hp
    }

    pub fn koopaling_state(&self) -> u8 {
        self.phase
    }

    /// In the shell (no HP loss) or flinching: hits bounce off.
    pub fn koopaling_guarded(&self) -> bool {
        self.v3.holding || self.phase == st::DAMAGE || self.v3.hp == 0
    }

    /// Under the ground (Roy): not drawn, not touchable.
    pub fn koopaling_hidden(&self) -> bool {
        self.v3.flags & fl::HIDDEN != 0
    }

    pub fn koopaling_key(&mut self) -> bool {
        let Kind::Koopaling { key: true, .. } = self.kind else {
            return false;
        };
        if self.v3.flags & fl::KEY_DROPPED != 0 {
            return false;
        }
        self.v3.flags |= fl::KEY_DROPPED;
        true
    }

    /// The moving shell (ShellJump / ShellSlide): its touch hurts [ext].
    pub fn koopaling_shell_moving(&self) -> bool {
        matches!(self.phase, st::SHELL_JUMP | st::SHELL_SLIDE)
    }

    // ------------------------------------------------------------ hits

    /// A hit that costs 1 HP (stomps, shells, block hits, …) [obs 0x7101143410]: in the shell it
    /// does nothing; at 0 HP the Koopaling is defeated (SMB1/SMB3/SMW at once; NSMBU after the
    /// 60-frame flinch). Otherwise it retreats into its shell. Returns (lost HP, defeated).
    pub fn koopaling_damage(&mut self, from_x: f32, stomp: bool) -> (bool, bool) {
        let st_ix = self.v3.style as usize;
        if self.koopaling_guarded() {
            return (false, false);
        }
        self.v3.hp -= 1;
        self.v3.fire = FIRE_COUNTER[self.big as usize];
        self.v3.flags &= !fl::ON_BALL; // Lemmy's ball pops (Damage enter) [obs]
        self.v3.shots = 0;
        if stomp {
            self.vy = self.vy.min(0.0);
        } else {
            self.vy = self.vy.min(2.5);
        }
        self.vx = 0.0;
        if self.v3.hp == 0 {
            if st_ix == 3 {
                self.kl_enter(st::DAMAGE);
            } else {
                self.die_fall(from_x);
            }
            return (true, true);
        }
        if st_ix == 3 {
            self.kl_enter(st::DAMAGE);
        } else {
            self.v3.holding = true;
            self.kl_enter(st::SHELL_WAIT);
        }
        (true, false)
    }

    /// A fireball (or an arrow): the fire counter drops; at 0 it costs 1 HP, otherwise a
    /// flash and a 1.3 scale pop [obs]. True when that defeated the Koopaling.
    pub fn koopaling_fire_hit(&mut self, from_x: f32) -> bool {
        if self.koopaling_guarded() {
            return false;
        }
        self.v3.fire = self.v3.fire.saturating_sub(1);
        if self.v3.fire > 0 {
            self.v3.pop = 1.3;
            self.kick_lock = 8;
            return false;
        }
        self.koopaling_damage(from_x, false).1
    }

    // ------------------------------------------------------------ setup and helpers

    fn kl_setup(&mut self, level: &Level) {
        self.v3.hp = KOOPALING_HP;
        self.v3.fire = FIRE_COUNTER[self.big as usize];
        self.v3.home = self.x;
        self.v3.count = 1 + self.rand(2) as u8;
        self.v3.turn = ENEMY_GRAVITY;
        self.v3.style = style_ix(level) as u8;
        if self.kl_who() == who::LEMMY {
            self.v3.flags |= fl::ON_BALL;
            self.v3.len = if style_ix(level) == 3 { 1.5 } else { 1.0 };
        }
        if self.kl_who() == who::MORTON {
            self.morton_reroll(style_ix(level), false);
        }
        self.dir = self.v3_toward();
        self.kl_enter(if self.on_ground { st::WAIT } else { st::FALL });
    }

    fn kl_enter(&mut self, s: u8) {
        self.phase = s;
        self.v3.anim = 0;
        self.timer = 0;
        match s {
            st::DAMAGE => self.timer = DAMAGE_FRAMES,
            st::SHELL_WAIT => {
                self.timer = SHELL_WAIT_2D;
                self.vx = 0.0;
            }
            _ => {}
        }
    }

    /// Ball radius × scale under Lemmy: 8 px (NSMBU 12) [obs DAT_71023cd8f0].
    fn kl_ball_lift(&self) -> f32 {
        if self.v3.flags & fl::ON_BALL != 0 {
            16.0 * self.v3.len * self.v3_scale()
        } else {
            0.0
        }
    }

    fn kl_blocked(&self, level: &Level, dir: i32) -> bool {
        let s = self.v3_scale();
        let x = self.x + dir as f32 * (16.0 * s + 8.0);
        let wall = level.cell_at(x, self.y + 8.0).is_solid()
            && level.cell_at(x, self.y + 8.0 + 3.0 * TILE * s).is_solid();
        // no floor at all below the spot ahead (a pit)
        let mut floor = false;
        let mut y = self.y - 1.0;
        while y > self.y - 8.0 * TILE {
            if level.cell_at(x + dir as f32 * 16.0, y).is_solid() {
                floor = true;
                break;
            }
            y -= TILE;
        }
        wall || !floor
    }

    /// The ledge height (tiles) in front, if a higher floor is within reach [obs 0x71011463d0;
    /// the probe pattern is simplified to one column one tile ahead: inf].
    fn kl_ledge_up(&self, level: &Level, dir: i32, max: i32) -> Option<i32> {
        let x = self.x + dir as f32 * (16.0 * self.v3_scale() + 8.0);
        let base = (self.y / TILE).floor() as i32;
        let tx = (x / TILE).floor() as i32;
        if !level.cell(tx, base).is_solid() {
            return None;
        }
        (1..=max).find(|&n| {
            level.cell(tx, base + n - 1).is_solid() && !level.cell(tx, base + n).is_solid()
        })
    }

    fn kl_wait_timer(&mut self) {
        // normally 0; after an attack 10 % of the time 0…19 [obs]
        self.timer = if self.v3.flags & fl::ATTACKED != 0 && self.rand(10) == 0 {
            self.rand(20) as i32
        } else {
            0
        };
        self.v3.flags &= !fl::ATTACKED;
    }

    fn kl_volley(&self) -> u8 {
        let h = hp_ix(self.v3.hp);
        match self.kl_who() {
            who::WENDY | who::ROY | who::LUDWIG => 1,
            who::LEMMY => [1, 3, 5][h],
            _ => [1, 2, 3][h],
        }
    }

    /// AttackReady hold before the first shot / between shots [obs controller slots 25/26].
    fn kl_hold(&self, first: bool) -> i32 {
        let h = hp_ix(self.v3.hp);
        let s = self.big as i32 + 1;
        match self.kl_who() {
            who::LEMMY => {
                if h == 2 {
                    30 * s
                } else {
                    35 * s
                }
            }
            who::ROY if first => [30, 20, 15][h],
            who::LUDWIG if first => 10,
            who::IGGY if !first => 10,
            _ if !first && h == 2 => 20,
            _ => 30,
        }
    }

    /// The attack animation's shot frame and length (2D sprites; NSMBU skeletal) [obs].
    fn kl_attack_anim(&self, sti: usize) -> (i32, i32) {
        let wu = sti == 3;
        match self.kl_who() {
            who::WENDY => {
                if wu {
                    (33, 75)
                } else if sti == 2 {
                    (12, 60)
                } else {
                    (12, 40)
                }
            }
            who::LEMMY => (if wu { 11 } else { 3 }, 60),
            who::IGGY => (if wu { 27 } else { 3 }, if wu { 35 } else { 60 }),
            who::ROY => (if wu { 20 } else { 3 }, 60),
            who::LUDWIG => (if wu { 15 } else { 3 }, 60),
            who::LARRY => (if wu { 8 } else { 3 }, if wu { 24 } else { 60 }),
            _ => (if wu { 8 } else { 3 }, 60),
        }
    }

    // ------------------------------------------------------------ the step

    pub(super) fn step_koopaling(&mut self, level: &Level) -> Option<Actor> {
        if let Kind::KoopalingShot { what } = self.kind {
            self.step_kl_shot(level, what);
            return None;
        }
        let sti = style_ix(level);
        if self.phase == 0 {
            self.kl_setup(level);
        }
        if self.v3.cool > 0 {
            self.v3.cool -= 1;
        }
        if self.v3.clock > 0 {
            self.v3.clock -= 1;
        }
        if self.v3.pop > 0.0 {
            self.v3.pop = (self.v3.pop - 0.07).max(0.0);
            if self.v3.pop <= 1.0 {
                self.v3.pop = 0.0;
            }
        }
        // turn to face a player who has been behind for 10 frames during the Wait [obs]
        if self.phase == st::WAIT {
            if self.v3_toward() != self.dir {
                self.pause += 1;
                if self.pause >= 10 {
                    self.dir = -self.dir;
                    self.pause = 0;
                }
            } else {
                self.pause = 0;
            }
        }
        self.v3.anim += 1;
        let mut gravity = true;
        match self.phase {
            st::FALL => {
                if self.on_ground {
                    self.kl_enter(st::WAIT);
                }
            }
            st::WAIT => {
                self.vx = 0.0;
                if self.timer > 0 {
                    self.timer -= 1;
                }
                if self.kl_who() == who::LEMMY
                    && self.v3.flags & fl::ON_BALL == 0
                    && self.on_ground
                    && self.kl_room_for_ball(level)
                {
                    self.kl_enter(st::CREATE_BALL);
                } else if self.timer <= 0 && self.on_ground {
                    self.kl_decide(level, sti);
                }
            }
            st::JUMP_ST => {
                self.vx = 0.0;
                let len = if self.kl_who() == who::LEMMY { 10 } else { 5 };
                if self.v3.anim >= len && self.on_ground {
                    self.kl_jump(level, sti);
                }
            }
            st::JUMP => {
                gravity = false;
                self.v3_fall(self.v3.turn, MAX_FALL);
                if self.vy <= 0.0 && self.v3.turn > ENEMY_GRAVITY {
                    self.v3.turn = ENEMY_GRAVITY; // the saved gravity at the apex (SMB3) [obs]
                }
                if self.kl_who() == who::LUDWIG && self.vy <= 0.0 {
                    self.kl_enter(st::JUMP_KEEP);
                    self.timer = 160;
                    self.v3.clock = 210;
                    self.vx = 0.3 * self.dir as f32;
                } else if self.on_ground && self.vy <= 0.0 && self.v3.anim > 2 {
                    self.vx = 0.0;
                    self.kl_enter(st::JUMP_ED);
                }
            }
            st::JUMP_ED => {
                self.vx = 0.0;
                let len = if self.kl_who() == who::LEMMY { 5 } else { 10 };
                if self.v3.anim >= len {
                    self.kl_enter(st::WAIT);
                    self.kl_wait_timer();
                }
            }
            st::READY => {
                self.vx = 0.0;
                // throw_st (5 frames, Lemmy riding 10) then throw_st_wait held
                let lead = if self.v3.flags & fl::ON_BALL != 0 {
                    10
                } else {
                    5
                };
                if self.v3.anim >= lead + self.timer {
                    self.kl_enter(st::ATTACK);
                }
            }
            st::ATTACK => {
                self.vx = 0.0;
                let (shot_at, len) = self.kl_attack_anim(sti);
                if self.kl_who() == who::WENDY
                    && self.v3.anim == if sti == 3 { 6 } else { 1 }
                    && self.on_ground
                {
                    self.vy = 4.0; // she hops as she throws [obs 0x71012bed60]
                    self.on_ground = false;
                }
                if self.v3.anim == shot_at + 1 {
                    self.v3.shots = 1;
                    self.v3.count2 += 1;
                }
                if self.v3.anim >= len {
                    if self.v3.count2 < self.kl_volley() {
                        self.kl_enter(st::READY);
                        self.timer = self.kl_hold(false);
                    } else {
                        self.v3.flags |= fl::ATTACKED;
                        self.kl_enter(st::WAIT);
                        self.kl_wait_timer();
                        if self.kl_who() == who::ROY {
                            self.kl_enter(st::PRE_DIVE);
                        }
                    }
                }
            }
            st::DAMAGE => {
                self.vx = 0.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    if self.v3.hp == 0 {
                        self.die_fall(self.x - self.dir as f32);
                    } else {
                        self.v3.holding = true;
                        self.kl_enter(st::SHELL_IN);
                        self.vy = 4.0; // shell_in hop [obs 0x710106f770]
                        self.on_ground = false;
                    }
                }
            }
            st::SHELL_IN => {
                if self.v3.anim >= 15 && self.on_ground {
                    self.kl_enter(st::SHELL_WAIT);
                    self.timer = SHELL_WAIT_WU;
                }
            }
            st::SHELL_WAIT => {
                self.vx = 0.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    if sti == 3 {
                        self.kl_enter(st::SHELL_SLIDE);
                        self.timer = SHELL_SLIDE_FRAMES;
                        self.dir = self.v3_toward();
                    } else {
                        self.kl_enter(st::SHELL_JUMP);
                        self.v3.count = 1 + self.rand(3) as u8;
                        self.kl_shell_hop();
                    }
                }
            }
            st::SHELL_JUMP => {
                if self.on_ground && self.vy <= 0.0 && self.v3.anim > 2 {
                    self.vx = 0.0;
                    self.v3.count = self.v3.count.saturating_sub(1);
                    if self.v3.count == 0 || self.v3.anim > 120 {
                        self.kl_recover();
                    } else {
                        self.kl_shell_hop();
                    }
                }
            }
            st::SHELL_SLIDE => {
                // NSMBU: the shell spins along the ground, chasing (accel 0.06, ≤ 3.0) for
                // 300 frames, turning at walls [obs: frames, cap; the speed curve inf]
                let t = 3.0 * self.v3_toward() as f32;
                self.vx += (t - self.vx).clamp(-0.06, 0.06);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.vx = 0.0;
                    self.kl_recover();
                }
            }
            st::RECOVER => {
                if self.on_ground && self.v3.anim > 10 {
                    self.vx = 0.0;
                    self.kl_enter(st::WAIT);
                    self.kl_wait_timer();
                }
            }
            st::WALK => self.kl_walk(level),
            st::CREATE_BALL => {
                gravity = self.kl_create_ball(sti);
            }
            st::RIDE_SIGN => {
                self.vx = 0.0;
                if self.v3.anim >= 60 {
                    self.kl_enter(st::RIDE_WALK);
                    self.v3.speed = (32 + self.rand(49)) as f32;
                }
            }
            st::RIDE_WALK => {
                self.vx = 1.0 * self.dir as f32;
                let gone = (self.x - self.v3.home).abs();
                self.v3.speed -= 1.0;
                if self.v3.speed <= 0.0
                    || self.v3.anim >= 120
                    || self.wall
                    || self.v3_ledge(level, self.dir)
                    || gone > HOME_RANGE
                {
                    self.kl_enter(st::RIDE_STOP);
                }
            }
            st::RIDE_STOP => {
                self.vx -= self.vx.clamp(-0.5, 0.5);
                if self.vx.abs() < 1e-3 {
                    self.vx = 0.0;
                    self.kl_enter(st::WAIT);
                    self.kl_wait_timer();
                }
            }
            st::QUAKE_ST..=st::QUAKE => gravity = self.kl_morton(sti),
            st::PRE_DIVE..=st::APPEAR_FROM => gravity = self.kl_roy(level),
            st::JUMP_KEEP | st::JUMP_KEEP_END => gravity = self.kl_ludwig(),
            _ => self.kl_enter(st::WAIT),
        }
        if gravity {
            self.v3_fall(ENEMY_GRAVITY, MAX_FALL);
        }
        if self.koopaling_hidden() {
            return None;
        }
        // Lemmy's feet are on top of his ball: the actor's origin is the ball's bottom
        self.collide(level);
        if self.wall && self.phase == st::SHELL_SLIDE {
            self.vx = -self.vx;
            self.dir = -self.dir;
        }
        None
    }

    fn kl_decide(&mut self, level: &Level, sti: usize) {
        let w = self.kl_who();
        if w == who::MORTON && self.v3.sub == 0 && self.v3.clock <= 0 && self.v3_in_view() {
            self.kl_enter(st::QUAKE_ST);
            return;
        }
        let near = (self.target.0 - self.x).dhypot(self.target.1 - self.y) < 160.0;
        let may_attack = self.v3_in_view()
            && !(w == who::MORTON && near)
            && !(w == who::IGGY && self.v3.speed > 0.0 && self.v3.clock > 0);
        if self.v3.count == 0 && self.v3.cool == 0 && may_attack {
            self.v3.count = match w {
                who::IGGY if self.v3.hp == 1 => 4 + self.rand(2) as u8,
                who::IGGY => 2 + self.rand(3) as u8,
                _ => 1 + self.rand(2) as u8,
            };
            self.v3.count2 = 0;
            if w == who::MORTON {
                self.v3.sub = self.v3.sub.saturating_sub(1);
            }
            self.kl_enter(st::READY);
            self.timer = self.kl_hold(true);
            return;
        }
        if self.v3.count > 0 {
            self.v3.count -= 1;
            if self.v3.count == 0 {
                self.v3.cool = 10;
            }
        }
        if self.kl_blocked(level, 1) && self.kl_blocked(level, -1) {
            let (lo, n) = LONG_WAIT[hp_ix(self.v3.hp)];
            self.timer = lo + self.rand(n as u32) as i32 + if self.v3.hp == 3 { 40 } else { 0 };
            return;
        }
        match w {
            who::IGGY if !self.wings => {
                self.kl_enter(st::WALK);
                self.kl_walk_start(sti);
            }
            who::LEMMY if self.v3.flags & fl::ON_BALL != 0 && !self.wings => {
                self.dir = self.kl_pick_dir(level, 92.0);
                self.kl_enter(st::RIDE_SIGN);
            }
            who::LEMMY if self.v3.flags & fl::ON_BALL == 0 => {
                self.kl_enter(st::WALK);
                self.kl_walk_start(sti);
            }
            who::ROY => self.kl_enter(st::PRE_DIVE),
            _ => self.kl_enter(st::JUMP_ST),
        }
    }

    /// Toward home once `range` px away, else a coin toss; away from blocked sides [obs].
    fn kl_pick_dir(&mut self, level: &Level, range: f32) -> i32 {
        let off = self.x - self.v3.home;
        let mut d = if off.abs() >= range {
            if off > 0.0 { -1 } else { 1 }
        } else if self.rand(2) == 0 {
            -1
        } else {
            1
        };
        if self.kl_blocked(level, d) {
            d = -d;
        }
        // stay inside the camera
        let (cx, _) = self.camera;
        if (d < 0 && self.x - 48.0 < cx) || (d > 0 && self.x + 48.0 > cx + crate::world::VIEW_W) {
            d = -d;
        }
        d
    }

    fn kl_jump(&mut self, level: &Level, sti: usize) {
        let w = self.kl_who();
        let d = self.kl_pick_dir(level, HOME_RANGE);
        let h = hp_ix(self.v3.hp);
        let wing = self.wings;
        let (mut vx, mut vy, mut g) = match w {
            who::MORTON => {
                // [obs slots 92 / 80 / 94]: land / wing by HP
                self.v3.sub = self.v3.sub.saturating_sub(1);
                if wing {
                    [
                        (1.0, 6.2 * 0.84, -0.45),
                        (1.1, 7.4 * 0.88, -0.5),
                        (1.2, 7.7 * 0.86, -0.5),
                    ][h]
                } else {
                    [
                        (0.7, 3.8 * 0.8, -0.3),
                        (1.0, 4.9 * 0.89, -0.35),
                        (1.3, 5.8 * 0.85, -0.4),
                    ][h]
                }
            }
            who::ROY => (0.8, 5.0 * 0.83, -0.35),
            who::LUDWIG => (
                if wing { 0.5 } else { 0.3 },
                if wing { 4.4 } else { 4.0 } * 0.83,
                -0.08,
            ),
            who::LARRY if self.v3.hp == 1 => {
                if sti == 1 {
                    (0.3, 5.6, JUMP_G[sti])
                } else {
                    (0.7, 6.0, JUMP_G[sti])
                }
            }
            _ => (if sti == 1 { 0.3 } else { 0.8 }, 5.0, JUMP_G[sti]),
        };
        let max = if wing {
            7
        } else if w == who::LARRY && self.v3.hp == 1 {
            4
        } else {
            3
        };
        if !matches!(w, who::MORTON | who::ROY | who::LUDWIG)
            && let Some(n) = self.kl_ledge_up(level, d, max)
        {
            vy = if sti == 1 {
                LEDGE_VY_M3.get(n as usize - 1).copied().unwrap_or(5.4)
            } else {
                LEDGE_VY.get(n as usize - 1).copied().unwrap_or(6.0)
            };
            g = JUMP_G[sti];
        }
        if level
            .cell_at(self.x + d as f32 * 24.0, self.y + 8.0)
            .is_solid()
        {
            vx *= 0.85; // a low obstacle ahead [obs slot 79]
        }
        self.kl_enter(st::JUMP);
        self.vx = vx * d as f32;
        self.vy = vy;
        self.v3.turn = g;
        self.on_ground = false;
    }

    fn kl_shell_hop(&mut self) {
        // 1–3 hops toward the player, vy 5.0, vx by the distance [obs 0x710106ae60]
        let dx = self.target.0 - self.x;
        let (lo, hi) = if dx.abs() < 32.0 {
            (0.5, 0.8)
        } else if dx.abs() < 64.0 {
            (0.8, 1.0)
        } else {
            (1.0, 1.2)
        };
        let r = self.rand(1001) as f32 / 1000.0;
        self.dir = if dx < 0.0 { -1 } else { 1 };
        self.vx = (lo + (hi - lo) * r) * self.dir as f32;
        self.vy = 5.0;
        self.on_ground = false;
        self.v3.anim = 0;
    }

    fn kl_recover(&mut self) {
        self.v3.holding = false;
        self.dir = self.v3_toward();
        self.kl_enter(st::RECOVER);
        self.vx = 0.8 * self.dir as f32;
        self.vy = 1.0;
        self.on_ground = false;
        if self.kl_who() == who::MORTON {
            self.v3.sub = 0;
            self.v3.clock = 0;
        }
    }

    // ------------------------------------------------------------ Iggy / Lemmy walks

    fn kl_walk_start(&mut self, sti: usize) {
        let w = self.kl_who();
        let off = self.x - self.v3.home;
        self.dir = if off.abs() >= HOME_RANGE {
            if off > 0.0 { -1 } else { 1 }
        } else if self.rand(2) == 0 {
            -1
        } else {
            1
        };
        if w == who::IGGY {
            // 32 + 0…48 px, kept within home ± 96 [obs]; attacks blocked 100 / 80 / 60
            self.v3.speed = (32 + self.rand(49)) as f32;
            self.v3.clock = [100, 80, 60][hp_ix(self.v3.hp)];
            self.vx = 0.0;
        } else {
            self.v3.speed = (8 + self.rand(25)) as f32;
            self.v3.clock = [100, 80, 60][hp_ix(self.v3.hp)];
        }
        let _ = sti;
    }

    fn kl_walk(&mut self, level: &Level) {
        let iggy = self.kl_who() == who::IGGY;
        let top = if iggy {
            if level.style == "M3" { 1.5 } else { 1.0 }
        } else if self.wings {
            0.75
        } else {
            0.5
        };
        let t = top * self.dir as f32;
        if iggy {
            self.vx += (t - self.vx).clamp(-0.02, 0.02);
        } else {
            self.vx = t;
        }
        self.v3.speed -= self.vx.abs();
        let off = self.x - self.v3.home;
        let cap = if iggy { 60 } else { 120 };
        if self.v3.speed <= 0.0
            || self.v3.anim >= cap
            || self.wall
            || self.v3_ledge(level, self.dir)
            || (off * self.dir as f32 > HOME_RANGE)
        {
            self.vx = 0.0;
            self.kl_enter(st::WAIT);
            self.kl_wait_timer();
            return;
        }
        // a one-tile step ahead is hopped (vy 2.5) [obs]
        if iggy && self.on_ground {
            let x = self.x + self.dir as f32 * 20.0;
            if level.cell_at(x, self.y + 4.0).is_solid()
                && !level.cell_at(x, self.y + 4.0 + TILE).is_solid()
            {
                self.vy = 2.5;
                self.on_ground = false;
            }
        }
    }

    fn kl_room_for_ball(&self, level: &Level) -> bool {
        let s = self.v3_scale();
        let need = 16.0 * s + 32.0 * s;
        [-8.0, 0.0, 8.0].iter().all(|&dx| {
            let mut y = self.y + 1.0;
            while y < self.y + need {
                if level.cell_at(self.x + dx * s, y).is_solid() {
                    return false;
                }
                y += 4.0;
            }
            true
        })
    }

    /// Lemmy climbs back onto a new ball: rises along 20·sin(2π·0.36·t) px (t = frame / 30),
    /// the ball growing from frame 15 [obs 0x71011b7950]. Returns whether gravity applies.
    fn kl_create_ball(&mut self, sti: usize) -> bool {
        let f = self.v3.anim as f32;
        let (amp, per) = if sti == 3 { (30.0, 40.0) } else { (20.0, 30.0) };
        if f <= per {
            let y = |f: f32| amp * (std::f32::consts::TAU * 0.36 * f / per).dsin();
            self.vy = (y(f) - y(f - 1.0)) * self.v3_scale();
            self.vx = 0.0;
            if self.v3.anim >= 15 {
                self.v3.len = (self.v3.len + 0.2).min(if sti == 3 { 1.5 } else { 1.0 });
            }
            if self.v3.anim == 15 {
                self.v3.len = 0.2;
            }
            return false;
        }
        if self.on_ground || self.v3.anim > 90 {
            self.v3.flags |= fl::ON_BALL;
            self.v3.len = if sti == 3 { 1.5 } else { 1.0 };
            self.kl_enter(st::WAIT);
        }
        true
    }

    // ------------------------------------------------------------ Morton

    /// N (moves before the next ground pound) and its cooldown [obs 0x71011e9440].
    fn morton_reroll(&mut self, sti: usize, after_quake: bool) {
        let h = hp_ix(self.v3.hp.max(1));
        let choices: [[u8; 2]; 3] = [[2, 3], [1, 3], [1, 2]];
        let n = choices[h][self.rand(2) as usize];
        self.v3.sub = n;
        let (k, base) = if sti == 3 { (50, 180) } else { (40, 130) };
        self.v3.clock = k * n as i32 + if after_quake { base } else { 0 };
    }

    fn kl_morton(&mut self, sti: usize) -> bool {
        match self.phase {
            st::QUAKE_ST => {
                self.vx = 0.0;
                if self.v3.anim >= 5 {
                    self.kl_enter(st::QUAKE_JUMP);
                    let wing = self.wings;
                    self.vx = 0.5 * self.v3_toward() as f32;
                    self.vy = if wing { 7.0 } else { 6.0 } * 0.86;
                    self.v3.turn = if wing { -0.195 } else { -0.17 };
                    self.on_ground = false;
                }
                true
            }
            st::QUAKE_JUMP => {
                self.v3_fall(self.v3.turn, MAX_FALL);
                if self.vy <= 0.0 {
                    self.kl_enter(st::QUAKE_SIGN);
                    self.vx = 0.0;
                    self.vy = 0.0;
                }
                false
            }
            st::QUAKE_SIGN => {
                // hangs: spins 10 frames, holds 10 [obs]
                self.vx = 0.0;
                self.vy = 0.0;
                if self.v3.anim >= 20 {
                    self.kl_enter(st::QUAKE_DROP);
                }
                false
            }
            st::QUAKE_DROP => {
                let g = if self.wings { -0.45 } else { -0.3 };
                self.v3_fall(g, -6.0);
                if self.on_ground {
                    self.kl_enter(st::QUAKE);
                    self.v3.quake = true;
                    self.v3.flames = true;
                }
                false
            }
            _ => {
                // jump_quake_ed (60 frames in 2D) → Wait; a new count [obs]
                self.vx = 0.0;
                if self.v3.anim >= 60 {
                    self.morton_reroll(sti, true);
                    self.kl_enter(st::WAIT);
                }
                true
            }
        }
    }

    // ------------------------------------------------------------ Roy

    fn kl_roy(&mut self, level: &Level) -> bool {
        match self.phase {
            st::PRE_DIVE => {
                self.vx = 0.0;
                if self.on_ground && self.timer <= 0 {
                    let below = level.cell_at(self.x, self.y - 1.0).is_solid();
                    if below {
                        self.kl_enter(st::DIVE);
                    } else {
                        self.kl_enter(st::WAIT); // nothing to dig into [obs 0x710124aa20]
                        self.phase = st::WAIT;
                        self.v3.count = 0;
                    }
                }
                true
            }
            st::DIVE => {
                // anim dive (44 frames); he sinks over 15 frames at its end [obs]
                self.vx = 0.0;
                if self.v3.anim >= 44 {
                    self.v3.flags |= fl::HIDDEN;
                    self.kl_enter(st::IN_GROUND);
                    self.timer = 60;
                }
                true
            }
            st::IN_GROUND => {
                self.vx = 0.0;
                self.vy = 0.0;
                // once the timer is out, one spot is tried per frame until one is found: no
                // retry delay (after 3 misses the game switches surface mode) [obs
                // InGroundSearch exe 0x7101241b80]
                if self.timer > 0 {
                    self.timer -= 1;
                }
                if self.timer <= 0 {
                    self.kl_roy_surface(level);
                }
                false
            }
            st::APPEAR_HEAD => {
                let len = if self.v3.hp <= 2 { 14 } else { 20 };
                if self.v3.anim >= len {
                    self.kl_enter(if self.v3.hp == 1 {
                        st::APPEAR_FROM
                    } else {
                        st::LOOK_AROUND
                    });
                }
                true
            }
            st::LOOK_AROUND => {
                if self.v3.anim >= 60 {
                    self.kl_enter(st::APPEAR_FROM);
                }
                true
            }
            _ => {
                // AppearFromGround: grows out over 15 frames of its 40-frame animation → Wait
                if self.v3.anim >= 40 {
                    self.kl_enter(st::WAIT);
                    self.dir = self.v3_toward();
                }
                true
            }
        }
    }

    /// One try at a floor spot inside the view, away from the player (x ±3 tiles, y −1…+4
    /// tiles), not in water [obs 0x7101241b80: one try per frame; the ceiling exits (≤ 33 %) and
    /// the object-surface weighting are not modelled: inf].
    fn kl_roy_surface(&mut self, level: &Level) -> bool {
        let (cx, cy) = self.camera;
        let x = cx + 32.0 + self.rand((crate::world::VIEW_W - 64.0) as u32) as f32;
        let mut y = cy + crate::world::VIEW_H - TILE;
        while y > cy {
            let here = level.cell_at(x, y);
            let below = level.cell_at(x, y - TILE);
            if here == Cell::Empty && below.is_solid() {
                break;
            }
            y -= TILE;
        }
        if y <= cy {
            return false;
        }
        let fy = (y / TILE).floor() * TILE;
        let (px, py) = self.target;
        if (px - x).abs() <= 3.0 * TILE && py - fy >= -TILE && py - fy <= 4.0 * TILE {
            return false;
        }
        self.x = x;
        self.y = fy;
        self.v3.flags &= !fl::HIDDEN;
        self.dir = self.v3_toward();
        self.kl_enter(st::APPEAR_HEAD);
        true
    }

    // ------------------------------------------------------------ Ludwig

    fn kl_ludwig(&mut self) -> bool {
        match self.phase {
            st::JUMP_KEEP => {
                // hovers in his shell: bob 24·(sin(θ − 4°) − sin θ) per frame; heads for the
                // player at |dx|/60 (≥ 0.25), accel 0.05, ≤ 2.0 [obs 0x71011c1390]
                let th = self.heading;
                self.heading -= 4.0;
                self.vy = 24.0 * ((th - 4.0).to_radians().dsin() - th.to_radians().dsin());
                let dx = self.target.0 - self.x;
                let want = (dx.abs() / 60.0).clamp(0.25, 2.0) * dx.signum();
                self.vx += (want - self.vx).clamp(-0.05, 0.05);
                self.timer -= 1;
                self.v3.clock -= 1;
                if dx.abs() < 4.0 || self.timer <= 0 || self.v3.clock <= 0 {
                    self.kl_enter(st::JUMP_KEEP_END);
                }
                false
            }
            _ => {
                self.vx -= self.vx.clamp(-0.2, 0.2);
                if self.on_ground && self.v3.anim > 2 {
                    self.vx = 0.0;
                    self.kl_enter(st::JUMP_ED);
                }
                true
            }
        }
    }

    // ------------------------------------------------------------ projectiles

    /// The projectiles of one shot (the world adds them): aimed at the player ±10°, inside
    /// ±60° of level on the facing side (Ludwig ±90°), fanned 30° apart [obs 0x7100ddb8d0].
    pub fn koopaling_shots(&mut self, level: &Level) -> Vec<Actor> {
        let w = self.kl_who();
        let sti = style_ix(level);
        let s = self.v3_scale();
        let (mx, my) = (
            self.x + 14.0 * s * self.dir as f32,
            self.y + self.kl_ball_lift() + 22.0 * s,
        );
        let what = match w {
            who::WENDY => shot::RING,
            who::LEMMY => shot::BALL,
            _ if sti == 3 => shot::FIRE,
            _ => shot::MAGIC,
        };
        let n = if w == who::LUDWIG {
            if self.v3.hp == 1 { 5 } else { 3 }
        } else {
            1
        };
        let mut out = Vec::new();
        for i in 0..n {
            let mut a = Actor::new(Kind::KoopalingShot { what }, mx, my);
            a.big = self.big;
            a.dir = self.dir;
            a.v3.owner = self.id;
            let (hw, h) = a.koopaling_size();
            a.y = my - h / 2.0;
            let _ = hw;
            match what {
                shot::RING => {
                    // 1.2 px/f, 45° down the facing side [obs 0x7100dfc5e0]
                    a.vx = 0.8485 * self.dir as f32;
                    a.vy = -0.8485;
                    a.timer = 1200;
                }
                shot::BALL => {
                    a.vx = 0.6 * self.dir as f32;
                    a.vy = 0.0;
                }
                _ => {
                    let speed = match (w, what) {
                        (who::IGGY, shot::FIRE) => 1.5,
                        (who::LUDWIG, shot::FIRE) => 0.75,
                        (_, shot::FIRE) => 1.0,
                        (who::IGGY, _) => 1.65,
                        (who::LUDWIG, _) => 0.8,
                        _ => 1.2,
                    };
                    if w == who::LUDWIG {
                        a.v3.flags |= 1; // no trail: the smaller box
                    }
                    let cone = if w == who::LUDWIG { 90.0 } else { 60.0 };
                    let (px, py) = (self.target.0, self.target.1 + 12.0);
                    let mut ang = (py - my).datan2((px - mx) * self.dir as f32).to_degrees();
                    let jitter = -10.0 + 20.0 * self.rand(256) as f32 / 255.0;
                    ang = (ang + jitter).clamp(-cone, cone);
                    let k = (i + 1) >> 1;
                    let off = if i % 2 == 0 { 1.0 } else { -1.0 } * 30.0 * k as f32
                        + if n % 2 == 0 { 15.0 } else { 0.0 };
                    ang += off;
                    let rad = ang.to_radians();
                    a.speed = speed;
                    a.v3.speed = 0.75; // the starting drift [obs 0x7100dd9eb0]
                    a.heading = if self.dir > 0 { ang } else { 180.0 - ang };
                    a.vx = rad.dcos() * self.dir as f32 * speed;
                    a.vy = rad.dsin() * speed;
                }
            }
            out.push(a);
        }
        out
    }

    /// Morton's two ground flames: one each way from (x, y + 4) [obs 0x71011eab30].
    pub fn morton_flames(&self) -> [Actor; 2] {
        let s = self.v3_scale();
        [1, -1].map(|d| {
            let mut a = Actor::new(Kind::KoopalingShot { what: shot::FLAME }, self.x, self.y);
            a.big = self.big;
            a.dir = d;
            a.v3.owner = self.id;
            a.vx = 1.5 * d as f32;
            a.y = self.y + 4.0 * s - 4.0 * s;
            a.heading = 0.0; // surface normal: up (0), wall/ceiling turns in `step_kl_shot`
            a
        })
    }

    fn step_kl_shot(&mut self, level: &Level, what: u8) {
        let (hw, h) = self.koopaling_size();
        match what {
            shot::RING => {
                // perfect reflections off floors, ceilings and walls; at most 1200 frames
                // [obs 0x7100dfc680]
                let r = 7.5 * self.v3_scale();
                let (cx, cy) = (self.x + self.vx, self.y + h / 2.0 + self.vy);
                if level.cell_at(cx + r * self.vx.signum(), cy).is_solid() {
                    self.vx = -self.vx;
                }
                if level.cell_at(cx, cy + r * self.vy.signum()).is_solid() {
                    self.vy = -self.vy;
                }
                self.x += self.vx;
                self.y += self.vy;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Dead;
                }
            }
            shot::BALL => {
                // vx ±0.6 constant, gravity −0.1 (cap −4), every floor landing vy 3.3 (big
                // 3.8), a ceiling stops the rise, a wall knocks it away [obs 0x7100de8e60]
                self.v3_fall(-0.1, -4.0);
                let up = self.vy;
                self.collide(level);
                if self.on_ground {
                    self.vy = if self.big { 3.8 } else { 3.3 };
                    self.on_ground = false;
                } else if up > 0.0 && self.vy == 0.0 {
                    self.vy = 0.0;
                }
                if self.wall {
                    self.mode = Mode::DieFall;
                    self.vx = -0.1 * self.dir as f32;
                    self.vy = 1.0;
                }
                self.vx = if self.mode == Mode::Normal {
                    0.6 * self.dir as f32
                } else {
                    self.vx
                };
            }
            shot::FLAME => self.step_flame(level),
            _ => {
                // magic: straight, plus a starting drift decaying 0.09/frame and 1 %; a wall
                // ends it [obs 0x7100ddd910, 0x7100ddd940]
                self.v3.speed = (self.v3.speed - 0.09).max(0.0) * 0.99;
                let v = self.speed + self.v3.speed;
                let rad = self.heading.to_radians();
                self.vx = rad.dcos() * v;
                self.vy = rad.dsin() * v;
                self.x += self.vx;
                self.y += self.vy;
                if level.cell_at(self.x, self.y + h / 2.0).is_solid() {
                    self.mode = Mode::Dead;
                }
                self.dir = if self.vx < 0.0 { -1 } else { 1 };
            }
        }
        let _ = hw;
    }

    /// MortonDossunFire: crawls 1.5 px/f along the surface it is on — floor, wall, ceiling —
    /// wrapping round corners, and goes after 500 px [obs 0x7100decd90]. `heading` is the
    /// surface normal (0 up, 90 left wall … as degrees), `vx`/`vy` the travel.
    fn step_flame(&mut self, level: &Level) {
        let r = 4.0 * self.v3_scale();
        let n = self.heading.to_radians();
        let (nx, ny) = (-n.dsin(), n.dcos()); // 0° = normal up
        let (cx, cy) = (self.x, self.y + r);
        let (dx, dy) = (
            self.vx.signum() * (self.vx != 0.0) as i32 as f32,
            self.vy.signum(),
        );
        let (ux, uy) = if self.vx.abs() > self.vy.abs() {
            (dx, 0.0)
        } else {
            (0.0, dy)
        };
        let solid = |x: f32, y: f32| level.cell_at(x, y).is_solid();
        let ahead = solid(cx + ux * (r + 1.5), cy + uy * (r + 1.5));
        let under = solid(
            cx - nx * (r + 1.0) + ux * 1.5,
            cy - ny * (r + 1.0) + uy * 1.5,
        );
        let (mut ux2, mut uy2, mut nx2, mut ny2) = (ux, uy, nx, ny);
        if ahead {
            // concave corner: climb the wall ahead (the old normal is the new travel)
            (ux2, uy2, nx2, ny2) = (nx, ny, -ux, -uy);
        } else if !under {
            // convex corner: wrap round onto the face below the edge
            (ux2, uy2, nx2, ny2) = (-nx, -ny, ux, uy);
        }
        let speed = 1.5;
        self.vx = ux2 * speed;
        self.vy = uy2 * speed;
        self.x += self.vx;
        self.y += self.vy;
        self.heading = (-nx2).datan2(ny2).to_degrees();
        self.v3.len += speed;
        if self.v3.len >= 500.0 {
            self.mode = Mode::Dead;
        }
    }

    // ------------------------------------------------------------ looks

    pub(super) fn koopaling_look(&self) -> Option<V3Look> {
        if self.lemmy_ball().is_some() && self.mode == Mode::Normal {
            return None; // drawn on top of his ball (world/koopalings.rs)
        }
        self.koopaling_body_look()
    }

    pub fn koopaling_body_look(&self) -> Option<V3Look> {
        let face = self.dir > 0;
        let t = self.age as i32;
        if let Kind::KoopalingShot { what } = self.kind {
            let (hw, h) = self.koopaling_size();
            let _ = hw;
            let owner_model = match what {
                shot::RING => "M1_Enemy_wendy",
                shot::BALL => "M1_Enemy_lemmy",
                shot::FLAME => "M1_Enemy_morton",
                _ => "M1_Enemy_larry",
            };
            let tex = match what {
                shot::RING => anim_key(
                    &[
                        (0, "ring.0"),
                        (12, "ring.1"),
                        (24, "ring.2"),
                        (37, "ring.1"),
                        (48, "ring.0"),
                    ],
                    56,
                    t,
                    true,
                ),
                shot::BALL => "ball.0",
                shot::FLAME => anim_key(&[(0, "fire.0"), (5, "fire.1")], 10, t, true),
                _ => "effect.0",
            };
            // ring and ball art is 24 × 24, the rest 16; NSMBU's ball is drawn at 2/3 [obs]
            let size = match what {
                shot::BALL if self.v3.style == 3 => 16.0,
                shot::RING | shot::BALL => 24.0,
                _ => 16.0,
            };
            let _ = h;
            return Some((owner_model, tex, size, size, face));
        }
        if self.koopaling_hidden() {
            return None;
        }
        if self.kick_lock > 0 && (self.kick_lock / 2) % 2 == 0 {
            return None; // the fireball flash
        }
        let model = MODELS[self.kl_who().min(6) as usize];
        let n = self.v3.anim;
        let tex = match self.phase {
            _ if self.v3.holding || self.phase == st::SHELL_IN => anim_key(
                &[
                    (0, "shell.0"),
                    (4, "shell.1"),
                    (8, "shell.2"),
                    (12, "shell.3"),
                ],
                16,
                t,
                true,
            ),
            st::DAMAGE => "damage.0",
            st::READY => anim_key(
                &[(0, "throw_st.0"), (2, "throw_st.1"), (4, "throw_st.2")],
                6,
                n,
                n > 5,
            ),
            st::ATTACK => anim_key(
                &[(0, "throw_ed.0"), (2, "throw_ed.1"), (4, "throw_ed.2")],
                6,
                n,
                true,
            ),
            st::QUAKE_JUMP | st::QUAKE_SIGN | st::QUAKE_DROP => anim_key(
                &[
                    (0, "jump_quake.0"),
                    (2, "jump_quake.1"),
                    (4, "jump_quake.2"),
                ],
                6,
                n,
                true,
            ),
            st::QUAKE => anim_key(
                &[
                    (0, "jump_quake_ed.0"),
                    (20, "jump_quake_ed.1"),
                    (40, "jump_quake_ed.2"),
                ],
                60,
                n,
                false,
            ),
            st::APPEAR_HEAD | st::LOOK_AROUND => "head.0",
            st::JUMP | st::JUMP_KEEP | st::JUMP_KEEP_END if self.kl_who() == who::LUDWIG => {
                anim_key(
                    &[
                        (0, "shell.0"),
                        (4, "shell.1"),
                        (8, "shell.2"),
                        (12, "shell.3"),
                    ],
                    16,
                    t,
                    true,
                )
            }
            st::JUMP | st::FALL => anim_key(&[(0, "wait.0"), (5, "wait.1")], 10, t, true),
            _ => anim_key(
                &[
                    (0, "wait.0"),
                    (30, "wait.3"),
                    (40, "wait.0"),
                    (50, "wait.3"),
                    (60, "wait.0"),
                ],
                120,
                t,
                true,
            ),
        };
        // the SMB1 frames are 40 × 40, the shell 32 × 32 (sprite sizes are the art's)
        let s = if self.v3.pop > 1.0 { self.v3.pop } else { 1.0 };
        let size = if tex.starts_with("shell") { 32.0 } else { 40.0 };
        Some((model, tex, size * s, size * s, face))
    }

    /// Where Lemmy's ball is drawn (centre) and its radius, while he rides it.
    pub fn lemmy_ball(&self) -> Option<(f32, f32, f32)> {
        if self.kl_who() != who::LEMMY {
            return None;
        }
        // as placed (not set up yet) he stands on his ball too
        let len = match self.phase {
            0 if self.v3.style == 3 => 1.5,
            0 => 1.0,
            _ if self.v3.flags & fl::ON_BALL == 0 => 0.0,
            _ => self.v3.len,
        };
        if len <= 0.0 {
            return None;
        }
        let r = 8.0 * len * self.v3_scale();
        Some((self.x, self.y + r, r))
    }

    /// The angle between the shot's travel and the player, for tests.
    pub fn koopaling_shot_heading(&self) -> f32 {
        angle_diff(0.0, self.heading)
    }
}
