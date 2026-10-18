//! SMM1 costumes (Phase 11): add the costume art the SMM1 importer wrote (`<smm1 cache>/costumes`,
//! from `mmk import-smm1`) to an asset set, and list the costumes for a picker.

use std::ffi::{CStr, CString, c_char};

use mmk_assets::Cache;

use crate::{MmkAssets, guard, set_error};

/// Add the costume cache at `path` to `assets`: its textures follow the ones already there.
/// Call it right after `mmk_assets_open`, before reading `mmk_assets_texture_count` / handing
/// the assets to an engine. Returns the number of costumes (0, with the last error set, when the
/// folder can't be read).
///
/// # Safety
/// `assets` must be a live handle not yet used by an engine; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_add_costumes(
    assets: *mut MmkAssets,
    path: *const c_char,
) -> u32 {
    guard(0, || {
        let Some(a) = (unsafe { assets.as_mut() }) else {
            return 0;
        };
        if path.is_null() {
            set_error("path is NULL");
            return 0;
        }
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        let extra = match Cache::open(&path) {
            Ok(c) => c,
            Err(e) => {
                set_error(e.to_string());
                return 0;
            }
        };
        if extra.manifest["kind"] != "costumes" {
            set_error(format!("{path}: not a costume cache"));
            return 0;
        }
        a.cache.merge(extra);
        a.names = a
            .cache
            .textures
            .iter()
            .map(|t| {
                (
                    CString::new(t.model.clone()).unwrap_or_default(),
                    CString::new(t.name.clone()).unwrap_or_default(),
                )
            })
            .collect();
        a.costume_names = a
            .cache
            .costume_names()
            .into_iter()
            .map(|n| CString::new(n).unwrap_or_default())
            .collect();
        a.costume_names.len() as u32
    })
}

/// Number of costumes the assets know (0 without a costume cache).
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_costume_count(assets: *const MmkAssets) -> u32 {
    guard(0, || {
        unsafe { assets.as_ref() }.map_or(0, |a| a.costume_names.len() as u32)
    })
}

/// Display name of costume `id` (valid while `assets` lives), or NULL when out of range.
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_costume_name(
    assets: *const MmkAssets,
    id: u32,
) -> *const c_char {
    guard(std::ptr::null(), || {
        unsafe { assets.as_ref() }
            .and_then(|a| a.costume_names.get(id as usize))
            .map_or(std::ptr::null(), |n| n.as_ptr())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mmk_assets_free, mmk_assets_placeholder, mmk_assets_texture_count};

    #[test]
    fn costume_cache_is_merged_and_listed() {
        let dir = std::env::temp_dir().join(format!("mmk-ffi-costumes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("textures/SMM1_Costume_000")).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"kind":"costumes","costumes":[{"id":0,"res":"A","name":"Alpha"},{"id":1,"res":"B"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("textures/index.json"),
            r#"{"SMM1_Costume_000":{"wait.0":{"w":1,"h":1}}}"#,
        )
        .unwrap();
        let t = mmk_assets::Mtex {
            format: mmk_assets::MtexFormat::Rgba8,
            variant: mmk_assets::MtexVariant::Unorm,
            width: 1,
            height: 1,
            channel_select: [2, 3, 4, 5],
            data: vec![9, 9, 9, 9],
        };
        std::fs::write(
            dir.join("textures/SMM1_Costume_000/wait.0.mtex"),
            t.to_bytes(),
        )
        .unwrap();
        let p = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let a = mmk_assets_placeholder();
            assert_eq!(mmk_assets_costume_count(a), 0);
            assert_eq!(mmk_assets_add_costumes(a, p.as_ptr()), 2);
            assert_eq!(mmk_assets_texture_count(a), 1);
            assert_eq!(mmk_assets_costume_count(a), 2);
            let name = CStr::from_ptr(mmk_assets_costume_name(a, 0))
                .to_str()
                .unwrap();
            assert_eq!(name, "Alpha");
            let name = CStr::from_ptr(mmk_assets_costume_name(a, 1))
                .to_str()
                .unwrap();
            assert_eq!(name, "B");
            assert!(mmk_assets_costume_name(a, 2).is_null());
            assert_eq!(mmk_assets_add_costumes(a, c"/nonexistent".as_ptr()), 0);
            mmk_assets_free(a);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
