//! Web entry (wasm32): the shell in a `<canvas id="mmk">` (created if the page has none), drawn
//! with WebGPU where available and WebGL2 otherwise. It starts on the built-in demo strip with
//! placeholder art; the page (`web/mmk-web.js`) then hands over the owner's game data and
//! courses through the functions exported here.
//!
//! - **Game data** lives in the browser's origin private file system (OPFS), copied there by the
//!   page from a folder the owner picked; nothing is uploaded anywhere. Rust reads it through
//!   the page's `mmkStorage.read(path)` (a promise of the file's bytes, or null). The texture
//!   index, sprites, sounds and key table are read when the data is opened ([`open_assets`]);
//!   texture pixels are read when a sprite first needs them ([`pump_textures`]), and a course's
//!   music when the course starts ([`play_course`]), so memory holds what is being played, not
//!   the 1.4 GB cache.
//! - **Sound** goes out through cpal's WebAudio backend, which browsers only let start after a
//!   user gesture: the page calls [`unlock_audio`] from its first pointer or key event.
//! - **Gamepads** come from the Gamepad API through gilrs (`input`), keys from the canvas.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mmk_assets::Cache;
use mmk_audio::Bank;
use mmk_host::audio::AudioOut;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::web::EventLoopExtWebSys;

use crate::app::{App, UserEvent, log};
use crate::audio_out::{self, Output};
use crate::game::Game;
use crate::source::{self, AssetSource, MemorySource};

#[wasm_bindgen]
extern "C" {
    /// `mmkStorage.read(path)`: a promise of the bytes (`Uint8Array`) of `path` in the imported
    /// game data, or of null when the file isn't there.
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = read)]
    fn storage_read(path: &str) -> Result<js_sys::Promise, JsValue>;
}

/// The bytes of `path` (relative to the game data root) from the page's storage.
async fn read(path: &str) -> Option<Vec<u8>> {
    let v = JsFuture::from(storage_read(path).ok()?).await.ok()?;
    if v.is_null() || v.is_undefined() {
        return None;
    }
    Some(v.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec())
}

/// The owner's game data, once opened.
struct Assets {
    cache: Arc<Cache>,
    bank: Option<Bank>,
    keys: Option<Vec<u8>>,
}

/// Page-side state; the game itself lives in the event loop's `App`, reached through `proxy`.
struct Web {
    proxy: EventLoopProxy<UserEvent>,
    assets: Option<Arc<Assets>>,
    /// The open sound output (after the first gesture) and the mixer it plays.
    output: Option<Output>,
    speaker: Arc<Mutex<Option<Arc<AudioOut>>>>,
    /// The current course's music files, read before it starts (the mixer reads them
    /// synchronously, the storage can't).
    music: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

thread_local! {
    static WEB: RefCell<Option<Web>> = const { RefCell::new(None) };
    /// Texture reads on their way, and frames the game has been held for them.
    static IN_FLIGHT: Cell<usize> = const { Cell::new(0) };
    static HELD: Cell<u32> = const { Cell::new(0) };
}

/// Frames the game waits at most for texture reads (then it plays on; the art pops in).
const MAX_HOLD: u32 = 30;

fn with_web<R>(f: impl FnOnce(&mut Web) -> R) -> Result<R, JsValue> {
    WEB.with(|w| w.borrow_mut().as_mut().map(f))
        .ok_or_else(|| JsValue::from_str("mmk-play has not started"))
}

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
    if web_sys::window().is_none() {
        // loaded in a worker (the page's importer): no window, no game
        return;
    }
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
    let proxy = event_loop.create_proxy();
    app.set_proxy(proxy.clone());
    WEB.with(|w| {
        *w.borrow_mut() = Some(Web {
            proxy,
            assets: None,
            output: None,
            speaker: Arc::default(),
            music: Arc::default(),
        })
    });
    event_loop.spawn_app(app);
}

/// Start the reads of the textures `cache` was asked for. True while the game should wait for
/// them (reads on their way, for at most [`MAX_HOLD`] frames in a row).
pub fn pump_textures(cache: &Arc<Cache>) -> bool {
    for (i, rel) in cache.take_requests() {
        IN_FLIGHT.set(IN_FLIGHT.get() + 1);
        let cache = cache.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes = read(&rel).await;
            if bytes.is_none() {
                log(&format!("texture missing: {rel}"));
            }
            cache.provide(i, bytes.as_deref());
            IN_FLIGHT.set(IN_FLIGHT.get().saturating_sub(1));
        });
    }
    let hold = IN_FLIGHT.get() > 0 && HELD.get() < MAX_HOLD;
    HELD.set(if hold { HELD.get() + 1 } else { 0 });
    hold
}

/// A mixer for `bank` at the output's rate, reading course music from the prefetched files.
fn mixer(bank: Bank, rate: u32, music: &Arc<Mutex<HashMap<String, Vec<u8>>>>) -> Arc<AudioOut> {
    let files = music.clone();
    Arc::new(
        AudioOut::with_rate(bank, rate)
            .with_music(Box::new(move |name| files.lock().ok()?.get(name).cloned())),
    )
}

/// Give the game a mixer if both the sound output and a sound bank are there.
fn connect_sound(web: &mut Web) {
    let (Some(output), Some(assets)) = (&web.output, &web.assets) else {
        return;
    };
    let Some(bank) = assets.bank.clone() else {
        return;
    };
    let out = mixer(bank, output.rate, &web.music);
    if let Ok(mut s) = web.speaker.lock() {
        *s = Some(out.clone());
    }
    let _ = web.proxy.send_event(UserEvent::Sound(out));
}

/// Call from a user gesture (pointer or key event): opens the sound output the first time, and
/// resumes it if the browser suspended it.
#[wasm_bindgen]
pub fn unlock_audio() -> Result<(), JsValue> {
    with_web(|web| {
        if let Some(o) = &web.output {
            return o.play().map_err(|e| JsValue::from_str(&e));
        }
        let speaker = web.speaker.clone();
        let output =
            audio_out::open(
                move |buf| match speaker.lock().ok().and_then(|s| s.clone()) {
                    Some(out) => out.render(buf),
                    None => buf.fill(0.0),
                },
            )
            .map_err(|e| JsValue::from_str(&e))?;
        web.output = Some(output);
        connect_sound(web);
        Ok(())
    })?
}

/// Files read when the game data is opened (the rest is read on demand).
const INDEX_FILES: &[&str] = &[
    "manifest.json",
    "textures/index.json",
    "sprites.json",
    "audio/index.json",
    mmk_host::course::KEY_TABLE,
];

/// Open the game data in the page's storage and play the demo strip with it. Resolves to a
/// summary (`{build, textures, sounds}`); rejects if there is no usable data.
#[wasm_bindgen]
pub async fn open_assets() -> Result<JsValue, JsValue> {
    let mut files = MemorySource::new("browser storage");
    for rel in INDEX_FILES {
        if let Some(bytes) = read(rel).await {
            files.insert(*rel, bytes);
        }
    }
    // the sound bank: every sound the audio index lists (a few MB; music is read per course)
    if let Ok(raw) = files.read("audio/index.json") {
        let index: serde_json::Value = serde_json::from_slice(&raw).unwrap_or_default();
        if let Some(map) = index.as_object() {
            for entry in map.values() {
                if let Some(file) = entry["file"].as_str() {
                    let rel = format!("audio/{file}");
                    if let Some(bytes) = read(&rel).await {
                        files.insert(rel, bytes);
                    }
                }
            }
        }
    }
    let cache = source::open_cache_deferred(&files).map_err(|e| JsValue::from_str(&e))?;
    let bank = match source::open_bank(&files) {
        Ok(b) => Some(b),
        Err(e) => {
            log(&format!("sound off: {e}"));
            None
        }
    };
    let summary = serde_json::json!({
        "build": cache.manifest["build"],
        "textures": cache.textures.len(),
        "sounds": bank.as_ref().map_or(0, Bank::len),
    });
    let assets = Arc::new(Assets {
        cache: Arc::new(cache),
        bank,
        keys: files.read(mmk_host::course::KEY_TABLE).ok(),
    });
    log(&format!("assets: browser storage, {summary}"));
    with_web(|web| {
        web.assets = Some(assets.clone());
        connect_sound(web);
        let game = Game::with_cache(assets.cache.clone(), false, None);
        let _ = web.proxy.send_event(UserEvent::Game(Box::new(game)));
    })?;
    Ok(JsValue::from_str(&summary.to_string()))
}

/// Forget the opened game data (the page removed it): back to placeholder art.
#[wasm_bindgen]
pub fn close_assets() -> Result<(), JsValue> {
    with_web(|web| {
        web.assets = None;
        if let Ok(mut s) = web.speaker.lock() {
            *s = None;
        }
        if let Ok(g) = Game::load(None, None) {
            let _ = web.proxy.send_event(UserEvent::Game(Box::new(g)));
        }
    })
}

/// Play a course: SMM2 course data, raw or zlib-compressed, or an encrypted `.bcd` (needs the
/// opened game data's key table). Empty bytes play the demo strip. Resolves to
/// `{name, style, theme}`.
#[wasm_bindgen]
pub async fn play_course(bytes: Vec<u8>) -> Result<JsValue, JsValue> {
    let assets = with_web(|w| w.assets.clone())?;
    let course = if bytes.is_empty() {
        None
    } else {
        let keys = assets.as_ref().and_then(|a| a.keys.clone());
        let c = mmk_host::course::decode(&bytes, || {
            keys.ok_or_else(|| mmk_host::course::no_key_table("no game data imported"))
        })
        .map_err(|e| JsValue::from_str(&e))?;
        Some(c)
    };
    let name = course
        .as_ref()
        .map_or_else(|| "Demo".to_string(), |c| c.header.name.to_string_lossy());
    let game = match &assets {
        Some(a) => Game::with_cache(a.cache.clone(), false, course),
        None => Game::with_cache(Arc::new(Cache::default()), true, course),
    };
    let level = &game.session.world.level;
    let (style, theme) = (level.style.clone(), level.theme);
    if assets.is_some() {
        // the course's music, before the mixer asks for it
        let mut music = HashMap::new();
        for names in mmk_host::audio::music_candidates(&style, theme) {
            for n in names {
                if let Some(bytes) = read(&format!("audio/music/{n}.bwav")).await {
                    music.insert(n, bytes);
                    break;
                }
            }
        }
        with_web(|w| {
            if let Ok(mut m) = w.music.lock() {
                *m = music;
            }
        })?;
    }
    with_web(|w| w.proxy.send_event(UserEvent::Game(Box::new(game))))?
        .map_err(|_| JsValue::from_str("the game has stopped"))?;
    let info = serde_json::json!({ "name": name, "style": style, "theme": theme });
    Ok(JsValue::from_str(&info.to_string()))
}
