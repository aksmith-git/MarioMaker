//! Audio mixer for the engine (docs/architecture.md §2).
//!
//! The game thread sends [`Command`]s through a bounded queue; the audio thread drains the queue
//! and renders interleaved stereo f32 at [`OUTPUT_RATE`]. Sounds come from the asset cache
//! ([`Bank::load_cache`]) and are decoded to PCM16 once when loaded.

pub mod bank;
pub mod mixer;

pub use bank::{Bank, BankError, Sound};
pub use mixer::{Bus, Command, MAX_VOICES, Mixer, OUTPUT_RATE, command_channel};
