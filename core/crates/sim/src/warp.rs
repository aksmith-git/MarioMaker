//! Pipe and door travel (9-pipe.md, 55-door.md): entry tests and the transit timeline.
//!
//! The World owns a `Transit` while the player is moving through a pipe or door. Entry tests
//! use the spec's windows: ±7.85 px from a vertical mouth's centre line, ±8 px around a side
//! mouth at probe height feet + 11.9, side entry after the direction is held 10 frames, doors
//! within |dx| ≤ 7, |dy| < 2 with Up and no Left/Right.

use crate::fmath::DetMath;
use crate::input::{self, Input};
use crate::level::{PipeDir, Warp, WarpKind};
use crate::player::Player;

pub const ALIGN_SPEED: f32 = 1.0;
pub const PIPE_SPEED: f32 = 0.75;
/// Hold after fully inside before the area change, and before the exit starts.
pub const HOLD_FRAMES: i32 = 30;
pub const DOOR_IN_FRAMES: i32 = 50;
pub const DOOR_OUT_FRAMES: i32 = 20;
/// Warp box exit: frames hidden in the exit box before popping out — its PrePlayerOutDemo
/// plays `OutSign` (28 frames, 3W_Object_warpbox) and the player is released as `Out` starts
/// the frame after (box states 0x71008c5800…; the online timeline puts OutSign at 68 and Out
/// at 97) [obs].
pub const BOX_OUT_FRAMES: i32 = 29;
const MOUTH_HALF_W: f32 = 7.85;
const SIDE_PROBE_Y: f32 = 11.9;
const SIDE_HOLD: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Slide along the cross axis to the pipe axis at 1 px/frame.
    Align,
    /// Move into the pipe at 0.75 px/frame.
    In,
    /// Fully inside (or behind the door): hold, then switch area.
    HoldOut(i32),
    /// At the destination, hidden: hold, then come out.
    HoldIn(i32),
    /// Move out of the destination pipe.
    Out,
    DoorIn(i32),
    DoorOut(i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transit {
    pub phase: Phase,
    pub from: Warp,
    pub to: Warp,
    /// Destination area index.
    pub to_area: usize,
}

/// Player height used for the in/out targets (min(height, 31) in the spec).
fn body_h(p: &Player) -> f32 {
    if p.big() { 31.0 } else { 16.0 }
}

/// Side-entry hold counters (frames the direction has been held, saturating).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideHold {
    pub left: u32,
    pub right: u32,
}

impl SideHold {
    pub fn update(&mut self, inp: Input, grounded: bool) {
        if !grounded {
            *self = Self::default();
            return;
        }
        self.left = if inp.held(input::LEFT) {
            (self.left + 1).min(SIDE_HOLD)
        } else {
            0
        };
        self.right = if inp.held(input::RIGHT) {
            (self.right + 1).min(SIDE_HOLD)
        } else {
            0
        };
    }
}

/// Which pipe (if any) the player is entering this frame. `pushing` = the player was stopped by
/// a wall this frame while facing it (immediate side entry).
pub fn pipe_entry(
    p: &Player,
    inp: Input,
    hold: SideHold,
    pushing: bool,
    warps: &[Warp],
) -> Option<Warp> {
    let mut best: Option<(f32, Warp)> = None;
    let head = p.y + p.sensors().head_y;
    for w in warps {
        let WarpKind::Pipe(dir) = w.kind else {
            continue;
        };
        let (probe, ok) = match dir {
            // up-facing pipe: stand on top and press Down
            PipeDir::Up => (
                (p.x, p.y - 1.0),
                p.on_ground && inp.held(input::DOWN) && (p.y - (w.y + 8.0)).abs() < 1.0,
            ),
            // down-facing pipe: head at the mouth, Up held (also while jumping)
            PipeDir::Down => ((p.x, head), inp.held(input::UP)),
            // left-facing pipe, entered moving right
            PipeDir::Left => (
                (p.x + 8.9, p.y + SIDE_PROBE_Y),
                p.on_ground
                    && inp.held(input::RIGHT)
                    && (hold.right >= SIDE_HOLD || (pushing && p.facing > 0)),
            ),
            PipeDir::Right => (
                (p.x - 8.9, p.y + SIDE_PROBE_Y),
                p.on_ground
                    && inp.held(input::LEFT)
                    && (hold.left >= SIDE_HOLD || (pushing && p.facing < 0)),
            ),
        };
        if !ok {
            continue;
        }
        let (bx, by) = match dir {
            PipeDir::Down => (w.x, w.y - 8.0),
            _ => (w.x, w.y),
        };
        let inside = match dir {
            PipeDir::Up | PipeDir::Down => {
                (probe.0 - bx).abs() <= MOUTH_HALF_W && (probe.1 - by).abs() <= 8.0
            }
            _ => (probe.0 - bx).abs() <= 8.0 && (probe.1 - by).abs() <= 8.0,
        };
        if inside {
            let d = (probe.0 - bx).dhypot(probe.1 - by);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, *w));
            }
        }
    }
    best.map(|(_, w)| w)
}

/// A door the player can enter this frame (Up pressed, no Left/Right, on the ground). P doors
/// work only while a P switch is active; key doors when `key_ok(id)` (unlocked, or the player
/// holds a key — the caller consumes it).
pub fn door_entry(
    p: &Player,
    inp: Input,
    up_pressed: bool,
    p_active: bool,
    key_ok: impl Fn(u8) -> bool,
    warps: &[Warp],
) -> Option<Warp> {
    if !p.on_ground || !up_pressed || inp.held(input::LEFT) || inp.held(input::RIGHT) {
        return None;
    }
    warps
        .iter()
        .filter(|w| {
            matches!(w.kind, WarpKind::Door(0))
                || (p_active && w.kind == WarpKind::Door(1))
                || (w.kind == WarpKind::Door(2) && key_ok(w.id))
        })
        .filter(|w| (p.x - w.x).abs() <= 7.0 && (p.y - w.y).abs() < 2.0)
        .min_by(|a, b| (p.x - a.x).abs().total_cmp(&(p.x - b.x).abs()))
        .copied()
}

/// The partner of `from`: pipes → same id in the other area; doors → id ^ 1 in the same area.
/// Warp Box entry (97-warp-box.md): touching the trigger box (12×12 centred 8 px above the
/// origin) is enough — no button, no ground check. The nearest box by |dx| wins. `usable` says
/// whether a box can be used now (not gone; a locked key box needs a key).
pub fn box_entry(p: &Player, usable: impl Fn(&Warp) -> bool, warps: &[Warp]) -> Option<Warp> {
    let (x0, y0, x1, y1) = p.hurt_box();
    warps
        .iter()
        .filter(|w| matches!(w.kind, WarpKind::Box { .. }) && usable(w))
        .filter(|w| x1 > w.x - 6.0 && x0 < w.x + 6.0 && y1 > w.y + 2.0 && y0 < w.y + 14.0)
        .min_by(|a, b| (p.x - a.x).abs().total_cmp(&(p.x - b.x).abs()))
        .copied()
}

pub fn destination(from: &Warp, area: usize, areas: &[Vec<Warp>]) -> Option<(usize, Warp)> {
    match from.kind {
        WarpKind::Pipe(_) => {
            let other = area ^ 1;
            areas.get(other)?.iter().find_map(|w| {
                (matches!(w.kind, WarpKind::Pipe(_)) && w.id == from.id).then_some((other, *w))
            })
        }
        WarpKind::Door(_) => areas.get(area)?.iter().find_map(|w| {
            (matches!(w.kind, WarpKind::Door(_)) && w.id == from.id ^ 1).then_some((area, *w))
        }),
        // warp boxes never link across areas
        WarpKind::Box { .. } => areas.get(area)?.iter().find_map(|w| {
            (matches!(w.kind, WarpKind::Box { .. }) && w.id == from.id ^ 1).then_some((area, *w))
        }),
    }
}

fn approach(v: f32, target: f32, step: f32) -> (f32, bool) {
    if (v - target).abs() <= step {
        (target, true)
    } else if v < target {
        (v + step, false)
    } else {
        (v - step, false)
    }
}

/// Where the player sits fully inside a pipe (feet position) for entering/exiting.
pub fn inside_pos(w: &Warp, p: &Player) -> (f32, f32) {
    let (ex, ey) = w.entry();
    let h = body_h(p);
    match w.kind {
        WarpKind::Pipe(PipeDir::Up) => (ex, ey - h),
        WarpKind::Pipe(PipeDir::Down) => (ex, ey + 1.0),
        WarpKind::Pipe(PipeDir::Left) => (ex + 8.0, ey - 15.0),
        WarpKind::Pipe(PipeDir::Right) => (ex - 8.0, ey - 15.0),
        WarpKind::Door(_) | WarpKind::Box { .. } => (w.x, w.y),
    }
}

/// Where a pipe exit ends (feet).
pub fn outside_pos(w: &Warp, p: &Player) -> (f32, f32) {
    let (ex, ey) = w.entry();
    let h = body_h(p);
    match w.kind {
        WarpKind::Pipe(PipeDir::Up) => (ex, ey),
        WarpKind::Pipe(PipeDir::Down) => (ex, ey - h),
        WarpKind::Pipe(PipeDir::Left) => (ex - 8.0, ey - 15.0),
        WarpKind::Pipe(PipeDir::Right) => (ex + 8.0, ey - 15.0),
        WarpKind::Door(_) | WarpKind::Box { .. } => (w.x, w.y),
    }
}

/// Advance the transit by one frame, moving the player. Returns true on the frame the area
/// should switch (end of HoldOut).
pub fn step(t: &mut Transit, p: &mut Player) -> bool {
    match t.phase {
        Phase::Align => {
            let (ex, ey) = t.from.entry();
            let done = match t.from.kind {
                WarpKind::Pipe(PipeDir::Up | PipeDir::Down) => {
                    let (x, d) = approach(p.x, ex, ALIGN_SPEED);
                    p.x = x;
                    d
                }
                _ => {
                    let (y, d) = approach(p.y, ey - 15.0, ALIGN_SPEED);
                    p.y = y;
                    d
                }
            };
            if done {
                t.phase = Phase::In;
            }
            false
        }
        Phase::In => {
            let (tx, ty) = inside_pos(&t.from, p);
            let (x, dx) = approach(p.x, tx, PIPE_SPEED);
            let (y, dy) = approach(p.y, ty, PIPE_SPEED);
            p.x = x;
            p.y = y;
            if dx && dy {
                t.phase = Phase::HoldOut(HOLD_FRAMES);
            }
            false
        }
        Phase::DoorIn(n) => {
            t.phase = if n <= 1 {
                Phase::HoldOut(HOLD_FRAMES)
            } else {
                Phase::DoorIn(n - 1)
            };
            false
        }
        Phase::HoldOut(n) => {
            if n <= 1 {
                // switch area: the player is placed inside the destination
                let (x, y) = inside_pos(&t.to, p);
                p.x = x;
                p.y = y;
                t.phase = Phase::HoldIn(HOLD_FRAMES);
                return true;
            }
            t.phase = Phase::HoldOut(n - 1);
            false
        }
        Phase::HoldIn(n) => {
            if n <= 1 {
                t.phase = match t.to.kind {
                    WarpKind::Door(_) => Phase::DoorOut(DOOR_OUT_FRAMES),
                    // the exit box's OutSign before the player pops out [obs]
                    WarpKind::Box { .. } => Phase::DoorOut(BOX_OUT_FRAMES),
                    _ => Phase::Out,
                };
            } else {
                t.phase = Phase::HoldIn(n - 1);
            }
            false
        }
        Phase::Out => {
            let (tx, ty) = outside_pos(&t.to, p);
            let (x, dx) = approach(p.x, tx, PIPE_SPEED);
            let (y, dy) = approach(p.y, ty, PIPE_SPEED);
            p.x = x;
            p.y = y;
            if dx && dy {
                t.phase = Phase::DoorOut(0);
            }
            false
        }
        Phase::DoorOut(n) => {
            t.phase = Phase::DoorOut(n - 1);
            false
        }
    }
}

/// Transit finished (control returns to the player).
pub fn finished(t: &Transit) -> bool {
    matches!(t.phase, Phase::DoorOut(n) if n <= 0)
}

/// The player is hidden (inside a pipe/behind a door) in this phase.
pub fn hidden(t: &Transit) -> bool {
    match t.phase {
        Phase::HoldOut(_) | Phase::HoldIn(_) => true,
        // a warp box hides the player at once (doors after 20 frames)
        Phase::DoorIn(n) => n < DOOR_IN_FRAMES - 20 || matches!(t.from.kind, WarpKind::Box { .. }),
        Phase::DoorOut(n) => match t.to.kind {
            WarpKind::Door(_) => n > DOOR_OUT_FRAMES - 6,
            WarpKind::Box { .. } => n > 0,
            _ => false,
        },
        _ => false,
    }
}

/// Screen fade (0 clear … 1 black) during the area change.
pub fn fade(t: &Transit) -> f32 {
    match t.phase {
        Phase::HoldOut(n) => 1.0 - n as f32 / HOLD_FRAMES as f32,
        Phase::HoldIn(n) => n as f32 / HOLD_FRAMES as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipe(dir: PipeDir, id: u8, x: f32, y: f32) -> Warp {
        Warp {
            kind: WarpKind::Pipe(dir),
            id,
            x,
            y,
            len: 2,
        }
    }

    #[test]
    fn down_into_up_pipe_needs_alignment_window() {
        // up pipe mouth centre at (88, 40): top at y 48
        let w = pipe(PipeDir::Up, 1, 88.0, 40.0);
        let mut p = Player::new(88.0 + 7.0, 48.0);
        p.on_ground = true;
        let down = Input {
            buttons: input::DOWN,
        };
        assert!(pipe_entry(&p, down, SideHold::default(), false, &[w]).is_some());
        p.x = 88.0 + 8.5;
        assert!(pipe_entry(&p, down, SideHold::default(), false, &[w]).is_none());
    }

    #[test]
    fn side_entry_after_ten_frames() {
        // left-facing pipe, mouth centre (200, 40): player stands on the floor (y 32) to its left
        let w = pipe(PipeDir::Left, 1, 200.0, 40.0);
        let mut p = Player::new(200.0 - 8.0 - 7.9, 32.0);
        p.on_ground = true;
        let right = Input {
            buttons: input::RIGHT,
        };
        let mut hold = SideHold::default();
        for i in 0..10 {
            hold.update(right, true);
            let e = pipe_entry(&p, right, hold, false, &[w]);
            assert_eq!(e.is_some(), i == 9, "frame {i}");
        }
    }

    #[test]
    fn pipes_link_across_areas_doors_within() {
        let a0 = vec![
            pipe(PipeDir::Up, 3, 0.0, 0.0),
            Warp {
                kind: WarpKind::Door(0),
                id: 4,
                x: 10.0,
                y: 0.0,
                len: 2,
            },
            Warp {
                kind: WarpKind::Door(0),
                id: 5,
                x: 90.0,
                y: 0.0,
                len: 2,
            },
        ];
        let a1 = vec![pipe(PipeDir::Down, 3, 50.0, 50.0)];
        let areas = vec![a0.clone(), a1];
        let (area, to) = destination(&a0[0], 0, &areas).unwrap();
        assert_eq!((area, to.x), (1, 50.0));
        let (area, to) = destination(&a0[1], 0, &areas).unwrap();
        assert_eq!((area, to.x), (0, 90.0));
    }

    #[test]
    fn transit_timeline() {
        let from = pipe(PipeDir::Up, 1, 88.0, 40.0);
        let to = pipe(PipeDir::Up, 1, 300.0, 72.0);
        let mut p = Player::new(85.0, 48.0);
        let mut t = Transit {
            phase: Phase::Align,
            from,
            to,
            to_area: 1,
        };
        let mut frames = 0;
        let mut switched_at = None;
        while !finished(&t) {
            if step(&mut t, &mut p) {
                switched_at = Some(frames);
            }
            frames += 1;
            assert!(frames < 1000);
        }
        // align 3 px, sink 16 px at 0.75 (22 f), hold 30, hold 30, rise 22 f
        assert_eq!(switched_at, Some(3 + 22 + 29));
        assert_eq!((p.x, p.y), (300.0, 80.0));
    }
}
