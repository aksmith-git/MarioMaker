//! Deterministic 60 Hz simulation.
//!
//! Written from the behavioural specs (Phase 3), never from decompiled code. Units: pixels and
//! pixels/frame, 16 px per tile, y up. Every step is a pure function of (state, input).
//!
//! Status: Phase 5 step 4 — a single player on flat ground, enough to drive the renderer.
//! Terrain collision, camera rules and objects follow in steps 5–6.

pub mod input;
pub mod level;
pub mod player;
pub mod render;
pub mod world;

pub use input::Input;
pub use render::{Sprite, SpriteImage};
pub use world::World;
