//! In-course camera for normal (non-autoscroll) horizontal areas, single player.
//!
//! Written from the behavioural spec `research/specs/camera.md` (private), not from game code.
//! The pipeline each frame:
//!
//! 1. A **tracker** follows the player with a small dead-zone box (±4 px wide, 68 px tall) and a
//!    filtered "tracked y" that ignores vertical wobble under 4 px.
//! 2. The box's home point becomes the **focus**; the desired view is centred on it.
//! 3. **Limits**: the area rectangle, a 216 px ceiling in low-camera areas (nothing built above the
//!    line), and full-height solid columns that stop the scroll.
//! 4. Two **followers** ease the view centre toward the clamped target: horizontally at up to the
//!    player's speed plus a capped catch-up, vertically with a speed curve that is gentler upward.
//!
//! Not covered yet: autoscroll, custom autoscroll, vertical areas, multiplayer, zoom, pipes/doors.
//! Units: px and px/frame, y up. The view is 384 × 216 px.

use crate::world::{VIEW_H, VIEW_W};

const TILE: f32 = 16.0;
const EPS: f32 = f32::EPSILON;

/// Low-camera areas: the view top may not pass this (one screen), plus the high-player offset.
const LOW_CAMERA_TOP: f32 = 216.0;
/// Scroll-stop scans in low-camera areas only look at rows below this height.
const LOW_CAMERA_SCAN_TOP: f32 = 224.0;
/// Tracked y at or above this lifts a low-camera view by up to `HIGH_OFFSET`; below
/// `HIGH_OFF` it drops back (hysteresis).
const HIGH_ON: f32 = 224.0;
const HIGH_OFF: f32 = 220.0;
const HIGH_OFFSET: f32 = 16.0;

/// Dead-zone box: half-width, and the vertical extent around home when relaxed / shrunk.
const BOX_HALF_W: f32 = 4.0;
const BOX_BELOW: (f32, f32) = (28.0, 16.0);
const BOX_ABOVE: (f32, f32) = (40.0, 16.0);
/// Blend toward the shrunk box, per frame.
const BOX_BLEND_STEP: f32 = 0.015;
/// Vertical motion smaller than this does not move the tracked y (until it starts following).
const Y_HYSTERESIS: f32 = 4.0;
const Y_KEEP_FOLLOWING: f32 = 0.01;

/// Scroll-stop hold: frames a found limit is kept, the frame relaxing starts, relax speed.
const STOP_HOLD: i32 = 60;
const STOP_RELAX_FROM: i32 = 50;
const STOP_RELAX_SPEED: f32 = 4.0;

/// Join weight of the follower caps; a multiplayer ramp that is 1 for a single player.
const JOIN_WEIGHT: f32 = 1.0;

/// An axis-aligned rectangle in world px (y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

impl Rect {
    pub fn new(left: f32, bottom: f32, right: f32, top: f32) -> Self {
        Self {
            left,
            bottom,
            right,
            top,
        }
    }

    fn centred(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self::new(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
    }

    pub fn cx(&self) -> f32 {
        (self.left + self.right) * 0.5
    }

    pub fn cy(&self) -> f32 {
        (self.bottom + self.top) * 0.5
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    fn shifted(self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.left + dx,
            self.bottom + dy,
            self.right + dx,
            self.top + dy,
        )
    }

    /// Shift into `outer`, per axis: in from the low side first, then from the high side.
    fn shifted_into(self, outer: &Rect) -> Self {
        let dx = (outer.left - self.left).max(0.0);
        let dx = dx + (outer.right - (self.right + dx)).min(0.0);
        let dy = (outer.bottom - self.bottom).max(0.0);
        let dy = dy + (outer.top - (self.top + dy)).min(0.0);
        self.shifted(dx, dy)
    }

    fn fits_in(&self, outer: &Rect) -> bool {
        self.width() <= outer.width() && self.height() <= outer.height()
    }
}

/// The area as the camera sees it.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    /// Scroll rectangle from the area header.
    pub bounds: Rect,
    /// Header "boundary type" ≠ 0 in a horizontal area: nothing is built above the line, so the
    /// view stays in the first 13.5 tiles (one more tile when the player is high up).
    pub low_camera: bool,
    pub vertical: bool,
    /// The main area (0), not the sub-area.
    pub main: bool,
}

impl Area {
    fn low(&self) -> bool {
        self.low_camera && !self.vertical
    }

    /// Scroll-stop column scans stop below this height.
    fn scan_top(&self) -> f32 {
        if self.low() {
            LOW_CAMERA_SCAN_TOP
        } else {
            self.bounds.top
        }
    }
}

/// What the player is doing, as far as the camera cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Walking, running, jumping, falling.
    Play,
    /// Swimming, climbing a vine, flying, riding a cloud (none in the engine yet).
    Floating,
    /// The goal sequence, from touching the pole on.
    Goal,
    /// Dead, or entering a pipe/door: the view does not move.
    Frozen,
}

/// The player as the camera needs it this frame.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    /// Horizontal centre and feet.
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub on_ground: bool,
    pub activity: Activity,
}

/// Tracker mode (the subset of player camera modes the engine produces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// On the ground or in a jump.
    Ground,
    /// Airborne after walking off a ledge.
    Fall,
    Floating,
    Goal,
}

/// Moves `a` toward `b` by at most `step`.
fn toward(a: f32, b: f32, step: f32) -> f32 {
    if a < b {
        (a + step).min(b)
    } else {
        (a - step).max(b)
    }
}

/// Truncate to 0.01 px.
fn hundredths(v: f32) -> f32 {
    (v * 100.0).trunc() * 0.01
}

/// The player tracker: filtered y, dead-zone box and the outputs derived from them.
#[derive(Clone, Debug)]
struct Tracker {
    x: f32,
    y: f32,
    prev_x: f32,
    prev_y: f32,
    tracked_y: f32,
    prev_tracked_y: f32,
    /// tracked y is following y (past the 4 px threshold).
    following: bool,
    /// The box drifts up so the feet end 16 px above home.
    settling: bool,
    /// tracked y above 14 tiles (low-camera offset).
    high: bool,
    /// 0 = relaxed box, 1 = shrunk box.
    blend: f32,
    /// Box extent relative to home (below is negative).
    rel_bottom: f32,
    rel_top: f32,
    dead_zone: Rect,
}

impl Tracker {
    fn rel(blend: f32) -> (f32, f32) {
        (
            -(BOX_BELOW.1 * blend + BOX_BELOW.0 * (1.0 - blend)),
            BOX_ABOVE.1 * blend + BOX_ABOVE.0 * (1.0 - blend),
        )
    }

    /// Fresh tracker at the player, with the box placed so that its home is the view centre.
    fn reset(x: f32, y: f32, view: &Rect) -> Self {
        let (rel_bottom, rel_top) = Self::rel(0.0);
        let half_h = (rel_top - rel_bottom) * 0.5;
        let mut t = Self {
            x,
            y,
            prev_x: x,
            prev_y: y,
            tracked_y: y,
            prev_tracked_y: y,
            following: false,
            settling: false,
            high: false,
            blend: 0.0,
            rel_bottom,
            rel_top,
            dead_zone: Rect::new(x - BOX_HALF_W, y - half_h, x + BOX_HALF_W, y + half_h),
        };
        let home = t.home();
        t.dead_zone = t
            .dead_zone
            .shifted(view.cx() - home.cx(), view.cy() - home.cy());
        t
    }

    fn update(&mut self, mode: Mode, x: f32, y: f32, low_area: bool) {
        self.prev_x = self.x;
        self.prev_y = self.y;
        self.prev_tracked_y = self.tracked_y;
        self.x = x;
        self.y = y;

        // tracked y
        if low_area {
            // falls follow only downward
            if !(mode == Mode::Fall && y >= self.tracked_y) {
                self.tracked_y = y;
            }
            self.following = false;
        } else if matches!(mode, Mode::Ground | Mode::Goal) {
            let threshold = if self.following {
                Y_KEEP_FOLLOWING
            } else {
                Y_HYSTERESIS
            };
            self.following = (y - self.tracked_y).abs() > threshold;
            if self.following {
                self.tracked_y = y;
            }
        } else {
            self.tracked_y = y;
            self.following = false;
        }

        // box size: shrinks while rising or floating
        let target = match mode {
            Mode::Floating => 1.0,
            Mode::Ground if self.tracked_y - self.prev_tracked_y > EPS => 1.0,
            _ => 0.0,
        };
        self.blend = toward(self.blend, target, BOX_BLEND_STEP);
        let (rb, rt) = Self::rel(self.blend);
        let b = &mut self.dead_zone;
        let (mut bottom, mut top) = (b.bottom + rb - self.rel_bottom, b.top + rt - self.rel_top);
        if top < bottom {
            std::mem::swap(&mut top, &mut bottom);
        }
        b.bottom = bottom;
        b.top = top;
        self.rel_bottom = rb;
        self.rel_top = rt;

        // drag
        let dx = if x < b.left {
            x - b.left
        } else if x > b.right {
            x - b.right
        } else {
            0.0
        };
        let ty = self.tracked_y;
        let mut dy = if ty < b.bottom {
            ty - b.bottom
        } else if ty > b.top {
            ty - b.top
        } else {
            0.0
        };

        // settle up: standing high in the box drifts it up until the feet are 16 px above home
        if dy == 0.0 {
            if mode == Mode::Ground {
                let anchor = b.bottom - rb + 16.0;
                if !self.settling {
                    self.settling = ty > anchor + 8.0;
                } else {
                    if ty <= anchor {
                        self.settling = false;
                    }
                    let step = ((ty - anchor).abs() * 0.1).max(0.1);
                    dy = toward(anchor, ty, step) - anchor;
                }
            } else {
                self.settling = false;
            }
        }
        *b = b.shifted(dx, dy);

        if !self.high && ty >= HIGH_ON {
            self.high = true;
        } else if self.high && ty < HIGH_OFF {
            self.high = false;
        }
    }

    /// The box's home point with a (±8, ±16) margin: what the view centres on.
    fn home(&self) -> Rect {
        let hx = hundredths(self.dead_zone.cx());
        let hy = hundredths(self.dead_zone.bottom - self.rel_bottom);
        Rect::centred(hx, hy, 16.0, 32.0)
    }

    /// The region the view must keep (nudged at ≤ 1 px/frame), if the situation asks for one:
    /// at the goal, or when running toward a view edge within 16 px of it.
    fn must_contain(&self, mode: Mode, vx: f32, view: &Rect) -> Option<Rect> {
        let priority = if mode == Mode::Goal {
            1
        } else if vx.abs() <= EPS {
            6
        } else if (vx <= 0.0 && self.x < view.left + 16.0)
            || (vx > 0.0 && self.x > view.right - 16.0)
        {
            4
        } else {
            5
        };
        // from the player's centre to 32 px above it
        let cy = self.y + 16.0;
        (priority < 5).then(|| Rect::new(self.x - 8.0, cy, self.x + 8.0, cy + 32.0))
    }
}

/// The in-course camera.
#[derive(Clone, Debug)]
pub struct Camera {
    /// The view (384 × 216), unclamped; `view()` gives the rendered one.
    view: Rect,
    /// Scroll limits this frame.
    limits: Rect,
    /// Frames left on a scroll-stop limit.
    hold: i32,
    /// Low-camera ceiling offset (0..16).
    top_offset: f32,
    /// The follow state was just entered (limits start from the area).
    entering: bool,
    tracker: Tracker,
    focus: Option<Rect>,
    /// Airborne after walking off a ledge (not a jump), and last frame's ground contact.
    falling: bool,
    grounded: bool,
    was_goal: bool,
    /// Vertical follower: strategy, current speed, blend weight.
    strategy: u8,
    speed_y: f32,
    weight: f32,
}

impl Camera {
    /// Camera at course start: centred on the player (feet `y`), clamped into the area.
    pub fn start(area: &Area, x: f32, y: f32, solid: impl Fn(i32, i32) -> bool) -> Self {
        let mut cam = Self {
            view: Rect::centred(x, y + 16.0, VIEW_W, VIEW_H),
            limits: area.bounds,
            hold: 0,
            top_offset: 0.0,
            entering: true,
            tracker: Tracker::reset(x, y, &area.bounds),
            focus: None,
            falling: false,
            grounded: true,
            was_goal: false,
            // the start strategy only ever descends [spec §4.6]
            strategy: 0,
            speed_y: 0.0,
            weight: 0.0,
        };
        cam.reset(area, (x, y + 16.0), (x, y), solid);
        cam
    }

    /// Re-centre on `centre` (respawn, area change), keeping the vertical follower; `player` is
    /// the player's centre-x and feet.
    pub fn reset(
        &mut self,
        area: &Area,
        centre: (f32, f32),
        player: (f32, f32),
        solid: impl Fn(i32, i32) -> bool,
    ) {
        let bounds = area.bounds;
        self.view = Rect::centred(centre.0, centre.1, VIEW_W, VIEW_H);
        let mut l = bounds;
        let scan_top = area.scan_top();

        // nearest full-height solid column either side of the centre, while a view still fits
        let half_w = bounds.width() * 0.5;
        let mut off = 8.0;
        while off < half_w {
            let xr = self.view.cx() + off;
            // (the game also requires x > 0, implied here)
            if xr > VIEW_W - 17.0 && column_solid(&solid, tile(xr), bounds.bottom, scan_top) {
                l.right = tile(xr) as f32 * TILE + TILE;
                break;
            }
            let xl = self.view.cx() - off;
            if xl < bounds.right - (VIEW_W - 16.0)
                && xl > 0.0
                && column_solid(&solid, tile(xl), bounds.bottom, scan_top)
            {
                l.left = tile(xl) as f32 * TILE;
                break;
            }
            off += TILE;
        }
        // and full-width solid rows above or below
        let half_h = bounds.height() * 0.5;
        let mut off = 8.0;
        while off < half_h {
            let yt = self.view.cy() + off;
            if yt > VIEW_H - 17.0 && row_solid(&solid, tile(yt), &bounds) {
                l.top = tile(yt) as f32 * TILE + TILE;
                break;
            }
            let yb = self.view.cy() - off;
            if yb < l.top - (VIEW_H - 16.0) && yb > 0.0 && row_solid(&solid, tile(yb), &bounds) {
                l.bottom = tile(yb) as f32 * TILE;
                break;
            }
            off += TILE;
        }
        if area.low() {
            l.top = LOW_CAMERA_TOP;
        }
        self.limits = l;
        self.view = self.view.shifted_into(&l).shifted_into(&bounds);
        self.top_offset = 0.0;
        self.tracker = Tracker::reset(player.0, player.1, &self.view);
        self.focus = None;
        self.entering = true;
        self.falling = false;
        self.grounded = true;
    }

    /// The rendered view (clamped into the area).
    pub fn view(&self, area: &Area) -> Rect {
        self.view.shifted_into(&area.bounds)
    }

    /// One frame, after the player has moved. `solid(tx, ty)` reports a full solid tile.
    pub fn update(&mut self, area: &Area, target: &Target, solid: impl Fn(i32, i32) -> bool) {
        let mode = match target.activity {
            // the view stays where it is
            Activity::Frozen => return,
            Activity::Goal => Mode::Goal,
            Activity::Floating => Mode::Floating,
            Activity::Play => {
                if target.on_ground || target.vy > 0.0 {
                    self.falling = false; // standing, or a jump / bounce
                } else if self.grounded {
                    self.falling = true; // walked off
                }
                if self.falling {
                    Mode::Fall
                } else {
                    Mode::Ground
                }
            }
        };
        self.grounded = target.on_ground;
        let goal_starts = mode == Mode::Goal && !self.was_goal;
        self.was_goal = mode == Mode::Goal;

        // tracker → focus
        self.tracker.update(mode, target.x, target.y, area.low());
        let motion = (
            self.tracker.x - self.tracker.prev_x,
            self.tracker.y - self.tracker.prev_y,
        );
        let home = self.tracker.home();
        self.focus = Some(match self.focus {
            None => home,
            Some(f) => {
                let (dx, dy) = (home.cx() - f.cx(), home.cy() - f.cy());
                let len = (dx * dx + dy * dy).sqrt();
                let (ux, uy) = if len > 0.0 {
                    (dx / len, dy / len)
                } else {
                    (0.0, 0.0)
                };
                let cx = toward(f.cx(), home.cx(), motion.0.abs() + ux.abs());
                let cy = toward(f.cy(), home.cy(), motion.1.abs() + uy.abs());
                Rect::centred(cx, cy, home.width(), home.height())
            }
        });
        let focus = self.focus.unwrap_or(home);
        let must = self.tracker.must_contain(mode, target.vx, &self.view);

        // low-camera ceiling lifts by a tile while the player is high up
        let lift = if area.low() && self.tracker.high {
            HIGH_OFFSET
        } else {
            0.0
        };
        self.top_offset = toward(self.top_offset, lift, 1.0);

        if self.entering {
            self.entering = false;
            self.limits = area.bounds;
            self.hold = 0;
        }

        // desired view
        let mut desired = Rect::centred(focus.cx(), focus.cy(), VIEW_W, VIEW_H);
        if !area.vertical && desired.bottom > focus.bottom - 16.0 {
            desired = desired.shifted(0.0, focus.bottom - 16.0 - desired.bottom);
        }
        let must = must.map(|m| {
            Rect::centred(
                m.cx(),
                m.cy(),
                m.width().min(VIEW_W),
                m.height().min(VIEW_H),
            )
            .shifted_into(&self.limits)
            .shifted_into(&area.bounds)
        });
        if let Some(m) = must.filter(|m| m.fits_in(&desired)) {
            desired = desired.shifted(contain_shift_x(&m, &desired), contain_shift_y(&m, &desired));
        }

        self.update_limits(area, &desired, &solid);
        let goal_rect = desired
            .shifted_into(&self.limits)
            .shifted_into(&area.bounds);

        // followers
        let vx_toward = {
            let (cx, tx) = (self.view.cx(), goal_rect.cx());
            let toward_target = if tx <= cx {
                target.vx < 0.0
            } else {
                target.vx > 0.0
            };
            if toward_target { target.vx.abs() } else { 0.0 }
        };
        let motion_x = if goal_starts { 0.0 } else { motion.0 };
        let cx = follow_x(
            self.view.cx(),
            goal_rect.cx(),
            vx_toward,
            motion_x,
            mode == Mode::Goal,
        );
        let requested = match mode {
            Mode::Goal => 4,
            Mode::Floating => 3,
            _ => 1,
        };
        let cy = self.follow_y(area, self.view.cy(), goal_rect.cy(), target.vy, requested);
        self.view = Rect::centred(cx, cy, VIEW_W, VIEW_H);

        // keep the must-contain region in view, at most 1 px per frame
        if let Some(m) = must.filter(|m| m.fits_in(&self.view)) {
            let (mut dx, mut dy) = (
                contain_shift_x(&m, &self.view),
                contain_shift_y(&m, &self.view),
            );
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1.0 {
                dx /= len;
                dy /= len;
            }
            self.view = self.view.shifted(dx, dy);
        }
    }

    /// Scroll limits for this frame [spec §5].
    fn update_limits(&mut self, area: &Area, desired: &Rect, solid: &impl Fn(i32, i32) -> bool) {
        let bounds = area.bounds;
        let mut base = bounds;
        if area.low() {
            base.top = LOW_CAMERA_TOP + self.top_offset;
        }
        let scan_top = area.scan_top();
        let view = self.view;
        let mut l = base;
        let mut found = false;
        if desired.right > view.right {
            let col = tile(view.right - 4.0);
            if column_solid(solid, col, bounds.bottom, scan_top) {
                l.right = col as f32 * TILE + TILE;
                found = true;
            }
        }
        if desired.left < view.left {
            let col = tile(view.left + 4.0);
            if column_solid(solid, col, bounds.bottom, scan_top) {
                l.left = col as f32 * TILE;
                found = true;
            }
        }
        if desired.top > view.top {
            let row = tile(view.top - 4.0);
            if row_solid(solid, row, &bounds) {
                l.top = l.top.min(row as f32 * TILE + TILE);
                found = true;
            }
        }
        if desired.bottom < view.bottom {
            let row = tile(view.bottom + 4.0);
            if row_solid(solid, row, &bounds) {
                l.bottom = (row as f32 * TILE).max(bounds.bottom);
                found = true;
            }
        }

        if found {
            self.limits = l;
            self.hold = STOP_HOLD;
        } else if self.hold == 0 {
            self.limits = base;
        } else {
            let h = self.hold;
            self.hold -= 1;
            if h <= STOP_RELAX_FROM {
                let old = self.limits;
                let (right, top) = step_point(
                    (old.right, old.top),
                    (base.right, base.top),
                    STOP_RELAX_SPEED,
                );
                let (left, bottom) = step_point(
                    (old.left, old.bottom),
                    (base.left, base.bottom),
                    STOP_RELAX_SPEED,
                );
                self.limits = Rect::new(left, bottom, right, top);
            }
        }
    }

    /// Vertical follower [spec §4.6].
    fn follow_y(&mut self, area: &Area, cy: f32, ty: f32, vy: f32, requested: u8) -> f32 {
        let mut s = requested;
        if self.strategy == 0 && area.main && area.low() && ty < cy {
            s = 0;
        }
        if s != self.strategy {
            self.strategy = s;
            self.weight = 0.0;
        }
        let half = VIEW_H * 0.5;
        let floor = area.bounds.bottom + half;
        let goal = if ty >= floor {
            ty.min(area.bounds.top - half)
        } else {
            floor
        };
        let v = strategy_speed(s, goal - cy);
        self.weight = toward(self.weight, 1.0, 0.1);
        if self.speed_y.abs() <= v.abs() {
            let step = ((v - self.speed_y) * self.weight).abs().max(0.1);
            self.speed_y = toward(self.speed_y, v, step);
        } else {
            self.speed_y = v;
        }
        let with_player = if cy < ty {
            self.speed_y + vy.max(0.0)
        } else {
            self.speed_y + (-vy).max(0.0)
        };
        toward(cy, goal, with_player.max(0.2))
    }
}

impl Default for Camera {
    /// A camera on a flat one-screen area at the origin (replaced when a course starts).
    fn default() -> Self {
        let area = Area {
            bounds: Rect::new(0.0, 0.0, VIEW_W, VIEW_H),
            low_camera: false,
            vertical: false,
            main: true,
        };
        Self::start(&area, VIEW_W / 2.0, 0.0, |_, _| false)
    }
}

/// Horizontal follower, normal state [spec §4.5]. `vx_toward` is |vx| when the player moves the
/// way the camera must move, else 0; `motion_x` is the player's x motion this frame.
fn follow_x(cx: f32, tx: f32, vx_toward: f32, motion_x: f32, goal: bool) -> f32 {
    let a = (tx - cx).abs();
    let distance_cap = if a >= 32.0 {
        let r = ((a - 32.0) / 64.0).clamp(0.0, 1.0);
        8.5 * r + 5.0 * (1.0 - r)
    } else {
        5.0
    };
    let floor = if goal { 2.0 } else { 1.0 };
    let s = motion_x.abs() * 1.5 + 1.0;
    let player_cap = if floor <= s {
        (JOIN_WEIGHT * 5.0 + 1.0).min(s)
    } else {
        floor
    };
    let cap = player_cap.min(distance_cap);
    let base = a * 0.3;
    let base = if base < 0.01 { 0.01 } else { base.min(cap) };
    toward(cx, tx, (base + vx_toward).max(0.2))
}

/// Vertical strategy speeds for a distance `d` (target − current) [spec §4.6 table].
fn strategy_speed(strategy: u8, d: f32) -> f32 {
    let a = d.abs();
    let curve = |cap_up: f32, cap_down: f32| {
        let f = a / 16.0;
        let (k, cap) = if d <= 0.0 {
            (0.3 * f + 0.2, cap_down)
        } else {
            (0.4 * f + 0.1, cap_up)
        };
        let v = a * k;
        if v < 0.01 {
            0.01
        } else {
            v.min(JOIN_WEIGHT * cap + 0.01)
        }
    };
    match strategy {
        0 => {
            if d < 0.0 {
                a * 0.1
            } else {
                0.0
            }
        }
        1 => curve(4.99, 7.99),
        2 => a * 0.5,
        3 => (a * 0.1).min(8.0 * JOIN_WEIGHT),
        4 => curve(4.99, 4.99),
        _ => a,
    }
}

/// Least x shift of `outer` that contains `inner` (the high edge wins if both are out).
fn contain_shift_x(inner: &Rect, outer: &Rect) -> f32 {
    if inner.right - outer.right > 0.0 {
        inner.right - outer.right
    } else {
        (inner.left - outer.left).min(0.0)
    }
}

fn contain_shift_y(inner: &Rect, outer: &Rect) -> f32 {
    if inner.top - outer.top > 0.0 {
        inner.top - outer.top
    } else {
        (inner.bottom - outer.bottom).min(0.0)
    }
}

/// Move `p` toward `q` along the line, at most `step`.
fn step_point(p: (f32, f32), q: (f32, f32), step: f32) -> (f32, f32) {
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len > step {
        (p.0 + dx / len * step, p.1 + dy / len * step)
    } else {
        q
    }
}

fn tile(px: f32) -> i32 {
    (px / TILE).floor() as i32
}

/// Every tile of column `col` from the bottom row up to (not including) `top` is solid.
fn column_solid(solid: &impl Fn(i32, i32) -> bool, col: i32, bottom: f32, top: f32) -> bool {
    let mut y = bottom + 8.0;
    while y < top {
        if !solid(col, tile(y)) {
            return false;
        }
        y += TILE;
    }
    true
}

/// Every tile of row `row` across the area is solid.
fn row_solid(solid: &impl Fn(i32, i32) -> bool, row: i32, bounds: &Rect) -> bool {
    let mut x = bounds.left + 8.0;
    while x < bounds.right {
        if !solid(tile(x), row) {
            return false;
        }
        x += TILE;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: f32 = 32.0;

    fn area(width_tiles: f32, low_camera: bool) -> Area {
        Area {
            bounds: Rect::new(0.0, 0.0, width_tiles * TILE, 432.0),
            low_camera,
            vertical: false,
            main: true,
        }
    }

    fn empty(_: i32, _: i32) -> bool {
        false
    }

    fn standing(x: f32, y: f32) -> Target {
        Target {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            on_ground: true,
            activity: Activity::Play,
        }
    }

    /// Run the camera while the player walks right at `speed` from `x0` for `frames`.
    fn walk_on(
        cam: &mut Camera,
        a: &Area,
        x0: f32,
        speed: f32,
        frames: usize,
        solid: impl Fn(i32, i32) -> bool + Copy,
    ) -> f32 {
        let mut x = x0;
        for _ in 0..frames {
            x += speed;
            let t = Target {
                vx: speed,
                ..standing(x, GROUND)
            };
            cam.update(a, &t, solid);
        }
        x
    }

    fn walk(cam: &mut Camera, a: &Area, x0: f32, speed: f32, frames: usize) -> f32 {
        walk_on(cam, a, x0, speed, frames, empty)
    }

    #[test]
    fn course_start_shows_the_bottom_left_corner() {
        let a = area(100.0, false);
        let cam = Camera::start(&a, 56.0, GROUND, empty);
        let v = cam.view(&a);
        assert_eq!((v.left, v.bottom), (0.0, 0.0));
        assert_eq!(v.width(), VIEW_W);
        assert_eq!(v.height(), VIEW_H);
    }

    #[test]
    fn no_scroll_until_the_player_passes_the_dead_zone() {
        let a = area(100.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        // the view centre is 192; the box lets the player 4 px past it before dragging
        let mut x = 56.0;
        while x < 196.0 {
            x += 1.0;
            cam.update(&a, &standing(x, GROUND), empty);
            assert_eq!(cam.view(&a).left, 0.0, "scrolled at x = {x}");
        }
        for _ in 0..3 {
            x += 1.0;
            cam.update(&a, &standing(x, GROUND), empty);
        }
        assert!(cam.view(&a).left > 0.0);
    }

    #[test]
    fn running_keeps_the_player_near_the_centre() {
        let a = area(200.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        let x = walk(&mut cam, &a, 56.0, 3.0, 400);
        let v = cam.view(&a);
        let off = x - v.cx();
        assert!(
            (0.0..=8.0).contains(&off),
            "player {off} px from the centre"
        );
    }

    #[test]
    fn never_shows_beyond_the_area() {
        let a = area(40.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        let x = walk(&mut cam, &a, 56.0, 4.0, 300);
        assert!(x > a.bounds.right);
        // the unclamped view itself stays inside, not just the rendered one
        assert_eq!(cam.view.right, a.bounds.right);
        // flying very high keeps the top inside
        for i in 0..200 {
            let t = Target {
                vy: 6.0,
                on_ground: false,
                ..standing(300.0, GROUND + 6.0 * i as f32)
            };
            cam.update(&a, &t, empty);
            let v = cam.view;
            assert!(v.top <= a.bounds.top && v.bottom >= 0.0 && v.left >= 0.0);
        }
        assert_eq!(cam.view.top, a.bounds.top);
    }

    #[test]
    fn a_jump_from_the_ground_does_not_scroll_up() {
        let a = area(100.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        // a 5-tile jump: up 80 px and back
        let mut y = GROUND;
        let mut vy: f32 = 7.0;
        for _ in 0..200 {
            y = (y + vy).max(GROUND);
            vy -= 0.3;
            let t = Target {
                vy,
                on_ground: y <= GROUND,
                ..standing(100.0, y)
            };
            cam.update(&a, &t, empty);
            assert_eq!(cam.view(&a).bottom, 0.0);
        }
    }

    #[test]
    fn standing_on_high_ground_brings_the_view_up() {
        let a = area(100.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        // the player climbs onto ground 12 tiles up and stands there
        let high = 12.0 * TILE;
        for _ in 0..300 {
            cam.update(&a, &standing(100.0, high), empty);
        }
        let v = cam.view(&a);
        assert!(v.bottom > 0.0);
        // settled: feet 16 px above the centre
        assert!(
            (high - (v.cy() + 16.0)).abs() < 1.0,
            "feet at {} vs centre {}",
            high,
            v.cy()
        );
    }

    #[test]
    fn small_vertical_wobble_is_ignored() {
        let a = area(100.0, false);
        let mut cam = Camera::start(&a, 56.0, 200.0, empty);
        for _ in 0..600 {
            cam.update(&a, &standing(100.0, 200.0), empty);
        }
        let settled = cam.view(&a).bottom;
        for i in 0..100 {
            let y = 200.0 + if i % 2 == 0 { 3.0 } else { 0.0 };
            cam.update(&a, &standing(100.0, y), empty);
        }
        assert_eq!(cam.view(&a).bottom, settled);
    }

    #[test]
    fn vertical_follow_is_gentler_upward() {
        assert!(strategy_speed(1, 64.0) < strategy_speed(1, -64.0));
        assert!((strategy_speed(1, 200.0) - 5.0).abs() < 1e-4);
        assert!((strategy_speed(1, -200.0) - 8.0).abs() < 1e-4);
        assert_eq!(strategy_speed(0, 50.0), 0.0);
        assert!((strategy_speed(0, -50.0) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn low_camera_area_stays_on_the_first_screen() {
        let a = area(100.0, true);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        // standing at 13 tiles: below the 224 px "high" line, so the view top stays at 216
        for _ in 0..300 {
            cam.update(&a, &standing(150.0, 13.0 * TILE), empty);
        }
        assert_eq!(cam.view(&a).top, LOW_CAMERA_TOP);
        // above 14 tiles the ceiling lifts by one tile, no further
        for _ in 0..300 {
            cam.update(&a, &standing(150.0, 20.0 * TILE), empty);
        }
        assert!((cam.view(&a).top - (LOW_CAMERA_TOP + HIGH_OFFSET)).abs() < 1e-3);
    }

    #[test]
    fn a_full_height_wall_stops_the_scroll() {
        let a = area(100.0, false);
        let wall = 40;
        let solid = |x: i32, _y: i32| x == wall;
        let mut cam = Camera::start(&a, 56.0, GROUND, solid);
        walk_on(&mut cam, &a, 56.0, 2.0, 400, solid);
        assert_eq!(cam.view.right, (wall + 1) as f32 * TILE);
        // a wall with a gap does not stop it
        let gappy = |x: i32, y: i32| x == wall && y != 5;
        let mut cam = Camera::start(&a, 56.0, GROUND, gappy);
        walk_on(&mut cam, &a, 56.0, 2.0, 400, gappy);
        assert!(cam.view(&a).right > (wall + 1) as f32 * TILE);
    }

    #[test]
    fn a_removed_wall_releases_the_limit_slowly() {
        let a = area(100.0, false);
        let wall = 40;
        let present = std::cell::Cell::new(true);
        let solid = |x: i32, _y: i32| present.get() && x == wall;
        let mut cam = Camera::start(&a, 56.0, GROUND, solid);
        let x = walk_on(&mut cam, &a, 56.0, 2.0, 400, solid);
        let stop = (wall + 1) as f32 * TILE;
        assert_eq!(cam.limits.right, stop);
        present.set(false);
        let t = standing(x, GROUND);
        // held for 10 frames, then relaxed at 4 px per frame
        for _ in 0..10 {
            cam.update(&a, &t, solid);
            assert_eq!(cam.limits.right, stop);
        }
        cam.update(&a, &t, solid);
        assert!((cam.limits.right - (stop + 4.0)).abs() < 1e-3);
        for _ in 0..60 {
            cam.update(&a, &t, solid);
        }
        assert_eq!(cam.limits.right, a.bounds.right);
    }

    #[test]
    fn course_start_next_to_a_wall_is_limited_by_it() {
        let a = area(100.0, false);
        let wall = 30;
        let solid = |x: i32, _y: i32| x == wall;
        let cam = Camera::start(&a, 26.0 * TILE, GROUND, solid);
        assert_eq!(cam.view(&a).right, (wall + 1) as f32 * TILE);
    }

    #[test]
    fn frozen_while_dead() {
        let a = area(100.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        walk(&mut cam, &a, 56.0, 3.0, 100);
        let before = cam.view(&a);
        for i in 0..60 {
            let t = Target {
                activity: Activity::Frozen,
                ..standing(400.0 + i as f32, -50.0)
            };
            cam.update(&a, &t, empty);
        }
        assert_eq!(cam.view(&a), before);
    }

    #[test]
    fn goal_keeps_following_within_the_area() {
        let a = area(60.0, false);
        let mut cam = Camera::start(&a, 56.0, GROUND, empty);
        let x = walk(&mut cam, &a, 56.0, 3.0, 250);
        for i in 0..200 {
            let t = Target {
                activity: Activity::Goal,
                vx: 1.0,
                ..standing(x + i as f32, GROUND)
            };
            cam.update(&a, &t, empty);
            assert!(cam.view(&a).right <= a.bounds.right);
        }
        assert_eq!(cam.view(&a).right, a.bounds.right);
    }

    #[test]
    fn follower_catch_up_is_capped() {
        // far behind and the player stopped: 5..8.5 px per frame
        assert!((follow_x(0.0, 20.0, 0.0, 0.0, false) - 1.0).abs() < 1e-4);
        assert!((follow_x(0.0, 200.0, 0.0, 10.0, false) - 6.0).abs() < 1e-4);
        assert!((follow_x(0.0, 200.0, 0.0, 0.0, false) - 1.0).abs() < 1e-4);
        // the player's speed is added when moving the same way
        assert!((follow_x(0.0, 200.0, 3.0, 3.0, false) - 8.5).abs() < 1e-4);
    }
}
