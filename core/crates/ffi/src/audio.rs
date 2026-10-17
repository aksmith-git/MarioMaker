//! Audio for the engine. The mixer handle and the sound director live in `mmk-host` (shared with
//! the portable shell); `MmkAudio` is the C ABI's name for the handle.

pub use mmk_host::audio::{AudioOut, Director, OUTPUT_GAIN};

/// The audio handle behind `mmk_audio_*` (opaque to C).
pub type MmkAudio = AudioOut;
