//! C ABI for the SMM2 front end (`mmk-front`): the opening, title, Course World, 100 Mario and the
//! in-course screens, the same screens `mmk-play` shows, driven by the Swift app. Declared in
//! `app/Sources/CMmk/mmk.h` (section "front end").
//!
//! The app creates an engine (`mmk_engine_new`, with its options) and hands it to
//! `mmk_front_new` with the UI imports and a small table of C callbacks (persistent values, the
//! pause volume). The front end's [`Host`] is implemented here over that engine (load, step,
//! restart, status), the library the app pushes (`mmk_front_set_library`, JSON), the course server
//! (`mmk-online`, on threads) and the callbacks. Per 60 Hz tick the app calls `mmk_front_tick`
//! with its menu buttons and engine buttons, then draws `mmk_front_frame` (layout quads with the
//! textures of `mmk_front_texture`, the course view where an item says so) and plays
//! `mmk_front_sounds` (WAVs of the SMM2 UI import's `ui/sound/`).

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use mmk_front::host::{Condition, CourseMeta, LoadedCourse};
use mmk_front::{
    Buttons, CourseEntry, CourseStatus, DrawItem, Front, FrontInput, Host, Options, Source, Ui,
    UiKind, WorldCourse,
};
use mmk_sim::player::{Control, DOWN_FALL_FRAMES};
use mmk_sim::world::Event;
use mmk_sim::{Input, World};
use serde_json::{Value, json};

use crate::audio::MmkAudio;
use crate::ui::MmkUiQuad;
use crate::{MmkAssets, MmkEngine, MmkTextureInfo, guard, set_error};

/// The app's side of the host (`MmkFrontHost` in mmk.h). Callbacks run on the thread that calls
/// `mmk_front_tick` / `mmk_front_new` (the main thread).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MmkFrontHost {
    pub user: *mut c_void,
    /// Value `key` (save data, settings): write it NUL-terminated into `buf` when it fits in
    /// `cap` and return its length; −1 when there is none.
    pub load_value: Option<
        unsafe extern "C" fn(
            user: *mut c_void,
            key: *const c_char,
            buf: *mut c_char,
            cap: usize,
        ) -> i64,
    >,
    pub store_value:
        Option<unsafe extern "C" fn(user: *mut c_void, key: *const c_char, value: *const c_char)>,
    /// The play is paused (turn the music down) or resumed.
    pub set_paused: Option<unsafe extern "C" fn(user: *mut c_void, paused: bool)>,
}

/// How the front end starts (`MmkFrontOptions`).
#[repr(C)]
pub struct MmkFrontOptions {
    /// The SMM2 UI import folder (required).
    pub smm2_ui: *const c_char,
    /// The SMM1 import folder (the SMM1-layout 100 Mario), or NULL.
    pub smm1_ui: *const c_char,
    /// Course server base URL, or NULL / "" for none.
    pub server: *const c_char,
    /// Where server courses are downloaded (created when needed), or NULL (the temp folder).
    pub downloads: *const c_char,
    /// Force this opening (InDefault, InLink …), or NULL.
    pub opening: *const c_char,
    /// Captures: menu presses at global UI ticks (`MMK_UI_SCRIPT`), or NULL.
    pub script: *const c_char,
    /// Captures: title presses at title ticks (`MMK_UI_INPUT`), or NULL.
    pub title_input: *const c_char,
    /// `MMK_FRONT_*` flags.
    pub flags: u32,
    /// The title's first state without an opening: 0 press-a-button, 1 Make / Play, 2 Play menu.
    pub title_start: u32,
    /// The Play menu's button to start on, −1 none.
    pub title_select: i32,
    /// The title demo course, −1 random.
    pub title_course: i32,
}

/// Play the opening first.
pub const MMK_FRONT_BOOT: u32 = 1;
/// Scene changes at once (captures).
pub const MMK_FRONT_INSTANT_FADES: u32 = 2;
/// 100 Mario with SMM1's layouts.
pub const MMK_FRONT_HUNDRED_SMM1: u32 = 4;

/// One tick's input (`MmkFrontInput`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkFrontInput {
    /// Menu buttons held: 1 up, 2 down, 4 confirm, 8 back, 16 left, 32 right.
    pub menu: u32,
    /// Engine buttons (MMK_BTN_*) for the course; START is masked (pausing is `flags` bit 0).
    pub buttons: u32,
    /// bit 0 pause held, bit 1 the 100 Mario skip key held, bit 2 a click / tap this tick.
    pub flags: u32,
    /// The click, layout units (1280 × 720 view, origin centre, y up).
    pub click_x: f32,
    pub click_y: f32,
    /// A key typed this tick while a text field is open (`MMK_FRONT_TYPING`): a Unicode scalar,
    /// 8 erases, 10 finishes, 27 cancels; 0 none (one per tick).
    pub text: u32,
}

/// One thing to draw, back to front (`MmkFrontItem`, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkFrontItem {
    /// 0 a layout's quads, 1 the course view (the engine's picture, the whole 16:9 view).
    pub kind: u32,
    /// 0 the SMM2 import's textures, 1 the SMM1 import's.
    pub ui: u32,
    /// Layout units mapped onto the 16:9 view (origin centre, y up).
    pub width: f32,
    pub height: f32,
    pub quads: *const MmkUiQuad,
    pub quad_count: usize,
}

/// A UI sound to start (`MmkFrontSound`, 16 bytes): `<smm2 ui>/ui/sound/<name>.wav`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkFrontSound {
    pub name: *const c_char,
    pub volume: f32,
    /// Frames (60 per second) to wait before it starts.
    pub delay: f32,
}

/// What the app needs to know about the front end now (`MmkFrontStatus`, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkFrontStatus {
    /// `MMK_FRONT_*` status bits.
    pub flags: u32,
    pub _pad: u32,
    /// The engine's tick (for scripted smoke-test input).
    pub engine_tick: u64,
}

/// The last frame showed the course view.
pub const MMK_FRONT_COURSE_SHOWN: u32 = 1;
/// The player left a course started with `mmk_front_play_course`.
pub const MMK_FRONT_COURSE_DONE: u32 = 2;
/// A course is being played (not the title's demo).
pub const MMK_FRONT_PLAYING: u32 = 4;
/// No screen (the import lacks the title's layouts).
pub const MMK_FRONT_NO_SCREEN: u32 = 8;
/// A text field is open: send typed keys (`MmkFrontInput::text`), keep letters off the menus.
pub const MMK_FRONT_TYPING: u32 = 16;

/// The front end behind `mmk_front_*`.
pub struct MmkFront {
    front: Front,
    host: FfiHost,
    frame: Vec<DrawItem>,
    items: Vec<MmkFrontItem>,
    toast: Option<String>,
    sounds: Vec<MmkFrontSound>,
    sound_names: Vec<CString>,
    smm2_names: Vec<CString>,
    smm1_names: Vec<CString>,
    course_shown: bool,
}

/// The [`Host`] over the app's engine.
struct FfiHost {
    engine: *mut MmkEngine,
    assets: *const MmkAssets,
    audio: *const MmkAudio,
    cb: MmkFrontHost,
    ui_dir: PathBuf,
    library: Vec<CourseEntry>,
    server: Option<String>,
    downloads: Option<PathBuf>,
    /// Engine buttons this tick.
    input: u32,
    title: Option<Vec<PathBuf>>,
    servers: Option<Receiver<Vec<WorldCourse>>>,
    online_tx: Sender<(u32, Result<Value, String>)>,
    online_rx: Receiver<(u32, Result<Value, String>)>,
    rng: u64,
    /// Recording the run as a replay (server courses), and the last clear's.
    recording: bool,
    replay: mmk_modes::replay::Replay,
    cleared_replay: Option<mmk_modes::replay::Replay>,
}

impl FfiHost {
    fn engine(&mut self) -> &mut MmkEngine {
        // SAFETY: the app keeps the engine alive while the front end lives (mmk_front_new)
        unsafe { &mut *self.engine }
    }

    fn engine_ref(&self) -> &MmkEngine {
        unsafe { &*self.engine }
    }

    fn assets(&self) -> &MmkAssets {
        unsafe { &*self.assets }
    }

    /// Load course bytes into the engine; its music when sound is on.
    fn load_bytes(&mut self, bytes: &[u8]) -> Result<LoadedCourse, String> {
        let (data, _) = crate::decode_course_data(bytes, Some(self.assets()))?;
        let course = mmk_formats::smm2::Course::parse(&data).map_err(|e| e.to_string())?;
        let loaded = LoadedCourse {
            name: course.header.name.to_string_lossy(),
            key: Some(format!("{:016x}", crate::course_info::fnv1a64(&data))),
            theme: course.main.header.theme,
        };
        let audio = self.audio;
        let e = self.engine();
        e.world = World::from_course(&course);
        e.world.set_options(e.options);
        e.course = Some(course);
        e.resolver.reset();
        e.planner.reset();
        // the course's own music (the director is made per course)
        e.director = unsafe { audio.as_ref() }.map(|a| {
            let mut d = a.director();
            d.load_music(a, &e.world.level.style, e.world.level.theme);
            d.start_course(&e.world);
            d
        });
        self.replay = mmk_modes::replay::Replay::new();
        self.cleared_replay = None;
        Ok(loaded)
    }

    fn load_file(&mut self, path: &Path) -> Result<LoadedCourse, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.load_bytes(&bytes)
    }

    /// The title demo courses the engine can play (the asset cache's, else the UI import's), in
    /// their numbering order.
    fn title_files(&mut self) -> &[PathBuf] {
        if self.title.is_none() {
            let root = self.assets().cache.root.clone();
            let dirs = [root.join("courses"), self.ui_dir.join("courses")];
            let mut list: Vec<PathBuf> = Vec::new();
            for dir in dirs {
                if !list.is_empty() {
                    break;
                }
                let Ok(rd) = std::fs::read_dir(&dir) else {
                    continue;
                };
                list = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("title_course_data_"))
                    })
                    .collect();
                list.sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()));
            }
            let assets = self.assets();
            let playable: Vec<PathBuf> = list
                .iter()
                .filter(|p| {
                    std::fs::read(p)
                        .ok()
                        .and_then(|b| crate::decode_course(&b, assets).ok())
                        .is_some_and(|c| playable(&c))
                })
                .cloned()
                .collect();
            self.title = Some(if playable.is_empty() { list } else { playable });
        }
        self.title.as_deref().unwrap_or(&[])
    }

    fn spawn_online(&self, id: u32, f: impl FnOnce() -> Result<Value, String> + Send + 'static) {
        let tx = self.online_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((id, f()));
        });
    }
}

fn playable(c: &mmk_formats::smm2::Course) -> bool {
    let style = c.header.game_style.as_str_lossy();
    mmk_sim::level::PLAYABLE_STYLES.contains(&style.as_str())
        && mmk_sim::level::unsupported_features(c).is_empty()
}

/// Numbers inside names compare by value ("course 2" before "course 10").
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut text = String::new();
        let mut num = String::new();
        for ch in s.chars() {
            if ch.is_ascii_digit() {
                num.push(ch);
            } else {
                if !num.is_empty() {
                    out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                    num.clear();
                }
                text.extend(ch.to_lowercase());
            }
        }
        out.push((text, num.parse().unwrap_or(0)));
        out
    };
    key(a).cmp(&key(b))
}

impl Host for FfiHost {
    fn library(&mut self) -> Vec<CourseEntry> {
        self.library.clone()
    }

    fn request_server_courses(&mut self) -> bool {
        let Some(server) = self.server.clone() else {
            return false;
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let found = mmk_online::Client::new(server)
                .search("", None, "popular", 0, 100)
                .unwrap_or_default();
            let list = found
                .iter()
                .filter_map(|l| WorldCourse::server(&mmk_online::listing_json(l)))
                .collect();
            let _ = tx.send(list);
        });
        self.servers = Some(rx);
        true
    }

    fn poll_server_courses(&mut self) -> Option<Vec<WorldCourse>> {
        let r = self.servers.as_ref()?.try_recv().ok()?;
        self.servers = None;
        Some(r)
    }

    fn load_course(&mut self, source: &Source) -> Result<LoadedCourse, String> {
        match source {
            Source::Library(path) => {
                self.recording = false;
                self.load_file(Path::new(path))
            }
            Source::Server(code) => {
                let server = self.server.clone().ok_or("no course server")?;
                let dir = self
                    .downloads
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("Course World"));
                let _ = std::fs::create_dir_all(&dir);
                let file = dir.join(format!("{code}.bin"));
                if !file.is_file() {
                    let bytes = mmk_online::Client::new(server)
                        .download(code)
                        .map_err(|e| e.to_string())?;
                    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
                }
                // server courses keep the run as a replay (sent on a clear when signed in)
                self.recording = true;
                self.load_file(&file)
            }
        }
    }

    fn title_courses(&mut self) -> usize {
        self.title_files().len()
    }

    fn load_title_course(&mut self, index: usize) -> bool {
        let Some(p) = self.title_files().get(index).cloned() else {
            return false;
        };
        self.recording = false;
        self.load_file(&p).is_ok()
    }

    fn unload_course(&mut self) {
        let audio = self.audio;
        if let Some(a) = unsafe { audio.as_ref() } {
            a.send(mmk_audio::Command::StopAll);
        }
        let e = self.engine();
        e.director = None;
        e.course = None;
        e.world = World::new();
        e.world.set_options(e.options);
        e.resolver.reset();
        e.planner.reset();
        self.recording = false;
    }

    fn step_course(&mut self, scripted: Option<u32>) {
        let b = scripted.unwrap_or(self.input) & !mmk_sim::input::START;
        let input = Input { buttons: b };
        if self.recording && scripted.is_none() {
            self.replay.push(input);
        }
        let e = self.engine();
        e.world.step(input);
        if let Some(d) = &mut e.director {
            d.after_step(&e.world);
        }
    }

    fn course_status(&self) -> CourseStatus {
        let e = self.engine_ref();
        let w = &e.world;
        let h = w.hud();
        let dying = !w.player.alive();
        let restart_in = match w.player.control {
            Control::DownPose { timer } => timer + DOWN_FALL_FRAMES,
            Control::DownFall { timer } => timer,
            _ => -1,
        };
        let condition = crate::condition::goal_code(w.clear.goal).map(|(kind, target)| Condition {
            kind,
            target,
            count: w.clear.count,
            needed: w.clear.n,
            met: w.clear.met(w.player.at_goal()),
            violated: w.clear.violated,
        });
        CourseStatus {
            score: h.score,
            coins: h.coins,
            lives: h.lives,
            time: h.time,
            cleared: h.cleared,
            dying,
            goal_event: w.events.contains(&Event::Goal),
            lives_gained: h.lives_gained,
            keys: w.keys,
            pink_coins: w.pink.len() as u32,
            pink_total: w.pink_total as u32,
            style: w.style as u32,
            character: w.main_char as u32,
            checkpoint: w.checkpoint.is_some(),
            time_up: dying && w.time == 0,
            player_x: w.player.x - w.camera_x,
            player_y: w.player.y - w.camera_y,
            restart_in,
            clear_color: mmk_host::clear_color(w, &self.assets().cache),
            condition,
        }
    }

    fn restart_course(&mut self) {
        self.replay = mmk_modes::replay::Replay::new();
        let e = self.engine();
        e.world = match &e.course {
            Some(c) => World::from_course(c),
            None => World::new(),
        };
        e.world.set_options(e.options);
        if let Some(d) = &mut e.director {
            d.start_course(&e.world);
        }
    }

    fn set_character(&mut self, character: u32) {
        self.engine().world.main_char = (character as usize).min(3);
    }

    fn set_run_counters(&mut self, score: u32, coins: u32, lives: u32) {
        self.engine().world.set_run_counters(score, coins, lives);
    }

    fn local_offset(&self, secs: i64) -> i64 {
        local_offset(secs)
    }

    fn set_paused(&mut self, paused: bool) {
        if let Some(f) = self.cb.set_paused {
            unsafe { f(self.cb.user, paused) };
        }
    }

    fn report(&mut self, code: &str, what: &str) {
        let Some(server) = self.server.clone() else {
            return;
        };
        let (code, what) = (code.to_string(), what.to_string());
        std::thread::spawn(move || {
            let _ = mmk_online::Client::new(server).report(&code, &what);
        });
    }

    fn cleared(&mut self, _ticks: i64) {
        // a replay means SMM2 rules: none with options that change play
        let e = self.engine_ref();
        if self.recording && e.world.others.is_empty() && !e.world.options.affects_play() {
            self.cleared_replay = Some(std::mem::take(&mut self.replay));
        }
    }

    fn online(&mut self, id: u32, action: &str, args: &Value, token: Option<&str>) -> bool {
        let Some(server) = self.server.clone() else {
            return false;
        };
        let token = token.map(str::to_string);
        let code = args["code"].as_str().unwrap_or_default().to_string();
        match action {
            "submit_replay" => {
                let Some(replay) = self.cleared_replay.take() else {
                    self.spawn_online(id, || Err("no cleared run to send".into()));
                    return true;
                };
                self.spawn_online(id, move || {
                    let l = mmk_online::Client::new(server)
                        .with_token(token)
                        .submit_replay(&code, &replay.encode())?;
                    Ok(mmk_online::listing_json(&l))
                });
            }
            // ghosts need the engine to draw a second world, which this C API doesn't yet
            "ghost" => self.spawn_online(id, || Err("ghosts are not shown in this app yet".into())),
            _ => {
                let (action, args) = (action.to_string(), args.clone());
                self.spawn_online(id, move || {
                    mmk_online::call(&server, token.as_deref(), &action, &args)
                });
            }
        }
        true
    }

    fn poll_online(&mut self) -> Option<(u32, Result<Value, String>)> {
        self.online_rx.try_recv().ok()
    }

    fn play_options_affect_play(&self) -> bool {
        self.engine_ref().options.affects_play()
    }

    fn load_value(&mut self, key: &str) -> Option<String> {
        let f = self.cb.load_value?;
        let key = CString::new(key).ok()?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = unsafe {
                f(
                    self.cb.user,
                    key.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if n < 0 {
                return None;
            }
            let n = n as usize;
            if n < buf.len() {
                buf.truncate(n);
                return String::from_utf8(buf).ok();
            }
            buf = vec![0u8; n + 1];
        }
    }

    fn store_value(&mut self, key: &str, value: &str) {
        let (Some(f), Ok(k), Ok(v)) = (
            self.cb.store_value,
            CString::new(key),
            CString::new(value.replace('\0', " ")),
        ) else {
            return;
        };
        unsafe { f(self.cb.user, k.as_ptr(), v.as_ptr()) };
    }

    fn random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

/// Local time minus UTC at `secs` (the C library's time zone).
#[cfg(unix)]
fn local_offset(secs: i64) -> i64 {
    let t = secs as libc::time_t;
    // SAFETY: localtime_r only writes the `tm` it is given
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() { 0 } else { tm.tm_gmtoff as i64 }
}

#[cfg(not(unix))]
fn local_offset(_secs: i64) -> i64 {
    0
}

/// The library as the app sends it: `[{id, name, description, style, theme, playable, built_in,
/// collection, key, modified, meta: {clear_rate, bucket, maker, code, world_record_ms, plays,
/// likes, clears, attempts, uploaded, tags}}]`.
pub fn parse_library(text: &str) -> Vec<CourseEntry> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
            let m = &c["meta"];
            let n = |k: &str| m[k].as_i64();
            Some(CourseEntry {
                id: c["id"].as_str()?.to_string(),
                name: s("name"),
                description: s("description"),
                style: s("style"),
                theme: c["theme"].as_u64().unwrap_or(0) as u8,
                playable: c["playable"].as_bool().unwrap_or(false),
                built_in: c["built_in"].as_bool().unwrap_or(false),
                collection: c["collection"].as_str().map(str::to_string),
                key: s("key"),
                modified: c["modified"].as_i64(),
                meta: CourseMeta {
                    clear_rate: m["clear_rate"].as_f64(),
                    bucket: n("bucket").map(|b| b.clamp(0, 3) as u8),
                    maker: m["maker"].as_str().map(str::to_string),
                    code: m["code"].as_str().map(str::to_string),
                    world_record_ms: n("world_record_ms"),
                    plays: n("plays"),
                    likes: n("likes"),
                    clears: n("clears"),
                    attempts: n("attempts"),
                    uploaded: n("uploaded"),
                    tags: m["tags"]
                        .as_array()
                        .map(|t| {
                            t.iter()
                                .filter_map(|x| x.as_u64())
                                .map(|x| x as u32)
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            })
        })
        .collect()
}

fn opt_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Create the front end. `engine` and `assets` (and `audio`, may be NULL: silent) must outlive
/// it; the engine is the course view's (draw it with `mmk_engine_snapshot`). NULL on error
/// (`mmk_last_error`), e.g. no SMM2 UI import.
///
/// # Safety
/// `opts` readable with valid strings; `engine` / `assets` live handles; `audio` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_new(
    opts: *const MmkFrontOptions,
    engine: *mut MmkEngine,
    assets: *const MmkAssets,
    audio: *const MmkAudio,
    host: MmkFrontHost,
) -> *mut MmkFront {
    guard(std::ptr::null_mut(), || {
        let Some(o) = (unsafe { opts.as_ref() }) else {
            set_error("opts is NULL");
            return std::ptr::null_mut();
        };
        if engine.is_null() || assets.is_null() {
            set_error("engine / assets is NULL");
            return std::ptr::null_mut();
        }
        let Some(ui_dir) = opt_str(o.smm2_ui).map(PathBuf::from) else {
            set_error("no SMM2 UI import");
            return std::ptr::null_mut();
        };
        let smm2 = match Ui::open(UiKind::Smm2, &ui_dir) {
            Ok(u) => u,
            Err(e) => {
                set_error(e);
                return std::ptr::null_mut();
            }
        };
        let smm1 = opt_str(o.smm1_ui).and_then(|d| Ui::open(UiKind::Smm1, Path::new(&d)).ok());
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
            | 1;
        let (online_tx, online_rx) = channel();
        let mut h = FfiHost {
            engine,
            assets,
            audio,
            cb: host,
            ui_dir,
            library: Vec::new(),
            server: opt_str(o.server),
            downloads: opt_str(o.downloads).map(PathBuf::from),
            input: 0,
            title: None,
            servers: None,
            online_tx,
            online_rx,
            rng: seed,
            recording: false,
            replay: mmk_modes::replay::Replay::new(),
            cleared_replay: None,
        };
        let options = Options {
            boot: o.flags & MMK_FRONT_BOOT != 0,
            opening: opt_str(o.opening),
            title_start: match o.title_start {
                1 => mmk_front::title::State::Main,
                2 => mmk_front::title::State::Category,
                _ => mmk_front::title::State::Top,
            },
            title_select: (o.title_select >= 0).then_some(o.title_select as usize),
            title_course: (o.title_course >= 0).then_some(o.title_course as usize),
            hundred: if o.flags & MMK_FRONT_HUNDRED_SMM1 != 0 {
                mmk_front::challenge::Presentation::Smm1
            } else {
                mmk_front::challenge::Presentation::Smm2
            },
            instant_fades: o.flags & MMK_FRONT_INSTANT_FADES != 0,
            script: opt_str(o.script).unwrap_or_default(),
            title_input: opt_str(o.title_input).unwrap_or_default(),
        };
        let front = Front::new(smm2, smm1, options, &mut h);
        Box::into_raw(Box::new(MmkFront {
            front,
            host: h,
            frame: Vec::new(),
            items: Vec::new(),
            toast: None,
            sounds: Vec::new(),
            sound_names: Vec::new(),
            smm2_names: Vec::new(),
            smm1_names: Vec::new(),
            course_shown: false,
        }))
    })
}

/// # Safety
/// `front` must come from `mmk_front_new` and not be used afterwards (free it before the engine).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_free(front: *mut MmkFront) {
    if !front.is_null() {
        drop(unsafe { Box::from_raw(front) });
    }
}

/// Give the front end the course library (JSON, see [`parse_library`]); call again whenever it
/// changes. Screens opened afterwards see it.
///
/// # Safety
/// `front` live; `json` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_set_library(front: *mut MmkFront, json: *const c_char) {
    guard((), || {
        let (Some(f), Some(text)) = (unsafe { front.as_mut() }, opt_str(json)) else {
            return;
        };
        f.host.library = parse_library(&text);
    })
}

/// The course server changed (Settings): base URL, NULL / "" none.
///
/// # Safety
/// `front` live; `server` NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_set_server(front: *mut MmkFront, server: *const c_char) {
    guard((), || {
        if let Some(f) = unsafe { front.as_mut() } {
            f.host.server = opt_str(server);
        }
    })
}

fn front_input(i: &MmkFrontInput) -> FrontInput {
    FrontInput {
        menu: Buttons((i.menu & 0x3f) as u8),
        start: i.flags & 1 != 0,
        skip: i.flags & 2 != 0,
        click: (i.flags & 4 != 0).then_some([i.click_x, i.click_y]),
        text: char::from_u32(i.text).filter(|&c| c != '\0'),
    }
}

impl MmkFront {
    fn tick(&mut self, input: &MmkFrontInput) {
        self.host.input = input.buttons;
        self.front.tick(&front_input(input), &mut self.host);
    }

    fn engine_tick(&self) -> u64 {
        self.host.engine_ref().world.tick
    }
}

/// One 60 Hz tick: the screens (and the course, when one plays) advance. The UI sounds it
/// started come from `mmk_front_sounds`.
///
/// # Safety
/// `front` live; `input` NULL (nothing held) or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_tick(front: *mut MmkFront, input: *const MmkFrontInput) {
    guard((), || {
        let Some(f) = (unsafe { front.as_mut() }) else {
            return;
        };
        let i = unsafe { input.as_ref() }.copied().unwrap_or_default();
        f.tick(&i);
    })
}

/// What to draw now, back to front; `*out` stays valid until the next `mmk_front_frame` or
/// `mmk_front_free`. Upload the textures `mmk_front_texture_count` reports first.
///
/// # Safety
/// `front` live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_frame(
    front: *mut MmkFront,
    out: *mut *const MmkFrontItem,
) -> usize {
    guard(0, || {
        let Some(f) = (unsafe { front.as_mut() }) else {
            return 0;
        };
        let frame = f.front.frame();
        f.toast = frame.toast;
        f.frame = frame.items;
        f.items = f
            .frame
            .iter()
            .map(|it| match it {
                DrawItem::Course => MmkFrontItem {
                    kind: 1,
                    ui: 0,
                    width: 1280.0,
                    height: 720.0,
                    quads: std::ptr::null(),
                    quad_count: 0,
                },
                DrawItem::Layout { ui, size, quads } => MmkFrontItem {
                    kind: 0,
                    ui: (*ui == UiKind::Smm1) as u32,
                    width: size[0],
                    height: size[1],
                    quads: quads.as_ptr(),
                    quad_count: quads.len(),
                },
            })
            .collect();
        f.course_shown = f.frame.iter().any(|i| matches!(i, DrawItem::Course));
        if let Some(out) = unsafe { out.as_mut() } {
            *out = f.items.as_ptr();
        }
        f.items.len()
    })
}

/// The message to show over the screen with the last frame (the toast), if any: its length
/// (`cap` 0 sizes it), 0 for none.
///
/// # Safety
/// `front` live; `buf` writable for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_toast(
    front: *const MmkFront,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let Some(t) = unsafe { front.as_ref() }.and_then(|f| f.toast.as_deref()) else {
            return 0;
        };
        if !buf.is_null() && cap > t.len() {
            unsafe {
                std::ptr::copy_nonoverlapping(t.as_ptr(), buf.cast(), t.len());
                *buf.add(t.len()) = 0;
            }
        }
        t.len()
    })
}

/// The UI sounds started since the last call (call once per tick); `*out` valid until the next
/// call.
///
/// # Safety
/// `front` live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_sounds(
    front: *mut MmkFront,
    out: *mut *const MmkFrontSound,
) -> usize {
    guard(0, || {
        let Some(f) = (unsafe { front.as_mut() }) else {
            return 0;
        };
        let cues = f.front.take_sounds();
        f.sound_names = cues
            .iter()
            .map(|c| CString::new(c.name.replace('\0', " ")).unwrap_or_default())
            .collect();
        f.sounds = cues
            .iter()
            .zip(&f.sound_names)
            .map(|(c, n)| MmkFrontSound {
                name: n.as_ptr(),
                volume: c.volume,
                delay: c.delay,
            })
            .collect();
        if let Some(out) = unsafe { out.as_mut() } {
            *out = f.sounds.as_ptr();
        }
        f.sounds.len()
    })
}

/// # Safety
/// `front` live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_status(front: *const MmkFront, out: *mut MmkFrontStatus) {
    guard((), || {
        let (Some(f), Some(out)) = (unsafe { front.as_ref() }, unsafe { out.as_mut() }) else {
            return;
        };
        let mut flags = 0;
        if f.course_shown {
            flags |= MMK_FRONT_COURSE_SHOWN;
            if !matches!(f.front.screen, mmk_front::Screen::Title(_)) {
                flags |= MMK_FRONT_PLAYING;
            }
        }
        if f.front.course_done {
            flags |= MMK_FRONT_COURSE_DONE;
        }
        if matches!(f.front.screen, mmk_front::Screen::None) {
            flags |= MMK_FRONT_NO_SCREEN;
        }
        if f.front.typing() {
            flags |= MMK_FRONT_TYPING;
        }
        *out = MmkFrontStatus {
            flags,
            _pad: 0,
            engine_tick: f.engine_tick(),
        };
    })
}

impl MmkFront {
    fn ui(&self, which: u32) -> Option<&Ui> {
        match which {
            0 => Some(&self.front.smm2),
            _ => self.front.smm1.as_ref(),
        }
    }
}

/// Textures of import `ui` (0 SMM2, 1 SMM2's companion SMM1 import) so far; the list grows as
/// screens open layouts (indices stay stable).
///
/// # Safety
/// `front` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_texture_count(front: *const MmkFront, ui: u32) -> usize {
    guard(0, || {
        unsafe { front.as_ref() }
            .and_then(|f| f.ui(ui))
            .map_or(0, |u| u.cache().textures.len())
    })
}

/// Texture `index` of import `ui`; its pixels stay valid while the front end lives.
///
/// # Safety
/// `front` live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_texture(
    front: *mut MmkFront,
    ui: u32,
    index: usize,
    out: *mut MmkTextureInfo,
) -> bool {
    guard(false, || {
        let (Some(f), Some(out)) = (unsafe { front.as_mut() }, unsafe { out.as_mut() }) else {
            return false;
        };
        let Some(u) = f.ui(ui).cloned() else {
            return false;
        };
        let cache = u.cache();
        let names = if ui == 0 {
            &mut f.smm2_names
        } else {
            &mut f.smm1_names
        };
        while names.len() < cache.textures.len() {
            let n = &cache.textures[names.len()].name;
            names.push(CString::new(n.replace('\0', " ")).unwrap_or_default());
        }
        let Some(t) = cache.textures.get(index) else {
            return false;
        };
        *out = MmkTextureInfo {
            model: c"ui".as_ptr(),
            name: names[index].as_ptr(),
            width: t.texture.width,
            height: t.texture.height,
            format: t.texture.format as u8,
            variant: t.texture.variant as u8,
            channel_select: t.texture.channel_select,
            data: t.texture.data.as_ptr(),
            len: t.texture.data.len(),
        };
        true
    })
}

/// Captures: jump to a screen as `mmk-play --front-screen` does (boot, title, title-main,
/// title-play, courseworld, courseworld-list, storymode, challenge, challenge-map,
/// challenge-walk, challenge-gameover, challenge-ending, challenge-pause / -clear / -skip /
/// -card). False for an unknown name.
///
/// # Safety
/// `front` live; `screen` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_capture(
    front: *mut MmkFront,
    screen: *const c_char,
    difficulty: u32,
    cleared: u32,
) -> bool {
    guard(false, || {
        let (Some(f), Some(screen)) = (unsafe { front.as_mut() }, opt_str(screen)) else {
            return false;
        };
        f.front
            .capture(&mut f.host, &screen, difficulty as usize, cleared as usize)
    })
}

/// Play the course file at `path` with the SMM2 screens (unlimited retries, as library play);
/// `intro` the course card first. When the player leaves, the status says
/// `MMK_FRONT_COURSE_DONE`. False (and an error) when it can't be read.
///
/// # Safety
/// `front` live; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_play_course(
    front: *mut MmkFront,
    path: *const c_char,
    name: *const c_char,
    intro: bool,
) -> bool {
    guard(false, || {
        let (Some(f), Some(path)) = (unsafe { front.as_mut() }, opt_str(path)) else {
            set_error("bad arguments");
            return false;
        };
        let loaded = match f.host.load_course(&Source::Library(path.clone())) {
            Ok(l) => l,
            Err(e) => {
                set_error(e);
                return false;
            }
        };
        let name = opt_str(name)
            .or_else(|| (!loaded.name.is_empty()).then(|| loaded.name.clone()))
            .unwrap_or_else(|| {
                Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        f.front.course_done = false;
        f.front
            .play_course(&mut f.host, &name, loaded.key.clone(), intro);
        true
    })
}

/// Captures of a course play's screens (after `mmk_front_play_course`): course-pause (pauses
/// after the first tick), course-clear, course-gameover, course-timeup, course-retire,
/// course-settings, course-death.
///
/// # Safety
/// `front` live; `screen` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_debug_course(
    front: *mut MmkFront,
    screen: *const c_char,
) -> bool {
    guard(false, || {
        let (Some(f), Some(screen)) = (unsafe { front.as_mut() }, opt_str(screen)) else {
            return false;
        };
        f.front.with_cx(&mut f.host, |cx, s| {
            let mmk_front::Screen::Course(p) = s else {
                return false;
            };
            if screen == "course-pause" {
                p.auto_pause_tick = Some(1);
            } else if screen != "course-play" {
                p.debug(&screen, cx, None);
            }
            true
        })
    })
}

/// Captures: run `ticks` ticks at once with nothing held, as `mmk-play`'s headless captures do.
/// `course_screen`: run until a course play's menu has been up `ticks` ticks instead.
/// `scripted`: the smoke test's run-right-and-jump as the player's input.
///
/// # Safety
/// `front` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_front_run_ticks(
    front: *mut MmkFront,
    ticks: u64,
    course_screen: bool,
    scripted: bool,
) {
    guard((), || {
        let Some(f) = (unsafe { front.as_mut() }) else {
            return;
        };
        let none = MmkFrontInput::default();
        if course_screen {
            for _ in 0..ticks + 3600 {
                let done = match &f.front.screen {
                    mmk_front::Screen::Course(p) => p.menu_up() && p.since_mode() as u64 >= ticks,
                    _ => true,
                };
                if done {
                    break;
                }
                f.tick(&none);
            }
        } else {
            for _ in 0..ticks {
                let mut i = none;
                if scripted {
                    i.buttons = scripted_run(f.engine_tick());
                }
                f.tick(&i);
            }
        }
    })
}

/// The smoke-test script (the Swift app's `Renderer.scriptedInput`): run right, jump for 20
/// ticks every 90.
fn scripted_run(tick: u64) -> u32 {
    use mmk_sim::input as b;
    let mut buttons = b::RIGHT | b::RUN;
    if tick % 90 >= 60 && tick % 90 < 80 {
        buttons |= b::JUMP;
    }
    buttons
}

/// A JSON string for the C side's convenience (unused fields are fine).
pub fn library_entry_json(e: &CourseEntry) -> Value {
    json!({
        "id": e.id, "name": e.name, "description": e.description, "style": e.style,
        "theme": e.theme, "playable": e.playable, "built_in": e.built_in,
        "collection": e.collection, "key": e.key, "modified": e.modified,
        "meta": {
            "clear_rate": e.meta.clear_rate, "bucket": e.meta.bucket, "maker": e.meta.maker,
            "code": e.meta.code, "world_record_ms": e.meta.world_record_ms,
            "plays": e.meta.plays, "likes": e.meta.likes, "clears": e.meta.clears,
            "attempts": e.meta.attempts, "uploaded": e.meta.uploaded, "tags": e.meta.tags,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sizes the Swift side relies on (mmk.h).
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<MmkFrontInput>(), 24);
        assert_eq!(size_of::<MmkFrontItem>(), 32);
        assert_eq!(size_of::<MmkFrontSound>(), 16);
        assert_eq!(size_of::<MmkFrontStatus>(), 16);
        assert_eq!(size_of::<MmkFrontOptions>(), 72);
        assert_eq!(size_of::<MmkFrontHost>(), 32);
    }

    #[test]
    fn library_round_trips_through_json() {
        let e = CourseEntry {
            id: "/c/a.bcd".into(),
            name: "A".into(),
            style: "M1".into(),
            playable: true,
            key: "00ff".into(),
            meta: CourseMeta {
                clear_rate: Some(12.5),
                bucket: Some(2),
                tags: vec![1, 4],
                plays: Some(9),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = Value::Array(vec![library_entry_json(&e)]).to_string();
        assert_eq!(parse_library(&text), vec![e]);
        assert!(parse_library("nope").is_empty());
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["t10", "t2", "T1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["T1", "t2", "t10"]);
    }

    /// With the owner's SMM2 UI import (skipped without it): the title runs and draws through
    /// the C API, and Course World opens over a pushed library.
    #[test]
    fn title_and_course_world_through_the_c_api() {
        let Some(dir) = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("smm-work/cache/smm2-ui-3.0.3"))
            .filter(|d| d.join("manifest.json").is_file())
        else {
            return;
        };
        let dir = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        let opts = MmkFrontOptions {
            smm2_ui: dir.as_ptr(),
            smm1_ui: std::ptr::null(),
            server: std::ptr::null(),
            downloads: std::ptr::null(),
            opening: std::ptr::null(),
            script: std::ptr::null(),
            title_input: std::ptr::null(),
            flags: MMK_FRONT_INSTANT_FADES,
            title_start: 0,
            title_select: -1,
            title_course: -1,
        };
        unsafe {
            let a = crate::mmk_assets_placeholder();
            let e = crate::mmk_engine_new(a);
            let host = MmkFrontHost {
                user: std::ptr::null_mut(),
                load_value: None,
                store_value: None,
                set_paused: None,
            };
            let f = mmk_front_new(&opts, e, a, std::ptr::null(), host);
            assert!(!f.is_null());
            mmk_front_run_ticks(f, 30, false, false);
            let mut items = std::ptr::null();
            let n = mmk_front_frame(f, &mut items);
            assert!(n > 0);
            let list = std::slice::from_raw_parts(items, n);
            assert!(list.iter().any(|i| i.kind == 0 && i.quad_count > 0));
            assert!(mmk_front_texture_count(f, 0) > 0);
            let mut t = std::mem::zeroed::<MmkTextureInfo>();
            assert!(mmk_front_texture(f, 0, 0, &mut t) && t.len > 0);
            mmk_front_set_library(f, c"[]".as_ptr());
            assert!(mmk_front_capture(f, c"courseworld".as_ptr(), 0, 0));
            mmk_front_run_ticks(f, 10, false, false);
            let mut st = MmkFrontStatus::default();
            mmk_front_status(f, &mut st);
            assert_eq!(st.flags & (MMK_FRONT_PLAYING | MMK_FRONT_NO_SCREEN), 0);
            mmk_front_free(f);
            crate::mmk_engine_free(e);
            crate::mmk_assets_free(a);
        }
    }

    /// The Apple app's host against a live course server: download and play a server course,
    /// clear it, send the run's replay as a signed-in account (a verified record), vote and
    /// comment through `Host::online`.
    #[test]
    fn host_plays_a_server_course_and_proves_the_clear() {
        let dir = std::env::temp_dir().join(format!("mmk-ffi-front-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = mmk_server::Store::open(dir.join("server"))
            .unwrap()
            .with_verified_records(true);
        let port = 20000 + (std::process::id() % 20000) as u16 + 7;
        let addr = format!("127.0.0.1:{port}");
        let a2 = addr.clone();
        std::thread::spawn(move || mmk_server::serve(store, &a2));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let server = format!("http://{addr}");
        // a flat course the run-right-and-jump script clears
        let mut ed = mmk_editor::Editor::new("M1");
        for tx in 0..ed.size().0 {
            let _ = ed.paint_ground(tx, 0);
        }
        let data = ed.save().unwrap();
        let client = mmk_online::Client::new(server.clone());
        let (_, token) = client.register("Ada").unwrap();
        let code = client.upload(&data, "Ada").unwrap().code;

        unsafe {
            let a = crate::mmk_assets_placeholder();
            let e = crate::mmk_engine_new(a);
            let (online_tx, online_rx) = channel();
            let mut h = FfiHost {
                engine: e,
                assets: a,
                audio: std::ptr::null(),
                cb: MmkFrontHost {
                    user: std::ptr::null_mut(),
                    load_value: None,
                    store_value: None,
                    set_paused: None,
                },
                ui_dir: PathBuf::new(),
                library: Vec::new(),
                server: Some(server),
                downloads: Some(dir.join("downloads")),
                input: 0,
                title: None,
                servers: None,
                online_tx,
                online_rx,
                rng: 1,
                recording: false,
                replay: mmk_modes::replay::Replay::new(),
                cleared_replay: None,
            };
            assert!(h.request_server_courses());
            let answer = |h: &mut FfiHost| {
                for _ in 0..500 {
                    if let Some(a) = h.poll_online() {
                        return a;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                panic!("no answer");
            };
            h.load_course(&Source::Server(code.clone())).unwrap();
            let mut ticks = 0;
            while !h.course_status().cleared && ticks < 20_000 {
                h.input = scripted_run(h.engine_ref().world.tick);
                h.step_course(None);
                ticks += 1;
            }
            assert!(h.course_status().cleared);
            h.cleared(ticks);
            let args = json!({"code": code});
            assert!(h.online(1, "submit_replay", &args, Some(&token)));
            let (id, r) = answer(&mut h);
            let l = r.unwrap();
            assert_eq!(id, 1);
            assert_eq!(l["record_verified"], true);
            assert!(l["record_ms"].as_u64().unwrap() > 0);
            assert!(h.online(
                2,
                "vote",
                &json!({"code": code, "like": true}),
                Some(&token)
            ));
            assert_eq!(answer(&mut h).1.unwrap()["likes"], 1);
            let c = json!({"code": code, "text": "nice"});
            assert!(h.online(3, "comment", &c, Some(&token)));
            answer(&mut h).1.unwrap();
            assert!(h.online(4, "comments", &args, None));
            assert_eq!(answer(&mut h).1.unwrap()[0]["text"], "nice");
            // nothing cleared since: no replay to send
            assert!(h.online(5, "submit_replay", &args, Some(&token)));
            assert!(answer(&mut h).1.is_err());
            crate::mmk_engine_free(e);
            crate::mmk_assets_free(a);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_ui_import_is_an_error() {
        let opts = MmkFrontOptions {
            smm2_ui: c"/nonexistent/smm2-ui".as_ptr(),
            smm1_ui: std::ptr::null(),
            server: std::ptr::null(),
            downloads: std::ptr::null(),
            opening: std::ptr::null(),
            script: std::ptr::null(),
            title_input: std::ptr::null(),
            flags: 0,
            title_start: 0,
            title_select: -1,
            title_course: -1,
        };
        unsafe {
            let a = crate::mmk_assets_placeholder();
            let e = crate::mmk_engine_new(a);
            let host = MmkFrontHost {
                user: std::ptr::null_mut(),
                load_value: None,
                store_value: None,
                set_paused: None,
            };
            let f = mmk_front_new(&opts, e, a, std::ptr::null(), host);
            assert!(f.is_null());
            crate::mmk_engine_free(e);
            crate::mmk_assets_free(a);
        }
    }
}
