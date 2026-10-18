//! DSP-ADPCM sounds decoded while they play (long music streams stay compressed in memory,
//! about a quarter of their PCM size). A voice reads samples through a [`Cursor`], which keeps
//! two decoded frames and the decoder history; jumps (loops, resume positions) restart from a
//! seek table of decoder histories made when the sound is loaded.

use std::sync::Arc;

use mmk_formats::audio::dsp::{self, DspState, FRAME_BYTES, FRAME_SAMPLES};

/// Channels a streamed sound may have (two stereo pairs: the music and its extra layer).
pub const MAX_CHANNELS: usize = 4;
/// Frames between seek table entries.
const SEEK_FRAMES: u32 = 256;
/// One channel for [`Adpcm::new`]: data offset and length in the file, coefficients, initial
/// history.
pub type ChanSpec = (usize, usize, [i16; 16], (i16, i16));

#[derive(Clone, Debug)]
struct Chan {
    /// Offset and length of the channel's ADPCM data in `bytes`.
    offset: usize,
    len: usize,
    coefs: [i16; 16],
}

/// A DSP-ADPCM sound (every channel ADPCM, same length).
#[derive(Clone, Debug)]
pub struct Adpcm {
    bytes: Arc<[u8]>,
    chans: Vec<Chan>,
    frames: u32,
    /// Decoder history (hist1, hist2) per channel at the start of frame k·SEEK_FRAMES.
    seek: Vec<[(i16, i16); MAX_CHANNELS]>,
}

impl Adpcm {
    /// `bytes` is the whole file; `chans` (offset, length, coefficients, initial history) for
    /// at most [`MAX_CHANNELS`] channels; `frames` sample frames.
    pub fn new(bytes: Arc<[u8]>, chans: &[ChanSpec], frames: u32) -> Self {
        let chans_v: Vec<Chan> = chans
            .iter()
            .take(MAX_CHANNELS)
            .map(|&(offset, len, coefs, _)| Chan { offset, len, coefs })
            .collect();
        let mut states: Vec<DspState> = chans
            .iter()
            .take(MAX_CHANNELS)
            .map(|&(_, _, coefs, (h1, h2))| DspState {
                coefs,
                hist1: h1,
                hist2: h2,
            })
            .collect();
        let n_frames = frames.div_ceil(FRAME_SAMPLES as u32);
        let mut seek = Vec::with_capacity((n_frames / SEEK_FRAMES + 1) as usize);
        let mut buf = [0i16; FRAME_SAMPLES];
        for f in 0..n_frames {
            if f % SEEK_FRAMES == 0 {
                let mut e = [(0, 0); MAX_CHANNELS];
                for (c, st) in states.iter().enumerate() {
                    e[c] = (st.hist1, st.hist2);
                }
                seek.push(e);
            }
            for (c, st) in states.iter_mut().enumerate() {
                let ch = &chans_v[c];
                let at = f as usize * FRAME_BYTES;
                let end = (at + FRAME_BYTES).min(ch.len);
                if at < ch.len {
                    dsp::decode_frame(&bytes[ch.offset + at..ch.offset + end], st, &mut buf);
                }
            }
        }
        Self {
            bytes,
            chans: chans_v,
            frames,
            seek,
        }
    }

    pub fn channels(&self) -> usize {
        self.chans.len()
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Decode frame `f` of every channel into `out` (`hist` is updated).
    fn decode(
        &self,
        f: u32,
        hist: &mut [(i16, i16); MAX_CHANNELS],
        out: &mut [[i16; FRAME_SAMPLES]; MAX_CHANNELS],
    ) {
        for (c, ch) in self.chans.iter().enumerate() {
            let at = f as usize * FRAME_BYTES;
            let mut st = DspState {
                coefs: ch.coefs,
                hist1: hist[c].0,
                hist2: hist[c].1,
            };
            if at < ch.len {
                let end = (at + FRAME_BYTES).min(ch.len);
                let n = dsp::decode_frame(
                    &self.bytes[ch.offset + at..ch.offset + end],
                    &mut st,
                    &mut out[c],
                );
                out[c][n..].fill(0);
            } else {
                out[c].fill(0);
            }
            hist[c] = (st.hist1, st.hist2);
        }
    }
}

/// A voice's read position in an [`Adpcm`] sound: frames `frame` and `frame + 1` decoded.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    frame: u32,
    valid: bool,
    buf: [[[i16; FRAME_SAMPLES]; MAX_CHANNELS]; 2],
    /// History after the second decoded frame.
    hist: [(i16, i16); MAX_CHANNELS],
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            frame: 0,
            valid: false,
            buf: [[[0; FRAME_SAMPLES]; MAX_CHANNELS]; 2],
            hist: [(0, 0); MAX_CHANNELS],
        }
    }
}

impl Cursor {
    /// Make frames `f` and `f + 1` available.
    fn seek(&mut self, a: &Adpcm, f: u32) {
        if self.valid && f == self.frame {
            return;
        }
        if self.valid && f == self.frame + 1 {
            self.buf[0] = self.buf[1];
            self.frame = f;
            let mut b = self.buf[1];
            a.decode(f + 1, &mut self.hist, &mut b);
            self.buf[1] = b;
            return;
        }
        // restart from the seek table entry at or before f
        let k = (f / SEEK_FRAMES) as usize;
        let mut hist = a.seek.get(k).copied().unwrap_or([(0, 0); MAX_CHANNELS]);
        let mut scratch = [[0i16; FRAME_SAMPLES]; MAX_CHANNELS];
        for g in (k as u32 * SEEK_FRAMES)..f {
            a.decode(g, &mut hist, &mut scratch);
        }
        let mut b0 = [[0i16; FRAME_SAMPLES]; MAX_CHANNELS];
        let mut b1 = [[0i16; FRAME_SAMPLES]; MAX_CHANNELS];
        a.decode(f, &mut hist, &mut b0);
        a.decode(f + 1, &mut hist, &mut b1);
        self.buf = [b0, b1];
        self.hist = hist;
        self.frame = f;
        self.valid = true;
    }

    /// Sample `i` of channel `c` (0 past the end).
    pub fn sample(&mut self, a: &Adpcm, i: u32, c: usize) -> i16 {
        if i >= a.frames || c >= a.chans.len() {
            return 0;
        }
        let f = i / FRAME_SAMPLES as u32;
        let o = (i % FRAME_SAMPLES as u32) as usize;
        if self.valid && f == self.frame + 1 {
            return self.buf[1][c][o];
        }
        self.seek(a, f);
        self.buf[0][c][o]
    }

    /// Advance so that sample `i` is in the first decoded frame (call before reading `i`).
    pub fn at(&mut self, a: &Adpcm, i: u32) {
        let f = i / FRAME_SAMPLES as u32;
        if !(self.valid && f == self.frame) {
            self.seek(a, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random-ish ADPCM bytes for two channels; streamed reads must equal a full decode.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn streamed_reads_match_full_decode() {
        let frames = 14 * 700 + 5;
        let n_bytes = mmk_formats::audio::dsp::padded_bytes_for_samples(frames as usize);
        let mut bytes = Vec::new();
        let mut x = 12345u32;
        for _ in 0..2 * n_bytes {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // keep scales small so values stay in range
            bytes.push((x & 0x3F) as u8);
        }
        let coefs = [
            0, 0, 2048, 0, 3840, -1792, 3584, -1600, 4000, -1990, 1024, 0, 3072, -1024, 4060, -2030,
        ];
        let bytes: Arc<[u8]> = bytes.into();
        let chans = [
            (0, n_bytes, coefs, (0, 0)),
            (n_bytes, n_bytes, coefs, (5, 3)),
        ];
        let a = Adpcm::new(bytes.clone(), &chans, frames);
        let full: Vec<Vec<i16>> = chans
            .iter()
            .map(|&(o, l, c, (h1, h2))| {
                let mut st = DspState {
                    coefs: c,
                    hist1: h1,
                    hist2: h2,
                };
                let mut out = Vec::new();
                dsp::decode(&bytes[o..o + l], frames as usize, &mut st, &mut out);
                out
            })
            .collect();
        let mut cur = Cursor::default();
        // sequential, with the look-ahead sample
        for i in 0..frames {
            for c in 0..2 {
                assert_eq!(cur.sample(&a, i, c), full[c][i as usize], "seq {i} ch {c}");
                if i + 1 < frames {
                    assert_eq!(cur.sample(&a, i + 1, c), full[c][i as usize + 1]);
                }
            }
            cur.at(&a, i);
        }
        // jumps (loop back, resume far ahead)
        for &i in &[9000u32, 3, 7777, 14 * 256 * 2 + 3, 0, frames - 1] {
            let mut cur = Cursor::default();
            for c in 0..2 {
                assert_eq!(cur.sample(&a, i, c), full[c][i as usize], "jump {i}");
            }
        }
        assert_eq!(cur.sample(&a, frames + 3, 0), 0);
    }
}
