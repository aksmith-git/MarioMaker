//! 3D World enemies: Ant Trooper (96-ant-trooper.md), Stingby (65-stingby.md), Skipsqueak
//! (51-skipsqueak.md), Hop-Chops (50-hop-chops.md), Porcupuffer (114-porcupuffer.md), Bully
//! (117-bully.md), Charvaargh (86-charvaargh.md) and Piranha Creeper (107-piranha-creeper.md).
//! Inferred choices are marked [inf]; DECISIONS.md lists them. The world-side rules (touching
//! them, their sprites) are in world/w3.rs.
//!
//! State layout: `phase` is the state (0 = not set up yet; numbering per enemy in the modules
//! below), `timer` the state timer, `hops` the Porcupuffer's HP; everything else lives in
//! `Actor::w3`, together with what the world senses for these enemies each frame.

use super::{Actor, ENEMY_GRAVITY, Kind, MAX_FALL};
use crate::level::{Cell, Level, TILE};

/// Per-actor state of the 3D World enemies, and what the world senses for them before each step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct W3State {
    /// Sensed: the liquid surface (px) and whether it is water (else lava or poison).
    pub liquid: Option<(f32, bool)>,
    /// Sensed: the whole area is under water (underwater theme).
    pub flooded: bool,
    /// Sensed: the nearest player is in water, its height (px) and whether it is rising in a
    /// jump.
    pub player_wet: bool,
    pub player_h: f32,
    pub player_rising: bool,
    /// Ant Trooper: the side its surface is on and the way it walks (indices into `DIRS`), and
    /// the corner it is wrapping round.
    pub down: u8,
    pub ahead: u8,
    pub corner: (f32, f32),
    /// Small counters: hops, crashes, recovery attempts, push strength.
    pub count: u8,
    pub tries: u8,
    /// A second timer: Stingby / Hop-Chops give-up, Porcupuffer cooldowns.
    pub clock: i32,
    /// Stingby patrol anchor, Porcupuffer surfacing x.
    pub anchor: f32,
    /// Piranha Creeper: distance of the head along its path (px).
    pub travel: f32,
    /// The state to go back to after a hit or a wait.
    pub resume: u8,
    /// Skipsqueak gave up going home.
    pub gave_up: bool,
}

/// Unit vectors of the crawler directions: 0 down, 1 right, 2 up, 3 left.
const DIRS: [(f32, f32); 4] = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];

fn opposite(d: u8) -> u8 {
    (d + 2) & 3
}

fn ease(v: f32, target: f32, step: f32) -> f32 {
    v + (target - v).clamp(-step, step)
}

// ---------------------------------------------------------------- Ant Trooper

/// [inf] walk speed: the shared crawler walk (Spike Top) was not decoded; a 0.49 constant is
/// seen there, and the engine's Spike Top walks at 0.5.
const ANT_WALK: f32 = 0.5;
/// Ledge wrap turn rate (deg/frame): 6, big 4 [obs].
const ANT_TURN: f32 = 6.0;
const ANT_TURN_BIG: f32 = 4.0;

mod ant {
    /// On the floor: walks with gravity (also while falling, InAir).
    pub const FLOOR: u8 = 1;
    /// On a wall or the ceiling: no gravity (Walk / RotationWall).
    pub const CRAWL: u8 = 2;
    /// Turning round a convex corner (RotationCliff).
    pub const WRAP: u8 = 3;
}

// ---------------------------------------------------------------- Stingby

const STING_PATROL: f32 = 0.2;
/// Turns 16 px past its anchor: a 32-px patrol [obs].
const STING_SPAN: f32 = 16.0;
/// X acceleration 0.06, halved when big [obs].
const STING_ACCEL: f32 = 0.06;
const STING_SEE: f32 = 96.0;
const STING_BAND: f32 = 8.0;
/// Charge speed 0.5 [obs; the spec notes it looks low for the game].
const STING_CHARGE: f32 = 0.5;
const STING_KEEP: f32 = 160.0;
const STING_GIVE_UP: i32 = 100;
/// [inf] Turn and the alert animation (Find, animation 0x0F) lengths.
const STING_TURN: i32 = 12;
const STING_ALERT: i32 = 30;

mod sting {
    pub const FLY: u8 = 1;
    pub const TURN: u8 = 2;
    pub const FIND: u8 = 3;
    pub const CHASE: u8 = 4;
}

// ---------------------------------------------------------------- Skipsqueak

const SKIP_JUMP: f32 = 2.7;
/// Take-off after more than 2 recovery attempts [obs].
const SKIP_JUMP_HIGH: f32 = 3.65;
const SKIP_GRAVITY: f32 = -0.20625;
const SKIP_GRAVITY_WATER: f32 = -0.103125;
const SKIP_RUN: f32 = 3.0;
const SKIP_RUN_ACCEL: f32 = 0.02;
const SKIP_BRAKE_ACCEL: f32 = 0.1;
const SKIP_CREEP: f32 = 0.2;
const SKIP_HOME: f32 = 1.0;
const SKIP_CRASH_VY: f32 = 1.875;
const SKIP_CRASH_KEEP: f32 = 0.35;
/// Gives up after more than 5 crashes or more than 2 failed recovery jumps [obs].
const SKIP_CRASHES: u8 = 5;
const SKIP_TRIES: u8 = 2;
/// [inf] crouch (JumpSign animation) frames before the take-off.
const SKIP_CROUCH: i32 = 6;
/// [inf] the recovery jump drifts toward home, at most this fast.
const SKIP_RECOVER_VX: f32 = 1.5;
/// Winged hover: y += 0.25·sin φ, φ += 1/72 turn per frame [obs].
const SKIP_HOVER: f32 = 0.25;

mod skip {
    pub const WAIT: u8 = 1;
    pub const FALL: u8 = 2;
    pub const CROUCH: u8 = 3;
    pub const JUMP: u8 = 4;
    pub const RETURN: u8 = 5;
    pub const CRASH: u8 = 6;
    pub const GIVE_UP: u8 = 7;
}

// ---------------------------------------------------------------- Hop-Chops

const HOP_VX: f32 = 0.5;
const HOP_VY: f32 = 1.2;
const HOP_GRAVITY: f32 = -0.1;
/// Ground frames before a patrol hop: the hop animation's airborne window starts at frame 9
/// [obs]; the chase window starts at frame 2 [obs].
const HOP_WAIT: i32 = 9;
const HOP_RUN_WAIT: i32 = 2;
const HOP_SEE: (f32, f32) = (96.0, 64.0);
const HOP_KEEP: (f32, f32) = (128.0, 80.0);
const HOP_LOST_GRACE: i32 = 16;
const HOP_FIND_VY: f32 = 2.5;
const HOP_RUN_VX: f32 = 0.6;
const HOP_RUN_GRAVITY: f32 = -0.2;
const HOP_TURN_DECEL: f32 = 0.06;
const HOP_LOST_DECEL: f32 = 0.05;
/// [inf] Turn and Lost animation lengths.
const HOP_TURN_FRAMES: i32 = 16;
const HOP_LOST_FRAMES: i32 = 30;

mod hop {
    pub const WALK: u8 = 1;
    pub const TURN: u8 = 2;
    pub const FIND: u8 = 3;
    pub const RUN: u8 = 4;
    pub const LOST: u8 = 5;
}

// ---------------------------------------------------------------- Porcupuffer

const PUFF_HP: u8 = 4;
/// Speed multiplier by HP (index = HP; 0 unused) [obs].
const PUFF_M: [f32; 5] = [0.75, 0.75, 0.8, 0.9, 1.0];
const PUFF_SWIM: f32 = 1.25;
const PUFF_SWIM_ACCEL: f32 = 0.25;
const PUFF_STEER: f32 = 2.0;
/// Lunge when lined up within acos(0.9995) ≈ 1.8° and closer than 64 px (big 80) [obs].
const PUFF_LINE_UP: f32 = 1.8;
const PUFF_LUNGE_NEAR: f32 = 64.0;
const PUFF_LUNGE_STOP: i32 = 10;
const PUFF_LUNGE_FRAMES: i32 = 60;
const PUFF_LUNGE: f32 = 2.0;
const PUFF_FLOAT: f32 = 1.875;
const PUFF_FLOAT_ACCEL: f32 = 0.075;
const PUFF_EASE: f32 = 0.75;
const PUFF_BAND: f32 = 160.0;
const PUFF_JUMP_WINDUP: i32 = 30;
const PUFF_JUMP: f32 = 4.0;
const PUFF_JUMP_RAMP: f32 = 0.75;
const PUFF_DIVE_DEPTH: f32 = 16.0;
const PUFF_COOLDOWN: i32 = 60;
/// [inf] SwimTurn: 12 steps of the yaw turn, then a 30-frame cooldown; Recover 20 frames.
const PUFF_TURN_FRAMES: i32 = 12;
const PUFF_TURN_COOLDOWN: i32 = 30;
const PUFF_RECOVER: i32 = 20;

mod puff {
    pub const FALL: u8 = 1;
    pub const SWIM: u8 = 2;
    pub const SWIM_TURN: u8 = 3;
    pub const LUNGE: u8 = 4;
    pub const FLOAT: u8 = 5;
    pub const SURFACE: u8 = 7;
    pub const SURFACE_JUMP: u8 = 8;
    pub const RECOVER: u8 = 9;
}

// ---------------------------------------------------------------- Bully

const BULLY_WAIT: i32 = 40;
const BULLY_WALK: f32 = 0.3;
const BULLY_CHASE: f32 = 0.8;
const BULLY_ACCEL: f32 = 0.025;
const BULLY_GIVE_UP: f32 = 256.0;
/// [inf] the Find trigger range is open: 128 px either side, 48 px up or down.
const BULLY_SEE: (f32, f32) = (128.0, 48.0);
/// [inf] Find animation length.
const BULLY_FIND: i32 = 30;
/// Push-back speed by hit strength [obs table]; which attack gives which index is [inf].
const BULLY_PUSH: [f32; 3] = [1.0, 1.75, 2.5];
const BULLY_PUSH_DECAY: f32 = 0.05;
const BULLY_FLIP: i32 = 60;
const BULLY_HOLD_OUT: i32 = 120;
const BULLY_FLY_CHASE: i32 = 300;
const BULLY_FLY_ACCEL: f32 = 0.075;
const BULLY_FLY_SPEED: f32 = 0.5;

/// Hit strengths for `Actor::bully_push` (indices into the push table) [inf mapping].
pub mod push {
    /// Fireballs and other light hits.
    pub const LIGHT: usize = 0;
    /// Stomps and spin jumps.
    pub const STOMP: usize = 1;
    /// Ground pounds and shoe stomps: the Bully is then stunned (HoldOut).
    pub const HEAVY: usize = 2;
}

mod bully {
    pub const WAIT: u8 = 1;
    pub const WALK: u8 = 2;
    pub const FIND: u8 = 3;
    pub const CHASE: u8 = 4;
    pub const FLIP: u8 = 8;
    pub const HOLD_OUT: u8 = 9;
    pub const FLY_CHASE: u8 = 11;
    pub const FLY_RETURN: u8 = 13;
}

// ---------------------------------------------------------------- Charvaargh

/// Detection: ±128 px horizontally, −88…+160 px vertically from the head [obs].
const CHAR_SEE_X: f32 = 128.0;
const CHAR_SEE_Y: (f32, f32) = (-88.0, 160.0);
const CHAR_LOCKOUT: i32 = 30;
const CHAR_SIGN: i32 = 30;
const CHAR_SIGN_RISE: f32 = 24.0;
/// 8 body segments, 12 px apart along the path [obs].
const CHAR_SEGMENTS: usize = 8;
const CHAR_SEGMENT_GAP: f32 = 12.0;
/// [inf] the arc is not decoded: the head leaps at 2.5 px/f round a half circle of radius 64
/// (its heading turns ±90° either side of up), then dives back.
const CHAR_SPEED: f32 = 2.5;
const CHAR_RADIUS: f32 = 64.0;
/// [inf] at rest the 64-px head sits just under the surface (its top 4 px below).
const CHAR_DEPTH: f32 = 4.0;

mod chr {
    pub const FALL: u8 = 1;
    pub const WAIT: u8 = 2;
    pub const SIGN: u8 = 3;
    /// Attack: rise (`count` 0), arc (1), dive (2).
    pub const ATTACK: u8 = 5;
}

// ---------------------------------------------------------------- Piranha Creeper

const CREEP_SPEED: f32 = 0.6;
const CREEP_ACCEL: f32 = 0.1;
/// Braking near a stop: 10 % of the gap, at least 0.05 px/f [obs].
const CREEP_BRAKE: f32 = 0.1;
const CREEP_CRAWL: f32 = 0.05;
const CREEP_DAMAGE_WAIT: i32 = 130;
/// [inf] start delay, the wait at either end and the trampled animation.
const CREEP_WAIT: i32 = 60;
const CREEP_TRAMPLED: i32 = 10;
/// A path cell is 2×2 tiles (the stalk pieces are 32 px long) [inf from the models].
pub const CREEP_CELL: f32 = 32.0;
/// Head box half size [inf].
const CREEP_HEAD: f32 = 12.0;

mod creep {
    pub const MOVE: u8 = 1;
    pub const MOVE_BACK: u8 = 2;
    pub const WAIT: u8 = 3;
    pub const SLEEP: u8 = 5;
    pub const DAMAGE: u8 = 6;
    pub const DAMAGE_WAIT: u8 = 7;
}

/// Direction (index into `DIRS`) a Piranha Creeper path node leads to. Node codes [inf, from
/// the course corpus]: 1–4 straight right / left / down / up; 5–12 corners (5 right→down,
/// 6 down→right, 7 right→up, 8 up→right, 9 left→down, 10 down→left, 11 left→up, 12 up→left);
/// 13–16 the end cap facing left / right / up / down.
pub fn creeper_out(code: u8) -> u8 {
    match code {
        1 | 6 | 8 | 14 => 1,
        2 | 10 | 12 | 13 => 3,
        3 | 5 | 9 | 16 => 0,
        _ => 2,
    }
}

/// A Piranha Creeper path node is a corner piece.
pub fn creeper_corner(code: u8) -> bool {
    (5..=12).contains(&code)
}

impl Kind {
    /// One of the 3D World enemies of this module.
    pub fn is_w3(self) -> bool {
        matches!(
            self,
            Kind::AntTrooper { .. }
                | Kind::Stingby
                | Kind::Skipsqueak { .. }
                | Kind::HopChops
                | Kind::Porcupuffer
                | Kind::Bully
                | Kind::Charvaargh
                | Kind::PiranhaCreeper { .. }
        )
    }

    /// The 3D World enemies that take the generic enemy paths (fireballs, shells, POW, Star).
    /// Charvaargh does not: nothing defeats it [inf].
    pub fn w3_enemy(self) -> bool {
        self.is_w3() && self != Kind::Charvaargh
    }

    /// Survives in lava and poison (Charvaargh lives there; Porcupuffer swims on any liquid
    /// [inf]).
    pub fn liquid_proof(self) -> bool {
        matches!(self, Kind::Charvaargh | Kind::Porcupuffer)
    }

    /// Its model's origin is the body centre, not the feet (Stingby, Porcupuffer, Charvaargh,
    /// the creeper's head) [obs from the models' bounds].
    pub fn w3_centred(self) -> bool {
        matches!(
            self,
            Kind::Stingby | Kind::Porcupuffer | Kind::Charvaargh | Kind::PiranhaCreeper { .. }
        )
    }
}

/// A sprite choice: model, texture (animation frame), width, height, mirror.
pub type W3Look = (&'static str, &'static str, f32, f32, bool);

impl Actor {
    /// Half width and height of the body box.
    pub(super) fn w3_size(&self) -> (f32, f32) {
        let s = if self.big { 2.0 } else { 1.0 };
        let (hw, h) = match self.kind {
            Kind::Porcupuffer | Kind::Bully => (16.0, 32.0),
            Kind::Charvaargh => (32.0, 64.0),
            Kind::PiranhaCreeper { .. } => (CREEP_HEAD, 2.0 * CREEP_HEAD),
            _ => (8.0, 16.0),
        };
        (hw * s, h * s)
    }

    fn size_k(&self) -> f32 {
        if self.big { 2.0 } else { 1.0 }
    }

    fn toward_player(&self) -> i32 {
        if self.target.0 < self.x { -1 } else { 1 }
    }

    /// One frame of a 3D World enemy (from `step_normal`).
    pub(super) fn step_w3(&mut self, level: &Level) -> Option<Actor> {
        match self.kind {
            Kind::AntTrooper { .. } => self.step_ant(level),
            Kind::Stingby => self.step_stingby(level),
            Kind::Skipsqueak { .. } => self.step_skipsqueak(level),
            Kind::HopChops => self.step_hop_chops(level),
            Kind::Porcupuffer => self.step_porcupuffer(level),
            Kind::Bully => self.step_bully(level),
            Kind::Charvaargh => self.step_charvaargh(level),
            Kind::PiranhaCreeper { path, sleeps } => self.step_creeper(level, path, sleeps),
            _ => {}
        }
        None
    }

    // ------------------------------------------------------------ Ant Trooper

    /// Ant Trooper (96-ant-trooper.md, the Spike Top crawler): walks the terrain contour. On the
    /// floor it walks with gravity; a wall ahead is climbed (the wall becomes its surface), and
    /// at a ledge it wraps round the corner at 6°/frame (big 4°) and carries on down the face,
    /// along ceilings and back up. The body is a square box, so turning does not change it.
    /// Winged: the generic flight [inf: the Wing flyer was not decoded].
    fn step_ant(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = ant::FLOOR;
        }
        if self.wings {
            self.phase = ant::FLOOR;
            self.step_generic_wings(level);
            return;
        }
        let h = 8.0 * self.size_k();
        match self.phase {
            ant::CRAWL => self.ant_crawl(level, h),
            ant::WRAP => self.ant_wrap(h),
            _ => self.ant_floor(level, h),
        }
    }

    fn ant_floor(&mut self, level: &Level, h: f32) {
        let dir = self.dir as f32;
        self.w3.down = 0;
        self.w3.ahead = if self.dir > 0 { 1 } else { 3 };
        if self.on_ground {
            self.vx = ANT_WALK * dir;
            // a ledge: the floor ends under the centre and the block behind is solid
            let ahead = level.cell_at(self.x + dir * 0.1, self.y - 1.0);
            let behind = level.cell_at(self.x - dir * 0.1, self.y - 1.0);
            let slope = level
                .slope_at(self.x + dir * 0.1)
                .is_some_and(|(s, _)| s >= self.y - 8.0);
            if ahead == Cell::Empty && behind.is_solid() && !slope {
                let edge = (self.x / TILE).round() * TILE;
                self.ant_start_wrap((edge, self.y));
                return;
            }
        } else {
            self.vx = self.vx.clamp(-ANT_WALK, ANT_WALK);
        }
        self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
        self.collide(level);
        if self.wall && self.on_ground {
            // a real wall (not the area edge) is climbed; anything else turns it round
            let wall = level
                .cell_at(self.x + dir * (h + 1.0), self.y + h)
                .is_solid();
            if wall {
                self.w3.down = self.w3.ahead;
                self.w3.ahead = 2;
                self.phase = ant::CRAWL;
                self.vx = 0.0;
                self.vy = 0.0;
            } else if self.turn_lock == 0 {
                self.turn();
            }
        }
    }

    fn ant_start_wrap(&mut self, corner: (f32, f32)) {
        self.w3.corner = corner;
        self.phase = ant::WRAP;
        self.timer = 0;
        self.vx = 0.0;
        self.vy = 0.0;
        self.on_ground = false;
    }

    /// Wrapping round a corner: the box centre swings a quarter circle round it, from over the
    /// old surface to beside the new one.
    fn ant_wrap(&mut self, h: f32) {
        let rate = if self.big { ANT_TURN_BIG } else { ANT_TURN };
        self.timer += 1;
        let a = (self.timer as f32 * rate).min(90.0).to_radians();
        let g = DIRS[self.w3.down as usize];
        let t = DIRS[self.w3.ahead as usize];
        let (kx, ky) = self.w3.corner;
        let cx = kx + h * (-g.0 * a.cos() + t.0 * a.sin());
        let cy = ky + h * (-g.1 * a.cos() + t.1 * a.sin());
        self.x = cx;
        self.y = cy - h;
        if self.timer as f32 * rate >= 90.0 {
            let (down, ahead) = (opposite(self.w3.ahead), self.w3.down);
            self.ant_settle(down, ahead);
        }
    }

    /// Now on the surface on side `down`, walking toward `ahead`.
    fn ant_settle(&mut self, down: u8, ahead: u8) {
        self.w3.down = down;
        self.w3.ahead = ahead;
        let t = DIRS[ahead as usize];
        if t.0 != 0.0 {
            self.dir = t.0 as i32;
        }
        if down == 0 {
            self.phase = ant::FLOOR;
            self.on_ground = true;
            self.vx = ANT_WALK * self.dir as f32;
        } else {
            self.phase = ant::CRAWL;
        }
        self.vy = 0.0;
    }

    /// Crawling along a wall or the ceiling: a solid cell ahead becomes the new surface; where
    /// the surface ends it wraps round; with nothing under it at all, it falls.
    fn ant_crawl(&mut self, level: &Level, h: f32) {
        let solid = |x: f32, y: f32| level.cell_at(x, y).is_solid();
        let g = DIRS[self.w3.down as usize];
        let t = DIRS[self.w3.ahead as usize];
        let mut cx = self.x + ANT_WALK * t.0;
        let mut cy = self.y + h + ANT_WALK * t.1;
        // the leading face: points across it, 1 px in from the surface side
        let (lx, ly) = (cx + (h + 0.01) * t.0, cy + (h + 0.01) * t.1);
        let blocked = [-(h - 1.0), 0.0, h - 1.0]
            .iter()
            .any(|&k| solid(lx + k * g.0, ly + k * g.1));
        if blocked {
            let snap = |v: f32, d: f32| {
                if d > 0.0 {
                    (v / TILE).floor() * TILE
                } else {
                    ((v / TILE).floor() + 1.0) * TILE
                }
            };
            if t.0 != 0.0 {
                cx = snap(lx, t.0) - h * t.0;
            } else {
                cy = snap(ly, t.1) - h * t.1;
            }
            self.x = cx;
            self.y = cy - h;
            let (down, ahead) = (self.w3.ahead, opposite(self.w3.down));
            self.ant_settle(down, ahead);
            return;
        }
        let probe = |dt: f32| {
            solid(
                cx + (h + 0.5) * g.0 + dt * t.0,
                cy + (h + 0.5) * g.1 + dt * t.1,
            )
        };
        self.x = cx;
        self.y = cy - h;
        if probe(0.1) {
            return;
        }
        if probe(-0.1) {
            // the surface ends here: the corner is on the cell edge nearest the centre
            let (mut kx, mut ky) = (cx + h * g.0, cy + h * g.1);
            if t.0 != 0.0 {
                kx = (cx / TILE).round() * TILE;
            } else {
                ky = (cy / TILE).round() * TILE;
            }
            self.ant_start_wrap((kx, ky));
        } else {
            // nothing to cling to: fall (InAir)
            self.phase = ant::FLOOR;
            self.on_ground = false;
            self.w3.down = 0;
        }
    }

    /// The side the Ant Trooper's surface is on (0 floor, 1 right wall, 2 ceiling, 3 left wall).
    pub fn ant_side(&self) -> u8 {
        if self.phase == ant::FLOOR {
            0
        } else {
            self.w3.down
        }
    }

    // ------------------------------------------------------------ Stingby

    /// The player's feet or head is within ±8·s of the Stingby's mid-height (s = 1, big 2).
    fn stingby_level(&self) -> bool {
        let s = self.size_k();
        let mid = self.y + 8.0 * s;
        let (_, py) = self.target;
        let head = py + self.w3.player_h;
        (py - mid).abs() <= STING_BAND * s || (head - mid).abs() <= STING_BAND * s
    }

    /// Stingby (65-stingby.md): flies with no gravity, patrolling ±16 px round its anchor at
    /// 0.2 px/f. A player within 96 px and level with it is charged: it turns to face them if
    /// needed, plays its alert, then flies at them at 0.5 px/f; it gives up after the player has
    /// been beyond 160 px or behind it for 100 frames. Walls turn it round.
    fn step_stingby(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = sting::FLY;
            self.w3.anchor = self.x;
            self.wings = false; // it already flies (action_wing Default) [obs]
        }
        let accel = if self.big {
            STING_ACCEL / 2.0
        } else {
            STING_ACCEL
        };
        let dx = self.target.0 - self.x;
        let facing = dx * self.dir as f32 >= 0.0;
        let target = match self.phase {
            sting::FLY => {
                if (self.x - self.w3.anchor) * self.dir as f32 > STING_SPAN {
                    self.dir = -self.dir;
                }
                if dx.abs() <= STING_SEE && self.stingby_level() {
                    if facing {
                        self.phase = sting::FIND;
                        self.timer = STING_ALERT;
                    } else {
                        self.phase = sting::TURN;
                        self.timer = STING_TURN;
                    }
                }
                STING_PATROL
            }
            sting::TURN => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.dir = -self.dir;
                    self.phase = sting::FIND;
                    self.timer = STING_ALERT;
                }
                0.0
            }
            sting::FIND => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = sting::CHASE;
                    self.w3.clock = STING_GIVE_UP;
                }
                0.0
            }
            _ => {
                if dx.abs() > STING_KEEP || !facing {
                    self.w3.clock -= 1;
                }
                if self.w3.clock <= 0 {
                    self.phase = sting::FLY;
                    self.w3.anchor = self.x;
                }
                STING_CHARGE
            }
        };
        self.vx = ease(self.vx, target * self.dir as f32, accel);
        self.vy = 0.0;
        self.collide(level);
        if self.wall {
            self.dir = -self.dir;
            self.vx = 0.0;
            if self.phase == sting::FLY {
                self.w3.anchor = self.x + STING_SPAN * self.dir as f32;
            }
        }
    }

    /// Stingby is charging (or about to).
    pub fn stingby_charging(&self) -> bool {
        matches!(self.phase, sting::FIND | sting::CHASE)
    }

    // ------------------------------------------------------------ Skipsqueak

    /// Skipsqueak (51-skipsqueak.md): keeps to its home spot (x and floor height). It jumps
    /// whenever the player is rising in a jump (crouch, take-off 2.7, gravity −0.20625), runs home
    /// when displaced (≤ 3.0 px/f, braking for the last v²/0.1 px), and jumps back up when below
    /// home (3.65 on the third try, then gives up). Running into a wall it bounces (Crash); more
    /// than 5 crashes and it gives up too. Winged: it hovers in place, bobbing.
    fn step_skipsqueak(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = skip::FALL;
            self.home_x = self.x;
        }
        if self.wings {
            self.w3.travel += 5.0; // 1/72 turn per frame
            self.y += SKIP_HOVER * self.w3.travel.to_radians().sin();
            self.vx = 0.0;
            self.dir = self.toward_player();
            return;
        }
        let g = if self.w3.liquid.is_some_and(|(s, w)| w && self.y + 8.0 < s) || self.w3.flooded {
            SKIP_GRAVITY_WATER
        } else {
            SKIP_GRAVITY
        };
        match self.phase {
            skip::WAIT => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                if !self.on_ground {
                    self.phase = skip::FALL;
                    return;
                }
                self.dir = self.toward_player();
                let away = (self.x - self.home_x).abs() > SKIP_HOME;
                let below = self.y < self.home_y - SKIP_HOME;
                if self.w3.gave_up {
                    self.home_x = self.x;
                    self.home_y = self.y;
                } else if away {
                    self.phase = skip::RETURN;
                    return;
                } else if below {
                    if self.w3.tries > SKIP_TRIES {
                        self.phase = skip::GIVE_UP;
                        self.w3.gave_up = true;
                        self.timer = 60;
                        return;
                    }
                    self.phase = skip::CROUCH;
                    self.timer = SKIP_CROUCH;
                    return;
                } else {
                    // at home: the floor it stands on is home (it follows a moving floor)
                    self.home_y = self.y;
                    self.w3.tries = 0;
                    self.w3.count = 0;
                }
                if self.w3.player_rising {
                    self.phase = skip::CROUCH;
                    self.timer = SKIP_CROUCH;
                }
            }
            skip::CROUCH => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    let recovering = self.y < self.home_y - SKIP_HOME && !self.w3.gave_up;
                    self.vy = if recovering && self.w3.tries >= SKIP_TRIES {
                        SKIP_JUMP_HIGH
                    } else {
                        SKIP_JUMP
                    };
                    if recovering {
                        self.w3.tries += 1;
                        // [inf] drifts toward home over the jump's airtime
                        let air = 2.0 * self.vy / -SKIP_GRAVITY;
                        self.vx =
                            ((self.home_x - self.x) / air).clamp(-SKIP_RECOVER_VX, SKIP_RECOVER_VX);
                    }
                    self.on_ground = false;
                    self.phase = skip::JUMP;
                }
            }
            skip::JUMP | skip::CRASH | skip::FALL => {
                let gravity = if self.phase == skip::FALL {
                    ENEMY_GRAVITY
                } else {
                    g
                };
                self.w3_fall(level, gravity);
                if self.on_ground && self.vy <= 0.0 {
                    if self.y >= self.home_y - SKIP_HOME {
                        self.w3.tries = 0;
                    }
                    self.vx = 0.0;
                    self.phase = skip::WAIT;
                }
            }
            skip::RETURN => {
                let d = self.home_x - self.x;
                let dir = d.signum();
                let v = self.vx * dir; // speed toward home
                let (target, accel) = if v > 0.0 && d.abs() <= v * v / SKIP_BRAKE_ACCEL {
                    (SKIP_CREEP, SKIP_BRAKE_ACCEL)
                } else {
                    (SKIP_RUN, SKIP_RUN_ACCEL)
                };
                self.vx = ease(self.vx, target * dir, accel);
                self.dir = dir as i32;
                if d.abs() <= self.vx.abs().max(SKIP_HOME) {
                    // arrived
                    self.x = self.home_x;
                    self.vx = 0.0;
                    self.phase = skip::WAIT;
                    self.w3_fall(level, ENEMY_GRAVITY);
                    return;
                }
                let before = self.vx;
                self.w3_fall(level, ENEMY_GRAVITY);
                if !self.on_ground {
                    self.phase = skip::FALL;
                } else if self.wall {
                    self.w3.count += 1;
                    if self.w3.count > SKIP_CRASHES {
                        self.phase = skip::GIVE_UP;
                        self.w3.gave_up = true;
                        self.timer = 60;
                        return;
                    }
                    // Crash: a bounce; after an earlier crash a higher wall jump [inf]
                    self.vy = if self.w3.count > 1 {
                        SKIP_JUMP_HIGH
                    } else {
                        SKIP_CRASH_VY
                    };
                    self.vx = before * SKIP_CRASH_KEEP;
                    self.on_ground = false;
                    self.phase = skip::CRASH;
                }
            }
            _ => {
                // GiveUp: a short sulk, then it stays where it is [inf]
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = skip::WAIT;
                }
            }
        }
    }

    fn w3_fall(&mut self, level: &Level, g: f32) {
        self.vy = (self.vy + g).max(MAX_FALL);
        self.collide(level);
    }

    // ------------------------------------------------------------ Hop-Chops

    /// The player is inside a box `reach` px ahead and ±`up` px vertically.
    fn ahead_box(&self, reach: f32, up: f32) -> bool {
        let (px, py) = self.target;
        let dx = (px - self.x) * self.dir as f32;
        (0.0..=reach).contains(&dx) && (py - self.y).abs() <= up
    }

    /// Hop-Chops (50-hop-chops.md): patrols in small hops (1.2 up, 0.5 forward, gravity −0.1),
    /// turning after 2–4 hops or at a wall. A player within 96 px ahead (±64) startles it (a
    /// 2.5 hop), then it chases in quicker hops (0.6, gravity −0.2), facing the player again on
    /// each landing (braking at 0.06 first when that turns it round). It gives up after the
    /// player has been behind it or outside 128 × ±80 px for 16 frames.
    fn step_hop_chops(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = hop::WALK;
            self.w3.tries = 2 + self.rand(3) as u8;
        }
        if self.wings {
            self.step_generic_wings(level);
            return;
        }
        match self.phase {
            hop::WALK => {
                if self.on_ground {
                    self.vx = 0.0;
                    if self.ahead_box(HOP_SEE.0, HOP_SEE.1) {
                        self.phase = hop::FIND;
                        self.vy = HOP_FIND_VY;
                        self.on_ground = false;
                        self.w3_fall(level, ENEMY_GRAVITY);
                        return;
                    }
                    self.timer += 1;
                    if self.timer >= HOP_WAIT {
                        self.timer = 0;
                        self.vy = HOP_VY;
                        self.vx = HOP_VX * self.dir as f32;
                        self.on_ground = false;
                    }
                }
                self.w3_fall(level, HOP_GRAVITY);
                if self.on_ground && self.vx != 0.0 {
                    // landed a hop
                    self.vx = 0.0;
                    self.w3.count += 1;
                }
                if self.on_ground && (self.wall || self.w3.count >= self.w3.tries) {
                    self.phase = hop::TURN;
                    self.timer = HOP_TURN_FRAMES;
                    self.w3.count = 0;
                }
            }
            hop::TURN => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.dir = -self.dir;
                    self.phase = hop::WALK;
                    self.timer = 0;
                    self.w3.tries = 2 + self.rand(3) as u8;
                }
            }
            hop::FIND => {
                self.dir = self.toward_player();
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                if self.on_ground {
                    self.phase = hop::RUN;
                    self.timer = 0;
                    self.w3.clock = 0;
                }
            }
            hop::RUN => {
                if self.ahead_box(HOP_KEEP.0, HOP_KEEP.1) {
                    self.w3.clock = 0;
                } else {
                    self.w3.clock += 1;
                    if self.w3.clock > HOP_LOST_GRACE {
                        self.phase = hop::LOST;
                        self.timer = HOP_LOST_FRAMES;
                    }
                }
                let mut g = ENEMY_GRAVITY;
                if self.on_ground {
                    let want = self.toward_player();
                    if want != self.dir && self.vx != 0.0 {
                        // turning round: brake to a stop first
                        self.vx = ease(self.vx, 0.0, HOP_TURN_DECEL);
                    } else {
                        self.dir = want;
                        self.vx = 0.0;
                        self.timer += 1;
                        if self.timer >= HOP_RUN_WAIT {
                            self.timer = 0;
                            self.vy = HOP_VY;
                            self.vx = HOP_RUN_VX * self.dir as f32;
                            self.on_ground = false;
                        }
                    }
                } else {
                    g = HOP_RUN_GRAVITY;
                }
                let vx = self.vx;
                self.w3_fall(level, g);
                if self.on_ground && !self.wall {
                    self.vx = vx; // keeps sliding into the turn-round brake
                } else if self.wall {
                    self.vx = 0.0;
                }
            }
            _ => {
                // Lost
                self.vx = ease(self.vx, 0.0, HOP_LOST_DECEL);
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = hop::WALK;
                    self.timer = 0;
                    self.w3.count = 0;
                }
            }
        }
    }

    /// Hop-Chops is chasing.
    pub fn hop_chops_running(&self) -> bool {
        matches!(self.phase, hop::FIND | hop::RUN)
    }

    // ------------------------------------------------------------ Porcupuffer

    /// Speed multiplier: by HP, ×1.25 winged [obs].
    fn puff_m(&self) -> f32 {
        PUFF_M[self.hops.min(PUFF_HP) as usize] * if self.wings { 1.25 } else { 1.0 }
    }

    /// Feet y when floating at the surface (its centre on the surface level: the liquid
    /// surface − 4 px, −16 in castle / forest themes, doubled when big); None without a surface.
    fn puff_float_y(&self, level: &Level) -> Option<f32> {
        if self.w3.flooded {
            return None;
        }
        let (surf, _) = self.w3.liquid?;
        let s = self.size_k();
        let off = if matches!(level.theme, 2 | 9) {
            -16.0
        } else {
            -4.0
        };
        Some(surf + off * s - 16.0 * s)
    }

    /// Porcupuffer (114-porcupuffer.md): 4 HP; every speed scales with the HP left. Under water
    /// it steers (≤ 2°/f) toward the player at 1.25 px/f and lunges at 2.0 when lined up and
    /// close (full HP only). When the player is out of the water it floats up and patrols the
    /// surface within 160 px of where it came up, leaping at a player just ahead and above it,
    /// and dives again when the player swims ≥ 16 px under it. Placed above the liquid it falls
    /// to the surface; with no liquid it just rests on the ground [inf].
    fn step_porcupuffer(&mut self, level: &Level) {
        let float_y = self.puff_float_y(level);
        if self.phase == 0 {
            self.hops = PUFF_HP;
            let under = float_y.is_some_and(|f| self.y < f - 32.0);
            self.phase = if self.w3.flooded || under {
                puff::SWIM
            } else {
                puff::FALL
            };
            self.heading = if self.dir > 0 { 0.0 } else { 180.0 };
        }
        if self.w3.clock > 0 {
            self.w3.clock -= 1;
        }
        let m = self.puff_m();
        let s = self.size_k();
        let (px, py) = self.target;
        match self.phase {
            puff::FALL => {
                self.vx = ease(self.vx, 0.0, 0.05);
                self.w3_fall(level, ENEMY_GRAVITY);
                if let Some(f) = float_y
                    && self.y <= f
                    && self.vy <= 0.0
                {
                    self.y = f;
                    self.vy = 0.0;
                    self.puff_surface();
                } else if self.on_ground {
                    self.dir = self.toward_player();
                }
            }
            puff::SWIM | puff::SWIM_TURN | puff::LUNGE => {
                self.puff_swim(level, float_y, m, s);
            }
            puff::FLOAT => {
                self.vx = ease(self.vx, 0.0, PUFF_FLOAT_ACCEL);
                self.vy = ease(self.vy, PUFF_FLOAT * m, PUFF_FLOAT_ACCEL);
                self.collide(level);
                match float_y {
                    Some(f) if self.y >= f => {
                        self.y = f;
                        self.vy = 0.0;
                        self.puff_surface();
                    }
                    None => self.phase = puff::SWIM,
                    _ => {}
                }
            }
            puff::SURFACE => {
                let Some(f) = float_y else {
                    self.phase = puff::SWIM;
                    return;
                };
                let out = (self.x - self.w3.anchor).abs() > PUFF_BAND;
                if out && (self.x - self.w3.anchor) * self.dir as f32 > 0.0 {
                    self.dir = -self.dir;
                }
                let speed = if out { 2.0 * PUFF_SWIM } else { PUFF_SWIM };
                self.vx = ease(self.vx, speed * m * self.dir as f32, PUFF_FLOAT_ACCEL * m);
                self.vy = (f - self.y).clamp(-PUFF_EASE, PUFF_EASE);
                self.collide(level);
                if self.wall {
                    self.vx *= -0.5;
                    self.dir = -self.dir;
                }
                // a leap at a player just ahead and above; a dive after one under water
                let (hw, hh) = if self.wings {
                    (48.0, 64.0)
                } else {
                    (28.0, 40.0)
                };
                let wx = self.x + self.dir as f32 * (16.0 * s + 28.0);
                let wy = self.y + 16.0 * s + hh;
                if !out && (px - wx).abs() <= hw && (py - wy).abs() <= hh {
                    self.phase = puff::SURFACE_JUMP;
                    self.timer = 0;
                } else if self.w3.player_wet
                    && f + 16.0 * s - py >= PUFF_DIVE_DEPTH
                    && self.w3.clock == 0
                {
                    self.phase = puff::SWIM;
                    self.timer = 0;
                    self.heading = -90.0;
                }
            }
            puff::SURFACE_JUMP => {
                self.timer += 1;
                self.vx = 0.9375 * m * self.dir as f32;
                if self.timer <= PUFF_JUMP_WINDUP {
                    self.vy -= 0.0375 * m;
                } else if self.w3.count == 0 {
                    self.vy += PUFF_JUMP_RAMP;
                    if self.vy >= PUFF_JUMP * m {
                        self.vy = PUFF_JUMP * m;
                        self.w3.count = 1;
                    }
                } else {
                    let g = if self.wings && self.vy < -0.5 {
                        -0.15
                    } else {
                        ENEMY_GRAVITY
                    };
                    self.vy = (self.vy + g).max(MAX_FALL);
                }
                self.collide(level);
                if self.w3.count == 1 && self.vy < 0.0 {
                    match float_y {
                        Some(f) if self.y <= f => {
                            self.y = f;
                            self.vy = 0.0;
                            self.w3.count = 0;
                            self.puff_surface();
                        }
                        _ if self.on_ground => {
                            self.w3.count = 0;
                            self.phase = puff::FALL;
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                // Recover: the hurt animation, slowing down; then back to what it was doing
                self.speed = ease(self.speed, 0.0, 0.05);
                self.vx = ease(self.vx, 0.0, 0.05);
                if float_y.is_none_or(|f| self.y < f) && self.w3.liquid.is_some() {
                    self.vy = ease(self.vy, 0.0, 0.05);
                } else {
                    self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                }
                self.collide(level);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = self.w3.resume.max(puff::FALL);
                    self.timer = 0;
                }
            }
        }
    }

    fn puff_surface(&mut self) {
        self.phase = puff::SURFACE;
        self.w3.anchor = self.x;
        self.w3.clock = PUFF_COOLDOWN;
    }

    fn puff_swim(&mut self, level: &Level, float_y: Option<f32>, m: f32, s: f32) {
        let (px, py) = self.target;
        let (cx, cy) = (self.x, self.y + 16.0 * s);
        let (ax, ay) = (px, py + self.w3.player_h / 2.0 - 16.0 * s);
        let want = (ay - cy).atan2(ax - cx).to_degrees();
        let diff = (want - self.heading + 540.0).rem_euclid(360.0) - 180.0;
        match self.phase {
            puff::LUNGE => {
                self.timer += 1;
                if self.timer <= PUFF_LUNGE_STOP {
                    self.speed = ease(self.speed, 0.0, 0.75);
                } else {
                    self.speed = PUFF_LUNGE * m;
                }
                if self.timer >= PUFF_LUNGE_STOP + PUFF_LUNGE_FRAMES {
                    self.phase = puff::SWIM;
                    self.timer = 0;
                }
            }
            puff::SWIM_TURN => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = puff::SWIM;
                    self.w3.clock = PUFF_TURN_COOLDOWN;
                }
            }
            _ => {
                self.heading += diff.clamp(-PUFF_STEER, PUFF_STEER);
                self.speed = ease(self.speed, PUFF_SWIM * m, PUFF_SWIM_ACCEL * m);
                let dist = ((ax - cx).powi(2) + (ay - cy).powi(2)).sqrt();
                let near = if self.big { 80.0 } else { PUFF_LUNGE_NEAR };
                let behind = (px - self.x) * (self.dir as f32) < 0.0;
                if diff.abs() <= PUFF_LINE_UP && self.hops == PUFF_HP && dist < near {
                    self.phase = puff::LUNGE;
                    self.timer = 0;
                } else if behind && self.w3.clock == 0 {
                    // SwimTurn: face the other way, mirroring the heading
                    self.dir = -self.dir;
                    self.heading = 180.0 - self.heading;
                    self.phase = puff::SWIM_TURN;
                    self.timer = PUFF_TURN_FRAMES;
                }
                // the player left the water: float up after 60 frames
                if self.w3.player_wet || float_y.is_none() {
                    self.timer = 0;
                } else {
                    self.timer += 1;
                    if self.timer >= PUFF_COOLDOWN {
                        self.phase = puff::FLOAT;
                        self.timer = 0;
                        self.vx = self.speed * self.heading.to_radians().cos();
                        self.vy = self.speed * self.heading.to_radians().sin();
                        return;
                    }
                }
            }
        }
        let a = self.heading.to_radians();
        self.vx = self.speed * a.cos();
        self.vy = self.speed * a.sin();
        if self.vx.abs() > 0.05 {
            self.dir = self.vx.signum() as i32;
        }
        self.collide(level);
        // under water it stays below its floating level
        if let Some(f) = float_y
            && self.y > f
        {
            self.y = f;
        }
    }

    /// A fireball (or arrow) hit: one HP. Returns true when it was the last.
    pub fn porcupuffer_hit(&mut self, from_x: f32) -> bool {
        if self.phase == 0 {
            self.hops = PUFF_HP;
        }
        self.hops = self.hops.saturating_sub(1);
        if self.hops == 0 {
            self.die_fall(from_x);
            return true;
        }
        if self.phase != puff::RECOVER {
            self.w3.resume = self.phase;
        }
        self.phase = puff::RECOVER;
        self.timer = PUFF_RECOVER;
        self.kick_lock = PUFF_RECOVER;
        false
    }

    /// Hit points left (4 before the first step).
    pub fn porcupuffer_hp(&self) -> u8 {
        if self.phase == 0 { PUFF_HP } else { self.hops }
    }

    // ------------------------------------------------------------ Bully

    /// Bully (117-bully.md): waits 40 frames, walks at 0.3 for 60–179 frames (turning at walls
    /// and ledges [inf]), and charges a player it sees at 0.8 (accel 0.025) until they are more
    /// than 256 px away. Hits only push it back (1.0 / 1.75 / 2.5 px/f decaying by 0.05/f over
    /// 60 frames; a heavy push leaves it stunned for 120). Winged: chases through the air for
    /// 300 frames (≤ 0.5 px/f, steering 0.075), then flies home [inf: FlyLand not modelled].
    fn step_bully(&mut self, level: &Level) {
        if self.phase == 0 {
            if self.wings {
                self.phase = bully::FLY_CHASE;
                self.timer = BULLY_FLY_CHASE;
                self.vy = 0.5;
            } else {
                self.phase = bully::WAIT;
                self.timer = BULLY_WAIT;
            }
        }
        let (px, py) = self.target;
        let dx = px - self.x;
        let sees = dx.abs() <= BULLY_SEE.0 && (py - self.y).abs() <= BULLY_SEE.1;
        let flying = self.wings;
        match self.phase {
            bully::WAIT => {
                self.vx = ease(self.vx, 0.0, BULLY_ACCEL);
                self.timer -= 1;
                if sees {
                    self.phase = bully::FIND;
                    self.timer = BULLY_FIND;
                } else if self.timer <= 0 {
                    self.phase = bully::WALK;
                    self.timer = 60 + self.rand(120) as i32;
                    if self.rand(2) == 0 {
                        self.dir = -self.dir;
                    }
                }
            }
            bully::WALK => {
                self.vx = ease(self.vx, BULLY_WALK * self.dir as f32, BULLY_ACCEL);
                let probe = self.x + self.dir as f32 * (self.half_w() + 0.1);
                let slope = level
                    .slope_at(probe)
                    .is_some_and(|(s, _)| s >= self.y - 8.0);
                if self.on_ground
                    && level.cell_at(probe, self.y - 1.0) == Cell::Empty
                    && !slope
                    && self.turn_lock == 0
                {
                    self.turn();
                }
                self.timer -= 1;
                if sees {
                    self.phase = bully::FIND;
                    self.timer = BULLY_FIND;
                } else if self.timer <= 0 {
                    self.phase = bully::WAIT;
                    self.timer = BULLY_WAIT;
                }
            }
            bully::FIND => {
                self.dir = self.toward_player();
                self.vx = ease(self.vx, 0.0, BULLY_ACCEL);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = bully::CHASE;
                }
            }
            bully::CHASE => {
                self.dir = self.toward_player();
                self.vx = ease(self.vx, BULLY_CHASE * self.dir as f32, BULLY_ACCEL);
                if dx.abs() > BULLY_GIVE_UP {
                    self.phase = bully::WAIT;
                    self.timer = BULLY_WAIT;
                }
            }
            bully::FLIP | bully::HOLD_OUT => {
                self.vx = ease(self.vx, 0.0, BULLY_PUSH_DECAY);
                if flying {
                    self.vy = ease(self.vy, 0.0, BULLY_PUSH_DECAY);
                }
                self.timer -= 1;
                if self.timer <= 0 {
                    if self.phase == bully::FLIP && self.w3.count as usize == push::HEAVY {
                        self.phase = bully::HOLD_OUT;
                        self.timer = BULLY_HOLD_OUT;
                    } else if flying {
                        self.phase = bully::FLY_RETURN;
                    } else {
                        self.phase = bully::WAIT;
                        self.timer = BULLY_WAIT;
                    }
                }
            }
            bully::FLY_CHASE => {
                let dy = py + 8.0 - (self.y + 16.0 * self.size_k());
                self.vx = ease(self.vx, BULLY_FLY_SPEED * dx.signum(), BULLY_FLY_ACCEL);
                self.vy = ease(self.vy, BULLY_FLY_SPEED * dy.signum(), BULLY_FLY_ACCEL);
                self.dir = self.toward_player();
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = bully::FLY_RETURN;
                }
            }
            _ => {
                // FlyReturn: back to the spawn point, then chase again once the player is near
                let (hx, hy) = (self.home_x - self.x, self.home_y - self.y);
                let d = (hx * hx + hy * hy).sqrt();
                if d > 1.0 {
                    let v = BULLY_FLY_SPEED.min(d);
                    self.vx = v * hx / d;
                    self.vy = v * hy / d;
                } else {
                    self.vx = 0.0;
                    self.vy = 0.0;
                    if dx.abs() <= BULLY_GIVE_UP {
                        self.phase = bully::FLY_CHASE;
                        self.timer = BULLY_FLY_CHASE;
                    }
                }
            }
        }
        if flying {
            self.collide(level);
        } else {
            self.w3_fall(level, ENEMY_GRAVITY);
            if self.wall && matches!(self.phase, bully::WALK) && self.turn_lock == 0 {
                self.turn();
            }
        }
    }

    /// A hit that only pushes a Bully back, away from `from_x`, at strength `strength` (see
    /// `push`).
    pub fn bully_push(&mut self, strength: usize, from_x: f32) {
        let away = if self.x < from_x { -1.0 } else { 1.0 };
        self.vx = BULLY_PUSH[strength.min(2)] * away;
        self.phase = bully::FLIP;
        self.timer = BULLY_FLIP;
        self.w3.count = strength as u8;
    }

    /// Bully is charging at the player.
    pub fn bully_charging(&self) -> bool {
        matches!(self.phase, bully::CHASE | bully::FLY_CHASE)
    }

    /// Bully is being pushed back or stunned.
    pub fn bully_pushed(&self) -> bool {
        matches!(self.phase, bully::FLIP | bully::HOLD_OUT)
    }

    // ------------------------------------------------------------ Charvaargh

    /// Feet y of the head at rest, just under the lava (or under the area bottom).
    fn charvaargh_rest(&self, level: &Level) -> f32 {
        let surf = match self.w3.liquid {
            Some((y, _)) if !self.w3.flooded => y,
            _ => level.bottom + 8.0,
        };
        surf - CHAR_DEPTH - 64.0 * self.size_k()
    }

    /// Charvaargh (86-charvaargh.md): waits under the lava; a player within ±128 px (and −88…
    /// +160 px vertically) gets a warning (the head rises 24 px for 30 frames), then it leaps
    /// out toward them in a half circle and dives back, its 8 body segments trailing 12 px
    /// apart. 30 frames' lockout before the next leap. Nothing defeats it [inf].
    fn step_charvaargh(&mut self, level: &Level) {
        let rest = self.charvaargh_rest(level);
        let s = self.size_k();
        if self.phase == 0 {
            self.phase = chr::FALL;
        }
        let (px, py) = self.target;
        match self.phase {
            chr::FALL => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.y += self.vy;
                if self.y <= rest {
                    self.y = rest;
                    self.vy = 0.0;
                    self.phase = chr::WAIT;
                    self.timer = CHAR_LOCKOUT;
                }
            }
            chr::WAIT => {
                self.y += (rest - self.y).clamp(-1.0, 1.0);
                self.timer -= 1;
                let head_y = self.y + 32.0 * s;
                let dy = py - head_y;
                if self.timer <= 0
                    && (px - self.x).abs() <= CHAR_SEE_X
                    && (CHAR_SEE_Y.0..=CHAR_SEE_Y.1).contains(&dy)
                {
                    self.phase = chr::SIGN;
                    self.timer = CHAR_SIGN;
                    self.dir = self.toward_player();
                }
            }
            chr::SIGN => {
                self.y = ease(self.y, rest + CHAR_SIGN_RISE, 2.0);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = chr::ATTACK;
                    self.w3.count = 0;
                    self.heading = 90.0;
                    self.trail.clear();
                }
            }
            _ => self.charvaargh_leap(rest, s),
        }
    }

    fn charvaargh_leap(&mut self, rest: f32, s: f32) {
        let v = CHAR_SPEED * s;
        let surface = rest + CHAR_DEPTH + 64.0 * s;
        let centre_y = self.y + 32.0 * s;
        match self.w3.count {
            0 => {
                // rise straight up until the head's centre clears the surface
                self.heading = 90.0;
                if centre_y >= surface {
                    self.w3.count = 1;
                }
            }
            1 => {
                // the half circle toward the facing side: heading 90° → ∓90°
                let w = (v / (CHAR_RADIUS * s)).to_degrees();
                self.heading -= w * self.dir as f32;
                let turned = (90.0 - self.heading) * self.dir as f32;
                if turned >= 180.0 {
                    self.heading = -90.0;
                    self.w3.count = 2;
                }
            }
            _ => {
                self.heading = -90.0;
                if self.y <= rest {
                    self.y = rest;
                    self.phase = chr::WAIT;
                    self.timer = CHAR_LOCKOUT;
                    self.trail.clear();
                    return;
                }
            }
        }
        let a = self.heading.to_radians();
        self.vx = v * a.cos();
        self.vy = v * a.sin();
        self.x += self.vx;
        self.y += self.vy;
        self.trail.push((self.x, self.y + 32.0 * s));
        let keep = (CHAR_SEGMENTS as f32 * CHAR_SEGMENT_GAP / v).ceil() as usize + 2;
        if self.trail.len() > keep {
            let n = self.trail.len() - keep;
            self.trail.drain(..n);
        }
    }

    /// Centres of the body segments trailing the head while it leaps (empty at rest).
    pub fn charvaargh_segments(&self) -> Vec<(f32, f32)> {
        if self.phase != chr::ATTACK || self.trail.is_empty() {
            return Vec::new();
        }
        let v = CHAR_SPEED * self.size_k();
        let step = (CHAR_SEGMENT_GAP / v).max(1.0);
        (1..=CHAR_SEGMENTS)
            .filter_map(|k| {
                let back = (k as f32 * step).round() as usize;
                let i = self.trail.len().checked_sub(1 + back)?;
                Some(self.trail[i])
            })
            .collect()
    }

    /// Charvaargh is leaping (or warning).
    pub fn charvaargh_attacking(&self) -> bool {
        matches!(self.phase, chr::SIGN | chr::ATTACK)
    }

    // ------------------------------------------------------------ Piranha Creeper

    /// Its path's node codes (empty without a path).
    fn creeper_codes<'a>(&self, level: &'a Level, path: u8) -> &'a [u8] {
        level
            .creepers
            .iter()
            .find(|(i, _)| *i == path)
            .map(|(_, c)| c.as_slice())
            .unwrap_or(&[])
    }

    /// Centre of the head at `travel` px along the path, from the base cell at (`home_x`,
    /// `home_y` + 16).
    pub fn creeper_point(&self, codes: &[u8], travel: f32) -> (f32, f32) {
        let (mut x, mut y) = (self.home_x, self.home_y + CREEP_CELL / 2.0);
        let mut left = travel;
        for &c in codes.iter().take(codes.len().saturating_sub(1)) {
            let (dx, dy) = DIRS[creeper_out(c) as usize];
            let step = left.min(CREEP_CELL);
            x += dx * step;
            y += dy * step;
            left -= step;
            if left <= 0.0 {
                break;
            }
        }
        (x, y)
    }

    /// Cells of the stalk behind the head (centre and node code), base first.
    pub fn creeper_stalk(&self, codes: &[u8]) -> Vec<(f32, f32, u8)> {
        let reached = (self.w3.travel / CREEP_CELL).floor() as usize;
        (0..codes.len().min(reached + 1))
            .filter(|&k| (k as f32) * CREEP_CELL < self.w3.travel - CREEP_CELL / 2.0)
            .map(|k| {
                let (x, y) = self.creeper_point(codes, k as f32 * CREEP_CELL);
                (x, y, codes[k])
            })
            .collect()
    }

    /// Piranha Creeper (107-piranha-creeper.md): the head creeps along its path (one 2×2 cell per
    /// node) at 0.6 px/f, easing by 0.1/f and braking near the ends (10 % of the gap, ≥ 0.05);
    /// the stalk grows behind it. At the end it waits and creeps back (the alternate stays out
    /// and sleeps). A stomp tramples the head for 130 frames, then it carries on [inf].
    fn step_creeper(&mut self, level: &Level, path: u8, sleeps: bool) {
        let codes = self.creeper_codes(level, path);
        let len = CREEP_CELL * codes.len().saturating_sub(1) as f32;
        if self.phase == 0 {
            self.phase = creep::WAIT;
            self.timer = CREEP_WAIT;
            self.w3.resume = creep::MOVE;
        }
        match self.phase {
            creep::MOVE | creep::MOVE_BACK => {
                let forward = self.phase == creep::MOVE;
                let gap = if forward {
                    len - self.w3.travel
                } else {
                    self.w3.travel
                };
                let cruise = ease(self.speed, CREEP_SPEED, CREEP_ACCEL);
                self.speed = cruise.min((gap * CREEP_BRAKE).max(CREEP_CRAWL));
                let step = self.speed.min(gap);
                self.w3.travel += if forward { step } else { -step };
                if gap - step <= 0.0 {
                    self.speed = 0.0;
                    if forward && sleeps {
                        self.phase = creep::SLEEP;
                    } else {
                        self.w3.resume = if forward {
                            creep::MOVE_BACK
                        } else {
                            creep::MOVE
                        };
                        self.phase = creep::WAIT;
                        self.timer = CREEP_WAIT;
                    }
                }
            }
            creep::WAIT => {
                self.timer -= 1;
                if self.timer <= 0 && len > 0.0 {
                    self.phase = self.w3.resume;
                }
            }
            creep::DAMAGE | creep::DAMAGE_WAIT => {
                self.timer -= 1;
                if self.timer <= 0 {
                    if self.phase == creep::DAMAGE {
                        self.phase = creep::DAMAGE_WAIT;
                        self.timer = CREEP_DAMAGE_WAIT;
                    } else {
                        self.phase = self.w3.resume;
                        self.timer = CREEP_WAIT;
                    }
                }
            }
            _ => {} // Sleep
        }
        let (hx, hy) = self.creeper_point(codes, self.w3.travel);
        self.x = hx;
        self.y = hy - CREEP_HEAD;
        // it faces along the path where the head is
        let k = ((self.w3.travel / CREEP_CELL) as usize).min(codes.len().saturating_sub(1));
        if let Some(&c) = codes.get(k) {
            self.heading = match creeper_out(c) {
                0 => -90.0,
                1 => 0.0,
                2 => 90.0,
                _ => 180.0,
            };
            if c == 1 || c == 14 || c == 6 || c == 8 {
                self.dir = 1;
            } else if matches!(c, 2 | 13 | 10 | 12) {
                self.dir = -1;
            }
        }
    }

    /// A stomp tramples the head (Damage → DamageWait); no effect while already trampled.
    /// Returns true if it took the stomp.
    pub fn creeper_trample(&mut self) -> bool {
        if self.creeper_trampled() {
            return false;
        }
        if !matches!(self.phase, creep::DAMAGE | creep::DAMAGE_WAIT) {
            self.w3.resume = match self.phase {
                creep::MOVE | creep::MOVE_BACK | creep::SLEEP => self.phase,
                0 => creep::MOVE,
                _ => self.w3.resume,
            };
        }
        self.phase = creep::DAMAGE;
        self.timer = CREEP_TRAMPLED;
        self.speed = 0.0;
        true
    }

    /// The head is trampled (harmless, flat).
    pub fn creeper_trampled(&self) -> bool {
        matches!(self.phase, creep::DAMAGE | creep::DAMAGE_WAIT)
    }

    /// The creeper is asleep at the end of its path (alternate).
    pub fn creeper_asleep(&self) -> bool {
        self.phase == creep::SLEEP
    }

    // ------------------------------------------------------------ hits and looks

    /// A block bumped from below under it: true if that knocks it off (Bully: DieFall [obs];
    /// the small walkers like a Goomba [inf]).
    pub fn w3_block_hit(&self) -> bool {
        matches!(
            self.kind,
            Kind::AntTrooper { .. }
                | Kind::Skipsqueak { .. }
                | Kind::HopChops
                | Kind::Bully
                | Kind::Porcupuffer
        )
    }

    /// Model and animation frame. The 3D World models (`3W_Enemy_…`) are reached through the
    /// `M1_` name like every other enemy; frame stems name the model's animations.
    pub fn w3_look(&self) -> Option<W3Look> {
        let flash = self.kick_lock > 0 && (self.kick_lock / 2) % 2 == 0;
        if flash && self.kind == Kind::Porcupuffer {
            return None;
        }
        let face = self.dir > 0;
        let stomped = self.mode == super::Mode::Stomped;
        Some(match self.kind {
            Kind::AntTrooper { horned } => (
                if horned {
                    "M1_Enemy_ariheitsuno"
                } else {
                    "M1_Enemy_arihei"
                },
                if stomped {
                    "trampled.0"
                } else if self.wings {
                    "waitfly.0"
                } else {
                    "walk.0"
                },
                16.0,
                if horned { 24.0 } else { 16.0 },
                face,
            ),
            Kind::Stingby => (
                "M1_Enemy_hacchin",
                if stomped {
                    "pressdown.0"
                } else if self.phase == sting::FIND {
                    "find.0"
                } else {
                    "walk.0"
                },
                16.0,
                16.0,
                face,
            ),
            Kind::Skipsqueak { spiny } => (
                if spiny {
                    "M1_Enemy_pyonchu_hari"
                } else {
                    "M1_Enemy_pyonchu"
                },
                match self.phase {
                    _ if stomped => "pressdown.0",
                    skip::CROUCH => "jumpsign.0",
                    skip::JUMP | skip::CRASH => "jump.0",
                    skip::FALL => "fall.0",
                    skip::RETURN => "run.0",
                    skip::GIVE_UP => "giveup.0",
                    _ => "wait.0",
                },
                16.0,
                16.0,
                face,
            ),
            Kind::HopChops => (
                "M1_Enemy_hopper",
                match self.phase {
                    _ if stomped => "blowdown.0",
                    hop::TURN => "turn.0",
                    hop::FIND => "find.0",
                    hop::RUN => "run.0",
                    hop::LOST => "giveup.0",
                    _ if self.wings => "waitfly.0",
                    _ if !self.on_ground => "jump.0",
                    _ => "walk.0",
                },
                32.0,
                16.0,
                face,
            ),
            Kind::Porcupuffer => (
                "M1_Enemy_fugumannen",
                match self.phase {
                    puff::RECOVER => "damage.0",
                    puff::LUNGE => "attack.0",
                    puff::SWIM_TURN => "turn.0",
                    puff::FLOAT => "floatup.0",
                    puff::SURFACE_JUMP => "jump.0",
                    puff::SURFACE => "swimfloat.0",
                    puff::FALL => "wait.0",
                    _ => "swim.0",
                },
                32.0,
                32.0,
                face,
            ),
            Kind::Bully => (
                "M1_Enemy_donketsu",
                match self.phase {
                    bully::FLIP => "pushback.0",
                    bully::HOLD_OUT => "stayback.0",
                    bully::FIND => "find.0",
                    bully::CHASE => "run.0",
                    bully::WALK => "walk.0",
                    bully::FLY_CHASE | bully::FLY_RETURN => "waitfly.0",
                    _ if !self.on_ground => "fall.0",
                    _ => "wait.0",
                },
                32.0,
                32.0,
                face,
            ),
            Kind::Charvaargh => (
                "M1_Enemy_magmafish",
                match (self.phase, self.w3.count) {
                    (chr::SIGN, _) if face => "attacksidesignright.0",
                    (chr::SIGN, _) => "attacksidesignleft.0",
                    (chr::ATTACK, 0) => "attacksidestart.0",
                    (chr::ATTACK, 1) => "attackside.0",
                    (chr::ATTACK, _) => "attacksideend.0",
                    _ => "fall.0",
                },
                64.0,
                64.0,
                face,
            ),
            Kind::PiranhaCreeper { .. } => (
                "M1_Enemy_packunpipe_head",
                match self.phase {
                    creep::DAMAGE => "trampled.0",
                    creep::DAMAGE_WAIT => "trampledloop.0",
                    creep::SLEEP => "sleep.0",
                    _ if self.mode == super::Mode::DieFall => "down.0",
                    _ => "move.0",
                },
                2.0 * CREEP_HEAD,
                2.0 * CREEP_HEAD,
                face,
            ),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    fn on_flat(kind: Kind, x: f32) -> (Level, Actor) {
        let lvl = Level::flat(40);
        let mut a = Actor::new(kind, x, 32.0);
        a.dir = 1;
        a.on_ground = true;
        (lvl, a)
    }

    fn run(a: &mut Actor, lvl: &Level, frames: usize) {
        for _ in 0..frames {
            a.step_spawning(lvl);
        }
    }

    #[test]
    fn creeper_node_codes_lead_the_right_way() {
        // straight, corners (in → out) and end caps
        assert_eq!(creeper_out(1), 1);
        assert_eq!(creeper_out(4), 2);
        assert_eq!(creeper_out(12), 3); // up → left
        assert_eq!(creeper_out(5), 0); // right → down
        assert_eq!(creeper_out(16), 0);
        assert!(creeper_corner(9) && !creeper_corner(13));
    }

    #[test]
    fn ant_trooper_climbs_a_wall_and_wraps_over_its_top() {
        let (mut lvl, mut a) = on_flat(Kind::AntTrooper { horned: false }, 100.0);
        // a 2-tile wall at tiles x 10, rows 2–3 (x 160–176, top y 64)
        lvl.set(10, 2, Cell::Solid);
        lvl.set(10, 3, Cell::Solid);
        let mut sides = Vec::new();
        for _ in 0..400 {
            a.step_spawning(&lvl);
            if sides.last() != Some(&a.ant_side()) {
                sides.push(a.ant_side());
            }
        }
        // floor → the wall on its right → on top of the block → down its far side → floor
        assert_eq!(&sides[..4], &[0, 1, 0, 3], "sides {sides:?}");
        assert!(a.x > 176.0, "went over the wall: x {}", a.x);
        assert!((a.y - 32.0).abs() < 0.5, "back on the floor: y {}", a.y);
    }

    #[test]
    fn ant_trooper_wraps_under_a_ledge_at_6_degrees_a_frame() {
        // a floating platform (tiles 4–9, row 6): walking right off its end wraps under it
        let mut lvl = Level::flat(40);
        for x in 4..10 {
            lvl.set(x, 6, Cell::Solid);
        }
        let mut a = Actor::new(Kind::AntTrooper { horned: false }, 120.0, 112.0);
        a.dir = 1;
        a.on_ground = true;
        let mut wrap_frames = 0;
        let mut ceiling = false;
        for _ in 0..200 {
            a.step_spawning(&lvl);
            if a.phase == ant::WRAP {
                wrap_frames += 1;
            }
            ceiling |= a.ant_side() == 2;
        }
        // two corners (top → right face, right face → underside), 15 frames each
        assert_eq!(wrap_frames, 30);
        assert!(ceiling, "crawled onto the underside");
        assert!(a.y < 96.0 && a.y > 70.0, "under the platform: y {}", a.y);
    }

    #[test]
    fn stingby_patrols_32_px_then_charges_a_level_player() {
        let (lvl, mut a) = on_flat(Kind::Stingby, 200.0);
        a.y = 64.0;
        a.target = (600.0, 0.0); // far away and below
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for _ in 0..1200 {
            a.step_spawning(&lvl);
            lo = lo.min(a.x);
            hi = hi.max(a.x);
        }
        assert!(hi - lo <= 34.0 && hi - lo >= 30.0, "span {}", hi - lo);
        assert!(a.vx.abs() <= STING_PATROL + 1e-4);
        // a player 80 px ahead at the same height: alert, then the charge
        a.target = (a.x + 80.0 * a.dir as f32, a.y);
        a.w3.player_h = 16.0;
        run(&mut a, &lvl, 1);
        assert!(a.stingby_charging());
        run(&mut a, &lvl, STING_ALERT as usize + 40);
        assert!((a.vx.abs() - STING_CHARGE).abs() < 1e-4, "vx {}", a.vx);
    }

    #[test]
    fn stingby_gives_up_after_100_frames_out_of_range() {
        let (lvl, mut a) = on_flat(Kind::Stingby, 200.0);
        a.y = 64.0;
        a.w3.player_h = 16.0;
        a.target = (260.0, 64.0);
        run(&mut a, &lvl, STING_ALERT as usize + 5);
        assert_eq!(a.phase, sting::CHASE);
        a.target = (a.x - 400.0, 64.0); // far behind
        run(&mut a, &lvl, 99);
        assert_eq!(a.phase, sting::CHASE);
        run(&mut a, &lvl, 2);
        assert_eq!(a.phase, sting::FLY);
    }

    #[test]
    fn skipsqueak_jumps_with_the_player_and_runs_home() {
        let (lvl, mut a) = on_flat(Kind::Skipsqueak { spiny: false }, 200.0);
        a.target = (150.0, 32.0);
        run(&mut a, &lvl, 5);
        assert_eq!(a.phase, skip::WAIT);
        // the player jumps: crouch, then a 2.7 take-off (apex ≈ 17.7 px)
        a.w3.player_rising = true;
        let mut top: f32 = 0.0;
        for _ in 0..60 {
            a.step_spawning(&lvl);
            a.w3.player_rising = false;
            top = top.max(a.y - 32.0);
        }
        assert!((top - 17.7).abs() < 1.5, "apex {top}");
        assert_eq!(a.phase, skip::WAIT);
        // pushed 40 px away it runs back and stops on its spot
        a.x = 240.0;
        run(&mut a, &lvl, 300);
        assert!((a.x - 200.0).abs() <= 1.0, "home x {}", a.x);
        assert_eq!(a.phase, skip::WAIT);
    }

    #[test]
    fn skipsqueak_off_its_ledge_tries_to_get_back_then_gives_up() {
        // home on a 3-tile-high pillar it cannot jump back onto from the floor
        let mut lvl = Level::flat(40);
        for y in 2..6 {
            lvl.set(12, y, Cell::Solid);
        }
        let mut a = Actor::new(Kind::Skipsqueak { spiny: false }, 200.0, 96.0);
        a.target = (100.0, 32.0);
        run(&mut a, &lvl, 10);
        assert!((a.home_y - 96.0).abs() < 0.01);
        // knocked to the floor beside the pillar
        a.x = 230.0;
        a.y = 32.0;
        a.home_x = 230.0; // displaced straight down: only the height is wrong
        let mut jumps = 0;
        let mut was_air = false;
        for _ in 0..600 {
            a.step_spawning(&lvl);
            let air = a.phase == skip::JUMP;
            if air && !was_air {
                jumps += 1;
            }
            was_air = air;
        }
        assert_eq!(jumps, 3, "three recovery jumps");
        assert!(a.w3.gave_up);
    }

    #[test]
    fn hop_chops_hops_then_turns_and_chases_a_player_ahead() {
        let (lvl, mut a) = on_flat(Kind::HopChops, 300.0);
        a.target = (0.0, 300.0); // out of sight
        let x0 = a.x;
        let mut turned = false;
        for _ in 0..600 {
            a.step_spawning(&lvl);
            turned |= a.dir < 0;
        }
        assert!(turned, "turned after its hops");
        assert!((a.x - x0).abs() < 80.0, "patrols near home");
        // a player 64 px ahead (it looks while on the ground): startled hop, then the chase
        while !(a.on_ground && a.phase == hop::WALK) {
            a.step_spawning(&lvl);
        }
        a.target = (a.x + 64.0 * a.dir as f32, 32.0);
        run(&mut a, &lvl, 1);
        assert!(a.hop_chops_running());
        assert!(a.vy > 2.0, "startled hop {}", a.vy);
        let mut top: f32 = 0.0;
        for _ in 0..120 {
            a.target = (a.x + 64.0 * a.dir as f32, 32.0);
            a.step_spawning(&lvl);
            top = top.max(a.vx.abs());
        }
        assert_eq!(a.phase, hop::RUN);
        assert!((top - HOP_RUN_VX).abs() < 1e-4, "chase speed {top}");
        // the player gone behind: lost after 16 frames
        a.target = (a.x - 200.0 * a.dir as f32, 32.0);
        run(&mut a, &lvl, 18);
        assert_eq!(a.phase, hop::LOST);
    }

    #[test]
    fn porcupuffer_loses_speed_with_hp_and_dies_on_the_fourth_hit() {
        let mut a = Actor::new(Kind::Porcupuffer, 100.0, 100.0);
        assert_eq!(a.porcupuffer_hp(), 4);
        a.phase = puff::SWIM;
        a.hops = PUFF_HP;
        let speeds: Vec<f32> = (0..4)
            .map(|k| {
                let m = a.puff_m();
                if k < 3 {
                    assert!(!a.porcupuffer_hit(0.0));
                } else {
                    assert!(a.porcupuffer_hit(0.0));
                }
                m * PUFF_SWIM
            })
            .collect();
        assert_eq!(speeds, [1.25, 1.125, 1.0, 0.9375]);
        assert_eq!(a.mode, super::super::Mode::DieFall);
    }

    #[test]
    fn porcupuffer_homes_in_under_water_and_lunges_when_lined_up() {
        let mut lvl = Level::flat(40);
        lvl.top = 432.0;
        let mut a = Actor::new(Kind::Porcupuffer, 100.0, 100.0);
        a.w3.flooded = true;
        a.w3.player_wet = true;
        a.w3.player_h = 16.0;
        a.dir = 1;
        // the player level with its aim point, 200 px ahead
        a.target = (300.0, 100.0);
        let mut lunged = false;
        let mut top: f32 = 0.0;
        for _ in 0..200 {
            a.step_spawning(&lvl);
            lunged |= a.phase == puff::LUNGE;
            if a.phase == puff::SWIM && !lunged {
                top = top.max(a.speed);
            }
        }
        assert!((top - PUFF_SWIM).abs() < 1e-4, "cruise {top}");
        assert!(lunged, "lunged when close");
    }

    #[test]
    fn porcupuffer_falls_to_the_surface_and_patrols_its_band() {
        let lvl = Level::flat(80);
        let mut a = Actor::new(Kind::Porcupuffer, 400.0, 200.0);
        a.w3.liquid = Some((120.0, true));
        a.target = (0.0, 300.0);
        a.dir = 1;
        run(&mut a, &lvl, 200);
        assert_eq!(a.phase, puff::SURFACE);
        let f = 120.0 - 4.0 - 16.0;
        assert!((a.y - f).abs() < 1.0, "floats at {}", a.y);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for _ in 0..1500 {
            a.step_spawning(&lvl);
            lo = lo.min(a.x);
            hi = hi.max(a.x);
        }
        assert!(hi - 400.0 <= PUFF_BAND + 20.0 && 400.0 - lo <= PUFF_BAND + 20.0);
        assert!(
            hi - lo > 2.0 * PUFF_BAND - 10.0,
            "patrols both ways: {lo}…{hi}"
        );
    }

    #[test]
    fn bully_charges_a_seen_player_and_is_only_pushed_back() {
        let (lvl, mut a) = on_flat(Kind::Bully, 300.0);
        a.target = (200.0, 32.0);
        run(&mut a, &lvl, BULLY_FIND as usize + 60);
        assert!(a.bully_charging());
        assert!((a.vx + BULLY_CHASE).abs() < 0.05, "charge vx {}", a.vx);
        a.bully_push(push::STOMP, a.x - 10.0);
        assert!((a.vx - 1.75).abs() < 1e-4);
        assert!(a.bully_pushed());
        run(&mut a, &lvl, 10);
        assert!(
            (a.vx - (1.75 - 0.5)).abs() < 1e-3,
            "decays 0.05/f: {}",
            a.vx
        );
        run(&mut a, &lvl, 51);
        assert!(!a.bully_pushed());
        // a heavy push stuns it for 120 more frames
        a.bully_push(push::HEAVY, a.x + 10.0);
        run(&mut a, &lvl, 61);
        assert_eq!(a.phase, bully::HOLD_OUT);
        assert!(a.alive());
    }

    #[test]
    fn bully_gives_up_beyond_256_px() {
        let (lvl, mut a) = on_flat(Kind::Bully, 300.0);
        a.target = (200.0, 32.0);
        run(&mut a, &lvl, BULLY_FIND as usize + 5);
        assert_eq!(a.phase, bully::CHASE);
        a.target = (a.x + 300.0, 32.0);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, bully::WAIT);
    }

    #[test]
    fn charvaargh_warns_then_leaps_a_half_circle_and_dives_back() {
        let lvl = Level::flat(40);
        let mut a = Actor::new(Kind::Charvaargh, 200.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = (0.0, 300.0);
        run(&mut a, &lvl, 60);
        let rest = 44.0 - CHAR_DEPTH - 64.0;
        assert_eq!(a.phase, chr::WAIT);
        assert!((a.y - rest).abs() < 0.01);
        // a player 100 px to the right on the shore
        a.target = (300.0, 48.0);
        run(&mut a, &lvl, 1);
        assert!(a.charvaargh_attacking());
        assert_eq!(a.dir, 1);
        let (mut top, mut segs) = (f32::MIN, 0);
        let mut back = false;
        for _ in 0..400 {
            a.step_spawning(&lvl);
            top = top.max(a.y + 32.0);
            segs = segs.max(a.charvaargh_segments().len());
            back = a.phase == chr::WAIT;
            if back {
                break;
            }
        }
        assert!(back, "dived back in further along: x {}", a.x);
        assert!((a.x - (200.0 + 2.0 * CHAR_RADIUS)).abs() < 4.0, "x {}", a.x);
        assert!((top - (44.0 + CHAR_RADIUS)).abs() < 6.0, "apex {top}");
        assert_eq!(segs, CHAR_SEGMENTS);
    }

    #[test]
    fn piranha_creeper_creeps_out_along_its_path_and_back() {
        let mut lvl = Level::flat(40);
        // up 2 cells, then right 1 (codes: up, up, up→right, end facing right)
        lvl.creepers.push((0, vec![4, 4, 8, 14]));
        let path = Kind::PiranhaCreeper {
            path: 0,
            sleeps: false,
        };
        let mut a = Actor::new(path, 200.0, 32.0);
        run(&mut a, &lvl, 1);
        let base = (200.0, 48.0);
        assert!((a.x - base.0).abs() < 0.01 && (a.y + CREEP_HEAD - base.1).abs() < 0.01);
        // 96 px of path at ≤ 0.6 px/f: out within ~200 frames after the start wait
        let mut top: f32 = 0.0;
        for _ in 0..CREEP_WAIT as usize + 220 {
            a.step_spawning(&lvl);
            top = top.max(a.speed);
        }
        assert!((top - CREEP_SPEED).abs() < 1e-4);
        assert!((a.x - 232.0).abs() < 0.01 && (a.y + CREEP_HEAD - 112.0).abs() < 0.01);
        let codes = lvl.creepers[0].1.clone();
        assert_eq!(a.creeper_stalk(&codes).len(), 3);
        // then back to the base
        run(&mut a, &lvl, 250);
        assert!((a.x - base.0).abs() < 0.01 && (a.y + CREEP_HEAD - base.1).abs() < 0.01);
    }

    #[test]
    fn sleeping_creeper_stays_out_and_a_trampled_one_resumes() {
        let mut lvl = Level::flat(40);
        lvl.creepers.push((3, vec![1, 1, 14]));
        let mut a = Actor::new(
            Kind::PiranhaCreeper {
                path: 3,
                sleeps: true,
            },
            100.0,
            32.0,
        );
        run(&mut a, &lvl, CREEP_WAIT as usize + 30);
        let x = a.x;
        assert!(a.creeper_trample());
        assert!(!a.creeper_trample(), "already flat");
        run(&mut a, &lvl, 100);
        assert_eq!(a.x, x, "held still while trampled");
        run(
            &mut a,
            &lvl,
            CREEP_DAMAGE_WAIT as usize + CREEP_WAIT as usize + 200,
        );
        assert!(a.creeper_asleep());
        assert!((a.x - 164.0).abs() < 0.01, "asleep at the end: x {}", a.x);
    }
}
