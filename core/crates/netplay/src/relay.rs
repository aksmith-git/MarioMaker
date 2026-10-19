//! Rooms through a relay, for players behind NAT (Phase 10).
//!
//! The course server runs a relay ([`Hub`]) on its UDP port. A peer joins a room by name with its
//! player slot; the relay answers with every member's public address. Peers then probe each other
//! directly (UDP hole punching) and, once a peer has been heard from directly, talk to it
//! directly. Until every peer is direct, packets also go through the relay, which forwards them to
//! the rest of the room. Getting a packet twice is harmless: input packets are idempotent.
//!
//! Browsers can't send UDP: they join the same rooms over a WebSocket (the server's `ws` module),
//! as stream members ([`Peer::Socket`]) the relay always forwards to, so web and native players
//! share rooms.
//!
//! Messages start with a 4-byte tag:
//! - `MMKJ` join / keepalive: slot (u8), room name length (u8), name (UTF-8, ≤ 16 bytes)
//! - `MMKW` welcome: count (u8), then per member slot (u8), address length (u8), "ip:port"
//!   (empty for a stream member: it can't be reached directly)
//! - `MMKD` data: an input [`Packet`]; to the relay it is forwarded to the sender's room
//! - `MMKP` probe: a direct hello between peers
//!
//! Members who go quiet for [`EXPIRE`] leave the room.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{Link, Packet};

pub const MAX_ROOM: usize = 4;
pub const MAX_ROOM_NAME: usize = 16;
/// Rooms a relay keeps at once (bounds memory).
pub const MAX_ROOMS: usize = 4096;
pub const EXPIRE: Duration = Duration::from_secs(30);
/// Sends between keepalives / probes (one per frame: about every half second).
pub const KEEPALIVE_SENDS: u32 = 30;
/// Largest relayed message: one UDP datagram below a 1500-byte MTU (the recomp's virtual LAN carries a game's own
/// packets, up to 1400 bytes plus its 19-byte header).
pub const MAX_MESSAGE: usize = 1450;

const JOIN: &[u8; 4] = b"MMKJ";
const WELCOME: &[u8; 4] = b"MMKW";
const DATA: &[u8; 4] = b"MMKD";
const PROBE: &[u8; 4] = b"MMKP";

/// Join / keepalive message for `room` as player `slot`.
pub fn join_bytes(room: &str, slot: u8) -> Vec<u8> {
    let mut v = JOIN.to_vec();
    v.push(slot);
    v.push(room.len() as u8);
    v.extend(room.as_bytes());
    v
}

fn parse_join(b: &[u8]) -> Option<(String, u8)> {
    let slot = *b.get(4)?;
    let n = *b.get(5)? as usize;
    if n == 0 || n > MAX_ROOM_NAME || slot as usize >= MAX_ROOM {
        return None;
    }
    let name = std::str::from_utf8(b.get(6..6 + n)?).ok()?;
    Some((name.to_string(), slot))
}

fn welcome_bytes(members: &[(u8, Peer)]) -> Vec<u8> {
    let mut v = WELCOME.to_vec();
    v.push(members.len() as u8);
    for (slot, p) in members {
        let s = match p {
            Peer::Udp(a) => a.to_string(),
            Peer::Socket(_) => String::new(),
        };
        v.push(*slot);
        v.push(s.len() as u8);
        v.extend(s.as_bytes());
    }
    v
}

/// The members of a welcome: (slot, public UDP address if reachable directly).
pub fn parse_welcome(b: &[u8]) -> Option<Vec<(u8, Option<SocketAddr>)>> {
    if b.get(..4)? != WELCOME {
        return None;
    }
    let n = *b.get(4)? as usize;
    let mut out = Vec::with_capacity(n);
    let mut at = 5;
    for _ in 0..n {
        let slot = *b.get(at)?;
        let len = *b.get(at + 1)? as usize;
        let s = std::str::from_utf8(b.get(at + 2..at + 2 + len)?).ok()?;
        out.push((slot, s.parse().ok()));
        at += 2 + len;
    }
    Some(out)
}

/// Data message carrying `p`.
pub fn data_bytes(p: &Packet) -> Vec<u8> {
    let mut b = DATA.to_vec();
    b.extend(p.to_bytes());
    b
}

/// The packet in a data message.
pub fn parse_data(b: &[u8]) -> Option<Packet> {
    if b.get(..4)? != DATA {
        return None;
    }
    Packet::parse(&b[4..])
}

/// A relay member: a UDP address, or a stream (WebSocket) connection by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Peer {
    Udp(SocketAddr),
    Socket(u64),
}

struct Member {
    slot: u8,
    addr: Peer,
    seen: Instant,
}

/// The relay's state: rooms by name. [`Relay::handle`] is pure (message in, messages out) so
/// it can be tested without sockets; [`Hub`] drives it.
#[derive(Default)]
pub struct Relay {
    rooms: HashMap<String, Vec<Member>>,
    room_of: HashMap<Peer, String>,
}

impl Relay {
    /// One message from `from` at `now`: the messages to send.
    pub fn handle(&mut self, b: &[u8], from: Peer, now: Instant) -> Vec<(Vec<u8>, Peer)> {
        if b.len() > MAX_MESSAGE || b.len() < 4 {
            return Vec::new();
        }
        match &b[..4] {
            t if t == JOIN => {
                let Some((name, slot)) = parse_join(b) else {
                    return Vec::new();
                };
                if !self.rooms.contains_key(&name) && self.rooms.len() >= MAX_ROOMS {
                    return Vec::new();
                }
                // moving rooms leaves the old one
                if let Some(old) = self.room_of.get(&from).cloned()
                    && old != name
                {
                    self.leave(&old, |m| m.addr == from);
                }
                let room = self.rooms.entry(name.clone()).or_default();
                // a slot belongs to its latest joiner (a peer whose NAT mapping changed)
                let stale: Vec<Peer> = room
                    .iter()
                    .filter(|m| m.slot == slot && m.addr != from)
                    .map(|m| m.addr)
                    .collect();
                room.retain(|m| m.slot != slot || m.addr == from);
                for a in stale {
                    self.room_of.remove(&a);
                }
                let room = self.rooms.get_mut(&name).unwrap();
                if let Some(m) = room.iter_mut().find(|m| m.addr == from) {
                    m.slot = slot;
                    m.seen = now;
                } else if room.len() < MAX_ROOM {
                    room.push(Member {
                        slot,
                        addr: from,
                        seen: now,
                    });
                } else {
                    return Vec::new();
                }
                self.room_of.insert(from, name.clone());
                let members: Vec<(u8, Peer)> = room.iter().map(|m| (m.slot, m.addr)).collect();
                vec![(welcome_bytes(&members), from)]
            }
            t if t == DATA => {
                let Some(name) = self.room_of.get(&from) else {
                    return Vec::new();
                };
                let Some(room) = self.rooms.get_mut(name) else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for m in room.iter_mut() {
                    if m.addr == from {
                        m.seen = now;
                    } else {
                        out.push((b.to_vec(), m.addr));
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn leave(&mut self, name: &str, gone: impl Fn(&Member) -> bool) {
        if let Some(room) = self.rooms.get_mut(name) {
            for m in room.iter().filter(|m| gone(m)) {
                self.room_of.remove(&m.addr);
            }
            room.retain(|m| !gone(m));
            if room.is_empty() {
                self.rooms.remove(name);
            }
        }
    }

    /// `peer` leaves its room (a closed connection).
    pub fn remove(&mut self, peer: Peer) {
        if let Some(name) = self.room_of.get(&peer).cloned() {
            self.leave(&name, |m| m.addr == peer);
        }
    }

    /// Drop members quiet for longer than [`EXPIRE`].
    pub fn expire(&mut self, now: Instant) {
        let names: Vec<String> = self.rooms.keys().cloned().collect();
        for n in names {
            self.leave(&n, |m| now.duration_since(m.seen) > EXPIRE);
        }
    }

    pub fn rooms(&self) -> usize {
        self.rooms.len()
    }
}

/// A running relay: the [`Relay`] shared by its UDP socket and its stream members, each stream
/// fed through a channel (its connection's writer thread drains it).
pub struct Hub {
    relay: Mutex<Relay>,
    udp: UdpSocket,
    streams: Mutex<HashMap<u64, Sender<Vec<u8>>>>,
    next: AtomicU64,
}

impl Hub {
    pub fn new(udp: UdpSocket) -> Arc<Self> {
        Arc::new(Self {
            relay: Mutex::new(Relay::default()),
            udp,
            streams: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        })
    }

    fn deliver(&self, out: Vec<(Vec<u8>, Peer)>) {
        for (b, to) in out {
            match to {
                Peer::Udp(a) => {
                    let _ = self.udp.send_to(&b, a);
                }
                Peer::Socket(id) => {
                    if let Some(tx) = self.streams.lock().unwrap().get(&id) {
                        let _ = tx.send(b);
                    }
                }
            }
        }
    }

    /// Serve the UDP side until the socket fails (also expires quiet members).
    pub fn run_udp(&self) -> std::io::Result<()> {
        self.udp.set_read_timeout(Some(Duration::from_secs(1)))?;
        let mut buf = [0u8; MAX_MESSAGE + 1];
        let mut last_expire = Instant::now();
        loop {
            match self.udp.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let out = self.relay.lock().unwrap().handle(
                        &buf[..n],
                        Peer::Udp(from),
                        Instant::now(),
                    );
                    self.deliver(out);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::ConnectionReset
                    ) => {}
                Err(e) => return Err(e),
            }
            if last_expire.elapsed() > Duration::from_secs(1) {
                self.relay.lock().unwrap().expire(Instant::now());
                last_expire = Instant::now();
            }
        }
    }

    /// A new stream member: its id and the messages to write to it.
    pub fn attach(&self) -> (u64, Receiver<Vec<u8>>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.streams.lock().unwrap().insert(id, tx);
        (id, rx)
    }

    /// A message read from stream member `id`.
    pub fn from_stream(&self, id: u64, b: &[u8]) {
        let out = self
            .relay
            .lock()
            .unwrap()
            .handle(b, Peer::Socket(id), Instant::now());
        self.deliver(out);
    }

    /// Stream member `id` disconnected.
    pub fn detach(&self, id: u64) {
        self.relay.lock().unwrap().remove(Peer::Socket(id));
        self.streams.lock().unwrap().remove(&id);
    }

    pub fn rooms(&self) -> usize {
        self.relay.lock().unwrap().rooms()
    }
}

/// A peer in a relay room: direct to peers that answered a probe, through the relay otherwise.
pub struct RelayLink {
    socket: UdpSocket,
    relay: SocketAddr,
    room: String,
    slot: u8,
    /// Other members' public addresses (from the last welcome; None for stream members) and
    /// whether each was heard from directly.
    peers: Vec<(Option<SocketAddr>, bool)>,
    sends: u32,
}

impl RelayLink {
    /// Join `room` as player `slot` through the relay at `relay`.
    pub fn connect(relay: SocketAddr, room: &str, slot: u8) -> std::io::Result<Self> {
        let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, m.to_string());
        if room.is_empty() || room.len() > MAX_ROOM_NAME {
            return Err(bad("room name: 1–16 bytes"));
        }
        if slot as usize >= MAX_ROOM {
            return Err(bad("player slot: 0–3"));
        }
        let any = if relay.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(any)?;
        socket.set_nonblocking(true)?;
        let link = Self {
            socket,
            relay,
            room: room.to_string(),
            slot,
            peers: Vec::new(),
            sends: 0,
        };
        link.keepalive();
        Ok(link)
    }

    fn keepalive(&self) {
        let _ = self
            .socket
            .send_to(&join_bytes(&self.room, self.slot), self.relay);
        for (a, direct) in &self.peers {
            if let (Some(a), false) = (a, direct) {
                let _ = self.socket.send_to(PROBE, a);
            }
        }
    }

    /// Other members known, and how many are reached directly.
    pub fn status(&self) -> (usize, usize) {
        (self.peers.len(), self.peers.iter().filter(|p| p.1).count())
    }
}

impl Link for RelayLink {
    fn send(&mut self, p: &Packet) {
        if self.sends.is_multiple_of(KEEPALIVE_SENDS) {
            self.keepalive();
        }
        self.sends = self.sends.wrapping_add(1);
        let b = data_bytes(p);
        for (a, direct) in &self.peers {
            if let (Some(a), true) = (a, direct) {
                let _ = self.socket.send_to(&b, a);
            }
        }
        if self.peers.is_empty() || self.peers.iter().any(|p| !p.1) {
            let _ = self.socket.send_to(&b, self.relay);
        }
    }

    fn poll(&mut self) -> Vec<Packet> {
        let mut out = Vec::new();
        let mut buf = [0u8; MAX_MESSAGE + 1];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let b = &buf[..n];
            if n < 4 {
                continue;
            }
            if from == self.relay {
                if &b[..4] == WELCOME {
                    if let Some(members) = parse_welcome(b) {
                        let me = self.slot;
                        let old = std::mem::take(&mut self.peers);
                        self.peers = members
                            .into_iter()
                            .filter(|(s, _)| *s != me)
                            .map(|(_, a)| (a, old.iter().any(|(o, d)| *o == a && *d)))
                            .collect();
                    }
                    continue;
                }
            } else if let Some(p) = self.peers.iter_mut().find(|p| p.0 == Some(from)) {
                // heard directly: the hole is open
                p.1 = true;
            } else {
                continue;
            }
            if let Some(p) = parse_data(b) {
                out.push(p);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Peer {
        Peer::Udp(s.parse().unwrap())
    }

    fn sock(p: Peer) -> Option<SocketAddr> {
        match p {
            Peer::Udp(a) => Some(a),
            Peer::Socket(_) => None,
        }
    }

    #[test]
    fn rooms_forward_to_the_other_members() {
        let mut r = Relay::default();
        let t = Instant::now();
        let (a, b, c) = (addr("1.1.1.1:5"), addr("2.2.2.2:6"), addr("3.3.3.3:7"));
        let w = r.handle(&join_bytes("ROOM1", 0), a, t);
        assert_eq!(parse_welcome(&w[0].0).unwrap(), vec![(0, sock(a))]);
        r.handle(&join_bytes("ROOM1", 1), b, t);
        r.handle(&join_bytes("OTHER", 0), c, t);
        let w = r.handle(&join_bytes("ROOM1", 0), a, t);
        assert_eq!(
            parse_welcome(&w[0].0).unwrap(),
            vec![(0, sock(a)), (1, sock(b))]
        );
        let mut d = DATA.to_vec();
        d.extend([0, 0, 0, 0, 0, 0]);
        let out = r.handle(&d, a, t);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, b, "only the room's other member");
        assert!(
            r.handle(&d, addr("9.9.9.9:1"), t).is_empty(),
            "strangers aren't forwarded"
        );
        // c moves into ROOM1, slot 1 replaces b
        r.handle(&join_bytes("ROOM1", 1), c, t);
        assert_eq!(r.rooms(), 1);
        assert_eq!(r.handle(&d, a, t)[0].1, c);
        assert!(r.handle(&d, b, t).is_empty(), "b lost its slot");
        // quiet members expire
        r.expire(t + EXPIRE + Duration::from_secs(1));
        assert_eq!(r.rooms(), 0);
    }

    #[test]
    fn bad_joins_are_ignored() {
        let mut r = Relay::default();
        let t = Instant::now();
        let a = addr("1.1.1.1:5");
        assert!(r.handle(&join_bytes("R", 4), a, t).is_empty(), "slot 4");
        assert!(r.handle(&join_bytes("", 0), a, t).is_empty());
        assert!(r.handle(b"MMKJ", a, t).is_empty());
        for i in 0..4 {
            r.handle(
                &join_bytes("FULL", i),
                addr(&format!("1.1.1.{}:1", i + 1)),
                t,
            );
        }
        // a fifth address can only take an existing slot
        assert_eq!(
            r.handle(&join_bytes("FULL", 2), addr("5.5.5.5:1"), t).len(),
            1
        );
    }

    #[test]
    fn peers_meet_through_a_live_relay() {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let at = sock.local_addr().unwrap();
        let hub = Hub::new(sock);
        std::thread::spawn(move || hub.run_udp());
        let mut a = RelayLink::connect(at, "TEST", 0).unwrap();
        let mut b = RelayLink::connect(at, "TEST", 1).unwrap();
        let p = Packet {
            player: 0,
            first_frame: 3,
            inputs: vec![1, 2],
            acks: vec![],
            check: None,
        };
        let mut got = Vec::new();
        for _ in 0..400 {
            a.send(&p);
            b.send(&Packet {
                player: 1,
                ..p.clone()
            });
            a.poll();
            got.extend(b.poll());
            if !got.is_empty() && a.status() == (1, 1) && b.status() == (1, 1) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(got.contains(&p));
        // on loopback the probes get through, so both ends end up direct
        assert_eq!((a.status(), b.status()), ((1, 1), (1, 1)));
    }

    #[test]
    fn four_players_in_a_relay_room_end_identical() {
        use crate::Session;
        use mmk_sim::level::Level;
        use mmk_sim::{Input, World, input};
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let at = sock.local_addr().unwrap();
        let hub = Hub::new(sock);
        std::thread::spawn(move || hub.run_udp());
        let mut links: Vec<RelayLink> = (0..4)
            .map(|i| RelayLink::connect(at, "FOUR", i).unwrap())
            .collect();
        // wait until everyone has met (empty packets drive the keepalives)
        for _ in 0..2000 {
            for (i, l) in links.iter_mut().enumerate() {
                l.send(&Packet {
                    player: i as u8,
                    first_frame: 0,
                    inputs: vec![],
                    acks: vec![],
                    check: None,
                });
                l.poll();
            }
            if links.iter().all(|l| l.status().0 == 3) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(links.iter().all(|l| l.status().0 == 3), "room formed");
        let script = |p: usize, f: usize| Input {
            buttons: match (p + f / 30) % 4 {
                0 => input::RIGHT,
                1 => input::RIGHT | input::JUMP,
                2 => input::LEFT,
                _ => input::RUN | input::RIGHT,
            },
        };
        let mut world = World::with_level(Level::demo());
        world.set_players(4);
        let mut sessions: Vec<Session<World>> =
            (0..4).map(|i| Session::new(world.clone(), 4, i)).collect();
        for f in 0..200 {
            for (s, l) in sessions.iter_mut().zip(links.iter_mut()) {
                for p in l.poll() {
                    s.receive(&p);
                }
                let i = if f < 160 {
                    script(s.local, f)
                } else {
                    Input::default()
                };
                s.advance(i);
                l.send(&s.packet());
            }
            std::thread::sleep(Duration::from_micros(300));
        }
        let mut reference = world;
        for f in 0..200 {
            let i: Vec<Input> = (0..4)
                .map(|p| {
                    if f < 160 {
                        script(p, f)
                    } else {
                        Input::default()
                    }
                })
                .collect();
            reference.step_players(&i);
        }
        for s in &sessions {
            assert_eq!(
                (s.game.player.x, s.game.player.y),
                (reference.player.x, reference.player.y)
            );
            for k in 0..3 {
                assert_eq!(
                    (s.game.others[k].x, s.game.others[k].y),
                    (reference.others[k].x, reference.others[k].y)
                );
            }
        }
    }

    #[test]
    fn stream_and_udp_members_share_a_room() {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let at = sock.local_addr().unwrap();
        let hub = Hub::new(sock);
        let h = hub.clone();
        std::thread::spawn(move || h.run_udp());
        let (id, rx) = hub.attach();
        hub.from_stream(id, &join_bytes("MIX", 1));
        let welcome = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(parse_welcome(&welcome).unwrap(), vec![(1, None)]);
        let mut udp = RelayLink::connect(at, "MIX", 0).unwrap();
        let p = Packet {
            player: 0,
            first_frame: 9,
            inputs: vec![4],
            acks: vec![],
            check: None,
        };
        let mut got = None;
        for _ in 0..500 {
            udp.send(&p);
            udp.poll();
            while let Ok(m) = rx.try_recv() {
                if let Some(q) = parse_data(&m) {
                    got = Some(q);
                }
            }
            if got.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(got, Some(p.clone()), "UDP → stream");
        let q = Packet { player: 1, ..p };
        let mut back = Vec::new();
        for _ in 0..500 {
            hub.from_stream(id, &data_bytes(&q));
            back.extend(udp.poll());
            if !back.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(back.contains(&q), "stream → UDP");
        assert_eq!(udp.status(), (1, 0), "a stream member is never direct");
        hub.detach(id);
        hub.from_stream(id, &data_bytes(&q));
        assert_eq!(hub.rooms(), 1, "the UDP peer is still there");
    }
}
