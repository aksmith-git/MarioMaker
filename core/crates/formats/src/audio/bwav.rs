//! BWAV: Nintendo's Switch wave container (one sound, one or more channels).
//!
//! Layout (little-endian), from inspection of the files themselves:
//!
//! ```text
//! 0x00  "BWAV"   u16 BOM (FF FE)   u16 version (1)
//! 0x08  u32 CRC32 of the sample data
//! 0x0C  u16 prefetch flag (1: only the first part of the samples is in this file)
//! 0x0E  u16 channel count
//! 0x10  channel info × count, 0x4C bytes each:
//!       +0x00 u16 codec (0 PCM16, 1 DSP-ADPCM)   +0x02 u16 pan (0 left, 1 right, 2 centre)
//!       +0x04 u32 sample rate                     +0x08 u32 sample count (whole sound)
//!       +0x0C u32 sample count in this file       +0x10 i16[16] DSP coefficients
//!       +0x30 u32 data offset in the full stream  +0x34 u32 data offset in this file
//!       +0x38 u32 (always 1 in the files seen)    +0x3C u32 loop end (0xFFFFFFFF: no loop)
//!       +0x40 u32 loop start                      +0x44 u16 initial predictor/scale
//!       +0x46 i16 history 1                       +0x48 i16 history 2   +0x4A u16 padding
//! ```
//!
//! Offsets are relative to the start of the BWAV (which matters for BWAVs embedded in a BARS).
//! The loop end is exclusive: playback runs to `loop_end - 1` and continues at `loop_start`.

use crate::audio::dsp::{self, DspState};
use crate::error::{Error, Result};

pub const HEADER_SIZE: usize = 0x10;
pub const CHANNEL_INFO_SIZE: usize = 0x4C;
const NO_LOOP: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Pcm16,
    DspAdpcm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub codec: Codec,
    pub pan: u16,
    pub sample_rate: u32,
    /// Samples of the whole sound.
    pub samples: u32,
    /// Samples present in this file (smaller than `samples` for a prefetch BWAV).
    pub samples_here: u32,
    pub coefs: [i16; 16],
    /// Offset of this channel's data, relative to the BWAV start.
    pub data_offset: u32,
    /// Loop as (start, end) in samples, end exclusive.
    pub loop_range: Option<(u32, u32)>,
    pub predictor_scale: u16,
    pub hist1: i16,
    pub hist2: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bwav<'a> {
    pub prefetch: bool,
    pub channels: Vec<Channel>,
    /// The BWAV bytes (from its magic to the end of the containing buffer).
    data: &'a [u8],
}

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BWAV",
        detail: detail.into(),
    }
}

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

impl<'a> Bwav<'a> {
    /// Parse a BWAV starting at `data[0]`. `data` may extend past the BWAV (e.g. the rest of a
    /// BARS); sample data is only read through [`Bwav::decode_channel`].
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE || &data[0..4] != b"BWAV" {
            return Err(Error::Magic { what: "BWAV" });
        }
        if u16_at(data, 4)? != 0xFEFF {
            return Err(bad("big-endian BWAV not supported"));
        }
        let prefetch = u16_at(data, 0x0C)? != 0;
        let count = u16_at(data, 0x0E)? as usize;
        if count == 0 {
            return Err(bad("no channels"));
        }
        let mut channels = Vec::with_capacity(count);
        for i in 0..count {
            let b = HEADER_SIZE + i * CHANNEL_INFO_SIZE;
            if data.len() < b + CHANNEL_INFO_SIZE {
                return Err(bad("truncated channel table"));
            }
            let codec = match u16_at(data, b)? {
                0 => Codec::Pcm16,
                1 => Codec::DspAdpcm,
                c => return Err(bad(format!("unknown codec {c}"))),
            };
            let mut coefs = [0i16; 16];
            for (k, c) in coefs.iter_mut().enumerate() {
                *c = u16_at(data, b + 0x10 + 2 * k)? as i16;
            }
            let loop_end = u32_at(data, b + 0x3C)?;
            let loop_start = u32_at(data, b + 0x40)?;
            let samples = u32_at(data, b + 0x08)?;
            let loop_range = (loop_end != NO_LOOP && loop_start < loop_end && loop_end <= samples)
                .then_some((loop_start, loop_end));
            channels.push(Channel {
                codec,
                pan: u16_at(data, b + 0x02)?,
                sample_rate: u32_at(data, b + 0x04)?,
                samples,
                samples_here: u32_at(data, b + 0x0C)?,
                coefs,
                data_offset: u32_at(data, b + 0x34)?,
                loop_range,
                predictor_scale: u16_at(data, b + 0x44)?,
                hist1: u16_at(data, b + 0x46)? as i16,
                hist2: u16_at(data, b + 0x48)? as i16,
            });
        }
        Ok(Self {
            prefetch,
            channels,
            data,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.channels[0].sample_rate
    }

    pub fn samples(&self) -> u32 {
        self.channels[0].samples
    }

    pub fn loop_range(&self) -> Option<(u32, u32)> {
        self.channels[0].loop_range
    }

    /// True when every sample of the sound is in this file.
    pub fn is_complete(&self) -> bool {
        !self.prefetch && self.channels.iter().all(|c| c.samples_here >= c.samples)
    }

    /// Size in bytes of the BWAV (header, channel table and every channel's data).
    pub fn byte_len(&self) -> usize {
        self.channels
            .iter()
            .map(|c| c.data_offset as usize + channel_bytes(c.codec, c.samples_here as usize))
            .max()
            .unwrap_or(0)
            .max(HEADER_SIZE + CHANNEL_INFO_SIZE * self.channels.len())
    }

    /// The BWAV's own bytes (for copying a BWAV out of a BARS).
    pub fn bytes(&self) -> Result<&'a [u8]> {
        let n = self.byte_len();
        self.data
            .get(..n)
            .ok_or_else(|| bad(format!("data runs past the end ({n:#x} bytes needed)")))
    }

    /// Channel `ch`'s raw sample data: DSP-ADPCM frames (with a last frame's padding when the
    /// file has it) or little-endian PCM16.
    pub fn channel_data(&self, ch: usize) -> Result<&'a [u8]> {
        let c = self
            .channels
            .get(ch)
            .ok_or_else(|| bad(format!("no channel {ch}")))?;
        let n = c.samples_here as usize;
        let start = c.data_offset as usize;
        let len = channel_bytes(c.codec, n);
        if self.data.len() < start + len {
            return Err(bad(format!("channel {ch} data out of range")));
        }
        let end = match c.codec {
            Codec::Pcm16 => start + len,
            Codec::DspAdpcm => (start + dsp::padded_bytes_for_samples(n)).min(self.data.len()),
        };
        Ok(&self.data[start..end])
    }

    /// Decode channel `ch` to PCM16 (the samples present in this file).
    pub fn decode_channel(&self, ch: usize) -> Result<Vec<i16>> {
        let c = self
            .channels
            .get(ch)
            .ok_or_else(|| bad(format!("no channel {ch}")))?;
        let n = c.samples_here as usize;
        let start = c.data_offset as usize;
        let len = channel_bytes(c.codec, n);
        if self.data.len() < start + len {
            return Err(bad(format!("channel {ch} data out of range")));
        }
        // take the padding of a last partial ADPCM frame when it is there
        let end = match c.codec {
            Codec::Pcm16 => start + len,
            Codec::DspAdpcm => (start + dsp::padded_bytes_for_samples(n)).min(self.data.len()),
        };
        let raw = &self.data[start..end];
        let mut out = Vec::with_capacity(n);
        match c.codec {
            Codec::Pcm16 => out.extend(
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| i16::from_le_bytes(b)),
            ),
            Codec::DspAdpcm => {
                let mut st = DspState {
                    coefs: c.coefs,
                    hist1: c.hist1,
                    hist2: c.hist2,
                };
                dsp::decode(raw, n, &mut st, &mut out);
            }
        }
        Ok(out)
    }
}

/// Bytes of sample data for `samples` samples of `codec`.
pub fn channel_bytes(codec: Codec, samples: usize) -> usize {
    match codec {
        Codec::Pcm16 => samples * 2,
        Codec::DspAdpcm => dsp::bytes_for_samples(samples),
    }
}

/// One channel for [`build`]: raw sample data already in `codec` form.
#[derive(Clone, Debug)]
pub struct BuildChannel<'a> {
    pub codec: Codec,
    pub pan: u16,
    pub sample_rate: u32,
    pub samples: u32,
    pub coefs: [i16; 16],
    pub loop_range: Option<(u32, u32)>,
    pub data: &'a [u8],
}

/// Write a (non-prefetch) BWAV. Used for synthetic test fixtures; channel data is 0x40-aligned
/// like the game's files.
pub fn build(channels: &[BuildChannel]) -> Vec<u8> {
    let table_end = HEADER_SIZE + CHANNEL_INFO_SIZE * channels.len();
    let mut offsets = Vec::new();
    let mut pos = table_end;
    for c in channels {
        pos = pos.next_multiple_of(0x40);
        offsets.push(pos);
        pos += c.data.len();
    }
    let mut out = vec![0u8; pos];
    out[0..4].copy_from_slice(b"BWAV");
    out[4..6].copy_from_slice(&0xFEFFu16.to_le_bytes());
    out[6..8].copy_from_slice(&1u16.to_le_bytes());
    out[0x0E..0x10].copy_from_slice(&(channels.len() as u16).to_le_bytes());
    let mut crc = crc32fast::Hasher::new();
    for (i, c) in channels.iter().enumerate() {
        let b = HEADER_SIZE + i * CHANNEL_INFO_SIZE;
        let codec: u16 = match c.codec {
            Codec::Pcm16 => 0,
            Codec::DspAdpcm => 1,
        };
        let w16 =
            |out: &mut Vec<u8>, o: usize, v: u16| out[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let w32 =
            |out: &mut Vec<u8>, o: usize, v: u32| out[o..o + 4].copy_from_slice(&v.to_le_bytes());
        w16(&mut out, b, codec);
        w16(&mut out, b + 2, c.pan);
        w32(&mut out, b + 4, c.sample_rate);
        w32(&mut out, b + 8, c.samples);
        w32(&mut out, b + 0x0C, c.samples);
        for (k, &cf) in c.coefs.iter().enumerate() {
            w16(&mut out, b + 0x10 + 2 * k, cf as u16);
        }
        w32(&mut out, b + 0x30, offsets[i] as u32);
        w32(&mut out, b + 0x34, offsets[i] as u32);
        w32(&mut out, b + 0x38, 1);
        let (ls, le) = c.loop_range.unwrap_or((0, NO_LOOP));
        w32(&mut out, b + 0x3C, le);
        w32(&mut out, b + 0x40, ls);
        w16(
            &mut out,
            b + 0x44,
            c.data.first().copied().unwrap_or(0) as u16,
        );
        out[offsets[i]..offsets[i] + c.data.len()].copy_from_slice(c.data);
        crc.update(c.data);
    }
    let crc = crc.finalize();
    out[8..12].copy_from_slice(&crc.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm_bytes(s: &[i16]) -> Vec<u8> {
        s.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn pcm16_stereo_round_trip() {
        let left: Vec<i16> = (0..100).map(|i| i * 10).collect();
        let right: Vec<i16> = (0..100).map(|i| -i * 10).collect();
        let (lb, rb) = (pcm_bytes(&left), pcm_bytes(&right));
        let file = build(&[
            BuildChannel {
                codec: Codec::Pcm16,
                pan: 0,
                sample_rate: 32000,
                samples: 100,
                coefs: [0; 16],
                loop_range: Some((20, 100)),
                data: &lb,
            },
            BuildChannel {
                codec: Codec::Pcm16,
                pan: 1,
                sample_rate: 32000,
                samples: 100,
                coefs: [0; 16],
                loop_range: Some((20, 100)),
                data: &rb,
            },
        ]);
        let w = Bwav::parse(&file).unwrap();
        assert_eq!(w.channels.len(), 2);
        assert!(w.is_complete());
        assert_eq!(w.sample_rate(), 32000);
        assert_eq!(w.loop_range(), Some((20, 100)));
        assert_eq!(w.channels[1].pan, 1);
        assert_eq!(w.decode_channel(0).unwrap(), left);
        assert_eq!(w.decode_channel(1).unwrap(), right);
        assert_eq!(w.byte_len(), file.len());
        assert!(w.decode_channel(2).is_err());
    }

    #[test]
    fn adpcm_channel_and_no_loop() {
        // one frame, zero coefficients, scale 0: residuals 1..7 then -8..-2
        let frame = [0x00u8, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE];
        let file = build(&[BuildChannel {
            codec: Codec::DspAdpcm,
            pan: 2,
            sample_rate: 48000,
            samples: 14,
            coefs: [0; 16],
            loop_range: None,
            data: &frame,
        }]);
        let w = Bwav::parse(&file).unwrap();
        assert_eq!(w.loop_range(), None);
        assert_eq!(w.channels[0].codec, Codec::DspAdpcm);
        assert_eq!(
            w.decode_channel(0).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, -8, -7, -6, -5, -4, -3, -2]
        );
    }

    #[test]
    fn embedded_bwav_ignores_trailing_bytes() {
        let frame = [0u8; 8];
        let mut file = build(&[BuildChannel {
            codec: Codec::DspAdpcm,
            pan: 2,
            sample_rate: 44100,
            samples: 14,
            coefs: [0; 16],
            loop_range: None,
            data: &frame,
        }]);
        let own = file.len();
        file.extend_from_slice(b"trailing data from the next entry");
        let w = Bwav::parse(&file).unwrap();
        assert_eq!(w.bytes().unwrap().len(), own);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Bwav::parse(b"BWAX").is_err());
        let mut f = build(&[BuildChannel {
            codec: Codec::Pcm16,
            pan: 0,
            sample_rate: 1,
            samples: 4,
            coefs: [0; 16],
            loop_range: None,
            data: &[0; 8],
        }]);
        f[0x10] = 9; // codec
        assert!(Bwav::parse(&f).is_err());
        assert!(Bwav::parse(&f[..0x20]).is_err());
    }

    #[test]
    fn prefetch_is_not_complete() {
        let mut f = build(&[BuildChannel {
            codec: Codec::Pcm16,
            pan: 0,
            sample_rate: 1,
            samples: 4,
            coefs: [0; 16],
            loop_range: None,
            data: &[0; 8],
        }]);
        f[0x0C] = 1;
        assert!(!Bwav::parse(&f).unwrap().is_complete());
    }
}
