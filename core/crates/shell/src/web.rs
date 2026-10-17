//! Web entry (wasm32): plays the built-in demo strip in a `<canvas id="mmk">` (created if the
//! page has none), drawn with WebGPU where available and WebGL2 otherwise.
//!
//! Later: the owner's asset cache picked through the browser's file picker, stored in
//! OPFS/IndexedDB and read through a `MemorySource`/OPFS-backed `AssetSource` (nothing is
//! uploaded anywhere); WebAudio output through cpal's wasm backend.

use wasm_bindgen::JsCast;
use winit::event_loop::EventLoop;
use winit::platform::web::EventLoopExtWebSys;

use crate::app::{App, UserEvent};
use crate::game::Game;

/// The page's `<canvas id="mmk">`, if it has one (otherwise winit creates and appends one).
pub fn canvas() -> Option<web_sys::HtmlCanvasElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("mmk")?
        .dyn_into()
        .ok()
}

pub fn start() {
    console_error_panic_hook::set_once();
    let game = match Game::load(None, None) {
        Ok(g) => g,
        Err(e) => {
            web_sys::console::error_1(&e.into());
            return;
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            web_sys::console::error_1(&format!("event loop: {e}").into());
            return;
        }
    };
    let mut app = App::new(game, "mmk-play".into(), true);
    app.set_proxy(event_loop.create_proxy());
    event_loop.spawn_app(app);
}
