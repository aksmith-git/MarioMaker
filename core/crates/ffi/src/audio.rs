//! Audio for the engine: the [`MmkAudio`] handle (mixer, rendered on the host's audio thread) and
//! the [`Director`], which turns each tick's sim events and state changes into mixer commands.
//!
//! Threading: the engine (game thread) only holds a command sender; the mixer lives behind a
//! mutex inside `MmkAudio` and is only locked by `mmk_audio_render` (and never by the game
//! thread), so the lock is uncontended.

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

pub struct MmkAudio {
    inner: Mutex<Inner>,
    pub(crate) tx: SyncSender<Command>,
    pub(crate) bank: Arc<Bank>,
}

struct Inner {
    mixer: Mixer,
    rx: Receiver<Command>,
}

impl MmkAudio {
    pub fn new(bank: Bank) -> Self {
        let bank = Arc::new(bank);
        let (tx, rx) = command_channel(256);
        Self {
            inner: Mutex::new(Inner {
                mixer: Mixer::new(bank.clone(), OUTPUT_RATE),
                rx,
            }),
            tx,
            bank,
        }
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
            // SMB1 ground music; other themes are not imported yet and use it too
            bgm: id("bgm_plain"),
            bgm_hurry: id("bgm_plain_hurry"),
            bgm_star: id("bgm_star"),
            bgm_star_hurry: id("bgm_star_hurry"),
        }
    }
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
        }
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

        for ev in &world.events {
            match ev {
                Event::Jump => self.sfx(if world.player.form == Form::Small {
                    self.ids.jump_small
                } else {
                    self.ids.jump_big
                }),
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
            self.music(self.ids.fanfare_clear, None);
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
        let a = MmkAudio::new(b);
        a.send(Command::PlaySfx { sound: 0 });
        let mut out = vec![0.0f32; 64];
        a.render(&mut out);
        assert!((out[0] - 0.25 * OUTPUT_GAIN).abs() < 1e-6);
    }
}
