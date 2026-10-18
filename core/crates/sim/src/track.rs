//! Tracks (59-track.md): the area's rail grid, built from the course's track records the way the
//! game builds it, and the rail follower that carries an object along it at 0.75 px/frame (1.5
//! with wings).
//!
//! Grid [obs 0x7100392210, 0x71003922c0, 0x7100391df0, 0x7100394600]: every record stamps a
//! template of tile codes into a grid of 16-px cells, 3×3 for types 0–7 and 5×5 for the junctions
//! 8–15, with the template's bottom-left cell on tile (x, y) (255 = −1). The record's end words
//! replace the template's end cells first: as one 32-bit word (k1 << 16 | k0), an ordinary piece
//! puts the high half on end A and the low half on end B; a junction takes three 10-bit fields
//! (bits 0–9 on A, 10–19 on B, 20 up on C). 0 keeps the template's cap, 0x104 clears the cell (the
//! neighbouring piece writes the shared cell). Each tile code maps to a rail cell word (class 2
//! ordinary, 3 junction; kind above), and pieces later in the record list overwrite earlier ones.
//!
//! Follower [obs 0x7100342f60 attach, 0x7100340720 TargetPointMove, 0x7100341280 look-ahead,
//! 0x7100341a00 cell read]: it walks from cell centre to cell centre. Looking ahead it passes
//! straight cells and heads for the first other cell; arriving there it leaves by the exit it did
//! not come in by (sharp turns at corner cells), turns back at a closed end (exit code 0x50) and
//! falls off at an open one (0x28). The 2×2 bends of types 4–7 are the only curves: a quarter
//! circle of radius 24 px about a cell corner, swept in 16-bit angle steps of 434.6·v per frame.
//! Junction cells follow the course-wide ON/OFF bit: some only exist in one state, others swap
//! their exits [obs tables 0x7101cfa6ec, 0x7101cfa87c / 0x7101cfab1c].

use std::collections::BTreeMap;

use crate::fmath::DetMath;

/// Normal and winged rider speeds, px/frame.
pub const SPEED: f32 = 0.75;
pub const SPEED_WINGED: f32 = 1.5;

const CELL: f32 = 16.0;
/// End word meaning "clear this cell".
const NO_CODE: u16 = 0x104;
/// Exit codes of end cells: turn back / fall off.
const CLOSED: u16 = 0x50;
const OPEN: u16 = 0x28;
/// Ordinary and junction kinds the follower knows (0x41 / 0x54 mean "no track").
const KINDS: usize = 65;
const JUNCTION_KINDS: usize = 84;
/// Radius of the bends (px) and the angle rate per px/frame of speed: 65536 / (2π·24)
/// [obs 0x7100340720, const 0x7101c8a740].
const BEND_RADIUS: f32 = 24.0;
const ARC_RATE: f32 = f32::from_bits(0x43d9_4cbb);
/// Snap distance at a target, as a fraction of the speed [obs 0x7100340720, const 0x7101c8a5ac].
const SNAP: f32 = f32::from_bits(0x3f33_3333);
/// Look-ahead limit in cells [obs 0x7100341280].
const LOOK_AHEAD: usize = 240;

/// Unit steps of the eight directions (×16 px), y up: 0 right, 1 up-right, 2 up, 3 up-left,
/// 4 left, 5 down-left, 6 down, 7 down-right [obs 0x7101cfa83c].
const DIRS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (1.0, 1.0),
    (0.0, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.0),
    (-1.0, -1.0),
    (0.0, -1.0),
    (1.0, -1.0),
];

/// The two ordinary-or-junction straight kinds a look-ahead passes through, per direction
/// [obs 0x7101c8ba80].
const STRAIGHT: [[u8; 2]; 8] = [
    [2, 24],
    [5, 27],
    [3, 25],
    [4, 26],
    [2, 24],
    [5, 27],
    [3, 25],
    [4, 26],
];

/// Piece templates, types 0–7: 3×3 tile codes, row 0 at the bottom (0x7101cfcab8).
const TEMPLATE_3: [[u16; 9]; 8] = [
    [0, 0, 0, 0x70, 0x60, 0x71, 0, 0, 0],
    [0, 0x73, 0, 0, 0x61, 0, 0, 0x72, 0],
    [0, 0, 0x75, 0, 0x62, 0, 0x74, 0, 0],
    [0x76, 0, 0, 0, 0x63, 0, 0, 0, 0x77],
    [0xae, 0xaf, 0x71, 0xac, 0xad, 0, 0x72, 0, 0],
    [0, 0, 0x73, 0, 0xb2, 0xb3, 0x70, 0xb0, 0xb1],
    [0x73, 0, 0, 0xaa, 0xab, 0, 0xa8, 0xa9, 0x71],
    [0x70, 0xb6, 0xb7, 0, 0xb4, 0xb5, 0, 0, 0x72],
];

/// Junction templates, types 8–15: 5×5 tile codes (0x7101cfcbd8, picked through 0x7101cfb460).
#[rustfmt::skip]
const TEMPLATE_5: [[u16; 25]; 8] = [
    [0, 0, 0, 0, 0x75, 0, 0, 0, 0x62, 0, 0x70, 0x60, 0x230, 0, 0, 0, 0, 0, 0x205, 0, 0, 0, 0, 0, 0x20a],
    [0x76, 0, 0, 0, 0, 0, 0x63, 0, 0, 0, 0, 0, 0x234, 0x60, 0x71, 0, 0x204, 0, 0, 0, 0x20b, 0, 0, 0, 0],
    [0, 0, 0x73, 0, 0, 0, 0, 0x61, 0, 0, 0, 0, 0x23c, 0, 0, 0, 0x62, 0, 0x205, 0, 0x74, 0, 0, 0, 0x20a],
    [0x76, 0, 0, 0, 0x208, 0, 0x63, 0, 0x204, 0, 0, 0, 0x238, 0, 0, 0, 0, 0x61, 0, 0, 0, 0, 0x72, 0, 0],
    [0, 0, 0, 0, 0x208, 0, 0, 0, 0x204, 0, 0x70, 0x60, 0x232, 0, 0, 0, 0, 0, 0x63, 0, 0, 0, 0, 0, 0x77],
    [0x209, 0, 0, 0, 0, 0, 0x205, 0, 0, 0, 0, 0, 0x236, 0x60, 0x71, 0, 0x62, 0, 0, 0, 0x74, 0, 0, 0, 0],
    [0, 0, 0x73, 0, 0, 0, 0, 0x61, 0, 0, 0, 0, 0x23e, 0, 0, 0, 0x204, 0, 0x63, 0, 0x20b, 0, 0, 0, 0x77],
    [0x209, 0, 0, 0, 0x75, 0, 0x205, 0, 0x62, 0, 0, 0, 0x23a, 0, 0, 0, 0, 0x61, 0, 0, 0, 0, 0x72, 0, 0],
];

/// Template cells (column, row) that take the record's end codes A, B and (junctions) C
/// (0x7101d000d8).
const END_CELLS: [[(usize, usize); 3]; 16] = [
    [(0, 1), (2, 1), (0, 0)],
    [(1, 0), (1, 2), (0, 0)],
    [(0, 2), (2, 0), (0, 0)],
    [(0, 0), (2, 2), (0, 0)],
    [(0, 2), (2, 0), (0, 0)],
    [(0, 2), (2, 0), (0, 0)],
    [(0, 0), (2, 2), (0, 0)],
    [(0, 0), (2, 2), (0, 0)],
    [(0, 2), (4, 0), (4, 4)],
    [(4, 2), (0, 0), (0, 4)],
    [(2, 0), (0, 4), (4, 4)],
    [(2, 4), (0, 0), (4, 0)],
    [(0, 2), (4, 0), (4, 4)],
    [(4, 2), (0, 0), (0, 4)],
    [(2, 0), (0, 4), (4, 4)],
    [(2, 4), (0, 0), (4, 0)],
];

/// Rail cell word per tile-code index: class in the low byte (2 ordinary, 3 junction), kind
/// above (_INIT_20, table 0x71022f820c).
#[rustfmt::skip]
const CELL_WORDS: [u16; 111] = [
    0x4002, 0x202, 0x302, 0x402, 0x502, 0x2, 0x102, 0x602,
    0x702, 0x802, 0x902, 0xa02, 0xb02, 0xc02, 0xd02, 0xe02,
    0xf02, 0x1002, 0x1102, 0x1202, 0x1302, 0x1402, 0x1502, 0x1602,
    0x1702, 0x1802, 0x1902, 0x1a02, 0x1b02, 0x1c02, 0x1d02, 0x1e02,
    0x1f02, 0x2002, 0x2102, 0x2202, 0x2302, 0x2402, 0x2502, 0x2602,
    0x2702, 0x2802, 0x2902, 0x2a02, 0x2b02, 0x2c02, 0x2d02, 0x2e02,
    0x2f02, 0x3002, 0x3102, 0x3202, 0x3302, 0x3402, 0x3502, 0x3602,
    0x3702, 0x3802, 0x3902, 0x3a02, 0x3b02, 0x3c02, 0x3d02, 0x3e02,
    0x3f02, 0x1003, 0x1103, 0x103, 0x3, 0x203, 0x303, 0x903,
    0x803, 0xa03, 0xb03, 0x2e03, 0x1c03, 0x1503, 0x3b03, 0x3a03,
    0x3903, 0x3803, 0x2303, 0x2203, 0x1403, 0x2703, 0x2603, 0x2503,
    0x2403, 0x3403, 0x2f03, 0x1d03, 0x1603, 0x4703, 0x4603, 0x4003,
    0x3503, 0x3003, 0x1e03, 0x1703, 0x4a03, 0x4403, 0x4103, 0x5003,
    0x5103, 0x4c03, 0x4d03, 0x4e03, 0x4f03, 0x5203, 0x5303,
];

/// Exit pair per ordinary kind (0x7101cfadbc): directions 0–7, CLOSED / OPEN for end cells, and
/// for the bend cells 48–63 the bend number in bits 8–10 (0x100–0x400).
#[rustfmt::skip]
const EXITS: [(u16, u16); KINDS] = [
    (1, 3), (5, 7), (0, 4), (2, 6), (3, 7), (1, 5),
    (3, 5), (1, 7), (0, 0x50), (4, 0x50), (6, 0x50), (2, 0x50),
    (7, 0x50), (3, 0x50), (1, 0x50), (5, 0x50), (0, 0x28), (4, 0x28),
    (6, 0x28), (2, 0x28), (7, 0x28), (3, 0x28), (1, 0x28), (5, 0x28),
    (0, 4), (2, 6), (3, 7), (1, 5), (0, 2), (0, 6),
    (4, 6), (2, 4), (1, 2), (2, 7), (2, 5), (2, 3),
    (0, 7), (0, 5), (0, 3), (0, 1), (5, 6), (3, 6),
    (1, 6), (6, 7), (3, 4), (1, 4), (4, 7), (4, 5),
    (0x180, 0x180), (0x100, 0x104), (0x102, 0x106), (0x180, 0x180), (0x202, 0x206), (0x280, 0x280),
    (0x280, 0x280), (0x200, 0x204), (0x300, 0x304), (0x380, 0x380), (0x380, 0x380), (0x302, 0x306),
    (0x480, 0x480), (0x402, 0x406), (0x400, 0x404), (0x480, 0x480), (0, 0),
];

/// Motion type per ordinary kind (0x7101cfbb80): 1 straight, 2 junction straight, 3 end, 4 corner,
/// 5 bend; read when attaching.
#[rustfmt::skip]
const MOTION: [u8; KINDS] = [
    4, 4, 1, 1, 1, 1, 4, 4, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 2, 2, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 0,
];

/// Junction kinds' ON/OFF rule (0x7101cfa6ec): 1 present only while the bit is set, 2 only while
/// it is clear, 0 and 3 always (3: exits swap with the bit).
#[rustfmt::skip]
const JUNCTION_RULE: [u8; JUNCTION_KINDS] = [
    2, 2, 2, 2, 1, 1, 1, 1, 2, 2, 2, 2, 1, 1, 1, 1, 2, 2, 1, 1, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Junction exits with the ON/OFF bit clear (0x7101cfab1c) and set (0x7101cfa87c).
#[rustfmt::skip]
const JUNCTION_EXITS: [[(u16, u16); JUNCTION_KINDS]; 2] = [[
    (1, 0x50), (3, 0x50), (5, 0x50), (7, 0x50), (1, 0x50), (3, 0x50), (5, 0x50),
    (7, 0x50), (1, 0x28), (3, 0x28), (5, 0x28), (7, 0x28), (1, 0x28), (3, 0x28),
    (5, 0x28), (7, 0x28), (3, 7), (1, 5), (3, 7), (1, 5), (0, 1),
    (0, 3), (0, 5), (0, 7), (0, 0x28), (0, 0x28), (0, 0x28), (0, 0x28),
    (1, 3), (1, 5), (1, 7), (1, 0x28), (1, 0x28), (1, 0x28), (1, 2),
    (1, 3), (1, 4), (1, 5), (1, 6), (1, 7), (2, 0x28), (3, 0x28),
    (4, 0x28), (5, 0x28), (6, 0x28), (7, 0x28), (2, 3), (2, 5), (2, 7),
    (2, 0x28), (2, 0x28), (2, 0x28), (3, 5), (3, 7), (3, 0x28), (3, 0x28),
    (3, 4), (3, 5), (3, 6), (3, 7), (4, 0x28), (5, 0x28), (6, 0x28),
    (7, 0x28), (4, 5), (4, 7), (4, 0x28), (4, 0x28), (5, 7), (5, 0x28),
    (5, 6), (5, 7), (6, 0x28), (7, 0x28), (6, 7), (6, 0x28), (0, 3),
    (0, 5), (2, 7), (2, 5), (1, 4), (4, 7), (1, 6), (3, 6),
], [
    (1, 0x50), (3, 0x50), (5, 0x50), (7, 0x50), (1, 0x50), (3, 0x50), (5, 0x50),
    (7, 0x50), (1, 0x28), (3, 0x28), (5, 0x28), (7, 0x28), (1, 0x28), (3, 0x28),
    (5, 0x28), (7, 0x28), (3, 7), (1, 5), (3, 7), (1, 5), (0, 0x28),
    (0, 0x28), (0, 0x28), (0, 0x28), (0, 1), (0, 3), (0, 5), (0, 7),
    (1, 0x28), (1, 0x28), (1, 0x28), (1, 3), (1, 5), (1, 7), (2, 0x28),
    (3, 0x28), (4, 0x28), (5, 0x28), (6, 0x28), (7, 0x28), (1, 2), (1, 3),
    (1, 4), (1, 5), (1, 6), (1, 7), (2, 0x28), (2, 0x28), (2, 0x28),
    (2, 3), (2, 5), (2, 7), (3, 0x28), (3, 0x28), (3, 5), (3, 7),
    (4, 0x28), (5, 0x28), (6, 0x28), (7, 0x28), (3, 4), (3, 5), (3, 6),
    (3, 7), (4, 0x28), (4, 0x28), (4, 5), (4, 7), (5, 0x28), (5, 7),
    (6, 0x28), (7, 0x28), (5, 6), (5, 7), (6, 0x28), (6, 7), (0, 5),
    (0, 3), (2, 5), (2, 7), (4, 7), (1, 4), (3, 6), (1, 6),
]];

/// Junction motion types with the bit clear (0x7101cfb8e0) and set (0x7101cfba30).
#[rustfmt::skip]
const JUNCTION_MOTION: [[u8; JUNCTION_KINDS]; 2] = [[
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 4, 4, 4, 4, 3, 3, 3, 3,
    4, 2, 4, 3, 3, 3, 4, 4, 4, 2, 4, 4, 3, 3, 3, 3, 3, 3, 4, 4, 4, 3, 3, 3, 4, 2, 3, 3,
    4, 4, 4, 2, 3, 3, 3, 3, 4, 4, 3, 3, 4, 3, 4, 4, 3, 3, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4,
], [
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 3, 3, 3, 3, 4, 4, 4, 4,
    3, 3, 3, 4, 2, 4, 3, 3, 3, 3, 3, 3, 4, 4, 4, 2, 4, 4, 3, 3, 3, 4, 4, 4, 3, 3, 4, 2,
    3, 3, 3, 3, 4, 4, 4, 2, 3, 3, 4, 4, 3, 4, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4,
]];

/// Index into CELL_WORDS of a tile code (the code switch in 0x7100391df0); None for codes that
/// make no rail cell.
fn code_index(code: u16) -> Option<usize> {
    let c = code as usize;
    Some(match code {
        0 | NO_CODE => 0,
        0x54 => 5,
        0x55 => 6,
        0x60..=0x63 => c - 0x60 + 1,
        0x64 => 7,
        0x65 => 8,
        0x70..=0x77 => c - 0x70 + 9,
        0x80..=0x87 => c - 0x80 + 17,
        0x90..=0xb7 => c - 0x90 + 25,
        0x204 => 65,
        0x205 => 66,
        0x208..=0x20b => c - 0x208 + 67,
        0x20c | 0x20f => 74,
        0x20d => 72,
        0x20e => 73,
        0x214..=0x230 => c - 0x214 + 75,
        0x232..=0x23e if code.is_multiple_of(2) => (c - 0x232) / 2 + 104,
        _ => return None,
    })
}

/// The rail word a junction piece's cell gets [obs 0x710037cdb0, the rail half]: ordinary codes
/// inside a junction become junction kinds (some by piece type, some only away from end A), and
/// every non-empty word is marked as coming from a junction piece (bit 16).
fn junction_word(code: u16, word: u32, ty: u8, end: Option<usize>) -> u32 {
    let of = |types: &[u8]| types.contains(&ty);
    // the game tests "end index != 0": not end A (or not an end at all)
    let not_a = end != Some(0);
    let w = match code {
        NO_CODE => return 0,
        0x54 if of(&[8, 12, 15]) => 0x1f03,
        0x54 if of(&[9, 11, 13]) => 0x2903,
        0x55 if of(&[8, 12, 14]) => 0x4903,
        0x55 if of(&[9, 10, 13]) => 0x4503,
        0x62 => 0x1203,
        0x63 => 0x1303,
        0x64 if of(&[8, 11, 15]) => 0x3d03,
        0x64 if of(&[10, 12, 14]) => 0x3603,
        0x65 if of(&[9, 11, 15]) => 0x2d03,
        0x65 if of(&[10, 13, 14]) => 0x2103,
        0x74 => 0x703,
        0x75 => 0x503,
        0x76 => 0x403,
        0x77 => 0x603,
        0x84 => 0xf03,
        0x85 => 0xd03,
        0x86 => 0xc03,
        0x87 => 0xe03,
        0x92 if of(&[8, 11, 12, 15]) => 0x3f03,
        0x92 => 0x3703,
        0x93 if of(&[8, 10, 12, 14]) => 0x2003,
        0x93 => 0x2b03,
        0x98 if of(&[11, 15]) => 0x2803,
        0x99 if of(&[9, 13]) || (of(&[10, 14]) && not_a) => 0x3303,
        0x9a if of(&[8, 12]) || (of(&[10, 14]) && not_a) => 0x3203,
        0x9b if of(&[11, 15]) => 0x3103,
        0x9c if of(&[9, 13]) => 0x1b03,
        0x9d if of(&[10, 14]) || (of(&[8, 12]) && not_a) => 0x1a03,
        0x9e if of(&[11, 15]) || (of(&[8, 12]) && not_a) => 0x1903,
        0x9f if of(&[9, 13]) => 0x1803,
        0xa0 if of(&[10, 14]) => 0x4803,
        0xa1 if of(&[8, 12]) || (of(&[11, 15]) && not_a) => 0x3e03,
        0xa2 if of(&[9, 13]) || (of(&[11, 15]) && not_a) => 0x2c03,
        0xa3 if of(&[10, 14]) => 0x4b03,
        0xa4 if of(&[8, 12]) => 0x3c03,
        0xa5 if of(&[11, 15]) || (of(&[9, 13]) && not_a) => 0x2a03,
        0xa6 if of(&[10, 11, 14, 15]) || (of(&[9, 13]) && not_a) => 0x4303,
        0xa7 if of(&[8, 12]) => 0x4203,
        _ => word,
    };
    if w == 0 {
        0
    } else {
        (w & !0xf_0000) | 0x1_0000
    }
}

/// Down to a multiple of 16 through an unsigned integer, as the game rounds (negatives give 0).
fn down16(v: f32) -> f32 {
    ((v as u32) & !15) as f32
}

/// Up to the next multiple of 16 unless already on one (away from zero for negatives).
fn up16(v: f32) -> f32 {
    let i = v as i32;
    if v == i as f32 && i % 16 == 0 {
        v
    } else {
        (i - i % 16 + if i >= 0 { 16 } else { -16 }) as f32
    }
}

/// Toward zero to a multiple of 16.
fn trunc16(v: f32) -> f32 {
    let i = v as i32;
    if v == i as f32 && i % 16 == 0 {
        v
    } else {
        (i - i % 16) as f32
    }
}

/// The game's 256-entry sine table: sin, its step to the next entry, cos, its step
/// [obs 0x7101d34bf8; same values as sin/cos of 2π·i/256 rounded to f32, exact 0 on the axes].
fn wave(i: u32) -> (f32, f32, f32, f32) {
    let at = |k: u32| (std::f64::consts::TAU * k as f64 / 256.0).dsin_cos();
    let (s0, c0) = at(i);
    let (s1, c1) = at(i + 1);
    let axis = |v: f64, zero: bool| if zero { 0.0 } else { v as f32 };
    (
        axis(s0, i.is_multiple_of(128)),
        (s1 - s0) as f32,
        axis(c0, i % 128 == 64),
        (c1 - c0) as f32,
    )
}

/// sin and cos of a 32-bit angle (top byte = table entry, the rest interpolates).
fn sin_cos(angle: u32) -> (f32, f32) {
    let (s, ds, c, dc) = wave(angle >> 24);
    let f = (angle & 0xff_ffff) as f32 * f32::from_bits(0x3380_0000);
    (s + ds * f, c + f * dc)
}

/// The bend (1–4) a cell's exits belong to (bits 8–10), if any.
fn bend_of((e0, e1): (u16, u16)) -> Option<u16> {
    [e0, e1]
        .into_iter()
        .map(|e| (e & 0x700) >> 8)
        .find(|b| (1..=4).contains(b))
}

/// The area's rail grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Tracks {
    /// Rail cell words by cell (x, y), y up.
    cells: BTreeMap<(i32, i32), u32>,
    /// The course-wide ON/OFF bit as the game holds it: set = ON (red), the course-start state
    /// [obs 0x7100ae4270: the switch-state reset writes 0x10000 to +0x28, i.e. +0x2a = 1].
    pub on: bool,
}

impl Default for Tracks {
    fn default() -> Self {
        Tracks {
            cells: BTreeMap::new(),
            on: true,
        }
    }
}

impl Tracks {
    /// Build from track records: (x, y, type), 255 meaning −1, with the templates' own end cells.
    pub fn new(records: impl IntoIterator<Item = (u8, u8, u8)>) -> Self {
        Self::with_ends(records.into_iter().map(|(x, y, ty)| (x, y, ty, 0, 0)))
    }

    /// Build from track records with their end words: (x, y, type, k0, k1).
    pub fn with_ends(records: impl IntoIterator<Item = (u8, u8, u8, u16, u16)>) -> Self {
        let mut t = Tracks::default();
        for (x, y, ty, k0, k1) in records {
            if ty > 15 {
                continue;
            }
            let junction = ty >= 8;
            let n = if junction { 5 } else { 3 };
            let mut tpl = [0u16; 25];
            if junction {
                tpl = TEMPLATE_5[ty as usize - 8];
            } else {
                tpl[..9].copy_from_slice(&TEMPLATE_3[ty as usize]);
            }
            let word = (k1 as u32) << 16 | k0 as u32;
            let codes = if junction {
                [word & 0x3ff, (word >> 10) & 0x3ff, word >> 20]
            } else {
                [word >> 16, word & 0xffff, 0]
            };
            let ends = &END_CELLS[ty as usize][..if junction { 3 } else { 2 }];
            for (&(cx, cy), &code) in ends.iter().zip(&codes) {
                if code != 0 {
                    tpl[cy * n + cx] = if code == NO_CODE as u32 {
                        0
                    } else {
                        code as u16
                    };
                }
            }
            let at = |v: u8| if v == 255 { -1 } else { v as i32 };
            let (ox, oy) = (at(x), at(y));
            for row in 0..n {
                for col in 0..n {
                    let code = tpl[row * n + col];
                    let mut w = code_index(code).map_or(0, |i| CELL_WORDS[i] as u32);
                    if junction {
                        let end = ends.iter().position(|&e| e == (col, row));
                        w = junction_word(code, w, ty, end);
                    }
                    let (class, kind) = (w & 0xff, (w >> 8) & 0xff);
                    let (cx, cy) = (ox + col as i32, oy + row as i32);
                    if w != 0 && (class == 3 || (class == 2 && kind != 0x40)) && cx >= 0 && cy >= 0
                    {
                        t.cells.insert((cx, cy), w);
                    }
                }
            }
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The raw cell word under a point.
    fn word(&self, (x, y): (f32, f32)) -> Option<u32> {
        let key = ((x * 0.0625).floor() as i32, (y * 0.0625).floor() as i32);
        self.cells.get(&key).copied()
    }

    /// Whether a junction kind exists in the current ON/OFF state.
    fn present(&self, kind: u8) -> bool {
        match JUNCTION_RULE[kind as usize] {
            1 => self.on,
            2 => !self.on,
            _ => true,
        }
    }

    /// The cell the follower sees under a point: (kind, is a junction cell), or None for no
    /// track. A follower already inside a junction (`inside`) sees junction cells whatever the
    /// ON/OFF state; entering one, the state decides [obs 0x7100341a00].
    fn read(&self, p: (f32, f32), inside: bool) -> Option<(u8, bool)> {
        let w = self.word(p)?;
        let kind = ((w >> 8) & 0xff) as u8;
        match w & 0xff {
            2 if (kind as usize) < KINDS => Some((kind, false)),
            3 if (kind as usize) < JUNCTION_KINDS && (inside || self.present(kind)) => {
                Some((kind, true))
            }
            _ => None,
        }
    }

    fn exits(&self, kind: u8, junction: bool) -> (u16, u16) {
        if junction {
            JUNCTION_EXITS[self.on as usize][kind as usize]
        } else {
            EXITS[kind as usize]
        }
    }

    fn motion(&self, kind: u8, junction: bool) -> u8 {
        if junction {
            JUNCTION_MOTION[self.on as usize][kind as usize]
        } else {
            MOTION[kind as usize]
        }
    }

    /// Cells the follower can use now, with their centres.
    fn live(&self) -> impl Iterator<Item = ((f32, f32), u8, bool)> + '_ {
        self.cells.keys().filter_map(|&(cx, cy)| {
            let c = (cx as f32 * CELL + 8.0, cy as f32 * CELL + 8.0);
            self.read(c, false).map(|(k, j)| (c, k, j))
        })
    }

    /// Track lines for drawing: half-cell spokes from each cell centre toward its exits, and the
    /// bends' quarter circles.
    pub fn polylines(&self) -> Vec<Vec<(f32, f32)>> {
        let mut out = Vec::new();
        for (c, kind, junction) in self.live() {
            let exits = self.exits(kind, junction);
            if let Some(b) = bend_of(exits) {
                // one arc per bend, drawn from its first cell
                if kind as u16 == 44 + 4 * b {
                    let arc = Bend::middle_of(b, 0, c, 0).1;
                    let a0 = [1.0f32, 2.0, 0.0, 3.0][b as usize - 1] * std::f32::consts::FRAC_PI_2;
                    out.push(
                        (0..=12)
                            .map(|k| {
                                let a = a0 + std::f32::consts::FRAC_PI_2 * k as f32 / 12.0;
                                (
                                    arc.centre.0 + BEND_RADIUS * a.dcos(),
                                    arc.centre.1 + BEND_RADIUS * a.dsin(),
                                )
                            })
                            .collect(),
                    );
                }
                continue;
            }
            for e in [exits.0, exits.1] {
                if let Some(&(dx, dy)) = DIRS.get(e as usize) {
                    out.push(vec![c, (c.0 + dx * 8.0, c.1 + dy * 8.0)]);
                }
            }
        }
        out
    }

    /// End cells (caps) for drawing.
    pub fn ends(&self) -> Vec<(f32, f32)> {
        self.live()
            .filter(|&(_, kind, junction)| {
                let e = self.exits(kind, junction).1;
                e == CLOSED || e == OPEN
            })
            .map(|(c, _, _)| c)
            .collect()
    }
}

/// A quarter-circle sweep about a cell corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Bend {
    centre: (f32, f32),
    /// Angle increasing.
    ccw: bool,
    /// Current and final angle (a full turn = 2³²).
    angle: u32,
    end: u32,
    /// Swept so far and in all, in 16-bit angle units.
    done: u32,
    sweep: u32,
    /// Direction of travel when it ends.
    exit: u8,
}

impl Bend {
    /// Entering bend `b` heading `dir` at the centre of its first cell (x, y): the circle's
    /// centre, sense, start and end angle (quarter turns) and the leaving direction
    /// [obs 0x7100341280]. None where the bend has no entry from that direction.
    fn enter(b: u16, dir: u8, (x, y): (f32, f32)) -> Option<Bend> {
        let r = BEND_RADIUS;
        let (centre, ccw, from, to, exit) = match (b, dir) {
            (1, 4) => ((up16(x), y - r), true, 1, 2, 6),
            (1, 2) => ((x + r, down16(y)), false, 2, 1, 0),
            (2, 6) => ((x + r, up16(y)), true, 2, 3, 0),
            (2, 4) => ((up16(x), y + r), false, 3, 2, 2),
            (3, 2) => ((x - r, down16(y)), true, 0, 1, 4),
            (3, 0) => ((down16(x), y - r), false, 1, 0, 6),
            (4, 6) => ((x - r, up16(y)), false, 0, 3, 4),
            (4, 0) => ((down16(x), y + r), true, 3, 0, 2),
            _ => return None,
        };
        Some(Bend {
            centre,
            ccw,
            angle: (from as u32) << 30,
            end: (to as u32) << 30,
            done: 0,
            sweep: 0x4000,
            exit,
        })
    }

    /// Attached on cell `sub` (0–3) of bend `b`, centre (x, y): the follower starts at the cell
    /// corner the four cells share and sweeps the second half of the bend, one way for start
    /// field 0 and the other otherwise [obs 0x7100342f60, corner rounding 0x7100342d10].
    fn middle_of(b: u16, sub: u16, (x, y): (f32, f32), start: u8) -> ((f32, f32), Bend) {
        let corner = match sub {
            0 => (up16(x), down16(y)),
            1 => (down16(x), down16(y)),
            2 => (up16(x), up16(y)),
            _ => (trunc16(x), up16(y)),
        };
        // (offset to the circle's centre, mid angle, then sense, end and exit per start field)
        let (off, mid, first, other) = match b {
            1 => ((16.0, -16.0), 0x6000_0000, (false, 1, 0), (true, 2, 6)),
            2 => ((16.0, 16.0), 0xa000_0000, (true, 3, 0), (false, 2, 2)),
            3 => ((-16.0, -16.0), 0x2000_0000, (false, 0, 6), (true, 1, 4)),
            _ => ((-16.0, 16.0), 0xe000_0000, (true, 0, 2), (false, 3, 4)),
        };
        let (ccw, to, exit) = if start == 0 { first } else { other };
        (
            corner,
            Bend {
                centre: (corner.0 + off.0, corner.1 + off.1),
                ccw,
                angle: mid,
                end: (to as u32) << 30,
                done: 0,
                sweep: 0x2000,
                exit,
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Leg {
    /// Heading for the target cell.
    Line,
    /// Heading for the edge where a bend starts.
    ToBend,
    /// Sweeping a bend.
    Bend,
}

/// An object riding a track: the game's rail follower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    pos: (f32, f32),
    target: (f32, f32),
    /// Velocity the follower reports (heading × speed), which a rider leaving the track keeps.
    vel: (f32, f32),
    dir: u8,
    leg: Leg,
    bend: Bend,
    /// Start-direction field ((flags >> 20) & 3).
    start: u8,
    /// On junction cells (ON/OFF already settled for this stretch).
    inside: bool,
    pub speed: f32,
    /// Left the track (FallDown): an open end, a missing cell or a switched-off junction arm.
    pub off: bool,
}

impl Rail {
    /// Attach at (x, y), the object's reference point, which snaps to the centre of its 16-px
    /// cell (a point on a cell edge goes right / down). `start` = the object's start-direction
    /// field ((flags >> 20) & 3) [obs 0x7100342f60]: on a straight cell 0 heads right / up /
    /// down-right / up-right (horizontal / vertical / the two diagonals) and 1 the other way;
    /// 2 heads up and 3 down whatever the piece; on corner and end cells it takes the cell's
    /// exits in the order 1, 7, 2, 3, 6, 5, 4 (else 0); on a bend it starts at the bend's middle.
    /// No track under the point: it is off at once (the game's FallDown).
    pub fn attach(tracks: &Tracks, x: f32, y: f32, start: u8, winged: bool) -> Option<Self> {
        let pos = (down16(x) + 8.0, up16(y) - 8.0);
        let mut r = Rail {
            pos,
            target: pos,
            vel: (0.0, 0.0),
            dir: 0,
            leg: Leg::Line,
            bend: Bend::default(),
            start,
            inside: false,
            speed: if winged { SPEED_WINGED } else { SPEED },
            off: false,
        };
        let Some((kind, junction)) = tracks.read(pos, false) else {
            r.off = true;
            return Some(r);
        };
        r.inside = junction;
        let exits = tracks.exits(kind, junction);
        r.dir = match tracks.motion(kind, junction) {
            5 => 0,
            1 => match start {
                2 => 2,
                3 => 6,
                s => {
                    let base = match kind {
                        2 => 0,
                        3 => 2,
                        4 => 7,
                        _ => 1,
                    };
                    if s == 1 { (base + 4) % 8 } else { base }
                }
            },
            _ => {
                r.start = 0;
                let (e0, e1) = exits;
                if e0 == 0 || e1 == 0 {
                    0
                } else {
                    [1, 7, 2, 3, 6, 5, 4]
                        .into_iter()
                        .find(|&d| e0 == d || e1 == d)
                        .unwrap_or(0) as u8
                }
            }
        };
        if let Some(b) = bend_of(exits) {
            let sub = kind as u16 - (44 + 4 * b);
            let (corner, bend) = Bend::middle_of(b, sub, pos, r.start);
            r.pos = corner;
            r.bend = bend;
            r.leg = Leg::Bend;
        } else {
            r.plan(tracks, pos, false);
        }
        Some(r)
    }

    pub fn point(&self, _tracks: &Tracks) -> (f32, f32) {
        self.pos
    }

    /// The velocity the follower reports; a rider leaving the track carries it on.
    pub fn velocity(&self) -> (f32, f32) {
        self.vel
    }

    /// Advance one frame; returns the new point.
    pub fn step(&mut self, tracks: &Tracks) -> (f32, f32) {
        if !self.off {
            match self.leg {
                Leg::Bend => self.sweep(tracks),
                _ => self.run(tracks),
            }
        }
        self.pos
    }

    /// Look ahead from `from` (from the next cell, or from `from` itself) along the heading:
    /// pass straight cells, stop at the first other cell (or where the track ends), or stop at
    /// the edge before a bend and set the bend up [obs 0x7100341280].
    fn plan(&mut self, tracks: &Tracks, from: (f32, f32), here: bool) {
        let (dx, dy) = DIRS[self.dir as usize];
        let step = (dx * CELL, dy * CELL);
        let mut prev = from;
        let mut cur = if here {
            from
        } else {
            (from.0 + step.0, from.1 + step.1)
        };
        for _ in 0..LOOK_AHEAD {
            let Some((kind, junction)) = tracks.read(cur, self.inside) else {
                break;
            };
            if let Some(b) = bend_of(tracks.exits(kind, junction)) {
                match Bend::enter(b, self.dir, cur) {
                    Some(bend) => self.bend = bend,
                    None => {
                        self.bend.done = 0;
                        self.bend.sweep = 0x4000;
                    }
                }
                self.leg = Leg::ToBend;
                self.target = (prev.0 + step.0 * 0.5, prev.1 + step.1 * 0.5);
                return;
            }
            if junction || !STRAIGHT[self.dir as usize].contains(&kind) {
                break;
            }
            prev = cur;
            cur = (cur.0 + step.0, cur.1 + step.1);
        }
        self.target = cur;
    }

    /// One frame toward the target; on reaching it, start the bend or take the cell's next exit.
    fn run(&mut self, tracks: &Tracks) {
        let v = self.speed;
        let (dx, dy) = (self.target.0 - self.pos.0, self.target.1 - self.pos.1);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = if len > 0.0 {
            let inv = 1.0 / len;
            (dx * inv, dy * inv)
        } else {
            (dx, dy)
        };
        self.pos = (self.pos.0 + ux * v, self.pos.1 + uy * v);
        let (s, _, c, _) = wave(self.dir as u32 * 32);
        self.vel = (v * c, s * v);
        // each axis snaps onto the target within 0.7 v; both snapped = arrived
        let tol = v * SNAP;
        let ry = self.target.1 - self.pos.1;
        let mut snapped = 0;
        if (self.target.0 - self.pos.0).abs() <= tol {
            self.pos.0 = self.target.0;
            snapped += 1;
        }
        if ry.abs() <= tol {
            self.pos.1 = self.target.1;
            snapped += 1;
        }
        if snapped < 2 {
            return;
        }
        if self.leg == Leg::ToBend {
            self.leg = Leg::Bend;
        } else {
            self.arrive(tracks);
        }
    }

    /// At a cell centre: leave by the exit not come in by; turn back at a closed end, leave the
    /// track at an open one [obs 0x7100340720].
    fn arrive(&mut self, tracks: &Tracks) {
        let Some((kind, junction)) = tracks.read(self.pos, self.inside) else {
            self.inside = false;
            self.off = true;
            return;
        };
        self.inside = junction;
        let (e0, e1) = tracks.exits(kind, junction);
        let back = ((self.dir + 4) % 8) as u16;
        let mut next = if back != e0 {
            e0
        } else if back != e1 {
            e1
        } else {
            0
        };
        if junction {
            let rule = JUNCTION_RULE[kind as usize];
            if rule == 0 && back != e0 && back != e1 {
                // a junction's middle cell entered off both its exits: take an axis exit
                next = [e0, e1]
                    .into_iter()
                    .find(|&e| e < 8 && e % 2 == 0)
                    .unwrap_or(next);
            } else if rule == 3 && next == OPEN {
                // a switched-off arm's end: closed when the cell is a junction piece's own
                next = match tracks.word(self.pos) {
                    Some(w) if w & 0xf_00ff == 0x1_0003 => CLOSED,
                    Some(_) => OPEN,
                    None => {
                        self.off = true;
                        return;
                    }
                };
            }
        }
        match next {
            CLOSED => self.dir = back as u8,
            d if d < 8 => self.dir = d as u8,
            _ => {
                self.off = true;
                return;
            }
        }
        self.plan(tracks, self.pos, false);
    }

    /// One frame along a bend; at its end, carry on in its exit direction.
    fn sweep(&mut self, tracks: &Tracks) {
        let v = self.speed;
        let b = &mut self.bend;
        let rate = v * ARC_RATE;
        let mut a = (b.angle >> 16) as f32 + if b.ccw { rate } else { -rate };
        if a < 0.0 {
            a += 65536.0;
        }
        b.angle = (a as i32 as u32) << 16;
        b.done = (rate + b.done as f32) as u32;
        let done = b.done >= b.sweep;
        if done {
            b.angle = b.end;
        }
        let (s, c) = sin_cos(b.angle);
        self.vel = if b.ccw {
            (-(v * s), v * c)
        } else {
            (v * s, -(v * c))
        };
        self.pos = (c * BEND_RADIUS + b.centre.0, s * BEND_RADIUS + b.centre.1);
        if done {
            self.dir = b.exit;
            self.leg = Leg::Line;
            let (dx, dy) = DIRS[self.dir as usize];
            let from = (self.pos.0 + dx * 8.0, self.pos.1 + dy * 8.0);
            self.plan(tracks, from, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cell centre in px.
    fn c(x: i32, y: i32) -> (f32, f32) {
        (x as f32 * 16.0 + 8.0, y as f32 * 16.0 + 8.0)
    }

    fn run(r: &mut Rail, t: &Tracks, frames: usize) -> Vec<(f32, f32)> {
        (0..frames).map(|_| r.step(t)).collect()
    }

    #[test]
    fn records_stamp_their_templates_and_end_words() {
        // a horizontal piece on tiles (10..12, 11): k1 on the left end, k0 on the right
        let t = Tracks::with_ends([(10, 10, 0, 0x81, 0x70)]);
        assert_eq!(t.cells.len(), 3);
        assert_eq!(t.word(c(10, 11)), Some(0x0802), "closed cap facing right");
        assert_eq!(t.word(c(11, 11)), Some(0x0202), "straight");
        assert_eq!(t.word(c(12, 11)), Some(0x1102), "open cap facing left");
        // 0x104 clears its end: the neighbour's word at the shared cell stays
        let t = Tracks::with_ends([(9, 3, 0, 0x90, 0x70), (11, 3, 0, 0x71, 0x104)]);
        assert_eq!(
            t.word(c(11, 4)),
            Some(0x1802),
            "joint written by the first piece"
        );
        assert_eq!(t.cells.len(), 5);
        // a junction piece's cells are marked and its arms are junction kinds
        let j = Tracks::with_ends([(20, 20, 8, 0, 0)]);
        assert_eq!(j.word(c(22, 22)).map(|w| w & 0xff), Some(3), "switch cell");
        assert_eq!(
            j.word(c(23, 21)),
            Some(0x1_1203),
            "lower arm, junction diagonal"
        );
        assert_eq!(j.cells.len(), 7);
    }

    #[test]
    fn a_straight_cell_takes_21_frames_and_closed_ends_turn_back() {
        // two cells of straight between closed caps: (10, 11) .. (12, 11)
        let t = Tracks::new([(10, 10, 0)]);
        let mut r = Rail::attach(&t, c(11, 11).0, c(11, 11).1, 0, false).unwrap();
        assert_eq!(r.dir, 0, "start field 0 heads right on a horizontal");
        let p = run(&mut r, &t, 21);
        assert_eq!(p[19].0, 184.0 + 15.0, "0.75 px a frame");
        assert_eq!(p[20], c(12, 11), "snapped onto the end cell's centre");
        // the closed end sends it back
        let p = run(&mut r, &t, 2);
        assert!(p[1].0 < p[0].0 && p[0].0 < 200.0, "{p:?}");
        assert!(!r.off);
        let mut l = Rail::attach(&t, c(11, 11).0, c(11, 11).1, 1, false).unwrap();
        assert_eq!(l.dir, 4, "start field 1 heads left");
        assert_eq!(run(&mut l, &t, 21)[20], c(10, 11));
        // vertical: 0 up, 1 down; 2 / 3 up / down whatever the piece
        let v = Tracks::new([(10, 10, 1)]);
        for (s, d) in [(0, 2), (1, 6), (2, 2), (3, 6)] {
            assert_eq!(Rail::attach(&v, 184.0, 184.0, s, false).unwrap().dir, d);
        }
        let h = Rail::attach(&t, 184.0, 184.0, 3, false).unwrap();
        assert_eq!(
            h.dir, 6,
            "a horizontal with start 3 heads down, off the track"
        );
    }

    #[test]
    fn open_ends_let_the_rider_off() {
        // k0 (right end) open, k1 (left end) closed
        let t = Tracks::with_ends([(10, 10, 0, 0x81, 0x70)]);
        let mut r = Rail::attach(&t, c(11, 11).0, c(11, 11).1, 0, false).unwrap();
        let p = run(&mut r, &t, 22);
        assert!(r.off, "fell off at the open end");
        assert_eq!(p[21], c(12, 11), "from the end cell's centre");
        assert_eq!(r.velocity(), (0.75, 0.0), "moving on at the rider speed");
        // nothing under the point: off at once
        let none = Rail::attach(&t, 0.0, 0.0, 0, false).unwrap();
        assert!(none.off);
    }

    #[test]
    fn joined_pieces_turn_corners_at_cell_centres() {
        // horizontal, vertical, horizontal; default caps at the joints still pass the rider on
        let t = Tracks::new([(14, 29, 0), (13, 30, 1), (12, 31, 0)]);
        let mut r = Rail::attach(&t, c(16, 30).0, c(16, 30).1, 2, false).unwrap();
        assert_eq!(r.dir, 4, "an end cell starts along its exit");
        // 32 px is 42 frames at 0.75 (the last step snaps 0.5 px)
        let p = run(&mut r, &t, 42 * 3);
        assert_eq!(p[41], c(14, 30), "32 px left");
        assert_eq!(p[83], c(14, 32), "32 px up");
        assert_eq!(p[125], c(12, 32), "32 px left to the far end");
        let q = run(&mut r, &t, 2);
        assert!(q[1].0 > q[0].0, "turned back at the closed end: {q:?}");
    }

    /// Ride a bend piece from its end A to end B; returns the path.
    fn ride_bend(ty: u8) -> (Tracks, Vec<(f32, f32)>) {
        let t = Tracks::new([(10, 10, ty)]);
        let (ax, ay) = END_CELLS[ty as usize][0];
        let a = c(10 + ax as i32, 10 + ay as i32);
        let mut r = Rail::attach(&t, a.0, a.1, 0, false).unwrap();
        let mut path = vec![r.point(&t)];
        for _ in 0..200 {
            let p = r.step(&t);
            path.push(p);
            let (bx, by) = END_CELLS[ty as usize][1];
            if path.len() > 60 && p == c(10 + bx as i32, 10 + by as i32) {
                break;
            }
        }
        (t, path)
    }

    #[test]
    fn bends_sweep_a_24_px_quarter_circle_toward_their_corner() {
        // (type, the template corner it bulges toward)
        for (ty, corner) in [(4, (0, 0)), (5, (2, 2)), (6, (0, 2)), (7, (2, 0))] {
            let (_, path) = ride_bend(ty);
            let (bx, by) = END_CELLS[ty as usize][1];
            assert_eq!(
                *path.last().unwrap(),
                c(10 + bx as i32, 10 + by as i32),
                "type {ty} reaches end B"
            );
            // the point farthest from the A–B chord lies on the bulge corner's side
            let k = c(10 + corner.0, 10 + corner.1);
            let mid = c(11, 11);
            let (ax, ay) = END_CELLS[ty as usize][0];
            let a = c(10 + ax as i32, 10 + ay as i32);
            let side =
                |p: (f32, f32)| (a.0 - mid.0) * (p.1 - mid.1) - (a.1 - mid.1) * (p.0 - mid.0);
            let far = path
                .iter()
                .copied()
                .max_by(|p, q| side(*p).abs().total_cmp(&side(*q).abs()))
                .unwrap();
            assert!(
                side(far) * side(k) > 0.0,
                "type {ty} bulges toward {corner:?}: {far:?}"
            );
            // the arc stays 24 px from the corner of the bend's 2×2 cells nearest the middle
            let o = (mid.0 + (k.0 - mid.0) / 2.0, mid.1 + (k.1 - mid.1) / 2.0);
            let centre = (2.0 * mid.0 - o.0, 2.0 * mid.1 - o.1);
            let on_arc = path
                .iter()
                .filter(|p| ((p.0 - centre.0).dhypot(p.1 - centre.1) - 24.0).abs() < 0.01)
                .count();
            assert!(
                on_arc > 40,
                "type {ty}: {on_arc} frames on the 24-px circle"
            );
        }
    }

    #[test]
    fn a_bend_takes_51_frames_at_walking_speed() {
        // type 6 from end A (0, 0) going up: 8 px to the bend, 51 frames of arc, 8 px out
        let t = Tracks::new([(10, 10, 6)]);
        let mut r = Rail::attach(&t, c(10, 10).0, c(10, 10).1, 0, false).unwrap();
        assert_eq!(r.dir, 2);
        let p = run(&mut r, &t, 10);
        assert_eq!(p[9], (168.0, 176.0), "at the bend's edge");
        assert_eq!(r.leg, Leg::Bend);
        let p = run(&mut r, &t, 51);
        assert_eq!(p[50], (192.0, 200.0), "quarter turn done after 51 frames");
        assert_eq!(r.dir, 0, "heading right");
        let s = run(&mut r, &t, 1)[0];
        assert_eq!(s, (192.75, 200.0));
    }

    #[test]
    fn junctions_route_by_the_on_off_state() {
        // type 8: arm from the left into the switch cell, branches up-right and down-right
        let mut t = Tracks::with_ends([(20, 20, 8, 0, 0)]);
        let ride = |t: &Tracks| {
            let mut r = Rail::attach(t, c(20, 22).0, c(20, 22).1, 0, false).unwrap();
            let p = run(&mut r, t, 300);
            (r, p)
        };
        t.on = true;
        let (_, on) = ride(&t);
        t.on = false;
        let (_, off) = ride(&t);
        let reached = |p: &[(f32, f32)], cell: (i32, i32)| p.contains(&c(cell.0, cell.1));
        assert!(
            reached(&on, (22, 22)) && reached(&off, (22, 22)),
            "both reach the switch"
        );
        // ON (the course-start state) takes the lower branch, OFF the upper one
        assert!(reached(&on, (24, 20)) && !reached(&on, (24, 24)), "{on:?}");
        assert!(
            reached(&off, (24, 24)) && !reached(&off, (24, 20)),
            "{off:?}"
        );
    }
}
