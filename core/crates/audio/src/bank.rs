//! Sounds, loaded from an asset cache's `audio/` folder.
//!
//! `audio/index.json` (written by the importer; the fixed set of SMB1 sounds used when a cache
//! has no gameplay sound database):
//!
//! ```text
//! { "<sound id>": { "file": "<id>.bwav", "gain": 0.2, "group": 1, ... }, ... }
//! ```
//!
//! Each file is a BWAV. Short sounds are decoded to PCM16 once when loaded; long ones (music)
//! can stay DSP-ADPCM and be decoded while they play ([`Sound::from_bwav_streamed`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mmk_formats::audio::{Bwav, Codec, decode_interleaved};
use serde_json::Value;

use crate::stream::{Adpcm, MAX_CHANNELS};

#[derive(Debug, thiserror::Error)]
pub enum BankError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {detail}")]
    Invalid { path: PathBuf, detail: String },
}

/// A sound's samples: decoded PCM16 (interleaved), or DSP-ADPCM decoded while it plays.
#[derive(Clone, Debug)]
pub enum Samples {
    Pcm(Arc<[i16]>),
    Adpcm(Arc<Adpcm>),
}

/// One sound: up to four channels (a stereo pair, plus a second pair the mixer fades in as a
/// music layer).
#[derive(Clone, Debug)]
pub struct Sound {
    pub name: String,
    pub rate: u32,
    pub channels: u8,
    pub frames: u32,
    /// (start, end) in frames, end exclusive.
    pub loop_range: Option<(u32, u32)>,
    /// Linear gain applied when the sound plays.
    pub gain: f32,
    /// Exclusive group (0 = none): starting a sound stops the others in its group.
    pub group: u8,
    pub data: Samples,
}

impl Sound {
    /// A sound from interleaved PCM16 (tests and synthetic sounds).
    pub fn from_pcm(name: &str, rate: u32, channels: u8, data: Vec<i16>) -> Self {
        let frames = (data.len() / channels.max(1) as usize) as u32;
        Self {
            name: name.into(),
            rate,
            channels,
            frames,
            loop_range: None,
            gain: 1.0,
            group: 0,
            data: Samples::Pcm(data.into()),
        }
    }

    pub fn with_loop(mut self, start: u32, end: u32) -> Self {
        self.loop_range = Some((start, end.min(self.frames)));
        self
    }

    pub fn with_gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    pub fn with_group(mut self, group: u8) -> Self {
        self.group = group;
        self
    }

    /// Interleaved PCM data, if decoded.
    pub fn pcm(&self) -> Option<&[i16]> {
        match &self.data {
            Samples::Pcm(d) => Some(d),
            Samples::Adpcm(_) => None,
        }
    }

    /// Bytes of sample data held in memory.
    pub fn memory(&self) -> usize {
        match &self.data {
            Samples::Pcm(d) => d.len() * 2,
            Samples::Adpcm(a) => a.frames() as usize * a.channels() * 8 / 14,
        }
    }

    /// Decode a BWAV (all of its samples must be present) to PCM, up to four channels.
    pub fn from_bwav(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let w = Bwav::parse(bytes).map_err(|e| e.to_string())?;
        if !w.is_complete() {
            return Err("prefetch BWAV (samples missing)".into());
        }
        let (ch, frames, data) = decode_interleaved(&w, MAX_CHANNELS).map_err(|e| e.to_string())?;
        Ok(Self {
            name: name.into(),
            rate: w.sample_rate(),
            channels: ch as u8,
            frames: frames as u32,
            loop_range: w
                .loop_range()
                .map(|(s, e)| (s, e.min(frames as u32)))
                .filter(|(s, e)| s < e),
            gain: 1.0,
            group: 0,
            data: Samples::Pcm(data.into()),
        })
    }

    /// A BWAV kept compressed and decoded while it plays (every channel must be DSP-ADPCM,
    /// else it is decoded to PCM like [`Sound::from_bwav`]).
    pub fn from_bwav_streamed(name: &str, bytes: Arc<[u8]>) -> Result<Self, String> {
        let w = Bwav::parse(&bytes).map_err(|e| e.to_string())?;
        if !w.is_complete() {
            return Err("prefetch BWAV (samples missing)".into());
        }
        if w.channels.iter().any(|c| c.codec != Codec::DspAdpcm) {
            return Self::from_bwav(name, &bytes);
        }
        let base = bytes.as_ptr() as usize;
        let mut chans = Vec::new();
        for (i, c) in w.channels.iter().take(MAX_CHANNELS).enumerate() {
            let data = w.channel_data(i).map_err(|e| e.to_string())?;
            chans.push((
                data.as_ptr() as usize - base,
                data.len(),
                c.coefs,
                (c.hist1, c.hist2),
            ));
        }
        let frames = w.samples();
        let a = Adpcm::new(bytes.clone(), &chans, frames);
        Ok(Self {
            name: name.into(),
            rate: w.sample_rate(),
            channels: chans.len() as u8,
            frames,
            loop_range: w
                .loop_range()
                .map(|(s, e)| (s, e.min(frames)))
                .filter(|(s, e)| s < e),
            gain: 1.0,
            group: 0,
            data: Samples::Adpcm(Arc::new(a)),
        })
    }
}

/// All sounds of a cache, by index; names map to indices.
#[derive(Clone, Debug, Default)]
pub struct Bank {
    pub sounds: Vec<Sound>,
    names: HashMap<String, u16>,
}

impl Bank {
    pub fn new(sounds: Vec<Sound>) -> Self {
        let names = sounds
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), i as u16))
            .collect();
        Self { sounds, names }
    }

    pub fn id(&self, name: &str) -> Option<u16> {
        self.names.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    /// Load `<cache>/audio/index.json` and decode every sound it lists, in name order.
    pub fn load_cache(root: impl AsRef<Path>) -> Result<Self, BankError> {
        Self::load_cache_with(root, &|p| std::fs::read(p))
    }

    /// [`Bank::load_cache`] through `read` (full path → bytes) instead of the file system.
    pub fn load_cache_with(
        root: impl AsRef<Path>,
        read: &dyn Fn(&Path) -> std::io::Result<Vec<u8>>,
    ) -> Result<Self, BankError> {
        let dir = root.as_ref().join("audio");
        let index_path = dir.join("index.json");
        let raw = read(&index_path).map_err(|source| BankError::Io {
            path: index_path.clone(),
            source,
        })?;
        let index: Value = serde_json::from_slice(&raw).map_err(|e| BankError::Invalid {
            path: index_path.clone(),
            detail: e.to_string(),
        })?;
        let Value::Object(map) = index else {
            return Err(BankError::Invalid {
                path: index_path,
                detail: "not an object".into(),
            });
        };
        let mut names: Vec<&String> = map.keys().collect();
        names.sort();
        let mut sounds = Vec::new();
        for name in names {
            let entry = &map[name];
            let file = entry["file"].as_str().unwrap_or_default();
            let path = dir.join(file);
            let bytes = read(&path).map_err(|source| BankError::Io {
                path: path.clone(),
                source,
            })?;
            let mut s = Sound::from_bwav(name, &bytes)
                .map_err(|detail| BankError::Invalid { path, detail })?;
            s.gain = entry["gain"].as_f64().unwrap_or(1.0) as f32;
            s.group = entry["group"].as_u64().unwrap_or(0).min(255) as u8;
            sounds.push(s);
        }
        Ok(Self::new(sounds))
    }
}
