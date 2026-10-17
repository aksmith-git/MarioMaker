//! `mmk_challenge_*`: the 100 Mario Challenge session (`mmk_modes::challenge`) for the app.
//!
//! The app adds its playable library courses, starts a run, plays the course the session picks
//! in an engine (seeded with `mmk_engine_set_run_counters`), and reports each result. Save data is
//! a JSON string the app stores; read it back with `mmk_challenge_save_json` after every result.

use std::ffi::{CStr, c_char};

use mmk_modes::challenge::Challenge;
use mmk_modes::hundred::{Difficulty, Next, Outcome};
use mmk_modes::pool::CourseRef;

use crate::course_info::fill;
use crate::{MmkEngine, guard, set_error};

pub struct MmkChallenge(Challenge);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MmkRunCourse {
    /// The id given to `mmk_challenge_add_course` (the app uses the file path), NUL-terminated.
    pub id: [c_char; 1024],
    pub name: [c_char; 128],
    pub maker: [c_char; 128],
    /// Difficulty value 0–100.
    pub difficulty: u8,
    pub _pad: [u8; 7],
}

impl Default for MmkRunCourse {
    fn default() -> Self {
        Self {
            id: [0; 1024],
            name: [0; 128],
            maker: [0; 128],
            difficulty: 0,
            _pad: [0; 7],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkRunStatus {
    pub course_number: u32,
    pub total: u32,
    pub lives: u32,
    pub coins: u32,
    pub score: u32,
    pub banked: u32,
    pub deaths: u32,
    pub skips: u32,
}

fn difficulty(d: u8) -> Option<Difficulty> {
    Difficulty::ALL.get(d as usize).copied()
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn write_course(c: &CourseRef, out: *mut MmkRunCourse) -> bool {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = MmkRunCourse::default();
    fill(&mut out.id, &c.id);
    fill(&mut out.name, &c.name);
    fill(&mut out.maker, &c.maker);
    out.difficulty = c.difficulty;
    true
}

/// `save_json` may be NULL (fresh save data); unreadable JSON also starts fresh.
///
/// # Safety
/// `save_json` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_new(
    save_json: *const c_char,
    seed: u64,
) -> *mut MmkChallenge {
    guard(std::ptr::null_mut(), || {
        let save = (!save_json.is_null()).then(|| text(save_json));
        Box::into_raw(Box::new(MmkChallenge(Challenge::from_json(
            save.as_deref(),
            seed,
        ))))
    })
}

/// # Safety
/// `c` is NULL or a handle from `mmk_challenge_new`, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_free(c: *mut MmkChallenge) {
    if !c.is_null() {
        drop(unsafe { Box::from_raw(c) });
    }
}

/// Offer a playable course to the pools. `difficulty` 0–100 (fail rate); `castle`: main area
/// uses the castle theme.
///
/// # Safety
/// `c` is a live handle; the strings are NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_add_course(
    c: *mut MmkChallenge,
    id: *const c_char,
    name: *const c_char,
    maker: *const c_char,
    difficulty: u8,
    castle: bool,
) {
    guard((), || {
        if let Some(c) = unsafe { c.as_mut() } {
            c.0.add_course(
                CourseRef {
                    id: text(id),
                    name: text(name),
                    maker: text(maker),
                    difficulty: difficulty.min(100),
                },
                castle,
            );
        }
    })
}

/// Library courses in the band of difficulty 0 Easy, 1 Normal, 2 Expert, 3 Super Expert.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_available(c: *const MmkChallenge, d: u8) -> u32 {
    guard(0, || match (unsafe { c.as_ref() }, difficulty(d)) {
        (Some(c), Some(d)) => c.0.available(d) as u32,
        _ => 0,
    })
}

/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_unlocked(c: *const MmkChallenge, d: u8) -> bool {
    guard(false, || match (unsafe { c.as_ref() }, difficulty(d)) {
        (Some(c), Some(d)) => c.0.save.unlocked(d),
        _ => false,
    })
}

/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_can_continue(c: *const MmkChallenge, d: u8) -> bool {
    guard(false, || match (unsafe { c.as_ref() }, difficulty(d)) {
        (Some(c), Some(d)) => c.0.save.can_continue(d),
        _ => false,
    })
}

/// Completed runs of a difficulty.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_clears(c: *const MmkChallenge, d: u8) -> u32 {
    guard(0, || match (unsafe { c.as_ref() }, difficulty(d)) {
        (Some(c), Some(d)) => c.0.save.clears[d as usize],
        _ => 0,
    })
}

/// New Game (`resume` false) or Continue. False when no course is available (error set).
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_start(c: *mut MmkChallenge, d: u8, resume: bool) -> bool {
    guard(false, || {
        let (Some(c), Some(d)) = (unsafe { c.as_mut() }, difficulty(d)) else {
            return false;
        };
        match c.0.start(d, resume) {
            Ok(()) => true,
            Err(_) => {
                set_error("not enough courses");
                false
            }
        }
    })
}

/// The course to play now; false when there is none.
///
/// # Safety
/// `c` is a live handle, `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_course(
    c: *const MmkChallenge,
    out: *mut MmkRunCourse,
) -> bool {
    guard(false, || {
        match unsafe { c.as_ref() }.and_then(|c| c.0.current()) {
            Some(course) => write_course(course, out),
            None => false,
        }
    })
}

/// The course begins (after its intro): pre-charges a life in the Continue record.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_course_started(c: *mut MmkChallenge) {
    guard((), || {
        if let Some(c) = unsafe { c.as_mut() } {
            c.0.course_started();
        }
    })
}

/// The current course could not be loaded: pick another. False when none is left.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_unloadable(c: *mut MmkChallenge) -> bool {
    guard(false, || {
        unsafe { c.as_mut() }.is_some_and(|c| c.0.unloadable().is_ok())
    })
}

/// Extra lives earned in the course (bank, at most 3 per course).
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_extra_lives(c: *mut MmkChallenge, n: u32) {
    guard((), || {
        if let Some(c) = unsafe { c.as_mut() } {
            c.0.extra_lives(n);
        }
    })
}

/// Report a result: outcome 0 cleared, 1 died (or restarted), 2 skipped, 3 Save & Quit; `score`
/// and `coins` are the engine's counters on a clear. Returns 0 play the next course, 1 retry,
/// 2 game over, 3 run complete, 4 back to the map.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_finish(
    c: *mut MmkChallenge,
    outcome: u8,
    score: u32,
    coins: u32,
) -> u8 {
    guard(4, || {
        let Some(c) = (unsafe { c.as_mut() }) else {
            return 4;
        };
        let outcome = match outcome {
            0 => Outcome::Cleared,
            1 => Outcome::Died,
            2 => Outcome::Skipped,
            _ => Outcome::SaveAndQuit,
        };
        match c.0.finish(outcome, score, coins) {
            Next::Play => 0,
            Next::Retry => 1,
            Next::GameOver => 2,
            Next::Complete => 3,
            Next::Quit => 4,
        }
    })
}

/// # Safety
/// `c` is a live handle, `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_status(c: *const MmkChallenge, out: *mut MmkRunStatus) {
    guard((), || {
        let (Some(c), Some(out)) = (unsafe { c.as_ref() }, unsafe { out.as_mut() }) else {
            return;
        };
        let s = c.0.status();
        *out = MmkRunStatus {
            course_number: s.course_number,
            total: s.total,
            lives: s.lives,
            coins: s.coins,
            score: s.score,
            banked: s.banked,
            deaths: s.deaths,
            skips: s.skips,
        };
    })
}

/// Courses cleared in this run (staff credits).
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_cleared_count(c: *const MmkChallenge) -> usize {
    guard(0, || {
        unsafe { c.as_ref() }.map_or(0, |c| c.0.cleared().len())
    })
}

/// # Safety
/// `c` is a live handle, `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_cleared(
    c: *const MmkChallenge,
    i: usize,
    out: *mut MmkRunCourse,
) -> bool {
    guard(false, || {
        match unsafe { c.as_ref() }.and_then(|c| c.0.cleared().get(i)) {
            Some(course) => write_course(course, out),
            None => false,
        }
    })
}

/// A difficulty newly unlocked by the run just completed (1 Expert … 3 Super Expert), once;
/// −1 when there is none.
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_take_unlock(c: *mut MmkChallenge) -> i32 {
    guard(-1, || {
        unsafe { c.as_mut() }
            .and_then(|c| c.0.take_unlock())
            .map_or(-1, |d| d as i32)
    })
}

/// Leave the run (after game over, the ending or Save & Quit).
///
/// # Safety
/// `c` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_end(c: *mut MmkChallenge) {
    guard((), || {
        if let Some(c) = unsafe { c.as_mut() } {
            c.0.end();
        }
    })
}

/// Save data as JSON. Returns its length in bytes (without the NUL); writes it NUL-terminated
/// into `buf` when `cap` > length. Call with `cap` 0 to size the buffer.
///
/// # Safety
/// `c` is a live handle; `buf` is NULL or holds `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_challenge_save_json(
    c: *const MmkChallenge,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let Some(c) = (unsafe { c.as_ref() }) else {
            return 0;
        };
        let s = c.0.save_json();
        if !buf.is_null() && cap > s.len() {
            unsafe {
                std::ptr::copy_nonoverlapping(s.as_ptr() as *const c_char, buf, s.len());
                *buf.add(s.len()) = 0;
            }
        }
        s.len()
    })
}

/// Start the loaded course from a run's counters: score, coins and lives shown in the HUD, and
/// what a restart after a death returns to.
///
/// # Safety
/// `engine` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_set_run_counters(
    engine: *mut MmkEngine,
    score: u32,
    coins: u32,
    lives: u32,
) {
    guard((), || {
        if let Some(e) = unsafe { engine.as_mut() } {
            e.world.set_run_counters(score, coins, lives);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<MmkRunCourse>(), 1288);
        assert_eq!(size_of::<MmkRunStatus>(), 32);
    }

    #[test]
    fn run_through_the_c_interface() {
        unsafe {
            let c = mmk_challenge_new(std::ptr::null(), 5);
            for i in 0..10 {
                let id = std::ffi::CString::new(format!("/lib/{i}.zlib")).unwrap();
                mmk_challenge_add_course(
                    c,
                    id.as_ptr(),
                    c"Name".as_ptr(),
                    c"Maker".as_ptr(),
                    20,
                    false,
                );
            }
            assert_eq!(mmk_challenge_available(c, 0), 10);
            assert!(!mmk_challenge_unlocked(c, 2));
            assert!(mmk_challenge_start(c, 0, false));
            let mut course = MmkRunCourse::default();
            assert!(mmk_challenge_course(c, &mut course));
            assert!(
                CStr::from_ptr(course.id.as_ptr())
                    .to_str()
                    .unwrap()
                    .starts_with("/lib/")
            );
            mmk_challenge_course_started(c);
            assert_eq!(mmk_challenge_finish(c, 1, 0, 0), 1);
            assert_eq!(mmk_challenge_finish(c, 0, 1200, 3), 0);
            let mut st = MmkRunStatus::default();
            mmk_challenge_status(c, &mut st);
            assert_eq!(
                (st.course_number, st.total, st.lives, st.score),
                (2, 8, 100, 1200)
            );
            let n = mmk_challenge_save_json(c, std::ptr::null_mut(), 0);
            let mut buf = vec![0 as c_char; n + 1];
            assert_eq!(mmk_challenge_save_json(c, buf.as_mut_ptr(), buf.len()), n);
            let json = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert!(json.contains("\"easy\""));
            mmk_challenge_free(c);
        }
    }
}
