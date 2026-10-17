//! Ground auto-tiling: the ground-tile graphic id for each neighbour mask.
//!
//! Derived by this project from the public mm2_level course sample (356,641 ground tiles in 550
//! courses): for every ground tile, the 8-neighbour mask (bit 0 up, then clockwise: 1 up-right,
//! 2 right, 3 down-right, 4 down, 5 down-left, 6 left, 7 up-left) and the id stored with it; the
//! table keeps the most common id per mask. Several ids are decorative variants of one another
//! (e.g. interior 12/13/14/62), so courses differ; any of them draws correctly.

/// Ground tile id by neighbour mask.
pub const GROUND_ID: [u8; 256] = [
    24, 30, 24, 30, 25, 32, 25, 64, 24, 30, 24, 30, 25, 32, 25, 64, 28, 29, 28, 29, 34, 38, 34, 42,
    28, 29, 28, 29, 58, 40, 58, 61, 24, 30, 62, 30, 25, 32, 25, 64, 24, 30, 24, 30, 25, 32, 25, 64,
    28, 29, 28, 29, 34, 38, 34, 42, 28, 29, 28, 29, 58, 40, 62, 61, 27, 31, 27, 31, 26, 35, 26, 67,
    27, 31, 27, 31, 26, 35, 26, 46, 33, 37, 33, 37, 36, 39, 36, 50, 33, 37, 33, 37, 44, 48, 44, 54,
    27, 31, 27, 31, 26, 35, 26, 46, 27, 31, 27, 31, 26, 35, 26, 46, 60, 41, 1, 41, 45, 49, 45, 57,
    60, 41, 62, 41, 59, 52, 59, 67, 24, 30, 24, 30, 25, 32, 25, 64, 64, 30, 24, 62, 25, 32, 25, 62,
    28, 29, 28, 29, 34, 38, 34, 42, 28, 29, 28, 29, 58, 40, 58, 61, 24, 30, 58, 30, 25, 32, 25, 64,
    24, 30, 24, 30, 25, 32, 25, 64, 28, 29, 28, 29, 34, 38, 34, 42, 28, 29, 28, 29, 59, 40, 58, 61,
    27, 66, 60, 66, 26, 68, 59, 62, 65, 66, 27, 66, 26, 47, 26, 65, 33, 43, 33, 43, 36, 51, 36, 53,
    33, 43, 33, 43, 44, 56, 44, 69, 27, 66, 27, 62, 26, 47, 26, 65, 27, 66, 27, 66, 26, 47, 26, 65,
    60, 63, 60, 63, 45, 55, 45, 70, 62, 63, 60, 63, 59, 68, 59, 62,
];
