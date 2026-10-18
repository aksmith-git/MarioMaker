//! The open-source course server for the app's Course World (`mmk-online`, Phase 10): search,
//! download, play / clear reports. Calls block on the network; the app runs them off the main
//! thread. Never talks to Nintendo's network.

use std::ffi::{CStr, c_char};

use mmk_online::Client;
use serde_json::{Value, json};

use crate::{guard, set_error};

fn str_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

use mmk_online::listing_json;

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

/// One account-side call, JSON in and out (so the C API stays one function): the actions of
/// [`mmk_online::call`], plus `submit_replay {code, path}` (a replay file) → listing and
/// `record_replay {code, path}` (written) → `{}`.
pub fn call(
    server: &str,
    token: Option<&str>,
    action: &str,
    args: &Value,
) -> Result<Value, String> {
    let c = Client::new(server).with_token(token.filter(|t| !t.is_empty()).map(str::to_string));
    let s = |k: &str| args[k].as_str().unwrap_or_default();
    Ok(match action {
        "submit_replay" => {
            let bytes = std::fs::read(s("path")).map_err(|e| format!("{}: {e}", s("path")))?;
            listing_json(&c.submit_replay(s("code"), &bytes)?)
        }
        "record_replay" => {
            let bytes = c.record_replay(s("code"))?;
            std::fs::write(s("path"), bytes).map_err(|e| format!("{}: {e}", s("path")))?;
            json!({})
        }
        _ => return mmk_online::call(server, token, action, args),
    })
}

/// [`call`] for C: `token` may be NULL (anonymous), `args` a JSON object (NULL: `{}`). Writes the
/// JSON result and returns its length (`cap` 0 sizes it); 0 on error (`mmk_last_error`).
///
/// # Safety
/// Strings NUL-terminated, `buf` writable for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_online_call(
    server: *const c_char,
    token: *const c_char,
    action: *const c_char,
    args: *const c_char,
    buf: *mut c_char,
    cap: usize,
) -> usize {
    guard(0, || {
        let (Some(server), Some(action)) = (str_arg(server), str_arg(action)) else {
            set_error("bad arguments");
            return 0;
        };
        let args: Value = str_arg(args)
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or_else(|| json!({}));
        match call(server, str_arg(token), action, &args) {
            Ok(v) => give(&v.to_string(), buf, cap),
            Err(e) => {
                set_error(format!("{action}: {e}"));
                0
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

    #[test]
    fn call_reports_bad_actions_and_unreachable_servers() {
        assert!(call("http://127.0.0.1:9", None, "nope", &json!({})).is_err());
        assert!(call("http://127.0.0.1:9", None, "me", &json!({})).is_err());
        let n = unsafe {
            mmk_online_call(
                c"http://127.0.0.1:9".as_ptr(),
                std::ptr::null(),
                c"rankings".as_ptr(),
                c"{\"kind\":\"records\"}".as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert_eq!(n, 0);
    }
}
