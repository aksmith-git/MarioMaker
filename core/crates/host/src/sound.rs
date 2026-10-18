//! The data-driven sound director: the game's own sound database (`mmk_audio::links`) decides
//! what each event sounds like for the course's game style, theme and the emitter's state; this
//! module decides when (sim events and observed state changes), where (position → pan,
//! distance attenuation, underwater filter) and keeps the course music state.
//!
//! Music rules (research/specs/audio.md §6):
//! - the current area's theme track, day or night (`BGM_<Theme>_<Day|Night>`), from the top on
//!   course start, restart and every area change;
//! - hurry-up (100 s left): the hurry jingle, then the theme's hurry track from the top
//!   [obs 0x7100b6a3c0, 0x7100b6b7f0];
//! - Super Star and P switch: their own tracks (hurry variants after hurry-up); when they end
//!   the theme track resumes where it was if the area, theme and hurry state are unchanged,
//!   aligned to 1152-sample blocks, else from the top [obs 0x7100b6b7f0];
//! - star ending: the warning sign 120 frames before the star runs out [obs 0x7100a078a0];
//! - death / time up: the death fanfare; goal: the music stops, the pole sound, then the clear
//!   fanfare when the player walks off;
//! - riding Yoshi: the second channel pair of four-channel tracks (SMW / NSMBU drums) fades in
//!   [inf].

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;

use mmk_audio::links::{Layered, Play as Asset, PropList, PropValue, Rng, SoundDb, Turns};
use mmk_audio::mixer::{GroupDef, MusicClock, NO_GROUP, SET_BASE, SharedSet, SoundSet};
use mmk_audio::{Bus, Command, Play, Sound};
use mmk_sim::World;
use mmk_sim::actors::{Kind, Mode};
use mmk_sim::player::Form;
use mmk_sim::world::{Event, GoalPhase};

use crate::costume_sound::{CostumeReader, CostumeSounds};
use crate::sound_map::{self as map, THEME_VALUES, bgm_key};

/// Reads a file of the cache's `audio/` folder by its relative path (`se/x.bwav`).
pub type FileReader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// The sound database and where its files are.
pub struct Library {
    pub db: SoundDb,
    pub read: FileReader,
}

impl Library {
    /// Load `sound.json` through `read` (None if the cache has no database).
    pub fn load(read: FileReader) -> Option<Self> {
        let bytes = read("sound.json")?;
        let db = SoundDb::parse(&bytes).ok()?;
        Some(Self { db, read })
    }
}

/// The view's size in pixels.
const VIEW_W: f32 = 384.0;
const VIEW_H: f32 = 216.0;
/// Pan of a sound at the screen's edge [inf: the game's mixer is set to stereo; its pan curve
/// is not known].
pub const PAN_DEPTH: f32 = 0.5;
/// Low-pass amount of effects heard under water, approached by 0.04 per frame
/// [obs 0x7100f20010: 0.4, 0.04].
pub const WATER_LPF: f32 = 0.4;
/// Star frames left when the ending sign plays [obs 0x7100a078a0: 120].
pub const STAR_END_SIGN: i32 = 120;
/// Frames of the Yoshi layer fade [inf].
const LAYER_FADE_FRAMES: f32 = 30.0;
/// Music resume positions are aligned to this many samples [obs 0x7100b6b7f0: 0x480].
const RESUME_ALIGN: u32 = 0x480;
/// Handles for loops the director stops itself.
const H_TIMER: u32 = 1;
const H_AMBIENT: u32 = 2;
const H_FIRST_FREE: u32 = 16;

fn theme_value(theme: u8) -> &'static str {
    THEME_VALUES[(theme as usize).min(9)]
}

/// A sound's source: where it is and whether it is under water.
#[derive(Clone, Copy, Debug)]
struct Emitter {
    x: f32,
    y: f32,
    water: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    kind: Kind,
    mode: Mode,
    x: f32,
    y: f32,
    vx: f32,
    on_ground: bool,
    wings: bool,
    big: bool,
    phase: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Over {
    None,
    Star,
    Switch,
}

pub struct Sounds {
    tx: SyncSender<Command>,
    lib: Arc<Library>,
    shared: SharedSet,
    clock: MusicClock,
    rate: u32,
    style: String,
    /// The course set and its sound ids by wave name; the previous set is kept alive so the
    /// audio thread never frees one.
    set: Arc<SoundSet>,
    old_set: Option<Arc<SoundSet>>,
    ids: HashMap<String, u16>,
    rng: Rng,
    turns: Turns,
    next_handle: u32,
    // course and music state
    area: usize,
    theme: u8,
    night: bool,
    hurry: bool,
    over: Over,
    star_on: bool,
    p_on: bool,
    /// The theme track playing (or interrupted): its key, and where it was when interrupted.
    track: Option<String>,
    saved: Option<(String, u32)>,
    fanfare: bool,
    walking: bool,
    counting: bool,
    count_gap: u32,
    was_alive: bool,
    layer: bool,
    coins: u32,
    on_off: bool,
    actors: BTreeMap<u32, Snap>,
    water_lpf: f32,
    prev_player: PlayerSnap,
    footstep: i32,
    /// Blocks bumping last tick (tile → bump frames).
    bumps: BTreeMap<(i32, i32), u32>,
    note_hits: usize,
    /// The worn SMM1 costume's sounds (Phase 11).
    costume: CostumeSounds,
    /// What the last tick played (for tests and diagnostics): user, key.
    pub log: Vec<(String, String)>,
}

impl Sounds {
    pub fn new(
        tx: SyncSender<Command>,
        lib: Arc<Library>,
        shared: SharedSet,
        clock: MusicClock,
        rate: u32,
    ) -> Self {
        Self {
            tx,
            lib,
            shared,
            clock,
            rate,
            style: String::new(),
            set: Arc::new(SoundSet::default()),
            old_set: None,
            ids: HashMap::new(),
            rng: Rng(0x5eed_1234),
            turns: Turns::new(),
            next_handle: H_FIRST_FREE,
            area: 0,
            theme: 0,
            night: false,
            hurry: false,
            over: Over::None,
            star_on: false,
            p_on: false,
            track: None,
            saved: None,
            fanfare: false,
            walking: false,
            counting: false,
            count_gap: 0,
            was_alive: true,
            layer: false,
            coins: 0,
            on_off: true,
            actors: BTreeMap::new(),
            water_lpf: 0.0,
            prev_player: PlayerSnap::default(),
            footstep: 0,
            bumps: BTreeMap::new(),
            note_hits: 0,
            costume: CostumeSounds::default(),
            log: Vec::new(),
        }
    }

    fn send(&self, c: Command) {
        let _ = self.tx.try_send(c);
    }

    fn user(&self, base: &str) -> String {
        format!("{}_{base}", self.style)
    }

    /// The first of `bases` the style has (as a full user name).
    fn pick_user(&self, bases: &[&str]) -> Option<String> {
        bases
            .iter()
            .map(|b| self.user(b))
            .find(|u| self.lib.db.user(u).is_some())
    }

    /// Course-wide properties.
    fn course_props(&self) -> PropList {
        PropList::default()
            .with("@style", self.style.as_str())
            .with("@theme", theme_value(self.theme))
            .with("@hour", if self.night { "cNight" } else { "cDay" })
            .with("@scene", "Course")
    }

    // ---- the course sound set -------------------------------------------------------------

    fn frames(&self, frames60: f32) -> u32 {
        (frames60.max(0.0) * self.rate as f32 / 60.0) as u32
    }

    /// Waves the engine can play in a course of this style (preloaded so the first play
    /// doesn't wait for a file).
    fn style_waves(&self, world: &World) -> Vec<String> {
        let db = &self.lib.db;
        let mut w = Vec::new();
        let player = self.user("Player");
        for k in map::PLAYER_KEYS {
            db.waves_of(&player, k, &mut w);
        }
        let obj = self.user("ObjectCommon");
        for k in map::OBJECT_KEYS {
            db.waves_of(&obj, k, &mut w);
        }
        for k in map::COMMON_KEYS {
            db.waves_of("CourseSceneBGM_Common", k, &mut w);
        }
        for k in ["jump", "high_jump", "sideHit"] {
            db.waves_of(&self.user("OnpuBlock"), k, &mut w);
        }
        for u in ["GoalPole", "CastleGoal"] {
            db.waves_of(&self.user(u), "TimerCount", &mut w);
            db.waves_of(&self.user(u), "TimerCountLast", &mut w);
        }
        // the course's objects (as loaded; generated ones come lazily)
        let mut seen = Vec::new();
        for a in &world.actors {
            let Some(u) = self.pick_user(map::actor_users(a.kind)) else {
                continue;
            };
            if seen.contains(&u) {
                continue;
            }
            for k in map::ACTOR_KEYS {
                db.waves_of(&u, k, &mut w);
            }
            seen.push(u);
        }
        w
    }

    /// A fresh set for a new style: groups from the database, the style's waves.
    fn build_set(&mut self, world: &World) {
        self.costume.forget();
        let db = &self.lib.db;
        let groups: Vec<GroupDef> = db
            .groups
            .iter()
            .map(|g| {
                // this group, then each folder above it
                let mut chain = Vec::new();
                let mut id = db.group_id(&g.name);
                while let Some(i) = id {
                    if chain.len() > 32 {
                        break;
                    }
                    chain.push(i);
                    id = db.groups.get(i as usize).and_then(|p| p.parent);
                }
                GroupDef {
                    chain,
                    volume: g.volume,
                    limit: g.limit,
                    limit_type: g.limit_type,
                    duck: g.duck.clone(),
                }
            })
            .collect();
        self.ids.clear();
        let mut set = SoundSet {
            sounds: Vec::new(),
            groups,
        };
        for w in self.style_waves(world) {
            if let Some(s) = self.load_wave(&w) {
                self.ids.insert(w, SET_BASE + set.sounds.len() as u16);
                set.sounds.push(s);
            }
        }
        self.publish(set);
    }

    fn load_wave(&self, wave: &str) -> Option<Sound> {
        let rel = self.lib.db.waves.get(wave)?;
        let bytes = (self.lib.read)(rel)?;
        if rel.starts_with("music/") {
            Sound::from_bwav_streamed(wave, bytes.into()).ok()
        } else {
            Sound::from_bwav(wave, &bytes).ok()
        }
    }

    fn publish(&mut self, set: SoundSet) {
        let set = Arc::new(set);
        if let Ok(mut g) = self.shared.lock() {
            *g = Some(set.clone());
        }
        self.old_set = Some(std::mem::replace(&mut self.set, set));
        self.send(Command::LoadSet);
    }

    /// Where the SMM1 costume sounds are read from (None: no costume sounds).
    pub fn set_costumes(&mut self, read: Option<CostumeReader>) {
        self.costume = CostumeSounds::new(read);
    }

    /// Add a sound to the set (costume sounds); its id.
    fn add_sound(&mut self, s: Sound) -> Option<u16> {
        if self.set.sounds.len() >= (u16::MAX - SET_BASE) as usize {
            return None;
        }
        let mut next = (*self.set).clone();
        let id = SET_BASE + next.sounds.len() as u16;
        next.sounds.push(s);
        self.publish(next);
        Some(id)
    }

    /// Play a sound by id at the player (costume sounds).
    fn play_id(&mut self, world: &World, sound: u16, music: bool) {
        let e = Self::player_emitter(world);
        let (pan, _) = self.pan_gain(world, e, None).unwrap_or((0.0, 1.0));
        self.send(Command::Play(if music {
            Play::music(sound)
        } else {
            Play {
                pan,
                ..Play::sfx(sound)
            }
        }));
    }

    /// The sound id of `wave`, loading it into the set if needed.
    fn wave_id(&mut self, wave: &str) -> Option<u16> {
        if let Some(&id) = self.ids.get(wave) {
            return Some(id);
        }
        let s = self.load_wave(wave)?;
        if self.set.sounds.len() >= (u16::MAX - SET_BASE) as usize {
            return None;
        }
        let mut next = (*self.set).clone();
        let id = SET_BASE + next.sounds.len() as u16;
        next.sounds.push(s);
        self.ids.insert(wave.to_string(), id);
        self.publish(next);
        Some(id)
    }

    // ---- playing -----------------------------------------------------------------------

    /// Resolve `user`'s `key` with `local` properties; false if the user or key is missing.
    fn resolve(&mut self, user: &str, key: &str, local: &PropList) -> Option<Vec<Asset>> {
        let course = self.course_props();
        let props = Layered(local, &course);
        let mut out = Vec::new();
        if !self
            .lib
            .db
            .resolve(user, key, &props, &mut self.rng, &mut self.turns, &mut out)
        {
            self.log.push((user.to_string(), format!("?{key}")));
            return None;
        }
        self.log.push((user.to_string(), key.to_string()));
        Some(out)
    }

    fn pan_gain(&self, world: &World, e: Emitter, distance: Option<&str>) -> Option<(f32, f32)> {
        let cx = world.camera_x + VIEW_W / 2.0;
        let pan = ((e.x - cx) / (VIEW_W / 2.0)).clamp(-1.0, 1.0) * PAN_DEPTH;
        let mut gain = 1.0;
        if let Some(a) = distance.and_then(|d| self.lib.db.attenuation.get(d)) {
            let (x0, y0) = (world.camera_x, world.camera_y);
            let dx = (x0 - e.x).max(e.x - (x0 + VIEW_W)).max(0.0);
            let dy = (y0 - e.y).max(e.y - (y0 + VIEW_H)).max(0.0);
            let d = (dx * dx + dy * dy).sqrt();
            if a.cull.is_some_and(|c| d > c) {
                return None;
            }
            gain = a.gain(d);
        }
        Some((pan, gain))
    }

    /// Play sound effects resolved from `user`/`key`; returns whether the key exists.
    fn sfx_at(
        &mut self,
        world: &World,
        user: &str,
        key: &str,
        local: &PropList,
        e: Emitter,
        handle: u32,
    ) -> bool {
        let Some(plays) = self.resolve(user, key, local) else {
            return false;
        };
        for a in plays {
            let Some(sound) = self.wave_id(&a.wave) else {
                continue;
            };
            let Some((pan, gain)) = self.pan_gain(world, e, a.distance.as_deref()) else {
                continue;
            };
            let water = if e.water && !a.dry { WATER_LPF } else { 0.0 };
            self.send(Command::Play(Play {
                sound,
                bus: Bus::Sfx,
                volume: a.volume * gain,
                pitch: a.pitch,
                pan,
                delay: self.frames(a.delay),
                duration: self.frames(a.duration),
                fade_in: (a.fade_in.max(0.0) * self.rate as f32) as u32,
                stop_fade: self.frames(a.stop),
                group: a.group.unwrap_or(NO_GROUP),
                priority: a.priority,
                lpf: a.lpf.max(water),
                send: 1.0,
                handle,
                start: 0,
                then: None,
            }));
        }
        true
    }

    fn player_emitter(world: &World) -> Emitter {
        Emitter {
            x: world.player.x,
            y: world.player.y,
            water: world.player.in_water,
        }
    }

    fn player_props(&self, world: &World) -> PropList {
        let p = &world.player;
        PropList::default()
            .with("PlayerMode", map::player_mode(p.form))
            .with("RideYoshi", if p.yoshi.is_some() { 1 } else { 0 })
            .with("BodyWaterType", if p.in_water { "Water" } else { "None" })
            .with("BgWaterType", liquid_type(world))
            .with("@ground", self.ground(world).as_str())
            .with(
                "ShoeKind",
                if p.shoe_karon {
                    "cKaronShell"
                } else {
                    "cNormal"
                },
            )
    }

    /// The ground material under the player: the theme's terrain (ice when on ice) [inf: the
    /// game looks up the part the player stands on].
    fn ground(&self, world: &World) -> String {
        let part = if world.player.on_ice {
            "IceBlock"
        } else {
            "Ground"
        };
        self.lib
            .db
            .ground
            .get(&self.style)
            .and_then(|parts| parts.get(part).or_else(|| parts.get("Ground")))
            .and_then(|t| t.get(theme_value(self.theme)))
            .cloned()
            .unwrap_or_else(|| "cNormal".into())
    }

    /// A sound of the Yoshi the player rides (or just left), at the player.
    fn yoshi_sfx(&mut self, world: &World, key: &str) {
        let p = &world.player;
        let red = world
            .actors
            .iter()
            .find(|a| Some(a.id) == p.yoshi)
            .is_some_and(|a| matches!(a.kind, Kind::Yoshi { red: true }));
        let (x, y) = (p.x, p.y);
        self.actor_sfx(world, Kind::Yoshi { red }, key, &PropList::default(), x, y);
    }

    fn has_player_key(&self, key: &str) -> bool {
        self.lib.db.has_key(&self.user("Player"), key)
    }

    /// Sounds from the player's state changes: landing, entering and leaving water, ground
    /// pound, vines, shoes, Yoshi, skids, the propeller, footsteps (the 3D styles play these
    /// from the player's states and animation frames; the step points here are [inf]).
    fn observe_player(&mut self, world: &World) {
        let p = &world.player;
        let now = PlayerSnap::of(world);
        let prev = self.prev_player;
        self.prev_player = now;
        if !p.alive() || world.goal_phase != GoalPhase::None {
            return;
        }
        if now.in_water != prev.in_water && self.theme != 4 {
            let (key, alt) = if now.in_water {
                ("r_splash_in", "water_in")
            } else {
                ("r_splash_out", "water_out")
            };
            if !self.player_sfx(world, key, &[]) {
                self.player_sfx(world, alt, &[]);
            }
        }
        if now.on_ground && !prev.on_ground && !now.in_water && !p.pound_landed {
            self.player_sfx(world, "landing", &[]);
        }
        if now.pound_start && !prev.pound_start {
            self.player_sfx(world, "hipdrop_start", &[]);
        }
        if p.pound_landed {
            self.player_sfx(world, "hipdrop", &[]);
        }
        if now.climbing && !prev.climbing && !self.player_sfx(world, "catchIvy", &[]) {
            self.player_sfx(world, "climb", &[]);
        }
        if now.shoe && !prev.shoe {
            self.player_sfx(world, "kutsuIn", &[]);
        }
        if now.yoshi && !prev.yoshi {
            self.yoshi_sfx(world, "marioRideOn");
        }
        if !now.yoshi && prev.yoshi && !self.player_sfx(world, "marioRideOff", &[]) {
            self.yoshi_sfx(world, "marioRideOff");
        }
        if now.tongue && !prev.tongue {
            self.yoshi_sfx(world, "tongue");
        }
        if now.skid && !prev.skid {
            self.player_sfx(world, "turn_slip", &[]);
        }
        if now.propeller && !prev.propeller {
            self.player_sfx(world, "propellerjump", &[]);
        }
        // footsteps every half walk cycle while walking on the ground
        if now.on_ground && now.walking {
            let step = (p.walk_phase / FOOTSTEP_PHASE) as i32;
            if step != self.footstep {
                self.footstep = step;
                let key = if step % 2 == 0 {
                    "footstep_L"
                } else {
                    "footstep_R"
                };
                self.player_sfx(world, key, &[]);
            }
        }
    }

    fn player_sfx(&mut self, world: &World, key: &str, extra: &[(&str, PropValue)]) -> bool {
        let mut props = self.player_props(world);
        for (k, v) in extra {
            props.set(k, v.clone());
        }
        let user = self.user("Player");
        self.sfx_at(world, &user, key, &props, Self::player_emitter(world), 0)
    }

    fn object_sfx(&mut self, world: &World, key: &str, e: Emitter) -> bool {
        let user = self.user("ObjectCommon");
        self.sfx_at(world, &user, key, &PropList::default(), e, 0)
    }

    fn in_water(&self, world: &World, y: f32) -> bool {
        self.theme == 4 || world.liquid.as_ref().is_some_and(|l| y < l.y)
    }

    fn actor_sfx(
        &mut self,
        world: &World,
        kind: Kind,
        key: &str,
        local: &PropList,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(user) = self.pick_user(map::actor_users(kind)) else {
            return false;
        };
        let e = Emitter {
            x,
            y,
            water: self.in_water(world, y),
        };
        let mut props = local.clone();
        props.set("BodyWaterType", if e.water { "Water" } else { "None" });
        self.sfx_at(world, &user, key, &props, e, 0)
    }

    // ---- music -------------------------------------------------------------------------

    fn music_user(&self) -> String {
        format!("CourseSceneBGM_{}", self.style)
    }

    /// Start music key `key` (from `start` frames), chaining `then` after a non-looping one.
    fn play_music(&mut self, key: &str, start: u32, then: Option<&str>) -> bool {
        let user = self.music_user();
        let Some(plays) = self.resolve(&user, key, &PropList::default()) else {
            return false;
        };
        let Some(a) = plays.into_iter().next() else {
            return false;
        };
        let Some(sound) = self.wave_id(&a.wave) else {
            return false;
        };
        let then_id = then.and_then(|k| {
            let p = self
                .resolve(&user, k, &PropList::default())?
                .into_iter()
                .next()?;
            self.wave_id(&p.wave)
        });
        self.send(Command::Play(Play {
            sound,
            bus: Bus::Music,
            volume: a.volume,
            delay: self.frames(a.delay),
            group: a.group.unwrap_or(NO_GROUP),
            priority: a.priority,
            send: 0.0,
            start,
            then: then_id,
            ..Play::music(sound)
        }));
        true
    }

    /// The theme track key; the day track where the style has no night one (3D World has no
    /// night themes).
    fn theme_key(&self) -> String {
        let night = bgm_key(self.theme, self.night, self.hurry);
        if self.night && !self.lib.db.has_key(&self.music_user(), &night) {
            bgm_key(self.theme, false, self.hurry)
        } else {
            night
        }
    }

    /// The theme track from the top, or where it was if it is the same track.
    fn course_music(&mut self) {
        let key = self.theme_key();
        let start = match self.saved.take() {
            Some((k, frame)) if k == key => frame / RESUME_ALIGN * RESUME_ALIGN,
            _ => 0,
        };
        self.play_music(&key, start, None);
        self.track = Some(key);
        self.over = Over::None;
    }

    fn save_position(&mut self) {
        if self.over != Over::None {
            return;
        }
        let (Some(track), Some(&id)) = (
            self.track.clone(),
            self.track
                .as_ref()
                .and_then(|k| {
                    let user = self.music_user();
                    let mut w = Vec::new();
                    self.lib.db.waves_of(&user, k, &mut w);
                    w.first().cloned()
                })
                .as_ref()
                .and_then(|w| self.ids.get(w)),
        ) else {
            return;
        };
        let v = self.clock.load(Ordering::Relaxed);
        if v != u64::MAX && (v >> 32) as u16 == id {
            self.saved = Some((track, v as u32));
        }
    }

    fn overlay_music(&mut self, which: Over) {
        self.save_position();
        let key = match (which, self.hurry) {
            (Over::Star, false) => "BGM_Star",
            (Over::Star, true) => "BGM_StarHurry",
            (_, false) => "BGM_Switch",
            (_, true) => "BGM_SwitchHurry",
        };
        self.play_music(key, 0, None);
        self.over = which;
    }

    // ---- course lifecycle ----------------------------------------------------------------

    fn set_area_audio(&mut self, world: &World) {
        self.theme = world.level.theme;
        self.night = world.level.night;
        self.area = world.area;
        let fx = self
            .lib
            .db
            .effects
            .get(&self.style)
            .and_then(|row| row.get(self.theme as usize))
            .cloned()
            .unwrap_or_default();
        let delay = fx.delay.as_ref().map(|p| mmk_audio::fx::DelayParams {
            time: p.get("Delay").copied().unwrap_or(0.0),
            feedback: p.get("Feedback").copied().unwrap_or(0.0),
            lpf: p.get("Lpf").copied().unwrap_or(0.0),
            gain: p.get("OutGain").copied().unwrap_or(0.0),
        });
        let reverb = fx.reverb.as_ref().map(|p| mmk_audio::fx::ReverbParams {
            room: p.get("Room").copied().unwrap_or(-10000.0),
            reverb: p.get("Reverb").copied().unwrap_or(-10000.0),
            decay: p.get("DecayTime").copied().unwrap_or(1.0),
            hf_ratio: p.get("DecayHfRatio").copied().unwrap_or(0.5),
            predelay: p.get("ReflectionsDelay").copied().unwrap_or(0.0)
                + p.get("ReverbDelay").copied().unwrap_or(0.0),
            diffusion: p.get("Diffusion").copied().unwrap_or(100.0),
        });
        self.send(Command::SetEffects { delay, reverb });
        // theme ambience (NSMBU / 3D World), looping until stopped
        self.send(Command::StopHandle {
            handle: H_AMBIENT,
            fade: 0,
        });
        let e = Self::player_emitter(world);
        self.sfx_at(
            world,
            "CourseSceneBGM_Common",
            "playAmbient",
            &PropList::default(),
            Emitter { water: false, ..e },
            H_AMBIENT,
        );
    }

    /// A course was loaded or restarted: its music from the top.
    pub fn start_course(&mut self, world: &World) {
        self.send(Command::StopAll);
        let style = world.level.style.clone();
        if style != self.style || self.set.groups.is_empty() {
            self.style = style;
            self.build_set(world);
        }
        self.hurry = world.time >= 0 && world.time <= 100 * 60;
        self.over = Over::None;
        self.star_on = false;
        self.p_on = false;
        self.saved = None;
        self.fanfare = false;
        self.walking = false;
        self.counting = false;
        self.count_gap = 0;
        self.was_alive = world.player.alive();
        self.layer = false;
        self.coins = world.coins;
        self.on_off = world.on_off;
        self.water_lpf = 0.0;
        self.prev_player = PlayerSnap::of(world);
        self.footstep = 0;
        self.bumps = bumps(world);
        self.actors = snapshot(world);
        self.send(Command::SetMusicLayer {
            gain: 0.0,
            frames: 0,
        });
        self.set_area_audio(world);
        self.course_music();
    }

    fn stop_count(&mut self, world: &World) {
        if self.counting {
            self.send(Command::StopHandle {
                handle: H_TIMER,
                fade: 0,
            });
            self.counting = false;
            let e = Self::player_emitter(world);
            for u in ["GoalPole", "CastleGoal"] {
                let user = self.user(u);
                if self.sfx_at(world, &user, "TimerCountLast", &PropList::default(), e, 0) {
                    break;
                }
            }
        }
    }

    /// Call after every `World::step`.
    #[allow(clippy::collapsible_match)]
    pub fn after_step(&mut self, world: &World) {
        self.log.clear();
        let mut costume = std::mem::take(&mut self.costume);
        costume.sync(world, &mut |s| self.add_sound(s));
        self.costume = costume;
        let alive = world.player.alive();
        if alive && !self.was_alive {
            // the death sequence ended and the course restarted
            self.start_course(world);
            return;
        }
        self.was_alive = alive;
        if world.area != self.area {
            // through a pipe or door into the other area: its theme, effects, ambience, music
            self.set_area_audio(world);
            self.saved = None;
            if self.over == Over::None && !self.fanfare {
                self.course_music();
            } else if self.over != Over::None {
                let o = self.over;
                self.over = Over::None;
                self.overlay_music(o);
            }
            self.actors = snapshot(world);
        }

        let mut stomps = 0;
        let mut kicks = 0;
        let mut downs = 0;
        let mut emerged = 0;
        self.observe_actors(world, &mut stomps, &mut kicks, &mut downs, &mut emerged);
        let before = self.prev_player;
        self.observe_player(world);
        self.observe_blocks(world);

        let pe = Self::player_emitter(world);
        let events = world.events.clone();
        let mut coin_done = false;
        for ev in &events {
            match ev {
                // a costume jumps with its own sound (its second take every other jump)
                Event::Jump if self.costume.sound(world, "jump").is_some() => {
                    if let Some(id) = self.costume.jump(world) {
                        self.play_id(world, id, false);
                    }
                }
                Event::PowerUp if self.costume.sound(world, "powerup").is_some() => {
                    if let Some(id) = self.costume.sound(world, "powerup") {
                        self.play_id(world, id, false);
                    }
                }
                Event::Land | Event::Skid | Event::Taunt => {
                    let e = match ev {
                        Event::Land => "ground",
                        Event::Skid => "slip",
                        _ => "appeal",
                    };
                    if let Some(id) = self.costume.sound(world, e) {
                        self.play_id(world, id, false);
                    }
                }
                Event::Death if self.costume.sound(world, "down").is_some() => {
                    self.stop_count(world);
                    if let Some(id) = self.costume.sound(world, "down") {
                        self.play_id(world, id, true);
                    }
                    self.fanfare = true;
                    self.over = Over::None;
                }
                Event::Jump => {
                    let p = &world.player;
                    // launched by a spring or trampoline the player stood on: its sound
                    let spring =
                        before.ride.and_then(|id| {
                            world.actors.iter().find(|a| a.id == id).filter(|a| {
                                matches!(a.kind, Kind::Spring | Kind::Trampoline { .. })
                            })
                        });
                    if let Some(s) = spring {
                        let key = if p.vy > 5.0 { "high_jump" } else { "jump" };
                        let (kind, x, y) = (s.kind, s.x, s.y);
                        if self.actor_sfx(world, kind, key, &PropList::default(), x, y) {
                            continue;
                        }
                    }
                    let key = if p.in_water {
                        "swim"
                    } else if before.wall_slide {
                        // off a wall: the wall kick (NSMBU "kabekick", 3D World "wallkick")
                        if self.has_player_key("kabekick") {
                            "kabekick"
                        } else {
                            "wallkick"
                        }
                    } else if p.spinning {
                        "spinjump"
                    } else {
                        "jump"
                    };
                    let count = [(
                        "TripleJumpCount",
                        PropValue::Num(p.jump_count.min(2) as f32),
                    )];
                    if !self.player_sfx(world, key, &count) && key != "jump" {
                        self.player_sfx(world, "jump", &count);
                    }
                    // the voice of a chained jump (NSMBU 2nd / 3rd, 3D World 3rd) [inf: the
                    // keys' names; the code playing them was not found]
                    if key == "jump" {
                        let voice = match p.jump_count {
                            1 => "voice_triple_jump1",
                            2 if self.has_player_key("voice_triple_jump2") => "voice_triple_jump2",
                            2 => "voice_jump_high",
                            _ => "",
                        };
                        if !voice.is_empty() && self.has_player_key(voice) {
                            self.player_sfx(world, voice, &count);
                        }
                    }
                }
                Event::Coin => {
                    // a 10/30/50-coin adds its value in one tick: its own sound, once
                    let gained = (world.coins + 100 - self.coins % 100) % 100;
                    let big = match gained {
                        10 => Some("playCoin10"),
                        30 => Some("playCoin30"),
                        50 => Some("playCoin50"),
                        _ => None,
                    };
                    match big {
                        Some(_) if coin_done => {}
                        Some(key) => {
                            coin_done = true;
                            if !self.object_sfx(world, key, pe) {
                                self.object_sfx(world, "playCoin", pe);
                            }
                        }
                        None => {
                            self.object_sfx(world, "playCoin", pe);
                        }
                    }
                }
                Event::Stomp => {
                    if stomps > 0 {
                        stomps -= 1;
                    } else {
                        self.generic_enemy(world, "r_damage_stamp");
                    }
                }
                Event::Kick => {
                    if kicks > 0 {
                        kicks -= 1;
                    } else {
                        let props = PropList::default();
                        self.actor_sfx(
                            world,
                            Kind::Shell { red: false },
                            "kick",
                            &props,
                            pe.x,
                            pe.y,
                        );
                    }
                }
                Event::EnemyDown => {
                    if downs > 0 {
                        downs -= 1;
                    } else {
                        self.generic_enemy(world, "r_damage");
                    }
                }
                Event::Bump if self.note_hits > 0 => {
                    // a note block plays its note instead of the bump
                    self.note_hits -= 1;
                }
                Event::Bump => {
                    let head = world.player.head_hit.is_some()
                        && self.player_sfx(world, "block_hit", &[("HeadHitType", "Bound".into())]);
                    if !head {
                        self.object_sfx(world, "block_hit", pe);
                    }
                }
                Event::Break => {
                    self.object_sfx(world, "block_break", pe);
                }
                Event::ItemAppear => {
                    if emerged > 0 {
                        emerged -= 1;
                    } else {
                        self.object_sfx(world, "block_out", pe);
                    }
                }
                Event::PowerUp => {
                    // a form change plays the power-up, an item without one the pickup
                    // a star: its pickup; a form change: the power-up; else the item pickup
                    let p = &world.player;
                    let changing = before.form != Some(p.form)
                        || matches!(p.control, mmk_sim::player::Control::Change { .. });
                    let key = if p.star > 0 && !self.star_on {
                        "r_get_star"
                    } else if changing {
                        "r_power_up"
                    } else {
                        "r_get_item"
                    };
                    if !self.player_sfx(world, key, &[]) && key == "r_get_star" {
                        self.player_sfx(world, "r_get_item", &[]);
                    }
                }
                Event::PowerDown => {
                    self.player_sfx(world, "r_power_down", &[]);
                }
                Event::OneUp => {
                    let key = if world.coins < self.coins {
                        "play1UpSe_100coin"
                    } else {
                        "play1UpSe"
                    };
                    let e = pe;
                    if !self.sfx_at(
                        world,
                        "CourseSceneBGM_Common",
                        key,
                        &PropList::default(),
                        e,
                        0,
                    ) {
                        self.sfx_at(
                            world,
                            "CourseSceneBGM_Common",
                            "play1UpSe",
                            &PropList::default(),
                            e,
                            0,
                        );
                    }
                }
                Event::Fireball => {
                    if !self.player_sfx(world, "fireball", &[]) {
                        self.player_sfx(world, "fireball_motion", &[]);
                    }
                }
                Event::Death => {
                    self.stop_count(world);
                    self.send(Command::StopHandle {
                        handle: H_AMBIENT,
                        fade: 0,
                    });
                    self.play_music("FanfareDown", 0, None);
                    self.fanfare = true;
                    self.over = Over::None;
                }
                Event::Goal => {
                    self.send(Command::StopMusic {
                        fade: self.frames(8.0),
                    });
                    self.send(Command::StopHandle {
                        handle: H_AMBIENT,
                        fade: 0,
                    });
                    self.fanfare = true;
                    self.player_sfx(world, "pole", &[]);
                }
                Event::Hurry => {
                    self.hurry = true;
                    self.saved = None;
                    match self.over {
                        Over::None if !self.fanfare => {
                            let key = self.theme_key();
                            if !self.play_music("BGM_Hurry", 0, Some(&key)) {
                                self.play_music(&key, 0, None);
                            }
                            self.track = Some(key);
                        }
                        Over::None => {}
                        o => {
                            self.over = Over::None;
                            self.overlay_music(o);
                        }
                    }
                }
                Event::TimeCount => {
                    self.count_gap = 0;
                    if !self.counting {
                        self.counting = true;
                        for u in ["GoalPole", "CastleGoal"] {
                            let user = self.user(u);
                            if self.sfx_at(
                                world,
                                &user,
                                "TimerCount",
                                &PropList::default(),
                                pe,
                                H_TIMER,
                            ) {
                                break;
                            }
                        }
                    }
                }
                Event::Clear => {}
                Event::Pipe => {
                    // a door at the player (the sim reports doors as warps too)
                    let p = &world.player;
                    let door = world
                        .level
                        .doors
                        .iter()
                        .any(|&(x, y, _)| (x - p.x).abs() < 16.0 && (y - p.y).abs() < 24.0);
                    if door {
                        let e = pe;
                        let door_user = self.user("Door");
                        if !self.sfx_at(world, &door_user, "DoorOpen", &PropList::default(), e, 0) {
                            self.player_sfx(world, "DoorIn", &[]);
                        }
                    } else {
                        self.player_sfx(world, "DokanIn", &[]);
                    }
                }
                Event::Checkpoint => {
                    self.player_sfx(world, "r_MiddleFlag_On", &[]);
                }
                Event::Switch => {
                    if world.on_off != self.on_off {
                        self.object_sfx(world, "onoff_sw", pe);
                    } else {
                        // the P switch (its own user)
                        let props = PropList::default();
                        self.actor_sfx(world, Kind::PSwitch, "p_switch", &props, pe.x, pe.y);
                    }
                }
                Event::ConditionCount => {
                    self.object_sfx(world, "p_GoalMission_CountUp", pe);
                }
                Event::ConditionMet => {
                    if !self.player_sfx(world, "p_get_missionClear", &[]) {
                        self.object_sfx(world, "p_GoalMission_Bubble_appear", pe);
                    }
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        self.coins = world.coins;
        self.on_off = world.on_off;

        // star and P-switch music
        let normal = alive && world.goal_phase == GoalPhase::None && !self.fanfare;
        let star = normal && world.player.star > 0;
        let p = normal && world.p_timer > 0;
        if star && world.player.star == STAR_END_SIGN {
            self.sfx_at(
                world,
                "CourseSceneBGM_Common",
                "playStarEndSign",
                &PropList::default(),
                pe,
                0,
            );
        }
        if normal {
            if star && !self.star_on {
                self.overlay_music(Over::Star);
            } else if p && !self.p_on {
                self.overlay_music(Over::Switch);
            } else if !star && self.over == Over::Star {
                if p {
                    self.over = Over::None;
                    self.overlay_music(Over::Switch);
                } else {
                    self.course_music();
                }
            } else if !p && self.over == Over::Switch {
                if star {
                    self.over = Over::None;
                    self.overlay_music(Over::Star);
                } else {
                    self.course_music();
                }
            }
        }
        self.star_on = star;
        self.p_on = p;

        // the music's Yoshi layer
        let riding = world.player.yoshi.is_some();
        if riding != self.layer {
            self.layer = riding;
            self.send(Command::SetMusicLayer {
                gain: if riding { 1.0 } else { 0.0 },
                frames: self.frames(LAYER_FADE_FRAMES),
            });
        }

        // the clear fanfare as the player steps off the pole and walks to the castle
        let walking = world.goal_phase == GoalPhase::Walk;
        if walking && !self.walking {
            // a costume's own clear jingle replaces the fanfare
            match self.costume.sound(world, "goal") {
                Some(id) => self.play_id(world, id, true),
                None => {
                    self.play_music("FanfareGoal", 0, None);
                }
            }
        }
        self.walking = walking;

        if self.counting {
            self.count_gap += 1;
            if self.count_gap > 2 {
                self.stop_count(world);
            }
        }
        // underwater filter target for the player's own looping sounds is applied at start;
        // keep the ramp value for diagnostics
        let target = if world.player.in_water {
            WATER_LPF
        } else {
            0.0
        };
        self.water_lpf += (target - self.water_lpf).clamp(-0.04, 0.04);
    }

    fn generic_enemy(&mut self, world: &World, key: &str) {
        let p = &world.player;
        let props = PropList::default()
            .with("DownComboCount", p.stomp_chain.max(1) as i32)
            .with("Reaction", "Die")
            .with("Option", "None")
            .with("ScaleLevel", "Normal");
        self.actor_sfx(world, Kind::Goomba, key, &props, p.x, p.y);
    }

    /// Sounds from actors' state changes since the last tick (the game plays these from its
    /// objects' states: stomped, shell kicked, defeated, out of a block, landing, firing).
    fn observe_actors(
        &mut self,
        world: &World,
        stomps: &mut usize,
        kicks: &mut usize,
        downs: &mut usize,
        emerged: &mut usize,
    ) {
        let now = snapshot(world);
        let chain = world.player.stomp_chain.max(1) as i32;
        let mut todo: Vec<(Kind, &'static str, PropList, f32, f32)> = Vec::new();
        for (id, s) in &now {
            let props = |reaction: &str, prev: &Snap| {
                PropList::default()
                    .with("DownComboCount", chain)
                    .with("Reaction", reaction)
                    .with("Option", if prev.wings { "Wing" } else { "None" })
                    .with("ScaleLevel", if prev.big { "Big" } else { "Normal" })
            };
            match self.actors.get(id) {
                None => {
                    if matches!(s.mode, Mode::Emerge { .. }) {
                        let key = if s.kind.is_enemy() {
                            "block_outy"
                        } else {
                            "block_out"
                        };
                        todo.push((s.kind, key, PropList::default(), s.x, s.y));
                        *emerged += 1;
                    } else if matches!(s.kind, Kind::BulletBill | Kind::BanzaiBill { .. }) {
                        todo.push((
                            Kind::BulletBill,
                            "r_shoot_blaster",
                            PropList::default(),
                            s.x,
                            s.y,
                        ));
                    } else if let Some(key) = launch_key(s.kind) {
                        // a projectile appeared: its own launch sound (the shooter's for a
                        // cannonball)
                        todo.push((s.kind, key, PropList::default(), s.x, s.y));
                    }
                }
                Some(prev) if prev.kind != s.kind && matches!(prev.kind, Kind::YoshiEgg { .. }) => {
                    todo.push((prev.kind, "eggcrack", PropList::default(), s.x, s.y));
                }
                Some(prev)
                    if s.kind == Kind::BobOmb
                        && prev.phase != s.phase
                        && (s.phase == 9 || prev.phase == 0 && s.phase != 0) =>
                {
                    let key = if s.phase == 9 { "explode" } else { "ignite" };
                    todo.push((s.kind, key, PropList::default(), s.x, s.y));
                }
                Some(prev)
                    if s.kind == Kind::Pow && prev.mode != Mode::Dead && s.mode == Mode::Dead =>
                {
                    todo.push((s.kind, "pow", PropList::default(), s.x, s.y));
                }
                Some(prev) => {
                    let was_shell = matches!(prev.kind, Kind::Shell { .. });
                    let is_shell = matches!(s.kind, Kind::Shell { .. });
                    if prev.mode != Mode::Stomped && s.mode == Mode::Stomped {
                        todo.push((s.kind, "r_damage_stamp", props("Die", prev), s.x, s.y));
                        *stomps += 1;
                    } else if !was_shell && is_shell && s.mode == Mode::ShellWait {
                        // a Koopa stomped into its shell
                        todo.push((prev.kind, "r_damage_stamp", props("Damage", prev), s.x, s.y));
                        *stomps += 1;
                    } else if prev.mode == Mode::ShellSlide && s.mode == Mode::ShellWait {
                        todo.push((s.kind, "r_damage_stamp", props("Damage", prev), s.x, s.y));
                        *stomps += 1;
                    } else if prev.mode != Mode::ShellSlide && s.mode == Mode::ShellSlide {
                        todo.push((s.kind, "kick", PropList::default(), s.x, s.y));
                        *kicks += 1;
                    } else if prev.mode != Mode::DieFall && s.mode == Mode::DieFall {
                        todo.push((prev.kind, "r_damage", props("Die", prev), s.x, s.y));
                        *downs += 1;
                    } else if s.mode == Mode::ShellSlide
                        && prev.mode == Mode::ShellSlide
                        && prev.vx != 0.0
                        && s.vx.signum() != prev.vx.signum()
                    {
                        todo.push((s.kind, "koura_hit", PropList::default(), s.x, s.y));
                    } else if matches!(s.kind, Kind::Thwomp { .. })
                        && s.on_ground
                        && !prev.on_ground
                    {
                        todo.push((s.kind, "press", PropList::default(), s.x, s.y));
                    }
                }
            }
        }
        // the player's fireballs that went away on screen hit something
        let (cx, cy) = (world.camera_x, world.camera_y);
        for (id, prev) in &self.actors {
            if matches!(prev.kind, Kind::Fireball | Kind::Superball)
                && !now.contains_key(id)
                && prev.x > cx
                && prev.x < cx + VIEW_W
                && prev.y > cy
                && prev.y < cy + VIEW_H
            {
                todo.push((prev.kind, "r_hit", PropList::default(), prev.x, prev.y));
            }
        }
        self.actors = now;
        for (kind, key, props, x, y) in todo {
            if key == "r_shoot_cannon" {
                let user = self.user("SenkanHoudai");
                let e = Emitter {
                    x,
                    y,
                    water: self.in_water(world, y),
                };
                self.sfx_at(world, &user, "r_shoot", &PropList::default(), e, 0);
                continue;
            }
            if key == "r_shoot_blaster" {
                // a Bill Blaster fired: the cannon's sound
                let user = self.user("KillerHoudai");
                let e = Emitter {
                    x,
                    y,
                    water: self.in_water(world, y),
                };
                self.sfx_at(world, &user, "r_shoot", &PropList::default(), e, 0);
                continue;
            }
            if !self.actor_sfx(world, kind, key, &props, x, y) && key == "r_damage_stamp" {
                self.generic_enemy(world, key);
            }
        }
    }

    /// Note blocks that started a bump: their note, pitched by the block's row (`MusicalScale`
    /// 0…27 [obs 0x7100f1b750: the property's range]; row = scale [inf]).
    fn observe_blocks(&mut self, world: &World) {
        use mmk_sim::blocks::BlockKind;
        let now = bumps(world);
        let mut notes = Vec::new();
        for (&(x, y), &frames) in &now {
            let new = self.bumps.get(&(x, y)).is_none_or(|&f| f > frames);
            if new
                && let Some(b) = world.blocks.get(x, y)
                && b.kind == BlockKind::Note
            {
                notes.push((x, y));
            }
        }
        self.bumps = now;
        self.note_hits = notes.len();
        for (x, y) in notes {
            let props = PropList::default().with("MusicalScale", y.clamp(0, 27) as f32);
            let user = self.user("OnpuBlock");
            let e = Emitter {
                x: x as f32 * 16.0 + 8.0,
                y: y as f32 * 16.0 + 8.0,
                water: self.in_water(world, y as f32 * 16.0),
            };
            self.sfx_at(world, &user, "jump", &props, e, 0);
        }
    }

    pub fn next_handle(&mut self) -> u32 {
        self.next_handle += 1;
        self.next_handle
    }
}

/// The key a projectile plays when it appears, on its own user (`r_shoot_cannon`: the
/// cannon's shot).
fn launch_key(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::BowserFire => "shot_fire",
        Kind::EnemyFire => "fire",
        Kind::Wrench => "fly",
        Kind::Magic => "castspell",
        Kind::Cannonball => "r_shoot_cannon",
        _ => return None,
    })
}

/// Walk-cycle travel between footsteps (px of `walk_phase`) [inf: half the SMB1 walk
/// animation's 18].
const FOOTSTEP_PHASE: f32 = 9.0;

/// The player state the director watches for sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PlayerSnap {
    on_ground: bool,
    in_water: bool,
    pound_start: bool,
    wall_slide: bool,
    climbing: bool,
    shoe: bool,
    yoshi: bool,
    skid: bool,
    propeller: bool,
    walking: bool,
    ride: Option<u32>,
    tongue: bool,
    form: Option<Form>,
}

impl PlayerSnap {
    fn of(world: &World) -> Self {
        use mmk_sim::player::Pose;
        let p = &world.player;
        Self {
            on_ground: p.on_ground,
            in_water: p.in_water,
            pound_start: p.pound > 0,
            wall_slide: p.wall_slide > 0,
            climbing: p.climbing.is_some(),
            shoe: p.shoe.is_some(),
            yoshi: p.yoshi.is_some(),
            skid: p.pose == Pose::Skid,
            propeller: p.prop_rise > 0,
            walking: p.pose == Pose::Walk,
            ride: p.ride,
            tongue: world.tongue > 0,
            form: Some(p.form),
        }
    }
}

/// The area liquid's `BgWaterType` value.
fn liquid_type(world: &World) -> &'static str {
    match world.liquid.as_ref().map(|l| l.spec.kind) {
        Some(2) => "Lava",
        Some(3) => "Poison",
        _ => "Water",
    }
}

/// Blocks bumping now: tile → frames into the bump.
fn bumps(world: &World) -> BTreeMap<(i32, i32), u32> {
    world
        .blocks
        .map
        .iter()
        .filter_map(|(&k, b)| b.bump.map(|bump| (k, bump.frames)))
        .collect()
}

fn snapshot(world: &World) -> BTreeMap<u32, Snap> {
    world
        .actors
        .iter()
        .map(|a| {
            (
                a.id,
                Snap {
                    kind: a.kind,
                    mode: a.mode,
                    x: a.x,
                    y: a.y,
                    vx: a.vx,
                    on_ground: a.on_ground,
                    wings: a.wings,
                    big: a.big,
                    phase: a.phase,
                },
            )
        })
        .collect()
}

/// Files (relative to `audio/`) a course of `world`'s style will read first: its sound set
/// and the current area's theme music (hosts that read files asynchronously fetch these
/// before the course starts).
pub fn course_files(db: &SoundDb, world: &World) -> Vec<String> {
    let kinds: Vec<Kind> = world.actors.iter().map(|a| a.kind).collect();
    files_for(
        db,
        &world.level.style,
        world.level.theme,
        &[world.level.night],
        &kinds,
    )
}

/// [`course_files`] for a course known only by its style and theme (day and night music;
/// objects' sounds are read when first played).
pub fn style_files(db: &SoundDb, style: &str, theme: u8) -> Vec<String> {
    files_for(db, style, theme, &[false, true], &[])
}

fn files_for(db: &SoundDb, style: &str, theme: u8, nights: &[bool], kinds: &[Kind]) -> Vec<String> {
    let mut waves = Vec::new();
    let music = format!("CourseSceneBGM_{style}");
    for hurry in [false, true] {
        for night in std::iter::once(false).chain(nights.iter().copied()) {
            db.waves_of(&music, &bgm_key(theme, night, hurry), &mut waves);
        }
    }
    for k in map::MUSIC_KEYS {
        db.waves_of(&music, k, &mut waves);
    }
    let user = |b: &str| format!("{style}_{b}");
    for k in map::PLAYER_KEYS {
        db.waves_of(&user("Player"), k, &mut waves);
    }
    for k in map::OBJECT_KEYS {
        db.waves_of(&user("ObjectCommon"), k, &mut waves);
    }
    for k in map::COMMON_KEYS {
        db.waves_of("CourseSceneBGM_Common", k, &mut waves);
    }
    for &kind in kinds {
        for b in map::actor_users(kind) {
            let u = user(b);
            if db.user(&u).is_some() {
                for k in map::ACTOR_KEYS {
                    db.waves_of(&u, k, &mut waves);
                }
                break;
            }
        }
    }
    let mut files: Vec<String> = waves
        .iter()
        .filter_map(|w| db.waves.get(w).cloned())
        .collect();
    files.sort();
    files.dedup();
    files
}
