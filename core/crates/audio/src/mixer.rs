//! The mixer: a fixed set of voices rendered to interleaved stereo f32 at the output rate.
//!
//! - Positions are 32.32 fixed point frames, so playback is deterministic and drift-free.
//! - Resampling is linear interpolation from the sound's rate (times the voice's pitch) to the
//!   output rate.
//! - Mono sounds go to both channels; stereo sounds keep their channels; a four-channel sound
//!   (music with an extra layer, e.g. the Yoshi drums) adds its second pair at the music layer
//!   gain. Voices have a volume, a balance pan and a one-pole low-pass.
//! - Sounds of the course's [`SoundSet`] belong to the game's sound groups: a group (and every
//!   folder above it) limits how many voices play at once, steals the lowest-priority, oldest
//!   voice for a new one of at least its priority, and lowers (ducks) the groups it names while
//!   it plays.
//! - Effect voices are also sent to the theme's echo and reverb.
//! - Voices that are stopped fade out over [`STOP_FADE`] frames (or their own stop fade).
//! - The render path does not allocate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use crate::bank::{Bank, Samples, Sound};
use crate::fx::{self, Delay, DelayParams, Reverb, ReverbParams};
use crate::stream::Cursor;

pub const OUTPUT_RATE: u32 = 48_000;
pub const MAX_VOICES: usize = 48;
/// Fade applied when a voice is cut by a new sound or a stop (output frames, ≈ 2.7 ms).
pub const STOP_FADE: u32 = 128;
/// Sound id of the course sound set's first sound.
pub const SET_BASE: u16 = 0x8000;
/// No sound group.
pub const NO_GROUP: u16 = u16::MAX;
/// Groups the mixer tracks ducking for.
pub const MAX_GROUPS: usize = 256;
/// Frames mixed per inner block (the effect send buffer's size).
const BLOCK: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Music,
    Sfx,
}

/// One voice to start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    pub sound: u16,
    pub bus: Bus,
    pub volume: f32,
    /// Playback rate factor.
    pub pitch: f32,
    /// −1 left … 1 right.
    pub pan: f32,
    /// Output frames before it starts.
    pub delay: u32,
    /// Output frames it plays (0: until it ends or is stopped).
    pub duration: u32,
    /// Output frames of fade-in.
    pub fade_in: u32,
    /// Output frames of fade-out when stopped (0: [`STOP_FADE`]).
    pub stop_fade: u32,
    pub group: u16,
    pub priority: f32,
    /// Low-pass amount 0…1.
    pub lpf: f32,
    /// Send to the theme effects.
    pub send: f32,
    /// Caller's id for later changes (0: none).
    pub handle: u32,
    /// First source frame.
    pub start: u32,
    /// Music: started in this voice when a non-looping sound ends.
    pub then: Option<u16>,
}

impl Play {
    pub fn sfx(sound: u16) -> Self {
        Self {
            sound,
            bus: Bus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            delay: 0,
            duration: 0,
            fade_in: 0,
            stop_fade: 0,
            group: NO_GROUP,
            priority: 0.5,
            lpf: 0.0,
            send: 1.0,
            handle: 0,
            start: 0,
            then: None,
        }
    }

    pub fn music(sound: u16) -> Self {
        Self {
            bus: Bus::Music,
            send: 0.0,
            ..Self::sfx(sound)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Start a bank sound effect at its own gain. It loops if the sound has loop points, until
    /// stopped. Any voice already playing the same sound, or another sound of the same
    /// exclusive group, is cut.
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
    /// Start a voice (music replaces the music).
    Play(Play),
    /// Stop the voices started with `handle`.
    StopHandle {
        handle: u32,
        fade: u32,
    },
    /// Change a playing handle's volume, pan and low-pass.
    SetHandle {
        handle: u32,
        volume: f32,
        pan: f32,
        lpf: f32,
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
    /// Gain of the music's second channel pair, reached over `frames` output frames.
    SetMusicLayer {
        gain: f32,
        frames: u32,
    },
    /// The theme's echo and reverb (None: off).
    SetEffects {
        delay: Option<DelayParams>,
        reverb: Option<ReverbParams>,
    },
    /// Take the course sound set now in the shared slot (see [`SharedSet`]).
    LoadSet,
}

/// A group of the course's sound set: itself and its folders (innermost first), volume, voice
/// limit, groups it ducks (group, volume, fade-in s, fade-out s).
#[derive(Clone, Debug, Default)]
pub struct GroupDef {
    pub chain: Vec<u16>,
    pub volume: f32,
    pub limit: Option<u16>,
    pub limit_type: u8,
    pub duck: Vec<(u16, f32, f32, f32)>,
}

/// The sounds and groups of one course; its sounds play as ids `SET_BASE + index`.
#[derive(Clone, Debug, Default)]
pub struct SoundSet {
    pub sounds: Vec<Sound>,
    pub groups: Vec<GroupDef>,
}

/// The game thread builds a [`SoundSet`] here, then sends [`Command::LoadSet`].
pub type SharedSet = Arc<Mutex<Option<Arc<SoundSet>>>>;

pub fn shared_set() -> SharedSet {
    Arc::new(Mutex::new(None))
}

/// The music voice's sound and source frame, `sound << 32 | frame` (u64::MAX: no music),
/// updated after every render.
pub type MusicClock = Arc<AtomicU64>;

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
    pitch: f32,
    fade: f32,
    /// Added to `fade` every output frame: negative while fading out, positive while fading in.
    fade_step: f32,
    next: Option<u16>,
    serial: u64,
    gain: f32,
    pan: f32,
    lpf_a: f32,
    lp: [f32; 2],
    delay: u32,
    remaining: u32,
    stop_fade: u32,
    group: u16,
    priority: f32,
    send: f32,
    handle: u32,
    cursor: Cursor,
}

impl Voice {
    fn stopping(&self) -> bool {
        self.fade_step < 0.0
    }
    fn stop(&mut self, frames: u32) {
        if frames == 0 {
            self.fade = 0.0;
            self.fade_step = -1.0;
        } else if !self.stopping() {
            self.fade_step = -self.fade.max(1e-6) / frames as f32;
        }
    }
}

fn step_for(rate: u32, out_rate: u32, pitch: f32) -> u64 {
    let base = ((rate as u64) << 32) / out_rate.max(1) as u64;
    (base as f64 * pitch.clamp(0.05, 8.0) as f64) as u64
}

fn lpf_a(amount: f32, out_rate: u32) -> f32 {
    if amount <= 0.0 {
        1.0
    } else {
        fx::lpf_coef(fx::lpf_cutoff(amount), out_rate)
    }
}

pub struct Mixer {
    bank: Arc<Bank>,
    shared: Option<SharedSet>,
    set: Option<Arc<SoundSet>>,
    out_rate: u32,
    voices: [Option<Voice>; MAX_VOICES],
    serial: u64,
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
    layer: f32,
    layer_target: f32,
    layer_step: f32,
    duck: [f32; MAX_GROUPS],
    duck_target: [f32; MAX_GROUPS],
    duck_rate: [(f32, f32); MAX_GROUPS],
    delay: Delay,
    reverb: Reverb,
    send: Vec<f32>,
    clock: Option<MusicClock>,
    output_gain: f32,
    limiter: Option<fx::Limiter>,
}

/// Bank sounds plus the course set, by id.
fn sound<'a>(bank: &'a Bank, set: Option<&'a SoundSet>, id: u16) -> Option<&'a Sound> {
    if id >= SET_BASE {
        set?.sounds.get((id - SET_BASE) as usize)
    } else {
        bank.sounds.get(id as usize)
    }
}

impl Mixer {
    pub fn new(bank: Arc<Bank>, out_rate: u32) -> Self {
        Self {
            bank,
            shared: None,
            set: None,
            out_rate,
            voices: [None; MAX_VOICES],
            serial: 0,
            master: 1.0,
            music: 1.0,
            sfx: 1.0,
            layer: 0.0,
            layer_target: 0.0,
            layer_step: 0.0,
            duck: [1.0; MAX_GROUPS],
            duck_target: [1.0; MAX_GROUPS],
            duck_rate: [(0.0, 0.0); MAX_GROUPS],
            delay: Delay::new(out_rate),
            reverb: Reverb::new(out_rate),
            send: vec![0.0; BLOCK * 2],
            clock: None,
            output_gain: 1.0,
            limiter: None,
        }
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    /// Share the per-course sound set slot with the game thread.
    pub fn set_shared(&mut self, shared: SharedSet) {
        self.shared = Some(shared);
    }

    /// Scale the final mix by `gain` (through the final-mix limiter if `limit`).
    pub fn set_output(&mut self, gain: f32, limit: bool) {
        self.output_gain = gain;
        self.limiter = limit.then(|| fx::Limiter::new(self.out_rate));
    }

    /// Publish the music position here after every render.
    pub fn set_clock(&mut self, clock: MusicClock) {
        self.clock = Some(clock);
    }

    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    /// Number of voices currently sounding (including ones fading out or waiting to start).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().flatten().count()
    }

    /// True if a voice is playing `sound` and not fading out.
    pub fn is_playing(&self, sound: u16) -> bool {
        self.voices
            .iter()
            .flatten()
            .any(|v| v.sound == sound && !v.stopping())
    }

    /// The sound in the music voice that is not fading out, if any.
    pub fn current_music(&self) -> Option<u16> {
        self.voices
            .iter()
            .flatten()
            .find(|v| v.bus == Bus::Music && !v.stopping())
            .map(|v| v.sound)
    }

    /// Apply every queued command.
    pub fn drain(&mut self, rx: &Receiver<Command>) {
        while let Ok(c) = rx.try_recv() {
            self.apply(c);
        }
    }

    fn group(&self, g: u16) -> Option<&GroupDef> {
        self.set.as_ref()?.groups.get(g as usize)
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::PlaySfx { sound } => {
                let Some(s) = self.bank.sounds.get(sound as usize) else {
                    return;
                };
                let group = s.group;
                let gain = s.gain;
                let bank = self.bank.clone();
                self.fade_where(STOP_FADE, |v| {
                    v.bus == Bus::Sfx
                        && v.sound < SET_BASE
                        && (v.sound == sound
                            || (group != 0
                                && bank.sounds.get(v.sound as usize).map(|s| s.group)
                                    == Some(group)))
                });
                self.start(Play {
                    volume: gain,
                    send: 0.0,
                    ..Play::sfx(sound)
                });
            }
            Command::StopSfx { sound, fade } => {
                self.fade_where(fade, |v| v.bus == Bus::Sfx && v.sound == sound)
            }
            Command::PlayMusic { sound, then } => {
                let gain = sound_gain(&self.bank, self.set.as_deref(), sound);
                self.apply(Command::Play(Play {
                    then,
                    volume: gain,
                    ..Play::music(sound)
                }))
            }
            Command::Play(p) => {
                if sound(&self.bank, self.set.as_deref(), p.sound).is_none() {
                    return;
                }
                if p.bus == Bus::Music {
                    self.fade_where(STOP_FADE, |v| v.bus == Bus::Music);
                } else if !self.admit(p.group, p.priority) {
                    return;
                }
                self.start(p);
            }
            Command::StopHandle { handle, fade } => {
                if handle != 0 {
                    for v in self.voices.iter_mut().flatten() {
                        if v.handle == handle {
                            let f = if fade == 0 {
                                v.stop_fade.max(STOP_FADE)
                            } else {
                                fade
                            };
                            v.stop(f);
                        }
                    }
                }
            }
            Command::SetHandle {
                handle,
                volume,
                pan,
                lpf,
            } => {
                let a = lpf_a(lpf, self.out_rate);
                for v in self.voices.iter_mut().flatten() {
                    if handle != 0 && v.handle == handle {
                        v.gain = volume;
                        v.pan = pan.clamp(-1.0, 1.0);
                        v.lpf_a = a;
                    }
                }
            }
            Command::StopMusic { fade } => self.fade_where(fade, |v| v.bus == Bus::Music),
            Command::StopAll => self.fade_where(STOP_FADE, |_| true),
            Command::SetVolumes { master, music, sfx } => {
                self.master = master.clamp(0.0, 1.0);
                self.music = music.clamp(0.0, 1.0);
                self.sfx = sfx.clamp(0.0, 1.0);
            }
            Command::SetMusicLayer { gain, frames } => {
                self.layer_target = gain.clamp(0.0, 2.0);
                if frames == 0 {
                    self.layer = self.layer_target;
                    self.layer_step = 0.0;
                } else {
                    self.layer_step = (self.layer_target - self.layer).abs() / frames as f32;
                }
            }
            Command::SetEffects { delay, reverb } => {
                self.delay.set(delay, self.out_rate);
                self.reverb.set(reverb);
            }
            Command::LoadSet => {
                // never block the audio thread: if the game thread holds the lock, keep the old
                let next = match self.shared.as_ref().map(|s| s.try_lock()) {
                    Some(Ok(g)) => g.clone(),
                    _ => return,
                };
                // voices of a set sound the new set no longer has at the same id stop (a set
                // that only grew keeps them playing)
                let same = |id: u16| {
                    let i = (id - SET_BASE) as usize;
                    match (
                        self.set.as_ref().and_then(|s| s.sounds.get(i)),
                        next.as_ref().and_then(|s| s.sounds.get(i)),
                    ) {
                        (Some(a), Some(b)) => same_data(a, b),
                        _ => false,
                    }
                };
                let keep: [bool; MAX_VOICES] = std::array::from_fn(|k| {
                    self.voices[k].is_none_or(|v| v.sound < SET_BASE || same(v.sound))
                });
                for (v, keep) in self.voices.iter_mut().zip(keep) {
                    if !keep {
                        *v = None;
                    }
                }
                let groups_changed = match (&self.set, &next) {
                    (Some(a), Some(b)) => a.groups.len() != b.groups.len(),
                    _ => true,
                };
                if groups_changed {
                    self.duck = [1.0; MAX_GROUPS];
                    self.duck_target = [1.0; MAX_GROUPS];
                }
                // the game thread keeps the previous set alive, so no memory is freed here
                self.set = next;
            }
        }
    }

    /// Room for a voice of `group` at `priority`: every limited group on its chain with no room
    /// left gives up its lowest-priority, oldest voice if the new one ranks at least as high
    /// (limit type 2; the other limit types need a strictly higher priority [inf]).
    fn admit(&mut self, group: u16, priority: f32) -> bool {
        let Some(set) = self.set.clone() else {
            return true;
        };
        let Some(g) = set.groups.get(group as usize) else {
            return true;
        };
        for &level in &g.chain {
            let Some(def) = set.groups.get(level as usize) else {
                continue;
            };
            let Some(limit) = def.limit else { continue };
            let in_level = |v: &Voice| {
                !v.stopping()
                    && set
                        .groups
                        .get(v.group as usize)
                        .is_some_and(|d| d.chain.contains(&level))
            };
            let count = self.voices.iter().flatten().filter(|v| in_level(v)).count();
            if count < limit as usize {
                continue;
            }
            if limit == 0 {
                return false;
            }
            let victim = (0..MAX_VOICES)
                .filter(|&i| self.voices[i].as_ref().is_some_and(in_level))
                .min_by(|&a, &b| {
                    let (va, vb) = (self.voices[a].unwrap(), self.voices[b].unwrap());
                    va.priority
                        .total_cmp(&vb.priority)
                        .then(va.serial.cmp(&vb.serial))
                });
            let Some(i) = victim else { continue };
            let vp = self.voices[i].map(|v| v.priority).unwrap_or(0.0);
            let ok = if def.limit_type == 2 {
                priority >= vp
            } else {
                priority > vp
            };
            if !ok {
                return false;
            }
            if let Some(v) = &mut self.voices[i] {
                v.stop(STOP_FADE);
            }
        }
        true
    }

    fn fade_where(&mut self, fade: u32, pred: impl Fn(&Voice) -> bool) {
        for slot in &mut self.voices {
            if let Some(v) = slot
                && pred(v)
            {
                if fade == 0 {
                    *slot = None;
                } else {
                    v.stop(fade);
                }
            }
        }
    }

    fn start(&mut self, p: Play) {
        let Some(s) = sound(&self.bank, self.set.as_deref(), p.sound) else {
            return;
        };
        let rate = s.rate;
        let start = p.start.min(s.frames.saturating_sub(1));
        self.serial += 1;
        let group_gain = self
            .group(p.group)
            .map(|g| {
                g.chain
                    .iter()
                    .filter_map(|&c| self.group(c).map(|d| d.volume))
                    .product::<f32>()
            })
            .unwrap_or(1.0);
        let voice = Voice {
            sound: p.sound,
            bus: p.bus,
            pos: (start as u64) << 32,
            step: step_for(rate, self.out_rate, p.pitch),
            pitch: p.pitch,
            fade: if p.fade_in > 0 { 0.0 } else { 1.0 },
            fade_step: if p.fade_in > 0 {
                1.0 / p.fade_in as f32
            } else {
                0.0
            },
            next: p.then,
            serial: self.serial,
            gain: p.volume * group_gain,
            pan: p.pan.clamp(-1.0, 1.0),
            lpf_a: lpf_a(p.lpf, self.out_rate),
            lp: [0.0; 2],
            delay: p.delay,
            remaining: p.duration,
            stop_fade: p.stop_fade,
            group: p.group,
            priority: p.priority,
            send: p.send,
            handle: p.handle,
            cursor: Cursor::default(),
        };
        let slot = match self.voices.iter().position(Option::is_none) {
            Some(i) => i,
            // steal: the oldest fading voice, else the oldest sound effect, else the oldest voice
            None => {
                let key = |v: &Voice| (!v.stopping(), v.bus == Bus::Music, v.serial);
                (0..MAX_VOICES)
                    .min_by_key(|&i| self.voices[i].as_ref().map(key))
                    .unwrap_or(0)
            }
        };
        self.voices[slot] = Some(voice);
    }

    /// Ducking targets from the voices now sounding, then a step toward them.
    fn update_ducking(&mut self, frames: usize) {
        let Some(set) = self.set.clone() else { return };
        let n = set.groups.len().min(MAX_GROUPS);
        let mut target = [1.0f32; MAX_GROUPS];
        let mut rate = self.duck_rate;
        for v in self.voices.iter().flatten() {
            if v.stopping() || v.delay > 0 {
                continue;
            }
            let Some(g) = set.groups.get(v.group as usize) else {
                continue;
            };
            for &(t, vol, fin, fout) in &g.duck {
                let t = t as usize;
                if t < n && vol < target[t] {
                    target[t] = vol;
                    rate[t] = (fin, fout);
                }
            }
        }
        let sec = frames as f32 / self.out_rate as f32;
        for g in 0..n {
            let (fin, fout) = rate[g];
            let cur = self.duck[g];
            let t = target[g];
            let time = if t < cur { fin } else { fout };
            self.duck[g] = if time <= 0.0 {
                t
            } else {
                let d = sec / time;
                if t < cur {
                    (cur - d).max(t)
                } else {
                    (cur + d).min(t)
                }
            };
            self.duck_target[g] = t;
        }
        self.duck_rate = rate;
    }

    fn duck_of(&self, group: u16) -> f32 {
        match self.group(group) {
            Some(g) => g
                .chain
                .iter()
                .map(|&c| self.duck.get(c as usize).copied().unwrap_or(1.0))
                .product(),
            None => 1.0,
        }
    }

    /// Mix `out.len() / 2` stereo frames (interleaved L, R) into `out`, replacing its contents.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(BLOCK);
            self.render_block(&mut out[2 * done..2 * (done + n)]);
            done += n;
        }
        match &mut self.limiter {
            Some(l) => l.process(out, self.output_gain),
            None if self.output_gain != 1.0 => out.iter_mut().for_each(|s| *s *= self.output_gain),
            None => {}
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        if let Some(c) = &self.clock {
            let v = self
                .voices
                .iter()
                .flatten()
                .find(|v| v.bus == Bus::Music && !v.stopping())
                .map(|v| (v.sound as u64) << 32 | (v.pos >> 32).min(u32::MAX as u64))
                .unwrap_or(u64::MAX);
            c.store(v, Ordering::Relaxed);
        }
    }

    fn render_block(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        self.update_ducking(frames);
        // music layer ramp (per block)
        if self.layer != self.layer_target {
            let d = self.layer_step * frames as f32;
            self.layer = if self.layer < self.layer_target {
                (self.layer + d).min(self.layer_target)
            } else {
                (self.layer - d).max(self.layer_target)
            };
        }
        let bank = self.bank.clone();
        let set = self.set.clone();
        let mut ducks = [1.0f32; MAX_VOICES];
        for (i, v) in self.voices.iter().enumerate() {
            if let Some(v) = v {
                ducks[i] = self.duck_of(v.group);
            }
        }
        let layer = self.layer;
        let (master, music, sfx, out_rate) = (self.master, self.music, self.sfx, self.out_rate);
        let send_any = self.delay.active() || self.reverb.active();
        let send = &mut self.send[..frames * 2];
        send.fill(0.0);
        for (i, slot) in self.voices.iter_mut().enumerate() {
            let Some(v) = slot else { continue };
            let bus_gain = match v.bus {
                Bus::Music => music,
                Bus::Sfx => sfx,
            } * master
                * ducks[i];
            let ctx = Ctx {
                bank: &bank,
                set: set.as_deref(),
                out_rate,
                layer: if v.bus == Bus::Music { layer } else { 0.0 },
            };
            let s = if send_any && v.send > 0.0 {
                Some(&mut *send)
            } else {
                None
            };
            if !render_voice(&ctx, v, bus_gain, out, s) {
                *slot = None;
            }
        }
        if send_any {
            let send = &self.send[..frames * 2];
            self.delay.process(send, out);
            self.reverb.process(send, out);
        }
    }
}

fn same_data(a: &Sound, b: &Sound) -> bool {
    match (&a.data, &b.data) {
        (Samples::Pcm(x), Samples::Pcm(y)) => Arc::ptr_eq(x, y),
        (Samples::Adpcm(x), Samples::Adpcm(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

fn sound_gain(bank: &Bank, set: Option<&SoundSet>, id: u16) -> f32 {
    sound(bank, set, id).map(|s| s.gain).unwrap_or(1.0)
}

struct Ctx<'a> {
    bank: &'a Bank,
    set: Option<&'a SoundSet>,
    out_rate: u32,
    layer: f32,
}

/// Sample `i` of channel `c`.
#[inline]
fn fetch(s: &Sound, cur: &mut Cursor, i: u32, c: usize) -> f32 {
    match &s.data {
        Samples::Pcm(d) => {
            let ch = s.channels.max(1) as usize;
            d.get(i as usize * ch + c.min(ch - 1)).copied().unwrap_or(0) as f32
        }
        Samples::Adpcm(a) => cur.sample(a, i, c.min(a.channels().saturating_sub(1))) as f32,
    }
}

/// Render one voice; false when it has finished.
fn render_voice(
    ctx: &Ctx,
    v: &mut Voice,
    bus_gain: f32,
    out: &mut [f32],
    mut send: Option<&mut [f32]>,
) -> bool {
    const SCALE: f32 = 1.0 / 32768.0;
    let Some(mut s) = sound(ctx.bank, ctx.set, v.sound) else {
        return false;
    };
    let frames = out.len() / 2;
    let mut f = 0;
    if v.delay > 0 {
        let skip = (v.delay as usize).min(frames);
        v.delay -= skip as u32;
        f = skip;
    }
    let (lg, rg) = ((1.0 - v.pan).min(1.0), (1.0 + v.pan).min(1.0));
    while f < frames {
        let mut i = (v.pos >> 32) as u32;
        match s.loop_range {
            Some((ls, le)) if i >= le => {
                let back = (i - le) % (le - ls).max(1);
                v.pos = ((ls + back) as u64) << 32 | (v.pos & 0xFFFF_FFFF);
                i = (v.pos >> 32) as u32;
            }
            None if i >= s.frames => {
                // finished: continue with the chained sound, if any
                match v
                    .next
                    .take()
                    .and_then(|n| sound(ctx.bank, ctx.set, n).map(|s| (n, s)))
                {
                    Some((n, ns)) => {
                        v.sound = n;
                        v.pos = 0;
                        s = ns;
                        v.step = step_for(s.rate, ctx.out_rate, v.pitch);
                        v.cursor = Cursor::default();
                        continue;
                    }
                    None => return false,
                }
            }
            _ => {}
        }
        if let Samples::Adpcm(a) = &s.data {
            v.cursor.at(a, i);
        }
        // the frame after i, wrapping at the loop end; silence after the last frame
        let j = match s.loop_range {
            Some((ls, le)) if i + 1 >= le => Some(ls),
            _ if i + 1 >= s.frames => None,
            _ => Some(i + 1),
        };
        let t = (v.pos & 0xFFFF_FFFF) as f32 * (1.0 / 4_294_967_296.0);
        let ch = s.channels.max(1) as usize;
        let mut lerp = |c: usize| -> f32 {
            let a = fetch(s, &mut v.cursor, i, c);
            let b = match j {
                // the loop start sits in another frame: read it without moving the cursor
                Some(k) if k != i + 1 => {
                    let mut tmp = Cursor::default();
                    fetch(s, &mut tmp, k, c)
                }
                Some(k) => fetch(s, &mut v.cursor, k, c),
                None => 0.0,
            };
            a + (b - a) * t
        };
        let (mut l, mut r) = if ch == 1 {
            let m = lerp(0);
            (m, m)
        } else {
            (lerp(0), lerp(1))
        };
        if ch >= 4 && ctx.layer > 0.0 {
            l += lerp(2) * ctx.layer;
            r += lerp(3) * ctx.layer;
        }
        if v.lpf_a < 1.0 {
            v.lp[0] += v.lpf_a * (l - v.lp[0]);
            v.lp[1] += v.lpf_a * (r - v.lp[1]);
            l = v.lp[0];
            r = v.lp[1];
        }
        let g = v.gain * bus_gain * v.fade * SCALE;
        let (ol, or) = (l * g * lg, r * g * rg);
        out[2 * f] += ol;
        out[2 * f + 1] += or;
        if let Some(sb) = send.as_deref_mut() {
            sb[2 * f] += ol * v.send;
            sb[2 * f + 1] += or * v.send;
        }
        if v.fade_step != 0.0 {
            v.fade += v.fade_step;
            if v.fade <= 0.0 {
                return false;
            }
            if v.fade >= 1.0 && v.fade_step > 0.0 {
                v.fade = 1.0;
                v.fade_step = 0.0;
            }
        }
        if v.remaining > 0 {
            v.remaining -= 1;
            if v.remaining == 0 {
                let fade = if v.stop_fade > 0 {
                    v.stop_fade
                } else {
                    STOP_FADE
                };
                v.stop(fade);
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

    fn left(out: &[f32]) -> Vec<i32> {
        out.iter()
            .step_by(2)
            .map(|s| (s * 32768.0).round() as i32)
            .collect()
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
    fn resamples_by_rate_ratio_and_pitch() {
        // 24 kHz ramp played at 48 kHz: every other output frame is an interpolated midpoint
        let data: Vec<i16> = (0..100).map(|i| i * 100).collect();
        let mut m = Mixer::new(
            bank(vec![Sound::from_pcm("a", 24000, 1, data.clone())]),
            48000,
        );
        m.apply(Command::PlaySfx { sound: 0 });
        assert_eq!(
            left(&render(&mut m, 8)),
            vec![0, 50, 100, 150, 200, 250, 300, 350]
        );
        // pitch 2: the same sound at its own rate
        let mut m = Mixer::new(bank(vec![Sound::from_pcm("a", 24000, 1, data)]), 48000);
        m.apply(Command::Play(Play {
            pitch: 2.0,
            ..Play::sfx(0)
        }));
        assert_eq!(left(&render(&mut m, 4)), vec![0, 100, 200, 300]);
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
        let got = left(&render(&mut m, 10));
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
    fn volumes_gain_and_pan_scale_output() {
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
        m.apply(Command::StopAll);
        render(&mut m, 200);
        m.apply(Command::SetVolumes {
            master: 1.0,
            music: 1.0,
            sfx: 1.0,
        });
        m.apply(Command::Play(Play {
            pan: 0.5,
            volume: 0.5,
            ..Play::sfx(0)
        }));
        let out = render(&mut m, 1);
        assert!((out[0] - 0.125).abs() < 1e-6, "left at half");
        assert!((out[1] - 0.25).abs() < 1e-6, "right at full");
    }

    #[test]
    fn delay_duration_and_fade_in() {
        let s = Sound::from_pcm("a", 48000, 1, vec![8192; 4000]);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        m.apply(Command::Play(Play {
            delay: 5,
            duration: 10,
            stop_fade: 4,
            ..Play::sfx(0)
        }));
        let out = left(&render(&mut m, 30));
        assert!(out[..5].iter().all(|&v| v == 0), "waits 5 frames");
        assert_eq!(out[5], 8192);
        assert_eq!(out[14], 8192);
        assert!(out[16] < 8192 && out[16] > 0, "fading after 10 frames");
        assert!(out[20..].iter().all(|&v| v == 0));
        assert_eq!(m.active_voices(), 0);
        m.apply(Command::Play(Play {
            fade_in: 4,
            ..Play::sfx(0)
        }));
        let out = left(&render(&mut m, 6));
        assert_eq!(out[0], 0);
        assert!(out[1] < out[2] && out[2] < out[3]);
        assert_eq!(out[5], 8192);
    }

    fn set_with_groups(n: usize) -> (SharedSet, Arc<Bank>) {
        let sounds: Vec<Sound> = (0..n)
            .map(|i| Sound::from_pcm(&format!("s{i}"), 48000, 1, vec![1000; 100_000]))
            .collect();
        // 0 root folder (limit 3); 1 "sqr" (limit 1, ducks 2 to 0.5 instantly); 2 "bgm"
        let groups = vec![
            GroupDef {
                chain: vec![0],
                volume: 1.0,
                limit: Some(3),
                limit_type: 2,
                duck: vec![],
            },
            GroupDef {
                chain: vec![1, 0],
                volume: 1.0,
                limit: Some(1),
                limit_type: 2,
                duck: vec![(2, 0.5, 0.0, 0.0)],
            },
            GroupDef {
                chain: vec![2, 0],
                volume: 0.5,
                limit: None,
                limit_type: 0,
                duck: vec![],
            },
        ];
        let shared = shared_set();
        *shared.lock().unwrap() = Some(Arc::new(SoundSet { sounds, groups }));
        (shared, bank(vec![]))
    }

    #[test]
    fn group_limits_steal_by_priority_and_folders() {
        let (shared, b) = set_with_groups(6);
        let mut m = Mixer::new(b, 48000);
        m.set_shared(shared);
        m.apply(Command::LoadSet);
        let sqr = |k: u16, prio: f32| {
            Command::Play(Play {
                group: 1,
                priority: prio,
                ..Play::sfx(SET_BASE + k)
            })
        };
        m.apply(sqr(0, 0.65));
        m.apply(sqr(1, 0.65));
        assert!(
            !m.is_playing(SET_BASE) && m.is_playing(SET_BASE + 1),
            "equal priority steals"
        );
        m.apply(Command::Play(Play {
            group: 1,
            priority: 1.0,
            ..Play::sfx(SET_BASE + 2)
        }));
        m.apply(sqr(3, 0.65));
        assert!(
            m.is_playing(SET_BASE + 2) && !m.is_playing(SET_BASE + 3),
            "lower priority is refused"
        );
        // the root folder holds 3: two more in group 2 fill it, a third steals the oldest
        for k in [3u16, 4, 5] {
            m.apply(Command::Play(Play {
                group: 2,
                ..Play::sfx(SET_BASE + k)
            }));
        }
        let playing = (0..6).filter(|&k| m.is_playing(SET_BASE + k)).count();
        assert_eq!(playing, 3);
    }

    #[test]
    fn ducking_and_group_volume() {
        let (shared, b) = set_with_groups(2);
        let mut m = Mixer::new(b, 48000);
        m.set_shared(shared);
        m.apply(Command::LoadSet);
        m.apply(Command::Play(Play {
            group: 2,
            ..Play::sfx(SET_BASE)
        }));
        let out = left(&render(&mut m, 4));
        assert_eq!(out[3], 500, "group volume 0.5");
        m.apply(Command::Play(Play {
            group: 1,
            volume: 0.0,
            ..Play::sfx(SET_BASE + 1)
        }));
        let out = left(&render(&mut m, 4));
        assert_eq!(out[3], 250, "ducked to half while group 1 plays");
    }

    #[test]
    fn handles_change_and_stop_voices() {
        let s = Sound::from_pcm("a", 48000, 1, vec![8192; 100_000]).with_loop(0, 100_000);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        m.apply(Command::Play(Play {
            handle: 7,
            ..Play::sfx(0)
        }));
        m.apply(Command::SetHandle {
            handle: 7,
            volume: 0.5,
            pan: 0.0,
            lpf: 0.0,
        });
        let out = left(&render(&mut m, 2));
        assert_eq!(out[1], 4096);
        m.apply(Command::StopHandle { handle: 7, fade: 8 });
        render(&mut m, 20);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn four_channel_music_layer_and_clock() {
        // channels: base L, base R, layer L, layer R
        let data: Vec<i16> = (0..100).flat_map(|_| [1000, 1000, 500, 500]).collect();
        let s = Sound::from_pcm("m", 48000, 4, data).with_loop(0, 100);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        let clock: MusicClock = Arc::new(AtomicU64::new(u64::MAX));
        m.set_clock(clock.clone());
        m.apply(Command::PlayMusic {
            sound: 0,
            then: None,
        });
        assert_eq!(left(&render(&mut m, 2))[1], 1000);
        m.apply(Command::SetMusicLayer {
            gain: 1.0,
            frames: 0,
        });
        assert_eq!(left(&render(&mut m, 2))[1], 1500);
        assert_eq!(clock.load(Ordering::Relaxed), 4);
        m.apply(Command::Play(Play {
            start: 50,
            ..Play::music(0)
        }));
        render(&mut m, 200);
        assert_eq!(
            clock.load(Ordering::Relaxed) & 0xFFFF_FFFF,
            (50 + 200) % 100
        );
    }

    #[test]
    fn effects_add_a_wet_signal_to_sent_voices() {
        let s = Sound::from_pcm("a", 48000, 1, vec![16000; 10]);
        let mut m = Mixer::new(bank(vec![s]), 48000);
        m.apply(Command::SetEffects {
            delay: Some(DelayParams {
                time: 0.01,
                feedback: 0.0,
                lpf: 0.0,
                gain: 0.5,
            }),
            reverb: None,
        });
        m.apply(Command::Play(Play::sfx(0)));
        let out = left(&render(&mut m, 1000));
        assert!(
            out[480..490].iter().all(|&v| (v - 8000).abs() <= 1),
            "echo after 10 ms"
        );
    }

    #[test]
    fn output_is_clamped_and_voices_are_stolen() {
        let s = Sound::from_pcm("loud", 48000, 1, vec![32767; 100_000]);
        let bank = bank((0..60).map(|i| s.clone().tap_name(i)).collect());
        let mut m = Mixer::new(bank, 48000);
        for i in 0..60 {
            m.apply(Command::PlaySfx { sound: i });
        }
        assert_eq!(m.active_voices(), MAX_VOICES);
        assert!(m.is_playing(59), "newest sound got a voice");
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
