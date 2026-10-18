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
/// Solid owner ids of ! blocks: base + block × 64 + piece.
const EXCL_OWNER: u32 = 0x9000_0000;
/// Solid owner ids of tree canopies: base + tree.
const TREE_OWNER: u32 = 0x9800_0000;
/// Solid ids of Piranha Creeper stalk boxes (actor index × 128 + box).
const CREEPER_OWNER: u32 = 0x9C00_0000;
use crate::fmath::DetMath;
use crate::player::{Control, Form, Player, Pose, STAR_FRAMES};
use crate::render::{Sprite, SpriteImage};
use crate::warp::{self, SideHold, Transit};

mod bosses;
mod debug;
mod smm1;
use smm1::Smm1Shell;
mod koopalings;
mod powerups;
mod v3;
pub use v3::v3_dash_frames;
mod w3;

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
const MAX_LIVES: u32 = 99;
const HURRY_SECONDS: i32 = 100;
/// Spawn zone (0x71003fbbc0, constants from 0x71000172e0) [obs]: a course object spawns when its
/// spawn box touches the view extended by 47 px left/right and 31 px up/down (plus 1000 px
/// vertically outside vertical areas), and is re-armed only after its box has left the view
/// extended by 48 px on every side.
const SPAWN_MARGIN_X: f32 = 47.0;
const SPAWN_MARGIN_Y: f32 = 31.0;
const REARM_MARGIN: f32 = 48.0;
/// Horizontal areas extend the spawn and despawn zones by this much vertically [obs].
const ZONE_EXTRA_Y: f32 = 1000.0;
/// Despawn (0x7100002d30): an actor goes once its culling box, grown by 80 px, misses the view
/// [obs].
const DESPAWN_MARGIN: f32 = 80.0;
/// A sideways-spring push within this many frames of the last counts as consecutive [obs].
const SIDE_PUSH_CHAIN: i32 = 10;
/// Fire Bar rotation per frame, as a 32-bit binary angle [obs _INIT_51].
const FIREBAR_STEP: u32 = 0xC0_0000;

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
    /// A clear-condition count went up / the condition is now met.
    ConditionCount,
    ConditionMet,
    /// SMM1 extras: a costumed player landed, started a skid, began the taunt (their costume
    /// sounds); Lanky Mario's float cue during a held jump.
    Land,
    Skid,
    Taunt,
    LankyFloat,
    /// The update power-ups (world/powerups.rs): Link's slash, dash into a wall
    /// ("p_DashSwoadHit"), down-thrust ("hipdrop_start"), shield block ("shieldHit"), bow
    /// ("PlayerArrowShoot"), arrow hit ("r_hit"); the SMB2 Mushroom's pull, throw ("throw2")
    /// and full crouch charge ("voice_squat_charge"); the balloon's bounce ("wall_hit"); the
    /// boomerang's throw ("fireball" with the form), wall bounce ("Rebound") and item hop
    /// ("BoomerangReaction"); a LinkBomb blast ("p_LinkBombExp"); wearables put on ("Wearing")
    /// and lost ("HeadDressOff"); the Cannon Box shot and its blast ("explode"); a Propeller
    /// Box flight; the Bullet Bill Mask breaking ("KillerBreak").
    SwordSlash,
    SwordDashHit,
    SwordThrust,
    ShieldHit,
    ArrowShoot,
    ArrowHit,
    Pull,
    Throw,
    SquatCharge,
    BalloonBounce,
    BoomerangThrow,
    Rebound,
    BoomerangReaction,
    LinkBombExplode,
    Wearing,
    HeadDressOff,
    CannonShot,
    Explode,
    PropellerBox,
    KillerBreak,
}

/// P-switch duration: 540 frames, reloaded by every press (the course switch state's P timer,
/// 0x71016c4a48 in v3.0.3) [obs].
pub const P_FRAMES: i32 = 540;

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
    /// The left area's liquid, held where it was (see `switch_area`).
    liquid: Option<Liquid>,
}

#[derive(Clone, Debug)]
struct SpawnRecord {
    spawn: Spawn,
    alive: bool,
    killed: bool,
    /// Spawned and not yet re-armed (record state bit 1): set when the actor is created, cleared
    /// once the spawn box leaves the keep-armed zone.
    spawned: bool,
}

#[derive(Clone)]
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
    /// Score and coins every (re)start begins from (a challenge run's course-start counters).
    pub base_score: u32,
    pub base_coins: u32,
    /// Extra lives earned since the world was made (1-Ups, 100-coin rollovers); never reset,
    /// counted even at the lives cap, so a run manager can bank them.
    pub lives_gained: u32,
    /// Challenge run: extra lives are banked by the run manager, not added to `lives`.
    pub bank_lives: bool,
    /// Time left in frames (−1: no timer).
    pub time: i32,
    /// The course's clear condition and its progress this attempt.
    pub clear: crate::clear::ClearCondition,
    pub goal_phase: GoalPhase,
    /// Goal flag height above Y0, tower flag offset, frames since StartPlayerRun.
    pub goal_flag: f32,
    pub tower_flag: f32,
    goal_frames: i32,
    countdown: bool,
    countdown_delay: i32,
    pub cleared: bool,
    pub events: Vec<Event>,
    /// Particle effect cues of this tick (cleared with `events`).
    pub fx: Vec<crate::fx::Fx>,
    rng: u32,
    /// The course's shared random generator (scene +0xf8 → +8): a 4-word xorshift
    /// (0x71014e43e0) that every actor draws from; the game seeds it per play, here it is fixed.
    course_rng: [u32; 4],
    /// Escort Toads knocked out: (actor id, frames until he is back).
    escort_down: Vec<(u32, i32)>,
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
    /// Shared 240-frame area timer of blinking and spike blocks; blinking group state.
    pub area_timer: u32,
    pub blink_alt: bool,
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
    /// Autoscroll view position (x, or y in vertical areas).
    autoscroll_pos: f32,
    /// Custom autoscroll: next node index (into the sorted nodes), speed, camera centre.
    custom_scroll: Option<(usize, f32, (f32, f32))>,
    /// The current area's liquid level, if any.
    pub liquid: Option<Liquid>,
    /// Cannon countdowns (current area).
    cannon_timers: Vec<i32>,
    /// Keys held.
    pub keys: u32,
    /// Pink coins collected this attempt (area, tile) and the course's total.
    pub pink: std::collections::BTreeSet<(usize, i32, i32)>,
    pub pink_total: usize,
    /// Jump / Run held last frame while flying a Clown Car.
    clown_prev: (bool, bool),
    /// The course's game style (feature switches for the player and objects).
    pub style: crate::player::Style,
    /// Players 2–4 (Luigi, Toad, Toadette) in local multiplayer; the camera follows player 1.
    pub others: Vec<Player>,
    /// Which character each extra player is (index into Mario, Luigi, Toad, Toadette).
    pub other_chars: Vec<usize>,
    /// Yoshi's mouth: what he holds to spit (an enemy kind; fire), tongue frames left, the Red
    /// Yoshi fire cooldown, and Run last frame (tongue presses).
    pub yoshi_mouth: Option<Kind>,
    pub tongue: i32,
    yoshi_fire_cd: i32,
    yoshi_prev_run: bool,
    /// Character of player 1's slot (changes when another player takes the goal).
    pub main_char: usize,
    /// Run held this frame (carrying).
    run_held: bool,
    /// Key-door pairs unlocked: (area, warp id >> 1).
    unlocked_doors: std::collections::BTreeSet<(usize, u8)>,
    /// Warp-box pairs unlocked with a key, and pairs used up (single player: a box vanishes
    /// after its warp): (area, warp id >> 1).
    unlocked_boxes: std::collections::BTreeSet<(usize, u8)>,
    gone_boxes: std::collections::BTreeSet<(usize, u8)>,
    /// Trees whose canopy item came out: (area, tree index).
    trees_shaken: std::collections::BTreeSet<(usize, usize)>,
    /// Snake blocks in the current area.
    pub snakes: Vec<Snake>,
    /// Rendering a whole course map (`look_at`): the backdrop doesn't parallax.
    pub map_view: bool,
    /// Frames since a plain Mushroom Trampoline bounce (1…6, the late-jump window; 0 = none),
    /// and whether jump was pressed this frame.
    tramp_late: i32,
    jump_pressed_now: bool,
    prev_jump_tramp: bool,
    /// The player's recent path (oldest first), which the escort Toad follows.
    player_trail: Vec<(f32, f32)>,
    /// ! blocks in the current area.
    pub exclamations: Vec<crate::exclamation::ExclamationBlock>,
    /// Swinging claw holding the player (actor id).
    held_by: Option<u32>,
    /// Desert-night wind: level, frame counter.
    pub wind: f32,
    wind_n: i32,
    prev_jump_claw: bool,
    /// Engine options (SMM1 extras); kept across restarts.
    pub options: crate::options::Options,
    /// World state of the 2.0 / 3.0 parts (world/v3.rs).
    pub v3: v3::V3World,
    /// World state of the update power-ups (world/powerups.rs).
    pu: powerups::PuWorld,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// Field atlas model per theme for SMB1 (Model/M1_Field_<theme>).
/// The model of multiplayer character `who` (0 Mario … 3 Toadette): the `_2`/`_3`/`_4` variant.
fn character_model(model: &'static str, who: usize) -> &'static str {
    const TABLE: [[&str; 4]; 6] = [
        [
            "M1_Player_MarioMdl",
            "M1_Player_MarioMdl_2",
            "M1_Player_MarioMdl_3",
            "M1_Player_MarioMdl_4",
        ],
        [
            "M1_Player_SpMarioMdl",
            "M1_Player_SpMarioMdl_2",
            "M1_Player_SpMarioMdl_3",
            "M1_Player_SpMarioMdl_4",
        ],
        [
            "M1_Player_FireMarioMdl",
            "M1_Player_FireMarioMdl_2",
            "M1_Player_FireMarioMdl_3",
            "M1_Player_FireMarioMdl_4",
        ],
        [
            "M1_Player_TnkMarioMdl",
            "M1_Player_TnkMarioMdl_2",
            "M1_Player_TnkMarioMdl_3",
            "M1_Player_TnkMarioMdl_4",
        ],
        [
            "M1_Player_MantMario",
            "M1_Player_MantMario_2",
            "M1_Player_MantMario_3",
            "M1_Player_MantMario_4",
        ],
        [
            "M1_Player_SpBallMarioMdl",
            "M1_Player_SpBallMarioMdl_2",
            "M1_Player_SpBallMarioMdl_3",
            "M1_Player_SpBallMarioMdl_4",
        ],
    ];
    TABLE
        .iter()
        .find(|row| row[0] == model)
        .map_or(model, |row| row[who.min(3)])
}

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

/// The spawn box of an actor kind (centre x, centre y, half width, half height from the
/// placement), from the actor profiles (profile +0x18, research profiles.txt) [obs]: most
/// enemies and items are {0, 8, 8, 8}, their Big rows doubled.
fn spawn_profile(kind: Kind, big: bool) -> (f32, f32, f32, f32) {
    let base = match kind {
        Kind::Thwomp { .. } | Kind::KoopaCar => (0.0, 16.0, 16.0, 16.0),
        Kind::ClownCar { .. } => (0.0, 16.0, 32.0, 32.0),
        Kind::Lift { falling: true, .. } | Kind::LavaLift { .. } => (0.0, 0.0, 64.0, 128.0),
        Kind::Lift { .. } | Kind::Trampoline { .. } => (0.0, 0.0, 80.0, 128.0),
        Kind::Seesaw { .. } => (0.0, 0.0, 128.0, 128.0),
        Kind::Key => (0.0, 8.0, 128.0, 128.0),
        Kind::Star => (0.0, 8.0, 8.0, 128.0),
        Kind::YoshiEgg { .. } => (0.0, 8.0, 22.0, 22.0),
        Kind::Yoshi { .. } => (0.0, 0.0, 12.0, 16.0),
        Kind::RingBoo { .. } => (0.0, 16.0, 96.0, 96.0),
        Kind::FloatCoin => (0.0, 0.0, 16.0, 16.0),
        _ => (0.0, 8.0, 8.0, 8.0),
    };
    let s = if big { 2.0 } else { 1.0 };
    (base.0 * s, base.1 * s, base.2 * s, base.3 * s)
}

impl World {
    /// The built-in demo strip (used when no course is loaded).
    pub fn new() -> Self {
        Self::with_level(Level::demo())
    }

    pub fn from_course(course: &Course) -> Self {
        let main = Level::from_course(course, 0);
        let sub = Level::from_course(course, 1);
        let mut w = Self::with_areas(vec![main, sub]);
        let h = &course.header;
        w.clear = crate::clear::ClearCondition::from_header(
            h.clear_condition_category,
            h.clear_condition,
            h.clear_condition_magnitude.max(0) as u16,
        );
        w
    }

    /// Use these engine options from now on (hosts call this on every world they make).
    pub fn set_options(&mut self, options: crate::options::Options) {
        self.options = options;
    }

    pub fn with_level(level: Level) -> Self {
        Self::with_areas(vec![level])
    }

    pub fn with_areas(areas: Vec<Level>) -> Self {
        let level = areas[0].clone();
        let pink_total = areas.iter().map(|l| l.pink_coins.len()).sum();
        let style = crate::player::Style::from_code(&level.style);
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
            base_score: 0,
            base_coins: 0,
            lives_gained: 0,
            bank_lives: false,
            time: -1,
            clear: crate::clear::ClearCondition::none(),
            goal_phase: GoalPhase::None,
            goal_flag: 144.0,
            tower_flag: -24.0,
            goal_frames: 0,
            countdown: false,
            countdown_delay: 0,
            cleared: false,
            events: Vec::new(),
            fx: Vec::new(),
            rng: 0x2545_f491,
            course_rng: [0x075b_cd15, 0x159a_55e5, 0x1f12_3bb5, 0x0549_1333],
            escort_down: Vec::new(),
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
            area_timer: 0,
            blink_alt: false,
            p_timer: 0,
            p_bricks: Vec::new(),
            p_coins: Vec::new(),
            donut_ride: None,
            donut_respawn: Vec::new(),
            carry: 0.0,
            autoscroll_pos: 0.0,
            custom_scroll: None,
            liquid: None,
            cannon_timers: Vec::new(),
            keys: 0,
            pink: Default::default(),
            pink_total,
            clown_prev: (false, false),
            style,
            others: Vec::new(),
            other_chars: Vec::new(),
            main_char: 0,
            yoshi_mouth: None,
            tongue: 0,
            yoshi_fire_cd: 0,
            yoshi_prev_run: false,
            run_held: false,
            unlocked_doors: Default::default(),
            unlocked_boxes: Default::default(),
            gone_boxes: Default::default(),
            trees_shaken: Default::default(),
            snakes: Vec::new(),
            map_view: false,
            tramp_late: 0,
            jump_pressed_now: false,
            prev_jump_tramp: false,
            player_trail: Vec::new(),
            exclamations: Vec::new(),
            held_by: None,
            prev_jump_claw: false,
            v3: v3::V3World::default(),
            pu: powerups::PuWorld::default(),
            wind: 0.0,
            wind_n: 0,
            options: Default::default(),
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
        self.clear.reset();
        self.score = self.base_score;
        self.coins = self.base_coins;
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
        self.area_timer = 0;
        self.blink_alt = false;
        self.keys = 0;
        self.pink.clear();
        self.unlocked_doors.clear();
        self.unlocked_boxes.clear();
        self.gone_boxes.clear();
        self.player_trail.clear();
        self.trees_shaken.clear();
        self.p_timer = 0;
        self.v3 = v3::V3World::default();
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
        self.custom_scroll = None;
        let (px, py) = (self.player.x, self.player.y);
        for (k, o) in self.others.iter_mut().enumerate() {
            *o = Player::new(px - 12.0 * (k + 1) as f32, py);
        }
        // course start: the view starts centred on the player, clamped to the area
        let area = self.camera_area();
        let (px, py) = (self.player.x, self.player.y);
        let level = &self.level;
        self.camera = Camera::start(&area, px, py, |x, y| level.cell(x, y) == Cell::Solid);
        let v = self.camera.view(&self.camera_area());
        self.autoscroll_pos = if self.level.vertical {
            v.bottom
        } else {
            v.left
        };
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
                spawned: false,
            })
            .collect();
        self.actors.clear();
        self.escort_down.clear();
        self.effects.clear();
        self.reset_generators();
        self.liquid = self.level.liquid.map(Liquid::new);
        self.snakes = self
            .level
            .snakes
            .iter()
            .map(|(l, y, n, fast, rev, codes)| Snake::new(*l, *y, *n, *fast, *rev, codes))
            .collect();
        self.exclamations = self
            .level
            .exclamations
            .iter()
            .map(|(t, codes)| crate::exclamation::ExclamationBlock::new(*t, codes))
            .collect();
    }

    fn add_actor(&mut self, mut a: Actor) {
        // SMM1 extras: Super Mushrooms of SMB1 courses become Weird / Mystery Mushrooms
        if a.kind == Kind::Mushroom && self.style == crate::player::Style::M1 {
            a.smm1_item = match self.options.smb1_mushroom {
                crate::options::MushroomSwap::Off => 0,
                crate::options::MushroomSwap::Weird => 1,
                crate::options::MushroomSwap::Mystery => 2,
            };
        }
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
            liquid: self.liquid,
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
                self.escort_down.clear();
                self.effects.clear();
                self.reset_generators();
                // the liquid waits where it was left: both area scenes are built at course start
                // (two calls of the scene ctor 0x71003fce30 in 0x7100ab7070, each with its own
                // liquid) and the course update runs the area update 0x71004005e0 for every area
                // each frame (0x7100aced20) [obs]; but the liquid only counts down its wait and
                // moves while its area's play flag (+0x88 → +0x40) is set, which the course
                // update writes on its current area (+0x2f8) only [inf: that +0x2f8 is the area
                // being played]
                self.liquid = st.liquid;
                self.snakes = self
                    .level
                    .snakes
                    .iter()
                    .map(|(l, y, n, fast, rev, codes)| Snake::new(*l, *y, *n, *fast, *rev, codes))
                    .collect();
                self.exclamations = self
                    .level
                    .exclamations
                    .iter()
                    .map(|(t, codes)| crate::exclamation::ExclamationBlock::new(*t, codes))
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
            s.spawned = false;
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

    /// A draw 0 ≤ r < n from the course generator: the next xorshift word scaled by n, as the
    /// game's callers do ((word × n) >> 32).
    fn course_rand(&mut self, n: u32) -> u32 {
        let s = &mut self.course_rng;
        let t = s[0] ^ (s[0] << 11);
        let w = s[3];
        let next = t ^ (t >> 8) ^ w ^ (w >> 19);
        *s = [s[1], s[2], w, next];
        ((next as u64 * n as u64) >> 32) as u32
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

    /// A run's counters for the course about to start (100 Mario): restarts begin from them.
    pub fn set_run_counters(&mut self, score: u32, coins: u32, lives: u32) {
        self.base_score = score;
        self.base_coins = coins % 100;
        self.score = score;
        self.coins = coins % 100;
        self.lives = lives;
        self.bank_lives = true;
    }

    fn add_life(&mut self) {
        self.lives_gained += 1;
        if !self.bank_lives {
            self.lives = (self.lives + 1).min(MAX_LIVES);
        }
        self.events.push(Event::OneUp);
    }

    /// A clear-condition count result: effects for counting up and for completing.
    fn condition_counted(&mut self, done: Option<bool>) {
        match done {
            Some(true) => {
                self.events.push(Event::ConditionCount);
                self.events.push(Event::ConditionMet);
            }
            Some(false) => self.events.push(Event::ConditionCount),
            None => {}
        }
    }

    /// The player flies a Koopa Clown Car (42-koopa-clown-car.md, player state 68): toward the
    /// pad direction at up to 2 px/f (diagonals normalised), accelerating 0.05/frame per axis,
    /// braking by 5 % when over speed; the Fire car shoots with Run. Jump climbs out [obs]: in
    /// the car the player runs the Wait body (state 68 exec 0x71009f5440 → 0x71009cd430),
    /// whose standing sub-states take the ordinary jump check (0x71009c4ed0 → 0x71009cf9c0 →
    /// jump 0x71009c1d50, launch 3.628 from rest); holding Down crouches in the car (sub-states
    /// 2–3, `stoop`), where that check isn't made, so Down + Jump does nothing.
    fn fly_clown_car(&mut self, input: Input) {
        let Some(id) = self.player.clown else { return };
        let Some(i) = self.actors.iter().position(|a| a.id == id && a.alive()) else {
            self.player.clown = None;
            return;
        };
        let jump = input.held(input::JUMP);
        let run = input.held(input::RUN);
        let jump_pressed = jump && !self.clown_prev.0;
        let run_pressed = run && !self.clown_prev.1;
        self.clown_prev = (jump, run);
        if jump_pressed && !input.held(input::DOWN) {
            self.player.clown = None;
            self.actors[i].phase = 0;
            // the standing jump launch (base 0x7101c8a648; no speed bonus inside the car)
            self.player.vy = 3.628;
            self.player.on_ground = false;
            self.events.push(Event::Jump);
            return;
        }
        let dx = input.dir_x() as f32;
        let dy = if input.held(input::UP) {
            1.0
        } else if input.held(input::DOWN) {
            -1.0
        } else {
            0.0
        };
        let n = (dx * dx + dy * dy).sqrt();
        let (tx, ty) = if n > 0.0 {
            (2.0 * dx / n, 2.0 * dy / n)
        } else {
            (0.0, 0.0)
        };
        let car = &mut self.actors[i];
        car.vx += (tx - car.vx).clamp(-0.05, 0.05);
        car.vy += (ty - car.vy).clamp(-0.05, 0.05);
        let v = (car.vx * car.vx + car.vy * car.vy).sqrt();
        if v > 2.0 {
            let k = (0.95 * v).max(2.0) / v;
            car.vx *= k;
            car.vy *= k;
        }
        if dx != 0.0 {
            car.dir = dx as i32;
        }
        let before = (car.x, car.y);
        car.collide(&self.level);
        car.moved = (car.x - before.0, car.y - before.1);
        let (cx, cy, dir, fire) = (
            car.x,
            car.y,
            car.dir,
            matches!(car.kind, Kind::ClownCar { fire: true }),
        );
        let p = &mut self.player;
        p.x = cx;
        p.y = cy + 14.0;
        p.vx = 0.0;
        p.vy = 0.0;
        p.on_ground = true;
        p.facing = dir;
        if fire && run_pressed {
            let mut f = Actor::new(Kind::Fireball, cx + dir as f32 * 18.0, cy + 10.0);
            f.dir = dir;
            f.vx = 4.0 * dir as f32;
            self.add_actor(f);
            self.events.push(Event::Fireball);
        }
    }

    /// Koopa Troopa Car with the player in it (72-koopa-troopa-car.md; KoopaCar states init
    /// 0x71006c7130) [obs]: a 60-frame start spin standing still (0x71006d0df0), then it runs
    /// on its own toward 4.0 px/f at 0.05 a frame (0x71006cc280 → 0x71006d0790; ×0.1 in the
    /// air while below that speed; slopes scale the target by the steepness table 0x7101d02c70),
    /// whatever the stick does. The player steers only by turning around: the turn brakes by
    /// 0.06 a frame to a stop while the car swings round 12° a frame, and another can't start
    /// for 30 frames (0x71006cc6a0 / 0x71006cc720). Jump hops it at vy 4.5 from the ground
    /// (0x71006d0fb0); Spin gets out. Enemies it runs into are knocked out; hits on the player
    /// damage the car (the third breaks it).
    fn drive_koopa_car(&mut self, input: Input) {
        let Some(id) = self.player.car else { return };
        let Some(i) = self.actors.iter().position(|a| a.id == id && a.alive()) else {
            self.player.car = None;
            return;
        };
        let jump = input.held(input::JUMP);
        let spin = input.held(input::SPIN);
        let jump_pressed = jump && !self.clown_prev.0;
        let spin_pressed = spin && !self.clown_prev.1;
        self.clown_prev = (jump, spin);
        if self.player.car_hit {
            self.player.car_hit = false;
            let car = &mut self.actors[i];
            car.hops += 1;
            self.events.push(Event::PowerDown);
            if car.hops >= 3 {
                car.die_fall(car.x);
                self.player.car = None;
                self.player.vy = 3.5;
                self.player.on_ground = false;
                return;
            }
        }
        if spin_pressed {
            self.player.car = None;
            self.actors[i].phase = 0;
            self.player.vy = 3.5;
            self.player.on_ground = false;
            self.events.push(Event::Jump);
            return;
        }
        let dir_in = input.dir_x();
        let slope = self.level.slope_at(self.actors[i].x).and_then(|(sy, s)| {
            ((self.actors[i].y - sy).abs() < 6.0).then_some((s.steep, s.rising))
        });
        let car = &mut self.actors[i];
        // `timer` counts the phase's frames; `hops` keeps the hits taken; `heading` the
        // frames left of the 30-frame lock against turning again
        if car.heading > 0.0 {
            car.heading -= 1.0;
        }
        match car.phase {
            1 => {
                // start spin: the wheels spin up in place for 60 frames
                car.vx = 0.0;
                car.timer += 1;
                if car.timer >= 60 {
                    car.phase = 2;
                    car.timer = 0;
                }
            }
            3 => {
                // turning: brake by 0.06 to a stop; the swing round takes 15 frames
                car.vx = crate::player::approach(car.vx, 0.0, 0.06);
                car.timer += 1;
                if car.timer >= 15 && car.vx == 0.0 {
                    car.phase = 2;
                    car.timer = 0;
                }
            }
            _ => {
                if dir_in == -car.dir && car.heading <= 0.0 {
                    car.phase = 3;
                    car.timer = 0;
                    car.dir = -car.dir;
                    car.heading = 30.0;
                } else {
                    let mut target = 4.0;
                    if let Some((steep, rising)) = slope {
                        // slope class by angle (< 20.3° / 35.8° / 54°): the gentle slope is
                        // class 2 (×1.25 down, ×0.6875 up), the steep one class 3 (×1.44 / ×0.5)
                        let downhill = (car.dir > 0) != rising;
                        target *= match (steep, downhill) {
                            (false, true) => 1.25,
                            (false, false) => 0.6875,
                            (true, true) => 1.44,
                            (true, false) => 0.5,
                        };
                    }
                    let mut acc = 0.05;
                    if !car.on_ground && car.vx.abs() < target {
                        acc *= 0.1;
                    }
                    car.vx = crate::player::approach(car.vx, target * car.dir as f32, acc);
                }
            }
        }
        if jump_pressed && car.on_ground {
            car.vy = 4.5;
            self.events.push(Event::Jump);
        }
        let g = if jump && car.vy > 0.0 { -0.2 } else { -0.4 };
        car.vy = (car.vy + g).max(crate::actors::MAX_FALL);
        let before = (car.x, car.y);
        car.collide(&self.level);
        car.moved = (car.x - before.0, car.y - before.1);
        if car.wall && car.phase == 2 {
            // bumping a wall turns it around [inf: RidePlayerRun (exec 0x71006cc280) has no
            // terrain-wall turn; its only bounce is the side-push callback (+0x7f0 =
            // 0x71006ca5f0: speed 0.75·|v| away, at least 2 px/f, facing kept, 30-frame lock),
            // which other actors' pushes send; what a terrain wall does to the car's speed is
            // the shared actor BG code, not traced]
            car.phase = 3;
            car.timer = 0;
            car.dir = -car.dir;
        }
        let (cx, cy, dir, ground, cb) = (car.x, car.y, car.dir, car.on_ground, car.bbox());
        let p = &mut self.player;
        p.x = cx;
        p.y = cy + 12.0;
        p.vx = 0.0;
        p.vy = 0.0;
        p.on_ground = ground;
        p.facing = dir;
        // what the car runs into is knocked out
        let mut hits = 0;
        for a in &mut self.actors {
            if a.id != id
                && a.kind.is_enemy()
                && a.alive()
                && a.solid_contact()
                && !matches!(
                    a.kind,
                    Kind::Bowser { .. }
                        | Kind::BoomBoom { .. }
                        | Kind::BowserJr { .. }
                        | Kind::Boo
                        | Kind::RingBoo { .. }
                        | Kind::Thwomp { .. }
                        | Kind::AngrySun { .. }
                )
                && overlap(cb, a.bbox())
            {
                a.die_fall(cx);
                hits += 1;
            }
        }
        for _ in 0..hits {
            self.events.push(Event::EnemyDown);
        }
    }

    /// Enemies riding a Koopa Clown Car stay on top of it (same link id, flag 0x200); the car
    /// chases the player while it has a rider.
    fn carry_clown_riders(&mut self) {
        let spawns = &self.level.spawns;
        let mut moves = Vec::new();
        for (ri, r) in self.actors.iter().enumerate() {
            let Some(rs) = r.spawn else { continue };
            let Some(rspawn) = spawns.get(rs) else {
                continue;
            };
            if !rspawn.clown
                || !r.alive()
                || matches!(
                    r.kind,
                    Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
                )
            {
                continue;
            }
            let car = self.actors.iter().position(|c| {
                matches!(c.kind, Kind::ClownCar { .. })
                    && c.alive()
                    && c.spawn
                        .and_then(|s| spawns.get(s))
                        .is_some_and(|s| s.link == rspawn.link)
            });
            if let Some(ci) = car {
                moves.push((ri, ci));
            }
        }
        for (ri, ci) in moves {
            let (cx, cy) = (self.actors[ci].x, self.actors[ci].y);
            if self.actors[ci].phase == 0 {
                self.actors[ci].phase = 1;
            }
            let r = &mut self.actors[ri];
            r.x = cx;
            r.y = cy + 24.0;
            r.vx = 0.0;
            r.vy = 0.0;
            r.on_ground = true;
        }
    }

    /// A carried item sits in front of the player; letting go of Run kicks it. A carried shell
    /// that touches an enemy defeats both. A shell that wakes up in the hands is dropped.
    fn carry_held(&mut self) {
        let Some(id) = self.player.held else { return };
        let Some(i) = self.actors.iter().position(|a| a.id == id && a.held) else {
            self.player.held = None;
            return;
        };
        if self.pu_carry(i) {
            return;
        }
        if self.actors[i].kind.is_v3() {
            self.v3_carry(i);
            return;
        }
        let p = &self.player;
        let (x, y, facing, pvx) = (p.x + p.facing as f32 * 11.0, p.y + 2.0, p.facing, p.vx);
        let crate_held = matches!(self.actors[i].kind, Kind::Crate { .. } | Kind::Stone);
        let heavy = self.actors[i].kind == Kind::Stone;
        let release = !self.run_held
            || !self.player.alive()
            || (self.actors[i].mode != Mode::ShellWait && !crate_held);
        if release && crate_held {
            // thrown forward [obs]: the map-object carry action's release message (slot
            // 0x7100852140, release kind 1) launches at (|thrower speed| + 3, or + 1 when the
            // thrower moves against the throw) × 0.9 (0x7101c8a540) × the object's throw scale,
            // vy 2 × its lift scale (both 1 for the shared action, 0x71008548b0), max fall −4;
            // it breaks on a wall or an enemy
            let a = &mut self.actors[i];
            a.held = false;
            a.x = x;
            if heavy {
                // the stone is heavy: a short slide (≤ 0.8 px/f)
                a.vx = 0.8 * facing as f32;
                a.vy = 1.0;
            } else {
                let against = pvx * (facing as f32) < 0.0;
                let base = pvx.abs() + if against { 1.0 } else { 3.0 };
                a.vx = base * 0.9 * facing as f32;
                a.vy = 2.0;
            }
            a.phase = 1;
            a.dir = facing;
            self.player.held = None;
            self.events.push(Event::Kick);
            return;
        }
        if release {
            self.actors[i].held = false;
            self.player.held = None;
            if self.actors[i].mode == Mode::ShellWait && self.player.alive() {
                self.actors[i].x = x;
                self.actors[i].kick(facing, pvx);
                self.events.push(Event::Kick);
            }
            return;
        }
        {
            let a = &mut self.actors[i];
            a.x = x;
            a.y = y;
            a.vx = 0.0;
            a.vy = 0.0;
            // carried, the shell's wake timer doesn't run (the Carried context replaces the
            // Shell one), and ShellWait enter (0x71005f6050) sets it back to 511 when it is let
            // go [obs]
            a.timer = crate::actors::SHELL_WAKE;
        }
        let sb = self.actors[i].bbox();
        let hit = self
            .actors
            .iter()
            .position(|e| e.id != id && e.alive() && e.kind.is_enemy() && overlap(sb, e.bbox()));
        if let Some(j) = hit {
            self.actors[j].die_fall(x);
            self.actors[i].held = false;
            self.actors[i].die_fall(x - facing as f32);
            self.player.held = None;
            self.events.push(Event::EnemyDown);
        }
    }

    /// Crates: pick one up by standing beside it with Run held (not in SMB1); a thrown crate
    /// that hits an enemy knocks it out and breaks; a broken crate releases its contents
    /// upward (vy 3.5).
    fn step_crates(&mut self) {
        let p = &self.player;
        if self.style.can_carry()
            && self.run_held
            && p.held.is_none()
            && p.on_ground
            && p.control == Control::Normal
            && p.yoshi.is_none()
        {
            let (px, py, facing) = (p.x, p.y, p.facing);
            if let Some(a) = self.actors.iter_mut().find(|a| {
                matches!(a.kind, Kind::Crate { .. } | Kind::Stone)
                    && a.alive()
                    && !a.held
                    && a.phase == 0
                    && (a.y - py).abs() < 4.0
                    && (a.x - px) * facing as f32 > 0.0
                    && (a.x - px).abs() <= 16.0
            }) {
                a.held = true;
                self.player.held = Some(a.id);
            }
        }
        // enemies landing on a Mushroom Trampoline bounce (EnemyDB jump-step: 5.5, 6.5 winged)
        let tramps: Vec<(f32, f32, f32)> = self
            .actors
            .iter()
            .filter_map(|a| match a.kind {
                Kind::Trampoline { width, .. } => Some((
                    a.x - width as f32 * 8.0,
                    a.x + width as f32 * 8.0,
                    a.y + 16.0,
                )),
                Kind::OnOffTrampoline { width, .. } if a.v3_bouncy(self.on_off) => Some((
                    a.x - width as f32 * 8.0,
                    a.x + width as f32 * 8.0,
                    a.y + 16.0,
                )),
                _ => None,
            })
            .collect();
        for a in &mut self.actors {
            if a.kind.is_enemy()
                && a.alive()
                && a.on_ground
                && tramps
                    .iter()
                    .any(|&(x0, x1, top)| a.x > x0 && a.x < x1 && (a.y - top).abs() < 1.5)
            {
                a.vy = if a.wings { 6.5 } else { 5.5 };
                a.on_ground = false;
            }
        }
        let mut broken = Vec::new();
        for ci in 0..self.actors.len() {
            let c = &self.actors[ci];
            let Kind::Crate { contents } = c.kind else {
                continue;
            };
            if !c.alive() {
                continue;
            }
            let mut hit = c.phase == 2;
            if c.phase == 1 {
                let cb = c.bbox();
                for j in 0..self.actors.len() {
                    let e = &self.actors[j];
                    if j != ci && e.kind.is_enemy() && e.alive() && overlap(cb, e.bbox()) {
                        let cx = self.actors[ci].x;
                        self.actors[j].die_fall(cx);
                        self.events.push(Event::EnemyDown);
                        hit = true;
                        break;
                    }
                }
            }
            if hit {
                let c = &mut self.actors[ci];
                c.mode = Mode::Dead;
                broken.push((c.x, c.y, contents));
            }
        }
        for (x, y, contents) in broken {
            self.effects.extend(Effect::debris(x, y, 0.0, 0.1));
            self.events.push(Event::Break);
            let kind = match contents {
                None => continue,
                Some(Contents::Coin) => {
                    self.effects.push(Effect::coin_pop(x, y, false));
                    self.events.push(Event::Coin);
                    continue;
                }
                Some(Contents::Mushroom) => Kind::Mushroom,
                Some(Contents::Power(w)) => self.pu_item(w),
                Some(Contents::Special) => self.style_item(),
                Some(Contents::FireFlower) => Kind::FireFlower,
                Some(Contents::Star) => Kind::Star,
                Some(Contents::OneUp) => Kind::OneUp,
                Some(Contents::Goomba) => Kind::Goomba,
                Some(Contents::Koopa { red }) => Kind::Koopa { red },
                Some(Contents::YoshiEgg) => Kind::YoshiEgg { red: false },
            };
            let mut a = Actor::new(kind, x, y);
            a.dir = 1;
            a.vy = 3.5;
            self.add_actor(a);
        }
    }

    /// Yoshi follows his rider; a dismount leaves him standing, a hit sends him running away.
    /// Run while riding works the tongue (or Red Yoshi's fire, 60-frame cooldown).
    fn step_yoshi_rider(&mut self) {
        let run = self.run_held;
        let run_pressed = run && !self.yoshi_prev_run;
        self.yoshi_prev_run = run;
        if self.yoshi_fire_cd > 0 {
            self.yoshi_fire_cd -= 1;
        }
        let p = &self.player;
        let (px, py, facing, pvx) = (p.x, p.y, p.facing, p.vx);
        if p.yoshi_off {
            let hit = p.yoshi_hit;
            self.player.yoshi_off = false;
            self.player.yoshi_hit = false;
            for a in &mut self.actors {
                if matches!(a.kind, Kind::Yoshi { .. }) && a.held {
                    a.held = false;
                    a.x = px;
                    a.y = py - if hit { 0.0 } else { 8.0 };
                    a.vy = 0.0;
                    a.vx = if hit { 0.0 } else { pvx };
                    a.dir = facing;
                    a.phase = hit as u8;
                }
            }
            self.yoshi_mouth = None;
            self.tongue = 0;
            return;
        }
        let Some(id) = self.player.yoshi else { return };
        let Some(i) = self.actors.iter().position(|a| a.id == id) else {
            self.player.yoshi = None;
            return;
        };
        {
            let y = &mut self.actors[i];
            y.x = px;
            y.y = py;
            y.dir = facing;
            y.vx = pvx;
        }
        let red = matches!(self.actors[i].kind, Kind::Yoshi { red: true });
        if run_pressed {
            if red {
                if self.yoshi_fire_cd == 0 {
                    self.yoshi_fire_cd = 60;
                    self.spit_fire(px, py, facing, pvx, 1);
                }
            } else if let Some(k) = self.yoshi_mouth.take() {
                self.spit(k, px, py, facing, pvx);
            } else if self.tongue == 0 {
                self.tongue = 10;
                self.events.push(Event::Kick);
            }
        }
        if self.tongue > 0 {
            self.tongue -= 1;
            // the tip goes out and back over 10 frames, up to 32 px [inf: the tongue contact
            // (0x7100a2e780 → 0x7100a2e9b0) tests from the tongue-tip point the model gives
            // (vtable +0x138: a bone of the `eat` animation); the reach lives in the Yoshi
            // model's animation data, not in code]
            let t = 10 - self.tongue;
            let reach = 32.0 * (1.0 - ((t as f32 - 5.0).abs() / 5.0));
            let x0 = px + facing as f32 * 8.0;
            let x1 = x0 + facing as f32 * reach.max(4.0);
            let tip = (x0.min(x1), py + 6.0, x0.max(x1), py + 18.0);
            let hit = self.actors.iter().position(|a| {
                a.id != id && a.alive() && a.solid_contact() && !a.held && overlap(tip, a.bbox())
            });
            if let Some(j) = hit {
                self.tongue = 0;
                self.eat(j);
            }
        }
    }

    /// Yoshi's tongue caught actor `j` (yoshi.md §5.3 eat reactions, simplified): eaten
    /// (GulpDown; a coin counts), held to spit (SpitOut enemies, fire, bones), or a power-up
    /// for the rider [inf: EnemyDB marks power-ups SpitOut, but the mouth acts on the caught
    /// actor's own eat handler (actor +0x338: type +0x24, swallow +0x30, replace +0x08;
    /// catch 0x7100a24920), and the item handlers weren't found, so whether an item is held
    /// and spat or swallowed into the rider is open]; Invalid things are left alone.
    fn eat(&mut self, j: usize) {
        let a = &self.actors[j];
        if a.big {
            return;
        }
        let (ax, ay, kind) = (a.x, a.y, a.kind);
        match kind {
            Kind::Goomba
            | Kind::Goombrat
            | Kind::ShoeGoomba { .. }
            | Kind::Piranha { fire: false }
            | Kind::HammerBro
            | Kind::BulletBill
            | Kind::MontyMole
            | Kind::BobOmb
            | Kind::SpikeTop { .. }
            | Kind::Lakitu
            | Kind::Magikoopa
            | Kind::Blooper
            | Kind::Wiggler
            | Kind::CheepCheep { .. }
            | Kind::ChainChomp { .. } => {
                self.actors[j].mode = Mode::Dead;
                self.add_score(200, ax, ay + 16.0);
                self.events.push(Event::Stomp);
            }
            Kind::Koopa { .. } if self.style == crate::player::Style::MW => {
                self.actors[j].mode = Mode::Dead;
                self.add_score(200, ax, ay + 16.0);
                self.events.push(Event::Stomp);
            }
            Kind::FloatCoin => {
                self.actors[j].mode = Mode::Dead;
                self.add_coin();
            }
            Kind::Koopa { .. } | Kind::Shell { .. } | Kind::Spiny | Kind::Buzzy => {
                self.actors[j].mode = Mode::Dead;
                self.actors[j].tallied = true;
                self.yoshi_mouth = Some(kind);
            }
            Kind::Piranha { fire: true } | Kind::LavaBubble => {
                self.actors[j].mode = Mode::Dead;
                self.yoshi_mouth = Some(Kind::EnemyFire);
            }
            Kind::DryBones | Kind::FishBone => {
                self.actors[j].mode = Mode::Dead;
                self.yoshi_mouth = Some(Kind::Hammer);
            }
            Kind::Mushroom
            | Kind::FireFlower
            | Kind::Star
            | Kind::OneUp
            | Kind::StyleItem
            | Kind::SuperHammer => {
                self.actors[j].mode = Mode::Dead;
                let sub = self.actors[j].smm1_item;
                self.collect_item(kind, ax, ay, sub);
            }
            _ => {}
        }
    }

    /// Spit what Yoshi holds: a shell kicked forward, or 3 fireballs / bones (yoshi.md §5.5:
    /// speed |v|·0.35 + 3.25 (bones + 2.0) at 0 and ±15°).
    fn spit(&mut self, k: Kind, px: f32, py: f32, facing: i32, pvx: f32) {
        match k {
            Kind::EnemyFire => self.spit_fire(px, py, facing, pvx, 3),
            Kind::Hammer => {
                for dy in [-0.54f32, 0.0, 0.54] {
                    let mut b = Actor::new(Kind::Fireball, px + facing as f32 * 12.0, py + 12.0);
                    b.dir = facing;
                    b.vx = (pvx.abs() * 0.35 + 2.0) * facing as f32;
                    b.vy = dy;
                    self.add_actor(b);
                }
            }
            _ => {
                let red = matches!(k, Kind::Koopa { red: true } | Kind::Shell { red: true });
                let mut s = Actor::new(Kind::Shell { red }, px + facing as f32 * 16.0, py + 2.0);
                s.buzzy = matches!(k, Kind::Buzzy);
                s.tallied = true;
                s.kick(facing, pvx);
                self.add_actor(s);
            }
        }
        self.events.push(Event::Kick);
    }

    fn spit_fire(&mut self, px: f32, py: f32, facing: i32, pvx: f32, n: usize) {
        let speed = pvx.abs() * 0.35 + 3.25;
        let angles: &[f32] = if n == 1 { &[0.0] } else { &[-15.0, 0.0, 15.0] };
        for &deg in angles {
            let (s, c) = deg.to_radians().dsin_cos();
            let mut f = Actor::new(Kind::Fireball, px + facing as f32 * 12.0, py + 12.0);
            f.dir = facing;
            f.vx = speed * c * facing as f32;
            f.vy = speed * s;
            self.add_actor(f);
        }
        self.events.push(Event::Fireball);
    }

    /// Yoshi's body and head sprites (SMW draws him in two parts placed by their quads).
    fn yoshi_sprites(&self, out: &mut Vec<Sprite>) {
        for a in &self.actors {
            let Kind::Yoshi { red } = a.kind else {
                continue;
            };
            let model = if red {
                "M1_Player_Yoshi_Red"
            } else {
                "M1_Player_Yoshi"
            };
            let ridden = a.held;
            let moving = a.vx.abs() > 0.1;
            let k3 = (self.tick as usize / 4) % 3;
            let (body, head) = if ridden && !self.player.on_ground {
                (
                    ["jump_body.0", "jump_body.1", "jump_body.2"][k3.min(2)],
                    "jump_head.0",
                )
            } else if moving || a.phase == 1 {
                (
                    ["run_body.0", "run_body.1", "run_body.2"][k3],
                    "wait_head.0",
                )
            } else {
                ("wait_body.0", "wait_head.0")
            };
            let head = if ridden && self.tongue > 0 {
                "eat_head.2"
            } else {
                head
            };
            let flip = a.dir < 0; // the art faces right
            // the body and head textures are laid out to compose when drawn at the same place
            // (the model's bone placement isn't applied)
            for tex in [body, head] {
                let mut sp = Self::model_sprite(model, tex, a.x - 16.0, a.y, 32.0, 32.0, flip, 34);
                sp.image.rect = Some((0.0, 0.0, 32.0, 32.0));
                out.push(sp);
            }
        }
    }

    /// A defeated shoe Goomba leaves its empty shoe behind (unless it fell into lava).
    fn drop_shoes(&mut self) {
        let mut shoes = Vec::new();
        for a in &mut self.actors {
            if let Kind::ShoeGoomba { heels } = a.kind
                && !a.alive()
                && a.chain == 0
            {
                a.chain = 1;
                let mut s = Actor::new(Kind::Shoe { heels }, a.x, a.y);
                s.big = a.big;
                shoes.push(s);
            }
        }
        for s in shoes {
            self.add_actor(s);
        }
    }

    /// Defeated enemies count toward a "defeat N" condition once each (placed objects by their
    /// spawn record; others every time).
    fn tally_defeats(&mut self) {
        let mut results = Vec::new();
        for a in &mut self.actors {
            if a.tallied {
                continue;
            }
            let shelled = matches!(a.kind, Kind::Shell { .. });
            if a.alive() && !shelled {
                continue;
            }
            a.tallied = true;
            if let Some(class) = a.kind.defeat_class(a.buzzy) {
                let key = a.spawn.map(|s| (self.area as u32) << 16 | s as u32);
                results.push(self.clear.defeated(class, key));
            }
        }
        for r in results {
            self.condition_counted(r);
        }
    }

    fn add_coin(&mut self) {
        let r = self.clear.coin(1);
        self.condition_counted(r);
        self.coins += 1;
        self.score += COIN_SCORE;
        self.events.push(Event::Coin);
        if self.coins >= 100 {
            self.coins -= 100;
            self.add_life();
        }
    }

    /// Chain score for a kill after `n` earlier ones in the same chain. Every chain counter
    /// is bumped (capped at 8) before its score is read (0x7100644920 and its siblings), so the
    /// first kill is score id 1: 200, 400 … 8000, then 1-UP from the 8th on [obs 0x7100644610,
    /// id table 0x7101d0114c, points 0x7101cfd924].
    fn chain_score(n: u32) -> u32 {
        SCORE_TABLE.get(n as usize + 1).copied().unwrap_or(ONE_UP)
    }

    pub fn step(&mut self, input: Input) {
        let input = if self.upside_down() {
            // swap Up and Down
            let b = input.buttons & !(input::UP | input::DOWN);
            let up = input.held(input::UP);
            let down = input.held(input::DOWN);
            Input {
                buttons: b | if up { input::DOWN } else { 0 } | if down { input::UP } else { 0 },
            }
        } else {
            input
        };
        self.events.clear();
        self.fx.clear();
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
        let prev_vy = self.player.vy;
        let was_ground = self.player.on_ground;
        let jump_held = input.held(input::JUMP);
        self.run_held = input.held(input::RUN);
        self.player.style = self.style;
        self.player.on_ice = self.ice_under_player();
        self.player.low_gravity = self.level.theme == 8 && self.level.night;
        self.step_wind();
        self.player.wind = if self.player.alive() { self.wind } else { 0.0 };
        if let Some(l) = &mut self.liquid {
            l.step();
        }
        self.player.in_water = self
            .liquid
            .is_some_and(|l| l.water() && l.covers(self.player.y + 8.0))
            || (self.level.theme == 2 && self.level.night); // castle night: swim in the air
        self.jump_pressed_now = jump_held && !self.prev_jump_tramp;
        self.prev_jump_tramp = jump_held;
        self.step_platforms(jump_held, input.dir_x());
        self.conveyor_carry();
        let prev_x = self.player.x;
        if self.player.clown.is_some() {
            self.fly_clown_car(input);
        } else if self.player.car.is_some() {
            self.drive_koopa_car(input);
        } else {
            self.pu_sense();
            self.player.step(input, &self.level);
        }
        self.v3_dash(prev_x);
        let shoe_safe = self.player.shoe.is_some() && self.player.on_ground;
        if self.player.spiked && !shoe_safe && self.player.hurt() {
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
            })
            .or_else(|| {
                let (keys, area) = (self.keys, self.area);
                let (unlocked, gone) = (&self.unlocked_boxes, &self.gone_boxes);
                warp::box_entry(
                    &self.player,
                    |w| {
                        let pair = (area, w.id >> 1);
                        !gone.contains(&pair)
                            && (w.kind == crate::level::WarpKind::Box { key: false }
                                || keys > 0
                                || unlocked.contains(&pair))
                    },
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
                    if let crate::level::WarpKind::Box { key } = from.kind {
                        if key && self.unlocked_boxes.insert((self.area, from.id >> 1)) {
                            self.keys -= 1;
                        }
                        // held items don't travel: dropped where the player was
                        if let Some(id) = self.player.held.take()
                            && let Some(a) = self.actors.iter_mut().find(|a| a.id == id)
                        {
                            a.held = false;
                        }
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
        self.smm1_player_events();
        self.check_hidden_blocks(prev_head);
        if prev_vy > 0.0 && self.player.vy <= 0.0 {
            self.hit_exclamation_blocks();
        }
        self.side_springs(input.held(input::DOWN));
        self.shake_trees();
        self.mega_breaks_bricks();
        if let Some((tx, ty)) = self.player.head_hit {
            self.head_hit(tx, ty);
        }
        if self.player.throw {
            if self.player.form == Form::Superball {
                self.throw_superball();
            } else {
                self.throw_fireball();
            }
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
                if a.alive()
                    && l.covers(a.y + 2.0)
                    && !a.kind.is_platform()
                    && a.kind != Kind::LavaBubble
                    && !a.kind.liquid_proof()
                {
                    a.mode = Mode::Dead;
                }
            }
        }
        self.snuff_superballs();
        self.collect_coins();
        self.step_donuts();
        self.step_p_switch();
        self.step_v3();
        self.step_powerups();
        self.step_area_timer();

        // blocks, actors, effects
        for r in self.blocks.step() {
            self.release(r);
        }
        let px = self.player.x;
        let py = self.player.y;
        let mut born = Vec::new();
        self.carry_held();
        self.step_crates();
        // Charvaarghs take turns and space their leaps (3D World)
        self.w3_charvaarghs();
        // the escort Toad's target: 32 px back along the player's path
        if self
            .player_trail
            .last()
            .is_none_or(|&(x, y)| (x - px).abs() + (y - py).abs() > 0.5)
        {
            self.player_trail.push((px, py));
            if self.player_trail.len() > 512 {
                self.player_trail.remove(0);
            }
        }
        let escort_target = {
            let mut left = 32.0;
            let mut at = (px, py);
            for w in self.player_trail.windows(2).rev() {
                let d = (w[1].0 - w[0].0).dhypot(w[1].1 - w[0].1);
                at = w[0];
                if d >= left {
                    let k = left / d.max(1e-3);
                    at = (
                        w[1].0 + (w[0].0 - w[1].0) * k,
                        w[1].1 + (w[0].1 - w[1].1) * k,
                    );
                    break;
                }
                left -= d;
            }
            at
        };
        // SMM1 shell physics: shells kicked by the mid-air wall assist
        self.step_shell_assist();
        let masked = self.player.pu.headdress == Some(crate::player::Headdress::Goomba);
        // the throwers' live children (a Lakitu's spawn limiter, actors.rs `step_lakitu`)
        let parents: Vec<u32> = self
            .actors
            .iter()
            .filter(|a| a.alive())
            .filter_map(|a| a.parent)
            .collect();
        for a in &mut self.actors {
            if a.held {
                continue;
            }
            if a.kind.is_platform() {
                continue; // already moved before the player
            }
            if a.kind == Kind::Lakitu {
                a.children = parents.iter().filter(|&&p| p == a.id).count() as u32;
            }
            a.target = if a.kind == Kind::EscortToad {
                escort_target
            } else {
                (px, py)
            };
            a.target_facing = self.player.facing;
            powerups::mask_target(a, masked);
            a.camera = (self.camera_x, self.camera_y);
            a.floor_ref = self.liquid.map(|l| l.y).unwrap_or(self.level.bottom - 12.0);
            if a.kind == Kind::Lakitu {
                a.speed = self.player.vx;
            }
            if matches!(a.kind, Kind::CheepCheep { .. } | Kind::FishBone) {
                a.ridden = self
                    .liquid
                    .is_some_and(|l| l.water() && l.covers(a.y + 16.0));
            }
            w3::sense(a, self.liquid, &self.player);
            if let Some(n) = a.step_spawning(&self.level) {
                born.push(n);
            }
        }
        self.explosions();
        for n in born {
            self.add_actor(n);
        }
        self.koopaling_world();
        self.step_generators();
        self.step_blasters();
        self.fire_hazards();
        self.step_cannons();
        self.twister_lift();
        self.magikoopas();
        self.claws(input);
        self.pow_quake();
        self.cape_quake();
        // a Sledge Bro landing on screen stuns grounded players for 100 frames (3-hammer-bro.md,
        // QuakeDamage 0x71009d5680 / 0x71009d5910) [obs]
        let v = self.view_rect(0.0);
        if self.actors.iter().any(|a| {
            a.kind == Kind::HammerBro
                && a.alt
                && a.kick_lock == 50
                && a.x > v.0
                && a.x < v.2
                && a.y > v.1
                && a.y < v.3
        }) {
            if self.player.on_ground && self.player.star == 0 && self.player.form != Form::Balloon {
                self.player.stun = 100;
                self.player.vx = 0.0;
            }
            self.events.push(Event::Bump);
        }
        self.bump_moving_block();
        self.tail_attack();
        self.pound_blocks();
        self.step_yoshi_rider();
        self.check_checkpoints();
        self.shell_walls();
        self.actor_interactions();
        self.player_interactions(jump_held);
        self.step_effects();
        self.carry_clown_riders();
        self.boss_keys();
        self.tally_defeats();
        self.drop_shoes();
        self.despawn();
        self.actors.retain(|a| a.mode != Mode::Dead);
        // action conditions: damage taken this frame, landings
        if self
            .events
            .iter()
            .any(|e| matches!(e, Event::PowerDown | Event::Death))
        {
            self.clear.damaged();
        }
        self.clear.ground(self.player.on_ground);
        if self.held_by.is_some() {
            self.clear.held_by_claw();
        }
        if self.player.in_water {
            self.clear.in_water();
        }
        self.escort_toad_comeback();

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
        self.autoscroll_push();
        self.activate();
    }

    /// The escort Toad knocked out doesn't fail the run [obs ChaseKinopio states, init
    /// 0x7100879060]: Dead (0x710087a6a0, `dead_pose`) lasts 45 frames (exec 0x710087a7c0), then
    /// Balloon (0x710087a7f0) carries him in a bubble back to the players. The bubble's flight
    /// isn't modelled: he comes back beside player 1 when the 45 frames are up.
    fn escort_toad_comeback(&mut self) {
        for a in &self.actors {
            if a.kind == Kind::EscortToad
                && !a.alive()
                && !self.escort_down.iter().any(|&(id, _)| id == a.id)
            {
                self.escort_down.push((a.id, 45));
            }
        }
        let mut back = Vec::new();
        self.escort_down.retain_mut(|(id, t)| {
            *t -= 1;
            if *t > 0 {
                return true;
            }
            back.push(*id);
            false
        });
        for id in back {
            if let Some(a) = self.actors.iter_mut().find(|a| a.id == id) {
                a.mode = Mode::Dead;
            }
            let p = &self.player;
            let mut t = Actor::new(Kind::EscortToad, p.x - 16.0 * p.facing as f32, p.y);
            t.dir = p.facing;
            self.add_actor(t);
        }
    }

    /// Local multiplayer: `n` players in all (1–4). The extra players join beside player 1.
    pub fn set_players(&mut self, n: usize) {
        let n = n.clamp(1, 4) - 1;
        let (px, py) = (self.player.x, self.player.y);
        self.others = (0..n)
            .map(|k| Player::new(px - 12.0 * (k + 1) as f32, py))
            .collect();
        self.other_chars = (1..=n).collect();
    }

    /// One tick with an input per player (player 1 first). Extra players run the same player
    /// rules — physics, coins, items, enemies, the goal — swapped into player 1's place one at a
    /// time. One who dies, or drops out of the view, comes back beside player 1 [inf: SMM2 puts
    /// them in a bubble — player state 97 Balloon (0x71009c10e0 / 0x71009c12d0), which belongs
    /// in player.rs]; any player reaching the goal clears the course for everyone.
    pub fn step_players(&mut self, inputs: &[Input]) {
        self.step(inputs.first().copied().unwrap_or_default());
        let mut i = 0;
        while i < self.others.len() {
            let input = inputs.get(i + 1).copied().unwrap_or_default();
            let goal_before = self.goal_phase != GoalPhase::None;
            std::mem::swap(&mut self.player, &mut self.others[i]);
            self.step_extra_player(input);
            let took_goal = !goal_before && self.goal_phase != GoalPhase::None;
            if took_goal {
                // this player finishes the course: they become player 1 for the goal demo
                std::mem::swap(&mut self.main_char, &mut self.other_chars[i]);
            } else {
                std::mem::swap(&mut self.player, &mut self.others[i]);
            }
            i += 1;
        }
    }

    /// The per-player part of a tick for an extra player (in `self.player` meanwhile).
    fn step_extra_player(&mut self, input: Input) {
        if self.goal_phase != GoalPhase::None || self.transit.is_some() {
            return;
        }
        let jump_held = input.held(input::JUMP);
        self.player.style = self.style;
        self.player.in_water = self
            .liquid
            .is_some_and(|l| l.water() && l.covers(self.player.y + 8.0));
        self.player.step(input, &self.level);
        if !self.player.alive() {
            if self.player.restart_due() {
                self.rejoin();
            }
            return;
        }
        if self.player.spiked && self.player.hurt() {
            self.events.push(Event::PowerDown);
        }
        self.collect_coins();
        self.player_interactions(jump_held);
        self.check_goal();
        let v = self.view_rect(64.0);
        let p = &self.player;
        if p.x < v.0 || p.x > v.2 || p.y < v.1 - 64.0 {
            self.rejoin();
        }
    }

    /// An extra player (in `self.player`) comes back beside player 1 (held in `others`).
    fn rejoin(&mut self) {
        let lead = self
            .others
            .iter()
            .find(|o| o.alive())
            .map(|o| (o.x, o.y))
            .unwrap_or(self.level.start);
        let form = self.player.form;
        self.player = Player::new(lead.0, lead.1 + 32.0);
        if self.player.alive() && form != Form::Small {
            self.player.form = form;
        }
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

    /// A player reaching a tree's canopy (106-tree.md climb zone: trunk ± 18 px, +8 px margin,
    /// the canopy band, +16 px) knocks its item out: up at vy 4.5 from 40 px above the origin
    /// [obs; the trigger itself is inferred].
    fn shake_trees(&mut self) {
        // a tree gives up its item, and counts for "shake N trees", when a player climbs its
        // trunk to the top: entering TreeClimbTop (0x7100a15830) sets the tree's per-player
        // release bit and posts the clear-condition event 0x4b with the tree's record [obs];
        // (posted on every entry, 0x7100418310); BellTree is condition kind 8, which the
        // condition manager de-duplicates by placed object (0x7100417fe0): once per tree [obs]
        let py = self.player.y;
        let climbing = self.player.climbing.map(|(vx, _)| vx);
        let mut out = Vec::new();
        let mut shaken = 0;
        for (ti, t) in self.level.trees.iter().enumerate() {
            let (bottom, top) = t.canopy();
            if climbing == Some(t.x)
                && py + 16.0 >= top - 1.0
                && self.trees_shaken.insert((self.area, ti))
            {
                shaken += 1;
                if let Some(c) = t.contents {
                    out.push((t.x, (bottom + 8.0).max(t.y + 40.0), c));
                }
            }
        }
        if self.clear.goal == crate::clear::Goal::BellTrees {
            for _ in 0..shaken {
                let r = self.clear.count(None, 1);
                self.condition_counted(Some(r));
            }
        }
        for (x, y, c) in out {
            let kind = match c {
                Contents::Coin => {
                    self.effects.push(Effect::coin_pop(x, y, false));
                    self.events.push(Event::Coin);
                    continue;
                }
                Contents::Special if self.pu_small() => Kind::Mushroom,
                Contents::FireFlower if self.pu_small() => Kind::Mushroom,
                Contents::Power(w) => self.pu_block_item(w),
                Contents::Mushroom => Kind::Mushroom,
                Contents::Special => self.style_item(),
                Contents::FireFlower => Kind::FireFlower,
                Contents::Star => Kind::Star,
                Contents::OneUp => Kind::OneUp,
                Contents::Goomba => Kind::Goomba,
                Contents::Koopa { red } => Kind::Koopa { red },
                Contents::YoshiEgg => Kind::YoshiEgg { red: false },
            };
            let mut a = Actor::new(kind, x, y);
            a.dir = 1;
            a.vy = 4.5;
            a.vx = 0.0;
            self.add_actor(a);
            self.events.push(Event::ItemAppear);
        }
    }

    /// Blooper Nanny babies sit on her path, 8 + 12·k frames back (8 + 8·k winged); without
    /// her they go too [obs]: the babies are the Nanny's own sub-models and collision bodies
    /// (component init 0x7100668ba0, trail 0x7100669120), not actors of their own.
    fn place_baby_bloopers(&mut self) {
        let mut moves = Vec::new();
        for (i, b) in self.actors.iter().enumerate() {
            let Kind::BabyBlooper { slot } = b.kind else {
                continue;
            };
            if !b.alive() {
                continue;
            }
            let mum = self
                .actors
                .iter()
                .find(|m| m.id == b.chain && m.alive() && m.kind == Kind::Blooper);
            let delay = if b.wings {
                8 + 8 * slot as usize
            } else {
                8 + 12 * slot as usize
            };
            moves.push((
                i,
                mum.map(|m| {
                    let n = m.trail.len();
                    m.trail
                        .get(n.saturating_sub(1 + delay))
                        .copied()
                        .unwrap_or((m.x, m.y + 8.0))
                }),
            ));
        }
        for (i, at) in moves {
            match at {
                Some((x, y)) => {
                    let b = &mut self.actors[i];
                    b.dir = if x < b.x { -1 } else { 1 };
                    b.moved = (x - b.x, y - 4.0 - b.y);
                    b.x = x;
                    b.y = y - 4.0;
                }
                None => {
                    let x = self.actors[i].x;
                    self.actors[i].die_fall(x);
                }
            }
        }
    }

    /// Big Mario breaks the bricks he walks into [inf: the v1.0.0 player code has the Mega form
    /// (6; scale 2.0 0x71009cca10): its update 0x71009c25c0 sets terrain-sensor flags +0x22c8
    /// |= 0x4000 (and 0x40000000 / 0x80000000) and, when moving, +0x22cc |= 1, which the shared
    /// BG collision acts on; which blocks and sides break there isn't traced].
    fn mega_breaks_bricks(&mut self) {
        let p = &self.player;
        if p.form != Form::Mega || p.vx.abs() < 0.5 {
            return;
        }
        let dir = p.vx.signum();
        let tx = ((p.x + dir * 13.0) / TILE).floor() as i32;
        let (y0, y1) = (
            ((p.y + 4.0) / TILE).floor() as i32,
            ((p.y + 19.0) / TILE).floor() as i32,
        );
        for ty in y0..=y1 {
            if self
                .blocks
                .get(tx, ty)
                .is_some_and(|b| b.kind == BlockKind::Brick)
            {
                self.hit_block(tx, ty, true, HitPower::Break);
            }
        }
    }

    /// Sideways springs push a player touching their side away (JumpStepSide 0x71008af470 →
    /// player 0x71009f37e0) [obs].
    fn side_springs(&mut self, down: bool) {
        let p = &self.player;
        let (_, y0, _, y1) = p.hurt_box();
        // the push comes from the spring's side colliders: the player's wall probe meeting a
        // side (0x710085e8e0 → 0x71008b0f90)
        let reach = p.sensors().wall_x + 1.0;
        let mut push = None;
        for a in &mut self.actors {
            if a.kind != Kind::SideSpring || a.pause > 6 {
                continue;
            }
            let touching = y1 > a.y + 2.0
                && y0 < a.y + 14.0
                && p.x + reach >= a.x - 8.0
                && p.x - reach <= a.x + 8.0;
            if touching {
                a.pause = 12;
                push = Some(if p.x < a.x { -1.0 } else { 1.0 });
                break;
            }
        }
        if let Some(dir) = push {
            let p = &mut self.player;
            // the push (player vf 0x1c0 = 0x71009f37e0) [obs]: speed 3.0 away from the spring
            // (2.0 in water); a fourth push in a row with Down held only 1.0 (water 0.1). The
            // player keeps their state and normal control. Pinned against a wall on the push
            // side they bounce back instead: facing reversed, 0.7 × the speed, vy 2.5.
            let chained = p.side_push > 0;
            p.side_chain = if chained {
                (p.side_chain + 1).min(4)
            } else {
                0
            };
            let slow = p.side_chain >= 4 && down;
            let speed = match (p.in_water, slow) {
                (false, false) => 3.0,
                (false, true) => 1.0,
                (true, false) => 2.0,
                (true, true) => 0.1,
            };
            p.side_push = SIDE_PUSH_CHAIN;
            let s = p.sensors();
            let wx = p.x + dir * (s.wall_x + 1.0);
            let pinned = [s.wall_y0, s.wall_y1]
                .iter()
                .any(|dy| self.level.cell_at(wx, p.y + dy).is_solid());
            if pinned {
                p.facing = -(dir as i32);
                p.vx = -dir * speed * 0.7;
                p.vy = 2.5;
                p.on_ground = false;
            } else {
                p.vx = dir * speed;
            }
            self.events.push(Event::Jump);
        }
    }

    /// The player's head bumped into a ! block (or its chain) from below: the whole chain
    /// bounces and enemies standing on it are knocked out.
    fn hit_exclamation_blocks(&mut self) {
        let p = &self.player;
        let head = p.y + p.sensors().head_y;
        let (px0, _, px1, _) = p.hurt_box();
        let mut hit = None;
        for (bi, b) in self.exclamations.iter().enumerate() {
            if b.boxes().iter().any(|&(x0, y0, x1, _)| {
                px1 > x0 && px0 < x1 && head >= y0 - 2.0 && head <= y0 + 2.0
            }) {
                hit = Some(bi);
                break;
            }
        }
        let Some(bi) = hit else { return };
        self.exclamations[bi].hit();
        self.events.push(Event::Bump);
        let boxes = self.exclamations[bi].boxes();
        let mut downs = 0;
        for a in &mut self.actors {
            if a.kind.is_enemy()
                && a.alive()
                && a.on_ground
                && boxes
                    .iter()
                    .any(|&(x0, _, x1, y1)| a.x > x0 && a.x < x1 && (a.y - y1).abs() < 2.0)
            {
                a.die_fall(a.x);
                downs += 1;
            }
        }
        for _ in 0..downs {
            self.events.push(Event::EnemyDown);
        }
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
            let (ax0, ay0, ax1, _) = a.bbox();
            // a Piranha Creeper's head over the block is trampled (2 pieces back) like a stomp
            // [obs 3W pass 2: blkhit on the head → Damage]
            if matches!(a.kind, Kind::PiranhaCreeper { .. })
                && (ay0 - top).abs() < 2.0
                && ax1 > x0
                && ax0 < x1
            {
                a.creeper_trample(false);
                continue;
            }
            let on_top = (a.y - top).abs() < 2.0 && ax1 > x0 && ax0 < x1;
            if !on_top || !a.solid_contact() {
                continue;
            }
            match a.kind {
                Kind::Goomba | Kind::Goombrat | Kind::Piranha { .. } => {
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
                k if k.is_boss() => {
                    a.boss_hit(crate::actors::BossAttack::BlockHit, cx);
                }
                k if k.is_w3() && a.w3_block_hit() => {
                    a.die_fall(cx);
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
        let contents = if r.contents.is_powerup() && self.pu_small() {
            Contents::Mushroom
        } else {
            r.contents
        };
        let item = match contents {
            Contents::Mushroom => Some(Kind::Mushroom),
            Contents::Power(w) => Some(self.pu_block_item(w)),
            Contents::Special => Some(self.style_item()),
            Contents::FireFlower => Some(Kind::FireFlower),
            Contents::Star => Some(Kind::Star),
            Contents::OneUp => Some(Kind::OneUp),
            Contents::Coin => None,
            Contents::Goomba => Some(Kind::Goomba),
            Contents::Koopa { red } => Some(Kind::Koopa { red }),
            Contents::YoshiEgg => Some(Kind::YoshiEgg { red: false }),
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

    /// Superball (PlayerSuperBall, ctor 0x71003b79a0, Move 0x71003b8990) [obs]: one at a time
    /// (the throw check 0x71009ed0a0 refuses while the ball's flag on the player is set),
    /// 3 px/f forward and 3 down (0x71003b80d0), 270 frames; it bounces off floors, ceilings
    /// and walls at the same speed (0x71003b8c30) and snuffs out on leaving the view, unless
    /// it is above the view and coming back down.
    fn throw_superball(&mut self) {
        if self
            .actors
            .iter()
            .any(|a| a.kind == Kind::Superball && a.alive())
        {
            return;
        }
        let p = &self.player;
        let mut f = Actor::new(Kind::Superball, p.x + p.facing as f32 * 8.0, p.y + 12.0);
        f.dir = p.facing;
        f.vx = 3.0 * p.facing as f32;
        f.vy = -3.0;
        f.timer = 270;
        self.add_actor(f);
        self.events.push(Event::Fireball);
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

    /// Superballs out of the view snuff out (0x71003b8990) [obs].
    fn snuff_superballs(&mut self) {
        let v = self.view_rect(0.0);
        for a in &mut self.actors {
            if a.kind == Kind::Superball && a.alive() {
                let inside = a.x >= v.0 && a.x <= v.2 && a.y >= v.1 && a.y <= v.3;
                let coming_back = a.vy < 0.0 && a.y > v.3;
                if !inside && !coming_back {
                    a.mode = Mode::Dead;
                }
            }
        }
    }

    fn collect_coins(&mut self) {
        // a superball collects the coins it passes through [ext]
        let balls: Vec<_> = self
            .actors
            .iter()
            .filter(|a| a.kind == Kind::Superball && a.alive())
            .map(|a| a.bbox())
            .collect();
        let mut got = 0;
        self.level.coins.retain(|c| {
            let cb = (
                c.x as f32 * TILE,
                c.y as f32 * TILE,
                (c.x + 1) as f32 * TILE,
                (c.y + 1) as f32 * TILE,
            );
            let hit = balls.iter().any(|&b| overlap(b, cb));
            got += hit as u32;
            !hit
        });
        for _ in 0..got {
            self.add_coin();
        }
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
        // pink coins (92-pink-coin.md): each counts once; the last one gives the key
        let mut newly = 0;
        for c in &self.level.pink_coins {
            let cb = (
                c.x as f32 * TILE,
                c.y as f32 * TILE,
                (c.x + 1) as f32 * TILE,
                (c.y + 1) as f32 * TILE,
            );
            if overlap(b, cb) && self.pink.insert((self.area, c.x, c.y)) {
                newly += 1;
            }
        }
        if newly > 0 {
            self.events.push(Event::Coin);
            if self.pink.len() >= self.pink_total {
                // the key counts at once [obs 0x7100411400]: the last coin queues a key event
                // (kind 3 if the pickup's flag byte is set, else 0) and the key manager adds it
                // to the player's keys when it processes the queue; kind 3 only shows a key
                // flying in (PinkCoinAppearWait → Appear). A key with no lock left for it is
                // dropped and not counted — the engine has no lock count, so it always counts
                self.keys += 1;
                self.events.push(Event::OneUp);
            }
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
                    if matches!(f.kind, Kind::Fireball | Kind::Superball)
                        && e.kind.is_enemy()
                        && f.mode != Mode::Dead
                    {
                        let (fx, fy) = (f.x, f.y);
                        f.mode = Mode::Dead;
                        if let Some(down) = w3::fire_hit(e, fx).or_else(|| v3::fire_hit(e, fx, fy))
                        {
                            if down {
                                scores.push((Self::chain_score(0), e.x, e.y + 24.0));
                            }
                        } else if e.kind.is_boss() {
                            let hit = e.boss_hit(crate::actors::BossAttack::Fire, fx);
                            if hit == crate::actors::BossHit::Defeated {
                                scores.push((Self::chain_score(0), e.x, e.y + 24.0));
                            }
                        } else if !e.fire_proof() {
                            e.die_fall(fx);
                            scores.push((Self::chain_score(0), e.x, e.y + 24.0));
                        }
                    }
                }
                if !(ae && be) || !a.solid_contact() || !b.solid_contact() {
                    continue;
                }
                if let Some(score) = bosses::boss_shell_hit(a, b) {
                    scores.extend(score);
                    continue;
                }
                if w3::shell_hit(a, b) {
                    continue;
                }
                if let Some(hit) = v3::shell_hit(a, b) {
                    if let Some((x, y)) = hit {
                        scores.push((Self::chain_score(0), x, y + 24.0));
                    }
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
                    // counted once as a big coin (its value toward "N coins")
                    let cond =
                        std::mem::replace(&mut self.clear, crate::clear::ClearCondition::none());
                    for _ in 0..n {
                        self.add_coin();
                    }
                    self.clear = cond;
                    let r = self.clear.coin(n);
                    self.condition_counted(r);
                    self.score += [1000, 3000, 5000][value as usize] - n * COIN_SCORE;
                    let _ = (ax, ay);
                }
                i += 1;
                continue;
            }
            if a.kind.is_item() {
                if overlap(contact, ab) {
                    let kind = a.kind;
                    let (ax, ay, sub) = (a.x, a.y, a.smm1_item);
                    self.actors[i].mode = Mode::Dead;
                    self.collect_item(kind, ax, ay, sub);
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
                // saw: hurts on touch — its collider is a box of half-size 22 × 22 centred 24 px
                // above the origin (descriptor _INIT_737 0x71001ad770; actor colliders always
                // overlap as boxes, 0x71003e6760) [obs]
                let (cx, cy) = (a.x, a.y + 24.0);
                let near = overlap(hurt, (cx - 22.0, cy - 22.0, cx + 22.0, cy + 22.0));
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
                // a Muncher hurts on every side, including from on top (not through a shoe)
                let shod = self.player.shoe.is_some() && self.player.y >= y1 - 2.0;
                if self.player.spinning && self.player.vy < 0.0 && self.player.y >= y1 - 4.0 {
                    // spin-jumping onto a Muncher bounces off it ("Pass")
                    self.player.bounce(jump_held);
                    i += 1;
                    continue;
                }
                if overlap(hurt, (x0 - 1.5, y0 - 1.5, x1 + 1.5, y1 + 1.5))
                    && !star
                    && !shod
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
            if a.kind == Kind::FloatCoin {
                if overlap(contact, ab) {
                    self.actors[i].mode = Mode::Dead;
                    self.add_coin();
                }
                i += 1;
                continue;
            }
            if let Kind::Yoshi { .. } = a.kind {
                // mount (yoshi.md §3): falling onto him from above, lockout over
                if overlap(contact, ab)
                    && self.player.vy <= 0.0
                    && self.player.y >= a.y + 10.0
                    && self.player.yoshi.is_none()
                    && self.player.yoshi_lock == 0
                    && self.player.held.is_none()
                    && self.player.shoe.is_none()
                {
                    let id = a.id;
                    self.actors[i].held = true;
                    self.actors[i].phase = 0;
                    self.player.yoshi = Some(id);
                    self.player.vy = 0.0;
                    self.events.push(Event::PowerUp);
                }
                i += 1;
                continue;
            }
            if let Kind::ClownCar { .. } = a.kind {
                // climb in by dropping into an empty car
                if a.phase == 0
                    && overlap(contact, ab)
                    && self.player.vy <= 0.0
                    && self.player.y >= a.y + 10.0
                    && self.player.clown.is_none()
                {
                    let id = a.id;
                    self.actors[i].phase = 2;
                    self.player.clown = Some(id);
                    self.player.vy = 0.0;
                    self.events.push(Event::Pipe);
                }
                i += 1;
                continue;
            }
            if a.kind == Kind::KoopaCar {
                // get in by dropping into an empty car
                if a.phase == 0
                    && overlap(contact, ab)
                    && self.player.vy <= 0.0
                    && self.player.y >= a.y + 8.0
                    && self.player.car.is_none()
                    && self.player.clown.is_none()
                    && self.player.yoshi.is_none()
                    && self.player.shoe.is_none()
                {
                    let id = a.id;
                    self.actors[i].phase = 1;
                    self.actors[i].timer = 0;
                    self.actors[i].dir = self.player.facing;
                    self.player.car = Some(id);
                    self.player.vy = 0.0;
                    self.clown_prev = (true, true);
                    self.events.push(Event::Pipe);
                }
                i += 1;
                continue;
            }
            if let Kind::Shoe { heels } = a.kind {
                // climb in by landing on it
                if overlap(contact, ab)
                    && self.player.vy <= 0.0
                    && self.player.y >= a.y + 4.0
                    && self.player.shoe.is_none()
                {
                    self.actors[i].mode = Mode::Dead;
                    self.player.shoe = Some(heels);
                    self.events.push(Event::PowerUp);
                }
                i += 1;
                continue;
            }
            if a.kind == (Kind::AngrySun { moon: true }) && overlap(contact, ab) {
                // the Moon wipes out the enemies on screen and disappears [ext behaviour]
                self.actors[i].mode = Mode::Dead;
                let v = self.view_rect(0.0);
                let px = self.player.x;
                for e in &mut self.actors {
                    if e.kind.is_enemy()
                        && e.alive()
                        && e.x >= v.0
                        && e.x <= v.2
                        && e.y >= v.1
                        && e.y <= v.3
                    {
                        e.die_fall(px);
                    }
                }
                self.events.push(Event::EnemyDown);
                i += 1;
                continue;
            }
            if a.kind.is_v3() {
                if self.v3_contact(i, jump_held) {
                    return;
                }
                i += 1;
                continue;
            }
            if a.kind.is_w3() {
                if self.w3_contact(i, jump_held) {
                    return;
                }
                i += 1;
                continue;
            }
            if a.kind.is_boss() || matches!(a.kind, Kind::BowserFire | Kind::Shuriken) {
                if self.boss_contact(i, jump_held) {
                    return;
                }
                i += 1;
                continue;
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
            let head = p.y + p.sensors().head_y;
            // empty shells (the 0x4 forms): a Buzzy or Spiny shell that comes down on the head
            // of a player without a helmet becomes the helmet (enemy contact 0x710072bca0 →
            // player 0x71009ee570, the shell's helmet type +0x540) [obs]; a Dry Bones Shell is
            // ridden with the shoe states, flagged as the shell (+0x4ac / +0x4b0: "karon_*"
            // animations, 0x71009f61a0) [obs]
            if matches!(a.kind, Kind::Shell { .. })
                && a.shell_of != 0
                && a.mode == Mode::ShellWait
                && !a.held
            {
                let what = a.shell_of;
                let from_below =
                    (p.vy > 0.0 && head <= ay + 6.0) || (a.vy < 0.0 && ay >= head - 6.0);
                let from_above = p.vy < 0.0 && p.y >= ay + ah * 0.5;
                let free = p.yoshi.is_none() && p.clown.is_none() && p.car.is_none();
                // (a costume can't wear a shell, SMM1 costume spec §1)
                if what != 3 && from_below && p.helmet.is_none() && p.form != Form::Costume {
                    self.actors[i].mode = Mode::Dead;
                    self.player.helmet = Some(what);
                    self.events.push(Event::PowerUp);
                    i += 1;
                    continue;
                }
                if what == 3 && from_above && free && p.shoe.is_none() {
                    self.actors[i].mode = Mode::Dead;
                    self.player.shoe = Some(false);
                    self.player.shoe_karon = true;
                    self.player.vy = 0.0;
                    self.events.push(Event::PowerUp);
                    i += 1;
                    continue;
                }
            }
            // a helmet shields the head and attacks what is above it [obs]: a helmet hit (cc
            // 16–19) only counts against a body whose bottom is above the wearer's, this frame
            // or the last (0x7100730510); it is metcap (Buzzy) / tgzcap (Spiny) in EnemyDB.
            // tgzcap is DieFall for nearly everything, Thwomps and Boos too, but CapBound for
            // Buzzy Beetles; metcap is CapBound (a harmless bounce) except for Spike Tops and
            // Lakitu (DieFall) [obs EnemyDB]
            if let Some(h) = p.helmet
                && a.kind.is_enemy()
                && ay >= head - 6.0
                && !matches!(
                    a.kind,
                    Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
                )
            {
                let falls = if h == 2 {
                    !matches!(a.kind, Kind::Buzzy | Kind::ClownCar { .. })
                } else {
                    matches!(a.kind, Kind::SpikeTop { .. } | Kind::Lakitu)
                };
                if falls && ay > p.y {
                    let px = p.x;
                    self.actors[i].die_fall(px);
                    self.events.push(Event::EnemyDown);
                }
                i += 1;
                continue;
            }
            // a slide knocks out enemies it runs into (not bosses or the indestructible)
            let slide_hit = self.player.sliding
                && !matches!(
                    a.kind,
                    Kind::Thwomp { .. }
                        | Kind::Boo
                        | Kind::RingBoo { .. }
                        | Kind::AngrySun { .. }
                        | Kind::EnemyFire
                        | Kind::Hammer
                        | Kind::Magic
                        | Kind::Wrench
                        | Kind::Bowser { .. }
                        | Kind::BoomBoom { .. }
                        | Kind::BowserJr { .. }
                );
            // Big Mario knocks out what he touches [inf: no Mega attack id is set on the body
            // colliders (0x71009cdc90 sets cc 2 only for the star); not found], bosses and the
            // indestructible aside
            let mega_hit = self.player.form == Form::Mega
                && !matches!(
                    a.kind,
                    Kind::Thwomp { .. }
                        | Kind::Boo
                        | Kind::RingBoo { .. }
                        | Kind::AngrySun { .. }
                        | Kind::EnemyFire
                        | Kind::Bowser { .. }
                        | Kind::BoomBoom { .. }
                        | Kind::BowserJr { .. }
                );
            if (star && a.kind != Kind::EnemyFire) || slide_hit || mega_hit {
                let px = p.x;
                self.actors[i].die_fall(px);
                let n = self.player.stomp_chain;
                self.player.stomp_chain += 1;
                self.add_score(Self::chain_score(n), ax, ay + 24.0);
                self.events.push(Event::EnemyDown);
                i += 1;
                continue;
            }
            // stomp window (interactions: 0x710076b6b0, classes 0x7101d035b0) [obs]: the feet
            // are inside the enemy's top by d > 0 and d < W; W = 10 − 3 + this frame's fall for
            // ordinary enemies when falling (3 when rising), 12 / 10 for big ones
            let d = ay + ah - p.y;
            let falling = p.vy <= 0.0;
            let w = match (a.big, falling) {
                // SMM1 (0x0234aa14): a flat 10 for ordinary enemies when falling
                (false, true) if self.options.smm1_shell_kicks => 10.0,
                (false, true) => 7.0 + (-p.vy).max(0.0),
                (false, false) => 3.0,
                (true, true) => 12.0,
                (true, false) => 10.0,
            };
            let in_window = d > 0.0 && d < w;
            // (a Spiny shell's spikes can't be stomped: EnemyDB TogezoShell has the walking
            // Spiny's stomp row — stamp Damage, stamp_shoe DieFall, stamp_spin DieVanish [obs])
            let stomp = in_window
                && !a.kind.unstompable()
                && !(a.shell_of == 2 && self.player.shoe.is_none());
            // a shoe stomp defeats most enemies, spiny or not (stamp_shoe in EnemyDB)
            let shoe_stomp = self.player.shoe.is_some()
                && in_window
                && !matches!(
                    a.kind,
                    Kind::Thwomp { .. }
                        | Kind::Boo
                        | Kind::RingBoo { .. }
                        | Kind::AngrySun { .. }
                        | Kind::EnemyFire
                        | Kind::Hammer
                        | Kind::Magic
                        | Kind::Wrench
                );
            // on Yoshi, stomps kill spiny enemies too (EnemyDB stamp_yoshi): not Piranha Plants or
            // Spike Tops
            if self.player.yoshi.is_some()
                && in_window
                && matches!(a.kind, Kind::Spiny | Kind::SpinyEgg | Kind::Buzzy)
            {
                let px = p.x;
                self.actors[i].die_fall(px);
                self.player.bounce(jump_held);
                self.events.push(Event::Stomp);
                i += 1;
                continue;
            }
            // a ground pound dive knocks out what it lands on (hipatk), bosses and the
            // indestructible aside
            if self.player.pound == -1 && in_window {
                let tough = matches!(
                    a.kind,
                    Kind::Bowser { .. }
                        | Kind::BoomBoom { .. }
                        | Kind::BowserJr { .. }
                        | Kind::Thwomp { .. }
                        | Kind::Boo
                        | Kind::RingBoo { .. }
                        | Kind::LavaBubble
                        | Kind::AngrySun { .. }
                        | Kind::ChainChomp { .. }
                );
                if !tough && !a.kind.is_platform() {
                    let px = p.x;
                    self.actors[i].die_fall(px);
                    self.events.push(Event::Stomp);
                    i += 1;
                    continue;
                }
            }
            // a spin jump: stomped enemies vanish; spiky ones are bounced off safely ("Pass")
            let spin_stomp = self.player.spinning && in_window;
            let spin_safe = matches!(
                a.kind,
                Kind::EnemyFire | Kind::Hammer | Kind::Magic | Kind::Wrench | Kind::AngrySun { .. }
            );
            if spin_stomp && !spin_safe {
                let boss = matches!(
                    a.kind,
                    Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
                );
                if !(a.kind.unstompable() || a.kind.stomp_bounces() || boss || a.kind.is_platform())
                {
                    self.actors[i].mode = Mode::Dead;
                    self.add_score(Self::chain_score(self.player.stomp_chain), ax, ay + 24.0);
                    self.player.stomp_chain += 1;
                    self.events.push(Event::Stomp);
                } else {
                    self.events.push(Event::Bump);
                }
                self.player.bounce(jump_held);
                i += 1;
                continue;
            }
            if shoe_stomp && (a.kind.unstompable() || a.kind.stomp_bounces()) {
                let px = p.x;
                self.actors[i].die_fall(px);
                self.player.bounce(jump_held);
                let n = self.player.stomp_chain;
                self.player.stomp_chain += 1;
                self.add_score(Self::chain_score(n), ax, ay + 24.0);
                self.events.push(Event::Stomp);
                i += 1;
                continue;
            }
            if stomp && a.kind.stomp_bounces() {
                self.player.bounce(jump_held);
                self.events.push(Event::Bump);
                i += 1;
                continue;
            }
            let still_shell = a.mode == Mode::ShellWait;
            if a.held {
                i += 1;
                continue;
            }
            if still_shell && self.style.can_carry() && self.run_held && self.player.held.is_none()
            {
                // pick it up (player-physics.md §5.5; not in SMB1)
                let id = a.id;
                self.actors[i].held = true;
                self.player.held = Some(id);
                i += 1;
                continue;
            }
            // SMM1 shell physics (Options::smm1_shell_kicks, world/smm1.rs)
            if self.options.smm1_shell_kicks && matches!(a.kind, Kind::Shell { .. }) {
                match self.smm1_shell_contact(i, stomp, jump_held) {
                    Smm1Shell::Done => {
                        i += 1;
                        continue;
                    }
                    Smm1Shell::Hurt => {}
                }
            }
            if still_shell {
                let dir = if ax >= self.player.x { 1 } else { -1 };
                let pvx = self.player.vx;
                self.actors[i].kick(dir, pvx);
                if stomp {
                    self.player.bounce(jump_held);
                }
                // the kick scores as a stomp-family attack on the player's stomp chain
                // (shell kick 0x71005f57a0 → 0x7100644610 with attack 0) [obs]
                let n = self.player.stomp_chain;
                self.player.stomp_chain += 1;
                self.add_score(Self::chain_score(n), ax, ay + 24.0);
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

    /// The style power-up as an item (each style's own: Big Mushroom, Leaf, …).
    fn style_item(&self) -> Kind {
        Kind::StyleItem
    }

    /// `smm1_item`: the Super Mushroom's SMM1 subtype (1 Weird, 2 Mystery; `Actor::smm1_item`).
    fn collect_item(&mut self, kind: Kind, x: f32, y: f32, smm1_item: u8) {
        match kind {
            Kind::StyleItem => {
                let form = match self.style {
                    crate::player::Style::MW => Form::Cape,
                    crate::player::Style::WU => Form::Propeller,
                    crate::player::Style::W3 => Form::Cat,
                    crate::player::Style::M1 => Form::Mega,
                    _ => Form::Raccoon,
                };
                self.player.power_up(form);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            Kind::SuperHammer => {
                self.player.power_up(Form::Builder);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            // the power-up sound plays for every pickup, also when the form does not change
            Kind::Mushroom if smm1_item == 1 => {
                // Weird Mushroom (SMB1 only): Lanky Mario from small or Super
                self.player.power_up(Form::Lanky);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            Kind::Mushroom if smm1_item == 2 => {
                self.take_mystery_mushroom();
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
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
            Kind::SuperballFlower => {
                self.player.power_up(Form::Superball);
                self.events.push(Event::PowerUp);
                self.add_score(ITEM_SCORE, x, y + 16.0);
            }
            Kind::Star => {
                self.player.star = STAR_FRAMES;
                self.events.push(Event::PowerUp);
            }
            Kind::PowerItem { what } => self.collect_power_item(what, x, y),
            Kind::OneUp => {
                if self.clear.goal == crate::clear::Goal::OneUps {
                    let r = self.clear.count(None, 1);
                    self.condition_counted(Some(r));
                }
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

    /// The view grown by (mx, my), with the extra vertical reach of horizontal areas.
    fn zone(&self, mx: f32, my: f32) -> (f32, f32, f32, f32) {
        let extra = if self.level.vertical {
            0.0
        } else {
            ZONE_EXTRA_Y
        };
        (
            self.camera_x - mx,
            self.camera_y - my - extra,
            self.camera_x + VIEW_W + mx,
            self.camera_y + VIEW_H + my + extra,
        )
    }

    /// Spawn course actors whose spawn box reaches the spawn zone (0x71003fcb90) [obs]. An actor
    /// that went off screen comes back once its home has left the keep-armed zone and returns;
    /// defeated enemies stay gone (entering Die, 0x7100795220, sets the no-respawn byte).
    fn activate(&mut self) {
        let za = self.zone(SPAWN_MARGIN_X, SPAWN_MARGIN_Y);
        let zc = self.zone(REARM_MARGIN, REARM_MARGIN);
        let touches = |b: (f32, f32, f32, f32), z: (f32, f32, f32, f32)| {
            b.0 <= z.2 && b.2 >= z.0 && b.1 <= z.3 && b.3 >= z.1
        };
        for idx in 0..self.spawns.len() {
            let rec = &self.spawns[idx];
            let s = rec.spawn;
            // spawn box: the actor profile's box (profile +0x18: centre offset, half size) around
            // the placement, which 0x71003fcb90 tests against the zone [obs profiles.txt]
            let (cx, cy, hw, hh) = spawn_profile(s.kind, s.big);
            let sb = (s.x + cx - hw, s.y + cy - hh, s.x + cx + hw, s.y + cy + hh);
            if rec.spawned {
                if !touches(sb, zc) {
                    self.spawns[idx].spawned = false;
                }
                continue;
            }
            if !touches(sb, za) || rec.alive || rec.killed {
                continue;
            }
            self.spawns[idx].spawned = true;
            // a boss riding a Clown Car draws its own car: skip the linked car object
            let boss_car = matches!(s.kind, Kind::ClownCar { .. })
                && s.link >= 0
                && self.spawns.iter().any(|r| {
                    r.spawn.clown
                        && r.spawn.link == s.link
                        && matches!(
                            r.spawn.kind,
                            Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
                        )
                });
            if boss_car {
                self.spawns[idx].killed = true;
                continue;
            }
            let mut a = Actor::new(s.kind, s.x, s.y);
            a.big = s.big;
            a.wings = s.wings;
            a.alt = s.alt;
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
            if let Kind::ChainChomp { chained: true } = s.kind {
                // the stake stays at the placement; the Chomp starts beside it
                a.home_x = s.x;
                a.home_y = s.y;
                let side = if self.player.x <= s.x { -20.0 } else { 20.0 };
                a.x = s.x + side * if s.big { 2.0 } else { 1.0 };
            }
            if let Some(start) = s.rail {
                // the reference point is the object's tile centre (feet + 8)
                if let Some(r) =
                    crate::track::Rail::attach(&self.level.tracks, s.x, s.y + 8.0, start, s.wings)
                {
                    let p = r.point(&self.level.tracks);
                    a.rail_off = (s.x - p.0, s.y - p.1);
                    a.rail = Some(r);
                }
            }
            // enemies face the player (left if player.x ≤ x); items face right; a Twister
            // starts facing left
            a.dir = if (s.kind.is_enemy() && self.player.x <= s.x) || s.kind == Kind::Twister {
                -1
            } else {
                1
            };
            if s.kind == Kind::Blooper && s.alt {
                let id = self.next_id;
                self.add_actor(a.clone());
                for slot in 1..=4 {
                    let mut b = Actor::new(Kind::BabyBlooper { slot }, s.x, s.y);
                    b.chain = id;
                    b.spawn = Some(idx);
                    b.wings = s.wings;
                    self.add_actor(b);
                }
                self.spawns[idx].alive = true;
                continue;
            }
            if s.kind == Kind::Boo && s.alt {
                // a Boo circle: eight Boos round the placement
                for slot in 0..8 {
                    let mut b = a.clone();
                    b.kind = Kind::RingBoo { slot };
                    b.home_x = s.x;
                    b.home_y = s.y + 8.0;
                    b.speed = 0.0;
                    b.heading = 90.0;
                    self.add_actor(b);
                }
            } else {
                self.add_actor(a);
            }
            self.spawns[idx].alive = true;
        }
    }

    /// Remove actors whose culling box (half-size 32 × 32, big 48, a sliding shell 160 × 32;
    /// 0x7100792c90) grown by 80 px misses the view; dead ones stay dead.
    fn despawn(&mut self) {
        let v = self.zone(DESPAWN_MARGIN, DESPAWN_MARGIN);
        for a in &mut self.actors {
            let (hx, hy) = match (a.mode, a.big) {
                (Mode::ShellSlide, _) => (160.0, 32.0),
                (_, true) => (48.0, 48.0),
                _ => (32.0, 32.0),
            };
            // a Lava Bubble hiding under the lava is still at home
            let cy = if a.kind == Kind::LavaBubble {
                a.home_y
            } else {
                a.y
            } + a.height() / 2.0;
            let cx = a.x;
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

    /// Bob-omb explosions (15-bob-omb.md): break breakable blocks around the
    /// bomb, knock out enemies and hurt the player nearby: terrain within the diamond of radius
    /// 2 tiles [obs]; actors and the player touching the blast box [obs]. While the Explode
    /// flag is up (0x71005eb8b0 sets +0x518 bit 25) the enemy collider update 0x7100732640
    /// swaps the bomb's body box for a shell-attack (cc 6) box centred 8 px above its feet,
    /// half 18 × 18 (Big: × 2), that hits any body it touches, players included.
    fn explosions(&mut self) {
        let bombs: Vec<(f32, f32, f32)> = self
            .actors
            .iter()
            .filter(|a| a.kind == Kind::BobOmb && a.phase == 9 && a.mode != Mode::Dead)
            .map(|a| (a.x, a.y, if a.big { 2.0 } else { 1.0 }))
            .collect();
        for &(bx, feet, s) in &bombs {
            let by = feet + 8.0;
            let (cy, h) = (feet + 8.0 * s, 18.0 * s);
            let blast = (bx - h, cy - h, bx + h, cy + h);
            for a in &mut self.actors {
                if a.kind == Kind::BobOmb && a.phase == 9 {
                    a.mode = Mode::Dead;
                    continue;
                }
                if a.kind.is_enemy() && a.alive() && overlap(blast, a.bbox()) {
                    if a.kind.is_boss() {
                        a.boss_hit(crate::actors::BossAttack::Blast, bx);
                    } else {
                        a.die_fall(bx);
                    }
                }
            }
            let (tx, ty) = ((bx / TILE).floor() as i32, (by / TILE).floor() as i32);
            // the terrain break regions (0x710060cb90) [obs]: a 32×32 box on the bomb's tile
            // centre plus lines 32 px out on each axis — the 3×3 tiles and the four two away
            for (dx, dy) in [
                (0, 0),
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
                (2, 0),
                (-2, 0),
                (0, 2),
                (0, -2),
            ] {
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
            if overlap(blast, self.player.hurt_box()) && self.player.hurt() {
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
        let ang = dy.datan2(dx).to_degrees().rem_euclid(360.0);
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

    /// Burner flame state from the area timer's 372-frame counter, which starts at 370 and
    /// only runs while the world isn't frozen (0x71004005e0; burner states 0x7100873fe0 read
    /// it) [obs] (54-burner.md): returns the flame length in px (0 when off). Fire 18–136
    /// (full), FireStart 0–17 (growing), FireFinish 137–155 (shrinking); the alternate form runs
    /// half a cycle later.
    pub fn burner_flame(&self, alt: bool) -> f32 {
        let c = (self.area_timer % 372 + 370) % 372;
        let t = ((c + if alt { 186 } else { 0 }) % 372) as f32;
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

    /// Fire Bar angle (degrees): the start angle ± the area's shared rotation counter, which
    /// advances by the binary angle 0xC00000 (1.0546875°, one turn per 341⅓ frames) every
    /// frame the world isn't frozen (area update 0x71004005e0, step from _INIT_51) [obs].
    pub fn firebar_angle(&self, fb: &crate::level::FireBar) -> f32 {
        let turn = self.area_timer.wrapping_mul(FIREBAR_STEP) as f64 / 4_294_967_296.0;
        fb.start + fb.dir * (turn * 360.0) as f32
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
                        let want = (py + 8.0 - (a.y + 16.0)).datan2(px - a.x).to_degrees();
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
            m.vx = 0.75 * ang.dcos();
            m.vy = 0.75 * ang.dsin();
            m.timer = 600;
            self.add_actor(m);
        }
    }

    /// Desert-night wind, SMB1 pattern (themes.md §2): period 600 frames, wind (toward −1, i.e.
    /// left) during the last 240, w eased by 0.1 per frame.
    fn step_wind(&mut self) {
        if !(self.level.theme == 7 && self.level.night) {
            self.wind = 0.0;
            return;
        }
        const P: i32 = 600;
        const A: i32 = 240;
        if self.wind_n > 0 {
            self.wind_n -= 1;
        }
        if self.wind_n < A {
            self.wind = (self.wind - 0.1).max(-1.0);
            if self.wind_n == 0 {
                self.wind_n = P;
            }
        } else if self.wind < 0.0 {
            self.wind = (self.wind + 0.1).min(0.0);
        }
    }

    /// Underground night shows the course upside down with Up and Down swapped (themes.md §1.1).
    pub fn upside_down(&self) -> bool {
        self.level.theme == 1 && self.level.night
    }

    /// Twister wind column: x within ±16 of the twister, from its feet to the column top. The
    /// player is lifted (vy toward 4.5 at +0.6/frame) [inf: the twister's contact 0x71008c0630
    /// fills the player's tornado receiver (actor +0x398: the twister's centre x, top y and x
    /// speed, flag bit 0); the player state list has no Tornado state, and the code reading
    /// the receiver for the player wasn't found].
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

    /// Blinking and spike blocks share one area timer (240 frames), which also turns the Fire
    /// Bars. Blinking blocks swap groups at each wrap. Spike blocks: their cycle counter starts
    /// at 180 (0x71004005e0, reset 0x7100408c20) [obs]: retracted for 0–117, warning 118–177,
    /// out 178–237, warning 238–239; ON/OFF linked ones are out while ON (mode 1) / OFF (mode 2).
    fn step_area_timer(&mut self) {
        self.area_timer = self.area_timer.wrapping_add(1);
        if self.area_timer.is_multiple_of(240) && !self.level.blink_blocks.is_empty() {
            self.blink_alt = !self.blink_alt;
            let blocks = self.level.blink_blocks.clone();
            for (x, y, alt) in blocks {
                let solid = alt == self.blink_alt;
                self.level
                    .set(x, y, if solid { Cell::Solid } else { Cell::Empty });
            }
            // a block appearing around the player pushes them out upward [inf: the blinking
            // tiles only switch collision (FlashBlock, 0x71004005e0); the escape of a body
            // inside a solid tile is the player's terrain-sensor code (player.rs side)]
            let p = &self.player;
            let (tx, ty) = (
                (p.x / TILE).floor() as i32,
                ((p.y + 1.0) / TILE).floor() as i32,
            );
            if self.level.cell(tx, ty) == Cell::Solid {
                self.player.y = (ty + 1) as f32 * TILE;
            }
        }
        if self.level.spike_blocks.is_empty() || !self.player.alive() {
            return;
        }
        let hb = self.player.hurt_box();
        let spiky = self.level.spike_blocks.iter().any(|&(x, y, mode)| {
            let out = match mode {
                1 => self.on_off,
                2 => !self.on_off,
                _ => self.spikes_out(),
            };
            let (x0, y0) = (x as f32 * TILE, y as f32 * TILE);
            out && overlap(hb, (x0 - 5.0, y0 - 2.0, x0 + 37.0, y0 + 34.0))
        });
        if spiky && self.player.shoe.is_none() && self.player.hurt() {
            self.events.push(if self.player.alive() {
                Event::PowerDown
            } else {
                Event::Death
            });
        }
    }

    /// Timer-driven spike blocks have their spikes out (frames 178–239 of the shared timer).
    pub fn spikes_out(&self) -> bool {
        (178..238).contains(&((self.area_timer % 240 + 180) % 240))
    }

    /// A ground pound landing hits the blocks under the feet from above (bricks break when big).
    fn pound_blocks(&mut self) {
        if !self.player.pound_landed {
            return;
        }
        let p = &self.player;
        let ty = ((p.y - 1.0) / TILE).floor() as i32;
        let power = if p.big() {
            HitPower::Break
        } else {
            HitPower::Bump
        };
        let mut cols = vec![
            ((p.x - 4.0) / TILE).floor() as i32,
            ((p.x + 4.0) / TILE).floor() as i32,
        ];
        cols.dedup();
        for tx in cols {
            if self.blocks.get(tx, ty).is_some() {
                self.hit_block(tx, ty, false, power);
            }
        }
        self.events.push(Event::Bump);
    }

    /// Raccoon tail swing (TailAttack, 18 frames) [obs], cat scratch / slide (ClimbAttack,
    /// ClimbSlidingAttack) [obs], cape spin (MantAttack) and hammer (HammerAttack) [obs]:
    /// knocks out enemies and bumps the block it touches.
    fn tail_attack(&mut self) {
        let p = &self.player;
        // the raccoon's tail hits through its whole 18-frame swing [obs]; the cat's scratch on
        // animation frames 0-10 of 17 (ClimbAttack exec 0x7100a1c230 → 0x7100a1b290) [obs]; the
        // cape's box is up on every frame of the spin (0x71009f7220 → 0x71009f6710) [obs]; the
        // hammer's on animation frames 11–17 (HammerAttack exec 0x71009c6150) [obs], here the
        // part of them inside the player's 16-frame swing (`attack` 15 → 0 is frame 0 → 15)
        let window = match p.form {
            Form::Raccoon => 0..=17,
            Form::Cat => {
                crate::player::CAT_SCRATCH_FRAMES - 11..=crate::player::CAT_SCRATCH_FRAMES - 1
            }
            Form::Cape => 1..=15,
            _ => 1..=4,
        };
        let cat_slide = p.form == Form::Cat && p.cat_slide > 0;
        if !(window.contains(&p.attack) || cat_slide)
            || !matches!(
                p.form,
                Form::Raccoon | Form::Cape | Form::Cat | Form::Builder
            )
        {
            return;
        }
        // the tail box (centre ±9, 10 up, half 9 × 8) swings behind on animation frames 0–3 and
        // 15–17 (TailAttack exec 0x7100a034e0) [obs]
        let frame = 17 - p.attack;
        // cape / hammer animation frame (0 on the swing's first frame)
        let swing = 15 - p.attack;
        let side = match p.form {
            Form::Raccoon if !(4..=14).contains(&frame) => -p.facing,
            // the cape box flips sides every 2 animation frames, starting behind
            // (side = facing ^ (frame / 2 & 1) ^ 1, 0x71009f6710) [obs]
            Form::Cape if (swing / 2) % 2 == 0 => -p.facing,
            _ => p.facing,
        };
        let (x0, x1) = if side > 0 {
            (p.x, p.x + 18.0)
        } else {
            (p.x - 18.0, p.x)
        };
        let top = if p.form == Form::Raccoon { 18.0 } else { 14.0 };
        let reach = match p.form {
            Form::Cat => {
                // the cat's attack box: centre 10 px ahead, 12 up; half 16 × 16 for the
                // scratch, 8 × 8 for the slide [obs 0x7100a1b290, 0x7100a1d150]
                let cx = p.x + 10.0 * p.facing as f32;
                let h = if cat_slide { 8.0 } else { 16.0 };
                (cx - h, p.y + 12.0 - h, cx + h, p.y + 12.0 + h)
            }
            Form::Cape => {
                // the mantle box (cc 21): centre 10 px to the side, 14 up; half 10 × 8 [obs]
                let cx = p.x + 10.0 * side as f32;
                (cx - 10.0, p.y + 6.0, cx + 10.0, p.y + 22.0)
            }
            Form::Builder => {
                // the hammer box (cc 25): centre 14 up, 20 px ahead once the swing is down
                // (14 / 16 / 18 on frames 11–13); half 10 × 14 [obs]
                let ahead = 20.0 + (2.0 * (swing - 10) as f32 - 8.0).min(0.0);
                let cx = p.x + ahead * side as f32;
                (cx - 10.0, p.y, cx + 10.0, p.y + 28.0)
            }
            _ => (x0, p.y + 2.0, x1, p.y + top),
        };
        let px = p.x;
        let mut hits = 0;
        for a in &mut self.actors {
            let boss = matches!(
                a.kind,
                Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. }
            );
            if a.kind.is_enemy()
                && a.alive()
                && a.solid_contact()
                && !boss
                && !matches!(
                    a.kind,
                    Kind::Boo | Kind::RingBoo { .. } | Kind::Thwomp { .. } | Kind::AngrySun { .. }
                )
                && overlap(reach, a.bbox())
            {
                if a.kind.is_v3() {
                    a.v3_tail_hit(px, reach.1);
                } else {
                    a.die_fall(px);
                }
                hits += 1;
            }
        }
        for _ in 0..hits {
            self.events.push(Event::EnemyDown);
        }
        // blocks at the swing's tip: the raccoon probes 16 px ahead, 8 up, on animation frames
        // 6–10 [obs]; the cape 16 px out on its current side, 8 up, while more than 2 of its
        // frames are left (0x71009f6710) [obs]; the hammer once, as its head comes down
        let probe = match self.player.form {
            Form::Raccoon => (6..=10).contains(&frame),
            // the scratch probes 16 px ahead, 8 up, from its first frame [obs 0x7100a1b290]
            Form::Cat => self.player.attack == crate::player::CAT_SCRATCH_FRAMES - 1,
            Form::Cape => true,
            _ => swing == 11,
        };
        if probe {
            let ty = ((self.player.y + 8.0) / TILE).floor() as i32;
            let reach = if self.player.form == Form::Builder {
                14.0
            } else {
                16.0
            };
            let tx = ((self.player.x + side as f32 * reach) / TILE).floor() as i32;
            if self.blocks.get(tx, ty).is_some_and(|b| b.bump.is_none()) {
                self.hit_block(tx, ty, true, HitPower::Break);
            }
        }
    }

    /// A moving block (wings / track) hit from below releases its contents once and is used up.
    fn bump_moving_block(&mut self) {
        // a flying hidden block: revealed by the head coming up into it
        if self.player.vy > 0.0 || self.player.head_hit.is_some() {
            let p = &self.player;
            let head_y = p.sensors().head_y;
            let head = p.y + head_y;
            let (px0, _, px1, _) = p.hurt_box();
            if let Some(i) = self.actors.iter().position(|a| {
                matches!(
                    a.kind,
                    Kind::MovingBlock {
                        block: BlockKind::Hidden,
                        ..
                    }
                ) && px1 > a.x - 8.0
                    && px0 < a.x + 8.0
                    && head >= a.y - 1.0
                    && head <= a.y + 8.0
            }) {
                let Kind::MovingBlock { contents, .. } = self.actors[i].kind else {
                    return;
                };
                self.actors[i].kind = Kind::MovingBlock {
                    block: BlockKind::Question,
                    contents: contents.or(Some(Contents::Coin)),
                };
                self.player.vy = self.player.vy.min(0.0);
                self.player.y = self.player.y.min(self.actors[i].y - head_y);
                self.player.head_solid = Some(self.actors[i].id);
            }
        }
        let Some(id) = self.player.head_solid else {
            return;
        };
        let Some(i) = self.actors.iter().position(|a| a.id == id) else {
            return;
        };
        let Kind::MovingBlock {
            block,
            contents: Some(c),
        } = self.actors[i].kind
        else {
            return;
        };
        let (x, y) = (self.actors[i].x, self.actors[i].y);
        self.actors[i].kind = Kind::MovingBlock {
            block: if block == BlockKind::Question {
                BlockKind::Empty
            } else {
                block
            },
            contents: None,
        };
        self.events.push(Event::Bump);
        let contents = if c.is_powerup() && self.pu_small() {
            Contents::Mushroom
        } else {
            c
        };
        let kind = match contents {
            Contents::Coin => {
                self.effects.push(Effect::coin_pop(x, y, false));
                return;
            }
            Contents::Mushroom => Kind::Mushroom,
            Contents::Power(w) => self.pu_block_item(w),
            Contents::Special => self.style_item(),
            Contents::FireFlower => Kind::FireFlower,
            Contents::Star => Kind::Star,
            Contents::OneUp => Kind::OneUp,
            Contents::Goomba => Kind::Goomba,
            Contents::Koopa { red } => Kind::Koopa { red },
            Contents::YoshiEgg => Kind::YoshiEgg { red: false },
        };
        let mut a = Actor::new(kind, x, y);
        a.dir = 1;
        a.vy = 0.5;
        a.mode = Mode::Emerge { until: y + 16.0 };
        self.events.push(Event::ItemAppear);
        self.add_actor(a);
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
        let red = self.actors[i].alt;
        let v = self.view_rect(16.0);
        let px = self.player.x;
        if red {
            // the red POW's quake_shock also breaks the bricks on screen [ext]
            let bricks: Vec<(i32, i32)> = self
                .blocks
                .map
                .iter()
                .filter(|&(&(x, y), b)| {
                    b.kind == BlockKind::Brick && b.contents.is_none() && {
                        let (bx, by) = (x as f32 * TILE, y as f32 * TILE);
                        bx >= v.0 && bx <= v.2 && by >= v.1 && by <= v.3
                    }
                })
                .map(|(&k, _)| k)
                .collect();
            for (x, y) in bricks {
                self.blocks.map.remove(&(x, y));
                self.level.set(x, y, Cell::Empty);
                let (bx, by) = blocks::origin(x, y);
                self.effects.extend(Effect::debris(bx, by, 0.0, 0.1));
            }
        }
        // the blue POW's quake goes to every live enemy standing on something, wherever it is
        // (the quake check runs in each enemy's update, 0x710078a9e0, with no range test; only
        // actors near the view exist) [obs]; the red one also reaches the air, in view [inf:
        // the red POW's Land (0x71008554a0, id 0x2e) does not post the global quake
        // (0x7100420860) at all; it sets bit 0 of its helper at +0x4d8 instead, whose effect
        // wasn't traced — the engine gives it the blue quake plus airborne enemies in view]
        for a in &mut self.actors {
            let in_view = a.x >= v.0 && a.x <= v.2 && a.y >= v.1 && a.y <= v.3;
            if a.kind.is_enemy() && a.alive() && (a.on_ground || (red && in_view)) {
                a.die_fall(px);
            }
        }
        self.events.push(Event::Break);
        if self.clear.goal == crate::clear::Goal::Pow {
            let r = self.clear.count(None, 1);
            self.condition_counted(Some(r));
        }
    }

    /// A cape dive landing (stage 5) shakes the ground: the player posts the same quake event
    /// as a POW (0x71009f5f50 → 0x7100420860, type 4) [obs]; grounded enemies fall.
    fn cape_quake(&mut self) {
        if !self.player.cape_quake {
            return;
        }
        let px = self.player.x;
        let mut downs = 0;
        for a in &mut self.actors {
            if a.kind.is_enemy() && a.alive() && a.on_ground {
                a.die_fall(px);
                downs += 1;
            }
        }
        self.events.push(Event::Break);
        if downs > 0 {
            self.events.push(Event::EnemyDown);
        }
    }

    /// Cannons (47-cannon.md): first shot 60 frames after spawn, then every 190, while the
    /// muzzle is on screen; the ball flies 1 px/frame along the barrel.
    fn step_cannons(&mut self) {
        let v = self.view_rect(0.0);
        for ci in 0..self.level.cannons.len() {
            let c = self.level.cannons[ci];
            let a = c.angle.to_radians();
            let (mx, my) = (c.x + 8.0 * a.dcos(), c.y + 8.0 + 8.0 * a.dsin());
            if mx < v.0 || mx > v.2 || my < v.1 || my > v.3 {
                continue;
            }
            self.cannon_timers[ci] -= 1;
            if self.cannon_timers[ci] > 0 {
                continue;
            }
            self.cannon_timers[ci] = 190;
            let mut b = Actor::new(Kind::Cannonball, mx, my - 8.0);
            let speed = if c.red { 2.4 } else { 1.0 };
            b.vx = speed * a.dcos();
            b.vy = speed * a.dsin();
            b.phase = c.red as u8;
            self.add_actor(b);
            self.events.push(Event::Kick);
        }
    }

    /// Hurt the player on Burner flames and Fire Bar fireballs.
    fn fire_hazards(&mut self) {
        let hb = self.player.hurt_box();
        let mut hit = false;
        for b in &self.level.burners {
            // the flame collider (Burner init 0x7100875190) [obs]: 10 px wide, 40 px long from
            // the block face (42 downward), centred on the block's middle. It is registered
            // only in the Fire state (enter 0x7100874b30 adds it, exit 0x7100874c30 removes
            // it): the growing (FireStart) and shrinking (FireFinish) flames don't hurt [obs]
            if self.burner_flame(b.alt) < 48.0 {
                continue;
            }
            let r = match b.dir {
                3 => (b.x - 5.0, b.y + 16.0, b.x + 5.0, b.y + 56.0),
                2 => (b.x - 5.0, b.y - 42.0, b.x + 5.0, b.y),
                1 => (b.x + 8.0, b.y + 3.0, b.x + 48.0, b.y + 13.0),
                _ => (b.x - 48.0, b.y + 3.0, b.x - 8.0, b.y + 13.0),
            };
            hit |= overlap(hb, r);
        }
        for fb in &self.level.firebars {
            let a = self.firebar_angle(fb).to_radians();
            for i in 1..fb.n {
                let (fx, fy) = (
                    fb.x + i as f32 * 8.0 * a.dcos(),
                    fb.y + i as f32 * 8.0 * a.dsin(),
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
            if matches!(a.kind, Kind::TrackBlock { .. }) {
                a.target_facing = self.on_off as i32;
            }
            a.step_spawning(&self.level);
        }
        self.place_baby_bloopers();
        // fixed objects riding tracks follow their carrier
        for a in &self.actors {
            let Kind::TrackMount { what, index, .. } = a.kind else {
                continue;
            };
            let i = index as usize;
            match what {
                0 => {
                    if let Some(f) = self.level.firebars.get_mut(i) {
                        (f.x, f.y) = (a.x, a.y + 8.0);
                    }
                }
                1 => {
                    if let Some(b) = self.level.burners.get_mut(i) {
                        (b.x, b.y) = (a.x, a.y);
                    }
                }
                2 => {
                    if let Some(c) = self.level.cannons.get_mut(i) {
                        (c.x, c.y) = (a.x, a.y);
                    }
                }
                _ => {
                    if let Some(b) = self.level.blasters.get_mut(i) {
                        (b.x, b.y) = (a.x, a.y);
                    }
                }
            }
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
        // tree canopies: platforms you can jump up through [inf: top-only — the canopy is a
        // model collision (tree init 0x710085cf10 → 0x71008fd2f0, flags 0x401) whose shape and
        // pass-through attribute come from the BellTree collision asset]
        for (ti, t) in self.level.trees.iter().enumerate() {
            let (_, top) = t.canopy();
            self.level.solids.push(crate::level::Solid {
                x0: t.x - 32.0,
                y0: top - 8.0,
                x1: t.x + 32.0,
                y1: top,
                dx: 0.0,
                dy: 0.0,
                top_only: true,
                owner: TREE_OWNER + ti as u32,
                slope: 0.0,
            });
        }
        // ! blocks: the block and its chain are solids (the bounce is drawn only)
        for (bi, b) in self.exclamations.iter_mut().enumerate() {
            let level = &self.level;
            b.step(|(tx, ty)| {
                (0..2).all(|dx| (0..2).all(|dy| !level.cell(tx + dx, ty + dy).is_solid()))
            });
            for (k, (x0, y0, x1, y1)) in b.boxes().into_iter().enumerate() {
                self.level.solids.push(crate::level::Solid {
                    x0,
                    y0,
                    x1,
                    y1,
                    dx: 0.0,
                    dy: 0.0,
                    top_only: false,
                    owner: EXCL_OWNER + bi as u32 * 64 + k as u32,
                    slope: 0.0,
                });
            }
        }
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
        // Piranha Creeper stalks can be stood on (3D World)
        for (ai, a) in self.actors.iter().enumerate() {
            let Kind::PiranhaCreeper { path, .. } = a.kind else {
                continue;
            };
            if !a.alive() {
                continue;
            }
            let Some((_, codes)) = self.level.creepers.iter().find(|(p, _)| *p == path) else {
                continue;
            };
            for (k, (x0, y0, x1, y1)) in a.creeper_solids(codes).into_iter().enumerate() {
                self.level.solids.push(crate::level::Solid {
                    x0,
                    y0,
                    x1,
                    y1,
                    dx: 0.0,
                    dy: 0.0,
                    top_only: false,
                    owner: CREEPER_OWNER + ai as u32 * 128 + k as u32,
                    slope: 0.0,
                });
            }
        }
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
            // Mushroom Trampoline: landing launches at once — 6.0, or 4.43 with jump held; jump
            // pressed within 6 frames of a plain bounce re-launches at 4.43 − 0.1·frames
            let on = self.on_off;
            let tramp = self.actors.iter().any(|a| {
                a.id == id && (matches!(a.kind, Kind::Trampoline { .. }) || a.v3_bouncy(on))
            });
            if tramp {
                self.player.vy = if jump_held { 4.43 } else { 6.0 };
                self.tramp_late = if jump_held { 0 } else { 1 };
                self.player.on_ground = false;
                self.player.ride = None;
                self.events.push(Event::Jump);
            }
        }
        if self.tramp_late > 0 {
            if self.jump_pressed_now && self.tramp_late <= 6 {
                self.player.vy = 4.43 - 0.1 * (self.tramp_late - 1) as f32;
                self.tramp_late = 0;
            } else {
                self.tramp_late += 1;
                if self.tramp_late > 6 {
                    self.tramp_late = 0;
                }
            }
        }
    }

    /// Bill Blasters fire toward the player (13-bill-blaster.md): the first shot 60 frames after
    /// the muzzle is on screen with no player at the blaster, then every 190 frames; with the
    /// default content both get 0–59 extra frames from the course's shared generator each time
    /// they are set (0x710069cea0: (word × 60) >> 32), so a held blaster draws every frame [obs].
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
            let extra = |w: &mut Self| {
                if b.contents.is_none() {
                    w.course_rand(60) as i32
                } else {
                    0
                }
            };
            if !on_screen || at_blaster {
                self.blaster_timers[bi].0 = 60 + extra(self);
                continue;
            }
            let (main, cool) = &mut self.blaster_timers[bi];
            *main -= 1;
            *cool -= 1;
            if *main > 0 || *cool > 0 {
                continue;
            }
            self.blaster_timers[bi].1 = 190 + extra(self);
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
                a.phase = b.homing as u8;
            }
            self.add_actor(a);
            self.events.push(Event::Kick);
        }
    }

    // ---------------------------------------------------------------- switches, donuts, belts

    /// Set dotted-line block solidity from the ON/OFF state (red solid when ON).
    fn apply_on_off(&mut self) {
        let on = self.on_off;
        // track junctions read the same bit (set = ON) [obs 0x7100341a00]
        self.level.tracks.on = on;
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
        if self.clear.goal == crate::clear::Goal::PSwitch {
            let r = self.clear.count(None, 1);
            self.condition_counted(Some(r));
        }
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
                    Some(Contents::Power(w)) => {
                        if let Kind::PowerItem { what } = self.pu_item(w) {
                            self.collect_power_item(what, cp.x, cp.y);
                        }
                    }
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
            if let crate::level::WarpKind::Box { .. } = t.to.kind {
                // single player: the pair is used up and vanishes
                self.gone_boxes.insert((self.area, t.to.id >> 1));
            }
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
        // pole touch box x ∈ [X0−4, X0+5], y ∈ [Y0+14, Y0+166]; its collider (0x710244d810)
        // accepts collider kinds {0, 1, 2, 24}, so both player body boxes (A, kind 0; B, kind 1;
        // 0x71009b3990) touch it and the union is box B [obs]
        let hb = p.contact_box();
        if !overlap(hb, (x0 - 4.0, y0 + 14.0, x0 + 5.0, y0 + 166.0)) {
            return;
        }
        // a clear condition not met: the goal lets the player pass
        let held_shell = p.held.is_some_and(|id| {
            self.actors
                .iter()
                .any(|a| a.id == id && matches!(a.kind, Kind::Shell { .. }) && !a.buzzy)
        });
        let holding_stone = p.held.is_some_and(|id| {
            self.actors
                .iter()
                .any(|a| a.id == id && a.kind == Kind::Stone)
        });
        let cloud = p.ride.is_some_and(|id| {
            self.actors
                .iter()
                .any(|a| a.id == id && a.kind == Kind::Cloud)
        });
        if !self.clear.met(crate::clear::AtGoal {
            holding_shell: held_shell,
            holding_stone,
            cloud,
            ..p.at_goal()
        }) {
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
        self.custom_scroll = None;
        let area = self.camera_area();
        let (x, y) = (self.player.x, self.player.y);
        let level = &self.level;
        self.camera.reset(&area, (x, y + 16.0), (x, y), |tx, ty| {
            level.cell(tx, ty) == Cell::Solid
        });
        let v = self.camera.view(&self.camera_area());
        self.autoscroll_pos = if self.level.vertical {
            v.bottom
        } else {
            v.left
        };
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
        // preset autoscroll (themes.md §4.2): the view moves right (vertical areas: up) at
        // 0.5 / 1.0 / 2.0 px/frame (vertical 0.28 / 0.53 / 1.12); the player is pushed along
        if let 1..=3 = self.level.autoscroll {
            let s = (self.level.autoscroll - 1) as usize;
            if self.level.vertical {
                let max_y = (self.level.top - VIEW_H).max(self.level.bottom);
                if self.player.alive() && self.goal_phase == GoalPhase::None {
                    self.autoscroll_pos = (self.autoscroll_pos + [0.28, 0.53, 1.12][s]).min(max_y);
                }
                self.camera_y = self.autoscroll_pos;
            } else {
                let max_x = (self.level.right - VIEW_W).max(self.level.left);
                if self.player.alive() && self.goal_phase == GoalPhase::None {
                    self.autoscroll_pos = (self.autoscroll_pos + [0.5, 1.0, 2.0][s]).min(max_x);
                }
                self.camera_x = self.autoscroll_pos;
            }
        }
        if self.level.autoscroll == 4 && !self.level.scroll_nodes.is_empty() {
            self.custom_autoscroll();
        }
    }

    /// Custom autoscroll (89-autoscroll-camera.md, themes.md §4.3): the view centre starts at
    /// the nearest node not behind it and travels node to node at the speed of each segment's
    /// start node (setting 0–2 → 0.5 / 1.0 / 2.0 px/f [inf: field + 1 = slow/normal/fast; the
    /// loader that fills the camera's node table (+0xb8 + area·0x90) wasn't found]),
    /// easing by max(0.03·|Δ|, 0.01) per frame; it pauses during warps, deaths and the goal.
    fn custom_autoscroll(&mut self) {
        let mut nodes = self.level.scroll_nodes.clone();
        nodes.sort_by_key(|n| n.0);
        let vertical = self.level.vertical;
        let (next, mut v, (mut cx, mut cy)) = match self.custom_scroll {
            Some(s) => s,
            None => {
                let c = (self.camera_x + VIEW_W / 2.0, self.camera_y + VIEW_H / 2.0);
                let key = |n: &(u8, f32, f32, u8)| if vertical { n.2 - c.1 } else { n.1 - c.0 };
                let nearest = (0..nodes.len())
                    .min_by(|&a, &b| key(&nodes[a]).abs().total_cmp(&key(&nodes[b]).abs()))
                    .unwrap_or(0);
                let next = (nearest..nodes.len())
                    .find(|&i| key(&nodes[i]) >= 0.0)
                    .unwrap_or(nodes.len());
                (next, 0.0, c)
            }
        };
        let paused =
            !self.player.alive() || self.goal_phase != GoalPhase::None || self.transit.is_some();
        let mut next = next;
        if !paused && next < nodes.len() {
            let from = next.saturating_sub(1);
            let target = [0.5, 1.0, 2.0][nodes[from].3.min(2) as usize];
            let dv = target - v;
            v += dv.signum() * (dv.abs() * 0.03).max(0.01).min(dv.abs());
            let (tx, ty) = (nodes[next].1, nodes[next].2);
            let (dx, dy) = (tx - cx, ty - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= v {
                (cx, cy) = (tx, ty);
                next += 1;
            } else {
                cx += dx / d * v;
                cy += dy / d * v;
            }
        }
        self.custom_scroll = Some((next, v, (cx, cy)));
        let l = &self.level;
        self.camera_x = (cx - VIEW_W / 2.0).clamp(l.left, (l.right - VIEW_W).max(l.left));
        self.camera_y = (cy - VIEW_H / 2.0).clamp(l.bottom, (l.top - VIEW_H).max(l.bottom));
    }

    /// Autoscroll pushes the player with the view edges; squeezed against a wall = death.
    fn autoscroll_push(&mut self) {
        let custom = self.level.autoscroll == 4 && !self.level.scroll_nodes.is_empty();
        if !(matches!(self.level.autoscroll, 1..=3) || custom)
            || self.level.vertical
            || !self.player.alive()
            || self.player.control != Control::Normal
        {
            return;
        }
        let (l, r) = (self.camera_x + 8.0, self.camera_x + VIEW_W - 8.0);
        if self.player.x < l {
            self.player.x = l;
            let s = self.player.sensors();
            let blocked = [s.wall_y0, s.wall_y1].iter().any(|&wy| {
                self.level
                    .cell_at(self.player.x + s.wall_x, self.player.y + wy)
                    .is_solid()
            });
            if blocked {
                self.player.kill();
                self.events.push(Event::Death);
            }
        } else if self.player.x > r {
            self.player.x = r;
        }
    }

    // ---------------------------------------------------------------- drawing

    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = self.sprites_in_layers();
        if self.options.smm1_layering {
            for s in &mut out {
                s.layer = smm1::smm1_layer(s.layer);
            }
        }
        out
    }

    fn sprites_in_layers(&self) -> Vec<Sprite> {
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
        self.background_sprites(&mut out);
        // tracks: drawn from the white line texels of the field atlas (tile 0x80) [placeholder
        // for the game's per-cell track tiles], with a dot at every free end
        let line = |px: f32, py: f32, size: f32| Sprite {
            image: SpriteImage {
                model: field,
                texture: field,
                rect: Some((12.0, 136.0, 1.0, 1.0)),
            },
            x: px - size / 2.0,
            y: py - size / 2.0,
            w: size,
            h: size,
            flip_x: false,
            flip_y: false,
            layer: 9,
        };
        for poly in self.level.tracks.polylines() {
            for w in poly.windows(2) {
                let (a, b) = (w[0], w[1]);
                let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs())).ceil().max(1.0) as usize;
                for k in 0..n {
                    let t = k as f32 / n as f32;
                    let (px, py) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    if visible(px, py) {
                        out.push(line(px, py, 1.5));
                    }
                }
            }
        }
        for (px, py) in self.level.tracks.ends() {
            if visible(px, py) {
                out.push(line(px, py, 5.0));
            }
        }
        for t in &self.level.tiles {
            let (px, py) = (t.x as f32 * TILE, t.y as f32 * TILE);
            if visible(px, py) {
                let layer = smm1::tile_layer(t, self.options.smm1_layering);
                let mut s = tile(px, py, t.atlas, layer);
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
        // blinking blocks (solid / outline) and spike blocks [placeholder art: dotted-line
        // blocks and hard blocks until the 3D World models draw]
        for &(x, y, alt) in &self.level.blink_blocks {
            let (px, py) = (x as f32 * TILE, y as f32 * TILE);
            if visible(px, py) {
                let solid = alt == self.blink_alt;
                out.push(tile(px, py, 21 * 16 + 2 + if solid { 0 } else { 16 }, 12));
            }
        }
        for &(x, y, mode) in &self.level.spike_blocks {
            let out_now = match mode {
                1 => self.on_off,
                2 => !self.on_off,
                _ => self.spikes_out(),
            };
            for dx in 0..2 {
                for dy in 0..2 {
                    let (px, py) = ((x + dx) as f32 * TILE, (y + dy) as f32 * TILE);
                    if visible(px, py) {
                        out.push(tile(px, py, if out_now { 43 } else { 6 }, 12));
                    }
                }
            }
        }
        for c in &self.level.pink_coins {
            let (px, py) = (c.x as f32 * TILE, c.y as f32 * TILE);
            if visible(px, py) && !self.pink.contains(&(self.area, c.x, c.y)) {
                out.push(Self::model_sprite(
                    "M1_Object_coinrotate_pink",
                    ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4],
                    px,
                    py,
                    16.0,
                    16.0,
                    false,
                    20,
                ));
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
        // warp boxes (3D World art; drawn until their pair is used up)
        for w in &self.level.warps {
            let crate::level::WarpKind::Box { key } = w.kind else {
                continue;
            };
            let pair = (self.area, w.id >> 1);
            if !visible(w.x, w.y) || self.gone_boxes.contains(&pair) {
                continue;
            }
            let locked = key && !self.unlocked_boxes.contains(&pair);
            out.push(Self::model_sprite(
                "M1_Object_warpbox",
                if locked { "key.0" } else { "wait.0" },
                w.x - 8.0,
                w.y,
                16.0,
                16.0,
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
        self.clown_sprites(&mut out);
        self.w3_sprites(&mut out);
        self.v3_sprites(&mut out);
        self.pu_sprites(&mut out);
        self.koopaling_sprites(&mut out);
        // Chain Chomp stakes and chains (the chain is cosmetic: 4 links, stake to Chomp)
        for a in &self.actors {
            if a.kind != (Kind::ChainChomp { chained: true }) || !visible(a.home_x, a.home_y) {
                continue;
            }
            let s = if a.big { 2.0 } else { 1.0 };
            out.push(Self::model_sprite(
                "M1_Enemy_wanwan",
                "kui.0",
                a.home_x - 8.0 * s,
                a.home_y,
                16.0 * s,
                16.0 * s,
                false,
                30,
            ));
            let (sx, sy) = (a.home_x, a.home_y + 12.0 * s);
            let (cx, cy) = (a.x, a.y + 8.0 * s);
            for k in 1..5 {
                let f = k as f32 / 5.0;
                let sag = (f * (1.0 - f))
                    * 12.0
                    * if a.phase == 2 || a.phase == 3 {
                        0.0
                    } else {
                        1.0
                    };
                out.push(Self::model_sprite(
                    "M1_Enemy_wanwan",
                    "chain.0",
                    sx + (cx - sx) * f - 4.0 * s,
                    sy + (cy - sy) * f - 4.0 * s - sag,
                    8.0 * s,
                    8.0 * s,
                    false,
                    30,
                ));
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
        for t in &self.level.trees {
            // the 3D World tree in parts (models at game scale): a 40-px root, 16-px trunk
            // units up to the canopy, the canopy (67 px tall) with its top at the platform
            let (_, top) = t.canopy();
            if !visible(t.x, t.y + 32.0) && !visible(t.x, top) {
                continue;
            }
            let ground = t.y - 8.0;
            let crown = top - 67.0;
            let mut parts = vec![("treebell_root.0", ground)];
            let mut y = ground + 40.0;
            const UNITS: [&str; 6] = [
                "treebell_unitA.0",
                "treebell_unitB.0",
                "treebell_unitC.0",
                "treebell_unitD.0",
                "treebell_unitE.0",
                "treebell_unitF.0",
            ];
            while y < crown {
                parts.push((UNITS[parts.len() % UNITS.len()], y));
                y += 16.0;
            }
            parts.push(("tree_top.0", crown));
            for (stem, y) in parts {
                out.push(Self::model_sprite(
                    "M1_Object_treebell",
                    stem,
                    t.x - 8.0,
                    y,
                    16.0,
                    16.0,
                    false,
                    4,
                ));
            }
        }
        for b in &self.exclamations {
            let lift = b.lift();
            for (k, (x0, y0, _, _)) in b.boxes().into_iter().enumerate() {
                if visible(x0 + 16.0, y0 + 16.0) {
                    out.push(Self::model_sprite(
                        "M1_Object_block_bikkuri",
                        if k == 0 && b.emptied() {
                            "block_bikkurikara.0"
                        } else {
                            "block_bikkuri.0"
                        },
                        x0 + 8.0,
                        y0 + lift,
                        16.0,
                        16.0,
                        false,
                        12,
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
                if c.red {
                    "senkan_houdai_search"
                } else {
                    "senkan_houdai_base"
                },
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
                    fb.x + i as f32 * 8.0 * a.dcos(),
                    fb.y + i as f32 * 8.0 * a.dsin(),
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
                if b.homing { "search.0" } else { "wait.0" },
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
                    if b.homing { "unit_search.0" } else { "unit.0" },
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
            if let Kind::Trampoline { width, .. } = a.kind {
                // 3D World pieces: left cap, middle units, right cap (meshes at their own size)
                let x0 = a.x - width as f32 * 8.0;
                for i in 0..width {
                    let stem = if width == 1 {
                        "tranpoline_unit.0"
                    } else if i == 0 {
                        "tranpoline_left.0"
                    } else if i == width - 1 {
                        "tranpoline_right.0"
                    } else {
                        "tranpoline_unit.0"
                    };
                    out.push(Self::model_sprite(
                        "M1_Object_trampoline",
                        stem,
                        x0 + i as f32 * TILE,
                        a.y + 8.0,
                        16.0,
                        16.0,
                        false,
                        15,
                    ));
                }
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
        for (k, o) in self.others.iter().enumerate() {
            let who = self.other_chars.get(k).copied().unwrap_or(k + 1);
            if let Some(s) = self.player_sprite_of(o, who) {
                out.push(s);
            }
        }
        self.yoshi_sprites(&mut out);
        if let Some(mut s) = self.player_sprite() {
            if self.player.yoshi.is_some() {
                // the rider sits on Yoshi's saddle [inf: drawing only; the rider is attached to
                // a joint of the Yoshi model (Ride exec 0x7100a32570 sits inside a larger
                // function Ghidra doesn't split), so the offset is in the model asset]
                s.y += 14.0;
                s.image.texture = "yoshi_wait.0";
            }
            if let Some(h) = self.player.helmet {
                // the helmet on the head [inf: drawing only; the shell model follows the
                // player model's head joint (asset data)]
                let p = &self.player;
                out.push(Self::model_sprite(
                    if h == 2 {
                        "M1_Player_Togezo_model"
                    } else {
                        "M1_Player_Met_model"
                    },
                    if h == 2 { "togezo.0" } else { "met.0" },
                    p.x - 8.0,
                    p.y + p.sensors().head_y - 6.0,
                    16.0,
                    16.0,
                    false,
                    s.layer + 1,
                ));
            }
            if let Some(heels) = self.player.shoe {
                // the player stands in the shoe: shoe at the feet, the player drawn 8 px higher
                let p = &self.player;
                let mut sh = if p.shoe_karon {
                    Self::model_sprite(
                        "M1_Player_Karon_model",
                        "shellkara.0",
                        p.x - 8.0,
                        p.y,
                        16.0,
                        16.0,
                        p.facing > 0,
                        s.layer + 1,
                    )
                } else {
                    let model = if heels {
                        "M1_Enemy_kutsu_kuriboB"
                    } else {
                        "M1_Enemy_kutsu_kuriboA"
                    };
                    let mut sh = Self::model_sprite(
                        model,
                        "wait.0",
                        p.x - 8.0,
                        p.y,
                        16.0,
                        16.0,
                        p.facing > 0,
                        s.layer + 1,
                    );
                    sh.image.rect = Some((0.0, 16.0, 16.0, 16.0));
                    sh
                };
                sh.layer = s.layer + 1;
                s.y += 8.0;
                out.push(s);
                out.push(sh);
            } else {
                out.push(s);
            }
        }
        out
    }

    /// Theme background model and texture (M1_DV_<theme>[_D]), if the theme has one.
    /// Underground and castle have none (black). Ghost house only has the night art.
    pub fn background(&self) -> Option<(&'static str, &'static str)> {
        let night = self.level.night;
        Some(match (self.level.theme, night) {
            (0, false) => ("M1_DV_plain", "M1_DV_plain_00"),
            (0, true) => ("M1_DV_plain_D", "M1_DV_plain_D_00.0"),
            (3, false) => ("M1_DV_airship", "M1_DV_airship_00"),
            (3, true) => ("M1_DV_airship_D", "M1_DV_airship_D.00.0"),
            (4, false) => ("M1_DV_water", "M1_DV_water"),
            (4, true) => ("M1_DV_water_D", "M1_DV_water_D_00.0"),
            (5, _) => ("M1_DV_hauntedhouse_D", "M1_DV_hauntedhouse_D.00"),
            (6, false) => ("M1_DV_snow", "M1_DV_snow_00"),
            (6, true) => ("M1_DV_snow_D", "M1_DV_snow_D_00.0"),
            (7, _) => ("M1_DV_desert", "M1_DV_desert_00"),
            (8, _) => ("M1_DV_athletic", "M1_DV_athletic_00"),
            (9, _) => ("M1_DV_woods", "M1_DV_woods_00"),
            _ => return None,
        })
    }

    /// The background: a 512×512 picture repeated sideways, anchored 32 px below the area bottom,
    /// scrolling at half the camera speed [inf parallax 0.5: drawing only; the background
    /// layer's scroll factor is set by the BG draw code, not traced this pass].
    fn background_sprites(&self, out: &mut Vec<Sprite>) {
        let Some((model, tex)) = self.background() else {
            return;
        };
        // (a whole-course render pins the backdrop to the world so the views join up)
        let p = if self.map_view { 1.0 } else { 0.5 };
        let ox = -128.0 + self.camera_x * (1.0 - p);
        let oy = (self.level.bottom - 32.0 + self.camera_y * (1.0 - p)).min(self.camera_y);
        let mut k = ((self.camera_x - ox) / 512.0).floor();
        while ox + k * 512.0 < self.camera_x + VIEW_W {
            let mut s = Self::model_sprite(model, tex, ox + k * 512.0, oy, 512.0, 512.0, false, 0);
            s.image.rect = Some((0.0, 0.0, 512.0, 512.0));
            out.push(s);
            k += 1.0;
        }
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
            Kind::Goomba | Kind::Goombrat => {
                // walk.1 is walk.0 mirrored: the texture swap alone animates it
                let t = if a.mode == Mode::Stomped {
                    "damage.0"
                } else {
                    walk2(WALK_FRAMES)
                };
                let model = if a.kind == Kind::Goombrat {
                    "M1_Enemy_kakibo"
                } else {
                    "M1_Enemy_kuribo"
                };
                (model, t, 16.0, 32.0, false)
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
                let spin = (a.age as usize / 2) % 4;
                let empty = a.shell_of != 0;
                let t = if a.mode == Mode::ShellSlide {
                    if empty {
                        ["shellkara.0", "shellkara.1", "shellkara.2", "shellkara.3"][spin]
                    } else {
                        ["shell.0", "shell.1", "shell.2", "shell.3"][spin]
                    }
                } else if empty {
                    "shellkara.0"
                } else if a.timer < crate::actors::SHELL_SHAKE && (a.timer / 4) % 2 == 0 {
                    "revival.0"
                } else {
                    "shell.0"
                };
                (
                    match (a.shell_of, a.buzzy, red) {
                        (2, _, _) => "M1_Enemy_togezo",
                        (3, _, _) => "M1_Enemy_karon",
                        (_, true, _) => "M1_Enemy_met",
                        (_, _, true) => "M1_Enemy_nokonokoB",
                        _ => "M1_Enemy_nokonokoA",
                    },
                    t,
                    16.0,
                    if t.starts_with("revival") { 32.0 } else { 16.0 },
                    false,
                )
            }
            // SMM1 extras: the Weird Mushroom's own art; the Mystery Mushroom's (24 × 24)
            Kind::Mushroom if a.smm1_item == 1 => {
                ("M1_Item_kinokofunny", "kinokofunny.0", 16.0, 16.0, false)
            }
            Kind::Mushroom if a.smm1_item == 2 => {
                ("M1_Item_CharaKinoko", "Add_kinoko", 24.0, 24.0, false)
            }
            Kind::Mushroom => ("M1_Item_kinoko", "wait.0", 16.0, 16.0, false),
            Kind::SuperHammer => ("M1_Item_hammer", "wait.0", 16.0, 16.0, false),
            // SMB1's Big Mushroom: one 16×16 texture drawn through a 32×32 quad
            Kind::StyleItem if self.style == crate::player::Style::M1 => {
                ("M1_Item_special", "Add_special.0", 16.0, 16.0, false)
            }
            Kind::StyleItem => (
                "M1_Item_special",
                ["wait.0", "wait.1", "wait.2"][(self.tick as usize / 8) % 3],
                16.0,
                16.0,
                a.x < a.home_x,
            ),
            Kind::OneUp => ("M1_Item_kinoko1up", "wait.0", 16.0, 16.0, false),
            Kind::SuperballFlower => (
                "M1_Item_ballflower",
                ["wait.0", "wait.1", "wait.2"][(a.age as usize / 4) % 3],
                16.0,
                16.0,
                false,
            ),
            Kind::Superball => ("M1_Object_superball", "superball", 8.0, 8.0, false),
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
            Kind::SideSpring => (
                "M1_Object_jumpstep2",
                ["jump.0", "jump.3", "jump.2", "jump.1"][(a.pause / 3).clamp(0, 3) as usize],
                16.0,
                16.0,
                false,
            ),
            Kind::Spring => (
                "M1_Object_jumpstep",
                ["jump.0", "jump.1", "jump.1", "jump.2", "jump.2"][a.pause.clamp(0, 4) as usize],
                16.0,
                16.0,
                false,
            ),
            Kind::BulletBill => (
                "M1_Enemy_killer",
                if a.phase == 1 { "search.0" } else { "wait.0" },
                16.0,
                16.0,
                a.dir > 0,
            ),
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
            Kind::SpikeTop { .. } => ("M1_Enemy_togemetA", walk2(8), 16.0, 16.0, a.dir > 0),
            Kind::HammerBro if a.alt => (
                "M1_Enemy_bros_mega",
                if a.timer < 12 { "throw.0" } else { walk2(8) },
                32.0,
                32.0,
                a.dir > 0,
            ),
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
            Kind::BanzaiBill { dir, homing } => (
                "M1_Enemy_killermagnum",
                if homing { "search.0" } else { "wait.0" },
                64.0,
                64.0,
                dir == 0,
            ),
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
                if a.phase == 1 {
                    "senkan_houdai_ball_search.0"
                } else {
                    "senkan_houdai_ball.0"
                },
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
            Kind::Bowser { .. }
            | Kind::BowserFire
            | Kind::BoomBoom { .. }
            | Kind::Shuriken
            | Kind::BowserJr { .. } => a.boss_look()?,
            Kind::AntTrooper { .. }
            | Kind::Stingby
            | Kind::Skipsqueak { .. }
            | Kind::HopChops
            | Kind::Porcupuffer
            | Kind::Bully
            | Kind::Charvaargh
            | Kind::PiranhaCreeper { .. } => a.w3_look()?,
            Kind::Spike
            | Kind::SpikeBall { .. }
            | Kind::Pokey { .. }
            | Kind::Mechakoopa { .. }
            | Kind::MechaMissile
            | Kind::MechaLaser
            | Kind::OnOffTrampoline { .. }
            | Kind::Koopaling { .. }
            | Kind::KoopalingShot { .. } => a.v3_look()?,
            Kind::PowerItem { .. }
            | Kind::PlayerArrow
            | Kind::PlayerBoomerang
            | Kind::LinkBomb
            | Kind::CannonShot
            | Kind::RedPowRing => a.pu_look()?,
            Kind::LavaBubble => ("M1_Enemy_bubble", "wait.0", 16.0, 16.0, false),
            // [placeholder: the egg uses Yoshi's hatching frame; drawn per style by the remap]
            Kind::YoshiEgg { .. } => ("M1_Player_Yoshi", "born_body.0", 32.0, 32.0, false),
            Kind::Yoshi { .. } => return None, // drawn with his rider (see `yoshi_sprites`)
            Kind::FloatCoin => {
                let field = self.field_model;
                let mut sp =
                    Self::model_sprite(field, field, a.x - 8.0, a.y, TILE, TILE, false, 25);
                sp.image.rect = Some((7.0 * TILE, 0.0, TILE, TILE));
                return Some(sp);
            }
            Kind::FishBone => (
                "M1_Enemy_fishbone",
                if a.phase == 2 {
                    walk_frame(&["attack.0", "attack.1"], a.age, 4)
                } else {
                    walk_frame(&["swim.0", "swim.1"], a.age, 10)
                },
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::MovingBlock {
                block: BlockKind::Hidden,
                ..
            } => return None,
            Kind::MovingBlock { block, .. } => {
                let i = crate::level::block_atlas(block);
                let field = self.field_model;
                let mut sp =
                    Self::model_sprite(field, field, a.x - 8.0, a.y, TILE, TILE, false, 25);
                sp.image.rect = Some(((i % 16) as f32 * TILE, (i / 16) as f32 * TILE, TILE, TILE));
                return Some(sp);
            }
            Kind::ClownCar { fire } => (
                if fire {
                    "M1_Enemy_koopaclownfire"
                } else {
                    "M1_Enemy_koopaclown"
                },
                ["wait.0", "wait.1", "wait.2", "wait.3"][(self.tick as usize / 8) % 4],
                32.0,
                32.0,
                a.dir > 0,
            ),
            Kind::Crate { .. } => ("M1_Object_woodbox", "woodbox.0", 16.0, 16.0, false),
            Kind::Stone => ("M1_Object_stone", "wait.0", 16.0, 16.0, false),
            // Story Mode NPCs: the Toad / Toadette player models (the _3 / _4 variants)
            Kind::EscortToad => (
                character_model("M1_Player_MarioMdl", 2),
                if !a.on_ground {
                    "jump.0"
                } else if a.vx.abs() > 0.3 {
                    ["walk.0", "walk.1", "walk.2"][(a.age as usize / 6) % 3]
                } else {
                    "wait.0"
                },
                16.0,
                16.0,
                a.dir < 0,
            ),
            Kind::GoalToadette => (
                character_model("M1_Player_MarioMdl", 3),
                "wait.0",
                16.0,
                16.0,
                true,
            ),
            Kind::TrackBlock { .. } => {
                // blinking before it vanishes; nothing while gone
                if a.phase == 5 || (a.phase == 4 && (a.timer / 4) % 2 == 0) {
                    return None;
                }
                ("M1_Object_block_orbit", "wait.0", 16.0, 16.0, false)
            }
            Kind::Trampoline { .. } | Kind::TrackMount { .. } | Kind::EmptyShell { .. } => {
                return None;
            }
            // 3D World only: the mesh is drawn at its own size (16 = no scaling)
            Kind::KoopaCar => (
                "M1_Enemy_koopacar",
                match (a.phase, a.hops) {
                    (0, 0) => "wait.0",
                    (0, 1) => "damage1.0",
                    (0, _) => "damage2.0",
                    (1, _) => "wheelspin.0",
                    (_, 0) => "run.0",
                    (_, 1) => "rundamaged1.0",
                    _ => "rundamaged2.0",
                },
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::ShoeGoomba { heels } => (
                if heels {
                    "M1_Enemy_kutsu_kuriboB"
                } else {
                    "M1_Enemy_kutsu_kuriboA"
                },
                walk_frame(&["wait.0", "wait.1"], a.age, 8),
                16.0,
                32.0,
                a.dir > 0,
            ),
            Kind::Shoe { heels } => {
                // the shoe is the lower half of the shoe Goomba's art
                let model = if heels {
                    "M1_Enemy_kutsu_kuriboB"
                } else {
                    "M1_Enemy_kutsu_kuriboA"
                };
                let s = if a.big { 2.0 } else { 1.0 };
                let mut sp = Self::model_sprite(
                    model,
                    "wait.0",
                    a.x - 8.0 * s,
                    a.y,
                    16.0 * s,
                    16.0 * s,
                    false,
                    layer,
                );
                sp.image.rect = Some((0.0, 16.0, 16.0, 16.0));
                return Some(sp);
            }
            Kind::MontyMole => (
                "M1_Enemy_choropoo",
                match a.phase {
                    0 => return None,
                    1 => "appear.0",
                    _ => walk_frame(&["walk.0", "walk.1"], a.age, 6),
                },
                16.0,
                16.0,
                a.dir > 0,
            ),
            Kind::AngrySun { moon } => (
                "M1_Object_sun",
                if moon || a.phase == 0 {
                    "wait.0"
                } else if (self.tick / 6).is_multiple_of(2) {
                    "attack.0"
                } else {
                    "attack.1"
                },
                32.0,
                32.0,
                false,
            ),
            Kind::ChainChomp { .. } => (
                "M1_Enemy_wanwan",
                if a.phase == 2 || a.phase == 3 || (self.tick / 8).is_multiple_of(2) {
                    "wait.1"
                } else {
                    "wait.0"
                },
                16.0,
                16.0,
                a.dir > 0,
            ),
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
            Kind::RingBoo { .. } => ("M1_Enemy_teresa", "walk.0", 16.0, 16.0, a.dir > 0),
            Kind::BabyBlooper { .. } => (
                "M1_Enemy_gessomini",
                ["wait.0", "wait.1"][(a.age as usize / 16) % 2],
                8.0,
                8.0,
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
        if matches!(a.kind, Kind::Fireball | Kind::EnemyFire) || a.kind.w3_centred() {
            s.y = a.y + a.height() / 2.0;
        }
        if a.kind.is_v3() {
            s.y = a.y + a.v3_draw_dy();
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

    /// Player 1's sprite (also used to draw a replay's player as a ghost).
    pub fn player_sprite(&self) -> Option<Sprite> {
        self.player_sprite_of(&self.player, self.main_char)
    }

    /// A player's sprite; `who` 0 Mario, 1 Luigi, 2 Toad, 3 Toadette.
    fn player_sprite_of(&self, p: &Player, who: usize) -> Option<Sprite> {
        if p.hidden {
            return None;
        }
        // flicker while invincible after damage
        if p.invuln > 0 && (p.invuln / 2) % 2 == 1 && p.control == Control::Normal {
            return None;
        }
        let form = match p.control {
            Control::Change { timer, from } if crate::player::change_shows_old(timer) => from,
            _ => p.form,
        };
        // the raccoon and cape forms are named as SMB1 models so the style remap finds them
        // (M3_Player_TnkMarioMdl; SMW's cape Mario uses its Super Mario body)
        let model = match form {
            Form::Small => "M1_Player_MarioMdl",
            // NSMBU draws one 3D Mario with the propeller part (the mesh path maps it)
            Form::Super | Form::Cape | Form::Propeller => "M1_Player_SpMarioMdl",
            Form::Fire => "M1_Player_FireMarioMdl",
            Form::Superball => "M1_Player_SpBallMarioMdl",
            Form::Mega => "M1_Player_MegaMarioMdl",
            Form::Raccoon => "M1_Player_TnkMarioMdl",
            // 3D World only: the mesh path draws the cat suit (3W_Player_MarioClimb) and the
            // builder outfit (3W_Player_MarioBuilder)
            Form::Cat => "M1_Player_NekoMarioMdl",
            Form::Builder => "M1_Player_BuilderMarioMdl",
            // SMM1 extras (SMB1 only): Lanky Mario's own art; a costume is drawn below
            Form::Lanky => "M1_Player_SlimMarioMdl",
            Form::Costume => "M1_Player_MarioMdl",
            // the update forms: chosen with their frames below (world/powerups.rs)
            Form::Link | Form::Balloon => "M1_Player_MarioMdl",
            Form::Frog | Form::Squirrel | Form::Boomerang | Form::Smb2 => "M1_Player_SpMarioMdl",
        };
        // the multiplayer characters are the model's variants: _2 Luigi, _3 Toad, _4 Toadette
        // (star power is a palette effect in the game; not drawn yet [⬜])
        let model = character_model(model, who);
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
            Pose::Slide => "slip.0",
            Pose::Spin => {
                ["spin.0", "spin.1", "spin.2", "spin.3"][((p.walk_phase / 3.0) as usize) % 4]
            }
            Pose::Climb => {
                if ((p.walk_phase / 8.0) as i32) % 2 == 0 {
                    "climb.0"
                } else {
                    "climb.1"
                }
            }
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
        // raccoon: tail swing, flight flaps, slow-fall swings; cape: flight / glide poses
        let k3 = (self.tick as usize / 3) % 3;
        let (model, frame) = match form {
            Form::Raccoon if p.attack > 0 && p.on_ground => (
                model,
                ["wait.1", "wait.2", "wait.3", "wait.4"][(p.attack as usize / 4) % 4],
            ),
            Form::Raccoon if !p.on_ground && p.flight > 0 => (
                model,
                [
                    "b_dash_flyswing.0",
                    "b_dash_flyswing.1",
                    "b_dash_flyswing.2",
                ][k3],
            ),
            Form::Raccoon if !p.on_ground && (p.slow_fall > 0 || p.attack > 0) => {
                (model, ["jump_swing.0", "jump_swing.1", "jump_swing.2"][k3])
            }
            // cat: the mesh path plays these with its "Climb" animation prefix
            Form::Cat if p.cat_climb > 0 => (model, "WallRun.0"),
            Form::Cat if p.cat_climb < 0 => (model, "WallFall.0"),
            Form::Cat if p.dive => (model, "BodyAttackLoop.0"),
            Form::Cat if p.attack > 0 && p.on_ground => (model, "SlidingAttack.0"),
            Form::Cat if p.attack > 0 => (model, "AttackAir.0"),
            Form::Builder if p.attack > 0 && p.on_ground => (model, "Hammer_Right.0"),
            Form::Builder if p.attack > 0 => (model, "HammerAir_Right.0"),
            // the cape's glide poses by stage (cape model list 0x71009875a0)
            Form::Cape if p.cape_stage().is_some() => (
                "M1_Player_MantMario",
                ["up2.0", "up1.0", "down1.0", "down2.0", "down3.0", "down4.0"]
                    [p.cape_stage().unwrap_or(0) as usize],
            ),
            Form::Cape if matches!(p.cape, crate::player::CapeState::Rise(_)) => {
                ("M1_Player_MantMario", "up1.0")
            }
            Form::Cape if !p.on_ground && p.vy <= -0.99 && p.vy >= -1.01 => {
                ("M1_Player_MantMario", "down1.0")
            }
            // a lanky jump plays the walk animation fast (3×) in the air (SMM1 0x02683e00)
            Form::Lanky if p.lanky_jump && !p.on_ground && p.pose == Pose::Jump => (
                model,
                ["walk.0", "walk.1", "walk.2"][(self.tick as usize / 2) % 3],
            ),
            _ => self
                .pu_player_look(p, form, who, frame)
                .unwrap_or((model, frame)),
        };
        let layer = if self.in_pipe() {
            5
        } else if self.options.smm1_layering {
            smm1::player_layer(p)
        } else {
            50
        };
        if form == Form::Costume {
            return Some(smm1::costume_sprite(p, layer, self.tick));
        }
        // carrying (not in SMB1): the carry poses, named per style
        let frame = if p.held.is_some() {
            let k = ((p.walk_phase / 6.0) as usize) % 3;
            match (p.pose, self.style) {
                (Pose::Walk, crate::player::Style::MW) => {
                    ["walk_carry.0", "walk_carry.1", "walk_carry.2"][k]
                }
                (Pose::Walk, _) => ["carry_walk.0", "carry_walk.1", "carry_walk.2"][k],
                (Pose::Jump, _) => "carry_jump.0",
                (Pose::Dead, _) => "dead.0",
                _ => "carry_wait.0",
            }
        } else {
            frame
        };
        let frame = if p.pose == Pose::Dead {
            "dead.0"
        } else {
            frame
        };
        let model = if p.pose == Pose::Dead {
            character_model("M1_Player_MarioMdl", who)
        } else {
            model
        };
        // (Mega Mario's art is 16×16 drawn through a 32×32 quad)
        let h = if small || p.pose == Pose::Dead || form == Form::Mega {
            16.0
        } else {
            32.0
        };
        // the update forms' art is 32 × 32 (world/powerups.rs)
        let (w, h) = if p.pose == Pose::Dead {
            (16.0, h)
        } else {
            powerups::sprite_size(form).unwrap_or((16.0, h))
        };
        Some(Sprite {
            image: SpriteImage {
                model,
                texture: frame,
                rect: None,
            },
            x: p.x - w / 2.0,
            y: p.y,
            w,
            h,
            flip_x: if p.style == crate::player::Style::M1 && !p.on_ground && !p.in_water {
                p.drawn_facing < 0
            } else {
                p.facing < 0
            },
            flip_y: false,
            // behind terrain while sliding through a pipe
            layer,
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
            lives_gained: self.lives_gained,
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
    pub lives_gained: u32,
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
            rail: None,
            link: -1,
            clown: false,
            alt: false,
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
        // the chain counter is bumped before it is read: the first stomp is score id 1
        assert_eq!(w.score, 200);
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
            rail: None,
            link: -1,
            clown: false,
            alt: false,
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
    fn unmet_clear_condition_locks_the_goal() {
        let mut lvl = Level::flat(60);
        lvl.set(20, 2, Cell::Solid);
        lvl.goal = Some((20.0 * TILE + 8.0, 3.0 * TILE));
        let mut w = World::with_level(lvl);
        // "reach the goal as Fire Mario", played as small Mario
        w.clear = crate::clear::ClearCondition::from_header(2, 0x2a8e77bb, 0);
        let mut grabbed = false;
        for i in 0..400 {
            let jump = if i % 30 < 20 { input::JUMP } else { 0 };
            w.step(Input {
                buttons: input::RIGHT | input::RUN | jump,
            });
            grabbed |= w.events.contains(&Event::Goal);
        }
        assert!(!grabbed, "passes the pole");
        assert!(w.player.x > 21.0 * TILE);
    }

    #[test]
    fn defeating_enemies_counts_toward_the_condition() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Goomba, 200.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 240.0, 32.0));
        let mut w = World::with_level(lvl);
        w.clear = crate::clear::ClearCondition::from_header(1, 0x7f07acbf, 2);
        w.player.x = 40.0;
        run(&mut w, 0, 1);
        for a in &mut w.actors {
            a.stomp();
        }
        run(&mut w, 0, 1);
        assert_eq!(w.clear.count, 2);
        assert!(w.clear.met(Default::default()));
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
            rail: None,
            link: -1,
            clown: false,
            alt: false,
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
            rail: None,
            link: -1,
            clown: false,
            alt: false,
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
        lvl.spawns.push(spawn(
            Kind::Thwomp {
                side: false,
                right: false,
            },
            200.0,
            120.0,
        ));
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
    fn the_blast_box_is_half_18_around_8_px_up() {
        let mut w = World::with_level(Level::flat(40));
        w.player.x = 40.0;
        let mut bomb = Actor::new(Kind::BobOmb, 200.0, 32.0);
        bomb.phase = 9;
        w.add_actor(bomb);
        // the blast spans x 182…218; a Goomba (half 8) touches it up to 25.99 px away
        w.add_actor(Actor::new(Kind::Goomba, 225.0, 32.0));
        w.add_actor(Actor::new(Kind::Goomba, 227.0, 32.0));
        w.explosions();
        let goombas: Vec<bool> = w
            .actors
            .iter()
            .filter(|a| a.kind == Kind::Goomba)
            .map(|a| a.alive())
            .collect();
        assert_eq!(goombas, [false, true]);
    }

    #[test]
    fn a_rising_liquid_waits_where_it_was_while_the_player_is_in_the_other_area() {
        let mut main = Level::flat(40);
        main.liquid = Some(crate::level::LiquidSpec {
            kind: 2,
            start: 12.0,
            end: 200.0,
            mode: 1,
            speed: 3,
            full: false,
        });
        let mut w = World::with_areas(vec![main, Level::flat(40)]);
        w.player.x = 100.0;
        w.player.y = 300.0;
        for _ in 0..120 {
            if let Some(l) = w.liquid.as_mut() {
                l.step();
            }
        }
        let h = w.liquid.unwrap().y;
        assert!(h > 12.0);
        w.switch_area(1);
        w.switch_area(0);
        assert_eq!(w.liquid.unwrap().y, h);
    }

    #[test]
    fn the_cape_spin_box_starts_behind_and_flips_every_two_frames() {
        let mut w = World::with_level(Level::flat(40));
        w.player.x = 100.0;
        w.player.y = 32.0;
        w.player.facing = 1;
        w.player.form = Form::Cape;
        w.add_actor(Actor::new(Kind::Goomba, 88.0, 32.0));
        w.add_actor(Actor::new(Kind::Goomba, 112.0, 32.0));
        w.player.attack = 15; // animation frame 0: behind
        w.tail_attack();
        assert_eq!(
            w.actors.iter().map(|a| a.alive()).collect::<Vec<_>>(),
            [false, true]
        );
        w.player.attack = 13; // frame 2: in front
        w.tail_attack();
        assert!(!w.actors[1].alive());
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
    fn chain_chomp_stays_on_its_chain_and_lunges() {
        let mut lvl = Level::flat(40);
        lvl.spawns
            .push(spawn(Kind::ChainChomp { chained: true }, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 280.0;
        let mut lunged = false;
        let mut far: f32 = 0.0;
        for _ in 0..400 {
            run(&mut w, 0, 1);
            let a = &w.actors[0];
            lunged |= a.phase == 2;
            let d = ((a.x - a.home_x).powi(2) + (a.y + 8.0 - (a.home_y + 8.0)).powi(2)).sqrt();
            far = far.max(d);
        }
        assert!(lunged, "lunges within 120–179 frames + 61 crouch");
        assert!(far <= 51.2 + 4.0 + 6.0, "reach {far}"); // the return curve may bow out a little
        // stomping bounces off and leaves it unharmed
        let a = &w.actors[0];
        let (ax, ay) = (a.x, a.y);
        w.player.x = ax;
        w.player.y = ay + 14.0;
        w.player.vy = -2.0;
        w.player_interactions(false);
        assert!(w.player.vy > 0.0);
        assert!(w.actors[0].alive());
    }

    #[test]
    fn lava_bubble_hides_then_leaps_to_its_height() {
        let mut lvl = Level::flat(40);
        lvl.bottom = 0.0;
        // placed 5 tiles above the reference level (bottom − 12)
        let home = -12.0 + 5.0 * TILE;
        lvl.spawns.push(spawn(Kind::LavaBubble, 200.0, home));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 60);
        assert!(!w.actors[0].solid_contact(), "hidden");
        let mut top = f32::MIN;
        let mut first_vy = 0.0;
        for _ in 0..200 {
            run(&mut w, 0, 1);
            let a = &w.actors[0];
            if a.phase == 1 && first_vy == 0.0 {
                first_vy = a.vy;
            }
            top = top.max(a.y);
        }
        // H = 6 tiles: A = 6·4/7 + 1 = 4.43
        assert!((first_vy - 4.43).abs() < 0.05, "{first_vy}");
        assert!(
            top <= home + 4.0 && top > home - 24.0,
            "top {top} home {home}"
        );
    }

    #[test]
    fn monty_mole_wakes_near_the_player_and_chases() {
        let mut lvl = Level::flat(60);
        // buried in a 2-tile-high mound of ground
        for x in 20..23 {
            lvl.set(x, 2, Cell::Solid);
        }
        lvl.spawns
            .push(spawn(Kind::MontyMole, 21.0 * TILE + 8.0, 2.0 * TILE));
        let mut w = World::with_level(lvl);
        w.player.x = 21.0 * TILE - 120.0;
        run(&mut w, 0, 30);
        assert_eq!(w.actors[0].phase, 0, "too far to wake");
        w.player.x = 21.0 * TILE - 60.0;
        run(&mut w, 0, 1);
        assert_eq!(w.actors[0].phase, 1);
        run(&mut w, 0, 200);
        let a = &w.actors[0];
        assert_eq!(a.phase, 3);
        assert!(a.vx < 0.0, "runs toward the player");
    }

    #[test]
    fn angry_sun_waits_in_a_corner_then_swoops_across() {
        let mut lvl = Level::flat(60);
        lvl.spawns
            .push(spawn(Kind::AngrySun { moon: false }, 200.0, 150.0));
        let mut w = World::with_level(lvl);
        w.player.x = 100.0;
        run(&mut w, 0, 2);
        let x0 = w.actors[0].x - w.camera_x;
        assert!((x0 - (VIEW_W - 48.0)).abs() < 1.0, "right corner: {x0}");
        run(&mut w, 0, 120 + 36 + 130);
        let x1 = w.actors[0].x - w.camera_x;
        assert!((x1 - 48.0).abs() < 2.0, "left corner: {x1}");
    }

    #[test]
    fn vines_are_climbed_up_to_their_top() {
        let mut lvl = Level::flat(40);
        // a 6-tile vine standing on the ground at tile 10
        lvl.vines.push((10.0 * TILE + 8.0, 2.0 * TILE, 8.0 * TILE));
        let mut w = World::with_level(lvl);
        w.player.x = 10.0 * TILE + 4.0;
        run(&mut w, input::UP, 1);
        assert!(w.player.climbing.is_some());
        assert_eq!(w.player.x, 10.0 * TILE + 8.0 - 6.0, "hangs 6 px beside it");
        run(&mut w, input::UP, 200);
        let top = w.player.y;
        assert!(
            top + 16.0 >= 8.0 * TILE - 1.0 && top < 8.0 * TILE,
            "stops at the top: {top}"
        );
        // jump off, and no re-grab for 30 frames
        run(&mut w, input::JUMP, 1);
        assert!(w.player.climbing.is_none());
        assert!(w.player.vy > 0.0);

        // the grab needs Up and a player who isn't rising (0x7100a12cb0)
        let mut lvl = Level::flat(40);
        lvl.vines.push((10.0 * TILE + 8.0, 2.0 * TILE, 8.0 * TILE));
        let mut w = World::with_level(lvl);
        w.player.x = 10.0 * TILE + 4.0;
        run(&mut w, input::DOWN, 1);
        assert!(w.player.climbing.is_none(), "Down doesn't grab");
        w.player.y = 4.0 * TILE;
        w.player.vy = 2.0;
        w.player.on_ground = false;
        w.step(Input { buttons: input::UP });
        assert!(w.player.climbing.is_none(), "not while rising");
        for _ in 0..30 {
            w.step(Input { buttons: input::UP });
        }
        assert!(w.player.climbing.is_some(), "grabs once falling");
    }

    #[test]
    fn shoe_goomba_hops_drops_its_shoe_and_the_player_wears_it() {
        let mut lvl = Level::flat(60);
        lvl.spawns
            .push(spawn(Kind::ShoeGoomba { heels: false }, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 150.0;
        run(&mut w, 0, 49);
        assert!(w.actors[0].on_ground);
        run(&mut w, 0, 3);
        assert!(
            w.actors[0].vy > 0.0 && w.actors[0].vx < 0.0,
            "hops toward the player"
        );
        // defeat it: the empty shoe stays
        w.actors[0].stomp();
        run(&mut w, 0, 2);
        assert!(w.actors.iter().any(|a| matches!(a.kind, Kind::Shoe { .. })));
        run(&mut w, 0, 60);
        let shoe = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::Shoe { .. }))
            .unwrap();
        let (sx, sy) = (shoe.x, shoe.y);
        w.player.x = sx;
        w.player.y = sy + 12.0;
        w.player.vy = -1.0;
        w.player_interactions(false);
        assert_eq!(w.player.shoe, Some(false));
        // a shoe stomp defeats a Spiny
        w.add_actor(Actor::new(Kind::Spiny, w.player.x + 60.0, 32.0));
        let i = w.actors.iter().position(|a| a.kind == Kind::Spiny).unwrap();
        let (ax, ay) = (w.actors[i].x, w.actors[i].y);
        w.player.x = ax;
        w.player.y = ay + 12.0;
        w.player.vy = -2.0;
        w.player_interactions(false);
        assert!(!w.actors[i].alive());
        // a hit only knocks the shoe off
        assert!(w.player.hurt());
        assert_eq!(w.player.shoe, None);
        assert_eq!(w.player.form, Form::Small);
        assert!(w.player.alive());
    }

    #[test]
    fn collecting_every_pink_coin_gives_a_key() {
        let mut lvl = Level::flat(40);
        lvl.pink_coins.push(CoinTile { x: 6, y: 2 });
        lvl.pink_coins.push(CoinTile { x: 9, y: 2 });
        let mut w = World::with_level(lvl);
        assert_eq!(w.pink_total, 2);
        w.player.x = 6.0 * TILE + 8.0;
        run(&mut w, 0, 1);
        assert_eq!((w.pink.len(), w.keys), (1, 0));
        w.player.x = 9.0 * TILE + 8.0;
        run(&mut w, 0, 1);
        assert_eq!((w.pink.len(), w.keys), (2, 1));
        run(&mut w, 0, 1);
        assert_eq!(w.keys, 1, "counted once");
    }

    #[test]
    fn clown_car_is_flown_and_carries_riders() {
        let mut lvl = Level::flat(80);
        lvl.spawns
            .push(spawn(Kind::ClownCar { fire: false }, 120.0, 64.0));
        let mut car2 = spawn(Kind::ClownCar { fire: false }, 300.0, 80.0);
        car2.link = 4;
        lvl.spawns.push(car2);
        let mut rider = spawn(Kind::Goomba, 300.0, 104.0);
        rider.link = 4;
        rider.clown = true;
        lvl.spawns.push(rider);
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        // the Goomba rides its car, which chases the player
        let car = w.actors.iter().find(|a| a.spawn == Some(1)).unwrap();
        let g = w.actors.iter().find(|a| a.kind == Kind::Goomba).unwrap();
        assert_eq!((g.x, g.y), (car.x, car.y + 24.0));
        assert_eq!(car.phase, 1);
        // drop into the empty car
        let i = w.actors.iter().position(|a| a.spawn == Some(0)).unwrap();
        let (cx, cy) = (w.actors[i].x, w.actors[i].y);
        w.player.x = cx;
        w.player.y = cy + 14.0;
        w.player.vy = -1.0;
        w.player_interactions(false);
        assert!(w.player.clown.is_some());
        // fly right: 0.05/frame up to 2 px/f
        run(&mut w, input::RIGHT, 60);
        let car = w.actors.iter().find(|a| a.spawn == Some(0)).unwrap();
        assert!((car.vx - 2.0).abs() < 1e-3, "{}", car.vx);
        assert_eq!(w.player.x, car.x);
        // crouching in the car, Jump does nothing; standing, Jump climbs out (0x71009cd430)
        run(&mut w, input::DOWN | input::JUMP, 1);
        assert!(w.player.clown.is_some());
        run(&mut w, 0, 1);
        run(&mut w, input::JUMP, 1);
        assert!(w.player.clown.is_none());
        assert!(w.player.vy > 3.0, "{}", w.player.vy);
    }

    #[test]
    fn winged_blocks_bob_and_release_their_contents() {
        let mut lvl = Level::flat(40);
        let mut b = spawn(
            Kind::MovingBlock {
                block: BlockKind::Question,
                contents: Some(Contents::Coin),
            },
            200.0,
            80.0,
        );
        b.wings = true;
        lvl.spawns.push(b);
        let mut lift = spawn(
            Kind::Lift {
                width: 3,
                falling: false,
            },
            300.0,
            80.0,
        );
        lift.wings = true;
        lvl.spawns.push(lift);
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 33);
        // a quarter period (90° / 2.8125° = 32 frames): 16 px up
        let blk = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::MovingBlock { .. }))
            .unwrap();
        assert!((blk.y - 96.0).abs() < 0.5, "{}", blk.y);
        let l = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::Lift { .. }))
            .unwrap();
        assert!((l.y - 96.0).abs() < 0.5, "{}", l.y);
        // bump from below: one coin, then used
        let i = w
            .actors
            .iter()
            .position(|a| matches!(a.kind, Kind::MovingBlock { .. }))
            .unwrap();
        let id = w.actors[i].id;
        w.player.head_solid = Some(id);
        w.bump_moving_block();
        assert_eq!(
            w.actors[i].kind,
            Kind::MovingBlock {
                block: BlockKind::Empty,
                contents: None
            }
        );
        assert!(
            w.effects
                .iter()
                .any(|e| matches!(e, Effect::CoinPop { .. }))
        );
    }

    #[test]
    fn fish_bone_aims_charges_and_shatters() {
        let mut lvl = Level::flat(40);
        lvl.liquid = Some(crate::level::LiquidSpec {
            kind: 1,
            start: 400.0,
            end: 400.0,
            mode: 0,
            speed: 0,
            full: true,
        });
        // a wall the charge will hit
        for y in 2..12 {
            lvl.set(4, y, Cell::Solid);
        }
        lvl.spawns.push(spawn(Kind::FishBone, 200.0, 80.0));
        let mut w = World::with_level(lvl);
        w.player.x = 140.0;
        w.player.y = 80.0;
        run(&mut w, 0, 2);
        let a = &w.actors[0];
        assert!(
            a.dir < 0 && a.phase == 1,
            "spots the player it faces: {} {}",
            a.dir,
            a.phase
        );
        w.player.x = 300.0; // step aside; it charges along the aim regardless
        run(&mut w, 0, 41);
        assert_eq!(w.actors[0].phase, 2);
        run(&mut w, 0, 120);
        assert!(
            w.actors.iter().all(|a| a.kind != Kind::FishBone),
            "shattered on the wall"
        );
    }

    #[test]
    fn custom_autoscroll_follows_its_nodes() {
        let mut lvl = Level::flat(200);
        lvl.autoscroll = 4;
        // two nodes along the floor: from the start view to x = 1000 at the fast setting
        lvl.scroll_nodes.push((2, 1000.0, 120.0, 0));
        lvl.scroll_nodes.push((1, 200.0, 120.0, 2));
        let mut w = World::with_level(lvl);
        w.reset_camera();
        let x0 = w.camera_x;
        run(&mut w, input::RIGHT, 300);
        let moved = w.camera_x - x0;
        // eases toward 2 px/f (0.03 smoothing): well past 300 px in 300 frames
        assert!(moved > 300.0 && moved < 600.0, "{moved}");
        run(&mut w, input::RIGHT | input::RUN, 400);
        assert!(
            (w.camera_x + VIEW_W / 2.0 - 1000.0).abs() < 1.0,
            "stops at the last node"
        );
    }

    #[test]
    fn shells_are_carried_outside_smb1_and_kicked_on_release() {
        for style in [crate::player::Style::M3, crate::player::Style::M1] {
            let mut lvl = Level::flat(40);
            lvl.spawns
                .push(spawn(Kind::Koopa { red: false }, 120.0, 32.0));
            let mut w = World::with_level(lvl);
            w.style = style;
            w.player.x = 60.0;
            run(&mut w, 0, 1);
            assert!(w.actors[0].stomp(), "into its shell");
            let sx = w.actors[0].x;
            w.player.x = sx - 10.0;
            run(&mut w, input::RUN, 2);
            if style == crate::player::Style::M1 {
                assert!(w.player.held.is_none(), "SMB1 kicks");
                assert_eq!(w.actors[0].mode, Mode::ShellSlide);
                continue;
            }
            assert!(w.player.held.is_some());
            run(&mut w, input::RUN | input::RIGHT, 30);
            let a = &w.actors[0];
            assert!(
                a.held && (a.x - (w.player.x + 11.0)).abs() < 0.01,
                "in front of the player"
            );
            run(&mut w, 0, 1);
            assert!(w.player.held.is_none());
            assert_eq!(w.actors[0].mode, Mode::ShellSlide);
            assert!(w.actors[0].vx > 0.0, "kicked forward");
        }
    }

    #[test]
    fn spin_jumps_destroy_or_bounce_off_enemies() {
        let mut lvl = Level::flat(40);
        lvl.spawns.push(spawn(Kind::Goomba, 140.0, 32.0));
        lvl.spawns.push(spawn(Kind::Spiny, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::MW;
        w.player.x = 60.0;
        run(&mut w, input::SPIN, 1);
        assert!(w.player.spinning);
        assert!((w.player.vy - (3.228 - 0.0)).abs() < 0.4, "{}", w.player.vy);
        // fall onto the Goomba: it vanishes
        let (gx, gy) = (w.actors[0].x, w.actors[0].y);
        w.player.x = gx;
        w.player.y = gy + 12.0;
        w.player.vy = -2.0;
        w.player_interactions(false);
        assert_eq!(w.actors[0].mode, Mode::Dead);
        // onto the Spiny: a safe bounce
        let i = w.actors.iter().position(|a| a.kind == Kind::Spiny).unwrap();
        let (sx, sy) = (w.actors[i].x, w.actors[i].y);
        w.player.x = sx;
        w.player.y = sy + 12.0;
        w.player.vy = -2.0;
        let form = w.player.form;
        w.player_interactions(false);
        assert!(w.player.vy > 0.0 && w.player.alive() && w.player.form == form);
        assert!(w.actors[i].alive());
    }

    #[test]
    fn super_bell_walks_and_gives_the_cat_scratch() {
        let mut lvl = Level::flat(200);
        lvl.style = "3W".into();
        lvl.spawns.push(spawn(Kind::StyleItem, 120.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 400.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::W3;
        w.player.x = 40.0;
        let bell = w
            .actors
            .iter()
            .position(|a| a.kind == Kind::StyleItem)
            .unwrap();
        let x0 = w.actors[bell].x;
        run(&mut w, 0, 10);
        assert!(
            w.actors[bell].x == x0,
            "the bell waits where it lands (0x7100837860)"
        );
        run(&mut w, input::RIGHT | input::RUN, 90);
        run(&mut w, 0, 60);
        assert_eq!(w.player.form, Form::Cat, "picked up the bell");
        let gi = w
            .actors
            .iter()
            .position(|a| a.kind == Kind::Goomba)
            .unwrap();
        let (gx, gy) = (w.actors[gi].x, w.actors[gi].y);
        w.player.x = gx - 20.0;
        w.player.y = gy;
        w.player.facing = 1;
        run(&mut w, 0, 2);
        run(&mut w, input::RUN, 6);
        assert!(!w.actors[gi].alive(), "scratch hit");
        assert_eq!(w.player.at_goal().form, 6);
    }

    #[test]
    fn super_leaf_gives_tail_attack_and_p_meter_flight() {
        let mut lvl = Level::flat(200);
        lvl.style = "M3".into();
        lvl.spawns.push(spawn(Kind::StyleItem, 80.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::M3;
        w.player.x = 60.0;
        run(&mut w, input::RIGHT, 20);
        run(&mut w, 0, 60);
        assert_eq!(w.player.form, Form::Raccoon, "picked up the leaf");
        // tail swing knocks out the Goomba in front
        let gi = w
            .actors
            .iter()
            .position(|a| a.kind == Kind::Goomba)
            .unwrap();
        let (gx, gy) = (w.actors[gi].x, w.actors[gi].y);
        w.player.x = gx - 20.0;
        w.player.y = gy;
        w.player.facing = 1;
        run(&mut w, input::RUN, 6);
        assert!(!w.actors[gi].alive(), "tail hit");
        // run 70 frames: the P-meter fills, a jump opens the flight window, flaps hold vy 1.5
        run(&mut w, 0, 30);
        run(&mut w, input::RIGHT | input::RUN, 130);
        assert_eq!(w.player.p_meter, 630);
        run(&mut w, input::RIGHT | input::RUN | input::JUMP, 1);
        assert!(w.player.flight > 0);
        run(&mut w, input::RIGHT | input::RUN, 10);
        assert!(!w.player.on_ground);
        run(&mut w, input::RIGHT | input::RUN | input::JUMP, 2);
        assert_eq!(w.player.vy, 1.5, "flapping");
    }

    #[test]
    fn wall_jumps_in_nsmbu_and_3dw_and_ground_pounds_in_nsmbu() {
        let mut lvl = Level::flat(40);
        for y in 2..12 {
            lvl.set(12, y, Cell::Solid);
        }
        // SMB1 (and SMB3, SMW): no wall slide
        let mut w = World::with_level(lvl.clone());
        w.player.x = 11.0 * TILE + 4.0;
        w.player.y = 120.0;
        w.player.on_ground = false;
        run(&mut w, input::RIGHT, 20);
        assert_eq!(w.player.wall_slide, 0, "no wall slide in SMB1");
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::WU;
        w.player.x = 11.0 * TILE + 4.0;
        w.player.y = 120.0;
        w.player.on_ground = false;
        run(&mut w, input::RIGHT, 20);
        assert_eq!(w.player.wall_slide, 1, "sliding on the wall");
        assert!(w.player.vy >= -2.0);
        run(&mut w, input::RIGHT | input::JUMP, 1);
        assert_eq!(w.player.wall_slide, 0);
        assert!(
            w.player.vx < -2.0 && w.player.vy > 3.0,
            "wall jump {} {}",
            w.player.vx,
            w.player.vy
        );
        // NSMBU ground pound: wind-up, then -6
        let mut w = World::with_level(Level::flat(40));
        w.style = crate::player::Style::WU;
        w.player.x = 100.0;
        w.player.y = 120.0;
        w.player.on_ground = false;
        // the command needs Down held 5 frames
        run(&mut w, input::DOWN, 4);
        assert_eq!(w.player.pound, 0);
        run(&mut w, input::DOWN, 1);
        assert!(w.player.pound > 0);
        // hip_start (10 frames) + 5 [obs: Ryujinx trace]
        run(&mut w, input::DOWN, 13);
        assert_eq!(w.player.vy, 0.0, "still winding up");
        run(&mut w, input::DOWN, 2);
        assert_eq!(w.player.vy, -6.0);
    }

    #[test]
    fn propeller_flies_up_then_falls_slowly() {
        let mut w = World::with_level(Level::flat(40));
        w.style = crate::player::Style::WU;
        w.player.x = 100.0;
        w.player.form = Form::Propeller;
        run(&mut w, 0, 2);
        run(&mut w, input::SPIN, 1);
        run(&mut w, 0, 10);
        assert!((w.player.vy - 2.5).abs() < 1e-3, "rising: {}", w.player.vy);
        run(&mut w, 0, 60);
        assert!(w.player.prop_fall);
        assert!(w.player.vy >= -1.25 - 1e-3, "slow fall: {}", w.player.vy);
        run(&mut w, input::DOWN, 30);
        assert!(
            w.player.vy < -3.0 || w.player.on_ground,
            "drill: {}",
            w.player.vy
        );
    }

    #[test]
    fn blinking_blocks_swap_and_spike_blocks_bite_on_the_beat() {
        let mut lvl = Level::flat(40);
        lvl.blink_blocks.push((10, 5, false));
        lvl.blink_blocks.push((11, 5, true));
        lvl.set(10, 5, Cell::Solid);
        lvl.spike_blocks.push((20, 2, 0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 239);
        assert_eq!(w.level.cell(10, 5), Cell::Solid);
        run(&mut w, 0, 1);
        assert_eq!(w.level.cell(10, 5), Cell::Empty, "swapped at 240");
        assert_eq!(w.level.cell(11, 5), Cell::Solid);
        // stand next to the spike block: safe until frame 178 of the cycle (the spike cycle
        // runs 180 frames ahead of the area timer)
        w.player.x = 20.0 * TILE - 6.0;
        w.area_timer = 160;
        run(&mut w, 0, 10);
        assert_eq!(w.player.form, Form::Small);
        assert!(w.player.alive());
        w.area_timer = 237;
        run(&mut w, 0, 2);
        assert!(
            !w.player.alive() || w.events.contains(&Event::Death),
            "spikes out"
        );
    }

    #[test]
    fn cloned_worlds_stay_identical_under_the_same_inputs() {
        // the basis of rollback netcode: a snapshot replays exactly
        let mut a = World::with_level(Level::demo());
        for i in 0..120u32 {
            a.step(Input {
                buttons: input::RIGHT | if i % 40 < 15 { input::JUMP } else { 0 },
            });
        }
        let mut b = a.clone();
        for i in 0..600u32 {
            let inp = Input {
                buttons: input::RIGHT | input::RUN | if i % 30 < 12 { input::JUMP } else { 0 },
            };
            a.step(inp);
            b.step(inp);
        }
        assert_eq!(
            (a.player.x, a.player.y, a.tick),
            (b.player.x, b.player.y, b.tick)
        );
        assert_eq!(
            (a.score, a.coins, a.actors.len()),
            (b.score, b.coins, b.actors.len())
        );
        let pos = |w: &World| w.actors.iter().map(|x| (x.x, x.y)).collect::<Vec<_>>();
        assert_eq!(pos(&a), pos(&b));
    }

    #[test]
    fn local_multiplayer_players_move_collect_and_rejoin() {
        let mut lvl = Level::flat(60);
        for x in 14..16 {
            lvl.coins.push(CoinTile { x, y: 2 });
        }
        let mut w = World::with_level(lvl);
        w.set_players(2);
        assert_eq!(w.others.len(), 1);
        let x0 = w.others[0].x;
        // player 2 runs right, player 1 stands still
        for _ in 0..90 {
            w.step_players(&[
                Input::default(),
                Input {
                    buttons: input::RIGHT,
                },
            ]);
        }
        assert!(w.others[0].x > x0 + 50.0, "player 2 moved");
        assert!(w.player.x < 70.0, "player 1 stayed");
        // player 2 walks into the coins
        for _ in 0..120 {
            w.step_players(&[
                Input::default(),
                Input {
                    buttons: input::RIGHT,
                },
            ]);
        }
        assert!(w.coins >= 1, "coins count for everyone");
        // far out of the view: back beside player 1
        w.others[0].x = 5000.0;
        w.step_players(&[Input::default(), Input::default()]);
        assert!((w.others[0].x - w.player.x).abs() < 1.0);
        assert!(w.sprites().len() > 2);
    }

    #[test]
    fn yoshi_hatches_is_ridden_eats_and_runs_off_when_hit() {
        let mut lvl = Level::flat(60);
        lvl.spawns
            .push(spawn(Kind::YoshiEgg { red: false }, 100.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::MW;
        w.player.x = 40.0;
        run(&mut w, 0, 40);
        let yi = w
            .actors
            .iter()
            .position(|a| matches!(a.kind, Kind::Yoshi { .. }))
            .unwrap();
        // drop onto him
        let (yx, yy) = (w.actors[yi].x, w.actors[yi].y);
        w.player.x = yx;
        w.player.y = yy + 14.0;
        w.player.vy = -1.0;
        w.player.on_ground = false;
        w.player_interactions(false);
        assert!(w.player.yoshi.is_some());
        // he follows the rider
        run(&mut w, input::RIGHT, 20);
        let yi = w
            .actors
            .iter()
            .position(|a| matches!(a.kind, Kind::Yoshi { .. }))
            .unwrap();
        assert_eq!(w.actors[yi].x, w.player.x);
        // the tongue eats a Goomba in front
        let gx = w.player.x + 24.0;
        w.add_actor(Actor::new(Kind::Goomba, gx, w.player.y));
        run(&mut w, 0, 1);
        run(&mut w, input::RUN, 8);
        assert!(
            w.actors
                .iter()
                .all(|a| a.kind != Kind::Goomba || !a.alive()),
            "eaten"
        );
        // a hit knocks the rider off without a power-down; Yoshi runs away
        let form = w.player.form;
        assert!(w.player.hurt());
        run(&mut w, 0, 1);
        assert!(w.player.yoshi.is_none());
        assert_eq!(w.player.form, form);
        let yi = w
            .actors
            .iter()
            .position(|a| matches!(a.kind, Kind::Yoshi { .. }))
            .unwrap();
        assert_eq!(w.actors[yi].phase, 1);
        run(&mut w, 0, 30);
        assert!(w.actors[yi].vx.abs() > 1.5, "running away");
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
        // the counter starts at 370: the flame starts two frames in
        let mut w = World::with_level(lvl.clone());
        w.player.x = 300.0;
        assert_eq!(w.burner_flame(false), 0.0);
        run(&mut w, 0, 2);
        assert!(w.burner_flame(false) > 0.0);
        // off between 156 and 371
        let mut w2 = World::with_level(lvl);
        w2.player.x = 300.0;
        run(&mut w2, 0, 200);
        assert_eq!(w2.burner_flame(false), 0.0);
        assert!(w2.burner_flame(true) > 0.0);
        // only the full flame (the Fire state) has a collider: a player in the growing flame
        // (FireStart, counter 5) is safe, in the full one (counter 50) burnt
        let mut lvl2 = Level::flat(40);
        lvl2.burners.push(crate::level::Burner {
            x: 72.0,
            y: 32.0,
            dir: 3,
            alt: false,
        });
        let mut w4 = World::with_level(lvl2);
        w4.player.x = 72.0;
        w4.player.y = 56.0;
        w4.area_timer = 7;
        w4.events.clear();
        w4.fire_hazards();
        assert!(
            !w4.events
                .iter()
                .any(|e| matches!(e, Event::PowerDown | Event::Death))
        );
        w4.area_timer = 52;
        w4.fire_hazards();
        assert!(
            w4.events
                .iter()
                .any(|e| matches!(e, Event::PowerDown | Event::Death))
        );
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
            red: false,
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
    fn warp_box_by_touch_then_used_up() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Key, 240.0, 32.0));
        for (id, x, key) in [
            (4u8, 600.0f32, false),
            (5, 100.0, false),
            (6, 300.0, true),
            (7, 700.0, true),
        ] {
            lvl.warps.push(crate::level::Warp {
                kind: crate::level::WarpKind::Box { key },
                id,
                x,
                y: 32.0,
                len: 1,
            });
        }
        let mut w = World::with_level(lvl);
        // the key box ignores a player without a key
        w.player.x = 285.0;
        run(&mut w, input::RIGHT, 20);
        assert!(w.transit.is_none(), "locked");
        // touching is enough (no button); the key is spent
        w.player.x = 225.0;
        run(&mut w, input::RIGHT, 50);
        assert!(w.transit.is_some() && w.keys == 0, "a key opens it");
        assert!(w.player.hidden, "hidden at once");
        run(&mut w, 0, 200);
        assert!(w.transit.is_none());
        assert_eq!(w.player.x, 700.0);
        // used up: walking back through the exit box does nothing
        run(&mut w, input::LEFT, 20);
        assert!(w.transit.is_none(), "the pair vanished");
        // a plain pair
        w.player.x = 580.0;
        run(&mut w, input::RIGHT, 60);
        assert!(w.transit.is_some(), "entered by touch");
        run(&mut w, 0, 200);
        assert_eq!(w.player.x, 100.0);
    }

    #[test]
    fn koopa_troopa_car_drives_turns_and_smashes() {
        let mut lvl = Level::flat(120);
        lvl.style = "3W".into();
        lvl.spawns.push(spawn(Kind::KoopaCar, 100.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 400.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::W3;
        // drop in from above
        w.player.x = 100.0;
        w.player.y = 80.0;
        run(&mut w, 0, 30);
        assert!(w.player.car.is_some(), "got in");
        // a 60-frame start spin, then it runs on its own toward 4.0 at 0.05 a frame
        run(&mut w, 0, 40);
        let car = w.actors.iter().find(|a| a.kind == Kind::KoopaCar).unwrap();
        assert_eq!(car.vx, 0.0, "still spinning up");
        run(&mut w, 0, 70);
        let car = w.actors.iter().find(|a| a.kind == Kind::KoopaCar).unwrap();
        assert!(car.vx >= 2.0 && car.vx < 4.0, "running: {}", car.vx);
        assert_eq!(w.player.x, car.x, "the player rides it");
        // into the Goomba
        run(&mut w, input::RIGHT, 120);
        let g = w.actors.iter().find(|a| a.kind == Kind::Goomba);
        assert!(g.is_none_or(|g| !g.alive()), "ran the Goomba over");
        // turning around brakes it to a stop at 0.06 a frame, then it heads left
        run(&mut w, input::LEFT, 1);
        run(&mut w, 0, 150);
        let car = w.actors.iter().find(|a| a.kind == Kind::KoopaCar).unwrap();
        assert!(car.vx <= -2.0, "turned: {}", car.vx);
        assert!(w.player.at_goal().koopa_car);
        // Spin gets out
        run(&mut w, input::SPIN, 1);
        assert!(w.player.car.is_none());
    }

    #[test]
    fn exclamation_block_hit_from_below_grows_its_chain() {
        let mut lvl = Level::flat(60);
        lvl.exclamations.push(((3, 4), vec![2, 2, 13]));
        let mut w = World::with_level(lvl);
        w.player.x = 4.0 * TILE;
        run(&mut w, 0, 2);
        run(&mut w, input::JUMP, 30);
        assert!(w.exclamations[0].timer > 0, "hit from below");
        run(&mut w, 0, 40);
        assert_eq!(
            w.exclamations[0].boxes().len(),
            2,
            "a head hit sends one segment out"
        );
        // the segment is solid: stand on it
        w.player.x = 6.0 * TILE;
        w.player.y = 6.0 * TILE + 4.0;
        run(&mut w, 0, 10);
        assert!(w.player.on_ground && (w.player.y - 6.0 * TILE).abs() < 0.5);
    }

    #[test]
    fn tree_trunk_climbs_to_the_canopy_which_drops_its_item() {
        let mut lvl = Level::flat(60);
        let t = crate::level::Tree {
            x: 200.0,
            y: 40.0,
            h: 6,
            contents: Some(Contents::Mushroom),
        };
        lvl.vines.push((t.x, t.y - 8.0, t.canopy().1));
        lvl.trees.push(t);
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        run(&mut w, 0, 2);
        run(&mut w, input::UP, 120);
        assert!(
            w.player.big() || w.actors.iter().any(|a| a.kind == Kind::Mushroom),
            "the canopy dropped its item"
        );
        // the canopy is a platform
        w.player.climbing = None;
        w.player.x = 220.0;
        w.player.y = t.canopy().1 + 10.0;
        w.player.vy = 0.0;
        run(&mut w, 0, 30);
        assert!(w.player.on_ground && (w.player.y - t.canopy().1).abs() < 0.5);
    }

    #[test]
    fn mushroom_trampoline_bounces_high_or_with_a_jump() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(
            Kind::Trampoline {
                width: 4,
                moving: false,
                range: 3,
            },
            200.0,
            48.0,
        ));
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        w.player.y = 100.0;
        let mut top = 0.0f32;
        for _ in 0..40 {
            run(&mut w, 0, 1);
            top = top.max(w.player.vy);
        }
        // launched at 6.0 before the player's step applies one frame of gravity
        assert!((top - (6.0 - 0.34)).abs() < 0.01, "plain bounce 6.0: {top}");
        // with jump held on landing: 4.43
        let mut lvl2 = Level::flat(60);
        lvl2.spawns.push(spawn(
            Kind::Trampoline {
                width: 4,
                moving: false,
                range: 3,
            },
            200.0,
            48.0,
        ));
        let mut w2 = World::with_level(lvl2);
        w2.player.x = 200.0;
        w2.player.y = 100.0;
        let mut top2 = 0.0f32;
        for _ in 0..40 {
            run(&mut w2, input::JUMP, 1);
            top2 = top2.max(w2.player.vy);
        }
        assert!(top2 > 3.5 && top2 < 4.43, "jump-held launch 4.43: {top2}");
    }

    #[test]
    fn crate_is_carried_thrown_and_breaks_on_an_enemy() {
        let mut lvl = Level::flat(60);
        lvl.style = "3W".into();
        lvl.spawns.push(spawn(
            Kind::Crate {
                contents: Some(Contents::OneUp),
            },
            120.0,
            32.0,
        ));
        lvl.spawns.push(spawn(Kind::Goomba, 220.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::W3;
        w.player.x = 100.0;
        run(&mut w, input::RIGHT, 8);
        run(&mut w, input::RUN | input::RIGHT, 4);
        assert!(w.player.held.is_some(), "picked up");
        run(&mut w, 0, 1);
        let c = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::Crate { .. }))
            .unwrap();
        assert!(c.phase == 1 && c.vx > 2.0, "thrown");
        run(&mut w, 0, 60);
        assert!(
            w.actors
                .iter()
                .all(|a| a.kind != Kind::Goomba || !a.alive()),
            "the Goomba was hit"
        );
        assert!(
            w.actors.iter().any(|a| a.kind == Kind::OneUp),
            "contents out"
        );
    }

    #[test]
    fn track_block_follows_its_path_while_on() {
        let mut lvl = Level::flat(60);
        // right, then the end (entered moving right): two 2-tile steps
        lvl.track_paths.push((0, vec![2, 13]));
        lvl.spawns.push(spawn(
            Kind::TrackBlock {
                path: 0,
                alt: false,
            },
            100.0,
            64.0,
        ));
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 2);
        let x0 = w.actors[0].x;
        // stand on it
        w.player.x = 100.0;
        w.player.y = 100.0;
        run(&mut w, 0, 40);
        assert!(w.player.ride.is_some(), "riding");
        let xa = w.actors[0].x;
        assert!(xa > x0, "set off once ridden");
        run(&mut w, 0, 20);
        let x1 = w.actors[0].x;
        assert!((x1 - xa - 10.0).abs() < 0.01, "0.5 px/f: {}", x1 - xa);
        // switched OFF it holds still
        w.on_off = false;
        run(&mut w, 0, 30);
        assert_eq!(w.actors[0].x, x1);
        w.on_off = true;
        let mut far = 0.0f32;
        for _ in 0..200 {
            run(&mut w, 0, 1);
            far = far.max(w.actors[0].x);
        }
        assert_eq!(far, x0 + 64.0, "reached the end");
        assert!(w.actors[0].x < far, "and came back after the 60-frame stop");
    }

    #[test]
    fn super_hammer_makes_builder_mario_who_swings() {
        let mut lvl = Level::flat(60);
        lvl.style = "3W".into();
        lvl.spawns.push(spawn(Kind::SuperHammer, 120.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 400.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::W3;
        w.player.x = 40.0;
        run(&mut w, input::RIGHT | input::RUN, 90);
        run(&mut w, 0, 60);
        assert_eq!(w.player.form, Form::Builder);
        let gi = w
            .actors
            .iter()
            .position(|a| a.kind == Kind::Goomba)
            .unwrap();
        let (gx, gy) = (w.actors[gi].x, w.actors[gi].y);
        w.player.x = gx - 30.0;
        w.player.y = gy;
        w.player.facing = 1;
        // the hammer's box comes down on animation frame 11 (0x71009c6150)
        run(&mut w, input::RUN, 10);
        assert!(w.actors[gi].alive(), "not yet");
        run(&mut w, input::RUN, 4);
        assert!(!w.actors[gi].alive(), "hammer hit");
        assert_eq!(w.player.form, Form::Builder, "not hurt");
        assert_eq!(w.player.at_goal().form, 7);
    }

    #[test]
    fn fire_bar_rides_a_track() {
        let mut lvl = Level::flat(60);
        // two joined horizontal pieces centred on tiles (10, 4) and (12, 4)
        lvl.tracks = crate::track::Tracks::new([(9, 3, 0), (11, 3, 0)]);
        lvl.firebars.push(crate::level::FireBar {
            x: 168.0,
            y: 72.0,
            n: 4,
            start: 0.0,
            dir: 1.0,
        });
        let mut s = spawn(
            Kind::TrackMount {
                what: 0,
                index: 0,
                h: 1,
            },
            168.0,
            64.0,
        );
        s.rail = Some(0); // start field 0: right on a horizontal track
        lvl.spawns.push(s);
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 2);
        let x0 = w.level.firebars[0].x;
        run(&mut w, 0, 20);
        let x1 = w.level.firebars[0].x;
        assert!((x1 - x0 - 15.0).abs() < 1.0, "carried at 0.75: {}", x1 - x0);
        assert!(
            w.level
                .solids
                .iter()
                .any(|b| (b.x0 + 8.0 - x1).abs() < 0.01 && !b.top_only),
            "the pivot block moves with it"
        );
    }

    #[test]
    fn escort_toad_follows_the_player_and_comes_back_when_knocked_out() {
        let mut lvl = Level::flat(80);
        lvl.spawns.push(spawn(Kind::EscortToad, 40.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 60.0;
        run(&mut w, input::RIGHT, 120);
        let t = w
            .actors
            .iter()
            .find(|a| a.kind == Kind::EscortToad)
            .unwrap();
        let gap = w.player.x - t.x;
        assert!(gap > 16.0 && gap < 60.0, "trails the player: {gap}");
        // knocked out: no fail; 45 frames later he is back beside the player (0x710087a7c0)
        let i = w
            .actors
            .iter()
            .position(|a| a.kind == Kind::EscortToad)
            .unwrap();
        let x = w.actors[i].x;
        w.actors[i].die_fall(x);
        let toads = |w: &World| {
            w.actors
                .iter()
                .filter(|a| a.kind == Kind::EscortToad && a.alive())
                .count()
        };
        run(&mut w, 0, 44);
        assert!(w.player.alive());
        assert_eq!(toads(&w), 0);
        run(&mut w, 0, 1);
        assert_eq!(toads(&w), 1);
    }

    #[test]
    fn stone_is_heavy_and_opens_its_goal_condition() {
        let mut lvl = Level::flat(80);
        lvl.style = "M3".into();
        lvl.spawns.push(spawn(Kind::Stone, 120.0, 32.0));
        let mut w = World::with_level(lvl);
        w.style = crate::player::Style::M3;
        w.clear.goal = crate::clear::Goal::HoldingStone;
        w.player.x = 100.0;
        run(&mut w, input::RIGHT, 8);
        run(&mut w, input::RUN | input::RIGHT, 4);
        assert!(w.player.held.is_some(), "picked up");
        run(&mut w, 0, 1);
        let s = w.actors.iter().find(|a| a.kind == Kind::Stone).unwrap();
        assert!(s.vx.abs() <= 0.8 + 1e-4, "heavy throw: {}", s.vx);
        run(&mut w, 0, 30);
        let s = w.actors.iter().find(|a| a.kind == Kind::Stone).unwrap();
        assert_eq!(s.vx, 0.0, "slid to a stop");
        assert!(s.alive(), "never breaks");
        assert!(w.clear.met(crate::clear::AtGoal {
            holding_stone: true,
            ..w.player.at_goal()
        }));
    }

    #[test]
    fn empty_shells_become_helmets_or_a_ride() {
        // a Buzzy shell on a block above: jump into it from below
        let mut lvl = Level::flat(60);
        lvl.set(6, 5, Cell::Solid);
        lvl.spawns.push(spawn(
            Kind::EmptyShell { what: 1 },
            6.0 * TILE + 8.0,
            6.0 * TILE,
        ));
        lvl.spawns
            .push(spawn(Kind::EmptyShell { what: 3 }, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        run(&mut w, 0, 2);
        let s = w.actors.iter().find(|a| a.shell_of == 1).unwrap();
        assert!(matches!(s.kind, Kind::Shell { .. }) && s.mode == Mode::ShellWait);
        // it never wakes up
        run(&mut w, 0, 600);
        assert!(
            w.actors
                .iter()
                .any(|a| a.shell_of == 1 && a.mode == Mode::ShellWait)
        );
        // the shell knocked off the block falls on the player's head
        let i = w.actors.iter().position(|a| a.shell_of == 1).unwrap();
        w.actors[i].y = 3.0 * TILE;
        w.actors[i].vy = -2.0;
        w.player.x = w.actors[i].x;
        run(&mut w, 0, 20);
        assert_eq!(w.player.helmet, Some(1), "worn as a helmet");
        // a hit costs the helmet, not the form
        w.player.form = Form::Super;
        assert!(w.player.hurt());
        assert_eq!(w.player.helmet, None);
        assert_eq!(w.player.form, Form::Super);
        // the Dry Bones Shell: land on it to ride
        w.player.invuln = 0;
        w.player.x = 300.0;
        w.player.y = 80.0;
        run(&mut w, 0, 30);
        assert!(
            w.player.shoe.is_some() && w.player.shoe_karon,
            "riding the shell"
        );
        assert!(w.player.at_goal().karon_shell);
    }

    #[test]
    fn flying_hidden_block_appears_when_bumped() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(
            Kind::MovingBlock {
                block: BlockKind::Hidden,
                contents: Some(Contents::Mushroom),
            },
            5.0 * TILE + 8.0,
            4.0 * TILE,
        ));
        let mut w = World::with_level(lvl);
        w.player.x = 5.0 * TILE + 8.0;
        run(&mut w, 0, 2);
        assert!(w.level.solids.is_empty(), "not solid while hidden");
        run(&mut w, input::JUMP, 30);
        let b = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::MovingBlock { .. }))
            .unwrap();
        assert!(matches!(
            b.kind,
            Kind::MovingBlock {
                block: BlockKind::Empty,
                ..
            }
        ));
        run(&mut w, 0, 2);
        assert!(!w.level.solids.is_empty(), "solid once found");
        assert!(w.actors.iter().any(|a| a.kind == Kind::Mushroom) || w.player.big());
    }

    #[test]
    fn enemies_spawn_47_px_before_the_view_and_rearm_past_48() {
        let mut lvl = Level::flat(200);
        // spawn boxes 8 px each side of x: one reaches the zone (right edge 384 + 47), one not
        lvl.spawns
            .push(spawn(Kind::Goomba, 384.0 + 47.0 + 8.0, 32.0));
        lvl.spawns
            .push(spawn(Kind::Goomba, 384.0 + 47.0 + 9.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        w.step(Input::default());
        assert!(w.camera_x.abs() < 1e-3);
        let near = w.actors.iter().filter(|a| a.kind == Kind::Goomba).count();
        assert_eq!(near, 1);
        assert!(w.spawns[0].spawned && !w.spawns[1].spawned);
    }

    #[test]
    fn fire_bars_turn_once_per_341_frames_on_the_area_timer() {
        let mut w = World::with_level(Level::flat(40));
        let fb = crate::level::FireBar {
            x: 100.0,
            y: 64.0,
            n: 4,
            start: 90.0,
            dir: -1.0,
        };
        assert_eq!(w.firebar_angle(&fb), 90.0);
        w.area_timer = 1;
        assert!((w.firebar_angle(&fb) - (90.0 - 1.0546875)).abs() < 1e-4);
        // three turns take exactly 1024 frames
        w.area_timer = 1024;
        assert!((w.firebar_angle(&fb) - 90.0).abs() < 1e-3);
    }

    #[test]
    fn side_spring_pushes_at_3_and_bounces_off_a_wall() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::SideSpring, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 170.0;
        let mut pushed = None;
        for _ in 0..80 {
            w.step(Input {
                buttons: input::RIGHT,
            });
            if w.player.side_push == SIDE_PUSH_CHAIN {
                pushed = Some(w.player.vx);
                break;
            }
        }
        assert_eq!(pushed, Some(-3.0));
        assert!(w.player.on_ground, "a plain push keeps the player's state");

        // pinned between the spring and a wall: bounce back at 2.1 with vy 2.5
        let mut lvl = Level::flat(60);
        for y in 2..6 {
            lvl.set(10, y, Cell::Solid); // wall face at x = 176
        }
        lvl.spawns.push(spawn(Kind::SideSpring, 192.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 184.0;
        w.player.facing = 1;
        w.step(Input::default());
        assert_eq!(w.player.side_push, SIDE_PUSH_CHAIN);
        assert!((w.player.vx - 2.1).abs() < 1e-5, "{}", w.player.vx);
        assert_eq!(w.player.vy, 2.5);
        assert_eq!(w.player.facing, 1);
    }

    #[test]
    fn alternate_forms_behave() {
        // Goombrat turns at a ledge
        let mut lvl = Level::flat(60);
        for x in 12..60 {
            lvl.set(x, 0, Cell::Empty);
            lvl.set(x, 1, Cell::Empty);
        }
        lvl.spawns.push(spawn(Kind::Goombrat, 150.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 20.0;
        run(&mut w, 0, 200);
        let g = w.actors.iter().find(|a| a.kind == Kind::Goombrat).unwrap();
        assert!(g.y >= 31.0, "stayed on the ledge: {}", g.y);

        // a sideways spring flings the player back
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::SideSpring, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 160.0;
        run(&mut w, input::RIGHT, 60);
        assert!(
            w.player.x < 190.0 && w.player.side_push >= 0,
            "pushed back: {}",
            w.player.x
        );

        // a Boo circle spreads to 52 px and turns
        let mut lvl = Level::flat(60);
        let mut b = spawn(Kind::Boo, 300.0, 64.0);
        b.alt = true;
        lvl.spawns.push(b);
        let mut w = World::with_level(lvl);
        w.player.x = 200.0;
        run(&mut w, 0, 120);
        let ring: Vec<_> = w
            .actors
            .iter()
            .filter(|a| matches!(a.kind, Kind::RingBoo { .. }))
            .collect();
        assert_eq!(ring.len(), 8);
        let r = (ring[0].x - 300.0).dhypot(ring[0].y + 8.0 - 72.0);
        assert!((r - 52.0).abs() < 1.0, "radius {r}");

        // a homing Bullet Bill turns toward the player
        let mut w = World::with_level(Level::flat(60));
        let mut bill = Actor::new(Kind::BulletBill, 400.0, 32.0);
        bill.dir = -1;
        bill.phase = 1;
        w.add_actor(bill);
        w.player.x = 200.0;
        w.player.y = 150.0;
        run(&mut w, 0, 60);
        let bill = w
            .actors
            .iter()
            .find(|a| a.kind == Kind::BulletBill)
            .unwrap();
        assert!(bill.y > 33.0, "climbed toward the player: {}", bill.y);

        // a Superball bounces off the floor and keeps going
        let mut w = World::with_level(Level::flat(60));
        let mut ball = Actor::new(Kind::Superball, 100.0, 40.0);
        ball.vx = 3.0;
        ball.vy = -3.0;
        ball.timer = 270;
        w.add_actor(ball);
        w.player.x = 30.0;
        run(&mut w, 0, 10);
        let ball = w.actors.iter().find(|a| a.kind == Kind::Superball).unwrap();
        assert!(
            ball.vy > 0.0 && ball.vx == 3.0,
            "reflected: {} {}",
            ball.vx,
            ball.vy
        );
    }

    #[test]
    fn big_mushroom_makes_big_mario() {
        let mut lvl = Level::flat(80);
        lvl.spawns.push(spawn(Kind::StyleItem, 120.0, 32.0));
        lvl.spawns.push(spawn(Kind::Goomba, 500.0, 32.0));
        lvl.set(30, 2, Cell::Solid);
        lvl.blocks.push(BlockPlacement {
            x: 30,
            y: 2,
            kind: BlockKind::Brick,
            contents: None,
        });
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, input::RIGHT | input::RUN, 90);
        run(&mut w, 0, 60);
        assert_eq!(w.player.form, Form::Mega, "picked up the Big Mushroom");
        assert_eq!(w.player.hurt_box().2 - w.player.hurt_box().0, 24.0);
        // walks through a brick and over a Goomba
        run(&mut w, input::RIGHT, 360);
        assert!(w.blocks.get(30, 2).is_none(), "the brick broke");
        assert!(
            w.actors
                .iter()
                .all(|a| a.kind != Kind::Goomba || !a.alive()),
            "the Goomba was knocked out: {} {:?}",
            w.player.x,
            w.actors
                .iter()
                .find(|a| a.kind == Kind::Goomba)
                .map(|a| (a.x, a.mode))
        );
        assert_eq!(w.player.form, Form::Mega);
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
    fn autoscroll_pushes_the_player() {
        let mut lvl = Level::flat(120);
        lvl.autoscroll = 2;
        let mut w = World::with_level(lvl);
        let c0 = w.camera_x;
        run(&mut w, 0, 300);
        assert!(
            (w.camera_x - c0 - 300.0).abs() < 2.0,
            "1 px/frame: {}",
            w.camera_x - c0
        );
        assert!(w.player.x >= w.camera_x + 8.0 - 0.01, "pushed along");
        assert!(w.player.alive());
    }

    #[test]
    fn run_counters_survive_a_restart_and_lives_are_banked() {
        let mut w = World::new();
        w.set_run_counters(1200, 42, 100);
        w.add_coin();
        assert_eq!((w.score, w.coins), (1200 + COIN_SCORE, 43));
        w.reset_course();
        assert_eq!((w.score, w.coins, w.lives), (1200, 42, 100));
        for _ in 0..60 {
            w.add_coin();
        }
        assert_eq!(w.coins, 2);
        assert_eq!(w.lives, 100, "rollover banked, not added");
        assert_eq!(w.lives_gained, 1);
    }

    #[test]
    fn sky_night_low_gravity_floats_longer() {
        let airtime = |night: bool| {
            let mut lvl = Level::flat(40);
            lvl.theme = 8;
            lvl.night = night;
            let mut w = World::with_level(lvl);
            run(&mut w, 0, 2);
            run(&mut w, input::JUMP, 1);
            let mut n = 1;
            while !w.player.on_ground && n < 1000 {
                w.step(Input {
                    buttons: input::JUMP,
                });
                n += 1;
            }
            n
        };
        assert!(airtime(true) > airtime(false) * 2);
    }

    #[test]
    fn desert_night_wind_pushes_left() {
        let mut lvl = Level::flat(80);
        lvl.theme = 7;
        lvl.night = true;
        let mut w = World::with_level(lvl);
        w.player.x = 600.0;
        // 6 s calm, then 4 s of wind to the left
        run(&mut w, 0, 300);
        assert!((w.player.x - 600.0).abs() < 1.0);
        run(&mut w, 0, 180);
        assert!(w.player.x < 600.0 - 50.0, "x {}", w.player.x);
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
                    rail: None,
                    link: -1,
                    clown: false,
                    alt: false,
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
