//! Front-end glue shared by every shell: the C ABI used by the Swift app (`mmk-ffi`) and the
//! portable Rust shell (`mmk-shell`). Nothing here touches a platform API.
//!
//! - [`course`]: course file bytes (zlib / raw / encrypted `.bcd`) → course data.
//! - [`frame`]: sim sprites → textured rectangles (texture index from the asset cache, BFRES quad
//!   placement, flips), and the theme's clear colour.
//! - [`audio`]: the mixer handle rendered on the audio thread, and the director that turns sim
//!   events into mixer commands.
//! - [`texture`]: cache textures decoded to RGBA8 for GPUs without BC support (WebGL2, iPhones).

pub mod audio;
pub mod costume_sound;
pub mod course;
pub mod effects;
pub mod frame;
pub mod mesh;
pub mod paths;
pub mod scene3d;
pub mod sound;
pub mod sound_map;
#[cfg(test)]
mod sound_tests;
pub mod texture;

pub use frame::{DrawSprite, SpriteResolver, clear_color, place};
