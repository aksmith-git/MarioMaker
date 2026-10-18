//! Player movement and terrain collision, SMB1 style.
//!
//! Implemented from research/specs/player-physics.md (behavioural spec): §1.3 integration, §2.2
//! ground, §2.3 skid, §2.4 air, §3.1 jump launch, §3.2 gravity, §6.1 terrain sensors.
//! Units: px, px/frame, 60 Hz, y up; position = feet centre.

use crate::input::{self, Input};
use crate::level::{Cell, Level, Slope, TILE};

// the 2.0 / 3.0 update power-ups (player/powerups.rs)
mod powerups;
pub use powerups::*;

/// Speed thresholds T0..T3 (§2.1); row 1 while the star runs.
const T: [f32; 4] = [0.5, 1.5, 2.25, 3.0];
const T_STAR: [f32; 4] = [0.5, 2.0, 3.25, 4.0];
pub const WALK_MAX: f32 = T[1];
pub const RUN_MAX: f32 = T[3];
pub const FALL_CAP: f32 = -4.0;

// Acceleration tables, 6 bands: b0 = reversing, b1..b4 = |v| ≤ T0..T3 in the facing direction,
// b5 = above T3 (§2.2, §2.4).
const GROUND_WALK: [f32; 6] = [0.1, 0.1, 0.03, 0.03, 0.035, 0.75];
const GROUND_RUN: [f32; 6] = [0.1, 0.1, 0.06, 0.029, 0.035, 0.75];
const GROUND_IDLE: [f32; 6] = [0.07, 0.05, 0.05, 0.035, 0.035, 0.75];
const GROUND_CROUCH: [f32; 6] = [0.07, 0.06, 0.06, 0.05, 0.05, 0.75];
const GROUND_WALK_STAR: [f32; 6] = [0.13, 0.13, 0.04, 0.04, 0.035, 0.75];
const GROUND_RUN_STAR: [f32; 6] = [0.13, 0.13, 0.078, 0.039, 0.035, 0.75];
const ICE_WALK: [f32; 6] = [0.02, 0.02, 0.04, 0.04, 0.013, 0.024];
const ICE_WALK_STAR: [f32; 6] = [0.027, 0.06, 0.04, 0.04, 0.013, 0.024];
const ICE_RUN: [f32; 6] = [0.02, 0.02, 0.06, 0.028, 0.013, 0.024];
const ICE_RUN_STAR: [f32; 6] = [0.027, 0.06, 0.078, 0.039, 0.013, 0.024];
const ICE_IDLE: [f32; 6] = [0.018, 0.015, 0.015, 0.013, 0.013, 0.024];
const ICE_CROUCH: [f32; 6] = [0.018, 0.018, 0.018, 0.015, 0.015, 0.024];
const AIR_DIR: [f32; 6] = [0.09, 0.08, 0.03, 0.029, 0.021, 0.021];
const AIR_DIR_RUN: [f32; 6] = [0.09, 0.08, 0.06, 0.029, 0.021, 0.021];
const AIR_DIR_STAR: [f32; 6] = [0.12, 0.11, 0.04, 0.039, 0.027, 0.027];
const AIR_DIR_RUN_STAR: [f32; 6] = [0.12, 0.11, 0.078, 0.039, 0.027, 0.027];

// Slopes, by steepness index 0..4 (§5.2)
const SLOPE_TARGET_DOWN: [f32; 5] = [1.0, 1.25, 1.25, 1.44, 1.62];
const SLOPE_TARGET_UP: [f32; 5] = [1.0, 0.6875, 0.6875, 0.5, 0.3];
const SLOPE_ACCEL_DOWN: [f32; 5] = [0.033, 0.044, 0.055, 0.077, 0.187];
const SLOPE_ACCEL_UP: [f32; 5] = [0.027, 0.027, 0.04, 0.108, 0.153];
const SLOPE_DECEL_DOWN: [f32; 5] = [0.035, 0.0315, 0.028, 0.021, 0.014];
const SLOPE_DECEL_UP: [f32; 5] = [0.035, 0.046, 0.06, 0.123, 0.175];
/// How far the feet may snap up/down to follow a slope surface while grounded (px). [inf]
const SLOPE_SNAP: f32 = 6.0;

const SKID_ENTER: f32 = 2.5;
const SKID_DECEL: f32 = 0.12;
const SKID_DECEL_ICE: f32 = 0.028;
const SKID_DECEL_ICE_STAR: f32 = 0.038;
const SKID_REACCEL: f32 = 0.25;
/// Cat scratch (ClimbAttack, 17-frame animation) and slide attack (ClimbSlidingAttack, 44)
/// lengths [obs: 3W_Player_Animation_MarioClimb].
pub const CAT_SCRATCH_FRAMES: i32 = 17;
const CAT_SLIDE_FRAMES: i32 = 44;
/// Frames the Turn state holds the stopped player (counter +0x242c = 8) [obs].
const SKID_PAUSE: i32 = 8;

const JUMP_BASE: f32 = 3.628;
/// 3D World long jump: launch vy, fixed gravity, landing hold [obs: binary 0x71009d3020;
/// landing length from a Ryujinx trace].
const BROAD_VY: f32 = 2.4;
const BROAD_GRAVITY: f32 = -0.1;
const BROAD_LAND_FRAMES: u8 = 30;
const GRAVITY_GROUND: f32 = -0.34;
/// vy while standing (the game's ground state keeps it; positions are unaffected).
const GROUNDED_VY: f32 = -0.5;
// vy band boundaries and gravities (§3.2)
const VY_BANDS: [f32; 5] = [2.5, 1.5, 0.3, -0.12, -3.0];
const GRAVITY_HELD: [f32; 6] = [-0.06, -0.25, -0.34, -0.08, -0.31, -0.34];
const GRAVITY_RELEASED: [f32; 6] = [-0.34, -0.34, -0.34, -0.25, -0.34, -0.34];

/// Terrain sensors, small Mario in M1/M3 (§6.1): feet ±4.9 at y 0, head ±2 at y 12, walls ±7.9
/// for y 4…8.5.
pub struct Sensors {
    pub foot_x: f32,
    pub head_x: f32,
    pub head_y: f32,
    pub wall_y0: f32,
    pub wall_y1: f32,
    pub wall_x: f32,
}
pub const SMALL_M1: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 12.0,
    wall_y0: 4.0,
    wall_y1: 8.5,
    wall_x: 7.9,
};
/// Super/Fire standing (§6.1 row 0) and crouching (row 1).
pub const SUPER: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 27.0,
    wall_y0: 4.0,
    wall_y1: 19.0,
    wall_x: 7.9,
};
/// Mega Mario (class 3, §6.1): wide feet and wall probes, Super height.
pub const MEGA: Sensors = Sensors {
    foot_x: 9.9,
    head_x: 8.0,
    head_y: 27.0,
    wall_y0: 4.0,
    wall_y1: 19.0,
    wall_x: 11.9,
};
pub const SUPER_CROUCH: Sensors = Sensors {
    foot_x: 4.9,
    head_x: 2.0,
    head_y: 13.9,
    wall_y0: 4.0,
    wall_y1: 11.0,
    wall_x: 7.9,
};

/// Power-up form (player+0x498: 0 small, 1 Super, 2 Fire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Small,
    Super,
    Fire,
    /// SMB3 Super Leaf: tail attack, slow fall, P-meter flight.
    Raccoon,
    /// SMW Cape Feather: cape spin, glide.
    Cape,
    /// NSMBU Propeller Mushroom: Spin flies up, then a slow fall.
    Propeller,
    /// 3D World Super Bell: climbs walls, scratches, dives.
    Cat,
    /// 3D World Super Hammer (Builder Mario): swings a hammer.
    Builder,
    /// SMB1 Superball Flower (the Fire Flower's 0x4 form in SMB1): throws bouncing superballs.
    Superball,
    /// SMB1 Big Mushroom (part 44 in SMB1): a double-size Mario (32×32 art) who defeats enemies
    /// he touches and breaks bricks he runs into [inf: from the SMM1 item].
    Mega,
    /// SMM1 Weird Mushroom (SMB1 only, `Options::smb1_mushroom`): Lanky ("Slim") Mario — Super
    /// Mario's body and moves with a jump launch 0.5 px/f higher (SMM1 form 3, 0x026c0cc0).
    Lanky,
    /// SMM1 Mystery Mushroom (SMB1 only): a costume (`Player::costume`) on small Mario's body and
    /// physics (SMM1 form 7, size class 1).
    Costume,
    /// SMB1 Master Sword (part 20 with flag 0x4): Link, on small Mario's body (form 10).
    Link,
    /// SMB3 Frog Suit (part 81 in SMB3, form 11).
    Frog,
    /// SMW Power Balloon (part 81 in SMW, form 12): small body, floats.
    Balloon,
    /// NSMBU Super Acorn (part 81 in NSMBU, form 13 "Flying").
    Squirrel,
    /// 3D World Boomerang Flower (part 81 in 3D World, form 14).
    Boomerang,
    /// SMB1 SMB2 Mushroom (part 81 in SMB1, form 15 "USA").
    Smb2,
}

/// Invincibility after taking damage (§7).
pub const INVULN_DAMAGE: i32 = 127;
/// Star power (§5.4).
pub const STAR_FRAMES: i32 = 660;
/// Grow/shrink (DemoChange; its controller's Active state sets 30 frames) [obs]; the world is
/// frozen meanwhile. The shown form alternates in 6-frame steps, see `change_shows_old`.
pub const CHANGE_FRAMES: i32 = 30;

/// During a grow/shrink with `timer` frames left: is the old form shown? Step k = 6 − timer/6
/// (≤ 6); odd steps show the old form, even ones the new [obs].
pub fn change_shows_old(timer: i32) -> bool {
    (6 - timer / 6).clamp(0, 6) % 2 == 1
}
// Death (DownPose 0x71009e13e0 / DownFall 0x71009e1ea0): 45-frame pose, then a hop at 3.2 with
// gravity −0.16; the restart request comes 140 frames into the fall.
pub const DOWN_POSE_FRAMES: i32 = 45;
pub const DOWN_VY: f32 = 3.2;
pub const DOWN_GRAVITY: f32 = -0.16;
pub const DOWN_FALL_FRAMES: i32 = 140;
/// Enemy stomp bounce (0x710072bca0 → jump info): vy 3.9095 whether or not jump is held; the
/// held-jump gravity then gives the higher bounce [obs]. Players stomping players use 4.128 /
/// 3.628 (0x71009ce0e0).
pub const STOMP_BOUNCE: f32 = 3.9095;
/// Lanky Mario's extra jump launch (SMM1 0x026c0cc0, constant 0x100d052c) [obs].
pub const LANKY_JUMP_BONUS: f32 = 0.5;
/// Frames after a lanky jump at which the "float" cue sounds if the player still rises with
/// jump held (SMM1 0x026c1470: timer 20, cue at 1, vy > 1.0) [obs].
pub const LANKY_FLOAT_TIMER: i32 = 20;
/// Player cues for sounds (`Player::cues`): landed, started a skid, began the costume taunt.
pub const CUE_LAND: u8 = 1;
pub const CUE_SKID: u8 = 2;
pub const CUE_APPEAL: u8 = 4;
/// The costume taunt (SMM1 "appeal": hold Up standing; the appeal pose runs 120 frames)
/// [obs pose length; inf the hold count].
pub const APPEAL_FRAMES: i32 = 120;
pub const APPEAL_HOLD: i32 = 8;
/// SMM1 costume ids: 0 … 99 the named costumes, 100 + n the amiibo costumes `Boss<n>`
/// (0 … 98) [obs: the costume enum, Block.rpx 0x1019add0].
pub const COSTUMES: usize = 199;

/// Who is steering the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Normal,
    /// Growing or shrinking; `from` is the old form (drawn alternately).
    Change {
        timer: i32,
        from: Form,
    },
    /// Dead: pose (timer counts the pose), then the fall.
    DownPose {
        timer: i32,
    },
    DownFall {
        timer: i32,
    },
    /// Scripted by the goal demo (World moves the player).
    Demo,
}

/// Game style: one movement model with style-specific moves on top (player-physics.md §2.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    M1,
    M3,
    MW,
    WU,
    W3,
}

impl Style {
    pub fn from_code(code: &str) -> Self {
        match code {
            "M3" => Style::M3,
            "MW" => Style::MW,
            "WU" => Style::WU,
            "3W" => Style::W3,
            _ => Style::M1,
        }
    }
    /// Items can be picked up and carried (disabled in SMB1).
    pub fn can_carry(self) -> bool {
        self != Style::M1
    }
    /// Consecutive (double / triple) jumps and the ground pound (NSMBU, 3D World).
    pub fn has_jump_chain(self) -> bool {
        matches!(self, Style::WU | Style::W3)
    }
    /// Spin jump from the ground (SMW, NSMBU, 3D World).
    pub fn can_spin(self) -> bool {
        matches!(self, Style::MW | Style::WU | Style::W3)
    }
}

fn jump_bonus(speed: f32) -> f32 {
    if speed < 0.7 {
        0.0
    } else if speed < 1.5 {
        0.18
    } else if speed < 2.8 {
        0.24
    } else {
        0.3
    }
}

fn vy_band(vy: f32) -> usize {
    VY_BANDS.iter().position(|&b| vy > b).unwrap_or(5)
}

/// Band for an acceleration table given speed along x, the facing direction and thresholds.
fn band(speed_f: f32, facing: i32, t: &[f32; 4]) -> usize {
    let s = speed_f.abs();
    if speed_f != 0.0 && speed_f.signum() as i32 != facing {
        return if s > t[3] { 5 } else { 0 };
    }
    t.iter().position(|&x| s <= x).map(|i| i + 1).unwrap_or(5)
}

/// Move `v` toward `target` by at most `a` (§1.3 step 4).
pub(crate) fn approach(v: f32, target: f32, a: f32) -> f32 {
    if v < target {
        (v + a).min(target)
    } else {
        (v - a).max(target)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Wait,
    Walk,
    Skid,
    Jump,
    Crouch,
    Dead,
    Pole,
    Fire,
    Swim,
    Climb,
    Slide,
    Spin,
}

/// SMW cape flight: MantFlyingUp (0x47), MantFlying (0x48), MantSlide (0x4a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CapeState {
    None,
    /// The climb: frames left of the 72-frame state counter.
    Rise(i32),
    /// Gliding: the cape's pose stage (0 up2 … 5 down4) steps one at a time toward `target`
    /// every 8 frames (2 after a change made with the stored lift at 3.5); `mode` 0 neutral,
    /// 1 pulled back, 2 diving; `lift` is the stored climb from a dive.
    Fly {
        stage: u8,
        target: u8,
        mode: u8,
        counter: u8,
        quick: bool,
        lift: f32,
    },
    /// Landed from a glide: skidding to a stop without control.
    Slide,
}

/// MantFlying per-stage minimum vy and vy change (0x7101d09cd8 / 0x7101d09cf0) [obs].
const CAPE_MIN_VY: [f32; 6] = [-1.0, -3.0, -3.0, -3.5, -3.5, -4.0];
const CAPE_DVY: [f32; 6] = [-0.06, -0.06, -0.18, -0.24, -0.3, -0.36];
/// Stored lift by fall speed, index clamp(int(−2·vy), 0, 8) (0x7101d09cb4) [obs].
const CAPE_LIFT: [f32; 9] = [0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.75, 1.0, 3.5];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkidPhase {
    None,
    /// Turn phase 0: braking to a stop.
    Braking,
    /// Phase 1: stopped, frames left of the 8-frame hold.
    Pause(i32),
    /// Phase 2: the `turned` animation (frames left) with the 0.25 re-acceleration.
    Turned(i32),
}

#[derive(Clone, Debug)]
pub struct Player {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// +1 right, −1 left. Follows the stick in the air too (player +0x26c) [obs: Ryujinx trace].
    pub facing: i32,
    /// Facing as drawn: SMB1 keeps the take-off facing on screen while airborne (§2.6).
    pub drawn_facing: i32,
    pub on_ground: bool,
    pub pose: Pose,
    /// Distance walked (px), drives the walk animation.
    pub walk_phase: f32,
    /// Slope under the feet while grounded: (steepness index, +1 if the surface rises to the
    /// right / −1 if it falls).
    pub slope: Option<(usize, i32)>,
    pub form: Form,
    pub control: Control,
    pub invuln: i32,
    pub star: i32,
    pub crouch: bool,
    /// Tile the head hit this frame (for block bumps).
    pub head_hit: Option<(i32, i32)>,
    /// Fireball throw requested this frame (World spawns it if the limits allow).
    pub throw: bool,
    pub fire_pose: i32,
    /// Hidden from view (inside the castle at the goal).
    pub hidden: bool,
    /// Stomp chain without touching the ground.
    pub stomp_chain: u32,
    /// Standing on ice (set by the world from the ground under the feet).
    pub on_ice: bool,
    /// Actor id of the moving solid the player stands on.
    pub ride: Option<u32>,
    /// Touched a spike trap this frame while moving into it (the world applies the damage).
    pub spiked: bool,
    /// Below a water surface (set by the world each frame): swim physics.
    pub in_water: bool,
    /// Sky theme at night: gravity × 0.2, max fall × 0.5, jumps × 0.7 (≤ 2.7) (§1.3, §3.1).
    pub low_gravity: bool,
    /// Desert-night wind level w (−1…1): the position moves w px per frame (themes.md §2).
    pub wind: f32,
    /// Moving solid the head bonked this frame (POW blocks react).
    pub head_solid: Option<u32>,
    was_in_water: bool,
    /// Jump pressed in the last 20 frames (a jump out of the water when leaving it).
    exit_window: i32,
    skid: SkidPhase,
    prev_jump: bool,
    prev_run: bool,
    /// Wearing a Goomba's Shoe (Some(stiletto)): spikes and Munchers underfoot are safe, stomps
    /// defeat most enemies; a hit knocks the shoe off instead of costing a form.
    pub shoe: Option<bool>,
    /// The "shoe" is a Dry Bones Shell (ridden with the shoe states, +0x4b0) [obs].
    pub shoe_karon: bool,
    /// Wearing a shell as a helmet: 1 Buzzy, 2 Spiny. It takes one hit [obs 0x71009ee7b0]; a Spiny helmet
    /// also knocks out what the player jumps into from below.
    pub helmet: Option<u8>,
    /// Flying a Koopa Clown Car (its actor id).
    pub clown: Option<u32>,
    /// Driving a Koopa Troopa Car (its actor id); `car_hit`: a hit landed on the car this frame
    /// (the world damages the car instead of the player).
    pub car: Option<u32>,
    pub car_hit: bool,
    pub style: Style,
    /// Carrying an item (its actor id; not in SMB1).
    pub held: Option<u32>,
    /// Sliding down a slope on the backside (Slip; not in SMB1).
    pub sliding: bool,
    /// In a spin jump (until landing).
    pub spinning: bool,
    /// SMB3 P-meter (0…630) and its hold timer; flight window after a full-meter jump.
    pub p_meter: i32,
    p_hold: i32,
    pub flight: i32,
    /// SMW cape flight and the cape run meter (0…30: frames running at ≥ 2.1 px/f); a cape
    /// dive landed this frame (the world shakes as for a POW); the head hit something last
    /// frame.
    pub cape: CapeState,
    pub cape_meter: i32,
    pub cape_quake: bool,
    ceiling: bool,
    /// Raccoon / cape: attack (tail swing, cape spin) frames left; flap and slow-fall frames.
    pub attack: i32,
    pub flap: i32,
    pub slow_fall: i32,
    /// Wall slide: the wall's side (−1 / +1, 0 = not sliding) and frames of stick held away.
    pub wall_slide: i32,
    wall_release: i32,
    /// Jump chain (NSMBU / 3DW): 0 single, 1 second, 2 third jump. A landing from a jump opens
    /// an 8-frame window (`chain_window`) and an 8-frame lock against counting twice; `in_jump`
    /// = airborne from a jump.
    pub jump_count: u8,
    chain_window: i32,
    chain_lock: i32,
    in_jump: bool,
    /// Ground pound: > 0 wind-up frames left, −1 diving; `pound_landed` for the world this frame.
    pub pound: i32,
    pub pound_landed: bool,
    /// Frames Down has been held in the air since it was pressed (no left/right): the input
    /// object turns the 5th into the ground-pound command (input +0x28) [obs: Ryujinx trace].
    pound_hold: u8,
    /// 3D World: frames of continuous running (the dash stage starts at 75, 60 as Cat).
    pub dash: i32,
    /// Cat: wall climb (> 0 frames climbed, −1 sliding down after it), the wall's side, height
    /// climbed; one climb per air time. Dive (ClimbBodyAttack) until landing; the air scratch's
    /// slow rise (frames left) and whether it was used this air time.
    pub cat_climb: i32,
    pub cat_wall: i32,
    cat_climbed: f32,
    cat_climbs: u8,
    cat_reattach: i32,
    pub dive: bool,
    /// Sideways spring pushes: frames left in which another push counts as consecutive (10
    /// after each push), and how many came in a row (to 4) [obs 0x71009f37e0].
    pub side_push: i32,
    pub side_chain: i32,
    /// Stunned by a Sledge Bro's landing quake (QuakeDamage 0x4e: no movement until the
    /// 100-frame counter runs out) [obs].
    pub stun: i32,
    pub roll: i32,
    /// Cat slide attack: frames left of its 44-frame animation.
    pub cat_slide: i32,
    roll_used: bool,
    /// Riding Yoshi (his actor id), the re-mount lockout, and what the world must do with
    /// Yoshi this frame (dismounted / knocked off by a hit).
    pub yoshi: Option<u32>,
    pub yoshi_lock: i32,
    pub yoshi_off: bool,
    pub yoshi_hit: bool,
    /// NSMBU flutter: 1 fluttering, 0 ready, −1 used this jump.
    flutter: i32,
    /// Propeller: rise frames left, falling slowly, used since the last landing.
    pub prop_rise: i32,
    pub prop_fall: bool,
    prop_used: bool,
    prev_down: bool,
    prev_spin: bool,
    /// Touched down in the last frame's collision: this frame still runs the air routine (the
    /// Jump/Fall state's execute sees the ground and only switches to Walk at its end), so the
    /// landing frame keeps the air speed [obs: Ryujinx trace, state +0x3f8 changes with its
    /// counter at 0].
    landed: bool,
    /// SMW Landing state (mode 3): frames the landing holds the speed with no movement routine
    /// (Landing begin 0x71009f2990 / exec 0x71009f2d00) [obs].
    land_hold: u8,
    /// The air routine's last target speed and rate (+0x278 / +0x284): states that skip the
    /// movement routine keep integrating toward them.
    air_move: (f32, f32),
    /// Ducking (Down on the ground; small Mario too, whose pose doesn't change): crouch friction
    /// and no steering. `duck_end`: frames of CrouchEnd (crouch friction kept 2 frames after
    /// Down is let go) [obs: Ryujinx trace, states 5 → 6 → 1].
    duck: bool,
    duck_end: u8,
    /// Airborne from a jump made while ducking (CrouchJump): its landing (CrouchJumpEnd) moves
    /// with crouch friction at once instead of the air routine [obs: Ryujinx trace].
    crouch_jump: bool,
    /// Frames of CrouchJumpEnd left (crouch friction whatever Down does): pixel styles 1,
    /// NSMBU 10, 3D World lands straight in Crouch [obs: Ryujinx traces].
    cje: u8,
    /// A jump press that didn't start a jump is tried once more 3 frames later (then the
    /// player can be on the ground: a press just before or on landing jumps 3 frames on)
    /// [obs: Ryujinx traces of presses around a landing, all styles].
    jump_retry: u8,
    /// This frame's ground move keeps the old facing (bands and no skid): the state change
    /// that led here ran before the facing check [obs: Ryujinx trace, a long jump landing].
    keep_facing: bool,
    /// The slope state of the collision before the last one, and frames of the weightless
    /// hop a slide makes over a slope's top (8, gravity 0) [obs: Ryujinx trace].
    slope_prev: Option<(usize, i32)>,
    slide_float: u8,
    slide_hop: bool,
    /// Walked off a ledge last frame: this frame (Walk handing over to Fall) adds no gravity.
    fell_off: bool,
    /// 3D World long jump (BroadJump, state 14: begin 0x71009d3020, exec 0x71009d3330) and the
    /// frames left of its landing (BroadJumpLand, 0x71009d3470), which stops the player dead.
    broad: bool,
    broad_land: u8,
    /// Climbing a vine: its centre x and the side the player hangs on (−1 left, +1 right).
    pub climbing: Option<(f32, i32)>,
    /// SMM1 costume id while `form` is `Costume` (an index into the imported costume table).
    pub costume: u16,
    /// Lanky Mario: airborne from a lanky jump (drawn running in the air), the float-cue timer,
    /// and whether the cue sounded this frame.
    pub lanky_jump: bool,
    pub lanky_timer: i32,
    pub float_cue: bool,
    /// Costume taunt: frames of the appeal pose left, and how long Up has been held.
    pub appeal: i32,
    up_hold: i32,
    /// Sound cues of this frame (`CUE_*`).
    pub cues: u8,
    /// Frames before a vine can be grabbed again (after jumping off).
    climb_lock: i32,
    /// The update power-ups' states (player/powerups.rs).
    pub pu: PowerUps,
}

impl Player {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            facing: 1,
            drawn_facing: 1,
            on_ground: true,
            pose: Pose::Wait,
            walk_phase: 0.0,
            slope: None,
            form: Form::Small,
            costume: 0,
            lanky_jump: false,
            lanky_timer: 0,
            float_cue: false,
            appeal: 0,
            up_hold: 0,
            cues: 0,
            control: Control::Normal,
            invuln: 0,
            star: 0,
            crouch: false,
            head_hit: None,
            throw: false,
            fire_pose: 0,
            hidden: false,
            stomp_chain: 0,
            on_ice: false,
            ride: None,
            spiked: false,
            in_water: false,
            low_gravity: false,
            wind: 0.0,
            head_solid: None,
            was_in_water: false,
            exit_window: 0,
            skid: SkidPhase::None,
            prev_jump: false,
            prev_run: false,
            climbing: None,
            climb_lock: 0,
            shoe: None,
            shoe_karon: false,
            helmet: None,
            clown: None,
            car: None,
            car_hit: false,
            style: Style::M1,
            held: None,
            sliding: false,
            spinning: false,
            p_meter: 0,
            p_hold: 0,
            flight: 0,
            cape: CapeState::None,
            cape_meter: 0,
            cape_quake: false,
            ceiling: false,
            attack: 0,
            flap: 0,
            slow_fall: 0,
            wall_slide: 0,
            wall_release: 0,
            jump_count: 0,
            chain_window: 0,
            chain_lock: 0,
            in_jump: false,
            pound: 0,
            pound_landed: false,
            pound_hold: 0,
            dash: 0,
            cat_climb: 0,
            cat_wall: 0,
            cat_climbed: 0.0,
            cat_climbs: 0,
            cat_reattach: 0,
            dive: false,
            side_push: 0,
            side_chain: 0,
            stun: 0,
            roll: 0,
            cat_slide: 0,
            roll_used: false,
            yoshi: None,
            yoshi_lock: 0,
            yoshi_off: false,
            yoshi_hit: false,
            flutter: 0,
            prop_rise: 0,
            prop_fall: false,
            prop_used: false,
            prev_down: false,
            prev_spin: false,
            landed: false,
            land_hold: 0,
            air_move: (0.0, 0.0),
            duck: false,
            duck_end: 0,
            crouch_jump: false,
            cje: 0,
            jump_retry: 0,
            keep_facing: false,
            slope_prev: None,
            slide_float: 0,
            slide_hop: false,
            fell_off: false,
            broad: false,
            broad_land: 0,
            pu: PowerUps::default(),
        }
    }

    /// Length of the `turned` animation that ends the Turn state (per-style PlayerAnmTable →
    /// model): the pixel styles 1 frame, NSMBU 24 (`ice_turned` 29), 3D World `Turn` 12.
    fn turned_frames(&self) -> i32 {
        match self.style {
            Style::WU if self.on_ice => 29,
            Style::WU => 24,
            Style::W3 => 12,
            _ => 1,
        }
    }

    fn thresholds(&self) -> &'static [f32; 4] {
        // 3D World keeps the normal row under a star (its run cap is raised instead, run_max)
        if self.star > 0 && self.style != Style::W3 {
            &T_STAR
        } else {
            &T
        }
    }

    /// Run target speed: T3, except 3D World's two-stage run (2.25 until the dash stage, then
    /// 3.0; 3.6 with a star) (§2.2).
    fn run_max(&self) -> f32 {
        if self.style != Style::W3 {
            return self.thresholds()[3];
        }
        if self.star > 0 {
            3.6
        } else if self.dashing() {
            3.0
        } else {
            2.25
        }
    }

    /// 3D World: the run has reached the dash stage (75 frames, 60 as Cat).
    /// 3D World run timer (0x7100a186a0, after the state's move, before integration) [obs]:
    /// counts frames on the ground with a direction and Run held while already moving forward
    /// (the speed before this frame's change), up to 75 (60 as Cat); anything else resets it,
    /// except that a full timer survives the air. With a star it jumps to full after 45.
    fn step_dash(&mut self, inp: Input, vx_before: f32, grounded: bool) {
        if self.style != Style::W3 {
            self.dash = 0;
            return;
        }
        let full = if self.form == Form::Cat { 60 } else { 75 };
        if !grounded {
            if self.dash < full {
                self.dash = 0;
            }
            return;
        }
        let forward = vx_before * self.facing as f32 > 0.0;
        if inp.dir_x() == 0 || !inp.held(input::RUN) || !forward {
            self.dash = 0;
        } else if self.dash < full {
            self.dash += 1;
            if self.star > 0 && self.dash > 45 {
                self.dash = full;
            }
        }
    }

    pub fn dashing(&self) -> bool {
        let need = if self.form == Form::Cat { 60 } else { 75 };
        self.style == Style::W3 && self.dash >= need
    }

    pub fn big(&self) -> bool {
        // Link and the Power Balloon have the small body (size class 1) [obs 0x71023d5e0c]
        !matches!(
            self.form,
            Form::Small | Form::Costume | Form::Link | Form::Balloon
        )
    }

    pub fn sensors(&self) -> &'static Sensors {
        if let Some(s) = self.pu_sensors() {
            return s;
        }
        if self.form == Form::Mega {
            return &MEGA;
        }
        match (self.big(), self.crouch) {
            (false, _) => &SMALL_M1,
            (true, false) => &SUPER,
            (true, true) => &SUPER_CROUCH,
        }
    }

    /// Body box A (§6.2), the hurt box: (x0, y0, x1, y1).
    pub fn hurt_box(&self) -> (f32, f32, f32, f32) {
        if self.form == Form::Mega {
            // body shape 4 (§6.2)
            return (self.x - 12.0, self.y, self.x + 12.0, self.y + 24.0);
        }
        let (hw, h) = match (self.big(), self.crouch) {
            (false, false) => (4.0, 14.0),
            (false, true) => (4.0, 7.0),
            (true, false) => (6.0, 24.0),
            (true, true) => (6.0, 14.0),
        };
        (self.x - hw, self.y, self.x + hw, self.y + h)
    }

    /// Body box B (§6.2), the wider contact box used for items and coins.
    pub fn contact_box(&self) -> (f32, f32, f32, f32) {
        if self.form == Form::Balloon {
            // the balloon's box B: centre (0, 8), half 8 × 8 [obs]
            return (self.x - 8.0, self.y, self.x + 8.0, self.y + 16.0);
        }
        if self.form == Form::Mega {
            return (self.x - 12.0, self.y, self.x + 12.0, self.y + 28.0);
        }
        let (hw, h) = match (self.big(), self.crouch) {
            (false, false) => (6.0, 18.0),
            (false, true) => (6.0, 10.0),
            (true, false) => (6.0, 28.0),
            (true, true) => (6.0, 16.0),
        };
        (self.x - hw, self.y, self.x + hw, self.y + h)
    }

    pub fn alive(&self) -> bool {
        !matches!(
            self.control,
            Control::DownPose { .. } | Control::DownFall { .. }
        )
    }

    /// Take a hit. Fire → Super, Super → Small (0x71009d45c0), Small → death. Ignored while
    /// invincible or starred. Returns true if it had an effect.
    pub fn hurt(&mut self) -> bool {
        if self.invuln > 0 || self.star > 0 || !matches!(self.control, Control::Normal) {
            return false;
        }
        if self.car.is_some() {
            // the car takes the hit (KoopaCar Damage 0x71006cec20) [obs]
            self.car_hit = true;
            self.invuln = INVULN_DAMAGE;
            return true;
        }
        // the damage entry (0x71009d41d0) [obs]: gliding with the cape (SMW) the hit only
        // spins the player out of the flight; otherwise Yoshi (0x7100a1fff0), then the shoe
        // (0x71009fc4c0), then the helmet (0x71009ee7b0: it flies off as a shell, 127 frames
        // of invincibility) take the hit before the form does
        if self.style == Style::MW && matches!(self.cape, CapeState::Fly { .. }) {
            self.cape = CapeState::None;
            self.spinning = true;
            self.invuln = INVULN_DAMAGE;
            return true;
        }
        if self.yoshi.take().is_some() {
            // yoshi.md §6: no power-down; the rider pops up (vy 4.628) and Yoshi runs away
            self.yoshi_off = true;
            self.yoshi_hit = true;
            self.yoshi_lock = 10;
            self.invuln = INVULN_DAMAGE;
            self.vy = 4.628;
            self.vx = 0.0;
            self.on_ground = false;
            return true;
        }
        if self.shoe.take().is_some() {
            // a hit in the shoe only knocks the shoe off
            self.shoe_karon = false;
            self.invuln = INVULN_DAMAGE;
            return true;
        }
        if self.helmet.take().is_some() {
            self.invuln = INVULN_DAMAGE;
            return true;
        }
        if self.pu_absorb() {
            return true;
        }
        match self.form {
            Form::Small => self.kill(),
            f => {
                self.control = Control::Change {
                    timer: CHANGE_FRAMES,
                    from: f,
                };
                // SMM1 (0x026b04e8): Lanky and costumes drop to small, like Super
                self.form = if matches!(f, Form::Super | Form::Lanky | Form::Costume) {
                    Form::Small
                } else {
                    Form::Super
                };
                self.crouch = false;
                self.invuln = INVULN_DAMAGE;
            }
        }
        true
    }

    /// Lose a life (pits, time up, damage as small).
    pub fn kill(&mut self) {
        if !self.alive() {
            return;
        }
        self.control = Control::DownPose {
            timer: DOWN_POSE_FRAMES,
        };
        self.vx = 0.0;
        self.vy = 0.0;
        self.star = 0;
        self.pose = Pose::Dead;
    }

    /// Collect a power-up (20-super-mushroom.md: form changes only upward).
    pub fn power_up(&mut self, to: Form) -> bool {
        // a mushroom only grows a small player; other power-ups replace the current form; a
        // Weird Mushroom only takes a small or Super player (SMM1 0x026b34a4)
        let up = to != self.form
            && (to != Form::Super || self.form == Form::Small)
            && (to != Form::Lanky || matches!(self.form, Form::Small | Form::Super));
        if up && self.control == Control::Normal {
            // small + Fire Flower goes straight to Fire (the block swap to a mushroom happens at
            // spawn time)
            self.control = Control::Change {
                timer: CHANGE_FRAMES,
                from: self.form,
            };
            self.form = to;
        }
        up
    }

    /// The costume taunt: Up held standing still on the ground for `APPEAL_HOLD` frames starts
    /// the 120-frame appeal pose; moving, jumping or leaving the ground ends it.
    fn step_appeal(&mut self, inp: Input) {
        let idle = self.on_ground
            && inp.dir_x() == 0
            && !inp.held(input::JUMP)
            && !self.crouch
            && self.vx.abs() < 0.01;
        if !idle {
            self.appeal = 0;
            self.up_hold = 0;
            return;
        }
        if self.appeal > 0 {
            self.appeal -= 1;
            return;
        }
        if inp.held(input::UP) {
            self.up_hold += 1;
            if self.up_hold == APPEAL_HOLD {
                self.appeal = APPEAL_FRAMES;
                self.cues |= CUE_APPEAL;
            }
        } else {
            self.up_hold = 0;
        }
    }

    /// Take a Mystery Mushroom: costume `id` (SMM1 0x026b34a4: refused only for the costume
    /// already worn). Returns true if the form changed.
    pub fn put_on_costume(&mut self, id: u16) -> bool {
        if self.form == Form::Costume && self.costume == id {
            return false;
        }
        if self.control != Control::Normal {
            return false;
        }
        self.control = Control::Change {
            timer: CHANGE_FRAMES,
            from: self.form,
        };
        self.form = Form::Costume;
        self.costume = id;
        self.crouch = false;
        true
    }

    /// Bounce off an enemy.
    pub fn bounce(&mut self, jump_held: bool) {
        let _ = jump_held; // the held-jump gravity makes the difference
        if self.pu_bounce(STOMP_BOUNCE) {
            return;
        }
        self.vy = STOMP_BOUNCE;
        self.on_ground = false;
        self.slope = None;
    }

    /// Advance the non-physics controls; returns true if normal physics should run.
    fn step_control(&mut self) -> bool {
        match self.control {
            Control::Normal => true,
            Control::Demo => false,
            Control::Change { timer, .. } => {
                self.control = if timer <= 1 {
                    Control::Normal
                } else {
                    Control::Change {
                        timer: timer - 1,
                        from: match self.control {
                            Control::Change { from, .. } => from,
                            _ => self.form,
                        },
                    }
                };
                false
            }
            Control::DownPose { timer } => {
                self.control = if timer <= 1 {
                    self.vy = DOWN_VY;
                    Control::DownFall {
                        timer: DOWN_FALL_FRAMES,
                    }
                } else {
                    Control::DownPose { timer: timer - 1 }
                };
                false
            }
            Control::DownFall { timer } => {
                self.vy = (self.vy + DOWN_GRAVITY).max(FALL_CAP);
                self.y += self.vy;
                if timer > 0 {
                    self.control = Control::DownFall { timer: timer - 1 };
                }
                false
            }
        }
    }

    /// The death sequence has finished: the course should restart.
    pub fn restart_due(&self) -> bool {
        matches!(self.control, Control::DownFall { timer: 0 })
    }

    fn ground(&mut self, inp: Input) {
        let dir = inp.dir_x();
        let run = inp.held(input::RUN);
        if self.cat_slide > 0 {
            // cat slide attack (ClimbSlidingAttack 0x7100a1d030 / 0x7100a1d150) [obs]: 2.5 px/f
            // forward with no accel for 12 frames, then down to 0 at 0.075 (ice 0.0375) until
            // the 44-frame animation ends; then it stops dead unless a direction is held (on
            // ice it halves instead). No steering meanwhile.
            let elapsed = CAT_SLIDE_FRAMES - self.cat_slide;
            if elapsed < 12 {
                self.vx = 2.5 * self.facing as f32;
            } else {
                let decel = if self.on_ice { 0.0375 } else { 0.075 };
                self.vx = approach(self.vx, 0.0, decel);
            }
            self.cat_slide -= 1;
            if self.cat_slide == 0 {
                if self.on_ice {
                    self.vx *= 0.5;
                } else if dir == 0 {
                    self.vx = 0.0;
                }
            }
            self.pose = Pose::Slide;
            return;
        }
        let t = self.thresholds();
        let star = self.star > 0;
        let (walk_max, run_max) = (t[1], self.run_max());

        // skid = the Turn state (entry 0x7100a197a0, exec 0x7100a19b40) [obs]: on the ground
        // with the stick against the facing and |v| ≥ 2.5 (3D World: ≥ 2.0, never on ice).
        // The player turns to face the stick, brakes to a stop (0.12; ice 0.028, ice + star
        // 0.038), holds still for 8 frames, then re-accelerates at 0.25 toward the normal
        // ground target until the `turned` animation ends. Letting go of the direction (or
        // pressing back) ends it at once.
        let skid_from = if self.style == Style::W3 {
            2.0
        } else {
            SKID_ENTER
        };
        if self.skid == SkidPhase::None
            && !self.keep_facing
            && dir != 0
            && dir != self.facing
            && self.vx.abs() >= skid_from
            && !(self.style == Style::W3 && self.on_ice)
        {
            self.skid = SkidPhase::Braking;
            self.cues |= CUE_SKID;
            self.facing = dir;
        }
        // letting go (or pressing back) ends the Turn state, but only after this frame's move:
        // the exec sets the phase's target and rate before its stick check (0x7100a19b40) [obs]
        let leaving = self.skid != SkidPhase::None && dir != self.facing;
        match self.skid {
            SkidPhase::Braking | SkidPhase::Pause(_) => {
                let decel = match (self.on_ice, star) {
                    (false, _) => SKID_DECEL,
                    (true, false) => SKID_DECEL_ICE,
                    (true, true) => SKID_DECEL_ICE_STAR,
                };
                // (the stop is seen the frame after the speed reaches 0: the state reads the
                // speed before this frame's integration)
                let stopped = self.vx.abs() <= f32::EPSILON;
                self.vx = approach(self.vx, 0.0, decel);
                self.pose = Pose::Skid;
                self.skid = match self.skid {
                    _ if leaving => SkidPhase::None,
                    SkidPhase::Braking if stopped => SkidPhase::Pause(SKID_PAUSE),
                    // the hold counter ticks before the state runs: 7 frames waiting, then
                    // the `turned` animation starts on the 8th
                    SkidPhase::Pause(n) if n > 1 => SkidPhase::Pause(n - 1),
                    SkidPhase::Pause(_) => SkidPhase::Turned(self.turned_frames()),
                    other => other,
                };
                return;
            }
            SkidPhase::Turned(n) => {
                let target = dir as f32 * if run { run_max } else { walk_max };
                self.vx = approach(self.vx, target, SKID_REACCEL);
                self.skid = if n > 1 && !leaving {
                    SkidPhase::Turned(n - 1)
                } else {
                    SkidPhase::None
                };
                self.pose = Pose::Walk;
                return;
            }
            SkidPhase::None => {}
        }

        if let Some((steep, rises)) = self.slope {
            // §5.2: rows chosen by whether the motion is downhill or uphill
            let moving = if dir != 0 {
                dir
            } else {
                self.vx.signum() as i32
            };
            let downhill = moving != 0 && moving != rises;
            let (mult, acc, dec) = if downhill {
                (
                    SLOPE_TARGET_DOWN[steep],
                    SLOPE_ACCEL_DOWN[steep],
                    SLOPE_DECEL_DOWN[steep],
                )
            } else {
                (
                    SLOPE_TARGET_UP[steep],
                    SLOPE_ACCEL_UP[steep],
                    SLOPE_DECEL_UP[steep],
                )
            };
            if dir == 0 {
                self.vx = approach(self.vx, 0.0, dec);
            } else {
                self.facing = dir;
                let target = dir as f32 * if run { run_max } else { walk_max } * mult;
                let reversing = self.vx != 0.0 && self.vx.signum() as i32 != dir;
                let a = if reversing {
                    // reversing: max(1.1 × the other row's no-input value, the input accel)
                    let other = if downhill {
                        SLOPE_DECEL_UP[steep]
                    } else {
                        SLOPE_DECEL_DOWN[steep]
                    };
                    acc.max(1.1 * other)
                } else {
                    acc
                };
                self.vx = approach(self.vx, target, a);
            }
            self.pose = if self.vx == 0.0 && dir == 0 {
                Pose::Wait
            } else {
                Pose::Walk
            };
            return;
        }

        let ice = self.on_ice;
        let (target, table) = if dir == 0 {
            let table = match (self.crouch || self.duck || self.duck_end > 0, ice) {
                (false, false) => &GROUND_IDLE,
                (true, false) => &GROUND_CROUCH,
                (false, true) => &ICE_IDLE,
                (true, true) => &ICE_CROUCH,
            };
            (0.0, table)
        } else {
            if !self.keep_facing {
                self.facing = dir;
            }
            let table = match (run, star, ice) {
                (false, false, false) => &GROUND_WALK,
                (false, true, false) => &GROUND_WALK_STAR,
                (true, false, false) => &GROUND_RUN,
                (true, true, false) => &GROUND_RUN_STAR,
                (false, false, true) => &ICE_WALK,
                (false, true, true) => &ICE_WALK_STAR,
                (true, false, true) => &ICE_RUN,
                (true, true, true) => &ICE_RUN_STAR,
            };
            (dir as f32 * if run { run_max } else { walk_max }, table)
        };
        self.vx = approach(self.vx, target, table[band(self.vx, self.facing, t)]);
        self.pose = if self.vx == 0.0 && dir == 0 {
            Pose::Wait
        } else {
            Pose::Walk
        };
    }

    fn air(&mut self, inp: Input) {
        let dir = inp.dir_x();
        if dir != 0 {
            self.facing = dir;
        }
        if dir == 0 {
            // no direction: accel 0 → momentum preserved
            self.air_move = (0.0, 0.0);
        } else {
            let run = inp.held(input::RUN);
            let t = self.thresholds();
            let star = self.star > 0;
            let s = self.vx.abs();
            let target = if run {
                self.run_max().max(s.min(t[3]))
            } else if s > t[1] && self.vx.signum() as i32 == dir {
                s
            } else {
                t[1]
            } * dir as f32;
            // SMB1 keeps the take-off facing in the air (§2.6), except a costume (SMM1 action
            // table: it turns in the air); bands use the movement facing
            if self.form == Form::Costume {
                self.facing = dir;
            }
            let table = match (run, star) {
                (false, false) => &AIR_DIR,
                (true, false) => &AIR_DIR_RUN,
                (false, true) => &AIR_DIR_STAR,
                (true, true) => &AIR_DIR_RUN_STAR,
            };
            let face = if self.vx == 0.0 {
                dir
            } else {
                self.vx.signum() as i32
            };
            let b = if dir != face {
                0
            } else {
                band(self.vx, face, t)
            };
            self.vx = approach(self.vx, target, table[b]);
            self.air_move = (target, table[b]);
        }
        let cap = self.thresholds()[3].max(self.run_max());
        self.vx = self.vx.clamp(-cap, cap); // hard air cap
    }

    /// One 60 Hz tick.
    pub fn step(&mut self, inp: Input, level: &Level) {
        let inp = if self.stun > 0 {
            self.stun -= 1;
            Input::default()
        } else {
            inp
        };
        self.head_hit = None;
        self.cues = 0;
        let was_ground = self.on_ground;
        self.head_solid = None;
        self.throw = false;
        if !self.step_control() {
            return;
        }
        if self.invuln > 0 {
            self.invuln -= 1;
        }
        if self.star > 0 {
            self.star -= 1;
        }
        if self.fire_pose > 0 {
            self.fire_pose -= 1;
        }
        let jump = inp.held(input::JUMP);
        let pressed = jump && !self.prev_jump;
        self.prev_jump = jump;
        let retry = self.jump_retry == 1;
        self.jump_retry = self.jump_retry.saturating_sub(1);
        // the frame after touchdown still belongs to the airborne state: no jump yet
        let jump_now = (pressed || retry) && !self.landed;
        let mut jumped = false;
        let spin = inp.held(input::SPIN);
        let mut spin_pressed = spin && !self.prev_spin;
        self.prev_spin = spin;
        if self.yoshi_lock > 0 {
            self.yoshi_lock -= 1;
        }
        if self.yoshi.is_some() && spin_pressed {
            // dismount (yoshi.md §3): SMW — a spin jump off him on the ground (vy 3.228, speed
            // −1·facing), a normal jump in the air; NSMBU — a back-hop (vy 2.85, −0.8·facing,
            // 30-frame lockout) on the ground, vy 3.728 in the air
            spin_pressed = false;
            self.yoshi = None;
            self.yoshi_off = true;
            self.y += 8.0;
            let wu = self.style == Style::WU;
            self.yoshi_lock = if wu && self.on_ground { 30 } else { 10 };
            match (wu, self.on_ground) {
                (true, true) => {
                    self.vy = 2.85;
                    self.vx = -0.8 * self.facing as f32;
                }
                (true, false) => self.vy = 3.728,
                (false, true) => {
                    self.vy = 3.228;
                    self.vx = -(self.facing as f32);
                    self.spinning = true;
                }
                (false, false) => self.vy = JUMP_BASE,
            }
            self.on_ground = false;
        }
        let run = inp.held(input::RUN);
        if run
            && !self.prev_run
            && matches!(self.form, Form::Fire | Form::Superball)
            && !self.crouch
        {
            self.throw = true;
        }
        // raccoon tail swing / cape spin / cat scratch on Run (player-physics.md §5.4); the
        // cape spin and the hammer last 16 frames [inf]
        if run
            && !self.prev_run
            && matches!(
                self.form,
                Form::Raccoon | Form::Cape | Form::Cat | Form::Builder
            )
            && self.held.is_none()
            && !self.dive
            && self.cat_climb == 0
            // a wearable turns the cat's moves off (form 7 with +0x4bc == −1 tests) [obs]
            && !(self.form == Form::Cat && self.pu.headdress.is_some())
        {
            // the raccoon's tail_attack animation is 18 frames (TailAttack 0x7100a034e0 ends
            // with it); the cat's scratch is ClimbAttack (0x7100a1c170 / 0x7100a1c230), which
            // ends with its 17-frame animation and leaves normal movement alone [obs]
            self.attack = match self.form {
                Form::Raccoon => 18,
                Form::Cat => CAT_SCRATCH_FRAMES,
                _ => 16,
            };
            if self.form == Form::Cat {
                if self.on_ground && self.crouch {
                    // crouching, the attack is the slide (ClimbSlidingAttack, entered from the
                    // crouch check 0x7100a1b3f0) [obs]
                    self.attack = 0;
                    self.cat_slide = CAT_SLIDE_FRAMES;
                    self.crouch = false;
                    self.vx = 2.5 * self.facing as f32;
                } else if !self.on_ground && !self.roll_used && !self.in_water {
                    // rolling attack: a small rise (vy 2.4, gravity −0.1) once per air time
                    self.vy = 2.4;
                    self.roll = 24;
                    self.roll_used = true;
                }
            }
        }
        if self.attack > 0 {
            self.attack -= 1;
        }
        self.step_meters(inp, pressed, jump);
        self.prev_run = run;
        let Some(inp) = self.pu_before(inp, pressed, spin_pressed, level) else {
            return;
        };

        // vines (64-vine.md)
        if self.climb_lock > 0 {
            self.climb_lock -= 1;
        }
        if (self.climbing.is_some() || self.try_grab(inp, level))
            && self.climb(inp, pressed, run, level)
        {
            return;
        }

        // slope slide (player-physics.md §5.2 Slip; not in SMB1): crouch on a slope
        if self.style != Style::M1 && self.on_ground && !self.in_water && !self.sliding {
            let on_slope = level
                .slope_at(self.x)
                .is_some_and(|(sy, _)| (self.y - sy).abs() < 6.0);
            // SMB3 slips at once; the other styles crouch for a frame first [obs: Ryujinx
            // traces, states 5 → 21]
            let ready = self.style == Style::M3 || self.duck || self.crouch;
            if on_slope && inp.held(input::DOWN) && self.held.is_none() && ready {
                self.sliding = true;
                self.crouch = false;
            }
        }
        if self.on_ground {
            self.slide_hop = false;
        }
        if self.sliding && (!self.on_ground || self.in_water) && self.vy > 0.0 && !self.slide_hop {
            self.sliding = false;
        }

        // crouch: big forms on the ground with Down held (kept through a crouch jump)
        if self.on_ground {
            // (a costume can crouch though small, SMM1 action table)
            let want = (self.big() || self.form == Form::Costume)
                && inp.held(input::DOWN)
                && !self.sliding
                && self.cat_slide == 0;
            if want != self.crouch && (want || self.can_stand(level)) {
                self.crouch = want;
            }
        }
        if self.on_ground {
            let want = inp.held(input::DOWN)
                && !self.sliding
                && self.cat_slide == 0
                && self.held.is_none()
                && (self.crouch || !self.big());
            // CrouchEnd runs its crouch-friction move until its animation ends, plus the
            // frame it hands over to Walk: pixel styles 1 + 1, NSMBU 2 + 1, 3D World 15 + 1
            // [obs: Ryujinx traces]
            let crouch_end = match self.style {
                Style::WU => 3,
                Style::W3 => 16,
                _ => 2,
            };
            if self.cje > 0 {
                self.cje -= 1;
                let ending = self.cje == 0 && !want;
                if ending {
                    // this frame, then (except 3D World) a Crouch frame, then CrouchEnd
                    self.duck_end = crouch_end + if self.style == Style::W3 { 1 } else { 2 };
                }
                self.duck = !ending;
            } else {
                if self.duck && !want {
                    self.duck_end = crouch_end;
                } else if want {
                    self.duck_end = 0;
                }
                self.duck = want;
            }
        } else {
            self.duck = false;
            self.duck_end = 0;
        }
        let inp = if (self.crouch || self.duck || self.duck_end > 0) && self.on_ground {
            Input {
                buttons: inp.buttons & !(input::LEFT | input::RIGHT),
            }
        } else {
            inp
        };

        if self.exit_window > 0 {
            self.exit_window -= 1;
        }
        let vy_before = self.vy;
        let (vx_before, face_before) = (self.vx, self.facing);
        let grounded_before = self.on_ground;
        if self.in_water {
            self.swim(inp, pressed);
        } else {
            if self.was_in_water && self.exit_window > 0 && self.vy > 0.0 {
                // leaving the water within 20 frames of a stroke: jump out (vy 3.5)
                self.vy = 3.5;
            }
            if self.on_ground {
                // a jump leaves Walk at the start of the frame: the Jump state's execute (air
                // routine) moves the player on the take-off frame [obs: Ryujinx trace]
                let walking = !(self.sliding || self.cape == CapeState::Slide || self.landed);
                let before = (self.vx, self.facing);
                let crest = match self.slope_prev {
                    Some((steep, rises)) if self.slope.is_none() => {
                        (self.vx * rises as f32 > 0.0).then_some(steep)
                    }
                    _ => None,
                };
                if let (true, Some(steep)) = (self.sliding, crest) {
                    // sliding up over a slope's top: the speed along the slope splits into x
                    // and y and the slide goes on through a weightless 8-frame hop
                    // [obs: Ryujinx trace, SMB3 / SMW / NSMBU]
                    let (c, s) = if steep >= 3 {
                        (
                            std::f32::consts::FRAC_1_SQRT_2,
                            std::f32::consts::FRAC_1_SQRT_2,
                        )
                    } else {
                        (0.894_400_9, 0.447_200_7)
                    };
                    self.vy = self.vx.abs() * s;
                    self.vx *= c;
                    self.on_ground = false;
                    self.slide_float = 8;
                    self.slide_hop = true;
                } else if self.sliding {
                    self.slide(level);
                } else if self.cape == CapeState::Slide {
                    self.cape_slide(level);
                } else if self.landed && self.broad {
                    // with a direction held the long jump lands straight into Walk (which
                    // moves this frame); without, BroadJumpLand stops the player dead
                    self.broad = false;
                    if inp.dir_x() != 0 {
                        self.keep_facing = true;
                        self.ground(inp);
                        self.keep_facing = false;
                    } else {
                        self.vx = 0.0;
                        self.broad_land = BROAD_LAND_FRAMES;
                    }
                } else if self.broad_land > 0 {
                    self.broad_land -= 1;
                    self.vx = 0.0;
                } else if self.landed && self.crouch_jump {
                    self.crouch_jump = false;
                    self.ground(inp);
                } else if self.landed {
                    // the airborne state's last frame (see `landed`), then the Landing state's
                    // begin: no direction held and |speed| ≤ 0.3 stops dead; SMW then holds
                    // the speed for 4 more frames, the other styles go on walking
                    let dir = inp.dir_x();
                    self.air(inp);
                    if dir == 0 && self.vx.abs() <= 0.3 {
                        self.vx = 0.0;
                    } else if self.style == Style::MW {
                        self.land_hold = 4;
                    }
                } else if self.land_hold > 0 {
                    // no movement routine: the speed keeps moving toward the air routine's
                    // last target at its last rate [obs: Ryujinx trace, SMW]
                    self.land_hold -= 1;
                    self.vx = approach(self.vx, self.air_move.0, self.air_move.1);
                } else {
                    self.ground(inp);
                }
                self.duck_end = self.duck_end.saturating_sub(1);
                if spin_pressed && self.form == Form::Propeller {
                    // the propeller launch replaces the spin jump
                } else if self.broad_land > 0 {
                    // no actions until the long-jump landing ends
                } else if spin_pressed && self.style == Style::W3 && self.held.is_none() {
                    // 3D World: the spin button is the long jump, and only when already moving
                    // faster than 0.5 (0x7100a0b2b0): 2.0 forward (3.0 above 1.5), vy 2.4
                    // [obs: binary; Ryujinx trace]
                    if self.vx.abs() > 0.5 {
                        let f = self.facing as f32;
                        self.vx = f * if self.vx.abs() <= 1.5 { 2.0 } else { 3.0 };
                        self.vy = BROAD_VY;
                        self.broad = true;
                        self.spinning = false;
                        self.sliding = false;
                        self.crouch = false;
                        self.on_ground = false;
                        self.skid = SkidPhase::None;
                    }
                } else if spin_pressed && self.style.can_spin() && self.held.is_none() {
                    if walking {
                        (self.vx, self.facing) = before;
                        self.air(inp);
                    }
                    // spin jump (player-physics.md §2.6): vy 3.228, 4.5 in 3D World
                    self.vy = if self.style == Style::W3 { 4.5 } else { 3.228 };
                    self.spinning = true;
                    self.sliding = false;
                    self.crouch = false;
                    self.on_ground = false;
                    self.skid = SkidPhase::None;
                } else if jump_now {
                    jumped = true;
                    if walking {
                        (self.vx, self.facing) = before;
                        self.air(inp);
                    }
                    self.sliding = false;
                    // jump chain (Jump enter 0x71009f0d90, exit 0x71009f1f40) [obs]: the count
                    // (advanced on landing) survives only inside the 8-frame window and at
                    // |speed| ≥ 3.0 (3D World: any speed); the third jump is ×1.05
                    let threshold = if self.style == Style::W3 {
                        f32::EPSILON
                    } else {
                        3.0
                    };
                    if !(self.style.has_jump_chain()
                        && self.chain_window > 0
                        && self.vx.abs() >= threshold)
                    {
                        self.jump_count = 0;
                    }
                    self.in_jump = true;
                    self.crouch_jump = self.duck || self.crouch;
                    self.vy = JUMP_BASE + jump_bonus(self.vx.abs());
                    self.vy = self.pu_launch(self.vy);
                    if self.jump_count == 2 || self.dashing() {
                        self.vy *= 1.05;
                    }
                    if self.form == Form::Lanky {
                        self.vy += LANKY_JUMP_BONUS;
                        self.lanky_jump = true;
                        self.lanky_timer = LANKY_FLOAT_TIMER;
                    }
                    if self.low_gravity {
                        self.vy = (self.vy * 0.7).min(2.7);
                    }
                    self.on_ground = false;
                    self.skid = SkidPhase::None;
                }
            } else {
                // leaving the ground ends the Turn state and the cat slide
                self.cat_slide = 0;
                self.skid = SkidPhase::None;
                if self.broad {
                    // the long jump keeps its speed; pressing back eases it toward half speed
                    // the other way (target facing × 0.5, rate 0.07)
                    let f = self.facing as f32;
                    let target = if inp.dir_x() == -self.facing {
                        f * 0.5
                    } else {
                        self.vx
                    };
                    self.vx = approach(self.vx, target, 0.07);
                } else {
                    self.air(inp);
                }
            }

            // gravity (§3.2): ground uses the fixed −0.34; airborne picks a band each frame
            if self.on_ground {
                // the game holds vy at −0.5 on the ground (the collision puts the feet back);
                // it shows when walking off a ledge: the first fall frame has −0.84
                // [obs: Ryujinx trace]
                self.vy = GROUNDED_VY;
            }
            let g = if self.on_ground {
                GRAVITY_GROUND
            } else if std::mem::take(&mut self.fell_off) {
                // Walk's last frame (it hands over to Fall): no gravity [obs: Ryujinx trace]
                0.0
            } else if self.slide_float > 0 {
                self.slide_float -= 1;
                0.0
            } else if self.broad {
                BROAD_GRAVITY
            } else if jump || spin {
                GRAVITY_HELD[vy_band(self.vy)]
            } else {
                GRAVITY_RELEASED[vy_band(self.vy)]
            };
            if self.low_gravity && !self.on_ground {
                self.vy = (self.vy + 0.2 * g).max(0.5 * FALL_CAP);
            } else {
                self.vy = (self.vy + g).max(FALL_CAP);
            }
            self.step_cape(inp, jump, vy_before, vx_before, face_before);
        }
        if pressed && !jumped {
            self.jump_retry = 3;
        }
        if self.chain_window > 0 {
            self.chain_window -= 1;
        }
        if self.side_push > 0 {
            self.side_push -= 1;
        }
        if self.chain_lock > 0 {
            self.chain_lock -= 1;
        }
        self.step_dash(inp, vx_before, grounded_before);
        self.step_wall_and_pound(inp, pressed, level, vy_before);
        self.step_propeller(inp, spin_pressed, vy_before);
        // NSMBU flutter (yoshi.md §4): falling faster than 2.0 with jump held, Yoshi flutters —
        // gravity turns upward (+0.2, +0.07 above −1.0) until vy > 2.4 or jump is released;
        // once per jump
        if self.yoshi.is_some() && self.style == Style::WU {
            if self.on_ground {
                self.flutter = 0;
            } else if self.flutter == 0 && jump && self.vy < -2.0 {
                self.flutter = 1;
            }
            if self.flutter == 1 {
                if !jump || vy_before > 2.4 {
                    self.flutter = -1;
                } else {
                    self.vy = vy_before + if vy_before <= -1.0 { 0.2 } else { 0.07 };
                }
            }
        }
        // raccoon flaps (vy 1.5, no gravity, 18 frames) and slow fall (≥ −1.0, 15 frames); the
        // cape glides (≥ −1.0) while jump is held
        if !self.on_ground && !self.in_water {
            if self.flap > 0 {
                self.flap -= 1;
                self.vy = 1.5;
            } else if self.slow_fall > 0 {
                self.slow_fall -= 1;
                self.vy = self.vy.max(-1.0);
            } else if self.form == Form::Cape
                && self.cape == CapeState::None
                && jump
                && self.vy < 0.0
            {
                self.vy = self.vy.max(-1.0);
            }
        } else {
            self.flap = 0;
            self.slow_fall = 0;
        }
        if self.form == Form::Builder && self.attack > 0 && !self.on_ground {
            // HammerAttack in the air: falls no faster than 1.5, |speed| ≤ 1.125
            self.vy = self.vy.max(-1.5);
            self.vx = self.vx.clamp(-1.125, 1.125);
        }
        if self.roll > 0 {
            self.roll -= 1;
            if !self.on_ground && self.cat_climb == 0 && !self.dive {
                self.vy = vy_before - 0.1;
            }
        }
        self.pu_after(inp, jump, vy_before, vx_before, level);
        self.was_in_water = self.in_water;
        self.x += self.wind;

        let sensors = self.sensors();
        let airborne = !self.on_ground;
        self.move_and_collide(level, sensors);
        if self.on_ground && !was_ground {
            self.cues |= CUE_LAND;
        }
        if self.form == Form::Costume {
            self.step_appeal(inp);
        } else {
            self.appeal = 0;
            self.up_hold = 0;
        }
        self.landed = airborne && self.on_ground;
        self.fell_off = !airborne && !self.on_ground && self.vy < 0.0;
        if self.landed && self.crouch_jump {
            self.cje = match self.style {
                Style::WU => 10,
                _ => 1,
            };
        }
        if self.in_water {
            self.broad = false;
        }
        if !self.on_ground {
            self.land_hold = 0;
        }
        self.ceiling = self.head_hit.is_some() || self.head_solid.is_some();
        // the cape lands: a dive (stage 5) shakes the ground (0x71009f5f50), any other glide
        // slides to a stop (MantSlide); the climb just ends
        self.cape_quake = false;
        if self.on_ground {
            self.cape = match self.cape {
                CapeState::Fly { stage: 5, .. } => {
                    self.cape_quake = true;
                    CapeState::None
                }
                CapeState::Fly { .. } => CapeState::Slide,
                CapeState::Slide => CapeState::Slide,
                _ => CapeState::None,
            };
        } else if self.cape == CapeState::Slide {
            self.cape = CapeState::None;
        }
        // landing from a jump advances the chain count 0 → 1 → 2 → 0 and opens its window
        if self.on_ground && self.in_jump {
            self.in_jump = false;
            if self.chain_lock == 0 {
                self.chain_window = 8;
                self.chain_lock = 8;
                self.jump_count = if self.jump_count < 2 {
                    self.jump_count + 1
                } else {
                    0
                };
            }
        }

        if self.on_ground {
            self.stomp_chain = 0;
            if self.vy <= 0.0 {
                self.spinning = false;
            }
        }
        // Lanky jump: the rising flag ends at vy ≤ 1.0 or when jump is let go; the float cue
        // sounds when the 20-frame timer reaches 1 with the flag still up (SMM1 0x026c1470)
        self.float_cue = false;
        if self.form != Form::Lanky || (self.on_ground && self.vy <= 0.0) || self.in_water {
            self.lanky_jump = false;
            self.lanky_timer = 0;
        }
        if self.lanky_timer > 0 {
            if !jump || self.vy <= 1.0 {
                self.lanky_timer = -1; // flag down: no cue this jump
            } else {
                self.lanky_timer -= 1;
                self.float_cue = self.lanky_timer == 1;
            }
        }
        if self.crouch {
            self.pose = Pose::Crouch;
        } else if self.in_water && !self.on_ground {
            self.pose = Pose::Swim;
            self.walk_phase += 1.0;
        } else if !self.on_ground && self.spinning {
            self.pose = Pose::Spin;
            self.walk_phase += 1.0;
        } else if !self.on_ground {
            self.pose = Pose::Jump;
        } else if self.pose == Pose::Walk {
            self.walk_phase += self.vx.abs();
        }
        if self.throw {
            self.fire_pose = 8;
        }
        if self.fire_pose > 0 && !self.crouch {
            self.pose = Pose::Fire;
        }
        if !(self.style == Style::M1 && !self.on_ground && !self.in_water) {
            self.drawn_facing = self.facing;
        }
        if self.y < -2.0 * TILE {
            self.kill(); // fell into a pit
        }
    }

    /// State that status clear conditions look at.
    pub fn at_goal(&self) -> crate::clear::AtGoal {
        crate::clear::AtGoal {
            star: self.star > 0,
            form: match self.form {
                Form::Small => 0,
                Form::Super => 1,
                Form::Fire => 2,
                Form::Raccoon => 3,
                Form::Cape => 4,
                Form::Propeller => 5,
                Form::Cat => 6,
                Form::Builder => 7,
                Form::Superball => 8,
                Form::Mega => 9,
                Form::Lanky => 10,
                Form::Costume => 11,
                Form::Link => 12,
                Form::Frog => 13,
                Form::Balloon => 14,
                Form::Squirrel => 15,
                Form::Boomerang => 16,
                Form::Smb2 => 17,
            },
            shoe: self.shoe.is_some() && !self.shoe_karon,
            karon_shell: self.shoe.is_some() && self.shoe_karon,
            helmet: self.helmet.unwrap_or(0),
            clown_car: self.clown.is_some(),
            koopa_car: self.car.is_some(),
            cloud: false,
            holding_stone: false,
            holding_shell: false,
            yoshi: self.yoshi.is_some(),
        }
    }

    /// Slip: downhill toward 4.0 px/f with the slide accel of the slope class (table
    /// 0x7101d09700: gentle, class 2, 0.05; steep, class 3, 0.07) [obs]; on flat ground it
    /// slows by 0.09 and ends at rest.
    fn slide(&mut self, level: &Level) {
        // slide rates by steepness (0x7101d09700 / 0x7101d09714): one row while moving
        // downhill, the other while still moving uphill [obs: Ryujinx trace, 0.045 uphill on
        // the gentle slope]
        const SLIDE_DOWN: [f32; 5] = [0.03, 0.04, 0.05, 0.07, 0.17];
        const SLIDE_UP: [f32; 5] = [0.03, 0.03, 0.045, 0.12, 0.17];
        let slope = level
            .slope_at(self.x)
            .filter(|(sy, s)| (self.y - sy).abs() < 6.0 && s.on_diagonal(self.x))
            .map(|(_, s)| {
                let down = if s.rising { -1.0 } else { 1.0 };
                let uphill = self.vx * down < 0.0;
                let row = if uphill { &SLIDE_UP } else { &SLIDE_DOWN };
                (down, row[s.steepness()])
            });
        match slope {
            Some((down, acc)) => self.vx = approach(self.vx, 4.0 * down, acc),
            None => {
                self.vx = approach(self.vx, 0.0, 0.09);
                if self.vx == 0.0 {
                    self.sliding = false;
                }
            }
        }
        if self.vx != 0.0 {
            self.facing = self.vx.signum() as i32;
        }
        self.pose = Pose::Slide;
    }

    /// Propeller (player-physics.md §5.4, NSMBU): Spin (once per airtime) → rise toward vy 2.5
    /// at +1.0/frame for 50 frames with no gravity; then PropellerFall: gravity −0.1, max fall
    /// −1.25; Down while falling drills down at up to −6.0. Air control: target ±2.5 rising /
    /// ±1.5 falling, accel 0.08 / 0.07, 0.11 reversing, 0.05 without input.
    fn step_propeller(&mut self, inp: Input, spin_pressed: bool, vy_before: f32) {
        if self.form != Form::Propeller || self.in_water {
            self.prop_rise = 0;
            self.prop_fall = false;
            return;
        }
        if self.on_ground && self.vy <= 0.0 {
            self.prop_used = false;
            self.prop_fall = false;
            self.prop_rise = 0;
        }
        if spin_pressed && !self.prop_used && self.held.is_none() {
            self.prop_used = true;
            self.prop_rise = 50;
            self.prop_fall = false;
            self.on_ground = false;
            self.spinning = false;
        }
        let rising = self.prop_rise > 0;
        if rising {
            self.prop_rise -= 1;
            self.vy = (vy_before + 1.0).min(2.5);
            if self.prop_rise == 0 {
                self.prop_fall = true;
            }
        } else if self.prop_fall && !self.on_ground {
            self.vy = if inp.held(input::DOWN) {
                (vy_before - 0.34).max(-6.0)
            } else {
                (vy_before - 0.1).max(-1.25)
            };
        } else {
            return;
        }
        let dir = inp.dir_x() as f32;
        let target = dir * if rising { 2.5 } else { 1.5 };
        let acc = if dir == 0.0 {
            0.05
        } else if self.vx * dir < 0.0 {
            0.11
        } else if rising {
            0.08
        } else {
            0.07
        };
        self.vx = approach(self.vx, target, acc);
    }

    /// Cat wall climb (§5.4 Neko): pushing into a wall in the air climbs it at 1.0 px/f (1.5
    /// with run) for up to 120 frames or 120 px, gravity 0; reaching the top pops up (vy 4.8).
    /// Then WallClimbFall: gravity −0.085, max fall −2.0 while still on the wall. Jump → the
    /// wall jump; stick away or the wall ending lets go. The wall is only taken while falling
    /// (attach check 0x7100a1aa30); the first two attachments of an air time climb, later ones
    /// only slide (count +0x271c, bumped in WallClimb enter 0x7100a1b5b0, cleared on landing);
    /// after letting go the wall can't be taken again for 16 frames (+0x2714) [obs].
    fn cat_wall_climb(&mut self, inp: Input, pressed: bool, level: &Level) -> bool {
        let dir = inp.dir_x();
        if self.cat_reattach > 0 {
            self.cat_reattach -= 1;
        }
        if self.cat_climb == 0 {
            if self.cat_reattach > 0
                || self.held.is_some()
                || self.shoe.is_some()
                || dir == 0
                || self.vy >= 0.0
                || !self.wall_at(level, dir)
            {
                return false;
            }
            if self.cat_climbs < 2 {
                self.cat_climb = 1;
                self.cat_climbs += 1;
            } else {
                self.cat_climb = -1;
            }
            self.cat_wall = dir;
            self.cat_climbed = 0.0;
            self.spinning = false;
        }
        let w = self.cat_wall;
        self.facing = w;
        self.vx = 0.0;
        if pressed {
            self.vy = 3.5;
            self.vx = -w as f32 * 2.25;
            self.facing = -w;
            self.cat_climb = 0;
            self.cat_reattach = 16;
            return true;
        }
        if dir == -w {
            self.cat_climb = 0;
            self.cat_reattach = 16;
            return false;
        }
        if self.cat_climb > 0 {
            if !self.wall_at(level, w) {
                // over the top
                self.vy = 4.8;
                self.cat_climb = 0;
                self.cat_reattach = 16;
                return true;
            }
            let speed = if inp.held(input::RUN) { 1.5 } else { 1.0 };
            self.vy = speed;
            self.cat_climbed += speed;
            self.cat_climb += 1;
            if self.cat_climb > 120 || self.cat_climbed >= 120.0 {
                self.cat_climb = -1;
            }
            return true;
        }
        // sliding down after the climb
        if !self.wall_at(level, w) {
            self.cat_climb = 0;
            self.cat_reattach = 16;
            return false;
        }
        self.vy = (self.vy - 0.085).max(-2.0);
        true
    }

    fn wall_at(&self, level: &Level, dir: i32) -> bool {
        let s = self.sensors();
        let x = self.x + dir as f32 * (s.wall_x + 0.5);
        level.cell_at(x, self.y + s.wall_y0).is_solid()
            && level.cell_at(x, self.y + s.wall_y1).is_solid()
    }

    /// Wall slide / wall jump (§3.3, NSMBU and 3D World): falling against a wall with the stick toward
    /// it slides at ≥ −2.0; jump → vy 3.5 and 2.25 px/f away; 15 frames of stick held away let go.
    /// Ground pound (NSMBU / 3DW, HipAttack): Down held 5 frames in the air → a wind-up with no
    /// speed or gravity for the `hip_start` animation plus 5 frames (NSMBU 10 + 5, 3D World 17 + 5
    /// [obs: Ryujinx trace]), then a dive that starts at −6.0 (fall cap −6.0) until landing.
    fn step_wall_and_pound(&mut self, inp: Input, pressed: bool, level: &Level, vy_before: f32) {
        let down = inp.held(input::DOWN);
        let down_pressed = down && !self.prev_down;
        self.prev_down = down;
        self.pound_landed = false;
        if self.on_ground || self.in_water {
            if self.pound == -1 && self.on_ground {
                self.pound_landed = true;
            }
            self.pound = 0;
            self.pound_hold = 0;
            self.wall_slide = 0;
            self.cat_climb = 0;
            self.cat_climbs = 0;
            self.cat_reattach = 0;
            self.dive = false;
            self.roll_used = false;
            return;
        }
        if self.form != Form::Cat {
            self.cat_climb = 0;
            self.dive = false;
        }
        if down && (down_pressed || self.pound_hold > 0) {
            if inp.dir_x() == 0 {
                self.pound_hold = self.pound_hold.saturating_add(1);
            }
        } else {
            self.pound_hold = 0;
        }
        if self.style.has_jump_chain()
            && self.pound_hold == 5
            && self.pound == 0
            && !self.dive
            && self.cat_climb == 0
            && self.held.is_none()
            && self.shoe.is_none()
        {
            if self.form == Form::Cat && self.pu.headdress.is_none() {
                self.dive = true;
            } else {
                self.pound = if self.style == Style::W3 {
                    17 + 5
                } else {
                    10 + 5
                };
            }
            self.wall_slide = 0;
        }
        if self.dive {
            // cat dive (ClimbBodyAttack): vy −3.0, 4.0 px/f forward, no accel, until landing
            // or a wall
            if self.wall_at(level, self.facing) {
                self.dive = false;
            } else {
                self.vx = 4.0 * self.facing as f32;
                self.vy = -3.0;
                self.attack = self.attack.max(8);
                return;
            }
        }
        if self.form == Form::Cat
            && self.pu.headdress.is_none()
            && self.cat_wall_climb(inp, pressed, level)
        {
            return;
        }
        if self.pound > 0 {
            self.pound -= 1;
            self.vx = 0.0;
            self.vy = 0.0;
            if self.pound == 0 {
                self.pound = -1;
            }
            return;
        }
        if self.pound == -1 {
            self.vx = 0.0;
            self.vy = -6.0;
            return;
        }
        let dir = inp.dir_x();
        // wall slide / wall jump exist only in NSMBU and 3D World (0x7100a1e0c0 tests the
        // style) and start only while falling (0x7100a0e010) [obs; Ryujinx trace: none in SMW]
        let able = matches!(self.style, Style::WU | Style::W3)
            && self.held.is_none()
            && self.shoe.is_none()
            && vy_before < 0.0;
        if self.wall_slide == 0 && able && dir != 0 && self.wall_at(level, dir) {
            self.wall_slide = dir;
            self.wall_release = 0;
        }
        if self.wall_slide != 0 {
            let w = self.wall_slide;
            if !self.wall_at(level, w) {
                self.wall_slide = 0;
            } else if pressed {
                // wall jump (0x7100a1e810): vy 3.5 through the jump entry, whose first-frame
                // gravity comes from the vy = 0 band with jump held (−0.08) [obs: Ryujinx trace]
                self.vy = 3.5 + GRAVITY_HELD[vy_band(0.0)];
                self.vx = -w as f32 * 2.25;
                self.facing = -w;
                self.wall_slide = 0;
                self.spinning = false;
            } else {
                if dir == -w {
                    self.wall_release += 1;
                    if self.wall_release >= 15 {
                        self.wall_slide = 0;
                    }
                } else {
                    self.wall_release = 0;
                }
                // WallSlide: fixed gravity −0.34 (entry 0x7100a0e010 sets it, the band table
                // isn't used), max fall −2.0; the player faces away from the wall [obs: trace]
                self.vy = (vy_before + GRAVITY_GROUND).max(-2.0);
                self.facing = -w;
            }
        }
    }

    /// P-meter (SMB3) / cape meter (SMW), raccoon flaps (player-physics.md §2.5, §5.4): the
    /// meter fills 10/frame while running on the ground at ≥ 2.1 px/f with run and a direction
    /// held (630 = full, 63 frames), holds 30 frames, then drains 4/frame. A jump with a full
    /// meter opens a 240-frame flight window; in the air each jump press flaps while the window
    /// is open, else (falling) slows the fall.
    fn step_meters(&mut self, inp: Input, pressed: bool, _jump: bool) {
        let running =
            self.on_ground && inp.held(input::RUN) && inp.dir_x() != 0 && self.vx.abs() >= 2.1;
        if running {
            self.p_meter = (self.p_meter + 10).min(630);
            self.p_hold = 30;
        } else if self.on_ground || self.flight == 0 {
            if self.p_hold > 0 {
                self.p_hold -= 1;
            } else {
                self.p_meter = (self.p_meter - 4).max(0);
            }
        }
        if self.flight > 0 {
            self.flight -= 1;
        }
        if pressed && self.on_ground && self.p_meter >= 630 && self.form == Form::Raccoon {
            self.flight = 240;
        }
        if pressed && !self.on_ground && self.form == Form::Raccoon {
            if self.flight > 0 {
                self.flap = 18;
            } else if self.vy < 0.0 {
                self.slow_fall = 15;
            }
            self.attack = self.attack.max(8);
        }
        // SMW cape meter (0x71009f5d10) [obs]: +1 a frame running on the ground with Run and a
        // direction held at ≥ 2.1 px/f, up to 30; anything else empties it, except that a full
        // meter is kept in the air
        if self.style == Style::MW {
            if running {
                self.cape_meter = (self.cape_meter + 1).min(30);
            } else if !(self.cape_meter == 30 && !self.on_ground) {
                self.cape_meter = 0;
            }
        }
    }

    /// SMW cape flight [obs]. A jump rising faster than 1.0 with a full cape meter (Jump exec
    /// 0x71009f1450) starts MantFlyingUp (0x71009f60a0 / 0x71009f61a0): a 72-frame counter
    /// holds vy at the jump launch speed (3.628 + the speed bonus) with no gravity; letting go
    /// of Jump after 11 frames, or a ceiling, ends it; then vy falls by 0.3 a frame and at 0
    /// the glide (MantFlying 0x71009f68c0 / 0x71009f6990) starts if Run is held. Gliding, the
    /// speed heads for 3.25 forward (0.06 a frame, only while diving), vy = max(vy, minimum of
    /// the cape stage) + the stage's change, and a dive's stored lift is paid back as +0.75 a
    /// frame in stage 0 while moving forward. Run released at stage 0 ends it. Called after
    /// gravity; `vy0` / `vx0` / `face0` are this frame's values before the movement code.
    fn step_cape(&mut self, inp: Input, jump: bool, vy0: f32, vx0: f32, face0: i32) {
        if self.form != Form::Cape || self.in_water {
            self.cape = CapeState::None;
            return;
        }
        let run = inp.held(input::RUN);
        match self.cape {
            CapeState::None => {
                if self.in_jump && !self.on_ground && vy0 > 1.0 && self.cape_meter == 30 {
                    self.cape = CapeState::Rise(72);
                }
            }
            CapeState::Rise(t) => {
                // the state counter ticks before the state runs
                let mut t = (t - 1).max(0);
                let mut vy = vy0;
                if self.ceiling {
                    t = 0;
                    vy = 0.0;
                }
                if t < 61 && !jump {
                    t = 0;
                }
                if t == 0 {
                    self.vy = vy - 0.3;
                    self.cape = if self.vy > 0.0 {
                        CapeState::Rise(0)
                    } else if run {
                        CapeState::Fly {
                            stage: 0,
                            target: 2,
                            mode: 0,
                            counter: 8,
                            quick: false,
                            lift: 0.0,
                        }
                    } else {
                        CapeState::None
                    };
                } else {
                    self.vy = JUMP_BASE + jump_bonus(self.vx.abs());
                    self.cape = CapeState::Rise(t);
                }
            }
            CapeState::Fly {
                mut stage,
                target,
                mode,
                mut counter,
                quick,
                mut lift,
            } => {
                if !run && stage == 0 {
                    self.cape = CapeState::None;
                    return;
                }
                // the glide steers by itself: undo this frame's air control and turning
                self.vx = vx0;
                self.facing = face0;
                let new_mode = if self.ceiling {
                    0
                } else if vy0 < 0.0 && run {
                    match inp.dir_x() {
                        0 => 0,
                        d if d == self.facing => 2,
                        _ => 1,
                    }
                } else {
                    1
                };
                if new_mode == 2 {
                    lift = 0.0;
                }
                let (mut mode, mut target, mut quick) = (mode, target, quick);
                if new_mode != mode {
                    mode = new_mode;
                    quick = (lift - 3.5).abs() <= f32::EPSILON;
                    target = [2, 0, 5][mode as usize];
                    counter = 0;
                }
                if mode == 2 {
                    self.vx = approach(self.vx, self.facing as f32 * 3.25, 0.06);
                }
                let k = ((-2.0 * vy0) as i32).clamp(0, 8) as usize;
                lift = lift.max(CAPE_LIFT[k]);
                let base = vy0.max(CAPE_MIN_VY[stage as usize]);
                let mut add = CAPE_DVY[stage as usize];
                if stage == 0 && lift != 0.0 {
                    if self.ceiling || lift < vy0 {
                        lift = 0.0;
                    }
                    if self.vx * self.facing as f32 > 0.0 {
                        add = 0.75;
                    }
                }
                self.vy = (base + add).max(FALL_CAP);
                // the cape's own update at the end of the frame (0x7100987080)
                if counter <= 1 {
                    if stage != target {
                        stage = if target > stage { stage + 1 } else { stage - 1 };
                    }
                    counter = if quick { 2 } else { 8 };
                } else {
                    counter -= 1;
                }
                self.cape = CapeState::Fly {
                    stage,
                    target,
                    mode,
                    counter,
                    quick,
                    lift,
                };
            }
            CapeState::Slide => {}
        }
    }

    /// The cape's pose stage while gliding (0 up2 … 5 down4).
    pub fn cape_stage(&self) -> Option<u8> {
        match self.cape {
            CapeState::Fly { stage, .. } => Some(stage),
            _ => None,
        }
    }

    /// MantSlide (0x71009f6df0) [obs]: on flat ground the speed is capped at 3.25 and runs down
    /// at 0.06 (ice 0.02) with no control; it ends at a stop. On a slope it becomes the slip.
    fn cape_slide(&mut self, level: &Level) {
        let on_slope = level
            .slope_at(self.x)
            .is_some_and(|(sy, _)| (self.y - sy).abs() < 6.0);
        if on_slope {
            self.cape = CapeState::None;
            self.sliding = true;
            return;
        }
        let decel = if self.on_ice { 0.02 } else { 0.06 };
        self.vx = approach(self.vx.clamp(-3.25, 3.25), 0.0, decel);
        self.pose = Pose::Slide;
        if self.vx == 0.0 {
            self.cape = CapeState::None;
        }
    }

    fn vine_at(level: &Level, x: f32, y: f32) -> Option<(f32, f32, f32)> {
        level
            .vines
            .iter()
            .copied()
            .find(|v| (x - v.0).abs() <= 10.0 && y + 8.0 >= v.1 && y + 4.0 <= v.2)
    }

    /// Grab a vine the player overlaps (grab check 0x7100a12cb0) [obs]: Up held, not carrying
    /// anything, not rising, and the re-grab lock run out. The side: the near one at |v| ≥ 1.5
    /// (the facing flipped), else the side the player is on.
    fn try_grab(&mut self, inp: Input, level: &Level) -> bool {
        if self.climb_lock > 0 || self.in_water || self.held.is_some() || self.vy > 0.0 {
            return false;
        }
        let want = inp.held(input::UP);
        let Some(v) = want.then(|| Self::vine_at(level, self.x, self.y)).flatten() else {
            return false;
        };
        let side = if self.vx.abs() >= 1.5 {
            -self.facing
        } else if self.x <= v.0 {
            -1
        } else {
            1
        };
        self.climbing = Some((v.0, side));
        self.vx = 0.0;
        self.vy = 0.0;
        self.crouch = false;
        true
    }

    /// Climbing (player state "Vine"): held 6 px beside the vine (SMB1), up/down at 1.0 px/f
    /// (1.5 with run), vy toward the target by 0.5/frame (reversing zeroes it first), up only
    /// while the vine continues above; toward the other side switches sides; jump lets go with
    /// a 30-frame re-grab lock; reaching the ground without Up held steps off. Returns false
    /// when the player let go (normal physics take over this frame).
    fn climb(&mut self, inp: Input, pressed: bool, run: bool, level: &Level) -> bool {
        let Some((vx, mut side)) = self.climbing else {
            return false;
        };
        let Some(v) = Self::vine_at(level, vx, self.y) else {
            self.climbing = None;
            return false;
        };
        if pressed {
            self.climbing = None;
            self.climb_lock = 30;
            self.vy = JUMP_BASE;
            self.on_ground = false;
            self.vx = inp.dir_x() as f32 * 1.0;
            return false;
        }
        let dir = inp.dir_x();
        if dir != 0 && dir == -side {
            side = -side;
        }
        let speed = if run { 1.5 } else { 1.0 };
        let target = if inp.held(input::UP) && self.y + 16.0 < v.2 {
            speed
        } else if inp.held(input::DOWN) {
            -speed
        } else {
            0.0
        };
        if target != 0.0 && self.vy * target < 0.0 {
            self.vy = 0.0;
        }
        self.vy += (target - self.vy).clamp(-0.5, 0.5);
        self.vx = 0.0;
        self.x = vx + side as f32 * 6.0;
        self.facing = -side;
        let sensors = self.sensors();
        self.move_and_collide(level, sensors);
        if self.on_ground && !inp.held(input::UP) {
            self.climbing = None;
            return true;
        }
        self.climbing = Some((vx, side));
        self.pose = Pose::Climb;
        self.walk_phase += self.vy.abs();
        true
    }

    /// Swimming (§5.1): strokes add 1.0 (1.25 from the bottom), the player sinks toward −1.5
    /// (−0.043/frame while rising, −0.027 while sinking), upward cap 1.8; horizontal speeds from
    /// the swim table (row 0 swimming, row 3 walking on the bottom).
    fn swim(&mut self, inp: Input, pressed: bool) {
        if !self.was_in_water {
            self.vy = self.vy.max(-1.5);
            self.vx = self.vx.clamp(-1.125, 1.125);
        }
        // vertical
        if self.vy >= 0.0 {
            self.vy -= 0.043;
        } else if self.vy > -1.5 {
            self.vy = (self.vy - 0.027).max(-1.5);
        } else {
            self.vy = (self.vy + 0.094).min(-1.5);
        }
        self.vy = self.vy.min(1.8);
        if pressed {
            if self.on_ground {
                self.vy = 1.25;
                self.on_ground = false;
            } else {
                self.vy += 1.0;
            }
            self.exit_window = 20;
            self.walk_phase = 0.0;
        }
        // horizontal: (target, reverse, accel ≤ target, decel > target, idle ≤, idle >, cap, above cap)
        let row = if self.on_ground {
            (0.5625, 0.05, 0.025, 0.05, 0.004, 0.04, 1.0, 0.05)
        } else {
            (1.125, 0.03, 0.025, 0.025, 0.004, 0.02, 1.5, 0.03)
        };
        // (0x7100a123a0) [obs]: the target is ±c0 or 0; above the cap the speed moves toward
        // that target at c7; otherwise the rate is picked by comparing |v| with |target|, so
        // with no direction any motion slows at the "idle >" rate
        let dir = inp.dir_x();
        let v = self.vx;
        let target = row.0 * dir as f32;
        if dir != 0 {
            self.facing = dir;
        }
        let a = if v.abs() > row.6 {
            row.7
        } else if dir == 0 {
            if v.abs() <= 0.0 { row.4 } else { row.5 }
        } else if v * target < 0.0 {
            row.1
        } else if v.abs() <= target.abs() {
            row.2
        } else {
            row.3
        };
        self.vx = approach(v, target, a);
        if self.on_ground {
            self.pose = if self.vx == 0.0 {
                Pose::Wait
            } else {
                Pose::Walk
            };
        }
    }

    /// Room to stand up from a crouch.
    fn can_stand(&self, level: &Level) -> bool {
        let hy = self.y + SUPER.head_y;
        ![-SUPER.head_x, SUPER.head_x]
            .iter()
            .any(|&hx| Self::solid(level, self.x + hx, hy))
    }

    fn solid(level: &Level, px: f32, py: f32) -> bool {
        level.cell_at(px, py).is_solid()
    }

    fn spike(level: &Level, px: f32, py: f32) -> bool {
        level.cell_at(px, py) == Cell::Spike
    }

    /// Axis-separated movement against the tile grid using the spec's sensor points.
    fn move_and_collide(&mut self, level: &Level, s: &Sensors) {
        self.slope_prev = self.slope;
        // horizontal: on a slope, ground speed is along the surface (x scaled by |cos θ|, §1.3)
        let cos = match self.slope {
            Some((steep, _)) if self.on_ground => {
                if steep >= 3 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    // cos of the gentle angle (0x12e4051d) from the game's 256-step sine
                    // table with linear interpolation, not the exact 0.8944272 [obs: trace]
                    0.894_400_9
                }
            }
            _ => 1.0,
        };
        let was_on_ground = self.on_ground;
        self.spiked = false;
        self.x += self.vx * cos;
        // the slope under the feet centre: its own body never blocks sideways (only its outer
        // faces do, approached from beside it) [obs: Ryujinx trace, running up a slope]
        let own_slope = level.slope_at(self.x).map(|(_, sl)| *sl);
        for dir in [-1.0f32, 1.0] {
            let wx = self.x + dir * s.wall_x;
            let prev_wx = wx - self.vx * cos;
            let cell_x = (wx / TILE).floor() * TILE;
            let inside_own = own_slope.is_some_and(|sl| sl.covers(wx));
            let hit = [s.wall_y0, s.wall_y1].iter().any(|&wy| {
                let c = level.cell_at(wx, self.y + wy);
                (c.is_solid() && !inside_own)
                    // one-way walls block only motion into them from their blocking side
                    || (dir < 0.0 && c == Cell::PassRight && prev_wx >= cell_x + TILE - 0.01)
                    || (dir > 0.0 && c == Cell::PassLeft && prev_wx <= cell_x + 0.01)
            });
            // spikes hurt only when moving into them along that axis
            if self.vx * dir > 0.0
                && [s.wall_y0, s.wall_y1]
                    .iter()
                    .any(|&wy| Self::spike(level, wx, self.y + wy))
            {
                self.spiked = true;
            }
            if hit {
                let tile_edge = if dir > 0.0 {
                    (wx / TILE).floor() * TILE
                } else {
                    ((wx / TILE).floor() + 1.0) * TILE
                };
                // flush with the tile edge, as the game resolves walls [obs: Ryujinx trace]
                self.x = tile_edge - dir * s.wall_x;
                if self.vx * dir > 0.0 {
                    self.vx = 0.0;
                }
            }
        }
        // moving solids: sides (full solids only)
        for b in level.solids.iter().filter(|b| !b.top_only) {
            let (y0, y1) = (self.y + s.wall_y0, self.y + s.wall_y1);
            if y1 <= b.y0 || y0 >= b.y1 {
                continue;
            }
            let (l, r) = (self.x - s.wall_x, self.x + s.wall_x);
            if r > b.x0 && l < b.x1 {
                if self.x < (b.x0 + b.x1) / 2.0 {
                    self.x = b.x0 - s.wall_x - 0.001;
                    self.vx = self.vx.min(0.0);
                } else {
                    self.x = b.x1 + s.wall_x + 0.001;
                    self.vx = self.vx.max(0.0);
                }
            }
        }
        self.x = self.x.clamp(level.left + s.wall_x, level.right - s.wall_x);

        // vertical
        let prev_y = self.y;
        self.y += self.vy;
        self.ride = None;
        if self.vy > 0.0 {
            let hy = self.y + s.head_y;
            if [-s.head_x, s.head_x]
                .iter()
                .any(|&hx| Self::spike(level, self.x + hx, hy))
            {
                self.spiked = true;
            }
            for b in level.solids.iter().filter(|b| !b.top_only) {
                if self.x + s.head_x > b.x0
                    && self.x - s.head_x < b.x1
                    && hy > b.y0
                    && prev_y + s.head_y <= b.y0 + 0.01
                {
                    self.y = b.y0 - s.head_y - 0.001;
                    self.head_solid = Some(b.owner);
                    self.vy = 0.0;
                }
            }
            let hy = self.y + s.head_y;
            let prev_head = prev_y + s.head_y;
            let hits: Vec<f32> = [-s.head_x, s.head_x]
                .into_iter()
                .filter(|&hx| {
                    let c = level.cell_at(self.x + hx, hy);
                    c.is_solid()
                        || (c == Cell::PassDown && prev_head <= (hy / TILE).floor() * TILE + 0.01)
                })
                .collect();
            if !hits.is_empty() {
                // the block that gets hit: the one under the head centre if both probes hit
                let cx = if hits.len() == 2 {
                    self.x
                } else {
                    self.x + hits[0]
                };
                self.head_hit = Some(((cx / TILE).floor() as i32, (hy / TILE).floor() as i32));
                self.y = (hy / TILE).floor() * TILE - s.head_y - 0.001;
                self.vy = 0.0;
            }
            self.on_ground = false;
            self.slope = None;
        } else {
            // feet: land on solid cells, or on one-way tops we were above last frame
            let mut landed = None;
            for fx in [-s.foot_x, s.foot_x] {
                // the probe sits just under the feet: arriving exactly on a tile top lands
                // [obs: Ryujinx trace, a long jump ending at y = 32.0]
                let (px, py) = (self.x + fx, self.y - 0.001);
                // a slope's columns are probed at the feet centre only (below)
                if level.slope_at(px).is_some() {
                    continue;
                }
                let top = ((py / TILE).floor() + 1.0) * TILE;
                let cell = level.cell_at(px, py);
                let ok = cell.is_solid() || (cell == Cell::SemiTop && prev_y >= top - 0.01);
                if ok {
                    landed = Some(landed.map_or(top, |t: f32| t.max(top)));
                }
            }
            // slopes: the surface under the feet centre (the game stands the player on it at
            // that point; the slope state only on the diagonal) [obs: Ryujinx trace]
            let mut on_slope: Option<&Slope> = None;
            if let Some((surf, slope)) = level.slope_at(self.x) {
                let reach = if was_on_ground { SLOPE_SNAP } else { 0.0 };
                let can_land = self.y <= surf + reach && prev_y >= surf - SLOPE_SNAP;
                if can_land && landed.is_none_or(|t| surf >= t) {
                    landed = Some(surf);
                    if slope.on_diagonal(self.x) {
                        on_slope = Some(slope);
                    }
                }
            }
            if [-s.foot_x, s.foot_x]
                .iter()
                .any(|&fx| Self::spike(level, self.x + fx, self.y))
                && self.vy < 0.0
            {
                self.spiked = true;
            }
            // moving solids: land on their tops if the feet were above last frame
            let mut ride = None;
            for b in &level.solids {
                let over = self.x + s.foot_x > b.x0 && self.x - s.foot_x < b.x1;
                let reach = if was_on_ground {
                    2.0 + b.slope.abs() * 2.0
                } else {
                    0.0
                };
                let top = b.top_at(self.x);
                if over
                    && self.y <= top + reach
                    && prev_y >= top - b.dy.abs() - 1.0 - b.slope.abs() * 2.0
                    && landed.is_none_or(|t| top >= t)
                {
                    landed = Some(top);
                    ride = Some(b.owner);
                    on_slope = None;
                }
            }
            self.ride = ride;
            match landed {
                Some(top) => {
                    self.y = top;
                    self.vy = 0.0;
                    self.on_ground = true;
                    self.slope =
                        on_slope.map(|sl| (sl.steepness(), if sl.rising { 1 } else { -1 }));
                }
                None => {
                    self.on_ground = false;
                    self.slope = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(run_frames: usize, run: bool) -> f32 {
        let level = Level::flat(400);
        let mut p = Player::new(56.0, 32.0);
        let mv = input::RIGHT | if run { input::RUN } else { 0 };
        for _ in 0..run_frames {
            p.step(Input { buttons: mv }, &level);
        }
        let base = p.y;
        let mut top: f32 = 0.0;
        for _ in 0..120 {
            p.step(
                Input {
                    buttons: mv | input::JUMP,
                },
                &level,
            );
            top = top.max(p.y - base);
        }
        top
    }

    /// Spec (player-physics.md §0): held jump peaks ≈ 67.6 px standing, ≈ 86.3 px at full run.
    #[test]
    fn jump_heights_match_spec() {
        let standing = peak(0, false);
        assert!((standing - 67.6).abs() < 0.6, "standing peak {standing}");
        let running = peak(150, true);
        assert!((running - 86.3).abs() < 0.6, "running peak {running}");
    }

    #[test]
    fn walk_run_caps_and_friction() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..120 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
        }
        assert_eq!(p.vx, WALK_MAX);
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | input::RUN,
                },
                &level,
            );
        }
        assert_eq!(p.vx, RUN_MAX);
        for _ in 0..200 {
            p.step(Input::default(), &level);
        }
        assert_eq!(p.vx, 0.0);
    }

    #[test]
    fn skid_brakes_at_0_12() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | input::RUN,
                },
                &level,
            );
        }
        p.step(
            Input {
                buttons: input::LEFT | input::RUN,
            },
            &level,
        );
        assert_eq!(p.pose, Pose::Skid);
        assert!((p.vx - (RUN_MAX - SKID_DECEL)).abs() < 1e-6);
    }

    #[test]
    fn skid_stops_holds_8_frames_then_turns() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        let right = Input {
            buttons: input::RIGHT | input::RUN,
        };
        let left = Input {
            buttons: input::LEFT | input::RUN,
        };
        for _ in 0..200 {
            p.step(right, &level);
        }
        // 3.0 / 0.12 = 25 braking frames, then 9 more frames stopped (the stop is noticed a
        // frame late, then the 8-frame hold), then the 0.25 re-acceleration
        let mut speeds = Vec::new();
        for _ in 0..40 {
            p.step(left, &level);
            speeds.push(p.vx);
        }
        let first_stop = speeds.iter().position(|v| *v == 0.0).unwrap();
        let moving_again = speeds.iter().rposition(|v| *v == 0.0).unwrap() + 1;
        assert_eq!(moving_again - first_stop, 10);
        assert!((speeds[moving_again] + SKID_REACCEL).abs() < 1e-6);
        assert_eq!(p.facing, -1);
        // letting go mid-brake ends the skid after that frame's braking (the Turn state sets
        // its rate before checking the stick): normal deceleration from then on
        let mut q = Player::new(56.0, 32.0);
        for _ in 0..200 {
            q.step(right, &level);
        }
        q.step(left, &level);
        let v = q.vx;
        q.step(Input::default(), &level);
        assert!((v - q.vx - SKID_DECEL).abs() < 1e-6);
        q.step(Input::default(), &level);
        assert_ne!(q.pose, Pose::Skid);
    }

    #[test]
    fn stands_on_ground_and_falls_into_pit() {
        let level = Level::flat(10); // ground ends at x = 160 px
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..10 {
            p.step(Input::default(), &level);
        }
        assert!(p.on_ground && p.y == 32.0);
        for _ in 0..400 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
        }
        assert!(p.x <= level.right - SMALL_M1.wall_x + 0.01);
    }

    #[test]
    fn walks_up_and_down_a_slope() {
        // flat ground (2 rows) with a gentle "/" slope whose bottom row is the floor's top row
        // (its low end is flush with the floor) up onto a raised plateau
        let mut level = Level::flat(60);
        level.add_test_slope(
            Slope {
                left: 10,
                bottom: 1,
                width: 6,
                height: 3,
                rising: true,
                steep: false,
            },
            16,
            4,
        );
        let mut p = Player::new(56.0, 32.0);
        let mut max_y: f32 = 0.0;
        for _ in 0..400 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
            max_y = max_y.max(p.y);
            assert!(p.alive());
        }
        assert!(
            p.x > 15.0 * TILE,
            "should have walked past the slope, x = {}",
            p.x
        );
        assert!(
            (max_y - 4.0 * TILE).abs() < 0.01,
            "should end up on the plateau, y = {max_y}"
        );
        assert!(p.on_ground);
    }

    #[test]
    fn crouching_on_a_slope_slides_down_outside_smb1() {
        // a steep "\" slope from a plateau at tile 10..14 down to the floor
        let mut level = Level::flat(60);
        for x in 6..10 {
            for y in 0..6 {
                level.set(x, y, Cell::Solid);
            }
        }
        level.slopes.push(Slope {
            left: 10,
            bottom: 2,
            width: 5,
            height: 4,
            rising: false,
            steep: true,
        });
        for style in [Style::M3, Style::M1] {
            // on the diagonal (the first column is the flat top)
            let mut p = Player::new(11.0 * TILE + 8.0, 88.0);
            p.style = style;
            for _ in 0..10 {
                p.step(Input { buttons: 0 }, &level);
            }
            for _ in 0..20 {
                p.step(
                    Input {
                        buttons: input::DOWN,
                    },
                    &level,
                );
            }
            if style == Style::M1 {
                assert!(!p.sliding);
                continue;
            }
            assert!(p.sliding && p.pose == Pose::Slide);
            assert!(p.vx > 1.0, "accelerates downhill: {}", p.vx);
            for _ in 0..200 {
                p.step(Input { buttons: 0 }, &level);
            }
            assert!(!p.sliding, "stops on the flat");
        }
    }

    #[test]
    fn slope_speed_rules() {
        // walking (not running) down a steep slope targets 1.5 × 1.44; uphill 1.5 × 0.5
        let mut level = Level::flat(80);
        level.add_test_slope(
            Slope {
                left: 10,
                bottom: 2,
                width: 9,
                height: 8,
                rising: false,
                steep: true,
            },
            99,
            0,
        );
        let mut p = Player::new(10.0 * TILE + 8.0, 10.0 * TILE);
        let mut fastest: f32 = 0.0;
        for _ in 0..90 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
            if p.slope.is_some() {
                fastest = fastest.max(p.vx);
            }
        }
        assert!(
            fastest > WALK_MAX + 0.1,
            "downhill should exceed walk speed, got {fastest}"
        );
        assert!(
            fastest <= WALK_MAX * 1.44 + 1e-4,
            "downhill cap is 1.5 x 1.44, got {fastest}"
        );
    }

    #[test]
    fn deterministic() {
        let level = Level::flat(400);
        let script: Vec<u32> = (0..900)
            .map(|i| {
                if i % 97 < 50 {
                    input::RIGHT | input::RUN
                } else {
                    input::JUMP | input::LEFT
                }
            })
            .collect();
        let run = || {
            let mut p = Player::new(56.0, 32.0);
            for &b in &script {
                p.step(Input { buttons: b }, &level);
            }
            (p.x.to_bits(), p.y.to_bits(), p.vx.to_bits(), p.vy.to_bits())
        };
        assert_eq!(run(), run());
    }

    fn cat(level: &Level) -> Player {
        let mut p = Player::new(56.0, 32.0);
        p.style = Style::W3;
        p.form = Form::Cat;
        p.step(Input::default(), level);
        p
    }

    /// 3D World's two-stage run: 2.25 until the dash stage (75 frames, 60 as Cat), then 3.0.
    #[test]
    fn w3_run_has_a_dash_stage() {
        let level = Level::flat(800);
        let mut p = Player::new(56.0, 32.0);
        p.style = Style::W3;
        let run = Input {
            buttons: input::RIGHT | input::RUN,
        };
        for _ in 0..70 {
            p.step(run, &level);
        }
        assert!((p.vx - 2.25).abs() < 1e-4, "before the dash {}", p.vx);
        for _ in 0..40 {
            p.step(run, &level);
        }
        assert!(p.dashing() && (p.vx - 3.0).abs() < 1e-4, "dashing {}", p.vx);
        let mut c = cat(&level);
        for _ in 0..61 {
            c.step(run, &level);
        }
        assert!(c.dashing(), "the cat dashes after 60 frames ({})", c.dash);
    }

    #[test]
    fn cat_slide_attack_from_a_crouch_and_scratch() {
        let level = Level::flat(100);
        let mut p = cat(&level);
        let down = Input {
            buttons: input::DOWN,
        };
        p.step(down, &level);
        assert!(p.crouch);
        p.step(
            Input {
                buttons: input::DOWN | input::RUN,
            },
            &level,
        );
        assert_eq!(p.cat_slide, CAT_SLIDE_FRAMES - 1);
        let mut speeds = vec![p.vx];
        for _ in 0..50 {
            p.step(down, &level);
            speeds.push(p.vx);
        }
        assert!(speeds[..12].iter().all(|v| *v == 2.5), "{speeds:?}");
        assert!((speeds[12] - (2.5 - 0.075)).abs() < 1e-5, "{speeds:?}");
        // the 44-frame animation ends with no direction held: a dead stop
        assert_eq!(p.cat_slide, 0);
        assert_eq!(p.vx, 0.0);
        // standing, Run is the scratch: 17 frames, movement untouched
        let mut q = cat(&level);
        q.step(
            Input {
                buttons: input::RUN,
            },
            &level,
        );
        assert_eq!(q.attack, CAT_SCRATCH_FRAMES - 1);
        assert_eq!(q.vx, 0.0);
    }

    #[test]
    fn cape_meter_climb_glide_and_dive() {
        let level = Level::flat(400);
        let mut p = Player::new(56.0, 32.0);
        p.style = Style::MW;
        p.form = Form::Cape;
        let run_right = input::RIGHT | input::RUN;
        // 30 frames at ≥ 2.1 px/f fill the meter
        let mut frames = 0;
        while p.cape_meter < 30 {
            p.step(Input { buttons: run_right }, &level);
            frames += 1;
            assert!(frames < 200);
        }
        // the jump turns into the climb: vy held at the launch speed while Jump is held
        p.step(
            Input {
                buttons: run_right | input::JUMP,
            },
            &level,
        );
        p.step(
            Input {
                buttons: run_right | input::JUMP,
            },
            &level,
        );
        assert!(matches!(p.cape, CapeState::Rise(_)), "{:?}", p.cape);
        let launch = JUMP_BASE + 0.3;
        for _ in 0..20 {
            p.step(
                Input {
                    buttons: run_right | input::JUMP,
                },
                &level,
            );
            assert!((p.vy - launch).abs() < 1e-5, "{}", p.vy);
        }
        // release Jump: vy drains by 0.3 a frame, then the glide starts (Run held)
        let mut glide = false;
        for _ in 0..40 {
            p.step(Input { buttons: run_right }, &level);
            if p.cape_stage().is_some() {
                glide = true;
                break;
            }
        }
        assert!(glide);
        // forward held while falling: the dive stage climbs to 5, one step per 8 frames
        let mut stage5 = false;
        for _ in 0..80 {
            p.step(Input { buttons: run_right }, &level);
            if p.cape_stage() == Some(5) {
                stage5 = true;
                break;
            }
            if p.on_ground {
                break;
            }
        }
        assert!(stage5);
        assert!(p.vx > 3.0, "dives toward 3.25: {}", p.vx);
        // landing in the dive shakes the ground
        let mut quake = false;
        for _ in 0..200 {
            p.step(Input { buttons: run_right }, &level);
            quake |= p.cape_quake;
            if p.on_ground {
                break;
            }
        }
        assert!(quake);
    }

    #[test]
    fn cat_climbs_a_wall_then_pops_over_the_top() {
        let mut level = Level::flat(40);
        // a 5-tile wall at tile x = 6
        for y in 2..7 {
            level.set_solid(6, y);
        }
        let mut p = cat(&level);
        let right = Input {
            buttons: input::RIGHT,
        };
        let mut climbed = false;
        let mut top = false;
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | if p.on_ground { input::JUMP } else { 0 },
                },
                &level,
            );
            if p.cat_climb > 0 {
                climbed = true;
                assert!((p.vy - 1.0).abs() < 1e-4, "climbs at 1.0: {}", p.vy);
            }
            if climbed && p.cat_climb == 0 && p.vy > 4.0 {
                top = true;
                break;
            }
        }
        assert!(climbed, "grabbed the wall");
        assert!(top, "popped over the top (vy 4.8)");
        for _ in 0..20 {
            p.step(right, &level);
        }
        for _ in 0..100 {
            p.step(Input::default(), &level);
        }
        assert!(
            p.on_ground && p.y >= 7.0 * 16.0 - 0.01,
            "landed on the wall top: {}",
            p.y
        );
    }

    #[test]
    fn cat_slides_down_after_a_long_climb_and_dives() {
        let mut level = Level::flat(40);
        for y in 2..30 {
            level.set_solid(6, y);
        }
        let mut p = cat(&level);
        for _ in 0..200 {
            p.step(
                Input {
                    buttons: input::RIGHT | if p.on_ground { input::JUMP } else { 0 },
                },
                &level,
            );
            if p.cat_climb < 0 {
                break;
            }
        }
        assert_eq!(p.cat_climb, -1, "the climb ran out");
        let mut fell = p.vy;
        for _ in 0..30 {
            p.step(
                Input {
                    buttons: input::RIGHT,
                },
                &level,
            );
            fell = p.vy;
        }
        assert!(
            (-2.0..0.0).contains(&fell),
            "WallClimbFall caps at −2.0: {fell}"
        );

        // dive: Down in the air → 4.0 forward, −3.0 down
        let level = Level::flat(80);
        let mut p = cat(&level);
        p.step(
            Input {
                buttons: input::JUMP,
            },
            &level,
        );
        for _ in 0..10 {
            p.step(
                Input {
                    buttons: input::JUMP,
                },
                &level,
            );
        }
        // the ground-pound command needs Down held for 5 frames
        for _ in 0..5 {
            p.step(
                Input {
                    buttons: input::DOWN,
                },
                &level,
            );
        }
        assert!(
            p.dive && p.vx == 4.0 && p.vy == -3.0,
            "dive {} {}",
            p.vx,
            p.vy
        );
        for _ in 0..60 {
            p.step(Input::default(), &level);
        }
        assert!(p.on_ground && !p.dive, "the dive ends on landing");
    }

    #[test]
    fn cat_scratch_rises_once_per_air_time() {
        let level = Level::flat(80);
        let mut p = cat(&level);
        p.step(
            Input {
                buttons: input::JUMP,
            },
            &level,
        );
        for _ in 0..8 {
            p.step(Input::default(), &level);
        }
        p.step(
            Input {
                buttons: input::RUN,
            },
            &level,
        );
        assert!(p.attack > 0 && p.vy > 2.0, "rolling attack rise: {}", p.vy);
        let vy = p.vy;
        p.step(Input::default(), &level);
        assert!(
            (vy - p.vy - 0.1).abs() < 1e-4,
            "gravity −0.1: {vy} → {}",
            p.vy
        );
        for _ in 0..20 {
            p.step(Input::default(), &level);
        }
        p.step(
            Input {
                buttons: input::RUN,
            },
            &level,
        );
        assert!(p.vy < 1.0, "no second rise in the same air time: {}", p.vy);
    }

    /// NSMBU jump chain: running jumps re-pressed within 8 frames of landing count 0, 1, 2; the
    /// third is ×1.05; a late press starts over.
    #[test]
    fn jump_chain_counts_on_landing_within_eight_frames() {
        let level = Level::flat(400);
        let mut p = Player::new(56.0, 32.0);
        p.style = Style::WU;
        let run = input::RIGHT | input::RUN;
        for _ in 0..120 {
            p.step(Input { buttons: run }, &level);
        }
        let mut counts = Vec::new();
        for _ in 0..3 {
            // press on the first grounded frame, hold until landing
            p.step(
                Input {
                    buttons: run | input::JUMP,
                },
                &level,
            );
            counts.push((p.jump_count, p.vy));
            while !p.on_ground {
                p.step(
                    Input {
                        buttons: run | input::JUMP,
                    },
                    &level,
                );
            }
            p.step(Input { buttons: run }, &level);
        }
        assert_eq!(
            counts.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(counts[2].1 > counts[0].1 * 1.04, "third jump ×1.05");
        // waiting past the window resets the chain
        for _ in 0..12 {
            p.step(Input { buttons: run }, &level);
        }
        p.step(
            Input {
                buttons: run | input::JUMP,
            },
            &level,
        );
        assert_eq!(p.jump_count, 0);
    }

    // ---- behaviour pinned by Ryujinx frame traces (`mmk trace`) ----

    fn held(b: u32) -> Input {
        Input { buttons: b }
    }

    #[test]
    fn landing_frame_keeps_the_air_speed() {
        let level = Level::flat(200);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..60 {
            p.step(held(input::RIGHT | input::RUN), &level);
        }
        p.step(held(input::RIGHT | input::RUN | input::JUMP), &level);
        while !p.on_ground {
            p.step(held(0), &level);
        }
        let v = p.vx;
        p.step(held(0), &level); // the airborne state's last frame: no friction yet
        assert_eq!(p.vx, v);
        p.step(held(0), &level);
        assert!(p.vx < v);
    }

    #[test]
    fn landing_slowly_without_a_direction_stops_dead() {
        let level = Level::flat(200);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..3 {
            p.step(held(input::RIGHT), &level);
        }
        assert!((p.vx - 0.3).abs() < 1e-6);
        p.step(held(input::JUMP), &level);
        while !p.on_ground {
            p.step(held(0), &level);
        }
        p.step(held(0), &level);
        assert_eq!(p.vx, 0.0);
    }

    #[test]
    fn a_press_just_before_landing_jumps_three_frames_later() {
        let level = Level::flat(200);
        let tap_and_fall = |extra: Option<usize>| {
            let mut p = Player::new(56.0, 32.0);
            p.step(held(input::JUMP), &level);
            let mut grounded = Vec::new();
            for i in 0..60 {
                let b = if Some(i) == extra { input::JUMP } else { 0 };
                p.step(held(b), &level);
                grounded.push(p.on_ground);
            }
            grounded
        };
        let base = tap_and_fall(None);
        let t = base.iter().position(|&g| g).unwrap(); // touchdown
        let g = tap_and_fall(Some(t - 1));
        assert!(
            g[t] && g[t + 1],
            "no jump on the touchdown or the next frame"
        );
        assert!(!g[t + 2], "the retried press jumps 3 frames after it");
    }

    #[test]
    fn slopes_have_flat_end_columns() {
        let gentle = Slope {
            left: 0,
            bottom: 0,
            width: 8,
            height: 4,
            rising: true,
            steep: false,
        };
        assert_eq!(gentle.surface(8.0), 16.0);
        assert_eq!(gentle.surface(24.0), 20.0);
        assert_eq!(gentle.surface(120.0), 64.0);
        assert!(!gentle.on_diagonal(8.0) && gentle.on_diagonal(24.0));
        let steep = Slope {
            left: 0,
            bottom: 0,
            width: 5,
            height: 4,
            rising: false,
            steep: true,
        };
        assert_eq!(steep.surface(8.0), 64.0);
        assert_eq!(steep.surface(40.0), 40.0);
        assert_eq!(steep.surface(72.0), 16.0);
    }

    #[test]
    fn small_mario_ducks_with_crouch_friction() {
        let level = Level::flat(200);
        let mut p = Player::new(56.0, 32.0);
        for _ in 0..80 {
            p.step(held(input::RIGHT | input::RUN), &level);
        }
        let v = p.vx;
        p.step(held(input::DOWN), &level);
        assert!(
            (v - p.vx - 0.05).abs() < 1e-6,
            "crouch band 4: {}",
            v - p.vx
        );
    }

    #[test]
    fn w3_dash_starts_75_frames_after_moving() {
        let level = Level::flat(400);
        let mut p = Player::new(56.0, 32.0);
        p.style = Style::W3;
        // the first frame doesn't count (the speed before it is 0)
        for _ in 0..76 {
            p.step(held(input::RIGHT | input::RUN), &level);
        }
        assert!(p.dashing() && p.vx <= 2.25 + 1e-6);
        p.step(held(input::RIGHT | input::RUN), &level);
        assert!(p.vx > 2.25);
    }
}
