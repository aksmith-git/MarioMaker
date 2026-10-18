//! The front end's UI sounds (`mmk_front::SoundCue`: WAVs of the SMM2 UI import's
//! `ui/sound/`), mixed into the same output stream as the game's sound: a small voice list of
//! 16-bit PCM clips with a gain and a start delay, resampled linearly to the device rate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A decoded clip: interleaved stereo at `rate` Hz.
#[derive(Clone, Debug)]
pub struct Clip {
    pub rate: u32,
    pub frames: Vec<[f32; 2]>,
}

/// Parse a PCM WAV (8/16-bit, mono or stereo).
pub fn parse_wav(b: &[u8]) -> Option<Clip> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut channels, mut rate, mut bits) = (0u16, 0u32, 0u16);
    let mut pos = 12;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let n = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = b.get(pos + 8..(pos + 8 + n).min(b.len()))?;
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if channels == 0 || rate == 0 {
                return None;
            }
            let ch = channels as usize;
            let sample = |i: usize| -> f32 {
                match bits {
                    8 => (body.get(i).copied().unwrap_or(128) as f32 - 128.0) / 128.0,
                    _ => {
                        let o = i * 2;
                        match body.get(o..o + 2) {
                            Some(s) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                            None => 0.0,
                        }
                    }
                }
            };
            let per = if bits == 8 { 1 } else { 2 };
            let count = body.len() / per / ch;
            let frames = (0..count)
                .map(|f| {
                    let l = sample(f * ch);
                    let r = if ch > 1 { sample(f * ch + 1) } else { l };
                    [l, r]
                })
                .collect();
            return Some(Clip { rate, frames });
        }
        pos += 8 + n + (n & 1);
    }
    None
}

/// Where clips come from without a folder: a WAV's bytes by name, or None while they are not
/// there (yet: the web build reads them from the browser's storage after starting).
pub type ClipReader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

struct Voice {
    clip: Arc<Clip>,
    pos: f64,
    step: f64,
    gain: f32,
    /// Output frames to wait before starting.
    delay: u64,
}

/// The UI sound mixer, shared between the game thread (`play`) and the audio callback
/// (`mix`).
pub struct UiMixer {
    rate: u32,
    voices: Mutex<Vec<Voice>>,
    clips: Mutex<HashMap<String, Option<Arc<Clip>>>>,
    dir: Option<std::path::PathBuf>,
    reader: Option<ClipReader>,
    /// Master × effects volume.
    pub volume: f32,
}

impl UiMixer {
    /// Clips are read from `dir` (`<ui import>/ui/sound`) on first use.
    pub fn new(dir: Option<std::path::PathBuf>, rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            voices: Mutex::new(Vec::new()),
            clips: Mutex::new(HashMap::new()),
            dir,
            reader: None,
            volume: 1.0,
        }
    }

    /// Clips from `reader` (asked again until it has them).
    pub fn with_reader(reader: ClipReader, rate: u32) -> Self {
        Self {
            reader: Some(reader),
            ..Self::new(None, rate)
        }
    }

    fn clip(&self, name: &str) -> Option<Arc<Clip>> {
        let mut clips = self.clips.lock().ok()?;
        if let Some(c) = clips.get(name) {
            return c.clone();
        }
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return None;
        }
        let bytes = match (&self.dir, &self.reader) {
            (Some(dir), _) => std::fs::read(dir.join(format!("{name}.wav"))).ok(),
            // not there yet: ask again next time
            (None, Some(read)) => Some(read(name)?),
            (None, None) => None,
        };
        let clip = bytes.and_then(|b| parse_wav(&b)).map(Arc::new);
        clips.insert(name.to_string(), clip.clone());
        clip
    }

    /// Start `cue` (its delay counts from now).
    pub fn play(&self, cue: &mmk_front::SoundCue) {
        if std::env::var("MMK_DEBUG_SOUND").as_deref() == Ok("1") {
            crate::app::log(&format!(
                "UISound {} volume {} delay {}",
                cue.name, cue.volume, cue.delay
            ));
        }
        let Some(clip) = self.clip(&cue.name) else {
            return;
        };
        let step = clip.rate as f64 / self.rate as f64;
        let delay = (cue.delay.max(0.0) as f64 / 60.0 * self.rate as f64) as u64;
        if let Ok(mut v) = self.voices.lock() {
            v.push(Voice {
                clip,
                pos: 0.0,
                step,
                gain: cue.volume * self.volume,
                delay,
            });
        }
    }

    /// Add the voices into interleaved stereo `out`.
    pub fn mix(&self, out: &mut [f32]) {
        let Ok(mut voices) = self.voices.lock() else {
            return;
        };
        for v in voices.iter_mut() {
            for frame in out.as_chunks_mut::<2>().0 {
                if v.delay > 0 {
                    v.delay -= 1;
                    continue;
                }
                let i = v.pos as usize;
                let Some(a) = v.clip.frames.get(i) else {
                    break;
                };
                let b = v.clip.frames.get(i + 1).unwrap_or(a);
                let t = (v.pos - i as f64) as f32;
                frame[0] += (a[0] + (b[0] - a[0]) * t) * v.gain;
                frame[1] += (a[1] + (b[1] - a[1]) * t) * v.gain;
                v.pos += v.step;
            }
        }
        voices.retain(|v| v.delay > 0 || (v.pos as usize) < v.clip.frames.len());
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

/// A running output stream carrying the game's sound (when the asset cache has a sound bank)
/// and the UI sounds.
pub struct FrontAudio {
    _stream: cpal::Stream,
    pub game: Option<Arc<mmk_host::audio::AudioOut>>,
    pub ui: Arc<UiMixer>,
}

/// Open the default output with the game's `bank` (optional) and the UI sounds of `ui_dir`.
pub fn start(
    bank: Option<mmk_audio::Bank>,
    music: Option<mmk_host::audio::MusicReader>,
    ui_dir: Option<std::path::PathBuf>,
) -> Result<FrontAudio, String> {
    type Slot = Arc<std::sync::OnceLock<(Option<Arc<mmk_host::audio::AudioOut>>, Arc<UiMixer>)>>;
    let slot: Slot = Arc::default();
    let feed = slot.clone();
    let output = crate::audio_out::open(move |buf| match feed.get() {
        Some((game, ui)) => {
            match game {
                Some(g) => g.render(buf),
                None => buf.fill(0.0),
            }
            ui.mix(buf);
            for s in buf.iter_mut() {
                *s = s.clamp(-1.0, 1.0);
            }
        }
        None => buf.fill(0.0),
    })?;
    let game = bank.map(|b| {
        let mut out = mmk_host::audio::AudioOut::with_rate(b, output.rate);
        if let Some(m) = music {
            out = out.with_music(m);
        }
        Arc::new(out)
    });
    let ui = Arc::new(UiMixer::new(ui_dir, output.rate));
    let _ = slot.set((game.clone(), ui.clone()));
    Ok(FrontAudio {
        _stream: output.stream,
        game,
        ui,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            b.extend(s.to_le_bytes());
        }
        b
    }

    #[test]
    fn clips_mix_with_gain_and_delay() {
        let c = parse_wav(&wav(100, &[16384, 16384, 16384, 16384])).unwrap();
        assert_eq!((c.rate, c.frames.len()), (100, 4));
        assert!((c.frames[0][1] - 0.5).abs() < 1e-6);
        let m = UiMixer::new(None, 100);
        m.voices.lock().unwrap().push(Voice {
            clip: Arc::new(c),
            pos: 0.0,
            step: 1.0,
            gain: 0.5,
            delay: 2,
        });
        let mut out = vec![0.0f32; 16];
        m.mix(&mut out);
        assert_eq!(out[0], 0.0);
        assert!((out[4] - 0.25).abs() < 1e-6);
        assert_eq!(out[12], 0.0);
        assert!(m.voices.lock().unwrap().is_empty());
    }
}
