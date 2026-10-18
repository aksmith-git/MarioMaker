//! Engine options (Phase 11, SMM1 extras), by name: `mmk_engine_set_option` / `mmk_engine_option`.
//! Names and values are `mmk_sim::Options::set`'s:
//!
//! | name | values |
//! |---|---|
//! | `smm1_shell_kicks` | 0 off (SMM2), 1 SMM1 mid-air shell kicks / shell jumps |
//! | `smm1_layering` | 0 off (SMM2), 1 SMM1 draw order (in a pipe the player is in front of blocks, behind the pipe; coins behind blocks; tracks in front of pipes) |
//! | `smb1_mushroom` | 0 off, 1 Super Mushrooms in SMB1 courses are Mystery Mushrooms, 2 Weird Mushrooms |
//! | `costume` | −1 a random costume per Mystery Mushroom, else a costume id (`mmk_costume_*`) |
//!
//! An engine keeps its options across course loads and restarts.

use std::ffi::{CStr, c_char};

use crate::{MmkEngine, guard, set_error};

/// Returned by `mmk_engine_option` for an unknown name.
pub const MMK_OPTION_UNKNOWN: i32 = i32::MIN;

/// Set an engine option (see the table above). Takes effect at once, also in the running course.
/// Returns false (and sets the last error) for an unknown name or value.
///
/// # Safety
/// `engine` must be a live handle; `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_set_option(
    engine: *mut MmkEngine,
    name: *const c_char,
    value: i32,
) -> bool {
    guard(false, || {
        let Some(e) = (unsafe { engine.as_mut() }) else {
            return false;
        };
        if name.is_null() {
            set_error("name is NULL");
            return false;
        }
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        if !e.options.set(&name, value) {
            set_error(format!("unknown option or value: {name} = {value}"));
            return false;
        }
        e.world.set_options(e.options);
        true
    })
}

/// An engine option's value, or `MMK_OPTION_UNKNOWN` (INT32_MIN) for an unknown name.
///
/// # Safety
/// `engine` must be a live handle; `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_option(engine: *const MmkEngine, name: *const c_char) -> i32 {
    guard(MMK_OPTION_UNKNOWN, || {
        let Some(e) = (unsafe { engine.as_ref() }) else {
            return MMK_OPTION_UNKNOWN;
        };
        if name.is_null() {
            return MMK_OPTION_UNKNOWN;
        }
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        e.options.get(&name).unwrap_or(MMK_OPTION_UNKNOWN)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mmk_assets_free, mmk_assets_placeholder, mmk_engine_free, mmk_engine_new};

    #[test]
    fn options_survive_course_loads() {
        unsafe {
            let a = mmk_assets_placeholder();
            let e = mmk_engine_new(a);
            assert_eq!(mmk_engine_option(e, c"smm1_shell_kicks".as_ptr()), 0);
            assert!(mmk_engine_set_option(e, c"smm1_shell_kicks".as_ptr(), 1));
            assert!(mmk_engine_set_option(e, c"smb1_mushroom".as_ptr(), 2));
            assert!(!mmk_engine_set_option(e, c"smb1_mushroom".as_ptr(), 7));
            assert!(!mmk_engine_set_option(e, c"bogus".as_ptr(), 1));
            assert_eq!(mmk_engine_option(e, c"bogus".as_ptr()), MMK_OPTION_UNKNOWN);
            let course = mmk_formats::smm2::Course::blank("M1").to_bytes().unwrap();
            assert!(crate::mmk_engine_load_course(
                e,
                course.as_ptr(),
                course.len()
            ));
            assert!((*e).world.options.smm1_shell_kicks);
            crate::mmk_engine_restart(e);
            assert_eq!(
                (*e).world.options.smb1_mushroom,
                mmk_sim::MushroomSwap::Weird
            );
            assert_eq!(mmk_engine_option(e, c"smb1_mushroom".as_ptr()), 2);
            mmk_engine_free(e);
            mmk_assets_free(a);
        }
    }
}
