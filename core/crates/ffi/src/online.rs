//! The open-source course server for the app's Course World (`mmk-online`, Phase 10): search,
//! download, play / clear reports. Calls block on the network; the app runs them off the main
//! thread. Never talks to Nintendo's network.

use std::ffi::{CStr, c_char};

use mmk_online::{Client, Listing};
use serde_json::{Value, json};

use crate::{guard, set_error};

fn str_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

fn listing_json(l: &Listing) -> Value {
    json!({
        "code": l.code, "name": l.name, "maker": l.maker, "style": l.style, "theme": l.theme,
        "plays": l.plays, "clears": l.clears, "likes": l.likes, "difficulty": l.difficulty,
    })
}

/// Copy `s` into `buf` (NUL-terminated when it fits) and return its length; `cap` 0 sizes it.
fn give(s: &str, buf: *mut c_char, cap: usize) -> usize {
    if !buf.is_null() && cap > s.len() {
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), buf as *mut u8, s.len());
            *buf.add(s.len()) = 0;
        }
    }
    s.len()
}

/// Search `server` ("http://host:port"): `query` text, `sort` "new" / "popular" / "hard".
/// Writes a JSON array of listings (code, name, maker, style, theme, plays, clears, likes,
/// difficulty) and returns its length; 0 on a network error (`mmk_last_error`). Call with
/// `cap` 0 to size, then again — or once with a large buffer.
///
/// # Safety
/// Strings NUL-terminated (`query` / `sort` may be NULL), `buf` writable for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_online_search(
    server: *const c_char,
    query: *const c_char,
    sort: *const c_char,
    offset: u32,
    limit: u32,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let Some(server) = str_arg(server) else {
            set_error("no server");
            return 0;
        };
        let client = Client::new(server);
        let q = str_arg(query).unwrap_or("");
        let sort = str_arg(sort).unwrap_or("popular");
        match client.search(q, None, sort, offset as usize, limit as usize) {
            Ok(list) => {
                let s = Value::Array(list.iter().map(listing_json).collect()).to_string();
                give(&s, buf, cap)
            }
            Err(e) => {
                set_error(format!("{server}: {e}"));
                0
            }
        }
    })
}

/// Download course `code` from `server` into the file `path` (raw course data).
///
/// # Safety
/// Strings NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_online_download(
    server: *const c_char,
    code: *const c_char,
    path: *const c_char,
) -> bool {
    guard(false, || {
        let (Some(server), Some(code), Some(path)) =
            (str_arg(server), str_arg(code), str_arg(path))
        else {
            set_error("bad arguments");
            return false;
        };
        match Client::new(server).download(code) {
            Ok(data) => match std::fs::write(path, data) {
                Ok(()) => true,
                Err(e) => {
                    set_error(format!("{path}: {e}"));
                    false
                }
            },
            Err(e) => {
                set_error(format!("{server}: {e}"));
                false
            }
        }
    })
}

/// Report a "play", "clear" or "like" of course `code` to `server`.
///
/// # Safety
/// Strings NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_online_report(
    server: *const c_char,
    code: *const c_char,
    what: *const c_char,
) -> bool {
    guard(false, || {
        let (Some(server), Some(code), Some(what)) =
            (str_arg(server), str_arg(code), str_arg(what))
        else {
            set_error("bad arguments");
            return false;
        };
        match Client::new(server).report(code, what) {
            Ok(()) => true,
            Err(e) => {
                set_error(format!("{server}: {e}"));
                false
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_server_reports_an_error() {
        let n = unsafe {
            mmk_online_search(
                c"http://127.0.0.1:9".as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                10,
                std::ptr::null_mut(),
                0,
            )
        };
        assert_eq!(n, 0);
    }

    #[test]
    fn give_sizes_and_copies() {
        assert_eq!(give("abc", std::ptr::null_mut(), 0), 3);
        let mut b = [0 as c_char; 8];
        assert_eq!(give("abc", b.as_mut_ptr(), b.len()), 3);
        assert_eq!(
            unsafe { CStr::from_ptr(b.as_ptr()) }.to_str().unwrap(),
            "abc"
        );
    }
}
