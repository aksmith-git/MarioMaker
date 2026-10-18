//! Tracks (59-track.md): pieces from the course's track records joined into a graph, and the rail
//! follower that carries an object along them at 0.75 px/frame (1.5 with wings).
//!
//! Geometry, worked out from the course corpus [inf]: a record at (x, y) is a piece centred on
//! object tile (x + 1, y + 1), i.e. px (16x + 24, 16y + 24); an on-track object's reference point
//! (its tile centre) lies exactly on the piece it links to. Each piece joins two points one tile
//! from its centre: type 0 horizontal, 1 vertical, 2 diagonal down to the right, 3 diagonal up to
//! the right; types 4–7 are bends between the same corner points as the diagonals: an 8-px
//! straight lead, a quarter circle of radius 24 px (the rail follower's arc radius,
//! 0x7100340720) [obs], and another 8-px lead (which way each bulges is a guess [inf]). Pieces sharing an end point are joined.
//! Junctions (types 8–15) are rare and not built yet. A rider reaching a free end turns back at a
//! closed end (end code 0x70–0x77) and leaves the track at an open one (0x80–0x87, the closed
//! cap + 0x10) [obs: reverse vs FallDown in the follower 0x7100340720; codes per corpus and the
//! editor]. The record's k0 belongs to the end at centre + (1, 0) / (0, 1) / (−1, 1) / (1, 1)
//! for types 0–3, k1 to the other end.

use std::collections::HashMap;

use crate::fmath::DetMath;
use crate::level::TILE;

/// Normal and winged rider speeds, px/frame.
pub const SPEED: f32 = 0.75;
pub const SPEED_WINGED: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Line {
        a: (f32, f32),
        b: (f32, f32),
    },
    /// A bend: a straight 8-px lead from `e0`, an arc about `c` of radius `r` from angle `a0` to
    /// `a1` (radians), and a straight 8-px lead to `e1`.
    Arc {
        c: (f32, f32),
        r: f32,
        a0: f32,
        a1: f32,
        e0: (f32, f32),
        e1: (f32, f32),
    },
}

/// Straight lead at each end of a bend (px): the follower's arcs have radius 24 px
/// (0x7100340720, const 0x7101c8a740) and join the tile-centre track lines half a tile in [obs].
const BEND_LEAD: f32 = 8.0;
const BEND_RADIUS: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    seg: Seg,
    len: f32,
    /// End points in half-tile units (graph keys).
    ends: [(i32, i32); 2],
}

impl Piece {
    fn point(&self, s: f32) -> (f32, f32) {
        let t = (s / self.len).clamp(0.0, 1.0);
        match self.seg {
            Seg::Line { a, b } => (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t),
            Seg::Arc {
                c,
                r,
                a0,
                a1,
                e0,
                e1,
            } => {
                let arc = self.len - 2.0 * BEND_LEAD;
                let at = |a: f32| (c.0 + r * a.dcos(), c.1 + r * a.dsin());
                let s = s.clamp(0.0, self.len);
                if s < BEND_LEAD {
                    let (p, k) = (at(a0), s / BEND_LEAD);
                    (e0.0 + (p.0 - e0.0) * k, e0.1 + (p.1 - e0.1) * k)
                } else if s > BEND_LEAD + arc {
                    let (p, k) = (at(a1), (s - BEND_LEAD - arc) / BEND_LEAD);
                    (p.0 + (e1.0 - p.0) * k, p.1 + (e1.1 - p.1) * k)
                } else {
                    at(a0 + (a1 - a0) * ((s - BEND_LEAD) / arc))
                }
            }
        }
    }

    /// Unit direction of travel toward end 1 at distance s.
    fn tangent(&self, s: f32) -> (f32, f32) {
        match self.seg {
            Seg::Line { a, b } => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l = (dx * dx + dy * dy).sqrt();
                (dx / l, dy / l)
            }
            Seg::Arc { a0, a1, .. } => {
                let arc = self.len - 2.0 * BEND_LEAD;
                let k = ((s - BEND_LEAD) / arc).clamp(0.0, 1.0);
                let a = a0 + (a1 - a0) * k;
                let sign = (a1 - a0).signum();
                (-a.dsin() * sign, a.dcos() * sign)
            }
        }
    }
}

/// The track graph of one area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    pieces: Vec<Piece>,
    nodes: HashMap<(i32, i32), Vec<(usize, usize)>>,
    /// Free ends whose code says riders fall off there.
    open: std::collections::HashSet<(i32, i32)>,
}

/// An end code (low 12 bits of k0 / k1) that marks an open end.
pub fn is_open_end(code: u16) -> bool {
    (0x80..0x88).contains(&(code & 0xFFF))
}

fn key(p: (f32, f32)) -> (i32, i32) {
    ((p.0 / 8.0).round() as i32, (p.1 / 8.0).round() as i32)
}

impl Tracks {
    /// Build from track records: (x, y, type) with 255 meaning −1 (all ends closed).
    pub fn new(records: impl IntoIterator<Item = (u8, u8, u8)>) -> Self {
        Self::with_ends(records.into_iter().map(|(x, y, ty)| (x, y, ty, 0, 0)))
    }

    /// Build from track records with their end codes: (x, y, type, k0, k1).
    pub fn with_ends(records: impl IntoIterator<Item = (u8, u8, u8, u16, u16)>) -> Self {
        let mut t = Tracks::default();
        for (x, y, ty, k0, k1) in records {
            let x = if x == 255 { -1 } else { x as i32 };
            let y = if y == 255 { -1 } else { y as i32 };
            let c = ((x + 1) as f32 * TILE + 8.0, (y + 1) as f32 * TILE + 8.0);
            let at = |dx: f32, dy: f32| (c.0 + dx * TILE, c.1 + dy * TILE);
            // which end each code belongs to (types 0–3)
            let k0_end = match ty {
                0 => Some((1.0, 0.0)),
                1 => Some((0.0, 1.0)),
                2 => Some((-1.0, 1.0)),
                3 => Some((1.0, 1.0)),
                _ => None,
            };
            if let Some((dx, dy)) = k0_end {
                if is_open_end(k0) {
                    t.open.insert(key(at(dx, dy)));
                }
                if is_open_end(k1) {
                    t.open.insert(key(at(-dx, -dy)));
                }
            }
            let seg = match ty {
                0 => Seg::Line {
                    a: at(-1.0, 0.0),
                    b: at(1.0, 0.0),
                },
                1 => Seg::Line {
                    a: at(0.0, -1.0),
                    b: at(0.0, 1.0),
                },
                2 => Seg::Line {
                    a: at(-1.0, 1.0),
                    b: at(1.0, -1.0),
                },
                3 => Seg::Line {
                    a: at(-1.0, -1.0),
                    b: at(1.0, 1.0),
                },
                4..=7 => {
                    use std::f32::consts::{FRAC_PI_2, PI};
                    // (centre corner, start angle, end angle)
                    let (cc, a0, a1) = match ty {
                        4 => ((-1.0, -1.0), FRAC_PI_2, 0.0), // (−1,1) → (1,−1), bulging up-right
                        5 => ((1.0, 1.0), PI, 3.0 * FRAC_PI_2), // (−1,1) → (1,−1), bulging down-left
                        6 => ((1.0, -1.0), PI, FRAC_PI_2),      // (−1,−1) → (1,1), bulging up-left
                        _ => ((-1.0, 1.0), 3.0 * FRAC_PI_2, 2.0 * PI), // bulging down-right
                    };
                    // the ends are the tile centres one tile out (as for the diagonals); the
                    // 24-px arc's centre sits half a tile in from the corner tile's centre
                    let o = at(cc.0, cc.1);
                    let dir = |a: f32| (a.dcos(), a.dsin());
                    let (d0, d1) = (dir(a0), dir(a1));
                    let r_out = 2.0 * TILE;
                    Seg::Arc {
                        c: (
                            o.0 + BEND_LEAD * (d0.0 + d1.0),
                            o.1 + BEND_LEAD * (d0.1 + d1.1),
                        ),
                        r: BEND_RADIUS,
                        a0,
                        a1,
                        e0: (o.0 + r_out * d0.0, o.1 + r_out * d0.1),
                        e1: (o.0 + r_out * d1.0, o.1 + r_out * d1.1),
                    }
                }
                _ => continue,
            };
            let len = match seg {
                Seg::Line { a, b } => ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt(),
                Seg::Arc { r, a0, a1, .. } => r * (a1 - a0).abs() + 2.0 * BEND_LEAD,
            };
            let mut p = Piece {
                seg,
                len,
                ends: [(0, 0); 2],
            };
            p.ends = [key(p.point(0.0)), key(p.point(len))];
            let i = t.pieces.len();
            for (e, k) in p.ends.iter().enumerate() {
                t.nodes.entry(*k).or_default().push((i, e));
            }
            t.pieces.push(p);
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Nearest point on any piece to (x, y): (piece, distance along it, distance away).
    pub fn nearest(&self, x: f32, y: f32) -> Option<(usize, f32, f32)> {
        let mut best: Option<(usize, f32, f32)> = None;
        for (i, p) in self.pieces.iter().enumerate() {
            let n = (p.len / 2.0).ceil() as usize;
            for k in 0..=n {
                let s = p.len * k as f32 / n as f32;
                let (px, py) = p.point(s);
                let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
                if best.is_none_or(|b| d < b.2) {
                    best = Some((i, s, d));
                }
            }
        }
        best
    }

    /// Points along every piece (drawing).
    pub fn polylines(&self) -> Vec<Vec<(f32, f32)>> {
        self.pieces
            .iter()
            .map(|p| {
                let n = (p.len / 4.0).ceil().max(1.0) as usize;
                (0..=n)
                    .map(|k| p.point(p.len * k as f32 / n as f32))
                    .collect()
            })
            .collect()
    }

    /// Free end points (no other piece joins there).
    pub fn ends(&self) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for p in &self.pieces {
            for (e, k) in p.ends.iter().enumerate() {
                if self.nodes.get(k).is_some_and(|v| v.len() == 1) {
                    out.push(p.point(if e == 0 { 0.0 } else { p.len }));
                }
            }
        }
        out
    }
}

/// An object riding a track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    piece: usize,
    s: f32,
    /// +1 toward the piece's end 1, −1 toward end 0.
    dir: f32,
    pub speed: f32,
    /// Ran past an open end: the rider has left the track.
    pub off: bool,
}

impl Rail {
    /// Attach at the track point nearest (x, y). `start` = the object's start-direction field
    /// ((flags >> 20) & 3), read as a preferred heading [inf]: 0 up-right, 1 right, 2 up-left,
    /// 3 up (the game remaps it through {1, 0, 3, 2} to directions 0–3).
    pub fn attach(tracks: &Tracks, x: f32, y: f32, start: u8, winged: bool) -> Option<Self> {
        let (piece, s, d) = tracks.nearest(x, y)?;
        if d > 24.0 {
            return None;
        }
        let want = [(1.0, 1.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0)][start as usize & 3];
        let t = tracks.pieces[piece].tangent(s);
        let dot = t.0 * want.0 + t.1 * want.1;
        Some(Rail {
            piece,
            s,
            dir: if dot < -1e-3 { -1.0 } else { 1.0 },
            speed: if winged { SPEED_WINGED } else { SPEED },
            off: false,
        })
    }

    pub fn point(&self, tracks: &Tracks) -> (f32, f32) {
        tracks.pieces[self.piece].point(self.s)
    }

    /// Advance one frame; returns the new point.
    pub fn step(&mut self, tracks: &Tracks) -> (f32, f32) {
        let mut left = self.speed;
        for _ in 0..8 {
            let p = tracks.pieces[self.piece];
            let to_end = if self.dir > 0.0 {
                p.len - self.s
            } else {
                self.s
            };
            if left <= to_end {
                self.s += left * self.dir;
                break;
            }
            left -= to_end;
            let end = if self.dir > 0.0 { 1 } else { 0 };
            self.s = if end == 1 { p.len } else { 0.0 };
            // heading at the end, to pick the straightest continuation
            let t = p.tangent(self.s);
            let h = (t.0 * self.dir, t.1 * self.dir);
            let next = tracks.nodes[&p.ends[end]]
                .iter()
                .filter(|&&(i, _)| i != self.piece)
                .map(|&(i, e)| {
                    let q = tracks.pieces[i];
                    let (s, dir) = if e == 0 { (0.0, 1.0) } else { (q.len, -1.0) };
                    let qt = q.tangent(s);
                    (i, s, dir, qt.0 * dir * h.0 + qt.1 * dir * h.1)
                })
                .max_by(|a, b| a.3.total_cmp(&b.3));
            match next {
                Some((i, s, dir, _)) => {
                    self.piece = i;
                    self.s = s;
                    self.dir = dir;
                }
                None if tracks.open.contains(&p.ends[end]) => {
                    // open end: off the track
                    self.off = true;
                    break;
                }
                None => self.dir = -self.dir, // closed end: turn back
            }
        }
        self.point(tracks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bends_are_a_24_px_arc_between_8_px_leads() {
        let t = Tracks::new([(0, 0, 4)]);
        let p = t.pieces[0];
        assert!((p.len - (16.0 + 12.0 * std::f32::consts::PI)).abs() < 1e-3);
        // the ends are the tile centres one tile from the piece centre (24, 24)
        let (a, b) = (p.point(0.0), p.point(p.len));
        assert!(
            (a.0 - 8.0).abs() < 1e-3 && (a.1 - 40.0).abs() < 1e-3,
            "{a:?}"
        );
        assert!(
            (b.0 - 40.0).abs() < 1e-3 && (b.1 - 8.0).abs() < 1e-3,
            "{b:?}"
        );
        // the lead runs straight, then the arc is 24 px from its centre (16, 16)
        let l = p.point(4.0);
        assert!(
            (l.0 - 12.0).abs() < 1e-3 && (l.1 - 40.0).abs() < 1e-3,
            "{l:?}"
        );
        let m = p.point(p.len / 2.0);
        let d = ((m.0 - 16.0).powi(2) + (m.1 - 16.0).powi(2)).sqrt();
        assert!((d - 24.0).abs() < 1e-3, "{d}");
    }

    #[test]
    fn pieces_join_and_riders_follow_corners() {
        // the corpus chain: horizontal (14,29), vertical (13,30), horizontal (12,31)
        let t = Tracks::new([(14, 29, 0), (13, 30, 1), (12, 31, 0)]);
        // a rider at the right end of the first piece, heading left
        let mut r = Rail::attach(&t, 16.0 * 16.0 + 8.0, 30.0 * 16.0 + 8.0, 2, false).unwrap();
        assert_eq!(r.dir, -1.0);
        let start = r.point(&t);
        assert_eq!(start, (264.0, 488.0));
        // 32 px left, then 32 up, then 32 left: 96 px at 0.75 = 128 frames
        for _ in 0..64 {
            r.step(&t);
        }
        assert_eq!(r.point(&t), (232.0, 504.0), "turned the corner upward");
        for _ in 0..64 {
            r.step(&t);
        }
        assert_eq!(r.point(&t), (200.0, 520.0), "reached the far end");
        r.step(&t);
        let p = r.step(&t);
        assert!(p.0 > 200.0, "turned back at the closed end: {p:?}");
    }

    #[test]
    fn winged_riders_go_twice_as_fast_and_arcs_are_smooth() {
        let t = Tracks::new([(10, 10, 4)]);
        assert_eq!(t.ends().len(), 2);
        let mut r = Rail::attach(&t, 10.0 * 16.0 + 8.0, 12.0 * 16.0 + 8.0, 1, true).unwrap();
        assert_eq!(r.speed, SPEED_WINGED);
        let mut prev = r.point(&t);
        for _ in 0..30 {
            let p = r.step(&t);
            let d = ((p.0 - prev.0).powi(2) + (p.1 - prev.1).powi(2)).sqrt();
            assert!((d - 1.5).abs() < 0.05, "{d}");
            prev = p;
        }
    }

    #[test]
    fn open_ends_let_the_rider_off_and_closed_ends_turn_it_back() {
        // one horizontal piece: k0 (its right end) open, k1 (left) closed
        let t = Tracks::with_ends([(10, 10, 0, 0x81, 0x70)]);
        let y = 11.0 * 16.0 + 8.0;
        // start heading right (start field 1 = right)
        let mut r = Rail::attach(&t, 11.0 * 16.0 + 8.0, y, 1, false).unwrap();
        for _ in 0..200 {
            r.step(&t);
            if r.off {
                break;
            }
        }
        assert!(r.off, "left at the open right end");
        assert!(r.point(&t).0 > 12.0 * 16.0, "at the right end");
        // the same piece with both ends closed: it keeps bouncing
        let closed = Tracks::with_ends([(10, 10, 0, 0x71, 0x70)]);
        let mut r = Rail::attach(&closed, 11.0 * 16.0 + 8.0, y, 1, false).unwrap();
        for _ in 0..500 {
            r.step(&closed);
        }
        assert!(!r.off);
        assert!(is_open_end(0x87) && !is_open_end(0x77) && !is_open_end(0x104));
    }
}
