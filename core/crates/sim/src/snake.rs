//! Snake blocks (84-snake-block.md): a chain of solid blocks that follows a drawn path once a
//! player stands on it. The head walks the path at 0.5 px/frame (1.0 for the fast kind); every
//! segment trails the head by 16 px along the same path. At the end of the path the chain shakes
//! (frames 15–59) and the segments drop one by one from frame 70 (+10 per segment) at 3 px/frame.
//!
//! Path nodes [obs path builder 0x71005be760]: every node is one tile on from the one before, in
//! the direction the path *enters* it: 1, 5, 7, 14 left; 2, 9, 11, 13 right; 3, 6, 10, 16 down;
//! 4, 8, 12, 15 up. The corners 5–12 also name the way out (the next node's direction) and
//! 13–16 are the end nodes, which take their step too (0 stops the list). The first node starts
//! one tile beyond the row's end in its direction (left of the leftmost block, right of the
//! rightmost) or, going up / down, from the head block. The game moves the head to node 0, then
//! to each corner and the end in turn [obs analyzer 0x7100404f80 keeps node 0, 5–16].

use crate::level::TILE;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Wait,
    Move,
    Fall(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snake {
    /// Path points (tile centres): tail … head start, then one per path step.
    pts: Vec<(f32, f32)>,
    /// Number of blocks.
    pub n: usize,
    /// Distance the head has travelled along the path.
    d: f32,
    speed: f32,
    state: State,
    /// Per-segment drop while falling apart.
    drop: Vec<f32>,
    pub fast: bool,
    /// Segment centres last frame (for rider carry).
    prev: Vec<(f32, f32)>,
}

/// Unit step into a path node with this code (also the ! block and track block builders'
/// mapping, 0x71005b4aa0 / 0x71005b86a0, at 2 tiles); None for codes that make no node.
pub fn node_step(code: u16) -> Option<(f32, f32)> {
    Some(match code {
        1 | 5 | 7 | 14 => (-1.0, 0.0),
        2 | 9 | 11 | 13 => (1.0, 0.0),
        3 | 6 | 10 | 16 => (0.0, -1.0),
        4 | 8 | 12 | 15 => (0.0, 1.0),
        _ => return None,
    })
}

impl Snake {
    /// `left`, `y`: tile of the leftmost block; `reverse` = the head is the left end.
    pub fn new(left: i32, y: i32, n: usize, fast: bool, reverse: bool, codes: &[u16]) -> Self {
        let n = n.max(1);
        let cy = y as f32 * TILE + 8.0;
        let mut pts: Vec<(f32, f32)> = (0..n)
            .map(|i| ((left + i as i32) as f32 * TILE + 8.0, cy))
            .collect();
        let (lx, rx) = (pts[0].0, pts[n - 1].0);
        if reverse {
            pts.reverse();
        }
        for (i, &c) in codes.iter().enumerate() {
            if c == 0 {
                break;
            }
            let Some((dx, dy)) = node_step(c) else {
                continue;
            };
            let (x, y) = *pts.last().unwrap();
            let from = match i {
                // the first node: past the row's end that way, or above / below the head
                0 if dx < 0.0 => lx,
                0 if dx > 0.0 => rx,
                _ => x,
            };
            pts.push((from + dx * TILE, y + dy * TILE));
        }
        let mut s = Self {
            pts,
            n,
            d: 0.0,
            speed: if fast { 1.0 } else { 0.5 },
            state: State::Wait,
            drop: vec![0.0; n],
            fast,
            prev: Vec::new(),
        };
        s.prev = s.segments();
        s
    }

    fn at(&self, s: f32) -> (f32, f32) {
        let s = s.max(0.0);
        let i = ((s / TILE).floor() as usize).min(self.pts.len().saturating_sub(1));
        if i + 1 >= self.pts.len() {
            return self.pts[self.pts.len() - 1];
        }
        let t = (s - i as f32 * TILE) / TILE;
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }

    /// Segment centres, head first.
    pub fn segments(&self) -> Vec<(f32, f32)> {
        let shake = match self.state {
            State::Fall(t) if (15..60).contains(&t) => [1.0, 0.0, -1.0, 0.0][(t % 4) as usize],
            _ => 0.0,
        };
        (0..self.n)
            .map(|k| {
                let (x, y) = self.at((self.n - 1) as f32 * TILE + self.d - k as f32 * TILE);
                (x + shake, y - self.drop[k])
            })
            .collect()
    }

    /// Advance one frame. `ridden`: a player stands on any segment.
    pub fn step(&mut self, ridden: bool) {
        self.prev = self.segments();
        match self.state {
            State::Wait => {
                if ridden {
                    self.state = State::Move;
                }
            }
            State::Move => {
                let end = (self.pts.len() - self.n) as f32 * TILE;
                self.d = (self.d + self.speed).min(end);
                if self.d >= end {
                    self.state = State::Fall(0);
                }
            }
            State::Fall(t) => {
                for k in 0..self.n {
                    if t >= 70 + 10 * k as i32 {
                        self.drop[k] += 3.0;
                    }
                }
                self.state = State::Fall(t + 1);
            }
        }
    }

    /// Movement of segment k this frame.
    pub fn moved(&self, k: usize) -> (f32, f32) {
        let now = self.segments();
        match (now.get(k), self.prev.get(k)) {
            (Some(a), Some(b)) => (a.0 - b.0, a.1 - b.1),
            _ => (0.0, 0.0),
        }
    }

    /// Fallen entirely out of the area.
    pub fn gone(&self) -> bool {
        matches!(self.state, State::Fall(_)) && self.segments().iter().all(|s| s.1 < -64.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_then_follows_the_path() {
        // 3 blocks at tiles 10..12, row 5, path: right, right into a turn up, up, end
        let mut s = Snake::new(10, 5, 3, false, false, &[2, 11, 4, 15]);
        assert_eq!(s.pts.len(), 3 + 4, "a node per code, the end included");
        assert_eq!(s.pts[4], (14.0 * 16.0 + 8.0, 88.0), "the corner tile");
        let start = s.segments();
        assert_eq!(start[0], (12.0 * 16.0 + 8.0, 88.0));
        s.step(false);
        assert_eq!(s.segments(), start, "waits for a rider");
        s.step(true);
        for _ in 0..66 {
            s.step(false);
        }
        // head travelled 33 px at 0.5: 2 tiles right, then 1 px up
        let head = s.segments()[0];
        assert!(head.1 > 88.0 && head.0 == 14.0 * 16.0 + 8.0, "{head:?}");
        // the tail follows the same path 32 px behind
        let tail = s.segments()[2];
        assert!(
            (tail.0 - (12.0 * 16.0 + 8.0 + 1.0)).abs() < 0.01,
            "{tail:?}"
        );
    }

    #[test]
    fn falls_apart_at_the_end() {
        let mut s = Snake::new(0, 5, 2, true, false, &[2, 13]);
        s.step(true);
        for _ in 0..200 {
            s.step(false);
        }
        assert!(s.segments().iter().all(|p| p.1 < 88.0));
    }
}
