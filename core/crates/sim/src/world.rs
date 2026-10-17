//! The world: one area, the player, blocks, actors, the HUD counters, the timer and the goal.
//!
//! Interaction rules come from the Phase 3 specs (objects.md, the per-object specs and
//! 27-goal.md). Where the spec has no number the choice is marked [inf] and listed in
//! DECISIONS.md.

use mmk_formats::smm2::Course;

use crate::actors::{Actor, Effect, FIREBALL_LIFE, FIREBALL_SPEED, Kind, Mode};
use crate::blocks::{self, Block, BlockKind, Blocks, Contents, HitPower, HitResult, Release};
use crate::camera::{self, Camera};
use crate::input::{self, Input};
use crate::level::{Cell, CoinTile, Level, PipeDir, Spawn, TILE, switch_atlas};
use crate::liquid::Liquid;
use crate::snake::Snake;

/// Moving-solid owner ids for snake segments: base + snake × 32 + segment.
const SNAKE_OWNER: u32 = 0x8000_0000;
use crate::player::{Control, Form, Player, Pose, STAR_FRAMES};
use crate::render::{Sprite, SpriteImage};
use crate::warp::{self, SideHold, Transit};

/// SMM2 shows 24 × 13.5 tiles.
pub const VIEW_W: f32 = 24.0 * TILE;
pub const VIEW_H: f32 = 13.5 * TILE;

/// Score values by score type (0x7101cfd924): 100, 200, 400, 800, 1000, 2000, 4000, 8000, 1-UP.
pub const SCORE_TABLE: [u32; 8] = [100, 200, 400, 800, 1000, 2000, 4000, 8000];
pub const ONE_UP: u32 = u32::MAX;
const COIN_SCORE: u32 = 100;
const ITEM_SCORE: u32 = 1000;
/// Brick break score (4-brick.md B8: "+10 to the score counter").
const BREAK_SCORE: u32 = 10;
/// [inf] Kicking a still shell scores like SMB1 (400).
const KICK_SCORE: u32 = 400;
const MAX_LIVES: u32 = 99;
const HURRY_SECONDS: i32 = 100;
/// Activation margin around the view (shared enemy draft §2.8; the area-out rule itself is [?]).
const ACTIVATE_MARGIN: f32 = 32.0;

/// Things that happened this tick, for audio and UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Jump,
    Coin,
    Stomp,
    Kick,
    Bump,
    Break,
    ItemAppear,
    PowerUp,
    PowerDown,
    OneUp,
    Fireball,
    EnemyDown,
    Death,
    Goal,
    Hurry,
    TimeCount,
    Clear,
    Pipe,
    Checkpoint,
    Switch,
}

/// P-switch duration: [ext] about 10 s (not found in the binary).
pub const P_FRAMES: i32 = 600;

/// Pipe generator timing by pipe colour 0–3 (green, red, blue, yellow), frames.
const GEN_INTERVAL: [i32; 4] = [180, 60, 240, 120];
const GEN_FIRST: [i32; 4] = [90, 30, 90, 60];
const GEN_RETRY: [i32; 4] = [30, 8, 41, 19];

/// Goal pole demo (27-goal.md, GoalPole M1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GoalPhase {
    None,
    /// FlagDownWait: 10 frames on the pole.
    Hold {
        t: i32,
    },
    /// Player slides at −1.9, flag drops at 1.833.
    Slide,
    /// StartPlayerRun: 10 frames, then turn and walk.
    Land {
        t: i32,
    },
    /// Walking to X0 + 112 at 1.5 px/frame.
    Walk,
    /// Inside the castle, waiting for the tower flag.
    Inside {
        t: i32,
    },
    /// Tower flag rising 1 px/frame for 24 px.
    FlagUp,
    /// Waiting for the 340-frame mark and the timer countdown, then fade.
    Finish,
    Done,
}

/// The saved state of the area the player is not in.
#[derive(Clone, Debug)]
struct AreaStash {
    level: Level,
    blocks: Blocks,
    spawns: Vec<SpawnRecord>,
}

#[derive(Clone, Debug)]
struct SpawnRecord {
    spawn: Spawn,
    alive: bool,
    killed: bool,
    was_visible: bool,
}

pub struct World {
    pub tick: u64,
    pub level: Level,
    /// The course as loaded (restored on restart; bricks break, coins go).
    initial: Level,
    pub player: Player,
    pub camera_x: f32,
    pub camera_y: f32,
    /// The follow camera; `camera_x`/`camera_y` are its view's bottom-left corner.
    pub camera: Camera,
    pub blocks: Blocks,
    pub actors: Vec<Actor>,
    pub effects: Vec<Effect>,
    spawns: Vec<SpawnRecord>,
    pub score: u32,
    pub coins: u32,
    pub lives: u32,
    /// Time left in frames (−1: no timer).
    pub time: i32,
    pub goal_phase: GoalPhase,
    /// Goal flag height above Y0, tower flag offset, frames since StartPlayerRun.
    pub goal_flag: f32,
    pub tower_flag: f32,
    goal_frames: i32,
    countdown: bool,
    countdown_delay: i32,
    pub cleared: bool,
    pub events: Vec<Event>,
    rng: u32,
    field_model: &'static str,
    /// Both areas as loaded (main, sub); index of the current one; the other area's live state.
    area_initial: Vec<Level>,
    pub area: usize,
    stash: Option<AreaStash>,
    transit: Option<Transit>,
    side_hold: SideHold,
    prev_up: bool,
    /// Screen fade for area changes (0 clear … 1 black).
    pub fade: f32,
    /// Pipe generator countdowns (current area).
    gen_timers: Vec<i32>,
    /// Checkpoint reached: (area, checkpoint index). Survives death.
    pub checkpoint: Option<(usize, usize)>,
    /// Big coins taken: (area, spawn index). Kept across a checkpoint respawn.
    big_coins: std::collections::BTreeSet<(usize, usize)>,
    next_id: u32,
    /// Bill Blaster countdowns (main timer, cooldown) for the current area.
    blaster_timers: Vec<(i32, i32)>,
    /// Course-wide ON/OFF state (true = ON / red, the start state).
    pub on_off: bool,
    /// P-switch time left (frames).
    pub p_timer: i32,
    /// Tiles swapped by the P switch: bricks that became coins, coins that became bricks.
    p_bricks: Vec<(i32, i32)>,
    p_coins: Vec<(i32, i32)>,
    /// Donut under the player and how long it has been ridden.
    donut_ride: Option<((i32, i32), i32)>,
    /// Fallen donuts waiting to come back: tile, frames left.
    donut_respawn: Vec<((i32, i32), i32)>,
    /// Conveyor carry applied to the player (px/frame).
    carry: f32,
    /// The current area's liquid level, if any.
    pub liquid: Option<Liquid>,
    /// Cannon countdowns (current area).
    cannon_timers: Vec<i32>,
    /// Keys held.
    pub keys: u32,
    /// Key-door pairs unlocked: (area, warp id >> 1).
    unlocked_doors: std::collections::BTreeSet<(usize, u8)>,
    /// Snake blocks in the current area.
    pub snakes: Vec<Snake>,
    /// Swinging claw holding the player (actor id).
    held_by: Option<u32>,
    prev_jump_claw: bool,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// Field atlas model per theme for SMB1 (Model/M1_Field_<theme>).
fn field_model(style: &str, theme: u8) -> String {
    let name = [
        "plain",
        "underground",
        "castle",
        "airship",
        "water",
        "hauntedhouse",
        "snow",
        "desert",
        "athletic",
        "woods",
    ]
    .get(theme as usize)
    .copied()
    .unwrap_or("plain");
    format!("{style}_Field_{name}")
}

/// Animation frame `age / rate` cycling through `frames`.
fn walk_frame(frames: &[&'static str], age: u32, rate: u32) -> &'static str {
    frames[((age / rate) as usize) % frames.len()]
}

fn overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

impl World {
    /// The built-in demo strip (used when no course is loaded).
    pub fn new() -> Self {
        Self::with_level(Level::demo())
    }

    pub fn from_course(course: &Course) -> Self {
        let main = Level::from_course(course, 0);
        let sub = Level::from_course(course, 1);
        Self::with_areas(vec![main, sub])
    }

    pub fn with_level(level: Level) -> Self {
        Self::with_areas(vec![level])
    }

    pub fn with_areas(areas: Vec<Level>) -> Self {
        let level = areas[0].clone();
        let mut w = Self {
            tick: 0,
            initial: level.clone(),
            player: Player::new(level.start.0, level.start.1),
            level,
            camera_x: 0.0,
            camera_y: 0.0,
            camera: Camera::default(),
            blocks: Blocks::default(),
            actors: Vec::new(),
            effects: Vec::new(),
            spawns: Vec::new(),
            score: 0,
            coins: 0,
            lives: 5,
            time: -1,
            goal_phase: GoalPhase::None,
            goal_flag: 144.0,
            tower_flag: -24.0,
            goal_frames: 0,
            countdown: false,
            countdown_delay: 0,
            cleared: false,
            events: Vec::new(),
            rng: 0x2545_f491,
            field_model: "",
            area_initial: areas,
            area: 0,
            stash: None,
            transit: None,
            side_hold: SideHold::default(),
            prev_up: false,
            fade: 0.0,
            gen_timers: Vec::new(),
            checkpoint: None,
            big_coins: Default::default(),
            next_id: 1,
            blaster_timers: Vec::new(),
            on_off: true,
            p_timer: 0,
            p_bricks: Vec::new(),
            p_coins: Vec::new(),
            donut_ride: None,
            donut_respawn: Vec::new(),
            carry: 0.0,
            liquid: None,
            cannon_timers: Vec::new(),
            keys: 0,
            unlocked_doors: Default::default(),
            snakes: Vec::new(),
            held_by: None,
            prev_jump_claw: false,
        };
        w.field_model = Box::leak(field_model(&w.level.style, w.level.theme).into_boxed_str());
        w.reset_course();
        w
    }

    /// (Re)build the course state from the loaded course; keeps lives.
    fn reset_course(&mut self) {
        self.area = 0;
        self.stash = None;
        self.transit = None;
        self.fade = 0.0;
        self.initial = self.area_initial[0].clone();
        self.load_area_fresh();
        self.score = 0;
        self.coins = 0;
        self.time = if self.level.timer > 0 {
            self.level.timer as i32 * 60
        } else {
            -1
        };
        self.goal_phase = GoalPhase::None;
        self.goal_flag = 144.0;
        self.tower_flag = -24.0;
        self.goal_frames = 0;
        self.countdown = false;
        self.cleared = false;
        self.on_off = true;
        self.keys = 0;
        self.unlocked_doors.clear();
        self.p_timer = 0;
        self.p_bricks.clear();
        self.p_coins.clear();
        self.donut_ride = None;
        self.donut_respawn.clear();
        self.carry = 0.0;
        let (sx, sy) = self.level.start;
        self.player = Player::new(sx, sy);
        if self.checkpoint.is_none() {
            self.big_coins.clear();
        }
        if let Some((area, idx)) = self.checkpoint {
            self.switch_area(area);
            if let Some(cp) = self.level.checkpoints.get(idx) {
                self.player = Player::new(cp.x, cp.y);
            }
        }
        self.mark_taken_big_coins();
        // course start: the view starts centred on the player, clamped to the area
        let area = self.camera_area();
        let (px, py) = (self.player.x, self.player.y);
        let level = &self.level;
        self.camera = Camera::start(&area, px, py, |x, y| level.cell(x, y) == Cell::Solid);
        self.sync_camera();
        self.activate();
    }

    /// Big coins already collected (before a checkpoint) don't come back.
    fn mark_taken_big_coins(&mut self) {
        for (i, rec) in self.spawns.iter_mut().enumerate() {
            if self.big_coins.contains(&(self.area, i)) {
                rec.killed = true;
            }
        }
    }

    /// Build the current area's live state from its loaded level.
    fn load_area_fresh(&mut self) {
        self.level = self.initial.clone();
        self.field_model =
            Box::leak(field_model(&self.level.style, self.level.theme).into_boxed_str());
        self.blocks = Blocks::default();
        for b in &self.level.blocks {
            self.blocks
                .map
                .insert((b.x, b.y), Block::new(b.kind, b.contents));
        }
        self.spawns = self
            .level
            .spawns
            .iter()
            .map(|&spawn| SpawnRecord {
                spawn,
                alive: false,
                killed: false,
                was_visible: false,
            })
            .collect();
        self.actors.clear();
        self.effects.clear();
        self.reset_generators();
        self.liquid = self.level.liquid.map(Liquid::new);
        self.snakes = self
            .level
            .snakes
            .iter()
            .map(|(l, y, n, fast, rev, codes)| Snake::new(*l, *y, *n, *fast, *rev, codes))
            .collect();
    }

    fn add_actor(&mut self, mut a: Actor) {
        a.id = self.next_id;
        self.next_id += 1;
        self.actors.push(a);
    }

    fn reset_generators(&mut self) {
        self.blaster_timers = self.level.blasters.iter().map(|_| (60, 0)).collect();
        self.cannon_timers = self.level.cannons.iter().map(|_| 60).collect();
        self.gen_timers = self
            .level
            .generators
            .iter()
            .map(|g| GEN_FIRST[g.colour as usize & 3])
            .collect();
    }

    /// Switch to the other area, keeping the state of the one being left.
    fn switch_area(&mut self, to: usize) {
        if to == self.area {
            return;
        }
        let leaving = AreaStash {
            level: std::mem::replace(&mut self.level, Level::flat(1)),
            blocks: std::mem::take(&mut self.blocks),
            spawns: std::mem::take(&mut self.spawns),
        };
        self.area = to;
        self.initial = self.area_initial[to].clone();
        match self.stash.replace(leaving) {
            Some(st) => {
                self.level = st.level;
                self.blocks = st.blocks;
                self.spawns = st.spawns;
                self.field_model =
                    Box::leak(field_model(&self.level.style, self.level.theme).into_boxed_str());
                self.actors.clear();
                self.effects.clear();
                self.reset_generators();
                // [?] whether the liquid keeps moving while the player is away; it restarts here
                self.liquid = self.level.liquid.map(Liquid::new);
                self.snakes = self
                    .level
                    .snakes
                    .iter()
                    .map(|(l, y, n, fast, rev, codes)| Snake::new(*l, *y, *n, *fast, *rev, codes))
                    .collect();
            }
            None => self.load_area_fresh(),
        }
        self.mark_taken_big_coins();
        self.apply_on_off();
        self.donut_ride = None;
        self.donut_respawn.clear();
        for s in &mut self.spawns {
            s.alive = false;
            s.was_visible = false;
        }
    }

    fn rand(&mut self) -> f32 {
        // xorshift32 (debris jitter only; deterministic per run)
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn add_score(&mut self, points: u32, x: f32, y: f32) {
        if points == ONE_UP {
            self.add_life();
        } else {
            self.score = (self.score + points).min(999_999_990);
        }
        self.effects.push(Effect::Score {
            x,
            y,
            points,
            life: 50,
        });
    }

    fn add_life(&mut self) {
        self.lives = (self.lives + 1).min(MAX_LIVES);
        self.events.push(Event::OneUp);
    }

    fn add_coin(&mut self) {
        self.coins += 1;
        self.score += COIN_SCORE;
        self.events.push(Event::Coin);
        if self.coins >= 100 {
            self.coins -= 100;
            self.add_life();
        }
    }

    /// Chain scores: 100, 200, 400 … 8000, then 1-UP.
    fn chain_score(n: u32) -> u32 {
        SCORE_TABLE.get(n as usize).copied().unwrap_or(ONE_UP)
    }

    pub fn step(&mut self, input: Input) {
        self.events.clear();
        self.tick += 1;

        // death sequence: the world is frozen; restart when the fall ends
        if !self.player.alive() {
            self.player.step(input, &self.level);
            if self.player.restart_due() {
                self.lives = self.lives.saturating_sub(1);
                self.reset_course();
            }
            return;
        }
        // grow/shrink: everything else waits
        if matches!(self.player.control, Control::Change { .. }) {
            self.player.step(input, &self.level);
            return;
        }
        if self.transit.is_some() {
            self.step_transit();
            self.step_effects();
            self.update_camera();
            return;
        }
        if self.goal_phase != GoalPhase::None {
            self.step_goal();
            self.step_effects();
            self.update_camera();
            return;
        }

        // player
        let prev_head = self.player.y + self.player.sensors().head_y;
        let was_ground = self.player.on_ground;
        let jump_held = input.held(input::JUMP);
        self.player.on_ice = self.ice_under_player();
        if let Some(l) = &mut self.liquid {
            l.step();
        }
        self.player.in_water = self
            .liquid
            .is_some_and(|l| l.water() && l.covers(self.player.y + 8.0))
            || (self.level.theme == 2 && self.level.night); // castle night: swim in the air
        self.step_platforms(jump_held, input.dir_x());
        self.conveyor_carry();
        let prev_x = self.player.x;
        self.player.step(input, &self.level);
        if self.player.spiked && self.player.hurt() {
            self.events.push(if self.player.alive() {
                Event::PowerDown
            } else {
                Event::Death
            });
        }
        // pipes and doors
        let up_pressed = input.held(input::UP) && !self.prev_up;
        self.prev_up = input.held(input::UP);
        self.side_hold.update(input, self.player.on_ground);
        if self.player.control == Control::Normal {
            let pushing = (self.player.x - prev_x).abs() < 0.01 && input.dir_x() != 0;
            let entry = warp::pipe_entry(
                &self.player,
                input,
                self.side_hold,
                pushing,
                &self.level.warps,
            )
            .map(|w| (w, true))
            .or_else(|| {
                let keys = self.keys;
                let area = self.area;
                let unlocked = &self.unlocked_doors;
                warp::door_entry(
                    &self.player,
                    input,
                    up_pressed,
                    self.p_timer > 0,
                    |id| keys > 0 || unlocked.contains(&(area, id >> 1)),
                    &self.level.warps,
                )
                .map(|w| (w, false))
            });
            if let Some((from, is_pipe)) = entry {
                let warps: Vec<Vec<crate::level::Warp>> =
                    self.area_initial.iter().map(|l| l.warps.clone()).collect();
                if let Some((to_area, to)) = warp::destination(&from, self.area, &warps) {
                    if from.kind == crate::level::WarpKind::Door(2)
                        && self.unlocked_doors.insert((self.area, from.id >> 1))
                    {
                        self.keys -= 1; // a key opens the pair for good
                    }
                    self.player.control = Control::Demo;
                    self.player.vx = 0.0;
                    self.player.vy = 0.0;
                    self.player.crouch = false;
                    self.transit = Some(Transit {
                        phase: if is_pipe {
                            warp::Phase::Align
                        } else {
                            warp::Phase::DoorIn(warp::DOOR_IN_FRAMES)
                        },
                        from,
                        to,
                        to_area,
                    });
                    self.events.push(Event::Pipe);
                    return;
                }
            }
        }
        if was_ground && !self.player.on_ground && self.player.vy > 3.0 {
            self.events.push(Event::Jump);
        }
        self.check_hidden_blocks(prev_head);
        if let Some((tx, ty)) = self.player.head_hit {
            self.head_hit(tx, ty);
        }
        if self.player.throw {
            self.throw_fireball();
        }
        if !self.player.alive() {
            self.events.push(Event::Death);
            return;
        }
        // lava and poison kill (the player sinks below the surface)
        if let Some(l) = self.liquid
            && l.deadly()
        {
            if l.covers(self.player.y + 4.0) {
                self.player.star = 0;
                self.player.invuln = 0;
                self.player.kill();
                self.events.push(Event::Death);
                return;
            }
            for a in &mut self.actors {
                if a.alive() && l.covers(a.y + 2.0) && !a.kind.is_platform() {
                    a.mode = Mode::Dead;
                }
            }
        }
        self.collect_coins();
        self.step_donuts();
        self.step_p_switch();

        // blocks, actors, effects
        for r in self.blocks.step() {
            self.release(r);
        }
        let px = self.player.x;
        let py = self.player.y;
        let mut born = Vec::new();
        for a in &mut self.actors {
            if a.kind.is_platform() {
                continue; // already moved before the player
            }
            a.target = (px, py);
            a.target_facing = self.player.facing;
            if a.kind == Kind::Lakitu {
                a.speed = self.player.vx;
            }
            if let Kind::CheepCheep { .. } = a.kind {
                a.ridden = self
                    .liquid
                    .is_some_and(|l| l.water() && l.covers(a.y + 16.0));
            }
            if let Some(n) = a.step_spawning(&self.level) {
                born.push(n);
            }
        }
        self.explosions();
        for n in born {
            self.add_actor(n);
        }
        self.step_generators();
        self.step_blasters();
        self.fire_hazards();
        self.step_cannons();
        self.twister_lift();
        self.magikoopas();
        self.claws(input);
        self.pow_quake();
        self.check_checkpoints();
        self.shell_walls();
        self.actor_interactions();
        self.player_interactions(jump_held);
        self.step_effects();
        self.despawn();
        self.actors.retain(|a| a.mode != Mode::Dead);

        // timer: one count per second of play
        if self.time > 0 {
            self.time -= 1;
            if self.time == HURRY_SECONDS * 60 {
                self.events.push(Event::Hurry);
            }
            if self.time == 0 {
                self.player.kill();
                self.events.push(Event::Death);
            }
        }

        self.check_goal();
        self.update_camera();
        self.activate();
    }

    fn ice_under_player(&self) -> bool {
        // any ground in a snow-night course; ice blocks everywhere (player-physics.md §2.2)
        if self.level.theme == 6 && self.level.night {
            return true;
        }
        let (tx, ty) = (
            (self.player.x / TILE).floor() as i32,
            ((self.player.y - 1.0) / TILE).floor() as i32,
        );
        self.blocks
            .get(tx, ty)
            .is_some_and(|b| b.kind == BlockKind::Ice)
    }

    /// A hidden block is only solid to a head coming from below.
    fn check_hidden_blocks(&mut self, prev_head: f32) {
        if self.player.vy <= 0.0 && self.player.head_hit.is_none() {
            // the head stopped this frame only if it hit something; a rising head may enter a
            // hidden block's cell
        }
        let s = self.player.sensors();
        let head = self.player.y + s.head_y;
        if head <= prev_head {
            return;
        }
        for hx in [-s.head_x, s.head_x] {
            let (tx, ty) = (
                ((self.player.x + hx) / TILE).floor() as i32,
                (head / TILE).floor() as i32,
            );
            let bottom = ty as f32 * TILE;
            if prev_head <= bottom
                && self
                    .blocks
                    .get(tx, ty)
                    .is_some_and(|b| b.kind == BlockKind::Hidden)
            {
                self.level.set(tx, ty, Cell::Solid);
                self.player.y = bottom - s.head_y - 0.001;
                self.player.vy = 0.0;
                self.player.head_hit = Some((tx, ty));
                return;
            }
        }
    }

    fn head_hit(&mut self, tx: i32, ty: i32) {
        let power = if self.player.big() {
            HitPower::Break
        } else {
            HitPower::Bump
        };
        self.hit_block(tx, ty, true, power);
    }

    /// A hit on a block from below or above by the player or a shell.
    fn hit_block(&mut self, tx: i32, ty: i32, from_below: bool, power: HitPower) {
        let (bx, by) = blocks::origin(tx, ty);
        match self.blocks.hit(tx, ty, from_below, power) {
            HitResult::Nothing => {}
            HitResult::Broke => {
                self.level.set(tx, ty, Cell::Empty);
                let h = if self.rand() < 0.5 {
                    0.0
                } else {
                    self.rand() * 0.5
                };
                let r = self.rand() * 0.2;
                self.effects.extend(Effect::debris(bx, by, h, r));
                self.score += BREAK_SCORE;
                self.events.push(Event::Break);
                self.bump_top(tx, ty);
            }
            HitResult::Coin => {
                self.effects.push(Effect::coin_pop(bx, by, !from_below));
                self.events.push(Event::Coin);
                self.bump_top(tx, ty);
            }
            HitResult::Bumped => {
                self.events.push(Event::Bump);
                if from_below {
                    self.bump_top(tx, ty);
                }
            }
            HitResult::Switch => {
                self.on_off = !self.on_off;
                self.apply_on_off();
                self.events.push(Event::Switch);
                if from_below {
                    self.bump_top(tx, ty);
                }
            }
        }
    }

    /// Things resting on a bumped block get a block hit (B7): enemies are knocked away from the
    /// block centre, items hop, coins on top are collected.
    fn bump_top(&mut self, tx: i32, ty: i32) {
        let top = (ty + 1) as f32 * TILE;
        let (x0, x1) = (tx as f32 * TILE, (tx + 1) as f32 * TILE);
        let cx = (x0 + x1) / 2.0;
        let mut kills = 0;
        let mut scores = Vec::new();
        for a in &mut self.actors {
            let (ax0, _, ax1, _) = a.bbox();
            let on_top = (a.y - top).abs() < 2.0 && ax1 > x0 && ax0 < x1;
            if !on_top || !a.solid_contact() {
                continue;
            }
            match a.kind {
                Kind::Goomba | Kind::Piranha { .. } => {
                    a.die_fall(cx);
                    kills += 1;
                    scores.push((a.x, a.y + 24.0));
                }
                Kind::Koopa { red } | Kind::Shell { red } => {
                    // blkhit → flipped shell, vx 0.75 away, vy 3.5
                    a.kind = Kind::Shell { red };
                    a.mode = Mode::ShellWait;
                    a.timer = crate::actors::SHELL_WAKE;
                    a.vx = 0.75 * if a.x < cx { -1.0 } else { 1.0 };
                    a.vy = 3.5;
                    a.on_ground = false;
                    kills += 1;
                    scores.push((a.x, a.y + 24.0));
                }
                k if k.is_item() => {
                    a.vy = 3.0;
                    a.dir = if a.x < cx { -1 } else { 1 };
                    if matches!(a.kind, Kind::Mushroom | Kind::OneUp | Kind::Star) {
                        a.vx = a.dir as f32;
                    }
                    a.on_ground = false;
                }
                _ => {}
            }
        }
        for (i, (x, y)) in scores.into_iter().enumerate() {
            self.add_score(Self::chain_score(i as u32), x, y);
        }
        if kills > 0 {
            self.events.push(Event::EnemyDown);
        }
        // coin resting on the block
        if let Some(i) = self
            .level
            .coins
            .iter()
            .position(|c| c.x == tx && c.y == ty + 1)
        {
            self.level.coins.remove(i);
            self.effects.push(Effect::coin_pop(cx, top, false));
            self.events.push(Event::Coin);
        }
    }

    /// Bump end: spawn the block's contents (B9).
    fn release(&mut self, r: Release) {
        let (bx, by) = blocks::origin(r.x, r.y);
        // small player + power-up beyond the mushroom → Super Mushroom (20-super-mushroom.md)
        let contents = if r.contents.is_powerup() && !self.player.big() {
            Contents::Mushroom
        } else {
            r.contents
        };
        let item = match contents {
            Contents::Mushroom => Some(Kind::Mushroom),
            Contents::FireFlower => Some(Kind::FireFlower),
            Contents::Star => Some(Kind::Star),
            Contents::OneUp => Some(Kind::OneUp),
            Contents::Coin => None,
            Contents::Goomba => Some(Kind::Goomba),
            Contents::Koopa { red } => Some(Kind::Koopa { red }),
        };
        let Some(kind) = item else { return };
        let mut a = Actor::new(kind, bx, by);
        a.dir = 1;
        if kind.is_item() {
            // rise 16 px at 0.5 (down: 1.0), no gravity
            a.vy = if r.down { -1.0 } else { 0.5 };
            a.mode = Mode::Emerge {
                until: if r.down { by - 16.0 } else { by + 16.0 },
            };
            self.events.push(Event::ItemAppear);
        } else {
            // enemies pop out: up from the block top at vy 2.5 for 6 frames
            a.mode = Mode::PopOut;
            a.timer = 6;
            if r.down {
                a.y = by - a.height();
                a.vy = 0.0;
            } else {
                a.y = by + 16.0;
                a.vy = 2.5;
            }
        }
        self.add_actor(a);
    }

    fn throw_fireball(&mut self) {
        let mine: Vec<&Actor> = self
            .actors
            .iter()
            .filter(|a| a.kind == Kind::Fireball)
            .collect();
        let recent = mine.iter().filter(|a| a.age < 60).count();
        if mine.len() >= 6 || recent >= 2 {
            return;
        }
        let p = &self.player;
        let mut f = Actor::new(Kind::Fireball, p.x + p.facing as f32 * 8.0, p.y + 8.0);
        f.dir = p.facing;
        let carry = p.vx * p.facing as f32;
        f.vx = p.facing as f32
            * if carry > FIREBALL_SPEED {
                carry + FIREBALL_SPEED
            } else {
                FIREBALL_SPEED
            };
        f.vy = -3.0;
        f.timer = FIREBALL_LIFE;
        self.add_actor(f);
        self.events.push(Event::Fireball);
    }

    fn collect_coins(&mut self) {
        let b = self.player.contact_box();
        let mut got = 0;
        self.level.coins.retain(|c| {
            let cb = (
                c.x as f32 * TILE,
                c.y as f32 * TILE,
                (c.x + 1) as f32 * TILE,
                (c.y + 1) as f32 * TILE,
            );
            if overlap(b, cb) {
                got += 1;
                false
            } else {
                true
            }
        });
        for _ in 0..got {
            self.add_coin();
        }
    }

    /// Sliding shells hit the block they run into (side hits bump upward, bricks break).
    fn shell_walls(&mut self) {
        let mut hits = Vec::new();
        for a in &self.actors {
            if a.mode == Mode::ShellSlide && a.wall {
                // the shell has already reversed: the wall is behind its new direction
                let wx = a.x - a.dir as f32 * (a.half_w() + 1.0);
                let tx = (wx / TILE).floor() as i32;
                let ty = ((a.y + 8.0) / TILE).floor() as i32;
                hits.push((tx, ty));
            }
        }
        for (tx, ty) in hits {
            self.hit_block(tx, ty, true, HitPower::Break);
        }
    }

    fn actor_interactions(&mut self) {
        let n = self.actors.len();
        let mut scores = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = {
                    let (l, r) = self.actors.split_at_mut(j);
                    (&mut l[i], &mut r[0])
                };
                if !a.solid_contact() || !b.solid_contact() || !overlap(a.bbox(), b.bbox()) {
                    continue;
                }
                let (ae, be) = (a.kind.is_enemy(), b.kind.is_enemy());
                // fireballs
                for swap in [false, true] {
                    let (f, e) = if swap {
                        (&mut *b, &mut *a)
                    } else {
                        (&mut *a, &mut *b)
                    };
                    if f.kind == Kind::Fireball && e.kind.is_enemy() && f.mode != Mode::Dead {
                        let fx = f.x;
                        f.mode = Mode::Dead;
                        if !e.fire_proof() {
                            e.die_fall(fx);
                            scores.push((SCORE_TABLE[0], e.x, e.y + 24.0));
                        }
                    }
                }
                if !(ae && be) || !a.solid_contact() || !b.solid_contact() {
                    continue;
                }
                let walkers = a.kind.is_walker() && b.kind.is_walker();
                let a_slide = a.mode == Mode::ShellSlide;
                let b_slide = b.mode == Mode::ShellSlide;
                match (a_slide, b_slide) {
                    (true, true) => {
                        let (ax, bx) = (a.x, b.x);
                        a.die_fall(bx);
                        b.die_fall(ax);
                    }
                    (true, false) | (false, true) => {
                        let (s, v) = if a_slide { (a, b) } else { (b, a) };
                        let sx = s.x;
                        v.die_fall(sx);
                        scores.push((Self::chain_score(s.chain), v.x, v.y + 24.0));
                        s.chain += 1;
                    }
                    (false, false) if walkers => {
                        // walkers turn on touching each other (when moving toward each other)
                        let toward = (b.x - a.x) * a.dir as f32 > 0.0;
                        if a.mode == Mode::Normal && toward && a.turn_lock == 0 {
                            a.turn();
                        }
                        let toward_b = (a.x - b.x) * b.dir as f32 > 0.0;
                        if b.mode == Mode::Normal && toward_b && b.turn_lock == 0 {
                            b.turn();
                        }
                    }
                    _ => {}
                }
            }
        }
        if !scores.is_empty() {
            self.events.push(Event::EnemyDown);
        }
        for (p, x, y) in scores {
            self.add_score(p, x, y);
        }
    }

    fn player_interactions(&mut self, jump_held: bool) {
        if !self.player.alive() {
            return;
        }
        let hurt = self.player.hurt_box();
        let contact = self.player.contact_box();
        let star = self.player.star > 0;
        let mut i = 0;
        while i < self.actors.len() {
            let a = &self.actors[i];
            if !a.solid_contact() {
                i += 1;
                continue;
            }
            let ab = a.bbox();
            if let Kind::BigCoin { value } = a.kind {
                if overlap(contact, ab) {
                    let (ax, ay) = (a.x, a.y);
                    if let Some(s) = a.spawn {
                        self.big_coins.insert((self.area, s));
                    }
                    self.actors[i].mode = Mode::Dead;
                    let n = [10, 30, 50][value as usize];
                    for _ in 0..n {
                        self.add_coin();
                    }
                    self.score += [1000, 3000, 5000][value as usize] - n * COIN_SCORE;
                    let _ = (ax, ay);
                }
                i += 1;
                continue;
            }
            if a.kind.is_item() {
                if overlap(contact, ab) {
                    let kind = a.kind;
                    let (ax, ay) = (a.x, a.y);
                    self.actors[i].mode = Mode::Dead;
                    self.collect_item(kind, ax, ay);
                }
                i += 1;
                continue;
            }
            if let Kind::Skewer { .. } = a.kind {
                if overlap(hurt, a.skewer_head()) && !star && self.player.hurt() {
                    self.events.push(if self.player.alive() {
                        Event::PowerDown
                    } else {
                        Event::Death
                    });
                    return;
                }
                i += 1;
                continue;
            }
            if a.kind == Kind::Grinder {
                // saw: hurts on touch (radius [inf] 20 px around the centre y + 24)
                let (cx, cy) = (a.x, a.y + 24.0);
                let near = overlap(hurt, (cx - 20.0, cy - 20.0, cx + 20.0, cy + 20.0));
                if near && !star && self.player.hurt() {
                    self.events.push(if self.player.alive() {
                        Event::PowerDown
                    } else {
                        Event::Death
                    });
                    return;
                }
                i += 1;
                continue;
            }
            if a.kind == Kind::Key {
                if overlap(contact, ab) {
                    self.actors[i].mode = Mode::Dead;
                    self.keys += 1;
                    self.events.push(Event::OneUp);
                }
                i += 1;
                continue;
            }
            if a.kind == Kind::Bumper {
                self.bumper_contact(i, jump_held);
                i += 1;
                continue;
            }
            if a.kind == Kind::PSwitch {
                // pressed by landing on it from above
                let p = &self.player;
                if a.phase == 0 && p.vy <= 0.0 && overlap(hurt, ab) && p.y >= a.y + 8.0 {
                    let id = a.id;
                    if let Some(sw) = self.actors.iter_mut().find(|x| x.id == id) {
                        sw.phase = 1;
                        sw.timer = 34;
                    }
                    self.press_p_switch();
                }
                i += 1;
                continue;
            }
            if let (Kind::Muncher, Some((x0, y0, x1, y1, _))) = (a.kind, a.solid_box()) {
                // a Muncher hurts on every side, including from on top
                if overlap(hurt, (x0 - 1.5, y0 - 1.5, x1 + 1.5, y1 + 1.5))
                    && !star
                    && self.player.hurt()
                {
                    self.events.push(if self.player.alive() {
                        Event::PowerDown
                    } else {
                        Event::Death
                    });
                    return;
                }
                i += 1;
                continue;
            }
            if let (Kind::Thwomp { .. }, Some((x0, y0, x1, y1, _))) = (a.kind, a.solid_box()) {
                // touching a Thwomp's sides or bottom hurts; standing on it is fine
                let touching = overlap(hurt, (x0 - 1.5, y0 - 1.5, x1 + 1.5, y1));
                let on_top = self.player.ride == Some(a.id) || self.player.y >= y1 - 0.5;
                if touching && !on_top && !star && self.player.hurt() {
                    self.events.push(if self.player.alive() {
                        Event::PowerDown
                    } else {
                        Event::Death
                    });
                    return;
                }
                i += 1;
                continue;
            }
            // collapsed Dry Bones is harmless; a lit Bob-omb is kicked
            if a.kind == Kind::DryBones && a.phase == 1 {
                i += 1;
                continue;
            }
            if a.kind == Kind::BobOmb && (a.phase == 1 || a.phase == 2) {
                if overlap(hurt, ab) && a.kick_lock == 0 {
                    let dir = if a.x >= self.player.x { 1 } else { -1 };
                    let pvx = self.player.vx;
                    let from_top = self.player.vy < 0.0 && self.player.y >= a.y + 8.0;
                    self.actors[i].kick_bobomb(dir, pvx);
                    if from_top {
                        self.player.bounce(jump_held);
                    }
                    self.events.push(Event::Kick);
                }
                i += 1;
                continue;
            }
            if a.kind == Kind::Magikoopa && a.phase == 0 {
                i += 1;
                continue; // hidden
            }
            if a.kind == Kind::RockyWrench && a.heading < 6.0 {
                i += 1;
                continue; // down the manhole
            }
            if a.kind == Kind::Wiggler
                && a.wiggler_segments()
                    .iter()
                    .any(|&(sx, sy)| overlap(hurt, (sx - 7.0, sy, sx + 7.0, sy + 14.0)))
                && !star
                && self.player.hurt()
            {
                self.events.push(if self.player.alive() {
                    Event::PowerDown
                } else {
                    Event::Death
                });
                return;
            }
            let harmful = a.kind.is_enemy()
                || matches!(
                    a.kind,
                    Kind::EnemyFire | Kind::Hammer | Kind::Magic | Kind::Wrench
                );
            if !harmful || !overlap(hurt, ab) {
                i += 1;
                continue;
            }
            let p = &self.player;
            let (ax, ay, ah) = (a.x, a.y, a.height());
            if star && a.kind != Kind::EnemyFire {
                let px = p.x;
                self.actors[i].die_fall(px);
                let n = self.player.stomp_chain;
                self.player.stomp_chain += 1;
                self.add_score(Self::chain_score(n), ax, ay + 24.0);
                self.events.push(Event::EnemyDown);
                i += 1;
                continue;
            }
            // stomp: falling with the feet in the enemy's upper half [inf: window not traced]
            let stomp = p.vy < 0.0 && p.y >= ay + ah * 0.5 && !a.kind.unstompable();
            let still_shell = a.mode == Mode::ShellWait;
            if still_shell {
                let dir = if ax >= p.x { 1 } else { -1 };
                let pvx = p.vx;
                self.actors[i].kick(dir, pvx);
                if stomp {
                    self.player.bounce(jump_held);
                }
                self.add_score(KICK_SCORE, ax, ay + 24.0);
                self.events.push(Event::Kick);
            } else if stomp {
                let was_lakitu = self.actors[i].kind == Kind::Lakitu;
                if self.actors[i].stomp() {
                    if was_lakitu {
                        // the empty cloud stays, rideable
                        let mut c = Actor::new(Kind::Cloud, ax, ay);
                        c.home_y = ay;
                        c.timer = -1;
                        self.add_actor(c);
                    }
                    self.player.bounce(jump_held);
                    let n = self.player.stomp_chain;
                    self.player.stomp_chain += 1;
                    self.add_score(Self::chain_score(n), ax, ay + 24.0);
                    self.events.push(Event::Stomp);
                }
            } else if self.actors[i].mode == Mode::ShellSlide && self.actors[i].kick_lock > 0 {
                // the kicker's no-hit window
            } else if self.player.hurt() {
                if self.player.alive() {
                    self.events.push(Event::PowerDown);
                } else {
                    self.events.push(Event::Death);
                }
                return;
            }
            i += 1;
        }
    }

    fn collect_item(&mut self, kind: Kind, x: f32, y: f32) {
        match kind {
            // the power-up sound plays for every pickup, also when the form does not change
            Kind::Mushroom => {
                self.player.power_up(Form::Super);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            Kind::FireFlower => {
                self.player.power_up(Form::Fire);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            Kind::Star => {
                self.player.star = STAR_FRAMES;
                self.events.push(Event::PowerUp);
            }
            Kind::OneUp => {
                self.add_score(ONE_UP, x, y + 16.0);
            }
            _ => {}
        }
    }

    fn step_effects(&mut self) {
        let mut credited = 0;
        self.effects.retain_mut(|e| {
            let alive = e.step();
            if !alive && matches!(e, Effect::CoinPop { .. }) {
                credited += 1;
            }
            alive
        });
        for _ in 0..credited {
            self.coins += 1;
            self.score += COIN_SCORE;
            if self.coins >= 100 {
                self.coins -= 100;
                self.add_life();
            }
        }
    }

    fn view_rect(&self, margin: f32) -> (f32, f32, f32, f32) {
        (
            self.camera_x - margin,
            self.camera_y - margin,
            self.camera_x + VIEW_W + margin,
            self.camera_y + VIEW_H + margin,
        )
    }

    /// Spawn course actors whose home comes into view (NSMB-style respawn: an actor that left
    /// the area comes back the next time its home enters the view) [inf, shared draft §1.5].
    fn activate(&mut self) {
        let v = self.view_rect(ACTIVATE_MARGIN);
        for idx in 0..self.spawns.len() {
            let rec = &self.spawns[idx];
            let s = rec.spawn;
            let visible = s.x >= v.0 && s.x <= v.2 && s.y >= v.1 - 32.0 && s.y <= v.3;
            let spawn_now = visible && !rec.was_visible && !rec.alive && !rec.killed;
            self.spawns[idx].was_visible = visible;
            if spawn_now {
                let mut a = Actor::new(s.kind, s.x, s.y);
                a.big = s.big;
                a.wings = s.wings;
                a.spawn = Some(idx);
                a.home_y = s.y;
                a.pipe = s.pipe;
                if s.parachute {
                    a.mode = Mode::Parachute {
                        x0: s.x,
                        phase: 0.0,
                    };
                }
                a.rng ^= (idx as u32 + 1).wrapping_mul(0x85eb_ca6b);
                if s.kind == Kind::RockyWrench {
                    a.home_y = s.y - 8.0;
                }
                if s.kind == Kind::Magikoopa {
                    a.timer = 30;
                }
                if s.kind == Kind::Claw {
                    // pivot at the top centre of the 3×4 object
                    a.home_x = s.x;
                    a.home_y = s.y + 64.0;
                }
                // enemies face the player (left if player.x ≤ x); items face right; a Twister
                // starts facing left
                a.dir = if (s.kind.is_enemy() && self.player.x <= s.x) || s.kind == Kind::Twister {
                    -1
                } else {
                    1
                };
                self.add_actor(a);
                self.spawns[idx].alive = true;
            }
        }
    }

    /// Remove actors whose activation box left the view; dead ones stay dead.
    fn despawn(&mut self) {
        let v = self.view_rect(ACTIVATE_MARGIN);
        for a in &mut self.actors {
            let (hx, hy) = match (a.mode, a.big) {
                (Mode::ShellSlide, _) => (160.0, 32.0),
                (_, true) => (48.0, 48.0),
                _ => (32.0, 32.0),
            };
            let (cx, cy) = (a.x, a.y + a.height() / 2.0);
            let outside = cx + hx < v.0 || cx - hx > v.2 || cy + hy < v.1 || cy - hy > v.3;
            if outside && a.mode != Mode::Dead {
                a.mode = Mode::Dead;
                if let Some(s) = a.spawn {
                    self.spawns[s].alive = false;
                }
            } else if a.mode == Mode::Dead
                && let Some(s) = a.spawn
            {
                self.spawns[s].alive = false;
                self.spawns[s].killed = true;
            }
        }
        // killed actors (stomped, knocked off) do not respawn
        for a in &self.actors {
            if matches!(a.mode, Mode::Stomped | Mode::DieFall)
                && let Some(s) = a.spawn
            {
                self.spawns[s].killed = true;
            }
        }
    }

    // ---------------------------------------------------------------- pipe generators

    /// Pipes with something inside keep producing it while on screen (EnemyWaki, 9-pipe.md):
    /// first delay / interval / retry by pipe colour, at most `spout` alive.
    fn step_generators(&mut self) {
        let v = self.view_rect(0.0);
        for gi in 0..self.level.generators.len() {
            let g = self.level.generators[gi];
            let c = g.colour as usize & 3;
            let on_screen =
                g.x >= v.0 - 16.0 && g.x <= v.2 + 16.0 && g.y >= v.1 - 16.0 && g.y <= v.3 + 16.0;
            if !on_screen {
                self.gen_timers[gi] = GEN_FIRST[c];
                continue;
            }
            self.gen_timers[gi] -= 1;
            if self.gen_timers[gi] > 0 {
                continue;
            }
            let alive = self
                .actors
                .iter()
                .filter(|a| a.generator == Some(gi) && a.mode != Mode::Dead)
                .count();
            let limit = match g.kind {
                Kind::Koopa { red: true } => 1,
                _ => 10,
            };
            // something sitting in front of the mouth blocks the spawn
            let probe = match g.dir {
                PipeDir::Up => (g.x - 8.0, g.y, g.x + 8.0, g.y + 15.0),
                PipeDir::Down => (g.x - 8.0, g.y - 15.0, g.x + 8.0, g.y),
                PipeDir::Right => (g.x, g.y - 8.0, g.x + 15.0, g.y + 8.0),
                PipeDir::Left => (g.x - 15.0, g.y - 8.0, g.x, g.y + 8.0),
            };
            let blocked = self
                .actors
                .iter()
                .any(|a| a.mode != Mode::Dead && overlap(a.bbox(), probe));
            if alive >= limit || blocked {
                self.gen_timers[gi] = GEN_RETRY[c];
                continue;
            }
            self.gen_timers[gi] = GEN_INTERVAL[c];
            let mut a = Actor::new(g.kind, g.x, g.y);
            a.big = g.big;
            a.wings = g.wings;
            a.generator = Some(gi);
            a.pipe = Some((g.dir, g.x, g.y));
            let h = a.height();
            let (sx, sy, tx, ty) = match g.dir {
                PipeDir::Up => (g.x, g.y - h, g.x, g.y),
                PipeDir::Down => (g.x, g.y + 1.0, g.x, g.y - h),
                PipeDir::Right => (
                    g.x - 20.0,
                    g.y - if h > 16.0 { 14.0 } else { 8.0 },
                    g.x + 7.9,
                    g.y - if h > 16.0 { 14.0 } else { 8.0 },
                ),
                PipeDir::Left => (
                    g.x + 20.0,
                    g.y - if h > 16.0 { 14.0 } else { 8.0 },
                    g.x - 7.9,
                    g.y - if h > 16.0 { 14.0 } else { 8.0 },
                ),
            };
            a.x = sx;
            a.y = sy;
            a.dir = match g.dir {
                PipeDir::Left => -1,
                PipeDir::Right => 1,
                _ => {
                    if self.player.x <= g.x {
                        -1
                    } else {
                        1
                    }
                }
            };
            a.mode = Mode::DokanOut { tx, ty };
            self.add_actor(a);
        }
    }

    // ---------------------------------------------------------------- explosions

    /// Bob-omb explosions (15-bob-omb.md): break breakable blocks in a plus shape around the
    /// bomb, knock out enemies and hurt the player nearby. Radius [inf]: 1 tile (plus) for
    /// terrain, 32 px for actors and the player.
    fn explosions(&mut self) {
        let bombs: Vec<(f32, f32)> = self
            .actors
            .iter()
            .filter(|a| a.kind == Kind::BobOmb && a.phase == 9 && a.mode != Mode::Dead)
            .map(|a| (a.x, a.y + 8.0))
            .collect();
        for &(bx, by) in &bombs {
            for a in &mut self.actors {
                if a.kind == Kind::BobOmb && a.phase == 9 {
                    a.mode = Mode::Dead;
                    continue;
                }
                let (cx, cy) = (a.x, a.y + a.height() / 2.0);
                if a.kind.is_enemy()
                    && a.alive()
                    && (cx - bx).abs() < 32.0
                    && (cy - by).abs() < 32.0
                {
                    a.die_fall(bx);
                }
            }
            let (tx, ty) = ((bx / TILE).floor() as i32, (by / TILE).floor() as i32);
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (x, y) = (tx + dx, ty + dy);
                if self
                    .blocks
                    .get(x, y)
                    .is_some_and(|b| matches!(b.kind, BlockKind::Brick | BlockKind::Hard))
                {
                    self.blocks.map.remove(&(x, y));
                    self.level.set(x, y, Cell::Empty);
                    let (ox, oy) = blocks::origin(x, y);
                    self.effects.extend(Effect::debris(ox, oy, 0.25, 0.1));
                }
            }
            let p = &self.player;
            if (p.x - bx).abs() < 32.0 && (p.y + 8.0 - by).abs() < 32.0 && self.player.hurt() {
                self.events.push(if self.player.alive() {
                    Event::PowerDown
                } else {
                    Event::Death
                });
            }
            self.events.push(Event::Break);
        }
    }

    /// Bumper (82-bumper.md): circle of half-size 22 around (x, y + 24). Hit classified by the
    /// angle from its centre to the player's centre: top → vy 6.0 (9.0 with jump held), vx
    /// 0.5 + 0.6·|vx| away; bottom → 1.6 × the unit vector away; side → vx 2.0 away, vy 3.0 /
    /// −0.5 / 5.0 by the player's vy. 16-frame cooldown.
    fn bumper_contact(&mut self, i: usize, jump_held: bool) {
        let a = &self.actors[i];
        if a.timer > 0 {
            return;
        }
        let (cx, cy) = (a.x, a.y + 24.0);
        let p = &self.player;
        let (px, py) = (p.x, p.y + 8.0);
        let (dx, dy) = (px - cx, py - cy);
        if dx * dx + dy * dy > 30.0 * 30.0 {
            return;
        }
        let ang = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        let away = if dx < 0.0 { -1.0 } else { 1.0 };
        let top = if p.vy > 0.0 {
            (45.0..=135.0).contains(&ang)
        } else {
            (22.5..=157.5).contains(&ang)
        };
        let bottom = (202.5..=337.5).contains(&ang);
        let (vx, vy) = if top {
            (
                away * (0.5 + 0.6 * p.vx.abs()),
                if jump_held { 9.0 } else { 6.0 },
            )
        } else if bottom {
            let n = (dx * dx + dy * dy).sqrt().max(0.001);
            (1.6 * dx / n, 1.6 * dy / n)
        } else {
            let vy = if p.vy > 0.0 {
                3.0
            } else if p.vy > -5.5 {
                -0.5
            } else {
                5.0
            };
            (2.0 * away, vy)
        };
        self.player.vx = vx;
        self.player.vy = vy;
        self.player.on_ground = false;
        self.actors[i].timer = 16;
        self.events.push(Event::Bump);
    }

    // ---------------------------------------------------------------- burners, fire bars

    /// Burner flame state from the shared 372-frame counter (54-burner.md): returns the flame
    /// length in px (0 when off). Fire 18–136 (full), FireStart 0–17 (growing), FireFinish
    /// 137–155 (shrinking); the alternate form runs half a cycle later.
    pub fn burner_flame(&self, alt: bool) -> f32 {
        let t = ((self.tick as i64 + if alt { 186 } else { 0 }) % 372) as f32;
        if t < 18.0 {
            48.0 * (t + 1.0) / 18.0
        } else if t <= 136.0 {
            48.0
        } else if t <= 155.0 {
            48.0 * (155.0 - t) / 19.0
        } else {
            0.0
        }
    }

    /// Fire Bar angle (degrees). The shared area counter's per-frame step was not located
    /// [?]; 2.8125°/frame (one turn per 128 frames) is used [inf].
    pub fn firebar_angle(&self, fb: &crate::level::FireBar) -> f32 {
        fb.start + fb.dir * 2.8125 * self.tick as f32
    }

    /// Swinging claws (105-swinging-claw.md): grab the player when the claw tip is within 46 px
    /// and the vertical offset is in (−8, 10); the player hangs from the tip until jump is
    /// pressed, then leaves with the tip's velocity; 30-frame re-grab cooldown.
    fn claws(&mut self, input: Input) {
        if let Some(id) = self.held_by {
            let Some(c) = self.actors.iter().find(|a| a.id == id) else {
                self.held_by = None;
                self.player.control = Control::Normal;
                return;
            };
            let (tx, ty) = c.claw_tip();
            let (vx, vy) = (tx - self.player.x, ty - 24.0 - self.player.y);
            self.player.x = tx;
            self.player.y = ty - 24.0;
            if input.held(input::JUMP) && !self.prev_jump_claw {
                self.player.control = Control::Normal;
                self.player.vx = vx.clamp(-3.0, 3.0);
                self.player.vy = vy.max(0.0) + 3.0;
                self.player.on_ground = false;
                if let Some(c) = self.actors.iter_mut().find(|a| a.id == id) {
                    c.timer = 30;
                }
                self.held_by = None;
            }
        } else if self.player.control == Control::Normal {
            let p = &self.player;
            let grab = self.actors.iter().find(|a| {
                if a.kind != Kind::Claw || a.timer > 0 {
                    return false;
                }
                let (tx, ty) = a.claw_tip();
                let dy = p.y + 16.0 - (ty - 8.0);
                (p.x - tx).abs() < 23.0 && dy > -8.0 && dy < 10.0
            });
            if let Some(c) = grab {
                self.held_by = Some(c.id);
                self.player.control = Control::Demo;
                self.player.vx = 0.0;
                self.player.vy = 0.0;
                self.player.pose = Pose::Jump;
            }
        }
        self.prev_jump_claw = input.held(input::JUMP);
    }

    /// Magikoopa (39-magikoopa.md): hidden 60–75 frames → appears near the view centre (x
    /// ± 16k px, y on the floor below a random height) → after 30–60 frames casts one magic shot
    /// (0.75 px/frame toward the player, within ±45° of its facing) → 30–60 frames → vanishes.
    /// `phase`: 0 hidden, 1 shown, 2 cast done. Hidden Magikoopas don't collide.
    fn magikoopas(&mut self) {
        let (cx, cy) = (self.camera_x + VIEW_W / 2.0, self.camera_y + VIEW_H / 2.0);
        let (px, py) = (self.player.x, self.player.y);
        let mut casts = Vec::new();
        for idx in 0..self.actors.len() {
            if self.actors[idx].kind != Kind::Magikoopa || !self.actors[idx].alive() {
                continue;
            }
            let r1 = self.rand();
            let r2 = self.rand();
            let a = &mut self.actors[idx];
            a.timer -= 1;
            match a.phase {
                0 => {
                    a.mode = Mode::Normal;
                    if a.timer <= 0 {
                        // pick a spot: x within ±144 of the view centre, on the floor below
                        let k = (r1 * 9.0).floor() * 16.0 * if r2 < 0.5 { -1.0 } else { 1.0 };
                        let x = cx + k.clamp(-144.0, 144.0);
                        let mut y = cy + (r2 - 0.5) * 160.0;
                        let mut tries = 0;
                        while !self.level.cell_at(x, y - 1.0).is_solid() && y > 0.0 && tries < 40 {
                            y = ((y - 1.0) / TILE).floor() * TILE;
                            tries += 1;
                        }
                        a.x = x;
                        a.y = y;
                        a.phase = 1;
                        a.timer = 30 + (r1 * 30.0) as i32;
                        a.dir = if px < a.x { -1 } else { 1 };
                    }
                }
                1 => {
                    if a.timer <= 0 {
                        let face = if px < a.x { 180.0f32 } else { 0.0 };
                        let want = (py + 8.0 - (a.y + 16.0)).atan2(px - a.x).to_degrees();
                        let mut d = want - face;
                        while d > 180.0 {
                            d -= 360.0;
                        }
                        while d < -180.0 {
                            d += 360.0;
                        }
                        let ang = (face + d.clamp(-45.0, 45.0)).to_radians();
                        casts.push((a.x + a.dir as f32 * 8.0, a.y + 12.0, ang));
                        a.phase = 2;
                        a.timer = 30 + (r2 * 30.0) as i32;
                    }
                }
                _ => {
                    if a.timer <= 0 {
                        a.phase = 0;
                        a.timer = 60 + (r1 * 16.0) as i32;
                    }
                }
            }
        }
        for (x, y, ang) in casts {
            let mut m = Actor::new(Kind::Magic, x, y);
            m.vx = 0.75 * ang.cos();
            m.vy = 0.75 * ang.sin();
            m.timer = 600;
            self.add_actor(m);
        }
    }

    /// Twister wind column: x within ±16 of the twister, from its feet to the column top. The
    /// player is lifted (vy toward 4.5 at +0.6/frame) [inf: lift speeds live in the player's
    /// tornado state, not extracted].
    fn twister_lift(&mut self) {
        let p = &self.player;
        let inside = self.actors.iter().any(|a| {
            a.kind == Kind::Twister
                && (p.x - a.x).abs() < 16.0
                && p.y >= a.y - 4.0
                && p.y <= a.y + a.heading
        });
        if inside && self.player.control == Control::Normal {
            self.player.vy = (self.player.vy + 0.6).min(4.5);
            self.player.on_ground = false;
        }
    }

    /// POW block hit from below: every grounded enemy in view is knocked out; the POW is used up.
    fn pow_quake(&mut self) {
        let Some(id) = self.player.head_solid else {
            return;
        };
        let Some(i) = self
            .actors
            .iter()
            .position(|a| a.id == id && a.kind == Kind::Pow)
        else {
            return;
        };
        self.actors[i].mode = Mode::Dead;
        let v = self.view_rect(16.0);
        let px = self.player.x;
        for a in &mut self.actors {
            if a.kind.is_enemy()
                && a.alive()
                && a.on_ground
                && a.x >= v.0
                && a.x <= v.2
                && a.y >= v.1
                && a.y <= v.3
            {
                a.die_fall(px);
            }
        }
        self.events.push(Event::Break);
    }

    /// Cannons (47-cannon.md): first shot 60 frames after spawn, then every 190, while the
    /// muzzle is on screen; the ball flies 1 px/frame along the barrel.
    fn step_cannons(&mut self) {
        let v = self.view_rect(0.0);
        for ci in 0..self.level.cannons.len() {
            let c = self.level.cannons[ci];
            let a = c.angle.to_radians();
            let (mx, my) = (c.x + 8.0 * a.cos(), c.y + 8.0 + 8.0 * a.sin());
            if mx < v.0 || mx > v.2 || my < v.1 || my > v.3 {
                continue;
            }
            self.cannon_timers[ci] -= 1;
            if self.cannon_timers[ci] > 0 {
                continue;
            }
            self.cannon_timers[ci] = 190;
            let mut b = Actor::new(Kind::Cannonball, mx, my - 8.0);
            b.vx = a.cos();
            b.vy = a.sin();
            self.add_actor(b);
            self.events.push(Event::Kick);
        }
    }

    /// Hurt the player on Burner flames and Fire Bar fireballs.
    fn fire_hazards(&mut self) {
        let hb = self.player.hurt_box();
        let mut hit = false;
        for b in &self.level.burners {
            let len = self.burner_flame(b.alt);
            if len <= 0.0 {
                continue;
            }
            // flame column 12 px wide starting at the block face [inf width]
            let r = match b.dir {
                3 => (b.x - 6.0, b.y + 16.0, b.x + 6.0, b.y + 16.0 + len),
                2 => (b.x - 6.0, b.y - len, b.x + 6.0, b.y),
                1 => (b.x + 8.0, b.y + 2.0, b.x + 8.0 + len, b.y + 14.0),
                _ => (b.x - 8.0 - len, b.y + 2.0, b.x - 8.0, b.y + 14.0),
            };
            hit |= overlap(hb, r);
        }
        for fb in &self.level.firebars {
            let a = self.firebar_angle(fb).to_radians();
            for i in 1..fb.n {
                let (fx, fy) = (
                    fb.x + i as f32 * 8.0 * a.cos(),
                    fb.y + i as f32 * 8.0 * a.sin(),
                );
                hit |= overlap(hb, (fx - 4.0, fy - 4.0, fx + 4.0, fy + 4.0));
            }
        }
        if hit && self.player.star == 0 && self.player.hurt() {
            self.events.push(if self.player.alive() {
                Event::PowerDown
            } else {
                Event::Death
            });
        }
    }

    // ---------------------------------------------------------------- platforms

    /// Moving solids step before the player; riders are carried by their movement. A spring
    /// launches its rider: 6.0, or 4.43 with the jump button held (the held-jump gravity then
    /// gives the high bounce) (10-spring.md).
    fn step_platforms(&mut self, jump_held: bool, dir: i32) {
        let (px, py) = (self.player.x, self.player.y);
        let ride = self.player.ride;
        for a in &mut self.actors {
            if !a.kind.is_platform() {
                continue;
            }
            a.target = (px, py);
            a.ridden = ride == Some(a.id);
            if a.kind == Kind::Cloud {
                a.target_facing = dir; // the rider steers the cloud
            }
            a.step_spawning(&self.level);
        }
        self.level.solids = self
            .actors
            .iter()
            .filter_map(|a| {
                a.solid_box()
                    .map(|(x0, y0, x1, y1, top_only)| crate::level::Solid {
                        x0,
                        y0,
                        x1,
                        y1,
                        dx: a.moved.0,
                        dy: a.moved.1,
                        top_only,
                        owner: a.id,
                        slope: a.solid_slope(),
                    })
            })
            .collect();
        // snake blocks
        for (si, sn) in self.snakes.iter_mut().enumerate() {
            let base = SNAKE_OWNER + si as u32 * 32;
            let ridden = ride.is_some_and(|r| r >= base && r < base + 32);
            sn.step(ridden);
            for (k, (cx, cy)) in sn.segments().into_iter().enumerate() {
                let (dx, dy) = sn.moved(k);
                self.level.solids.push(crate::level::Solid {
                    x0: cx - 8.0,
                    y0: cy - 8.0,
                    x1: cx + 8.0,
                    y1: cy + 8.0,
                    dx,
                    dy,
                    top_only: false,
                    owner: base + k as u32,
                    slope: 0.0,
                });
            }
        }
        self.snakes.retain(|s| !s.gone());
        if let Some(id) = ride {
            if let Some(s) = self.level.solids.iter().find(|s| s.owner == id) {
                self.player.x += s.dx;
                self.player.y = s.top_at(self.player.x);
            }
            let launch = self
                .actors
                .iter()
                .any(|a| a.id == id && a.kind == Kind::Spring && a.chain == 1);
            if launch {
                self.player.vy = if jump_held { 4.43 } else { 6.0 };
                self.player.on_ground = false;
                self.player.ride = None;
                self.events.push(Event::Jump);
            }
        }
    }

    /// Bill Blasters fire toward the player (13-bill-blaster.md): the first shot 60 frames after
    /// the muzzle is on screen with no player at the blaster, then every 190 frames (random
    /// extras 0–59 are [inf] seeded per blaster).
    fn step_blasters(&mut self) {
        let v = self.view_rect(0.0);
        for bi in 0..self.level.blasters.len() {
            let b = self.level.blasters[bi];
            let h = b.h as f32 * TILE;
            let dir = if self.player.x < b.x { -1 } else { 1 };
            let mx = b.x + dir as f32 * 8.0;
            let my = b.y + h - 16.0;
            let on_screen = mx >= v.0 && mx <= v.2 && my >= v.1 && my <= v.3;
            let p = &self.player;
            let at_blaster =
                (p.x - b.x).abs() < 24.0 && (p.y - (b.y + h / 2.0)).abs() < h / 2.0 + 20.0;
            let (main, cool) = &mut self.blaster_timers[bi];
            if !on_screen || at_blaster {
                *main = 60;
                continue;
            }
            *main -= 1;
            *cool -= 1;
            if *main > 0 || *cool > 0 {
                continue;
            }
            *cool = 190;
            let alive = self
                .actors
                .iter()
                .filter(|a| a.kind == Kind::BulletBill)
                .count();
            let blocked = self.level.cell_at(mx + dir as f32 * 15.0, my).is_solid();
            if alive >= 10 || blocked {
                continue;
            }
            let kind = b.contents.unwrap_or(Kind::BulletBill);
            let mut a = Actor::new(kind, mx, my - 8.0);
            a.dir = dir;
            if kind == Kind::BulletBill {
                a.vx = dir as f32;
            }
            self.add_actor(a);
            self.events.push(Event::Kick);
        }
    }

    // ---------------------------------------------------------------- switches, donuts, belts

    /// Set dotted-line block solidity from the ON/OFF state (red solid when ON).
    fn apply_on_off(&mut self) {
        let on = self.on_off;
        let cells: Vec<((i32, i32), bool)> = self
            .blocks
            .map
            .iter()
            .filter_map(|(&k, b)| match b.kind {
                BlockKind::Dotted { blue } => Some((k, blue != on)),
                _ => None,
            })
            .collect();
        for ((x, y), solid) in cells {
            self.level
                .set(x, y, if solid { Cell::Solid } else { Cell::Empty });
        }
    }

    /// P switch pressed: bricks and coins swap for P_FRAMES (18-p-switch.md).
    fn press_p_switch(&mut self) {
        self.events.push(Event::Switch);
        if self.p_timer > 0 {
            self.p_timer = P_FRAMES;
            return;
        }
        self.p_timer = P_FRAMES;
        let bricks: Vec<(i32, i32)> = self
            .blocks
            .map
            .iter()
            .filter(|(_, b)| b.kind == BlockKind::Brick && b.contents.is_none() && b.bump.is_none())
            .map(|(&k, _)| k)
            .collect();
        let coins: Vec<(i32, i32)> = self.level.coins.iter().map(|c| (c.x, c.y)).collect();
        self.level.coins.clear();
        for &(x, y) in &bricks {
            self.blocks.map.remove(&(x, y));
            self.level.set(x, y, Cell::Empty);
            self.level.coins.push(CoinTile { x, y });
        }
        for &(x, y) in &coins {
            self.blocks
                .map
                .insert((x, y), Block::new(BlockKind::Brick, None));
            self.level.set(x, y, Cell::Solid);
        }
        self.p_bricks = bricks;
        self.p_coins = coins;
    }

    fn step_p_switch(&mut self) {
        if self.p_timer <= 0 {
            return;
        }
        self.p_timer -= 1;
        if self.p_timer > 0 {
            return;
        }
        // back: remaining P coins become bricks again, unbroken P bricks become coins
        for (x, y) in std::mem::take(&mut self.p_bricks) {
            if let Some(i) = self.level.coins.iter().position(|c| (c.x, c.y) == (x, y)) {
                self.level.coins.remove(i);
                self.blocks
                    .map
                    .insert((x, y), Block::new(BlockKind::Brick, None));
                self.level.set(x, y, Cell::Solid);
            }
        }
        for (x, y) in std::mem::take(&mut self.p_coins) {
            if self
                .blocks
                .get(x, y)
                .is_some_and(|b| b.kind == BlockKind::Brick)
            {
                self.blocks.map.remove(&(x, y));
                self.level.set(x, y, Cell::Empty);
                self.level.coins.push(CoinTile { x, y });
            }
        }
    }

    /// Donut blocks drop after 48 frames of being stood on and come back 120 frames after
    /// falling out of the area (21-donut-block.md).
    fn step_donuts(&mut self) {
        let p = &self.player;
        let under = if p.on_ground {
            [-4.9f32, 4.9].iter().find_map(|&fx| {
                let (tx, ty) = (
                    ((p.x + fx) / TILE).floor() as i32,
                    ((p.y - 1.0) / TILE).floor() as i32,
                );
                self.blocks
                    .get(tx, ty)
                    .is_some_and(|b| b.kind == BlockKind::Donut)
                    .then_some((tx, ty))
            })
        } else {
            None
        };
        self.donut_ride = match (under, self.donut_ride) {
            (Some(t), Some((r, n))) if t == r => Some((t, n + 1)),
            (Some(t), _) => Some((t, 1)),
            _ => None,
        };
        if let Some(((x, y), n)) = self.donut_ride
            && n >= 48
        {
            self.blocks.map.remove(&(x, y));
            self.level.set(x, y, Cell::Empty);
            let (bx, by) = blocks::origin(x, y);
            let mut a = Actor::new(Kind::Donut, bx, by);
            a.home_x = x as f32;
            a.home_y = y as f32;
            self.add_actor(a);
            self.donut_ride = None;
        }
        for a in &mut self.actors {
            if a.kind == Kind::Donut && a.y < -2.0 * TILE && a.mode != Mode::Dead {
                a.mode = Mode::Dead;
                self.donut_respawn
                    .push(((a.home_x as i32, a.home_y as i32), 120));
            }
        }
        let mut back = Vec::new();
        self.donut_respawn.retain_mut(|(t, n)| {
            *n -= 1;
            if *n <= 0 {
                back.push(*t);
                false
            } else {
                true
            }
        });
        for (x, y) in back {
            self.blocks
                .map
                .insert((x, y), Block::new(BlockKind::Donut, None));
            self.level.set(x, y, Cell::SemiTop);
        }
    }

    /// Conveyor carry: toward the belt speed by 0.2/frame while standing on it; off the belt the
    /// carry stops at once (53-conveyor-belt.md).
    fn conveyor_carry(&mut self) {
        let p = &self.player;
        let target = if p.on_ground {
            let (tx, ty) = (
                (p.x / TILE).floor() as i32,
                ((p.y - 1.0) / TILE).floor() as i32,
            );
            self.level
                .conveyors
                .iter()
                .find(|c| ty == c.2 && tx >= c.0 && tx <= c.1)
                .map_or(0.0, |c| c.3)
        } else {
            0.0
        };
        self.carry = if target == 0.0 {
            0.0
        } else if self.carry < target {
            (self.carry + 0.2).min(target)
        } else {
            (self.carry - 0.2).max(target)
        };
        self.player.x += self.carry;
        // walkers on belts move with them
        let belts = self.level.conveyors.clone();
        for a in &mut self.actors {
            if !a.on_ground || !a.kind.is_walker() {
                continue;
            }
            let (tx, ty) = (
                (a.x / TILE).floor() as i32,
                ((a.y - 1.0) / TILE).floor() as i32,
            );
            if let Some(c) = belts.iter().find(|c| ty == c.2 && tx >= c.0 && tx <= c.1) {
                a.x += c.3;
            }
        }
    }

    // ---------------------------------------------------------------- checkpoints

    /// Touching a checkpoint flag records the respawn point, scores 2000 and gives its power-up
    /// (90-checkpoint-flag.md). Touch box x ∈ [−14, −2], y ∈ [3, 29] from the flag origin.
    fn check_checkpoints(&mut self) {
        let b = self.player.contact_box();
        for (i, cp) in self.level.checkpoints.clone().into_iter().enumerate() {
            if self.checkpoint == Some((self.area, i)) {
                continue;
            }
            if overlap(b, (cp.x - 14.0, cp.y + 3.0, cp.x - 2.0, cp.y + 29.0)) {
                self.checkpoint = Some((self.area, i));
                self.add_score(2000, cp.x, cp.y + 32.0);
                self.events.push(Event::Checkpoint);
                match cp.contents {
                    Some(Contents::Mushroom) => {
                        self.player.power_up(Form::Super);
                    }
                    Some(Contents::FireFlower) => {
                        self.player.power_up(Form::Fire);
                    }
                    Some(Contents::Star) => self.player.star = STAR_FRAMES,
                    _ => {}
                }
            }
        }
    }

    // ---------------------------------------------------------------- pipes and doors

    fn step_transit(&mut self) {
        let Some(mut t) = self.transit else { return };
        if warp::step(&mut t, &mut self.player) {
            self.switch_area(t.to_area);
            self.reset_camera();
            self.activate();
        }
        self.fade = warp::fade(&t);
        self.player.hidden = warp::hidden(&t);
        if warp::finished(&t) {
            self.transit = None;
            self.player.control = Control::Normal;
            self.player.hidden = false;
            self.player.on_ground = true;
            self.fade = 0.0;
            if let crate::level::WarpKind::Pipe(dir) = t.to.kind {
                match dir {
                    crate::level::PipeDir::Left => self.player.facing = -1,
                    crate::level::PipeDir::Right => self.player.facing = 1,
                    _ => {}
                }
            }
        } else {
            self.transit = Some(t);
        }
    }

    /// The player is travelling through a pipe (drawn behind terrain).
    pub fn in_pipe(&self) -> bool {
        self.transit
            .is_some_and(|t| matches!(t.phase, warp::Phase::In | warp::Phase::Out))
    }

    // ---------------------------------------------------------------- goal

    fn check_goal(&mut self) {
        let Some((x0, y0)) = self.level.goal else {
            return;
        };
        let p = &self.player;
        if !p.alive() {
            return;
        }
        // pole touch box x ∈ [X0−4, X0+5], y ∈ [Y0+14, Y0+166]; tested against body box B
        // [inf: box A of a small player on the base block only touches Y0+14]
        let hb = p.contact_box();
        if !overlap(hb, (x0 - 4.0, y0 + 14.0, x0 + 5.0, y0 + 166.0)) {
            return;
        }
        // grab: snap to the pole, score by height
        let top = y0 + 144.0;
        let mut y = p.y;
        if y >= top {
            y = top + 5.0;
        }
        let h = y - y0;
        let d = 150.0 - h;
        let index = if d <= 6.0 {
            0
        } else {
            (((d - 6.0) / 32.0).floor() as usize + 1).min(4)
        };
        let award = [ONE_UP, 4000, 800, 200, 100][index];
        self.player.control = Control::Demo;
        self.player.x = x0 - 6.0;
        self.player.y = y;
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.player.facing = 1;
        self.player.pose = Pose::Pole;
        self.player.star = 0;
        self.add_score(award, x0 + 16.0, y);
        self.goal_phase = GoalPhase::Hold { t: 10 };
        self.events.push(Event::Goal);
    }

    fn step_goal(&mut self) {
        let Some((x0, y0)) = self.level.goal else {
            return;
        };
        let target_x = x0 + 112.0;
        match self.goal_phase {
            GoalPhase::None | GoalPhase::Done => {}
            GoalPhase::Hold { t } => {
                self.goal_phase = if t <= 1 {
                    GoalPhase::Slide
                } else {
                    GoalPhase::Hold { t: t - 1 }
                };
            }
            GoalPhase::Slide => {
                // player slides at −1.9 to the base block top; the flag drops 1.833 to Y0+16
                self.player.y = (self.player.y - 1.9).max(y0);
                self.goal_flag = (self.goal_flag - 1.833).max(16.0);
                if self.player.y <= y0 && self.goal_flag <= 16.0 {
                    self.player.on_ground = true;
                    self.goal_phase = GoalPhase::Land { t: 10 };
                    self.goal_frames = 0;
                }
            }
            GoalPhase::Land { t } => {
                // M1 GoalLandTurn: flip to the far side of the pole, then walk
                if t == 10 {
                    self.player.x = x0 + 6.0;
                    self.player.facing = -1;
                }
                self.goal_phase = if t <= 1 {
                    self.player.facing = 1;
                    self.player.control = Control::Normal;
                    self.countdown_delay = 90;
                    GoalPhase::Walk
                } else {
                    GoalPhase::Land { t: t - 1 }
                };
            }
            GoalPhase::Walk => {
                self.player.step(
                    Input {
                        buttons: input::RIGHT,
                    },
                    &self.level,
                );
                if self.player.x >= target_x {
                    self.player.hidden = true;
                    self.player.control = Control::Demo;
                    self.goal_phase = GoalPhase::Inside { t: 30 };
                }
            }
            GoalPhase::Inside { t } => {
                self.goal_phase = if t <= 1 {
                    GoalPhase::FlagUp
                } else {
                    GoalPhase::Inside { t: t - 1 }
                };
            }
            GoalPhase::FlagUp => {
                self.tower_flag = (self.tower_flag + 1.0).min(0.0);
                if self.tower_flag >= 0.0 {
                    self.goal_phase = GoalPhase::Finish;
                }
            }
            GoalPhase::Finish => {
                if self.goal_frames >= 340 && !self.countdown && self.time <= 0 {
                    self.goal_phase = GoalPhase::Done;
                    self.cleared = true;
                    self.events.push(Event::Clear);
                }
            }
        }
        if matches!(
            self.goal_phase,
            GoalPhase::Walk | GoalPhase::Inside { .. } | GoalPhase::FlagUp | GoalPhase::Finish
        ) {
            self.goal_frames += 1;
            if self.countdown_delay > 0 {
                self.countdown_delay -= 1;
                if self.countdown_delay == 0 && self.time > 0 {
                    self.countdown = true;
                }
            }
            if self.countdown && self.goal_frames % 2 == 0 {
                self.count_time_step();
            }
        }
    }

    /// Time → score, one step every 2 frames: 50 points per whole second left.
    fn count_time_step(&mut self) {
        let s = self.time / 60;
        let (drain, points) = if s >= 200 {
            (6000, 5000)
        } else if s >= 10 {
            (600, 500)
        } else if s >= 1 {
            (60, 50)
        } else {
            (60, 0)
        };
        self.time = (self.time - drain).max(0);
        self.score += points;
        self.events.push(Event::TimeCount);
        if self.time == 0 {
            self.countdown = false;
        }
    }

    // ---------------------------------------------------------------- camera

    /// Follow camera (camera.rs, research spec camera.md).
    fn update_camera(&mut self) {
        let p = &self.player;
        // frozen while dead and while travelling through a pipe/door (reset at the exit)
        let activity = if !p.alive() || self.transit.is_some() {
            camera::Activity::Frozen
        } else if self.goal_phase != GoalPhase::None {
            camera::Activity::Goal
        } else {
            camera::Activity::Play
        };
        let target = camera::Target {
            x: p.x,
            y: p.y,
            vx: p.vx,
            vy: p.vy,
            on_ground: p.on_ground,
            activity,
        };
        let area = self.camera_area();
        let level = &self.level;
        self.camera
            .update(&area, &target, |x, y| level.cell(x, y) == Cell::Solid);
        self.sync_camera();
    }

    /// Re-centre the camera on the player, as the game does after a respawn or a pipe exit (also
    /// used by the debug teleport).
    pub fn reset_camera(&mut self) {
        let area = self.camera_area();
        let (x, y) = (self.player.x, self.player.y);
        let level = &self.level;
        self.camera.reset(&area, (x, y + 16.0), (x, y), |tx, ty| {
            level.cell(tx, ty) == Cell::Solid
        });
        self.sync_camera();
    }

    fn camera_area(&self) -> camera::Area {
        let l = &self.level;
        camera::Area {
            bounds: camera::Rect::new(l.left, l.bottom, l.right, l.top),
            low_camera: l.boundary_type != 0,
            vertical: l.vertical,
            main: l.main_area,
        }
    }

    fn sync_camera(&mut self) {
        let v = self.camera.view(&self.camera_area());
        self.camera_x = v.left;
        self.camera_y = v.bottom;
    }

    // ---------------------------------------------------------------- drawing

    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        let field = self.field_model;
        let (x0, x1) = (self.camera_x - TILE, self.camera_x + VIEW_W + TILE);
        let (y0, y1) = (self.camera_y - 2.0 * TILE, self.camera_y + VIEW_H + TILE);
        let visible = |px: f32, py: f32| px >= x0 && px <= x1 && py >= y0 && py <= y1;
        let atlas = |idx: u32| {
            Some((
                (idx % 16) as f32 * TILE,
                (idx / 16) as f32 * TILE,
                TILE,
                TILE,
            ))
        };
        let tile = |px: f32, py: f32, idx: u32, layer: u32| Sprite {
            image: SpriteImage {
                model: field,
                texture: field,
                rect: atlas(idx),
            },
            x: px,
            y: py,
            w: TILE,
            h: TILE,
            flip_x: false,
            flip_y: false,
            layer,
        };
        for t in &self.level.tiles {
            let (px, py) = (t.x as f32 * TILE, t.y as f32 * TILE);
            if visible(px, py) {
                let mut s = tile(px, py, t.atlas, 10);
                s.flip_x = t.flip & 1 != 0;
                s.flip_y = t.flip & 2 != 0;
                out.push(s);
            }
        }
        for (&(bx, by), b) in &self.blocks.map {
            let (px, py) = (bx as f32 * TILE, by as f32 * TILE + b.offset());
            if b.kind == BlockKind::Hidden || !visible(px, py) {
                continue;
            }
            let mut s = tile(px, py, switch_atlas(b.kind, self.on_off), 12);
            if let Some(((dx, dy), n)) = self.donut_ride
                && (dx, dy) == (bx, by)
            {
                s.x += [1.0, 2.0, 1.0, 0.0, -1.0, -2.0, -1.0, 0.0][(n as usize) % 8];
            }
            out.push(s);
        }
        for c in &self.level.coins {
            let (px, py) = (c.x as f32 * TILE, c.y as f32 * TILE);
            if visible(px, py) {
                out.push(tile(px, py, 7, 20));
            }
        }
        self.goal_sprites(&mut out);
        for (i, cp) in self.level.checkpoints.iter().enumerate() {
            if !visible(cp.x, cp.y) {
                continue;
            }
            let on = self.checkpoint == Some((self.area, i));
            out.push(Self::model_sprite(
                "M1_Object_middle_flag",
                if on { "wait2.0" } else { "wait.0" },
                cp.x - 16.0,
                cp.y,
                32.0,
                32.0,
                false,
                8,
            ));
        }
        for &(dx, dy, kind) in &self.level.doors {
            if !visible(dx, dy) {
                continue;
            }
            let locked = kind == 2
                && !self.level.warps.iter().any(|w| {
                    w.kind == crate::level::WarpKind::Door(2)
                        && (w.x, w.y) == (dx, dy)
                        && self.unlocked_doors.contains(&(self.area, w.id >> 1))
                });
            let tex = match kind {
                2 if !locked => "wait.0",
                2 => "nokey_wait.0",
                1 if self.p_timer == 0 => "door_dot",
                1 => "door_p",
                _ => "wait.0",
            };
            out.push(Self::model_sprite(
                "M1_Object_door",
                tex,
                dx - 8.0,
                dy,
                16.0,
                32.0,
                false,
                8,
            ));
        }
        // liquid surface strip (16×32 animated tiles) and fill below it
        if let Some(l) = self.liquid
            && !l.spec.full
            && l.y > self.camera_y - 32.0
        {
            let model = match l.spec.kind {
                2 => "M1_Object_magmahalf",
                3 => "M1_Object_poisonhalf",
                _ => "M1_Object_waterhalf",
            };
            let frame = ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4];
            let mut x = (self.camera_x / TILE).floor() * TILE;
            while x < self.camera_x + VIEW_W + TILE {
                let mut s = Self::model_sprite(model, frame, x, l.y - 32.0, 16.0, 32.0, false, 48);
                s.image.rect = Some((0.0, 0.0, 16.0, 32.0));
                out.push(s);
                let mut y = l.y - 48.0;
                while y > self.camera_y - TILE {
                    let mut f = Self::model_sprite(model, "wait.0", x, y, 16.0, 16.0, false, 48);
                    f.image.rect = Some((0.0, 16.0, 16.0, 16.0));
                    out.push(f);
                    y -= TILE;
                }
                x += TILE;
            }
        }
        for a in &self.actors {
            if a.kind != Kind::Claw || !visible(a.home_x, a.home_y) {
                continue;
            }
            out.push(Self::model_sprite(
                "M1_Object_swingcrane",
                "neji",
                a.home_x - 8.0,
                a.home_y - 8.0,
                16.0,
                16.0,
                false,
                30,
            ));
            let (tx, ty) = a.claw_tip();
            for k in 1..4 {
                let f = k as f32 / 4.0;
                let (cx, cy) = (
                    a.home_x + (tx - a.home_x) * f,
                    a.home_y + (ty - a.home_y) * f,
                );
                out.push(Self::model_sprite(
                    "M1_Object_swingcrane",
                    "chain",
                    cx - 8.0,
                    cy,
                    16.0,
                    16.0,
                    false,
                    30,
                ));
            }
            out.push(Self::model_sprite(
                "M1_Object_swingcrane",
                if self.held_by == Some(a.id) {
                    "arm.2"
                } else {
                    "arm.0"
                },
                tx - 24.0,
                ty,
                48.0,
                32.0,
                false,
                31,
            ));
        }
        for a in &self.actors {
            let Kind::Skewer { dir } = a.kind else {
                continue;
            };
            let (x0, y0, x1, y1) = a.skewer_head();
            if !visible((x0 + x1) / 2.0, (y0 + y1) / 2.0) && !visible(a.x, a.y) {
                continue;
            }
            // head (spikes) and the shaft back to the base, vertical skewers only for now
            if dir == 0 || dir == 2 {
                let mut h = Self::model_sprite(
                    "M1_Object_daikonbou",
                    "top",
                    x0,
                    if dir == 0 { y0 } else { y1 - 16.0 },
                    64.0,
                    16.0,
                    false,
                    35,
                );
                h.flip_y = dir == 0;
                out.push(h);
                let (base_y, head_y) = if dir == 0 {
                    (a.y + 64.0, y0 + 16.0)
                } else {
                    (a.y, y1 - 16.0)
                };
                let (lo, hi) = (base_y.min(head_y), base_y.max(head_y));
                let mut y = lo;
                while y < hi {
                    out.push(Self::model_sprite(
                        "M1_Object_daikonbou",
                        "wait_body.0",
                        x0,
                        y,
                        64.0,
                        16.0_f32.min(hi - y),
                        false,
                        34,
                    ));
                    y += 16.0;
                }
            }
        }
        for a in &self.actors {
            if a.kind == Kind::Wiggler && visible(a.x, a.y) {
                let angry = a.phase == 1;
                for (k, (sx, sy)) in a.wiggler_segments().into_iter().enumerate() {
                    let tex = match (angry, (a.age / 8 + k as u32) % 2) {
                        (false, 0) => "walk.0",
                        (false, _) => "walk.1",
                        (true, 0) => "walk_angry.0",
                        (true, _) => "walk_angry.1",
                    };
                    out.push(Self::model_sprite(
                        "M1_Enemy_hanachan",
                        tex,
                        sx - 8.0,
                        sy,
                        16.0,
                        16.0,
                        a.dir > 0,
                        39,
                    ));
                }
            }
            if let Kind::LavaLift { .. } = a.kind
                && visible(a.x, a.y)
            {
                for k in 0..4 {
                    out.push(Self::model_sprite(
                        "M1_Object_lift_yougan",
                        "wait.0",
                        a.x - 32.0 + k as f32 * 16.0,
                        a.y + 8.0,
                        16.0,
                        16.0,
                        false,
                        15,
                    ));
                }
            }
        }
        for sn in &self.snakes {
            let model = if sn.fast {
                "M1_Object_block_snake2"
            } else {
                "M1_Object_block_snake"
            };
            for (cx, cy) in sn.segments() {
                if visible(cx, cy) {
                    out.push(Self::model_sprite(
                        model,
                        "wait.0",
                        cx - 8.0,
                        cy - 8.0,
                        16.0,
                        16.0,
                        false,
                        12,
                    ));
                }
            }
        }
        for c in &self.level.cannons {
            if !visible(c.x, c.y) {
                continue;
            }
            out.push(Self::model_sprite(
                "M1_Enemy_senkan_houdai",
                "senkan_houdai_base",
                c.x - 8.0,
                c.y,
                16.0,
                16.0,
                false,
                11,
            ));
        }
        for b in &self.level.burners {
            if !visible(b.x, b.y) {
                continue;
            }
            out.push(Self::model_sprite(
                "M1_Object_burner",
                "standB",
                b.x - 8.0,
                b.y,
                16.0,
                16.0,
                false,
                11,
            ));
            let len = self.burner_flame(b.alt);
            if len > 0.0 && matches!(b.dir, 2 | 3) {
                let mut s = Self::model_sprite(
                    "M1_Object_burner",
                    if (self.tick / 4).is_multiple_of(2) {
                        "wait.0"
                    } else {
                        "wait.1"
                    },
                    b.x - 8.0,
                    if b.dir == 3 { b.y + 16.0 } else { b.y - 48.0 },
                    16.0,
                    48.0,
                    false,
                    45,
                );
                s.flip_y = b.dir == 2;
                out.push(s);
            }
        }
        for fb in &self.level.firebars {
            if !visible(fb.x, fb.y) {
                continue;
            }
            out.push(Self::model_sprite(
                "M1_Object_firebar",
                "block",
                fb.x - 8.0,
                fb.y,
                16.0,
                16.0,
                false,
                11,
            ));
            let a = self.firebar_angle(fb).to_radians();
            for i in 0..fb.n {
                let (fx, fy) = (
                    fb.x + i as f32 * 8.0 * a.cos(),
                    fb.y + i as f32 * 8.0 * a.sin(),
                );
                out.push(Self::model_sprite(
                    "M1_Object_firebar",
                    "fire.0",
                    fx - 8.0,
                    fy,
                    16.0,
                    8.0,
                    (self.tick / 5) % 2 == 1,
                    46,
                ));
            }
        }
        for b in &self.level.blasters {
            if !visible(b.x, b.y) && !visible(b.x, b.y + b.h as f32 * TILE) {
                continue;
            }
            let top = b.y + (b.h - 2) as f32 * TILE;
            out.push(Self::model_sprite(
                "M1_Enemy_killer_houdai",
                "wait.0",
                b.x - 8.0,
                top,
                16.0,
                32.0,
                false,
                11,
            ));
            for i in 0..(b.h - 2).max(0) {
                out.push(Self::model_sprite(
                    "M1_Enemy_killer_houdai",
                    "unit.0",
                    b.x - 8.0,
                    b.y + i as f32 * TILE,
                    16.0,
                    16.0,
                    false,
                    11,
                ));
            }
        }
        for a in &self.actors {
            if matches!(a.mode, Mode::Parachute { .. }) && visible(a.x, a.y) {
                out.push(Self::model_sprite(
                    "M1_Enemy_parachutecommon",
                    "parachute",
                    a.x - 8.0,
                    a.y + a.height(),
                    16.0,
                    16.0,
                    false,
                    42,
                ));
            }
            if let Kind::Seesaw { width } = a.kind {
                // stepped approximation of the tilted plank (sprites don't rotate yet)
                let hw = width as f32 * 8.0;
                let slope = a.solid_slope();
                for i in 0..width {
                    let cx = a.x - hw + i as f32 * TILE + 8.0;
                    let tex = if i == 0 {
                        "seesaw_sideL.00"
                    } else if i == width - 1 {
                        "seesaw_sideR.00"
                    } else {
                        "seesaw_center.00"
                    };
                    out.push(Self::model_sprite(
                        "M1_Object_seesaw",
                        tex,
                        cx - 8.0,
                        a.y + 8.0 + slope * (cx - a.x),
                        16.0,
                        16.0,
                        false,
                        15,
                    ));
                }
                out.push(Self::model_sprite(
                    "M1_Object_seesaw",
                    "seesaw_point.00",
                    a.x - 8.0,
                    a.y + 8.0,
                    16.0,
                    16.0,
                    false,
                    16,
                ));
                continue;
            }
            if let Kind::Lift { width, falling } = a.kind {
                let x0 = a.x - width as f32 * 8.0;
                for i in 0..width {
                    let tex = if i == 0 {
                        "lift_sideL.00"
                    } else if i == width - 1 {
                        "lift_sideR.00"
                    } else if falling {
                        "lift_center.01"
                    } else {
                        "lift_center.00"
                    };
                    out.push(Self::model_sprite(
                        "M1_Object_lift_unite",
                        tex,
                        x0 + i as f32 * TILE,
                        a.y,
                        16.0,
                        16.0,
                        false,
                        15,
                    ));
                }
                continue;
            }
            if !visible(a.x, a.y) {
                continue;
            }
            if let Some(s) = self.actor_sprite(a) {
                out.push(s);
            }
        }
        for e in &self.effects {
            match *e {
                Effect::CoinPop { x, y, life, .. } => {
                    let frame = ["wait.0", "wait.1", "wait.2", "wait.3"][(life as usize / 2) % 4];
                    out.push(Self::model_sprite(
                        "M1_Object_coinrotate",
                        frame,
                        x - 8.0,
                        y,
                        16.0,
                        16.0,
                        false,
                        60,
                    ));
                }
                Effect::Debris { x, y, life, .. } => {
                    let mut s = tile(x - 4.0, y - 4.0, 80, 60);
                    s.w = 8.0;
                    s.h = 8.0;
                    s.flip_x = (life / 4) % 2 == 0;
                    out.push(s);
                }
                Effect::Score { .. } => {}
            }
        }
        if let Some(s) = self.player_sprite() {
            out.push(s);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn model_sprite(
        model: &'static str,
        texture: &'static str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        flip_x: bool,
        layer: u32,
    ) -> Sprite {
        Sprite {
            image: SpriteImage {
                model,
                texture,
                rect: None,
            },
            x,
            y,
            w,
            h,
            flip_x,
            flip_y: false,
            layer,
        }
    }

    fn goal_sprites(&self, out: &mut Vec<Sprite>) {
        let Some((x0, y0)) = self.level.goal else {
            return;
        };
        // pole (16×176 texture: ball on top, base block in the bottom 16 rows), flag on the left
        // of the pole, castle (door centre) at X0 + 112
        out.push(Self::model_sprite(
            "M1_Object_goalpole",
            "goalpoal",
            x0 - 8.0,
            y0 - 16.0,
            16.0,
            176.0,
            false,
            8,
        ));
        out.push(Self::model_sprite(
            "M1_Object_goalpole",
            "flag",
            x0 - 16.0,
            y0 + self.goal_flag - 16.0,
            16.0,
            16.0,
            false,
            9,
        ));
        let cx = x0 + 112.0;
        // the flag's model rests on top of the tower (BFRES quad y 80…96 above the tower origin
        // at y0 − 16); tower_flag is its offset from there (−24 = hidden, 0 = raised)
        out.push(Self::model_sprite(
            "M1_Object_tower",
            "flag",
            cx - 8.0,
            y0 - 16.0 + self.tower_flag,
            16.0,
            16.0,
            false,
            7,
        ));
        out.push(Self::model_sprite(
            "M1_Object_tower",
            "tower",
            cx - 40.0,
            y0 - 16.0,
            80.0,
            80.0,
            false,
            8,
        ));
    }

    fn actor_sprite(&self, a: &Actor) -> Option<Sprite> {
        // [obs] M1 Goomba/Koopa `walk_ftp`: 20 frames, walk.0 at 0, walk.1 at 10 (play rate 1.0
        // in their EnemyModelParams)
        const WALK_FRAMES: u32 = 10;
        let walk2 = |rate: u32| {
            if (a.age / rate).is_multiple_of(2) {
                "walk.0"
            } else {
                "walk.1"
            }
        };
        let layer = if matches!(a.mode, Mode::Emerge { .. }) {
            5
        } else {
            40
        };
        let scale = if a.big { 2.0 } else { 1.0 };
        let (model, tex, w, h, flip_x) = match a.kind {
            Kind::Goomba => {
                // walk.1 is walk.0 mirrored: the texture swap alone animates it
                let t = if a.mode == Mode::Stomped {
                    "damage.0"
                } else {
                    walk2(WALK_FRAMES)
                };
                ("M1_Enemy_kuribo", t, 16.0, 32.0, false)
            }
            Kind::Koopa { red } => (
                if red {
                    "M1_Enemy_nokonokoB"
                } else {
                    "M1_Enemy_nokonokoA"
                },
                walk2(WALK_FRAMES),
                16.0,
                32.0,
                a.dir > 0,
            ),
            Kind::Shell { red } => {
                let t = if a.mode == Mode::ShellSlide {
                    ["shell.0", "shell.1", "shell.2", "shell.3"][(a.age as usize / 2) % 4]
                } else if a.timer < crate::actors::SHELL_SHAKE && (a.timer / 4) % 2 == 0 {
                    "revival.0"
                } else {
                    "shell.0"
                };
                (
                    if red {
                        "M1_Enemy_nokonokoB"
                    } else {
                        "M1_Enemy_nokonokoA"
                    },
                    t,
                    16.0,
                    if t.starts_with("revival") { 32.0 } else { 16.0 },
                    false,
                )
            }
            Kind::Mushroom => ("M1_Item_kinoko", "wait.0", 16.0, 16.0, false),
            Kind::OneUp => ("M1_Item_kinoko1up", "wait.0", 16.0, 16.0, false),
            Kind::FireFlower => (
                "M1_Item_flower",
                ["wait.0", "wait.1", "wait.2", "wait.3"][(a.age as usize / 4) % 4],
                16.0,
                16.0,
                false,
            ),
            Kind::Star => (
                "M1_Item_star",
                ["wait.0", "wait.1", "wait.2", "wait.3"][(a.age as usize / 4) % 4],
                16.0,
                16.0,
                false,
            ),
            Kind::Fireball | Kind::EnemyFire => ("M1_Object_firebar", "fire.0", 16.0, 8.0, false),
            Kind::Piranha { fire: false } => (
                "M1_Enemy_packun",
                if (a.age / 8).is_multiple_of(2) {
                    "wait.0"
                } else {
                    "wait.1"
                },
                16.0,
                24.0,
                false,
            ),
            Kind::Piranha { fire: true } => (
                "M1_Enemy_packunfire",
                if (a.age / 8).is_multiple_of(2) {
                    "up.0"
                } else {
                    "up.1"
                },
                32.0,
                32.0,
                a.dir > 0,
            ),
            Kind::Thwomp { .. } => (
                "M1_Enemy_dossun",
                match a.phase {
                    1 => "down.0",
                    0 if (a.target.0 - a.x).abs() < 64.0 => "look.0",
                    _ => "wait.0",
                },
                24.0,
                32.0,
                false,
            ),
            Kind::Spring => (
                "M1_Object_jumpstep",
                ["jump.0", "jump.1", "jump.1", "jump.2", "jump.2"][a.pause.clamp(0, 4) as usize],
                16.0,
                16.0,
                false,
            ),
            Kind::BulletBill => ("M1_Enemy_killer", "wait.0", 16.0, 16.0, a.dir > 0),
            Kind::Lift { .. } => return None,
            Kind::PSwitch => (
                "M1_Object_pswitch",
                if a.phase > 0 {
                    "push.1"
                } else {
                    ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4]
                },
                16.0,
                16.0,
                false,
            ),
            Kind::Muncher => (
                "M1_Enemy_packunblack",
                if (self.tick / 8).is_multiple_of(2) {
                    "wait.0"
                } else {
                    "wait.1"
                },
                16.0,
                16.0,
                false,
            ),
            Kind::Donut => ("M1_Object_block_chikuwa", "fall.0", 16.0, 16.0, false),
            Kind::Spiny => ("M1_Enemy_togezo", walk2(8), 16.0, 16.0, a.dir > 0),
            Kind::Buzzy => ("M1_Enemy_met", walk2(8), 16.0, 16.0, a.dir > 0),
            Kind::SpikeTop => ("M1_Enemy_togemetA", walk2(8), 16.0, 16.0, a.dir > 0),
            Kind::HammerBro => (
                "M1_Enemy_bros",
                if a.timer < 12 { "throw.0" } else { walk2(8) },
                16.0,
                24.0,
                a.dir > 0,
            ),
            Kind::Hammer => (
                "M1_Enemy_bros",
                "hammer.0",
                16.0,
                16.0,
                (a.age / 4) % 2 == 1,
            ),
            Kind::BobOmb => (
                "M1_Enemy_bombhei",
                if a.phase > 0 { "damage.0" } else { walk2(8) },
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::DryBones => (
                "M1_Enemy_karon",
                if a.phase == 1 {
                    if a.timer < 61 { "shake.0" } else { "damage.1" }
                } else {
                    walk2(8)
                },
                24.0,
                32.0,
                a.dir > 0,
            ),
            Kind::BanzaiBill { dir } => ("M1_Enemy_killermagnum", "wait.0", 64.0, 64.0, dir == 0),
            Kind::Bumper => ("M1_Object_saw", "saw_maru", 48.0, 48.0, false),
            Kind::Grinder => (
                "M1_Object_saw",
                if (self.tick / 4).is_multiple_of(2) {
                    "wait.0"
                } else {
                    "blink.0"
                },
                48.0,
                48.0,
                (self.tick / 2) % 2 == 1,
            ),
            Kind::Cannonball => (
                "M1_Enemy_senkan_houdai",
                "senkan_houdai_ball.0",
                16.0,
                16.0,
                false,
            ),
            Kind::Seesaw { .. } => return None,
            Kind::Claw => return None,
            Kind::Magikoopa => {
                if a.phase == 0 {
                    return None;
                }
                (
                    "M1_Enemy_kameck",
                    if a.phase == 2 { "throw.0" } else { "wait.0" },
                    32.0,
                    32.0,
                    a.dir > 0,
                )
            }
            Kind::Magic => (
                "M1_Enemy_kameck",
                walk_frame(&["effect.0", "effect.1", "effect.2"], a.age, 4),
                16.0,
                16.0,
                false,
            ),
            Kind::RockyWrench => (
                "M1_Enemy_poo",
                if a.phase == 4 { "throw.0" } else { "wait.0" },
                16.0,
                32.0,
                a.dir > 0,
            ),
            Kind::Wrench => (
                "M1_Enemy_poo",
                walk_frame(&["hammer.0", "hammer.1", "hammer.2", "hammer.3"], a.age, 4),
                16.0,
                16.0,
                false,
            ),
            Kind::Wiggler => ("M1_Enemy_hanachan", "head.0", 16.0, 32.0, a.dir > 0),
            Kind::LavaLift { .. } => return None,
            Kind::Skewer { .. } => return None,
            Kind::CheepCheep { red } => (
                if red {
                    "M1_Enemy_pukupukuB"
                } else {
                    "M1_Enemy_pukupukuA"
                },
                walk_frame(&["wait.0", "wait.1"], a.age, 8),
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::Blooper => (
                "M1_Enemy_gesso",
                if a.phase == 1 { "wait.1" } else { "wait.0" },
                16.0,
                24.0,
                false,
            ),
            Kind::Twister => (
                "M1_Object_tornado",
                if (self.tick / 4).is_multiple_of(2) {
                    "wait.0"
                } else {
                    "wait.1"
                },
                32.0,
                a.heading.max(16.0),
                false,
            ),
            Kind::Lakitu => (
                "M1_Enemy_jugem",
                if a.pause > 0 { "throw.0" } else { "wait.0" },
                16.0,
                24.0,
                a.target.0 > a.x,
            ),
            Kind::SpinyEgg => ("M1_Enemy_togezo", "shell.0", 16.0, 16.0, false),
            Kind::Cloud => ("M1_Enemy_jugem_cloud", "wait.0", 32.0, 32.0, false),
            Kind::Pow => (
                "M1_Object_block_pow",
                ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4],
                16.0,
                16.0,
                false,
            ),
            Kind::Key => (
                "M1_Object_key",
                ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4],
                16.0,
                16.0,
                false,
            ),
            Kind::Boo => (
                "M1_Enemy_teresa",
                if (a.target.0 < a.x) == (a.target_facing > 0) {
                    "wait.0"
                } else {
                    "walk.0"
                },
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::BigCoin { value } => (
                ["M1_Object_coin10", "M1_Object_coin30", "M1_Object_coin50"][value as usize],
                ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4],
                32.0,
                32.0,
                false,
            ),
        };
        let (w, h) = (w * scale, h * scale);
        // model origin = actor origin (feet centre); the front end places the art with the
        // BFRES quad (render.rs). Fireball models have their origin at the centre.
        let mut s = Self::model_sprite(model, tex, a.x - w / 2.0, a.y, w, h, flip_x, layer);
        if matches!(a.kind, Kind::Fireball | Kind::EnemyFire) {
            s.y = a.y + a.height() / 2.0;
        }
        if matches!(a.kind, Kind::Piranha { .. }) {
            // in a pipe: behind the pipe; a down-facing pipe hangs the plant upside down
            if a.pipe.is_some() {
                s.layer = 5;
            }
            if let Some((PipeDir::Down, ..)) = a.pipe {
                s.flip_y = true;
                s.y = a.y - h;
            }
        }
        if matches!(a.mode, Mode::DokanOut { .. }) {
            s.layer = 5;
        }
        if a.mode == Mode::DieFall {
            s.flip_y = true;
            s.layer = 70;
        }
        if matches!(a.mode, Mode::Parachute { .. }) {
            s.layer = 41;
        }
        Some(s)
    }

    fn player_sprite(&self) -> Option<Sprite> {
        let p = &self.player;
        if p.hidden {
            return None;
        }
        // flicker while invincible after damage
        if p.invuln > 0 && (p.invuln / 2) % 2 == 1 && p.control == Control::Normal {
            return None;
        }
        let form = match p.control {
            Control::Change { timer, from } if (timer / 4) % 2 == 0 => from,
            _ => p.form,
        };
        let model = match form {
            Form::Small => "M1_Player_MarioMdl",
            Form::Super => "M1_Player_SpMarioMdl",
            Form::Fire => "M1_Player_FireMarioMdl",
        };
        // star: cycle the palette variants (_2, _3, _4)
        let model: &'static str = if p.star > 0 {
            let k = (self.tick / 2) % 4;
            match (form, k) {
                (_, 0) => model,
                (Form::Small, 1) => "M1_Player_MarioMdl_2",
                (Form::Small, 2) => "M1_Player_MarioMdl_3",
                (Form::Small, _) => "M1_Player_MarioMdl_4",
                (Form::Super, 1) => "M1_Player_SpMarioMdl_2",
                (Form::Super, 2) => "M1_Player_SpMarioMdl_3",
                (Form::Super, _) => "M1_Player_SpMarioMdl_4",
                (Form::Fire, 1) => "M1_Player_FireMarioMdl_2",
                (Form::Fire, 2) => "M1_Player_FireMarioMdl_3",
                (Form::Fire, _) => "M1_Player_FireMarioMdl_4",
            }
        } else {
            model
        };
        let small = form == Form::Small;
        let frame = match p.pose {
            Pose::Wait => "wait.0",
            Pose::Skid => "turn.0",
            Pose::Jump => "jump.0",
            Pose::Walk => ["walk.0", "walk.1", "walk.2"][((p.walk_phase / 6.0) as usize) % 3],
            Pose::Crouch => "stoop.0",
            Pose::Dead => {
                if small {
                    "dead.0"
                } else {
                    "jump.0"
                }
            }
            Pose::Pole => "climb.0",
            Pose::Swim => ["swim.0", "swim.1", "swim.2", "swim.3", "swim.4", "swim.5"]
                [((p.walk_phase / 4.0) as usize).min(5)],
            Pose::Fire => {
                if form == Form::Fire {
                    "fire_attack.0"
                } else {
                    "jump.0"
                }
            }
        };
        let frame = if p.pose == Pose::Dead {
            "dead.0"
        } else {
            frame
        };
        let model = if p.pose == Pose::Dead {
            "M1_Player_MarioMdl"
        } else {
            model
        };
        let h = if small || p.pose == Pose::Dead {
            16.0
        } else {
            32.0
        };
        Some(Sprite {
            image: SpriteImage {
                model,
                texture: frame,
                rect: None,
            },
            x: p.x - 8.0,
            y: p.y,
            w: 16.0,
            h,
            flip_x: p.facing < 0,
            flip_y: false,
            // behind terrain while sliding through a pipe
            layer: if self.in_pipe() { 5 } else { 50 },
        })
    }

    /// HUD counters for the front end.
    pub fn hud(&self) -> Hud {
        Hud {
            score: self.score,
            coins: self.coins,
            lives: self.lives,
            time: if self.time < 0 {
                -1
            } else {
                (self.time + 59) / 60
            },
            cleared: self.cleared,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Hud {
    pub score: u32,
    pub coins: u32,
    pub lives: u32,
    pub time: i32,
    pub cleared: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{BlockPlacement, CoinTile};

    fn run(w: &mut World, buttons: u32, frames: usize) {
        for _ in 0..frames {
            w.step(Input { buttons });
        }
    }

    #[test]
    fn coin_collection_scores() {
        let mut lvl = Level::flat(100);
        lvl.coins.push(CoinTile { x: 6, y: 2 });
        let mut w = World::with_level(lvl);
        run(&mut w, input::RIGHT, 60);
        assert_eq!(w.coins, 1);
        assert_eq!(w.score, 100);
        assert!(w.level.coins.is_empty());
    }

    #[test]
    fn head_hit_question_block_pops_coin() {
        let mut lvl = Level::flat(100);
        lvl.set(3, 5, Cell::Solid);
        lvl.blocks.push(BlockPlacement {
            x: 3,
            y: 5,
            kind: BlockKind::Question,
            contents: None,
        });
        let mut w = World::with_level(lvl);
        run(&mut w, input::JUMP, 60);
        assert_eq!(w.blocks.get(3, 5).unwrap().kind, BlockKind::Empty);
        run(&mut w, 0, 30);
        assert_eq!(w.coins, 1);
    }

    #[test]
    fn mushroom_from_block_makes_super() {
        let mut lvl = Level::flat(100);
        lvl.set(3, 5, Cell::Solid);
        lvl.blocks.push(BlockPlacement {
            x: 3,
            y: 5,
            kind: BlockKind::Question,
            contents: Some(Contents::Mushroom),
        });
        let mut w = World::with_level(lvl);
        run(&mut w, input::JUMP, 30);
        assert_eq!(w.actors.len(), 1);
        // let the mushroom walk off the block and land, then catch up with it
        run(&mut w, 0, 100);
        run(&mut w, input::RIGHT, 240);
        assert_eq!(w.player.form, Form::Super);
    }

    #[test]
    fn stomp_goomba_and_bounce() {
        let mut lvl = Level::flat(100);
        lvl.spawns.push(Spawn {
            kind: Kind::Goomba,
            x: 150.0,
            y: 32.0,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
        });
        let mut w = World::with_level(lvl);
        assert_eq!(w.actors.len(), 1);
        let mut stomped = false;
        for i in 0..200 {
            // walk, then jump once the goomba is close
            let near = w.actors[0].x - w.player.x < 40.0;
            let jump = if near && i % 2 == 0 { input::JUMP } else { 0 };
            w.step(Input {
                buttons: input::RIGHT | jump,
            });
            if w.events.contains(&Event::Stomp) {
                stomped = true;
                assert!(w.player.vy > 3.0);
                break;
            }
        }
        assert!(stomped);
        assert!(w.player.alive());
        assert_eq!(w.score, 100);
    }

    #[test]
    fn walking_into_goomba_kills_small_player_then_restart() {
        let mut lvl = Level::flat(100);
        lvl.spawns.push(Spawn {
            kind: Kind::Goomba,
            x: 160.0,
            y: 32.0,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
        });
        let mut w = World::with_level(lvl);
        let lives = w.lives;
        let mut died = false;
        for _ in 0..200 {
            w.step(Input {
                buttons: input::RIGHT,
            });
            if !w.player.alive() {
                died = true;
                break;
            }
        }
        assert!(died);
        // 45-frame pose, then 140-frame fall, then restart with one life less
        run(&mut w, 0, 45 + 141);
        assert_eq!(w.lives, lives - 1);
        assert!(w.player.alive());
    }

    #[test]
    fn super_player_breaks_brick() {
        let mut lvl = Level::flat(100);
        lvl.set(3, 6, Cell::Solid);
        lvl.blocks.push(BlockPlacement {
            x: 3,
            y: 6,
            kind: BlockKind::Brick,
            contents: None,
        });
        let mut w = World::with_level(lvl);
        w.player.form = Form::Super;
        run(&mut w, input::JUMP, 30);
        assert!(w.blocks.get(3, 6).is_none());
        assert_eq!(w.level.cell(3, 6), Cell::Empty);
        assert!(w.effects.iter().any(|e| matches!(e, Effect::Debris { .. })));
    }

    #[test]
    fn hurt_shrinks_super_and_grants_invulnerability() {
        let mut w = World::new();
        w.player.form = Form::Fire;
        assert!(w.player.hurt());
        assert_eq!(w.player.form, Form::Super);
        assert!(!w.player.hurt()); // mid-change / invulnerable
    }

    #[test]
    fn timer_runs_out_kills() {
        let mut lvl = Level::flat(100);
        lvl.timer = 2;
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 119);
        assert!(w.player.alive());
        run(&mut w, 0, 1);
        assert!(!w.player.alive());
    }

    #[test]
    fn goal_pole_sequence_clears_course() {
        let mut lvl = Level::flat(60);
        lvl.timer = 300;
        // pole at tile 20, base block on the ground
        lvl.set(20, 2, Cell::Solid);
        lvl.goal = Some((20.0 * TILE + 8.0, 3.0 * TILE));
        let mut w = World::with_level(lvl);
        let mut grabbed = false;
        for i in 0..3000 {
            let b = if w.goal_phase == GoalPhase::None {
                input::RIGHT | input::RUN | if i % 30 < 20 { input::JUMP } else { 0 }
            } else {
                0
            };
            w.step(Input { buttons: b });
            grabbed |= w.events.contains(&Event::Goal);
            if w.cleared {
                break;
            }
        }
        assert!(grabbed);
        assert!(w.cleared);
        assert_eq!(w.time, 0);
        assert!(w.player.hidden);
    }

    #[test]
    fn pipe_to_sub_area_and_back() {
        use crate::level::PipeDir;
        let mut main = Level::flat(60);
        main.add_pipe(10, 2, 2, PipeDir::Up);
        main.warps.push(crate::level::Warp {
            kind: crate::level::WarpKind::Pipe(PipeDir::Up),
            id: 1,
            x: 10.0 * TILE + TILE,
            y: 3.0 * TILE + 8.0,
            len: 2,
        });
        let mut sub = Level::flat(40);
        sub.theme = 1;
        sub.add_pipe(20, 2, 3, PipeDir::Up);
        sub.warps.push(crate::level::Warp {
            kind: crate::level::WarpKind::Pipe(PipeDir::Up),
            id: 1,
            x: 20.0 * TILE + TILE,
            y: 4.0 * TILE + 8.0,
            len: 3,
        });
        sub.coins.push(CoinTile { x: 3, y: 2 });
        let mut w = World::with_areas(vec![main, sub]);
        // jump onto the pipe (top at y 64), stand at its centre, press Down
        w.player.x = 10.0 * TILE + TILE;
        w.player.y = 4.0 * TILE;
        run(&mut w, 0, 5);
        assert!(w.player.on_ground);
        run(&mut w, input::DOWN, 1);
        assert!(w.transit.is_some());
        run(&mut w, 0, 200);
        assert!(w.transit.is_none());
        assert_eq!(w.area, 1);
        assert_eq!((w.player.x, w.player.y), (20.0 * TILE + TILE, 5.0 * TILE));
        assert_eq!(w.field_model, "M1_Field_underground");
        // collect the sub-area coin, then go back: the coin stays collected
        w.level.coins.clear();
        run(&mut w, input::DOWN, 1);
        run(&mut w, 0, 200);
        assert_eq!(w.area, 0);
        assert!(w.stash.as_ref().unwrap().level.coins.is_empty());
    }

    #[test]
    fn pipe_piranha_cycles_and_hides_when_player_on_pipe() {
        let mut lvl = Level::flat(60);
        lvl.add_pipe(20, 2, 2, PipeDir::Up);
        // entry point (top centre of the mouth) at (336, 64); hidden 36 px below
        lvl.spawns.push(Spawn {
            kind: Kind::Piranha { fire: false },
            x: 336.0,
            y: 64.0 - 36.0,
            big: false,
            wings: false,
            pipe: Some((PipeDir::Up, 336.0, 64.0)),
            parachute: false,
        });
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        assert_eq!(w.actors.len(), 1);
        // first cycle: rise (8 + 36 frames), out 96, sink 36, hidden 72, rise again
        let mut ys = Vec::new();
        for _ in 0..300 {
            w.step(Input::default());
            ys.push(w.actors[0].y);
        }
        let first_out = ys.iter().position(|&y| y == 64.0).unwrap();
        let out_len = ys[first_out..].iter().take_while(|&&y| y == 64.0).count();
        // the first frame is the (zero-length) initial hidden check
        assert_eq!(first_out, 1 + 8 + 36 - 1);
        assert!((96..=97).contains(&out_len), "out {out_len}");
        let hidden_from = first_out + out_len + 36;
        assert_eq!(ys[hidden_from], 64.0 - 36.0);
        assert!(ys[hidden_from + 72 + 8 + 36..].contains(&64.0));
        // standing on the pipe keeps it hidden
        let mut w2 = World::with_level(w.initial.clone());
        w2.player.x = 336.0;
        w2.player.y = 64.0;
        for _ in 0..300 {
            w2.step(Input::default());
            assert!(w2.actors[0].y <= 64.0 - 36.0 + 0.01);
        }
    }

    #[test]
    fn pipe_generator_spawns_every_180_frames() {
        let mut lvl = Level::flat(60);
        lvl.add_pipe(14, 2, 2, PipeDir::Up);
        lvl.generators.push(crate::level::PipeGenerator {
            kind: Kind::Goomba,
            big: false,
            wings: false,
            dir: PipeDir::Up,
            x: 240.0,
            y: 64.0,
            colour: 0,
        });
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        let mut births = Vec::new();
        let mut n = 0;
        for f in 0..500 {
            w.step(Input::default());
            let now = w.actors.iter().filter(|a| a.generator.is_some()).count();
            if now > n {
                births.push(f);
            }
            n = now;
        }
        assert!(births.len() >= 2, "{births:?}");
        assert_eq!(births[0], 89);
        assert_eq!(births[1] - births[0], 180);
    }

    #[test]
    fn checkpoint_respawn() {
        let mut lvl = Level::flat(100);
        lvl.checkpoints.push(crate::level::Checkpoint {
            x: 20.0 * TILE,
            y: 32.0,
            contents: None,
        });
        lvl.timer = 300;
        let mut w = World::with_level(lvl);
        run(&mut w, input::RIGHT, 300);
        assert_eq!(w.checkpoint, Some((0, 0)));
        w.player.kill();
        run(&mut w, 0, 200);
        assert!(w.player.alive());
        assert_eq!(w.player.x, 20.0 * TILE);
    }

    fn spawn(kind: Kind, x: f32, y: f32) -> Spawn {
        Spawn {
            kind,
            x,
            y,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
        }
    }

    #[test]
    fn spring_launches_higher_with_jump_held() {
        let peak = |held: bool| {
            let mut lvl = Level::flat(40);
            lvl.spawns.push(spawn(Kind::Spring, 120.0, 32.0));
            let mut w = World::with_level(lvl);
            run(&mut w, 0, 5);
            // drop onto the spring
            w.player.x = 120.0;
            w.player.y = 80.0;
            let mut top: f32 = 0.0;
            let mut launched = false;
            for _ in 0..240 {
                w.step(Input {
                    buttons: if held { input::JUMP } else { 0 },
                });
                launched |= w.events.contains(&Event::Jump);
                if launched {
                    top = top.max(w.player.y);
                }
            }
            assert!(launched);
            top - 44.5
        };
        let low = peak(false);
        let high = peak(true);
        assert!(low > 40.0, "low {low}");
        assert!(high > low + 40.0, "high {high} low {low}");
    }

    #[test]
    fn thwomp_slams_and_returns() {
        let mut lvl = Level::flat(40);
        lvl.spawns
            .push(spawn(Kind::Thwomp { side: false }, 200.0, 120.0));
        let mut w = World::with_level(lvl);
        w.player.x = 60.0;
        run(&mut w, 0, 10);
        assert_eq!(w.actors[0].y, 120.0);
        // walk underneath: it drops to the ground
        w.player.x = 200.0 - 40.0;
        let mut landed = false;
        for _ in 0..60 {
            w.step(Input::default());
            if w.actors[0].y == 32.0 {
                landed = true;
                break;
            }
        }
        assert!(landed);
        w.player.x = 60.0;
        run(&mut w, 0, 64 + 120);
        assert_eq!(w.actors[0].y, 120.0);
    }

    #[test]
    fn standing_on_a_falling_lift() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(
            Kind::Lift {
                width: 4,
                falling: true,
            },
            200.0,
            96.0,
        ));
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        w.player.y = 130.0;
        run(&mut w, 0, 40);
        assert!(w.player.ride.is_some(), "y {}", w.player.y);
        let y0 = w.actors[0].y;
        run(&mut w, 0, 20);
        assert!(w.actors[0].y < y0);
        assert_eq!(w.player.y, w.actors[0].y + 16.0);
    }

    #[test]
    fn spikes_hurt() {
        let mut lvl = Level::flat(40);
        lvl.set(6, 2, Cell::Spike);
        let mut w = World::with_level(lvl);
        let mut hurt = false;
        for _ in 0..60 {
            w.step(Input {
                buttons: input::RIGHT,
            });
            hurt |= !w.player.alive();
        }
        assert!(hurt);
    }

    fn place(lvl: &mut Level, x: i32, y: i32, kind: BlockKind, cell: Cell) {
        lvl.set(x, y, cell);
        lvl.blocks.push(BlockPlacement {
            x,
            y,
            kind,
            contents: None,
        });
    }

    #[test]
    fn on_off_switch_flips_dotted_blocks() {
        let mut lvl = Level::flat(40);
        place(&mut lvl, 3, 5, BlockKind::OnOff, Cell::Solid);
        place(
            &mut lvl,
            10,
            2,
            BlockKind::Dotted { blue: false },
            Cell::Solid,
        );
        place(
            &mut lvl,
            11,
            2,
            BlockKind::Dotted { blue: true },
            Cell::Empty,
        );
        let mut w = World::with_level(lvl);
        assert_eq!(w.level.cell(10, 2), Cell::Solid);
        assert_eq!(w.level.cell(11, 2), Cell::Empty);
        run(&mut w, input::JUMP, 30);
        assert!(!w.on_off);
        assert_eq!(w.level.cell(10, 2), Cell::Empty);
        assert_eq!(w.level.cell(11, 2), Cell::Solid);
    }

    #[test]
    fn p_switch_swaps_bricks_and_coins_then_reverts() {
        let mut lvl = Level::flat(60);
        place(&mut lvl, 20, 5, BlockKind::Brick, Cell::Solid);
        lvl.coins.push(CoinTile { x: 22, y: 5 });
        lvl.spawns.push(spawn(Kind::PSwitch, 120.0, 32.0));
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 5);
        w.player.x = 120.0;
        w.player.y = 70.0;
        run(&mut w, 0, 20);
        assert!(w.p_timer > 0);
        assert_eq!(w.level.cell(20, 5), Cell::Empty);
        assert!(w.level.coins.iter().any(|c| (c.x, c.y) == (20, 5)));
        assert_eq!(w.level.cell(22, 5), Cell::Solid);
        run(&mut w, 0, P_FRAMES as usize);
        assert_eq!(w.p_timer, 0);
        assert_eq!(w.level.cell(20, 5), Cell::Solid);
        assert!(w.level.coins.iter().any(|c| (c.x, c.y) == (22, 5)));
    }

    #[test]
    fn donut_drops_after_48_frames() {
        let mut lvl = Level::flat(40);
        for x in 10..16 {
            for y in 0..2 {
                lvl.set(x, y, Cell::Empty);
            }
        }
        place(&mut lvl, 12, 4, BlockKind::Donut, Cell::SemiTop);
        let mut w = World::with_level(lvl);
        w.player.x = 12.0 * TILE + 8.0;
        w.player.y = 5.0 * TILE + 10.0;
        run(&mut w, 0, 20);
        assert!(w.player.on_ground);
        run(&mut w, 0, 40);
        assert!(w.actors.iter().any(|a| a.kind == Kind::Donut));
        assert!(w.blocks.get(12, 4).is_none());
    }

    #[test]
    fn conveyor_carries_the_player() {
        let mut lvl = Level::flat(40);
        lvl.conveyors.push((2, 20, 1, 2.0));
        let mut w = World::with_level(lvl);
        let x0 = w.player.x;
        run(&mut w, 0, 60);
        // ramps 0.2/frame to 2.0 (10 frames), then 2 px/frame
        let moved = w.player.x - x0;
        assert!((moved - (2.0 * 60.0 - 9.0)).abs() < 1.0, "moved {moved}");
    }

    #[test]
    fn spiny_hurts_when_stomped() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Spiny, 160.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.form = Form::Super;
        w.player.x = 160.0;
        w.player.y = 80.0;
        run(&mut w, 0, 30);
        assert_eq!(w.player.form, Form::Small);
    }

    #[test]
    fn buzzy_is_fire_proof_and_shells() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Buzzy, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut f = Actor::new(Kind::Fireball, 180.0, 36.0);
        f.vx = 3.0;
        f.timer = 600;
        w.add_actor(f);
        run(&mut w, 0, 20);
        assert!(w.actors.iter().any(|a| a.kind == Kind::Buzzy && a.alive()));
        assert!(w.actors[0].stomp());
        assert!(matches!(w.actors[0].kind, Kind::Shell { .. }) && w.actors[0].buzzy);
    }

    #[test]
    fn bob_omb_fuse_and_explosion() {
        let mut lvl = Level::flat(40);
        place(&mut lvl, 13, 2, BlockKind::Brick, Cell::Solid);
        lvl.spawns
            .push(spawn(Kind::BobOmb, 12.0 * TILE + 8.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        assert!(w.actors[0].stomp());
        assert_eq!(w.actors[0].phase, 1);
        run(&mut w, 0, 241);
        assert!(w.actors.iter().all(|a| a.kind != Kind::BobOmb));
        assert!(
            w.blocks.get(13, 2).is_none(),
            "brick next to the bomb breaks"
        );
    }

    #[test]
    fn hammer_bro_throws() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::HammerBro, 250.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        let mut hammers = 0;
        for _ in 0..300 {
            w.step(Input::default());
            hammers = hammers.max(w.actors.iter().filter(|a| a.kind == Kind::Hammer).count());
        }
        assert!(hammers >= 1);
    }

    #[test]
    fn boo_freezes_when_watched() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Boo, 200.0, 64.0));
        let mut w = World::with_level(lvl);
        w.player.x = 100.0;
        w.player.facing = -1; // looking away: the Boo approaches
        run(&mut w, 0, 120);
        let x1 = w.actors[0].x;
        assert!(x1 < 200.0);
        w.player.facing = 1; // looking at it: it slows to a stop
        run(&mut w, 0, 60);
        let x2 = w.actors[0].x;
        run(&mut w, 0, 30);
        assert!((w.actors[0].x - x2).abs() < 0.5);
    }

    #[test]
    fn dry_bones_reassembles() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::DryBones, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        assert!(w.actors[0].stomp());
        run(&mut w, 0, 223);
        assert_eq!(w.actors[0].phase, 1);
        run(&mut w, 0, 2);
        assert_eq!(w.actors[0].phase, 0);
    }

    #[test]
    fn one_way_wall_passes_right_blocks_left() {
        let mut lvl = Level::flat(40);
        lvl.set(10, 2, Cell::PassRight);
        lvl.set(10, 3, Cell::PassRight);
        let mut w = World::with_level(lvl);
        run(&mut w, input::RIGHT, 200);
        assert!(
            w.player.x > 11.0 * TILE,
            "passes rightward, x {}",
            w.player.x
        );
        run(&mut w, input::LEFT, 200);
        assert!(
            w.player.x >= 11.0 * TILE,
            "blocked leftward, x {}",
            w.player.x
        );
    }

    #[test]
    fn parachute_drifts_down_slowly() {
        let mut lvl = Level::flat(40);
        let mut s = spawn(Kind::Goomba, 200.0, 200.0);
        s.parachute = true;
        lvl.spawns.push(s);
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 60);
        let a = &w.actors[0];
        assert!(matches!(a.mode, Mode::Parachute { .. }));
        assert!(a.vy >= -0.4 && a.y > 170.0, "y {} vy {}", a.y, a.vy);
        run(&mut w, 0, 600);
        assert_eq!(w.actors[0].mode, Mode::Normal);
        assert!(w.actors[0].on_ground);
    }

    #[test]
    fn burner_cycle_and_fire_bar_hurt() {
        let mut lvl = Level::flat(40);
        lvl.burners.push(crate::level::Burner {
            x: 72.0,
            y: 32.0,
            dir: 3,
            alt: false,
        });
        let w = World::with_level(lvl.clone());
        assert!(w.burner_flame(false) > 0.0);
        // off between 156 and 371
        let mut w2 = World::with_level(lvl);
        w2.player.x = 300.0;
        run(&mut w2, 0, 200);
        assert_eq!(w2.burner_flame(false), 0.0);
        assert!(w2.burner_flame(true) > 0.0);
        let mut lvl = Level::flat(40);
        lvl.firebars.push(crate::level::FireBar {
            x: 120.0,
            y: 40.0,
            n: 6,
            start: 180.0,
            dir: 1.0,
        });
        let mut w3 = World::with_level(lvl);
        w3.player.x = 100.0;
        let mut hurt = false;
        for _ in 0..200 {
            w3.step(Input::default());
            hurt |= !w3.player.alive();
        }
        assert!(hurt);
    }

    #[test]
    fn swimming_strokes_and_slow_sink() {
        let mut lvl = Level::flat(40);
        lvl.liquid = Some(crate::level::LiquidSpec {
            kind: 1,
            start: f32::MAX,
            end: f32::MAX,
            mode: 0,
            speed: 0,
            full: true,
        });
        let mut w = World::with_level(lvl);
        w.player.y = 150.0;
        w.player.on_ground = false;
        run(&mut w, 0, 120);
        assert!(
            w.player.vy >= -1.5 - 1e-4,
            "sink capped at -1.5: {}",
            w.player.vy
        );
        // tap jump: each stroke rises
        let y0 = w.player.y;
        for i in 0..60 {
            w.step(Input {
                buttons: if i % 10 == 0 { input::JUMP } else { 0 },
            });
        }
        assert!(w.player.y > y0, "strokes lift the player");
        assert_eq!(w.player.pose, Pose::Swim);
    }

    #[test]
    fn rising_lava_kills() {
        let mut lvl = Level::flat(40);
        lvl.liquid = Some(crate::level::LiquidSpec {
            kind: 2,
            start: 12.0,
            end: 140.0,
            mode: 1,
            speed: 3,
            full: false,
        });
        let mut w = World::with_level(lvl);
        let mut died = false;
        for _ in 0..600 {
            w.step(Input::default());
            died |= !w.player.alive();
        }
        assert!(died);
    }

    #[test]
    fn seesaw_tilts_under_the_rider() {
        let mut lvl = Level::flat(40);
        lvl.spawns
            .push(spawn(Kind::Seesaw { width: 5 }, 200.0, 80.0));
        let mut w = World::with_level(lvl);
        w.player.x = 230.0; // right end
        w.player.y = 110.0;
        run(&mut w, 0, 60);
        assert!(w.player.ride.is_some());
        assert!(
            w.actors[0].heading < -5.0,
            "tilts right-down: {}",
            w.actors[0].heading
        );
        // the rider is carried down with the plank
        assert!(w.player.y < 96.0);
    }

    #[test]
    fn bumper_bounces_from_above() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Bumper, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        w.player.y = 90.0;
        let mut bounced = false;
        for _ in 0..30 {
            w.step(Input::default());
            bounced |= w.player.vy > 5.0;
        }
        assert!(bounced);
    }

    #[test]
    fn cannon_fires_every_190_frames() {
        let mut lvl = Level::flat(40);
        lvl.cannons.push(crate::level::Cannon {
            x: 200.0,
            y: 32.0,
            angle: 180.0,
        });
        let mut w = World::with_level(lvl);
        w.player.x = 300.0; // behind the cannon (it fires left)
        let mut shots = Vec::new();
        for f in 0..450 {
            w.step(Input::default());
            if w.events.contains(&Event::Kick) {
                shots.push(f);
            }
        }
        assert_eq!(shots.len(), 3);
        assert_eq!(shots[1] - shots[0], 190);
    }

    #[test]
    fn lakitu_throws_spiny_eggs_that_hatch() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Lakitu, 200.0, 160.0));
        let mut w = World::with_level(lvl);
        w.player.x = 150.0;
        let mut spinies = 0;
        for _ in 0..400 {
            w.step(Input::default());
            spinies = spinies.max(w.actors.iter().filter(|a| a.kind == Kind::Spiny).count());
            if !w.player.alive() {
                break;
            }
        }
        assert!(spinies >= 1);
    }

    #[test]
    fn pow_quake_knocks_out_grounded_enemies() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Pow, 56.0, 64.0));
        lvl.spawns.push(spawn(Kind::Goomba, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 5);
        run(&mut w, input::JUMP, 20);
        assert!(w.actors.iter().all(|a| a.kind != Kind::Pow));
        assert!(
            w.actors
                .iter()
                .any(|a| a.kind == Kind::Goomba && a.mode == Mode::DieFall)
        );
    }

    #[test]
    fn key_opens_a_locked_door() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Key, 100.0, 32.0));
        for (id, x) in [(2u8, 200.0f32), (3, 600.0)] {
            lvl.warps.push(crate::level::Warp {
                kind: crate::level::WarpKind::Door(2),
                id,
                x,
                y: 32.0,
                len: 2,
            });
            lvl.doors.push((x, 32.0, 2));
        }
        let mut w = World::with_level(lvl);
        // locked without a key
        w.player.x = 200.0;
        run(&mut w, 0, 2);
        run(&mut w, input::UP, 1);
        assert!(w.transit.is_none());
        // get the key, come back
        w.player.x = 100.0;
        run(&mut w, 0, 3);
        assert_eq!(w.keys, 1);
        w.player.x = 200.0;
        run(&mut w, 0, 2);
        run(&mut w, input::UP, 1);
        assert!(w.transit.is_some());
        assert_eq!(w.keys, 0);
        run(&mut w, 0, 200);
        assert_eq!(w.player.x, 600.0);
    }

    #[test]
    fn riding_a_snake_block() {
        let mut lvl = Level::flat(60);
        lvl.snakes
            .push((10, 5, 3, false, false, vec![2, 2, 2, 2, 13]));
        let mut w = World::with_level(lvl);
        w.player.x = 11.0 * TILE + 8.0;
        w.player.y = 6.0 * TILE + 4.0;
        run(&mut w, 0, 10);
        assert!(w.player.ride.is_some(), "standing on the snake");
        let x0 = w.player.x;
        run(&mut w, 0, 60);
        assert!(w.player.x > x0 + 25.0, "carried along: {}", w.player.x - x0);
    }

    #[test]
    fn twister_lifts_the_player() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Twister, 120.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut top: f32 = 0.0;
        for _ in 0..200 {
            w.step(Input::default());
            top = top.max(w.player.y);
        }
        assert!(top > 80.0, "lifted to {top}");
    }

    #[test]
    fn claw_grabs_and_releases() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Claw, 120.0, 72.0));
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 1);
        // put the player right at the claw tip
        let (tx, ty) = w.actors[0].claw_tip();
        w.player.x = tx;
        w.player.y = ty - 20.0;
        w.player.on_ground = false;
        run(&mut w, 0, 2);
        assert!(w.held_by.is_some());
        run(&mut w, 0, 30);
        assert_eq!(w.player.control, Control::Demo);
        run(&mut w, input::JUMP, 1);
        assert!(w.held_by.is_none());
        assert_eq!(w.player.control, Control::Normal);
        assert!(w.player.vy > 0.0);
    }

    #[test]
    fn magikoopa_appears_and_casts() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Magikoopa, 250.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut magic = false;
        for _ in 0..300 {
            w.step(Input::default());
            magic |= w.actors.iter().any(|a| a.kind == Kind::Magic);
            if !w.player.alive() {
                break;
            }
        }
        assert!(magic);
    }

    #[test]
    fn rocky_wrench_throws() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::RockyWrench, 250.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut wrench = false;
        for _ in 0..400 {
            w.step(Input::default());
            wrench |= w.actors.iter().any(|a| a.kind == Kind::Wrench);
            if !w.player.alive() {
                break;
            }
        }
        assert!(wrench);
    }

    #[test]
    fn wiggler_gets_angry() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Wiggler, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 30);
        assert!(w.actors[0].stomp());
        assert_eq!(w.actors[0].phase, 1);
        run(&mut w, 0, 61);
        assert!((w.actors[0].vx.abs() - 1.0).abs() < 1e-6, "angry speed");
        assert_eq!(w.actors[0].wiggler_segments().len(), 4);
    }

    #[test]
    fn lava_lift_moves_when_ridden() {
        let mut lvl = Level::flat(60);
        lvl.spawns
            .push(spawn(Kind::LavaLift { fast: false }, 200.0, 64.0));
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        w.player.y = 100.0;
        run(&mut w, 0, 30);
        assert!(w.player.ride.is_some());
        let x0 = w.actors[0].x;
        run(&mut w, 0, 50);
        assert!((w.actors[0].x - x0 - 40.0).abs() < 1.0);
    }

    #[test]
    fn deterministic_with_objects() {
        let make = || {
            let mut lvl = Level::flat(150);
            for i in 0..5 {
                lvl.spawns.push(Spawn {
                    kind: if i % 2 == 0 {
                        Kind::Goomba
                    } else {
                        Kind::Koopa { red: i == 3 }
                    },
                    x: 200.0 + 90.0 * i as f32,
                    y: 32.0,
                    big: false,
                    wings: i == 4,
                    pipe: None,
                    parachute: false,
                });
            }
            World::with_level(lvl)
        };
        let script: Vec<u32> = (0..1500)
            .map(|i| match i % 120 {
                0..=60 => input::RIGHT | input::RUN,
                61..=80 => input::RIGHT | input::JUMP,
                _ => input::LEFT,
            })
            .collect();
        let go = || {
            let mut w = make();
            for &b in &script {
                w.step(Input { buttons: b });
            }
            (
                w.player.x.to_bits(),
                w.player.y.to_bits(),
                w.score,
                w.actors.len(),
            )
        };
        assert_eq!(go(), go());
    }
}
