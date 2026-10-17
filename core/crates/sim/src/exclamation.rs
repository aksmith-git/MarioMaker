//! ! Blocks (119-exclamation-block.md, 3D World): a 2×2 block with a drawn path. Hitting it from
//! below bounces it (a 7-frame ramp up, then down, 16 px reference height) and starts a
//! 600-frame timer; when the timer runs out the chain is restored to its starting layout.
//!
//! What the chain does between the hit and the restore was not decoded [?]. The editor files put
//! the block with the snake blocks and the model has a "!" and an empty variant, so here the hit
//! block empties and the path fills with 2×2 blocks, one every `GROW_FRAMES` frames, one path
//! step (2 tiles) apart [inf]. Path node codes are the snake-block codes (`snake::node_step`).

use crate::level::TILE;
use crate::snake::node_step;

/// Frames until the chain is restored after a hit [obs].
pub const RESTORE_FRAMES: i32 = 600;
/// Bounce ramp length (frames up, then the same down) [obs].
pub const BOUNCE_FRAMES: i32 = 7;
/// Frames between new chain blocks [inf].
pub const GROW_FRAMES: i32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct ExclamationBlock {
    /// Bottom-left tile of the 2×2 block.
    pub tile: (i32, i32),
    /// Bottom-left tiles of the chain blocks along the path.
    path: Vec<(i32, i32)>,
    /// Chain blocks out so far.
    pub shown: usize,
    grow: i32,
    pub timer: i32,
    /// Bounce frame (0 = at rest, 1…2·BOUNCE_FRAMES).
    bounce: i32,
}

impl ExclamationBlock {
    pub fn new(tile: (i32, i32), codes: &[u16]) -> Self {
        let mut path = Vec::new();
        let (mut x, mut y) = tile;
        for &c in codes {
            let Some((dx, dy)) = node_step(c) else { break };
            x += 2 * dx as i32;
            y += 2 * dy as i32;
            path.push((x, y));
        }
        Self {
            tile,
            path,
            shown: 0,
            grow: 0,
            timer: 0,
            bounce: 0,
        }
    }

    /// Hit from below: bounce, empty the block, (re)start the restore timer.
    pub fn hit(&mut self) {
        if self.bounce == 0 {
            self.bounce = 1;
        }
        self.timer = RESTORE_FRAMES;
    }

    pub fn emptied(&self) -> bool {
        self.timer > 0
    }

    /// One frame. `free(tile)` says whether a 2×2 block may appear with that bottom-left tile.
    pub fn step(&mut self, free: impl Fn((i32, i32)) -> bool) {
        if self.bounce > 0 {
            self.bounce += 1;
            if self.bounce > 2 * BOUNCE_FRAMES {
                self.bounce = 0;
            }
        }
        if self.timer > 0 {
            self.timer -= 1;
            if self.timer == 0 {
                // restore
                self.shown = 0;
                self.grow = 0;
                return;
            }
            if self.shown < self.path.len() {
                self.grow += 1;
                if self.grow >= GROW_FRAMES {
                    self.grow = 0;
                    if free(self.path[self.shown]) {
                        self.shown += 1;
                    } else {
                        // blocked: the chain stops here
                        self.path.truncate(self.shown);
                    }
                }
            }
        }
    }

    /// Vertical bounce offset in px.
    pub fn lift(&self) -> f32 {
        let f = if self.bounce <= BOUNCE_FRAMES {
            self.bounce
        } else {
            2 * BOUNCE_FRAMES - self.bounce
        };
        16.0 * f.max(0) as f32 / BOUNCE_FRAMES as f32 * 0.5
    }

    /// The blocks' boxes (x0, y0, x1, y1), the ! block first, then the chain.
    pub fn boxes(&self) -> Vec<(f32, f32, f32, f32)> {
        let b = |(tx, ty): (i32, i32)| {
            let (x0, y0) = (tx as f32 * TILE, ty as f32 * TILE);
            (x0, y0, x0 + 2.0 * TILE, y0 + 2.0 * TILE)
        };
        std::iter::once(self.tile)
            .chain(self.path[..self.shown].iter().copied())
            .map(b)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_grows_the_chain_along_the_path_then_restores() {
        // up, up, then right, end
        let mut b = ExclamationBlock::new((10, 2), &[4, 4, 2, 15]);
        assert_eq!(b.boxes().len(), 1);
        b.hit();
        assert!(b.emptied());
        for _ in 0..3 * GROW_FRAMES {
            b.step(|_| true);
        }
        let boxes = b.boxes();
        assert_eq!(boxes.len(), 4);
        assert_eq!(boxes[1].1, 4.0 * TILE, "first step up 2 tiles");
        assert_eq!(boxes[3].0, 12.0 * TILE, "then right 2 tiles");
        for _ in 0..RESTORE_FRAMES {
            b.step(|_| true);
        }
        assert_eq!(b.boxes().len(), 1, "restored");
        assert!(!b.emptied());
    }

    #[test]
    fn blocked_path_stops_and_bounce_ramps() {
        let mut b = ExclamationBlock::new((0, 0), &[2, 2, 2, 15]);
        b.hit();
        let mut top: f32 = 0.0;
        for _ in 0..40 {
            b.step(|t| t.0 < 4);
            top = top.max(b.lift());
        }
        assert_eq!(b.shown, 1, "stops at the blocked cell");
        assert!((top - 8.0).abs() < 1e-4);
        assert_eq!(b.lift(), 0.0);
    }
}
