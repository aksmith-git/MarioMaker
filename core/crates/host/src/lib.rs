//! Front-end glue shared by every shell: the C ABI used by the Swift app (`mmk-ffi`) and the
//! portable Rust shell (`mmk-shell`). Nothing here touches a platform API.
//!
//! - [`course`]: course file bytes (zlib / raw / encrypted `.bcd`) → course data.
//! - [`frame`]: sim sprites → textured rectangles (texture index from the asset cache, BFRES quad
//!   placement, flips), and the theme's clear colour.
//! - [`audio`]: the mixer handle rendered on the audio thread, and the director that turns sim
//!   events into mixer commands.

pub mod audio;
pub mod course;
pub mod frame;

pub use frame::{DrawSprite, SpriteResolver, clear_color, place};
