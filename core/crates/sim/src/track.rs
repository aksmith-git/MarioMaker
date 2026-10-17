//! Tracks (59-track.md): pieces from the course's track records joined into a graph, and the rail
//! follower that carries an object along them at 0.75 px/frame (1.5 with wings).
//!
//! Geometry, worked out from the course corpus [inf]: a record at (x, y) is a piece centred on
//! object tile (x + 1, y + 1), i.e. px (16x + 24, 16y + 24); an on-track object's reference point
//! (its tile centre) lies exactly on the piece it links to. Each piece joins two points one tile
//! from its centre: type 0 horizontal, 1 vertical, 2 diagonal down to the right, 3 diagonal up to
//! the right; types 4–7 are quarter circles between the same corner points as the diagonals
//! (radius 32 px; which way each bulges is a guess). Pieces sharing an end point are joined.
//! Junctions (types 8–15) are rare and not built yet. A rider reaching a free end turns back
//! (closed end); open ends, where the rider falls off, are not told apart yet [?].

use std::collections::HashMap;

use crate::level::TILE;

/// Normal and winged rider speeds, px/frame.
pub const SPEED: f32 = 0.75;
pub const SPEED_WINGED: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Line { a: (f32, f32), b: (f32, f32) },
    /// Centre, radius, start and end angle (radians).
    Arc { c: (f32, f32), r: f32, a0: f32, a1: f32 },
}

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
            Seg::Arc { c, r, a0, a1 } => {
                let a = a0 + (a1 - a0) * t;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
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
                let a = a0 + (a1 - a0) * (s / self.len).clamp(0.0, 1.0);
                let sign = (a1 - a0).signum();
                (-a.sin() * sign, a.cos() * sign)
            }
        }
    }
}

/// The track graph of one area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    pieces: Vec<Piece>,
    nodes: HashMap<(i32, i32), Vec<(usize, usize)>>,
}

fn key(p: (f32, f32)) -> (i32, i32) {
    ((p.0 / 8.0).round() as i32, (p.1 / 8.0).round() as i32)
}

impl Tracks {
    /// Build from track records: (x, y, type) with 255 meaning −1.
    pub fn new(records: impl IntoIterator<Item = (u8, u8, u8)>) -> Self {
        let mut t = Tracks::default();
        for (x, y, ty) in records {
            let x = if x == 255 { -1 } else { x as i32 };
            let y = if y == 255 { -1 } else { y as i32 };
            let c = ((x + 1) as f32 * TILE + 8.0, (y + 1) as f32 * TILE + 8.0);
            let at = |dx: f32, dy: f32| (c.0 + dx * TILE, c.1 + dy * TILE);
            let seg = match ty {
                0 => Seg::Line { a: at(-1.0, 0.0), b: at(1.0, 0.0) },
                1 => Seg::Line { a: at(0.0, -1.0), b: at(0.0, 1.0) },
                2 => Seg::Line { a: at(-1.0, 1.0), b: at(1.0, -1.0) },
                3 => Seg::Line { a: at(-1.0, -1.0), b: at(1.0, 1.0) },
                4..=7 => {
                    use std::f32::consts::{FRAC_PI_2, PI};
                    // (centre corner, start angle, end angle)
                    let (cc, a0, a1) = match ty {
                        4 => ((-1.0, -1.0), FRAC_PI_2, 0.0),  // (−1,1) → (1,−1), bulging up-right
                        5 => ((1.0, 1.0), PI, 3.0 * FRAC_PI_2), // (−1,1) → (1,−1), bulging down-left
                        6 => ((1.0, -1.0), PI, FRAC_PI_2),     // (−1,−1) → (1,1), bulging up-left
                        _ => ((-1.0, 1.0), 3.0 * FRAC_PI_2, 2.0 * PI), // bulging down-right
                    };
                    Seg::Arc {
                        c: at(cc.0, cc.1),
                        r: 2.0 * TILE,
                        a0,
                        a1,
                    }
                }
                _ => continue,
            };
            let len = match seg {
                Seg::Line { a, b } => ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt(),
                Seg::Arc { r, a0, a1, .. } => r * (a1 - a0).abs(),
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
                (0..=n).map(|k| p.point(p.len * k as f32 / n as f32)).collect()
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
            let to_end = if self.dir > 0.0 { p.len - self.s } else { self.s };
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
}
