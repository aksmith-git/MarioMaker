//! Audio for the engine (docs/architecture.md §2): the mixer, the sounds, the gameplay sound
//! database and the effects.
//!
//! The game thread sends [`Command`]s through a bounded queue; the audio thread drains the queue
//! and renders interleaved stereo f32 at [`OUTPUT_RATE`]. Sounds come from the asset cache
//! ([`Bank::load_cache`], and per course a [`mixer::SoundSet`] built from the sound database,
//! [`links::SoundDb`]).

pub mod bank;
pub mod fx;
pub mod links;
pub mod mixer;
pub mod stream;
pub mod wav;

pub use bank::{Bank, BankError, Samples, Sound};
pub use mixer::{Bus, Command, MAX_VOICES, Mixer, OUTPUT_RATE, Play, command_channel};
