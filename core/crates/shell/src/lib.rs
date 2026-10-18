//! Portable shell: plays a course with the Rust core on every platform the Swift app can't reach
//! (Windows, Linux, web) and on macOS too. winit window and loop, wgpu renderer (Metal, Vulkan,
//! DX12, GL, WebGPU/WebGL2), gilrs gamepads (Gamepad API in browsers), cpal audio (WebAudio in
//! browsers).
//!
//! Platform-neutral parts (session, input mapping, HUD, texture planning, asset sources) are
//! unit-tested without a window or GPU; `headless` renders offscreen for smoke tests.

pub mod app;
pub mod audio_out;
/// Moved to `mmk_formats::bc7` (the Swift app decodes with it too, through `mmk-ffi`).
pub use mmk_formats::bc7;
pub mod clock;
pub mod effects;
pub mod front_course;
pub mod game;
pub mod hud;
pub mod input;
pub mod maker;
pub mod mesh;
pub mod render;
pub mod session;
pub mod source;
pub mod texture;
pub mod ui_layer;

pub mod front_app;
#[cfg(not(target_arch = "wasm32"))]
pub mod front_host;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
pub mod ui_sound;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub mod web_front;
#[cfg(target_arch = "wasm32")]
pub mod web_import;
#[cfg(target_arch = "wasm32")]
pub mod web_net;
