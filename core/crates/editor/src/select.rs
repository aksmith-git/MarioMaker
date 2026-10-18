//! Rectangle selection: copy, cut, paste, move and delete parts, ground and track pieces.
//!
//! A selection takes every part whose footprint meets the rectangle (with what rides it), the
//! ground tiles and the track pieces centred inside it. Pasting re-checks every placement rule;
//! if anything doesn't fit, nothing is pasted. Pasted copies get fresh link ids and path slots;
//! a copied pipe is unlinked, and a door or warp box is only pasted with its partner (as a new
//! pair). Moving keeps warp links and pairs.

use mmk_formats::smm2::course::{CourseObject, Track};

use crate::{EditError, Editor, Rect, paths, rules, tracks};

/// What was copied, relative to the selection's bottom-left tile.
#[derive(Clone, Debug)]
pub struct Clipboard {
    pub size: (i32, i32),
    pub objects: Vec<CourseObject>,
    /// Path node codes of path parts, by index into `objects`.
    pub paths: Vec<(usize, Vec<u8>)>,
    pub ground: Vec<(i32, i32)>,
    pub tracks: Vec<Track>,
}

fn shift_obj(o: &mut CourseObject, dx: i32, dy: i32) {
    o.x += dx * 160;
    o.y += dy * 160;
}

fn shift_track(t: &mut Track, dx: i32, dy: i32) -> bool {
    let (cx, cy) = tracks::centre(t);
    let (x, y) = (cx - 1 + dx, cy - 1 + dy);
    if !(-1..=254).contains(&x) || !(-1..=254).contains(&y) {
        return false;
    }
    t.x = if x < 0 { 255 } else { x as u8 };
    t.y = if y < 0 { 255 } else { y as u8 };
    true
}

impl Editor {
    /// Indices of the selected parts (riders and door / box partners included once).
    fn selected(&self, r: Rect) -> Vec<usize> {
        let mut v = self.objects_in(r);
        let links: Vec<i16> = v
            .iter()
            .map(|&i| &self.area().objects.items[i])
            .filter(|o| rules::is_carrier(o.id) && o.link_id >= 0)
            .map(|o| o.link_id)
            .collect();
        for (i, o) in self.area().objects.iter().enumerate() {
            if rules::is_rider(o) && links.contains(&o.link_id) {
                v.push(i);
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    fn selected_tracks(&self, r: Rect) -> Vec<usize> {
        self.area()
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let (cx, cy) = tracks::centre(t);
                r.contains(cx, cy)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Copy what is in `r` (also kept as the editor's clipboard).
    pub fn copy(&mut self, r: Rect) -> Clipboard {
        let a = self.area();
        let mut clip = Clipboard {
            size: (r.w, r.h),
            objects: Vec::new(),
            paths: Vec::new(),
            ground: Vec::new(),
            tracks: Vec::new(),
        };
        for i in self.selected(r) {
            let mut o = a.objects.items[i].clone();
            if let Some(p) = self.path(i) {
                clip.paths.push((clip.objects.len(), p));
            }
            shift_obj(&mut o, -r.x, -r.y);
            clip.objects.push(o);
        }
        for g in a.ground.iter() {
            if r.contains(g.x as i32, g.y as i32) {
                clip.ground.push((g.x as i32 - r.x, g.y as i32 - r.y));
            }
        }
        for i in self.selected_tracks(r) {
            let mut t = a.tracks.items[i].clone();
            shift_track(&mut t, -r.x, -r.y);
            clip.tracks.push(t);
        }
        self.clipboard = Some(clip.clone());
        clip
    }

    /// Delete everything in `r`. Returns how many things went.
    pub fn delete_rect(&mut self, r: Rect) -> usize {
        self.remove_rect(r, true)
    }

    fn remove_rect(&mut self, r: Rect, cascade: bool) -> usize {
        let objs = self.selected(r);
        let trks = self.selected_tracks(r);
        let ground: Vec<(i32, i32)> = self
            .area()
            .ground
            .iter()
            .map(|g| (g.x as i32, g.y as i32))
            .filter(|&(x, y)| r.contains(x, y))
            .collect();
        let n = objs.len() + trks.len() + ground.len();
        for &i in objs.iter().rev() {
            if cascade {
                if i < self.area().objects.len() {
                    self.erase_object(i);
                }
            } else {
                let o = self.area_mut().objects.items.remove(i);
                self.free_path(&o);
            }
        }
        for &i in trks.iter().rev() {
            self.erase_track(i);
        }
        for (x, y) in ground {
            self.erase_ground(x, y);
        }
        self.refresh_track_riders();
        n
    }

    /// Cut: copy, then delete.
    pub fn cut(&mut self, r: Rect) -> Clipboard {
        let c = self.copy(r);
        self.delete_rect(r);
        c
    }

    /// Paste a clipboard with its bottom-left at (tx, ty). All or nothing.
    pub fn paste(&mut self, clip: &Clipboard, tx: i32, ty: i32) -> Result<(), EditError> {
        self.paste_with(clip, tx, ty, false)
    }

    /// Paste the editor's own clipboard.
    pub fn paste_clipboard(&mut self, tx: i32, ty: i32) -> Result<(), EditError> {
        let Some(c) = self.clipboard.clone() else {
            return Err(EditError::Missing);
        };
        self.paste(&c, tx, ty)
    }

    /// Move what is in `r` by (dx, dy). All or nothing.
    pub fn move_rect(&mut self, r: Rect, dx: i32, dy: i32) -> Result<(), EditError> {
        let saved = self.course.clone();
        let keep = self.clipboard.clone();
        let clip = self.copy(r);
        self.clipboard = keep;
        self.remove_rect(r, false);
        match self.paste_with(&clip, r.x + dx, r.y + dy, true) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.course = saved;
                Err(e)
            }
        }
    }

    fn paste_with(
        &mut self,
        clip: &Clipboard,
        tx: i32,
        ty: i32,
        moving: bool,
    ) -> Result<(), EditError> {
        let saved = self.course.clone();
        let r = self.paste_inner(clip, tx, ty, moving);
        if r.is_err() {
            self.course = saved;
        }
        r
    }

    fn paste_inner(
        &mut self,
        clip: &Clipboard,
        tx: i32,
        ty: i32,
        moving: bool,
    ) -> Result<(), EditError> {
        // tracks first: their new link ids are what track riders point at
        let mut link_map: Vec<(i16, i16)> = Vec::new();
        for t in &clip.tracks {
            let mut t = t.clone();
            if !shift_track(&mut t, tx, ty) {
                return Err(EditError::OutOfBounds);
            }
            let (cx, cy) = tracks::centre(&t);
            let i = self.place_track(cx, cy, t.r#type)?;
            let new = self.area().tracks.items[i].link_id as i16 - 1;
            link_map.push((t.link_id as i16 - 1, new));
            // keep open ends open
            if let Some(e) = tracks::end_points(&self.area().tracks.items[i]) {
                for (k, p) in e.iter().enumerate() {
                    let code = if k == 0 { t.k0 } else { t.k1 };
                    if (0x80..0x88).contains(&code) {
                        let _ = self.set_track_end(*p, true);
                    }
                }
            }
        }
        // door / box pairs: ids present twice get a new pair (moving keeps them)
        let pair_ok = |o: &CourseObject| {
            !matches!(o.id, 55 | 97)
                || moving
                || clip
                    .objects
                    .iter()
                    .any(|p| p.id == o.id && (p.flags >> 20 & 0x1F) == (o.flags >> 20 & 0x1F) ^ 1)
        };
        let mut pair_map: Vec<(i16, u32, u32)> = Vec::new();
        // carriers and plain parts, then riders
        let order: Vec<usize> = (0..clip.objects.len())
            .filter(|&i| !rules::is_rider(&clip.objects[i]))
            .chain((0..clip.objects.len()).filter(|&i| rules::is_rider(&clip.objects[i])))
            .collect();
        for i in order {
            let mut o = clip.objects[i].clone();
            if !pair_ok(&o) {
                continue;
            }
            shift_obj(&mut o, tx, ty);
            let rider = rules::is_rider(&o);
            if o.flags & 0x400 != 0 {
                match link_map.iter().find(|m| m.0 == o.link_id) {
                    Some(m) => o.link_id = m.1,
                    None => {
                        o.flags &= !0x400;
                        o.link_id = -1;
                    }
                }
            } else if rider {
                match link_map.iter().find(|m| m.0 == o.link_id) {
                    Some(m) => o.link_id = m.1,
                    None => continue, // its carrier wasn't copied
                }
            } else if rules::is_carrier(o.id) {
                let old = o.link_id;
                o.link_id = -1;
                if !moving && o.id == 9 {
                    o.flags &= !(0x1F << 20);
                }
                self.check_new(&o)?;
                let j = self.push_object(o);
                link_map.push((old, self.area().objects.items[j].link_id));
                continue;
            }
            if !moving && matches!(o.id, 55 | 97) {
                let k = (o.flags >> 20 & 0x1F) >> 1;
                let new = match pair_map.iter().find(|p| p.0 == o.id && p.1 == k) {
                    Some(p) => p.2,
                    None => {
                        let used: Vec<u32> = self
                            .area()
                            .objects
                            .iter()
                            .filter(|p| p.id == o.id)
                            .map(|p| (p.flags >> 20 & 0x1F) >> 1)
                            .chain(pair_map.iter().filter(|p| p.0 == o.id).map(|p| p.2))
                            .collect();
                        let n = (0..16)
                            .find(|n| !used.contains(n))
                            .ok_or(EditError::Limit("doors"))?;
                        pair_map.push((o.id, k, n));
                        n
                    }
                };
                let low = o.flags >> 20 & 1;
                o.flags = (o.flags & !(0x1F << 20)) | (new * 2 + low) << 20;
            }
            if rider {
                if !self.in_bounds(
                    ((o.x / 10 - 8) as f32 / 16.0).round() as i32,
                    ((o.y / 10 - 8) as f32 / 16.0).round() as i32,
                ) {
                    return Err(EditError::OutOfBounds);
                }
                self.check_limits(&o, 1)?;
            } else {
                self.check_new(&o)?;
                if !self.path_slot_free(o.id) {
                    return Err(EditError::Limit("paths"));
                }
            }
            let path = clip.paths.iter().find(|p| p.0 == i).map(|p| p.1.clone());
            if paths::has_path(o.id) {
                o.extended_data &= if o.id == 84 { 0x10 } else { 0 };
            }
            let j = self.push_object(o);
            if let Some(codes) = path {
                let (heading, steps) = paths::route_of(&codes);
                self.set_path(j, heading, &steps)?;
            }
        }
        for &(gx, gy) in &clip.ground {
            self.paint_ground(gx + tx, gy + ty)?;
        }
        self.refresh_track_riders();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::DoorKind;
    use crate::parts;
    use crate::test_util::*;

    #[test]
    fn copy_paste_parts_ground_and_tracks() {
        let mut e = flat("M1");
        for x in 20..23 {
            e.paint_ground(x, 4).unwrap();
        }
        let q = e.place(5, 21, 7).unwrap();
        e.set_child(q, 20, 0);
        let p = e.place_pipe(24, 2, 3, parts::Dir::Up, 0).unwrap();
        e.put_in_pipe(p, 0).unwrap();
        e.draw_track((20, 10), (24, 10)).unwrap();
        let lift = e
            .place_sized(11, 21, 10, 3, 1, crate::DEFAULT_FLAGS)
            .unwrap();
        e.attach_to_track(lift).unwrap();
        let r = Rect::new(20, 2, 6, 10);
        let clip = e.copy(r);
        assert_eq!(clip.ground.len(), 3);
        assert_eq!(clip.tracks.len(), 2);
        e.paste(&clip, 50, 2).unwrap();
        assert!(e.has_ground(51, 4));
        let q2 = e.object_at(51, 7).unwrap();
        assert_eq!(e.object(q2).unwrap().child_id, 20);
        let p2 = e.object_at(54, 3).unwrap();
        assert_ne!(
            e.object(p2).unwrap().link_id,
            e.object(p).unwrap().link_id,
            "fresh link id"
        );
        assert!(e.pipe_contents(p2).is_some(), "the Goomba came along");
        assert_eq!(e.area().tracks.len(), 4);
        let riders = e
            .area()
            .objects
            .iter()
            .filter(|o| o.flags & 0x400 != 0)
            .count();
        assert_eq!(riders, 2);
        // overlapping paste fails as a whole
        let n = e.area().objects.len();
        assert_eq!(e.paste(&clip, 52, 2), Err(EditError::Occupied));
        assert_eq!(e.area().objects.len(), n);
        let back = round_trip(&e);
        let lvl = mmk_sim::level::Level::from_course(&back, 0);
        assert_eq!(lvl.spawns.iter().filter(|s| s.rail.is_some()).count(), 2);
        assert_eq!(lvl.generators.len(), 2);
    }

    #[test]
    fn move_and_delete() {
        let mut e = flat("M1");
        let (a, _) = e.place_doors((20, 2), (40, 2), DoorKind::Key).unwrap();
        let ia = e.warp(a);
        e.place(6, 22, 4).unwrap();
        e.move_rect(Rect::new(19, 2, 5, 4), 10, 1).unwrap();
        assert!(e.object_at(32, 5).is_some());
        let d = e.object_at(30, 3).unwrap();
        assert_eq!(e.warp(d), ia, "a moved door keeps its pair");
        assert_eq!(e.area().objects.iter().filter(|o| o.id == 55).count(), 2);
        // moving onto something fails and changes nothing
        let before = e.save().unwrap();
        assert!(e.move_rect(Rect::new(29, 3, 4, 3), 10, -1).is_err());
        assert_eq!(e.save().unwrap(), before);
        // copying one door of a pair pastes nothing of it
        let c = e.copy(Rect::new(30, 3, 1, 2));
        e.paste(&c, 60, 3).unwrap();
        assert_eq!(e.area().objects.iter().filter(|o| o.id == 55).count(), 2);
        assert_eq!(e.delete_rect(Rect::new(28, 2, 6, 6)), 2);
        assert!(
            e.area().objects.iter().all(|o| o.id != 55),
            "the partner went too"
        );
        round_trip(&e);
    }
}
