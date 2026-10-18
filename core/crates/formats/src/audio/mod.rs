//! Switch audio containers: BARS archives of AMTA + BWAV, BWAV waves, DSP-ADPCM.

pub mod bars;
pub mod bwav;
pub mod dsp;
pub mod fwav;
pub mod settings;

pub use bars::Bars;
pub use bwav::{Bwav, Codec};
pub use fwav::Fwav;

/// Decode every channel of a BWAV to interleaved PCM16 with at most `max_channels` channels.
/// Returns (channels, samples per channel, interleaved data).
pub fn decode_interleaved(
    w: &Bwav,
    max_channels: usize,
) -> crate::Result<(usize, usize, Vec<i16>)> {
    let ch = w.channels.len().min(max_channels.max(1));
    let decoded: Vec<Vec<i16>> = (0..ch)
        .map(|c| w.decode_channel(c))
        .collect::<crate::Result<_>>()?;
    let n = decoded.iter().map(Vec::len).min().unwrap_or(0);
    let mut out = Vec::with_capacity(n * ch);
    for i in 0..n {
        for d in &decoded {
            out.push(d[i]);
        }
    }
    Ok((ch, n, out))
}
