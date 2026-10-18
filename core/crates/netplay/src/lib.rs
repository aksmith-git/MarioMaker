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
/// Inputs repeated in every packet.
pub const REDUNDANCY: usize = 8;

/// Something rollback can drive: the world stepped with one input per player.
pub trait Game: Clone {
    fn advance(&mut self, inputs: &[Input]);
}

impl Game for World {
    fn advance(&mut self, inputs: &[Input]) {
        self.step_players(inputs);
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
        !old.confirmed && old.input != input
    }
}

pub struct Session<G: Game> {
    pub game: G,
    /// Frames simulated so far.
    pub frame: usize,
    pub local: usize,
    timelines: Vec<Timeline>,
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
    }

    /// The packet to send this frame: our last REDUNDANCY inputs.
    pub fn packet(&self) -> Packet {
        let end = self.frame;
        let start = end.saturating_sub(REDUNDANCY);
        Packet {
            player: self.local as u8,
            first_frame: start as u32,
            inputs: (start..end)
                .map(|n| self.timelines[self.local].get(n).input.buttons as u16)
                .collect(),
        }
    }

    pub fn receive(&mut self, p: &Packet) {
        for (k, &b) in p.inputs.iter().enumerate() {
            self.remote_input(
                p.player as usize,
                p.first_frame as usize + k,
                Input { buttons: b as u32 },
            );
        }
    }
}

/// Wire format: player (u8), first frame (u32 LE), count (u8), inputs (u16 LE each).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub player: u8,
    pub first_frame: u32,
    pub inputs: Vec<u16>,
}

impl Packet {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = vec![self.player];
        v.extend(self.first_frame.to_le_bytes());
        v.push(self.inputs.len() as u8);
        for i in &self.inputs {
            v.extend(i.to_le_bytes());
        }
        v
    }

    pub fn parse(b: &[u8]) -> Option<Self> {
        let (&player, rest) = b.split_first()?;
        let first_frame = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?);
        let n = *rest.get(4)? as usize;
        let data = rest.get(5..5 + 2 * n)?;
        Some(Self {
            player,
            first_frame,
            inputs: data
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
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
        };
        assert_eq!(Packet::parse(&p.to_bytes()), Some(p));
        assert_eq!(Packet::parse(&[1, 2]), None);
    }
}
