//! Audio for the engine: the [`AudioOut`] handle (mixer, rendered on the host's audio thread) and
//! the [`Director`], which turns each tick's sim events and state changes into mixer commands.
//! Shared by the FFI (`MmkAudio` is an `AudioOut`) and the portable shell.
//!
//! With the cache's gameplay sound database (`audio/sound.json`) the director plays the game's
//! own sound links per style, theme and object (module [`crate::sound`]); a cache without one
//! gets the fixed SMB1 sounds of `audio/index.json`.
//!
//! Threading: the game thread only holds a command sender (inside the `Director`); the mixer lives
//! behind a mutex inside `AudioOut` and is only locked by `render` (the audio thread), so the lock
//! is uncontended.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use mmk_audio::mixer::{MusicClock, SET_BASE, SharedSet, SoundSet, shared_set};
use mmk_audio::{Bank, Command, Mixer, OUTPUT_RATE, Play, Sound, command_channel};
use mmk_sim::World;
use mmk_sim::player::Form;
use mmk_sim::world::{Event, GoalPhase};

use crate::costume_sound::CostumeSounds;
pub use crate::costume_sound::{COSTUME_EVENTS, CostumeReader};
use crate::sound::Sounds;
pub use crate::sound::{FileReader, Library, course_files, style_files};

/// Hurry-up threshold (course timer frames), as in the sim.
const HURRY_FRAMES: i32 = 100 * 60;
/// Ticks without a `TimeCount` event after which the count-down loop stops (the sim counts every
/// second tick).
const COUNT_GAP: u32 = 2;
/// Fixed output gain: the game's sounds are mastered with a lot of headroom.
pub const OUTPUT_GAIN: f32 = 2.0;

/// Reads a file of the cache's `audio/` folder by relative path (kept for older hosts).
pub type MusicReader = FileReader;

pub struct AudioOut {
    inner: Mutex<Inner>,
    tx: SyncSender<Command>,
    bank: Arc<Bank>,
    rate: u32,
    shared: SharedSet,
    clock: MusicClock,
    library: Option<Arc<Library>>,
    /// SMM1 costume sounds (`costume_<id>_<event>.bwav` of the SMM1 sprite cache's `audio/`).
    costumes: Option<CostumeReader>,
}

struct Inner {
    mixer: Mixer,
    rx: Receiver<Command>,
}

impl AudioOut {
    /// A mixer rendering at [`OUTPUT_RATE`].
    pub fn new(bank: Bank) -> Self {
        Self::with_rate(bank, OUTPUT_RATE)
    }

    /// A mixer rendering at `rate` Hz (the output device's rate).
    pub fn with_rate(bank: Bank, rate: u32) -> Self {
        let bank = Arc::new(bank);
        let (tx, rx) = command_channel(1024);
        let shared = shared_set();
        let clock: MusicClock = Arc::new(AtomicU64::new(u64::MAX));
        let mut mixer = Mixer::new(bank.clone(), rate);
        mixer.set_shared(shared.clone());
        mixer.set_clock(clock.clone());
        mixer.set_output(OUTPUT_GAIN, true);
        Self {
            inner: Mutex::new(Inner { mixer, rx }),
            tx,
            bank,
            rate,
            shared,
            clock,
            library: None,
            costumes: None,
        }
    }

    /// SMM1 costume sounds from an SMM1 sprite cache directory (`<root>/audio/`).
    pub fn with_costume_dir(mut self, root: impl Into<PathBuf>) -> Self {
        self.set_costume_dir(root);
        self
    }

    /// [`AudioOut::with_costume_dir`] on an existing mixer; directors made before keep their
    /// old setting.
    pub fn set_costume_dir(&mut self, root: impl Into<PathBuf>) {
        let dir = root.into().join("audio");
        self.costumes = Some(Arc::new(move |name: &str| {
            std::fs::read(dir.join(format!("{name}.bwav"))).ok()
        }));
    }

    /// The sound database and waves of a cache directory (`<root>/audio/`).
    pub fn with_cache_dir(self, root: impl Into<PathBuf>) -> Self {
        let dir = root.into().join("audio");
        self.with_files(Box::new(move |rel| std::fs::read(dir.join(rel)).ok()))
    }

    /// Same as [`AudioOut::with_cache_dir`] (older name).
    pub fn with_music_dir(self, root: impl Into<PathBuf>) -> Self {
        self.with_cache_dir(root)
    }

    /// The sound database and waves through `read` (paths relative to `audio/`; the browser
    /// reads prefetched files).
    pub fn with_files(mut self, read: FileReader) -> Self {
        self.library = Library::load(read).map(Arc::new);
        self
    }

    /// Same as [`AudioOut::with_files`] (older name).
    pub fn with_music(self, read: MusicReader) -> Self {
        self.with_files(read)
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    pub fn library(&self) -> Option<&Arc<Library>> {
        self.library.as_ref()
    }

    /// A director sending this mixer the sounds of a world's ticks.
    pub fn director(&self) -> Director {
        match &self.library {
            Some(lib) => {
                let mut s = Sounds::new(
                    self.tx.clone(),
                    lib.clone(),
                    self.shared.clone(),
                    self.clock.clone(),
                    self.rate,
                );
                s.set_costumes(self.costumes.clone());
                Director {
                    tx: self.tx.clone(),
                    imp: Imp::Data(Box::new(s)),
                }
            }
            None => {
                let mut d = Director::new(self.tx.clone(), &self.bank);
                if let Imp::Legacy(l) = &mut d.imp {
                    l.costume = CostumeSounds::new(self.costumes.clone());
                    l.shared = Some(self.shared.clone());
                }
                d
            }
        }
    }

    /// Apply pending commands and render interleaved stereo frames into `out`.
    pub fn render(&self, out: &mut [f32]) {
        match self.inner.lock() {
            Ok(mut g) => {
                let Inner { mixer, rx } = &mut *g;
                mixer.drain(rx);
                mixer.render(out);
            }
            Err(_) => out.fill(0.0),
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.try_send(cmd);
    }
}

/// Maps sim events and state changes to mixer commands.
pub struct Director {
    tx: SyncSender<Command>,
    imp: Imp,
}

enum Imp {
    Data(Box<Sounds>),
    Legacy(Legacy),
}

impl Director {
    /// A director for the fixed SMB1 sounds of `bank` (no sound database).
    pub fn new(tx: SyncSender<Command>, bank: &Bank) -> Self {
        Self {
            tx: tx.clone(),
            imp: Imp::Legacy(Legacy::new(tx, bank)),
        }
    }

    /// Kept for hosts that load a course's music before starting it: the director now loads
    /// what a course needs itself in [`Director::start_course`].
    pub fn load_music(&mut self, _out: &AudioOut, _style: &str, _theme: u8) {}

    /// A course was loaded or restarted: start its music from the top.
    pub fn start_course(&mut self, world: &World) {
        match &mut self.imp {
            Imp::Data(s) => s.start_course(world),
            Imp::Legacy(l) => l.start_course(world),
        }
    }

    /// Call after every `World::step`.
    pub fn after_step(&mut self, world: &World) {
        match &mut self.imp {
            Imp::Data(s) => s.after_step(world),
            Imp::Legacy(l) => l.after_step(world),
        }
    }

    /// The (user, key) pairs the last tick played (data-driven director only).
    pub fn played(&self) -> &[(String, String)] {
        match &self.imp {
            Imp::Data(s) => &s.log,
            Imp::Legacy(_) => &[],
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.try_send(cmd);
    }
}

/// Sound ids of the fixed SMB1 set, resolved from the bank once.
#[derive(Clone, Copy, Debug, Default)]
struct Ids {
    jump_small: Option<u16>,
    jump_big: Option<u16>,
    coin: Option<u16>,
    stomp: Option<u16>,
    kick: [Option<u16>; 2],
    bump: Option<u16>,
    brick_break: Option<u16>,
    item_appear: Option<u16>,
    power_up: Option<u16>,
    power_down: Option<u16>,
    one_up: Option<u16>,
    fireball: Option<u16>,
    pole: Option<u16>,
    timer_count: Option<u16>,
    fanfare_down: Option<u16>,
    fanfare_clear: Option<u16>,
    fanfare_hurry: Option<u16>,
    bgm: Option<u16>,
    bgm_hurry: Option<u16>,
    bgm_star: Option<u16>,
    bgm_star_hurry: Option<u16>,
}

impl Ids {
    fn resolve(bank: &Bank) -> Self {
        let id = |n: &str| bank.id(n);
        Self {
            jump_small: id("jump_small"),
            jump_big: id("jump_big"),
            coin: id("coin"),
            stomp: id("stomp"),
            kick: [id("kick_a"), id("kick_b")],
            bump: id("bump"),
            brick_break: id("break"),
            item_appear: id("item_appear"),
            power_up: id("power_up"),
            power_down: id("power_down"),
            one_up: id("one_up"),
            fireball: id("fireball"),
            pole: id("pole"),
            timer_count: id("timer_count"),
            fanfare_down: id("fanfare_down"),
            fanfare_clear: id("fanfare_clear"),
            fanfare_hurry: id("fanfare_hurry"),
            bgm: id("bgm_plain"),
            bgm_hurry: id("bgm_plain_hurry"),
            bgm_star: id("bgm_star"),
            bgm_star_hurry: id("bgm_star_hurry"),
        }
    }
}

/// The SMB1-only director for caches without a sound database.
struct Legacy {
    tx: SyncSender<Command>,
    ids: Ids,
    was_alive: bool,
    star: bool,
    walking: bool,
    hurry: bool,
    counting: bool,
    count_gap: u32,
    kick_alt: usize,
    /// The worn SMM1 costume's sounds, added to the mixer's (otherwise empty) course set.
    costume: CostumeSounds,
    shared: Option<SharedSet>,
    set: Arc<SoundSet>,
    old_set: Option<Arc<SoundSet>>,
}

impl Legacy {
    fn new(tx: SyncSender<Command>, bank: &Bank) -> Self {
        Self {
            tx,
            ids: Ids::resolve(bank),
            was_alive: true,
            star: false,
            walking: false,
            hurry: false,
            counting: false,
            count_gap: 0,
            kick_alt: 0,
            costume: CostumeSounds::default(),
            shared: None,
            set: Arc::new(SoundSet::default()),
            old_set: None,
        }
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.try_send(cmd);
    }

    /// Add a sound to the course set (costume sounds); its id.
    fn add_sound(&mut self, s: Sound) -> Option<u16> {
        let shared = self.shared.as_ref()?;
        let mut next = (*self.set).clone();
        let id = SET_BASE + next.sounds.len() as u16;
        next.sounds.push(s);
        let next = Arc::new(next);
        if let Ok(mut g) = shared.lock() {
            *g = Some(next.clone());
        }
        self.old_set = Some(std::mem::replace(&mut self.set, next));
        self.send(Command::LoadSet);
        Some(id)
    }

    fn play_set(&self, id: Option<u16>, music: bool) {
        if let Some(sound) = id {
            self.send(Command::Play(if music {
                Play::music(sound)
            } else {
                Play::sfx(sound)
            }));
        }
    }

    fn sfx(&self, id: Option<u16>) {
        if let Some(sound) = id {
            self.send(Command::PlaySfx { sound });
        }
    }

    fn music(&self, id: Option<u16>, then: Option<u16>) {
        if let Some(sound) = id {
            self.send(Command::PlayMusic { sound, then });
        } else if let Some(sound) = then {
            self.send(Command::PlayMusic { sound, then: None });
        }
    }

    fn course_music(&self) {
        let id = if self.hurry {
            self.ids.bgm_hurry
        } else {
            self.ids.bgm
        };
        self.music(id, None);
    }

    fn stop_count(&mut self) {
        if self.counting {
            if let Some(sound) = self.ids.timer_count {
                self.send(Command::StopSfx { sound, fade: 128 });
            }
            self.counting = false;
        }
    }

    fn start_course(&mut self, world: &World) {
        self.send(Command::StopAll);
        self.was_alive = world.player.alive();
        self.star = false;
        self.walking = false;
        self.counting = false;
        self.count_gap = 0;
        self.hurry = world.time >= 0 && world.time <= HURRY_FRAMES;
        self.course_music();
    }

    fn after_step(&mut self, world: &World) {
        let alive = world.player.alive();
        if alive && !self.was_alive {
            self.start_course(world);
            return;
        }
        self.was_alive = alive;
        let mut costume = std::mem::take(&mut self.costume);
        costume.sync(world, &mut |s| self.add_sound(s));
        self.costume = costume;
        for ev in &world.events {
            match ev {
                // a costume jumps with its own sound, its second take every other jump
                Event::Jump if self.costume.sound(world, "jump").is_some() => {
                    let id = self.costume.jump(world);
                    self.play_set(id, false);
                }
                Event::Jump => self.sfx(
                    if matches!(world.player.form, Form::Small | Form::Costume) {
                        self.ids.jump_small
                    } else {
                        self.ids.jump_big
                    },
                ),
                Event::PowerUp if self.costume.sound(world, "powerup").is_some() => {
                    self.play_set(self.costume.sound(world, "powerup"), false)
                }
                Event::Land | Event::Skid | Event::Taunt => {
                    let e = match ev {
                        Event::Land => "ground",
                        Event::Skid => "slip",
                        _ => "appeal",
                    };
                    self.play_set(self.costume.sound(world, e), false);
                }
                // Lanky Mario's float cue: its SMM1 sound isn't imported [⬜]
                Event::LankyFloat => {}
                // the update power-ups' sounds (their names are on the Event variants): not
                // mapped yet [⬜]
                Event::SwordSlash
                | Event::SwordDashHit
                | Event::SwordThrust
                | Event::ShieldHit
                | Event::ArrowShoot
                | Event::ArrowHit
                | Event::Pull
                | Event::Throw
                | Event::SquatCharge
                | Event::BalloonBounce
                | Event::BoomerangThrow
                | Event::Rebound
                | Event::BoomerangReaction
                | Event::LinkBombExplode
                | Event::Wearing
                | Event::HeadDressOff
                | Event::CannonShot
                | Event::Explode
                | Event::PropellerBox
                | Event::KillerBreak => {}
                Event::Death if self.costume.sound(world, "down").is_some() => {
                    self.stop_count();
                    self.play_set(self.costume.sound(world, "down"), true);
                }
                Event::Coin => self.sfx(self.ids.coin),
                Event::Stomp => self.sfx(self.ids.stomp),
                Event::Kick | Event::EnemyDown => {
                    self.sfx(self.ids.kick[self.kick_alt]);
                    self.kick_alt ^= 1;
                }
                Event::Bump | Event::Switch => self.sfx(self.ids.bump),
                Event::Pipe => self.sfx(self.ids.power_down),
                Event::Checkpoint => {}
                Event::ConditionCount => self.sfx(self.ids.coin),
                Event::ConditionMet => self.sfx(self.ids.power_up),
                Event::Break => self.sfx(self.ids.brick_break),
                Event::ItemAppear => self.sfx(self.ids.item_appear),
                Event::PowerUp => self.sfx(self.ids.power_up),
                Event::PowerDown => self.sfx(self.ids.power_down),
                Event::OneUp => self.sfx(self.ids.one_up),
                Event::Fireball => self.sfx(self.ids.fireball),
                Event::Death => {
                    self.stop_count();
                    self.music(self.ids.fanfare_down, None);
                }
                Event::Goal => {
                    self.send(Command::StopMusic { fade: 128 });
                    self.sfx(self.ids.pole);
                }
                Event::Hurry => {
                    self.hurry = true;
                    if self.star {
                        self.music(self.ids.bgm_star_hurry, None);
                    } else {
                        self.music(self.ids.fanfare_hurry, self.ids.bgm_hurry);
                    }
                }
                Event::TimeCount => {
                    self.count_gap = 0;
                    if !self.counting {
                        self.counting = true;
                        self.sfx(self.ids.timer_count);
                    }
                }
                Event::Clear => {}
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        let star = alive && world.player.star > 0 && world.goal_phase == GoalPhase::None;
        if star && !self.star {
            self.music(
                if self.hurry {
                    self.ids.bgm_star_hurry
                } else {
                    self.ids.bgm_star
                },
                None,
            );
        } else if !star && self.star && alive && world.goal_phase == GoalPhase::None {
            self.course_music();
        }
        self.star = star;
        let walking = world.goal_phase == GoalPhase::Walk;
        if walking && !self.walking {
            // a costume's own clear jingle replaces the fanfare
            match self.costume.sound(world, "goal") {
                Some(id) => self.play_set(Some(id), true),
                None => self.music(self.ids.fanfare_clear, None),
            }
        }
        self.walking = walking;
        if self.counting {
            self.count_gap += 1;
            if self.count_gap > COUNT_GAP {
                self.stop_count();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_audio::Sound;

    const NAMES: &[&str] = &[
        "jump_small",
        "jump_big",
        "coin",
        "stomp",
        "kick_a",
        "kick_b",
        "bump",
        "break",
        "item_appear",
        "power_up",
        "power_down",
        "one_up",
        "fireball",
        "pole",
        "timer_count",
        "fanfare_down",
        "fanfare_clear",
        "fanfare_hurry",
        "bgm_plain",
        "bgm_plain_hurry",
        "bgm_star",
        "bgm_star_hurry",
    ];

    fn bank() -> Bank {
        Bank::new(
            NAMES
                .iter()
                .map(|n| Sound::from_pcm(n, 48000, 1, vec![0; 10]))
                .collect(),
        )
    }

    fn drain(rx: &Receiver<Command>) -> Vec<Command> {
        rx.try_iter().collect()
    }

    fn name(b: &Bank, id: u16) -> &str {
        &b.sounds[id as usize].name
    }

    fn played(b: &Bank, cmds: &[Command]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::PlaySfx { sound } => Some(format!("sfx:{}", name(b, *sound))),
                Command::PlayMusic { sound, then } => Some(format!(
                    "music:{}{}",
                    name(b, *sound),
                    then.map(|t| format!(">{}", name(b, t))).unwrap_or_default()
                )),
                Command::StopMusic { .. } => Some("stop_music".into()),
                Command::StopSfx { sound, .. } => Some(format!("stop:{}", name(b, *sound))),
                Command::StopAll => Some("stop_all".into()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn course_start_plays_ground_music() {
        let b = bank();
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let w = World::new();
        d.start_course(&w);
        assert_eq!(played(&b, &drain(&rx)), ["stop_all", "music:bgm_plain"]);
    }

    #[test]
    fn events_map_to_sounds() {
        let b = bank();
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let mut w = World::new();
        d.start_course(&w);
        drain(&rx);
        w.events = vec![
            Event::Jump,
            Event::Coin,
            Event::Kick,
            Event::EnemyDown,
            Event::Break,
        ];
        d.after_step(&w);
        assert_eq!(
            played(&b, &drain(&rx)),
            [
                "sfx:jump_small",
                "sfx:coin",
                "sfx:kick_a",
                "sfx:kick_b",
                "sfx:break"
            ]
        );
        w.player.form = Form::Super;
        w.events = vec![Event::Jump];
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["sfx:jump_big"]);
    }

    #[test]
    fn hurry_chains_jingle_into_fast_music_and_star_follows_hurry() {
        let b = bank();
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let mut w = World::new();
        d.start_course(&w);
        drain(&rx);
        w.events = vec![Event::Hurry];
        d.after_step(&w);
        assert_eq!(
            played(&b, &drain(&rx)),
            ["music:fanfare_hurry>bgm_plain_hurry"]
        );
        w.events.clear();
        w.player.star = 100;
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["music:bgm_star_hurry"]);
        w.player.star = 0;
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["music:bgm_plain_hurry"]);
    }

    #[test]
    fn death_then_restart_and_timer_count_loop() {
        let b = bank();
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let mut w = World::new();
        d.start_course(&w);
        drain(&rx);
        w.events = vec![Event::TimeCount];
        d.after_step(&w);
        w.events.clear();
        d.after_step(&w);
        w.events = vec![Event::TimeCount];
        d.after_step(&w);
        w.events.clear();
        for _ in 0..3 {
            d.after_step(&w);
        }
        assert_eq!(
            played(&b, &drain(&rx)),
            ["sfx:timer_count", "stop:timer_count"]
        );
        w.events = vec![Event::Death];
        w.player.kill();
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["music:fanfare_down"]);
        let w2 = World::new();
        d.after_step(&w2);
        assert_eq!(played(&b, &drain(&rx)), ["stop_all", "music:bgm_plain"]);
    }

    #[test]
    fn missing_sounds_are_skipped() {
        let b = Bank::new(vec![Sound::from_pcm("coin", 48000, 1, vec![0; 4])]);
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let mut w = World::new();
        d.start_course(&w);
        w.events = vec![Event::Jump, Event::Coin, Event::Hurry];
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["stop_all", "sfx:coin"]);
    }

    #[test]
    fn handle_renders_commands() {
        let b = Bank::new(vec![Sound::from_pcm("coin", 48000, 1, vec![8192; 100])]);
        let a = AudioOut::new(b);
        a.send(Command::PlaySfx { sound: 0 });
        let mut out = vec![0.0f32; 64];
        a.render(&mut out);
        assert!((out[0] - 0.25 * OUTPUT_GAIN).abs() < 1e-6);
    }

    /// A costumed player's jump plays the costume's own sound, added to the course set when the
    /// costume is put on; without the costume, the normal jump sound.
    #[test]
    fn costume_sounds_replace_the_jump() {
        let b = bank();
        let (tx, rx) = command_channel(64);
        let mut d = Director::new(tx, &b);
        let pcm: Vec<u8> = (0..64i16).flat_map(|v| (v * 100).to_le_bytes()).collect();
        let wave = mmk_formats::audio::bwav::build(&[mmk_formats::audio::bwav::BuildChannel {
            codec: mmk_formats::audio::Codec::Pcm16,
            pan: 2,
            sample_rate: 32000,
            samples: 64,
            coefs: [0; 16],
            loop_range: None,
            data: &pcm,
        }]);
        let reader: CostumeReader =
            Arc::new(move |n: &str| (n == "costume_045_jump").then(|| wave.clone()));
        let shared = shared_set();
        if let Imp::Legacy(l) = &mut d.imp {
            l.costume = CostumeSounds::new(Some(reader));
            l.shared = Some(shared.clone());
        }
        let mut w = World::new();
        d.start_course(&w);
        drain(&rx);
        w.player.form = Form::Costume;
        w.player.costume = 45;
        w.events = vec![Event::Jump];
        d.after_step(&w);
        let cmds = drain(&rx);
        assert!(matches!(cmds[0], Command::LoadSet));
        assert!(matches!(cmds[1], Command::Play(p) if p.sound == SET_BASE));
        let set = shared.lock().unwrap().clone().unwrap();
        assert_eq!(set.sounds[0].name, "costume_045_jump");
        w.player.form = Form::Small;
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["sfx:jump_small"]);
    }
}
