//! Course Maker core (Phase 9): an editable SMM2 course with the operations the maker UI needs —
//! place, resize and erase parts, paint ground (auto-tiled), both areas, warp links (pipes, doors,
//! warp boxes), tracks and block paths, rectangle selection with copy / paste / move, course and
//! area settings, the game's placement rules, undo/redo — and the course file it saves. No UI
//! here; the shells draw the editor with the engine.
//!
//! Coordinates are tiles of the current area (x right, y up; tile (0, 0) is the bottom-left).
//! Parts are stored the way courses store them (docs/formats/smm2-course.md): positions in
//! 1/10 px, default flags 0x06000040. Everything written here was checked against how the 550
//! sample courses encode the same thing (research/specs/maker.md, private).

pub mod autotile;
pub mod links;
pub mod parts;
pub mod paths;
pub mod rules;
pub mod select;
pub mod settings;
pub mod tracks;

use mmk_formats::record::Record;
use mmk_formats::smm2::course::{Area, Course, CourseObject, GroundTile};

pub use parts::default_size;

/// Default placement flags of a course object.
pub const DEFAULT_FLAGS: u32 = 0x0600_0040;
/// Undo depth (each step is a full course snapshot, ≈ 370 KB).
pub const UNDO_DEPTH: usize = 64;

/// A tile rectangle: bottom-left (x, y), width and height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// The rectangle spanning two corner tiles (inclusive, any order).
    pub fn spanning(a: (i32, i32), b: (i32, i32)) -> Self {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        Self::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    pub fn tiles(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.x..self.x + self.w).flat_map(move |x| (self.y..self.y + self.h).map(move |y| (x, y)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    OutOfBounds,
    /// Another part or ground is in the way.
    Occupied,
    /// The area's record array (or a path / link table) is full.
    Full,
    /// The game's part limit for this part (or its group) is reached.
    Limit(&'static str),
    /// The start or goal zone can't be built over.
    Protected,
    /// The game doesn't allow this (wrong part, style, container …).
    NotAllowed(&'static str),
    /// No part at that tile / index.
    Missing,
    Format(String),
}

pub struct Editor {
    pub course: Course,
    /// 0 main area, 1 sub area.
    pub area: usize,
    /// The last copied selection (copy / cut / paste).
    pub clipboard: Option<select::Clipboard>,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
}

impl Editor {
    /// A new, empty course in `style` ("M1", "M3", "MW", "WU", "3W").
    pub fn new(style: &str) -> Self {
        let mut course = Course::blank(style);
        course.header.name.set("New course");
        // a new course's sub-area is 84 tiles wide (every unused sub-area in the sample is) and
        // its boundary type 1 [corp]
        course.sub.header.boundary_right = settings::SUB_DEFAULT_WIDTH * 16;
        course.main.header.boundary_type = 0;
        course.sub.header.boundary_type = 1;
        course.header.management_flags = 0x3;
        Self::from_course(course)
    }

    pub fn open(bytes: &[u8]) -> Result<Self, EditError> {
        Course::parse(bytes)
            .map(Self::from_course)
            .map_err(|e| EditError::Format(e.to_string()))
    }

    fn from_course(course: Course) -> Self {
        Self {
            course,
            area: 0,
            clipboard: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The course file (raw 0x5BFC0-byte course data).
    pub fn save(&self) -> Result<Vec<u8>, EditError> {
        self.course
            .to_bytes()
            .map_err(|e| EditError::Format(e.to_string()))
    }

    pub fn area(&self) -> &Area {
        if self.area == 0 {
            &self.course.main
        } else {
            &self.course.sub
        }
    }

    pub(crate) fn area_mut(&mut self) -> &mut Area {
        if self.area == 0 {
            &mut self.course.main
        } else {
            &mut self.course.sub
        }
    }

    /// Edit the main (0) or sub (1) area from now on.
    pub fn set_area(&mut self, area: usize) {
        self.area = area.min(1);
        if self.area == 1 {
            // management flag bit 6: the sub-area has been set up (set in every sample course
            // whose sub-area holds anything) [corp]
            self.course.header.management_flags |= 0x40;
        }
    }

    /// Area size in tiles (from its right and top boundaries).
    pub fn size(&self) -> (i32, i32) {
        let h = &self.area().header;
        (h.boundary_right / 16, h.boundary_top / 16)
    }

    pub(crate) fn in_bounds(&self, tx: i32, ty: i32) -> bool {
        let (w, h) = self.size();
        (0..w).contains(&tx) && (0..h).contains(&ty)
    }

    // ------------------------------------------------------------ undo

    /// Remember the current state before an edit (call once per user action).
    pub fn checkpoint(&mut self) {
        if let Ok(b) = self.course.to_bytes() {
            self.undo.push(b);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn undo(&mut self) -> bool {
        self.swap_state(true)
    }

    pub fn redo(&mut self) -> bool {
        self.swap_state(false)
    }

    fn swap_state(&mut self, back: bool) -> bool {
        let (from, to) = if back {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some(state) = from.pop() else {
            return false;
        };
        let Ok(now) = self.course.to_bytes() else {
            return false;
        };
        to.push(now);
        match Course::parse(&state) {
            Ok(c) => {
                self.course = c;
                true
            }
            Err(_) => false,
        }
    }

    /// Forget redo steps (after an undo that only reverted a no-op checkpoint).
    pub fn redo_clear(&mut self) {
        self.redo.clear();
    }

    /// Drop the last checkpoint without restoring it (the action changed nothing).
    pub fn discard_checkpoint(&mut self) {
        self.undo.pop();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ------------------------------------------------------------ parts

    /// Index of the part covering a tile (the last placed wins; parts riding in pipes, clown
    /// cars, clouds and claws are found through their carrier).
    pub fn object_at(&self, tx: i32, ty: i32) -> Option<usize> {
        let objs = &self.area().objects.items;
        let hit = |want_solid: bool| {
            objs.iter()
                .enumerate()
                .rev()
                .filter(|(_, o)| !rules::is_rider(o))
                .filter(|(_, o)| rules::is_background(o.id) != want_solid)
                .find(|(_, o)| parts::covers(o, tx, ty))
                .map(|(i, _)| i)
        };
        // parts drawn in front of semisolids are picked first
        hit(true).or_else(|| hit(false))
    }

    /// Indices of the parts whose footprint meets `r` (riders excluded).
    pub fn objects_in(&self, r: Rect) -> Vec<usize> {
        self.area()
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| !rules::is_rider(o) && parts::footprint(o).intersects(&r))
            .map(|(i, _)| i)
            .collect()
    }

    pub(crate) fn ground_at(&self, tx: i32, ty: i32) -> Option<usize> {
        self.area()
            .ground
            .iter()
            .position(|g| g.x as i32 == tx && g.y as i32 == ty)
    }

    pub fn has_ground(&self, tx: i32, ty: i32) -> bool {
        self.ground_at(tx, ty).is_some()
    }

    /// Place a part with its default size at a free tile (bottom-left of its footprint).
    pub fn place(&mut self, part: i16, tx: i32, ty: i32) -> Result<usize, EditError> {
        let (w, h) = default_size(part);
        self.place_sized(part, tx, ty, w, h, DEFAULT_FLAGS)
    }

    /// Place a part covering `w`×`h` tiles from (tx, ty) (bottom-left), after checking the
    /// game's placement rules (bounds, overlap, start / goal zones, part limits, style).
    pub fn place_sized(
        &mut self,
        part: i16,
        tx: i32,
        ty: i32,
        w: u8,
        h: u8,
        flags: u32,
    ) -> Result<usize, EditError> {
        let o = parts::new_object(part, tx, ty, w, h, flags);
        self.check_new(&o)?;
        if !self.path_slot_free(part) {
            return Err(EditError::Limit("paths"));
        }
        let i = self.push_object(o);
        if let Some((heading, steps)) = paths::default_route(part) {
            self.set_path(i, heading, &steps)?;
        }
        Ok(i)
    }

    /// Append an object (no checks) and give containers a link id. Returns its index.
    pub(crate) fn push_object(&mut self, mut o: CourseObject) -> usize {
        if rules::needs_link_id(o.id) && o.link_id < 0 {
            o.link_id = self.alloc_link_id();
        }
        let objects = &mut self.area_mut().objects.items;
        objects.push(o);
        objects.len() - 1
    }

    /// Change a placed part's flags (wings 0x2, alternate form 0x4, big 0x4000, …).
    pub fn set_flags(&mut self, index: usize, flags: u32) {
        if let Some(o) = self.area_mut().objects.items.get_mut(index) {
            o.flags = flags;
        }
    }

    /// A placed part (to read its flags and child).
    pub fn object(&self, index: usize) -> Option<&CourseObject> {
        self.area().objects.items.get(index)
    }

    /// Toggle flag bits on a part (wings 0x2, alternate 0x4, parachute 0x8000); wings and
    /// parachutes only go on parts the game lets carry them.
    pub fn toggle_flags(&mut self, index: usize, bits: u32) {
        let Some(o) = self.object(index) else { return };
        let caps = parts::caps(o.id);
        if (bits & 0x2 != 0 && !caps.wings) || (bits & 0x8000 != 0 && !caps.parachute) {
            return;
        }
        if bits & 0x4 != 0 && parts::forms(o.id) < 2 {
            return;
        }
        let mut n = o.clone();
        n.flags ^= bits;
        if rules::counter(&n) != rules::counter(o) && self.check_limits(&n, 1).is_err() {
            return;
        }
        if let Some(o) = self.area_mut().objects.items.get_mut(index) {
            o.flags ^= bits;
        }
    }

    /// Toggle the big size (size field 0x6000: 0x4000 big). Big parts store their doubled size
    /// and keep their bottom-left tile [corp: every big 1×1 enemy is stored 2×2].
    pub fn toggle_big(&mut self, index: usize) -> Result<(), EditError> {
        let Some(o) = self.object(index).cloned() else {
            return Err(EditError::Missing);
        };
        if !parts::caps(o.id).big {
            return Err(EditError::NotAllowed("can't be made big"));
        }
        let fp = parts::footprint(&o);
        let big = o.is_big();
        let (w, h) = if big {
            ((o.width / 2).max(1), (o.height / 2).max(1))
        } else {
            (o.width.max(1) * 2, o.height.max(1) * 2)
        };
        let mut n = parts::new_object(o.id, fp.x, fp.y, w, h, o.flags);
        n.flags = (o.flags & !0x6000) | if big { 0 } else { 0x4000 };
        n.child_id = o.child_id;
        n.child_flags = o.child_flags;
        n.extended_data = o.extended_data;
        n.link_id = o.link_id;
        n.sound_effect_id = o.sound_effect_id;
        self.check_replace(index, &n)?;
        self.area_mut().objects.items[index] = n;
        Ok(())
    }

    /// Resize a part (semisolids, mushroom platforms, bridges, pipes, conveyors, lifts, slopes,
    /// vines, Bill Blasters, seesaws, trampolines, snake blocks, trees …) to `w`×`h` tiles within
    /// the ranges the game allows, keeping its bottom-left tile. Slopes keep their shape: the
    /// width follows the height (gentle 2h, steep h + 1). Pipes: `h` is the length.
    pub fn resize(&mut self, index: usize, w: u8, h: u8) -> Result<(), EditError> {
        let Some(o) = self.object(index).cloned() else {
            return Err(EditError::Missing);
        };
        let Some((wr, hr)) = parts::resize_range(o.id) else {
            return Err(EditError::NotAllowed("this part has a fixed size"));
        };
        if o.id == 9 {
            let dir = parts::Dir::from_flags(o.flags);
            return self.set_pipe(index, h, dir, ((o.flags >> 18) & 3) as u8);
        }
        let w = match o.id {
            87 => h.saturating_mul(2),
            88 | 94 => h.saturating_add(1),
            _ => w,
        };
        if !wr.contains(&w) || !hr.contains(&h) {
            return Err(EditError::NotAllowed("size out of range"));
        }
        let fp = parts::footprint(&o);
        let mut n = parts::new_object(o.id, fp.x, fp.y, w, h, o.flags);
        n.child_id = o.child_id;
        n.child_flags = o.child_flags;
        n.extended_data = o.extended_data;
        n.link_id = o.link_id;
        n.sound_effect_id = o.sound_effect_id;
        self.check_replace(index, &n)?;
        self.area_mut().objects.items[index] = n;
        Ok(())
    }

    /// Current area's theme and night flag.
    pub fn theme(&self) -> (u8, bool) {
        let h = &self.area().header;
        (h.theme, h.flags & 2 != 0)
    }

    /// Put a part inside another (a ? block's item, a Bill Blaster's enemy, a tree's fruit): its
    /// child id. Pipes take their contents as a separate part (`put_in_pipe`). −1 empties it.
    pub fn set_child(&mut self, index: usize, part: i16, child_flags: u32) {
        let _ = self.try_set_child(index, part, child_flags);
    }

    /// `set_child` with the game's rule check: only containers take children, and only parts the
    /// game lets go into something.
    pub fn try_set_child(
        &mut self,
        index: usize,
        part: i16,
        child_flags: u32,
    ) -> Result<(), EditError> {
        let Some(o) = self.object(index) else {
            return Err(EditError::Missing);
        };
        if part >= 0 {
            rules::check_child(o.id, part)?;
        }
        let o = &mut self.area_mut().objects.items[index];
        o.child_id = part;
        // no child: the default flags stay in the slot (the game rejects 0 there)
        o.child_flags = if part >= 0 {
            child_flags
        } else {
            DEFAULT_FLAGS
        };
        Ok(())
    }

    /// Erase what is at a tile: a part first, then a track piece, else ground. True if
    /// something was removed.
    pub fn erase(&mut self, tx: i32, ty: i32) -> bool {
        if let Some(i) = self.object_at(tx, ty) {
            self.erase_object(i);
            return true;
        }
        if self.erase_track_at(tx, ty) {
            return true;
        }
        self.erase_ground(tx, ty)
    }

    /// Erase a part and what belongs to it: its riders (pipe contents, clown-car passengers …),
    /// its path record, a door's or warp box's partner; parts riding a track piece stay.
    pub fn erase_object(&mut self, index: usize) {
        let Some(o) = self.object(index).cloned() else {
            return;
        };
        let mut gone = vec![index];
        // riders share the carrier's link id
        if o.link_id >= 0 && rules::is_carrier(o.id) {
            for (i, r) in self.area().objects.iter().enumerate() {
                if i != index && r.link_id == o.link_id && rules::is_rider(r) {
                    gone.push(i);
                }
            }
        }
        // a pair of doors / warp boxes goes together
        if let Some(j) = self.partner(index) {
            gone.push(j);
        }
        for &i in &gone {
            let p = self.area().objects.items[i].clone();
            self.free_path(&p);
            self.release_links(&p);
        }
        gone.sort_unstable();
        gone.dedup();
        for i in gone.into_iter().rev() {
            self.area_mut().objects.items.remove(i);
        }
        self.refresh_track_riders();
    }

    // ------------------------------------------------------------ ground

    /// Paint one ground tile (re-tiling its neighbours).
    pub fn paint_ground(&mut self, tx: i32, ty: i32) -> Result<(), EditError> {
        if !self.in_bounds(tx, ty) || tx > 255 || ty > 255 {
            return Err(EditError::OutOfBounds);
        }
        if self.ground_at(tx, ty).is_some() {
            return Ok(());
        }
        self.check_ground(tx, ty)?;
        let mut g = GroundTile::parse(&[0u8; 4]);
        g.x = tx as u8;
        g.y = ty as u8;
        self.area_mut().ground.items.push(g);
        self.retile(tx, ty);
        Ok(())
    }

    pub fn erase_ground(&mut self, tx: i32, ty: i32) -> bool {
        let Some(i) = self.ground_at(tx, ty) else {
            return false;
        };
        self.area_mut().ground.items.remove(i);
        self.retile(tx, ty);
        true
    }

    /// Recompute the graphic id of the tile and its 8 neighbours.
    pub(crate) fn retile(&mut self, tx: i32, ty: i32) {
        const N: [(i32, i32); 8] = [
            (0, 1),
            (1, 1),
            (1, 0),
            (1, -1),
            (0, -1),
            (-1, -1),
            (-1, 0),
            (-1, 1),
        ];
        for dx in -1..=1 {
            for dy in -1..=1 {
                let (x, y) = (tx + dx, ty + dy);
                let Some(i) = self.ground_at(x, y) else {
                    continue;
                };
                let mut mask = 0usize;
                for (k, (nx, ny)) in N.iter().enumerate() {
                    if self.ground_at(x + nx, y + ny).is_some() {
                        mask |= 1 << k;
                    }
                }
                self.area_mut().ground.items[i].id = autotile::GROUND_ID[mask];
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    /// Save, re-open and compare; the course must also play with nothing unsupported. Returns
    /// the re-opened course.
    pub fn round_trip(e: &Editor) -> Course {
        let bytes = e.save().unwrap();
        let back = Course::parse(&bytes).unwrap();
        assert!(back == e.course, "the saved course re-opens identically");
        assert_eq!(back.to_bytes().unwrap(), bytes);
        let missing = mmk_sim::level::unsupported_features(&back);
        assert!(missing.is_empty(), "unsupported: {missing:?}");
        back
    }

    /// A flat test course: ground rows 0–1 across the main area.
    pub fn flat(style: &str) -> Editor {
        let mut e = Editor::new(style);
        let (w, _) = e.size();
        for x in 0..w {
            for y in 0..2 {
                if !e.protected(x, y) {
                    e.paint_ground(x, y).unwrap();
                }
            }
        }
        e
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    #[test]
    fn place_erase_and_occupancy() {
        let mut e = Editor::new("M1");
        let i = e.place(0, 10, 3).unwrap();
        assert_eq!(e.object_at(10, 3), Some(i));
        assert_eq!(e.place(5, 10, 3), Err(EditError::Occupied));
        let p = e.place(9, 20, 2).unwrap();
        assert_eq!(e.object_at(21, 3), Some(p), "pipes are 2×2");
        assert!(e.erase(10, 3));
        assert_eq!(e.object_at(10, 3), None);
        assert_eq!(e.place(0, -1, 3), Err(EditError::OutOfBounds));
    }

    #[test]
    fn ground_auto_tiles_and_undo_redo() {
        let mut e = Editor::new("M1");
        e.checkpoint();
        for x in 15..18 {
            for y in 2..5 {
                e.paint_ground(x, y).unwrap();
            }
        }
        let id = |e: &Editor, x: i32, y: i32| {
            e.area()
                .ground
                .iter()
                .find(|g| g.x as i32 == x && g.y as i32 == y)
                .map(|g| g.id)
        };
        assert_eq!(id(&e, 16, 3), Some(autotile::GROUND_ID[255]), "interior");
        assert_eq!(
            id(&e, 16, 4),
            Some(autotile::GROUND_ID[0b0111_1100]),
            "top edge"
        );
        assert!(e.undo());
        assert!(e.area().ground.is_empty());
        assert!(e.redo());
        assert_eq!(e.area().ground.len(), 9);
        e.erase(16, 4);
        assert_eq!(
            id(&e, 16, 3),
            Some(autotile::GROUND_ID[0b0111_1100]),
            "now the top"
        );
    }

    #[test]
    fn saved_courses_play_in_the_engine() {
        let mut e = Editor::new("M3");
        e.set_name("Made here");
        e.set_theme(6);
        for x in 7..40 {
            e.paint_ground(x, 0).unwrap();
        }
        let q = e.place(5, 12, 4).unwrap();
        e.set_child(q, 34, 0); // a Fire Flower inside
        e.place(0, 18, 1).unwrap();
        let back = round_trip(&e);
        assert_eq!(back.header.name.to_string_lossy(), "Made here");
        let lvl = mmk_sim::level::Level::from_course(&back, 0);
        assert_eq!(lvl.theme, 6);
        assert!(
            lvl.spawns
                .iter()
                .any(|s| s.kind == mmk_sim::actors::Kind::Goomba)
        );
        assert!(lvl.blocks.iter().any(|b| b.x == 12 && b.y == 4));
    }

    #[test]
    fn resizable_parts_play_at_their_size() {
        use mmk_sim::level::{Cell, Level};
        let mut e = flat("M1");
        let semi = e.place(16, 10, 2).unwrap();
        e.resize(semi, 6, 4).unwrap();
        let mush = e.place(14, 20, 2).unwrap();
        e.resize(mush, 5, 6).unwrap();
        let bridge = e.place(17, 30, 6).unwrap();
        e.resize(bridge, 8, 2).unwrap();
        assert!(e.resize(bridge, 8, 3).is_err(), "bridges are 2 tall");
        let belt = e.place(53, 40, 4).unwrap();
        e.resize(belt, 7, 1).unwrap();
        let lift = e.place(11, 50, 6).unwrap();
        e.resize(lift, 6, 1).unwrap();
        assert!(e.resize(lift, 9, 1).is_err(), "lifts are at most 8 wide");
        let slope = e.place(88, 60, 2).unwrap();
        e.resize(slope, 0, 4).unwrap();
        assert_eq!(
            e.object(slope).unwrap().width,
            5,
            "steep slopes stay h + 1 wide"
        );
        let vine = e.place(64, 70, 2).unwrap();
        e.resize(vine, 1, 9).unwrap();
        let pipe = e.place(9, 80, 2).unwrap();
        e.resize(pipe, 2, 6).unwrap();
        assert!(
            e.resize(e.object_at(10, 2).unwrap(), 1, 1).is_err(),
            "not below 3 wide"
        );
        // a wider semisolid can't grow into the mushroom platform's stem
        assert!(e.resize(semi, 3, 27).is_err());
        let back = round_trip(&e);
        let lvl = Level::from_course(&back, 0);
        assert_eq!(lvl.cell(15, 5), Cell::SemiTop, "semisolid top row, 6 wide");
        assert_eq!(lvl.cell(16, 5), Cell::Empty);
        assert_eq!(lvl.cell(24, 7), Cell::SemiTop, "mushroom cap");
        assert_eq!(lvl.cell(37, 7), Cell::SemiTop, "bridge deck");
        assert!(lvl.conveyors.iter().any(|c| c.0 == 40 && c.1 == 46));
        assert_eq!(lvl.cell(80, 7), Cell::Solid, "6-tile pipe");
        assert!(lvl.vines.iter().any(|v| v.2 - v.1 == 9.0 * 16.0));
        // a 5 × 4 steep slope (its flat end columns are part of the slope)
        assert!(lvl.slopes.iter().any(|s| s.width == 5 && s.height == 4));
    }

    #[test]
    fn big_parts_store_their_doubled_size() {
        let mut e = flat("M1");
        let g = e.place(0, 20, 2).unwrap();
        e.toggle_big(g).unwrap();
        let o = e.object(g).unwrap();
        assert_eq!((o.width, o.height, o.is_big()), (2, 2, true));
        assert_eq!(
            parts::footprint(o),
            Rect::new(20, 2, 2, 2),
            "keeps its bottom-left"
        );
        assert_eq!(
            o.x,
            (20 * 16 + 16) * 10,
            "x on the tile edge, like the sample courses"
        );
        let brick = e.place(4, 30, 4).unwrap();
        assert!(e.toggle_big(brick).is_err(), "bricks don't grow");
        let back = round_trip(&e);
        let lvl = mmk_sim::level::Level::from_course(&back, 0);
        assert!(lvl.spawns.iter().any(|s| s.big));
    }
}
