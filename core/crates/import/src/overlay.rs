//! A RomFS view with a separately extracted update laid over the base game (update files win),
//! built from links so the dump is never copied: symlinks on Unix, hard links on Windows.

use std::path::Path;

/// Build a RomFS view with `update` laid over `base`: a tree of symlinks under `dir`
/// (update files win). Cheap: the dump is never copied.
pub fn overlay_romfs(base: &Path, update: &Path, dir: &Path) -> std::io::Result<()> {
    fn link_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let from = entry.path();
            let to = dst.join(entry.file_name());
            // follow symlinks in the source (the owner's RomFS folder may itself be one)
            if std::fs::metadata(&from)?.is_dir() {
                if to.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
                    std::fs::remove_file(&to)?;
                }
                link_tree(&from, &to)?;
            } else {
                if to.symlink_metadata().is_ok() {
                    std::fs::remove_file(&to)?;
                }
                symlink(&from, &to)?;
            }
        }
        Ok(())
    }
    #[cfg(unix)]
    fn symlink(from: &Path, to: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(from, to)
    }
    // Windows: symlinks need special rights, so hard links (same volume, no rights needed),
    // else a copy
    #[cfg(not(unix))]
    fn symlink(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::hard_link(from, to).or_else(|_| std::fs::copy(from, to).map(|_| ()))
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    link_tree(base, dir)?;
    link_tree(update, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_prefers_update_files() {
        let root = std::env::temp_dir().join(format!("mmk-overlay-imp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (base, upd, out) = (root.join("base"), root.join("upd"), root.join("out"));
        std::fs::create_dir_all(base.join("Pack")).unwrap();
        std::fs::create_dir_all(upd.join("Pack")).unwrap();
        std::fs::write(base.join("Pack/a"), "base-a").unwrap();
        std::fs::write(base.join("Pack/b"), "base-b").unwrap();
        std::fs::write(upd.join("Pack/b"), "upd-b").unwrap();
        overlay_romfs(&base, &upd, &out).unwrap();
        let read = |p: &str| std::fs::read_to_string(out.join(p)).unwrap();
        assert_eq!(
            (read("Pack/a"), read("Pack/b")),
            ("base-a".into(), "upd-b".into())
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
