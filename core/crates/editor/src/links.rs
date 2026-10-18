//! Warp links and carriers: pipes (length, direction, colour, warp pairs between the areas, the
//! part that comes out), doors (normal, P, key) and warp boxes in pairs, and the link ids that tie
//! riders and track parts to their carrier.
//!
//! How course files encode them [corp: 550 sample courses; sim level.rs / warp.rs read the same]:
//! - every pipe, Lakitu's Cloud, Koopa Clown Car and swinging claw has a link id; a part riding
//!   it (in the pipe: flag 0x1) has the same link id. Track pieces draw from the same numbers
//!   (a piece stores its link id + 1). Ids are small and dense per area.
//! - pipes: direction (flags >> 5) & 3, colour (flags >> 18) & 3, warp id (flags >> 20) & 0x1F;
//!   a warp id k ≠ 0 joins the main-area pipe k with the sub-area pipe k (all 843 linked pairs in
//!   the sample cross areas).
//! - doors and warp boxes: id (flags >> 20) & 0x1F; ids 2k and 2k + 1 in the same area are a
//!   pair, and both doors of a pair have the same kind ((flags >> 18) & 3: 0 normal, 1 P door,
//!   2 key door) / both boxes the same key flag (0x4) [corp: 468 door and 51 box pairs].

use mmk_formats::smm2::course::CourseObject;

use crate::parts::{self, Dir};
use crate::{DEFAULT_FLAGS, EditError, Editor};

/// Door kinds ((flags >> 18) & 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorKind {
    Normal = 0,
    PSwitch = 1,
    Key = 2,
}

/// Highest warp id the 5-bit field holds.
pub const WARP_MAX: u8 = 31;

fn warp_id(o: &CourseObject) -> u8 {
    ((o.flags >> 20) & 0x1F) as u8
}

fn with_warp_id(flags: u32, id: u8) -> u32 {
    (flags & !(0x1F << 20)) | ((id as u32 & 0x1F) << 20)
}

/// Where a pipe's contents sit: the mouth, half a tile out from the base's axis [corp: every
/// in-pipe part of the sample, big or small].
fn mouth(o: &CourseObject) -> (i32, i32) {
    let l = (o.height.max(1) as i32 - 1) * 160;
    match Dir::from_flags(o.flags) {
        Dir::Up => (o.x + 80, o.y + l + 80),
        Dir::Down => (o.x - 80, o.y - l - 80),
        Dir::Right => (o.x + l + 80, o.y - 80),
        Dir::Left => (o.x - l - 80, o.y + 80),
    }
}

impl Editor {
    /// Link ids in use in the current area: objects' and track pieces' (stored + 1).
    pub(crate) fn link_ids(&self) -> Vec<i16> {
        let a = self.area();
        let mut v: Vec<i16> = a
            .objects
            .iter()
            .map(|o| o.link_id)
            .chain(a.tracks.iter().map(|t| t.link_id as i16 - 1))
            .filter(|&l| l >= 0)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The smallest free link id in the current area.
    pub(crate) fn alloc_link_id(&self) -> i16 {
        let used = self.link_ids();
        (0..).find(|i| used.binary_search(i).is_err()).unwrap_or(0)
    }

    // ------------------------------------------------------------ pipes

    /// Place a pipe whose bounding box starts at (tx, ty): `len` tiles long, mouth facing `dir`,
    /// colour 0 green, 1 red, 2 blue, 3 orange.
    pub fn place_pipe(
        &mut self,
        tx: i32,
        ty: i32,
        len: u8,
        dir: Dir,
        colour: u8,
    ) -> Result<usize, EditError> {
        let range = parts::resize_range(9).map(|r| r.1).unwrap_or(2..=2);
        if !range.contains(&len) {
            return Err(EditError::NotAllowed("pipe length"));
        }
        let flags = (DEFAULT_FLAGS & !0x60) | dir.flags() | ((colour as u32 & 3) << 18);
        self.place_sized(9, tx, ty, 2, len, flags)
    }

    /// Re-shape a placed pipe (keeping its bounding box's bottom-left tile) and move what is in it.
    pub fn set_pipe(
        &mut self,
        index: usize,
        len: u8,
        dir: Dir,
        colour: u8,
    ) -> Result<(), EditError> {
        let o = self.pipe(index)?.clone();
        let range = parts::resize_range(9).map(|r| r.1).unwrap_or(2..=2);
        if !range.contains(&len) {
            return Err(EditError::NotAllowed("pipe length"));
        }
        let fp = parts::footprint(&o);
        let flags = (o.flags & !(0x60 | 3 << 18)) | dir.flags() | ((colour as u32 & 3) << 18);
        let mut n = parts::new_object(9, fp.x, fp.y, 2, len, flags);
        n.link_id = o.link_id;
        n.sound_effect_id = o.sound_effect_id;
        n.child_id = o.child_id;
        n.child_flags = o.child_flags;
        self.check_replace(index, &n)?;
        let (mx, my) = mouth(&n);
        let link = n.link_id;
        let objects = &mut self.area_mut().objects.items;
        objects[index] = n;
        for r in objects.iter_mut() {
            if r.flags & 1 != 0 && r.link_id == link && r.id != 9 {
                r.x = mx;
                r.y = my;
            }
        }
        Ok(())
    }

    fn pipe(&self, index: usize) -> Result<&CourseObject, EditError> {
        match self.object(index) {
            Some(o) if o.id == 9 => Ok(o),
            Some(_) => Err(EditError::NotAllowed("not a pipe")),
            None => Err(EditError::Missing),
        }
    }

    /// The warp id of a pipe, door or warp box (0 = not linked, for pipes).
    pub fn warp(&self, index: usize) -> Option<u8> {
        self.object(index).map(warp_id)
    }

    /// Join main-area pipe `main` with sub-area pipe `sub` (indices in their areas): both get the
    /// smallest warp id no other pipe uses. Returns the id.
    pub fn link_pipes(&mut self, main: usize, sub: usize) -> Result<u8, EditError> {
        let ok = |a: &mmk_formats::smm2::Area, i: usize| {
            a.objects.items.get(i).is_some_and(|o| o.id == 9)
        };
        if !ok(&self.course.main, main) || !ok(&self.course.sub, sub) {
            return Err(EditError::NotAllowed("not a pipe"));
        }
        self.unlink_pipe_in(0, main);
        self.unlink_pipe_in(1, sub);
        let used: Vec<u8> = self
            .course
            .areas()
            .iter()
            .flat_map(|a| a.objects.iter().filter(|o| o.id == 9).map(warp_id))
            .collect();
        let id = (1..=WARP_MAX)
            .find(|i| !used.contains(i))
            .ok_or(EditError::Limit("warp pipes"))?;
        for (a, i) in [(0, main), (1, sub)] {
            let area = if a == 0 {
                &mut self.course.main
            } else {
                &mut self.course.sub
            };
            let o = &mut area.objects.items[i];
            o.flags = with_warp_id(o.flags, id);
        }
        Ok(id)
    }

    /// Break a pipe's warp link (its partner in the other area too).
    pub fn unlink_pipe(&mut self, index: usize) {
        self.unlink_pipe_in(self.area, index);
    }

    fn unlink_pipe_in(&mut self, area: usize, index: usize) {
        let Some(id) = self.course.areas()[area]
            .objects
            .items
            .get(index)
            .filter(|o| o.id == 9)
            .map(warp_id)
        else {
            return;
        };
        if id == 0 {
            return;
        }
        for a in [&mut self.course.main, &mut self.course.sub] {
            for o in a
                .objects
                .items
                .iter_mut()
                .filter(|o| o.id == 9 && warp_id(o) == id)
            {
                o.flags = with_warp_id(o.flags, 0);
            }
        }
    }

    /// Put a part in a pipe: it comes out of the mouth (a Piranha Plant lives there). Replaces
    /// what was in it. Returns the part's index.
    pub fn put_in_pipe(&mut self, pipe: usize, part: i16) -> Result<usize, EditError> {
        let p = self.pipe(pipe)?.clone();
        if !parts::caps(part).pipe {
            return Err(EditError::NotAllowed("can't come out of a pipe"));
        }
        if !crate::rules::style_allows(&self.course.header.game_style.as_str_lossy(), part) {
            return Err(EditError::NotAllowed("not in this game style"));
        }
        self.empty_pipe(pipe);
        let (w, h) = parts::default_size(part);
        let mut o = parts::new_object(part, 0, 0, w, h, DEFAULT_FLAGS | 0x1);
        self.check_limits(&o, 1)?;
        (o.x, o.y) = mouth(&p);
        o.link_id = p.link_id;
        Ok(self.push_object(o))
    }

    /// Remove what is in a pipe.
    pub fn empty_pipe(&mut self, pipe: usize) {
        let Some(link) = self.object(pipe).filter(|o| o.id == 9).map(|o| o.link_id) else {
            return;
        };
        self.area_mut()
            .objects
            .items
            .retain(|o| !(o.id != 9 && o.flags & 1 != 0 && o.link_id == link));
    }

    /// The part in a pipe, if any.
    pub fn pipe_contents(&self, pipe: usize) -> Option<usize> {
        let link = self.object(pipe).filter(|o| o.id == 9)?.link_id;
        self.area()
            .objects
            .iter()
            .position(|o| o.id != 9 && o.flags & 1 != 0 && o.link_id == link)
    }

    // ------------------------------------------------------------ doors and warp boxes

    /// Place a pair of doors (bottom tiles a and b) of one kind; they lead to each other.
    pub fn place_doors(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        kind: DoorKind,
    ) -> Result<(usize, usize), EditError> {
        self.place_pair(55, a, b, (kind as u32) << 18)
    }

    /// Place a pair of warp boxes (with a key lock: they need a key to open).
    pub fn place_warp_boxes(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        key: bool,
    ) -> Result<(usize, usize), EditError> {
        self.place_pair(97, a, b, if key { 0x4 } else { 0 })
    }

    fn place_pair(
        &mut self,
        part: i16,
        a: (i32, i32),
        b: (i32, i32),
        extra: u32,
    ) -> Result<(usize, usize), EditError> {
        let used: Vec<u8> = self
            .area()
            .objects
            .iter()
            .filter(|o| o.id == part)
            .map(|o| warp_id(o) >> 1)
            .collect();
        let k = (0..=WARP_MAX / 2)
            .find(|k| !used.contains(k))
            .ok_or(EditError::Limit(if part == 55 {
                "doors"
            } else {
                "warp boxes"
            }))?;
        let (w, h) = parts::default_size(part);
        let first = parts::new_object(
            part,
            a.0,
            a.1,
            w,
            h,
            with_warp_id(DEFAULT_FLAGS | extra, 2 * k),
        );
        let second = parts::new_object(
            part,
            b.0,
            b.1,
            w,
            h,
            with_warp_id(DEFAULT_FLAGS | extra, 2 * k + 1),
        );
        self.check_new(&first)?;
        let i = self.push_object(first);
        if let Err(e) = self.check_new(&second) {
            self.area_mut().objects.items.remove(i);
            return Err(e);
        }
        let j = self.push_object(second);
        Ok((i, j))
    }

    /// Change the kind of a door (and its partner).
    pub fn set_door_kind(&mut self, index: usize, kind: DoorKind) -> Result<(), EditError> {
        match self.object(index) {
            Some(o) if o.id == 55 => {}
            _ => return Err(EditError::NotAllowed("not a door")),
        }
        let partner = self.partner(index);
        for i in std::iter::once(index).chain(partner) {
            let o = &mut self.area_mut().objects.items[i];
            o.flags = (o.flags & !(3 << 18)) | (kind as u32) << 18;
        }
        Ok(())
    }

    /// A door's or warp box's partner in this area.
    pub fn partner(&self, index: usize) -> Option<usize> {
        let o = self.object(index)?;
        if !matches!(o.id, 55 | 97) {
            return None;
        }
        let (part, id) = (o.id, warp_id(o));
        self.area()
            .objects
            .iter()
            .enumerate()
            .find(|(i, p)| *i != index && p.id == part && warp_id(p) == id ^ 1)
            .map(|(i, _)| i)
    }

    /// Before erasing a pipe: its partner in the other area loses the link.
    pub(crate) fn release_links(&mut self, o: &CourseObject) {
        if o.id == 9 && warp_id(o) != 0 {
            let id = warp_id(o);
            let other = if self.area == 0 {
                &mut self.course.sub
            } else {
                &mut self.course.main
            };
            for p in other
                .objects
                .items
                .iter_mut()
                .filter(|p| p.id == 9 && warp_id(p) == id)
            {
                p.flags = with_warp_id(p.flags, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;
    use mmk_sim::level::{Level, WarpKind};

    #[test]
    fn pipes_link_main_and_sub_area() {
        let mut e = flat("M1");
        let a = e.place_pipe(20, 2, 3, Dir::Up, 0).unwrap();
        e.set_area(1);
        for x in 0..20 {
            e.paint_ground(x, 0).unwrap();
        }
        let b = e.place_pipe(5, 1, 2, Dir::Up, 1).unwrap();
        assert_eq!(e.link_pipes(a, b), Ok(1));
        e.set_area(0);
        let g = e.put_in_pipe(a, 2).unwrap(); // a Piranha Plant lives in it
        assert_eq!(e.object(g).unwrap().link_id, e.object(a).unwrap().link_id);
        let back = round_trip(&e);
        let main = Level::from_course(&back, 0);
        let sub = Level::from_course(&back, 1);
        let w = main
            .warps
            .iter()
            .find(|w| w.id == 1)
            .expect("main pipe warp");
        assert_eq!(w.kind, WarpKind::Pipe(mmk_sim::level::PipeDir::Up));
        assert_eq!(w.len, 3);
        assert!(sub.warps.iter().any(|w| w.id == 1), "sub-area end");
        let dest = mmk_sim::warp::destination(w, 0, &[main.warps.clone(), sub.warps.clone()]);
        assert_eq!(dest.map(|d| d.0), Some(1), "the pipe leads to the sub-area");
        assert!(
            main.spawns.iter().any(|s| s.pipe.is_some()),
            "the Piranha Plant sits in the pipe"
        );
        // erasing one end unlinks the other
        e.erase_object(a);
        assert_eq!(e.course.sub.objects.items[b].flags >> 20 & 0x1F, 0);
        assert!(
            e.area().objects.iter().all(|o| o.id != 2),
            "the contents went with it"
        );
    }

    #[test]
    fn pipe_shapes_and_contents_follow_resizing() {
        let mut e = flat("M1");
        let p = e.place_pipe(30, 2, 2, Dir::Up, 0).unwrap();
        let g = e.put_in_pipe(p, 0).unwrap();
        e.set_pipe(p, 4, Dir::Right, 2).unwrap();
        let o = e.object(p).unwrap();
        assert_eq!(parts::footprint(o), crate::Rect::new(30, 2, 4, 2));
        assert_eq!((o.flags >> 18) & 3, 2, "blue");
        let lvl = Level::from_course(&round_trip(&e), 0);
        assert!(
            lvl.generators
                .iter()
                .any(|g| g.dir == mmk_sim::level::PipeDir::Right)
        );
        assert!(e.object(g).is_some());
        assert!(e.set_pipe(p, 1, Dir::Up, 0).is_err(), "too short");
        assert!(
            e.put_in_pipe(p, 6).is_err(),
            "hard blocks don't come out of pipes"
        );
    }

    #[test]
    fn doors_and_warp_boxes_come_in_pairs() {
        let mut e = flat("M1");
        let (a, b) = e.place_doors((12, 2), (40, 2), DoorKind::Normal).unwrap();
        let (c, d) = e.place_doors((14, 2), (44, 2), DoorKind::Key).unwrap();
        assert_eq!(
            (e.warp(a), e.warp(b), e.warp(c), e.warp(d)),
            (Some(0), Some(1), Some(2), Some(3))
        );
        e.set_door_kind(c, DoorKind::PSwitch).unwrap();
        assert_eq!(
            e.object(d).unwrap().flags >> 18 & 3,
            1,
            "the partner changes too"
        );
        let back = round_trip(&e);
        let lvl = Level::from_course(&back, 0);
        let from = lvl.warps.iter().find(|w| w.id == 0).unwrap();
        let (_, to) =
            mmk_sim::warp::destination(from, 0, std::slice::from_ref(&lvl.warps)).unwrap();
        assert_eq!(to.x, 40.0 * 16.0 + 8.0);
        assert!(lvl.warps.iter().any(|w| w.kind == WarpKind::Door(1)));
        // erasing a door takes its partner
        e.erase(12, 2);
        assert_eq!(e.area().objects.iter().filter(|o| o.id == 55).count(), 2);
        let mut w = flat("3W");
        let (x, _) = w.place_warp_boxes((10, 2), (20, 2), true).unwrap();
        assert_eq!(w.object(x).unwrap().flags & 4, 4);
        let lvl = Level::from_course(&round_trip(&w), 0);
        assert_eq!(
            lvl.warps
                .iter()
                .filter(|w| w.kind == WarpKind::Box { key: true })
                .count(),
            2
        );
    }
}
