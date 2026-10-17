//! `mmk_course_info`: summary of a course file for the app's library (no engine needed).

use std::ffi::c_char;

use mmk_formats::smm2::Course;

use crate::{MmkAssets, decode_course_data, guard, set_error};

/// Game styles the engine can play today (the sim decides; a course in another style is
/// listed as "not yet supported").
pub use mmk_sim::level::PLAYABLE_STYLES;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkCourseInfo {
    /// Course name, UTF-8, NUL-terminated (truncated on a character boundary if needed).
    pub name: [c_char; 128],
    /// Course description, UTF-8, NUL-terminated.
    pub description: [c_char; 320],
    /// Game style code, NUL-terminated: "M1", "M3", "MW", "WU" or "3W".
    pub style: [c_char; 4],
    /// Main / sub area theme: 0 ground, 1 underground, 2 castle, 3 airship, 4 underwater,
    /// 5 ghost house, 6 snow, 7 desert, 8 sky, 9 forest.
    pub theme: u8,
    pub sub_theme: u8,
    /// bit 0: main area is night, bit 1: sub area is night.
    pub night: u8,
    /// 1 if the engine can play this course (see `PLAYABLE_STYLES` and `unsupported`).
    pub playable: u8,
    /// Time limit in seconds (0 = none).
    pub timer: i16,
    /// 1 if the input was an encrypted `.bcd`.
    pub encrypted: u8,
    pub clear_condition_category: u8,
    pub clear_condition: u32,
    /// Placed objects in both areas.
    pub object_count: u32,
    /// Objects the engine cannot run yet. Always 0 for now: hook for when the sim reports
    /// unsupported object ids (a course with any is then not playable).
    pub unsupported: u32,
    /// FNV-1a 64 of the decrypted, decompressed course data: the same course gives the same
    /// hash whether it came zlib-compressed, raw or encrypted. Used as the save-data key.
    pub content_hash: u64,
}

impl Default for MmkCourseInfo {
    fn default() -> Self {
        Self {
            name: [0; 128],
            description: [0; 320],
            style: [0; 4],
            theme: 0,
            sub_theme: 0,
            night: 0,
            playable: 0,
            timer: 0,
            encrypted: 0,
            clear_condition_category: 0,
            clear_condition: 0,
            object_count: 0,
            unsupported: 0,
            content_hash: 0,
        }
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Copy `s` into a C string buffer, truncating on a UTF-8 character boundary.
pub(crate) fn fill<const N: usize>(out: &mut [c_char; N], s: &str) {
    let mut end = s.len().min(N - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    for (o, &b) in out.iter_mut().zip(&s.as_bytes()[..end]) {
        *o = b as c_char;
    }
    out[end] = 0;
}

pub fn course_info(bytes: &[u8], assets: Option<&MmkAssets>) -> Result<MmkCourseInfo, String> {
    let (data, encrypted) = decode_course_data(bytes, assets)?;
    let c = Course::parse(&data).map_err(|e| e.to_string())?;
    let h = &c.header;
    let style = h.game_style.as_str_lossy();
    let unsupported = mmk_sim::level::unsupported_features(&c).len() as u32;
    let mut info = MmkCourseInfo {
        theme: c.main.header.theme,
        sub_theme: c.sub.header.theme,
        night: c.main.header.is_night() as u8 | (c.sub.header.is_night() as u8) << 1,
        playable: (PLAYABLE_STYLES.contains(&style.as_str()) && unsupported == 0) as u8,
        timer: h.timer,
        encrypted: encrypted as u8,
        clear_condition_category: h.clear_condition_category,
        clear_condition: h.clear_condition,
        object_count: (c.main.objects.len() + c.sub.objects.len()) as u32,
        unsupported,
        content_hash: fnv1a64(&data),
        ..Default::default()
    };
    fill(&mut info.name, &h.name.to_string_lossy());
    fill(&mut info.description, &h.description.to_string_lossy());
    fill(&mut info.style, &style);
    Ok(info)
}

/// The SMM2 course code ("XXX-XXX-XXX") of server data id `data_id`, NUL-terminated into
/// `buf` (12 bytes are enough). Returns the code's length; `cap` 0 sizes it.
///
/// # Safety
/// `buf` must be writable for `cap` bytes (or NULL with `cap` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_smm2_course_code(data_id: u32, buf: *mut c_char, cap: usize) -> usize {
    guard(0, || {
        let code = mmk_formats::course_id::smm2_data_id_to_code(data_id, false);
        if !buf.is_null() && cap > 0 {
            let out = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
            let n = code.len().min(cap - 1); // ASCII: any cut is a character boundary
            for (o, &b) in out.iter_mut().zip(&code.as_bytes()[..n]) {
                *o = b as c_char;
            }
            out[n] = 0;
        }
        code.len()
    })
}

/// Read a course file's summary. Accepts the same inputs as `mmk_engine_load_course`;
/// `assets` (may be NULL) supplies the key table for encrypted `.bcd` files.
/// Returns false and sets the last error if the data is not a course. Thread-safe.
///
/// # Safety
/// `data` must point to `len` readable bytes, `out` must be writable, `assets` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_course_info(
    assets: *const MmkAssets,
    data: *const u8,
    len: usize,
    out: *mut MmkCourseInfo,
) -> bool {
    guard(false, || {
        let Some(out) = (unsafe { out.as_mut() }) else {
            set_error("out is NULL");
            return false;
        };
        if data.is_null() {
            set_error("data is NULL");
            return false;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        match course_info(bytes, unsafe { assets.as_ref() }) {
            Ok(info) => {
                *out = info;
                true
            }
            Err(msg) => {
                set_error(msg);
                false
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_formats::smm2::Course;
    use std::ffi::CStr;
    use std::io::Write;

    fn cstr<const N: usize>(b: &[c_char; N]) -> String {
        unsafe { CStr::from_ptr(b.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn info_from_raw_and_zlib_agree() {
        let mut c = Course::blank("M1");
        c.header.name.set("Test ☆ course");
        c.header.description.set("A description");
        c.main.header.theme = 2;
        c.main.header.flags = 2;
        let raw = c.to_bytes().unwrap();
        let a = course_info(&raw, None).unwrap();
        assert_eq!(cstr(&a.name), "Test ☆ course");
        assert_eq!(cstr(&a.description), "A description");
        assert_eq!(cstr(&a.style), "M1");
        assert_eq!((a.theme, a.night, a.timer, a.playable), (2, 1, 300, 1));

        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let b = course_info(&z.finish().unwrap(), None).unwrap();
        assert_eq!(a.content_hash, b.content_hash);

        let other = course_info(&Course::blank("WU").to_bytes().unwrap(), None).unwrap();
        assert_eq!(other.playable, 1);
        assert_ne!(other.content_hash, a.content_hash);
    }

    #[test]
    fn encrypted_without_keys_is_an_error() {
        let err = course_info(&vec![0u8; mmk_formats::smm2::BCD_SIZE], None).unwrap_err();
        assert!(err.contains("imported game data"), "{err}");
        assert!(course_info(b"not a course", None).is_err());
    }

    #[test]
    fn fill_truncates_on_char_boundary() {
        let mut b = [0 as c_char; 4];
        fill(&mut b, "a☆b"); // 'a' + 3-byte star: only 'a' fits before the terminator
        assert_eq!(cstr(&b), "a");
    }
}
