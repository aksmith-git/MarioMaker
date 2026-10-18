//! Where the owner's asset cache comes from. Native builds read a directory; the web build reads
//! the copy the owner imported into the browser's storage (OPFS, never uploaded), so every read
//! goes through [`AssetSource`] rather than `std::fs`. The browser reads asynchronously: the web
//! build fills a [`MemorySource`] with the index files and opens the cache deferred
//! ([`open_cache_deferred`]), handing texture files over as they arrive (`web`).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use mmk_assets::Cache;
use mmk_audio::Bank;

/// A read-only tree of files, addressed by paths relative to the cache root (`/` separators).
pub trait AssetSource {
    /// The bytes of `rel` (e.g. `textures/index.json`). `NotFound` when absent.
    fn read(&self, rel: &str) -> io::Result<Vec<u8>>;
    /// Shown in messages (a directory path, "browser files", ...).
    fn describe(&self) -> String;
    /// The directory behind the source, if it is one (the cache is then read lazily from it).
    fn dir(&self) -> Option<&Path> {
        None
    }
}

/// A directory on disk (native builds).
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl AssetSource for DirSource {
    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.root.join(rel))
    }
    fn describe(&self) -> String {
        self.root.display().to_string()
    }
    fn dir(&self) -> Option<&Path> {
        Some(&self.root)
    }
}

/// Files held in memory (the web build fills this from the browser's file picker; tests).
#[derive(Default)]
pub struct MemorySource {
    files: HashMap<String, Vec<u8>>,
    label: String,
}

impl MemorySource {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            files: HashMap::new(),
            label: label.into(),
        }
    }
    pub fn insert(&mut self, rel: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(rel.into().replace('\\', "/"), bytes);
    }
}

impl AssetSource for MemorySource {
    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        self.files
            .get(rel)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, rel.to_string()))
    }
    fn describe(&self) -> String {
        self.label.clone()
    }
}

/// The relative path of `full` under the virtual root used for loading (see [`open_cache`]).
fn relative(full: &Path) -> String {
    let rel = full.strip_prefix(ROOT).unwrap_or(full);
    rel.to_string_lossy().replace('\\', "/")
}

/// Virtual root the loaders join paths onto; stripped again before reaching the source.
const ROOT: &str = "/mmk-assets";

/// Open the asset cache through `source`.
pub fn open_cache(source: &dyn AssetSource) -> Result<Cache, String> {
    if let Some(dir) = source.dir() {
        // a directory: texture pixels are read when first drawn
        return Cache::open(dir).map_err(|e| format!("{}: {e}", source.describe()));
    }
    let mut cache = Cache::load(ROOT, &|p| source.read(&relative(p)))
        .map_err(|e| format!("{}: {e}", source.describe()))?;
    // the cache's root is only used for messages and for files read later; point it at the
    // source's own description
    cache.root = PathBuf::from(source.describe());
    Ok(cache)
}

/// Open the asset cache's index through `source` and leave the texture pixels to the host,
/// which reads them later and hands them over (`Cache::take_requests` / `Cache::provide`): the
/// browser's storage only reads asynchronously, and a whole cache doesn't fit in memory.
pub fn open_cache_deferred(source: &dyn AssetSource) -> Result<Cache, String> {
    let mut cache = Cache::load_deferred(ROOT, &|p| source.read(&relative(p)))
        .map_err(|e| format!("{}: {e}", source.describe()))?;
    cache.root = PathBuf::from(source.describe());
    Ok(cache)
}

/// Load the cache's sounds (an error if it has none: the game then runs silently).
pub fn open_bank(source: &dyn AssetSource) -> Result<Bank, String> {
    Bank::load_cache_with(ROOT, &|p| source.read(&relative(p))).map_err(|e| e.to_string())
}

/// The sound database and its waves (`audio/<path>`) read from the source's directory as
/// courses need them. None for sources without one (the web build prefetches the files, see
/// `web`).
pub fn music_reader(source: &dyn AssetSource) -> Option<mmk_host::audio::MusicReader> {
    let dir = source.dir()?.join("audio");
    Some(Box::new(move |rel| std::fs::read(dir.join(rel)).ok()))
}

/// The course key table (for encrypted `.bcd` courses).
pub fn key_table(source: Option<&dyn AssetSource>) -> Result<Vec<u8>, String> {
    let source = source.ok_or_else(|| mmk_host::course::no_key_table("no asset cache"))?;
    source
        .read(mmk_host::course::KEY_TABLE)
        .map_err(|e| mmk_host::course::no_key_table(format!("{}: {e}", source.describe())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_opens_a_cache() {
        let mut m = MemorySource::new("test files");
        m.insert("manifest.json", b"{}".to_vec());
        m.insert("textures/index.json", b"{}".to_vec());
        let c = open_cache(&m).unwrap();
        assert!(c.textures.is_empty());
        assert!(open_cache_deferred(&m).unwrap().textures.is_empty());
        assert!(music_reader(&m).is_none());
        assert_eq!(c.root, PathBuf::from("test files"));
        assert!(open_bank(&m).is_err(), "no audio/index.json");
        assert!(
            key_table(Some(&m))
                .unwrap_err()
                .contains("imported game data")
        );
    }

    #[test]
    fn missing_manifest_is_an_error() {
        let m = MemorySource::new("empty");
        assert!(open_cache(&m).unwrap_err().starts_with("empty: "));
    }
}
