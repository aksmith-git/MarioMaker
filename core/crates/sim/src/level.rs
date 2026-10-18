//! A playable area built from SMM2 course data: collision cells and the tiles to draw.
//!
//! Sources: research/specs/course-loading.md (ground placement, start/goal ground from the header,
//! object anchoring) and the format doc. Ground records are tile coordinates with y up from the
//! bottom row; objects are in 1/160-tile units, anchored at the horizontal centre of their bottom
//! row. Terrain art: SMB1 field atlases are 16 tiles wide and ground tile id n is atlas index
//! 184 + n.

use mmk_formats::smm2::{Area, Course};

use crate::actors::Kind;
use crate::blocks::{BlockKind, Contents};

mod v3;
pub use v3::V3Tiles;

pub const TILE: f32 = 16.0;
const ATLAS_COLS: u32 = 16;
const GROUND_ATLAS_BASE: u32 = 184;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Solid,
    /// One-way platform: solid only from above.
    SemiTop,
    /// Spike trap: solid on every side, hurts the player on contact (43-spike-trap.md).
    Spike,
    /// One-way wall (67) edges: PassRight blocks leftward entry at the cell's right edge,
    /// PassLeft blocks rightward entry at its left edge, PassDown blocks upward entry at its
    /// bottom edge (the floor orientation uses SemiTop).
    PassRight,
    PassLeft,
    PassDown,
}

impl Cell {
    pub fn is_solid(self) -> bool {
        matches!(self, Cell::Solid | Cell::Spike)
    }
}

/// A moving solid owned by an actor (Thwomp, lift, spring), rebuilt by the world every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// How far it moved this frame (riders are carried by this).
    pub dx: f32,
    pub dy: f32,
    /// Only the top surface collides (lifts, springs).
    pub top_only: bool,
    /// Actor id.
    pub owner: u32,
    /// Top-surface slope (dy/dx about the box centre; seesaws). 0 = flat.
    pub slope: f32,
}

impl Solid {
    /// Height of the top surface at x.
    pub fn top_at(&self, x: f32) -> f32 {
        let cx = (self.x0 + self.x1) / 2.0;
        self.y1 + self.slope * (x.clamp(self.x0, self.x1) - cx)
    }
}

/// A tile to draw: grid position and atlas index (row-major, 16 per row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileDraw {
    pub x: i32,
    pub y: i32,
    pub atlas: u32,
    /// bit 0: mirror horizontally, bit 1: mirror vertically
    pub flip: u8,
    /// Part of a pipe (drawn in front of the player with the SMM1 layering option).
    pub pipe: bool,
    /// A background-type tile (semisolid, mushroom platform, bridge, half-collision platform,
    /// vine, conveyor), drawn behind coins in the SMM1 order (world/smm1.rs `tile_layer`).
    pub back: bool,
}

impl TileDraw {
    pub fn atlas_rect(&self) -> (f32, f32, f32, f32) {
        (
            (self.atlas % ATLAS_COLS) as f32 * TILE,
            (self.atlas / ATLAS_COLS) as f32 * TILE,
            TILE,
            TILE,
        )
    }
}

/// The sloped part of a slope object, in tiles: `width` columns rising `height` tiles from the
/// row `bottom` (the top of the low end cap). The course object is the band of 87-gentle-slope.md
/// / 88-steep-slope.md (layouts 0x71003929b0 / 0x71003932f0) [obs]: a gentle slope is
/// (2n + 2) × (n + 1) tiles and a steep one (n + 2) × (n + 1), with a full 1×1 end cap in the low
/// corner and in the high corner and the diagonal between them (2n columns for 1:2, n for 45°);
/// the caps are solid cells, flush with the ground beside the low end [obs corpus: the ground
/// next to a slope's low end has its top row level with the cap]. The band's hollow underside
/// (16 px under the surface) is not modelled: cells under the surface and under the high cap
/// are solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slope {
    pub left: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
    /// true: low on the left, high on the right ("/")
    pub rising: bool,
    pub steep: bool,
}

impl Slope {
    pub fn covers(&self, px: f32) -> bool {
        let x0 = self.left as f32 * TILE;
        px >= x0 && px < x0 + self.width as f32 * TILE
    }

    /// Surface height (px) at x. Both kinds have a flat column at each end: the low one a tile
    /// above the bottom row, the high one at the top; between them the diagonal rises 1:1
    /// (steep, (n+1)×n) or 1:2 (gentle, 2n×n) [obs: Ryujinx traces of the player walking over
    /// all four kinds; the bottom row is a solid base].
    pub fn surface(&self, px: f32) -> f32 {
        let (x0, w, h) = (
            self.left as f32 * TILE,
            self.width as f32 * TILE,
            self.height as f32 * TILE,
        );
        // distance from the low end
        let u = if self.rising { px - x0 } else { x0 + w - px }.clamp(0.0, w);
        let along = (u - TILE).clamp(0.0, (w - 2.0 * TILE).max(0.0));
        let rise = if self.steep { along } else { along / 2.0 };
        self.bottom as f32 * TILE + TILE + rise.min((h - TILE).max(0.0))
    }

    /// x lies over the diagonal (not one of the flat end columns).
    pub fn on_diagonal(&self, px: f32) -> bool {
        let x0 = self.left as f32 * TILE;
        px > x0 + TILE && px < x0 + (self.width - 1) as f32 * TILE
    }

    /// Steepness index for the slope physics tables (player-physics.md §5.2). The player's
    /// terrain sensor (0x71009b0010) classes the surface angle: < 7.03° 0, < 20.3° 1, < 35.8° 2,
    /// < 54.2° 3, else 4 [obs] — so the gentle slope (26.6°) is 2 and the steep one (45°) 3.
    pub fn steepness(&self) -> usize {
        if self.steep { 3 } else { 2 }
    }
}

/// A reactive block placed in the course (drawn and animated by the world, solid in the grid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockPlacement {
    pub x: i32,
    pub y: i32,
    pub kind: BlockKind,
    pub contents: Option<Contents>,
}

/// An actor placed in the course: spawned when its position comes into view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub kind: Kind,
    /// Feet centre (px).
    pub x: f32,
    pub y: f32,
    pub big: bool,
    pub wings: bool,
    /// Lives in a pipe (Piranha Plant): direction and mouth (entry point).
    pub pipe: Option<(PipeDir, f32, f32)>,
    /// Drifts down under a parachute (flag 0x8000).
    pub parachute: bool,
    /// On a track (flag 0x400): the start-direction field ((flags >> 20) & 3).
    pub rail: Option<u8>,
    /// Link id (−1 none) and "rides a Koopa Clown Car" (flag 0x200): a rider and its car share
    /// the link id.
    pub link: i16,
    pub clown: bool,
    /// The object's alternate form (flag 0x4), for actors whose kind doesn't encode it.
    pub alt: bool,
}

/// A tree (106-tree.md): `x` trunk centre, `y` origin (centre of the bottom row), `h` height in
/// tiles, and what its canopy holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tree {
    pub x: f32,
    pub y: f32,
    pub h: i32,
    pub contents: Option<crate::blocks::Contents>,
}

impl Tree {
    /// Canopy bottom and top (px): (H − 3)·16 − 20 above the origin, 45 tall [obs].
    pub fn canopy(&self) -> (f32, f32) {
        let bottom = self.y + (self.h - 3) as f32 * TILE - 20.0;
        (bottom, bottom + 45.0)
    }
}

/// An airship cannon (47): block origin, barrel angle (degrees), next shot timer kept by the
/// world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cannon {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    /// The red (0x4) cannon: its balls fly at 2.4 px/f instead of 1.0 (47-cannon.md).
    pub red: bool,
}

/// The area's liquid (themes.md §3): kind 1 water, 2 lava, 3 poison; heights in px
/// (h × 16 + 12); mode 1 one-way, 2 oscillate (else static); speed 1–3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidSpec {
    pub kind: u8,
    pub start: f32,
    pub end: f32,
    pub mode: u8,
    pub speed: u8,
    /// The whole area is water (underwater theme).
    pub full: bool,
}

/// A Burner (54): block origin, flame direction (0 left, 1 right, 2 down, 3 up), alternate phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burner {
    pub x: f32,
    pub y: f32,
    pub dir: u8,
    pub alt: bool,
}

/// A Fire Bar (24): pivot (block centre), fireball count incl. the pivot, start angle (degrees),
/// direction (+1 counter-clockwise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireBar {
    pub x: f32,
    pub y: f32,
    pub n: u32,
    pub start: f32,
    pub dir: f32,
}

/// Something placed inside a pipe (object flag bit 0): the pipe keeps producing it
/// (EnemyWaki, 9-pipe.md). `colour` selects the timing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipeGenerator {
    pub kind: Kind,
    pub big: bool,
    pub wings: bool,
    pub dir: PipeDir,
    /// Pipe entry point (outer edge of the mouth).
    pub x: f32,
    pub y: f32,
    pub colour: u8,
}

/// A Bill Blaster column: x centre, y bottom (px), height in tiles, what it fires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blaster {
    pub x: f32,
    pub y: f32,
    pub h: i32,
    /// None = Bullet Bill.
    pub contents: Option<Kind>,
    /// The red (0x4) Bull's-Eye Blaster: its Bullet Bills home in.
    pub homing: bool,
}

/// A checkpoint flag: origin (bottom centre) and the power-up inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checkpoint {
    pub x: f32,
    pub y: f32,
    pub contents: Option<Contents>,
}

/// Pipe mouth direction ((flags >> 5) & 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeDir {
    Right,
    Left,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpKind {
    Pipe(PipeDir),
    /// type: 0 normal, 1 P-switch door, 2 key door
    Door(u8),
    /// Warp Box (97-warp-box.md), key-locked or not: entered by touch.
    Box {
        key: bool,
    },
}

/// A warp entrance/exit (9-pipe.md, 55-door.md). Pipes link to the same id in the other area;
/// doors link 2k ↔ 2k+1 in the same area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Warp {
    pub kind: WarpKind,
    pub id: u8,
    /// Pipe: centre of the mouth tile pair. Door: origin (bottom centre).
    pub x: f32,
    pub y: f32,
    /// Pipe length in tiles.
    pub len: i32,
}

impl Warp {
    /// Outer edge of a pipe mouth (the entry point), or the door origin.
    pub fn entry(&self) -> (f32, f32) {
        match self.kind {
            WarpKind::Pipe(PipeDir::Right) => (self.x + 8.0, self.y),
            WarpKind::Pipe(PipeDir::Left) => (self.x - 8.0, self.y),
            WarpKind::Pipe(PipeDir::Up) => (self.x, self.y + 8.0),
            WarpKind::Pipe(PipeDir::Down) => (self.x, self.y - 8.0),
            WarpKind::Door(_) | WarpKind::Box { .. } => (self.x, self.y),
        }
    }
}

/// A coin placed as a tile (collectable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoinTile {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct Level {
    /// Grid covers tiles x in [min_x, min_x + width), y in [0, height).
    pub min_x: i32,
    pub width: i32,
    pub height: i32,
    cells: Vec<Cell>,
    pub tiles: Vec<TileDraw>,
    pub coins: Vec<CoinTile>,
    /// Pink coins (92): all of them, in both areas, make a key.
    pub pink_coins: Vec<CoinTile>,
    pub slopes: Vec<Slope>,
    pub blocks: Vec<BlockPlacement>,
    pub spawns: Vec<Spawn>,
    pub warps: Vec<Warp>,
    /// Moving solids this frame (runtime, filled by the world).
    pub solids: Vec<Solid>,
    pub generators: Vec<PipeGenerator>,
    pub checkpoints: Vec<Checkpoint>,
    pub blasters: Vec<Blaster>,
    /// Conveyor belts: left tile, right tile (inclusive), row, speed (px/frame, + = right).
    pub conveyors: Vec<(i32, i32, i32, f32)>,
    pub burners: Vec<Burner>,
    pub firebars: Vec<FireBar>,
    pub cannons: Vec<Cannon>,
    /// Snake blocks: leftmost tile, row, length, fast, head on the left, path node codes.
    pub snakes: Vec<(i32, i32, usize, bool, bool, Vec<u16>)>,
    /// ! blocks (119): bottom-left tile of the 2×2 block and its path's node codes.
    pub exclamations: Vec<((i32, i32), Vec<u16>)>,
    /// Trees (106, 3D World): climbable trunk (a vine) and a canopy platform.
    pub trees: Vec<Tree>,
    /// Track-block paths: (path index, node codes).
    pub track_paths: Vec<(u8, Vec<u16>)>,
    /// Track graph (59-track.md).
    pub tracks: crate::track::Tracks,
    /// Piranha Creeper paths (107): record index and node codes (actors/w3.rs).
    pub creepers: Vec<(u8, Vec<u8>)>,
    /// Custom autoscroll path (89 nodes): (node number, x, y, speed setting 0–2), in px.
    pub scroll_nodes: Vec<(u8, f32, f32, u8)>,
    /// Blinking blocks (108): tile and phase group (flag 0x4: the opposite group).
    pub blink_blocks: Vec<(i32, i32, bool)>,
    /// Spike blocks (110): bottom-left tile of the 2×2 block and mode (0 timer, 1 out while ON,
    /// 2 out while OFF; see `spike_block_mode`).
    pub spike_blocks: Vec<(i32, i32, u8)>,
    /// Vines (64): climb areas (centre x, bottom, top) in px.
    pub vines: Vec<(f32, f32, f32)>,
    /// Tile parts of the 2.0 / 3.0 updates (level/v3.rs).
    pub v3: V3Tiles,
    pub liquid: Option<LiquidSpec>,
    /// One-way walls (drawing): centre x, bottom y, orientation.
    pub oneway: Vec<(f32, f32, u8)>,
    /// Doors to draw: origin (bottom centre) and type.
    pub doors: Vec<(f32, f32, u8)>,
    /// Goal pole position (X0, Y0): pole x and the top of its base block (main area only).
    pub goal: Option<(f32, f32)>,
    pub style: String,
    pub theme: u8,
    pub night: bool,
    /// Player start (feet) in px.
    pub start: (f32, f32),
    /// Playable area in px.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    /// Header boundary type: ≠ 0 when nothing is built above the line (the camera stays on the
    /// first screen height) [camera spec §5.2].
    pub boundary_type: u8,
    pub vertical: bool,
    /// Main area (0) rather than the sub-area.
    pub main_area: bool,
    pub timer: i16,
    /// Area autoscroll: 0 none, 1 slow, 2 normal, 3 fast, 4 custom (themes.md §4).
    pub autoscroll: u8,
}

/// Atlas index of a block in SMB1 field atlases (row 0: brick 1, ? 2, hidden 3, note 4, used 5,
/// hard 6; donut 64; cloud 102; ice 120) — read off the M1 atlas.
pub fn block_atlas(kind: BlockKind) -> u32 {
    match kind {
        BlockKind::Brick => 1,
        BlockKind::Question => 2,
        BlockKind::Hidden => 3,
        BlockKind::Note => 4,
        BlockKind::Empty => 5,
        BlockKind::Hard => 6,
        BlockKind::Donut => 64,
        BlockKind::Cloud => 102,
        BlockKind::Ice => 120,
        BlockKind::OnOff => 21 * 16 + 2 + 32,
        BlockKind::Dotted { blue } => 21 * 16 + 2 + blue as u32,
    }
}

/// ON/OFF-dependent art: switch (ON 370 / OFF 371), dotted blocks solid (338/339) or outline
/// (354/355), from atlas rows 21–23.
pub fn switch_atlas(kind: BlockKind, on: bool) -> u32 {
    match kind {
        BlockKind::OnOff => 23 * 16 + 2 + (!on) as u32,
        BlockKind::Dotted { blue } => {
            let solid = blue != on;
            21 * 16 + 2 + blue as u32 + if solid { 0 } else { 16 }
        }
        k => block_atlas(k),
    }
}

/// Course part ids the engine implements (fully or in a documented simplified form). Ids 26/27/
/// 37/38 (start/goal ground, goal, start sign) come from the header; 66 (arrow sign) and 109
/// (sound effect) are decoration only.
pub const SUPPORTED_PARTS: &[i16] = &[
    0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 50, 51,
    52, 53, 54, 55, 56, 57, 58, 60, 61, 62, 63, 64, 65, 66, 67, 68, 70, 71, 72, 73, 75, 76, 77, 82,
    83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 94, 95, 96, 97, 98, 99, 100, 102, 103, 104, 105, 106,
    107, 108, 109, 110, 112, 113, 114, 115, 116, 117, 119,
    // 2.0 / 3.0 parts (level/v3.rs)
    74, 78, 79, 80, 111, 120, 121, 122, 123, 124, 125, 126, 132,
    // the update power-ups (part 81 by style, the wearables)
    81, 127, 128, 129, 130, 131,
];

/// Parts whose alternate form (flag 0x4) the engine implements (or where the flag only picks a
/// look or sound: 23 the music block, kind 0x56 "GakkiBlock", built by the Note Block's builder
/// 0x71008722e0 and bounced by the same code — only its note sound differs [obs]). 52: the
/// alternate Wiggler starts angry (0x710067bcc0 sets its state to Angry at build) [obs]. Any
/// other part with the flag is reported by `unsupported_features`.
const ALT_FORMS: &[i16] = &[
    0, 1, 2, 3, 10, 11, 12, 13, 15, 19, 23, 25, 28, 32, 34, 36, 40, 41, 42, 45, 46, 47, 48, 51, 52,
    54, 56, 61, 77, 84, 85, 96, 97, 100, 103, 104, 107, 108, 113,
    // 2.0 / 3.0: the spike ball, the P Block that appears, the blue ON/OFF Trampoline; on a
    // Mechakoopa the bit does nothing [obs]
    74, 79, 111, 132, // the Super Mushroom's 0x4 form: the SMB1 Master Sword
    20,
];

/// Game styles the engine plays.
pub const PLAYABLE_STYLES: &[&str] = &["M1", "M3", "MW", "WU", "3W"];

/// Parts that become actors and so can ride a track (59-track.md).
const RIDES_TRACKS: &[i16] = &[
    0, 1, 2, 3, 4, 5, 6, 8, 10, 11, 12, 13, 15, 24, 47, 54, 18, 19, 20, 22, 23, 25, 28, 30, 31, 32,
    33, 34, 35, 36, 39, 40, 41, 42, 44, 45, 46, 48, 50, 51, 52, 56, 57, 58, 60, 61, 62, 63, 65, 68,
    70, 76, 77, 82, 83, 91, 95, 96, 98, 102, 103, 105, 114, 117, 74, 111,
];

/// Why a course is not fully playable yet: one entry per missing feature (empty = playable).
pub fn unsupported_features(course: &Course) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    // styles: SMB3 and SMW are 2D sprite styles drawn like SMB1; NSMBU and 3D World draw 3D
    // models where the cache has them (ffi mesh module)
    let style = course.header.game_style.as_str_lossy();
    if !PLAYABLE_STYLES.contains(&style.as_str()) {
        out.insert(format!("style {style}"));
    }
    // (a custom autoscroll without nodes runs from its start unit to its end unit,
    // `Level::finish_custom_scroll`)
    for (ai, area) in course.areas().iter().enumerate() {
        if ai == 1 && area.objects.is_empty() && area.ground.is_empty() {
            continue; // unused sub-area
        }

        for o in area.objects.iter() {
            if !SUPPORTED_PARTS.contains(&o.id) {
                out.insert(format!("part {}", o.id));
            } else if o.flags & 0x400 != 0 && !RIDES_TRACKS.contains(&o.id) {
                // blocks, coins and fixed objects on tracks need moving versions: not yet
                out.insert(format!("part {} on a track", o.id));
            } else if o.flags & 0x4 != 0 && !ALT_FORMS.contains(&o.id) {
                out.insert(format!("alternate form of part {}", o.id));
            } else if let Some(why) = v3::unsupported(o) {
                out.insert(why);
            } else if o.flags & 0x2 != 0 && o.id == 13 {
                // winged Bill Blasters fly as actors: not yet
                out.insert(format!("wings on part {}", o.id));
            }
        }
    }
    let cond = crate::clear::ClearCondition::from_header(
        course.header.clear_condition_category,
        course.header.clear_condition,
        course.header.clear_condition_magnitude.max(0) as u16,
    );
    if let crate::clear::Goal::Unsupported(i) = cond.goal {
        out.insert(format!("clear condition {i}"));
    }
    out.into_iter().collect()
}

/// A spike block's mode in `Level::spike_blocks` from its course flags. The course field
/// (flags >> 18) & 3 is the colour: 0 red, 1 blue, 2 the timer block. Its BG record
/// (0x7100394600, group 0x1F) is ON/OFF-linked for 0 and 1, and the area update (0x71004005e0)
/// drives only the others from the shared 240-frame timer; at load a linked block's phase is
/// "out" (0) for blue and "in" (2) for red while the switch is ON, the reverse while OFF
/// (0x7100408000), and every ON/OFF change flips it; the editor models agree (0x71003c2660:
/// red → `needlecube_red_model` "In", blue → `needlecube_blue_model` "Out", 2 → the timer
/// model) [obs].
fn spike_block_mode(flags: u32) -> u8 {
    match (flags >> 18) & 3 {
        0 => 2, // red: out while OFF
        1 => 1, // blue: out while ON
        _ => 0, // timer
    }
}

/// A cannon's barrel angle (degrees, 0 right, counter-clockwise) from its course flags. The
/// cannon's build (0x71007628a0) takes the facing r = (flags >> 25) & 3 (as a Piranha Plant's:
/// 0 left, 1 right, 2 down, 3 up) and the aim d = (flags >> 22) & 7, and turns the barrel to
/// base[r] + 180° − 45°·((d + 1) mod 5), with base = {90°, 270°, 180°, 0°} (_INIT_605,
/// 0x710242afa4). Relative to the facing, d = 0…4 aim +45°, 0°, −45°, −90°, +90°: five aims
/// over the half circle on the facing side [obs; corpus d ∈ 0…4].
fn cannon_angle(flags: u32) -> f32 {
    let side = [90, 270, 180, 0][((flags >> 25) & 3) as usize];
    let aim = ((flags >> 22) & 7) as i32;
    (side + 180 - 45 * ((aim + 1) % 5)).rem_euclid(360) as f32
}

fn pipe_dir(flags: u32) -> PipeDir {
    match (flags >> 5) & 3 {
        0 => PipeDir::Right,
        1 => PipeDir::Left,
        2 => PipeDir::Up,
        _ => PipeDir::Down,
    }
}

fn block_kind(id: i16) -> Option<BlockKind> {
    Some(match id {
        4 => BlockKind::Brick,
        5 => BlockKind::Question,
        6 => BlockKind::Hard,
        23 => BlockKind::Note,
        21 => BlockKind::Donut,
        22 => BlockKind::Cloud,
        29 => BlockKind::Hidden,
        63 => BlockKind::Ice,
        99 => BlockKind::OnOff,
        // dotted-line block: the 0x4 flag is its tile variant (0x71003d2600) [obs]; variant 1
        // is drawn blue, as the blinking block's [inf: the TensenBlock tile entries' colour bit
        // was not tied to the variant]
        100 => BlockKind::Dotted { blue: false },
        _ => return None,
    })
}

impl Level {
    /// Build area 0 (main) or 1 (sub) of a course.
    pub fn from_course(course: &Course, area_index: usize) -> Self {
        let area: &Area = course.areas()[area_index];
        let h = &area.header;
        let right = h.boundary_right as f32;
        let top = h.boundary_top as f32;
        // Grid: a few tiles left of 0 (start ground starts at tile −3) to the right boundary.
        let min_x = -4;
        let width = (right / TILE).ceil() as i32 - min_x + 1;
        let height = (top / TILE).ceil() as i32 + 1;
        let mut lvl = Level {
            min_x,
            width,
            height,
            cells: vec![Cell::Empty; (width * height) as usize],
            tiles: Vec::new(),
            coins: Vec::new(),
            slopes: Vec::new(),
            blocks: Vec::new(),
            spawns: Vec::new(),
            warps: Vec::new(),
            solids: Vec::new(),
            generators: Vec::new(),
            checkpoints: Vec::new(),
            blasters: Vec::new(),
            conveyors: Vec::new(),
            burners: Vec::new(),
            firebars: Vec::new(),
            cannons: Vec::new(),
            snakes: Vec::new(),
            exclamations: Vec::new(),
            trees: Vec::new(),
            track_paths: Vec::new(),
            blink_blocks: Vec::new(),
            spike_blocks: Vec::new(),
            scroll_nodes: Vec::new(),
            pink_coins: Vec::new(),
            vines: Vec::new(),
            v3: V3Tiles::default(),
            tracks: Default::default(),
            creepers: Vec::new(),
            liquid: None,
            oneway: Vec::new(),
            doors: Vec::new(),
            goal: None,
            style: course.header.game_style.as_str_lossy(),
            theme: h.theme,
            night: h.is_night(),
            start: (56.0, 0.0),
            left: h.boundary_left as f32,
            right,
            top,
            bottom: h.boundary_bottom as f32,
            boundary_type: h.boundary_type,
            vertical: h.orientation != 0,
            main_area: area_index == 0,
            autoscroll: h.autoscroll_type,
            timer: course.header.timer,
        };

        // liquid by theme: underwater = all water; castle = lava; forest = water (poison at night)
        let height = |b: u8| b as f32 * TILE + 12.0;
        lvl.liquid = match h.theme {
            4 => Some(LiquidSpec {
                kind: 1,
                start: f32::MAX,
                end: f32::MAX,
                mode: 0,
                speed: 0,
                full: true,
            }),
            2 | 9 => Some(LiquidSpec {
                kind: if h.theme == 2 {
                    2
                } else if lvl.night {
                    3
                } else {
                    1
                },
                start: height(h.liquid_start_height),
                end: height(h.liquid_end_height),
                mode: h.liquid_mode,
                speed: h.liquid_speed,
                full: false,
            }),
            _ => None,
        };

        // ground records
        for g in area.ground.iter() {
            let (x, y) = (g.x as i32, g.y as i32);
            lvl.set(x, y, Cell::Solid);
            lvl.tiles.push(TileDraw {
                pipe: false,
                back: false,
                flip: 0,
                x,
                y,
                atlas: GROUND_ATLAS_BASE + g.id as u32,
            });
        }

        // start and goal ground come from the header (main area only); x is the centre of the
        // leftmost tile [course-loading.md §1.5, decoded against goal_x]
        if area_index == 0 {
            let start_h = course.header.start_y as i32;
            lvl.fill_ground(-3, 10, start_h);
            // x = 72 px [obs: Ryujinx trace, the player position in courses the game loads]
            lvl.start = (72.0, start_h as f32 * TILE);
            let goal_left = ((right - 152.0 - 8.0) / TILE).round() as i32;
            let goal_h = course.header.goal_y as i32;
            lvl.fill_ground(goal_left, 11, goal_h);
            // the pole stands on a block over the goal ground's first column: the goal pole's
            // fixed unit is rebuilt at x = right − 152, y = 16·goal_y (0x71005aa940, the goal
            // ground's x), the spawn adds no offset (0x710041b520) and GoalPole's create keeps x
            // and puts its 16-px solid block one tile above y (0x710088c7f0, shape 0x7101d04710)
            // [obs]
            let pole_tile = goal_left;
            lvl.blocks.push(BlockPlacement {
                x: pole_tile,
                y: goal_h,
                kind: BlockKind::Hard,
                contents: None,
            });
            lvl.set(pole_tile, goal_h, Cell::Solid);
            lvl.goal = Some((
                pole_tile as f32 * TILE + TILE / 2.0,
                (goal_h + 1) as f32 * TILE,
            ));
        }

        lvl.creepers = area
            .piranha_creepers
            .slots
            .values()
            .map(|c| (c.head.index, c.nodes.iter().map(|n| n.direction).collect()))
            .collect();
        lvl.tracks = crate::track::Tracks::with_ends(
            area.tracks
                .iter()
                .filter(|t| t.x != 0 || t.y != 0 || t.r#type != 0 || t.k0 != 0)
                .map(|t| (t.x, t.y, t.r#type, t.k0, t.k1)),
        );

        // tile-like objects: (x, y) is the centre of the bottom-left tile [toost LevelDrawer
        // semisolid/pipe placement; course-loading.md: tile parts always sit on tile centres]
        // pipe entry points (for objects placed inside pipes): (x, y, dir, colour)
        let pipe_entries: Vec<(f32, f32, PipeDir, u8)> = area
            .objects
            .iter()
            .filter(|o| o.id == 9)
            .map(|o| {
                let dir = pipe_dir(o.flags);
                let (ax, ay) = (o.x as f32 / 10.0, o.y as f32 / 10.0);
                let l = (o.height.max(1) as f32 - 1.0) * TILE;
                let (ex, ey) = match dir {
                    PipeDir::Right => (ax + l + 8.0, ay - 8.0),
                    PipeDir::Left => (ax - l - 8.0, ay + 8.0),
                    PipeDir::Up => (ax + 8.0, ay + l + 8.0),
                    PipeDir::Down => (ax - 8.0, ay - l - 8.0),
                };
                (ex, ey, dir, ((o.flags >> 18) & 3) as u8)
            })
            .collect();
        lvl.track_paths = area
            .track_blocks
            .slots
            .values()
            .map(|p| {
                (
                    p.head.index,
                    p.nodes.iter().map(|n| n.direction as u16).collect(),
                )
            })
            .collect();
        for o in area.objects.iter() {
            let (w, hgt) = (o.width.max(1) as i32, o.height.max(1) as i32);
            let left_px = o.x as f32 / 10.0 - 8.0;
            let bottom_px = o.y as f32 / 10.0 - 8.0;
            let tx = (left_px / TILE).round() as i32;
            let ty = (bottom_px / TILE).round() as i32;
            let actorish = o.flags & 0x2 != 0 || o.flags & 0x400 != 0; // winged / on track → actor (step 6)
            let big = o.flags & 0x6000 == 0x4000;
            let wings = o.flags & 0x2 != 0;
            let actor = match o.id {
                0 if o.flags & 0x4 != 0 => Some(Kind::Goombrat),
                0 => Some(Kind::Goomba),
                1 => Some(Kind::Koopa {
                    red: o.flags & 0x4 != 0,
                }),
                // the Super Mushroom's 0x4 form in SMB1 is the Master Sword (Triforce 0xff)
                // [obs resolver 0x7100d80df0]
                20 if o.flags & 0x4 != 0 && lvl.style == "M1" => Some(Kind::PowerItem {
                    what: crate::actors::SWORD,
                }),
                20 => Some(Kind::Mushroom),
                // the style power-up: Super Leaf (SMB3), Cape Feather (SMW); SMB1 keeps the
                // mushroom for now
                44 if lvl.style != "M1" => Some(Kind::StyleItem),
                44 => Some(Kind::Mushroom),
                34 if o.flags & 0x4 != 0 && lvl.style == "M1" => Some(Kind::SuperballFlower),
                34 => Some(Kind::FireFlower),
                35 => Some(Kind::Star),
                33 => Some(Kind::OneUp),
                2 => Some(Kind::Piranha {
                    fire: o.flags & 0x4 != 0,
                }),
                82 => Some(Kind::Bumper),
                76 => Some(Kind::Twister),
                105 => Some(Kind::Claw),
                56 => Some(Kind::CheepCheep {
                    red: o.flags & 0x4 != 0,
                }),
                48 => Some(Kind::Blooper),
                39 => Some(Kind::Magikoopa),
                58 => Some(Kind::RockyWrench),
                52 => Some(Kind::Wiggler),
                42 => Some(Kind::ClownCar {
                    fire: o.flags & 0x4 != 0,
                }),
                72 => Some(Kind::KoopaCar),
                73 => Some(Kind::EscortToad),
                75 => Some(Kind::Stone),
                115 => Some(Kind::GoalToadette),
                116 => Some(Kind::SuperHammer),
                85 => Some(Kind::TrackBlock {
                    path: o.extended_data.wrapping_sub(1) as u8,
                    alt: o.flags & 0x4 != 0,
                }),
                113 => Some(Kind::Trampoline {
                    width: o.width.max(1),
                    // Trampoline init 0x71008c3200 [obs]: flag 0x4 → stationary (lift mode 6);
                    // otherwise bits 3–4 = s: s odd → mode 1, ±2 tiles, s even → mode 0, ±3
                    // (the first move is leftward for s = 0 or 3, +0x264 = 1)
                    moving: o.flags & 0x4 == 0,
                    range: if o.flags & 0x8 != 0 { 2 } else { 3 },
                }),
                112 => Some(Kind::Crate {
                    contents: Contents::from_part(o.child_id, o.child_flags),
                }),
                60 => Some(Kind::LavaBubble),
                103 => Some(Kind::FishBone),
                45 if matches!(lvl.style.as_str(), "MW" | "WU") => Some(Kind::YoshiEgg {
                    red: o.flags & 0x4 != 0,
                }),
                45 => Some(Kind::ShoeGoomba {
                    heels: o.flags & 0x4 != 0,
                }),
                // bosses: a child Key (95) drops on defeat; flag 0x200 = in a Koopa Clown Car;
                // Boom Boom's alternate form is Pom Pom only in 3D World (Bunbun3W → Punpun)
                62 => Some(Kind::Bowser {
                    key: o.child_id == 95,
                    clown: o.flags & 0x200 != 0,
                }),
                77 => Some(Kind::BoomBoom {
                    key: o.child_id == 95,
                    clown: o.flags & 0x200 != 0,
                    pom: o.flags & 0x4 != 0 && lvl.style == "3W",
                }),
                98 => Some(Kind::BowserJr {
                    key: o.child_id == 95,
                    clown: o.flags & 0x200 != 0,
                }),
                102 => Some(Kind::MontyMole),
                104 => Some(Kind::AngrySun {
                    moon: o.flags & 0x4 != 0,
                }),
                61 => Some(Kind::ChainChomp {
                    chained: o.flags & 0x4 == 0,
                }),
                36 => Some(Kind::LavaLift {
                    fast: o.flags & 0x4 != 0,
                }),
                83 => Some(Kind::Skewer {
                    dir: ((o.flags >> 22) & 3) as u8,
                }),
                30 => Some(Kind::Lakitu),
                31 => Some(Kind::Cloud),
                19 => Some(Kind::Pow),
                95 => Some(Kind::Key),
                68 => Some(Kind::Grinder),
                91 => Some(Kind::Seesaw {
                    width: o.width.max(2),
                }),
                32 => Some(Kind::BanzaiBill {
                    dir: ((o.flags >> 22) & 3) as u8,
                    homing: o.flags & 0x4 != 0,
                }),
                70 => Some(Kind::BigCoin {
                    value: ((o.flags >> 18) & 3).min(2) as u8,
                }),
                10 if o.flags & 0x4 != 0 => Some(Kind::SideSpring),
                10 => Some(Kind::Spring),
                18 => Some(Kind::PSwitch),
                // the 0x4 forms are empty shells (MetShell / TogezoShell / KaronShell rows)
                25 if o.flags & 0x4 != 0 => Some(Kind::EmptyShell { what: 2 }),
                28 if o.flags & 0x4 != 0 => Some(Kind::EmptyShell { what: 1 }),
                46 if o.flags & 0x4 != 0 => Some(Kind::EmptyShell { what: 3 }),
                25 => Some(Kind::Spiny),
                28 => Some(Kind::Buzzy),
                40 => Some(Kind::SpikeTop {
                    surface: ((o.flags >> 25) & 3) as u8,
                }),
                3 => Some(Kind::HammerBro),
                15 => Some(Kind::BobOmb),
                46 => Some(Kind::DryBones),
                41 => Some(Kind::Boo),
                57 => Some(Kind::Muncher),
                // bits 0x18 both set → Dossun, otherwise SideDossun, which looks right when
                // (flags >> 3) & 3 == 2, else left (0x710076ab10) [obs]
                12 => Some(Kind::Thwomp {
                    side: o.flags & 0x18 != 0x18,
                    right: (o.flags >> 3) & 3 == 2,
                }),
                11 => Some(Kind::Lift {
                    width: o.width.max(1),
                    falling: o.flags & 0x4 != 0,
                }),
                // 3D World enemies (actors/w3.rs); the alternate (0x4) is the horned / spiny /
                // sleeping form; a creeper's path is extended data − 1
                96 => Some(Kind::AntTrooper {
                    horned: o.flags & 0x4 != 0,
                }),
                65 => Some(Kind::Stingby),
                51 => Some(Kind::Skipsqueak {
                    spiny: o.flags & 0x4 != 0,
                }),
                50 => Some(Kind::HopChops),
                114 => Some(Kind::Porcupuffer),
                117 => Some(Kind::Bully),
                86 => Some(Kind::Charvaargh),
                107 => Some(Kind::PiranhaCreeper {
                    path: o.extended_data.wrapping_sub(1).min(255) as u8,
                    sleeps: o.flags & 0x4 != 0,
                }),
                _ => v3::actor_kind(o, &lvl).or_else(|| {
                    // the update power-ups: part 81 by style, the wearables 127–131
                    crate::actors::item_for_part(o.id).map(|what| Kind::PowerItem { what })
                }),
            };
            if o.id == 61 && o.flags & 0x4 == 0 && o.flags & 0x400 == 0 {
                // the Chain Chomp's stake is solid (WanwanPile has a collision body)
                lvl.set(tx, ty, Cell::Solid);
            }
            if let Some(kind) = actor {
                // inside a pipe (flag bit 0): the object sits on the pipe's entry point
                let (ox, oy) = (o.x as f32 / 10.0, o.y as f32 / 10.0);
                let in_pipe = (o.flags & 1 != 0)
                    .then(|| {
                        pipe_entries
                            .iter()
                            .min_by(|a, b| {
                                ((a.0 - ox).abs() + (a.1 - oy).abs())
                                    .total_cmp(&((b.0 - ox).abs() + (b.1 - oy).abs()))
                            })
                            .copied()
                    })
                    .flatten()
                    .filter(|e| (e.0 - ox).abs() + (e.1 - oy).abs() <= 16.0);
                if let Some((ex, ey, dir, colour)) = in_pipe {
                    if matches!(kind, Kind::Piranha { .. }) {
                        let travel =
                            24.0 * if big { 2.0 } else { 1.0 } + if big { 4.0 } else { 12.0 };
                        let (hx, hy) = match dir {
                            PipeDir::Up => (ex, ey - travel),
                            PipeDir::Down => (ex, ey + travel),
                            PipeDir::Right => (ex - travel, ey),
                            PipeDir::Left => (ex + travel, ey),
                        };
                        lvl.spawns.push(Spawn {
                            kind,
                            x: hx,
                            y: hy,
                            big,
                            wings,
                            pipe: Some((dir, ex, ey)),
                            parachute: false,
                            rail: None,
                            link: -1,
                            clown: false,
                            alt: false,
                        });
                    } else {
                        lvl.generators.push(PipeGenerator {
                            kind,
                            big,
                            wings,
                            dir,
                            x: ex,
                            y: ey,
                            colour,
                        });
                    }
                    continue;
                }
                // actors: x is the horizontal centre, y the centre of the bottom row
                lvl.spawns.push(Spawn {
                    kind,
                    x: ox,
                    y: oy - 8.0,
                    big,
                    wings,
                    pipe: None,
                    parachute: o.flags & 0x8000 != 0,
                    rail: (o.flags & 0x400 != 0).then_some(((o.flags >> 20) & 3) as u8),
                    link: o.link_id,
                    clown: o.flags & 0x200 != 0,
                    alt: o.flags & 0x4 != 0,
                });
                continue;
            }
            if v3::place_tile(&mut lvl, o, tx, ty) {
                continue;
            }
            let on_track = o.flags & 0x400 != 0;
            // a fixed object on a track: an actor carries it (Kind::TrackMount)
            let mount = |lvl: &mut Level, what: u8, index: usize, h: i32| {
                lvl.spawns.push(Spawn {
                    kind: Kind::TrackMount {
                        what,
                        index: index as u16,
                        h: h as u8,
                    },
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE,
                    big: false,
                    wings: false,
                    pipe: None,
                    parachute: false,
                    rail: Some(((o.flags >> 20) & 3) as u8),
                    link: o.link_id,
                    clown: false,
                    alt: false,
                });
            };
            if o.id == 13 {
                // Bill Blaster: a solid column, anchored at its bottom tile
                if !on_track {
                    for dy in 0..hgt {
                        lvl.set(tx, ty + dy, Cell::Solid);
                    }
                }
                let contents = match o.child_id {
                    0 => Some(Kind::Goomba),
                    1 => Some(Kind::Koopa {
                        red: o.child_flags & 0x4 != 0,
                    }),
                    _ => None,
                };
                lvl.blasters.push(Blaster {
                    x: tx as f32 * TILE + TILE / 2.0,
                    y: ty as f32 * TILE,
                    h: hgt.max(2),
                    contents,
                    homing: o.flags & 0x4 != 0,
                });
                if on_track {
                    let i = lvl.blasters.len() - 1;
                    mount(&mut lvl, 3, i, hgt.max(2));
                }
                continue;
            }
            if o.id == 89 {
                // custom autoscroll node (89-autoscroll-camera.md): number (flags >> 20) & 0xF,
                // speed (flags >> 18) & 3; the camera centre follows the nodes in order
                lvl.scroll_nodes.push((
                    ((o.flags >> 20) & 0xF) as u8,
                    o.x as f32 / 10.0,
                    o.y as f32 / 10.0,
                    ((o.flags >> 18) & 3) as u8,
                ));
                continue;
            }
            if o.id == 108 {
                // blinking block (108-blinking-block.md): the tile hit test (0x71003e14a0,
                // attribute 13) makes variant 0 solid while the area's blink state is 0 — its
                // value at the start — and variant 1 solid while it is 1 [obs]; the variant is the
                // 0x4 flag: a tile part's variant is (flags >> 18) & 3, or the 0x4 bit when that
                // field is 0 (0x71003d2600), and the editor shows the 0x4 block blue (model
                // 0xF0D "ChikaChikaBlockBlue", else 0xF0C red; 0x71003c2660) [obs]
                let alt = o.flags & 0x4 != 0;
                lvl.blink_blocks.push((tx, ty, alt));
                lvl.set(tx, ty, if alt { Cell::Empty } else { Cell::Solid });
                continue;
            }
            if o.id == 110 {
                // spike block (110-spike-block.md): a solid 2×2 block whose spikes come and go
                for dx in 0..2 {
                    for dy in 0..2 {
                        lvl.set(tx + dx, ty + dy, Cell::Solid);
                    }
                }
                lvl.spike_blocks.push((tx, ty, spike_block_mode(o.flags)));
                continue;
            }
            if o.id == 64 {
                // vine: a climb area, not solid; extended data holds the top and bottom tile ids
                // (0x7F00_7D / 0x7F00_7E), the middle uses 0x7E (64-vine.md)
                let top = (o.extended_data >> 16) & 0xFF;
                let bottom = o.extended_data & 0xFF;
                for dy in 0..hgt {
                    let atlas = if dy == hgt - 1 && top != 0 {
                        top
                    } else if dy == 0 && bottom != 0 {
                        bottom
                    } else {
                        0x7E
                    };
                    lvl.tiles.push(TileDraw {
                        pipe: false,
                        back: true,
                        x: tx,
                        y: ty + dy,
                        atlas,
                        flip: 0,
                    });
                }
                lvl.vines.push((
                    tx as f32 * TILE + 8.0,
                    ty as f32 * TILE,
                    (ty + hgt) as f32 * TILE,
                ));
                continue;
            }
            if o.id == 53 {
                // conveyor: solid belt, direction (flags >> 3) & 3 (0 left, 1 right), fast
                // when (flags >> 18) & 3 = 1 (53-conveyor-belt.md)
                let right = (o.flags >> 3) & 3 == 1;
                let fast = (o.flags >> 18) & 3 == 1;
                let speed = if fast { 2.0 } else { 1.0 } * if right { 1.0 } else { -1.0 };
                for dx in 0..w {
                    lvl.set(tx + dx, ty, Cell::Solid);
                    lvl.tiles.push(TileDraw {
                        pipe: false,
                        back: true,
                        x: tx + dx,
                        y: ty,
                        // [inf] placeholder art (bridge): the belt's look is style art (3D World
                        // draws `3W_Object_conveyor` models, table 0x71020fb4b0), which needs the
                        // game's atlases to place
                        atlas: 49,
                        flip: 0,
                    });
                }
                lvl.conveyors.push((tx, tx + w - 1, ty, speed));
                continue;
            }
            if o.id == 67 {
                // one-way wall: 2×2, x on the tile edge (centre), y the centre of the bottom row;
                // orientation (flags >> 22) & 3: 0 pass right, 1 pass down, 2 pass left, 3 floor
                let cx = o.x as f32 / 10.0;
                let by = o.y as f32 / 10.0 - 8.0;
                let (l, b) = (
                    ((cx - 16.0) / TILE).round() as i32,
                    (by / TILE).round() as i32,
                );
                match (o.flags >> 22) & 3 {
                    0 => {
                        lvl.set(l + 1, b, Cell::PassRight);
                        lvl.set(l + 1, b + 1, Cell::PassRight);
                    }
                    1 => {
                        lvl.set(l, b, Cell::PassDown);
                        lvl.set(l + 1, b, Cell::PassDown);
                    }
                    2 => {
                        lvl.set(l, b, Cell::PassLeft);
                        lvl.set(l, b + 1, Cell::PassLeft);
                    }
                    _ => {
                        lvl.set(l, b + 1, Cell::SemiTop);
                        lvl.set(l + 1, b + 1, Cell::SemiTop);
                    }
                }
                lvl.oneway.push((cx, by, ((o.flags >> 22) & 3) as u8));
                continue;
            }
            if o.id == 106 {
                // tree: 4 wide, x = trunk centre, y = centre of the bottom row
                let t = Tree {
                    x: o.x as f32 / 10.0,
                    y: o.y as f32 / 10.0,
                    h: hgt,
                    contents: Contents::from_part(o.child_id, o.child_flags),
                };
                // the trunk climbs like a vine, up into the canopy [inf: the trunk is BG unit
                // group 0x1C (0x7100396ff0) and the player climbs it in its own states
                // TreeClimb / TreeClimbTop / TreeClimbJump (player code, not traced)]
                lvl.vines.push((t.x, t.y - 8.0, t.canopy().1));
                lvl.trees.push(t);
                continue;
            }
            if o.id == 119 {
                // ! block: 2×2, x = its horizontal centre, y = the centre of its bottom row; path
                // number = extended data − 1 (course-loading.md §1.6)
                let tile = (
                    ((o.x as f32 / 10.0 - TILE) / TILE).round() as i32,
                    ((o.y as f32 / 10.0 - 8.0) / TILE).round() as i32,
                );
                let idx = o.extended_data.wrapping_sub(1) as u8;
                let codes: Vec<u16> = area
                    .exclamation_blocks
                    .slots
                    .values()
                    .find(|p| p.head.index == idx)
                    .map(|p| p.nodes.iter().map(|n| n.direction as u16).collect())
                    .unwrap_or_default();
                lvl.exclamations.push((tile, codes));
                continue;
            }
            if o.id == 84 {
                // snake block: path from the snake section (extended data & 0xF is its index + 1:
                // every sample course stores it so [obs corpus, 65 areas]), head on the left when
                // extended data bit 0x10 ("start from the other end") is set
                let idx = ((o.extended_data & 0xF) as u8).wrapping_sub(1);
                let codes: Vec<u16> = area
                    .snake_blocks
                    .slots
                    .values()
                    .find(|sb| sb.head.index == idx)
                    .map(|sb| sb.nodes.iter().map(|n| n.direction).collect())
                    .unwrap_or_default();
                lvl.snakes.push((
                    tx,
                    ty,
                    w as usize,
                    o.flags & 0x4 != 0,
                    o.extended_data & 0x10 != 0,
                    codes,
                ));
                continue;
            }
            if o.id == 47 {
                // cannon: solid block
                if !on_track {
                    lvl.set(tx, ty, Cell::Solid);
                }
                lvl.cannons.push(Cannon {
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE,
                    angle: cannon_angle(o.flags),
                    red: o.flags & 0x4 != 0,
                });
                if on_track {
                    let i = lvl.cannons.len() - 1;
                    mount(&mut lvl, 2, i, 1);
                }
                continue;
            }
            if o.id == 54 {
                if !on_track {
                    lvl.set(tx, ty, Cell::Solid);
                }
                let (bx, by) = (tx as f32 * TILE + 8.0, ty as f32 * TILE);
                let dir = match (o.flags >> 25) & 3 {
                    0 => 0,
                    1 => 1,
                    2 => 2,
                    _ => 3,
                };
                lvl.burners.push(Burner {
                    x: bx,
                    y: by,
                    dir,
                    alt: o.flags & 0x4 != 0,
                });
                if on_track {
                    let i = lvl.burners.len() - 1;
                    mount(&mut lvl, 1, i, 1);
                }
                continue;
            }
            if o.id == 24 {
                // pivot block: solid; length bits 22–24, direction bit 3, start angle in the
                // extended data (5° units) [ext docs/formats/smm2-course.md]
                if !on_track {
                    lvl.set(tx, ty, Cell::Solid);
                }
                lvl.firebars.push(FireBar {
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE + 8.0,
                    n: 4 + 2 * ((o.flags >> 22) & 7),
                    start: (o.extended_data & 0xFF) as f32 * 5.0,
                    dir: if (o.flags >> 3) & 1 == 1 { 1.0 } else { -1.0 },
                });
                if on_track {
                    let i = lvl.firebars.len() - 1;
                    mount(&mut lvl, 0, i, 1);
                }
                continue;
            }
            if o.id == 43 {
                for dx in 0..w {
                    for dy in 0..hgt {
                        lvl.set(tx + dx, ty + dy, Cell::Spike);
                    }
                }
                continue;
            }
            if o.id == 90 {
                lvl.checkpoints.push(Checkpoint {
                    x: o.x as f32 / 10.0,
                    y: o.y as f32 / 10.0 - 8.0,
                    contents: Contents::from_part(o.child_id, o.child_flags),
                });
                continue;
            }
            if actorish && o.id == 8 {
                lvl.spawns.push(Spawn {
                    kind: Kind::FloatCoin,
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE,
                    big: false,
                    wings: o.flags & 0x2 != 0,
                    pipe: None,
                    parachute: false,
                    rail: (o.flags & 0x400 != 0).then_some(((o.flags >> 20) & 3) as u8),
                    link: -1,
                    clown: false,
                    alt: false,
                });
                continue;
            }
            if actorish && matches!(o.id, 4 | 5 | 6 | 22 | 23 | 29 | 63) {
                // a winged or tracked block becomes a moving solid actor (4-brick.md B12)
                let block = block_kind(o.id).unwrap();
                lvl.spawns.push(Spawn {
                    kind: Kind::MovingBlock {
                        block,
                        contents: Contents::from_part(o.child_id, o.child_flags),
                    },
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE,
                    big: false,
                    wings: o.flags & 0x2 != 0,
                    pipe: None,
                    parachute: false,
                    rail: (o.flags & 0x400 != 0).then_some(((o.flags >> 20) & 3) as u8),
                    link: -1,
                    clown: false,
                    alt: false,
                });
                continue;
            }
            match o.id {
                8 if !actorish => lvl.coins.push(CoinTile { x: tx, y: ty }),
                92 => lvl.pink_coins.push(CoinTile { x: tx, y: ty }),
                id if !actorish && block_kind(id).is_some() => {
                    let mut kind = block_kind(id).unwrap();
                    if id == 100 && o.flags & 0x4 != 0 {
                        kind = BlockKind::Dotted { blue: true };
                    }
                    let contents = Contents::from_part(o.child_id, o.child_flags);
                    let cell = match kind {
                        BlockKind::Cloud | BlockKind::Donut => Cell::SemiTop,
                        BlockKind::Hidden | BlockKind::Dotted { blue: true } => Cell::Empty,
                        _ => Cell::Solid,
                    };
                    for dx in 0..w {
                        for dy in 0..hgt {
                            lvl.set(tx + dx, ty + dy, cell);
                            lvl.blocks.push(BlockPlacement {
                                x: tx + dx,
                                y: ty + dy,
                                kind,
                                contents,
                            });
                        }
                    }
                }
                9 => {
                    // pipe (9-pipe.md): anchor = base tile; 2 wide, `height` long, mouth at the
                    // far end; direction (flags >> 5) & 3, warp id (flags >> 20) & 0x1F
                    let len = hgt;
                    let dir = pipe_dir(o.flags);
                    let (ax, ay) = (o.x as f32 / 10.0, o.y as f32 / 10.0);
                    let (ix, iy) = ((ax / TILE).floor() as i32, (ay / TILE).floor() as i32);
                    lvl.add_pipe(ix, iy, len, dir);
                    let l = (len - 1) as f32 * TILE;
                    let (mx, my) = match dir {
                        PipeDir::Right => (ax + l, ay - 8.0),
                        PipeDir::Left => (ax - l, ay + 8.0),
                        PipeDir::Up => (ax + 8.0, ay + l),
                        PipeDir::Down => (ax - 8.0, ay - l),
                    };
                    let id = ((o.flags >> 20) & 0x1F) as u8;
                    if id != 0 {
                        lvl.warps.push(Warp {
                            kind: WarpKind::Pipe(dir),
                            id,
                            x: mx,
                            y: my,
                            len,
                        });
                    }
                }
                55 => {
                    // door: 1×2 tiles, origin at the bottom centre
                    let (dx, dy) = (o.x as f32 / 10.0, o.y as f32 / 10.0 - 8.0);
                    let kind = ((o.flags >> 18) & 3) as u8;
                    lvl.doors.push((dx, dy, kind));
                    lvl.warps.push(Warp {
                        kind: WarpKind::Door(kind),
                        id: ((o.flags >> 20) & 0x1F) as u8,
                        x: dx,
                        y: dy,
                        len: 2,
                    });
                }
                97 => {
                    // warp box: one tile, origin at the bottom centre; pairs 2k ↔ 2k+1 (5 bits)
                    lvl.warps.push(Warp {
                        kind: WarpKind::Box {
                            key: o.flags & 0x4 != 0,
                        },
                        id: ((o.flags >> 20) & 0x1F) as u8,
                        x: o.x as f32 / 10.0,
                        y: o.y as f32 / 10.0 - 8.0,
                        len: 1,
                    });
                }
                16 | 14 | 17 | 71 => {
                    // semisolid / mushroom platform / bridge / half-collision platform: one-way top row
                    let y_top = ty + hgt - 1;
                    for dx in 0..w {
                        lvl.set(tx + dx, y_top, Cell::SemiTop);
                        let atlas = match o.id {
                            14 => {
                                35 + if dx == 0 {
                                    0
                                } else if dx == w - 1 {
                                    2
                                } else {
                                    1
                                }
                            }
                            17 => 49,
                            _ => {
                                55 + if dx == 0 {
                                    0
                                } else if dx == w - 1 {
                                    2
                                } else {
                                    1
                                }
                            }
                        };
                        lvl.tiles.push(TileDraw {
                            pipe: false,
                            back: true,
                            flip: 0,
                            x: tx + dx,
                            y: y_top,
                            atlas,
                        });
                    }
                }
                87 | 88 | 94 => {
                    // slopes: bit 20 = 0 rises to the right ("/"), 1 falls to the right ("\");
                    // a conveyor slope (94) is a steep slope that carries like a belt
                    // (53-conveyor-belt.md: vector (±1, ±1), fast (±2, ±2))
                    let rising = (o.flags >> 20) & 1 == 0;
                    let steep = o.id != 87;
                    if o.id == 94 {
                        let right = (o.flags >> 3) & 3 == 1;
                        let fast = (o.flags >> 18) & 3 == 1;
                        let speed = if fast { 2.0 } else { 1.0 } * if right { 1.0 } else { -1.0 };
                        for dx in 0..w {
                            for dy in 0..hgt {
                                lvl.conveyors.push((tx + dx, tx + dx, ty + dy, speed));
                            }
                        }
                    }
                    let slope = Slope {
                        left: tx,
                        bottom: ty,
                        width: w,
                        height: hgt,
                        rising,
                        steep,
                    };
                    for dx in 0..w {
                        for dy in 0..hgt {
                            // cells entirely under the surface are solid; surface cells are
                            // resolved against the slope line
                            let (cx0, cx1) = ((tx + dx) as f32 * TILE, (tx + dx + 1) as f32 * TILE);
                            let low = slope.surface(cx0).min(slope.surface(cx1 - 0.01));
                            let cell_top = (ty + dy + 1) as f32 * TILE;
                            if cell_top <= low + 0.01 {
                                lvl.set(tx + dx, ty + dy, Cell::Solid);
                            }
                        }
                    }
                    lvl.draw_slope(&slope);
                    lvl.slopes.push(slope);
                }
                _ => {}
            }
        }
        if lvl.autoscroll == 4 {
            lvl.finish_custom_scroll(course, area_index);
        }
        lvl
    }

    /// Custom autoscroll path (0x7100b61fd0) [obs]: every node's point is clamped to the range
    /// of view centres; in the main area the path also starts at the autoscroll start unit and
    /// ends at the end unit, the two fixed units the header rebuilds (0x71005263f0: start x =
    /// min(right − 160, 160) + 24, end x = max(right − 160, 160) − 24; the start's speed field is
    /// header byte 0x0E & 3). Their y is (start_y + 4)·16 − 8 and (goal_y + 4)·16 − 8, clamped
    /// to 104…328 (104 when the area's boundary type is set), + 4. The nodes fill 12 slots by
    /// number: the start is slot 0 (a course node numbered 0 takes its place), the end slot 11
    /// (it replaces a course node 11). So a custom-autoscroll course without nodes scrolls from
    /// the start to the goal.
    fn finish_custom_scroll(&mut self, course: &Course, area_index: usize) {
        use crate::world::{VIEW_H, VIEW_W};
        let span = |v: f32, lo: f32, hi: f32| {
            if lo > hi {
                (lo + hi) / 2.0
            } else {
                v.clamp(lo, hi)
            }
        };
        let (x0, x1) = (self.left + VIEW_W / 2.0, self.right - VIEW_W / 2.0);
        let (y0, y1) = (self.bottom + VIEW_H / 2.0, self.top - VIEW_H / 2.0);
        if area_index == 0 {
            let y = |h: u8| {
                let y = if self.boundary_type != 0 {
                    104.0
                } else {
                    ((h as f32 + 4.0) * TILE - 8.0).clamp(104.0, 328.0)
                };
                y + 4.0
            };
            let edge = self.right - 160.0;
            let speed = course.header.autoscroll_speed & 3;
            let (start, end) = (y(course.header.start_y), y(course.header.goal_y));
            if !self.scroll_nodes.iter().any(|n| n.0 == 0) {
                self.scroll_nodes
                    .push((0, edge.min(160.0) + 24.0, start, speed));
            }
            self.scroll_nodes.retain(|n| n.0 != 11);
            self.scroll_nodes.push((11, edge.max(160.0) - 24.0, end, 0));
        }
        for n in &mut self.scroll_nodes {
            n.1 = span(n.1, x0, x1);
            n.2 = span(n.2, y0, y1);
        }
    }

    /// Slope art from the field atlas: per column, the cell holding the surface gets a 45° piece
    /// (atlas row 24) or a lower/upper 2:1 half piece (row 28); cells below get interior ground,
    /// and the end caps the ground top tile.
    /// [inf] atlas positions read off the M1 atlas: the game's per-cell tile codes (layout
    /// tables 0x7101cfd9d0 / 0x7101cfdaf0) map to atlas cells only through the game's art.
    fn draw_slope(&mut self, sl: &Slope) {
        for dx in 0..sl.width {
            let x = sl.left + dx;
            let (x0, x1) = (x as f32 * TILE, (x + 1) as f32 * TILE - 0.01);
            let (s0, s1) = (sl.surface(x0), sl.surface(x1));
            let (lo, hi) = (s0.min(s1), s0.max(s1));
            if hi - lo < 1.0 {
                continue; // flat part (steep slope ends): interior below, nothing on top
            }
            let row = (lo / TILE).floor() as i32;
            for y in sl.bottom..row {
                self.tiles.push(TileDraw {
                    pipe: false,
                    back: false,
                    flip: 0,
                    x,
                    y,
                    atlas: GROUND_ATLAS_BASE + 12,
                });
            }
            let rising = s1 > s0;
            let atlas = if hi - lo > 12.0 {
                if rising { 24 * 16 } else { 24 * 16 + 1 }
            } else {
                let upper = lo - row as f32 * TILE >= 7.9;
                match (rising, upper) {
                    (true, false) => 28 * 16,
                    (true, true) => 28 * 16 + 1,
                    (false, true) => 28 * 16 + 2,
                    (false, false) => 28 * 16 + 3,
                }
            };
            self.tiles.push(TileDraw {
                pipe: false,
                back: false,
                flip: 0,
                x,
                y: row,
                atlas,
            });
        }
    }

    /// Pipe terrain and art. (ix, iy) is the anchor tile; see 9-pipe.md for the shapes: up =
    /// columns ix, ix+1 rising from iy; down = columns ix−1, ix falling; right = rows iy−1, iy
    /// running right; left = rows iy, iy+1 running left. Atlas (M1): vertical lip 14/15 over body
    /// 30/31 (row 1); horizontal lip 11 (+16 for the lower half) with body 12/28, drawn for a
    /// left-facing mouth and mirrored for right.
    pub fn add_pipe(&mut self, ix: i32, iy: i32, len: i32, dir: PipeDir) {
        for i in 0..len {
            let lip = i == len - 1;
            for k in 0..2 {
                let (x, y, atlas, flip) = match dir {
                    PipeDir::Up => (ix + k, iy + i, if lip { 14 } else { 30 } + k as u32, 0),
                    PipeDir::Down => (ix - 1 + k, iy - i, if lip { 14 } else { 30 } + k as u32, 2),
                    PipeDir::Left => (
                        ix - i,
                        iy + 1 - k,
                        if lip { 11 } else { 12 } + 16 * k as u32,
                        0,
                    ),
                    PipeDir::Right => {
                        (ix + i, iy - k, if lip { 11 } else { 12 } + 16 * k as u32, 1)
                    }
                };
                self.set(x, y, Cell::Solid);
                self.tiles.push(TileDraw {
                    x,
                    y,
                    atlas,
                    flip,
                    pipe: true,
                    back: false,
                });
            }
        }
    }

    /// Highest slope surface at x (px), if any slope covers x.
    pub fn slope_at(&self, px: f32) -> Option<(f32, &Slope)> {
        self.slopes
            .iter()
            .filter(|s| s.covers(px))
            .map(|s| (s.surface(px), s))
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    fn fill_ground(&mut self, left: i32, width: i32, height: i32) {
        for x in left..left + width {
            for y in 0..height {
                self.set(x, y, Cell::Solid);
                let id = if y == height - 1 { 10 } else { 12 }; // top edge / interior
                self.tiles.push(TileDraw {
                    pipe: false,
                    back: false,
                    flip: 0,
                    x,
                    y,
                    atlas: GROUND_ATLAS_BASE + id,
                });
            }
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let gx = x - self.min_x;
        (gx >= 0 && gx < self.width && y >= 0 && y < self.height)
            .then(|| (y * self.width + gx) as usize)
    }

    pub fn set(&mut self, x: i32, y: i32, c: Cell) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] = c;
        }
    }

    /// Cell at tile coordinates; outside the grid is empty (pits, sky).
    pub fn cell(&self, x: i32, y: i32) -> Cell {
        // the bottom row continues below the course: a pit's walls go all the way down
        // [obs: Ryujinx trace, the player blocked by a pit wall below y = 0]
        let y = y.max(0);
        self.index(x, y)
            .map(|i| self.cells[i])
            .unwrap_or(Cell::Empty)
    }

    /// Cell containing a pixel position.
    pub fn cell_at(&self, px: f32, py: f32) -> Cell {
        self.cell((px / TILE).floor() as i32, (py / TILE).floor() as i32)
    }

    /// Test helper: make one tile solid.
    pub fn set_solid(&mut self, x: i32, y: i32) {
        self.set(x, y, Cell::Solid);
    }

    /// Test helper: add a slope and a solid plateau of `plateau_h` tiles from tile `plateau_x` on.
    pub fn add_test_slope(&mut self, slope: Slope, plateau_x: i32, plateau_h: i32) {
        let right = (self.right / TILE) as i32;
        for x in plateau_x..right {
            for y in 0..plateau_h {
                self.set(x, y, Cell::Solid);
            }
        }
        self.slopes.push(slope);
    }

    /// A short demo course with one of each core object and a goal (no course loaded; also the
    /// screenshot smoke test).
    pub fn demo() -> Self {
        let mut lvl = Self::flat(80);
        lvl.timer = 300;
        let block = |lvl: &mut Level, x: i32, y: i32, kind: BlockKind, contents| {
            lvl.set(x, y, Cell::Solid);
            lvl.blocks.push(BlockPlacement {
                x,
                y,
                kind,
                contents,
            });
        };
        block(
            &mut lvl,
            10,
            5,
            BlockKind::Question,
            Some(Contents::Mushroom),
        );
        block(&mut lvl, 14, 5, BlockKind::Brick, None);
        block(&mut lvl, 15, 5, BlockKind::Question, None);
        block(&mut lvl, 16, 5, BlockKind::Brick, Some(Contents::Coin));
        block(
            &mut lvl,
            15,
            9,
            BlockKind::Question,
            Some(Contents::FireFlower),
        );
        for x in 20..23 {
            lvl.coins.push(CoinTile { x, y: 4 });
        }
        lvl.spawns.push(Spawn {
            kind: Kind::Goomba,
            x: 26.0 * TILE,
            y: 32.0,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
            rail: None,
            link: -1,
            clown: false,
            alt: false,
        });
        lvl.spawns.push(Spawn {
            kind: Kind::Koopa { red: false },
            x: 40.0 * TILE,
            y: 32.0,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
            rail: None,
            link: -1,
            clown: false,
            alt: false,
        });
        // pipe
        lvl.add_pipe(46, 2, 2, PipeDir::Up);
        // goal: pole on a hard block 2 tiles from the end, as from a course header
        let pole = 66;
        block(&mut lvl, pole, 2, BlockKind::Hard, None);
        lvl.goal = Some((pole as f32 * TILE + TILE / 2.0, 3.0 * TILE));
        lvl
    }

    /// A flat test strip.
    pub fn flat(width_tiles: i32) -> Self {
        let mut lvl = Level {
            min_x: -4,
            width: width_tiles + 5,
            height: 28,
            cells: vec![Cell::Empty; ((width_tiles + 5) * 28) as usize],
            tiles: Vec::new(),
            coins: Vec::new(),
            slopes: Vec::new(),
            blocks: Vec::new(),
            spawns: Vec::new(),
            warps: Vec::new(),
            solids: Vec::new(),
            generators: Vec::new(),
            checkpoints: Vec::new(),
            blasters: Vec::new(),
            conveyors: Vec::new(),
            burners: Vec::new(),
            firebars: Vec::new(),
            cannons: Vec::new(),
            snakes: Vec::new(),
            exclamations: Vec::new(),
            trees: Vec::new(),
            track_paths: Vec::new(),
            blink_blocks: Vec::new(),
            spike_blocks: Vec::new(),
            scroll_nodes: Vec::new(),
            pink_coins: Vec::new(),
            vines: Vec::new(),
            v3: V3Tiles::default(),
            tracks: Default::default(),
            creepers: Vec::new(),
            liquid: None,
            oneway: Vec::new(),
            doors: Vec::new(),
            goal: None,
            style: "M1".into(),
            theme: 0,
            night: false,
            start: (56.0, 32.0),
            left: 0.0,
            right: width_tiles as f32 * TILE,
            top: 432.0,
            bottom: 0.0,
            boundary_type: 0,
            vertical: false,
            main_area: true,
            autoscroll: 0,
            timer: 300,
        };
        lvl.fill_ground(-3, width_tiles + 3, 2);
        lvl
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_formats::smm2::CourseObject;

    /// A part at tile (x, y) of footprint w × h.
    fn object(id: i16, x: i32, y: i32, w: u8, h: u8, flags: u32) -> CourseObject {
        use mmk_formats::record::Record;
        let mut o = CourseObject::parse(&[0; 0x20]);
        (o.x, o.y, o.width, o.height) = (x * 160 + 80, y * 160 + 80, w, h);
        (o.flags, o.child_flags) = (0x0600_0040 | flags, 0x0600_0040);
        (o.id, o.child_id, o.link_id, o.sound_effect_id) = (id, -1, -1, -1);
        o
    }

    /// The main area of a blank SMB1 course holding the objects.
    fn level(objects: Vec<CourseObject>, autoscroll: u8) -> Level {
        let mut c = Course::blank("M1");
        c.main.header.autoscroll_type = autoscroll;
        c.main.objects.items = objects;
        Level::from_course(&c, 0)
    }

    #[test]
    fn cannon_aims_from_its_facing_and_aim_fields() {
        let f = |side: u32, aim: u32| cannon_angle((side << 25) | (aim << 22));
        // facing up (3): aims 135, 90, 45, 0, 180
        assert_eq!(
            [f(3, 0), f(3, 1), f(3, 2), f(3, 3), f(3, 4)],
            [135.0, 90.0, 45.0, 0.0, 180.0]
        );
        // the middle aim points along the facing: left, right, down
        assert_eq!([f(0, 1), f(1, 1), f(2, 1)], [180.0, 0.0, 270.0]);
    }

    #[test]
    fn spike_block_colours() {
        let lvl = level(
            vec![
                object(110, 10, 4, 2, 2, 0),
                object(110, 14, 4, 2, 2, 1 << 18),
                object(110, 18, 4, 2, 2, 2 << 18),
            ],
            0,
        );
        // red: out while OFF (2); blue: out while ON (1); 2: the timer block (0)
        let modes: Vec<u8> = lvl.spike_blocks.iter().map(|b| b.2).collect();
        assert_eq!(modes, [2, 1, 0]);
    }

    #[test]
    fn goal_pole_stands_over_the_goal_grounds_first_column() {
        let lvl = level(vec![], 0);
        let goal_left = ((lvl.right - 160.0) / TILE).round() as i32;
        let (x, y) = lvl.goal.unwrap();
        assert_eq!(x, lvl.right - 152.0);
        // goal_y 1: the base block on the ground, its top at 32
        assert_eq!(y, 2.0 * TILE);
        assert_eq!(lvl.cell(goal_left, 1), Cell::Solid);
    }

    #[test]
    fn custom_autoscroll_runs_from_the_start_unit_to_the_end_unit() {
        use crate::world::{VIEW_H, VIEW_W};
        // no nodes: the path is the start node (0) and the end node (11); start_y = goal_y = 1
        // give y = (1 + 4)·16 − 8 = 72 → 104, + 4
        let lvl = level(vec![], 4);
        let nodes: Vec<_> = lvl.scroll_nodes.iter().map(|n| (n.0, n.1, n.2)).collect();
        assert_eq!(
            nodes,
            [
                (0, VIEW_W / 2.0, 108.0),
                (11, lvl.right - VIEW_W / 2.0, 108.0)
            ]
        );
        // a course node is clamped into the range of view centres
        let lvl = level(vec![object(89, 0, 26, 3, 1, 1 << 20)], 4);
        let n1 = lvl.scroll_nodes.iter().find(|n| n.0 == 1).unwrap();
        assert_eq!((n1.1, n1.2), (VIEW_W / 2.0, lvl.top - VIEW_H / 2.0));
    }
}
