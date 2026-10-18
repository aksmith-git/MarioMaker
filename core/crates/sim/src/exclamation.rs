//! ! Blocks (119-exclamation-block.md, 3D World): a 2×2 block with a drawn path. Hitting it
//! bounces it (a 7-frame ramp up, then down, 16 px reference height) and sends chain blocks out
//! along the path; 600 frames after the first hit the chain is pulled back in.
//!
//! The chain [obs BlockBikkuri, init 0x710086de50, update 0x710086ee80]: one 2×2 segment per path
//! point, all starting hidden inside the ! block. The path points are the block's position plus
//! one 32-px step per node code, every code counted, the end codes included (1, 5, 7, 14 left;
//! 2, 9, 11, 13 right; 3, 6, 10, 16 down; 4, 8, 12, 15 up — the step *into* each node)
//! [obs 0x71005b4aa0, consts ±32 at 0x7101c8a748 / 0x7101c8a624]. A hidden segment sits on the
//! one before it. Points where the path turns are marked; so is point 0 when the first two steps
//! differ [obs 0x710086de50].
//! - A head hit (the player's head carries hit bit 18) extends one segment: the first hidden one
//!   takes the point after the previous segment's and slides there at 3 px a frame, easing in
//!   over the last 16 px (dist·3/16, at least 0.1) [obs 0x71008714e0, 0x7100871310].
//! - A hit with bit 19 or word-2 bit 26 extends them all (the player's feet carry bit 19 in one
//!   state, +0x1060 bit 23 — taken here for the ground pound [inf: state not named]):
//!   each run of hidden segments up to the next turn sets off together, each at its distance /
//!   max(4, run + 2) a frame (at most 8), and when the run's last segment arrives the next run
//!   starts; within 96 px of the path's end the speed scales down to remaining / 96 (at least
//!   0.1) [obs 0x710086f7a0, 0x710086ee80].
//! - Hits are ignored while bouncing, while extending them all, and once the last point is
//!   taken; after the bounce that took the last point the ! block shows empty. The 600-frame
//!   timer is armed by the first hit only; at 0 every segment returns inside at once
//!   [obs 0x710086fe50, 0x710086fe20, Restore 0x7100870df0]. The path is not tested for room:
//!   segments pass through terrain.

use crate::level::TILE;
use crate::snake::node_step;

/// Frames from the first hit until the chain is restored [obs].
pub const RESTORE_FRAMES: i32 = 600;
/// Bounce ramp length (frames up, then the same down) [obs].
pub const BOUNCE_FRAMES: i32 = 7;
/// Frames a bounce state lasts before hits count again [obs 0x710086ffb0: 7 calls].
const BOUND_FRAMES: i32 = 7;
/// Path step (px) and the cap on a segment's speed (px/frame).
const STEP: f32 = 32.0;
const MAX_SPEED: f32 = 8.0;
/// Remaining path length below which an all-out run slows down [obs const 0x7101c8a9e4].
const SLOW_LEN: f32 = 96.0;

/// Length of (dx, dy), as the game takes it (a square root).
fn len(dx: f32, dy: f32) -> f32 {
    (dx * dx + dy * dy).sqrt()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Out {
    /// Inside the ! block (or the previous segment).
    Hidden,
    /// Sent out by a head hit.
    One,
    /// Sent out by an all-out run.
    All,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    /// Bottom-left corner of its 2×2 box, px.
    pos: (f32, f32),
    point: usize,
    out: Out,
    speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bound {
    Wait,
    /// A hit is being taken: extend one segment / all of them; frames so far.
    One(i32),
    All(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExclamationBlock {
    /// Bottom-left tile of the 2×2 block.
    pub tile: (i32, i32),
    /// Path points (bottom-left px of a segment there) and whether the path turns there.
    points: Vec<(f32, f32)>,
    turns: Vec<bool>,
    segs: Vec<Segment>,
    /// Restore countdown; −1 = not armed.
    pub timer: i32,
    bound: Bound,
    /// Bounce frame (0 = at rest, 1…2·BOUNCE_FRAMES), drawn only.
    bounce: i32,
    /// Extending them all.
    all: bool,
    /// The last point is taken; the ! block shows empty after that bounce.
    done: bool,
    empty: bool,
    /// Path length, and what the last moving segment still has to go (all-out slow-down).
    length: f32,
    left: f32,
    pace: f32,
}

impl ExclamationBlock {
    pub fn new(tile: (i32, i32), codes: &[u16]) -> Self {
        let home = (tile.0 as f32 * TILE, tile.1 as f32 * TILE);
        let mut points = Vec::new();
        let mut at = home;
        for &c in codes {
            if c == 0 {
                break;
            }
            if let Some((dx, dy)) = node_step(c) {
                at = (at.0 + dx * STEP, at.1 + dy * STEP);
                points.push(at);
            }
        }
        // turns: the step into point i differs from the step out of it
        let mut turns = vec![false; points.len()];
        let mut length = 0.0;
        let mut prev = home;
        let mut heading = None;
        for (i, &p) in points.iter().enumerate() {
            let d = (p.0 - prev.0, p.1 - prev.1);
            length += len(d.0, d.1);
            if i > 0 && heading != Some(d) {
                turns[i - 1] = true;
            }
            heading = Some(d);
            prev = p;
        }
        let segs = vec![
            Segment {
                pos: home,
                point: 0,
                out: Out::Hidden,
                speed: 0.0,
            };
            points.len()
        ];
        Self {
            tile,
            points,
            turns,
            segs,
            timer: -1,
            bound: Bound::Wait,
            bounce: 0,
            all: false,
            done: false,
            empty: false,
            length,
            left: length,
            pace: 0.0,
        }
    }

    fn home(&self) -> (f32, f32) {
        (self.tile.0 as f32 * TILE, self.tile.1 as f32 * TILE)
    }

    /// Whether a hit counts now.
    fn takes_hits(&self) -> bool {
        self.bound == Bound::Wait && !self.all && !self.done
    }

    /// Hit from below by the player's head: bounce and send one more segment out.
    pub fn hit(&mut self) {
        self.strike(false);
    }

    /// A hit that sends the whole chain out (hit bit 19, see the module notes).
    pub fn hit_all(&mut self) {
        self.strike(true);
    }

    fn strike(&mut self, all: bool) {
        if !self.takes_hits() {
            return;
        }
        if self.timer == -1 {
            self.timer = RESTORE_FRAMES;
        }
        self.bound = if all { Bound::All(0) } else { Bound::One(0) };
        if self.bounce == 0 {
            self.bounce = 1;
        }
    }

    /// The ! block shows empty (its chain is all out).
    pub fn emptied(&self) -> bool {
        self.empty
    }

    /// Segments out along the path.
    pub fn shown(&self) -> usize {
        self.segs.iter().filter(|s| s.out != Out::Hidden).count()
    }

    /// One frame. (The game does not test the path for room, so `_free` is not consulted.)
    pub fn step(&mut self, _free: impl Fn((i32, i32)) -> bool) {
        if self.bounce > 0 {
            self.bounce += 1;
            if self.bounce > 2 * BOUNCE_FRAMES {
                self.bounce = 0;
            }
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.restore();
            return;
        }
        match self.bound {
            Bound::Wait => {}
            Bound::One(n) | Bound::All(n) => {
                if n == 0 {
                    if matches!(self.bound, Bound::One(_)) {
                        self.send_one();
                    } else {
                        self.all = true;
                        self.send_run();
                    }
                }
                self.bound = if n + 1 >= BOUND_FRAMES {
                    // the bounce that took the last point leaves the block empty
                    self.empty |= self.done;
                    Bound::Wait
                } else if matches!(self.bound, Bound::One(_)) {
                    Bound::One(n + 1)
                } else {
                    Bound::All(n + 1)
                };
            }
        }
        self.slide();
    }

    /// The first hidden segment takes the point after the previous segment's [obs 0x7100871310].
    fn send_one(&mut self) {
        let Some(j) = self.segs.iter().position(|s| s.out == Out::Hidden) else {
            return;
        };
        if j > 0 {
            self.segs[j].point = self.segs[j - 1].point + 1;
        }
        self.segs[j].out = Out::One;
        if self.segs[j].point + 1 >= self.points.len() {
            self.done = true;
        }
    }

    /// Send the next run of hidden segments, up to and including the next turn
    /// [obs 0x710086f7a0].
    fn send_run(&mut self) {
        let mut run = Vec::new();
        for j in 0..self.segs.len() {
            if self.segs[j].out != Out::Hidden {
                continue;
            }
            if j > 0 {
                self.segs[j].point = self.segs[j - 1].point + 1;
            }
            self.segs[j].out = Out::All;
            run.push(j);
            let p = self.segs[j].point;
            if p + 1 >= self.points.len() {
                self.done = true;
            }
            if self.turns.get(p).copied().unwrap_or(false) {
                break;
            }
        }
        if !run.is_empty() {
            let frames = if run.len() < 3 {
                4.0
            } else {
                run.len() as f32 + 2.0
            };
            for j in run {
                let s = &mut self.segs[j];
                let t = self.points.get(s.point).copied().unwrap_or(s.pos);
                let d = len(t.0 - s.pos.0, t.1 - s.pos.1);
                if d > 0.0 {
                    s.speed = (d / frames).min(MAX_SPEED);
                }
            }
        }
        if self.pace <= 0.0 {
            self.pace = 1.0;
        }
    }

    /// Move the segments that are out toward their points; hidden ones ride on the one before.
    fn slide(&mut self) {
        if self.all {
            self.pace = if self.left <= SLOW_LEN {
                (self.left / SLOW_LEN).max(0.1)
            } else {
                1.0
            };
        }
        let last = self.segs.iter().rposition(|s| s.out != Out::Hidden);
        let mut arrived = false;
        for j in 0..self.segs.len() {
            if self.segs[j].out == Out::Hidden {
                if j > 0 {
                    self.segs[j].pos = self.segs[j - 1].pos;
                }
                continue;
            }
            let s = self.segs[j];
            let t = self.points.get(s.point).copied().unwrap_or(s.pos);
            let (dx, dy) = (t.0 - s.pos.0, t.1 - s.pos.1);
            let d = len(dx, dy);
            let step = match s.out {
                Out::All => (s.speed * self.pace).min(MAX_SPEED),
                _ if d < 16.0 => (d * 0.0625 * 3.0).max(0.1),
                _ => 3.0,
            };
            let lead = last == Some(j);
            if d <= step {
                self.segs[j].pos = t;
                if lead {
                    self.left -= d;
                    arrived = true;
                }
            } else {
                let inv = 1.0 / d;
                self.segs[j].pos = (s.pos.0 + dx * inv * step, s.pos.1 + dy * inv * step);
                if lead {
                    self.left -= step;
                }
            }
        }
        if arrived && self.all && !self.done {
            self.send_run();
        }
    }

    /// Every segment back inside, timer disarmed [obs 0x7100870df0].
    fn restore(&mut self) {
        let home = self.home();
        self.segs.fill(Segment {
            pos: home,
            point: 0,
            out: Out::Hidden,
            speed: 0.0,
        });
        self.timer = -1;
        self.bound = Bound::Wait;
        self.all = false;
        self.done = false;
        self.empty = false;
        self.pace = 0.0;
        self.left = self.length;
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

    /// The blocks' boxes (x0, y0, x1, y1), the ! block first, then the segments that are out.
    pub fn boxes(&self) -> Vec<(f32, f32, f32, f32)> {
        let b = |(x0, y0): (f32, f32)| (x0, y0, x0 + 2.0 * TILE, y0 + 2.0 * TILE);
        std::iter::once(self.home())
            .chain(
                self.segs
                    .iter()
                    .filter(|s| s.out != Out::Hidden)
                    .map(|s| s.pos),
            )
            .map(b)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(b: &mut ExclamationBlock, frames: usize) {
        for _ in 0..frames {
            b.step(|_| true);
        }
    }

    #[test]
    fn path_points_take_a_step_per_code_end_codes_included() {
        // right, right, end coming right: three points, 2, 4 and 6 tiles out
        let b = ExclamationBlock::new((10, 2), &[2, 2, 13]);
        assert_eq!(b.points, vec![(192.0, 32.0), (224.0, 32.0), (256.0, 32.0)]);
        // a corner: up (11 = coming right, turning up), up, end coming up
        let c = ExclamationBlock::new((0, 0), &[11, 4, 15]);
        assert_eq!(c.points, vec![(32.0, 0.0), (32.0, 32.0), (32.0, 64.0)]);
        assert_eq!(c.turns, vec![true, false, false], "turns at the corner");
        assert_eq!(c.boxes().len(), 1, "the chain starts inside the block");
    }

    #[test]
    fn each_head_hit_sends_one_segment_at_3_px_a_frame() {
        let mut b = ExclamationBlock::new((10, 2), &[2, 2, 13]);
        b.hit();
        assert_eq!(b.timer, RESTORE_FRAMES);
        run(&mut b, 1);
        assert_eq!(b.shown(), 1);
        assert_eq!(b.boxes()[1].0, 160.0 + 3.0, "3 px out after a frame");
        // a hit during the 7-frame bounce does not count
        b.hit();
        run(&mut b, 40);
        assert_eq!(
            b.boxes()[1].0,
            192.0,
            "at its point (32 px, easing in at the end)"
        );
        assert_eq!(b.shown(), 1);
        // the next hit sends the next one; the timer keeps counting from the first hit
        b.hit();
        run(&mut b, 1);
        assert_eq!(b.shown(), 2);
        assert_eq!(b.timer, RESTORE_FRAMES - 42);
        assert!(!b.emptied());
        run(&mut b, 20);
        b.hit();
        run(&mut b, 7);
        assert!(b.emptied(), "the last point taken: the block shows empty");
        b.hit();
        run(&mut b, 1);
        assert_eq!(b.shown(), 3, "no more hits once the chain is out");
        run(&mut b, RESTORE_FRAMES as usize);
        assert_eq!(b.boxes().len(), 1, "restored");
        assert_eq!(b.timer, -1);
        assert!(!b.emptied());
    }

    #[test]
    fn an_all_out_hit_sends_runs_up_to_each_turn() {
        // right ×3, then a turn up and up ×2: runs (point 0..2, turning at 2) and (3..4)
        let mut b = ExclamationBlock::new((0, 0), &[2, 2, 11, 4, 15]);
        assert_eq!(b.turns, vec![false, false, true, false, false]);
        b.hit_all();
        run(&mut b, 1);
        assert_eq!(b.shown(), 3, "the first run, up to the turn");
        // the run of 3 takes max(4, 3 + 2) = 5 frames: 96 px at 19.2 is capped at 8 px/f
        let far = b.boxes()[3];
        assert_eq!(far.0, 8.0);
        run(&mut b, 12);
        assert_eq!(b.shown(), 5, "the last arrival starts the next run");
        run(&mut b, 60);
        let boxes = b.boxes();
        assert_eq!(boxes[5].0, 96.0);
        assert_eq!(boxes[5].1, 64.0, "out to the end");
        assert!(b.done);
    }
}
