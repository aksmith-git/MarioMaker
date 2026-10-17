//! C ABI for the Swift app. The header is `app/Sources/CMmk/mmk.h` — keep both in sync
//! (struct layouts are checked by `layout_matches_header` below).
//!
//! Rules: opaque handles; POD structs only; every entry point catches panics; errors are reported
//! through `mmk_last_error()`. Pointers returned by the library stay valid until the next call on
//! the same handle (snapshots) or until the handle is freed (textures).

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use mmk_assets::{Cache, Quad};
use mmk_sim::{Input, Sprite, World};

pub mod challenge;
pub mod condition;
pub mod course_info;
pub mod import;
pub mod ui;

pub use course_info::{MmkCourseInfo, mmk_course_info};
pub use import::{MmkImportProgress, mmk_import_run};
pub mod audio;
use audio::Director;
pub use audio::MmkAudio;

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

/// An empty asset set (no textures, no key table) for running without a game dump. Sprites then
/// come out of `mmk_engine_snapshot` with `texture = MMK_TEXTURE_NONE` and the app draws its own
/// placeholder shapes. Never NULL.
#[unsafe(no_mangle)]
pub extern "C" fn mmk_assets_placeholder() -> *mut MmkAssets {
    Box::into_raw(Box::new(MmkAssets {
        cache: Cache::default(),
        names: Vec::new(),
    }))
}

/// # Safety
/// `assets` must come from `mmk_assets_open` / `mmk_assets_placeholder` and not be used afterwards.
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

/// Texture index and BFRES quad of a (model, texture) name; None if the cache lacks the texture.
type Resolved = Option<(u32, Option<Quad>)>;

pub struct MmkEngine {
    world: World,
    /// The loaded course, kept for `mmk_engine_restart` (None = built-in demo strip).
    course: Option<mmk_formats::smm2::Course>,
    sprites: Vec<MmkSprite>,
    /// (model, texture) → resolved once per name.
    lookup: std::collections::HashMap<(&'static str, &'static str), Resolved>,
    assets: *const MmkAssets,
    /// Sends sound commands to an attached `MmkAudio` (see `mmk_engine_set_audio`).
    director: Option<Director>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkInput {
    /// Bitmask: 1 left, 2 right, 4 up, 8 down, 16 jump, 32 run, 64 start.
    pub buttons: u32,
}

/// `MmkSprite::texture` value for a sprite whose art is not in the asset cache.
pub const MMK_TEXTURE_NONE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkSprite {
    /// Texture index (see mmk_assets_texture), or `MMK_TEXTURE_NONE`.
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
            course: None,
            sprites: Vec::new(),
            lookup: Default::default(),
            assets,
            director: None,
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
                e.course = Some(course);
                e.lookup.clear();
                if let Some(d) = &mut e.director {
                    d.start_course(&e.world);
                }
                true
            }
            Err(msg) => {
                set_error(msg);
                false
            }
        }
    })
}

/// Course bytes in any accepted form → decrypted course data (0x5BFC0 bytes).
/// Returns the data and whether it came from an encrypted `.bcd`.
fn decode_course_data(bytes: &[u8], assets: Option<&MmkAssets>) -> Result<(Vec<u8>, bool), String> {
    use mmk_formats::smm2::{BCD_SIZE, crypto};
    use std::io::Read;
    let mut data = bytes.to_vec();
    if data.len() > 2 && data[0] == 0x78 {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&data[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib: {e}"))?;
        data = out;
    }
    let mut encrypted = false;
    if data.len() == BCD_SIZE {
        let assets = assets.ok_or("encrypted course: needs imported game data (key table)")?;
        let path = assets.cache.root.join("keys/course_table.bin");
        let raw = std::fs::read(&path).map_err(|e| {
            format!(
                "encrypted course: needs imported game data ({}: {e})",
                path.display()
            )
        })?;
        let table = crypto::table_from_bytes(&raw).map_err(|e| e.to_string())?;
        data = crypto::decrypt_bcd(&data, &table, true)
            .map_err(|e| e.to_string())?
            .body;
        encrypted = true;
    }
    Ok((data, encrypted))
}

fn decode_course(bytes: &[u8], assets: &MmkAssets) -> Result<mmk_formats::smm2::Course, String> {
    let (data, _) = decode_course_data(bytes, Some(assets))?;
    mmk_formats::smm2::Course::parse(&data).map_err(|e| e.to_string())
}

/// Restart the loaded course from the beginning (fresh lives, score, timer). With no course
/// loaded, restarts the demo strip.
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_restart(engine: *mut MmkEngine) {
    guard((), || {
        if let Some(e) = unsafe { engine.as_mut() } {
            e.world = match &e.course {
                Some(c) => World::from_course(c),
                None => World::new(),
            };
        }
    })
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
        if let Some(d) = &mut e.director {
            d.after_step(&e.world);
        }
    })
}

/// Screen rectangle and texture coordinates of a sim sprite: [x, y, w, h, u0, v0, u1, v1].
///
/// Tile sprites (`rect` set) draw that part of the texture into the sim's rectangle. Model
/// sprites (`rect` None) follow the convention that the sim's (x + w/2, y) is the model origin
/// and (w, h) the texture size times the sim's scale; when the BFRES quad of the texture is known
/// it is drawn at origin + quad (scaled), mirrored around the origin for `flip_x`, with the
/// quad's texture coordinates. Without a quad the texture fills the sim's rectangle.
/// `flip_y` flips the image in place. Horizontal flipping of the image itself is left to the
/// renderer (`flags` bit 0 swaps u0/u1).
pub fn place(s: &Sprite, tex_w: f32, tex_h: f32, quad: Option<Quad>) -> [f32; 8] {
    let mut out = match (s.image.rect, quad) {
        (Some((x, y, w, h)), _) => [
            s.x,
            s.y,
            s.w,
            s.h,
            x / tex_w,
            y / tex_h,
            (x + w) / tex_w,
            (y + h) / tex_h,
        ],
        (None, Some(q)) => {
            let (sx, sy) = (s.w / tex_w, s.h / tex_h);
            let (ox, oy) = (s.x + s.w / 2.0, s.y);
            let (mut x0, mut x1) = (q.x0 * sx, q.x1 * sx);
            if s.flip_x {
                (x0, x1) = (-x1, -x0);
            }
            [
                ox + x0,
                oy + q.y0 * sy,
                x1 - x0,
                q.height() * sy,
                q.u0,
                q.v0,
                q.u1,
                q.v1,
            ]
        }
        (None, None) => [s.x, s.y, s.w, s.h, 0.0, 0.0, 1.0, 1.0],
    };
    if s.flip_y {
        out.swap(5, 7);
    }
    out
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
            let found = *e.lookup.entry(key).or_insert_with(|| {
                let i = assets.cache.texture_index(key.0, key.1)?;
                Some((i as u32, assets.cache.sprite_quad(key.0, key.1)))
            });
            // missing art (placeholder mode, or not imported yet): report the sprite with
            // MMK_TEXTURE_NONE so the app can draw a stand-in shape
            let (texture, p) = match found {
                Some((texture, quad)) => {
                    let t = &assets.cache.textures[texture as usize].texture;
                    (texture, place(&s, t.width as f32, t.height as f32, quad))
                }
                None => (
                    MMK_TEXTURE_NONE,
                    place(&s, s.w.max(1.0), s.h.max(1.0), None),
                ),
            };
            e.sprites.push(MmkSprite {
                texture,
                x: p[0],
                y: p[1],
                w: p[2],
                h: p[3],
                u0: p[4],
                v0: p[5],
                u1: p[6],
                v1: p[7],
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

/// View flags: bit 0 = draw upside down (underground night).
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_view_flags(engine: *const MmkEngine) -> u32 {
    guard(0, || {
        unsafe { engine.as_ref() }.map_or(0, |e| e.world.upside_down() as u32)
    })
}

/// Background clear colour (0xRRGGBBAA): the theme background's top-left pixel when the cache has
/// it as RGBA8, black otherwise (underground, castle, placeholder art).
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_clear_color(engine: *const MmkEngine) -> u32 {
    guard(0x0000_00ff, || {
        let Some(e) = (unsafe { engine.as_ref() }) else {
            return 0x0000_00ff;
        };
        let Some((model, tex)) = e.world.background() else {
            return 0x0000_00ff;
        };
        let assets = unsafe { &*e.assets };
        let Some(i) = assets.cache.texture_index(model, tex) else {
            return 0x0000_00ff;
        };
        let t = &assets.cache.textures[i].texture;
        if t.format != mmk_assets::mtex::MtexFormat::Rgba8 || t.data.len() < 4 {
            return 0x0000_00ff;
        }
        u32::from_be_bytes([t.data[0], t.data[1], t.data[2], 0xff])
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
    /// Extra lives earned since the course was loaded (monotonic; a run manager banks them).
    pub lives_gained: u32,
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
            lives_gained: h.lives_gained,
        };
    })
}

// --- audio -----------------------------------------------------------------------------------

/// Output sample rate of `mmk_audio_render` (Hz).
pub const MMK_AUDIO_RATE: u32 = mmk_audio::OUTPUT_RATE;

/// Load the sounds of an asset cache (`<path>/audio/`) and create a mixer. NULL on error (e.g. a
/// cache imported without audio); the engine then runs silently.
///
/// # Safety
/// `path` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_audio_open(path: *const c_char) -> *mut MmkAudio {
    guard(std::ptr::null_mut(), || {
        if path.is_null() {
            set_error("path is NULL");
            return std::ptr::null_mut();
        }
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        match mmk_audio::Bank::load_cache(&path) {
            Ok(bank) => Box::into_raw(Box::new(MmkAudio::new(bank))),
            Err(e) => {
                set_error(e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// Free an audio handle. Stop the host's audio thread first (no `mmk_audio_render` may be running
/// or start afterwards). Engines attached to it keep working, silently.
///
/// # Safety
/// `audio` must come from `mmk_audio_open` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_audio_free(audio: *mut MmkAudio) {
    if !audio.is_null() {
        drop(unsafe { Box::from_raw(audio) });
    }
}

/// Output sample rate in Hz (48000).
#[unsafe(no_mangle)]
pub extern "C" fn mmk_audio_sample_rate() -> u32 {
    MMK_AUDIO_RATE
}

/// Render `frames` frames of interleaved stereo f32 (L, R) into `out` (2 × frames floats). Call
/// from the audio thread; does not allocate. Writes silence if `audio` is NULL.
///
/// # Safety
/// `audio` must be a live handle or NULL; `out` must point to `2 * frames` writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_audio_render(audio: *const MmkAudio, out: *mut f32, frames: usize) {
    guard((), || {
        if out.is_null() {
            return;
        }
        let buf = unsafe { std::slice::from_raw_parts_mut(out, frames * 2) };
        match unsafe { audio.as_ref() } {
            Some(a) => a.render(buf),
            None => buf.fill(0.0),
        }
    })
}

/// Set master, music and sound-effect volumes (0…1).
///
/// # Safety
/// `audio` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_audio_set_volumes(
    audio: *const MmkAudio,
    master: f32,
    music: f32,
    sfx: f32,
) {
    guard((), || {
        if let Some(a) = unsafe { audio.as_ref() } {
            a.send(mmk_audio::Command::SetVolumes { master, music, sfx });
        }
    })
}

/// Attach an audio handle to an engine (NULL detaches): from now on each step sends its sound
/// effects and music changes to it, and the current course's music starts.
///
/// # Safety
/// `engine` must be a live handle; `audio` a live handle or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_set_audio(engine: *mut MmkEngine, audio: *const MmkAudio) {
    guard((), || {
        let Some(e) = (unsafe { engine.as_mut() }) else {
            return;
        };
        e.director = unsafe { audio.as_ref() }.map(|a| {
            let mut d = Director::new(a.tx.clone(), &a.bank);
            d.start_course(&e.world);
            d
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(rect: Option<(f32, f32, f32, f32)>, x: f32, y: f32, w: f32, h: f32) -> Sprite {
        Sprite {
            image: mmk_sim::render::SpriteImage {
                model: "M",
                texture: "t",
                rect,
            },
            x,
            y,
            w,
            h,
            flip_x: false,
            flip_y: false,
            layer: 0,
        }
    }

    const GOOMBA: Quad = Quad {
        x0: -8.0,
        y0: -16.0,
        x1: 8.0,
        y1: 16.0,
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
    };

    #[test]
    fn model_sprite_uses_quad_around_origin() {
        // origin (100, 32): sim passes x = origin - w/2, y = origin
        let s = sprite(None, 92.0, 32.0, 16.0, 32.0);
        assert_eq!(
            place(&s, 16.0, 32.0, Some(GOOMBA)),
            [92.0, 16.0, 16.0, 32.0, 0.0, 0.0, 1.0, 1.0]
        );
        // big (×2): the sim's size is twice the texture size
        let s = sprite(None, 84.0, 32.0, 32.0, 64.0);
        assert_eq!(
            place(&s, 16.0, 32.0, Some(GOOMBA))[..4],
            [84.0, 0.0, 32.0, 64.0]
        );
        // no quad: fill the sim rectangle
        let s = sprite(None, 92.0, 32.0, 16.0, 32.0);
        assert_eq!(place(&s, 16.0, 32.0, None)[..4], [92.0, 32.0, 16.0, 32.0]);
    }

    #[test]
    fn flips_mirror_around_origin_and_in_place() {
        let q = Quad {
            x0: 0.0,
            x1: 40.0,
            y0: 0.0,
            y1: 40.0,
            ..GOOMBA
        };
        let mut s = sprite(None, 92.0, 0.0, 40.0, 40.0); // origin x = 112
        s.flip_x = true;
        assert_eq!(place(&s, 40.0, 40.0, Some(q))[..4], [72.0, 0.0, 40.0, 40.0]);
        s.flip_x = false;
        s.flip_y = true;
        let p = place(&s, 40.0, 40.0, Some(q));
        assert_eq!((p[5], p[7]), (1.0, 0.0));
    }

    #[test]
    fn tile_sprite_uses_atlas_rect() {
        let s = sprite(Some((16.0, 0.0, 16.0, 16.0)), 5.0, 6.0, 16.0, 16.0);
        assert_eq!(
            place(&s, 256.0, 256.0, Some(GOOMBA)),
            [5.0, 6.0, 16.0, 16.0, 0.0625, 0.0, 0.125, 0.0625]
        );
    }

    /// Sizes the Swift side relies on (mmk.h).
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<MmkInput>(), 4);
        assert_eq!(size_of::<MmkSprite>(), 44);
        assert_eq!(size_of::<MmkSnapshot>(), 40);
        assert_eq!(size_of::<MmkTextureInfo>(), 48);
        assert_eq!(size_of::<MmkHud>(), 28);
        assert_eq!(size_of::<MmkCourseInfo>(), 480);
        assert_eq!(std::mem::offset_of!(MmkCourseInfo, theme), 452);
        assert_eq!(std::mem::offset_of!(MmkCourseInfo, timer), 456);
        assert_eq!(std::mem::offset_of!(MmkCourseInfo, clear_condition), 460);
        assert_eq!(std::mem::offset_of!(MmkCourseInfo, content_hash), 472);
    }

    #[test]
    fn placeholder_engine_reports_missing_art_and_restarts() {
        unsafe {
            let a = mmk_assets_placeholder();
            assert_eq!(mmk_assets_texture_count(a), 0);
            let e = mmk_engine_new(a);
            let course = mmk_formats::smm2::Course::blank("M1").to_bytes().unwrap();
            assert!(mmk_engine_load_course(e, course.as_ptr(), course.len()));
            let input = MmkInput { buttons: 2 };
            for _ in 0..30 {
                mmk_engine_step(e, &input);
            }
            let mut snap = std::mem::zeroed::<MmkSnapshot>();
            mmk_engine_snapshot(e, &mut snap);
            assert!(snap.sprite_count > 0);
            let sprites = std::slice::from_raw_parts(snap.sprites, snap.sprite_count);
            assert!(sprites.iter().all(|s| s.texture == MMK_TEXTURE_NONE));
            assert_eq!(snap.tick, 30);
            mmk_engine_restart(e);
            mmk_engine_snapshot(e, &mut snap);
            assert_eq!(snap.tick, 0);
            mmk_engine_free(e);
            mmk_assets_free(a);
        }
    }

    /// The header declares every exported function (catches a stale mmk.h).
    #[test]
    fn header_declares_exports() {
        let header = include_str!("../../../../app/Sources/CMmk/mmk.h");
        let src = include_str!("lib.rs");
        for line in src.lines() {
            let Some(rest) = line
                .strip_prefix("pub unsafe extern \"C\" fn ")
                .or_else(|| line.strip_prefix("pub extern \"C\" fn "))
            else {
                continue;
            };
            let name = rest.split('(').next().unwrap();
            assert!(
                header.contains(&format!(" {name}(")) || header.contains(&format!("*{name}(")),
                "{name} missing from mmk.h"
            );
        }
    }

    #[test]
    fn audio_attach_step_and_render() {
        let bank = mmk_audio::Bank::new(vec![
            mmk_audio::Sound::from_pcm("bgm_plain", 48000, 2, vec![1000; 2000]).with_loop(0, 1000),
        ]);
        let audio = Box::into_raw(Box::new(MmkAudio::new(bank)));
        let mut engine = MmkEngine {
            world: World::new(),
            course: None,
            sprites: Vec::new(),
            lookup: Default::default(),
            assets: std::ptr::null(),
            director: None,
        };
        unsafe {
            mmk_engine_set_audio(&mut engine, audio);
            mmk_engine_step(&mut engine, std::ptr::null());
            let mut out = vec![0.0f32; 512];
            mmk_audio_render(audio, out.as_mut_ptr(), 256);
            assert!(out.iter().any(|&s| s > 0.0), "course music is playing");
            mmk_engine_set_audio(&mut engine, std::ptr::null());
            mmk_audio_render(std::ptr::null(), out.as_mut_ptr(), 256);
            assert!(out.iter().all(|&s| s == 0.0));
            mmk_audio_free(audio);
        }
    }
}
