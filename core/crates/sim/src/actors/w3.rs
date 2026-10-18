//! 3D World enemies: Ant Trooper (96-ant-trooper.md), Stingby (65-stingby.md), Skipsqueak
//! (51-skipsqueak.md), Hop-Chops (50-hop-chops.md), Porcupuffer (114-porcupuffer.md), Bully
//! (117-bully.md), Charvaargh (86-charvaargh.md) and Piranha Creeper (107-piranha-creeper.md).
//! Every rule here follows the specs' [obs] facts; the few that the binary does not settle are
//! marked [inf] with the reason. The world-side rules (touching them, their sprites) are in
//! world/w3.rs.
//!
//! Many of these enemies are timed by their model animations: a state ends when a one-shot
//! animation ends, or acts inside a window of animation frames. The frame counts are the
//! animations' own lengths and the play rates come from the style's enemy model parameters
//! (`anim_len`); both are listed in the specs.
//!
//! State layout: `phase` is the state (0 = not set up yet; numbering per enemy in the modules
//! below), `timer` the state timer, `hops` the Porcupuffer's HP; everything else lives in
//! `Actor::w3`, together with what the world senses for these enemies each frame.

use super::{Actor, ENEMY_GRAVITY, Kind, MAX_FALL};
use crate::fmath::DetMath;
use crate::level::{Cell, Level, TILE};
use crate::world::VIEW_W;

/// Per-actor state of the 3D World enemies, and what the world senses for them before each step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct W3State {
    /// Sensed: the liquid surface (px) and whether it is water (else lava or poison).
    pub liquid: Option<(f32, bool)>,
    /// Sensed: the whole area is under water (underwater theme).
    pub flooded: bool,
    /// Sensed: the nearest player is in water, its height (px) and whether it launched a jump
    /// (or a swim stroke) this frame.
    pub player_wet: bool,
    pub player_h: f32,
    pub player_rising: bool,
    /// Sensed last frame: the player was on the ground, its vertical speed.
    pub player_ground_prev: bool,
    pub player_vy_prev: f32,
    /// Ant Trooper: the side its surface is on and the way it walks (indices into `DIRS`), and
    /// the corner it is wrapping round.
    pub down: u8,
    pub ahead: u8,
    pub corner: (f32, f32),
    /// Small counters: hops, recovery attempts, Bully push sub-phase.
    pub count: u8,
    pub tries: u8,
    /// Skipsqueak: wall crashes per direction (right, left).
    pub crashes: [u8; 2],
    /// A second timer: Stingby / Hop-Chops give-up, Porcupuffer cooldowns, Bully flip timer.
    pub clock: i32,
    /// Stingby patrol anchor, Porcupuffer surfacing x.
    pub anchor: f32,
    /// Charvaargh: distance the head has travelled since the leap began (px).
    pub travel: f32,
    /// Model animation frame (Hop-Chops), Piranha Creeper piece animation frame.
    pub frame: f32,
    /// Piranha Creeper: piece animation rate; the braking tracker near a stop.
    pub rate: f32,
    pub track: f32,
    /// Piranha Creeper: the piece the head is in; the piece a trampled head retracts to.
    pub node: i32,
    pub goal: i32,
    /// The state to go back to after a hit or a wait.
    pub resume: u8,
    /// Flag bits per enemy (see the `flag` constants).
    pub flags: u8,
    /// Bully: frames before the player's body can push it again; shoves of the player so far.
    pub cooldown: i32,
    pub shoves: u8,
    /// Charvaargh: its side / vertical leap spots on the area's record (`chr::REC_*` bits in
    /// `recs`, which also says whether it holds the turn).
    pub rec: [f32; 2],
    pub recs: u8,
    /// Sensed (`World::w3_charvaarghs`): the other Charvaarghs' side and vertical spots on record
    /// (the first `peers_n` of each, in placement order), and whether another one holds the turn.
    pub peers_side: [f32; PEERS],
    pub peers_vert: [f32; PEERS],
    pub peers_n: [u8; 2],
    pub turn_busy: bool,
    /// Bully: its last move ran into a wall (the game's states read the previous frame's
    /// terrain check; `Actor::wall` is cleared before each step).
    pub walled: bool,
}

/// Other Charvaarghs a Charvaargh keeps track of. The game's list (area +0xC0) is a pool with a
/// capacity at +0x28: a Charvaargh created while it is full is simply not registered [obs init
/// 0x710079db70]; the first 8 others are kept here [?: where the pool is built and its capacity
/// set was not found].
pub const PEERS: usize = 8;

/// `W3State::flags` bits.
mod flag {
    /// Hop-Chops: hopped in this animation window.
    pub const HOPPED: u8 = 1;
    /// Hop-Chops: braking to turn round in the chase.
    pub const BRAKING: u8 = 2;
    /// Bully: may stop at a ledge edge (HoldOut) when pushed.
    pub const CAN_HOLD: u8 = 1;
    /// Bully: a player is its target (chased after a push).
    pub const TARGET: u8 = 2;
    /// Bully (winged): may fly back up after being knocked off a ledge (set by Wait and Land).
    pub const CAN_RETURN: u8 = 4;
    /// Bully (winged): climbing without gravity in FlyChase.
    pub const CLIMBING: u8 = 8;
    /// Piranha Creeper: the trampled head retracted into its base and dies.
    pub const DIES: u8 = 1;
    /// Piranha Creeper: it was moving back when trampled.
    pub const WAS_BACK: u8 = 2;
    /// Piranha Creeper: reached the trample retract goal.
    pub const RETRACTED: u8 = 4;
}

/// Unit vectors of the crawler directions: 0 down, 1 right, 2 up, 3 left.
const DIRS: [(f32, f32); 4] = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];

fn opposite(d: u8) -> u8 {
    (d + 2) & 3
}

fn ease(v: f32, target: f32, step: f32) -> f32 {
    v + (target - v).clamp(-step, step)
}

/// Frames a one-shot animation of `frames` frames takes at play rate `rate`: the state that
/// waits for its end leaves on the first frame the animation frame reaches the end. The model
/// update adds the rate once per frame and holds a one-shot at its frame count (v1 0x7100f4d7f0);
/// the end test is frame ≥ frame count (0x7100f4d4a0, from the anim's slot 0x48 0x7100903190);
/// state changes run exit and enter at once (0x71011ca9b0) and the enter's play call sets the
/// start frame (0x7100902f30; 0 for these states), so on
/// the k-th frame after the enter the state sees k × rate whether the model update runs before
/// or after it [obs].
fn anim_len(frames: f32, rate: f32) -> i32 {
    (frames / rate).ceil() as i32
}

/// Frames a model yaw turn of `deg` degrees takes at the enemy turn rate (8°/frame, set for
/// every enemy model at EnemySys init 0x71007872e0) [obs].
fn yaw_frames(deg: f32) -> i32 {
    (deg / 8.0).ceil() as i32
}

// ---------------------------------------------------------------- Ant Trooper

/// Crawl speed along the surface: the shared crawler walk's 0.5 [obs 0x7100778710 tail]; the
/// Ant Trooper overrides the speed hook that makes the yellow Spike Top 1.5× faster with an
/// empty one, so the horned form walks at 0.5 too [obs vtable slot +0x1a0 = 0x7100603910].
const ANT_WALK: f32 = 0.5;
/// Ledge wrap turn rate (deg/frame): 6, big 4 [obs].
const ANT_TURN: f32 = 6.0;
const ANT_TURN_BIG: f32 = 4.0;
/// Winged: the winged Spiny's flyer with the Ant Trooper's overrides [obs Wing action vtable
/// 0x710214b180; FlyMove 0x7100782670 / 0x7100602980, hover 0x7100782a80, Turn 0x71007829a0]:
/// it flies level toward 0.5·dir (accel max(0.02, 0.03·|vx|); the horned one too, its
/// alternate-speed hook 0x71006042b0 returns 0), with no gravity, bobbing about a centre 4 px
/// above where it took wing: vy steps 0.004/f toward the centre, |vy| ≤ 0.18 (an 8-px, 180-frame
/// bob); a ceiling sends it down at 0.18. A wall turns it round: the speed is mirrored at once
/// and held (no acceleration) until the 8-frame turn lock is over and the model's yaw has swung
/// from one side to the other (DirDeg 75 at 8°/f). It never lands, and a stomp leaves its wings
/// (stomps are ReactOnly, which skips the lose-wing routine 0x710078db00).
const ANT_FLY: f32 = 0.5;
const ANT_FLY_ACCEL: f32 = 0.02;
const ANT_FLY_ACCEL_K: f32 = 0.03;
const ANT_FLY_ABOVE: f32 = 4.0;
const ANT_FLY_BOB: f32 = 0.004;
const ANT_FLY_BOB_MAX: f32 = 0.18;
const ANT_FLY_TURN_LOCK: i32 = 8;
const ANT_DIR_DEG: f32 = 75.0;

mod ant {
    /// On the floor: walks with gravity (also while falling, InAir).
    pub const FLOOR: u8 = 1;
    /// On a wall or the ceiling: no gravity (Walk / RotationWall).
    pub const CRAWL: u8 = 2;
    /// Turning round a convex corner (RotationCliff).
    pub const WRAP: u8 = 3;
    /// Winged: FlyMove, and its wall Turn.
    pub const FLY: u8 = 4;
    pub const FLY_TURN: u8 = 5;
}

// ---------------------------------------------------------------- Stingby

const STING_PATROL: f32 = 0.2;
/// Turns 16 px past its anchor: a 32-px patrol [obs].
const STING_SPAN: f32 = 16.0;
/// X acceleration 0.06, halved when big [obs].
const STING_ACCEL: f32 = 0.06;
const STING_SEE: f32 = 96.0;
const STING_BAND: f32 = 8.0;
/// Charge speed 0.5 [obs Chase enter 0x7100670640].
const STING_CHARGE: f32 = 0.5;
const STING_KEEP: f32 = 160.0;
const STING_GIVE_UP: i32 = 100;
/// Turn: the model yaws from one side (60°, the model's 3D World facing angle) to the other
/// before the alert [obs 0x7100670510 waits for the yaw turn]; Find plays the 37-frame alert at
/// rate 1 [obs 0x71006705e0].
const STING_FACE_DEG: f32 = 60.0;
const STING_ALERT_FRAMES: f32 = 37.0;

mod sting {
    pub const FLY: u8 = 1;
    pub const TURN: u8 = 2;
    pub const FIND: u8 = 3;
    pub const CHASE: u8 = 4;
}

// ---------------------------------------------------------------- Skipsqueak

const SKIP_JUMP: f32 = 2.7;
/// Take-off after more than 2 recovery attempts [obs].
const SKIP_JUMP_HIGH: f32 = 3.65;
/// Jump gravity: −0.103125 out of water, −0.20625 in water [obs 0x710075f710: the table at
/// 0x7101c8c448 is indexed by "not in water"].
const SKIP_GRAVITY: f32 = -0.103125;
const SKIP_GRAVITY_WATER: f32 = -0.20625;
const SKIP_RUN: f32 = 3.0;
const SKIP_RUN_ACCEL: f32 = 0.02;
const SKIP_BRAKE_ACCEL: f32 = 0.1;
const SKIP_CREEP: f32 = 0.2;
const SKIP_HOME: f32 = 1.0;
const SKIP_CRASH_VY: f32 = 1.875;
/// A crash bounces it back at 1.2 px/f; after landing ×0.35 [obs 0x710075ffa0, 0x71007601a0].
const SKIP_CRASH_BACK: f32 = 1.2;
const SKIP_CRASH_KEEP: f32 = 0.35;
/// 4 crashes into the same side: it gives up [obs 0x710075ffa0].
const SKIP_CRASHES: u8 = 4;
/// Gives up after more than 2 recovery attempts [obs].
const SKIP_TRIES: u8 = 2;
/// Wall jump: 1.0 px/f forward, the 17-frame JumpSign at rate 2, then 3.75 up (5.0 on the
/// third crash) [obs 0x7100760210, 0x7100760290].
const SKIP_WALL_VX: f32 = 1.0;
const SKIP_WALL_VY: f32 = 3.75;
const SKIP_WALL_VY_HIGH: f32 = 5.0;
/// Animations [obs model data]: crouch JumpStandingSign 17, landing JumpStandingEnd 13,
/// JumpSign 17, GiveUp 69 (played at 0.75).
const SKIP_CROUCH: f32 = 17.0;
const SKIP_LAND: f32 = 13.0;
const SKIP_SIGN: f32 = 17.0;
const SKIP_GIVE_UP: f32 = 69.0;
/// Its model faces 60° to either side: turning round to run home takes this yaw turn first
/// [obs 0x710075ffa0 waits for the turn].
const SKIP_FACE_DEG: f32 = 60.0;
/// Winged hover: y += 0.25·sin φ, φ += 1/72 turn per frame [obs].
const SKIP_HOVER: f32 = 0.25;

mod skip {
    pub const WAIT: u8 = 1;
    pub const FALL: u8 = 2;
    pub const CROUCH: u8 = 3;
    pub const JUMP: u8 = 4;
    pub const RETURN: u8 = 5;
    pub const CRASH: u8 = 6;
    pub const GIVE_UP: u8 = 7;
    pub const LAND: u8 = 8;
    pub const WALL_SIGN: u8 = 9;
    pub const WALL_JUMP: u8 = 10;
}

// ---------------------------------------------------------------- Hop-Chops

const HOP_VX: f32 = 0.5;
const HOP_VY: f32 = 1.2;
const HOP_GRAVITY: f32 = -0.1;
/// Patrol hops are timed by the looping Walk animation (35 frames at rate 0.7): it stands
/// outside frames 9–31 and takes off once inside 9–12 [obs 0x71006833f0, 0x7100682bc0,
/// 0x7100682c00, 0x7100683b90].
const HOP_WALK_FRAMES: f32 = 35.0;
const HOP_WALK_RATE: f32 = 0.7;
const HOP_WINDOW: (f32, f32) = (9.0, 31.0);
const HOP_TAKE_OFF: f32 = 12.0;
const HOP_SEE: (f32, f32) = (96.0, 64.0);
const HOP_KEEP: (f32, f32) = (128.0, 80.0);
const HOP_LOST_GRACE: i32 = 16;
const HOP_FIND_VY: f32 = 2.5;
/// The chase: the looping Run animation (20 frames at rate 0.8) moves it at 0.6 during frames
/// 2–15; at frame 2 it sets vy 0 and gravity −0.2 (the hop is only in the animation) [obs
/// 0x7100682e70, 0x7100682c40, 0x7100682c80].
const HOP_RUN_FRAMES: f32 = 20.0;
const HOP_RUN_RATE: f32 = 0.8;
const HOP_RUN_WINDOW: (f32, f32) = (2.0, 15.0);
const HOP_RUN_JUMP: f32 = 5.0;
const HOP_RUN_VX: f32 = 0.6;
const HOP_RUN_GRAVITY: f32 = -0.2;
const HOP_TURN_DECEL: f32 = 0.06;
const HOP_LOST_DECEL: f32 = 0.05;
/// Turn: Turn 35 frames at 0.9, with a 2.0 hop 7 frames in; Find 36 frames at 0.95; Lost plays
/// GiveUp, 114 frames at 0.9 [obs 0x7100682400/0x7100682480, 0x71006f9490, 0x71006f9640].
const HOP_TURN: (f32, f32) = (35.0, 0.9);
const HOP_TURN_HOP: f32 = 2.0;
const HOP_FIND: (f32, f32) = (36.0, 0.95);
const HOP_LOST: (f32, f32) = (114.0, 0.9);

mod hop {
    pub const WALK: u8 = 1;
    pub const TURN: u8 = 2;
    pub const FIND: u8 = 3;
    pub const RUN: u8 = 4;
    pub const LOST: u8 = 5;
}

// ---------------------------------------------------------------- Porcupuffer

const PUFF_HP: u8 = 4;
/// Speed multiplier by HP (index = HP; 0 unused) [obs].
const PUFF_M: [f32; 5] = [0.75, 0.75, 0.8, 0.9, 1.0];
const PUFF_SWIM: f32 = 1.25;
const PUFF_SWIM_ACCEL: f32 = 0.25;
const PUFF_STEER: f32 = 2.0;
/// Lunge when lined up within acos(0.9995) ≈ 1.8° and closer than 64 px (big 80) [obs].
const PUFF_LINE_UP: f32 = 1.8;
const PUFF_LUNGE_NEAR: f32 = 64.0;
const PUFF_LUNGE_STOP: i32 = 10;
const PUFF_LUNGE_FRAMES: i32 = 60;
const PUFF_LUNGE: f32 = 2.0;
const PUFF_FLOAT: f32 = 1.875;
const PUFF_FLOAT_ACCEL: f32 = 0.075;
const PUFF_EASE: f32 = 0.75;
const PUFF_BAND: f32 = 160.0;
const PUFF_JUMP_WINDUP: i32 = 30;
const PUFF_JUMP: f32 = 4.0;
const PUFF_JUMP_RAMP: f32 = 0.75;
const PUFF_DIVE_DEPTH: f32 = 16.0;
const PUFF_COOLDOWN: i32 = 60;
/// SwimTurn: the heading mirrors in 12 equal steps while the yaw swings 120° at 10°/frame, 12
/// frames [obs 0x71006612c0, 0x7100661340]; Swim then blocks the next turn for 5 frames [obs
/// Swim enter 0x7100660af0]; Recover plays DamageReset, 60 frames [obs 0x71006635d0].
const PUFF_TURN_FRAMES: i32 = 12;
const PUFF_TURN_COOLDOWN: i32 = 5;
const PUFF_RECOVER: i32 = 60;

mod puff {
    pub const FALL: u8 = 1;
    pub const SWIM: u8 = 2;
    pub const SWIM_TURN: u8 = 3;
    pub const LUNGE: u8 = 4;
    pub const FLOAT: u8 = 5;
    pub const SURFACE: u8 = 7;
    pub const SURFACE_JUMP: u8 = 8;
    pub const RECOVER: u8 = 9;
}

// ---------------------------------------------------------------- Bully

const BULLY_WAIT: i32 = 40;
const BULLY_WALK: f32 = 0.3;
const BULLY_CHASE: f32 = 0.8;
const BULLY_ACCEL: f32 = 0.025;
const BULLY_GIVE_UP: f32 = 256.0;
/// Turns round in the chase once the player is more than 28 px behind it [obs 0x71006478e0].
const BULLY_BEHIND: f32 = 28.0;
/// A wall stops the charge with a 1.0 px/f bounce [obs 0x71006478e0].
const BULLY_WALL_BOUNCE: f32 = 1.0;
/// Sight box: from its x to 128 px ahead (centre 64 ahead, half width 64), and the player's
/// body within ±(32 + 16·k) of y + 16·k (k = 1, big 2) [obs 0x71006490a0].
const BULLY_SEE_AHEAD: f32 = 64.0;
const BULLY_SEE_HALF: f32 = 32.0;
/// Find plays its 39-frame notice at rate 1 [obs 0x71006477a0].
const BULLY_FIND_FRAMES: f32 = 39.0;
/// Push-back speed by hit strength (table 0x7101d01204), ×0.75 when big [obs 0x7100647fe0].
const BULLY_PUSH: [f32; 3] = [1.0, 1.75, 2.5];
const BULLY_PUSH_DECAY: f32 = 0.05;
/// The push animations: pushed the way it faces PushFront 27, else PushBack 39; the push state
/// times out after 60 frames; 10 frames' recovery after it stops (15 in water) [obs].
const BULLY_PUSH_FRONT: i32 = 27;
const BULLY_PUSH_BACK: i32 = 39;
const BULLY_FLIP: i32 = 60;
const BULLY_FLIP_RECOVER: i32 = 10;
const BULLY_FLIP_RECOVER_WATER: i32 = 15;
/// Pushed slower than 1.75 towards a ledge it stops on the edge (HoldOut, 120 frames), then
/// Land (FlyHitLand, 59 frames) [obs 0x71006481f0, 0x7100648370, 0x7100647f10].
const BULLY_HOLD_SPEED: f32 = 1.75;
const BULLY_HOLD_OUT: i32 = 120;
const BULLY_LAND: i32 = 59;
/// Off a ledge: speed eases to 0 at 0.2; landing stuns it 40 frames (FallStan) [obs].
const BULLY_FALL_DECEL: f32 = 0.2;
const BULLY_FALL_STAN: i32 = 40;
/// Winged [obs 0x71006478e0, 0x71006484c0–0x7100648c00, 0x7100647dd0]: it lives on the ground
/// like the others, but its sight box reaches 256 + 16·k px up and down, and from the chase it
/// takes off (FlyChase) when it runs off a ledge or hits a wall with the player above it: it
/// rises at 0.5 px/f (vy eased by 0.05) for up to 300 frames while its forward speed decays by
/// 0.075 (the chase's pull leaves a 0.025 px/f creep); level with or above a player within
/// 256 px it lands toward them (FlyLand: 0.5 px/f forward, sinking by 0.015/f² for 36 frames,
/// 60 big, then falling); losing the player, a ceiling or the 300 frames end the climb and it
/// falls. Knocked off a ledge it flies back up (FlyReturn: vy eased to 0.5 by 0.075, 180 frames)
/// once its 12-frame fall (40 in water) is over, and lands when above the ledge it left.
const BULLY_FLY_SEE_HALF: f32 = 256.0;
const BULLY_FLY_CHASE: i32 = 300;
const BULLY_FLY_RISE: f32 = 0.5;
const BULLY_FLY_RISE_ACCEL: f32 = 0.05;
const BULLY_FLY_DECAY: f32 = 0.075;
const BULLY_FLY_LAND_SPEED: f32 = 0.5;
const BULLY_FLY_LAND_SINK: f32 = 0.015;
const BULLY_FLY_LAND: (i32, i32) = (36, 60);
const BULLY_FLY_RETURN: i32 = 180;
const BULLY_FLY_RETURN_ACCEL: f32 = 0.075;
const BULLY_FLY_RETURN_ACCEL_WATER: f32 = 0.035;
const BULLY_FALL_FRAMES: i32 = 12;
const BULLY_FALL_FRAMES_WATER: i32 = 40;

/// Hit strengths for `Actor::bully_push` (indices into the push table) [obs 0x7100649cb0: by the
/// attack type of the hit].
pub mod push {
    /// The player's body walking into it, fireballs: 1.0.
    pub const LIGHT: usize = 0;
    /// Stomps, spin jumps, shoe stomps, Yoshi stomps, tail and slide hits: 1.75.
    pub const STOMP: usize = 1;
    /// Ground pounds, shells, hammers, Koopa Troopa Car hits, the Cat dive: 2.5.
    pub const HEAVY: usize = 2;
}

mod bully {
    pub const WAIT: u8 = 1;
    pub const WALK: u8 = 2;
    pub const FIND: u8 = 3;
    pub const CHASE: u8 = 4;
    pub const FALL: u8 = 5;
    pub const FALL_STAN: u8 = 6;
    pub const LAND: u8 = 7;
    pub const FLIP: u8 = 8;
    pub const HOLD_OUT: u8 = 9;
    pub const FLY_CHASE: u8 = 11;
    pub const FLY_LAND: u8 = 12;
    pub const FLY_RETURN: u8 = 13;
}

// ---------------------------------------------------------------- Charvaargh

/// It waits 30 frames under the lava between leaps [obs Wait enter 0x710079ff40] and leaps at
/// the nearest player wherever they are (no sight range) [obs Wait exe 0x71007a00a0]; within
/// 128 px of the area's edge it leaps away from the edge.
const CHAR_LOCKOUT: i32 = 30;
const CHAR_EDGE: f32 = 128.0;
/// The warning: the 131-frame AttackSideSign (its silhouette swims in the background lava),
/// then 30 more frames [obs 0x71007a07c0, 0x71007a0910].
const CHAR_SIGN: i32 = 131 + 30;
/// The leap starts 32 px under the base level at 2.0 px/f; AttackStart eases the speed to 0.2
/// by 0.005/f over its 150 frames, Attack back towards 2.0 by 0.002/f over 190 frames, and
/// AttackEnd dives on [obs 0x71007a09b0, 0x71007a0c00, 0x71007a1620].
const CHAR_START_SPEED: f32 = 2.0;
const CHAR_START: (f32, f32, i32) = (0.2, 0.005, 150);
const CHAR_ATTACK: (f32, f32, i32) = (2.0, 0.002, 190);
/// The head's heading is the Head bone's forward axis in the leap animations, the root bone
/// pitching from straight up to straight down: pitch α (radians, −π/2 up … π/2 down) as
/// piecewise cubics in t = segment progress [obs model data, read through 0x71007a1620].
const CHAR_PITCH_START: [(f32, f32, [f32; 4]); 2] = [
    (
        0.0,
        84.0,
        [-1.570_783, 0.011_683_17, 0.961_865_2, -0.264_221_6],
    ),
    (
        84.0,
        150.0,
        [-0.861_456_2, 0.897_901_3, 0.092_790_18, -0.129_248_7],
    ),
];
const CHAR_PITCH_ATTACK: [(f32, f32, [f32; 4]); 2] = [
    (0.0, 149.0, [0.010_563_49, 1.591_044, 1.488_299, -1.519_084]),
    (149.0, 190.0, [1.570_774, 0.0, 0.0, 0.0]),
];
/// The body follows the head's path: 8 touch boxes 40×40 at these distances behind the head
/// (negative = ahead): head + 20, head, the 5 spine bones, the last one − 20 [obs 0x710079e290,
/// bone chain from the model]; the tail end (Spine5 − 48) ends the dive once it is 64 px under
/// the base level [obs 0x710079f890, 0x71007a0cf0].
const CHAR_SEGMENTS: [f32; 8] = [-20.0, 0.0, 19.95, 46.55, 73.15, 99.75, 130.34, 150.34];
const CHAR_TAIL: f32 = 130.34 + 48.0;
const CHAR_DIVE_DEPTH: f32 = 64.0;
/// Its box is 32 px either side of the head; within that of the area's edge it is moved 64 px
/// inside [obs Wait enter 0x710079ff40].
const CHAR_HALF: f32 = 32.0;
const CHAR_EDGE_IN: f32 = 64.0;
/// Turn-taking: one Charvaargh per area holds the turn from its wait until its warning ends;
/// another one's wait is held at 10 frames meanwhile [obs 0x71007a00a0, released 0x71007a0970].
const CHAR_TURN_HOLD: i32 = 10;
/// Spacing: a side leap starts at least 48 px from the other Charvaarghs' last side-leap spots,
/// a vertical one 56 px from their vertical spots [obs 0x71007a1d60, 0x71007a00a0].
const CHAR_SPACE_SIDE: f32 = 48.0;
const CHAR_SPACE_VERT: f32 = 56.0;
/// The vertical leap out of the background [obs 0x71007a00a0, 0x71007a07c0, 0x71007a09b0]: in the
/// last 3 frames of the wait it aims at the nearest player's body centre (in the main area only
/// when that is ≥ 88 px from the area's left edge and ≥ 160 from its right) and probes 4 columns
/// 16 px apart (x − 24 … x + 24), each from 48 px above to 48 px below base + 150, for terrain;
/// when the wait ends with the player within 96 px of it and nothing found, it leaps there.
const CHAR_VERT_REACH: f32 = 96.0;
const CHAR_VERT_LEFT: f32 = 88.0;
const CHAR_VERT_RIGHT: f32 = 160.0;
const CHAR_VERT_APEX: f32 = 150.0;
const CHAR_VERT_PROBE: f32 = 48.0;
/// Its warning: AttackFrontSign / AttackRearSign (50 frames) and 5 more [obs 0x71007a0910].
const CHAR_SIGN_VERT: i32 = 50 + 5;
/// Depth: the vertical leap rises from behind the course (z −108) toward the viewer, or from in
/// front of it (z +24, the model turned round) into the background, at random; its body touches
/// only while within z −40 … 0 [obs 0x71007a07c0, 0x710079e290].
const CHAR_Z_BACK: f32 = -108.0;
const CHAR_Z_FRONT: f32 = 24.0;
const CHAR_Z_TOUCH: (f32, f32) = (-40.0, 0.0);
/// The vertical leap is root motion: the actor rides the model's JointRoot bone, posed by
/// AttackFront/RearStart (150 frames), AttackFront/Rear (190) and the End dive [obs 0x710079e290
/// with +0x4B8 bit 7]. Head height and depth above the leap's anchor (base − 32), every 10
/// frames from the leap's start [obs model data, sampled; linear between knots ≤ 0.6 px off].
const CHAR_VERT_FRONT: [(f32, f32); 39] = [
    (0.0, 0.0),
    (23.9, 0.2),
    (46.0, 0.6),
    (66.2, 1.4),
    (84.7, 2.5),
    (101.3, 3.9),
    (116.1, 5.6),
    (129.3, 8.5),
    (140.7, 13.7),
    (150.4, 20.9),
    (158.5, 29.9),
    (164.9, 40.4),
    (169.8, 52.2),
    (173.1, 64.9),
    (174.8, 78.4),
    (175.0, 91.9),
    (173.8, 104.6),
    (171.4, 116.1),
    (167.7, 125.9),
    (162.6, 133.5),
    (156.1, 138.4),
    (148.1, 140.2),
    (138.5, 140.1),
    (127.4, 140.1),
    (114.6, 140.1),
    (100.1, 140.1),
    (83.8, 140.1),
    (65.7, 140.1),
    (45.8, 140.1),
    (23.9, 140.1),
    (0.0, 140.1),
    (-26.7, 140.1),
    (-57.1, 140.1),
    (-91.2, 140.1),
    (-129.1, 140.1),
    (-171.0, 140.1),
    (-216.9, 140.1),
    (-267.0, 140.1),
    (-321.3, 140.1),
];
const CHAR_VERT_REAR: [(f32, f32); 39] = [
    (0.0, -15.7),
    (24.6, -15.6),
    (48.3, -15.1),
    (70.9, -14.3),
    (92.1, -13.3),
    (111.6, -11.9),
    (129.1, -10.1),
    (144.2, -7.3),
    (156.7, -2.5),
    (166.3, 4.2),
    (172.6, 12.5),
    (175.4, 22.1),
    (175.5, 32.8),
    (174.9, 44.4),
    (173.9, 56.6),
    (172.2, 68.3),
    (169.8, 78.8),
    (166.6, 88.1),
    (162.4, 96.3),
    (157.1, 103.3),
    (150.7, 109.3),
    (143.1, 114.3),
    (134.0, 118.2),
    (123.5, 121.1),
    (111.5, 123.1),
    (97.7, 124.2),
    (82.2, 124.4),
    (64.7, 124.4),
    (45.3, 124.4),
    (23.7, 124.4),
    (0.0, 124.4),
    (-26.7, 124.4),
    (-57.0, 124.4),
    (-91.1, 124.4),
    (-129.0, 124.4),
    (-170.9, 124.4),
    (-216.8, 124.4),
    (-266.8, 124.4),
    (-321.1, 124.4),
];
/// The model root sits 11.97 px left (front) / 9.31 right (rear) of the anchor and is yawed 10°
/// (−10° rear), so the depth also drifts x by tan 10° per px [obs model data, AllRoot].
const CHAR_VERT_X0: (f32, f32) = (-11.97, 9.31);
const CHAR_VERT_DRIFT: f32 = 0.176_327;
/// The chained leap [obs AttackEnd 0x71007a0cf0]: a dive that ends with its spot (± 16 px) outside
/// every player's view starts a side leap at once from within 64 px outside the nearest player's
/// view, facing the view's centre.
const CHAR_CHAIN_HALF: f32 = 16.0;
const CHAR_CHAIN_MARGIN: f32 = 64.0;

mod chr {
    pub const FALL: u8 = 1;
    pub const WAIT: u8 = 2;
    pub const SIGN: u8 = 3;
    pub const START: u8 = 4;
    pub const ATTACK: u8 = 5;
    pub const END: u8 = 6;

    /// `W3State::flags` bits for the Charvaargh.
    /// The vertical leap is still possible this wait (cleared by a failed test).
    pub const VERT_OK: u8 = 1;
    /// Aimed at the player: probing for terrain.
    pub const PROBING: u8 = 2;
    /// This leap is the vertical one.
    pub const VERTICAL: u8 = 4;
    /// The vertical leap goes into the background (from in front).
    pub const REAR: u8 = 8;

    /// `W3State::recs` bits: its side / vertical leap spot is on record, it holds the turn.
    pub const REC_SIDE: u8 = 1;
    pub const REC_VERT: u8 = 2;
    pub const TURN: u8 = 4;
}

/// Spacing of a side leap at `x` from the other Charvaarghs' side spots (in placement order),
/// leaping left when `left` [obs 0x71007a1d60]: the first spot nearer than 48 px moves it just
/// clear, to the right — to the left instead when it leaps left and an earlier spot was clear of
/// it; a second spot still too near after that reverses the move. Returns the shift.
fn charvaargh_space_side(peers: &[f32], x: f32, left: bool) -> f32 {
    let (mut cur, mut shift, mut clear) = (x, 0.0f32, 0.0f32);
    for &p in peers {
        let d = p - cur;
        if d.abs() >= CHAR_SPACE_SIDE {
            clear = d;
            continue;
        }
        if shift != 0.0 {
            return -shift;
        }
        let push = CHAR_SPACE_SIDE - d.abs();
        shift = if left && clear != 0.0 { -push } else { push };
        cur += shift;
    }
    shift
}

/// Spacing of a vertical leap aimed at `x` from the other Charvaarghs' vertical spots [obs
/// 0x71007a00a0]: the first spot nearer than 56 px moves it just clear, away from that spot — or,
/// when an earlier spot was clear of it, 56 px past the near one on the side away from that
/// earlier spot; a second near spot puts it 56 px beyond the previous spot instead.
fn charvaargh_space_vert(peers: &[f32], x: f32) -> f32 {
    let (mut cur, mut shift, mut clear) = (x, 0.0f32, 0.0f32);
    let mut prev = 0.0f32;
    for &p in peers {
        let d = p - cur;
        let last = prev;
        prev = p;
        if d.abs() >= CHAR_SPACE_VERT {
            clear = d;
            continue;
        }
        if shift != 0.0 {
            let side = if shift > 0.0 {
                -CHAR_SPACE_VERT
            } else {
                CHAR_SPACE_VERT
            };
            return last + side - x;
        }
        let push = CHAR_SPACE_VERT - d.abs();
        shift = if d > 0.0 { -push } else { push };
        if clear != 0.0 {
            let far = d.abs() + CHAR_SPACE_VERT;
            return if clear > 0.0 { -far } else { far };
        }
        cur += shift;
    }
    shift
}

/// The vertical leap's head offset from its anchor at leap frame `t`: (x, y, depth), the model
/// turned round for the rear leap.
fn charvaargh_vert_offset(rear: bool, t: f32) -> (f32, f32, f32) {
    let knots = if rear {
        &CHAR_VERT_REAR
    } else {
        &CHAR_VERT_FRONT
    };
    let k = (t / 10.0).max(0.0);
    let i = (k as usize).min(knots.len() - 2);
    let f = k - i as f32;
    let (a, b) = (knots[i], knots[i + 1]);
    let (y, z) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
    if rear {
        (-(CHAR_VERT_X0.1 - CHAR_VERT_DRIFT * z), y, -z)
    } else {
        (CHAR_VERT_X0.0 + CHAR_VERT_DRIFT * z, y, z)
    }
}

// ---------------------------------------------------------------- Piranha Creeper

/// The head rides the end of the stalk piece that is growing: each piece's Appear animation
/// moves the head 32 px over 12 frames (from 20 px before the piece's centre to 12 past it) and
/// ends at frame 11 [obs model data; 0x7100722e30 advances a piece when its animation ends].
pub const CREEP_CELL: f32 = 32.0;
const CREEP_ENTRY: f32 = 20.0;
const CREEP_EXIT: f32 = 12.0;
const CREEP_PIECE_FRAMES: f32 = 12.0;
const CREEP_PIECE_END: f32 = 11.0;
/// The first piece starts at frame 3; the base piece stops back at frame 3 [obs 0x7100722d30,
/// 0x71007245d0].
const CREEP_BASE_FRAME: f32 = 3.0;
/// Play rate 0.6 (−0.6 back), eased by 0.1 per frame; on the last piece a tracker approaches
/// the end frame by 10 % of the gap (≥ 0.05) and the rate follows it (≥ 0.01) [obs].
const CREEP_RATE: f32 = 0.6;
const CREEP_RATE_STEP: f32 = 0.1;
const CREEP_BRAKE: f32 = 0.1;
const CREEP_CRAWL: f32 = 0.05;
const CREEP_MIN_RATE: f32 = 0.01;
/// Waits 3 frames at the end before retracting and 10 at the base before creeping out again
/// [obs Wait 0x7100723930].
const CREEP_WAIT_END: i32 = 3;
const CREEP_WAIT_BASE: i32 = 10;
/// Trampled: the head retracts 2 pieces (4 for a ground pound) at −0.6 while the 59-frame
/// Trampled animation plays, then 130 frames' TrampledLoop [obs 0x7100723d40, 0x7100723f40,
/// 0x71007241b0, 0x710071e460].
const CREEP_TRAMPLED: i32 = 59;
const CREEP_DAMAGE_WAIT: i32 = 130;
const CREEP_RETRACT: i32 = 2;
const CREEP_RETRACT_POUND: i32 = 4;
/// Any hit the head takes shuts its touch callback for 16 frames: no hurt, no new hit [obs
/// 0x710071e460, +0xDCFC].
const CREEP_COOLDOWN: i32 = 16;
/// PreStop: Move checks the head's terrain sensor (a ±12 px box 14 px ahead of the head) on the
/// side the growing piece leads to; met terrain freezes the stalk for 30 frames, then it
/// retracts (the alternate sleeps there) [obs 0x7100722e30, 0x7100723a40, 0x7100723ae0].
const CREEP_PRE_STOP: i32 = 30;
const CREEP_SENSOR_AHEAD: f32 = 14.0;
const CREEP_SENSOR_HALF: f32 = 12.0;
/// The sensor's wall probes span 20 px, its floor and ceiling probes 16 [obs BgCheck at +0xCCB0].
const CREEP_SENSOR_SPAN: (f32, f32) = (10.0, 8.0);
/// Head box: the profile's 32×32 [obs 0x7101d030dc].
const CREEP_HEAD: f32 = 16.0;
/// The corner piece's head path in (along entry, along exit) px, as cubics over frames 0–6 and
/// 6–12 [obs model data].
const CREEP_CORNER_A: [[f32; 4]; 2] = [
    [-20.0, 14.575_415, 0.185_934_7, -1.257_951],
    [-6.496_597, 11.173_877, -2.857_968, -1.819_313],
];
const CREEP_CORNER_B: [[f32; 4]; 2] = [
    [0.000_182_6, 0.0, -0.424_670_2, 1.224_563],
    [0.800_075, 2.824_764, 13.615_625, -5.240_231],
];

mod creep {
    pub const MOVE: u8 = 1;
    pub const MOVE_BACK: u8 = 2;
    pub const WAIT: u8 = 3;
    pub const PRE_STOP: u8 = 4;
    pub const SLEEP: u8 = 5;
    pub const DAMAGE: u8 = 6;
    pub const DAMAGE_WAIT: u8 = 7;
}

/// Direction (index into `DIRS`) a Piranha Creeper path node leads to: the game's node-code
/// table 0x7101cff340 [obs]. Codes: 1 left, 2 right, 3 down, 4 up; corners 5 left→down, 6 down→
/// left, 7 left→up, 8 up→left, 9 right→down, 10 down→right, 11 right→up, 12 up→right; 13–16
/// end caps (pointing back along the path).
pub fn creeper_out(code: u8) -> u8 {
    match code {
        1 | 6 | 8 | 13 => 3,
        2 | 10 | 12 | 14 => 1,
        3 | 5 | 9 | 15 => 0,
        _ => 2,
    }
}

/// Direction the path enters a node from (the way the head moves into it).
fn creeper_in(code: u8) -> u8 {
    match code {
        1 | 5 | 7 => 3,
        2 | 9 | 11 => 1,
        3 | 6 | 10 => 0,
        4 | 8 | 12 => 2,
        _ => opposite(creeper_out(code)),
    }
}

/// A Piranha Creeper path node is a corner piece.
pub fn creeper_corner(code: u8) -> bool {
    (5..=12).contains(&code)
}

/// Pieces of a creeper path: (entry, exit) direction per piece. The first piece leads out of
/// the base the way the first node enters; then one piece per node but the last (its end cap)
/// [obs builder 0x710071f980].
fn creeper_pieces(codes: &[u8]) -> Vec<(u8, u8)> {
    let Some(&first) = codes.first() else {
        return Vec::new();
    };
    let d0 = creeper_in(first);
    let mut out = vec![(d0, d0)];
    for &c in codes.iter().take(codes.len().saturating_sub(1)) {
        let prev = out.last().map_or(d0, |p| p.1);
        out.push((prev, creeper_out(c)));
    }
    out
}

fn cubic(c: &[f32; 4], t: f32) -> f32 {
    ((c[3] * t + c[2]) * t + c[1]) * t + c[0]
}

fn pitch(curve: &[(f32, f32, [f32; 4]); 2], f: f32) -> f32 {
    let (f0, f1, c) = if f < curve[0].1 { curve[0] } else { curve[1] };
    let t = ((f - f0) / (f1 - f0)).clamp(0.0, 1.0);
    cubic(&c, t)
}

impl Kind {
    /// One of the 3D World enemies of this module.
    pub fn is_w3(self) -> bool {
        matches!(
            self,
            Kind::AntTrooper { .. }
                | Kind::Stingby
                | Kind::Skipsqueak { .. }
                | Kind::HopChops
                | Kind::Porcupuffer
                | Kind::Bully
                | Kind::Charvaargh
                | Kind::PiranhaCreeper { .. }
        )
    }

    /// The 3D World enemies that take the generic enemy paths (fireballs, shells, POW, Star).
    /// Charvaargh does not: it has no EnemyDB row and its own touch callback reacts to a Super
    /// Star only [obs 0x710079c5f0].
    pub fn w3_enemy(self) -> bool {
        self.is_w3() && self != Kind::Charvaargh
    }

    /// Survives in lava and poison: Charvaargh lives there; Porcupuffer's lava reaction is
    /// Invalid [obs EnemyDB].
    pub fn liquid_proof(self) -> bool {
        matches!(self, Kind::Charvaargh | Kind::Porcupuffer)
    }

    /// Its model's origin is the body centre, not the feet (Stingby, Porcupuffer, Charvaargh,
    /// the creeper's head) [obs from the models' bounds].
    pub fn w3_centred(self) -> bool {
        matches!(
            self,
            Kind::Stingby | Kind::Porcupuffer | Kind::Charvaargh | Kind::PiranhaCreeper { .. }
        )
    }
}

/// A sprite choice: model, texture (animation frame), width, height, mirror.
pub type W3Look = (&'static str, &'static str, f32, f32, bool);

impl Actor {
    /// Half width and height of the body box.
    pub(super) fn w3_size(&self) -> (f32, f32) {
        let s = if self.big { 2.0 } else { 1.0 };
        let (hw, h) = match self.kind {
            Kind::Porcupuffer | Kind::Bully => (16.0, 32.0),
            Kind::Charvaargh => (32.0, 64.0),
            Kind::PiranhaCreeper { .. } => (CREEP_HEAD, 2.0 * CREEP_HEAD),
            _ => (8.0, 16.0),
        };
        (hw * s, h * s)
    }

    fn size_k(&self) -> f32 {
        if self.big { 2.0 } else { 1.0 }
    }

    fn toward_player(&self) -> i32 {
        if self.target.0 < self.x { -1 } else { 1 }
    }

    /// One frame of a 3D World enemy (from `step_normal`).
    pub(super) fn step_w3(&mut self, level: &Level) -> Option<Actor> {
        match self.kind {
            Kind::AntTrooper { .. } => self.step_ant(level),
            Kind::Stingby => self.step_stingby(level),
            Kind::Skipsqueak { .. } => self.step_skipsqueak(level),
            Kind::HopChops => self.step_hop_chops(level),
            Kind::Porcupuffer => self.step_porcupuffer(level),
            Kind::Bully => self.step_bully(level),
            Kind::Charvaargh => self.step_charvaargh(level),
            Kind::PiranhaCreeper { path, sleeps } => self.step_creeper(level, path, sleeps),
            _ => {}
        }
        None
    }

    // ------------------------------------------------------------ Ant Trooper

    /// Ant Trooper (96-ant-trooper.md, the Spike Top crawler): walks the terrain contour. On the
    /// floor it walks with gravity; a wall ahead is climbed (the wall becomes its surface), and
    /// at a ledge it wraps round the corner at 6°/frame (big 4°) and carries on down the face,
    /// along ceilings and back up. The body is a square box, so turning does not change it.
    /// Winged: flies level, bobbing (see `ANT_FLY`).
    fn step_ant(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = ant::FLOOR;
        }
        if self.wings {
            self.ant_fly(level);
            return;
        }
        if matches!(self.phase, ant::FLY | ant::FLY_TURN) {
            self.phase = ant::FLOOR;
        }
        let h = 8.0 * self.size_k();
        match self.phase {
            ant::CRAWL => self.ant_crawl(level, h),
            ant::WRAP => self.ant_wrap(h),
            _ => self.ant_floor(level, h),
        }
    }

    fn ant_floor(&mut self, level: &Level, h: f32) {
        let dir = self.dir as f32;
        self.w3.down = 0;
        self.w3.ahead = if self.dir > 0 { 1 } else { 3 };
        if self.on_ground {
            self.vx = ANT_WALK * dir;
            // a ledge: the floor ends under the centre and the block behind is solid
            let ahead = level.cell_at(self.x + dir * 0.1, self.y - 1.0);
            let behind = level.cell_at(self.x - dir * 0.1, self.y - 1.0);
            let slope = level
                .slope_at(self.x + dir * 0.1)
                .is_some_and(|(s, _)| s >= self.y - 8.0);
            if ahead == Cell::Empty && behind.is_solid() && !slope {
                let edge = (self.x / TILE).round() * TILE;
                self.ant_start_wrap((edge, self.y));
                return;
            }
        } else {
            self.vx = self.vx.clamp(-ANT_WALK, ANT_WALK);
        }
        self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
        self.collide(level);
        if self.wall && self.on_ground {
            // a real wall (not the area edge) is climbed; anything else turns it round
            let wall = level
                .cell_at(self.x + dir * (h + 1.0), self.y + h)
                .is_solid();
            if wall {
                self.w3.down = self.w3.ahead;
                self.w3.ahead = 2;
                self.phase = ant::CRAWL;
                self.vx = 0.0;
                self.vy = 0.0;
            } else if self.turn_lock == 0 {
                self.turn();
            }
        }
    }

    fn ant_start_wrap(&mut self, corner: (f32, f32)) {
        self.w3.corner = corner;
        self.phase = ant::WRAP;
        self.timer = 0;
        self.vx = 0.0;
        self.vy = 0.0;
        self.on_ground = false;
    }

    /// Wrapping round a corner: the box centre swings a quarter circle round it, from over the
    /// old surface to beside the new one.
    fn ant_wrap(&mut self, h: f32) {
        let rate = if self.big { ANT_TURN_BIG } else { ANT_TURN };
        self.timer += 1;
        let a = (self.timer as f32 * rate).min(90.0).to_radians();
        let g = DIRS[self.w3.down as usize];
        let t = DIRS[self.w3.ahead as usize];
        let (kx, ky) = self.w3.corner;
        let cx = kx + h * (-g.0 * a.dcos() + t.0 * a.dsin());
        let cy = ky + h * (-g.1 * a.dcos() + t.1 * a.dsin());
        self.x = cx;
        self.y = cy - h;
        if self.timer as f32 * rate >= 90.0 {
            let (down, ahead) = (opposite(self.w3.ahead), self.w3.down);
            self.ant_settle(down, ahead);
        }
    }

    /// Now on the surface on side `down`, walking toward `ahead`.
    fn ant_settle(&mut self, down: u8, ahead: u8) {
        self.w3.down = down;
        self.w3.ahead = ahead;
        let t = DIRS[ahead as usize];
        if t.0 != 0.0 {
            self.dir = t.0 as i32;
        }
        if down == 0 {
            self.phase = ant::FLOOR;
            self.on_ground = true;
            self.vx = ANT_WALK * self.dir as f32;
        } else {
            self.phase = ant::CRAWL;
        }
        self.vy = 0.0;
    }

    /// Crawling along a wall or the ceiling: a solid cell ahead becomes the new surface; where
    /// the surface ends it wraps round; with nothing under it at all, it falls.
    fn ant_crawl(&mut self, level: &Level, h: f32) {
        let solid = |x: f32, y: f32| level.cell_at(x, y).is_solid();
        let g = DIRS[self.w3.down as usize];
        let t = DIRS[self.w3.ahead as usize];
        let mut cx = self.x + ANT_WALK * t.0;
        let mut cy = self.y + h + ANT_WALK * t.1;
        // the leading face: points across it, 1 px in from the surface side
        let (lx, ly) = (cx + (h + 0.01) * t.0, cy + (h + 0.01) * t.1);
        let blocked = [-(h - 1.0), 0.0, h - 1.0]
            .iter()
            .any(|&k| solid(lx + k * g.0, ly + k * g.1));
        if blocked {
            let snap = |v: f32, d: f32| {
                if d > 0.0 {
                    (v / TILE).floor() * TILE
                } else {
                    ((v / TILE).floor() + 1.0) * TILE
                }
            };
            if t.0 != 0.0 {
                cx = snap(lx, t.0) - h * t.0;
            } else {
                cy = snap(ly, t.1) - h * t.1;
            }
            self.x = cx;
            self.y = cy - h;
            let (down, ahead) = (self.w3.ahead, opposite(self.w3.down));
            self.ant_settle(down, ahead);
            return;
        }
        let probe = |dt: f32| {
            solid(
                cx + (h + 0.5) * g.0 + dt * t.0,
                cy + (h + 0.5) * g.1 + dt * t.1,
            )
        };
        self.x = cx;
        self.y = cy - h;
        if probe(0.1) {
            return;
        }
        if probe(-0.1) {
            // the surface ends here: the corner is on the cell edge nearest the centre
            let (mut kx, mut ky) = (cx + h * g.0, cy + h * g.1);
            if t.0 != 0.0 {
                kx = (cx / TILE).round() * TILE;
            } else {
                ky = (cy / TILE).round() * TILE;
            }
            self.ant_start_wrap((kx, ky));
        } else {
            // nothing to cling to: fall (InAir)
            self.phase = ant::FLOOR;
            self.on_ground = false;
            self.w3.down = 0;
        }
    }

    /// Winged flight: FlyMove and the wall Turn.
    fn ant_fly(&mut self, level: &Level) {
        if !matches!(self.phase, ant::FLY | ant::FLY_TURN) {
            self.phase = ant::FLY;
            self.w3.anchor = self.y + ANT_FLY_ABOVE;
            self.vy = 0.0;
        }
        let step = if self.y >= self.w3.anchor {
            -ANT_FLY_BOB
        } else {
            ANT_FLY_BOB
        };
        self.vy = (self.vy + step).clamp(-ANT_FLY_BOB_MAX, ANT_FLY_BOB_MAX);
        if self.phase == ant::FLY_TURN {
            self.timer -= 1;
            if self.timer <= 0 {
                self.phase = ant::FLY;
            }
        } else {
            let accel = (ANT_FLY_ACCEL_K * self.vx.abs()).max(ANT_FLY_ACCEL);
            self.vx = ease(self.vx, ANT_FLY * self.dir as f32, accel);
        }
        let rising = self.vy > 0.0;
        self.collide(level);
        if rising && self.vy <= 0.0 && !self.on_ground {
            self.vy = -ANT_FLY_BOB_MAX;
        }
        if self.wall && self.phase == ant::FLY {
            self.dir = -self.dir;
            self.vx = self.vx.abs() * self.dir as f32;
            self.phase = ant::FLY_TURN;
            self.timer = ANT_FLY_TURN_LOCK.max(yaw_frames(2.0 * ANT_DIR_DEG));
        }
    }

    /// The side the Ant Trooper's surface is on (0 floor, 1 right wall, 2 ceiling, 3 left wall);
    /// 0 in flight too.
    pub fn ant_side(&self) -> u8 {
        if matches!(self.phase, ant::FLOOR | ant::FLY | ant::FLY_TURN) {
            0
        } else {
            self.w3.down
        }
    }

    // ------------------------------------------------------------ Stingby

    /// The player's feet or head is within ±8·s of the Stingby's mid-height (s = 1, big 2).
    fn stingby_level(&self) -> bool {
        let s = self.size_k();
        let mid = self.y + 8.0 * s;
        let (_, py) = self.target;
        let head = py + self.w3.player_h;
        (py - mid).abs() <= STING_BAND * s || (head - mid).abs() <= STING_BAND * s
    }

    /// Stingby (65-stingby.md): flies with no gravity, patrolling ±16 px round its anchor at
    /// 0.2 px/f. A player within 96 px and level with it is charged: facing away, it turns
    /// round first (its model yaws across, 15 frames), plays its 37-frame alert, then flies at
    /// them at 0.5 px/f; it gives up after the player has been beyond 160 px or behind it for
    /// 100 frames. Walls turn it round.
    fn step_stingby(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = sting::FLY;
            self.w3.anchor = self.x;
            self.wings = false; // it already flies (action_wing Default) [obs]
        }
        let accel = if self.big {
            STING_ACCEL / 2.0
        } else {
            STING_ACCEL
        };
        let dx = self.target.0 - self.x;
        let facing = dx * self.dir as f32 >= 0.0;
        let target = match self.phase {
            sting::FLY => {
                if (self.x - self.w3.anchor) * self.dir as f32 > STING_SPAN {
                    self.dir = -self.dir;
                }
                if dx.abs() <= STING_SEE && self.stingby_level() {
                    if facing {
                        self.phase = sting::FIND;
                        self.timer = anim_len(STING_ALERT_FRAMES, 1.0);
                    } else {
                        // Turn enter flips the facing; the state waits for the yaw turn
                        self.dir = -self.dir;
                        self.phase = sting::TURN;
                        self.timer = yaw_frames(2.0 * STING_FACE_DEG);
                    }
                }
                STING_PATROL
            }
            sting::TURN => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = sting::FIND;
                    self.timer = anim_len(STING_ALERT_FRAMES, 1.0);
                }
                0.0
            }
            sting::FIND => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = sting::CHASE;
                    self.w3.clock = STING_GIVE_UP;
                }
                0.0
            }
            _ => {
                if dx.abs() > STING_KEEP || !facing {
                    self.w3.clock -= 1;
                }
                if self.w3.clock <= 0 {
                    self.phase = sting::FLY;
                    self.w3.anchor = self.x;
                }
                STING_CHARGE
            }
        };
        self.vx = ease(self.vx, target * self.dir as f32, accel);
        self.vy = 0.0;
        self.collide(level);
        if self.wall {
            self.dir = -self.dir;
            self.vx = 0.0;
            if self.phase == sting::FLY {
                self.w3.anchor = self.x + STING_SPAN * self.dir as f32;
            }
        }
    }

    /// Stingby is charging (or about to).
    pub fn stingby_charging(&self) -> bool {
        matches!(self.phase, sting::FIND | sting::CHASE)
    }

    // ------------------------------------------------------------ Skipsqueak

    fn skip_crash_slot(&self) -> usize {
        if self.dir > 0 { 0 } else { 1 }
    }

    /// Skipsqueak (51-skipsqueak.md): keeps to its home spot (x and floor height). Whenever the
    /// player launches a jump it crouches (17 frames) and jumps straight up (2.7, gravity
    /// −0.103125; −0.20625 in water), landing with a 13-frame recovery. Displaced, it runs home
    /// (≤ 3.0 px/f, braking for the last v²/0.1 px); below its home floor it jumps back up
    /// (3.65 on the third try) and after three tries makes the spot it is on its new home.
    /// Running into a wall it bounces back (Crash) the first time and tries a wall jump after
    /// that; 4 crashes into the same side and it gives up too. Winged: it hovers, bobbing.
    fn step_skipsqueak(&mut self, level: &Level) {
        if self.phase == 0 {
            self.phase = skip::FALL;
            self.home_x = self.x;
        }
        if self.wings {
            self.w3.travel += 5.0; // 1/72 turn per frame
            self.y += SKIP_HOVER * self.w3.travel.to_radians().dsin();
            self.vx = 0.0;
            self.dir = self.toward_player();
            return;
        }
        let wet = self.w3.liquid.is_some_and(|(s, w)| w && self.y + 8.0 < s) || self.w3.flooded;
        let jump_g = if wet {
            SKIP_GRAVITY_WATER
        } else {
            SKIP_GRAVITY
        };
        match self.phase {
            skip::WAIT => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                if !self.on_ground {
                    self.phase = skip::FALL;
                    return;
                }
                let below = self.y < self.home_y - SKIP_HOME;
                if below && self.w3.tries > SKIP_TRIES {
                    self.skip_give_up();
                    return;
                }
                if (self.x - self.home_x).abs() > SKIP_HOME {
                    self.skip_return();
                    return;
                }
                self.dir = self.toward_player();
                if self.w3.player_rising || below {
                    self.skip_crouch(below);
                    return;
                }
                // at home: the floor it stands on is home (it follows a moving floor)
                self.home_y = self.y;
                self.w3.tries = 0;
            }
            skip::CROUCH => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 && self.on_ground {
                    // straight up: the take-off sets no horizontal speed [obs 0x710075f710]
                    self.vy = if self.w3.tries > SKIP_TRIES {
                        SKIP_JUMP_HIGH
                    } else {
                        SKIP_JUMP
                    };
                    self.on_ground = false;
                    self.phase = skip::JUMP;
                }
            }
            skip::JUMP | skip::WALL_JUMP => {
                if self.phase == skip::WALL_JUMP {
                    // the forward speed holds through the jump
                    self.vx = SKIP_WALL_VX * self.dir as f32;
                }
                self.w3_fall(
                    level,
                    if self.phase == skip::JUMP {
                        jump_g
                    } else {
                        ENEMY_GRAVITY
                    },
                );
                if self.on_ground && self.vy <= 0.0 {
                    self.vx = 0.0;
                    if self.phase == skip::JUMP {
                        self.phase = skip::LAND;
                        self.timer = anim_len(SKIP_LAND, 1.0);
                    } else {
                        // a wall jump that cleared the wall takes one crash back
                        let k = self.skip_crash_slot();
                        if !self.wall {
                            self.w3.crashes[k] = self.w3.crashes[k].saturating_sub(1);
                        }
                        self.skip_return();
                    }
                }
            }
            skip::LAND => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = skip::WAIT;
                }
            }
            skip::FALL => {
                self.w3_fall(level, ENEMY_GRAVITY);
                if self.on_ground && self.vy <= 0.0 {
                    self.vx = 0.0;
                    self.phase = skip::WAIT;
                }
            }
            skip::RETURN => {
                if self.timer > 0 {
                    // the model is still turning round
                    self.timer -= 1;
                    self.vx = 0.0;
                    self.w3_fall(level, ENEMY_GRAVITY);
                    return;
                }
                let d = self.home_x - self.x;
                if d.abs() < SKIP_HOME {
                    self.vx = 0.0;
                    self.phase = skip::WAIT;
                    self.w3_fall(level, ENEMY_GRAVITY);
                    return;
                }
                let dir = self.dir as f32;
                let v = self.vx * dir; // speed toward home
                let (target, accel) = if v > 0.0 && d.abs() <= v * v / SKIP_BRAKE_ACCEL {
                    (SKIP_CREEP, SKIP_BRAKE_ACCEL)
                } else {
                    (SKIP_RUN, SKIP_RUN_ACCEL)
                };
                self.vx = ease(self.vx, target * dir, accel);
                self.w3_fall(level, ENEMY_GRAVITY);
                if self.wall {
                    let k = self.skip_crash_slot();
                    let c = self.w3.crashes[k];
                    if c >= SKIP_CRASHES {
                        self.skip_give_up();
                    } else if c < 1 || !self.on_ground {
                        // Crash: bounce back
                        self.w3.crashes[k] += 1;
                        self.vx = -SKIP_CRASH_BACK * dir;
                        self.vy = SKIP_CRASH_VY;
                        self.on_ground = false;
                        self.phase = skip::CRASH;
                    } else {
                        // WallJump: the jump sign at double speed, then up
                        self.vx = SKIP_WALL_VX * dir;
                        self.phase = skip::WALL_SIGN;
                        self.timer = anim_len(SKIP_SIGN, 2.0);
                    }
                }
            }
            skip::CRASH => {
                self.w3_fall(level, ENEMY_GRAVITY);
                if self.vy <= 0.0 && self.on_ground {
                    self.vx *= SKIP_CRASH_KEEP;
                    self.phase = skip::RETURN;
                    self.timer = 0;
                }
            }
            skip::WALL_SIGN => {
                self.vx = SKIP_WALL_VX * self.dir as f32;
                self.w3_fall(level, ENEMY_GRAVITY);
                if !self.on_ground {
                    self.skip_return();
                    return;
                }
                self.timer -= 1;
                if self.timer <= 0 {
                    let k = self.skip_crash_slot();
                    self.vy = if self.w3.crashes[k] == 3 {
                        SKIP_WALL_VY_HIGH
                    } else {
                        SKIP_WALL_VY
                    };
                    self.on_ground = false;
                    self.phase = skip::WALL_JUMP;
                }
            }
            _ => {
                // GiveUp: sulks, then stays where it is: this spot became its home
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = skip::WAIT;
                }
            }
        }
    }

    /// Jump: the crouch; a jump from below home counts as a recovery attempt [obs Jump enter
    /// 0x710075f690].
    fn skip_crouch(&mut self, below: bool) {
        self.w3.tries = if below { self.w3.tries + 1 } else { 0 };
        self.phase = skip::CROUCH;
        self.timer = anim_len(SKIP_CROUCH, 1.0);
    }

    /// Return: faces home (turning the model round first) and runs [obs 0x710075fee0].
    fn skip_return(&mut self) {
        let dir = if self.home_x > self.x { 1 } else { -1 };
        self.timer = if dir != self.dir {
            yaw_frames(2.0 * SKIP_FACE_DEG)
        } else {
            0
        };
        self.dir = dir;
        self.w3.tries = 0;
        self.phase = skip::RETURN;
    }

    /// GiveUp: the spot it is on becomes home [obs 0x7100760460].
    fn skip_give_up(&mut self) {
        self.phase = skip::GIVE_UP;
        self.home_x = self.x;
        self.home_y = self.y;
        self.w3.tries = 0;
        self.vx = 0.0;
        self.timer = anim_len(SKIP_GIVE_UP, 0.75);
    }

    fn w3_fall(&mut self, level: &Level, g: f32) {
        self.vy = (self.vy + g).max(MAX_FALL);
        self.collide(level);
    }

    // ------------------------------------------------------------ Hop-Chops

    /// The player is inside a box `reach` px ahead and ±`up` px vertically.
    fn ahead_box(&self, reach: f32, up: f32) -> bool {
        let (px, py) = self.target;
        let dx = (px - self.x) * self.dir as f32;
        (0.0..=reach).contains(&dx) && (py - self.y).abs() <= up
    }

    fn hop_walk_enter(&mut self) {
        self.phase = hop::WALK;
        self.w3.frame = 0.0;
        self.w3.count = 0;
        self.w3.flags &= !flag::HOPPED;
        self.w3.tries = 2 + self.rand(3) as u8;
    }

    /// Hop-Chops (50-hop-chops.md): patrols in small hops (1.2 up, 0.5 forward, gravity −0.1)
    /// timed by its walk animation, one every 50 frames, turning (with a little hop) after 2–4
    /// hops or at a wall. A player within 96 px ahead (±64) startles it (a 2.5 hop), then it
    /// chases in quick runs (0.6 px/f during part of each 25-frame run cycle), turning round to
    /// face the player on the ground (braking at 0.06 first). It gives up after the player has
    /// been behind it or outside 128 × ±80 px for 16 frames.
    fn step_hop_chops(&mut self, level: &Level) {
        if self.phase == 0 {
            self.hop_walk_enter();
        }
        if self.wings {
            self.step_generic_wings(level);
            return;
        }
        match self.phase {
            hop::WALK => {
                self.w3.frame = (self.w3.frame + HOP_WALK_RATE) % HOP_WALK_FRAMES;
                if self.on_ground && self.ahead_box(HOP_SEE.0, HOP_SEE.1) {
                    self.phase = hop::FIND;
                    self.timer = anim_len(HOP_FIND.0, HOP_FIND.1);
                    self.vx = 0.0;
                    self.vy = HOP_FIND_VY;
                    self.on_ground = false;
                    self.w3_fall(level, ENEMY_GRAVITY);
                    return;
                }
                let turn = self.wall || (self.on_ground && self.w3.count >= self.w3.tries);
                if turn {
                    self.phase = hop::TURN;
                    self.timer = anim_len(HOP_TURN.0, HOP_TURN.1);
                    self.w3.clock = 7;
                    self.w3.flags &= !flag::HOPPED;
                    self.dir = -self.dir;
                    self.w3_fall(level, ENEMY_GRAVITY);
                    return;
                }
                let f = self.w3.frame;
                let mut g = if self.w3.flags & flag::HOPPED != 0 {
                    HOP_GRAVITY
                } else {
                    ENEMY_GRAVITY
                };
                if self.on_ground {
                    if (HOP_WINDOW.0..=HOP_WINDOW.1).contains(&f) {
                        if f <= HOP_TAKE_OFF {
                            self.vx = HOP_VX * self.dir as f32;
                            if self.w3.flags & flag::HOPPED == 0 && self.vy <= 0.0 {
                                self.vy = HOP_VY;
                                g = HOP_GRAVITY;
                                self.on_ground = false;
                                self.w3.count += 1;
                                self.w3.flags |= flag::HOPPED;
                            }
                        }
                    } else {
                        self.vx = 0.0;
                        g = ENEMY_GRAVITY;
                        self.w3.flags &= !flag::HOPPED;
                    }
                }
                self.w3_fall(level, g);
            }
            hop::TURN => {
                if self.on_ground {
                    self.vx = 0.0;
                }
                let before = self.w3.clock;
                if self.w3.clock > 0 {
                    self.w3.clock -= 1;
                }
                if before < 2 && self.on_ground && self.w3.flags & flag::HOPPED == 0 {
                    self.vy = HOP_TURN_HOP;
                    self.on_ground = false;
                    self.w3.flags |= flag::HOPPED;
                }
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.hop_walk_enter();
                }
            }
            hop::FIND => {
                self.vx = 0.0;
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = hop::RUN;
                    self.w3.frame = 0.0;
                    self.w3.clock = 0;
                    self.w3.flags &= !(flag::HOPPED | flag::BRAKING);
                }
            }
            hop::RUN => {
                let braking = self.w3.flags & flag::BRAKING != 0;
                let mut g = ENEMY_GRAVITY;
                if self.on_ground {
                    if !braking && self.toward_player() != self.dir {
                        self.dir = -self.dir;
                        self.w3.flags |= flag::BRAKING;
                    }
                    let braking = self.w3.flags & flag::BRAKING != 0;
                    let f = self.w3.frame;
                    if (HOP_RUN_WINDOW.0..=HOP_RUN_WINDOW.1).contains(&f) {
                        if !braking {
                            self.vx = HOP_RUN_VX * self.dir as f32;
                        }
                        if f <= HOP_RUN_JUMP && self.w3.flags & flag::HOPPED == 0 && self.vy <= 0.0
                        {
                            self.vy = 0.0;
                            self.w3.flags |= flag::HOPPED;
                        }
                        if self.w3.flags & flag::HOPPED != 0 {
                            g = HOP_RUN_GRAVITY;
                        }
                    } else {
                        self.w3.flags &= !flag::HOPPED;
                        if braking {
                            self.vx = ease(self.vx, 0.0, HOP_TURN_DECEL);
                            if self.vx == 0.0 {
                                self.w3.flags &= !flag::BRAKING;
                            }
                        } else {
                            self.vx = 0.0;
                        }
                    }
                }
                // the run animation stands still while it brakes outside the window
                let paused = self.w3.flags & flag::BRAKING != 0
                    && !(HOP_RUN_WINDOW.0..=HOP_RUN_WINDOW.1).contains(&self.w3.frame);
                if !paused {
                    self.w3.frame = (self.w3.frame + HOP_RUN_RATE) % HOP_RUN_FRAMES;
                }
                self.w3_fall(level, g);
                if self.wall {
                    self.vx = 0.0;
                }
                if self.on_ground {
                    if self.ahead_box(HOP_KEEP.0, HOP_KEEP.1) {
                        self.w3.clock = 0;
                    } else {
                        self.w3.clock += 1;
                        if self.w3.clock > HOP_LOST_GRACE {
                            self.phase = hop::LOST;
                            self.timer = anim_len(HOP_LOST.0, HOP_LOST.1);
                        }
                    }
                }
            }
            _ => {
                // Lost: slows to a stop while the give-up animation plays
                self.vx = ease(self.vx, 0.0, HOP_LOST_DECEL);
                self.w3_fall(level, ENEMY_GRAVITY);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.hop_walk_enter();
                }
            }
        }
    }

    /// Hop-Chops is chasing.
    pub fn hop_chops_running(&self) -> bool {
        matches!(self.phase, hop::FIND | hop::RUN)
    }

    // ------------------------------------------------------------ Porcupuffer

    /// Speed multiplier: by HP, ×1.25 winged [obs].
    fn puff_m(&self) -> f32 {
        PUFF_M[self.hops.min(PUFF_HP) as usize] * if self.wings { 1.25 } else { 1.0 }
    }

    /// Feet y when floating at the surface (its centre on the surface level: the liquid
    /// surface − 4 px, −16 in castle / forest themes, doubled when big). With no liquid the
    /// surface is the area's bottom [obs 0x7100663740]; None in an all-water area.
    fn puff_float_y(&self, level: &Level) -> Option<f32> {
        if self.w3.flooded {
            return None;
        }
        let surf = self.w3.liquid.map_or(level.bottom, |(y, _)| y);
        let s = self.size_k();
        let off = if matches!(level.theme, 2 | 9) {
            -16.0
        } else {
            -4.0
        };
        Some(surf + off * s - 16.0 * s)
    }

    /// Porcupuffer (114-porcupuffer.md): 4 HP; every speed scales with the HP left. Under water
    /// it steers (≤ 2°/f) toward the player at 1.25 px/f and lunges at 2.0 when lined up and
    /// close (full HP only). When the player is out of the water it floats up and patrols the
    /// surface within 160 px of where it came up, leaping at a player just ahead and above it,
    /// and dives again when the player swims ≥ 16 px under it. Placed above the liquid it falls
    /// to the surface; with no liquid it rests on the ground.
    fn step_porcupuffer(&mut self, level: &Level) {
        let float_y = self.puff_float_y(level);
        if self.phase == 0 {
            self.hops = PUFF_HP;
            let under = self.w3.liquid.is_some() && float_y.is_some_and(|f| self.y < f - 32.0);
            self.phase = if self.w3.flooded || under {
                puff::SWIM
            } else {
                puff::FALL
            };
            self.heading = if self.dir > 0 { 0.0 } else { 180.0 };
        }
        if self.w3.clock > 0 {
            self.w3.clock -= 1;
        }
        let m = self.puff_m();
        let s = self.size_k();
        let (px, py) = self.target;
        match self.phase {
            puff::FALL => {
                self.vx = ease(self.vx, 0.0, 0.05);
                self.w3_fall(level, ENEMY_GRAVITY);
                if let Some(f) = float_y
                    && self.y <= f
                    && self.vy <= 0.0
                {
                    self.y = f;
                    self.vy = 0.0;
                    self.puff_surface();
                } else if self.on_ground {
                    self.dir = self.toward_player();
                }
            }
            puff::SWIM | puff::SWIM_TURN | puff::LUNGE => {
                self.puff_swim(level, float_y, m, s);
            }
            puff::FLOAT => {
                self.vx = ease(self.vx, 0.0, PUFF_FLOAT_ACCEL);
                self.vy = ease(self.vy, PUFF_FLOAT * m, PUFF_FLOAT_ACCEL);
                self.collide(level);
                match float_y {
                    Some(f) if self.y >= f => {
                        self.y = f;
                        self.vy = 0.0;
                        self.puff_surface();
                    }
                    None => self.phase = puff::SWIM,
                    _ => {}
                }
            }
            puff::SURFACE => {
                let Some(f) = float_y else {
                    self.phase = puff::SWIM;
                    return;
                };
                let out = (self.x - self.w3.anchor).abs() > PUFF_BAND;
                if out && (self.x - self.w3.anchor) * self.dir as f32 > 0.0 {
                    self.dir = -self.dir;
                }
                let speed = if out { 2.0 * PUFF_SWIM } else { PUFF_SWIM };
                self.vx = ease(self.vx, speed * m * self.dir as f32, PUFF_FLOAT_ACCEL * m);
                self.vy = (f - self.y).clamp(-PUFF_EASE, PUFF_EASE);
                self.collide(level);
                if self.wall {
                    self.vx *= -0.5;
                    self.dir = -self.dir;
                }
                // a leap at a player just ahead and above; a dive after one under water
                let (hw, hh) = if self.wings {
                    (48.0, 64.0)
                } else {
                    (28.0, 40.0)
                };
                let wx = self.x + self.dir as f32 * (16.0 * s + 28.0);
                let wy = self.y + 16.0 * s + hh;
                if !out && (px - wx).abs() <= hw && (py - wy).abs() <= hh {
                    self.phase = puff::SURFACE_JUMP;
                    self.timer = 0;
                } else if self.w3.player_wet
                    && f + 16.0 * s - py >= PUFF_DIVE_DEPTH
                    && self.w3.clock == 0
                {
                    self.phase = puff::SWIM;
                    self.timer = 0;
                    self.heading = -90.0;
                }
            }
            puff::SURFACE_JUMP => {
                self.timer += 1;
                self.vx = 0.9375 * m * self.dir as f32;
                if self.timer <= PUFF_JUMP_WINDUP {
                    self.vy -= 0.0375 * m;
                } else if self.w3.count == 0 {
                    self.vy += PUFF_JUMP_RAMP;
                    if self.vy >= PUFF_JUMP * m {
                        self.vy = PUFF_JUMP * m;
                        self.w3.count = 1;
                    }
                } else {
                    let g = if self.wings && self.vy < -0.5 {
                        -0.15
                    } else {
                        ENEMY_GRAVITY
                    };
                    self.vy = (self.vy + g).max(MAX_FALL);
                }
                self.collide(level);
                if self.w3.count == 1 && self.vy < 0.0 {
                    match float_y {
                        Some(f) if self.y <= f => {
                            self.y = f;
                            self.vy = 0.0;
                            self.w3.count = 0;
                            self.puff_surface();
                        }
                        _ if self.on_ground => {
                            self.w3.count = 0;
                            self.phase = puff::FALL;
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                // Recover: the hurt animation, slowing down; then back to what it was doing
                self.speed = ease(self.speed, 0.0, 0.05);
                self.vx = ease(self.vx, 0.0, 0.05);
                if float_y.is_none_or(|f| self.y < f) && self.w3.liquid.is_some() {
                    self.vy = ease(self.vy, 0.0, 0.05);
                } else {
                    self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                }
                self.collide(level);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = self.w3.resume.max(puff::FALL);
                    self.timer = 0;
                }
            }
        }
    }

    fn puff_surface(&mut self) {
        self.phase = puff::SURFACE;
        self.w3.anchor = self.x;
        self.w3.clock = PUFF_COOLDOWN;
    }

    fn puff_swim(&mut self, level: &Level, float_y: Option<f32>, m: f32, s: f32) {
        let (px, py) = self.target;
        let (cx, cy) = (self.x, self.y + 16.0 * s);
        let (ax, ay) = (px, py + self.w3.player_h / 2.0 - 16.0 * s);
        let want = (ay - cy).datan2(ax - cx).to_degrees();
        let diff = (want - self.heading + 540.0).rem_euclid(360.0) - 180.0;
        match self.phase {
            puff::LUNGE => {
                self.timer += 1;
                if self.timer <= PUFF_LUNGE_STOP {
                    self.speed = ease(self.speed, 0.0, 0.75);
                } else {
                    self.speed = PUFF_LUNGE * m;
                }
                if self.timer >= PUFF_LUNGE_STOP + PUFF_LUNGE_FRAMES {
                    self.phase = puff::SWIM;
                    self.timer = 0;
                    self.w3.clock = self.w3.clock.max(PUFF_TURN_COOLDOWN);
                }
            }
            puff::SWIM_TURN => {
                // the heading moves to its mirror image in 12 equal steps
                let step = self.w3.anchor;
                let goal = self.w3.travel;
                self.heading = ease(self.heading, goal, step);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.heading = goal;
                    self.phase = puff::SWIM;
                    self.w3.clock = self.w3.clock.max(PUFF_TURN_COOLDOWN);
                }
            }
            _ => {
                self.heading += diff.clamp(-PUFF_STEER, PUFF_STEER);
                self.speed = ease(self.speed, PUFF_SWIM * m, PUFF_SWIM_ACCEL * m);
                let dist = ((ax - cx).powi(2) + (ay - cy).powi(2)).sqrt();
                let near = if self.big { 80.0 } else { PUFF_LUNGE_NEAR };
                let behind = (px - self.x) * (self.dir as f32) < 0.0;
                if diff.abs() <= PUFF_LINE_UP && self.hops == PUFF_HP && dist < near {
                    self.phase = puff::LUNGE;
                    self.timer = 0;
                } else if behind && self.w3.clock == 0 {
                    // SwimTurn: face the other way, the heading mirroring over 12 frames
                    self.dir = -self.dir;
                    let goal = 180.0 - self.heading;
                    self.w3.travel = goal;
                    self.w3.anchor = (goal - self.heading).abs() / PUFF_TURN_FRAMES as f32;
                    self.phase = puff::SWIM_TURN;
                    self.timer = PUFF_TURN_FRAMES;
                }
                // the player left the water: float up after 60 frames
                if self.w3.player_wet || float_y.is_none() || self.w3.liquid.is_none() {
                    self.timer = 0;
                } else {
                    self.timer += 1;
                    if self.timer >= PUFF_COOLDOWN {
                        self.phase = puff::FLOAT;
                        self.timer = 0;
                        self.vx = self.speed * self.heading.to_radians().dcos();
                        self.vy = self.speed * self.heading.to_radians().dsin();
                        return;
                    }
                }
            }
        }
        let a = self.heading.to_radians();
        self.vx = self.speed * a.dcos();
        self.vy = self.speed * a.dsin();
        if self.vx.abs() > 0.05 && self.phase != puff::SWIM_TURN {
            self.dir = self.vx.signum() as i32;
        }
        self.collide(level);
        // under water it stays below its floating level
        if let Some(f) = float_y
            && self.w3.liquid.is_some()
            && self.y > f
        {
            self.y = f;
        }
    }

    /// A fireball (or arrow) hit: one HP. Returns true when it was the last.
    pub fn porcupuffer_hit(&mut self, from_x: f32) -> bool {
        if self.phase == 0 {
            self.hops = PUFF_HP;
        }
        self.hops = self.hops.saturating_sub(1);
        if self.hops == 0 {
            self.die_fall(from_x);
            return true;
        }
        if self.phase != puff::RECOVER {
            self.w3.resume = if self.phase == puff::SWIM_TURN {
                puff::SWIM
            } else {
                self.phase
            };
        }
        self.phase = puff::RECOVER;
        self.timer = PUFF_RECOVER;
        self.kick_lock = PUFF_RECOVER;
        false
    }

    /// Hit points left (4 before the first step).
    pub fn porcupuffer_hp(&self) -> u8 {
        if self.phase == 0 { PUFF_HP } else { self.hops }
    }

    // ------------------------------------------------------------ Bully

    /// The nearest player is in its sight box [obs 0x71006490a0].
    fn bully_sees(&self) -> bool {
        let k = self.size_k();
        let (px, py) = self.target;
        let cx = self.x + BULLY_SEE_AHEAD * self.dir as f32;
        let cy = self.y + 16.0 * k;
        let half = if self.wings {
            BULLY_FLY_SEE_HALF
        } else {
            BULLY_SEE_HALF
        } + 16.0 * k;
        let head = py + self.w3.player_h.max(1.0);
        (px - cx).abs() < BULLY_SEE_AHEAD && py < cy + half && head > cy - half
    }

    /// No floor ahead in direction `dir` (the ledge probe 0x7100648d80).
    fn bully_ledge(&self, level: &Level, dir: f32) -> bool {
        if !self.on_ground {
            return false;
        }
        let probe = self.x + dir * (self.half_w() + 0.1);
        let slope = level
            .slope_at(probe)
            .is_some_and(|(s, _)| s >= self.y - 8.0);
        level.cell_at(probe, self.y - 1.0) == Cell::Empty && !slope
    }

    fn bully_wait(&mut self) {
        self.phase = bully::WAIT;
        self.timer = BULLY_WAIT;
        self.vx = 0.0;
        self.w3.flags = flag::CAN_HOLD | flag::CAN_RETURN;
    }

    /// In water (its fall and fly-back timings are slower there): BG flag +0x648 bit 28, the
    /// in-water bit the Skipsqueak's jump gravity also reads [obs 0x710075f710].
    fn bully_wet(&self) -> bool {
        self.w3.liquid.is_some_and(|(s, w)| w && self.y + 16.0 < s) || self.w3.flooded
    }

    fn bully_find(&mut self) {
        self.phase = bully::FIND;
        self.timer = anim_len(BULLY_FIND_FRAMES, 1.0);
        self.vx = 0.0;
        self.w3.flags |= flag::TARGET;
    }

    /// Bully (117-bully.md): waits 40 frames, walks at 0.3 for 60–179 frames (turning at walls
    /// and ledges), and when a player is in the 128-px box ahead plays its notice (39 frames)
    /// and charges at 0.8 (accel 0.025), turning round only once the player is 28 px behind,
    /// until they are 256 px away. Hits only push it back (1.0 / 1.75 / 2.5 px/f by the attack,
    /// decaying by 0.05/f), after which it charges the attacker; pushed slowly towards a ledge
    /// it stops teetering on the edge (HoldOut). Winged, it does all that on the ground, sees
    /// much higher, flies up walls and ledges after the player and flies back onto a ledge it
    /// was knocked off (see `BULLY_FLY_*`).
    fn step_bully(&mut self, level: &Level) {
        if self.phase == 0 {
            self.bully_wait();
        }
        if self.w3.cooldown > 0 {
            self.w3.cooldown -= 1;
        }
        // its home is where it last stood [obs 0x7100647380]
        if self.on_ground {
            self.home_x = self.x;
            self.home_y = self.y;
        }
        let (px, py) = self.target;
        let dx = px - self.x;
        let dir = self.dir as f32;
        let mut gravity = ENEMY_GRAVITY;
        match self.phase {
            bully::WAIT => {
                self.vx = 0.0;
                self.timer -= 1;
                if self.bully_sees() {
                    self.bully_find();
                } else if self.timer <= 0 {
                    if self.rand(2) == 0 {
                        self.dir = -self.dir;
                    }
                    self.phase = bully::WALK;
                    self.timer = 60 + self.rand(120) as i32;
                    self.vx = BULLY_WALK * self.dir as f32;
                }
            }
            bully::WALK => {
                if self.bully_ledge(level, dir) || self.w3.walled {
                    self.dir = -self.dir;
                }
                self.vx = BULLY_WALK * self.dir as f32;
                self.timer -= 1;
                if self.bully_sees() {
                    self.bully_find();
                } else if self.timer <= 0 {
                    self.bully_wait();
                }
            }
            bully::FIND => {
                self.vx = 0.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = bully::CHASE;
                }
            }
            bully::CHASE => {
                let dist = (dx * dx + (py - self.y).powi(2)).sqrt();
                if dist > BULLY_GIVE_UP {
                    self.bully_wait();
                } else if self.w3.walled {
                    // a wall stops the charge with a bounce; winged, with the player above, it
                    // takes off
                    self.vx = -BULLY_WALL_BOUNCE * dir;
                    if self.wings && py > self.y {
                        self.bully_fly_chase();
                    }
                } else if self.wings && !self.on_ground {
                    // winged, off a ledge: it takes off
                    self.bully_fly_chase();
                } else if !self.wings
                    && self.bully_ledge(level, dir)
                    && self.w3.flags & flag::CAN_HOLD != 0
                {
                    self.bully_hold_out();
                } else {
                    if (self.x - (px + BULLY_BEHIND * dir)) * dir > 0.0 {
                        self.dir = -self.dir;
                    }
                    self.vx = ease(self.vx, BULLY_CHASE * self.dir as f32, BULLY_ACCEL);
                }
            }
            bully::FLIP => {
                if !self.on_ground {
                    self.bully_fall();
                } else {
                    let v = self.vx;
                    let moving = v.signum();
                    if self.w3.flags & flag::CAN_HOLD != 0
                        && v.abs() < BULLY_HOLD_SPEED
                        && v != 0.0
                        && self.bully_ledge(level, moving)
                    {
                        self.bully_hold_out();
                    } else {
                        self.timer += 1;
                        self.w3.clock -= 1;
                        if v != 0.0 {
                            self.vx = ease(v, 0.0, BULLY_PUSH_DECAY);
                        } else if self.w3.count == 0 {
                            // stopped: once the push animation is over, a short recovery
                            if self.timer >= self.w3.tries as i32 || self.w3.clock <= 0 {
                                self.w3.count = 1;
                                self.w3.clock = if self.bully_wet() {
                                    BULLY_FLIP_RECOVER_WATER
                                } else {
                                    BULLY_FLIP_RECOVER
                                };
                            }
                        } else if self.w3.clock <= 0 {
                            if self.w3.flags & flag::TARGET != 0 {
                                self.phase = bully::CHASE;
                            } else {
                                self.bully_wait();
                            }
                        }
                    }
                }
            }
            bully::HOLD_OUT | bully::LAND | bully::FALL_STAN => {
                self.vx = 0.0;
                if !self.on_ground {
                    self.bully_fall();
                } else {
                    self.timer -= 1;
                    if self.timer <= 0 {
                        if self.phase == bully::HOLD_OUT {
                            self.bully_land();
                        } else {
                            self.bully_wait();
                        }
                    }
                }
            }
            bully::FALL => {
                self.vx = ease(self.vx, 0.0, BULLY_FALL_DECEL);
                self.timer -= 1;
                if self.on_ground && self.vy <= 0.0 {
                    self.phase = bully::FALL_STAN;
                    self.timer = BULLY_FALL_STAN;
                } else if self.wings && self.w3.flags & flag::CAN_RETURN != 0 && self.timer <= 0 {
                    // FlyReturn: back up to the ledge it left
                    self.phase = bully::FLY_RETURN;
                    self.timer = BULLY_FLY_RETURN;
                    self.w3.flags &= !flag::CAN_RETURN;
                }
            }
            bully::FLY_CHASE => {
                // the chase's pull on the forward speed still acts after the decay
                let v = ease(self.vx, 0.0, BULLY_FLY_DECAY);
                self.vx = ease(v, BULLY_CHASE * dir, BULLY_ACCEL);
                self.timer -= 1;
                let dist = (dx * dx + (py - self.y).powi(2)).sqrt();
                if self.on_ground {
                    self.bully_wait();
                } else {
                    if self.timer <= 0 || dist > BULLY_GIVE_UP {
                        // out of time or lost the player: the climb ends and it drops
                        if self.w3.flags & flag::CLIMBING != 0 {
                            self.vy = 0.0;
                        }
                        self.w3.flags &= !flag::CLIMBING;
                    } else if self.y >= py {
                        // FlyChase exit stops it dead [obs 0x7100648920]
                        self.vx = 0.0;
                        self.vy = 0.0;
                        self.bully_fly_land();
                        gravity = 0.0;
                    }
                    if self.w3.flags & flag::CLIMBING != 0 {
                        self.vy = ease(self.vy, BULLY_FLY_RISE, BULLY_FLY_RISE_ACCEL);
                        gravity = 0.0;
                    }
                }
            }
            bully::FLY_LAND => {
                if self.timer > 0 {
                    self.timer -= 1;
                    self.vy -= BULLY_FLY_LAND_SINK;
                    gravity = 0.0;
                } else if self.on_ground && self.vy <= 0.0 {
                    self.bully_land();
                }
                self.vx = BULLY_FLY_LAND_SPEED * dir;
            }
            _ => {
                // FlyReturn: up at 0.5, its forward speed held, until above the ledge it left
                let accel = if self.bully_wet() {
                    BULLY_FLY_RETURN_ACCEL_WATER
                } else {
                    BULLY_FLY_RETURN_ACCEL
                };
                self.vy = ease(self.vy, BULLY_FLY_RISE, accel);
                gravity = 0.0;
                self.timer -= 1;
                if self.y > self.home_y {
                    self.bully_fly_land();
                    self.vx = BULLY_FLY_LAND_SPEED * self.dir as f32;
                } else if self.timer <= 0 {
                    self.bully_fall();
                }
            }
        }
        let rising = self.vy > 0.0;
        self.w3_fall(level, gravity);
        self.w3.walled = self.wall;
        if self.phase == bully::FLY_CHASE && rising && self.vy <= 0.0 && !self.on_ground {
            // a ceiling ends the climb
            self.w3.flags &= !flag::CLIMBING;
        }
    }

    /// FlyChase enter: up at 0.5 with no gravity for up to 300 frames [obs 0x71006484c0].
    fn bully_fly_chase(&mut self) {
        self.phase = bully::FLY_CHASE;
        self.timer = BULLY_FLY_CHASE;
        self.vy = BULLY_FLY_RISE;
        self.on_ground = false;
        self.w3.flags = (self.w3.flags & !flag::CAN_RETURN) | flag::CLIMBING;
    }

    /// FlyLand enter: forward at 0.5, its vertical speed sinking by 0.015/f² for 36 frames (60
    /// big) [obs 0x7100648980].
    fn bully_fly_land(&mut self) {
        self.phase = bully::FLY_LAND;
        self.timer = if self.big {
            BULLY_FLY_LAND.1
        } else {
            BULLY_FLY_LAND.0
        };
        self.w3.flags &= !flag::CLIMBING;
    }

    /// Land: the landing animation, then Wait; a winged one may fly back again [obs 0x7100647f10].
    fn bully_land(&mut self) {
        self.phase = bully::LAND;
        self.timer = BULLY_LAND;
        self.w3.flags |= flag::CAN_RETURN;
    }

    /// In the air on its wings (FlyChase, FlyLand, FlyReturn): a light or stomp-class hit drops it
    /// instead of pushing it [obs 0x7100648cb0].
    fn bully_flying(&self) -> bool {
        matches!(
            self.phase,
            bully::FLY_CHASE | bully::FLY_LAND | bully::FLY_RETURN
        )
    }

    fn bully_hold_out(&mut self) {
        self.phase = bully::HOLD_OUT;
        self.timer = BULLY_HOLD_OUT;
        self.vx = 0.0;
        self.w3.flags &= !flag::CAN_HOLD;
    }

    /// Fall enter: it turns to face against its motion; the fall counts 12 frames (40 in water)
    /// before a winged one may fly back [obs 0x7100647c70, 0x7100647dd0].
    fn bully_fall(&mut self) {
        self.phase = bully::FALL;
        if self.vx != 0.0 {
            self.dir = if self.vx > 0.0 { -1 } else { 1 };
        }
        self.timer = if self.bully_wet() {
            BULLY_FALL_FRAMES_WATER
        } else {
            BULLY_FALL_FRAMES
        };
    }

    /// A hit that only pushes a Bully back, away from `from_x`, at strength `strength` (see
    /// `push`); `by_player` makes the attacker its target.
    pub fn bully_push(&mut self, strength: usize, from_x: f32) {
        self.bully_push_by(strength, from_x, true);
    }

    pub fn bully_push_by(&mut self, strength: usize, from_x: f32, by_player: bool) {
        if self.bully_flying() && strength < push::HEAVY {
            // in flight a light or stomp-class hit drops it, no push [obs 0x7100648cb0]
            self.w3.flags &= !flag::CLIMBING;
            self.vx = 0.0;
            self.vy = self.vy.min(0.0);
            self.bully_fall();
            return;
        }
        let away = if self.x < from_x { -1.0 } else { 1.0 };
        let k = if self.big { 0.75 } else { 1.0 };
        self.vx = BULLY_PUSH[strength.min(2)] * k * away;
        if self.phase == 0 {
            self.bully_wait();
        }
        let keep = self.w3.flags & (flag::CAN_HOLD | flag::CAN_RETURN);
        self.w3.flags = keep | if by_player { flag::TARGET } else { 0 };
        self.phase = bully::FLIP;
        self.timer = 0;
        self.w3.clock = BULLY_FLIP;
        self.w3.count = 0;
        // the push animation: PushFront when pushed the way it faces, else PushBack
        self.w3.tries = if away * self.dir as f32 > 0.0 {
            BULLY_PUSH_FRONT
        } else {
            BULLY_PUSH_BACK
        } as u8;
    }

    /// The player's body touching it pushes it (light, 7 frames apart per Bully) [obs
    /// 0x7100649cb0]; true when this touch pushes.
    pub fn bully_body_push(&mut self, from_x: f32) -> bool {
        if self.w3.cooldown > 0 {
            return false;
        }
        self.w3.cooldown = 7;
        self.bully_push_by(push::LIGHT, from_x, true);
        true
    }

    /// Shoves of the player so far (every 4th one also throws the player up; the game counts
    /// them on the player [obs 0x71009d5b90], here per Bully).
    pub fn bully_shove_count(&mut self) -> u8 {
        self.w3.shoves = self.w3.shoves.wrapping_add(1);
        self.w3.shoves
    }

    /// Bully is charging at the player.
    pub fn bully_charging(&self) -> bool {
        matches!(self.phase, bully::CHASE | bully::FLY_CHASE)
    }

    /// Bully is being pushed back or teetering on a ledge.
    pub fn bully_pushed(&self) -> bool {
        matches!(self.phase, bully::FLIP | bully::HOLD_OUT)
    }

    // ------------------------------------------------------------ Charvaargh

    /// The level it leaps from: the lava surface − 16 in lava themes (castle; forest at night),
    /// else the area's bottom [obs 0x710079db70, 0x710079ff40].
    fn charvaargh_base(&self, level: &Level) -> f32 {
        let lava = level.theme == 2 || (level.theme == 9 && level.night);
        match self.w3.liquid {
            Some((y, _)) if lava && !self.w3.flooded => y - 16.0,
            _ => level.bottom,
        }
    }

    /// The head point (the model's origin): 32 px above `y` (the box's foot).
    fn charvaargh_head(&self) -> (f32, f32) {
        (self.x, self.y + 32.0)
    }

    /// Charvaargh (86-charvaargh.md): rests under the lava; every 30 frames it warns (its
    /// silhouette swims in the background for 161 frames), then leaps out at the nearest
    /// player: it rises from 32 px under the base level at 2.0 px/f, its heading swinging from
    /// straight up to straight down as the leap animations pitch it (speed easing to 0.2, then
    /// back towards 2.0), and dives until its tail is 64 px under the base. The body (8 boxes)
    /// follows the head's path. Only a Super Star defeats it. With the player close above open
    /// space it leaps vertically instead, out of the background (or into it) under the player;
    /// a dive that ends out of view chains straight into a leap from the view's edge; several
    /// Charvaarghs take turns to warn and keep their leaps apart.
    fn step_charvaargh(&mut self, level: &Level) {
        let base = self.charvaargh_base(level);
        if self.phase == 0 {
            self.phase = chr::FALL;
        }
        match self.phase {
            chr::FALL => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.y += self.vy;
                if self.charvaargh_head().1 <= base + 4.0 + CHAR_DIVE_DEPTH {
                    self.charvaargh_wait(level, base);
                }
            }
            chr::WAIT => self.charvaargh_waiting(level, base),
            chr::SIGN => {
                self.timer -= 1;
                if self.timer <= 0 {
                    // the warning is over: the turn passes on [obs AttackSign exit 0x71007a0970]
                    self.w3.recs &= !chr::TURN;
                    self.charvaargh_start(base);
                }
            }
            _ => self.charvaargh_leap(level, base),
        }
    }

    /// Wait enter: under the lava at the base level, inside the area's edges; the vertical leap
    /// may be tried again [obs 0x710079ff40].
    fn charvaargh_wait(&mut self, level: &Level, base: f32) {
        self.phase = chr::WAIT;
        self.timer = CHAR_LOCKOUT;
        self.vx = 0.0;
        self.vy = 0.0;
        self.y = base + 4.0 - 32.0;
        self.x = self.charvaargh_inside(level, self.x);
        self.w3.flags = chr::VERT_OK;
        self.w3.count = 0;
        self.trail.clear();
    }

    /// `x` moved 64 px inside the area when its box crosses an edge.
    fn charvaargh_inside(&self, level: &Level, x: f32) -> f32 {
        if x - CHAR_HALF < level.left {
            level.left + CHAR_EDGE_IN
        } else if x + CHAR_HALF > level.right {
            level.right - CHAR_EDGE_IN
        } else {
            x
        }
    }

    /// Wait: the vertical leap's test in the last frames, the turn, then the warning.
    fn charvaargh_waiting(&mut self, level: &Level, base: f32) {
        if self.w3.flags & chr::VERT_OK != 0 && self.timer < 3 {
            self.charvaargh_aim(level, base);
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.w3.turn_busy && self.w3.recs & chr::TURN == 0 {
            self.timer = self.timer.max(CHAR_TURN_HOLD);
        }
        if self.timer > 0 {
            return;
        }
        // face the nearest player, away from an area edge within 128 px
        self.dir = if self.x - CHAR_EDGE < level.left {
            1
        } else if self.x + CHAR_EDGE > level.right {
            -1
        } else {
            self.toward_player()
        };
        self.phase = chr::SIGN;
        let near = (self.target.0 - self.x).abs() <= CHAR_VERT_REACH;
        if self.w3.flags & chr::VERT_OK != 0 && near {
            // under the player, out of the background or into it at random
            self.w3.flags |= chr::VERTICAL;
            if self.rand(2) == 1 {
                self.w3.flags |= chr::REAR;
            }
            self.x = self.w3.anchor;
            self.timer = CHAR_SIGN_VERT;
        } else {
            let n = self.w3.peers_n[0] as usize;
            let left = self.dir < 0;
            self.x += charvaargh_space_side(&self.w3.peers_side[..n], self.x, left);
            self.charvaargh_record_side();
            self.timer = CHAR_SIGN;
        }
    }

    /// The vertical leap's test [obs 0x71007a00a0]: aim (once) at the player, spaced from the
    /// other vertical spots, then probe for terrain two columns a frame.
    fn charvaargh_aim(&mut self, level: &Level, base: f32) {
        if self.w3.flags & chr::PROBING == 0 {
            let tx = self.target.0;
            let room = tx - CHAR_VERT_LEFT >= level.left && tx + CHAR_VERT_RIGHT <= level.right;
            if level.main_area && !room {
                self.w3.flags &= !chr::VERT_OK;
                return;
            }
            self.w3.flags |= chr::PROBING;
            let n = self.w3.peers_n[1] as usize;
            let x = tx + charvaargh_space_vert(&self.w3.peers_vert[..n], tx);
            self.w3.anchor = x;
            self.w3.rec[1] = x;
            self.w3.recs |= chr::REC_VERT;
        }
        let y = base + CHAR_VERT_APEX;
        let blocked = |k: u8| {
            let px = self.w3.anchor - 24.0 + 16.0 * f32::from(k);
            let steps = (2.0 * CHAR_VERT_PROBE / 4.0) as i32;
            (0..=steps).any(|i| {
                let py = y - CHAR_VERT_PROBE + 4.0 * i as f32;
                level.cell_at(px, py).is_solid()
            })
        };
        if blocked(self.w3.count) {
            self.w3.flags &= !chr::VERT_OK;
            return;
        }
        self.w3.count += 1;
        if self.w3.count > 3 {
            return;
        }
        if blocked(self.w3.count) {
            self.w3.flags &= !chr::VERT_OK;
            return;
        }
        self.w3.count += 1;
    }

    fn charvaargh_record_side(&mut self) {
        self.w3.rec[0] = self.x;
        self.w3.recs |= chr::REC_SIDE;
    }

    /// AttackStart enter: the side leap rises from 32 px under the base level at 2.0 px/f; the
    /// vertical one anchors there and rides its root bone [obs 0x71007a09b0].
    fn charvaargh_start(&mut self, base: f32) {
        self.phase = chr::START;
        self.timer = 0;
        self.speed = CHAR_START_SPEED;
        self.trail.clear();
        self.w3.travel = 0.0;
        self.heading = 90.0;
        if self.w3.flags & chr::VERTICAL != 0 {
            self.w3.anchor = self.x;
            self.w3.track = base - 32.0;
            self.charvaargh_vert_place(0.0);
        } else {
            self.y = base - 32.0 - 32.0;
        }
    }

    /// Frames into the vertical leap (its animations' timeline).
    fn charvaargh_vert_t(&self) -> f32 {
        let start = match self.phase {
            chr::START => 0,
            chr::ATTACK => CHAR_START.2,
            _ => CHAR_START.2 + CHAR_ATTACK.2,
        };
        (start + self.timer) as f32
    }

    /// Head point and depth of the vertical leap at leap frame `t`.
    fn charvaargh_vert_at(&self, t: f32) -> (f32, f32, f32) {
        let rear = self.w3.flags & chr::REAR != 0;
        let (ox, oy, oz) = charvaargh_vert_offset(rear, t);
        let z0 = if rear { CHAR_Z_FRONT } else { CHAR_Z_BACK };
        (self.w3.anchor + ox, self.w3.track + oy, z0 + oz)
    }

    fn charvaargh_vert_place(&mut self, t: f32) {
        let (x, y, _) = self.charvaargh_vert_at(t);
        let (hx, hy) = self.charvaargh_head();
        self.vx = x - hx;
        self.vy = y - hy;
        self.x = x;
        self.y = y - 32.0;
    }

    fn charvaargh_leap(&mut self, level: &Level, base: f32) {
        self.timer += 1;
        let f = self.timer as f32;
        let (target, step, frames) = match self.phase {
            chr::START => CHAR_START,
            _ => CHAR_ATTACK,
        };
        if self.w3.flags & chr::VERTICAL != 0 {
            let t = self.charvaargh_vert_t();
            self.charvaargh_vert_place(t);
        } else {
            let a = match self.phase {
                chr::START => pitch(&CHAR_PITCH_START, f),
                chr::ATTACK => pitch(&CHAR_PITCH_ATTACK, f),
                _ => std::f32::consts::FRAC_PI_2,
            };
            self.speed = ease(self.speed, target, step);
            let (fx, fy) = (a.dcos() * self.dir as f32, -a.dsin());
            self.heading = fy.datan2(fx).to_degrees();
            self.vx = self.speed * fx;
            self.vy = self.speed * fy;
            let (hx, hy) = self.charvaargh_head();
            if self.trail.is_empty() {
                self.trail.push((hx, hy));
            }
            self.x += self.vx;
            self.y += self.vy;
            self.w3.travel += self.speed;
            let head = self.charvaargh_head();
            self.trail.push(head);
            // keep enough of the path for the tail
            let keep = (CHAR_TAIL / 0.2).ceil() as usize + 2;
            if self.trail.len() > keep {
                let n = self.trail.len() - keep;
                self.trail.drain(..n);
            }
            // its spot on the area's record follows it [obs 0x71007a1620]
            self.charvaargh_record_side();
        }
        if self.phase != chr::END && self.timer >= frames {
            self.phase = if self.phase == chr::START {
                chr::ATTACK
            } else {
                chr::END
            };
            self.timer = 0;
        } else if self.phase == chr::END {
            let tail = if self.w3.flags & chr::VERTICAL != 0 {
                let p = self.charvaargh_vert_points(&[CHAR_TAIL])[0];
                (p.0, p.1)
            } else {
                self.charvaargh_point(CHAR_TAIL)
            };
            if tail.1 + CHAR_DIVE_DEPTH <= base {
                self.charvaargh_dive_end(level, base);
            }
        }
    }

    /// The dive is over [obs AttackEnd 0x71007a0cf0, exit 0x71007a1030]: its spot (inside the
    /// area's edges) still in view → the wait; out of view → straight into a side leap from just
    /// outside the view, facing its centre, a spacing shift toward the view turned round and
    /// doubled. The views list (camera manager +0x138, one per area, also read by the cannons'
    /// fire test and the in-view flag) is rebuilt every frame from the active cameras' view
    /// rectangles (+0xC left … +0x18 top): in single player just the camera's view, in
    /// multiplayer each player's camera [obs v1 0x7100b64340].
    fn charvaargh_dive_end(&mut self, level: &Level, base: f32) {
        self.w3.flags &= !(chr::VERTICAL | chr::REAR);
        self.w3.recs &= !chr::REC_VERT;
        let x = self.charvaargh_inside(level, self.x);
        let (v0, v1) = (self.camera.0, self.camera.0 + VIEW_W);
        if v0 <= x + CHAR_CHAIN_HALF && v1 >= x - CHAR_CHAIN_HALF {
            self.charvaargh_wait(level, base);
            return;
        }
        let x = x.clamp(v0 - CHAR_CHAIN_MARGIN, v1 + CHAR_CHAIN_MARGIN);
        let left = (v0 + v1) / 2.0 - x <= 0.0;
        self.dir = if left { -1 } else { 1 };
        let n = self.w3.peers_n[0] as usize;
        let mut s = charvaargh_space_side(&self.w3.peers_side[..n], x, left);
        if (left && s < 0.0) || (!left && s > 0.0) {
            s *= -2.0;
        }
        self.x = x + s;
        self.w3.flags = 0;
        self.charvaargh_record_side();
        self.charvaargh_start(base);
    }

    /// The point `back` px behind the head along its path (ahead of it when negative); behind
    /// the start of the path it continues straight down.
    fn charvaargh_point(&self, back: f32) -> (f32, f32) {
        let (hx, hy) = self.charvaargh_head();
        if back < 0.0 {
            let a = self.heading.to_radians();
            return (hx - back * a.dcos(), hy - back * a.dsin());
        }
        let mut left = back;
        let mut p = (hx, hy);
        for q in self.trail.iter().rev().skip(1) {
            let d = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
            if d >= left && d > 0.0 {
                let t = left / d;
                return (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
            }
            left -= d;
            p = *q;
        }
        (p.0, p.1 - left)
    }

    /// Points `backs` px (ascending) behind the head along the vertical leap's path, with their
    /// depth: the body follows the head's path as in the side leap; before the leap's start the
    /// path runs straight down [inf: the game puts the boxes on the posed spine bones of the
    /// AttackFront/Rear animations (0x710079e290); against those the boxes stay within ≈ 25 px.
    /// Matching exactly needs every bone's per-frame pose from the model, i.e. a large table of
    /// asset data, which the sim does not carry].
    fn charvaargh_vert_points(&self, backs: &[f32]) -> Vec<(f32, f32, f32)> {
        const STEP: f32 = 0.25;
        let t0 = self.charvaargh_vert_t();
        let head = self.charvaargh_vert_at(t0);
        let mut out = Vec::with_capacity(backs.len());
        let (mut p, mut s, mut walked) = (head, t0, 0.0f32);
        for &d in backs {
            if d < 0.0 {
                let q = self.charvaargh_vert_at(t0 + 1.0);
                let (dx, dy, dz) = (q.0 - head.0, q.1 - head.1, q.2 - head.2);
                let n = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-6);
                out.push((
                    head.0 - d * dx / n,
                    head.1 - d * dy / n,
                    head.2 - d * dz / n,
                ));
                continue;
            }
            loop {
                if s <= 0.0 {
                    out.push((p.0, p.1 - (d - walked), p.2));
                    break;
                }
                let q = self.charvaargh_vert_at((s - STEP).max(0.0));
                let seg = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2) + (p.2 - q.2).powi(2)).sqrt();
                if walked + seg >= d && seg > 0.0 {
                    let f = (d - walked) / seg;
                    out.push((
                        p.0 + (q.0 - p.0) * f,
                        p.1 + (q.1 - p.1) * f,
                        p.2 + (q.2 - p.2) * f,
                    ));
                    break;
                }
                walked += seg;
                p = q;
                s -= STEP;
            }
        }
        out
    }

    /// Centres of the body's touch boxes while it leaps (empty otherwise); in the vertical leap
    /// only those crossing the course's depth.
    pub fn charvaargh_segments(&self) -> Vec<(f32, f32)> {
        if !matches!(self.phase, chr::START | chr::ATTACK | chr::END) {
            return Vec::new();
        }
        if self.w3.flags & chr::VERTICAL != 0 {
            let (z0, z1) = CHAR_Z_TOUCH;
            return self
                .charvaargh_vert_points(&CHAR_SEGMENTS)
                .into_iter()
                .filter(|p| (z0..=z1).contains(&p.2))
                .map(|p| (p.0, p.1))
                .collect();
        }
        CHAR_SEGMENTS
            .iter()
            .map(|&d| self.charvaargh_point(d))
            .collect()
    }

    /// Charvaargh is leaping (or warning).
    pub fn charvaargh_attacking(&self) -> bool {
        matches!(self.phase, chr::SIGN | chr::START | chr::ATTACK | chr::END)
    }

    /// Charvaargh is leaping vertically (out of or into the background).
    pub fn charvaargh_vertical(&self) -> bool {
        self.w3.flags & chr::VERTICAL != 0 && self.charvaargh_attacking()
    }

    /// What the area's Charvaargh list holds for it: its side and vertical leap spots, whether
    /// it holds the turn, and whether it is waiting (could take the turn).
    pub fn charvaargh_record(&self) -> (Option<f32>, Option<f32>, bool, bool) {
        let r = self.w3.recs;
        (
            (r & chr::REC_SIDE != 0).then_some(self.w3.rec[0]),
            (r & chr::REC_VERT != 0).then_some(self.w3.rec[1]),
            r & chr::TURN != 0,
            self.phase == chr::WAIT,
        )
    }

    /// It takes the area's turn (the first one waiting while nobody holds it).
    pub fn charvaargh_take_turn(&mut self) {
        self.w3.recs |= chr::TURN;
    }

    /// Sensed before its step: the other Charvaarghs' spots on record and whether another one
    /// holds the turn.
    pub fn charvaargh_sense(&mut self, side: &[f32], vert: &[f32], busy: bool) {
        let ns = side.len().min(PEERS);
        let nv = vert.len().min(PEERS);
        self.w3.peers_side[..ns].copy_from_slice(&side[..ns]);
        self.w3.peers_vert[..nv].copy_from_slice(&vert[..nv]);
        self.w3.peers_n = [ns as u8, nv as u8];
        self.w3.turn_busy = busy;
    }

    // ------------------------------------------------------------ Piranha Creeper

    /// Its path's node codes (empty without a path).
    fn creeper_codes<'a>(&self, level: &'a Level, path: u8) -> &'a [u8] {
        level
            .creepers
            .iter()
            .find(|(i, _)| *i == path)
            .map(|(_, c)| c.as_slice())
            .unwrap_or(&[])
    }

    /// Centre of piece `k`: the first piece at (`home_x`, `home_y` + 16) whichever way the path
    /// leaves [obs 0x710071f980], each next one 32 px further along.
    fn creeper_centre(&self, pieces: &[(u8, u8)], k: usize) -> (f32, f32) {
        let (mut x, mut y) = (self.home_x, self.home_y + CREEP_CELL / 2.0);
        for p in pieces.iter().take(k) {
            let (dx, dy) = DIRS[p.1 as usize];
            x += dx * CREEP_CELL;
            y += dy * CREEP_CELL;
        }
        (x, y)
    }

    /// Centre of the head in piece `k` at piece animation frame `f`.
    pub fn creeper_point_at(&self, codes: &[u8], k: usize, f: f32) -> (f32, f32) {
        let pieces = creeper_pieces(codes);
        let Some(&(e, o)) = pieces.get(k) else {
            return (self.home_x, self.home_y + CREEP_CELL / 2.0);
        };
        let (cx, cy) = self.creeper_centre(&pieces, k);
        let (ex, ey) = DIRS[e as usize];
        let (ox, oy) = DIRS[o as usize];
        let (a, b) = if e == o {
            (
                -CREEP_ENTRY + (CREEP_ENTRY + CREEP_EXIT) * f / CREEP_PIECE_FRAMES,
                0.0,
            )
        } else {
            let half = CREEP_PIECE_FRAMES / 2.0;
            let (s, t) = if f < half {
                (0, f / half)
            } else {
                (1, ((f - half) / half).min(1.0))
            };
            (cubic(&CREEP_CORNER_A[s], t), cubic(&CREEP_CORNER_B[s], t))
        };
        (cx + ex * a + ox * b, cy + ey * a + oy * b)
    }

    /// Centre of the head now.
    pub fn creeper_point(&self, codes: &[u8]) -> (f32, f32) {
        self.creeper_point_at(codes, self.w3.node.max(0) as usize, self.w3.frame)
    }

    /// Stalk pieces behind the head (centre and node code), base first.
    pub fn creeper_stalk(&self, codes: &[u8]) -> Vec<(f32, f32, u8)> {
        let pieces = creeper_pieces(codes);
        let n = (self.w3.node.max(0) as usize).min(pieces.len());
        (0..n)
            .map(|k| {
                let (x, y) = self.creeper_centre(&pieces, k);
                let code = if k == 0 { codes[0] } else { codes[k - 1] };
                (x, y, code)
            })
            .collect()
    }

    /// The stalk's solid boxes (x0, y0, x1, y1): each grown piece is two 16-px-thick boxes, from
    /// its entry side to its centre and from its centre to its exit side (an L at corners)
    /// [obs: stalk terrain colliders, type 0x12; the half-tile base box isn't modelled, inf].
    pub fn creeper_solids(&self, codes: &[u8]) -> Vec<(f32, f32, f32, f32)> {
        let pieces = creeper_pieces(codes);
        let n = (self.w3.node.max(0) as usize).min(pieces.len());
        let half = CREEP_CELL / 4.0;
        let mut out = Vec::with_capacity(2 * n);
        for (k, &(entry, exit)) in pieces.iter().enumerate().take(n) {
            let (cx, cy) = self.creeper_centre(&pieces, k);
            for (d, sign) in [(entry, -1.0), (exit, 1.0)] {
                let (dx, dy) = DIRS[d as usize];
                let (mx, my) = (cx + sign * dx * half, cy + sign * dy * half);
                out.push((mx - half, my - half, mx + half, my + half));
            }
        }
        out
    }

    /// Plays piece `k` from its start (or its end when going back).
    fn creeper_enter_piece(&mut self, k: i32, back: bool) {
        self.w3.node = k;
        self.w3.frame = if back { CREEP_PIECE_END } else { 0.0 };
    }

    /// Piranha Creeper (107-piranha-creeper.md): the head grows out along its path, a 32-px
    /// stalk piece at a time (each piece's animation at rate 0.6 ≈ 1.68 px/f, easing in by 0.1
    /// per frame and braking into the last piece), waits 3 frames at the end and retracts the
    /// same way (the alternate stays out and sleeps), then waits 10 frames at the base. Terrain
    /// just ahead of the growing head stops it for 30 frames (PreStop), then it retracts. A
    /// stomp, a fireball or a shell tramples the head (a ground pound harder): it retracts 2
    /// pieces (4), lies flat 130 frames and carries on; trampled within reach of its base it
    /// retracts into the base and dies there.
    fn step_creeper(&mut self, level: &Level, path: u8, sleeps: bool) {
        let codes = self.creeper_codes(level, path);
        let n = creeper_pieces(codes).len() as i32;
        if self.phase == 0 {
            self.phase = creep::MOVE;
            self.creeper_enter_piece(0, false);
            self.w3.frame = CREEP_BASE_FRAME;
            self.w3.rate = 0.0;
            self.w3.track = 0.0;
        }
        if self.w3.cooldown > 0 {
            self.w3.cooldown -= 1;
        }
        match self.phase {
            // the sensor result is the last frame's: tested before this frame's growth
            creep::MOVE if n > 0 && self.creeper_blocked(level, codes) => {
                self.phase = creep::PRE_STOP;
                self.timer = CREEP_PRE_STOP;
                self.w3.rate = 0.0;
            }
            creep::PRE_STOP => {
                self.timer -= 1;
                if self.timer <= 0 {
                    if sleeps {
                        self.phase = creep::SLEEP;
                    } else {
                        self.creeper_move_back();
                    }
                }
            }
            creep::MOVE if n > 0 => {
                let last = self.w3.node >= n - 1;
                if last {
                    let gap = CREEP_PIECE_END - self.w3.track;
                    self.w3.track =
                        (self.w3.track + (gap * CREEP_BRAKE).max(CREEP_CRAWL)).min(CREEP_PIECE_END);
                    self.w3.rate = (self.w3.track - self.w3.frame).max(CREEP_MIN_RATE);
                } else {
                    self.w3.rate = ease(self.w3.rate, CREEP_RATE, CREEP_RATE_STEP);
                }
                self.w3.frame = (self.w3.frame + self.w3.rate).min(CREEP_PIECE_END);
                if self.w3.frame >= CREEP_PIECE_END {
                    if last {
                        self.w3.rate = 0.0;
                        if sleeps {
                            self.phase = creep::SLEEP;
                        } else {
                            self.phase = creep::WAIT;
                            self.timer = CREEP_WAIT_END;
                            self.w3.resume = creep::MOVE_BACK;
                        }
                    } else {
                        self.creeper_enter_piece(self.w3.node + 1, false);
                        self.w3.rate = CREEP_RATE;
                    }
                }
            }
            creep::MOVE_BACK if n > 0 => {
                let base = self.w3.node <= 0;
                if base {
                    self.w3.track -= (self.w3.track * CREEP_BRAKE).max(CREEP_CRAWL);
                    self.w3.track = self.w3.track.max(0.0);
                    self.w3.rate = (self.w3.track - self.w3.frame).min(-CREEP_MIN_RATE);
                } else {
                    self.w3.rate = ease(self.w3.rate, -CREEP_RATE, CREEP_RATE_STEP);
                }
                self.w3.frame = (self.w3.frame + self.w3.rate).max(0.0);
                if base && self.w3.frame <= CREEP_BASE_FRAME {
                    self.w3.rate = 0.0;
                    self.phase = creep::WAIT;
                    self.timer = CREEP_WAIT_BASE;
                    self.w3.resume = creep::MOVE;
                } else if self.w3.frame <= 0.0 {
                    self.creeper_enter_piece(self.w3.node - 1, true);
                    self.w3.rate = -CREEP_RATE;
                }
            }
            creep::WAIT => {
                self.timer -= 1;
                if self.timer <= 0 && n > 0 {
                    if self.w3.resume == creep::MOVE {
                        // the tracker restarts from 0 creeping out
                        self.phase = creep::MOVE;
                        self.w3.rate = 0.0;
                        self.w3.track = 0.0;
                    } else {
                        self.creeper_move_back();
                    }
                }
            }
            creep::DAMAGE => {
                // retract at −0.6 to the goal piece while the trampled animation plays
                if self.w3.flags & flag::RETRACTED == 0 {
                    self.w3.frame = (self.w3.frame - CREEP_RATE).max(0.0);
                    let ended = self.w3.frame <= 0.0
                        || (self.w3.node == 0 && self.w3.frame <= CREEP_BASE_FRAME);
                    if ended {
                        let next = self.w3.node - 1;
                        if self.w3.goal < next {
                            self.creeper_enter_piece(next, true);
                        } else if self.w3.node < 1 {
                            // retracted into its base: it dies at once, not when the animation
                            // ends [obs 0x71007245d0, Damage exe 0x7100723f40]
                            self.w3.flags |= flag::DIES;
                            self.die_fall(self.x);
                            return;
                        } else {
                            self.creeper_enter_piece(self.w3.goal.max(0), true);
                            self.w3.flags |= flag::RETRACTED;
                        }
                    }
                }
                self.timer -= 1;
                if self.timer <= 0 && self.w3.flags & flag::RETRACTED != 0 {
                    self.phase = creep::DAMAGE_WAIT;
                    self.timer = CREEP_DAMAGE_WAIT;
                }
            }
            creep::DAMAGE_WAIT => {
                self.timer -= 1;
                if self.timer <= 0 {
                    if self.w3.flags & flag::WAS_BACK != 0 {
                        self.creeper_move_back();
                    } else {
                        self.phase = creep::MOVE;
                        self.w3.rate = 0.0;
                        self.w3.track = 0.0;
                    }
                }
            }
            _ => {} // Sleep
        }
        let k = self.w3.node.clamp(0, (n - 1).max(0)) as usize;
        let (hx, hy) = self.creeper_point_at(codes, k, self.w3.frame);
        self.x = hx;
        self.y = hy - CREEP_HEAD;
        // it faces along the piece it is in
        if let Some(&(_, o)) = creeper_pieces(codes).get(k) {
            self.heading = match o {
                0 => -90.0,
                1 => 0.0,
                2 => 90.0,
                _ => 180.0,
            };
            match o {
                1 => self.dir = 1,
                3 => self.dir = -1,
                _ => {}
            }
        }
    }

    /// MoveReverse enter: the rate eases in from 0, the tracker starts at the head's frame [obs
    /// 0x7100723530].
    fn creeper_move_back(&mut self) {
        self.phase = creep::MOVE_BACK;
        self.w3.rate = 0.0;
        self.w3.track = self.w3.frame;
    }

    /// The head's terrain sensor meets something on the side the growing piece leads to [obs
    /// 0x7100722e30; the sensor ignores the creepers' own stalk colliders; solid actors are not
    /// seen here].
    fn creeper_blocked(&self, level: &Level, codes: &[u8]) -> bool {
        let k = self.w3.node.max(0) as usize;
        let Some(&(_, o)) = creeper_pieces(codes).get(k) else {
            return false;
        };
        let (hx, hy) = self.creeper_point_at(codes, k, self.w3.frame);
        let (dx, dy) = DIRS[o as usize];
        let reach = CREEP_SENSOR_AHEAD + CREEP_SENSOR_HALF;
        let (px, py) = (hx + dx * reach, hy + dy * reach);
        let span = if dx != 0.0 {
            CREEP_SENSOR_SPAN.0
        } else {
            CREEP_SENSOR_SPAN.1
        };
        [-1.0, -0.5, 0.0, 0.5, 1.0].iter().any(|&f| {
            let (ox, oy) = (dy.abs() * span * f, dx.abs() * span * f);
            level.cell_at(px + ox, py + oy).is_solid()
        })
    }

    /// A hit on the head tramples it (Damage → DamageWait), retracting 2 pieces (4 for a ground
    /// `pound`) [obs 0x710071e460]. Not within 16 frames of the last hit; a later hit tramples it
    /// again, further. Returns true if it took the hit.
    pub fn creeper_trample(&mut self, pound: bool) -> bool {
        let pieces = if pound {
            CREEP_RETRACT_POUND
        } else {
            CREEP_RETRACT
        };
        if self.w3.cooldown > 0 || self.mode != super::Mode::Normal {
            return false;
        }
        self.w3.cooldown = CREEP_COOLDOWN;
        let back = if self.creeper_trampled() {
            self.w3.flags & flag::WAS_BACK != 0
        } else {
            self.phase == creep::MOVE_BACK
                || (self.phase == creep::WAIT && self.w3.resume == creep::MOVE_BACK)
        };
        self.w3.flags = if back { flag::WAS_BACK } else { 0 };
        self.w3.goal = (self.w3.node - pieces).max(-1);
        self.phase = creep::DAMAGE;
        self.timer = CREEP_TRAMPLED;
        self.w3.rate = 0.0;
        true
    }

    /// Within 16 frames of a hit the head's touch does nothing at all.
    pub fn creeper_shielded(&self) -> bool {
        self.w3.cooldown > 0
    }

    /// The head is trampled (flat, retracting or lying still; it still hurts to touch).
    pub fn creeper_trampled(&self) -> bool {
        matches!(self.phase, creep::DAMAGE | creep::DAMAGE_WAIT)
    }

    /// The creeper is asleep at the end of its path (alternate).
    pub fn creeper_asleep(&self) -> bool {
        self.phase == creep::SLEEP
    }

    // ------------------------------------------------------------ hits and looks

    /// A block bumped from below under it: true if that knocks it off (blkhit DieFall in each
    /// EnemyDB row) [obs].
    pub fn w3_block_hit(&self) -> bool {
        matches!(
            self.kind,
            Kind::AntTrooper { .. }
                | Kind::Stingby
                | Kind::Skipsqueak { .. }
                | Kind::HopChops
                | Kind::Bully
                | Kind::Porcupuffer
        )
    }

    /// Model and animation frame. The 3D World models (`3W_Enemy_…`) are reached through the
    /// `M1_` name like every other enemy; frame stems name the model's animations.
    pub fn w3_look(&self) -> Option<W3Look> {
        // 3D World-only models: the mesh path draws at (size / 16) × the model's own size, and
        // these models are authored at game scale
        const M: f32 = 16.0;
        let flash = self.kick_lock > 0 && (self.kick_lock / 2) % 2 == 0;
        if flash && self.kind == Kind::Porcupuffer {
            return None;
        }
        let face = self.dir > 0;
        let stomped = self.mode == super::Mode::Stomped;
        Some(match self.kind {
            Kind::AntTrooper { horned } => (
                if horned {
                    "M1_Enemy_ariheitsuno"
                } else {
                    "M1_Enemy_arihei"
                },
                if stomped {
                    "trampled.0"
                } else if self.wings {
                    "waitfly.0"
                } else {
                    "walk.0"
                },
                M,
                M,
                face,
            ),
            Kind::Stingby => (
                "M1_Enemy_hacchin",
                if stomped {
                    "pressdown.0"
                } else if self.phase == sting::FIND {
                    "find.0"
                } else {
                    "walk.0"
                },
                M,
                M,
                face,
            ),
            Kind::Skipsqueak { spiny } => (
                if spiny {
                    "M1_Enemy_pyonchu_hari"
                } else {
                    "M1_Enemy_pyonchu"
                },
                match self.phase {
                    _ if stomped => "pressdown.0",
                    skip::CROUCH => "jumpstandingsign.0",
                    skip::WALL_SIGN => "jumpsign.0",
                    skip::JUMP | skip::CRASH | skip::WALL_JUMP => "jump.0",
                    skip::LAND => "jumpstandingend.0",
                    skip::FALL => "fall.0",
                    skip::RETURN => "run.0",
                    skip::GIVE_UP => "giveup.0",
                    _ => "wait.0",
                },
                M,
                M,
                face,
            ),
            Kind::HopChops => (
                "M1_Enemy_hopper",
                match self.phase {
                    _ if stomped => "blowdown.0",
                    hop::TURN => "turn.0",
                    hop::FIND => "find.0",
                    hop::RUN => "run.0",
                    hop::LOST => "giveup.0",
                    _ if self.wings => "waitfly.0",
                    _ if !self.on_ground => "jump.0",
                    _ => "walk.0",
                },
                M,
                M,
                face,
            ),
            Kind::Porcupuffer => (
                "M1_Enemy_fugumannen",
                match self.phase {
                    puff::RECOVER => "damagereset.0",
                    puff::LUNGE => "attack.0",
                    puff::SWIM_TURN => "reaction.0",
                    puff::FLOAT => "floatup.0",
                    puff::SURFACE_JUMP => "jump.0",
                    puff::SURFACE => "swimfloat.0",
                    puff::FALL => "wait.0",
                    _ => "swim.0",
                },
                M,
                M,
                face,
            ),
            Kind::Bully => (
                "M1_Enemy_donketsu",
                match self.phase {
                    bully::FLIP if self.w3.tries as i32 == BULLY_PUSH_FRONT => "pushfront.0",
                    bully::FLIP => "pushback.0",
                    bully::HOLD_OUT => "stayfront.0",
                    bully::LAND | bully::FALL_STAN => "flyhitland.0",
                    bully::FALL => "flyhitfall.0",
                    bully::FIND => "find.0",
                    bully::CHASE => "run.0",
                    bully::WALK => "walk.0",
                    bully::FLY_CHASE | bully::FLY_LAND | bully::FLY_RETURN => "waitfly.0",
                    _ if !self.on_ground => "fall.0",
                    _ => "wait.0",
                },
                M,
                M,
                face,
            ),
            Kind::Charvaargh => (
                "M1_Enemy_magmafish",
                match self.phase {
                    _ if self.charvaargh_vertical() => {
                        let rear = self.w3.flags & chr::REAR != 0;
                        match (self.phase, rear) {
                            (chr::SIGN, false) => "attackfrontsign.0",
                            (chr::SIGN, true) => "attackrearsign.0",
                            (chr::START, false) => "attackfrontstart.0",
                            (chr::START, true) => "attackrearstart.0",
                            (chr::ATTACK, false) => "attackfront.0",
                            (chr::ATTACK, true) => "attackrear.0",
                            (_, false) => "attackfrontend.0",
                            (_, true) => "attackrearend.0",
                        }
                    }
                    chr::SIGN | chr::WAIT if face => "attacksidesignright.0",
                    chr::SIGN | chr::WAIT => "attacksidesignleft.0",
                    chr::START => "attacksidestart.0",
                    chr::ATTACK => "attackside.0",
                    chr::END => "attacksideend.0",
                    _ => "fall.0",
                },
                M,
                M,
                face,
            ),
            Kind::PiranhaCreeper { .. } => (
                "M1_Enemy_packunpipe_head",
                match self.phase {
                    creep::DAMAGE => "trampled.0",
                    creep::DAMAGE_WAIT => "trampledloop.0",
                    creep::SLEEP => "sleep.0",
                    _ if self.mode == super::Mode::DieFall => "down.0",
                    _ => "move.0",
                },
                M,
                M,
                face,
            ),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    fn on_flat(kind: Kind, x: f32) -> (Level, Actor) {
        let lvl = Level::flat(40);
        let mut a = Actor::new(kind, x, 32.0);
        a.dir = 1;
        a.on_ground = true;
        (lvl, a)
    }

    fn run(a: &mut Actor, lvl: &Level, frames: usize) {
        for _ in 0..frames {
            a.step_spawning(lvl);
        }
    }

    #[test]
    fn animation_lengths_round_up() {
        assert_eq!(anim_len(37.0, 1.0), 37);
        assert_eq!(anim_len(35.0, 0.9), 39);
        assert_eq!(anim_len(17.0, 2.0), 9);
        assert_eq!(yaw_frames(120.0), 15);
    }

    #[test]
    fn creeper_node_codes_follow_the_game_table() {
        // straight 1 left, 2 right, 3 down, 4 up; corners lead out of the turn; end caps point
        // back along the path
        assert_eq!(creeper_out(1), 3);
        assert_eq!(creeper_out(2), 1);
        assert_eq!(creeper_out(3), 0);
        assert_eq!(creeper_out(4), 2);
        assert_eq!(creeper_out(5), 0); // left → down
        assert_eq!(creeper_out(12), 1); // up → right
        assert_eq!(creeper_out(14), 1); // end cap of a path heading left
        assert_eq!(creeper_in(12), 2);
        assert_eq!(creeper_in(14), 3);
        assert!(creeper_corner(9) && !creeper_corner(13));
        // up, up, up→right, end: 3 pieces, the last a corner
        assert_eq!(
            creeper_pieces(&[4, 4, 12, 13]),
            vec![(2, 2), (2, 2), (2, 2), (2, 1)]
        );
    }

    #[test]
    fn ant_trooper_climbs_a_wall_and_wraps_over_its_top() {
        let (mut lvl, mut a) = on_flat(Kind::AntTrooper { horned: false }, 100.0);
        // a 2-tile wall at tiles x 10, rows 2–3 (x 160–176, top y 64)
        lvl.set(10, 2, Cell::Solid);
        lvl.set(10, 3, Cell::Solid);
        let mut sides = Vec::new();
        for _ in 0..400 {
            a.step_spawning(&lvl);
            if sides.last() != Some(&a.ant_side()) {
                sides.push(a.ant_side());
            }
        }
        // floor → the wall on its right → on top of the block → down its far side → floor
        assert_eq!(&sides[..4], &[0, 1, 0, 3], "sides {sides:?}");
        assert!(a.x > 176.0, "went over the wall: x {}", a.x);
        assert!((a.y - 32.0).abs() < 0.5, "back on the floor: y {}", a.y);
    }

    #[test]
    fn a_winged_ant_trooper_flies_level_bobbing_8_px_and_turns_at_walls() {
        let mut lvl = Level::flat(40);
        // a wall at x 400
        for y in 2..12 {
            lvl.set(25, y, Cell::Solid);
        }
        let mut a = Actor::new(Kind::AntTrooper { horned: true }, 200.0, 100.0);
        a.wings = true;
        a.dir = 1;
        let (mut lo, mut hi, mut top) = (f32::MAX, f32::MIN, 0.0f32);
        let mut crossings: Vec<i32> = Vec::new();
        let mut turn = None;
        for t in 0..400 {
            let before = a.y;
            a.step_spawning(&lvl);
            lo = lo.min(a.y);
            hi = hi.max(a.y);
            top = top.max(a.vx.abs());
            if before < 104.0 && a.y >= 104.0 {
                crossings.push(t);
            }
            if a.phase == ant::FLY_TURN && turn.is_none() {
                turn = Some((t, a.vx));
            }
        }
        assert!(
            (lo - 100.0).abs() < 0.1 && (hi - 108.1).abs() < 0.3,
            "bob {lo}…{hi}"
        );
        assert!(
            (crossings[1] - crossings[0] - 180).abs() <= 1,
            "{crossings:?}"
        );
        assert!((top - ANT_FLY).abs() < 1e-4, "speed {top}");
        let (t0, _) = turn.expect("turned at the wall");
        assert_eq!(a.dir, -1);
        // the turn holds for the 19-frame yaw swing
        let mut b = Actor::new(Kind::AntTrooper { horned: false }, 380.0, 100.0);
        b.wings = true;
        b.dir = 1;
        let mut frames = 0;
        let mut seen = false;
        for _ in 0..200 {
            b.step_spawning(&lvl);
            if b.phase == ant::FLY_TURN {
                seen = true;
                frames += 1;
            } else if seen {
                break;
            }
        }
        assert_eq!(frames, yaw_frames(2.0 * ANT_DIR_DEG), "turn after {t0}");
    }

    #[test]
    fn horned_ant_trooper_walks_at_the_plain_speed() {
        let (lvl, mut a) = on_flat(Kind::AntTrooper { horned: true }, 100.0);
        run(&mut a, &lvl, 5);
        assert!((a.vx - ANT_WALK).abs() < 1e-6, "vx {}", a.vx);
    }

    #[test]
    fn ant_trooper_wraps_under_a_ledge_at_6_degrees_a_frame() {
        // a floating platform (tiles 4–9, row 6): walking right off its end wraps under it
        let mut lvl = Level::flat(40);
        for x in 4..10 {
            lvl.set(x, 6, Cell::Solid);
        }
        let mut a = Actor::new(Kind::AntTrooper { horned: false }, 120.0, 112.0);
        a.dir = 1;
        a.on_ground = true;
        let mut wrap_frames = 0;
        let mut ceiling = false;
        for _ in 0..200 {
            a.step_spawning(&lvl);
            if a.phase == ant::WRAP {
                wrap_frames += 1;
            }
            ceiling |= a.ant_side() == 2;
        }
        // two corners (top → right face, right face → underside), 15 frames each
        assert_eq!(wrap_frames, 30);
        assert!(ceiling, "crawled onto the underside");
        assert!(a.y < 96.0 && a.y > 70.0, "under the platform: y {}", a.y);
    }

    #[test]
    fn stingby_patrols_32_px_then_charges_a_level_player() {
        let (lvl, mut a) = on_flat(Kind::Stingby, 200.0);
        a.y = 64.0;
        a.target = (600.0, 0.0); // far away and below
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for _ in 0..1200 {
            a.step_spawning(&lvl);
            lo = lo.min(a.x);
            hi = hi.max(a.x);
        }
        assert!(hi - lo <= 34.0 && hi - lo >= 30.0, "span {}", hi - lo);
        assert!(a.vx.abs() <= STING_PATROL + 1e-4);
        // a player 80 px ahead at the same height: the 37-frame alert, then the charge
        a.target = (a.x + 80.0 * a.dir as f32, a.y);
        a.w3.player_h = 16.0;
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, sting::FIND);
        run(&mut a, &lvl, 36);
        assert_eq!(a.phase, sting::FIND);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, sting::CHASE);
        run(&mut a, &lvl, 40);
        assert!((a.vx.abs() - STING_CHARGE).abs() < 1e-4, "vx {}", a.vx);
    }

    #[test]
    fn stingby_turns_round_for_15_frames_before_the_alert() {
        let (lvl, mut a) = on_flat(Kind::Stingby, 200.0);
        a.y = 64.0;
        a.w3.player_h = 16.0;
        a.target = (140.0, 64.0); // behind it
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, sting::TURN);
        assert_eq!(a.dir, -1, "faces the player from the start of the turn");
        run(&mut a, &lvl, 14);
        assert_eq!(a.phase, sting::TURN);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, sting::FIND);
    }

    #[test]
    fn stingby_gives_up_after_100_frames_out_of_range() {
        let (lvl, mut a) = on_flat(Kind::Stingby, 200.0);
        a.y = 64.0;
        a.w3.player_h = 16.0;
        a.target = (260.0, 64.0);
        run(&mut a, &lvl, 37 + 5);
        assert_eq!(a.phase, sting::CHASE);
        a.target = (a.x - 400.0, 64.0); // far behind
        run(&mut a, &lvl, 99);
        assert_eq!(a.phase, sting::CHASE);
        run(&mut a, &lvl, 2);
        assert_eq!(a.phase, sting::FLY);
    }

    #[test]
    fn skipsqueak_jumps_with_the_player_and_runs_home() {
        let (lvl, mut a) = on_flat(Kind::Skipsqueak { spiny: false }, 200.0);
        a.target = (150.0, 32.0);
        run(&mut a, &lvl, 5);
        assert_eq!(a.phase, skip::WAIT);
        // the player jumps: a 17-frame crouch, then a 2.7 take-off at −0.103125 (apex ≈ 35 px)
        a.w3.player_rising = true;
        run(&mut a, &lvl, 1);
        a.w3.player_rising = false;
        assert_eq!(a.phase, skip::CROUCH);
        run(&mut a, &lvl, 16);
        assert_eq!(a.phase, skip::CROUCH);
        let mut top: f32 = 0.0;
        for _ in 0..120 {
            a.step_spawning(&lvl);
            top = top.max(a.y - 32.0);
        }
        assert!((top - 34.0).abs() < 0.5, "apex {top}");
        assert_eq!(a.phase, skip::WAIT);
        assert!((a.x - 200.0).abs() < 1e-3, "straight up");
        // pushed 40 px away it turns, runs back and stops on its spot
        a.x = 240.0;
        run(&mut a, &lvl, 300);
        assert!((a.x - 200.0).abs() <= 1.0, "home x {}", a.x);
        assert_eq!(a.phase, skip::WAIT);
    }

    #[test]
    fn skipsqueak_off_its_ledge_tries_three_times_then_takes_a_new_home() {
        // home on a 4-tile-high pillar it cannot jump back onto from the floor
        let mut lvl = Level::flat(40);
        for y in 2..6 {
            lvl.set(12, y, Cell::Solid);
        }
        let mut a = Actor::new(Kind::Skipsqueak { spiny: false }, 200.0, 96.0);
        a.target = (100.0, 32.0);
        run(&mut a, &lvl, 10);
        assert!((a.home_y - 96.0).abs() < 0.01);
        // knocked to the floor beside the pillar
        a.x = 230.0;
        a.y = 32.0;
        a.home_x = 230.0; // displaced straight down: only the height is wrong
        let mut jumps = 0;
        let mut was_air = false;
        let mut gave_up = false;
        for _ in 0..800 {
            a.step_spawning(&lvl);
            let air = a.phase == skip::JUMP;
            if air && !was_air {
                jumps += 1;
            }
            was_air = air;
            gave_up |= a.phase == skip::GIVE_UP;
        }
        assert_eq!(jumps, 3, "three recovery jumps");
        assert!(gave_up);
        assert!((a.home_y - 32.0).abs() < 0.5, "the floor is its new home");
        assert_eq!(a.phase, skip::WAIT);
    }

    #[test]
    fn skipsqueak_bounces_off_a_wall_then_wall_jumps() {
        let mut lvl = Level::flat(40);
        lvl.set(14, 2, Cell::Solid); // a 1-tile step at x 224–240
        let mut a = Actor::new(Kind::Skipsqueak { spiny: false }, 200.0, 32.0);
        a.target = (100.0, 32.0);
        run(&mut a, &lvl, 5);
        a.home_x = 260.0; // home beyond the step
        let mut seen = Vec::new();
        for _ in 0..400 {
            a.step_spawning(&lvl);
            if seen.last() != Some(&a.phase) {
                seen.push(a.phase);
            }
        }
        assert!(seen.contains(&skip::CRASH), "{seen:?}");
        assert!(seen.contains(&skip::WALL_JUMP), "{seen:?}");
        assert!(a.x > 240.0, "got over the step: x {}", a.x);
    }

    #[test]
    fn hop_chops_hops_every_50_frames_then_turns_and_chases_a_player_ahead() {
        let (lvl, mut a) = on_flat(Kind::HopChops, 300.0);
        a.target = (0.0, 300.0); // out of sight
        let mut take_offs = Vec::new();
        let mut was_ground = true;
        let mut turned = false;
        for t in 0..400 {
            a.step_spawning(&lvl);
            if was_ground && !a.on_ground && a.phase == hop::WALK {
                take_offs.push(t);
            }
            was_ground = a.on_ground;
            turned |= a.phase == hop::TURN;
        }
        assert!(take_offs.len() >= 2, "{take_offs:?}");
        assert_eq!(take_offs[1] - take_offs[0], 50, "{take_offs:?}");
        assert!(turned, "turned after its hops");
        // a player 64 px ahead (it looks while on the ground): startled hop, then the chase
        while !(a.on_ground && a.phase == hop::WALK) {
            a.step_spawning(&lvl);
        }
        a.target = (a.x + 64.0 * a.dir as f32, 32.0);
        run(&mut a, &lvl, 1);
        assert!(a.hop_chops_running());
        assert!(a.vy > 2.0, "startled hop {}", a.vy);
        let mut top: f32 = 0.0;
        let mut high: f32 = 0.0;
        for _ in 0..160 {
            a.target = (a.x + 64.0 * a.dir as f32, 32.0);
            a.step_spawning(&lvl);
            if a.phase == hop::RUN {
                top = top.max(a.vx.abs());
                high = high.max(a.y - 32.0);
            }
        }
        assert_eq!(a.phase, hop::RUN);
        assert!((top - HOP_RUN_VX).abs() < 1e-4, "chase speed {top}");
        assert!(high < 0.5, "the run does not leave the ground: {high}");
        // the player gone behind: it turns to face them; far away: lost after 16 frames
        a.target = (a.x - 400.0 * a.dir as f32, 32.0);
        run(&mut a, &lvl, 18);
        assert_eq!(a.phase, hop::LOST);
    }

    #[test]
    fn porcupuffer_loses_speed_with_hp_and_dies_on_the_fourth_hit() {
        let mut a = Actor::new(Kind::Porcupuffer, 100.0, 100.0);
        assert_eq!(a.porcupuffer_hp(), 4);
        a.phase = puff::SWIM;
        a.hops = PUFF_HP;
        let speeds: Vec<f32> = (0..4)
            .map(|k| {
                let m = a.puff_m();
                if k < 3 {
                    assert!(!a.porcupuffer_hit(0.0));
                } else {
                    assert!(a.porcupuffer_hit(0.0));
                }
                m * PUFF_SWIM
            })
            .collect();
        assert_eq!(speeds, [1.25, 1.125, 1.0, 0.9375]);
        assert_eq!(a.mode, super::super::Mode::DieFall);
    }

    #[test]
    fn porcupuffer_recovers_for_60_frames_after_a_hit() {
        let mut lvl = Level::flat(40);
        lvl.top = 432.0;
        let mut a = Actor::new(Kind::Porcupuffer, 100.0, 100.0);
        a.w3.flooded = true;
        a.w3.player_wet = true;
        a.target = (0.0, 300.0);
        run(&mut a, &lvl, 2);
        assert!(!a.porcupuffer_hit(0.0));
        run(&mut a, &lvl, 59);
        assert_eq!(a.phase, puff::RECOVER);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, puff::SWIM);
    }

    #[test]
    fn porcupuffer_homes_in_under_water_and_lunges_when_lined_up() {
        let mut lvl = Level::flat(40);
        lvl.top = 432.0;
        let mut a = Actor::new(Kind::Porcupuffer, 100.0, 100.0);
        a.w3.flooded = true;
        a.w3.player_wet = true;
        a.w3.player_h = 16.0;
        a.dir = 1;
        // the player level with its aim point, 200 px ahead
        a.target = (300.0, 100.0);
        let mut lunged = false;
        let mut top: f32 = 0.0;
        for _ in 0..200 {
            a.step_spawning(&lvl);
            lunged |= a.phase == puff::LUNGE;
            if a.phase == puff::SWIM && !lunged {
                top = top.max(a.speed);
            }
        }
        assert!((top - PUFF_SWIM).abs() < 1e-4, "cruise {top}");
        assert!(lunged, "lunged when close");
    }

    #[test]
    fn porcupuffer_falls_to_the_surface_and_patrols_its_band() {
        let lvl = Level::flat(80);
        let mut a = Actor::new(Kind::Porcupuffer, 400.0, 200.0);
        a.w3.liquid = Some((120.0, true));
        a.target = (0.0, 300.0);
        a.dir = 1;
        run(&mut a, &lvl, 200);
        assert_eq!(a.phase, puff::SURFACE);
        let f = 120.0 - 4.0 - 16.0;
        assert!((a.y - f).abs() < 1.0, "floats at {}", a.y);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for _ in 0..1500 {
            a.step_spawning(&lvl);
            lo = lo.min(a.x);
            hi = hi.max(a.x);
        }
        assert!(hi - 400.0 <= PUFF_BAND + 20.0 && 400.0 - lo <= PUFF_BAND + 20.0);
        assert!(
            hi - lo > 2.0 * PUFF_BAND - 10.0,
            "patrols both ways: {lo}…{hi}"
        );
    }

    #[test]
    fn bully_charges_a_player_ahead_and_is_only_pushed_back() {
        let (lvl, mut a) = on_flat(Kind::Bully, 300.0);
        a.dir = -1;
        a.w3.player_h = 16.0;
        a.target = (200.0, 32.0); // 100 px ahead
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, bully::FIND);
        run(&mut a, &lvl, 38);
        assert_eq!(a.phase, bully::FIND);
        run(&mut a, &lvl, 60);
        assert!(a.bully_charging());
        assert!((a.vx + BULLY_CHASE).abs() < 0.05, "charge vx {}", a.vx);
        a.bully_push(push::STOMP, a.x - 10.0);
        assert!((a.vx - 1.75).abs() < 1e-4);
        assert!(a.bully_pushed());
        run(&mut a, &lvl, 10);
        assert!(
            (a.vx - (1.75 - 0.5)).abs() < 1e-3,
            "decays 0.05/f: {}",
            a.vx
        );
        // it stops after 35 frames; the PushBack animation (pushed against its facing) ends at
        // 39; 10 frames' recovery; then it charges its attacker
        let mut n = 10;
        while a.bully_pushed() && n < 200 {
            a.step_spawning(&lvl);
            n += 1;
        }
        assert_eq!(n, 49, "push lasted {n} frames");
        assert!(a.bully_charging());
        assert!(a.alive());
    }

    #[test]
    fn bully_does_not_see_a_player_behind_it() {
        let (lvl, mut a) = on_flat(Kind::Bully, 300.0);
        a.dir = 1;
        a.w3.player_h = 16.0;
        a.target = (250.0, 32.0); // behind
        run(&mut a, &lvl, 10);
        assert_eq!(a.phase, bully::WAIT);
    }

    #[test]
    fn a_pushed_bully_stops_on_a_ledge_edge() {
        let mut lvl = Level::flat(40);
        for x in 21..40 {
            lvl.set(x, 0, Cell::Empty);
            lvl.set(x, 1, Cell::Empty);
        }
        let mut a = Actor::new(Kind::Bully, 318.0, 32.0);
        a.dir = -1;
        a.on_ground = true;
        a.target = (0.0, 400.0);
        run(&mut a, &lvl, 2);
        a.bully_push(push::LIGHT, 300.0); // pushed right at 1.0 towards the drop at x 336
        let mut held = false;
        for _ in 0..80 {
            a.step_spawning(&lvl);
            held |= a.phase == bully::HOLD_OUT;
        }
        assert!(held, "teetered on the edge");
        assert!(a.y > 31.0, "did not fall: y {}", a.y);
    }

    #[test]
    fn a_winged_bully_flies_up_a_wall_after_a_player_above_and_lands_by_them() {
        let mut lvl = Level::flat(40);
        // a pillar at x 304–336, top y 112
        for x in 19..21 {
            for y in 2..7 {
                lvl.set(x, y, Cell::Solid);
            }
        }
        let mut a = Actor::new(Kind::Bully, 270.0, 32.0);
        a.wings = true;
        a.dir = 1;
        a.on_ground = true;
        a.w3.player_h = 16.0;
        a.target = (322.0, 112.0); // on the pillar, well above: only the winged sight box sees it
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, bully::FIND, "winged sight reaches up");
        let mut seen = Vec::new();
        let mut top_speed: f32 = 0.0;
        for _ in 0..400 {
            a.step_spawning(&lvl);
            if seen.last() != Some(&a.phase) {
                seen.push(a.phase);
            }
            if a.phase == bully::FLY_CHASE {
                top_speed = top_speed.max(a.vy);
            }
        }
        let order = [bully::CHASE, bully::FLY_CHASE, bully::FLY_LAND, bully::LAND];
        let at: Vec<usize> = order
            .iter()
            .map(|p| {
                seen.iter()
                    .position(|q| q == p)
                    .unwrap_or_else(|| panic!("{p} missing: {seen:?}"))
            })
            .collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert!(
            (top_speed - BULLY_FLY_RISE).abs() < 1e-4,
            "climbs at {top_speed}"
        );
        assert!(a.y >= 111.0 && a.on_ground, "on the pillar: {}", a.y);
    }

    #[test]
    fn a_winged_bully_knocked_off_a_ledge_flies_back_onto_it() {
        let mut lvl = Level::flat(40);
        // a ledge at x 160–336, top y 160
        for x in 10..21 {
            for y in 2..10 {
                lvl.set(x, y, Cell::Solid);
            }
        }
        let mut a = Actor::new(Kind::Bully, 330.0, 160.0);
        a.wings = true;
        a.dir = 1;
        a.on_ground = true;
        a.target = (0.0, 600.0);
        run(&mut a, &lvl, 2);
        a.bully_push_by(push::HEAVY, 310.0, false); // shoved right off it, too fast to teeter
        let mut seen = Vec::new();
        for _ in 0..300 {
            a.step_spawning(&lvl);
            if seen.last() != Some(&a.phase) {
                seen.push(a.phase);
            }
        }
        assert!(seen.contains(&bully::FALL), "{seen:?}");
        assert!(seen.contains(&bully::FLY_RETURN), "{seen:?}");
        assert!(seen.contains(&bully::FLY_LAND), "{seen:?}");
        assert!((a.y - 160.0).abs() < 1.0, "back on the ledge: y {}", a.y);
        assert!(a.x < 352.0, "over it: x {}", a.x);
    }

    #[test]
    fn a_stomp_drops_a_flying_bully_and_a_heavy_hit_pushes_it() {
        let mut a = Actor::new(Kind::Bully, 300.0, 120.0);
        a.wings = true;
        a.phase = bully::FLY_CHASE;
        a.timer = BULLY_FLY_CHASE;
        a.bully_push(push::STOMP, 290.0);
        assert_eq!(a.phase, bully::FALL, "dropped");
        assert_eq!(a.vx, 0.0, "no push");
        assert!(a.wings, "it keeps its wings");
        a.phase = bully::FLY_RETURN;
        a.bully_push(push::HEAVY, 290.0);
        assert!(a.bully_pushed());
        assert!(a.vx > 2.0);
    }

    #[test]
    fn bully_gives_up_beyond_256_px() {
        let (lvl, mut a) = on_flat(Kind::Bully, 300.0);
        a.dir = -1;
        a.w3.player_h = 16.0;
        a.target = (200.0, 32.0);
        run(&mut a, &lvl, 39 + 5);
        assert_eq!(a.phase, bully::CHASE);
        a.target = (a.x + 300.0, 32.0);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, bully::WAIT);
    }

    #[test]
    fn charvaargh_warns_then_leaps_its_arc_and_dives_back() {
        let mut lvl = Level::flat(40);
        lvl.theme = 2;
        let mut a = Actor::new(Kind::Charvaargh, 200.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = (300.0, 48.0); // beyond 96 px: the side leap
        a.camera = (150.0, 0.0); // the dive ends in view
        run(&mut a, &lvl, 2);
        let base = 44.0 - 16.0;
        assert_eq!(a.phase, chr::WAIT);
        assert!((a.charvaargh_head().1 - (base + 4.0)).abs() < 0.01);
        assert!(a.charvaargh_segments().is_empty(), "harmless at rest");
        run(&mut a, &lvl, 29);
        assert_eq!(a.phase, chr::SIGN);
        assert_eq!(a.dir, 1, "leaps toward the player");
        run(&mut a, &lvl, CHAR_SIGN as usize);
        assert_eq!(a.phase, chr::START);
        let x0 = a.x;
        let (mut top, mut frames) = (f32::MIN, 0);
        while a.phase != chr::WAIT && frames < 800 {
            a.step_spawning(&lvl);
            frames += 1;
            top = top.max(a.charvaargh_head().1 - base);
            if a.phase == chr::ATTACK {
                assert_eq!(a.charvaargh_segments().len(), 8);
            }
        }
        // ≈ 148 px up and ≈ 224 px across; 150 + 190 frames, then the dive (≈ 100 frames)
        assert!((top - 148.0).abs() < 4.0, "apex {top}");
        let across = a.x - x0;
        assert!((across - 224.0).abs() < 4.0, "across {across}");
        assert!((430..460).contains(&frames), "frames {frames}");
        assert!(a.alive());
        assert_eq!(
            a.charvaargh_record().0,
            Some(a.x),
            "its side spot is on record"
        );
    }

    /// A Charvaargh resting in castle lava at x 200 (base 28), its wait run down to the warning.
    fn charvaargh_to_sign(lvl: &Level, player: (f32, f32)) -> Actor {
        let mut a = Actor::new(Kind::Charvaargh, 200.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = player;
        a.camera = (100.0, 0.0);
        run(&mut a, lvl, 2);
        assert_eq!(a.phase, chr::WAIT);
        run(&mut a, lvl, 29);
        assert_eq!(a.phase, chr::SIGN);
        a
    }

    #[test]
    fn charvaargh_leaps_vertically_under_a_player_close_above_open_space() {
        let mut lvl = Level::flat(40);
        lvl.theme = 2;
        let base = 44.0 - 16.0;
        let mut a = charvaargh_to_sign(&lvl, (250.0, 48.0));
        assert!(a.charvaargh_vertical(), "within 96 px, nothing overhead");
        assert_eq!(a.x, 250.0, "snapped under the player");
        assert_eq!(a.charvaargh_record().1, Some(250.0));
        // the 50-frame front/rear sign and 5 more
        run(&mut a, &lvl, CHAR_SIGN_VERT as usize - 1);
        assert_eq!(a.phase, chr::SIGN);
        run(&mut a, &lvl, 1);
        assert_eq!(a.phase, chr::START);
        let (mut top, mut touch, mut frames) = (f32::MIN, 0, 0);
        let mut xs = (f32::MAX, f32::MIN);
        while a.phase != chr::WAIT && frames < 600 {
            a.step_spawning(&lvl);
            frames += 1;
            let (hx, hy) = a.charvaargh_head();
            top = top.max(hy - base);
            xs = (xs.0.min(hx), xs.1.max(hx));
            if !a.charvaargh_segments().is_empty() {
                touch += 1;
            }
        }
        // the root bone rises 175 px from 32 under the base and dives back; it drifts ≈ 25 px
        assert!((top - 143.0).abs() < 2.0, "apex {top}");
        assert!(xs.1 - xs.0 < 30.0, "straight up and down: {xs:?}");
        // the body touches only while crossing the course's depth
        assert!(
            touch > 20 && touch < frames / 2,
            "touchable {touch} of {frames}"
        );
        assert!((350..380).contains(&frames), "frames {frames}");
        assert!(!a.charvaargh_vertical());
        assert_eq!(
            a.charvaargh_record().1,
            None,
            "its vertical spot is off the record"
        );
    }

    #[test]
    fn terrain_over_the_player_or_the_area_edge_keeps_charvaargh_to_the_side_leap() {
        let mut lvl = Level::flat(40);
        lvl.theme = 2;
        // a block 150 px above the base, over the player
        lvl.set(15, 11, Cell::Solid);
        let a = charvaargh_to_sign(&lvl, (250.0, 48.0));
        assert!(!a.charvaargh_vertical(), "terrain in the probes");
        assert_eq!(a.timer, CHAR_SIGN);
        // the player within 88 px of the area's left edge (main area)
        let lvl = {
            let mut l = Level::flat(40);
            l.theme = 2;
            l
        };
        let mut a = Actor::new(Kind::Charvaargh, 120.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = (60.0, 48.0);
        run(&mut a, &lvl, 31);
        assert_eq!(a.phase, chr::SIGN);
        assert!(!a.charvaargh_vertical());
    }

    #[test]
    fn a_dive_ending_out_of_view_chains_into_a_leap_from_the_views_edge() {
        let mut lvl = Level::flat(80);
        lvl.theme = 2;
        let mut a = Actor::new(Kind::Charvaargh, 200.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = (360.0, 48.0);
        a.camera = (0.0, 0.0);
        run(&mut a, &lvl, 2 + 29 + CHAR_SIGN as usize);
        assert_eq!(a.phase, chr::START);
        // the player runs on: the view is now 600…984
        a.camera = (600.0, 0.0);
        let mut n = 0;
        while a.phase != chr::END && n < 600 {
            a.step_spawning(&lvl);
            n += 1;
        }
        let mut n = 0;
        while a.phase == chr::END && n < 600 {
            a.step_spawning(&lvl);
            n += 1;
        }
        assert_eq!(a.phase, chr::START, "no wait, no warning");
        assert_eq!(a.x, 600.0 - CHAR_CHAIN_MARGIN, "just outside the view");
        assert_eq!(a.dir, 1, "facing the view's centre");
    }

    #[test]
    fn charvaargh_spacing_follows_the_game_rules() {
        // side: nothing near → no shift; one near → clear of it to the right; leaping left
        // after a clear one → to the left; a second near one reverses the move
        assert_eq!(charvaargh_space_side(&[400.0], 200.0, false), 0.0);
        assert_eq!(charvaargh_space_side(&[210.0], 200.0, false), 38.0);
        assert_eq!(charvaargh_space_side(&[210.0], 200.0, true), 38.0);
        assert_eq!(charvaargh_space_side(&[400.0, 210.0], 200.0, true), -38.0);
        assert_eq!(charvaargh_space_side(&[210.0, 250.0], 200.0, false), -38.0);
        // vertical: away from the near one; past it when an earlier one was clear
        assert_eq!(charvaargh_space_vert(&[210.0], 200.0), -46.0);
        assert_eq!(charvaargh_space_vert(&[190.0], 200.0), 46.0);
        assert_eq!(charvaargh_space_vert(&[400.0, 190.0], 200.0), -66.0);
        // a second near one: 56 px beyond the previous spot
        assert_eq!(
            charvaargh_space_vert(&[190.0, 250.0], 200.0),
            190.0 - 56.0 - 200.0
        );
    }

    #[test]
    fn a_charvaargh_whose_turn_is_taken_holds_its_wait_at_10() {
        let mut lvl = Level::flat(40);
        lvl.theme = 2;
        let mut a = Actor::new(Kind::Charvaargh, 200.0, 12.0);
        a.w3.liquid = Some((44.0, false));
        a.target = (400.0, 48.0);
        run(&mut a, &lvl, 2);
        a.charvaargh_sense(&[], &[], true);
        run(&mut a, &lvl, 100);
        assert_eq!(a.phase, chr::WAIT);
        assert_eq!(a.timer, CHAR_TURN_HOLD);
        a.charvaargh_sense(&[], &[], false);
        run(&mut a, &lvl, 10);
        assert_eq!(a.phase, chr::SIGN);
        // a turn holder lets it go when its warning ends
        a.charvaargh_take_turn();
        run(&mut a, &lvl, CHAR_SIGN as usize);
        assert_eq!(a.phase, chr::START);
        assert!(!a.charvaargh_record().2);
    }

    #[test]
    fn piranha_creeper_creeps_out_along_its_path_and_back() {
        let mut lvl = Level::flat(40);
        // up 2 pieces, then right (codes: up, up, up→right, end)
        lvl.creepers.push((0, vec![4, 4, 12, 13]));
        let path = Kind::PiranhaCreeper {
            path: 0,
            sleeps: false,
        };
        let mut a = Actor::new(path, 200.0, 32.0);
        run(&mut a, &lvl, 1);
        let codes = lvl.creepers[0].1.clone();
        // the head starts in the base piece (centre y 48), 3 frames in
        let (_, hy) = a.creeper_point(&codes);
        assert!(
            (hy - (48.0 - 20.0 + 32.0 * 3.1 / 12.0)).abs() < 0.1,
            "head y {hy}"
        );
        // 4 pieces (the last a corner) at ≈ 1.6 px/f: out within ~90 frames
        let mut out = 0;
        while a.phase == creep::MOVE && out < 400 {
            a.step_spawning(&lvl);
            out += 1;
        }
        assert!((60..110).contains(&out), "out in {out} frames");
        let (hx, hy) = a.creeper_point(&codes);
        // the corner piece at (200, 144) ends 12 px to its right … 11/12 of the way
        assert!(
            (hx - 209.6).abs() < 0.2 && (hy - 143.8).abs() < 0.2,
            "end ({hx}, {hy})"
        );
        assert_eq!(a.creeper_stalk(&codes).len(), 3);
        // 3 frames' wait, back down, then 10 frames at the base
        run(&mut a, &lvl, 3);
        assert_eq!(a.phase, creep::MOVE_BACK);
        let mut back = 0;
        while a.phase == creep::MOVE_BACK && back < 400 {
            a.step_spawning(&lvl);
            back += 1;
        }
        assert_eq!(a.phase, creep::WAIT);
        let (_, hy) = a.creeper_point(&codes);
        assert!((hy - 36.0).abs() < 1.0, "back at the base: {hy}");
        run(&mut a, &lvl, 10);
        assert_eq!(a.phase, creep::MOVE);
    }

    #[test]
    fn sleeping_creeper_stays_out_and_a_trampled_one_retracts_then_resumes() {
        let mut lvl = Level::flat(40);
        lvl.creepers.push((3, vec![2, 2, 2, 2, 14]));
        let mut a = Actor::new(
            Kind::PiranhaCreeper {
                path: 3,
                sleeps: true,
            },
            100.0,
            32.0,
        );
        run(&mut a, &lvl, 70);
        let codes = lvl.creepers[0].1.clone();
        let node = a.w3.node;
        assert!(node >= 3, "piece {node}");
        let x = a.x;
        assert!(a.creeper_trample(false));
        assert!(!a.creeper_trample(false), "already flat");
        run(&mut a, &lvl, CREEP_TRAMPLED as usize);
        assert_eq!(a.phase, creep::DAMAGE_WAIT);
        assert_eq!(a.w3.node, node - 2, "retracted 2 pieces");
        assert!(a.x < x - 40.0, "x {} from {x}", a.x);
        run(&mut a, &lvl, CREEP_DAMAGE_WAIT as usize + 200);
        assert!(a.creeper_asleep());
        let (hx, _) = a.creeper_point(&codes);
        assert!(
            (hx - (100.0 + 4.0 * 32.0 + 12.0 - 32.0 / 12.0)).abs() < 1.0,
            "x {hx}"
        );
    }

    #[test]
    fn terrain_ahead_stops_a_growing_creeper_for_30_frames_then_it_retracts() {
        for sleeps in [false, true] {
            let mut lvl = Level::flat(40);
            // straight up from (100, 32); a block at y 112–128 over the second piece
            lvl.creepers.push((0, vec![4, 4, 4, 4, 4, 15]));
            lvl.set(6, 7, Cell::Solid);
            let mut a = Actor::new(Kind::PiranhaCreeper { path: 0, sleeps }, 100.0, 32.0);
            let mut n = 0;
            while a.phase != creep::PRE_STOP && n < 200 {
                a.step_spawning(&lvl);
                n += 1;
            }
            assert_eq!(a.phase, creep::PRE_STOP, "stopped");
            assert_eq!(a.w3.node, 1);
            let y = a.y;
            assert!(y + 2.0 * CREEP_HEAD < 112.0, "short of the block: {y}");
            run(&mut a, &lvl, CREEP_PRE_STOP as usize - 1);
            assert_eq!(a.phase, creep::PRE_STOP);
            assert_eq!(a.y, y, "frozen");
            run(&mut a, &lvl, 1);
            let next = if sleeps {
                creep::SLEEP
            } else {
                creep::MOVE_BACK
            };
            assert_eq!(a.phase, next);
        }
    }

    #[test]
    fn a_creeper_trampled_near_its_base_dies() {
        let mut lvl = Level::flat(40);
        lvl.creepers.push((0, vec![4, 4, 4, 15]));
        let mut a = Actor::new(
            Kind::PiranhaCreeper {
                path: 0,
                sleeps: false,
            },
            100.0,
            32.0,
        );
        run(&mut a, &lvl, 15);
        assert!(a.w3.node <= 1);
        assert!(a.creeper_trample(false));
        // it dies as soon as it is back in its base, before the Trampled animation ends
        let mut n = 0;
        while a.mode == super::super::Mode::Normal && n < 100 {
            a.step_spawning(&lvl);
            n += 1;
        }
        assert_eq!(a.mode, super::super::Mode::DieFall);
        assert!(n < CREEP_TRAMPLED, "died after {n} frames");
    }

    #[test]
    fn a_ground_pound_tramples_a_creeper_4_pieces_back_and_hits_need_16_frames_apart() {
        let mut lvl = Level::flat(40);
        lvl.creepers.push((0, vec![2, 2, 2, 2, 2, 2, 2, 14]));
        let mut a = Actor::new(
            Kind::PiranhaCreeper {
                path: 0,
                sleeps: true,
            },
            100.0,
            32.0,
        );
        run(&mut a, &lvl, 200);
        assert!(a.creeper_asleep());
        let node = a.w3.node;
        assert!(a.creeper_trample(true));
        assert!(!a.creeper_trample(false), "within 16 frames");
        let flat = |a: &mut Actor| {
            let mut n = 0;
            while a.phase != creep::DAMAGE_WAIT && n < 200 {
                a.step_spawning(&lvl);
                n += 1;
            }
        };
        flat(&mut a);
        assert_eq!(a.w3.node, node - 4);
        // trampled again once the 16 frames are up: further back
        assert!(a.creeper_trample(false));
        flat(&mut a);
        assert_eq!(a.w3.node, node - 6);
    }
}
