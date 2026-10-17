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

/// A slope object (87 gentle 2n×n, 88 steep (n+1)×n), in tiles.
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

    /// Surface height (px) at x. Gentle: straight line over the full width. Steep: 45° with a
    /// half-tile flat at each end ([inf]: the extra column of the (n+1)×n footprint).
    pub fn surface(&self, px: f32) -> f32 {
        let (x0, w, h) = (
            self.left as f32 * TILE,
            self.width as f32 * TILE,
            self.height as f32 * TILE,
        );
        let mut t = ((px - x0) / w).clamp(0.0, 1.0);
        if !self.rising {
            t = 1.0 - t;
        }
        let rise = if self.steep {
            let d = t * w - TILE / 2.0;
            d.clamp(0.0, h)
        } else {
            t * h
        };
        self.bottom as f32 * TILE + rise
    }

    /// Steepness index for the slope physics tables (player-physics.md §5.2): gentle → 1,
    /// steep → 3 ([inf] mapping).
    pub fn steepness(&self) -> usize {
        if self.steep { 3 } else { 1 }
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
}

/// An airship cannon (47): block origin, barrel angle (degrees), next shot timer kept by the
/// world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cannon {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
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
            WarpKind::Door(_) => (self.x, self.y),
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
    /// Track graph (59-track.md).
    pub tracks: crate::track::Tracks,
    /// Vines (64): climb areas (centre x, bottom, top) in px.
    pub vines: Vec<(f32, f32, f32)>,
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
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 43, 44, 46, 47, 48, 52, 53, 54, 55,
    56, 57, 58, 60, 61, 63, 64, 66, 67, 68, 70, 71, 76, 82, 83, 84, 87, 88, 90, 91, 95, 99, 100,
    102, 104, 105, 109,
];

/// Parts that become actors and so can ride a track (59-track.md).
const RIDES_TRACKS: &[i16] = &[
    0, 1, 2, 3, 10, 11, 12, 15, 18, 19, 20, 25, 28, 30, 31, 32, 33, 34, 35, 36, 39, 40, 41, 44, 46,
    48, 52, 56, 57, 58, 60, 61, 68, 70, 76, 82, 83, 91, 95, 102, 105,
];

/// Why a course is not fully playable yet: one entry per missing feature (empty = playable).
pub fn unsupported_features(course: &Course) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for (ai, area) in course.areas().iter().enumerate() {
        let h = &area.header;
        if ai == 1 && area.objects.is_empty() && area.ground.is_empty() {
            continue; // unused sub-area
        }

        if h.autoscroll_type == 4 {
            out.insert("custom autoscroll".to_string());
        }

        for o in area.objects.iter() {
            if !SUPPORTED_PARTS.contains(&o.id) {
                out.insert(format!("part {}", o.id));
            } else if o.flags & 0x400 != 0 && !RIDES_TRACKS.contains(&o.id) {
                // blocks, coins and fixed objects on tracks need moving versions: not yet
                out.insert(format!("part {} on a track", o.id));
            } else if o.flags & 0x2 != 0 && matches!(o.id, 4 | 5 | 6 | 11 | 12 | 13 | 29) {
                // winged blocks, lifts, Thwomps and Bill Blasters fly as actors: not yet
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
        // dotted-line block: colour from the alternate flag [?]
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
            vines: Vec::new(),
            tracks: Default::default(),
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
            lvl.start = (56.0, start_h as f32 * TILE); // x = 56 px: [inf] from the battle-start marker
            let goal_left = ((right - 152.0 - 8.0) / TILE).round() as i32;
            let goal_h = course.header.goal_y as i32;
            lvl.fill_ground(goal_left, 11, goal_h);
            // pole on a hard block in the goal ground's second column [inf: walk target X0+112
            // and the 80-px castle then end flush with the ground's right edge]
            let pole_tile = goal_left + 1;
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

        lvl.tracks = crate::track::Tracks::new(
            area.tracks
                .iter()
                .filter(|t| t.x != 0 || t.y != 0 || t.r#type != 0 || t.k0 != 0)
                .map(|t| (t.x, t.y, t.r#type)),
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
                0 => Some(Kind::Goomba),
                1 => Some(Kind::Koopa {
                    red: o.flags & 0x4 != 0,
                }),
                20 | 44 => Some(Kind::Mushroom),
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
                60 => Some(Kind::LavaBubble),
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
                }),
                70 => Some(Kind::BigCoin {
                    value: ((o.flags >> 18) & 3).min(2) as u8,
                }),
                10 => Some(Kind::Spring),
                18 => Some(Kind::PSwitch),
                25 => Some(Kind::Spiny),
                28 => Some(Kind::Buzzy),
                40 => Some(Kind::SpikeTop),
                3 => Some(Kind::HammerBro),
                15 => Some(Kind::BobOmb),
                46 => Some(Kind::DryBones),
                41 => Some(Kind::Boo),
                57 => Some(Kind::Muncher),
                12 => Some(Kind::Thwomp {
                    side: o.flags & 0x4 != 0,
                }),
                11 => Some(Kind::Lift {
                    width: o.width.max(1),
                    falling: o.flags & 0x4 != 0,
                }),
                _ => None,
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
                });
                continue;
            }
            if o.id == 13 {
                // Bill Blaster: a solid column, anchored at its bottom tile
                for dy in 0..hgt {
                    lvl.set(tx, ty + dy, Cell::Solid);
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
                });
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
                        x: tx + dx,
                        y: ty,
                        atlas: 49, // [inf] placeholder art (bridge) until the belt model is found
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
            if o.id == 84 {
                // snake block: path from the snake section (index = extended data & 0xF), head on
                // the left when extended data bit 0x10 ("start from the other end") is set
                let idx = (o.extended_data & 0xF) as u8;
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
                // cannon: solid block; barrel direction from the facing field (flags >> 24) & 7
                // (even values left/right/down/up as for Piranha Plants; odd = diagonals [inf])
                lvl.set(tx, ty, Cell::Solid);
                let angle = match (o.flags >> 24) & 7 {
                    0 => 180.0,
                    1 => 225.0,
                    2 => 0.0,
                    3 => 315.0,
                    4 => 270.0,
                    5 => 135.0,
                    6 => 90.0,
                    _ => 45.0,
                };
                lvl.cannons.push(Cannon {
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE,
                    angle,
                });
                continue;
            }
            if o.id == 54 {
                lvl.set(tx, ty, Cell::Solid);
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
                continue;
            }
            if o.id == 24 {
                // pivot block: solid; length bits 22–24, direction bit 3, start angle in the
                // extended data (5° units) [ext docs/formats/smm2-course.md]
                lvl.set(tx, ty, Cell::Solid);
                lvl.firebars.push(FireBar {
                    x: tx as f32 * TILE + 8.0,
                    y: ty as f32 * TILE + 8.0,
                    n: 4 + 2 * ((o.flags >> 22) & 7),
                    start: (o.extended_data & 0xFF) as f32 * 5.0,
                    dir: if (o.flags >> 3) & 1 == 1 { 1.0 } else { -1.0 },
                });
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
            match o.id {
                8 if !actorish => lvl.coins.push(CoinTile { x: tx, y: ty }),
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
                            flip: 0,
                            x: tx + dx,
                            y: y_top,
                            atlas,
                        });
                    }
                }
                87 | 88 => {
                    // slopes: bit 20 = 0 rises to the right ("/"), 1 falls to the right ("\")
                    let rising = (o.flags >> 20) & 1 == 0;
                    let steep = o.id == 88;
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
        lvl
    }

    /// Slope art from the field atlas: per column, the cell holding the surface gets a 45° piece
    /// (atlas row 24) or a lower/upper 2:1 half piece (row 28); cells below get interior ground.
    /// [inf] atlas positions read off the M1 atlas; to be checked against the game.
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
                self.tiles.push(TileDraw { x, y, atlas, flip });
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
            vines: Vec::new(),
            tracks: Default::default(),
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
