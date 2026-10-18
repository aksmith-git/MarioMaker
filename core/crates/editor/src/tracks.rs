//! Tracks: pieces laid two tiles at a time, their end codes, and parts riding them.
//!
//! A track record (12 bytes) at (x, y) is a piece centred on tile (x + 1, y + 1) joining two
//! points one tile from its centre: type 0 horizontal, 1 vertical, 2 diagonal down to the right,
//! 3 diagonal up to the right (4–7 bends, 8–15 junctions; kept as they are but not laid here).
//! Each piece has its own link id (stored + 1, shared with objects' link ids); a part on a track
//! has flag 0x400 and the link id of the piece it sits on, and the piece's flag byte is 1 while a
//! part sits on it [corp: 657 of 657 riders]. The two end words are the tile codes the game
//! writes at the piece's ends: a cap (closed end 0x70–0x77, open end 0x80–0x87) at a free end, a
//! joint code at a shared end written by one of the two pieces (the other writes 0x104,
//! "nothing"). The codes below are the most common ones per end pairing in the sample courses
//! (research/specs/maker.md); the engine draws tracks from the geometry alone.

use mmk_formats::record::Record;
use mmk_formats::smm2::course::Track;

use crate::{EditError, Editor, parts};

/// Track records per area (the file's array).
pub const TRACK_MAX: usize = 1500;
/// End word meaning "nothing written here".
pub const NO_CODE: u16 = 0x104;

/// End points of a piece type, relative to its centre tile; end 0 is stored in k0.
fn ends(kind: u8) -> Option<[(i32, i32); 2]> {
    Some(match kind {
        0 => [(1, 0), (-1, 0)],
        1 => [(0, 1), (0, -1)],
        2 => [(-1, 1), (1, -1)],
        3 => [(1, 1), (-1, -1)],
        _ => return None,
    })
}

/// Closed-end cap per (type, end); open ends are 0x10 higher.
const CAPS: [[u16; 2]; 4] = [[0x71, 0x70], [0x72, 0x73], [0x75, 0x74], [0x77, 0x76]];

/// Joint codes for two piece ends meeting, keyed by (type, end) pairs in sorted order: the codes
/// the first and the second end write [corp: majority per pairing].
/// A piece end: (type, end number).
type End = (u8, u8);

const JOINTS: &[(End, End, u16, u16)] = &[
    ((0, 0), (0, 1), 0x90, NO_CODE),
    ((0, 0), (1, 0), 0x96, NO_CODE),
    ((0, 0), (1, 1), 0x97, NO_CODE),
    ((0, 0), (2, 0), 0xa6, 0x9e),
    ((0, 0), (2, 1), 0xa4, 0x9c),
    ((0, 0), (3, 0), NO_CODE, 0xa7),
    ((0, 0), (3, 1), 0xa5, NO_CODE),
    ((0, 1), (1, 0), NO_CODE, 0x95),
    ((0, 1), (1, 1), NO_CODE, 0x94),
    ((0, 1), (3, 0), NO_CODE, 0x9d),
    ((0, 1), (3, 1), 0x9f, NO_CODE),
    ((1, 0), (1, 1), 0x91, NO_CODE),
    ((1, 0), (2, 0), 0xa3, 0xa1),
    ((1, 0), (3, 0), NO_CODE, 0xa0),
    ((1, 0), (3, 1), 0xa2, NO_CODE),
    ((1, 1), (2, 0), 0x99, 0x9e),
    ((1, 1), (3, 0), NO_CODE, 0x9a),
    ((1, 1), (3, 1), 0x98, NO_CODE),
    ((2, 0), (2, 1), 0x92, NO_CODE),
    ((2, 0), (3, 0), 0x9e, 0x55),
    ((2, 1), (3, 0), 0x65, 0x64),
    ((3, 0), (3, 1), 0x93, NO_CODE),
];

fn coord(v: u8) -> i32 {
    if v == 255 { -1 } else { v as i32 }
}

/// Centre tile of a piece.
pub fn centre(t: &Track) -> (i32, i32) {
    (coord(t.x) + 1, coord(t.y) + 1)
}

/// End points of a piece (None for bends and junctions).
pub fn end_points(t: &Track) -> Option<[(i32, i32); 2]> {
    let (cx, cy) = centre(t);
    ends(t.r#type).map(|e| e.map(|(dx, dy)| (cx + dx, cy + dy)))
}

/// A track piece: its type and whether its free ends are open (riders fall off) or closed.
fn new_track(cx: i32, cy: i32, kind: u8, link: i16) -> Track {
    let mut t = Track::parse(&[0u8; 12]);
    let enc = |v: i32| if v < 0 { 255 } else { v as u8 };
    t.x = enc(cx - 1);
    t.y = enc(cy - 1);
    t.r#type = kind;
    t.link_id = (link + 1) as u16;
    t.k0 = NO_CODE;
    t.k1 = NO_CODE;
    t
}

impl Editor {
    /// Pieces (indices) with an end at a point, with the end number.
    fn pieces_at(&self, p: (i32, i32)) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (i, t) in self.area().tracks.iter().enumerate() {
            if let Some(e) = end_points(t) {
                for (k, q) in e.iter().enumerate() {
                    if *q == p {
                        v.push((i, k));
                    }
                }
            } else if t.r#type < 8 {
                // bends: their corners are where the matching diagonal's are
                let e = ends(if matches!(t.r#type, 4 | 5) { 2 } else { 3 }).unwrap();
                let c = centre(t);
                for (k, (dx, dy)) in e.iter().enumerate() {
                    if (c.0 + dx, c.1 + dy) == p {
                        v.push((i, k));
                    }
                }
            }
        }
        v
    }

    /// Lay one piece centred on (cx, cy): type 0 horizontal, 1 vertical, 2 diagonal down-right,
    /// 3 diagonal up-right.
    pub fn place_track(&mut self, cx: i32, cy: i32, kind: u8) -> Result<usize, EditError> {
        let Some(e) = ends(kind) else {
            return Err(EditError::NotAllowed("track piece type"));
        };
        if !crate::rules::style_allows(&self.course.header.game_style.as_str_lossy(), 59) {
            return Err(EditError::NotAllowed("not in this game style"));
        }
        if !self.in_bounds(cx, cy) || cx > 255 || cy > 255 {
            return Err(EditError::OutOfBounds);
        }
        if self.area().tracks.iter().any(|t| centre(t) == (cx, cy)) {
            return Err(EditError::Occupied);
        }
        for (dx, dy) in e {
            // two pieces may meet at a point; junctions aren't laid by the editor
            if self.pieces_at((cx + dx, cy + dy)).len() >= 2 {
                return Err(EditError::Occupied);
            }
        }
        if self.area().tracks.len() >= TRACK_MAX {
            return Err(EditError::Limit("tracks"));
        }
        let t = new_track(cx, cy, kind, self.alloc_link_id());
        self.area_mut().tracks.items.push(t);
        let i = self.area().tracks.len() - 1;
        for (dx, dy) in e {
            self.recode((cx + dx, cy + dy));
        }
        Ok(i)
    }

    /// Lay a straight or 45° track from end point `a` to end point `b` (a multiple of 2 tiles
    /// apart). Returns the new pieces; nothing is laid if any piece can't be.
    pub fn draw_track(&mut self, a: (i32, i32), b: (i32, i32)) -> Result<Vec<usize>, EditError> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let n = dx.abs().max(dy.abs());
        if n == 0 || n % 2 != 0 || !(dx == 0 || dy == 0 || dx.abs() == dy.abs()) {
            return Err(EditError::NotAllowed(
                "tracks run straight or at 45°, 2 tiles a piece",
            ));
        }
        let (sx, sy) = (dx.signum(), dy.signum());
        let kind = match (sx, sy) {
            (_, 0) => 0,
            (0, _) => 1,
            (x, y) if x == y => 3,
            _ => 2,
        };
        let before = self.area().tracks.items.clone();
        let mut out = Vec::new();
        for i in 0..n / 2 {
            let c = (a.0 + sx * (2 * i + 1), a.1 + sy * (2 * i + 1));
            match self.place_track(c.0, c.1, kind) {
                Ok(j) => out.push(j),
                Err(e) => {
                    self.area_mut().tracks.items = before;
                    return Err(e);
                }
            }
        }
        Ok(out)
    }

    /// Rewrite the end codes at a point from the pieces meeting there.
    fn recode(&mut self, p: (i32, i32)) {
        let at = self.pieces_at(p);
        let tracks = &self.area().tracks.items;
        // bends and junctions keep their codes, and so do their neighbours
        if at.iter().any(|&(i, _)| tracks[i].r#type > 3) {
            return;
        }
        let code_of = |t: &Track, k: usize| if k == 0 { t.k0 } else { t.k1 };
        let mut writes: Vec<(usize, usize, u16)> = Vec::new();
        match at.as_slice() {
            [(i, k)] => {
                let t = &tracks[*i];
                let open = (0x80..0x88).contains(&code_of(t, *k));
                let cap = CAPS[t.r#type as usize][*k];
                writes.push((*i, *k, if open { cap + 0x10 } else { cap }));
            }
            [(i, ki), (j, kj)] => {
                let a = (tracks[*i].r#type, *ki as u8);
                let b = (tracks[*j].r#type, *kj as u8);
                let ((first, fa), (second, sb)) = if a <= b {
                    ((*i, a), (*j, b))
                } else {
                    ((*j, b), (*i, a))
                };
                let (ca, cb) = JOINTS
                    .iter()
                    .find(|j| j.0 == fa && j.1 == sb)
                    .map_or((NO_CODE, NO_CODE), |j| (j.2, j.3));
                writes.push((first, fa.1 as usize, ca));
                writes.push((second, sb.1 as usize, cb));
            }
            _ => {}
        }
        for (i, k, c) in writes {
            let t = &mut self.area_mut().tracks.items[i];
            if k == 0 {
                t.k0 = c;
            } else {
                t.k1 = c;
            }
        }
    }

    /// Open (riders fall off) or close (riders turn back) a free track end at a point.
    pub fn set_track_end(&mut self, p: (i32, i32), open: bool) -> Result<(), EditError> {
        let at = self.pieces_at(p);
        let [(i, k)] = at.as_slice() else {
            return Err(EditError::NotAllowed("not a free track end"));
        };
        let (i, k) = (*i, *k);
        let t = &self.area().tracks.items[i];
        if t.r#type > 3 {
            return Err(EditError::NotAllowed("not a free track end"));
        }
        let cap = CAPS[t.r#type as usize][k] + if open { 0x10 } else { 0 };
        let t = &mut self.area_mut().tracks.items[i];
        if k == 0 {
            t.k0 = cap;
        } else {
            t.k1 = cap;
        }
        Ok(())
    }

    /// Whether the free track end at a point is open (None: no free end there).
    pub fn track_end_open(&self, p: (i32, i32)) -> Option<bool> {
        let at = self.pieces_at(p);
        let [(i, k)] = at.as_slice() else {
            return None;
        };
        let t = &self.area().tracks.items[*i];
        let code = if *k == 0 { t.k0 } else { t.k1 };
        Some((0x80..0x88).contains(&code))
    }

    /// The track piece at a tile: its centre, else one of its ends.
    pub fn track_at(&self, tx: i32, ty: i32) -> Option<usize> {
        let tracks = &self.area().tracks.items;
        tracks
            .iter()
            .rposition(|t| centre(t) == (tx, ty))
            .or_else(|| self.pieces_at((tx, ty)).last().map(|&(i, _)| i))
    }

    /// Erase the track piece at a tile; parts riding it stay where they are, off the track.
    pub fn erase_track_at(&mut self, tx: i32, ty: i32) -> bool {
        let Some(i) = self.track_at(tx, ty) else {
            return false;
        };
        self.erase_track(i);
        true
    }

    pub fn erase_track(&mut self, i: usize) {
        let t = self.area_mut().tracks.items.remove(i);
        let link = t.link_id as i16 - 1;
        for o in self.area_mut().objects.items.iter_mut() {
            if o.flags & 0x400 != 0 && o.link_id == link {
                o.flags &= !0x400;
                o.link_id = -1;
            }
        }
        if let Some(e) = end_points(&t) {
            for p in e {
                self.recode(p);
            }
        }
    }

    /// Put a part on the nearest track (piece centre or end within 2 tiles of its reference
    /// tile): it snaps there, gets flag 0x400 and the piece's link id.
    pub fn attach_to_track(&mut self, index: usize) -> Result<(), EditError> {
        let Some(o) = self.object(index) else {
            return Err(EditError::Missing);
        };
        if !parts::caps(o.id).track {
            return Err(EditError::NotAllowed("can't ride a track"));
        }
        let r = (
            ((o.x as f32 / 10.0 - 8.0) / 16.0).round() as i32,
            ((o.y as f32 / 10.0 - 8.0) / 16.0).round() as i32,
        );
        let mut best: Option<(i32, (i32, i32), u16)> = None;
        for t in self.area().tracks.iter() {
            let mut pts = vec![centre(t)];
            pts.extend(end_points(t).into_iter().flatten());
            for p in pts {
                let d = (p.0 - r.0).abs().max((p.1 - r.1).abs());
                if d <= 2 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, p, t.link_id));
                }
            }
        }
        let Some((_, p, link)) = best else {
            return Err(EditError::NotAllowed("no track there"));
        };
        let o = &mut self.area_mut().objects.items[index];
        o.x = (p.0 * 16 + 8) * 10;
        o.y = (p.1 * 16 + 8) * 10;
        o.flags |= 0x400;
        o.link_id = link as i16 - 1;
        self.refresh_track_riders();
        Ok(())
    }

    /// Take a part off its track.
    pub fn detach_from_track(&mut self, index: usize) {
        if let Some(o) = self.area_mut().objects.items.get_mut(index)
            && o.flags & 0x400 != 0
        {
            o.flags &= !0x400;
            o.link_id = -1;
        }
        self.refresh_track_riders();
    }

    /// Track pieces' "a part sits on me" byte from the parts on tracks.
    pub(crate) fn refresh_track_riders(&mut self) {
        let riders: Vec<i16> = self
            .area()
            .objects
            .iter()
            .filter(|o| o.flags & 0x400 != 0)
            .map(|o| o.link_id)
            .collect();
        for t in self.area_mut().tracks.items.iter_mut() {
            if t.r#type < 8 {
                t.flags = riders.contains(&(t.link_id as i16 - 1)) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;

    #[test]
    fn tracks_join_and_cap_like_course_files() {
        let mut e = flat("M1");
        // a horizontal run of three pieces from (10, 6) to (16, 6), then up to (16, 10)
        let h = e.draw_track((10, 6), (16, 6)).unwrap();
        assert_eq!(h.len(), 3);
        let v = e.draw_track((16, 6), (16, 10)).unwrap();
        let t = &e.area().tracks.items;
        assert_eq!((t[h[0]].k0, t[h[0]].k1), (0x90, 0x70), "left cap, joint");
        assert_eq!((t[h[1]].k0, t[h[1]].k1), (0x90, NO_CODE));
        assert_eq!(
            (t[h[2]].k0, t[h[2]].k1),
            (0x97, NO_CODE),
            "horizontal into a vertical going up"
        );
        assert_eq!((t[v[1]].k0, t[v[1]].k1), (0x72, NO_CODE), "top cap");
        assert_eq!(t[h[0]].link_id, 1, "link ids start at 0 (+1)");
        e.set_track_end((10, 6), true).unwrap();
        assert_eq!(e.area().tracks.items[h[0]].k1, 0x80, "open end");
        assert!(e.draw_track((0, 3), (3, 3)).is_err(), "odd length");
        // a lift rides it
        let lift = e
            .place_sized(11, 11, 4, 3, 1, crate::DEFAULT_FLAGS)
            .unwrap();
        e.attach_to_track(lift).unwrap();
        let o = e.object(lift).unwrap();
        assert!(o.flags & 0x400 != 0);
        assert_eq!(o.link_id as u16 + 1, e.area().tracks.items[h[0]].link_id);
        assert_eq!(e.area().tracks.items[h[0]].flags, 1);
        let spikes = e.place(43, 30, 2).unwrap();
        assert!(e.attach_to_track(spikes).is_err(), "spikes don't ride");
        let back = round_trip(&e);
        let lvl = mmk_sim::level::Level::from_course(&back, 0);
        assert!(!lvl.tracks.is_empty());
        assert!(
            lvl.spawns.iter().any(|s| s.rail.is_some()),
            "the lift is on the rail"
        );
        // erasing the piece takes the lift off it
        assert!(e.erase_track_at(11, 6));
        assert_eq!(e.object(lift).unwrap().flags & 0x400, 0);
        assert_eq!(e.area().tracks.items[0].k1, 0x70, "new free end capped");
    }

    #[test]
    fn a_lift_rides_off_an_open_end_and_falls() {
        let mut e = flat("M1");
        e.draw_track((10, 12), (16, 12)).unwrap();
        e.set_track_end((16, 12), true).unwrap();
        let lift = e
            .place_sized(11, 11, 12, 3, 1, crate::DEFAULT_FLAGS)
            .unwrap();
        e.attach_to_track(lift).unwrap();
        let back = round_trip(&e);
        let mut w = mmk_sim::World::from_course(&back);
        let is_lift =
            |a: &&mmk_sim::actors::Actor| matches!(a.kind, mmk_sim::actors::Kind::Lift { .. });
        let y0 = w.actors.iter().find(is_lift).expect("lift spawned").y;
        for _ in 0..600 {
            w.step(mmk_sim::Input::default());
        }
        let gone_or_fell = w
            .actors
            .iter()
            .find(is_lift)
            .is_none_or(|a| a.y < y0 - 48.0);
        assert!(
            gone_or_fell,
            "the lift left the track at the open end and fell"
        );
        // with the end closed it stays on the track
        e.set_track_end((16, 12), false).unwrap();
        let mut w = mmk_sim::World::from_course(&round_trip(&e));
        for _ in 0..600 {
            w.step(mmk_sim::Input::default());
        }
        let a = w.actors.iter().find(is_lift).expect("still there");
        assert!((a.y - y0).abs() < 1.0, "still on the track");
    }

    #[test]
    fn diagonal_tracks() {
        let mut e = flat("M1");
        let d = e.draw_track((10, 4), (14, 8)).unwrap();
        let t = &e.area().tracks.items;
        assert!(t.iter().all(|p| p.r#type == 3));
        assert_eq!((t[d[0]].k0, t[d[0]].k1), (0x93, 0x76));
        assert_eq!((t[d[1]].k0, t[d[1]].k1), (0x77, NO_CODE));
        round_trip(&e);
    }
}
