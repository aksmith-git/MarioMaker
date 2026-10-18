//! Audio for the engine: the [`AudioOut`] handle (mixer, rendered on the host's audio thread) and
//! the [`Director`], which turns each tick's sim events and state changes into mixer commands.
//! Shared by the FFI (`MmkAudio` is an `AudioOut`) and the portable shell.
//!
//! Threading: the game thread only holds a command sender (inside the `Director`); the mixer lives
//! behind a mutex inside `AudioOut` and is only locked by `render` (the audio thread), so the lock
//! is uncontended.

use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use mmk_audio::{Bank, Command, Mixer, OUTPUT_RATE, command_channel};
use mmk_sim::World;
use mmk_sim::player::Form;
use mmk_sim::world::{Event, GoalPhase};

/// Hurry-up threshold (course timer frames), as in the sim.
const HURRY_FRAMES: i32 = 100 * 60;
/// Ticks without a `TimeCount` event after which the count-down loop stops (the sim counts every
/// second tick).
const COUNT_GAP: u32 = 2;
/// Fixed output gain: the game's sounds are mastered with a lot of headroom.
pub const OUTPUT_GAIN: f32 = 2.0;

pub struct AudioOut {
    inner: Mutex<Inner>,
    tx: SyncSender<Command>,
    bank: Arc<Bank>,
    rate: u32,
    /// Per-course music (decoded on the game thread) and where its files are read from.
    slots: mmk_audio::mixer::MusicSlots,
    music: Option<MusicReader>,
    /// SMM1 costume sounds (`costume_<id>_<event>.bwav` of the SMM1 sprite cache's `audio/`).
    costumes: Option<std::sync::Arc<MusicReader>>,
}

/// SMM1 costume sound events, in the order of their slots after the music's (SMM1 event enum
/// "down, goal, jump, powerup, appeal, ground, slip"; `jump2` is an alternate take).
const COSTUME_EVENTS: [&str; 8] = [
    "jump", "jump2", "powerup", "appeal", "ground", "slip", "down", "goal",
];

/// Reads `audio/music/<name>.bwav` of an asset cache (a directory, or the browser's files).
pub type MusicReader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

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
        let (tx, rx) = command_channel(256);
        let slots = mmk_audio::mixer::music_slots();
        let mut mixer = Mixer::new(bank.clone(), rate);
        mixer.set_slots(slots.clone());
        Self {
            inner: Mutex::new(Inner { mixer, rx }),
            tx,
            bank,
            rate,
            slots,
            music: None,
            costumes: None,
        }
    }

    /// SMM1 costume sounds from an SMM1 sprite cache directory (`<root>/audio/`).
    pub fn with_costume_dir(mut self, root: impl Into<std::path::PathBuf>) -> Self {
        self.set_costume_dir(root);
        self
    }

    /// [`AudioOut::with_costume_dir`] on an existing mixer; directors made before keep their
    /// old setting.
    pub fn set_costume_dir(&mut self, root: impl Into<std::path::PathBuf>) {
        let dir = root.into().join("audio");
        self.costumes = Some(std::sync::Arc::new(Box::new(move |name: &str| {
            std::fs::read(dir.join(format!("{name}.bwav"))).ok()
        })));
    }

    /// Theme music from a cache directory (`<root>/audio/music/`).
    pub fn with_music_dir(self, root: impl Into<std::path::PathBuf>) -> Self {
        let dir = root.into().join("audio/music");
        self.with_music(Box::new(move |name| {
            std::fs::read(dir.join(format!("{name}.bwav"))).ok()
        }))
    }

    pub fn with_music(mut self, reader: MusicReader) -> Self {
        self.music = Some(reader);
        self
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    /// A director sending this mixer the sounds of a world's ticks.
    pub fn director(&self) -> Director {
        let mut d = Director::new(self.tx.clone(), &self.bank);
        d.costume_audio = self.costumes.clone().map(|r| (r, self.slots.clone()));
        d
    }

    /// Apply pending commands and render interleaved stereo frames into `out`.
    pub fn render(&self, out: &mut [f32]) {
        match self.inner.lock() {
            Ok(mut g) => {
                let Inner { mixer, rx } = &mut *g;
                mixer.drain(rx);
                mixer.render(out);
                for s in out.iter_mut() {
                    *s = (*s * OUTPUT_GAIN).clamp(-1.0, 1.0);
                }
            }
            Err(_) => out.fill(0.0),
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.try_send(cmd);
    }
}

/// Sound ids the director uses, resolved from the bank once.
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
        Self::resolve_style(bank, "M1")
    }

    /// Prefer the style's variant of each sound (`M3.coin`), else the SMB1 one.
    fn resolve_style(bank: &Bank, style: &str) -> Self {
        let id = |n: &str| bank.id(&format!("{style}.{n}")).or_else(|| bank.id(n));
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
            // SMB1 ground music; other themes are not imported yet and use it too
            bgm: id("bgm_plain"),
            bgm_hurry: id("bgm_plain_hurry"),
            bgm_star: id("bgm_star"),
            bgm_star_hurry: id("bgm_star_hurry"),
        }
    }
}

/// The music files (`audio/music/<name>.bwav`) a course of `style` and `theme` plays, in order
/// of preference: theme track, its hurry variant, star music, star hurry. Hosts that read files
/// asynchronously (the browser) fetch these before [`Director::load_music`].
pub fn music_candidates(style: &str, theme: u8) -> [Vec<String>; 4] {
    let t = theme;
    let theme_names = |suffix: &str| {
        vec![
            format!("{style}_{t}{suffix}"),
            format!("{style}_0{suffix}"),
            format!("M1_{t}{suffix}"),
            format!("M1_0{suffix}"),
        ]
    };
    [
        theme_names(""),
        theme_names("_hurry"),
        vec![format!("{style}_star"), "M1_star".into()],
        vec![format!("{style}_star_hurry"), "M1_star_hurry".into()],
    ]
}

/// Maps sim events and state changes to mixer commands. Pure bookkeeping, no audio state: it
/// only remembers what it last asked for.
pub struct Director {
    tx: SyncSender<Command>,
    ids: Ids,
    was_alive: bool,
    star: bool,
    walking: bool,
    hurry: bool,
    counting: bool,
    count_gap: u32,
    kick_alt: usize,
    /// SMM1 costume sounds: where they are read from and the mixer slots they go into; the
    /// costume loaded, its sound ids by `COSTUME_EVENTS`, and the jump take to use next.
    costume_audio: Option<(std::sync::Arc<MusicReader>, mmk_audio::mixer::MusicSlots)>,
    costume_loaded: Option<u16>,
    costume_ids: [Option<u16>; 8],
    jump_alt: bool,
}

impl Director {
    pub fn new(tx: SyncSender<Command>, bank: &Bank) -> Self {
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
            costume_audio: None,
            costume_loaded: None,
            costume_ids: [None; 8],
            jump_alt: false,
        }
    }

    /// Load the worn costume's sounds into the costume slots when it changes.
    fn sync_costume(&mut self, world: &World) {
        let want = (world.player.form == Form::Costume).then_some(world.player.costume);
        if want.is_none() || want == self.costume_loaded {
            return;
        }
        let Some((read, slots)) = &self.costume_audio else {
            return;
        };
        let id = want.unwrap_or(0);
        let sounds: Vec<Option<mmk_audio::Sound>> = COSTUME_EVENTS
            .iter()
            .map(|e| {
                let name = format!("costume_{id:03}_{e}");
                let bytes = read(&name)?;
                mmk_audio::Sound::from_bwav(&name, &bytes).ok()
            })
            .collect();
        let base = mmk_audio::mixer::MUSIC_SLOTS;
        match slots.lock() {
            Ok(mut g) => {
                for (k, s) in sounds.iter().enumerate() {
                    g[base + k] = s.clone();
                }
            }
            Err(_) => return,
        }
        self.send(Command::LoadSlots);
        for (k, s) in sounds.iter().enumerate() {
            self.costume_ids[k] = s
                .as_ref()
                .map(|_| mmk_audio::mixer::SLOT_BASE + (base + k) as u16);
        }
        self.costume_loaded = want;
    }

    /// The worn costume's sound for `event` (None: not costumed, or the costume lacks it).
    fn costume_sound(&self, world: &World, event: &str) -> Option<u16> {
        if world.player.form != Form::Costume || self.costume_loaded != Some(world.player.costume) {
            return None;
        }
        let k = COSTUME_EVENTS.iter().position(|e| *e == event)?;
        self.costume_ids[k]
    }

    /// The course's own music: its style's theme track and hurry variant, and the style's star
    /// music, decoded from the cache's `audio/music/` into the mixer's slots. Missing tracks fall
    /// back to the style's ground track, then SMB1's, then the bank's SMB1 ground music.
    pub fn load_music(&mut self, out: &AudioOut, style: &str, theme: u8) {
        self.ids = Ids::resolve_style(&out.bank, style);
        let Some(read) = &out.music else { return };
        let load = |names: &[String]| {
            names.iter().find_map(|n| {
                let bytes = read(n)?;
                mmk_audio::Sound::from_bwav(n, &bytes).ok()
            })
        };
        let [main, hurry, star, star_hurry] = music_candidates(style, theme);
        let tracks = [load(&main), load(&hurry), load(&star), load(&star_hurry)];
        let present: Vec<bool> = tracks.iter().map(Option::is_some).collect();
        match out.slots.lock() {
            Ok(mut g) => {
                for (k, t) in tracks.into_iter().enumerate() {
                    g[k] = t;
                }
            }
            Err(_) => return,
        }
        self.send(Command::LoadSlots);
        let slot = |k: usize, old: Option<u16>| {
            if present[k] {
                Some(mmk_audio::mixer::SLOT_BASE + k as u16)
            } else {
                old
            }
        };
        self.ids.bgm = slot(0, self.ids.bgm);
        self.ids.bgm_hurry = slot(1, self.ids.bgm_hurry);
        self.ids.bgm_star = slot(2, self.ids.bgm_star);
        self.ids.bgm_star_hurry = slot(3, self.ids.bgm_star_hurry);
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.try_send(cmd);
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

    /// A course was loaded or restarted: start its music from the top.
    pub fn start_course(&mut self, world: &World) {
        self.send(Command::StopAll);
        self.was_alive = world.player.alive();
        self.star = false;
        self.walking = false;
        self.counting = false;
        self.count_gap = 0;
        self.hurry = world.time >= 0 && world.time <= HURRY_FRAMES;
        self.course_music();
    }

    /// Call after every `World::step`.
    pub fn after_step(&mut self, world: &World) {
        let alive = world.player.alive();
        if alive && !self.was_alive {
            // the death sequence ended and the course restarted
            self.start_course(world);
            return;
        }
        self.was_alive = alive;
        self.sync_costume(world);
        let worn: [Option<u16>; 8] = COSTUME_EVENTS.map(|e| self.costume_sound(world, e));
        let costume = |e: &str| {
            COSTUME_EVENTS
                .iter()
                .position(|x| *x == e)
                .and_then(|k| worn[k])
        };

        for ev in &world.events {
            match ev {
                // a costume jumps with its own sound, its second take every other jump
                Event::Jump if costume("jump").is_some() => {
                    let alt = costume("jump2").filter(|_| self.jump_alt);
                    self.jump_alt = !self.jump_alt;
                    self.sfx(alt.or(costume("jump")));
                }
                Event::Jump => self.sfx(
                    if matches!(world.player.form, Form::Small | Form::Costume) {
                        self.ids.jump_small
                    } else {
                        self.ids.jump_big
                    },
                ),
                Event::PowerUp if costume("powerup").is_some() => self.sfx(costume("powerup")),
                Event::Land | Event::Skid | Event::Taunt => {
                    let e = match ev {
                        Event::Land => "ground",
                        Event::Skid => "slip",
                        _ => "appeal",
                    };
                    self.sfx(costume(e));
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
                Event::Death if costume("down").is_some() => {
                    self.stop_count();
                    self.music(costume("down"), None);
                }
                Event::Coin => self.sfx(self.ids.coin),
                Event::Stomp => self.sfx(self.ids.stomp),
                Event::Kick | Event::EnemyDown => {
                    // the game picks between two takes of the kick sound
                    self.sfx(self.ids.kick[self.kick_alt]);
                    self.kick_alt ^= 1;
                }
                // SMB1 uses the power-down sound for pipes; switches reuse the bump [inf]
                Event::Bump | Event::Switch => self.sfx(self.ids.bump),
                Event::Pipe => self.sfx(self.ids.power_down),
                Event::Checkpoint => {}
                // the game's count-up / condition-met sounds are not mapped yet [inf stand-ins]
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
            }
        }

        // star music while invincible in normal play
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

        // course-clear fanfare as the player steps off the pole and walks to the castle
        let walking = world.goal_phase == GoalPhase::Walk;
        if walking && !self.walking {
            // a costume's own clear jingle replaces the fanfare
            self.music(
                self.costume_sound(world, "goal").or(self.ids.fanfare_clear),
                None,
            );
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
        // goal count-down: loop starts on the first count, stops after the counts end
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
        // death fanfare, then the course music again when the player is back
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

    /// A costumed player's jump plays the costume's own sound, loaded into a costume slot when
    /// the costume is put on; without the costume, the normal jump sound.
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
        let reader: MusicReader =
            Box::new(move |n: &str| (n == "costume_045_jump").then(|| wave.clone()));
        let slots = mmk_audio::mixer::music_slots();
        d.costume_audio = Some((std::sync::Arc::new(reader), slots.clone()));
        let mut w = World::new();
        d.start_course(&w);
        drain(&rx);
        w.player.form = Form::Costume;
        w.player.costume = 45;
        w.events = vec![Event::Jump];
        d.after_step(&w);
        let cmds = drain(&rx);
        let slot = mmk_audio::mixer::SLOT_BASE + mmk_audio::mixer::MUSIC_SLOTS as u16;
        assert!(matches!(cmds[0], Command::LoadSlots));
        assert!(matches!(cmds[1], Command::PlaySfx { sound } if sound == slot));
        assert!(slots.lock().unwrap()[mmk_audio::mixer::MUSIC_SLOTS].is_some());
        w.player.form = Form::Small;
        d.after_step(&w);
        assert_eq!(played(&b, &drain(&rx)), ["sfx:jump_small"]);
    }
}
