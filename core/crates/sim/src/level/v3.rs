//! Course parts added in SMM2's 2.0 / 3.0 updates: which actor each one becomes, and the tile
//! parts (P Block, Dash Block) the world switches or reads (world/v3.rs).

use mmk_formats::smm2::CourseObject;

use super::{Cell, Level};
use crate::actors::Kind;

/// Tile parts of the 2.0 / 3.0 updates in an area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct V3Tiles {
    /// P Blocks (79): tile and kind (`alt` = flag 0x4).
    pub p_blocks: Vec<(i32, i32, bool)>,
    /// Dash Blocks (80, 3D World): tile and boost direction (+1 right, −1 left).
    pub dash_blocks: Vec<(i32, i32, i32)>,
}

/// The actor a 2.0 / 3.0 course part becomes, if it is one.
pub(super) fn actor_kind(o: &CourseObject, lvl: &Level) -> Option<Kind> {
    let _ = lvl;
    Some(match o.id {
        74 => Kind::Spike,
        78 => Kind::Pokey {
            snow: lvl.theme == 5,
        },
        111 => Kind::Mechakoopa { weapon: 0 },
        132 => Kind::OnOffTrampoline {
            blue: o.flags & 0x4 != 0,
        },
        120..=126 => Kind::Koopaling {
            who: (o.id - 120) as u8,
            key: o.child_id == 95,
            clown: o.flags & 0x200 != 0,
        },
        _ => return None,
    })
}

/// Tile parts: placed into the area (P Block, Dash Block). True when `o` was one.
pub(super) fn place_tile(lvl: &mut Level, o: &CourseObject, tx: i32, ty: i32) -> bool {
    match o.id {
        79 => {
            let alt = o.flags & 0x4 != 0;
            lvl.v3.p_blocks.push((tx, ty, alt));
            lvl.set(tx, ty, if alt { Cell::Solid } else { Cell::Empty });
            true
        }
        80 => {
            lvl.v3.dash_blocks.push((tx, ty, 1));
            lvl.set(tx, ty, Cell::Solid);
            true
        }
        _ => false,
    }
}
