//! The world: for step 4, a flat strip of ground and one player.

use crate::input::Input;
use crate::player::{Player, Pose};
use crate::render::{Sprite, SpriteImage};

pub const TILE: f32 = 16.0;
/// SMM2 shows 24 × 13.5 tiles.
pub const VIEW_W: f32 = 24.0 * TILE;
pub const VIEW_H: f32 = 13.5 * TILE;

const FIELD_MODEL: &str = "M1_Field_plain";
const MARIO_MODEL: &str = "M1_Player_MarioMdl";

pub struct World {
    pub tick: u64,
    pub player: Player,
    pub floor_y: f32,
    pub camera_x: f32,
    pub camera_y: f32,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let floor_y = 2.0 * TILE;
        Self {
            tick: 0,
            player: Player::new(3.0 * TILE, floor_y),
            floor_y,
            camera_x: 0.0,
            camera_y: 0.0,
        }
    }

    pub fn step(&mut self, input: Input) {
        self.player.step(input, self.floor_y);
        // provisional camera: keep the player a little left of centre (real rules in step 5)
        self.camera_x = (self.player.x - VIEW_W * 0.4).max(0.0);
        self.tick += 1;
    }

    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        // ground: two rows of the SMB1 ground tile from the field atlas
        let first = (self.camera_x / TILE).floor() as i32 - 1;
        for col in first..first + (VIEW_W / TILE) as i32 + 3 {
            for row in 0..2 {
                out.push(Sprite {
                    image: SpriteImage {
                        model: FIELD_MODEL,
                        texture: FIELD_MODEL,
                        rect: Some((0.0, 12.0 * TILE, TILE, TILE)),
                    },
                    x: col as f32 * TILE,
                    y: row as f32 * TILE,
                    w: TILE,
                    h: TILE,
                    flip_x: false,
                    layer: 10,
                });
            }
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
                model: MARIO_MODEL,
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
