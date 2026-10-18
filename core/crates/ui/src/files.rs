//! Where a UI import's files come from: a folder on disk ([`DirFiles`]), or files in memory
//! ([`MemFiles`]) that a host without a synchronous file system fills in — the web build reads
//! the browser's storage asynchronously, so it hands over the small files (layouts, fonts'
//! metrics, message tables) before opening and lets textures follow on request ([`Fetch::Later`],
//! see [`crate::UiCache::take_requests`]).
//!
//! Paths are relative to the import root (the folder holding `manifest.json` and `ui/`), with
//! `/` separators.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// The answer to a read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetch {
    Ready(Vec<u8>),
    /// The file exists but is read later: the cache records a request for it (textures only;
    /// other files must be [`Fetch::Ready`]).
    Later,
    Missing,
}

/// A UI import's files.
pub trait UiFiles: Send + Sync {
    fn read(&self, rel: &str) -> Fetch;
    /// Names of the entries (files and folders) directly inside folder `rel` (`""`: the root).
    fn list(&self, rel: &str) -> Vec<String>;
    fn is_dir(&self, rel: &str) -> bool;
    /// For messages: where the files are.
    fn describe(&self) -> String;
    /// The folder on disk, when the files are one.
    fn dir(&self) -> Option<&Path> {
        None
    }
}

/// An import folder on disk.
pub struct DirFiles(PathBuf);

impl DirFiles {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self(dir.into())
    }
}

impl UiFiles for DirFiles {
    fn read(&self, rel: &str) -> Fetch {
        match std::fs::read(self.0.join(rel)) {
            Ok(b) => Fetch::Ready(b),
            Err(_) => Fetch::Missing,
        }
    }
    fn list(&self, rel: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(self.0.join(rel)) else {
            return Vec::new();
        };
        rd.flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }
    fn is_dir(&self, rel: &str) -> bool {
        self.0.join(rel).is_dir()
    }
    fn describe(&self) -> String {
        self.0.display().to_string()
    }
    fn dir(&self) -> Option<&Path> {
        Some(&self.0)
    }
}

/// Files in memory, plus files known to exist that are read later ([`Fetch::Later`]).
#[derive(Default)]
pub struct MemFiles {
    name: String,
    files: HashMap<String, Vec<u8>>,
    later: BTreeSet<String>,
}

impl MemFiles {
    /// `name`: where the files came from (for messages).
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn insert(&mut self, rel: impl Into<String>, bytes: Vec<u8>) {
        let rel = rel.into();
        self.later.remove(&rel);
        self.files.insert(rel, bytes);
    }

    /// `rel` exists and is read on request.
    pub fn insert_later(&mut self, rel: impl Into<String>) {
        let rel = rel.into();
        if !self.files.contains_key(&rel) {
            self.later.insert(rel);
        }
    }

    pub fn len(&self) -> usize {
        self.files.len() + self.later.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys().chain(self.later.iter())
    }
}

impl UiFiles for MemFiles {
    fn read(&self, rel: &str) -> Fetch {
        match self.files.get(rel) {
            Some(b) => Fetch::Ready(b.clone()),
            None if self.later.contains(rel) => Fetch::Later,
            None => Fetch::Missing,
        }
    }
    fn list(&self, rel: &str) -> Vec<String> {
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{}/", rel.trim_end_matches('/'))
        };
        let names: BTreeSet<String> = self
            .paths()
            .filter_map(|p| p.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect();
        names.into_iter().collect()
    }
    fn is_dir(&self, rel: &str) -> bool {
        let prefix = format!("{}/", rel.trim_end_matches('/'));
        self.paths().any(|p| p.starts_with(&prefix))
    }
    fn describe(&self) -> String {
        self.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_files_list_and_defer() {
        let mut m = MemFiles::new("test");
        m.insert("manifest.json", b"{}".to_vec());
        m.insert("ui/layouts/A.json", b"{}".to_vec());
        m.insert_later("ui/textures/A/t.mtex");
        assert_eq!(m.read("manifest.json"), Fetch::Ready(b"{}".to_vec()));
        assert_eq!(m.read("ui/textures/A/t.mtex"), Fetch::Later);
        assert_eq!(m.read("nope"), Fetch::Missing);
        assert_eq!(m.list(""), ["manifest.json", "ui"]);
        assert_eq!(m.list("ui"), ["layouts", "textures"]);
        assert!(m.is_dir("ui/layouts") && !m.is_dir("ui/layouts/A.json"));
        m.insert("ui/textures/A/t.mtex", vec![1]);
        assert_eq!(m.read("ui/textures/A/t.mtex"), Fetch::Ready(vec![1]));
        assert_eq!(m.len(), 3);
    }
}
