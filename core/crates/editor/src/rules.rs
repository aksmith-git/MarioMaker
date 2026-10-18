//! Placement rules: what may overlap what, the protected start and goal zones, which parts go
//! inside which containers, part limits, and parts a game style doesn't have.
//!
//! Sources (research/specs/maker.md): the editor part table (EditDB) for composition, the game's
//! code for limits and zones where it was traced, and the 550 sample courses for everything the
//! files show (overlaps, containers in use, maxima per area).

use mmk_formats::smm2::course::CourseObject;

use crate::{EditError, Editor, Rect, parts};

/// Riders don't occupy tiles: parts in a pipe (flag 0x1), in a Koopa Clown Car (0x200), on a
/// Lakitu's Cloud (0x10000 / 0x20000) or held by a swinging claw (0x8000000). They share the
/// carrier's link id [corp].
pub fn is_rider(o: &CourseObject) -> bool {
    o.flags & (0x1 | 0x200 | 0x3_0000 | 0x800_0000) != 0
}

/// Parts that carry riders through a shared link id: pipe, Lakitu's Cloud, Koopa Clown Car,
/// swinging claw. Every one of them has a link id in the sample courses, rider or not [corp].
pub fn is_carrier(part: i16) -> bool {
    matches!(part, 9 | 31 | 42 | 105)
}

/// Parts that get a link id when placed.
pub fn needs_link_id(part: i16) -> bool {
    is_carrier(part)
}

/// Semisolid-like parts are drawn behind everything: other parts may overlap them and they may
/// overlap anything, ground included (semisolids overlap ground in 637 of 1129 sample placements,
/// hard blocks sit on 1577 of them) [corp].
pub fn is_background(part: i16) -> bool {
    matches!(part, 14 | 16 | 17 | 71)
}

/// Parts that may sit in ground (but not in other parts): Monty Moles hide in walls, vines and
/// claws hang from ceilings, fire bars, burners, grinders, bumpers and one-way walls are often
/// set into terrain [corp: 7–60 % of their sample placements overlap ground].
pub fn embeds_in_ground(part: i16) -> bool {
    matches!(part, 24 | 54 | 64 | 67 | 68 | 82 | 102 | 105)
}

/// Parts the editor never places as objects: ground (7) is painted, start / goal parts (26, 27,
/// 37, 38) and the player (69) come from the header, tracks (59) are their own records, clear
/// pipes (93) are paths, sound effects (109) and icicles (118) have their own arrays. Ids above
/// 132 don't exist.
pub fn placeable(part: i16) -> bool {
    (0..=132).contains(&part) && !matches!(part, 7 | 26 | 27 | 37 | 38 | 59 | 69 | 93 | 109 | 118)
}

/// Parts a game style lacks: the course validators' style checks [obs], plus parts no sample
/// course ever uses in that style (45, 52, 105 never in 3D World; 97 only there) [corp].
pub fn style_allows(style: &str, part: i16) -> bool {
    const W3_ONLY: &[i16] = &[
        50, 51, 65, 71, 72, 85, 86, 93, 96, 97, 106, 107, 108, 110, 112, 113, 114, 116, 117, 119,
        // 3.0: P Block, Dash Block, ON/OFF Trampoline; the wearables (their put-on works only
        // in 3D World, 0x71015c53e0, and only 3D World has their models) [obs]
        79, 80, 132, 127, 128, 129, 130, 131,
    ];
    const NOT_W3: &[i16] = &[
        14, 16, 17, 23, 24, 28, 30, 31, 36, 40, 42, 43, 45, 47, 52, 54, 57, 58, 59, 61, 64, 67, 68,
        82, 83, 91, 98, 100, 102, 104, 105,
        // 3.0: Mechakoopa and the Koopalings have no 3D World models
        111, 120, 121, 122, 123, 124, 125, 126,
    ];
    if style == "3W" {
        !NOT_W3.contains(&part)
    } else {
        !W3_ONLY.contains(&part)
    }
}

/// Items a checkpoint flag or a tree can hold.
const ITEMS: &[i16] = &[8, 10, 18, 19, 20, 33, 34, 35, 44, 64, 81, 95, 116];

/// Whether `child` may go inside `container` (its child id).
pub fn check_child(container: i16, child: i16) -> Result<(), EditError> {
    let caps = parts::caps(child);
    let ok = match container {
        // blocks, Bill Blasters, Lakitu (what he throws), crates: anything that can be put
        // into something [EditDB], plus the key
        4 | 5 | 23 | 29 | 13 | 30 | 112 => caps.inside || child == 95,
        // checkpoint flags and trees hold items [corp]
        90 | 106 => ITEMS.contains(&child),
        // enemies and bosses can hold a key, dropped when defeated
        c => parts::caps(c).key && child == 95,
    };
    if ok {
        Ok(())
    } else {
        Err(EditError::NotAllowed("can't go in there"))
    }
}

/// Per-area part counters and their limits, from the editor's placement check [obs 0x710057f940:
/// a part is refused when its counter is at the limit; each area has its own counters, there are
/// none per course]. Every limit is reached exactly by some sample course and never exceeded
/// [corp]. Parts in blocks, Bill Blasters, Lakitus, trees and checkpoints aren't counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counter {
    /// Enemies, gizmos, lifts, keys, springs …, and blocks or coins with wings, a parachute or a
    /// link (track, carrier).
    General,
    PowerUps,
    /// Plain blocks and coins.
    Blocks,
    /// Pipes, semisolids, platforms, bridges, belts, vines, slopes.
    Terrain,
    /// Arrow signs and one-way walls share one.
    Signs,
    /// Bowser and Bowser Jr. share one.
    Bowsers,
    /// Its own counter (part id).
    Part(i16),
}

const POWER_UPS: &[i16] = &[20, 33, 34, 35, 44, 75, 116];
const BLOCKS: &[i16] = &[4, 5, 6, 8, 21, 22, 23, 29, 43, 63, 100, 108, 110];
const TERRAIN: &[i16] = &[9, 14, 16, 17, 53, 64, 71, 87, 88, 94];

/// The counter a placed part uses.
pub fn counter(o: &CourseObject) -> Counter {
    match o.id {
        p if POWER_UPS.contains(&p) => Counter::PowerUps,
        p if BLOCKS.contains(&p) && o.flags & (0x2 | 0x8000) == 0 && o.link_id < 0 => {
            Counter::Blocks
        }
        p if TERRAIN.contains(&p) => Counter::Terrain,
        66 | 67 => Counter::Signs,
        62 | 98 => Counter::Bowsers,
        p @ (55 | 70 | 72 | 77 | 84 | 85 | 86 | 89 | 90 | 92 | 97 | 104 | 107 | 119) => {
            Counter::Part(p)
        }
        _ => Counter::General,
    }
}

/// A counter's limit and the name shown when it's reached.
pub fn limit(c: Counter) -> (usize, &'static str) {
    match c {
        Counter::General => (100, "enemies and gizmos"),
        Counter::PowerUps => (100, "power-ups"),
        Counter::Blocks => (2000, "blocks and coins"),
        Counter::Terrain => (200, "terrain parts"),
        Counter::Signs => (100, "signs and one-way walls"),
        Counter::Bowsers => (3, "Bowsers"),
        Counter::Part(p) => match p {
            90 => (1, "checkpoint flags"),
            104 => (1, "Angry Suns"),
            92 => (5, "pink coins"),
            70 => (5, "big coins"),
            55 => (8, "doors"),
            97 => (8, "warp boxes"),
            77 => (5, "Boom Booms"),
            86 => (3, "Charvaarghs"),
            72 => (10, "Koopa Troopa Cars"),
            84 => (5, "snake blocks"),
            89 => (10, "scroll nodes"),
            _ => (10, "path blocks"),
        },
    }
}

impl Editor {
    /// Whether a tile is in the start or goal zone of the main area: the start ground (x 0–6)
    /// with the three rows above it where the player and the start sign stand, and the goal
    /// ground [corp: of 550 courses, no ground tile and 1–2 parts ever sit there, while the
    /// rows just above / beside hold hundreds].
    pub fn protected(&self, x: i32, y: i32) -> bool {
        if self.area != 0 {
            return false;
        }
        let h = &self.course.header;
        let (start, goal) = self.zones();
        (start.contains(x, y) && y < h.start_y as i32 + START_ABOVE)
            || (goal.contains(x, y) && y < h.goal_y as i32)
    }

    /// The start and goal columns (whole height; `protected` limits the rows).
    pub fn zones(&self) -> (Rect, Rect) {
        let right = self.course.main.header.boundary_right;
        // goal ground: 11 tiles from (right − 152 px − 8) / 16 [course-loading.md §1.5]
        let goal_left = ((right - 160) as f32 / 16.0).round() as i32;
        (
            Rect::new(0, 0, START_W, 256),
            Rect::new(goal_left, 0, 11, 256),
        )
    }

    /// Check a new part against the rules (no part is ignored).
    pub(crate) fn check_new(&self, o: &CourseObject) -> Result<(), EditError> {
        self.check_object(o, None)
    }

    /// Check a part replacing the one at `index` (resizing, growing, moving).
    pub(crate) fn check_replace(&self, index: usize, o: &CourseObject) -> Result<(), EditError> {
        self.check_object(o, Some(index))
    }

    fn check_object(&self, o: &CourseObject, skip: Option<usize>) -> Result<(), EditError> {
        if !placeable(o.id) {
            return Err(EditError::NotAllowed("not a placeable part"));
        }
        let style = self.course.header.game_style.as_str_lossy();
        if !style_allows(&style, o.id) {
            return Err(EditError::NotAllowed("not in this game style"));
        }
        let cells = parts::cells(o);
        for &(x, y) in &cells {
            if !self.in_bounds(x, y) {
                return Err(EditError::OutOfBounds);
            }
        }
        if !is_rider(o) {
            for &(x, y) in &cells {
                if self.protected(x, y) {
                    return Err(EditError::Protected);
                }
            }
            if !is_background(o.id) {
                let embeds = embeds_in_ground(o.id);
                for &(x, y) in &cells {
                    if !embeds && self.ground_at(x, y).is_some() {
                        return Err(EditError::Occupied);
                    }
                    if self.solid_part_at(x, y, skip) {
                        return Err(EditError::Occupied);
                    }
                }
            }
        }
        if skip.is_none() {
            self.check_limits(o, 1)?;
        }
        Ok(())
    }

    /// A non-background, non-rider part covers the tile (`skip` ignored).
    pub(crate) fn solid_part_at(&self, x: i32, y: i32, skip: Option<usize>) -> bool {
        self.area().objects.iter().enumerate().any(|(i, p)| {
            Some(i) != skip && !is_rider(p) && !is_background(p.id) && parts::covers(p, x, y)
        })
    }

    /// Can ground go on this tile?
    pub(crate) fn check_ground(&self, x: i32, y: i32) -> Result<(), EditError> {
        if self.protected(x, y) {
            return Err(EditError::Protected);
        }
        let blocked = self.area().objects.iter().any(|p| {
            !is_rider(p)
                && !is_background(p.id)
                && !embeds_in_ground(p.id)
                && parts::covers(p, x, y)
        });
        if blocked {
            return Err(EditError::Occupied);
        }
        if self.area().ground.len() >= GROUND_MAX {
            return Err(EditError::Limit("ground"));
        }
        Ok(())
    }

    /// Room for `n` more parts like `o` in the current area?
    pub(crate) fn check_limits(&self, o: &CourseObject, n: usize) -> Result<(), EditError> {
        if self.area().objects.len() + n > OBJECTS_MAX {
            return Err(EditError::Full);
        }
        let c = counter(o);
        let have = self
            .area()
            .objects
            .iter()
            .filter(|p| counter(p) == c)
            .count();
        let (max, what) = limit(c);
        if have + n > max {
            return Err(EditError::Limit(what));
        }
        Ok(())
    }
}

/// Width of the start column in tiles (start ground x 0–6 and the player above it).
pub const START_W: i32 = 7;
/// Rows above the start ground that stay clear.
pub const START_ABOVE: i32 = 3;
/// Objects per area (the file's object array).
pub const OBJECTS_MAX: usize = 2600;
/// Ground tiles per area (the file's ground array).
pub const GROUND_MAX: usize = 4000;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;

    #[test]
    fn overlap_rules() {
        let mut e = flat("M1");
        // semisolids go over ground and other parts sit on / in front of them
        let s = e
            .place_sized(16, 20, 1, 5, 4, crate::DEFAULT_FLAGS)
            .unwrap();
        e.place(6, 21, 3).unwrap();
        assert_eq!(e.object_at(21, 3).map(|i| e.object(i).unwrap().id), Some(6));
        assert_eq!(e.object_at(23, 3), Some(s));
        // enemies don't go into ground, Monty Moles do
        assert_eq!(e.place(0, 30, 1), Err(EditError::Occupied));
        e.place(102, 31, 1).unwrap();
        assert_eq!(e.place(0, 31, 1), Err(EditError::Occupied));
        // ground isn't painted over a block
        e.place(4, 40, 5).unwrap();
        assert_eq!(e.paint_ground(40, 5), Err(EditError::Occupied));
        round_trip(&e);
    }

    #[test]
    fn start_and_goal_are_protected() {
        let mut e = Editor::new("M1");
        assert_eq!(e.place(4, 3, 0), Err(EditError::Protected));
        assert_eq!(
            e.place(0, 5, 3),
            Err(EditError::Protected),
            "where the player stands"
        );
        e.place(0, 5, 4).unwrap();
        assert_eq!(e.paint_ground(2, 0), Err(EditError::Protected));
        let (w, _) = e.size();
        assert_eq!(e.place(4, w - 5, 0), Err(EditError::Protected));
        e.place(4, w - 5, 1).unwrap();
        e.place(4, 10, 0).unwrap();
        e.set_area(1);
        e.place(4, 3, 0).unwrap();
    }

    #[test]
    fn containers_take_what_the_game_allows() {
        let mut e = flat("M1");
        let q = e.place(5, 10, 5).unwrap();
        assert!(e.try_set_child(q, 20, 0).is_ok(), "mushroom in a ? block");
        assert!(e.try_set_child(q, 0, 0).is_ok(), "Goomba in a ? block");
        assert!(
            e.try_set_child(q, 6, 0).is_err(),
            "no hard block in a block"
        );
        let g = e.place(0, 14, 2).unwrap();
        assert!(e.try_set_child(g, 95, 0).is_ok(), "a Goomba can hold a key");
        assert!(e.try_set_child(g, 20, 0).is_err());
        let c = e.place(90, 30, 2).unwrap();
        assert!(
            e.try_set_child(c, 34, 0).is_ok(),
            "fire flower at the checkpoint"
        );
        assert!(e.try_set_child(c, 0, 0).is_err());
    }

    #[test]
    fn style_and_course_limits() {
        let mut e = flat("M1");
        assert!(matches!(e.place(96, 10, 2), Err(EditError::NotAllowed(_))));
        e.place(90, 20, 2).unwrap();
        assert_eq!(
            e.place(90, 30, 2),
            Err(EditError::Limit("checkpoint flags"))
        );
        e.set_area(1);
        e.place(90, 30, 0).unwrap();
        e.set_area(0);
        // 100 enemies per area; plain blocks have their own counter, winged ones don't
        for i in 0..100 {
            e.place(0, 30 + i, 2).unwrap();
        }
        assert_eq!(
            e.place(0, 150, 2),
            Err(EditError::Limit("enemies and gizmos"))
        );
        let b = e.place(4, 150, 6).unwrap();
        e.toggle_flags(b, 0x2);
        assert_eq!(
            e.object(b).unwrap().flags & 2,
            0,
            "no room for a winged block"
        );
        assert!(e.place(55, 160, 2).is_ok(), "doors have their own counter");
        let mut w = flat("3W");
        assert!(w.place(96, 10, 2).is_ok());
        assert!(matches!(w.place(43, 14, 2), Err(EditError::NotAllowed(_))));
        assert!(matches!(
            w.place_track(20, 6, 0),
            Err(EditError::NotAllowed(_))
        ));
    }
}
