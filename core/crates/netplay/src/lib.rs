//! Rollback netcode (Phase 10): online play on the deterministic sim, GGPO-style.
//!
//! Every peer runs the whole game. Each frame a peer adds its local input and steps at once,
//! predicting the inputs it hasn't received yet (the player keeps doing what they last did).
//! When a remote input arrives that differs from the prediction, the session restores the
//! snapshot of that frame and re-simulates up to now with the corrected inputs. The sim is
//! deterministic and [`World`] is `Clone`, so all peers end in the same state.
//!
//! Transport: [`Packet`]s carrying a peer's last inputs (redundant, so one lost datagram does not
//! matter) over a [`Link`]: plain UDP to known addresses ([`UdpLink`]) or a room on the course
//! server's relay, which also punches through NAT ([`relay::RelayLink`]). Browsers need a WebRTC
//! data channel instead [⬜].

pub mod relay;

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};

use mmk_sim::{Input, World};

/// How far back a correction can reach (frames of snapshots kept).
pub const MAX_ROLLBACK: usize = 12;
/// Inputs repeated in every packet at least.
pub const REDUNDANCY: usize = 8;
/// Most inputs in one packet (unacknowledged inputs beyond this wait for later packets).
pub const MAX_RESEND: usize = 64;
/// How far a peer may run ahead of the inputs it has from everyone: it waits ([`Session::can_advance`])
/// rather than predict so far that a late input falls outside the rollback window. This also lines
/// up a start: whoever starts first waits for the others.
pub const MAX_LEAD: usize = MAX_ROLLBACK - 2;

/// Frames between desync checks (a hash of the state at a frame every peer has all inputs for).
pub const CHECK_EVERY: usize = 60;

/// Something rollback can drive: the world stepped with one input per player.
pub trait Game: Clone {
    fn advance(&mut self, inputs: &[Input]);
    /// A hash of the state, equal on every peer when the sims agree (0: no checking).
    fn checksum(&self) -> u64 {
        0
    }
}

impl Game for World {
    fn advance(&mut self, inputs: &[Input]) {
        self.step_players(inputs);
    }

    fn checksum(&self) -> u64 {
        World::checksum(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    input: Input,
    /// Received (true) or predicted (false).
    confirmed: bool,
}

/// One player's input timeline from the session start.
#[derive(Clone, Debug, Default)]
struct Timeline {
    slots: Vec<Slot>,
    /// Frames confirmed without a gap from the start.
    through: usize,
}

impl Timeline {
    fn get(&self, frame: usize) -> Slot {
        self.slots.get(frame).copied().unwrap_or_else(|| Slot {
            // predict: the last known input repeats
            input: self
                .slots
                .iter()
                .rev()
                .find(|s| s.confirmed)
                .map_or(Input::default(), |s| s.input),
            confirmed: false,
        })
    }

    fn set(&mut self, frame: usize, input: Input) -> bool {
        while self.slots.len() <= frame {
            let s = self.get(self.slots.len());
            self.slots.push(s);
        }
        let old = self.slots[frame];
        self.slots[frame] = Slot {
            input,
            confirmed: true,
        };
        // later predicted slots follow the new last-known input
        for s in self.slots[frame + 1..]
            .iter_mut()
            .take_while(|s| !s.confirmed)
        {
            s.input = input;
        }
        while self.slots.get(self.through).is_some_and(|s| s.confirmed) {
            self.through += 1;
        }
        !old.confirmed && old.input != input
    }
}

pub struct Session<G: Game> {
    pub game: G,
    /// Frames simulated so far.
    pub frame: usize,
    pub local: usize,
    timelines: Vec<Timeline>,
    /// Per player: how many of our inputs they have confirmed (from their packets' acks).
    acked: Vec<usize>,
    /// Our state hashes at check frames (recent), peers' hashes not compared yet, and the
    /// first check frame where a peer's hash differed from ours (the sims disagree).
    checks: VecDeque<(usize, u64)>,
    peer_checks: VecDeque<(usize, u64)>,
    /// Per player: the newest check frame taken from their packets (each counted once).
    peer_seen: Vec<usize>,
    next_check: usize,
    pub desync: Option<usize>,
    /// Check hashes compared with a peer's so far.
    pub checks_compared: u64,
    /// Snapshot before frame `n` (the last MAX_ROLLBACK frames).
    snapshots: VecDeque<(usize, G)>,
    /// Earliest frame whose inputs changed since it was simulated.
    dirty: Option<usize>,
    pub rollbacks: u64,
}

impl<G: Game> Session<G> {
    pub fn new(game: G, players: usize, local: usize) -> Self {
        Self {
            game,
            frame: 0,
            local,
            timelines: vec![Timeline::default(); players],
            acked: vec![0; players],
            checks: VecDeque::new(),
            peer_checks: VecDeque::new(),
            peer_seen: vec![0; players],
            next_check: CHECK_EVERY,
            desync: None,
            checks_compared: 0,
            snapshots: VecDeque::new(),
            dirty: None,
            rollbacks: 0,
        }
    }

    /// A remote player's input for a frame (from a packet). Older than the rollback window:
    /// ignored (it was predicted and that prediction stands — keep the window above the delay).
    pub fn remote_input(&mut self, player: usize, frame: usize, input: Input) {
        if player == self.local || player >= self.timelines.len() {
            return;
        }
        if frame + MAX_ROLLBACK < self.frame {
            return;
        }
        let mispredicted = self.timelines[player].set(frame, input);
        if mispredicted && frame < self.frame {
            self.dirty = Some(self.dirty.map_or(frame, |d| d.min(frame)));
        }
    }

    /// Whether stepping now keeps every unconfirmed remote input inside the rollback window. When
    /// it doesn't, the caller skips the step (still sending its packet) until inputs arrive.
    pub fn can_advance(&self) -> bool {
        self.timelines
            .iter()
            .enumerate()
            .all(|(i, t)| i == self.local || self.frame < t.through + MAX_LEAD)
    }

    pub fn players(&self) -> usize {
        self.timelines.len()
    }

    /// Frames ahead of the slowest peer's confirmed inputs.
    pub fn lead(&self) -> usize {
        self.timelines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.local)
            .map(|(_, t)| self.frame.saturating_sub(t.through))
            .max()
            .unwrap_or(0)
    }

    fn inputs(&self, frame: usize) -> Vec<Input> {
        self.timelines.iter().map(|t| t.get(frame).input).collect()
    }

    /// Add the local input for the next frame, fix any mispredictions, then step one frame.
    pub fn advance(&mut self, local_input: Input) {
        let f = self.frame;
        self.timelines[self.local].set(f, local_input);
        if let Some(d) = self.dirty.take()
            && let Some(pos) = self.snapshots.iter().position(|(n, _)| *n == d)
        {
            self.game = self.snapshots[pos].1.clone();
            self.snapshots.truncate(pos);
            self.rollbacks += 1;
            for n in d..f {
                self.snapshots.push_back((n, self.game.clone()));
                let inp = self.inputs(n);
                self.game.advance(&inp);
            }
        }
        self.snapshots.push_back((f, self.game.clone()));
        while self.snapshots.len() > MAX_ROLLBACK {
            self.snapshots.pop_front();
        }
        let inp = self.inputs(f);
        self.game.advance(&inp);
        self.frame += 1;
        self.take_checks();
    }

    /// Frames every peer's inputs are known for (the state before this frame is final).
    fn confirmed(&self) -> usize {
        self.timelines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.local)
            .map(|(_, t)| t.through)
            .min()
            .unwrap_or(self.frame)
            .min(self.frame)
    }

    /// Hash the states at check frames that have become final.
    fn take_checks(&mut self) {
        let c = self.confirmed();
        while self.next_check <= c {
            let k = self.next_check;
            self.next_check += CHECK_EVERY;
            let state = if k == self.frame {
                Some(&self.game)
            } else {
                self.snapshots.iter().find(|(n, _)| *n == k).map(|(_, g)| g)
            };
            let Some(h) = state.map(Game::checksum) else {
                continue; // fell out of the window (a caller ignored can_advance)
            };
            self.checks.push_back((k, h));
            if self.checks.len() > 16 {
                self.checks.pop_front();
            }
            let theirs: Vec<u64> = self
                .peer_checks
                .iter()
                .filter(|(f, _)| *f == k)
                .map(|&(_, h)| h)
                .collect();
            self.peer_checks.retain(|(f, _)| *f > k);
            for t in theirs {
                self.compare(k, h, t);
            }
        }
    }

    fn compare(&mut self, frame: usize, ours: u64, theirs: u64) {
        self.checks_compared += 1;
        if ours != theirs && self.desync.is_none() {
            self.desync = Some(frame);
        }
    }

    fn peer_check(&mut self, frame: usize, hash: u64) {
        if let Some(&(_, ours)) = self.checks.iter().find(|(f, _)| *f == frame) {
            self.compare(frame, ours, hash);
        } else if frame >= self.next_check {
            self.peer_checks.push_back((frame, hash));
            if self.peer_checks.len() > 64 {
                self.peer_checks.pop_front();
            }
        }
    }

    /// The packet to send this frame: our inputs from the oldest one some peer hasn't confirmed
    /// (at least the last REDUNDANCY, at most MAX_RESEND), and how far we have everyone's.
    pub fn packet(&self) -> Packet {
        let end = self.frame;
        let unacked = (0..self.acked.len())
            .filter(|&i| i != self.local)
            .map(|i| self.acked[i])
            .min()
            .unwrap_or(end);
        let start = unacked
            .min(end.saturating_sub(REDUNDANCY))
            .max(end.saturating_sub(MAX_RESEND));
        Packet {
            player: self.local as u8,
            first_frame: start as u32,
            inputs: (start..end)
                .map(|n| self.timelines[self.local].get(n).input.buttons as u16)
                .collect(),
            acks: self
                .timelines
                .iter()
                .enumerate()
                .map(|(i, t)| if i == self.local { end } else { t.through } as u32)
                .collect(),
            check: self.checks.back().map(|&(f, h)| (f as u32, h)),
        }
    }

    pub fn receive(&mut self, p: &Packet) {
        let from = p.player as usize;
        if let (Some(&a), Some(mine)) = (p.acks.get(self.local), self.acked.get_mut(from)) {
            *mine = (*mine).max(a as usize);
        }
        if let Some((f, h)) = p.check
            && from != self.local
            && let Some(seen) = self.peer_seen.get_mut(from)
            && f as usize > *seen
        {
            *seen = f as usize;
            self.peer_check(f as usize, h);
        }
        for (k, &b) in p.inputs.iter().enumerate() {
            self.remote_input(
                p.player as usize,
                p.first_frame as usize + k,
                Input { buttons: b as u32 },
            );
        }
    }
}

/// Wire format: player (u8), first frame (u32 LE), count (u8), inputs (u16 LE each), ack count
/// (u8) and acks (u32 LE each), then optionally a desync check: frame (u32 LE), hash (u64 LE).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packet {
    pub player: u8,
    pub first_frame: u32,
    pub inputs: Vec<u16>,
    /// Per player: how many of their inputs the sender has without a gap (its own: its frame).
    pub acks: Vec<u32>,
    /// The sender's latest desync check: (frame, state hash).
    pub check: Option<(u32, u64)>,
}

impl Packet {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = vec![self.player];
        v.extend(self.first_frame.to_le_bytes());
        v.push(self.inputs.len() as u8);
        for i in &self.inputs {
            v.extend(i.to_le_bytes());
        }
        v.push(self.acks.len() as u8);
        for a in &self.acks {
            v.extend(a.to_le_bytes());
        }
        if let Some((f, h)) = self.check {
            v.extend(f.to_le_bytes());
            v.extend(h.to_le_bytes());
        }
        v
    }

    pub fn parse(b: &[u8]) -> Option<Self> {
        let (&player, rest) = b.split_first()?;
        let first_frame = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?);
        let n = *rest.get(4)? as usize;
        let data = rest.get(5..5 + 2 * n)?;
        let (acks, tail): (Vec<u32>, &[u8]) = match rest[5 + 2 * n..].split_first() {
            Some((&k, a)) => (
                a.get(..4 * k as usize)?
                    .chunks(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
                &a[4 * k as usize..],
            ),
            None => (Vec::new(), &[]),
        };
        let check = (tail.len() >= 12).then(|| {
            (
                u32::from_le_bytes(tail[..4].try_into().unwrap()),
                u64::from_le_bytes(tail[4..12].try_into().unwrap()),
            )
        });
        Some(Self {
            player,
            first_frame,
            inputs: data
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
            acks,
            check,
        })
    }
}

/// Moves [`Packet`]s between the peers (non-blocking).
pub trait Link {
    fn send(&mut self, p: &Packet);
    /// Every packet waiting.
    fn poll(&mut self) -> Vec<Packet>;
}

/// A UDP link to the other peers (non-blocking).
pub struct UdpLink {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
}

impl UdpLink {
    pub fn bind(addr: &str, peers: Vec<SocketAddr>) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, peers })
    }
}

impl Link for UdpLink {
    fn send(&mut self, p: &Packet) {
        let b = p.to_bytes();
        for peer in &self.peers {
            let _ = self.socket.send_to(&b, peer);
        }
    }

    fn poll(&mut self) -> Vec<Packet> {
        let mut out = Vec::new();
        let mut buf = [0u8; 512];
        while let Ok((n, _)) = self.socket.recv_from(&mut buf) {
            if let Some(p) = Packet::parse(&buf[..n]) {
                out.push(p);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_sim::input;
    use mmk_sim::level::Level;

    fn script(player: usize, frame: usize) -> Input {
        let b = match (player, frame / 40 % 3) {
            (0, 0) | (1, 1) => input::RIGHT,
            (0, 1) => input::RIGHT | input::JUMP,
            (1, 0) => input::LEFT | input::JUMP,
            _ => input::RUN | input::RIGHT,
        };
        Input { buttons: b }
    }

    #[test]
    fn two_peers_with_delay_and_loss_end_identical() {
        let mut world = World::with_level(Level::demo());
        world.set_players(2);
        let mut a = Session::new(world.clone(), 2, 0);
        let mut b = Session::new(world.clone(), 2, 1);
        // in flight: (arrival frame, packet), 4 frames of delay, every 3rd packet lost
        let mut to_a: Vec<(usize, Packet)> = Vec::new();
        let mut to_b: Vec<(usize, Packet)> = Vec::new();
        for f in 0..300 {
            for (at, p) in std::mem::take(&mut to_a) {
                if at <= f {
                    a.receive(&p)
                } else {
                    to_a.push((at, p))
                }
            }
            for (at, p) in std::mem::take(&mut to_b) {
                if at <= f {
                    b.receive(&p)
                } else {
                    to_b.push((at, p))
                }
            }
            a.advance(script(0, f));
            b.advance(script(1, f));
            if f % 3 != 0 {
                to_b.push((f + 4, a.packet()));
                to_a.push((f + 4, b.packet()));
            }
        }
        // let the last inputs arrive (both idle meanwhile, as both would see)
        for f in 300..330 {
            for (_, p) in to_a.drain(..) {
                a.receive(&p);
            }
            for (_, p) in to_b.drain(..) {
                b.receive(&p);
            }
            a.advance(Input::default());
            b.advance(Input::default());
            to_b.push((f, a.packet()));
            to_a.push((f, b.packet()));
        }
        assert!(
            a.rollbacks > 0 && b.rollbacks > 0,
            "predictions were corrected"
        );
        assert_eq!((a.desync, b.desync), (None, None));
        assert!(a.checks_compared > 0 && b.checks_compared > 0);
        // replay with every input known: the reference result
        let mut reference = world;
        for f in 0..330 {
            let inp = if f < 300 {
                vec![script(0, f), script(1, f)]
            } else {
                vec![Input::default(); 2]
            };
            reference.step_players(&inp);
        }
        for s in [&a.game, &b.game] {
            assert_eq!(
                (s.player.x, s.player.y),
                (reference.player.x, reference.player.y)
            );
            assert_eq!(
                (s.others[0].x, s.others[0].y),
                (reference.others[0].x, reference.others[0].y)
            );
            assert_eq!(s.tick, reference.tick);
        }
    }

    #[test]
    fn a_late_peer_is_waited_for_and_both_agree() {
        let mut world = World::with_level(Level::demo());
        world.set_players(2);
        let mut a = Session::new(world.clone(), 2, 0);
        let mut b = Session::new(world.clone(), 2, 1);
        // a starts 200 ticks before b; each tick both exchange packets (2 ticks of delay)
        let mut to_a: Vec<(usize, Packet)> = Vec::new();
        let mut to_b: Vec<(usize, Packet)> = Vec::new();
        let (mut fa, mut fb) = (0, 0);
        for t in 0..800 {
            for (at, p) in std::mem::take(&mut to_a) {
                if at <= t {
                    a.receive(&p)
                } else {
                    to_a.push((at, p))
                }
            }
            for (at, p) in std::mem::take(&mut to_b) {
                // b isn't running yet: whatever reaches it before then is lost
                if at > t {
                    to_b.push((at, p))
                } else if t >= 200 {
                    b.receive(&p)
                }
            }
            if a.can_advance() && fa < 500 {
                a.advance(script(0, fa));
                fa += 1;
            }
            to_b.push((t + 2, a.packet()));
            if t >= 200 {
                if b.can_advance() && fb < 500 {
                    b.advance(script(1, fb));
                    fb += 1;
                }
                to_a.push((t + 2, b.packet()));
            }
            assert!(a.lead() <= MAX_LEAD && b.lead() <= MAX_LEAD);
        }
        assert_eq!((fa, fb), (500, 500));
        let mut reference = world;
        for f in 0..500 {
            reference.step_players(&[script(0, f), script(1, f)]);
        }
        for s in [&a.game, &b.game] {
            assert_eq!(
                (s.player.x, s.player.y),
                (reference.player.x, reference.player.y)
            );
            assert_eq!(
                (s.others[0].x, s.others[0].y),
                (reference.others[0].x, reference.others[0].y)
            );
        }
    }

    /// A toy game whose peer 1 drifts at frame 200 (a determinism bug).
    #[derive(Clone)]
    struct Toy {
        v: i64,
        frame: usize,
        buggy: bool,
    }

    impl Game for Toy {
        fn advance(&mut self, inputs: &[Input]) {
            self.v += inputs.iter().map(|i| i.buttons as i64).sum::<i64>();
            self.frame += 1;
            if self.buggy && self.frame == 200 {
                self.v += 1;
            }
        }
        fn checksum(&self) -> u64 {
            self.v as u64
        }
    }

    #[test]
    fn a_drifting_peer_is_caught_by_the_checks() {
        let toy = |buggy| Toy {
            v: 0,
            frame: 0,
            buggy,
        };
        let mut a = Session::new(toy(false), 2, 0);
        let mut b = Session::new(toy(true), 2, 1);
        for f in 0..400 {
            let (pa, pb) = (a.packet(), b.packet());
            a.receive(&pb);
            b.receive(&pa);
            a.advance(Input {
                buttons: (f % 7) as u32,
            });
            b.advance(Input {
                buttons: (f % 5) as u32,
            });
        }
        // the first check after the drift (frame 240) differs; earlier ones agreed
        assert_eq!(a.desync, Some(240));
        assert_eq!(b.desync, Some(240));
        // one comparison per check frame (packets repeat the latest check)
        assert!(
            (4..=6).contains(&a.checks_compared),
            "{}",
            a.checks_compared
        );
    }

    #[test]
    fn udp_links_exchange_packets() {
        let pa = 40000 + (std::process::id() % 10000) as u16;
        let pb = pa + 1;
        let mut a = UdpLink::bind(
            &format!("127.0.0.1:{pa}"),
            vec![format!("127.0.0.1:{pb}").parse().unwrap()],
        )
        .unwrap();
        let mut b = UdpLink::bind(
            &format!("127.0.0.1:{pb}"),
            vec![format!("127.0.0.1:{pa}").parse().unwrap()],
        )
        .unwrap();
        let p = Packet {
            player: 0,
            first_frame: 7,
            inputs: vec![16, 0],
            acks: vec![],
            check: None,
        };
        a.send(&p);
        let mut got = Vec::new();
        for _ in 0..50 {
            got.extend(b.poll());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, vec![p]);
    }

    #[test]
    fn packets_round_trip() {
        let p = Packet {
            player: 2,
            first_frame: 1234,
            inputs: vec![1, 16, 0x30],
            acks: vec![7, 0, 1234567],
            check: Some((120, 0xDEAD_BEEF_0123_4567)),
        };
        assert_eq!(Packet::parse(&p.to_bytes()), Some(p));
        assert_eq!(Packet::parse(&[1, 2]), None);
    }
}
