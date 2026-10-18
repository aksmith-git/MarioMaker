//! The browser as the front end's [`mmk_front::Host`] (the web build's counterpart of
//! `front_host`): the engine is the shell's [`Game`]; the library is the built-in courses of the
//! game data in the browser's storage (read and decoded once, in the background, after the menus
//! open — the front end reads the library synchronously, the storage can't be), plus course files
//! the page opens; save data and settings go to `localStorage`. There is no course server
//! listing (the web build only speaks the relay's WebSocket, not HTTP).

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;

use mmk_front::host::LoadedCourse;
use mmk_front::{CourseEntry, CourseStatus, Host, Source, WorldCourse};
use mmk_host::audio::AudioOut;

use crate::front_course;
use crate::game::Game;

/// A library course: its entry and its decoded course data, zlib-compressed (a decoded course is
/// mostly zeros; the library holds a few hundred).
pub struct LibCourse {
    pub entry: CourseEntry,
    data: Vec<u8>,
}

impl LibCourse {
    /// A course file's bytes (raw, zlib or `.bcd` with `keys`) as library course `id`.
    pub fn new(
        id: String,
        stem: &str,
        built_in: bool,
        bytes: &[u8],
        keys: Option<&[u8]>,
    ) -> Result<Self, String> {
        let (c, data, key) = front_course::decode(bytes, || {
            keys.map(<[u8]>::to_vec)
                .ok_or_else(|| mmk_host::course::no_key_table("no game data imported"))
        })?;
        let entry = front_course::entry(id, stem, built_in, &c, key, None);
        Ok(Self {
            entry,
            data: pack(&data),
        })
    }
}

fn pack(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    match z.write_all(data).and_then(|_| z.finish()) {
        Ok(out) => out,
        Err(_) => data.to_vec(),
    }
}

/// The library, shared with the scan that fills it.
pub type Library = Rc<RefCell<Vec<LibCourse>>>;

/// What the host keeps between ticks.
pub struct HostData {
    /// The player's buttons this tick (engine bits).
    pub input: u32,
    /// Sound output for courses (after the page's first gesture).
    pub audio: Option<Arc<AudioOut>>,
    /// The title demo courses the engine can play, in their numbering order.
    pub title: Vec<LibCourse>,
    /// Built-in courses (filled in the background) and opened files.
    pub library: Library,
    /// The style and theme of the course just loaded, whose music the page must now read
    /// (the mixer reads it synchronously; see `web::pump_ui`).
    pub music_wanted: Option<(String, u8)>,
    rng: u64,
}

impl HostData {
    pub fn new(title: Vec<LibCourse>, library: Library) -> Self {
        let seed = (js_sys::Math::random() * 9.0e15) as u64 | 1;
        Self {
            input: 0,
            audio: None,
            title,
            library,
            music_wanted: None,
            rng: seed,
        }
    }

    fn load(&mut self, game: &mut Game, c: &LibCourse) -> Result<LoadedCourse, String> {
        let (data, _) = mmk_host::course::decode_data(&c.data, || Err("packed".into()))?;
        let course = mmk_formats::smm2::Course::parse(&data).map_err(|e| e.to_string())?;
        let loaded = LoadedCourse {
            name: course.header.name.to_string_lossy(),
            key: Some(c.entry.key.clone()),
            theme: course.main.header.theme,
        };
        game.set_course(Some(course));
        game.session.set_audio(self.audio.clone());
        let level = &game.session.world.level;
        self.music_wanted = Some((level.style.clone(), level.theme));
        Ok(loaded)
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

const STORAGE_PREFIX: &str = "mmk-front:";

/// The host for one tick: the game and the data.
pub struct ShellHost<'a> {
    pub game: &'a mut Game,
    pub data: &'a mut HostData,
}

impl Host for ShellHost<'_> {
    fn library(&mut self) -> Vec<CourseEntry> {
        let mut v: Vec<CourseEntry> = self
            .data
            .library
            .borrow()
            .iter()
            .map(|c| c.entry.clone())
            .collect();
        v.sort_by(|a, b| front_course::natural(&a.name, &b.name));
        v
    }

    fn request_server_courses(&mut self) -> bool {
        false
    }

    fn poll_server_courses(&mut self) -> Option<Vec<WorldCourse>> {
        None
    }

    fn load_course(&mut self, source: &Source) -> Result<LoadedCourse, String> {
        match source {
            Source::Library(id) => {
                let library = self.data.library.clone();
                let lib = library.borrow();
                let c = lib
                    .iter()
                    .find(|c| &c.entry.id == id)
                    .ok_or_else(|| format!("no course {id}"))?;
                self.data.load(self.game, c)
            }
            Source::Server(_) => Err("no course server in the browser".into()),
        }
    }

    fn title_courses(&mut self) -> usize {
        self.data.title.len()
    }

    fn load_title_course(&mut self, index: usize) -> bool {
        if index >= self.data.title.len() {
            return false;
        }
        let c = self.data.title.remove(index);
        let ok = self.data.load(self.game, &c).is_ok();
        self.data.title.insert(index, c);
        ok
    }

    fn unload_course(&mut self) {
        if let Some(a) = &self.data.audio {
            a.send(mmk_audio::Command::StopAll);
        }
        self.game.session.set_audio(None);
        self.game.set_course(None);
    }

    fn step_course(&mut self, scripted: Option<u32>) {
        let b = scripted.unwrap_or(self.data.input) & !mmk_sim::input::START;
        self.game.session.tick(mmk_sim::Input { buttons: b });
    }

    fn course_status(&self) -> CourseStatus {
        front_course::status(self.game)
    }

    fn restart_course(&mut self) {
        self.game.session.restart();
    }

    fn set_character(&mut self, character: u32) {
        self.game.session.world.main_char = (character as usize).min(3);
    }

    fn set_run_counters(&mut self, score: u32, coins: u32, lives: u32) {
        self.game
            .session
            .world
            .set_run_counters(score, coins, lives);
    }

    fn local_offset(&self, secs: i64) -> i64 {
        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(secs as f64 * 1000.0));
        // minutes UTC is ahead of local time
        -(d.get_timezone_offset() * 60.0) as i64
    }

    fn set_paused(&mut self, paused: bool) {
        if let Some(a) = &self.data.audio {
            a.send(mmk_audio::Command::SetVolumes {
                master: 1.0,
                music: if paused { 0.3 } else { 1.0 },
                sfx: 1.0,
            });
        }
    }

    fn load_value(&mut self, key: &str) -> Option<String> {
        storage()?
            .get_item(&format!("{STORAGE_PREFIX}{key}"))
            .ok()
            .flatten()
    }

    fn store_value(&mut self, key: &str, value: &str) {
        if let Some(s) = storage() {
            let _ = s.set_item(&format!("{STORAGE_PREFIX}{key}"), value);
        }
    }

    fn random(&mut self) -> u64 {
        let mut x = self.data.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.data.rng = x;
        x
    }
}
