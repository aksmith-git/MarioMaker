//! Particle effects: the imported emitter sets and effect links (`library`), the emitter
//! simulation that turns playing sets into textured quads (`system`), and the director that
//! plays what a running course asks for (`director`).
//!
//! Drawing a quad: texture `EffectPlayer::texture_names()[q.texture]` of the cache's
//! `Effect` texture model, corners in world pixels, colour = texel × `q.color`, blended by
//! `q.blend`; drawn over the course's sprites and meshes, under the HUD.

pub mod director;
pub mod library;
pub mod system;

pub use director::EffectDirector;
pub use library::EffectLibrary;
pub use system::{Blend, EffectPlayer, FxHandle, FxQuad};

/// Texture model of the effect textures in the asset cache.
pub const TEXTURE_MODEL: &str = "Effect";
