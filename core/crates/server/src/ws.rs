//! The online-play relay for browsers: a minimal WebSocket server (RFC 6455) whose connections
//! join the relay's rooms as stream members (`mmk_netplay::relay::Hub`), so web and native
//! players share rooms. Each binary message is one relay message (`MMKJ` / `MMKD`, answered with
//! `MMKW` / `MMKD`). It runs on its own TCP port (the HTTP port + 1 by default) so each connection
//! can read and write from separate threads.
//!
//! Supported: the opening handshake, unfragmented masked binary frames from the client, close.
//! Pings are ignored (browsers don't send them; a quiet member expires as in the relay). Pages
//! served over HTTPS need `wss://`: put a TLS proxy in front.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use mmk_netplay::relay::{EXPIRE, Hub, MAX_MESSAGE};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_HEADER: usize = 8192;

/// Accept WebSocket members for `hub` on `addr` (blocks; one thread per connection).
pub fn serve(hub: Arc<Hub>, listener: TcpListener) {
    for s in listener.incoming() {
        let Ok(s) = s else { continue };
        let hub = hub.clone();
        std::thread::spawn(move || {
            let _ = connection(hub, s);
        });
    }
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `Sec-WebSocket-Accept` answer to a key.
pub fn accept_key(key: &str) -> String {
    let d = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{key}{GUID}").as_bytes(),
    );
    base64(d.as_ref())
}

/// Read the request head; the client's key if it asks for a WebSocket.
fn handshake(s: &mut TcpStream) -> io::Result<Option<String>> {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > MAX_HEADER || s.read(&mut b)? == 0 {
            return Ok(None);
        }
        head.push(b[0]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.split("\r\n");
    if !lines.next().is_some_and(|l| l.starts_with("GET ")) {
        return Ok(None);
    }
    let mut key = None;
    let mut upgrade = false;
    for l in lines {
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "sec-websocket-key" => key = Some(v.trim().to_string()),
            "upgrade" => upgrade = v.trim().eq_ignore_ascii_case("websocket"),
            _ => {}
        }
    }
    Ok(key.filter(|_| upgrade))
}

/// Write one unmasked binary frame.
fn write_frame(w: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut f = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => f.push(n as u8),
        n if n < 65536 => {
            f.push(126);
            f.extend((n as u16).to_be_bytes());
        }
        n => {
            f.push(127);
            f.extend((n as u64).to_be_bytes());
        }
    }
    f.extend(payload);
    w.write_all(&f)
}

/// Read one client frame: (opcode, unmasked payload). Errors on oversized or unmasked frames.
fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let opcode = h[0] & 0x0F;
    if h[1] & 0x80 == 0 {
        return Err(bad("unmasked client frame"));
    }
    let len = match h[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            u64::from_be_bytes(b).min(u32::MAX as u64) as usize
        }
        n => n as usize,
    };
    if len > MAX_MESSAGE {
        return Err(bad("message too large"));
    }
    let mut mask = [0u8; 4];
    r.read_exact(&mut mask)?;
    let mut p = vec![0u8; len];
    r.read_exact(&mut p)?;
    for (i, x) in p.iter_mut().enumerate() {
        *x ^= mask[i % 4];
    }
    Ok((opcode, p))
}

fn connection(hub: Arc<Hub>, mut s: TcpStream) -> io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let Some(key) = handshake(&mut s)? else {
        let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return Ok(());
    };
    s.write_all(
        format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(&key)
        )
        .as_bytes(),
    )?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(EXPIRE))?;
    let (id, rx) = hub.attach();
    let mut w = s.try_clone()?;
    // writer: until the hub drops this member's channel (detach)
    let writer = std::thread::spawn(move || {
        for m in rx {
            if write_frame(&mut w, 2, &m).is_err() {
                break;
            }
        }
        let _ = write_frame(&mut w, 8, &[]);
    });
    loop {
        match read_frame(&mut s) {
            Ok((2, m)) => hub.from_stream(id, &m),
            Ok((8, _)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    hub.detach(id);
    let _ = writer.join();
    let _ = s.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_netplay::Packet;
    use mmk_netplay::relay::{data_bytes, join_bytes, parse_data, parse_welcome};
    use std::net::UdpSocket;

    #[test]
    fn rfc_example_key() {
        // RFC 6455 section 1.3
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert_eq!(base64(b"ab"), "YWI=");
    }

    fn client_frame(payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut f = vec![0x82, 0x80 | payload.len() as u8];
        f.extend(mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        f
    }

    fn server_frame(s: &mut TcpStream) -> Vec<u8> {
        let mut h = [0u8; 2];
        s.read_exact(&mut h).unwrap();
        let mut p = vec![0u8; (h[1] & 0x7F) as usize];
        s.read_exact(&mut p).unwrap();
        p
    }

    #[test]
    fn two_browsers_meet_in_a_room() {
        let hub = Hub::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let at = l.local_addr().unwrap();
        std::thread::spawn(move || serve(hub, l));
        let open = |slot: u8| {
            let mut s = TcpStream::connect(at).unwrap();
            s.write_all(
                b"GET /relay HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                s.read_exact(&mut b).unwrap();
                head.push(b[0]);
            }
            let head = String::from_utf8(head).unwrap();
            assert!(head.starts_with("HTTP/1.1 101"), "{head}");
            assert!(head.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
            s.write_all(&client_frame(&join_bytes("WEB", slot)))
                .unwrap();
            let w = parse_welcome(&server_frame(&mut s)).unwrap();
            assert_eq!(w.len(), slot as usize + 1);
            s
        };
        let mut a = open(0);
        let mut b = open(1);
        let p = Packet {
            player: 0,
            first_frame: 5,
            inputs: vec![1, 2, 3],
            acks: vec![],
            check: None,
        };
        a.write_all(&client_frame(&data_bytes(&p))).unwrap();
        assert_eq!(parse_data(&server_frame(&mut b)), Some(p));
        // not a websocket request
        let mut c = TcpStream::connect(at).unwrap();
        c.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let mut r = String::new();
        c.read_to_string(&mut r).unwrap();
        assert!(r.starts_with("HTTP/1.1 400"));
    }
}
