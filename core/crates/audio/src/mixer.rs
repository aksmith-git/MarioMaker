//! The mixer: a fixed set of voices rendered to interleaved stereo f32 at the output rate.
//!
//! - Positions are 32.32 fixed point frames, so playback is deterministic and drift-free.
//! - Resampling is linear interpolation from the sound's rate to the output rate.
//! - Mono sounds go to both channels at full level; stereo sounds keep their channels.
//! - Voices that are stopped fade out over [`STOP_FADE`] frames to avoid clicks.
//! - The render path does not allocate.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use crate::bank::{Bank, Sound};

pub const OUTPUT_RATE: u32 = 48_000;
pub const MAX_VOICES: usize = 32;
/// Fade applied when a voice is cut by a new sound or a stop (output frames, ≈ 2.7 ms).
pub const STOP_FADE: u32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Music,
    Sfx,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Start a sound effect. It loops if the sound has loop points, until stopped. Any voice
    /// already playing the same sound, or another sound of the same exclusive group, is cut.
    PlaySfx {
        sound: u16,
    },
    /// Stop every voice playing `sound`, fading over `fade` output frames.
    StopSfx {
        sound: u16,
        fade: u32,
    },
    /// Replace the music with `sound`. If `sound` does not loop, `then` starts when it ends.
    PlayMusic {
        sound: u16,
        then: Option<u16>,
    },
    /// Stop the music, fading over `fade` output frames.
    StopMusic {
        fade: u32,
    },
    /// Stop everything immediately (with the short anti-click fade).
    StopAll,
    SetVolumes {
        master: f32,
        music: f32,
        sfx: f32,
    },
    /// Take the sounds now in the shared music slots (see [`MusicSlots`]); they play as sound
    /// ids `SLOT_BASE + slot`.
    LoadSlots,
}

/// Sounds loaded per course (theme music is too large to keep every track decoded): the game
/// thread decodes into these slots, then sends [`Command::LoadSlots`].
pub type MusicSlots = Arc<std::sync::Mutex<[Option<Sound>; SLOTS]>>;
pub const SLOTS: usize = 4;
/// Sound id of music slot 0.
pub const SLOT_BASE: u16 = 0xFF00;

pub fn music_slots() -> MusicSlots {
    Arc::new(std::sync::Mutex::new(std::array::from_fn(|_| None)))
}

/// Bank sounds plus the loaded slots, by id.
struct Sounds<'a> {
    bank: &'a Bank,
    slots: &'a [Option<Sound>; SLOTS],
}

impl<'a> Sounds<'a> {
    fn get(&self, id: u16) -> Option<&'a Sound> {
        if id >= SLOT_BASE {
            self.slots.get((id - SLOT_BASE) as usize)?.as_ref()
        } else {
            self.bank.sounds.get(id as usize)
        }
    }
}

/// Bounded command queue from the game thread to the audio thread (no allocation per command).
pub fn command_channel(capacity: usize) -> (SyncSender<Command>, Receiver<Command>) {
    sync_channel(capacity)
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    sound: u16,
    bus: Bus,
    /// Position in source frames, 32.32 fixed point.
    pos: u64,
    /// Source frames per output frame, 32.32.
    step: u64,
    fade: f32,
    /// Added to `fade` every output frame (negative while fading out, 0 otherwise).
    fade_step: f32,
    /// Music chain: started in this voice when a non-looping sound ends.
    next: Option<u16>,
    /// Start order, for voice stealing.
    serial: u64,
}

pub struct Mixer {
    bank: Arc<Bank>,
    shared_slots: Option<MusicSlots>,
    slots: [Option<Sound>; SLOTS],
    out_rate: u32,
    voices: [Option<Voice>; MAX_VOICES],
    serial: u64,
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
}

fn step_for(rate: u32, out_rate: u32) -> u64 {
    ((rate as u64) << 32) / out_rate.max(1) as u64
}

impl Mixer {
    pub fn new(bank: Arc<Bank>, out_rate: u32) -> Self {
        Self {
            bank,
            shared_slots: None,
            slots: std::array::from_fn(|_| None),
            out_rate,
            voices: [None; MAX_VOICES],
            serial: 0,
            master: 1.0,
            music: 1.0,
            sfx: 1.0,
        }
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    /// Share the per-course music slots with the game thread.
    pub fn set_slots(&mut self, slots: MusicSlots) {
        self.shared_slots = Some(slots);
    }

    fn sounds(&self) -> Sounds<'_> {
        Sounds {
            bank: &self.bank,
            slots: &self.slots,
        }
    }

    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    /// Number of voices currently sounding (including ones fading out).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().flatten().count()
    }

    /// True if a voice is playing `sound` and not fading out.
    pub fn is_playing(&self, sound: u16) -> bool {
        self.voices
            .iter()
            .flatten()
            .any(|v| v.sound == sound && v.fade_step == 0.0)
    }

    /// The sound in the music voice that is not fading out, if any.
    pub fn current_music(&self) -> Option<u16> {
        self.voices
            .iter()
            .flatten()
            .find(|v| v.bus == Bus::Music && v.fade_step == 0.0)
            .map(|v| v.sound)
    }

    /// Apply every queued command.
    pub fn drain(&mut self, rx: &Receiver<Command>) {
        while let Ok(c) = rx.try_recv() {
            self.apply(c);
        }
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::PlaySfx { sound } => {
                let Some(s) = self.bank.sounds.get(sound as usize) else {
                    return;
                };
                let group = s.group;
                let bank = self.bank.clone();
                self.fade_where(STOP_FADE, |v| {
                    v.bus == Bus::Sfx
                        && (v.sound == sound
                            || (group != 0 && bank.sounds[v.sound as usize].group == group))
                });
                self.start(sound, Bus::Sfx, None);
            }
            Command::StopSfx { sound, fade } => {
                self.fade_where(fade, |v| v.bus == Bus::Sfx && v.sound == sound)
            }
            Command::PlayMusic { sound, then } => {
                if self.sounds().get(sound).is_none() {
                    return;
                }
                self.fade_where(STOP_FADE, |v| v.bus == Bus::Music);
                self.start(sound, Bus::Music, then);
            }
            Command::StopMusic { fade } => self.fade_where(fade, |v| v.bus == Bus::Music),
            Command::StopAll => self.fade_where(STOP_FADE, |_| true),
            Command::SetVolumes { master, music, sfx } => {
                self.master = master.clamp(0.0, 1.0);
                self.music = music.clamp(0.0, 1.0);
                self.sfx = sfx.clamp(0.0, 1.0);
            }
            Command::LoadSlots => {
                // never block the audio thread: if the game thread holds the lock, keep the old
                if let Some(Ok(g)) = self.shared_slots.as_ref().map(|s| s.try_lock()) {
                    // voices playing a slot sound stop first (its data is about to change)
                    for v in &mut self.voices {
                        if v.is_some_and(|v| v.sound >= SLOT_BASE) {
                            *v = None;
                        }
                    }
                    self.slots = g.clone();
                }
            }
        }
    }

    fn fade_where(&mut self, fade: u32, pred: impl Fn(&Voice) -> bool) {
        for slot in &mut self.voices {
            if let Some(v) = slot
                && pred(v)
            {
                if fade == 0 {
                    *slot = None;
                } else if v.fade_step == 0.0 {
                    v.fade_step = -v.fade / fade as f32;
                }
            }
        }
    }

    fn start(&mut self, sound: u16, bus: Bus, next: Option<u16>) {
        let Some(s) = self.sounds().get(sound) else {
            return;
        };
        let rate = s.rate;
        self.serial += 1;
        let voice = Voice {
            sound,
            bus,
            pos: 0,
            step: step_for(rate, self.out_rate),
            fade: 1.0,
            fade_step: 0.0,
            next,
            serial: self.serial,
        };
        let slot = match self.voices.iter().position(Option::is_none) {
            Some(i) => i,
            // steal: the oldest fading voice, else the oldest sound effect, else the oldest voice
            None => {
                let key = |v: &Voice| (v.fade_step == 0.0, v.bus == Bus::Music, v.serial);
                (0..MAX_VOICES)
                    .min_by_key(|&i| self.voices[i].as_ref().map(key))
                    .unwrap_or(0)
            }
        };
        self.voices[slot] = Some(voice);
    }

    /// Mix `out.len() / 2` stereo frames (interleaved L, R) into `out`, replacing its contents.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let bank = self.bank.clone();
        let sounds = Sounds {
            bank: &bank,
            slots: &self.slots,
        };
        for slot in &mut self.voices {
            let Some(v) = slot else { continue };
            let bus_gain = match v.bus {
                Bus::Music => self.music,
                Bus::Sfx => self.sfx,
            } * self.master;
            if !render_voice(&sounds, v, bus_gain, self.out_rate, &mut out[..frames * 2]) {
                *slot = None;
            }
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

/// Render one voice; false when it has finished.
fn render_voice(
    sounds: &Sounds,
    v: &mut Voice,
    bus_gain: f32,
    out_rate: u32,
    out: &mut [f32],
) -> bool {
    const SCALE: f32 = 1.0 / 32768.0;
    let Some(mut s) = sounds.get(v.sound) else {
        return false;
    };
    let mut f = 0;
    let frames = out.len() / 2;
    while f < frames {
        let mut i = (v.pos >> 32) as u32;
        match s.loop_range {
            Some((ls, le)) if i >= le => {
                v.pos -= ((le - ls) as u64) << 32;
                i = (v.pos >> 32) as u32;
            }
            None if i >= s.frames => {
                // finished: continue with the chained sound, if any
                match v.next.take().and_then(|n| sounds.get(n).map(|s| (n, s))) {
                    Some((n, ns)) => {
                        v.sound = n;
                        v.pos = 0;
                        s = ns;
                        v.step = step_for(s.rate, out_rate);
                        continue;
                    }
                    None => return false,
                }
            }
            _ => {}
        }
        // the frame after i, wrapping at the loop end; silence after the last frame
        let j = match s.loop_range {
            Some((ls, le)) if i + 1 >= le => Some(ls),
            _ if i + 1 >= s.frames => None,
            _ => Some(i + 1),
        };
        let t = (v.pos & 0xFFFF_FFFF) as f32 * (1.0 / 4_294_967_296.0);
        let ch = s.channels.max(1) as usize;
        let at = |k: Option<u32>, c: usize| -> f32 {
            k.map(|k| s.data[k as usize * ch + c.min(ch - 1)] as f32)
                .unwrap_or(0.0)
        };
        let l = at(Some(i), 0) + (at(j, 0) - at(Some(i), 0)) * t;
        let r = at(Some(i), 1) + (at(j, 1) - at(Some(i), 1)) * t;
        let g = s.gain * bus_gain * v.fade * SCALE;
        out[2 * f] += l * g;
        out[2 * f + 1] += r * g;
        if v.fade_step != 0.0 {
            v.fade += v.fade_step;
            if v.fade <= 0.0 {
                return false;
            }
        }
        v.pos += v.step;
        f += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Sound;

    fn bank(sounds: Vec<Sound>) -> Arc<Bank> {
        Arc::new(Bank::new(sounds))
    }

    fn render(m: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        m.render(&mut out);
        out
    }

    #[test]
    fn one_shot_plays_once_at_unity_rate() {
        let data: Vec<i16> = (0..10).map(|i| i * 1000).collect();
        let mut m = Mixer::new(bank(vec![Sound::from_pcm("a", 48000, 1, data)]), 48000);
        m.apply(Command::PlaySfx { sound: 0 });
        let out = render(&mut m, 16);
        for i in 0..10 {
            let want = (i * 1000) as f32 / 32768.0;
            assert!((out[2 * i] - want).abs() < 1e-6, "frame {i}");
            assert_eq!(out[2 * i], out[2 * i + 1], "mono goes to both channels");
        }
        assert!(out[20..].iter().all(|&s| s == 0.0));
        assert_eq!(m.active_voices(), 0, "voice ends after the last frame");
    }

    #[test]
    fn resamples_by_rate_ratio() {
        // 24 kHz ramp played at 48 kHz: every other output frame is an interpolated midpoint
        let data: Vec<i16> = (0..100).map(|i| i * 100).collect();
        let mut m = Mixer::new(bank(vec![Sound::from_pcm("a", 24000, 1, data)]), 48000);
        m.apply(Command::PlaySfx { sound: 0 });
        let out = render(&mut m, 8);
        let got: Vec<i32> = out
            .iter()
            .step_by(2)
            .map(|s| (s * 32768.0).round() as i32)
            .collect();
        assert_eq!(got, vec![0, 50, 100, 150, 200, 250, 300, 350]);
        // a 44.1 kHz sound of 441 frames lasts 480 output frames
        let mut m = Mixer::new(
            bank(vec![Sound::from_pcm("b", 44100, 1, vec![1000; 441])]),
            48000,
        );
        m.apply(Command::PlaySfx { sound: 0 });
        let out = render(&mut m, 600);
        let last = out.iter().step_by(2).rposition(|&s| s != 0.0).unwrap();
        assert_eq!(last, 479);
    }

    #[test]
    fn loops_between_loop_points_until_stopped() {
        // frames 0..4 then loop 2..4: 0 1 2 3 2 3 2 3 ...
        let s = Sound::from_pcm("l", 48000, 1, vec![0, 1000, 2000, 3000]).with_loop(2, 4);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        m.apply(Command::PlaySfx { sound: 0 });
        let out = render(&mut m, 10);
        let got: Vec<i32> = out
            .iter()
            .step_by(2)
            .map(|s| (s * 32768.0).round() as i32)
            .collect();
        assert_eq!(
            got,
            vec![0, 1000, 2000, 3000, 2000, 3000, 2000, 3000, 2000, 3000]
        );
        assert!(m.is_playing(0));
        m.apply(Command::StopSfx { sound: 0, fade: 4 });
        render(&mut m, 10);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn music_replaces_music_and_chains() {
        let jingle = Sound::from_pcm("jingle", 48000, 2, vec![100; 20]); // 10 stereo frames
        let bgm = Sound::from_pcm("bgm", 48000, 2, vec![200; 8]).with_loop(0, 4);
        let other = Sound::from_pcm("other", 48000, 1, vec![300; 50]);
        let mut m = Mixer::new(bank(vec![jingle, bgm, other]), 48000);
        m.apply(Command::PlayMusic {
            sound: 2,
            then: None,
        });
        assert_eq!(m.current_music(), Some(2));
        m.apply(Command::PlayMusic {
            sound: 0,
            then: Some(1),
        });
        assert_eq!(m.current_music(), Some(0));
        let out = render(&mut m, 400);
        assert_eq!(m.current_music(), Some(1), "jingle ended, loop took over");
        assert!((out[2 * 399] - 200.0 / 32768.0).abs() < 1e-6);
        assert_eq!(m.active_voices(), 1, "replaced music faded out");
        m.apply(Command::StopMusic { fade: 10 });
        render(&mut m, 20);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn same_sound_and_exclusive_group_retrigger() {
        let a = Sound::from_pcm("a", 48000, 1, vec![1000; 1000]).with_group(1);
        let b = Sound::from_pcm("b", 48000, 1, vec![1000; 1000]).with_group(1);
        let c = Sound::from_pcm("c", 48000, 1, vec![1000; 1000]);
        let mut m = Mixer::new(bank(vec![a, b, c]), 48000);
        m.apply(Command::PlaySfx { sound: 0 });
        m.apply(Command::PlaySfx { sound: 2 });
        m.apply(Command::PlaySfx { sound: 2 });
        render(&mut m, STOP_FADE as usize + 1);
        assert_eq!(m.active_voices(), 2, "c retriggered, not doubled");
        m.apply(Command::PlaySfx { sound: 1 });
        assert!(!m.is_playing(0), "b cuts a (same group)");
        render(&mut m, STOP_FADE as usize + 1);
        assert_eq!(m.active_voices(), 2);
        assert!(m.is_playing(1) && m.is_playing(2));
    }

    #[test]
    fn volumes_and_gain_scale_output() {
        let s = Sound::from_pcm("a", 48000, 1, vec![16384; 10]).with_gain(0.5);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        m.apply(Command::SetVolumes {
            master: 0.5,
            music: 1.0,
            sfx: 0.5,
        });
        m.apply(Command::PlaySfx { sound: 0 });
        let out = render(&mut m, 2);
        assert!((out[0] - 0.0625).abs() < 1e-6);
        m.apply(Command::PlayMusic {
            sound: 0,
            then: None,
        });
        let out = render(&mut m, 2);
        assert!(out[0] > 0.0);
    }

    #[test]
    fn output_is_clamped_and_voices_are_stolen() {
        let s = Sound::from_pcm("loud", 48000, 1, vec![32767; 100_000]);
        let bank = bank((0..40).map(|i| s.clone().tap_name(i)).collect());
        let mut m = Mixer::new(bank, 48000);
        for i in 0..40 {
            m.apply(Command::PlaySfx { sound: i });
        }
        assert_eq!(m.active_voices(), MAX_VOICES);
        assert!(m.is_playing(39), "newest sound got a voice");
        let out = render(&mut m, 4);
        assert!(out.iter().all(|&s| s <= 1.0));
    }

    #[test]
    fn deterministic_render() {
        let s = Sound::from_pcm(
            "a",
            44100,
            2,
            (0..2000).map(|i| (i * 37 % 4000) as i16).collect(),
        )
        .with_loop(100, 1000);
        let b = bank(vec![s]);
        let run = || {
            let mut m = Mixer::new(b.clone(), 48000);
            m.apply(Command::PlayMusic {
                sound: 0,
                then: None,
            });
            let mut all = Vec::new();
            for _ in 0..10 {
                all.extend(render(&mut m, 333));
            }
            all
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn commands_cross_threads() {
        let s = Sound::from_pcm("a", 48000, 1, vec![1000; 10]);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        let (tx, rx) = command_channel(8);
        std::thread::spawn(move || tx.try_send(Command::PlaySfx { sound: 0 }).unwrap())
            .join()
            .unwrap();
        m.drain(&rx);
        assert!(m.is_playing(0));
    }

    trait TapName {
        fn tap_name(self, i: u16) -> Self;
    }
    impl TapName for Sound {
        fn tap_name(mut self, i: u16) -> Self {
            self.name = format!("s{i}");
            self
        }
    }
}
