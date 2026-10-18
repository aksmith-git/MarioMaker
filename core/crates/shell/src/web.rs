//! Web entry (wasm32): the shell in a `<canvas id="mmk">` (created if the page has none), drawn
//! with WebGPU where available and WebGL2 otherwise. It starts on the built-in demo strip with
//! placeholder art; the page (`web/mmk-web.js`) then hands over the owner's game data and
//! courses through the functions exported here.
//!
//! - **Game data** lives in the browser's origin private file system (OPFS), copied there by the
//!   page from an asset cache folder the owner picked, or imported from their dump's RomFS and
//!   ExeFS in a worker ([`crate::web_import`]); nothing is uploaded anywhere. Rust reads it through
//!   the page's `mmkStorage.read(path)` (a promise of the file's bytes, or null). The texture
//!   index, sprites, sounds and key table are read when the data is opened ([`open_assets`]);
//!   texture pixels are read when a sprite first needs them ([`pump_textures`]), and a course's
//!   music when the course starts ([`play_course`]), so memory holds what is being played, not
//!   the 1.4 GB cache.
//! - **The SMM2 menus** (`mmk-front`, as `mmk-play` runs them natively) start when the page has
//!   an SMM2 UI import (`mmk import-smm2-ui`) in its storage ([`open_front`]). The UI cache reads
//!   through an in-memory file source: the import's layouts, fonts' metrics and message tables
//!   (13 MB of JSON) are read before the menus open; its textures (170 MB) are read when a screen
//!   first shows them ([`pump_ui`]: the cache records what it was asked for, the page reads it,
//!   the cache gets the pixels; the front end waits a few frames for them, as the game does for
//!   its own); its sounds are read in the background. Courses the page plays then play through
//!   the front end's screens.
//! - **Sound** goes out through cpal's WebAudio backend, which browsers only let start after a
//!   user gesture: the page calls [`unlock_audio`] from its first pointer or key event.
//! - **Gamepads** come from the Gamepad API through gilrs (`input`), keys and clicks from the
//!   canvas.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mmk_assets::Cache;
use mmk_audio::Bank;
use mmk_front::{Front, Host, Options, Source, Ui, UiKind, UiSounds};
use mmk_host::audio::AudioOut;
use mmk_ui::{MemFiles, UiCache};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::web::EventLoopExtWebSys;

use crate::app::{App, UserEvent, log};
use crate::audio_out::{self, Output};
use crate::front_app::FrontApp;
use crate::front_course::natural;
use crate::game::Game;
use crate::source::{self, AssetSource, MemorySource};
use crate::ui_sound::UiMixer;
use crate::web_front::{HostData, LibCourse, Library, ShellHost};

#[wasm_bindgen]
extern "C" {
    /// `mmkStorage.read(path)`: a promise of the bytes (`Uint8Array`) of `path` in the imported
    /// game data, or of null when the file isn't there.
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = read)]
    fn storage_read(path: &str) -> Result<js_sys::Promise, JsValue>;
    /// `mmkStorage.list(dir)`: a promise of the names of the files directly in `dir` of the
    /// game data.
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = list)]
    fn storage_list(dir: &str) -> Result<js_sys::Promise, JsValue>;
    /// `mmkStorage.readUi(path)`: as `read`, in the stored SMM2 UI import.
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = readUi)]
    fn storage_read_ui(path: &str) -> Result<js_sys::Promise, JsValue>;
    /// `mmkStorage.readAllUi(paths)`: a promise of the bytes (or null) of each of `paths` in the
    /// UI import, in order.
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = readAllUi)]
    fn storage_read_all_ui(paths: js_sys::Array) -> Result<js_sys::Promise, JsValue>;
    /// `mmkStorage.walkUi()`: a promise of the paths of all files of the UI import (none when
    /// there is no import).
    #[wasm_bindgen(catch, js_namespace = mmkStorage, js_name = walkUi)]
    fn storage_walk_ui() -> Result<js_sys::Promise, JsValue>;
}

fn bytes_of(v: JsValue) -> Option<Vec<u8>> {
    if v.is_null() || v.is_undefined() {
        return None;
    }
    Some(v.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec())
}

fn strings_of(v: JsValue) -> Vec<String> {
    v.dyn_into::<js_sys::Array>()
        .map(|a| a.iter().filter_map(|s| s.as_string()).collect())
        .unwrap_or_default()
}

/// The bytes of `path` (relative to the game data root) from the page's storage.
async fn read(path: &str) -> Option<Vec<u8>> {
    bytes_of(JsFuture::from(storage_read(path).ok()?).await.ok()?)
}

/// The bytes of `path` in the stored SMM2 UI import.
async fn read_ui(path: &str) -> Option<Vec<u8>> {
    bytes_of(JsFuture::from(storage_read_ui(path).ok()?).await.ok()?)
}

/// The files directly in `dir` of the game data.
async fn list(dir: &str) -> Vec<String> {
    match storage_list(dir) {
        Ok(p) => JsFuture::from(p).await.map(strings_of).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// The owner's game data, once opened.
struct Assets {
    cache: Arc<Cache>,
    bank: Option<Bank>,
    /// The gameplay sound database, to know which files a course reads.
    sounds: Option<Arc<mmk_audio::links::SoundDb>>,
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
    /// The front end runs.
    front: bool,
    /// The UI sounds' mixer (made with the sound output) and the WAVs read for it, which it
    /// takes on first use.
    ui_speaker: Arc<Mutex<Option<Arc<UiMixer>>>>,
    ui_clips: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    ui_clips_read: bool,
    /// Course files the page opened (numbering their library ids).
    opened: u32,
}

thread_local! {
    static WEB: RefCell<Option<Web>> = const { RefCell::new(None) };
    /// Texture reads on their way, and frames the game has been held for them.
    static IN_FLIGHT: Cell<usize> = const { Cell::new(0) };
    static HELD: Cell<u32> = const { Cell::new(0) };
    /// The same for the UI import's textures and the front end.
    static UI_IN_FLIGHT: Cell<usize> = const { Cell::new(0) };
    static UI_HELD: Cell<u32> = const { Cell::new(0) };
}

/// Frames the game waits at most for texture reads (then it plays on; the art pops in).
const MAX_HOLD: u32 = 30;

fn with_web<R>(f: impl FnOnce(&mut Web) -> R) -> Result<R, JsValue> {
    WEB.with(|w| w.borrow_mut().as_mut().map(f))
        .ok_or_else(|| JsValue::from_str("mmk-play has not started"))
}

/// Run `f` on the app in its event loop.
fn send(f: impl FnOnce(&mut App) + 'static) -> Result<(), JsValue> {
    with_web(|w| w.proxy.send_event(UserEvent::Run(Box::new(f))))?
        .map_err(|_| JsValue::from_str("the game has stopped"))
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
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
            front: false,
            ui_speaker: Arc::default(),
            ui_clips: Arc::default(),
            ui_clips_read: false,
            opened: 0,
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

/// Start the reads of the UI import's textures the front end's cache was asked for, and of the
/// music of a course the front end just loaded. True while the front end should wait for the
/// textures (reads on their way, for at most [`MAX_HOLD`] frames in a row).
pub fn pump_ui(front: Option<&mut FrontApp>) -> bool {
    let Some(f) = front else {
        return false;
    };
    if let Some((style, theme)) = f.data.music_wanted.take() {
        fetch_music(style, theme);
    }
    let ui = &f.front.smm2;
    let wanted = ui.cache_mut().take_requests();
    for (id, rel) in wanted {
        UI_IN_FLIGHT.set(UI_IN_FLIGHT.get() + 1);
        let ui = ui.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes = read_ui(&rel).await;
            if !ui.cache_mut().provide(id, bytes.as_deref()) {
                log(&format!("UI texture missing: {rel}"));
            }
            UI_IN_FLIGHT.set(UI_IN_FLIGHT.get().saturating_sub(1));
        });
    }
    let hold = UI_IN_FLIGHT.get() > 0 && UI_HELD.get() < MAX_HOLD;
    UI_HELD.set(if hold { UI_HELD.get() + 1 } else { 0 });
    hold
}

/// The sound files (paths under `audio/`) in `wanted`, reusing those already read and keeping
/// the sound database.
async fn read_sounds(wanted: Vec<String>) -> HashMap<String, Vec<u8>> {
    let mut have =
        with_web(|w| w.music.lock().map(|m| m.clone()).unwrap_or_default()).unwrap_or_default();
    have.retain(|k, _| k == "sound.json" || wanted.contains(k));
    for rel in wanted {
        if !have.contains_key(&rel)
            && let Some(bytes) = read(&format!("audio/{rel}")).await
        {
            have.insert(rel, bytes);
        }
    }
    have
}

/// The sound files of a course of `style` and `theme` (none without a sound database).
async fn read_music(style: &str, theme: u8) -> HashMap<String, Vec<u8>> {
    let db = with_web(|w| w.assets.as_ref().and_then(|a| a.sounds.clone()))
        .ok()
        .flatten();
    match db {
        Some(db) => read_sounds(mmk_host::audio::style_files(&db, style, theme)).await,
        None => {
            with_web(|w| w.music.lock().map(|m| m.clone()).unwrap_or_default()).unwrap_or_default()
        }
    }
}

/// A course the front end loaded: read its music, then start the course's sound again with it
/// (until then the sound bank's own music plays).
fn fetch_music(style: String, theme: u8) {
    let Ok((music, has_assets)) = with_web(|w| (w.music.clone(), w.assets.is_some())) else {
        return;
    };
    if !has_assets {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let files = read_music(&style, theme).await;
        log(&format!(
            "music: {style} theme {theme}: {} tracks",
            files.len()
        ));
        if let Ok(mut m) = music.lock() {
            *m = files;
        }
        let _ = send(move |app| {
            let Some(f) = app.front.as_mut() else { return };
            let level = &app.game.session.world.level;
            if level.style == style && level.theme == theme && app.game.session.audio.is_some() {
                app.game.session.set_audio(f.data.audio.clone());
            }
        });
    });
}

/// A mixer for `bank` at the output's rate, reading course music from the prefetched files.
fn mixer(bank: Bank, rate: u32, music: &Arc<Mutex<HashMap<String, Vec<u8>>>>) -> Arc<AudioOut> {
    let files = music.clone();
    Arc::new(
        AudioOut::with_rate(bank, rate)
            .with_music(Box::new(move |name| files.lock().ok()?.get(name).cloned())),
    )
}

/// Give the game (and the front end) a mixer if both the sound output and a sound bank are
/// there.
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
    let _ = web.proxy.send_event(UserEvent::Sound(out.clone()));
    let _ = web.proxy.send_event(UserEvent::Run(Box::new(move |app| {
        if let Some(f) = app.front.as_mut() {
            f.data.audio = Some(out);
        }
    })));
}

/// The UI sounds' mixer once the sound output is open (one per output).
fn ui_mixer(web: &mut Web) -> Option<Arc<UiMixer>> {
    let rate = web.output.as_ref()?.rate;
    let mut slot = web.ui_speaker.lock().ok()?;
    if slot.is_none() {
        let clips = web.ui_clips.clone();
        let read = Box::new(move |name: &str| clips.lock().ok()?.remove(name));
        *slot = Some(Arc::new(UiMixer::with_reader(read, rate)));
    }
    slot.clone()
}

/// Hand the front end the UI sounds' mixer when there are both.
fn connect_ui_sound(web: &mut Web) {
    if !web.front {
        return;
    }
    if let Some(m) = ui_mixer(web) {
        let _ = web.proxy.send_event(UserEvent::Run(Box::new(move |app| {
            if let Some(f) = app.front.as_mut() {
                f.ui_audio = Some(m);
            }
        })));
    }
}

/// Call from a user gesture (pointer or key event): opens the sound output the first time, and
/// resumes it if the browser suspended it.
#[wasm_bindgen]
pub fn unlock_audio() -> Result<(), JsValue> {
    with_web(|web| {
        if let Some(o) = &web.output {
            return o.play().map_err(|e| JsValue::from_str(&e));
        }
        let (speaker, ui) = (web.speaker.clone(), web.ui_speaker.clone());
        let output = audio_out::open(move |buf| {
            match speaker.lock().ok().and_then(|s| s.clone()) {
                Some(out) => out.render(buf),
                None => buf.fill(0.0),
            }
            if let Some(m) = ui.lock().ok().and_then(|s| s.clone()) {
                m.mix(buf);
                for s in buf.iter_mut() {
                    *s = s.clamp(-1.0, 1.0);
                }
            }
        })
        .map_err(|e| JsValue::from_str(&e))?;
        web.output = Some(output);
        connect_sound(web);
        connect_ui_sound(web);
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
    // the gameplay sound database (its waves are read per course, see `play_course`)
    let sound_db = read("audio/sound.json").await;
    let sounds = sound_db
        .as_deref()
        .and_then(|b| mmk_audio::links::SoundDb::parse(b).ok())
        .map(Arc::new);
    if let Some(b) = sound_db {
        with_web(|w| {
            if let Ok(mut m) = w.music.lock() {
                m.insert("sound.json".into(), b);
            }
        })?;
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
        sounds,
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

/// Play the current course online through the relay at `url` (the course server's WebSocket,
/// e.g. `ws://host:8641`): room `room`, `players` players (2–4), we are player `player` (1-based).
/// Everyone in the room must be playing the same course; each waits for the others to start.
#[wasm_bindgen]
pub fn net_join(url: &str, room: &str, player: usize, players: usize) -> Result<(), JsValue> {
    let me = player.clamp(1, 4) - 1;
    let link = crate::web_net::WsLink::connect(url, room, me as u8)?;
    with_web(|w| {
        w.proxy
            .send_event(UserEvent::Net(Box::new(link), players, me))
    })?
    .map_err(|_| JsValue::from_str("the game has stopped"))
}

/// Play a course: SMM2 course data, raw or zlib-compressed, or an encrypted `.bcd` (needs the
/// opened game data's key table). Empty bytes play the demo strip (with the front end: back to
/// the title). While the front end runs, the course plays through its screens (course card,
/// HUD, pause menu, clear). `name`: the file's name (names a course without one of its own).
/// Resolves to `{name, style, theme}`.
#[wasm_bindgen]
pub async fn play_course(bytes: Vec<u8>, name: Option<String>) -> Result<JsValue, JsValue> {
    let (assets, front) = with_web(|w| (w.assets.clone(), w.front))?;
    if front {
        return play_in_front(bytes, name, assets.as_ref().and_then(|a| a.keys.clone()));
    }
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
    if let Some(db) = assets.as_ref().and_then(|a| a.sounds.clone()) {
        // the course's sounds and music, before the mixer asks for them (others are read
        // when first played and miss that play)
        let have = read_sounds(mmk_host::audio::course_files(&db, &game.session.world)).await;
        with_web(|w| {
            if let Ok(mut m) = w.music.lock() {
                *m = have;
            }
        })?;
    }
    with_web(|w| w.proxy.send_event(UserEvent::Game(Box::new(game))))?
        .map_err(|_| JsValue::from_str("the game has stopped"))?;
    let info = serde_json::json!({ "name": name, "style": style, "theme": theme });
    Ok(JsValue::from_str(&info.to_string()))
}

/// [`play_course`] with the front end: the course joins the library and plays with the course
/// card first; leaving it goes back to the title's Play menu.
fn play_in_front(
    bytes: Vec<u8>,
    name: Option<String>,
    keys: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    if bytes.is_empty() {
        send(|app| {
            if let Some(f) = app.front.as_mut() {
                let mut host = ShellHost {
                    game: &mut app.game,
                    data: &mut f.data,
                };
                f.front.capture(&mut host, "title", 0, 0);
            }
        })?;
        let info = serde_json::json!({ "name": "", "style": "", "theme": 0 });
        return Ok(JsValue::from_str(&info.to_string()));
    }
    let n = with_web(|w| {
        w.opened += 1;
        w.opened
    })?;
    let file = name.as_deref().map_or("file", stem);
    let c = LibCourse::new(format!("file:{n}"), file, false, &bytes, keys.as_deref())
        .map_err(|e| JsValue::from_str(&e))?;
    let info = serde_json::json!({
        "name": c.entry.name, "style": c.entry.style, "theme": c.entry.theme,
    });
    let id = c.entry.id.clone();
    send(move |app| {
        let Some(f) = app.front.as_mut() else { return };
        let fallback = c.entry.name.clone();
        f.data.library.borrow_mut().push(c);
        let mut host = ShellHost {
            game: &mut app.game,
            data: &mut f.data,
        };
        match host.load_course(&Source::Library(id)) {
            Ok(l) => {
                let name = if l.name.is_empty() { fallback } else { l.name };
                f.front.play_course(&mut host, &name, l.key, true);
            }
            Err(e) => log(&format!("course: {e}")),
        }
    })?;
    Ok(JsValue::from_str(&info.to_string()))
}

/// The game data's built-in course files (`courses/`), in their numbering order.
async fn builtin_courses() -> Vec<String> {
    let mut v: Vec<String> = list("courses")
        .await
        .into_iter()
        .filter(|n| [".bcd", ".zlib", ".bin"].iter().any(|e| n.ends_with(e)))
        .collect();
    v.sort_by(|a, b| natural(a, b));
    v
}

/// A file name without its extensions (`quest_001.bcd` → `quest_001`).
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name)
}

/// Start the SMM2 menus from the UI import in the page's storage (`opening`: play the opening
/// first), with the opened game data's art, sounds and courses if there are any; again
/// restarts them (e.g. after the game data changed). Resolves to `{build, layouts, title}`;
/// rejects without an import.
#[wasm_bindgen]
pub async fn open_front(opening: bool) -> Result<JsValue, JsValue> {
    let t0 = js_sys::Date::now();
    let paths = strings_of(JsFuture::from(storage_walk_ui()?).await?);
    if !paths.iter().any(|p| p == "manifest.json") {
        return Err(JsValue::from_str(
            "no SMM2 UI import in this browser's storage",
        ));
    }
    // layouts, fonts' metrics and message tables, and (once) the sounds (the opening's start at
    // once) now; textures on request
    let mut files = MemFiles::new("browser storage (SMM2 UI import)");
    let (mut now, mut wavs, mut courses) = (Vec::new(), Vec::new(), Vec::new());
    for p in paths {
        if p.ends_with(".mtex") {
            files.insert_later(p);
        } else if p.ends_with(".wav") {
            wavs.push(p);
        } else if p.ends_with(".json") {
            now.push(p);
        } else if p.starts_with("courses/") {
            courses.push(p);
        }
    }
    let array = |paths: &[String]| {
        let a = js_sys::Array::new();
        for p in paths {
            a.push(&JsValue::from_str(p));
        }
        a
    };
    let first = with_web(|w| !std::mem::replace(&mut w.ui_clips_read, true))?;
    let wav_reads = if first {
        Some(storage_read_all_ui(array(&wavs))?)
    } else {
        None
    };
    let got: js_sys::Array = JsFuture::from(storage_read_all_ui(array(&now))?)
        .await?
        .dyn_into()?;
    let mut sounds = None;
    for (p, v) in now.iter().zip(got.iter()) {
        let Some(b) = bytes_of(v) else { continue };
        if p == "ui/sound/sounds.json" {
            sounds = UiSounds::from_json(&String::from_utf8_lossy(&b));
        } else {
            files.insert(p.clone(), b);
        }
    }
    let cache = UiCache::from_files(Box::new(files)).map_err(js_err)?;
    let layouts = cache.layout_names().len();
    let smm2 = Ui::new(UiKind::Smm2, cache, sounds);
    let build = smm2.build();

    // title demo courses: the game data's, else the UI import's copies (both need the key table)
    let assets = with_web(|w| w.assets.clone())?;
    let keys = assets.as_ref().and_then(|a| a.keys.clone());
    let builtin = if assets.is_some() {
        builtin_courses().await
    } else {
        Vec::new()
    };
    let is_title = |p: &String| stem(p).starts_with("title_course_data_");
    let mut title = Vec::new();
    for n in builtin.iter().filter(|n| is_title(n)) {
        if let Some(b) = read(&format!("courses/{n}")).await
            && let Ok(c) =
                LibCourse::new(format!("builtin:{n}"), stem(n), true, &b, keys.as_deref())
        {
            title.push(c);
        }
    }
    if title.is_empty() && keys.is_some() {
        courses.sort_by(|a, b| natural(a, b));
        for p in courses.iter().filter(|p| is_title(p)) {
            if let Some(b) = read_ui(p).await
                && let Ok(c) = LibCourse::new(format!("ui:{p}"), stem(p), true, &b, keys.as_deref())
            {
                title.push(c);
            }
        }
    }
    if title.iter().any(|c| c.entry.playable) {
        title.retain(|c| c.entry.playable);
    }
    let titles = title.len();

    // the library: the game data's built-in courses, read in the background (Course World reads
    // whatever has arrived when it opens)
    let library: Library = Rc::default();
    {
        let (library, keys) = (library.clone(), keys.clone());
        wasm_bindgen_futures::spawn_local(async move {
            for n in builtin {
                if let Some(b) = read(&format!("courses/{n}")).await
                    && let Ok(c) =
                        LibCourse::new(format!("builtin:{n}"), stem(&n), true, &b, keys.as_deref())
                {
                    library.borrow_mut().push(c);
                }
            }
            log(&format!("library: {} courses", library.borrow().len()));
        });
    }

    let mut game = match &assets {
        Some(a) => Game::with_cache(a.cache.clone(), false, None),
        None => Game::with_cache(Arc::new(Cache::default()), true, None),
    };
    game.no_hud = true;
    if let Some(p) = wav_reads {
        let got = js_sys::Array::from(&JsFuture::from(p).await?);
        let clips = with_web(|w| w.ui_clips.clone())?;
        if let Ok(mut m) = clips.lock() {
            for (p, v) in wavs.iter().zip(got.iter()) {
                if let Some(b) = bytes_of(v) {
                    m.insert(stem(p).to_string(), b);
                }
            }
        }
    }
    let mut data = HostData::new(title, library);
    data.audio = with_web(|w| w.speaker.lock().ok().and_then(|s| s.clone()))?;
    let opts = Options {
        boot: opening,
        ..Default::default()
    };
    let front = {
        let mut host = ShellHost {
            game: &mut game,
            data: &mut data,
        };
        Front::new(smm2, None, opts, &mut host)
    };
    let mut app = FrontApp::new(front, data);
    with_web(|w| {
        w.front = true;
        app.ui_audio = ui_mixer(w);
    })?;
    send(move |a| {
        a.replace_game(game);
        a.front = Some(Box::new(app));
    })?;
    let ms = (js_sys::Date::now() - t0).round();
    let summary =
        serde_json::json!({ "build": build, "layouts": layouts, "title": titles, "ms": ms });
    log(&format!("front end: browser storage, {summary}"));
    Ok(JsValue::from_str(&summary.to_string()))
}

/// Stop the menus (the page removed the UI import): back to the plain game.
#[wasm_bindgen]
pub fn close_front() -> Result<(), JsValue> {
    with_web(|w| w.front = false)?;
    send(|app| {
        app.front = None;
        app.game.no_hud = false;
    })
}
