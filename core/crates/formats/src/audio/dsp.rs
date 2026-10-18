//! Nintendo DSP-ADPCM ("GC-ADPCM") decoder.
//!
//! Each 8-byte frame holds 14 samples: a header byte (high nibble = coefficient pair index,
//! low nibble = scale exponent) followed by 14 signed 4-bit residuals. Every sample is predicted
//! from the previous two with the selected coefficient pair (fixed point, 11 fractional bits):
//!
//! ```text
//! s[n] = clamp16(((r << scale) << 11) + c1 * s[n-1] + c2 * s[n-2] + 1024) >> 11
//! ```

/// Bytes per ADPCM frame and samples per frame.
pub const FRAME_BYTES: usize = 8;
pub const FRAME_SAMPLES: usize = 14;

/// Bytes needed for `samples` samples. A last, partial frame only needs its header byte and the
/// bytes holding its residuals (some stream files end there instead of padding the frame).
pub fn bytes_for_samples(samples: usize) -> usize {
    let rest = samples % FRAME_SAMPLES;
    samples / FRAME_SAMPLES * FRAME_BYTES + if rest > 0 { 1 + rest.div_ceil(2) } else { 0 }
}

/// Bytes of `samples` samples when every frame is complete.
pub fn padded_bytes_for_samples(samples: usize) -> usize {
    samples.div_ceil(FRAME_SAMPLES) * FRAME_BYTES
}

/// Decoder state: coefficient table and the two previous output samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DspState {
    pub coefs: [i16; 16],
    pub hist1: i16,
    pub hist2: i16,
}

impl DspState {
    pub fn new(coefs: [i16; 16]) -> Self {
        Self {
            coefs,
            hist1: 0,
            hist2: 0,
        }
    }
}

fn clamp16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// Decode one frame (8 bytes, or fewer for a last partial frame) into `out` without
/// allocating; returns the number of samples written (≤ 14).
pub fn decode_frame(frame: &[u8], state: &mut DspState, out: &mut [i16; FRAME_SAMPLES]) -> usize {
    let Some(&header) = frame.first() else {
        return 0;
    };
    let room = (frame.len().saturating_sub(1) * 2).min(FRAME_SAMPLES);
    let pair = (header >> 4) as usize & 7;
    let scale = (header & 0xF) as u32;
    let c1 = state.coefs[pair * 2] as i32;
    let c2 = state.coefs[pair * 2 + 1] as i32;
    for (i, o) in out.iter_mut().enumerate().take(room) {
        let byte = frame[1 + i / 2];
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0xF };
        let r = ((nibble as i8) << 4 >> 4) as i32;
        let predicted = ((r << scale) << 11) + c1 * state.hist1 as i32 + c2 * state.hist2 as i32;
        let s = clamp16((predicted + 1024) >> 11);
        state.hist2 = state.hist1;
        state.hist1 = s;
        *o = s;
    }
    room
}

/// Decode `samples` samples from `data` into `out` (appended). Stops early if `data` runs out; the
/// last frame may be shorter than 8 bytes.
pub fn decode(data: &[u8], samples: usize, state: &mut DspState, out: &mut Vec<i16>) {
    out.reserve(samples);
    let mut left = samples;
    for frame in data.chunks(FRAME_BYTES) {
        if left == 0 {
            break;
        }
        // samples this (possibly short) frame can hold
        let room = (frame.len().saturating_sub(1) * 2).min(FRAME_SAMPLES);
        let header = frame[0];
        let pair = (header >> 4) as usize & 7;
        let scale = (header & 0xF) as u32;
        let c1 = state.coefs[pair * 2] as i32;
        let c2 = state.coefs[pair * 2 + 1] as i32;
        for i in 0..room.min(left) {
            let byte = frame[1 + i / 2];
            let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0xF };
            // sign-extend the 4-bit residual
            let r = ((nibble as i8) << 4 >> 4) as i32;
            let predicted =
                ((r << scale) << 11) + c1 * state.hist1 as i32 + c2 * state.hist2 as i32;
            let s = clamp16((predicted + 1024) >> 11);
            state.hist2 = state.hist1;
            state.hist1 = s;
            out.push(s);
        }
        left = left.saturating_sub(room);
        if room < FRAME_SAMPLES {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero coefficients: each sample is just the residual scaled by 2^scale.
    #[test]
    fn zero_coefficients_scale_residuals() {
        // header: pair 0, scale 3; residuals 1, -1, 7, -8, 0, 2, ... (nibbles)
        let frame = [0x03, 0x1F, 0x78, 0x02, 0x00, 0x00, 0x00, 0x00];
        let mut st = DspState::new([0; 16]);
        let mut out = Vec::new();
        decode(&frame, 14, &mut st, &mut out);
        assert_eq!(&out[..6], &[8, -8, 56, -64, 0, 16]);
        assert_eq!(out.len(), 14);
        assert_eq!(st.hist1, 0);
        assert_eq!(st.hist2, 0);
    }

    /// A pure first-order predictor (c1 = 1.0 in 5.11, c2 = 0): an integrator of the residuals.
    #[test]
    fn integrating_predictor() {
        let mut coefs = [0i16; 16];
        coefs[2] = 2048; // pair 1: c1 = 1.0
        // header: pair 1, scale 0; residuals all +1
        let frame = [0x10, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11];
        let mut st = DspState::new(coefs);
        let mut out = Vec::new();
        decode(&frame, 14, &mut st, &mut out);
        let expect: Vec<i16> = (1..=14).collect();
        assert_eq!(out, expect);
        assert_eq!((st.hist1, st.hist2), (14, 13));
    }

    /// Second-order prediction with rounding, checked against values worked out by hand:
    /// c1 = 0.5 (1024), c2 = -0.25 (-512), history (1000, 400), scale 2, residuals 3 then -2.
    #[test]
    fn second_order_with_history_and_rounding() {
        let mut coefs = [0i16; 16];
        coefs[4] = 1024;
        coefs[5] = -512;
        let frame = [0x22, 0x3E, 0, 0, 0, 0, 0, 0];
        let mut st = DspState {
            coefs,
            hist1: 1000,
            hist2: 400,
        };
        let mut out = Vec::new();
        decode(&frame, 2, &mut st, &mut out);
        // s0 = (12·2048 + 1024·1000 − 512·400 + 1024) >> 11 = 844_800 >> 11 = 412
        // s1 = (−8·2048 + 1024·412 − 512·1000 + 1024) >> 11 = −105_472 >> 11 = −52 (floor)
        assert_eq!(out, vec![412, -52]);
    }

    #[test]
    fn output_saturates() {
        let mut coefs = [0i16; 16];
        coefs[0] = 4096; // c1 = 2.0
        let frame = [0x0C, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77];
        let mut st = DspState::new(coefs);
        let mut out = Vec::new();
        decode(&frame, 14, &mut st, &mut out);
        assert_eq!(*out.last().unwrap(), i16::MAX);
        assert!(out.iter().all(|&s| s >= 0));
    }

    /// Brute-force encoder (test only): per frame, try every pair and scale, quantise each
    /// residual against the decoder's own prediction, keep the lowest squared error.
    fn encode(pcm: &[i16], coefs: [i16; 16]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut h1, mut h2) = (0i32, 0i32);
        for chunk in pcm.chunks(FRAME_SAMPLES) {
            let mut best: Option<(i64, [u8; 8], i32, i32)> = None;
            for pair in 0..8 {
                let (c1, c2) = (coefs[pair * 2] as i32, coefs[pair * 2 + 1] as i32);
                for scale in 0..=12u32 {
                    let (mut a, mut b) = (h1, h2);
                    let mut frame = [0u8; 8];
                    frame[0] = (pair as u8) << 4 | scale as u8;
                    let mut err = 0i64;
                    for (i, &x) in chunk.iter().enumerate() {
                        let pred = c1 * a + c2 * b;
                        let want = ((x as i32) << 11) - pred;
                        let step = 1i32 << (scale + 11);
                        let r = ((want as f64 / step as f64).round() as i32).clamp(-8, 7);
                        let s = ((((r << scale) << 11) + pred + 1024) >> 11).clamp(-32768, 32767);
                        err += ((s - x as i32) as i64).pow(2);
                        frame[1 + i / 2] |= ((r as u8) & 0xF) << if i % 2 == 0 { 4 } else { 0 };
                        b = a;
                        a = s;
                    }
                    if best.is_none_or(|bst| err < bst.0) {
                        best = Some((err, frame, a, b));
                    }
                }
            }
            let (_, frame, a, b) = best.unwrap();
            out.extend_from_slice(&frame);
            h1 = a;
            h2 = b;
        }
        out
    }

    #[test]
    fn encoded_sine_decodes_closely() {
        let coefs: [i16; 16] = [
            0, 0, 2048, 0, 3840, -1792, 3584, -1600, 4000, -1990, 1024, 0, 3072, -1024, 4060, -2030,
        ];
        let pcm: Vec<i16> = (0..700)
            .map(|i| ((i as f64 * 0.05).sin() * 12000.0) as i16)
            .collect();
        let data = encode(&pcm, coefs);
        assert_eq!(data.len(), bytes_for_samples(pcm.len()));
        let mut st = DspState::new(coefs);
        let mut out = Vec::new();
        decode(&data, pcm.len(), &mut st, &mut out);
        let max_err = pcm
            .iter()
            .zip(&out)
            .map(|(&a, &b)| (a as i32 - b as i32).abs())
            .max()
            .unwrap();
        assert!(max_err < 120, "max error {max_err}");
    }

    #[test]
    fn partial_last_frame_and_short_data() {
        let frames = [0x00u8; 16];
        let mut st = DspState::new([0; 16]);
        let mut out = Vec::new();
        decode(&frames, 20, &mut st, &mut out);
        assert_eq!(out.len(), 20);
        let mut out = Vec::new();
        decode(&frames, 100, &mut st, &mut out);
        assert_eq!(out.len(), 28, "stops when the data runs out");
        assert_eq!(bytes_for_samples(20), 12);
        assert_eq!(bytes_for_samples(28), 16);
        assert_eq!(bytes_for_samples(29), 18);
        assert_eq!(padded_bytes_for_samples(20), 16);
    }

    /// A stream may end mid-frame: 9 samples need the header and 5 residual bytes.
    #[test]
    fn short_last_frame() {
        let mut data = vec![0u8; 8];
        data.extend_from_slice(&[0x00, 0x11, 0x11, 0x11, 0x11, 0x10]);
        assert_eq!(data.len(), bytes_for_samples(14 + 9));
        let mut st = DspState::new([0; 16]);
        let mut out = Vec::new();
        decode(&data, 23, &mut st, &mut out);
        assert_eq!(out.len(), 23);
        assert_eq!(&out[14..], &[1, 1, 1, 1, 1, 1, 1, 1, 1]);
    }
}
