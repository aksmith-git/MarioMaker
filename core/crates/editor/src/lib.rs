//! Course Maker core (Phase 9): an editable SMM2 course with the operations the maker UI needs —
//! place and erase parts, paint ground (auto-tiled), course and area settings, undo/redo — and
//! the course file it saves. No UI here; the shells draw the editor with the engine.
//!
//! Coordinates are tiles of the current area (x right, y up; tile (0, 0) is the bottom-left).
//! Parts are stored the way courses store them (docs/formats/smm2-course.md): positions in
//! 1/10 px at the centre of the bottom-left tile, default flags 0x06000040.

pub mod autotile;

use mmk_formats::record::Record;
use mmk_formats::smm2::course::{Area, Course, CourseObject, GroundTile};

/// Default placement flags of a course object.
pub const DEFAULT_FLAGS: u32 = 0x0600_0040;
/// Undo depth (each step is a full course snapshot, ≈ 370 KB).
pub const UNDO_DEPTH: usize = 64;

/// Size in tiles a freshly placed part gets (most parts 1×1).
pub fn default_size(part: i16) -> (u8, u8) {
    match part {
        9 => (2, 2),            // pipe: 2 wide, height = length
        16 | 17 | 14 => (3, 1), // semisolid, bridge, mushroom platform
        11 => (3, 1),           // lift
        12 => (2, 2),           // Thwomp
        13 => (1, 2),           // Bill Blaster
        53 => (3, 1),           // conveyor
        62 | 77 | 98 => (2, 2), // Bowser, Boom Boom, Bowser Jr
        42 => (2, 2),           // Koopa Clown Car
        67 => (2, 2),           // one-way wall
        90 => (2, 2),           // checkpoint flag
        _ => (1, 1),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    OutOfBounds,
    Occupied,
    Full,
    Format(String),
}

pub struct Editor {
    pub course: Course,
    /// 0 main area, 1 sub area.
    pub area: usize,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
}

impl Editor {
    /// A new, empty course in `style` ("M1", "M3", "MW", "WU", "3W").
    pub fn new(style: &str) -> Self {
        let mut course = Course::blank(style);
        course.header.name.set("New course");
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

    fn area_mut(&mut self) -> &mut Area {
        if self.area == 0 {
            &mut self.course.main
        } else {
            &mut self.course.sub
        }
    }

    /// Area size in tiles (from its right and top boundaries).
    pub fn size(&self) -> (i32, i32) {
        let h = &self.area().header;
        (h.boundary_right / 16, h.boundary_top / 16)
    }

    fn in_bounds(&self, tx: i32, ty: i32) -> bool {
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

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ------------------------------------------------------------ parts

    /// Tile rectangle (left, bottom, width, height) a part covers.
    fn footprint(o: &CourseObject) -> (i32, i32, i32, i32) {
        let big = o.flags & 0x6000 == 0x4000;
        let (w, h) = (o.width.max(1) as i32, o.height.max(1) as i32);
        let (w, h) = if big && w == 1 && h == 1 {
            (2, 2)
        } else {
            (w, h)
        };
        let left = ((o.x as f32 / 10.0 - 8.0) / 16.0).round() as i32;
        let bottom = ((o.y as f32 / 10.0 - 8.0) / 16.0).round() as i32;
        (left, bottom, w, h)
    }

    /// Index of the part covering a tile (the last placed wins).
    pub fn object_at(&self, tx: i32, ty: i32) -> Option<usize> {
        self.area()
            .objects
            .iter()
            .enumerate()
            .rev()
            .find(|(_, o)| {
                let (l, b, w, h) = Self::footprint(o);
                (l..l + w).contains(&tx) && (b..b + h).contains(&ty)
            })
            .map(|(i, _)| i)
    }

    fn ground_at(&self, tx: i32, ty: i32) -> Option<usize> {
        self.area()
            .ground
            .iter()
            .position(|g| g.x as i32 == tx && g.y as i32 == ty)
    }

    /// Place a part with its default size at a free tile.
    pub fn place(&mut self, part: i16, tx: i32, ty: i32) -> Result<usize, EditError> {
        let (w, h) = default_size(part);
        self.place_sized(part, tx, ty, w, h, DEFAULT_FLAGS)
    }

    /// Place a part covering `w`×`h` tiles from (tx, ty) (bottom-left).
    pub fn place_sized(
        &mut self,
        part: i16,
        tx: i32,
        ty: i32,
        w: u8,
        h: u8,
        flags: u32,
    ) -> Result<usize, EditError> {
        for dx in 0..w as i32 {
            for dy in 0..h as i32 {
                let (x, y) = (tx + dx, ty + dy);
                if !self.in_bounds(x, y) {
                    return Err(EditError::OutOfBounds);
                }
                if self.object_at(x, y).is_some() || self.ground_at(x, y).is_some() {
                    return Err(EditError::Occupied);
                }
            }
        }
        if self.area().objects.len() >= 2600 {
            return Err(EditError::Full);
        }
        let mut o = CourseObject::parse(&[0u8; 0x20]);
        o.x = (tx * 16 + 8) * 10;
        o.y = (ty * 16 + 8) * 10;
        o.width = w;
        o.height = h;
        o.flags = flags;
        o.id = part;
        o.child_id = -1;
        o.link_id = -1;
        o.sound_effect_id = -1;
        let objects = &mut self.area_mut().objects.items;
        objects.push(o);
        Ok(objects.len() - 1)
    }

    /// Change a placed part's flags (wings 0x2, alternate form 0x4, big 0x4000, …).
    pub fn set_flags(&mut self, index: usize, flags: u32) {
        if let Some(o) = self.area_mut().objects.items.get_mut(index) {
            o.flags = flags;
        }
    }

    /// Put a part inside another (a ? block's item, a pipe's enemy): its child id.
    pub fn set_child(&mut self, index: usize, part: i16, child_flags: u32) {
        if let Some(o) = self.area_mut().objects.items.get_mut(index) {
            o.child_id = part;
            o.child_flags = child_flags;
        }
    }

    /// Erase what is at a tile: a part first, else ground. True if something was removed.
    pub fn erase(&mut self, tx: i32, ty: i32) -> bool {
        if let Some(i) = self.object_at(tx, ty) {
            self.area_mut().objects.items.remove(i);
            return true;
        }
        self.erase_ground(tx, ty)
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
        if self.object_at(tx, ty).is_some() {
            return Err(EditError::Occupied);
        }
        if self.area().ground.len() >= 4000 {
            return Err(EditError::Full);
        }
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
    fn retile(&mut self, tx: i32, ty: i32) {
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

    // ------------------------------------------------------------ settings

    pub fn set_name(&mut self, name: &str) {
        self.course.header.name.set(name);
    }

    pub fn set_description(&mut self, text: &str) {
        self.course.header.description.set(text);
    }

    /// Game style code: "M1", "M3", "MW", "WU", "3W".
    pub fn set_style(&mut self, style: &str) {
        self.course.header.game_style.set(style);
    }

    /// Time limit in seconds (0 = none).
    pub fn set_timer(&mut self, seconds: i16) {
        self.course.header.timer = seconds;
    }

    /// Theme of the current area: 0 ground … 9 forest.
    pub fn set_theme(&mut self, theme: u8) {
        self.area_mut().header.theme = theme;
    }

    /// Night version of the current area's theme.
    pub fn set_night(&mut self, night: bool) {
        let h = &mut self.area_mut().header;
        h.flags = if night { h.flags | 2 } else { h.flags & !2 };
    }

    /// Autoscroll of the current area: 0 none, 1 slow, 2 normal, 3 fast.
    pub fn set_autoscroll(&mut self, speed: u8) {
        self.area_mut().header.autoscroll_type = speed.min(3);
    }

    /// Height (tiles) of the start and goal ground.
    pub fn set_start_goal(&mut self, start_y: u8, goal_y: u8) {
        self.course.header.start_y = start_y;
        self.course.header.goal_y = goal_y;
    }

    /// Main-area width in tiles (30–240): the right boundary and the goal position (the header
    /// stores goal x in tenths of a tile, 2400 for a full-width course [inf from the blank
    /// course layout]).
    pub fn set_length(&mut self, tiles: i32) {
        let tiles = tiles.clamp(30, 240);
        self.course.header.goal_x = (tiles * 10) as i16;
        self.course.main.header.boundary_right = tiles * 16;
    }
}

#[cfg(test)]
mod tests {
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
        for x in 5..8 {
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
        assert_eq!(id(&e, 6, 3), Some(autotile::GROUND_ID[255]), "interior");
        assert_eq!(
            id(&e, 6, 4),
            Some(autotile::GROUND_ID[0b0111_1100]),
            "top edge"
        );
        assert!(e.undo());
        assert!(e.area().ground.is_empty());
        assert!(e.redo());
        assert_eq!(e.area().ground.len(), 9);
        e.erase(6, 4);
        assert_eq!(
            id(&e, 6, 3),
            Some(autotile::GROUND_ID[0b0111_1100]),
            "now the top"
        );
    }

    #[test]
    fn saved_courses_play_in_the_engine() {
        let mut e = Editor::new("M3");
        e.set_name("Made here");
        e.set_theme(6);
        for x in 0..40 {
            e.paint_ground(x, 0).unwrap();
        }
        let q = e.place(5, 12, 4).unwrap();
        e.set_child(q, 34, 0); // a Fire Flower inside
        e.place(0, 18, 1).unwrap();
        let bytes = e.save().unwrap();
        let back = Editor::open(&bytes).unwrap();
        assert_eq!(back.course.header.name.to_string_lossy(), "Made here");
        let lvl = mmk_sim::level::Level::from_course(&back.course, 0);
        assert_eq!(lvl.theme, 6);
        assert!(
            lvl.spawns
                .iter()
                .any(|s| s.kind == mmk_sim::actors::Kind::Goomba)
        );
        assert!(lvl.blocks.iter().any(|b| b.x == 12 && b.y == 4));
        assert!(mmk_sim::level::unsupported_features(&back.course).is_empty());
    }
}
