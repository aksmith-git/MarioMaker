//! Deterministic 60 Hz simulation.
//!
//! Written from the behavioural specs (Phase 3), never from decompiled code. Units: pixels and
//! pixels/frame, 16 px per tile, y up. Every step is a pure function of (state, input).
//!
//! Status: Phase 5 step 6 — terrain, slopes, blocks, coins, Goomba, Koopa and shells, items,
//! power-up forms, damage and death, the course timer and the M1 goal pole.

pub mod actors;
pub mod blocks;
pub mod camera;
pub mod checksum;
pub mod clear;
pub mod exclamation;
pub mod fmath;
pub mod fx;
pub mod input;
pub mod level;
pub mod liquid;
pub mod options;
pub mod player;
pub mod render;
pub mod snake;
pub mod track;
pub mod warp;
pub mod world;

pub use input::Input;
pub use options::{MushroomSwap, Options};
pub use render::{Sprite, SpriteImage};
pub use world::{Event, Hud, World};
