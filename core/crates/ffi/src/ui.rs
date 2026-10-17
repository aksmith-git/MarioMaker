//! C ABI for the layout player (SMM1 screens): open an imported SMM1 UI folder, instantiate a
//! layout, bind animations at frames, and get a flat list of quads to draw. Declared in
//! `app/Sources/CMmk/mmk.h` (section "layout player").
//!
//! Textures: every texture the opened layouts use is listed by `mmk_ui_texture_count` /
//! `mmk_ui_texture` with stable indices; the list only grows (when another layout is opened), so
//! the renderer uploads the new entries after each `mmk_ui_layout`.

use std::ffi::{CStr, CString, c_char};

use mmk_ui::{LayoutPlayer, UiCache};

use crate::{MmkTextureInfo, guard, set_error};

/// One quad of a layout draw list (`MmkUiQuad` in mmk.h, 132 bytes).
pub type MmkUiQuad = mmk_ui::Quad;

pub struct MmkUi {
    cache: UiCache,
    names: Vec<CString>,
}

pub struct MmkUiLayout {
    player: LayoutPlayer,
    quads: Vec<MmkUiQuad>,
}

static MODEL: &CStr = c"ui";

unsafe fn str_arg<'a>(p: *const c_char, what: &str) -> Option<&'a str> {
    if p.is_null() {
        set_error(format!("{what} is NULL"));
        return None;
    }
    match unsafe { CStr::from_ptr(p) }.to_str() {
        Ok(s) => Some(s),
        Err(_) => {
            set_error(format!("{what} is not UTF-8"));
            None
        }
    }
}

/// Open an imported SMM1 UI folder (holding `manifest.json` and `ui/`). NULL on error.
///
/// # Safety
/// `dir` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_open(dir: *const c_char) -> *mut MmkUi {
    guard(std::ptr::null_mut(), || {
        let Some(dir) = (unsafe { str_arg(dir, "dir") }) else {
            return std::ptr::null_mut();
        };
        match UiCache::open(dir) {
            Ok(cache) => Box::into_raw(Box::new(MmkUi {
                cache,
                names: Vec::new(),
            })),
            Err(e) => {
                set_error(e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// # Safety
/// `ui` must come from `mmk_ui_open` and not be used afterwards. Layout handles stay usable
/// (they own their data) but their texture indices refer to this cache.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_free(ui: *mut MmkUi) {
    if !ui.is_null() {
        drop(unsafe { Box::from_raw(ui) });
    }
}

/// Instantiate layout `name` (e.g. "Nwk_WorldMapEsy_00"), loading it, its part layouts,
/// textures, fonts and messages on first use. NULL on error.
///
/// # Safety
/// `ui` must be a live handle; `name` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_layout(ui: *mut MmkUi, name: *const c_char) -> *mut MmkUiLayout {
    guard(std::ptr::null_mut(), || {
        let Some(ui) = (unsafe { ui.as_mut() }) else {
            set_error("ui is NULL");
            return std::ptr::null_mut();
        };
        let Some(name) = (unsafe { str_arg(name, "name") }) else {
            return std::ptr::null_mut();
        };
        match ui.cache.layout(name) {
            Ok(res) => Box::into_raw(Box::new(MmkUiLayout {
                player: LayoutPlayer::new(res),
                quads: Vec::new(),
            })),
            Err(e) => {
                set_error(e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// # Safety
/// `layout` must come from `mmk_ui_layout` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_layout_free(layout: *mut MmkUiLayout) {
    if !layout.is_null() {
        drop(unsafe { Box::from_raw(layout) });
    }
}

/// Layout size in layout units (1280×720 for full-screen SMM1 layouts).
///
/// # Safety
/// `layout` must be a live handle; `w` and `h` writable (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_layout_size(layout: *const MmkUiLayout, w: *mut f32, h: *mut f32) {
    guard((), || {
        if let Some(l) = unsafe { layout.as_ref() } {
            let [sw, sh] = l.player.size();
            if let Some(w) = unsafe { w.as_mut() } {
                *w = sw;
            }
            if let Some(h) = unsafe { h.as_mut() } {
                *h = sh;
            }
        }
    })
}

/// Bind animation `anim` (`[PartPane/…/]Name`, e.g. "In" or "L_WorldMapMario_00/Walk") at
/// `frame`, or move it to `frame` if already bound. False (and an error) if unknown.
///
/// # Safety
/// `layout` must be a live handle; `anim` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_play(
    layout: *mut MmkUiLayout,
    anim: *const c_char,
    frame: f32,
) -> bool {
    guard(false, || {
        let (Some(l), Some(anim)) = (unsafe { layout.as_mut() }, unsafe { str_arg(anim, "anim") })
        else {
            return false;
        };
        let ok = l.player.play(anim, frame);
        if !ok {
            set_error(format!("no animation '{anim}'"));
        }
        ok
    })
}

/// Unbind `anim`, or every animation when `anim` is NULL.
///
/// # Safety
/// `layout` must be a live handle; `anim` NULL or a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_stop(layout: *mut MmkUiLayout, anim: *const c_char) {
    guard((), || {
        let Some(l) = (unsafe { layout.as_mut() }) else {
            return;
        };
        let anim = if anim.is_null() {
            None
        } else {
            unsafe { CStr::from_ptr(anim) }.to_str().ok()
        };
        l.player.stop(anim);
    })
}

/// Length of an animation in frames (60 per second), or −1 if there is none by that name.
///
/// # Safety
/// `layout` must be a live handle; `anim` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_anim_length(layout: *mut MmkUiLayout, anim: *const c_char) -> f32 {
    guard(-1.0, || {
        let (Some(l), Some(anim)) = (unsafe { layout.as_mut() }, unsafe { str_arg(anim, "anim") })
        else {
            return -1.0;
        };
        l.player.animation_length(anim).unwrap_or(-1.0)
    })
}

/// Set the text of text pane `pane` (`[PartPane/…/]Pane`) to UTF-8 `text`.
///
/// # Safety
/// `layout` must be a live handle; `pane` and `text` valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_set_text(
    layout: *mut MmkUiLayout,
    pane: *const c_char,
    text: *const c_char,
) -> bool {
    guard(false, || {
        let Some(l) = (unsafe { layout.as_mut() }) else {
            return false;
        };
        let (Some(pane), Some(text)) = (unsafe { str_arg(pane, "pane") }, unsafe {
            str_arg(text, "text")
        }) else {
            return false;
        };
        let ok = l.player.set_text(pane, text);
        if !ok {
            set_error(format!("no text pane '{pane}'"));
        }
        ok
    })
}

/// A pane's values after the bound animations (`MmkUiPane` in mmk.h, 48 bytes), in its parent's
/// space (layout units, y up).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkUiPane {
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 2],
    pub size: [f32; 2],
    pub alpha: f32,
    pub visible: u32,
}

/// Force pane `pane` (`[PartPane/…/]Pane`) hidden (0) or shown (1) over its animations, or hand
/// it back to them (−1). False (and an error) if there is no such pane.
///
/// # Safety
/// `layout` must be a live handle; `pane` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_set_visible(
    layout: *mut MmkUiLayout,
    pane: *const c_char,
    visible: i32,
) -> bool {
    guard(false, || {
        let (Some(l), Some(pane)) = (unsafe { layout.as_mut() }, unsafe { str_arg(pane, "pane") })
        else {
            return false;
        };
        let v = match visible {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        let ok = l.player.set_visible(pane, v);
        if !ok {
            set_error(format!("no pane '{pane}'"));
        }
        ok
    })
}

/// Fill `out` with pane `pane`'s values (`[PartPane/…/]Pane`) under the bound animations.
/// False if there is no such pane.
///
/// # Safety
/// `layout` must be a live handle; `pane` a valid NUL-terminated string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_pane(
    layout: *const MmkUiLayout,
    pane: *const c_char,
    out: *mut MmkUiPane,
) -> bool {
    guard(false, || {
        let (Some(l), Some(pane), Some(out)) = (
            unsafe { layout.as_ref() },
            unsafe { str_arg(pane, "pane") },
            unsafe { out.as_mut() },
        ) else {
            return false;
        };
        match l.player.pane_info(pane) {
            Some(p) => {
                *out = MmkUiPane {
                    translate: p.translate,
                    rotate: p.rotate,
                    scale: p.scale,
                    size: p.size,
                    alpha: p.alpha,
                    visible: p.visible as u32,
                };
                true
            }
            None => {
                set_error(format!("no pane '{pane}'"));
                false
            }
        }
    })
}

/// Copy message `label` (from this layout's text table, else a part layout's) as UTF-8 into
/// `buf` (NUL-terminated, truncated to `cap`), each value it takes (number, name) replaced by
/// U+FFFC. Returns the full length in bytes, or 0 if there is no such message; `cap` 0 sizes it.
///
/// # Safety
/// `layout` must be a live handle; `label` a valid NUL-terminated string; `buf` writable for
/// `cap` bytes (or NULL with `cap` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_message(
    layout: *const MmkUiLayout,
    label: *const c_char,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let (Some(l), Some(label)) = (unsafe { layout.as_ref() }, unsafe { str_arg(label, "label") })
        else {
            return 0;
        };
        let Some(text) = l.player.message(label) else {
            return 0;
        };
        let bytes = text.as_bytes();
        if !buf.is_null() && cap > 0 {
            let n = bytes.len().min(cap - 1);
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, n);
                *buf.add(n) = 0;
            }
        }
        bytes.len()
    })
}

/// Evaluate the layout with its bound animations and return the quads to draw, back to front.
/// `*out` stays valid until the next call on this layout.
///
/// # Safety
/// `layout` must be a live handle; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_draw_list(
    layout: *mut MmkUiLayout,
    out: *mut *const MmkUiQuad,
) -> usize {
    guard(0, || {
        let (Some(l), Some(out)) = (unsafe { layout.as_mut() }, unsafe { out.as_mut() }) else {
            return 0;
        };
        l.quads.clear();
        l.player.draw(&mut l.quads);
        *out = l.quads.as_ptr();
        l.quads.len()
    })
}

/// Number of textures known to the cache so far.
///
/// # Safety
/// `ui` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_texture_count(ui: *const MmkUi) -> usize {
    guard(0, || {
        unsafe { ui.as_ref() }
            .map(|u| u.cache.textures.len())
            .unwrap_or(0)
    })
}

/// Fill `out` with texture `index` (`model` = "ui", `name` = "<layout>/<texture>" or
/// "font:<font>/<sheet>"). Pointers stay valid until `mmk_ui_free`.
///
/// # Safety
/// `ui` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_ui_texture(
    ui: *mut MmkUi,
    index: usize,
    out: *mut MmkTextureInfo,
) -> bool {
    guard(false, || {
        let (Some(u), Some(out)) = (unsafe { ui.as_mut() }, unsafe { out.as_mut() }) else {
            return false;
        };
        while u.names.len() < u.cache.textures.len() {
            let n = &u.cache.textures[u.names.len()].name;
            u.names
                .push(CString::new(n.replace('\0', " ")).unwrap_or_default());
        }
        let Some(t) = u.cache.textures.get(index) else {
            return false;
        };
        *out = MmkTextureInfo {
            model: MODEL.as_ptr(),
            name: u.names[index].as_ptr(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Sizes and offsets the Swift side relies on (mmk.h, MmkUiQuad).
    #[test]
    fn ui_layout_matches_header() {
        assert_eq!(size_of::<MmkUiQuad>(), 132);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, uv0), 32);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, uv1), 64);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, color), 96);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, black), 112);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, white), 116);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, tex0), 120);
        assert_eq!(std::mem::offset_of!(MmkUiQuad, flags), 128);
        assert_eq!(size_of::<MmkUiPane>(), 48);
        assert_eq!(std::mem::offset_of!(MmkUiPane, alpha), 40);
    }

    #[test]
    fn errors_are_reported() {
        unsafe {
            assert!(mmk_ui_open(c"/nonexistent/dir".as_ptr()).is_null());
            assert!(!CStr::from_ptr(crate::mmk_last_error()).is_empty());
            assert!(mmk_ui_open(std::ptr::null()).is_null());
            let mut p = std::ptr::null();
            assert_eq!(mmk_ui_draw_list(std::ptr::null_mut(), &mut p), 0);
        }
    }

    /// Against the owner's import when present (skipped in CI).
    #[test]
    fn owner_import_world_map() {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let dir = home.join("smm-work/cache/smm1-v208");
        if !dir.join("manifest.json").is_file() {
            eprintln!("no SMM1 import; skipping");
            return;
        }
        unsafe {
            let d = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
            let ui = mmk_ui_open(d.as_ptr());
            assert!(!ui.is_null());
            let l = mmk_ui_layout(ui, c"Nwk_WorldMapEsy_00".as_ptr());
            assert!(
                !l.is_null(),
                "{:?}",
                CStr::from_ptr(crate::mmk_last_error())
            );
            assert!(mmk_ui_anim_length(l, c"MapIn".as_ptr()) > 0.0);
            assert!(mmk_ui_play(l, c"MapIn".as_ptr(), 30.0));
            let mut q = std::ptr::null();
            let n = mmk_ui_draw_list(l, &mut q);
            assert!(n > 20, "{n} quads");
            assert!(mmk_ui_texture_count(ui) > 10);
            let mut info = std::mem::zeroed::<MmkTextureInfo>();
            assert!(mmk_ui_texture(ui, 0, &mut info));
            mmk_ui_layout_free(l);
            mmk_ui_free(ui);
        }
    }
}
