//! What the editor knows about each part (course object id 0–132): the size a freshly placed part
//! gets, how far it can be resized, how its position is anchored, its tile footprint, how many
//! forms it has, and what it can carry or ride.
//!
//! Sizes, forms and capabilities come from the game's editor part table (EditDB, v3.0.3 data,
//! read from the user's own dump; data-params.md §2.3): the base footprint, the number of
//! alternate forms, and whether the part can wear wings or a parachute, hold a key, ride a track,
//! a Koopa Clown Car, a Lakitu's Cloud or a swinging claw, come out of a pipe, go inside a block,
//! be stacked, and grow with a Super Mushroom. Resize ranges and anchors were checked against the
//! 550 sample courses (research/specs/maker.md).

use std::ops::RangeInclusive;

use mmk_formats::record::Record;
use mmk_formats::smm2::course::CourseObject;

use crate::Rect;

pub const WINGS: u16 = 1;
pub const PARA: u16 = 2;
pub const KEY: u16 = 4;
pub const TRACK: u16 = 8;
pub const CLOWN: u16 = 16;
pub const CLOUD: u16 = 32;
pub const CLAW: u16 = 64;
pub const PIPE: u16 = 128;
pub const INSIDE: u16 = 256;
pub const STACK: u16 = 512;
pub const BIG: u16 = 1024;

#[derive(Clone, Copy)]
struct Part {
    w: u8,
    h: u8,
    forms: u8,
    caps: u16,
}

const fn p(w: u8, h: u8, forms: u8, caps: u16) -> Part {
    Part { w, h, forms, caps }
}

/// Indexed by part id.
#[rustfmt::skip]
const TABLE: [Part; 133] = [
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 0
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 1
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 2
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 3
    p(1, 1, 1, WINGS | KEY | TRACK), // 4
    p(1, 1, 1, WINGS | KEY | TRACK), // 5
    p(1, 1, 1, WINGS | TRACK), // 6
    p(1, 1, 1, 0), // 7
    p(1, 1, 2, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 8
    p(2, 2, 4, 0), // 9
    p(1, 1, 2, WINGS | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 10
    p(4, 1, 2, WINGS | TRACK), // 11
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | STACK | BIG), // 12
    p(1, 2, 2, KEY | TRACK | CLOWN | CLOUD | STACK), // 13
    p(3, 3, 3, 0), // 14
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 15
    p(3, 3, 3, 0), // 16
    p(3, 2, 1, 0), // 17
    p(1, 1, 1, WINGS | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 18
    p(1, 1, 2, WINGS | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 19
    p(1, 1, 2, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 20
    p(1, 1, 1, WINGS | TRACK), // 21
    p(1, 1, 1, WINGS | TRACK), // 22
    p(1, 1, 2, WINGS | KEY | TRACK), // 23
    p(1, 1, 1, TRACK), // 24
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 25
    p(1, 1, 1, 0), // 26: not placeable
    p(1, 1, 1, 0), // 27: not placeable
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 28
    p(1, 1, 1, WINGS | KEY | TRACK), // 29
    p(1, 2, 1, WINGS | KEY | TRACK | CLOUD), // 30
    p(1, 1, 2, WINGS | KEY | TRACK), // 31
    p(4, 4, 2, 0), // 32
    p(1, 1, 1, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 33
    p(1, 1, 2, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 34
    p(1, 1, 1, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 35
    p(4, 1, 2, TRACK), // 36
    p(1, 1, 1, 0), // 37: not placeable
    p(1, 1, 1, 0), // 38: not placeable
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 39
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 40
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | PIPE | INSIDE | STACK | BIG), // 41
    p(2, 2, 2, TRACK | PIPE | INSIDE), // 42
    p(1, 1, 1, 0), // 43
    p(1, 1, 1, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 44
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 45
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 46
    p(1, 1, 2, TRACK | CLOWN | CLOUD | STACK | BIG), // 47
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 48
    p(14, 1, 1, 0), // 49
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | BIG), // 50
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 51
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 52
    p(3, 1, 2, 0), // 53
    p(1, 1, 2, TRACK), // 54
    p(1, 2, 3, 0), // 55
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | BIG), // 56
    p(1, 1, 1, WINGS | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 57
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 58
    p(3, 3, 3, 0), // 59
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | BIG), // 60
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 61
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 62
    p(1, 1, 1, WINGS | TRACK), // 63
    p(1, 3, 1, INSIDE), // 64
    p(1, 1, 1, PARA | KEY | TRACK | CLAW | PIPE | INSIDE | STACK | BIG), // 65
    p(2, 2, 1, 0), // 66
    p(2, 2, 1, 0), // 67
    p(3, 3, 1, TRACK), // 68
    p(1, 1, 1, 0), // 69: not placeable
    p(2, 2, 3, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW), // 70
    p(3, 3, 1, 0), // 71
    p(2, 2, 1, 0), // 72
    p(1, 1, 1, 0), // 73
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 74
    p(1, 1, 1, 0), // 75
    p(1, 1, 1, 0), // 76
    p(2, 2, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 77
    p(1, 4, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW), // 78
    p(1, 1, 2, 0), // 79
    p(3, 1, 1, 0), // 80
    p(1, 1, 1, WINGS | PARA | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE), // 81
    p(3, 3, 1, TRACK), // 82
    p(4, 4, 1, 0), // 83
    p(5, 1, 2, 0), // 84
    p(2, 2, 2, 0), // 85
    p(4, 4, 1, KEY), // 86
    p(4, 2, 1, 0), // 87
    p(3, 2, 1, 0), // 88
    p(3, 1, 1, 0), // 89
    p(2, 2, 1, 0), // 90
    p(5, 1, 1, TRACK), // 91
    p(1, 1, 1, 0), // 92
    p(2, 2, 1, 0), // 93
    p(3, 2, 2, 0), // 94
    p(1, 1, 2, INSIDE), // 95
    p(1, 1, 2, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 96
    p(1, 1, 2, 0), // 97
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 98
    p(1, 1, 1, 0), // 99
    p(1, 1, 2, 0), // 100
    p(1, 1, 3, 0), // 101
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 102
    p(1, 1, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | BIG), // 103
    p(2, 2, 2, KEY), // 104
    p(3, 4, 1, TRACK), // 105
    p(4, 6, 1, 0), // 106
    p(2, 2, 2, 0), // 107
    p(1, 1, 2, 0), // 108
    p(1, 1, 1, 0), // 109
    p(2, 2, 3, 0), // 110
    p(1, 1, 3, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | PIPE | INSIDE | STACK | BIG), // 111
    p(1, 1, 1, KEY), // 112
    p(4, 1, 2, TRACK), // 113
    p(2, 2, 1, WINGS | PARA | KEY | BIG), // 114
    p(1, 1, 1, 0), // 115
    p(1, 1, 1, WINGS | PARA | TRACK | CLAW | PIPE | INSIDE), // 116
    p(2, 2, 1, WINGS | PARA | KEY | BIG), // 117
    p(1, 2, 2, 0), // 118
    p(2, 2, 1, 0), // 119
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 120
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 121
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 122
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 123
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 124
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 125
    p(2, 2, 1, WINGS | PARA | KEY | TRACK | CLOWN | CLOUD | CLAW | STACK | BIG), // 126
    p(1, 1, 1, WINGS | PARA | PIPE | INSIDE), // 127
    p(1, 1, 1, WINGS | PARA | PIPE | INSIDE), // 128
    p(1, 1, 1, WINGS | PARA | PIPE | INSIDE), // 129
    p(1, 1, 1, WINGS | PARA | PIPE | INSIDE), // 130
    p(1, 1, 1, WINGS | PARA | PIPE | INSIDE), // 131
    p(4, 1, 2, 0), // 132
];

fn row(part: i16) -> Option<&'static Part> {
    usize::try_from(part).ok().and_then(|i| TABLE.get(i))
}

/// What a part can carry or ride (EditDB composition columns).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub wings: bool,
    pub parachute: bool,
    /// Can hold a key (drops it when defeated / opened).
    pub key: bool,
    pub track: bool,
    pub clown: bool,
    pub cloud: bool,
    pub claw: bool,
    /// Can come out of a pipe.
    pub pipe: bool,
    /// Can be put inside a block (or another container).
    pub inside: bool,
    pub stack: bool,
    /// Grows with a Super Mushroom (the big size).
    pub big: bool,
}

pub fn caps(part: i16) -> Caps {
    let c = row(part).map_or(0, |r| r.caps);
    Caps {
        wings: c & WINGS != 0,
        parachute: c & PARA != 0,
        key: c & KEY != 0,
        track: c & TRACK != 0,
        clown: c & CLOWN != 0,
        cloud: c & CLOUD != 0,
        claw: c & CLAW != 0,
        pipe: c & PIPE != 0,
        inside: c & INSIDE != 0,
        stack: c & STACK != 0,
        big: c & BIG != 0,
    }
}

/// Number of forms the part cycles through (1 = no alternate form).
pub fn forms(part: i16) -> u8 {
    row(part).map_or(1, |r| r.forms)
}

/// Size in tiles a freshly placed part gets (the editor table's base footprint).
pub fn default_size(part: i16) -> (u8, u8) {
    // 2.0 / 3.0 parts aren't in the v1.0.0 table [v3.0.3 Edit* params]
    match part {
        78 => return (1, 4),        // Pokey: head + 3 (EditSambo BaseUnitSizeY 4)
        80 => return (3, 1),        // Dash Block
        120..=126 => return (2, 2), // Koopalings
        132 => return (4, 1),       // ON/OFF Trampoline
        _ => {}
    }
    row(part).map_or((1, 1), |r| (r.w, r.h))
}

/// Width and height ranges of a resizable part (None: fixed size), the bounds the game's course
/// validators accept [obs: per-part validators 0x7100565410–0x710056cc80; every sample course is
/// within them]. Pipes: width is the pipe's thickness (2) and height its length. Slopes keep their
/// shape (gentle w = 2h, steep w = h + 1); `Editor::resize` derives the width from the height.
pub fn resize_range(part: i16) -> Option<(RangeInclusive<u8>, RangeInclusive<u8>)> {
    Some(match part {
        9 => (2..=2, 2..=27),        // pipe
        16 | 71 => (3..=20, 3..=27), // semisolid, 3D World semisolid
        14 => (3..=9, 3..=27),       // mushroom platform
        17 => (3..=20, 2..=2),       // bridge (deck + rail row)
        53 => (3..=9, 1..=1),        // conveyor belt
        94 => (3..=9, 2..=8),        // conveyor slope
        87 => (4..=26, 2..=13),      // gentle slope
        88 => (3..=14, 2..=13),      // steep slope
        11 | 113 => (3..=8, 1..=1),  // lift, mushroom trampoline
        132 => (3..=8, 1..=1),       // ON/OFF trampoline (v3 part; as the mushroom one) [inf]
        91 => (3..=15, 1..=1),       // seesaw
        84 => (3..=12, 1..=1),       // snake block
        13 => (1..=1, 2..=14),       // Bill Blaster
        64 => (1..=1, 3..=27),       // vine
        106 => (4..=4, 6..=14),      // tree
        _ => return None,
    })
}

/// Parts whose (x, y) is the centre of their bottom-left tile; all others sit on their horizontal
/// centre. y is always the centre of the bottom row [corp: every sample course].
const LEFT_ANCHORED: &[i16] = &[9, 14, 16, 17, 53, 71, 87, 88, 94, 110];

pub fn left_anchored(part: i16) -> bool {
    LEFT_ANCHORED.contains(&part)
}

/// Pipe mouth direction, the course flag field (flags >> 5) & 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Right = 0,
    Left = 1,
    Up = 2,
    Down = 3,
}

impl Dir {
    pub fn from_flags(flags: u32) -> Self {
        match (flags >> 5) & 3 {
            0 => Dir::Right,
            1 => Dir::Left,
            2 => Dir::Up,
            _ => Dir::Down,
        }
    }

    pub fn flags(self) -> u32 {
        (self as u32) << 5
    }

    /// Unit step (x right, y up).
    pub fn step(self) -> (i32, i32) {
        match self {
            Dir::Right => (1, 0),
            Dir::Left => (-1, 0),
            Dir::Up => (0, 1),
            Dir::Down => (0, -1),
        }
    }
}

/// A pipe's bounding box from its base tile (the anchor the course stores), length and
/// direction: the base is the bottom-left tile of an up pipe and turns with the pipe (top-right
/// for down, bottom-right for left, top-left for right) — the layout the engine builds
/// (sim level.rs `add_pipe`).
pub fn pipe_rect(ax: i32, ay: i32, len: i32, dir: Dir) -> Rect {
    match dir {
        Dir::Up => Rect::new(ax, ay, 2, len),
        Dir::Down => Rect::new(ax - 1, ay - len + 1, 2, len),
        Dir::Left => Rect::new(ax - len + 1, ay, len, 2),
        Dir::Right => Rect::new(ax, ay - 1, len, 2),
    }
}

/// The base tile of a pipe whose bounding box starts at (bx, by).
pub fn pipe_anchor(bx: i32, by: i32, len: i32, dir: Dir) -> (i32, i32) {
    match dir {
        Dir::Up => (bx, by),
        Dir::Down => (bx + 1, by + len - 1),
        Dir::Left => (bx + len - 1, by),
        Dir::Right => (bx, by + 1),
    }
}

/// Tile rectangle a part covers (its bounding box).
pub fn footprint(o: &CourseObject) -> Rect {
    let big = o.flags & 0x6000 == 0x4000;
    let (w, h) = (o.width.max(1) as i32, o.height.max(1) as i32);
    // older files keep a big part's 1×1 size
    let (w, h) = if big && w == 1 && h == 1 {
        (2, 2)
    } else {
        (w, h)
    };
    let tile = |v: i32, half: f32| ((v as f32 / 10.0 - half) / 16.0).round() as i32;
    if o.id == 9 {
        return pipe_rect(tile(o.x, 8.0), tile(o.y, 8.0), h, Dir::from_flags(o.flags));
    }
    let half = if left_anchored(o.id) {
        8.0
    } else {
        w as f32 * 8.0
    };
    Rect::new(tile(o.x, half), tile(o.y, 8.0), w, h)
}

/// Whether a part covers a tile. Slopes cover only their diagonal strip (the cells below it are
/// ground tiles, the cells above are free) [corp: coins and enemies sit in the free half].
pub fn covers(o: &CourseObject, x: i32, y: i32) -> bool {
    let r = footprint(o);
    if !r.contains(x, y) {
        return false;
    }
    match o.id {
        87 | 88 | 94 => {
            let (cx, row) = (x - r.x, y - r.y);
            let rising = (o.flags >> 20) & 1 == 0;
            let col = if rising { cx } else { r.w - 1 - cx };
            let first = if o.id == 87 { 2 * row } else { row };
            (first..first + 2).contains(&col)
        }
        _ => true,
    }
}

/// The tiles a part covers.
pub fn cells(o: &CourseObject) -> Vec<(i32, i32)> {
    footprint(o)
        .tiles()
        .filter(|&(x, y)| covers(o, x, y))
        .collect()
}

/// A new course object covering `w`×`h` tiles from (tx, ty) (bottom-left of its bounding box;
/// for pipes `w` is ignored and `h` is the length, the direction coming from `flags`).
pub fn new_object(part: i16, tx: i32, ty: i32, w: u8, h: u8, flags: u32) -> CourseObject {
    let mut o = CourseObject::parse(&[0u8; 0x20]);
    let (ax, ay) = if part == 9 {
        pipe_anchor(tx, ty, h as i32, Dir::from_flags(flags))
    } else {
        (tx, ty)
    };
    // left-anchored parts sit on their left tile's centre, the rest on their horizontal centre
    o.x = if left_anchored(part) {
        (ax * 16 + 8) * 10
    } else {
        (ax * 16 + w as i32 * 8) * 10
    };
    o.y = (ay * 16 + 8) * 10;
    o.width = if part == 9 { 2 } else { w };
    o.height = h;
    o.flags = flags;
    // a part without a child still carries the default flags in its child slot (as in the
    // game's own courses); the game rejects a course whose objects have 0 there
    o.child_flags = crate::DEFAULT_FLAGS;
    o.id = part;
    o.child_id = -1;
    o.link_id = -1;
    o.sound_effect_id = -1;
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipes_turn_around_their_base_tile() {
        for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            let o = new_object(9, 10, 4, 2, 3, 0x0600_0000 | dir.flags());
            let r = footprint(&o);
            let want = match dir {
                Dir::Up | Dir::Down => Rect::new(10, 4, 2, 3),
                _ => Rect::new(10, 4, 3, 2),
            };
            assert_eq!(r, want, "{dir:?}");
        }
    }

    #[test]
    fn slopes_cover_their_diagonal_only() {
        let o = new_object(88, 0, 0, 3, 2, 0x0600_0040);
        assert_eq!(cells(&o), vec![(0, 0), (1, 0), (1, 1), (2, 1)]);
        let o = new_object(87, 0, 0, 4, 2, 0x0600_0040 | 1 << 20);
        assert_eq!(cells(&o), vec![(0, 1), (1, 1), (2, 0), (3, 0)]);
    }

    #[test]
    fn editor_table_agrees_with_known_parts() {
        assert_eq!(default_size(9), (2, 2));
        assert_eq!(default_size(55), (1, 2));
        assert!(caps(0).wings && caps(0).big && caps(0).pipe);
        assert!(!caps(6).inside, "hard blocks don't go in blocks");
        assert!(caps(20).inside && !caps(20).big);
        assert_eq!(forms(9), 4, "pipe colours");
    }
}
