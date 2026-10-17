//! The world: one level (area) and the player.

use mmk_formats::smm2::Course;

use crate::input::Input;
use crate::level::{Level, TILE};
use crate::player::{Player, Pose};
use crate::render::{Sprite, SpriteImage};

/// SMM2 shows 24 × 13.5 tiles.
pub const VIEW_W: f32 = 24.0 * TILE;
pub const VIEW_H: f32 = 13.5 * TILE;

pub struct World {
    pub tick: u64,
    pub level: Level,
    pub player: Player,
    pub camera_x: f32,
    pub camera_y: f32,
    /// Leaked once per world (one string per course load) so sprites can borrow it.
    field_model: &'static str,
    player_model: &'static str,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// Field atlas model per theme for SMB1 (Model/M1_Field_<theme>).
fn field_model(style: &str, theme: u8) -> String {
    let name = [
        "plain",
        "underground",
        "castle",
        "airship",
        "water",
        "hauntedhouse",
        "snow",
        "desert",
        "athletic",
        "woods",
    ]
    .get(theme as usize)
    .copied()
    .unwrap_or("plain");
    format!("{style}_Field_{name}")
}

impl World {
    /// A flat test strip.
    pub fn new() -> Self {
        Self::with_level(Level::flat(200))
    }

    pub fn from_course(course: &Course) -> Self {
        Self::with_level(Level::from_course(course, 0))
    }

    fn with_level(level: Level) -> Self {
        let (sx, sy) = level.start;
        let mut w = Self {
            tick: 0,
            player: Player::new(sx, sy),
            field_model: Box::leak(field_model(&level.style, level.theme).into_boxed_str()),
            player_model: "M1_Player_MarioMdl",
            level,
            camera_x: 0.0,
            camera_y: 0.0,
        };
        w.update_camera();
        w
    }

    pub fn step(&mut self, input: Input) {
        self.player.step(input, &self.level);
        if self.player.dead {
            // provisional: restart at the start position (lives, death animation: step 6)
            let (sx, sy) = self.level.start;
            self.player = Player::new(sx, sy);
        }
        self.update_camera();
        self.tick += 1;
    }

    /// Provisional camera (exact follow rules are an open spec item): keep the player near the
    /// horizontal centre, scroll vertically when they leave a middle band; clamp to the area.
    fn update_camera(&mut self) {
        let max_x = (self.level.right - VIEW_W).max(self.level.left);
        self.camera_x = (self.player.x - VIEW_W / 2.0).clamp(self.level.left, max_x);
        let max_y = (self.level.top - VIEW_H).max(0.0);
        let (lo, hi) = (3.0 * TILE, 8.0 * TILE);
        if self.player.y - self.camera_y > hi {
            self.camera_y = self.player.y - hi;
        } else if self.player.y - self.camera_y < lo {
            self.camera_y = self.player.y - lo;
        }
        self.camera_y = self.camera_y.clamp(0.0, max_y);
    }

    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        let field = self.field_model;
        let (x0, x1) = (self.camera_x - TILE, self.camera_x + VIEW_W + TILE);
        let (y0, y1) = (self.camera_y - TILE, self.camera_y + VIEW_H + TILE);
        for t in &self.level.tiles {
            let (px, py) = (t.x as f32 * TILE, t.y as f32 * TILE);
            if px < x0 || px > x1 || py < y0 || py > y1 {
                continue;
            }
            out.push(Sprite {
                image: SpriteImage {
                    model: field,
                    texture: field,
                    rect: Some(t.atlas_rect()),
                },
                x: px,
                y: py,
                w: TILE,
                h: TILE,
                flip_x: false,
                layer: 10,
            });
        }
        for c in &self.level.coins {
            let (px, py) = (c.x as f32 * TILE, c.y as f32 * TILE);
            if px < x0 || px > x1 || py < y0 || py > y1 {
                continue;
            }
            out.push(Sprite {
                image: SpriteImage {
                    model: field,
                    texture: field,
                    rect: Some((7.0 * TILE, 0.0, TILE, TILE)),
                },
                x: px,
                y: py,
                w: TILE,
                h: TILE,
                flip_x: false,
                layer: 20,
            });
        }
        let p = &self.player;
        let frame = match p.pose {
            Pose::Wait => "wait.0",
            Pose::Skid => "turn.0",
            Pose::Jump => "jump.0",
            Pose::Walk => ["walk.0", "walk.1", "walk.2"][((p.walk_phase / 6.0) as usize) % 3],
        };
        out.push(Sprite {
            image: SpriteImage {
                model: self.player_model,
                texture: frame,
                rect: None,
            },
            x: p.x - 8.0,
            y: p.y,
            w: 16.0,
            h: 16.0,
            flip_x: p.facing < 0,
            layer: 50,
        });
        out
    }
}
