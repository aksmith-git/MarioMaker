//! Actors: enemies, items and the player's fireballs.
//!
//! From the Phase 3 specs: objects.md §3 (generic walker), 0-goomba.md, 1-koopa.md,
//! 20-super-mushroom.md (shared item movement and block emergence), 34-fire-flower.md (fireballs),
//! 35-super-star.md, 33-one-up.md and the shared enemy draft (death, emergence, activation).
//! Positions are the feet centre (px, y up); boxes extend half_w either side and h upward.

use crate::level::{Cell, Level, TILE};

pub const WALK_SPEED: f32 = 0.5;
pub const ENEMY_GRAVITY: f32 = -0.1875;
pub const MAX_FALL: f32 = -4.0;
pub const TURN_LOCK: i32 = 8;
pub const STOMP_FRAMES: i32 = 30;
pub const SHELL_WAKE: i32 = 511;
pub const SHELL_SHAKE: i32 = 96;
pub const ITEM_SPEED: f32 = 1.0;
pub const FIREBALL_SPEED: f32 = 3.625;
pub const FIREBALL_GRAVITY: f32 = -0.4375;
pub const FIREBALL_BOUNCE: f32 = 4.0;
pub const FIREBALL_LIFE: i32 = 600;
// DieFall default launch (shared enemy draft §6.2)
const DIE_SPEED: f32 = 1.5;
const DIE_VY: f32 = 3.9;
const DIE_GRAVITY: f32 = -0.24375;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Goomba,
    Koopa {
        red: bool,
    },
    /// A Koopa in its shell (still or sliding).
    Shell {
        red: bool,
    },
    Mushroom,
    FireFlower,
    Star,
    OneUp,
    Fireball,
}

impl Kind {
    pub fn is_enemy(self) -> bool {
        matches!(self, Kind::Goomba | Kind::Koopa { .. } | Kind::Shell { .. })
    }
    pub fn is_item(self) -> bool {
        matches!(
            self,
            Kind::Mushroom | Kind::FireFlower | Kind::Star | Kind::OneUp
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Item rising out of (or sinking below) a block at constant speed, no gravity, until the
    /// feet reach `until`.
    Emerge {
        until: f32,
    },
    /// Enemy popping out of a block (vy 2.5 up, gravity on) for `timer` frames, no collisions.
    PopOut,
    Normal,
    /// Flattened Goomba (DieStamp), deleted when the timer runs out.
    Stomped,
    /// Knocked off (DieFall): no collision, falls off the area.
    DieFall,
    ShellWait,
    ShellSlide,
    /// Remove at the end of the frame.
    Dead,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub kind: Kind,
    pub mode: Mode,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// +1 right, −1 left
    pub dir: i32,
    pub on_ground: bool,
    pub big: bool,
    pub wings: bool,
    pub timer: i32,
    pub turn_lock: i32,
    pub age: u32,
    /// Frames during which a just-kicked shell cannot hurt the kicker.
    pub kick_lock: i32,
    /// Kill chain (shells) / stomp chain bookkeeping.
    pub chain: u32,
    /// Paragoomba hop cycle and pause.
    pub hops: u8,
    pub pause: i32,
    pub home_y: f32,
    /// Index into World::spawns, if this actor came from a course object.
    pub spawn: Option<usize>,
    /// Hit a wall this frame (in the movement direction).
    pub wall: bool,
}

impl Actor {
    pub fn new(kind: Kind, x: f32, y: f32) -> Self {
        Self {
            kind,
            mode: Mode::Normal,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            dir: 1,
            on_ground: false,
            big: false,
            wings: false,
            timer: 0,
            turn_lock: 0,
            age: 0,
            kick_lock: 0,
            chain: 0,
            hops: 0,
            pause: 0,
            home_y: y,
            spawn: None,
            wall: false,
        }
    }

    pub fn half_w(&self) -> f32 {
        match self.kind {
            Kind::Fireball => 4.0,
            _ if self.big => 16.0,
            _ => 8.0,
        }
    }

    pub fn height(&self) -> f32 {
        match self.kind {
            Kind::Fireball => 8.0,
            Kind::Koopa { .. } if !self.big => 16.0, // body box 16×16 (art is 16×32)
            _ if self.big => 32.0,
            _ => 16.0,
        }
    }

    /// (x0, y0, x1, y1) hit box.
    pub fn bbox(&self) -> (f32, f32, f32, f32) {
        let hw = self.half_w();
        (self.x - hw, self.y, self.x + hw, self.y + self.height())
    }

    pub fn alive(&self) -> bool {
        !matches!(self.mode, Mode::Stomped | Mode::DieFall | Mode::Dead)
    }

    /// Can touch the player / other actors.
    pub fn solid_contact(&self) -> bool {
        matches!(self.mode, Mode::Normal | Mode::ShellWait | Mode::ShellSlide)
    }

    /// Knock off (DieFall) away from `from_x`.
    pub fn die_fall(&mut self, from_x: f32) {
        let dir = if self.x < from_x { -1.0 } else { 1.0 };
        self.mode = Mode::DieFall;
        self.vx = DIE_SPEED * dir;
        self.vy = DIE_VY;
    }

    /// Stomp reaction. Returns true if the actor took it (player bounces).
    pub fn stomp(&mut self) -> bool {
        if self.wings {
            // a stomp removes the wings (0-goomba.md, 1-koopa.md)
            self.wings = false;
            self.vy = self.vy.min(0.0);
            return true;
        }
        match self.kind {
            Kind::Goomba => {
                self.mode = Mode::Stomped;
                self.timer = STOMP_FRAMES;
                self.vx = 0.0;
                self.vy = 0.0;
                true
            }
            Kind::Koopa { red } => {
                self.kind = Kind::Shell { red };
                self.enter_shell_wait();
                true
            }
            // a stomp on a sliding shell stops it dead; on a still shell, nothing (the touch kicks
            // it instead, handled by the caller)
            Kind::Shell { .. } if self.mode == Mode::ShellSlide => {
                self.enter_shell_wait();
                true
            }
            _ => false,
        }
    }

    fn enter_shell_wait(&mut self) {
        self.mode = Mode::ShellWait;
        self.timer = SHELL_WAKE;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    /// Kick a still shell (1-koopa.md): 2.5, plus 0.5·|kicker vx| when the kicker moves in the
    /// kick direction.
    pub fn kick(&mut self, dir: i32, kicker_vx: f32) {
        let mut speed = 2.5;
        if kicker_vx != 0.0 && kicker_vx.signum() as i32 == dir {
            speed += 0.5 * kicker_vx.abs();
        }
        self.mode = Mode::ShellSlide;
        self.dir = dir;
        self.vx = speed * dir as f32;
        self.kick_lock = 16;
        self.chain = 0;
    }

    pub fn turn(&mut self) {
        self.dir = -self.dir;
        self.vx = -self.vx;
        self.turn_lock = TURN_LOCK;
    }

    /// One frame of behaviour and movement. `player_x` is used for facing rules.
    pub fn step(&mut self, level: &Level, player_x: f32) {
        self.age += 1;
        self.wall = false;
        if self.kick_lock > 0 {
            self.kick_lock -= 1;
        }
        if self.turn_lock > 0 {
            self.turn_lock -= 1;
        }
        match self.mode {
            Mode::Dead => {}
            Mode::Emerge { until } => {
                self.y += self.vy;
                if (self.vy > 0.0 && self.y >= until) || (self.vy < 0.0 && self.y <= until) {
                    self.y = until;
                    self.mode = Mode::Normal;
                    self.vy = 0.0;
                    self.vx = match self.kind {
                        Kind::Mushroom | Kind::OneUp | Kind::Star => ITEM_SPEED * self.dir as f32,
                        _ => 0.0,
                    };
                    if self.kind == Kind::Star {
                        self.vy = FIREBALL_BOUNCE;
                    }
                }
            }
            Mode::PopOut => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.y += self.vy;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Normal;
                }
            }
            Mode::Stomped => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Dead;
                }
            }
            Mode::DieFall => {
                self.vy = (self.vy + DIE_GRAVITY).max(MAX_FALL);
                self.x += self.vx;
                self.y += self.vy;
                if self.y < -4.0 * TILE {
                    self.mode = Mode::Dead;
                }
            }
            Mode::Normal => self.step_normal(level, player_x),
            Mode::ShellWait => {
                if self.on_ground && self.vy <= 0.0 {
                    self.vx = 0.0;
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                self.timer -= 1;
                if self.timer <= 0
                    && let Kind::Shell { red } = self.kind
                {
                    // the Koopa walks out, facing the player
                    self.kind = Kind::Koopa { red };
                    self.mode = Mode::Normal;
                    self.dir = if player_x <= self.x { -1 } else { 1 };
                }
            }
            Mode::ShellSlide => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall {
                    // reverse, keeping the speed (the caller hits the block in the way)
                    self.dir = -self.dir;
                    self.vx = -self.vx;
                }
                if self.vx.abs() < 0.001 {
                    self.enter_shell_wait();
                }
            }
        }
        if self.y < -4.0 * TILE && self.mode != Mode::DieFall {
            self.mode = Mode::Dead; // fell into a pit
        }
    }

    fn step_normal(&mut self, level: &Level, player_x: f32) {
        match self.kind {
            Kind::Goomba | Kind::Koopa { .. } => {
                if self.wings {
                    self.step_wings(level, player_x);
                    return;
                }
                if self.on_ground {
                    self.vx = WALK_SPEED * self.dir as f32;
                } else {
                    self.vx = self.vx.clamp(-WALK_SPEED, WALK_SPEED);
                }
                // red Koopa turns at ledges: probe |half width| + 0.1 ahead for floor
                if let Kind::Koopa { red: true } = self.kind
                    && self.on_ground
                    && self.turn_lock == 0
                {
                    let px = self.x + self.dir as f32 * (self.half_w() + 0.1);
                    let below = level.cell_at(px, self.y - 1.0);
                    let slope = level.slope_at(px).is_some_and(|(s, _)| s >= self.y - 8.0);
                    if below == Cell::Empty && !slope {
                        self.turn();
                    }
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Mushroom | Kind::OneUp => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                if self.on_ground {
                    self.vx = ITEM_SPEED * self.dir as f32;
                }
                self.collide(level);
                if self.wall {
                    self.dir = -self.dir;
                    self.vx = ITEM_SPEED * self.dir as f32;
                }
            }
            Kind::FireFlower => {
                self.vx = 0.0;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
            }
            Kind::Star => {
                self.vx = ITEM_SPEED * self.dir as f32;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.on_ground {
                    self.vy = FIREBALL_BOUNCE; // bounces 4.0 on every landing
                    self.on_ground = false;
                }
                if self.wall {
                    self.dir = -self.dir;
                }
            }
            Kind::Fireball => {
                self.timer -= 1;
                self.vy = (self.vy + FIREBALL_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.on_ground {
                    self.vy = FIREBALL_BOUNCE;
                    self.on_ground = false;
                }
                if self.wall || self.timer <= 0 {
                    self.mode = Mode::Dead; // snuffed against a wall
                }
            }
            Kind::Shell { .. } => {}
        }
    }

    /// Paragoomba hops (small, small, big; 80-frame pause after the big hop); green Paratroopa
    /// hops 3.5625 on every landing; red Paratroopa bobs around its spawn height.
    fn step_wings(&mut self, level: &Level, player_x: f32) {
        match self.kind {
            Kind::Goomba => {
                self.vx = WALK_SPEED * self.dir as f32;
                if self.pause > 0 {
                    self.pause -= 1;
                }
                if self.on_ground && self.vy <= 0.0 {
                    let behind = (player_x - self.x).signum() as i32 != self.dir;
                    if self.timer > 64 && behind {
                        self.turn();
                        self.timer = 0;
                        self.hops = 0;
                        self.pause = 0;
                    } else if self.pause == 0 {
                        self.hops = (self.hops + 1) % 3;
                        if self.hops == 0 {
                            self.pause = 80;
                        }
                        self.vy = if self.hops == 0 { 3.0 } else { 1.25 };
                        self.on_ground = false;
                    }
                }
                self.timer += 1;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Koopa { red: false } => {
                self.vx = 0.6875 * self.dir as f32;
                if self.on_ground && self.vy <= 0.0 {
                    self.vy = 3.5625;
                    self.on_ground = false;
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Koopa { red: true } => {
                // vertical speed eases toward ±0.5 by 0.01/frame between home−32 and home+48
                let above = self.y + 8.0 > self.home_y + 48.0;
                let below = self.y + 8.0 < self.home_y - 32.0;
                let target = if above {
                    -0.5
                } else if below || self.vy >= 0.0 {
                    0.5
                } else {
                    -0.5
                };
                self.vy = if self.vy < target {
                    (self.vy + 0.01).min(target)
                } else {
                    (self.vy - 0.01).max(target)
                };
                self.vx = 0.0;
                self.y += self.vy;
                if self.timer <= 0 {
                    self.dir = if player_x <= self.x { -1 } else { 1 };
                    self.timer = 10;
                }
                self.timer -= 1;
            }
            _ => {}
        }
    }

    /// Axis-separated movement against the tile grid and slopes. Sets on_ground and wall.
    pub fn collide(&mut self, level: &Level) {
        let (hw, h) = (self.half_w(), self.height());
        let was_ground = self.on_ground;
        let solid = |px: f32, py: f32| level.cell_at(px, py) == Cell::Solid;

        // horizontal
        self.x += self.vx;
        if self.vx != 0.0 {
            let side = self.vx.signum();
            let wx = self.x + side * hw;
            let ys = [self.y + 2.0, self.y + (h - 2.0).max(2.0)];
            if ys.iter().any(|&wy| solid(wx, wy)) && level.slope_at(wx).is_none() {
                let edge = if side > 0.0 {
                    (wx / TILE).floor() * TILE
                } else {
                    ((wx / TILE).floor() + 1.0) * TILE
                };
                self.x = edge - side * (hw + 0.001);
                self.wall = true;
            }
        }
        if self.x - hw < level.left {
            self.x = level.left + hw;
            self.wall = self.vx < 0.0 || self.wall;
        }

        // vertical
        let prev_y = self.y;
        self.y += self.vy;
        let fx = (hw - 3.0).max(1.0);
        if self.vy > 0.0 {
            let hy = self.y + h;
            if [-fx, fx].iter().any(|&dx| solid(self.x + dx, hy)) {
                self.y = (hy / TILE).floor() * TILE - h - 0.001;
                self.vy = match self.kind {
                    Kind::Mushroom | Kind::OneUp => -1.0,
                    Kind::Star => -2.0,
                    _ => 0.0,
                };
            }
            self.on_ground = false;
        } else {
            let mut landed: Option<f32> = None;
            for dx in [-fx, fx] {
                let (px, py) = (self.x + dx, self.y);
                let top = ((py / TILE).floor() + 1.0) * TILE;
                let cell = level.cell_at(px, py);
                if cell == Cell::Solid || (cell == Cell::SemiTop && prev_y >= top - 0.01) {
                    landed = Some(landed.map_or(top, |t: f32| t.max(top)));
                }
            }
            for dx in [-fx, 0.0, fx] {
                if let Some((surf, _)) = level.slope_at(self.x + dx) {
                    let reach = if was_ground { 6.0 } else { 0.0 };
                    if self.y <= surf + reach
                        && prev_y >= surf - 6.0
                        && landed.is_none_or(|t| surf >= t)
                    {
                        landed = Some(surf);
                    }
                }
            }
            match landed {
                Some(top) => {
                    self.y = top;
                    self.vy = 0.0;
                    self.on_ground = true;
                }
                None => self.on_ground = false,
            }
        }
    }
}

/// Non-interacting effects ("simple objects").
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Coin popped from a block (B10): credited when its life ends.
    CoinPop {
        x: f32,
        y: f32,
        vy: f32,
        g: f32,
        cap: f32,
        life: i32,
    },
    /// Brick debris (B8): 4 per brick, 127 frames.
    Debris {
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        life: i32,
    },
    /// Score popup rising above its source.
    Score {
        x: f32,
        y: f32,
        points: u32,
        life: i32,
    },
}

impl Effect {
    pub fn coin_pop(x: f32, block_y: f32, down: bool) -> Self {
        if down {
            Effect::CoinPop {
                x,
                y: block_y - 16.0,
                vy: 0.0,
                g: -0.0625,
                cap: -2.0,
                life: 28,
            }
        } else {
            Effect::CoinPop {
                x,
                y: block_y + 16.0,
                vy: 3.0,
                g: -0.1875,
                cap: -8.0,
                life: 28,
            }
        }
    }

    /// Four debris pieces for a brick broken at block origin (bx, by). `h`, `r` are the shared
    /// random offsets (B8: h ∈ {0} ∪ [0, 0.5), r ∈ [0, 0.2)).
    pub fn debris(bx: f32, by: f32, h: f32, r: f32) -> [Effect; 4] {
        let starts = [(-4.0, 8.0), (4.0, 8.0), (-4.0, 4.0), (4.0, 4.0)];
        let vxs = [-1.0, 1.0, -1.0, 1.0];
        let vys = [4.5, 4.5, 3.0, 3.0];
        std::array::from_fn(|i| Effect::Debris {
            x: bx + starts[i].0,
            y: by + starts[i].1,
            vx: vxs[i] + if vxs[i] > 0.0 { h } else { -h },
            vy: vys[i] + r,
            life: 127,
        })
    }

    /// Advance; returns false when finished.
    pub fn step(&mut self) -> bool {
        match self {
            Effect::CoinPop {
                y,
                vy,
                g,
                cap,
                life,
                ..
            } => {
                *vy = (*vy + *g).max(*cap);
                *y += *vy;
                *life -= 1;
                *life > 0
            }
            Effect::Debris {
                x, y, vx, vy, life, ..
            } => {
                *vy = (*vy - 0.1875).max(-4.0);
                *y += *vy;
                *x += *vx;
                *life -= 1;
                *life > 0
            }
            Effect::Score { y, life, .. } => {
                if *life > 20 {
                    *y += 1.0;
                }
                *life -= 1;
                *life > 0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground_actor(kind: Kind) -> (Level, Actor) {
        let level = Level::flat(100);
        let mut a = Actor::new(kind, 20.0 * TILE, 32.0);
        a.on_ground = true;
        a.dir = 1;
        (level, a)
    }

    #[test]
    fn goomba_walks_half_pixel_per_frame() {
        let (level, mut a) = ground_actor(Kind::Goomba);
        let x0 = a.x;
        for _ in 0..60 {
            a.step(&level, 0.0);
        }
        assert!((a.x - x0 - 30.0).abs() < 1e-3, "moved {}", a.x - x0);
        assert!(a.on_ground);
    }

    #[test]
    fn goomba_turns_at_wall() {
        let mut level = Level::flat(100);
        level.set_solid(25, 2);
        let mut a = Actor::new(Kind::Goomba, 22.0 * TILE, 32.0);
        a.on_ground = true;
        for _ in 0..200 {
            a.step(&level, 0.0);
        }
        assert_eq!(a.dir, -1);
        assert!(a.x < 25.0 * TILE);
    }

    #[test]
    fn red_koopa_turns_at_ledge_green_walks_off() {
        let level = Level::flat(30); // ground ends at x = 480
        let mut red = Actor::new(Kind::Koopa { red: true }, 28.0 * TILE, 32.0);
        red.on_ground = true;
        let mut green = red.clone();
        green.kind = Kind::Koopa { red: false };
        for _ in 0..200 {
            red.step(&level, 0.0);
            green.step(&level, 0.0);
        }
        assert_eq!(red.dir, -1);
        assert!(red.on_ground);
        assert!(!green.on_ground || green.mode == Mode::Dead);
    }

    #[test]
    fn stomped_goomba_lasts_30_frames() {
        let (level, mut a) = ground_actor(Kind::Goomba);
        assert!(a.stomp());
        for i in 0..30 {
            assert_eq!(a.mode, Mode::Stomped, "frame {i}");
            a.step(&level, 0.0);
        }
        assert_eq!(a.mode, Mode::Dead);
    }

    #[test]
    fn koopa_shell_wakes_after_511() {
        let (level, mut a) = ground_actor(Kind::Koopa { red: false });
        a.stomp();
        assert_eq!(a.kind, Kind::Shell { red: false });
        for _ in 0..510 {
            a.step(&level, 0.0);
        }
        assert_eq!(a.mode, Mode::ShellWait);
        a.step(&level, 0.0);
        assert_eq!(a.kind, Kind::Koopa { red: false });
    }

    #[test]
    fn kicked_shell_keeps_speed_and_bounces() {
        let mut level = Level::flat(100);
        level.set_solid(30, 2);
        let mut a = Actor::new(Kind::Shell { red: false }, 20.0 * TILE, 32.0);
        a.on_ground = true;
        a.mode = Mode::ShellWait;
        a.kick(1, 1.5);
        assert!((a.vx - 3.25).abs() < 1e-6);
        let mut reversed = false;
        for _ in 0..120 {
            a.step(&level, 0.0);
            if a.vx < 0.0 {
                reversed = true;
            }
        }
        assert!(reversed);
        assert!((a.vx.abs() - 3.25).abs() < 1e-6);
    }

    #[test]
    fn mushroom_emerges_in_32_frames_then_walks() {
        let level = Level::flat(100);
        let mut a = Actor::new(Kind::Mushroom, 20.0 * TILE + 8.0, 4.0 * TILE);
        a.vy = 0.5;
        a.mode = Mode::Emerge { until: 5.0 * TILE };
        let mut n = 0;
        while matches!(a.mode, Mode::Emerge { .. }) {
            a.step(&level, 0.0);
            n += 1;
        }
        assert_eq!(n, 32);
        assert_eq!(a.vx, ITEM_SPEED);
    }

    #[test]
    fn coin_pop_peaks_near_38_5() {
        let mut e = Effect::coin_pop(0.0, 0.0, false);
        let mut top: f32 = 0.0;
        let mut n = 0;
        while e.step() {
            n += 1;
            if let Effect::CoinPop { y, .. } = e {
                top = top.max(y);
            }
        }
        assert_eq!(n + 1, 28);
        assert!((top - 38.5).abs() < 0.2, "peak {top}");
    }

    #[test]
    fn fireball_bounces_and_dies_at_wall() {
        let mut level = Level::flat(100);
        level.set_solid(26, 2);
        let mut f = Actor::new(Kind::Fireball, 20.0 * TILE, 40.0);
        f.vx = FIREBALL_SPEED;
        f.vy = -3.0;
        f.timer = FIREBALL_LIFE;
        let mut bounced = false;
        for _ in 0..60 {
            f.step(&level, 0.0);
            if f.vy > 3.0 {
                bounced = true;
            }
            if f.mode == Mode::Dead {
                break;
            }
        }
        assert!(bounced);
        assert_eq!(f.mode, Mode::Dead);
    }
}
