//! Sound output through cpal (Core Audio, WASAPI, ALSA; WebAudio in the browser): the device's
//! callback pulls frames from the shared mixer (`mmk_host::audio::AudioOut`), which the session's
//! director feeds. In the browser the callback runs on the page's main thread (cpal schedules
//! WebAudio buffers ahead), and the stream may only start after a user gesture (`web`).

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use mmk_audio::Bank;
use mmk_host::audio::{AudioOut, MusicReader};

/// A running output stream (sound stops when it is dropped).
pub struct AudioStream {
    _stream: cpal::Stream,
    pub out: Arc<AudioOut>,
}

/// Frames rendered per mixer call; the callback loops for larger buffers.
const CHUNK: usize = 1024;

/// An open output stream and its sample rate.
pub struct Output {
    pub stream: cpal::Stream,
    pub rate: u32,
}

/// Open the default output device; `render` fills interleaved stereo frames at the returned
/// rate. The stream is created playing (a browser keeps it suspended until a gesture resumes it,
/// see [`Output::play`]).
pub fn open(render: impl FnMut(&mut [f32]) + Send + 'static) -> Result<Output, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no audio output device")?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("audio output: {e}"))?;
    let config = supported.config();
    let rate = config.sample_rate;
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, render),
        SampleFormat::I16 => build::<i16>(&device, config, render),
        SampleFormat::U16 => build::<u16>(&device, config, render),
        SampleFormat::I32 => build::<i32>(&device, config, render),
        f => return Err(format!("unsupported audio sample format {f:?}")),
    }?;
    let out = Output { stream, rate };
    out.play()?;
    crate::app::log(&format!(
        "sound: {} Hz, {} channels, {:?}",
        rate,
        config.channels,
        supported.sample_format()
    ));
    Ok(out)
}

impl Output {
    /// Start (or, in a browser, resume) the stream.
    pub fn play(&self) -> Result<(), String> {
        self.stream.play().map_err(|e| format!("audio output: {e}"))
    }
}

/// Open the default output device and play `bank` (plus the course music `music` reads, and the
/// SMM1 costume sounds of an SMM1 sprite cache, if given) through a mixer at the device's rate.
pub fn start(
    bank: Bank,
    music: Option<MusicReader>,
    smm1_sprites: Option<std::path::PathBuf>,
) -> Result<AudioStream, String> {
    // the mixer needs the device's rate, which is only known once the stream is open
    let slot: Arc<std::sync::OnceLock<Arc<AudioOut>>> = Arc::default();
    let feed = slot.clone();
    let output = open(move |buf| match feed.get() {
        Some(out) => out.render(buf),
        None => buf.fill(0.0),
    })?;
    let mut out = AudioOut::with_rate(bank, output.rate);
    if let Some(m) = music {
        out = out.with_music(m);
    }
    if let Some(dir) = smm1_sprites {
        out = out.with_costume_dir(dir);
    }
    let out = Arc::new(out);
    let _ = slot.set(out.clone());
    Ok(AudioStream {
        _stream: output.stream,
        out,
    })
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut render: impl FnMut(&mut [f32]) + Send + 'static,
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
                    render(buf);
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
            |e| crate::app::log(&format!("audio: {e}")),
            None,
        )
        .map_err(|e| format!("audio output: {e}"))
}
