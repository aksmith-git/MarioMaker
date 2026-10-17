//! Sound output through cpal (Core Audio, WASAPI, ALSA): the device's callback pulls frames from
//! the shared mixer (`mmk_host::audio::AudioOut`), which the session's director feeds.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use mmk_audio::Bank;
use mmk_host::audio::AudioOut;

/// A running output stream (sound stops when it is dropped).
pub struct AudioStream {
    _stream: cpal::Stream,
    pub out: Arc<AudioOut>,
}

/// Frames rendered per mixer call; the callback loops for larger buffers.
const CHUNK: usize = 1024;

/// Open the default output device and play `bank` through a mixer at the device's rate.
pub fn start(bank: Bank) -> Result<AudioStream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no audio output device")?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("audio output: {e}"))?;
    let config = supported.config();
    let out = Arc::new(AudioOut::with_rate(bank, config.sample_rate));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, out.clone()),
        SampleFormat::I16 => build::<i16>(&device, config, out.clone()),
        SampleFormat::U16 => build::<u16>(&device, config, out.clone()),
        SampleFormat::I32 => build::<i32>(&device, config, out.clone()),
        f => return Err(format!("unsupported audio sample format {f:?}")),
    }?;
    stream.play().map_err(|e| format!("audio output: {e}"))?;
    crate::app::log(&format!(
        "sound: {} Hz, {} channels, {:?}",
        config.sample_rate,
        config.channels,
        supported.sample_format()
    ));
    Ok(AudioStream {
        _stream: stream,
        out,
    })
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    out: Arc<AudioOut>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels.max(1) as usize;
    // interleaved stereo scratch, allocated once: the callback must not allocate
    let mut scratch = vec![0.0f32; CHUNK * 2];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frames in data.chunks_mut(CHUNK * channels) {
                    let n = frames.len() / channels;
                    let buf = &mut scratch[..n * 2];
                    out.render(buf);
                    for (f, lr) in frames.chunks_mut(channels).zip(buf.as_chunks::<2>().0) {
                        for (c, s) in f.iter_mut().enumerate() {
                            // mono devices get the mix of both; extra channels stay silent
                            let v = match (channels, c) {
                                (1, _) => (lr[0] + lr[1]) * 0.5,
                                (_, 0) => lr[0],
                                (_, 1) => lr[1],
                                _ => 0.0,
                            };
                            *s = T::from_sample(v);
                        }
                    }
                }
            },
            |e| eprintln!("mmk-play: audio: {e}"),
            None,
        )
        .map_err(|e| format!("audio output: {e}"))
}
