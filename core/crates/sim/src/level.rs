//! A playable area built from SMM2 course data: collision cells and the tiles to draw.
//!
//! Sources: research/specs/course-loading.md (ground placement, start/goal ground from the header,
//! object anchoring) and the format doc. Ground records are tile coordinates with y up from the
//! bottom row; objects are in 1/160-tile units, anchored at the horizontal centre of their bottom
//! row. Terrain art: SMB1 field atlases are 16 tiles wide and ground tile id n is atlas index
//! 184 + n.

use mmk_formats::smm2::{Area, Course};

pub const TILE: f32 = 16.0;
const ATLAS_COLS: u32 = 16;
const GROUND_ATLAS_BASE: u32 = 184;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Solid,
    /// One-way platform: solid only from above.
    SemiTop,
}

/// A tile to draw: grid position and atlas index (row-major, 16 per row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileDraw {
    pub x: i32,
    pub y: i32,
    pub atlas: u32,
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
    pub style: String,
    pub theme: u8,
    pub night: bool,
    /// Player start (feet) in px.
    pub start: (f32, f32),
    /// Playable area in px.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub timer: i16,
}

/// Atlas indices of tile-like objects in SMB1 field atlases (first rows of the atlas).
fn object_atlas(id: i16) -> Option<u32> {
    Some(match id {
        4 => 1,    // brick
        5 => 2,    // ? block
        6 => 5,    // hard block
        23 => 4,   // note block
        21 => 82,  // donut block
        22 => 102, // cloud block
        63 => 120, // ice block
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
            style: course.header.game_style.as_str_lossy(),
            theme: h.theme,
            night: h.is_night(),
            start: (56.0, 0.0),
            left: h.boundary_left as f32,
            right,
            top,
            timer: course.header.timer,
        };

        // ground records
        for g in area.ground.iter() {
            let (x, y) = (g.x as i32, g.y as i32);
            lvl.set(x, y, Cell::Solid);
            lvl.tiles.push(TileDraw {
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
            lvl.fill_ground(goal_left, 11, course.header.goal_y as i32);
        }

        // tile-like objects
        for o in area.objects.iter() {
            let (w, hgt) = (o.width.max(1) as i32, o.height.max(1) as i32);
            let left_px = o.x as f32 / 10.0 - w as f32 * 8.0;
            let bottom_px = o.y as f32 / 10.0 - 8.0;
            let tx = (left_px / TILE).round() as i32;
            let ty = (bottom_px / TILE).round() as i32;
            let actorish = o.flags & 0x2 != 0 || o.flags & 0x400 != 0; // winged / on track → actor (step 6)
            match o.id {
                8 if !actorish => lvl.coins.push(CoinTile { x: tx, y: ty }),
                id if !actorish && object_atlas(id).is_some() => {
                    let atlas = object_atlas(id).unwrap();
                    let cell = if id == 22 { Cell::SemiTop } else { Cell::Solid };
                    for dx in 0..w {
                        for dy in 0..hgt {
                            lvl.set(tx + dx, ty + dy, cell);
                            lvl.tiles.push(TileDraw {
                                x: tx + dx,
                                y: ty + dy,
                                atlas,
                            });
                        }
                    }
                }
                9 => {
                    // pipe: solid rectangle; art for an upward pipe (lip on top)
                    for dx in 0..w {
                        for dy in 0..hgt {
                            lvl.set(tx + dx, ty + dy, Cell::Solid);
                            let lip = dy == hgt - 1;
                            let col = if dx == 0 { 14 } else { 15 };
                            lvl.tiles.push(TileDraw {
                                x: tx + dx,
                                y: ty + dy,
                                atlas: if lip { col } else { 16 + col },
                            });
                        }
                    }
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
                            x: tx + dx,
                            y: y_top,
                            atlas,
                        });
                    }
                }
                _ => {}
            }
        }
        lvl
    }

    fn fill_ground(&mut self, left: i32, width: i32, height: i32) {
        for x in left..left + width {
            for y in 0..height {
                self.set(x, y, Cell::Solid);
                let id = if y == height - 1 { 10 } else { 12 }; // top edge / interior
                self.tiles.push(TileDraw {
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

    fn set(&mut self, x: i32, y: i32, c: Cell) {
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

    /// A flat test strip (used when no course is loaded).
    pub fn flat(width_tiles: i32) -> Self {
        let mut lvl = Level {
            min_x: -4,
            width: width_tiles + 5,
            height: 28,
            cells: vec![Cell::Empty; ((width_tiles + 5) * 28) as usize],
            tiles: Vec::new(),
            coins: Vec::new(),
            style: "M1".into(),
            theme: 0,
            night: false,
            start: (56.0, 32.0),
            left: 0.0,
            right: width_tiles as f32 * TILE,
            top: 432.0,
            timer: 300,
        };
        lvl.fill_ground(-3, width_tiles + 3, 2);
        lvl
    }
}
