//! BFWAV (`FWAV`): the Wii U wave container (SMM1 sounds), big-endian. Converted to a BWAV with
//! the same samples so the engine's audio path (BWAV only) plays it.
//!
//! Layout (from the public descriptions of the NW4F/NW4C wave format):
//!
//! ```text
//! 0x00 "FWAV"  u16 BOM (FE FF)  u16 header size  u32 version  u32 file size
//! 0x10 u16 section count  u16 pad, then per section: u16 type (0x7000 INFO, 0x7001 DATA),
//!      u16 pad, u32 offset, u32 size
//! INFO +0x08 u8 encoding (0 PCM8, 1 PCM16, 2 DSP-ADPCM)  u8 loop flag  u16 pad
//!      +0x0C u32 sample rate  +0x10 u32 loop start  +0x14 u32 loop end (= sample count)
//!      +0x18 u32 original loop start (version ≥ 1.1 only), then the channel table:
//!      u32 count, per channel a reference (u16 type, u16 pad, i32 offset from the count field)
//! channel info: sample reference (offset from DATA + 8), ADPCM info reference (offset from the
//!      channel info), u32 reserved
//! ADPCM info: i16 coefficients[16], u16 predictor/scale, i16 history 1, i16 history 2, then the
//!      loop context (same three fields)
//! ```

use crate::audio::bwav::{self, BuildChannel, Codec};
use crate::audio::dsp;
use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFWAV",
        detail: detail.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FwavChannel {
    /// Samples in the channel's own encoding (PCM16 big-endian, or DSP-ADPCM frames).
    pub data: Vec<u8>,
    pub coefs: [i16; 16],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fwav {
    /// 0 PCM8, 1 PCM16, 2 DSP-ADPCM.
    pub encoding: u8,
    pub sample_rate: u32,
    pub samples: u32,
    /// (start, end) in samples when the wave loops.
    pub loop_range: Option<(u32, u32)>,
    pub channels: Vec<FwavChannel>,
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn u8(&self, o: usize) -> Result<u8> {
        self.0
            .get(o)
            .copied()
            .ok_or_else(|| bad(format!("truncated at {o:#x}")))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_be_bytes([self.u8(o)?, self.u8(o + 1)?]))
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_be_bytes([
            self.u8(o)?,
            self.u8(o + 1)?,
            self.u8(o + 2)?,
            self.u8(o + 3)?,
        ]))
    }
}

impl Fwav {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.get(..4) != Some(b"FWAV") {
            return Err(Error::Magic { what: "BFWAV" });
        }
        let r = R(d);
        if r.u16(4)? != 0xFEFF {
            return Err(bad("little-endian wave not supported"));
        }
        let version = r.u32(8)?;
        let sections = r.u16(0x10)? as usize;
        let (mut info, mut data) = (None, None);
        for i in 0..sections {
            let s = 0x14 + 12 * i;
            match r.u16(s)? {
                0x7000 => info = Some(r.u32(s + 4)? as usize),
                0x7001 => data = Some(r.u32(s + 4)? as usize),
                _ => {}
            }
        }
        let (Some(info), Some(data)) = (info, data) else {
            return Err(bad("missing INFO or DATA section"));
        };
        let encoding = r.u8(info + 8)?;
        let looping = r.u8(info + 9)? != 0;
        let sample_rate = r.u32(info + 0x0C)?;
        let loop_start = r.u32(info + 0x10)?;
        let samples = r.u32(info + 0x14)?;
        let table = info + if version >= 0x0001_0100 { 0x1C } else { 0x18 };
        let count = r.u32(table)? as usize;
        if count == 0 || count > 8 {
            return Err(bad(format!("{count} channels")));
        }
        let bytes = match encoding {
            0 => samples as usize,
            1 => samples as usize * 2,
            2 => dsp::padded_bytes_for_samples(samples as usize),
            e => return Err(bad(format!("encoding {e}"))),
        };
        let mut channels = Vec::with_capacity(count);
        for i in 0..count {
            let ci = table + r.u32(table + 4 + 8 * i + 4)? as usize;
            let start = data + 8 + r.u32(ci + 4)? as usize;
            let raw = d
                .get(start..(start + bytes).min(d.len()))
                .ok_or_else(|| bad(format!("channel {i} data outside file")))?;
            let mut coefs = [0i16; 16];
            if encoding == 2 {
                let adpcm = ci + r.u32(ci + 12)? as usize;
                for (k, c) in coefs.iter_mut().enumerate() {
                    *c = r.u16(adpcm + 2 * k)? as i16;
                }
            }
            channels.push(FwavChannel {
                data: raw.to_vec(),
                coefs,
            });
        }
        Ok(Self {
            encoding,
            sample_rate,
            samples,
            loop_range: (looping && loop_start < samples).then_some((loop_start, samples)),
            channels,
        })
    }

    /// The same wave as a BWAV (DSP-ADPCM kept as is; PCM8/PCM16 become little-endian PCM16).
    pub fn to_bwav(&self) -> Vec<u8> {
        let pcm: Vec<Vec<u8>> = self
            .channels
            .iter()
            .map(|c| match self.encoding {
                0 => c
                    .data
                    .iter()
                    .flat_map(|&b| ((b as i8 as i16) << 8).to_le_bytes())
                    .collect(),
                1 => c
                    .data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| [b[1], b[0]])
                    .collect(),
                _ => c.data.clone(),
            })
            .collect();
        let codec = if self.encoding == 2 {
            Codec::DspAdpcm
        } else {
            Codec::Pcm16
        };
        let n = self.channels.len();
        let chans: Vec<BuildChannel> = pcm
            .iter()
            .zip(&self.channels)
            .enumerate()
            .map(|(i, (data, c))| BuildChannel {
                codec,
                pan: match (n, i) {
                    (1, _) => 2,
                    (_, 0) => 0,
                    _ => 1,
                },
                sample_rate: self.sample_rate,
                samples: self.samples,
                coefs: c.coefs,
                loop_range: self.loop_range,
                data,
            })
            .collect();
        bwav::build(&chans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Bwav;

    /// A mono PCM16 FWAV (version 1.2 layout).
    fn pcm_fwav(samples: &[i16]) -> Vec<u8> {
        let mut f = vec![0u8; 0x40];
        f[..4].copy_from_slice(b"FWAV");
        f[4..6].copy_from_slice(&[0xFE, 0xFF]);
        f[6..8].copy_from_slice(&0x40u16.to_be_bytes());
        f[8..12].copy_from_slice(&0x0001_0200u32.to_be_bytes());
        f[0x10..0x12].copy_from_slice(&2u16.to_be_bytes());
        let info = 0x40usize;
        f[0x14..0x16].copy_from_slice(&0x7000u16.to_be_bytes());
        f[0x18..0x1C].copy_from_slice(&(info as u32).to_be_bytes());
        f.resize(info + 0x40, 0);
        f[info..info + 4].copy_from_slice(b"INFO");
        f[info + 8] = 1;
        f[info + 0x0C..info + 0x10].copy_from_slice(&32000u32.to_be_bytes());
        f[info + 0x14..info + 0x18].copy_from_slice(&(samples.len() as u32).to_be_bytes());
        let table = info + 0x1C;
        f[table..table + 4].copy_from_slice(&1u32.to_be_bytes());
        f[table + 8..table + 12].copy_from_slice(&0xCu32.to_be_bytes());
        // channel info at table + 0xC: sample offset 0
        let data = f.len();
        f[0x20..0x22].copy_from_slice(&0x7001u16.to_be_bytes());
        f[0x24..0x28].copy_from_slice(&(data as u32).to_be_bytes());
        f.extend_from_slice(b"DATA\0\0\0\0");
        for s in samples {
            f.extend_from_slice(&s.to_be_bytes());
        }
        f
    }

    #[test]
    fn pcm16_converts_to_bwav() {
        let s: Vec<i16> = (0..50).map(|i| i * 300 - 7000).collect();
        let w = Fwav::parse(&pcm_fwav(&s)).unwrap();
        assert_eq!((w.encoding, w.sample_rate, w.samples), (1, 32000, 50));
        let b = w.to_bwav();
        let bw = Bwav::parse(&b).unwrap();
        assert_eq!(bw.decode_channel(0).unwrap(), s);
        assert_eq!(bw.sample_rate(), 32000);
    }

    #[test]
    fn rejects_other_data() {
        assert!(Fwav::parse(b"BWAV\xff\xfe").is_err());
        let mut f = pcm_fwav(&[1, 2]);
        f[0x40 + 8] = 9;
        assert!(Fwav::parse(&f).is_err());
    }
}
