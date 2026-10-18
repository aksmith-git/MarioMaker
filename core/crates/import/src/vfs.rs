//! The importer's file access. Natively it is the file system; a host without one (the web
//! build's import worker, which reads the files the owner picked and hands the output to the
//! browser's storage) installs its own [`Vfs`] for the duration of an import with [`with_vfs`].

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Files the importer reads (RomFS, ExeFS) and writes (the cache).
pub trait Vfs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Write a whole file, creating its parent directories.
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    /// The entries directly inside `dir`, as full paths (any order).
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
    fn is_file(&self, path: &Path) -> bool;
    /// Remove a directory and everything in it (stale output of an earlier import).
    fn remove_dir_all(&self, dir: &Path) -> io::Result<()>;
}

/// The real file system.
pub struct StdFs;

impl Vfs for StdFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)
    }
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.path()))
            .collect()
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn remove_dir_all(&self, dir: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(dir)
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<dyn Vfs>>> = const { RefCell::new(None) };
}

/// Run `f` (an import) with `vfs` as this thread's file access, then restore the previous one.
pub fn with_vfs<R>(vfs: Rc<dyn Vfs>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rc<dyn Vfs>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = prev);
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.borrow_mut().replace(vfs)));
    f()
}

/// The file access in effect on this thread.
pub(crate) fn current() -> Rc<dyn Vfs> {
    CURRENT
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| Rc::new(StdFs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Files in memory.
    #[derive(Default)]
    pub struct MemFs(RefCell<BTreeMap<PathBuf, Vec<u8>>>);

    impl Vfs for MemFs {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.0
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
            self.0.borrow_mut().insert(path.into(), data.to_vec());
            Ok(())
        }
        fn read_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
            let mut out: Vec<PathBuf> = Vec::new();
            for p in self.0.borrow().keys() {
                if let Ok(rest) = p.strip_prefix(dir)
                    && let Some(first) = rest.iter().next()
                    && !out.contains(&dir.join(first))
                {
                    out.push(dir.join(first));
                }
            }
            Ok(out)
        }
        fn is_file(&self, path: &Path) -> bool {
            self.0.borrow().contains_key(path)
        }
        fn remove_dir_all(&self, dir: &Path) -> io::Result<()> {
            self.0.borrow_mut().retain(|p, _| !p.starts_with(dir));
            Ok(())
        }
    }

    #[test]
    fn installed_vfs_is_used_then_restored() {
        let mem = Rc::new(MemFs::default());
        mem.write(Path::new("/r/a/x.bin"), b"x").unwrap();
        let seen = with_vfs(mem.clone(), || {
            let fs = current();
            fs.write(Path::new("/r/b.bin"), b"b").unwrap();
            let mut d = fs.read_dir(Path::new("/r")).unwrap();
            d.sort();
            (
                d,
                fs.is_file(Path::new("/r/a/x.bin")),
                fs.read(Path::new("/r/b.bin")).unwrap(),
            )
        });
        assert_eq!(seen.0, [PathBuf::from("/r/a"), PathBuf::from("/r/b.bin")]);
        assert!(seen.1);
        assert_eq!(seen.2, b"b");
        // outside: the real file system again
        assert!(!current().is_file(Path::new("/r/b.bin")));
    }
}
