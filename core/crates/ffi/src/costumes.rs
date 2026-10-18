//! SMM1 extras art (Phase 11): add the SMM1 sprite cache `mmk import-smm1` writes
//! (`<smm1 cache>/sprites`: Mystery Mushroom costumes, Lanky Mario, the Weird and Mystery
//! Mushrooms) to an asset set, and list the costumes for a picker.

use std::ffi::{CStr, CString, c_char};

use mmk_assets::Cache;

use crate::{MmkAssets, guard, set_error};

/// Add the SMM1 sprite cache at `path` to `assets`: its textures follow the ones already there.
/// Call it right after `mmk_assets_open`, before reading `mmk_assets_texture_count` or making an
/// engine. Returns the number of costumes with art (0 with the last error set when the folder
/// can't be read; a cache without costumes also returns 0, without an error).
///
/// # Safety
/// `assets` must be a live handle not yet used by an engine; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_add_smm1_sprites(
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
        if extra.manifest["kind"] != "smm1-sprites" {
            set_error(format!("{path}: not an SMM1 sprite cache"));
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
        a.costumes = a
            .cache
            .costumes()
            .into_iter()
            .map(|(id, n)| (id, CString::new(n).unwrap_or_default()))
            .collect();
        a.costumes.len() as u32
    })
}

/// Play the SMM1 costume sounds of the SMM1 sprite cache at `path` (its `audio/`) through
/// `audio`. Call it before `mmk_engine_set_audio` (engines attached earlier keep their setting).
///
/// # Safety
/// `audio` must be a live handle from `mmk_audio_open`; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_audio_add_smm1_sprites(
    audio: *mut crate::MmkAudio,
    path: *const c_char,
) -> bool {
    guard(false, || {
        let Some(a) = (unsafe { audio.as_mut() }) else {
            return false;
        };
        if path.is_null() {
            set_error("path is NULL");
            return false;
        }
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        a.set_costume_dir(path);
        true
    })
}

/// Make the engine's random Mystery Mushroom costumes come from the costumes `assets` has art
/// for (`mmk_engine_new` does this already; call again after adding an SMM1 sprite cache to
/// assets an engine already uses).
///
/// # Safety
/// `engine` and `assets` must be live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_use_costumes(
    engine: *mut crate::MmkEngine,
    assets: *const MmkAssets,
) {
    guard((), || {
        let (Some(e), Some(a)) = (unsafe { engine.as_mut() }, unsafe { assets.as_ref() }) else {
            return;
        };
        e.options
            .set_costume_pool(a.costumes.iter().map(|(id, _)| *id));
        e.world.set_options(e.options);
    })
}

/// Number of costumes the assets have art for (0 without an SMM1 sprite cache).
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_costume_count(assets: *const MmkAssets) -> u32 {
    guard(0, || {
        unsafe { assets.as_ref() }.map_or(0, |a| a.costumes.len() as u32)
    })
}

/// Costume id of list entry `index` (0 … count − 1), the value for the engine option
/// `costume`; −1 when out of range.
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_costume_id(assets: *const MmkAssets, index: u32) -> i32 {
    guard(-1, || {
        unsafe { assets.as_ref() }
            .and_then(|a| a.costumes.get(index as usize))
            .map_or(-1, |(id, _)| i32::from(*id))
    })
}

/// Display name of list entry `index` (valid while `assets` lives), or NULL when out of range.
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_costume_name(
    assets: *const MmkAssets,
    index: u32,
) -> *const c_char {
    guard(std::ptr::null(), || {
        unsafe { assets.as_ref() }
            .and_then(|a| a.costumes.get(index as usize))
            .map_or(std::ptr::null(), |(_, n)| n.as_ptr())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mmk_assets_free, mmk_assets_placeholder, mmk_assets_texture_count};

    #[test]
    fn sprite_cache_is_merged_and_costumes_listed() {
        let dir = std::env::temp_dir().join(format!("mmk-ffi-smm1-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("textures/SMM1_Costume_045")).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"kind":"smm1-sprites","costumes":[{"id":45,"res":"A","name":"Alpha"},
                {"id":107,"res":"B"},{"id":3,"res":"C","art":false}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("textures/index.json"),
            r#"{"SMM1_Costume_045":{"wait.0":{"w":1,"h":1}}}"#,
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
            dir.join("textures/SMM1_Costume_045/wait.0.mtex"),
            t.to_bytes(),
        )
        .unwrap();
        let p = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let a = mmk_assets_placeholder();
            assert_eq!(mmk_assets_costume_count(a), 0);
            assert_eq!(mmk_assets_add_smm1_sprites(a, p.as_ptr()), 2);
            assert_eq!(mmk_assets_texture_count(a), 1);
            assert_eq!(mmk_assets_costume_count(a), 2);
            assert_eq!(mmk_assets_costume_id(a, 0), 45);
            assert_eq!(mmk_assets_costume_id(a, 1), 107);
            assert_eq!(mmk_assets_costume_id(a, 2), -1);
            let name = CStr::from_ptr(mmk_assets_costume_name(a, 0))
                .to_str()
                .unwrap();
            assert_eq!(name, "Alpha");
            let name = CStr::from_ptr(mmk_assets_costume_name(a, 1))
                .to_str()
                .unwrap();
            assert_eq!(name, "B");
            assert!(mmk_assets_costume_name(a, 2).is_null());
            assert_eq!(mmk_assets_add_smm1_sprites(a, c"/nonexistent".as_ptr()), 0);
            mmk_assets_free(a);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
