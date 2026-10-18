//! Course parts added in SMM2's 2.0 / 3.0 updates: which actor each one becomes (the v3.0.3
//! part table at 0x7102a55688, resolver 0x7100d80df0 [obs]), and the tile parts (P Block, Dash
//! Block) that the world switches or reads (world/v3.rs).

use mmk_formats::smm2::CourseObject;

use super::{Cell, Level, TILE};
use crate::actors::Kind;

/// Tile parts of the 2.0 / 3.0 updates in an area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct V3Tiles {
    /// P Blocks (79): tile and kind (`alt` = flag 0x4, "PBlockAppear": solid only while a P
    /// Switch runs; the normal "PBlockDisappear" is solid only while none does) [obs].
    pub p_blocks: Vec<(i32, i32, bool)>,
    /// Dash Blocks (80, 3D World): left tile, row and width in tiles (3) [obs EditDB].
    pub dash_blocks: Vec<(i32, i32, i32)>,
}

impl V3Tiles {
    /// The Dash Block whose top a point at (x, feet y) stands on.
    pub fn dash_under(&self, x: f32, y: f32) -> bool {
        self.dash_blocks.iter().any(|&(l, row, w)| {
            let (x0, x1, top) = (
                l as f32 * TILE,
                (l + w) as f32 * TILE,
                (row + 1) as f32 * TILE,
            );
            x >= x0 && x <= x1 && (y - top).abs() < 1.0
        })
    }
}

/// The actor a 2.0 / 3.0 course part becomes, if it is one.
pub(super) fn actor_kind(o: &CourseObject, lvl: &Level) -> Option<Kind> {
    let snow = lvl.theme == 6;
    Some(match o.id {
        // Spike: the 0x4 form is the ball itself (GabonIronBallWU, GabonSnowBallWU in the snow
        // theme), toggled by shaking in the editor [obs]
        74 if o.flags & 0x4 != 0 => Kind::SpikeBall { snow },
        74 => Kind::Spike,
        // Pokey: the stack is the placement height (1–8); SanboSnow in the snow theme [obs]
        78 => Kind::Pokey {
            snow,
            height: o.height.clamp(1, 8),
        },
        // Mechakoopa: the 2-bit attribute (flags >> 18) & 3 picks the kind: 1 Blasta, 2 Zappa,
        // 0 / 3 the plain one; the 0x4 bit does nothing [obs 0x7100d80df0]
        111 => Kind::Mechakoopa {
            weapon: match (o.flags >> 18) & 3 {
                1 => 1,
                2 => 2,
                _ => 0,
            },
        },
        132 => Kind::OnOffTrampoline {
            blue: o.flags & 0x4 != 0,
            width: o.width.max(1),
        },
        120..=126 => Kind::Koopaling {
            who: (o.id - 120) as u8,
            key: o.child_id == 95,
            clown: o.flags & 0x200 != 0,
        },
        _ => return None,
    })
}

/// What the engine does not play yet on these parts.
pub(super) fn unsupported(o: &CourseObject) -> Option<String> {
    // a Koopaling in a Koopa Clown Car uses its own Clown context (Move / ShellMove …), not
    // modelled
    ((120..=126).contains(&o.id) && o.flags & 0x200 != 0)
        .then(|| format!("Koopaling {} in a Clown Car", o.id))
}

/// Tile parts: placed into the area (P Block, Dash Block). True when `o` was one.
pub(super) fn place_tile(lvl: &mut Level, o: &CourseObject, tx: i32, ty: i32) -> bool {
    match o.id {
        79 => {
            let alt = o.flags & 0x4 != 0;
            lvl.v3.p_blocks.push((tx, ty, alt));
            // no P Switch runs at the start
            lvl.set(tx, ty, if alt { Cell::Empty } else { Cell::Solid });
            true
        }
        80 => {
            // a 48 × 16 solid, walkable on top, centred on the placement [obs 0x71023cf5c8]
            let w = o.width.max(3) as i32;
            let left = ((o.x as f32 / 10.0 - w as f32 * 8.0) / TILE).round() as i32;
            for dx in 0..w {
                lvl.set(left + dx, ty, Cell::Solid);
            }
            lvl.v3.dash_blocks.push((left, ty, w));
            true
        }
        _ => false,
    }
}
