//! C ABI for the Swift app. The header is `app/Sources/CMmk/include/mmk.h` — keep both in sync
//! (struct layouts are checked by `layout_matches_header` below).
//!
//! Rules: opaque handles; POD structs only; every entry point catches panics; errors are reported
//! through `mmk_last_error()`. Pointers returned by the library stay valid until the next call on
//! the same handle (snapshots) or until the handle is freed (textures).

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use mmk_assets::Cache;
use mmk_sim::{Input, World};

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: impl Into<String>) {
    let s = CString::new(msg.into().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = s);
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            set_error(format!("internal error: {msg}"));
            fallback
        }
    }
}

/// Last error message on this thread (empty if none). Valid until the next failing call.
#[unsafe(no_mangle)]
pub extern "C" fn mmk_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

// --- assets ----------------------------------------------------------------------------------

pub struct MmkAssets {
    cache: Cache,
    names: Vec<(CString, CString)>,
}

#[repr(C)]
pub struct MmkTextureInfo {
    pub model: *const c_char,
    pub name: *const c_char,
    pub width: u32,
    pub height: u32,
    /// mtex format code: 1 rgba8, 2 bc1, 3 bc2, 4 bc3, 5 bc4, 6 bc5, 7 bc6h, 8 bc7
    pub format: u8,
    /// 0 unorm, 1 snorm, 2 srgb
    pub variant: u8,
    pub channel_select: [u8; 4],
    pub data: *const u8,
    pub len: usize,
}

/// Open an asset cache directory (e.g. `.../assets/smm2-3.0.3`). NULL on error.
///
/// # Safety
/// `path` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_open(path: *const c_char) -> *mut MmkAssets {
    guard(std::ptr::null_mut(), || {
        if path.is_null() {
            set_error("path is NULL");
            return std::ptr::null_mut();
        }
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        match Cache::open(&path) {
            Ok(cache) => {
                let names = cache
                    .textures
                    .iter()
                    .map(|t| {
                        (
                            CString::new(t.model.clone()).unwrap_or_default(),
                            CString::new(t.name.clone()).unwrap_or_default(),
                        )
                    })
                    .collect();
                Box::into_raw(Box::new(MmkAssets { cache, names }))
            }
            Err(e) => {
                set_error(e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// # Safety
/// `assets` must come from `mmk_assets_open` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_free(assets: *mut MmkAssets) {
    if !assets.is_null() {
        drop(unsafe { Box::from_raw(assets) });
    }
}

/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_texture_count(assets: *const MmkAssets) -> usize {
    guard(0, || {
        unsafe { assets.as_ref() }
            .map(|a| a.cache.textures.len())
            .unwrap_or(0)
    })
}

/// Fill `out` with texture `index`. Returns false if out of range.
///
/// # Safety
/// `assets` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_assets_texture(
    assets: *const MmkAssets,
    index: usize,
    out: *mut MmkTextureInfo,
) -> bool {
    guard(false, || {
        let (Some(a), Some(out)) = (unsafe { assets.as_ref() }, unsafe { out.as_mut() }) else {
            return false;
        };
        let Some(t) = a.cache.textures.get(index) else {
            return false;
        };
        let (model, name) = &a.names[index];
        *out = MmkTextureInfo {
            model: model.as_ptr(),
            name: name.as_ptr(),
            width: t.texture.width,
            height: t.texture.height,
            format: t.texture.format as u8,
            variant: t.texture.variant as u8,
            channel_select: t.texture.channel_select,
            data: t.texture.data.as_ptr(),
            len: t.texture.data.len(),
        };
        true
    })
}

// --- engine ----------------------------------------------------------------------------------

pub struct MmkEngine {
    world: World,
    sprites: Vec<MmkSprite>,
    /// (model, texture) → texture index, resolved once per name.
    lookup: std::collections::HashMap<(&'static str, &'static str), Option<u32>>,
    assets: *const MmkAssets,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkInput {
    /// Bitmask: 1 left, 2 right, 4 up, 8 down, 16 jump, 32 run, 64 start.
    pub buttons: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkSprite {
    /// Texture index (see mmk_assets_texture).
    pub texture: u32,
    /// Bottom-left corner in world pixels (y up) and size in pixels.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Texture coordinates, (0,0) = top-left of the texture.
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub layer: u32,
    /// bit 0: flip horizontally
    pub flags: u32,
}

#[repr(C)]
pub struct MmkSnapshot {
    pub sprites: *const MmkSprite,
    pub sprite_count: usize,
    /// Bottom-left of the view in world pixels, and view size.
    pub camera_x: f32,
    pub camera_y: f32,
    pub view_w: f32,
    pub view_h: f32,
    pub tick: u64,
}

/// Create an engine drawing from `assets` (which must outlive the engine).
///
/// # Safety
/// `assets` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_new(assets: *const MmkAssets) -> *mut MmkEngine {
    guard(std::ptr::null_mut(), || {
        if assets.is_null() {
            set_error("assets is NULL");
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new(MmkEngine {
            world: World::new(),
            sprites: Vec::new(),
            lookup: Default::default(),
            assets,
        }))
    })
}

/// # Safety
/// `engine` must come from `mmk_engine_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_free(engine: *mut MmkEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Load a course into the engine (main area). Accepts SMM2 course data (0x5BFC0 bytes, optionally
/// zlib-compressed) or an encrypted `.bcd` (decrypted with the asset cache's key table).
/// Returns false and sets the last error on failure.
///
/// # Safety
/// `engine` must be a live handle; `data` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_load_course(
    engine: *mut MmkEngine,
    data: *const u8,
    len: usize,
) -> bool {
    guard(false, || {
        let Some(e) = (unsafe { engine.as_mut() }) else {
            return false;
        };
        if data.is_null() {
            set_error("data is NULL");
            return false;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        match decode_course(bytes, unsafe { &*e.assets }) {
            Ok(course) => {
                e.world = World::from_course(&course);
                e.lookup.clear();
                true
            }
            Err(msg) => {
                set_error(msg);
                false
            }
        }
    })
}

fn decode_course(bytes: &[u8], assets: &MmkAssets) -> Result<mmk_formats::smm2::Course, String> {
    use mmk_formats::smm2::{BCD_SIZE, Course, crypto};
    use std::io::Read;
    let mut data = bytes.to_vec();
    if data.len() > 2 && data[0] == 0x78 {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&data[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib: {e}"))?;
        data = out;
    }
    if data.len() == BCD_SIZE {
        let path = assets.cache.root.join("keys/course_table.bin");
        let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let table = crypto::table_from_bytes(&raw).map_err(|e| e.to_string())?;
        data = crypto::decrypt_bcd(&data, &table, true)
            .map_err(|e| e.to_string())?
            .body;
    }
    Course::parse(&data).map_err(|e| e.to_string())
}

/// Advance the simulation by exactly one 60 Hz tick.
///
/// # Safety
/// `engine` must be a live handle; `input` may be NULL (no buttons).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_step(engine: *mut MmkEngine, input: *const MmkInput) {
    guard((), || {
        let Some(e) = (unsafe { engine.as_mut() }) else {
            return;
        };
        let buttons = unsafe { input.as_ref() }.map(|i| i.buttons).unwrap_or(0);
        e.world.step(Input { buttons });
    })
}

/// Describe the current frame. Pointers stay valid until the next call on this engine.
///
/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_snapshot(engine: *mut MmkEngine, out: *mut MmkSnapshot) {
    guard((), || {
        let (Some(e), Some(out)) = (unsafe { engine.as_mut() }, unsafe { out.as_mut() }) else {
            return;
        };
        let assets = unsafe { &*e.assets };
        let mut sprites = e.world.sprites();
        sprites.sort_by_key(|s| s.layer);
        e.sprites.clear();
        for s in sprites {
            let key = (s.image.model, s.image.texture);
            let idx = *e
                .lookup
                .entry(key)
                .or_insert_with(|| assets.cache.texture_index(key.0, key.1).map(|i| i as u32));
            let Some(texture) = idx else { continue };
            let t = &assets.cache.textures[texture as usize].texture;
            let (tw, th) = (t.width as f32, t.height as f32);
            let (u0, mut v0, u1, mut v1) = match s.image.rect {
                Some((x, y, w, h)) => (x / tw, y / th, (x + w) / tw, (y + h) / th),
                None => (0.0, 0.0, 1.0, 1.0),
            };
            if s.flip_y {
                std::mem::swap(&mut v0, &mut v1);
            }
            e.sprites.push(MmkSprite {
                texture,
                x: s.x,
                y: s.y,
                w: s.w,
                h: s.h,
                u0,
                v0,
                u1,
                v1,
                layer: s.layer,
                flags: s.flip_x as u32,
            });
        }
        *out = MmkSnapshot {
            sprites: e.sprites.as_ptr(),
            sprite_count: e.sprites.len(),
            camera_x: e.world.camera_x,
            camera_y: e.world.camera_y,
            view_w: mmk_sim::world::VIEW_W,
            view_h: mmk_sim::world::VIEW_H,
            tick: e.world.tick,
        };
    })
}

/// Debug: move the player (feet, px). Used by smoke tests to reach a spot quickly.
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_debug_teleport(engine: *mut MmkEngine, x: f32, y: f32) {
    guard((), || {
        if let Some(e) = unsafe { engine.as_mut() } {
            e.world.player.x = x;
            e.world.player.y = y;
            e.world.player.vy = 0.0;
            e.world.reset_camera();
        }
    })
}

/// HUD counters and this tick's events (bit n = `mmk_sim::Event` discriminant n).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkHud {
    pub score: u32,
    pub coins: u32,
    pub lives: u32,
    /// Seconds left, −1 = no timer.
    pub time: i32,
    /// bit 0: course cleared; bit 1: player dying
    pub flags: u32,
    pub events: u32,
}

/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_hud(engine: *const MmkEngine, out: *mut MmkHud) {
    guard((), || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return;
        };
        let h = e.world.hud();
        let events = e
            .world
            .events
            .iter()
            .fold(0u32, |m, &ev| m | 1 << (ev as u32));
        *out = MmkHud {
            score: h.score,
            coins: h.coins,
            lives: h.lives,
            time: h.time,
            flags: h.cleared as u32 | (!e.world.player.alive() as u32) << 1,
            events,
        };
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sizes the Swift side relies on (mmk.h).
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<MmkInput>(), 4);
        assert_eq!(size_of::<MmkSprite>(), 44);
        assert_eq!(size_of::<MmkSnapshot>(), 40);
        assert_eq!(size_of::<MmkTextureInfo>(), 48);
        assert_eq!(size_of::<MmkHud>(), 24);
    }
}
