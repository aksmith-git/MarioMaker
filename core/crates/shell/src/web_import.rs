//! The game-data import in the browser (wasm32, run in a worker by `web/mmk-import-worker.js`):
//! the same importer as `mmk import` (`mmk-import`), reading the RomFS and ExeFS files the owner
//! picked and handing every output file to the page, which stores it in the browser's own
//! storage. Nothing leaves the browser.
//!
//! Workers can read picked files synchronously (`FileReaderSync`), which the importer needs; they
//! can't wait for storage, so outputs go to the page as messages. The worker's
//! `mmkImport` object provides the calls below.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mmk_import::vfs::Vfs;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// The bytes of a picked file (`/romfs/...`, `/exefs/main`), or null.
    #[wasm_bindgen(js_namespace = mmkImport, js_name = read)]
    fn js_read(path: &str) -> JsValue;
    /// Names of the entries directly inside a picked directory.
    #[wasm_bindgen(js_namespace = mmkImport, js_name = list)]
    fn js_list(dir: &str) -> js_sys::Array;
    #[wasm_bindgen(js_namespace = mmkImport, js_name = isFile)]
    fn js_is_file(path: &str) -> bool;
    /// An output file, by its path in the asset cache (`textures/...`).
    #[wasm_bindgen(js_namespace = mmkImport, js_name = write)]
    fn js_write(path: &str, bytes: js_sys::Uint8Array);
    #[wasm_bindgen(js_namespace = mmkImport, js_name = progress)]
    fn js_progress(stage: &str, done: u32, total: u32);
}

/// Where the importer is told to put the cache; stripped (with the build folder) on output.
const OUT: &str = "/out";
/// Where the SMM2 UI importer is told to put its import; output paths there get [`UI_PREFIX`].
const UI_OUT: &str = "/ui-out";
const UI_PREFIX: &str = "@ui/";

struct WorkerFs;

fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

impl Vfs for WorkerFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let v = js_read(&path_str(path));
        if v.is_null() || v.is_undefined() {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(js_sys::Uint8Array::new(&v).to_vec())
    }
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        // the SMM2 UI import: /ui-out/<rel> → @ui/<rel> (the page keeps it apart)
        if let Ok(rel) = path.strip_prefix(UI_OUT) {
            js_write(
                &format!("{UI_PREFIX}{}", path_str(rel)),
                js_sys::Uint8Array::from(data),
            );
            return Ok(());
        }
        // /out/smm2-<build>/<rel> → <rel>
        let rel = path
            .strip_prefix(OUT)
            .ok()
            .and_then(|p| {
                p.iter()
                    .next()
                    .map(|build| p.strip_prefix(build).unwrap_or(p))
            })
            .ok_or_else(|| {
                io::Error::other(format!("output outside the cache: {}", path_str(path)))
            })?;
        js_write(&path_str(rel), js_sys::Uint8Array::from(data));
        Ok(())
    }
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(js_list(&path_str(dir))
            .iter()
            .filter_map(|n| n.as_string())
            .map(|n| dir.join(n))
            .collect())
    }
    fn is_file(&self, path: &Path) -> bool {
        js_is_file(&path_str(path))
    }
    fn remove_dir_all(&self, _dir: &Path) -> io::Result<()> {
        // the page imports into empty storage
        Ok(())
    }
}

/// Run the import over the worker's files (`/romfs/…`, `/exefs/main`). Returns a summary
/// (`{build, textures, sounds, courses}`).
#[wasm_bindgen]
pub fn import_dump() -> Result<JsValue, JsValue> {
    let cfg = mmk_import::ImportConfig {
        romfs: PathBuf::from("/romfs"),
        exefs: PathBuf::from("/exefs"),
        out_root: PathBuf::from(OUT),
    };
    let report = mmk_import::vfs::with_vfs(Rc::new(WorkerFs), || {
        mmk_import::run(&cfg, &mut |stage, done, total| {
            js_progress(stage, done as u32, total as u32)
        })
    })
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let summary = serde_json::json!({
        "build": report.build,
        "textures": report.textures,
        "sounds": report.sounds,
        "courses": report.builtin_courses_verified,
    });
    Ok(JsValue::from_str(&summary.to_string()))
}

/// Run the SMM2 UI import (`mmk import-smm2-ui`: the menus' layouts, fonts, messages and
/// sounds) over the same files; its outputs are written as `@ui/<path>`. Returns a summary
/// (`{build, layouts, textures, fonts}`).
#[wasm_bindgen]
pub fn import_ui() -> Result<JsValue, JsValue> {
    let cfg = mmk_import::smm2_ui::Smm2UiConfig {
        romfs: PathBuf::from("/romfs"),
        exefs: Some(PathBuf::from("/exefs")),
        out: PathBuf::from(UI_OUT),
        language: "USen".into(),
    };
    let report = mmk_import::vfs::with_vfs(Rc::new(WorkerFs), || {
        mmk_import::run_smm2_ui(&cfg, &mut |stage, done, total| {
            js_progress(&format!("menus: {stage}"), done as u32, total as u32)
        })
    })
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let summary = serde_json::json!({
        "build": report.build,
        "layouts": report.layouts,
        "textures": report.textures,
        "fonts": report.fonts,
    });
    Ok(JsValue::from_str(&summary.to_string()))
}
