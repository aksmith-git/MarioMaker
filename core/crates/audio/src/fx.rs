//! Effects: a one-pole low-pass (voice filters, the underwater filter), the per-theme echo
//! (delay with filtered feedback) and reverb (a Schroeder–Moorer network with damped combs and
//! all-passes). Parameters follow the game's effect files (`Sound/Effect/*.byml`); how the
//! game's own effects compute them is not known, so these are close stand-ins [inf].
//! Buffers are allocated when the mixer is made; processing never allocates.

/// Cutoff of the voice low-pass for a filter amount 0…1 (0: off): 16 kHz halved every 1/8.
pub fn lpf_cutoff(amount: f32) -> f32 {
    16_000.0 * (-8.0 * amount.clamp(0.0, 1.0)).exp2()
}

/// One-pole low-pass coefficient for `cutoff` Hz at `rate`.
pub fn lpf_coef(cutoff: f32, rate: u32) -> f32 {
    let x = (-2.0 * std::f32::consts::PI * cutoff / rate.max(1) as f32).exp();
    1.0 - x
}

/// The final mix limiter (the game's bus settings name one with a −3 dB threshold and a
/// 300 ms release on the final mix): instant attack, exponential release.
pub struct Limiter {
    threshold: f32,
    release: f32,
    gain: f32,
}

impl Limiter {
    pub fn new(rate: u32) -> Self {
        Self {
            threshold: 10f32.powf(-3.0 / 20.0),
            release: (-1.0 / (0.3 * rate.max(1) as f32)).exp(),
            gain: 1.0,
        }
    }

    /// Scale interleaved stereo `buf` by `pre`, then limit it in place.
    pub fn process(&mut self, buf: &mut [f32], pre: f32) {
        for f in buf.as_chunks_mut::<2>().0.iter_mut() {
            let (l, r) = (f[0] * pre, f[1] * pre);
            let peak = l.abs().max(r.abs());
            let want = if peak > self.threshold {
                self.threshold / peak
            } else {
                1.0
            };
            self.gain = if want < self.gain {
                want
            } else {
                want - (want - self.gain) * self.release
            };
            f[0] = l * self.gain;
            f[1] = r * self.gain;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayParams {
    /// Seconds.
    pub time: f32,
    pub feedback: f32,
    /// High-frequency damping in the feedback loop, 0…1.
    pub lpf: f32,
    /// Wet output gain.
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParams {
    /// Room and late reverb levels, millibels (I3DL2).
    pub room: f32,
    pub reverb: f32,
    /// Seconds.
    pub decay: f32,
    pub hf_ratio: f32,
    /// Pre-delay (reflections + reverb delay), seconds.
    pub predelay: f32,
    /// 0…100.
    pub diffusion: f32,
}

const MAX_DELAY_S: f32 = 1.0;

pub struct Delay {
    p: Option<DelayParams>,
    line: [Vec<f32>; 2],
    pos: usize,
    len: usize,
    lp: [f32; 2],
}

impl Delay {
    pub fn new(rate: u32) -> Self {
        let n = (rate as f32 * MAX_DELAY_S) as usize + 1;
        Self {
            p: None,
            line: [vec![0.0; n], vec![0.0; n]],
            pos: 0,
            len: 1,
            lp: [0.0; 2],
        }
    }

    pub fn set(&mut self, p: Option<DelayParams>, rate: u32) {
        if p == self.p {
            return;
        }
        self.p = p;
        let cap = self.line[0].len();
        self.len = p
            .map(|p| ((p.time.max(0.0) * rate as f32) as usize).clamp(1, cap))
            .unwrap_or(1);
        for l in &mut self.line {
            l.fill(0.0);
        }
        self.pos = 0;
        self.lp = [0.0; 2];
    }

    pub fn active(&self) -> bool {
        self.p.is_some_and(|p| p.gain > 0.0)
    }

    /// Add the wet signal of `input` (interleaved stereo) to `out`.
    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let Some(p) = self.p else { return };
        let a = 1.0 - 0.95 * p.lpf.clamp(0.0, 1.0);
        let fb = p.feedback.clamp(0.0, 0.98);
        for (i, o) in input
            .as_chunks::<2>()
            .0
            .iter()
            .zip(out.as_chunks_mut::<2>().0.iter_mut())
        {
            for c in 0..2 {
                let d = self.line[c][self.pos];
                self.lp[c] += a * (d - self.lp[c]);
                self.line[c][self.pos] = i[c] + self.lp[c] * fb;
                o[c] += self.lp[c] * p.gain;
            }
            self.pos += 1;
            if self.pos >= self.len {
                self.pos = 0;
            }
        }
    }
}

struct Comb {
    buf: Vec<f32>,
    len: usize,
    pos: usize,
    store: f32,
}

struct Allpass {
    buf: Vec<f32>,
    len: usize,
    pos: usize,
}

/// Freeverb tunings (samples at 44.1 kHz).
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

pub struct Reverb {
    p: Option<ReverbParams>,
    rate: u32,
    combs: [Vec<Comb>; 2],
    alls: [Vec<Allpass>; 2],
    pre: Vec<f32>,
    pre_len: usize,
    pre_pos: usize,
    feedback: [f32; 8],
    damp: f32,
    wet: f32,
}

impl Reverb {
    pub fn new(rate: u32) -> Self {
        let scale = rate as f32 / 44_100.0;
        let mk_combs = |extra: usize| {
            COMBS
                .iter()
                .map(|&l| {
                    let len = ((l + extra) as f32 * scale) as usize + 1;
                    Comb {
                        buf: vec![0.0; len],
                        len,
                        pos: 0,
                        store: 0.0,
                    }
                })
                .collect::<Vec<_>>()
        };
        let mk_alls = |extra: usize| {
            ALLPASSES
                .iter()
                .map(|&l| {
                    let len = ((l + extra) as f32 * scale) as usize + 1;
                    Allpass {
                        buf: vec![0.0; len],
                        len,
                        pos: 0,
                    }
                })
                .collect::<Vec<_>>()
        };
        Self {
            p: None,
            rate,
            combs: [mk_combs(0), mk_combs(SPREAD)],
            alls: [mk_alls(0), mk_alls(SPREAD)],
            pre: vec![0.0; (rate as f32 * 0.5) as usize * 2 + 2],
            pre_len: 1,
            pre_pos: 0,
            feedback: [0.0; 8],
            damp: 0.0,
            wet: 0.0,
        }
    }

    pub fn set(&mut self, p: Option<ReverbParams>) {
        if p == self.p {
            return;
        }
        self.p = p;
        let Some(p) = p else { return };
        let rate = self.rate as f32;
        for (k, c) in self.combs[0].iter().enumerate() {
            // gain for −60 dB after `decay` seconds
            self.feedback[k] = 10f32.powf(-3.0 * c.len as f32 / (p.decay.max(0.05) * rate));
        }
        self.damp = (1.0 - p.hf_ratio).clamp(0.0, 0.9);
        // millibels → amplitude; Freeverb's own wet scaling (×3) on top
        self.wet = 10f32.powf((p.room + p.reverb) / 2000.0) * 3.0;
        self.pre_len = ((p.predelay.max(0.0) * rate) as usize).clamp(1, self.pre.len() / 2);
        for side in self.combs.iter_mut() {
            for c in side {
                c.buf.fill(0.0);
                c.store = 0.0;
            }
        }
        for side in self.alls.iter_mut() {
            for a in side {
                a.buf.fill(0.0);
            }
        }
        self.pre.fill(0.0);
    }

    pub fn active(&self) -> bool {
        self.p.is_some() && self.wet > 1e-4
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        if !self.active() {
            return;
        }
        let diffusion = self.p.map(|p| p.diffusion).unwrap_or(100.0);
        let ap_gain = 0.5 * (diffusion / 100.0).clamp(0.0, 1.0);
        for (i, o) in input
            .as_chunks::<2>()
            .0
            .iter()
            .zip(out.as_chunks_mut::<2>().0.iter_mut())
        {
            // mono input through the pre-delay
            let x = (i[0] + i[1]) * 0.015;
            let idx = self.pre_pos * 2;
            let delayed = self.pre[idx];
            self.pre[idx] = x;
            self.pre_pos += 1;
            if self.pre_pos >= self.pre_len {
                self.pre_pos = 0;
            }
            for c in 0..2 {
                let mut acc = 0.0;
                for (k, comb) in self.combs[c].iter_mut().enumerate() {
                    let y = comb.buf[comb.pos];
                    comb.store = y * (1.0 - self.damp) + comb.store * self.damp;
                    comb.buf[comb.pos] = delayed + comb.store * self.feedback[k];
                    comb.pos += 1;
                    if comb.pos >= comb.len {
                        comb.pos = 0;
                    }
                    acc += y;
                }
                for a in self.alls[c].iter_mut() {
                    let b = a.buf[a.pos];
                    let y = b - acc;
                    a.buf[a.pos] = acc + b * ap_gain;
                    a.pos += 1;
                    if a.pos >= a.len {
                        a.pos = 0;
                    }
                    acc = y;
                }
                o[c] += acc * self.wet;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_echoes_after_its_time() {
        let mut d = Delay::new(1000);
        d.set(
            Some(DelayParams {
                time: 0.01,
                feedback: 0.5,
                lpf: 0.0,
                gain: 1.0,
            }),
            1000,
        );
        let mut input = vec![0.0; 2 * 40];
        input[0] = 1.0;
        input[1] = 1.0;
        let mut out = vec![0.0; 2 * 40];
        d.process(&input, &mut out);
        // the impulse comes back after 10 frames, then again at half level after 20
        let first = out.iter().step_by(2).position(|&v| v.abs() > 1e-3).unwrap();
        assert_eq!(first, 10);
        assert!((out[2 * 20] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn reverb_tail_decays_and_null_is_silent() {
        let mut r = Reverb::new(48_000);
        r.set(Some(ReverbParams {
            room: 0.0,
            reverb: 0.0,
            decay: 0.5,
            hf_ratio: 0.8,
            predelay: 0.01,
            diffusion: 100.0,
        }));
        let n = 48_000;
        let mut input = vec![0.0; 2 * n];
        input[0] = 1.0;
        let mut out = vec![0.0; 2 * n];
        r.process(&input, &mut out);
        let energy = |a: usize, b: usize| out[2 * a..2 * b].iter().map(|v| v * v).sum::<f32>();
        assert!(energy(0, 12_000) > 0.0);
        assert!(energy(36_000, 48_000) < energy(0, 12_000) * 0.01, "decayed");
        let mut silent = Reverb::new(48_000);
        silent.set(Some(ReverbParams {
            room: -4000.0,
            reverb: -10000.0,
            decay: 2.0,
            hf_ratio: 0.8,
            predelay: 0.0,
            diffusion: 90.0,
        }));
        assert!(!silent.active());
    }

    #[test]
    fn lpf_mapping() {
        assert_eq!(lpf_cutoff(0.0), 16_000.0);
        assert!((lpf_cutoff(0.125) - 8_000.0).abs() < 1.0);
        assert!(lpf_coef(16_000.0, 48_000) > lpf_coef(1_000.0, 48_000));
    }
}
