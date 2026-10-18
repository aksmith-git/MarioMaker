//! The portable shell as the front end's [`mmk_front::Host`]: the engine (the shell's [`Game`]),
//! the course library (a folder of course files plus the asset cache's built-in courses, with
//! the dataset's `index.json` statistics), the course server (`mmk-online`, on a thread), and
//! small files for save data and settings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use mmk_front::host::{CourseMeta, LoadedCourse};
use mmk_front::{CourseEntry, CourseStatus, Host, Source, WorldCourse};
use mmk_host::audio::AudioOut;
use serde_json::{Value, json};

pub use crate::front_course::fnv1a64;
use crate::front_course::{self, natural};
use crate::game::Game;
use crate::source::{self, AssetSource, DirSource};

/// What the host keeps between ticks.
pub struct HostData {
    /// The asset cache (None: placeholder art).
    pub source: Option<DirSource>,
    /// Course folders.
    pub library_dirs: Vec<PathBuf>,
    /// The SMM2 UI import (its `courses/` stand in for the title courses).
    pub ui_dir: Option<PathBuf>,
    /// Course server base URL.
    pub server: Option<String>,
    /// Where save data and settings go (None: nowhere, captures).
    pub data_dir: Option<PathBuf>,
    /// The player's buttons this tick (engine bits).
    pub input: u32,
    /// Sound output for courses.
    pub audio: Option<Arc<AudioOut>>,
    title: Option<Vec<PathBuf>>,
    library: Option<Vec<CourseEntry>>,
    scanning: Option<Receiver<Vec<CourseEntry>>>,
    rx: Option<Receiver<Vec<WorldCourse>>>,
    rng: u64,
    memory: HashMap<String, String>,
    /// Server calls (`Host::online`) answer here, from their threads.
    online: Option<(Sender<Reply>, Receiver<Reply>)>,
    /// The record run downloaded by a `ghost` call: raced from the next start of that course.
    ghost: Option<(String, mmk_modes::replay::Replay)>,
}

/// A server call's id and what its thread sent back.
type Reply = (u32, Answer);

/// What a server call's thread sends back.
enum Answer {
    Json(Result<Value, String>),
    /// A `ghost` call: the course code and the replay's bytes.
    Ghost(String, Result<Vec<u8>, String>),
}

impl HostData {
    pub fn new(source: Option<DirSource>) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
            | 1;
        Self {
            source,
            library_dirs: Vec::new(),
            ui_dir: None,
            server: None,
            data_dir: None,
            input: 0,
            audio: None,
            title: None,
            library: None,
            scanning: None,
            rx: None,
            rng: seed,
            memory: HashMap::new(),
            online: None,
            ghost: None,
        }
    }

    fn online_tx(&mut self) -> Sender<Reply> {
        self.online
            .get_or_insert_with(std::sync::mpsc::channel)
            .0
            .clone()
    }

    fn src(&self) -> Option<&dyn AssetSource> {
        self.source.as_ref().map(|s| s as &dyn AssetSource)
    }

    fn decode(&self, bytes: &[u8]) -> Result<(mmk_formats::smm2::Course, String), String> {
        decode(self.src(), bytes)
    }

    /// Scan the library on a thread (`library` waits for it).
    pub fn start_scan(&mut self) {
        let dirs = self.library_dirs.clone();
        let root = self
            .source
            .as_ref()
            .and_then(|s| s.dir())
            .map(Path::to_path_buf);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan(&dirs, root.as_deref()));
        });
        self.scanning = Some(rx);
    }

    /// The title demo courses the engine can play (the asset cache's built-in title courses,
    /// else the UI import's copies), in their numbering order.
    fn title_files(&mut self) -> &[PathBuf] {
        if self.title.is_none() {
            let mut list: Vec<PathBuf> = Vec::new();
            let dirs: Vec<PathBuf> = [
                self.source
                    .as_ref()
                    .and_then(|s| s.dir())
                    .map(Path::to_path_buf),
                self.ui_dir.clone(),
            ]
            .into_iter()
            .flatten()
            .map(|d| d.join("courses"))
            .collect();
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
            let playable: Vec<PathBuf> = list
                .iter()
                .filter(|p| {
                    std::fs::read(p)
                        .ok()
                        .and_then(|b| self.decode(&b).ok())
                        .is_some_and(|(c, _)| front_course::playable(&c))
                })
                .cloned()
                .collect();
            self.title = Some(if playable.is_empty() { list } else { playable });
        }
        self.title.as_deref().unwrap_or(&[])
    }

    fn load_file(&self, game: &mut Game, path: &Path) -> Result<LoadedCourse, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (c, key) = self.decode(&bytes)?;
        let loaded = LoadedCourse {
            name: c.header.name.to_string_lossy(),
            key: Some(key),
            theme: c.main.header.theme,
        };
        game.set_course(Some(c));
        game.session.set_audio(self.audio.clone());
        Ok(loaded)
    }
}

fn scan(library_dirs: &[PathBuf], asset_root: Option<&Path>) -> Vec<CourseEntry> {
    let source = asset_root.map(DirSource::new);
    let src = source.as_ref().map(|s| s as &dyn AssetSource);
    let mut out = Vec::new();
    let mut sources: Vec<(PathBuf, bool)> = Vec::new();
    let mut meta: HashMap<PathBuf, CourseMeta> = HashMap::new();
    for dir in library_dirs {
        for f in files(dir) {
            if f.file_name().is_some_and(|n| n == "index.json") {
                if let Ok(text) = std::fs::read_to_string(&f) {
                    let base = f.parent().unwrap_or(Path::new(""));
                    for (file, m) in CourseMeta::parse_index(&text, |id| {
                        mmk_formats::course_id::smm2_data_id_to_code(id, false)
                    }) {
                        let p = base.join(file);
                        meta.insert(p.canonicalize().unwrap_or(p), m);
                    }
                }
            } else if candidate(&f) {
                sources.push((f, false));
            }
        }
    }
    if let Some(root) = asset_root {
        sources.extend(
            files(&root.join("courses"))
                .into_iter()
                .filter(|f| candidate(f))
                .map(|f| (f, true)),
        );
    }
    for (path, built_in) in sources {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok((c, key)) = decode(src, &bytes) else {
            continue;
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let canon = path.canonicalize().unwrap_or(path.clone());
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let mut e = front_course::entry(
            path.to_string_lossy().into_owned(),
            &stem,
            built_in,
            &c,
            key,
            modified,
        );
        e.meta = meta.remove(&canon).unwrap_or_default();
        out.push(e);
    }
    out.sort_by(|a, b| natural(&a.name, &b.name));
    out
}

fn decode(
    src: Option<&dyn AssetSource>,
    bytes: &[u8],
) -> Result<(mmk_formats::smm2::Course, String), String> {
    front_course::decode(bytes, || source::key_table(src)).map(|(c, _, key)| (c, key))
}

/// Likely course files: `.course.zlib`, `.zlib`, `.bcd`, raw data (`.bin`, `.dat`, `.course`,
/// no extension), at most 1 MiB.
fn candidate(p: &Path) -> bool {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if name.starts_with('.') {
        return false;
    }
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    ["zlib", "bcd", "bin", "dat", "course", ""].contains(&ext.as_str())
        && std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= 1 << 20)
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// The host for one tick: the game and the data.
pub struct ShellHost<'a> {
    pub game: &'a mut Game,
    pub data: &'a mut HostData,
}

impl Host for ShellHost<'_> {
    fn library(&mut self) -> Vec<CourseEntry> {
        if self.data.library.is_none() {
            let found = match self.data.scanning.take() {
                Some(rx) => rx.recv().unwrap_or_default(),
                None => {
                    let root = self.data.source.as_ref().and_then(|s| s.dir());
                    scan(&self.data.library_dirs, root)
                }
            };
            self.data.library = Some(found);
        }
        self.data.library.clone().unwrap_or_default()
    }

    fn request_server_courses(&mut self) -> bool {
        let Some(server) = self.data.server.clone() else {
            return false;
        };
        let (tx, rx) = std::sync::mpsc::channel();
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
        self.data.rx = Some(rx);
        true
    }

    fn poll_server_courses(&mut self) -> Option<Vec<WorldCourse>> {
        let r = self.data.rx.as_ref()?.try_recv().ok()?;
        self.data.rx = None;
        Some(r)
    }

    fn load_course(&mut self, source: &Source) -> Result<LoadedCourse, String> {
        match source {
            Source::Library(path) => self.data.load_file(self.game, Path::new(path)),
            Source::Server(code) => {
                let server = self.data.server.clone().ok_or("no course server")?;
                let dir = self
                    .data
                    .data_dir
                    .clone()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("Course World");
                let _ = std::fs::create_dir_all(&dir);
                let file = dir.join(format!("{code}.bin"));
                if !file.is_file() {
                    let bytes = mmk_online::Client::new(server)
                        .download(code)
                        .map_err(|e| e.to_string())?;
                    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
                }
                let loaded = self.data.load_file(self.game, &file)?;
                // a server course's clears go back as replays the server verifies
                self.game.session.record_replays();
                if self.data.ghost.as_ref().is_some_and(|g| &g.0 == code)
                    && let Some((_, r)) = self.data.ghost.take()
                {
                    self.game.session.set_ghost(r);
                }
                Ok(loaded)
            }
        }
    }

    fn title_courses(&mut self) -> usize {
        self.data.title_files().len()
    }

    fn load_title_course(&mut self, index: usize) -> bool {
        let Some(p) = self.data.title_files().get(index).cloned() else {
            return false;
        };
        self.data.load_file(self.game, &p).is_ok()
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
        local_offset(secs)
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

    fn report(&mut self, code: &str, what: &str) {
        let Some(server) = self.data.server.clone() else {
            return;
        };
        let (code, what) = (code.to_string(), what.to_string());
        std::thread::spawn(move || {
            let _ = mmk_online::Client::new(server).report(&code, &what);
        });
    }

    fn online(&mut self, id: u32, action: &str, args: &Value, token: Option<&str>) -> bool {
        let Some(server) = self.data.server.clone() else {
            return false;
        };
        let tx = self.data.online_tx();
        let token = token.filter(|t| !t.is_empty()).map(str::to_string);
        let code = args["code"].as_str().unwrap_or_default().to_string();
        match action {
            "submit_replay" => {
                // the run just cleared (kept by the session since the course started)
                let replay = self.game.session.take_cleared_replay();
                let affects = self.play_options_affect_play();
                match replay.filter(|_| !affects) {
                    Some(r) => {
                        std::thread::spawn(move || {
                            let c = mmk_online::Client::new(server).with_token(token);
                            let a = c
                                .submit_replay(&code, &r.encode())
                                .map(|l| mmk_online::listing_json(&l));
                            let _ = tx.send((id, Answer::Json(a)));
                        });
                    }
                    None => {
                        let _ = tx.send((id, Answer::Json(Err("no replay of a clear".into()))));
                    }
                }
            }
            "ghost" => {
                std::thread::spawn(move || {
                    let bytes = mmk_online::Client::new(server).record_replay(&code);
                    let _ = tx.send((id, Answer::Ghost(code, bytes)));
                });
            }
            _ => {
                let (action, args) = (action.to_string(), args.clone());
                std::thread::spawn(move || {
                    let a = mmk_online::call(&server, token.as_deref(), &action, &args);
                    let _ = tx.send((id, Answer::Json(a)));
                });
            }
        }
        true
    }

    fn poll_online(&mut self) -> Option<(u32, Result<Value, String>)> {
        let (id, a) = self.data.online.as_ref()?.1.try_recv().ok()?;
        Some((
            id,
            match a {
                Answer::Json(r) => r,
                Answer::Ghost(code, bytes) => bytes.and_then(|b| {
                    let r = mmk_modes::replay::Replay::decode(&b)
                        .ok_or_else(|| "the record's run isn't a replay".to_string())?;
                    let frames = r.inputs.len();
                    let same = r.sim_version == mmk_modes::replay::SIM_VERSION;
                    self.data.ghost = Some((code, r));
                    Ok(json!({ "frames": frames, "same_engine": same }))
                }),
            },
        ))
    }

    fn play_options_affect_play(&self) -> bool {
        self.game.session.world.options.affects_play()
    }

    fn load_value(&mut self, key: &str) -> Option<String> {
        match &self.data.data_dir {
            Some(d) => std::fs::read_to_string(d.join(file_name(key))).ok(),
            None => self.data.memory.get(key).cloned(),
        }
    }

    fn store_value(&mut self, key: &str, value: &str) {
        match &self.data.data_dir {
            Some(d) => {
                let _ = std::fs::create_dir_all(d);
                let _ = std::fs::write(d.join(file_name(key)), value);
            }
            None => {
                self.data.memory.insert(key.to_string(), value.to_string());
            }
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

/// Local time minus UTC at `secs` (the C library's time zone; 0 where it isn't known).
#[cfg(unix)]
pub fn local_offset(secs: i64) -> i64 {
    let t = secs as libc::time_t;
    // SAFETY: localtime_r only writes the `tm` it is given
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() { 0 } else { tm.tm_gmtoff as i64 }
}

#[cfg(not(unix))]
pub fn local_offset(_secs: i64) -> i64 {
    0
}

fn file_name(key: &str) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_and_keys() {
        let mut v = vec!["c10", "c2", "C1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["C1", "c2", "c10"]);
        assert_eq!(file_name("player.character"), "player.character.json");
        assert_eq!(file_name("a/b"), "a_b.json");
    }
}
