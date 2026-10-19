//! The course server inside an app: the recomp starts it in its own process (a static library, C
//! interface in recomp/runtime/net/embed_server.h) so a device needs no server of its own. It
//! serves on 127.0.0.1 only, from a folder in the app's data, with one local account; with an
//! upstream source (`tgrcode`) the Endless Challenge and 100 Mario courses come from there
//! ([`mmk_server::upstream`]).

use std::ffi::{CStr, c_char};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use mmk_server::upstream::{self, Keys, TgrCode};
use mmk_server::{ServeOptions, Store, limits::Limits, serve_background};

static STARTED: OnceLock<Option<(u16, String)>> = OnceLock::new();

/// Start the server (once per process; later calls return the first result).
///
/// - `dir`: its folder (created).
/// - `upstream`: `"tgrcode"` or null.
/// - `image`, `image_len`: the game's executable as loaded (read-only and data segments), where
///   the course key table is found with the first download; null when there is no upstream.
/// - `name`: the local account's name (made once, its token kept in `dir`).
/// - `token_out`, `token_cap`: the account's token, NUL-terminated.
///
/// Returns the port on 127.0.0.1, 0 on failure (the reason is on stderr).
///
/// # Safety
/// The strings are NUL-terminated; `image` points at `image_len` readable bytes (or is null);
/// `token_out` has room for `token_cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_embed_start(
    dir: *const c_char,
    upstream: *const c_char,
    image: *const u8,
    image_len: usize,
    name: *const c_char,
    token_out: *mut c_char,
    token_cap: usize,
) -> u16 {
    let s = |p: *const c_char| {
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    };
    let (Some(dir), Some(name)) = (s(dir), s(name)) else {
        return 0;
    };
    let upstream = s(upstream);
    let image = (!image.is_null() && image_len > 0)
        .then(|| unsafe { std::slice::from_raw_parts(image, image_len) });
    let started = STARTED.get_or_init(|| start(Path::new(&dir), upstream.as_deref(), image, &name));
    let Some((port, token)) = started else {
        return 0;
    };
    if !token_out.is_null() && token_cap > token.len() {
        unsafe {
            std::ptr::copy_nonoverlapping(token.as_ptr(), token_out.cast::<u8>(), token.len());
            *token_out.add(token.len()) = 0;
        }
    }
    *port
}

fn start(
    dir: &Path,
    upstream: Option<&str>,
    image: Option<&[u8]>,
    name: &str,
) -> Option<(u16, String)> {
    let store = match Store::open(dir) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("[embed-server] {}: {e:?}", dir.display());
            return None;
        }
    };
    let token = account(&store, dir, name)?;
    match upstream {
        Some("tgrcode") => match image {
            Some(img) => upstream::start(
                store.clone(),
                Box::new(TgrCode::new()),
                Keys::Find(vec![img.to_vec()]),
                Default::default(),
            ),
            None => eprintln!("[embed-server] no game image: no course key table, no upstream"),
        },
        Some(other) => eprintln!("[embed-server] unknown upstream {other}"),
        None => {}
    }
    // (one local client: the limits that protect a public server don't apply)
    let opts = ServeOptions {
        threads: 4,
        limits: Limits {
            writes_per_minute: 100_000,
            accounts_per_hour: 1000,
        },
        trust_proxy: false,
    };
    match serve_background(store, "127.0.0.1:0", opts) {
        Ok(port) => {
            eprintln!(
                "[embed-server] {} on http://127.0.0.1:{port}{}",
                dir.display(),
                if upstream.is_some() {
                    " (courses also from tgrcode.com/mm2)"
                } else {
                    ""
                }
            );
            Some((port, token))
        }
        Err(e) => {
            eprintln!("[embed-server] {e}");
            None
        }
    }
}

/// The local account: the token kept in `dir/local-account.token`, or a new account.
fn account(store: &Store, dir: &Path, name: &str) -> Option<String> {
    let file = dir.join("local-account.token");
    if let Ok(t) = std::fs::read_to_string(&file) {
        let t = t.trim().to_string();
        if store.account(&t).is_some() {
            return Some(t);
        }
    }
    match store.register(name) {
        Ok((_, token)) => {
            if let Err(e) = std::fs::write(&file, &token) {
                eprintln!("[embed-server] {}: {e}", file.display());
            }
            Some(token)
        }
        Err(e) => {
            eprintln!("[embed-server] account {name}: {e:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::io::{Read, Write};

    #[test]
    fn it_serves_its_own_account_on_loopback() {
        let dir = std::env::temp_dir().join(format!("mmk-embed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = CString::new(dir.to_str().unwrap()).unwrap();
        let name = CString::new("Player").unwrap();
        let mut tok = [0 as c_char; 128];
        let port = unsafe {
            mmk_embed_start(
                d.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                name.as_ptr(),
                tok.as_mut_ptr(),
                tok.len(),
            )
        };
        assert_ne!(port, 0);
        let token = unsafe { CStr::from_ptr(tok.as_ptr()) }
            .to_str()
            .unwrap()
            .to_string();
        assert!(!token.is_empty());
        // the same token next time (kept in the folder)
        assert_eq!(
            account(&Store::open(&dir).unwrap(), &dir, "Player").unwrap(),
            token
        );
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "GET /api/me HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.contains("\"Player\""), "{out}");
    }
}
