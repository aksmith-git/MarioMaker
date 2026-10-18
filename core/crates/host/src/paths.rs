//! Where SMCraft keeps a player's data on each platform, so the tools and players agree:
//!
//! | platform | data root |
//! |---|---|
//! | Windows | `%LOCALAPPDATA%\SMCraft` |
//! | macOS | `~/Library/Application Support/SMCraft` (the Apple app's folder) |
//! | Linux and others | `$XDG_DATA_HOME/smcraft`, else `~/.local/share/smcraft` |
//!
//! `$MMK_HOME` overrides the root. Under it: `assets/` (imports: `smm2-<build>`,
//! `smm2-ui-<build>`, `smm1-<build>`), `courses/` (the course library), `play/` (saves and
//! settings of the portable players). Older developer layouts (`~/smm-work/cache/…`) are still
//! found as a fallback.

use std::path::PathBuf;

/// The user's home folder.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// SMCraft's per-user data root (see the module docs).
pub fn data_root() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env("MMK_HOME") {
        return Some(p);
    }
    if cfg!(windows) {
        return env("LOCALAPPDATA")
            .or_else(|| home().map(|h| h.join("AppData").join("Local")))
            .map(|d| d.join("SMCraft"));
    }
    if cfg!(target_os = "macos") {
        return home().map(|h| h.join("Library/Application Support/SMCraft"));
    }
    env("XDG_DATA_HOME")
        .or_else(|| home().map(|h| h.join(".local/share")))
        .map(|d| d.join("smcraft"))
}

/// Where imports go: `<data root>/assets`.
pub fn assets_root() -> Option<PathBuf> {
    data_root().map(|d| d.join("assets"))
}

/// The newest import whose folder name starts with `prefix` (e.g. "smm2-", "smm2-ui-",
/// "smm1-") and holds a manifest, under the assets root; then the developer layout
/// `~/smm-work/cache/<legacy>`. `exclude` skips names with that prefix ("smm2-ui-" when
/// looking for game caches).
pub fn find_import(prefix: &str, exclude: Option<&str>, legacy: &str) -> Option<PathBuf> {
    if let Some(root) = assets_root()
        && let Ok(rd) = std::fs::read_dir(&root)
    {
        let mut hits: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                n.starts_with(prefix)
                    && exclude.is_none_or(|x| !n.starts_with(x))
                    && p.join("manifest.json").is_file()
            })
            .collect();
        hits.sort();
        if let Some(p) = hits.pop() {
            return Some(p);
        }
    }
    home()
        .map(|h| h.join("smm-work/cache").join(legacy))
        .filter(|p| p.join("manifest.json").is_file())
}

/// The game asset cache (`smm2-<build>`).
pub fn game_cache() -> Option<PathBuf> {
    find_import("smm2-", Some("smm2-ui-"), "smm2-3.0.3")
}

/// The SMM2 UI import (`smm2-ui-<build>`).
pub fn smm2_ui() -> Option<PathBuf> {
    find_import("smm2-ui-", None, "smm2-ui-3.0.3")
}

/// The SMM1 UI import (`smm1-<build>`).
pub fn smm1_ui() -> Option<PathBuf> {
    find_import("smm1-", None, "smm1-v208")
}

/// The course library: `<data root>/courses`, else `~/smm-work/courses`.
pub fn library() -> Option<PathBuf> {
    data_root()
        .map(|d| d.join("courses"))
        .filter(|d| d.is_dir())
        .or_else(|| {
            home()
                .map(|h| h.join("smm-work/courses"))
                .filter(|d| d.is_dir())
        })
}

/// Saves and settings of the portable players: `<data root>/play`.
pub fn play_data() -> Option<PathBuf> {
    data_root().map(|d| d.join("play"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_are_found_under_the_data_root() {
        let dir = std::env::temp_dir().join(format!("mmk-paths-{}", std::process::id()));
        let assets = dir.join("assets");
        for n in ["smm2-3.0.3", "smm2-ui-3.0.3", "smm1-v208", "smm2-1.0.0"] {
            std::fs::create_dir_all(assets.join(n)).unwrap();
            std::fs::write(assets.join(n).join("manifest.json"), "{}").unwrap();
        }
        // SAFETY: tests in this crate don't read MMK_HOME concurrently
        unsafe { std::env::set_var("MMK_HOME", &dir) };
        assert_eq!(data_root(), Some(dir.clone()));
        assert_eq!(
            game_cache(),
            Some(assets.join("smm2-3.0.3")),
            "newest build, not the UI"
        );
        assert_eq!(smm2_ui(), Some(assets.join("smm2-ui-3.0.3")));
        assert_eq!(smm1_ui(), Some(assets.join("smm1-v208")));
        unsafe { std::env::remove_var("MMK_HOME") };
        let _ = std::fs::remove_dir_all(&dir);
    }
}
