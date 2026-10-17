//! Actors: enemies, items and the player's fireballs.
//!
//! From the Phase 3 specs: objects.md §3 (generic walker), 0-goomba.md, 1-koopa.md,
//! 20-super-mushroom.md (shared item movement and block emergence), 34-fire-flower.md (fireballs),
//! 35-super-star.md, 33-one-up.md and the shared enemy draft (death, emergence, activation).
//! Positions are the feet centre (px, y up); boxes extend half_w either side and h upward.

use crate::level::{Cell, Level, PipeDir, TILE};

mod bosses;
pub use bosses::{BossAttack, BossHit};

pub const WALK_SPEED: f32 = 0.5;
pub const ENEMY_GRAVITY: f32 = -0.1875;
pub const MAX_FALL: f32 = -4.0;
pub const TURN_LOCK: i32 = 8;
pub const STOMP_FRAMES: i32 = 30;
pub const SHELL_WAKE: i32 = 511;
pub const SHELL_SHAKE: i32 = 96;
pub const ITEM_SPEED: f32 = 1.0;
pub const FIREBALL_SPEED: f32 = 3.625;
pub const FIREBALL_GRAVITY: f32 = -0.4375;
pub const FIREBALL_BOUNCE: f32 = 4.0;
pub const FIREBALL_LIFE: i32 = 600;
/// Piranha Plant height (EnemyDB tall_max 24; texture 16×24).
pub const PIRANHA_H: f32 = 24.0;
// DieFall default launch (shared enemy draft §6.2)
const DIE_SPEED: f32 = 1.5;
const DIE_VY: f32 = 3.9;
const DIE_GRAVITY: f32 = -0.24375;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Goomba,
    Koopa {
        red: bool,
    },
    /// A Koopa in its shell (still or sliding).
    Shell {
        red: bool,
    },
    Mushroom,
    FireFlower,
    Star,
    OneUp,
    Fireball,
    /// Piranha Plant (fire = alternate form). Free-standing, or cycling in a pipe.
    Piranha {
        fire: bool,
    },
    /// 10/30/50-coin (value 0, 1, 2).
    BigCoin {
        value: u8,
    },
    /// Fire Piranha Plant's fireball: straight line at 1 px/frame.
    EnemyFire,
    /// Thwomp (side = Side Thwomp, alternate form). A solid that slams down.
    Thwomp {
        side: bool,
    },
    /// One-way lift, `width` tiles; falling = drops once stood on.
    Lift {
        width: u8,
        falling: bool,
    },
    /// Trampoline.
    Spring,
    /// Bullet Bill from a blaster: flies straight at 1 px/frame, through terrain.
    BulletBill,
    /// P Switch: pressed from above.
    PSwitch,
    /// Muncher: invulnerable solid hazard.
    Muncher,
    /// A Donut block that has dropped (falls g −0.1875, cap −2, carries riders).
    Donut,
    /// Spiny: walker that can't be stomped.
    Spiny,
    /// Buzzy Beetle: walker, shell when stomped, immune to fire.
    Buzzy,
    /// Spike Top: [inf simplified] walks the floor turning at ledges; can't be stomped; fire-proof.
    SpikeTop,
    /// Hammer Bro (paces, jumps, throws hammers).
    HammerBro,
    /// Hammer: arcs (vx 1.5, vy 3.0, g −0.125, cap −6), through terrain.
    Hammer,
    /// Bob-omb: lit by a stomp (fuse 240), can be kicked, explodes.
    BobOmb,
    /// Dry Bones: walker that turns at ledges; collapses for 224 frames when stomped.
    DryBones,
    /// Boo: chases when not looked at.
    Boo,
    /// Banzai Bill: 64×64, flies 1 px/frame (dir 0 right, 1 down, 2 left, 3 up).
    BanzaiBill {
        dir: u8,
    },
    /// Bumper: bounces the player (82-bumper.md).
    Bumper,
    /// Grinder (saw): hazard circle around (x, y + 24).
    Grinder,
    /// Seesaw plank `width` tiles wide pivoting at its centre (91-seesaw.md).
    Seesaw {
        width: u8,
    },
    /// Airship cannonball: straight line at 1 px/frame (velocity in vx, vy).
    Cannonball,
    /// Lakitu: hovers near the top of the view, drifts ±48 px around an anchor that follows the
    /// player, throws Spiny eggs every 60–119 frames (30-lakitu.md).
    Lakitu,
    /// A thrown Spiny egg: falls, becomes a Spiny on landing.
    SpinyEgg,
    /// Rideable Lakitu cloud (31-lakitu-cloud.md): bobs in place; the rider steers it.
    Cloud,
    /// POW block: a head hit from below sets off a quake (19-pow-block.md).
    Pow,
    /// Key: collect to open a locked door.
    Key,
    /// Twister: walks 0.5 px/frame; its wind column (grows 0.7/frame to 128 px) lifts riders.
    Twister,
    /// Swinging claw (105): a claw on a rope swinging from a pivot; grabs the player.
    Claw,
    /// Cheep Cheep (red = alternate): swims in water, flops and hops out of it.
    CheepCheep {
        red: bool,
    },
    /// Blooper: pulses toward the player, then sinks (48-blooper.md).
    Blooper,
    /// Skewer (83): a spiked pillar that thrusts out 144 px (dir 0 down, 1 left, 2 up, 3 right).
    Skewer {
        dir: u8,
    },
    /// Magikoopa: teleports near the view centre, casts magic at the player (39-magikoopa.md).
    Magikoopa,
    /// Magikoopa's magic: straight line at 0.75 px/frame (velocity in vx, vy).
    Magic,
    /// Rocky Wrench: pops out of its manhole and throws a wrench (58-rocky-wrench.md).
    RockyWrench,
    /// Thrown wrench: rises 0–30 px then flies at 1 px/frame; 480-frame life.
    Wrench,
    /// Wiggler: head + 4 trailing segments; a stomp makes it angry (52-wiggler.md).
    Wiggler,
    /// Lava Bubble (60): leaps from the lava (or the area floor) to its placed height.
    LavaBubble,
    /// Monty Mole (102): buried until the player comes near, then chases.
    MontyMole,
    /// Angry Sun (104), screen-locked swooper; `moon` = the alternate form (wipes enemies).
    AngrySun {
        moon: bool,
    },
    /// Chain Chomp (61): chained to a stake at (home_x, home_y), or free (alternate form).
    ChainChomp {
        chained: bool,
    },
    /// Lava lift (skull raft, 64 px): drifts right at 0.8 (fast 2.5) once ridden.
    LavaLift {
        fast: bool,
    },
    /// Bowser (62): 20 HP (big 40), paces around home, jumps and breathes fire (actors/bosses.rs).
    /// `key`: holds a Key (child object 95) that drops on defeat; `clown`: rides a Koopa Clown
    /// Car (flag 0x200).
    Bowser {
        key: bool,
        clown: bool,
    },
    /// Bowser's (and Bowser Jr.'s) flame: flies level, settling onto its lane (`home_y`).
    BowserFire,
    /// Boom Boom (77): 3 HP plus a fireball counter; `pom` = Pom Pom (the 3D World alternate).
    BoomBoom {
        key: bool,
        clown: bool,
        pom: bool,
    },
    /// Pom Pom's shuriken: flies straight (velocity in vx, vy).
    Shuriken,
    /// Bowser Jr. (98): 3 HP plus a fireball counter.
    BowserJr {
        key: bool,
        clown: bool,
    },
}

impl Kind {
    pub fn is_enemy(self) -> bool {
        matches!(
            self,
            Kind::Goomba
                | Kind::Bowser { .. }
                | Kind::BoomBoom { .. }
                | Kind::BowserJr { .. }
                | Kind::Koopa { .. }
                | Kind::Shell { .. }
                | Kind::Piranha { .. }
                | Kind::BulletBill
                | Kind::BanzaiBill { .. }
                | Kind::Cannonball
                | Kind::Lakitu
                | Kind::SpinyEgg
                | Kind::Magikoopa
                | Kind::RockyWrench
                | Kind::Wiggler
                | Kind::Spiny
                | Kind::Buzzy
                | Kind::SpikeTop
                | Kind::HammerBro
                | Kind::BobOmb
                | Kind::DryBones
                | Kind::Boo
                | Kind::ChainChomp { .. }
                | Kind::LavaBubble
                | Kind::MontyMole
                | Kind::AngrySun { moon: false }
        )
    }
    /// Clear-condition enemy class (clear.rs) of a defeated actor of this kind.
    pub fn defeat_class(self, buzzy: bool) -> Option<u8> {
        use crate::clear::enemy as e;
        Some(match self {
            Kind::Goomba => e::GOOMBA,
            Kind::Bowser { .. } => e::BOWSER,
            Kind::BoomBoom { pom: true, .. } => e::POM_POM,
            Kind::BoomBoom { .. } => e::BOOM_BOOM,
            Kind::BowserJr { .. } => e::BOWSER_JR,
            Kind::Koopa { .. } => e::KOOPA,
            Kind::Shell { .. } if buzzy => e::BUZZY,
            Kind::Shell { .. } => e::KOOPA,
            Kind::Buzzy => e::BUZZY,
            Kind::Spiny | Kind::SpinyEgg => e::SPINY,
            Kind::Piranha { fire: false } => e::PIRANHA,
            Kind::Piranha { fire: true } => e::FIRE_PIRANHA,
            Kind::Muncher => e::MUNCHER,
            Kind::DryBones => e::DRY_BONES,
            Kind::RockyWrench => e::ROCKY_WRENCH,
            Kind::HammerBro => e::HAMMER_BRO,
            Kind::Lakitu => e::LAKITU,
            Kind::Blooper => e::BLOOPER,
            Kind::CheepCheep { .. } => e::CHEEP_CHEEP,
            Kind::SpikeTop => e::SPIKE_TOP,
            Kind::ChainChomp { .. } => e::CHAIN_CHOMP,
            Kind::LavaBubble => e::LAVA_BUBBLE,
            Kind::MontyMole => e::MONTY_MOLE,
            Kind::AngrySun { moon: false } => e::ANGRY_SUN,
            Kind::Wiggler => e::WIGGLER,
            Kind::BobOmb => e::BOB_OMB,
            Kind::Thwomp { .. } => e::THWOMP,
            Kind::Boo => e::BOO,
            Kind::BanzaiBill { .. } => e::BANZAI_BILL,
            Kind::Magikoopa => e::MAGIKOOPA,
            Kind::BulletBill => e::BULLET_BILL,
            _ => return None,
        })
    }
    /// Stomps bounce off without harming it or the player (EnemyDB "Invalid").
    pub fn stomp_bounces(self) -> bool {
        matches!(self, Kind::ChainChomp { .. })
    }
    /// Walks along the ground (turns at walls and other walkers).
    pub fn is_walker(self) -> bool {
        matches!(
            self,
            Kind::Goomba
                | Kind::Koopa { .. }
                | Kind::Shell { .. }
                | Kind::Spiny
                | Kind::Buzzy
                | Kind::SpikeTop
                | Kind::BobOmb
                | Kind::DryBones
        )
    }
    /// Hurts the player on touch but cannot be stomped.
    pub fn unstompable(self) -> bool {
        matches!(
            self,
            Kind::Piranha { .. }
                | Kind::Bowser { .. }
                | Kind::BowserFire
                | Kind::Shuriken
                | Kind::EnemyFire
                | Kind::Spiny
                | Kind::SpikeTop
                | Kind::Boo
                | Kind::Hammer
                | Kind::LavaBubble
                | Kind::AngrySun { .. }
        )
    }
    /// Walkers that turn back at ledges.
    pub fn ledge_turner(self) -> bool {
        matches!(
            self,
            Kind::Koopa { red: true } | Kind::SpikeTop | Kind::DryBones
        )
    }
    /// Moves before the player and offers a solid surface (riders are carried).
    pub fn is_platform(self) -> bool {
        matches!(
            self,
            Kind::Thwomp { .. }
                | Kind::Lift { .. }
                | Kind::Spring
                | Kind::Muncher
                | Kind::Donut
                | Kind::Seesaw { .. }
                | Kind::Cloud
                | Kind::Pow
                | Kind::LavaLift { .. }
        )
    }
    pub fn is_item(self) -> bool {
        matches!(
            self,
            Kind::Mushroom | Kind::FireFlower | Kind::Star | Kind::OneUp
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Item rising out of (or sinking below) a block at constant speed, no gravity, until the
    /// feet reach `until`.
    Emerge {
        until: f32,
    },
    /// Enemy popping out of a block (vy 2.5 up, gravity on) for `timer` frames, no collisions.
    PopOut,
    /// Coming out of a pipe mouth toward (tx, ty): 1 px/frame vertically, 0.5 sideways, no
    /// collisions (shared enemy draft §5.3).
    DokanOut {
        tx: f32,
        ty: f32,
    },
    /// Drifting down under a parachute (shared enemy draft §5.2): gravity −0.01875, max fall
    /// −0.4, swinging ±24 px around x0 (2.5°/frame); landing drops the parachute.
    Parachute {
        x0: f32,
        phase: f32,
    },
    Normal,
    /// Flattened Goomba (DieStamp), deleted when the timer runs out.
    Stomped,
    /// Knocked off (DieFall): no collision, falls off the area.
    DieFall,
    ShellWait,
    ShellSlide,
    /// Remove at the end of the frame.
    Dead,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub kind: Kind,
    pub mode: Mode,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// +1 right, −1 left
    pub dir: i32,
    pub on_ground: bool,
    pub big: bool,
    pub wings: bool,
    pub timer: i32,
    pub turn_lock: i32,
    pub age: u32,
    /// Frames during which a just-kicked shell cannot hurt the kicker.
    pub kick_lock: i32,
    /// Kill chain (shells) / stomp chain bookkeeping.
    pub chain: u32,
    /// Paragoomba hop cycle and pause.
    pub hops: u8,
    pub pause: i32,
    pub home_y: f32,
    pub home_x: f32,
    /// Index into World::spawns, if this actor came from a course object.
    pub spawn: Option<usize>,
    /// Hit a wall this frame (in the movement direction).
    pub wall: bool,
    /// Pipe this actor lives in (pipe Piranha) and its mouth (entry point).
    pub pipe: Option<(PipeDir, f32, f32)>,
    /// Index of the pipe generator that made it.
    pub generator: Option<usize>,
    /// Behaviour phase for actors with small state machines (Piranha).
    pub phase: u8,
    /// Wiggler: recent head positions (newest last), for the trailing segments.
    pub trail: Vec<(f32, f32)>,
    /// Per-actor random state (xorshift32), seeded at spawn.
    pub rng: u32,
    /// Nearest player's position, set by the world before each step.
    pub target: (f32, f32),
    /// Stable id (moving solids are tracked by it).
    pub id: u32,
    /// Something stands on this platform (set by the world).
    pub ridden: bool,
    /// Position change this frame (for carrying riders).
    pub moved: (f32, f32),
    /// This shell belongs to a Buzzy Beetle (fire-proof; wakes as a Buzzy).
    pub buzzy: bool,
    /// Floating movers (Boo): speed along `heading` (degrees).
    pub speed: f32,
    pub heading: f32,
    /// The player's facing (+1 right), set by the world.
    pub target_facing: i32,
    /// Riding a track, and the offset from the track point to (x, y).
    pub rail: Option<crate::track::Rail>,
    pub rail_off: (f32, f32),
    /// Counted toward a "defeat N" clear condition.
    pub tallied: bool,
    /// Set by the world each frame: the camera's bottom-left corner, and the reference level a
    /// Lava Bubble leaps from (the liquid surface, else the area bottom − 12).
    pub camera: (f32, f32),
    pub floor_ref: f32,
}

impl Actor {
    pub fn new(kind: Kind, x: f32, y: f32) -> Self {
        Self {
            kind,
            mode: Mode::Normal,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            dir: 1,
            on_ground: false,
            big: false,
            wings: false,
            timer: 0,
            turn_lock: 0,
            age: 0,
            kick_lock: 0,
            chain: 0,
            hops: 0,
            pause: 0,
            home_y: y,
            home_x: x,
            spawn: None,
            wall: false,
            pipe: None,
            trail: Vec::new(),
            generator: None,
            phase: 0,
            rng: 0x9e37_79b9 ^ (x.to_bits().rotate_left(7)) ^ y.to_bits(),
            target: (0.0, 0.0),
            id: 0,
            ridden: false,
            moved: (0.0, 0.0),
            buzzy: false,
            speed: 0.0,
            heading: 0.0,
            target_facing: 1,
            rail: None,
            rail_off: (0.0, 0.0),
            tallied: false,
            camera: (0.0, 0.0),
            floor_ref: 0.0,
        }
    }

    /// Fireballs bounce off / are snuffed without killing.
    pub fn fire_proof(&self) -> bool {
        matches!(
            self.kind,
            Kind::Buzzy
                | Kind::SpikeTop
                | Kind::DryBones
                | Kind::Boo
                | Kind::Thwomp { .. }
                | Kind::ChainChomp { .. }
                | Kind::LavaBubble
                | Kind::AngrySun { .. }
        ) || (matches!(self.kind, Kind::Shell { .. }) && self.buzzy)
    }

    /// The solid surface this actor offers (for platforms): (x0, y0, x1, y1, top_only).
    pub fn solid_box(&self) -> Option<(f32, f32, f32, f32, bool)> {
        match self.kind {
            Kind::Thwomp { .. } if self.mode == Mode::Normal => {
                let hw = if self.big { 32.0 } else { 16.0 };
                Some((self.x - hw, self.y, self.x + hw, self.y + 2.0 * hw, false))
            }
            Kind::Lift { width, .. } => {
                let hw = width as f32 * 8.0;
                Some((self.x - hw, self.y + 8.0, self.x + hw, self.y + 16.0, true))
            }
            Kind::Muncher if self.mode == Mode::Normal => {
                Some((self.x - 8.0, self.y, self.x + 8.0, self.y + 16.0, false))
            }
            Kind::Cloud => Some((
                self.x - 16.0,
                self.y + 4.0,
                self.x + 16.0,
                self.y + 12.0,
                true,
            )),
            Kind::LavaLift { .. } => Some((
                self.x - 32.0,
                self.y + 8.0,
                self.x + 32.0,
                self.y + 16.0,
                true,
            )),
            Kind::Pow if self.mode == Mode::Normal => {
                Some((self.x - 8.0, self.y, self.x + 8.0, self.y + 16.0, false))
            }
            Kind::Seesaw { width } => {
                let hw = width as f32 * 8.0;
                Some((self.x - hw, self.y + 8.0, self.x + hw, self.y + 16.0, true))
            }
            Kind::Donut => Some((
                self.x - 8.0,
                self.y + 8.0,
                self.x + 8.0,
                self.y + 16.0,
                true,
            )),
            Kind::Spring if self.mode == Mode::Normal => {
                let top = self.y + 12.5 - self.aux_compression();
                Some((self.x - 8.0, top - 4.0, self.x + 8.0, top, true))
            }
            _ => None,
        }
    }

    /// Top-surface slope of a platform (seesaw tilt: `heading` in degrees).
    pub fn solid_slope(&self) -> f32 {
        match self.kind {
            Kind::Seesaw { .. } => self.heading.to_radians().tan(),
            _ => 0.0,
        }
    }

    /// Seesaw (91-seesaw.md): riders' lever arm tilts the plank. Tip speed ω (`speed`) gains
    /// lever / (6·64·W²) per frame (at least 0.0015), the angle advances 2·asin(ω/16); limits
    /// ±45° with bounce e = (W + 3)/36; free damping ω × 0.95 and a slow return to level.
    /// Rider weight 1 per px of lever arm [inf].
    fn step_seesaw(&mut self, width: u8) {
        let w = width as f32;
        let lever = if self.ridden {
            self.target.0 - self.x
        } else {
            0.0
        };
        if lever.abs() > 0.5 {
            // positive lever (rider on the right) pushes the right end down (negative angle)
            let mut a = lever / (6.0 * 64.0 * w * w);
            if a.abs() < 0.0015 {
                a = 0.0015 * a.signum();
            }
            self.speed -= a;
        } else {
            self.speed *= 0.95;
            let th = self.heading.abs().min(25.0);
            let back = (0.0025 * th / 25.0).max(0.001);
            if self.heading > 0.0 {
                self.speed -= back;
            } else if self.heading < 0.0 {
                self.speed += back;
            }
            if self.heading.abs() < 0.125 && self.speed.abs() < 0.00075 {
                self.heading = 0.0;
                self.speed = 0.0;
            }
        }
        self.speed = self.speed.clamp(-8.0, 8.0);
        self.heading += (2.0 * (self.speed / 16.0).clamp(-1.0, 1.0).asin()).to_degrees();
        if self.heading.abs() > 45.0 {
            self.heading = 45.0 * self.heading.signum();
            self.speed = -self.speed * (w + 3.0) / 36.0;
        }
    }

    /// Lakitu: anchor x (home_x) follows the player by at most max(|player vx|, 0.05) — the
    /// player's x-speed comes in `speed`; drift toward ±1.5 (accel 0.05), turning 48 px either
    /// side of the anchor; throw every 60–119 frames (release 30 frames into the throw); after
    /// 10 throws leave upward at 3.0 [inf budget].
    fn step_lakitu(&mut self) -> Option<Actor> {
        let (px, _) = self.target;
        if self.chain >= 10 {
            self.vy = (self.vy + 0.1).min(3.0);
            self.y += self.vy;
            return None;
        }
        let follow = self.speed.abs().max(0.05);
        self.home_x += (px - self.home_x).clamp(-follow, follow);
        if self.x > self.home_x + 48.0 {
            self.dir = -1;
        } else if self.x < self.home_x - 48.0 {
            self.dir = 1;
        }
        let t = 1.5 * self.dir as f32;
        self.vx += (t - self.vx).clamp(-0.05, 0.05);
        self.x += self.vx;
        if self.timer <= 0 {
            self.timer = 60 + self.rand(60) as i32;
            self.pause = 30;
        }
        self.timer -= 1;
        if self.pause > 0 {
            self.pause -= 1;
            if self.pause == 0 {
                self.chain += 1;
                let face = if px < self.x { -1.0 } else { 1.0 };
                let mut egg = Actor::new(Kind::SpinyEgg, self.x, self.y + 16.8);
                egg.vx = (self.vx.abs() * 0.9).max(0.5) * face;
                egg.vy = 2.0;
                egg.dir = face as i32;
                return Some(egg);
            }
        }
        None
    }

    /// Skewer extension along its direction (`speed` = velocity, `heading` = extension, px):
    /// wait 100, wind back to −34 (0.15/frame², ≤ 1.5), hold 40, thrust (0.5, ≤ 7) to 144,
    /// hold 60, retract (0.1, ≤ 2) to 0, repeat.
    fn step_skewer(&mut self) {
        match self.phase {
            0 => {
                self.timer += 1;
                if self.timer >= 100 {
                    self.phase = 1;
                    self.timer = 0;
                }
            }
            1 => {
                self.speed = (self.speed - 0.15).max(-1.5);
                self.heading = (self.heading + self.speed).max(-34.0);
                if self.heading <= -34.0 {
                    self.phase = 2;
                    self.speed = 0.0;
                }
            }
            2 => {
                self.timer += 1;
                if self.timer >= 40 {
                    self.phase = 3;
                    self.timer = 0;
                }
            }
            3 => {
                self.speed = (self.speed + 0.5).min(7.0);
                self.heading = (self.heading + self.speed).min(144.0);
                if self.heading >= 144.0 {
                    self.phase = 4;
                    self.speed = 0.0;
                }
            }
            4 => {
                self.timer += 1;
                if self.timer >= 60 {
                    self.phase = 5;
                    self.timer = 0;
                }
            }
            _ => {
                self.speed = (self.speed - 0.1).max(-2.0);
                self.heading = (self.heading + self.speed).max(0.0);
                if self.heading <= 0.0 {
                    self.phase = 0;
                    self.speed = 0.0;
                }
            }
        }
    }

    /// Rocky Wrench: hidden (30–59) → peek +2 at 0.2/frame for 96 → rise to +10 at 0.4 →
    /// throw → sink at 0.8. `heading` holds the height above home; only a risen one can be hit.
    fn step_rocky(&mut self) -> Option<Actor> {
        let (px, _) = self.target;
        self.dir = if px < self.x { -1 } else { 1 };
        let mut thrown = None;
        match self.phase {
            0 => {
                self.heading = 0.0;
                if self.timer <= 0 {
                    self.timer = 30 + self.rand(30) as i32;
                    self.phase = 1;
                }
            }
            1 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 2;
                    self.timer = 96;
                }
            }
            2 => {
                self.heading = (self.heading + 0.2).min(2.0);
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 3;
                }
            }
            3 => {
                self.heading = (self.heading + 0.4).min(10.0);
                if self.heading >= 10.0 {
                    self.phase = 4;
                    self.timer = 47;
                }
            }
            4 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    let d = self.dir as f32;
                    let mut w = Actor::new(Kind::Wrench, self.x - 2.0 * d, self.home_y + 28.0);
                    w.home_y = w.y + 5.0 * self.rand(7) as f32;
                    w.vx = d;
                    w.timer = 480;
                    thrown = Some(w);
                    self.phase = 5;
                }
            }
            _ => {
                self.heading = (self.heading - 0.8).max(0.0);
                if self.heading <= 0.0 {
                    self.phase = 0;
                    self.timer = 0;
                }
            }
        }
        self.y = self.home_y + self.heading;
        thrown
    }

    /// Wiggler segment positions (4, nearest first), 16 px apart along the head's path.
    pub fn wiggler_segments(&self) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let mut dist = 0.0;
        let mut want = 16.0;
        let mut prev = (self.x, self.y);
        for &p in self.trail.iter().rev() {
            dist += ((p.0 - prev.0).powi(2) + (p.1 - prev.1).powi(2)).sqrt();
            prev = p;
            if dist >= want {
                out.push(p);
                want += 16.0;
                if out.len() == 4 {
                    break;
                }
            }
        }
        while out.len() < 4 {
            let k = out.len() as f32 + 1.0;
            out.push((self.x - self.dir as f32 * 16.0 * k, self.y));
        }
        out
    }

    /// The Skewer's spiked head box: 64 wide × 32 deep at the extended end.
    pub fn skewer_head(&self) -> (f32, f32, f32, f32) {
        let (cx, cy) = (self.x, self.y + 32.0);
        let e = self.heading;
        match self.kind {
            Kind::Skewer { dir: 0 } => (cx - 32.0, cy - 32.0 - e, cx + 32.0, cy - e),
            Kind::Skewer { dir: 2 } => (cx - 32.0, cy + e, cx + 32.0, cy + 32.0 + e),
            Kind::Skewer { dir: 1 } => (cx - 32.0 - e, cy - 32.0, cx - e, cy + 32.0),
            _ => (cx + e, cy - 32.0, cx + 32.0 + e, cy + 32.0),
        }
    }

    /// Swinging claw tip (where a held player hangs): pivot (home) + rope of 40 px.
    pub fn claw_tip(&self) -> (f32, f32) {
        let a = self.heading.to_radians();
        (self.home_x + 40.0 * a.sin(), self.home_y - 40.0 * a.cos())
    }

    /// Spring compression c (0…4 px), kept in `pause`.
    pub fn aux_compression(&self) -> f32 {
        if self.kind == Kind::Spring {
            self.pause as f32
        } else {
            0.0
        }
    }

    /// Thwomp (12-thwomp.md): Wait (notices a player below within |dx| < W + 32) → Fall
    /// (g −0.25, cap −5) → FallWait 64 → Return (≤ 0.875, +0.2/frame) to the home height.
    /// Side Thwomp [inf]: charges toward a player level with it (0.075/frame² to 3.0), waits, returns.
    fn step_thwomp(&mut self, level: &Level, side: bool) {
        let (px, py) = self.target;
        let w = if self.big { 32.0 } else { 16.0 };
        match self.phase {
            0 => {
                self.vx = 0.0;
                self.vy = 0.0;
                let triggered = if side {
                    (py - self.y).abs() < 2.0 * w && (px - self.x).abs() < 256.0
                } else {
                    (px - self.x).abs() < w + 32.0 && py > self.y + w - 260.0 && py < self.y + w
                };
                if triggered {
                    self.phase = 1;
                    if side {
                        self.dir = if px < self.x { -1 } else { 1 };
                    }
                }
            }
            1 => {
                if side {
                    let t = 3.0 * self.dir as f32;
                    self.vx = if self.vx < t {
                        (self.vx + 0.075).min(t)
                    } else {
                        (self.vx - 0.075).max(t)
                    };
                    self.vy = 0.0;
                } else {
                    self.vy = (self.vy - 0.25).max(-5.0);
                }
                let before = (self.x, self.y);
                self.collide(level);
                let stuck = (self.x - before.0).abs() < 1e-5 && (self.y - before.1).abs() < 1e-5;
                if (!side && self.on_ground) || (side && self.wall) || stuck {
                    self.phase = 2;
                    self.timer = 64;
                    self.vx = 0.0;
                    self.vy = 0.0;
                }
            }
            2 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 3;
                }
            }
            _ => {
                if side {
                    let d = self.home_x - self.x;
                    let step = d.abs().min(0.875);
                    self.x += step * d.signum();
                    if d.abs() < 0.01 {
                        self.phase = 0;
                    }
                } else {
                    self.vy = (self.vy + 0.2).min(0.875);
                    let target = self.home_y;
                    self.y = (self.y + self.vy).min(target);
                    if self.y >= target {
                        self.y = target;
                        self.vy = 0.0;
                        self.phase = 0;
                    }
                }
            }
        }
    }

    fn rand(&mut self, n: u32) -> u32 {
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x % n
    }

    pub fn half_w(&self) -> f32 {
        match self.kind {
            Kind::Fireball | Kind::EnemyFire => 4.0,
            Kind::Bowser { .. } | Kind::BoomBoom { .. } if self.big => 32.0,
            Kind::Bowser { .. } | Kind::BoomBoom { .. } => 16.0,
            Kind::BowserFire => 12.0,
            Kind::Shuriken => 4.0,
            Kind::BigCoin { .. } => 15.0,
            Kind::BanzaiBill { .. } => 32.0,
            Kind::Thwomp { .. } => {
                if self.big {
                    32.0
                } else {
                    16.0
                }
            }
            Kind::Lift { width, .. } => width as f32 * 8.0,
            _ if self.big => 16.0,
            _ => 8.0,
        }
    }

    pub fn height(&self) -> f32 {
        match self.kind {
            Kind::Fireball | Kind::EnemyFire => 8.0,
            Kind::Bowser { .. } | Kind::BoomBoom { .. } if self.big => 64.0,
            Kind::Bowser { .. } | Kind::BoomBoom { .. } => 32.0,
            Kind::BowserFire | Kind::Shuriken => 8.0,
            Kind::BigCoin { .. } => 30.0,
            Kind::BanzaiBill { .. } => 64.0,
            Kind::Thwomp { .. } => {
                if self.big {
                    64.0
                } else {
                    32.0
                }
            }
            Kind::Piranha { .. } => PIRANHA_H * if self.big { 2.0 } else { 1.0 },
            Kind::Koopa { .. } if !self.big => 16.0, // body box 16×16 (art is 16×32)
            _ if self.big => 32.0,
            _ => 16.0,
        }
    }

    /// (x0, y0, x1, y1) hit box. A Piranha's body extends from its origin along its pipe's
    /// outward direction (down for a down-facing pipe, sideways for side pipes).
    pub fn bbox(&self) -> (f32, f32, f32, f32) {
        let hw = self.half_w();
        let h = self.height();
        match (self.kind, self.pipe) {
            (Kind::Piranha { .. }, Some((PipeDir::Down, _, _))) => {
                (self.x - hw, self.y - h, self.x + hw, self.y)
            }
            (Kind::Piranha { .. }, Some((PipeDir::Right, _, _))) => {
                (self.x, self.y - hw, self.x + h, self.y + hw)
            }
            (Kind::Piranha { .. }, Some((PipeDir::Left, _, _))) => {
                (self.x - h, self.y - hw, self.x, self.y + hw)
            }
            (Kind::BigCoin { .. }, _) => {
                (self.x - 15.0, self.y + 1.0, self.x + 15.0, self.y + 31.0)
            }
            _ => (self.x - hw, self.y, self.x + hw, self.y + h),
        }
    }

    pub fn alive(&self) -> bool {
        !matches!(self.mode, Mode::Stomped | Mode::DieFall | Mode::Dead)
    }

    /// Can touch the player / other actors.
    pub fn solid_contact(&self) -> bool {
        matches!(self.mode, Mode::Normal | Mode::ShellWait | Mode::ShellSlide)
            && !(matches!(self.kind, Kind::LavaBubble | Kind::MontyMole) && self.phase == 0)
            && !self.boss_hidden()
            && !(matches!(self.kind, Kind::Piranha { .. }) && self.piranha_hidden())
    }

    /// A pipe Piranha is harmless while less than 30 % of the way out.
    fn piranha_hidden(&self) -> bool {
        let Some((dir, px, py)) = self.pipe else {
            return false;
        };
        let travel = self.height() + if self.big { 4.0 } else { 12.0 };
        let out = match dir {
            PipeDir::Up => self.y - (py - travel),
            PipeDir::Down => (py + travel) - self.y,
            PipeDir::Right => self.x - (px - travel),
            PipeDir::Left => (px + travel) - self.x,
        };
        out <= 0.3 * travel
    }

    /// Knock off (DieFall) away from `from_x`.
    pub fn die_fall(&mut self, from_x: f32) {
        let dir = if self.x < from_x { -1.0 } else { 1.0 };
        self.mode = Mode::DieFall;
        self.vx = DIE_SPEED * dir;
        self.vy = DIE_VY;
    }

    /// Stomp reaction. Returns true if the actor took it (player bounces).
    pub fn stomp(&mut self) -> bool {
        if self.wings {
            // a stomp removes the wings (0-goomba.md, 1-koopa.md)
            self.wings = false;
            self.vy = self.vy.min(0.0);
            return true;
        }
        match self.kind {
            Kind::Buzzy => {
                self.kind = Kind::Shell { red: false };
                self.buzzy = true;
                self.enter_shell_wait();
                true
            }
            Kind::DryBones if self.phase == 0 => {
                // collapse into a pile of bones for 224 frames
                self.phase = 1;
                self.timer = 224;
                self.vx = 0.0;
                true
            }
            Kind::BobOmb if self.phase == 0 => {
                // fuse lit: it stops (the fuse runs 240 frames)
                self.phase = 1;
                self.timer = 240;
                self.vx = 0.0;
                true
            }
            Kind::HammerBro => {
                self.die_fall(self.x - self.dir as f32);
                true
            }
            Kind::Lakitu | Kind::Magikoopa | Kind::RockyWrench => {
                self.die_fall(self.x - 1.0);
                true
            }
            Kind::Wiggler => {
                // not hurt: angry after a 60-frame pause
                if self.phase == 0 {
                    self.phase = 1;
                    self.timer = 60;
                }
                true
            }
            Kind::BulletBill | Kind::BanzaiBill { .. } | Kind::Cannonball => {
                // DieStamp for a Bullet Bill: it drops away
                self.mode = Mode::DieFall;
                self.vx = 0.0;
                self.vy = 0.0;
                true
            }
            Kind::Goomba => {
                self.mode = Mode::Stomped;
                self.timer = STOMP_FRAMES;
                self.vx = 0.0;
                self.vy = 0.0;
                true
            }
            Kind::Koopa { red } => {
                self.kind = Kind::Shell { red };
                self.enter_shell_wait();
                true
            }
            // a stomp on a sliding shell stops it dead; on a still shell, nothing (the touch kicks
            // it instead, handled by the caller)
            Kind::Shell { .. } if self.mode == Mode::ShellSlide => {
                self.enter_shell_wait();
                true
            }
            _ => false,
        }
    }

    fn enter_shell_wait(&mut self) {
        self.mode = Mode::ShellWait;
        self.timer = SHELL_WAKE;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    /// Kick a still shell (1-koopa.md): 2.5, plus 0.5·|kicker vx| when the kicker moves in the
    /// kick direction.
    pub fn kick(&mut self, dir: i32, kicker_vx: f32) {
        let mut speed = 2.5;
        if kicker_vx != 0.0 && kicker_vx.signum() as i32 == dir {
            speed += 0.5 * kicker_vx.abs();
        }
        self.mode = Mode::ShellSlide;
        self.dir = dir;
        self.vx = speed * dir as f32;
        self.kick_lock = 16;
        self.chain = 0;
    }

    pub fn turn(&mut self) {
        self.dir = -self.dir;
        self.vx = -self.vx;
        self.turn_lock = TURN_LOCK;
    }

    /// One frame of behaviour and movement. `player_x` is used for facing rules.
    pub fn step(&mut self, level: &Level, player_x: f32) {
        self.target.0 = player_x;
        let _ = self.step_spawning(level);
    }

    /// One frame; may return a new actor to add (a Fire Piranha's fireball). Uses `target`.
    pub fn step_spawning(&mut self, level: &Level) -> Option<Actor> {
        let before = (self.x, self.y);
        let out = self.step_inner(level);
        self.ride_track(level);
        self.moved = (self.x - before.0, self.y - before.1);
        out
    }

    /// On a track the rail sets the position (the actor's own motion is replaced); knocked off
    /// (stomped, killed, kicked), it falls under its own rules.
    fn ride_track(&mut self, level: &Level) {
        let Some(mut r) = self.rail else { return };
        if !matches!(self.mode, Mode::Normal) {
            self.rail = None;
            return;
        }
        let before = r.point(&level.tracks);
        let (px, py) = r.step(&level.tracks);
        self.rail = Some(r);
        if self.kind == Kind::Claw {
            // the claw hangs from a pivot that rides the track
            self.home_x += px - before.0;
            self.home_y += py - before.1;
            return;
        }
        self.x = px + self.rail_off.0;
        self.y = py + self.rail_off.1;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    fn step_inner(&mut self, level: &Level) -> Option<Actor> {
        let player_x = self.target.0;
        let mut spawned = None;
        self.age += 1;
        self.wall = false;
        if self.kick_lock > 0 {
            self.kick_lock -= 1;
        }
        if self.turn_lock > 0 {
            self.turn_lock -= 1;
        }
        match self.mode {
            Mode::Dead => {}
            Mode::Emerge { until } => {
                self.y += self.vy;
                if (self.vy > 0.0 && self.y >= until) || (self.vy < 0.0 && self.y <= until) {
                    self.y = until;
                    self.mode = Mode::Normal;
                    self.vy = 0.0;
                    self.vx = match self.kind {
                        Kind::Mushroom | Kind::OneUp | Kind::Star => ITEM_SPEED * self.dir as f32,
                        _ => 0.0,
                    };
                    if self.kind == Kind::Star {
                        self.vy = FIREBALL_BOUNCE;
                    }
                }
            }
            Mode::DokanOut { tx, ty } => {
                let horizontal = (tx - self.x).abs() > 0.01;
                let speed = if horizontal { 0.5 } else { 1.0 };
                let step = |v: f32, t: f32| {
                    if (t - v).abs() <= speed {
                        t
                    } else {
                        v + speed * (t - v).signum()
                    }
                };
                self.x = step(self.x, tx);
                self.y = step(self.y, ty);
                if self.x == tx && self.y == ty {
                    self.mode = Mode::Normal;
                    self.vx = match self.kind {
                        Kind::Mushroom | Kind::OneUp | Kind::Star => ITEM_SPEED * self.dir as f32,
                        _ => 0.0,
                    };
                    self.vy = if matches!(self.pipe, Some((PipeDir::Down, _, _))) {
                        -1.0
                    } else {
                        0.0
                    };
                    self.pipe = None;
                }
            }
            Mode::Parachute { x0, phase } => {
                self.vy = (self.vy - 0.01875).max(-0.4);
                let phase = phase + 2.5;
                let nx = x0 + 24.0 * phase.to_radians().sin();
                self.vx = nx - self.x;
                self.collide(level);
                self.mode = if self.on_ground {
                    self.vx = 0.0;
                    Mode::Normal
                } else {
                    Mode::Parachute { x0, phase }
                };
            }
            Mode::PopOut => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.y += self.vy;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Normal;
                }
            }
            Mode::Stomped => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Dead;
                }
            }
            Mode::DieFall => {
                self.vy = (self.vy + DIE_GRAVITY).max(MAX_FALL);
                self.x += self.vx;
                self.y += self.vy;
                if self.y < -4.0 * TILE {
                    self.mode = Mode::Dead;
                }
            }
            Mode::Normal => spawned = self.step_normal(level, player_x),
            Mode::ShellWait => {
                if self.on_ground && self.vy <= 0.0 {
                    self.vx = 0.0;
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                self.timer -= 1;
                if self.timer <= 0
                    && let Kind::Shell { red } = self.kind
                {
                    // the Koopa (or Buzzy) walks out, facing the player
                    self.kind = if self.buzzy {
                        Kind::Buzzy
                    } else {
                        Kind::Koopa { red }
                    };
                    self.buzzy = false;
                    self.mode = Mode::Normal;
                    self.dir = if player_x <= self.x { -1 } else { 1 };
                }
            }
            Mode::ShellSlide => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall {
                    // reverse, keeping the speed (the caller hits the block in the way)
                    self.dir = -self.dir;
                    self.vx = -self.vx;
                }
                if self.vx.abs() < 0.001 {
                    self.enter_shell_wait();
                }
            }
        }
        if self.mode == Mode::Normal
            && let Kind::Piranha { fire } = self.kind
        {
            spawned = self.step_piranha(fire);
        }
        if self.y < -4.0 * TILE && self.mode != Mode::DieFall {
            self.mode = Mode::Dead; // fell into a pit
        }
        spawned
    }

    /// Piranha Plant (2-piranha-plant.md). Free-standing: bites in place (and a Fire Piranha
    /// spits every 48–95 + 40 frames). In a pipe: hidden 72 → rise 1 px/f → out 96 → sink.
    fn step_piranha(&mut self, fire: bool) -> Option<Actor> {
        let (px, py) = self.target;
        if fire {
            self.dir = if px <= self.x { -1 } else { 1 };
        }
        let Some((dir, mx, my)) = self.pipe else {
            // free-standing: phase 0 = waiting to spit (timer), 1 = spitting
            if !fire {
                return None;
            }
            return self.fire_cycle(px, py);
        };
        let travel = self.height() + if self.big { 4.0 } else { 12.0 };
        let u = match dir {
            PipeDir::Up => (0.0, 1.0),
            PipeDir::Down => (0.0, -1.0),
            PipeDir::Right => (1.0, 0.0),
            PipeDir::Left => (-1.0, 0.0),
        };
        let hidden = (mx - u.0 * travel, my - u.1 * travel);
        let out = (mx, my);
        let towards = |a: &mut Actor, t: (f32, f32)| {
            let step = |v: f32, t: f32| {
                if (t - v).abs() <= 1.0 {
                    t
                } else {
                    v + (t - v).signum()
                }
            };
            a.x = step(a.x, t.0);
            a.y = step(a.y, t.1);
            a.x == t.0 && a.y == t.1
        };
        let mut shot = None;
        match self.phase {
            // hidden; a player at the mouth keeps it in (timer ≥ 60)
            0 => {
                let at_mouth = match dir {
                    PipeDir::Up => (px - mx).abs() <= 28.0 && py <= my + 16.0 && py >= my - 32.0,
                    PipeDir::Down => (px - mx).abs() <= 19.0 && py <= my && py >= my - 35.0,
                    PipeDir::Right => (px - (mx + 8.0)).abs() <= 8.0 && (py - my).abs() <= 17.0,
                    PipeDir::Left => (px - (mx - 8.0)).abs() <= 8.0 && (py - my).abs() <= 17.0,
                };
                if at_mouth {
                    self.timer = self.timer.max(60);
                }
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 1;
                    self.timer = 8;
                }
            }
            // OutMove: 8-frame delay, then 1 px/frame
            1 => {
                if self.timer > 0 {
                    self.timer -= 1;
                } else if towards(self, out) {
                    self.phase = 2;
                    self.timer = 96;
                    self.pause = 48 + self.rand(48) as i32; // fire delay
                }
            }
            // DokanWait: 96 frames (a Fire Piranha waits until it has fired)
            2 => {
                self.timer -= 1;
                let mut fired = true;
                if fire {
                    fired = self.phase_fired();
                    if !fired {
                        shot = self.fire_cycle(px, py);
                    }
                }
                if self.timer <= 0 && fired {
                    self.phase = 3;
                }
            }
            // DokanIn
            _ => {
                if towards(self, hidden) {
                    self.phase = 0;
                    self.timer = 72;
                    self.hops = 0;
                }
            }
        }
        shot
    }

    fn phase_fired(&self) -> bool {
        self.hops >= 2
    }

    /// Fire Piranha spit: wait `pause` frames, then 20 frames of animation, spit, 20 frames.
    /// `hops` = 0 waiting, 1 spitting, 2 done (pipe plants fire once per cycle).
    fn fire_cycle(&mut self, px: f32, py: f32) -> Option<Actor> {
        match self.hops {
            0 => {
                if self.pause > 0 {
                    self.pause -= 1;
                } else {
                    self.hops = 1;
                    self.chain = 0;
                }
                None
            }
            1 => {
                self.chain += 1;
                if self.chain == 20 {
                    // angle ±22° / ±45° toward the player
                    let (mx, my) = match self.pipe {
                        Some((PipeDir::Down, ..)) => (self.x, self.y - self.height() + 8.0),
                        _ => (self.x, self.y + self.height() - 8.0),
                    };
                    let want = (py + 8.0 - my).atan2(px - mx);
                    let face = if px <= self.x {
                        std::f32::consts::PI
                    } else {
                        0.0
                    };
                    let best = [22.0f32, -22.0, 45.0, -45.0]
                        .iter()
                        .map(|d| {
                            let a = if face == 0.0 {
                                d.to_radians()
                            } else {
                                std::f32::consts::PI - d.to_radians()
                            };
                            let diff = (a - want).sin().atan2((a - want).cos()).abs();
                            (diff, a)
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, a)| a)
                        .unwrap_or(0.0);
                    let mut f = Actor::new(Kind::EnemyFire, mx, my - 4.0);
                    f.vx = best.cos();
                    f.vy = best.sin();
                    f.timer = 600;
                    return Some(f);
                }
                if self.chain >= 40 {
                    if self.pipe.is_some() {
                        self.hops = 2;
                    } else {
                        self.hops = 0;
                        self.pause = 48 + self.rand(48) as i32;
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn step_normal(&mut self, level: &Level, player_x: f32) -> Option<Actor> {
        if self.wings {
            match self.kind {
                Kind::Goomba | Kind::Koopa { .. } => {} // their own wing behaviour below
                Kind::Bowser { .. } | Kind::BoomBoom { .. } | Kind::BowserJr { .. } => {}
                Kind::Piranha { .. } | Kind::Muncher if self.pipe.is_none() => {
                    self.step_winged_plant(level);
                    return None;
                }
                k if k.is_enemy() || k.is_item() || matches!(k, Kind::BigCoin { .. }) => {
                    self.step_generic_wings(level);
                    return None;
                }
                _ => {}
            }
        }
        match self.kind {
            Kind::DryBones if self.phase == 1 => {
                // collapsed: reassembles after 224 frames
                self.timer -= 1;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.timer <= 0 {
                    self.phase = 0;
                    self.dir = if player_x <= self.x { -1 } else { 1 };
                }
            }
            Kind::BobOmb if self.phase > 0 => self.step_lit_bobomb(level),
            Kind::HammerBro => return self.step_hammer_bro(level),
            Kind::Hammer => {
                self.vy = (self.vy - 0.125).max(-6.0);
                self.x += self.vx;
                self.y += self.vy;
            }
            Kind::Boo => self.step_boo(),
            Kind::Bowser { .. } => return self.step_bowser(level),
            Kind::BoomBoom { .. } => return self.step_boom_boom(level),
            Kind::BowserJr { .. } => return self.step_bowser_jr(level),
            Kind::BowserFire | Kind::Shuriken => self.step_boss_shot(),
            Kind::ChainChomp { chained } => self.step_chomp(level, chained),
            Kind::LavaBubble => self.step_lava_bubble(),
            Kind::MontyMole => self.step_monty(level),
            Kind::AngrySun { moon } => self.step_sun(moon),
            Kind::Goomba
            | Kind::Koopa { .. }
            | Kind::Spiny
            | Kind::Buzzy
            | Kind::SpikeTop
            | Kind::BobOmb
            | Kind::DryBones => {
                if self.wings && matches!(self.kind, Kind::Goomba | Kind::Koopa { .. }) {
                    self.step_wings(level, player_x);
                    return None;
                }
                if self.on_ground {
                    self.vx = WALK_SPEED * self.dir as f32;
                } else {
                    self.vx = self.vx.clamp(-WALK_SPEED, WALK_SPEED);
                }
                // red Koopa turns at ledges: probe |half width| + 0.1 ahead for floor
                if self.kind.ledge_turner() && self.on_ground && self.turn_lock == 0 {
                    let px = self.x + self.dir as f32 * (self.half_w() + 0.1);
                    let below = level.cell_at(px, self.y - 1.0);
                    let slope = level.slope_at(px).is_some_and(|(s, _)| s >= self.y - 8.0);
                    if below == Cell::Empty && !slope {
                        self.turn();
                    }
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Mushroom | Kind::OneUp => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                if self.on_ground {
                    self.vx = ITEM_SPEED * self.dir as f32;
                }
                self.collide(level);
                if self.wall {
                    self.dir = -self.dir;
                    self.vx = ITEM_SPEED * self.dir as f32;
                }
            }
            Kind::FireFlower => {
                self.vx = 0.0;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
            }
            Kind::Star => {
                self.vx = ITEM_SPEED * self.dir as f32;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.on_ground {
                    self.vy = FIREBALL_BOUNCE; // bounces 4.0 on every landing
                    self.on_ground = false;
                }
                if self.wall {
                    self.dir = -self.dir;
                }
            }
            Kind::Fireball => {
                self.timer -= 1;
                self.vy = (self.vy + FIREBALL_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.on_ground {
                    self.vy = FIREBALL_BOUNCE;
                    self.on_ground = false;
                }
                if self.wall || self.timer <= 0 {
                    self.mode = Mode::Dead; // snuffed against a wall
                }
            }
            Kind::EnemyFire => {
                self.x += self.vx;
                self.y += self.vy;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Dead;
                }
            }
            Kind::Piranha { .. } => {
                // free-standing plants stand on their surface and may fall if unsupported
                if self.pipe.is_none() {
                    self.vx = 0.0;
                    self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                    self.collide(level);
                }
            }
            Kind::Thwomp { side } => self.step_thwomp(level, side),
            Kind::Lift { falling, .. } => {
                if falling && (self.ridden || self.phase == 1) {
                    self.phase = 1;
                    self.vy = (self.vy - 0.0625).max(-4.0);
                    self.y += self.vy;
                }
            }
            Kind::Spring => {
                // compress 1 px/frame for 4 frames, hold 5, then launch and release over 4
                // (10-spring.md); `chain` = 1 on the launch frame (the world launches the rider)
                self.chain = 0;
                match self.phase {
                    0 => {
                        if self.ridden {
                            self.phase = 1;
                        }
                    }
                    1 => {
                        self.pause += 1;
                        if self.pause >= 4 {
                            self.phase = 2;
                            self.timer = 5;
                        }
                    }
                    2 => {
                        self.timer -= 1;
                        if self.timer <= 0 {
                            self.phase = 3;
                            self.chain = 1;
                        }
                    }
                    _ => {
                        self.pause -= 1;
                        if self.pause <= 0 {
                            self.pause = 0;
                            self.phase = 0;
                        }
                    }
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.vx = 0.0;
                self.collide(level);
            }
            Kind::BulletBill => {
                self.vx = self.dir as f32;
                self.x += self.vx;
            }
            Kind::BanzaiBill { dir } => {
                let (dx, dy) = [(1.0, 0.0), (0.0, -1.0), (-1.0, 0.0), (0.0, 1.0)][dir as usize & 3];
                self.x += dx;
                self.y += dy;
            }
            Kind::Donut => {
                self.vy = (self.vy - 0.1875).max(-2.0);
                self.y += self.vy;
            }
            Kind::Seesaw { width } => self.step_seesaw(width),
            Kind::Lakitu => return self.step_lakitu(),
            Kind::SpinyEgg => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.on_ground {
                    self.kind = Kind::Spiny;
                    self.dir = if self.target.0 < self.x { -1 } else { 1 };
                }
            }
            Kind::Cloud => {
                if self.ridden {
                    // the rider steers (target_facing carries the input direction here)
                    self.vx = 1.5 * self.target_facing as f32;
                    self.vy = 0.0;
                    self.x += self.vx;
                    if self.timer < 0 {
                        self.timer = 900;
                    }
                } else {
                    // idle bob ±1.6 px around home
                    let below = self.y < self.home_y - 1.6;
                    let t = if self.y > self.home_y + 1.6 {
                        -0.1
                    } else if below || self.vy >= 0.0 {
                        0.1
                    } else {
                        -0.1
                    };
                    self.vy += (t - self.vy).clamp(-0.004, 0.004);
                    self.y += self.vy;
                }
                if self.timer > 0 {
                    self.timer -= 1;
                    if self.timer == 0 {
                        self.mode = Mode::Dead;
                    }
                }
            }
            Kind::Pow | Kind::Key => {}
            Kind::CheepCheep { red } => {
                // `in_water` is passed in `ridden` by the world
                if self.ridden {
                    let t = self.dir as f32;
                    self.vx += (t - self.vx).clamp(-0.02, 0.02);
                    if red && (self.x - self.home_x).abs() > 40.0 {
                        self.dir = if self.x > self.home_x { -1 } else { 1 };
                    }
                    let phase = self.age as f32 * 3.0;
                    let tv = 0.3 * phase.to_radians().sin();
                    self.vy += (tv - self.vy).clamp(-0.1, 0.1);
                    self.collide(level);
                    if self.wall {
                        self.dir = -self.dir;
                    }
                } else {
                    // out of water: flop, hopping 4.0 on every landing
                    self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                    self.collide(level);
                    if self.on_ground {
                        self.vy = 4.0;
                        self.vx = 0.5 * self.dir as f32;
                        self.on_ground = false;
                    }
                    if self.wall {
                        self.dir = -self.dir;
                    }
                }
            }
            Kind::Blooper => {
                let (px, py) = self.target;
                if self.phase == 0 {
                    // sink at 0.5 for at least 30 frames, until level with the player
                    self.speed += (0.5 - self.speed).clamp(-0.5, 0.5);
                    self.x += 0.0;
                    self.y -= self.speed;
                    self.timer += 1;
                    if self.timer >= 30 && self.y <= py + 16.0 {
                        self.phase = 1;
                        self.timer = 0;
                        self.heading = if px >= self.x { 45.0 } else { 135.0 };
                    }
                } else {
                    // pulse: speed 2·|sin φ|, φ += 6°/frame, 38 frames
                    let t = 2.0 * (self.timer as f32 * 6.0).to_radians().sin().abs();
                    self.speed += (t - self.speed).clamp(-1.0, 1.0);
                    let a = self.heading.to_radians();
                    self.x += self.speed * a.cos();
                    self.y += self.speed * a.sin();
                    self.timer += 1;
                    if self.timer >= 38 {
                        self.phase = 0;
                        self.timer = 0;
                        self.speed = 0.0;
                    }
                }
                self.dir = if px >= self.x { 1 } else { -1 };
            }
            Kind::Skewer { .. } => self.step_skewer(),
            Kind::Magic | Kind::Wrench => {
                if self.kind == Kind::Wrench && self.y < self.home_y {
                    self.y += 1.0; // rises first
                } else {
                    self.x += self.vx;
                    self.y += if self.kind == Kind::Magic {
                        self.vy
                    } else {
                        0.0
                    };
                }
                self.timer -= 1;
                if self.timer <= 0 {
                    self.mode = Mode::Dead;
                }
            }
            Kind::RockyWrench => return self.step_rocky(),
            Kind::Wiggler => {
                if self.timer > 0 {
                    self.timer -= 1; // anger pause
                    self.vx = 0.0;
                } else {
                    let speed = if self.phase == 1 { 1.0 } else { 0.5 };
                    if self.phase == 1 && self.on_ground && self.age.is_multiple_of(64) {
                        self.dir = if self.target.0 < self.x { -1 } else { 1 };
                    }
                    if self.on_ground {
                        self.vx = speed * self.dir as f32;
                    }
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
                self.trail.push((self.x, self.y));
                if self.trail.len() > 200 {
                    self.trail.remove(0);
                }
            }
            Kind::LavaLift { fast } => {
                if self.ridden || self.phase == 1 {
                    self.phase = 1;
                    self.x += if fast { 2.5 } else { 0.8 };
                }
            }
            Kind::Magikoopa => {}
            Kind::Claw => {
                // pendulum: angle = A·sin(2π t / T); A 60°, T 160 frames [inf: the rope helper
                // simulation is outside the decompiled range]
                self.heading = 60.0 * (self.age as f32 * std::f32::consts::TAU / 160.0).sin();
                if self.timer > 0 {
                    self.timer -= 1; // re-grab cooldown
                }
            }
            Kind::Twister => {
                if self.on_ground {
                    self.vx = 0.5 * self.dir as f32;
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall {
                    self.dir = -self.dir;
                }
                // column height in `heading`
                self.heading = (self.heading + 0.7).clamp(12.0, 128.0);
            }
            Kind::Cannonball => {
                self.x += self.vx;
                self.y += self.vy;
            }
            Kind::Bumper => {
                if self.timer > 0 {
                    self.timer -= 1;
                }
            }
            Kind::Grinder => {}
            Kind::PSwitch => {
                if self.phase > 0 {
                    // pressed: squash animation, then removed (PushReserve 2 + DelWait 2 + anim)
                    self.timer -= 1;
                    if self.timer <= 0 {
                        self.mode = Mode::Dead;
                    }
                } else {
                    self.vx = 0.0;
                    self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                    self.collide(level);
                }
            }
            Kind::Muncher => {}
            Kind::BigCoin { .. } | Kind::Shell { .. } => {}
        }
        None
    }

    /// Lit Bob-omb (15-bob-omb.md): fuse 240 frames; stopped (phase 1) or kicked (phase 2,
    /// sliding with friction 0.03125, bouncing). At 0 it explodes (`phase` = 9, world handles).
    fn step_lit_bobomb(&mut self, level: &Level) {
        self.timer -= 1;
        if self.phase == 2 {
            let f = 0.03125;
            self.vx = if self.vx > 0.0 {
                (self.vx - f).max(0.0)
            } else {
                (self.vx + f).min(0.0)
            };
        } else if self.on_ground {
            self.vx = 0.0;
        }
        self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
        let impact = self.vy;
        self.collide(level);
        if self.on_ground && impact < -0.1875 && self.hops < 2 {
            self.vy = 0.5 * impact.abs();
            self.on_ground = false;
            self.hops += 1;
        }
        if self.wall {
            self.vx = -self.vx;
            self.dir = -self.dir;
        }
        if self.timer <= 0 {
            self.phase = 9;
        }
    }

    /// Kick a lit Bob-omb: 2.5, or min(2.5 + 0.35·|v|, 4.0) moving that way; vy 2.0.
    pub fn kick_bobomb(&mut self, dir: i32, kicker_vx: f32) {
        let mut speed = 2.5;
        if kicker_vx != 0.0 && kicker_vx.signum() as i32 == dir {
            speed = (2.5 + 0.35 * kicker_vx.abs()).min(4.0);
        }
        self.phase = 2;
        self.dir = dir;
        self.vx = speed * dir as f32;
        self.vy = 2.0;
        self.on_ground = false;
        self.hops = 0;
        self.kick_lock = 16;
    }

    /// Hammer Bro (3-hammer-bro.md, SMB1 "Bros1st"): paces ±16 px around home at 0.3, jumps
    /// every 300–449 frames (5.5 up, [inf] alternating with a 2.5 hop), throws hammers every
    /// 75–104 frames (70 % chance of a double, 10 frames apart), always facing the player.
    fn step_hammer_bro(&mut self, level: &Level) -> Option<Actor> {
        let (px, _) = self.target;
        self.dir = if px < self.x { -1 } else { 1 };
        if self.on_ground {
            let d = self.home_x - self.x;
            let pace = if self.phase == 0 { 1.0 } else { -1.0 };
            if (pace > 0.0 && d < -16.0) || (pace < 0.0 && d > 16.0) {
                self.phase ^= 1;
            }
            let target = 0.3 * if self.phase == 0 { 1.0 } else { -1.0 };
            self.vx = if self.vx < target {
                (self.vx + 0.1).min(target)
            } else {
                (self.vx - 0.1).max(target)
            };
        }
        // jump timer in `pause`
        if self.pause <= 0 {
            self.pause = 300 + self.rand(150) as i32;
        }
        self.pause -= 1;
        if self.pause == 0 && self.on_ground {
            self.hops ^= 1;
            self.vy = if self.hops == 1 { 5.5 } else { 2.5 };
            self.on_ground = false;
            self.home_x = self.x;
        }
        self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
        self.collide(level);
        if self.wall {
            self.phase ^= 1;
        }
        // throw timer in `timer`; second hammer pending in `chain`
        if self.timer <= 0 {
            self.timer = 75 + self.rand(30) as i32;
            if self.rand(100) < 70 {
                self.chain = 10;
            }
            return Some(self.make_hammer());
        }
        self.timer -= 1;
        if self.chain > 0 {
            self.chain -= 1;
            if self.chain == 0 {
                return Some(self.make_hammer());
            }
        }
        None
    }

    fn make_hammer(&self) -> Actor {
        let d = self.dir as f32;
        let mut h = Actor::new(Kind::Hammer, self.x - d * 3.0, self.y + 18.0);
        h.vx = 1.5 * d;
        h.vy = 3.0;
        h.dir = self.dir;
        h
    }

    /// Boo (41-boo.md): stops while the player faces it (speed decays by max(0.12·|v|, 0.002));
    /// otherwise turns toward the player at ≤ 1°/frame (±45° from horizontal) and speeds up by
    /// 0.005 to 0.4, with a wobble growing 0.0015/frame to 0.2 (2°/frame phase).
    fn step_boo(&mut self) {
        let (px, py) = self.target;
        if self.age == 1 {
            // spawned facing the player
            self.heading = if self.dir < 0 { 180.0 } else { 0.0 };
        }
        let looking = (px < self.x) == (self.target_facing > 0);
        if looking {
            self.speed = (self.speed - (0.12 * self.speed).max(0.002)).max(0.0);
        } else {
            let want = (py + 8.0 - self.y).atan2(px - self.x).to_degrees();
            let want = if want.abs() > 90.0 {
                let w = if want < 0.0 { want + 360.0 } else { want };
                w.clamp(135.0, 225.0)
            } else {
                want.clamp(-45.0, 45.0)
            };
            let mut diff = want - self.heading;
            while diff > 180.0 {
                diff -= 360.0;
            }
            while diff < -180.0 {
                diff += 360.0;
            }
            self.heading += diff.clamp(-1.0, 1.0);
            self.speed = (self.speed + 0.005).min(0.4);
            self.dir = if px < self.x { -1 } else { 1 };
        }
        // wobble only while chasing (its amplitude grows from the start of the chase)
        if looking {
            self.timer = 0;
        } else {
            self.timer += 1;
        }
        let amp = (self.timer as f32 * 0.0015).min(0.2);
        let w = amp * (self.timer as f32 * 2.0).to_radians().sin();
        let (s, c) = self.heading.to_radians().sin_cos();
        self.vx = self.speed * c - w * s;
        self.vy = self.speed * s + w * c;
        self.x += self.vx;
        self.y += self.vy;
    }

    /// Lava Bubble (60-lava-bubble.md). Hidden 120 frames 16 px below the reference level L, then
    /// rises with vy = A·cos θ, θ = 90°·(risen / (H·16)) changing ≤ 10°/frame, where H = (home −
    /// L + 16)/16 tiles and A = clamp(min(H·4/7, 8) + 1, 1, H + 1); within 20 px of the top it
    /// falls under gravity −0.1875 (≥ −6), upside down, until it is below the hide depth again.
    /// Phases: 0 hidden, 1 rising, 2 falling. θ (degrees) in `heading`.
    fn step_lava_bubble(&mut self) {
        let s = if self.big { 2.0 } else { 1.0 };
        let hide = self.floor_ref - 16.0 * s;
        match self.phase {
            0 => {
                if self.age <= 1 {
                    self.timer = 120;
                }
                self.y = hide;
                self.vy = 0.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 1;
                    self.heading = 0.0;
                }
            }
            1 => {
                let h_tiles = ((self.home_y - self.floor_ref + 16.0) / 16.0).max(1.0);
                let a = ((h_tiles * 4.0 / 7.0).min(8.0) + 1.0)
                    .min(h_tiles + 1.0)
                    .max(1.0);
                let want = 90.0 * ((self.y - hide) / (h_tiles * 16.0)).clamp(0.0, 1.0);
                self.heading += (want - self.heading).clamp(-10.0, 10.0);
                self.vy = (a * self.heading.to_radians().cos()).max(0.25);
                self.y += self.vy;
                if self.home_y - self.y < 20.0 {
                    self.phase = 2;
                }
            }
            _ => {
                self.vy = (self.vy - 0.1875).max(-6.0);
                self.y += self.vy;
                if self.y < hide {
                    self.phase = 0;
                    self.timer = 120;
                }
            }
        }
    }

    /// Monty Mole (102-monty-mole.md). Buried (phase 0) until the player is within 96 × 64 px,
    /// then a 60-frame mound (1), a leap out at vy 5.0 through the terrain (2), and a chase (3):
    /// toward the player at up to 1.7 px/f, accelerating 0.018 (0.03 when turning), re-aiming at
    /// most every 64 frames; walls bounce it back at 0.675× (≥ 0.5). Not buried → leaps at once.
    fn step_monty(&mut self, level: &Level) {
        let (px, py) = self.target;
        match self.phase {
            0 => {
                let covered = level.cell_at(self.x, self.y + 8.0).is_solid();
                let near = (px - self.x).abs() <= 96.0 && (py - (self.y - 1.0)).abs() <= 64.0;
                if !covered {
                    self.phase = 2;
                    self.vy = 5.0;
                } else if near {
                    self.phase = 1;
                    self.timer = 60;
                }
            }
            1 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 2;
                    self.vy = 5.0;
                }
            }
            2 => {
                // through the terrain until clear of it, then normal physics
                self.vy = (self.vy - 0.1875).max(MAX_FALL);
                let inside = level.cell_at(self.x, self.y + 1.0).is_solid()
                    || level
                        .cell_at(self.x, self.y + self.height() - 1.0)
                        .is_solid();
                if inside && self.vy > 0.0 {
                    self.y += self.vy;
                } else {
                    self.collide(level);
                    if self.on_ground {
                        self.phase = 3;
                        self.dir = if px < self.x { -1 } else { 1 };
                        self.turn_lock = 64;
                    }
                }
            }
            _ => {
                if self.turn_lock == 0 {
                    let d = if px < self.x { -1 } else { 1 };
                    if d != self.dir {
                        self.dir = d;
                        self.turn_lock = 64;
                    }
                }
                if self.on_ground {
                    let target = 1.7 * self.dir as f32;
                    let acc = if self.vx * (self.dir as f32) < 0.0 {
                        0.03
                    } else {
                        0.018
                    };
                    self.vx += (target - self.vx).clamp(-acc, acc);
                    if self.wings && self.age.is_multiple_of(48) {
                        self.vy = 3.5625;
                    }
                }
                let vx = self.vx;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall {
                    let b = -0.675 * vx;
                    self.vx = if b.abs() < 0.5 { 0.5 * b.signum() } else { b };
                }
            }
        }
    }

    /// Angry Sun (104-angry-sun.md): hovers 48 px in from a top corner of the view and 64 px below
    /// its top edge, following the camera; waits 120 frames, spins one turn (circling 16 px around
    /// the anchor, 36 frames), then swoops along a half-ellipse 121 px deep to the other corner at
    /// 1.40°/frame. The Moon (alternate) only hangs in its corner. Phases: 0 wait, 1 spin, 2 swoop;
    /// side in `hops` (0 right, 1 left), angle in `heading`. The first wait is [inf] 120 frames.
    fn step_sun(&mut self, moon: bool) {
        use crate::world::{VIEW_H, VIEW_W};
        let (cx0, cy0) = self.camera;
        let side = if self.hops == 0 { 1.0 } else { -1.0 };
        let centre = cx0 + VIEW_W / 2.0;
        let r = VIEW_W / 2.0 - 48.0;
        let top = cy0 + VIEW_H - 64.0;
        let anchor = (centre + side * r, top);
        if self.age <= 1 {
            self.timer = 120;
        }
        if moon {
            self.x = anchor.0;
            self.y = anchor.1 - 16.0;
            return;
        }
        match self.phase {
            0 => {
                self.x = anchor.0;
                self.y = anchor.1 - 16.0;
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 1;
                    self.heading = 0.0;
                }
            }
            1 => {
                self.heading += 10.0;
                let a = self.heading.to_radians();
                self.x = anchor.0 + 16.0 * a.sin();
                self.y = anchor.1 - 16.0 + 8.0 * (1.0 - a.cos());
                if self.heading >= 360.0 {
                    self.phase = 2;
                    self.heading = 0.0;
                }
            }
            _ => {
                self.heading = (self.heading + 1.40).min(180.0);
                let a = self.heading.to_radians();
                self.x = centre + side * r * a.cos();
                self.y = top - 16.0 - 121.0 * a.sin().abs();
                self.dir = -side as i32;
                if self.heading >= 180.0 {
                    self.hops ^= 1;
                    self.phase = 0;
                    self.timer = 120;
                }
            }
        }
    }

    /// Chain Chomp (61-chain-chomp.md). Chained: hops around the stake (0.8 px/f, 1.5 hop on each
    /// landing, turns past 40·s px or at walls, pulled back past the 49.6·s chain), and after
    /// 120–179 frames crouches 61 frames, lunges at 8 px/f (+1/frame) to 51.2·s + 4 px from the
    /// stake, bites 61 frames and curves back. 3 of 7 lunges aim at the player (elevation 73.1°,
    /// 45° or 18.3° by height); the rest are random. Free: hops at 1.2 px/f with 3.0 hops.
    /// Phases: 0 walk, 1 crouch, 2 lunge, 3 bite, 4 return. Attack counter in `hops`.
    fn step_chomp(&mut self, level: &Level, chained: bool) {
        let s = if self.big { 2.0 } else { 1.0 };
        let (px, py) = self.target;
        let stake = (self.home_x, self.home_y + 8.0 * s);
        if !chained {
            if self.on_ground {
                self.vx += (1.2 * self.dir as f32 - self.vx).clamp(-0.05, 0.05);
                self.vy = 3.0;
                self.on_ground = false;
            }
            self.vy = (self.vy - 0.1875).max(-4.0);
            self.collide(level);
            if self.wall {
                self.turn();
                self.vx = 1.2 * self.dir as f32;
            }
            return;
        }
        match self.phase {
            0 => {
                if self.timer <= 0 && self.age <= 1 {
                    self.timer = 120 + self.rand(60) as i32;
                }
                let away = (self.x - stake.0) * self.dir as f32 > 0.0;
                if self.turn_lock == 0 && away && (self.x - stake.0).abs() > 40.0 * s {
                    self.turn();
                    self.turn_lock = 10;
                }
                self.vx = 0.8 * self.dir as f32;
                if self.on_ground {
                    self.vy = 1.5;
                    self.on_ground = false;
                }
                self.vy = (self.vy - 0.1875).max(-4.0);
                // chain: pulled back past its slack length
                let (cx, cy) = (self.x, self.y + 8.0 * s);
                let d = ((cx - stake.0).powi(2) + (cy - stake.1).powi(2)).sqrt();
                let l = 49.6 * s;
                if d > l {
                    let k = (d - l) * 0.3 / d;
                    self.vx = (self.vx - (cx - stake.0) * k).clamp(-4.0, 4.0);
                    self.vy = (self.vy - (cy - stake.1) * k).max(-4.0);
                }
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                    self.turn_lock = 10;
                }
                self.timer -= 1;
                if self.timer <= 0 && self.on_ground {
                    self.phase = 1;
                    self.timer = 61;
                    self.vx = 0.0;
                    self.vy = 0.0;
                    self.hops = (self.hops + 1) % 7;
                    let aimed = matches!(self.hops, 0 | 3 | 5);
                    let (dir, deg) = if aimed {
                        let dy = py - self.y;
                        let deg = if dy > 36.0 {
                            73.1
                        } else if dy > 16.0 {
                            45.0
                        } else {
                            18.3
                        };
                        (if px < self.x { -1 } else { 1 }, deg)
                    } else {
                        let r = self.rand(8);
                        let deg = match r {
                            0 => 73.1,
                            1..=3 => 45.0,
                            _ => 18.3,
                        };
                        (if self.rand(2) == 0 { -1 } else { 1 }, deg)
                    };
                    self.dir = dir;
                    let reach = 51.2 * s + 4.0;
                    let (sn, cs) = (deg as f32).to_radians().sin_cos();
                    // lunge target (centre) in `trail[0]`
                    self.trail = vec![(stake.0 + reach * cs * dir as f32, stake.1 + reach * sn)];
                }
            }
            1 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 2;
                    self.speed = 0.0;
                }
            }
            2 => {
                let (tx, ty) = self.trail.first().copied().unwrap_or(stake);
                let (cx, cy) = (self.x, self.y + 8.0 * s);
                let (dx, dy) = (tx - cx, ty - cy);
                let d = (dx * dx + dy * dy).sqrt();
                self.speed = (self.speed + 1.0).min(8.0);
                if d <= self.speed {
                    self.x = tx;
                    self.y = ty - 8.0 * s;
                    self.phase = 3;
                    self.timer = 61;
                } else {
                    self.x += dx / d * self.speed;
                    self.y += dy / d * self.speed;
                }
            }
            3 => {
                self.timer -= 1;
                if self.timer <= 0 {
                    self.phase = 4;
                    self.heading = 0.0; // curve parameter
                    self.trail = vec![(self.x, self.y)];
                }
            }
            _ => {
                // back to the ground beside the stake on a curve [inf: control points not read]
                let (sx, sy) = self.trail.first().copied().unwrap_or((self.x, self.y));
                let side = if sx < stake.0 { -1.0 } else { 1.0 };
                let (ex, ey) = (stake.0 + 32.0 * s * side, self.home_y);
                self.heading = (self.heading + 0.04).min(1.0);
                let t = self.heading;
                let (mx, my) = ((sx + ex) / 2.0, sy.max(ey) + 16.0);
                let u = 1.0 - t;
                self.x = u * u * sx + 2.0 * u * t * mx + t * t * ex;
                self.y = u * u * sy + 2.0 * u * t * my + t * t * ey;
                if t >= 1.0 {
                    self.phase = 0;
                    self.timer = 120 + self.rand(60) as i32;
                    self.vy = 0.0;
                    self.collide(level);
                }
            }
        }
    }

    /// Winged Piranha Plant / Muncher (2-piranha-plant.md): wait 72 frames, jump at 4.5
    /// (gravity −0.1875); once the speed drops to 0.8 hover 120 frames (vy = 0.8·sin φ, φ from
    /// 90° at +5°/frame), then fall back and wait again.
    fn step_winged_plant(&mut self, level: &Level) {
        match self.phase {
            0 => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.vx = 0.0;
                self.collide(level);
                if self.on_ground {
                    self.timer += 1;
                    if self.timer >= 72 {
                        self.phase = 1;
                        self.vy = 4.5;
                        self.on_ground = false;
                    }
                }
            }
            1 => {
                self.vy += ENEMY_GRAVITY;
                self.collide(level);
                if self.vy <= 0.8 {
                    self.phase = 2;
                    self.timer = 0;
                }
            }
            _ => {
                self.vy = 0.8 * (90.0 + self.timer as f32 * 5.0).to_radians().sin();
                self.y += self.vy;
                self.timer += 1;
                if self.timer >= 120 {
                    self.phase = 0;
                    self.timer = 0;
                    self.vy = 0.0;
                }
            }
        }
    }

    /// Generic winged flight [inf, from the winged Thwomp]: drift ±0.3 px/frame (turning at
    /// walls), bob vy = 0.15·sin(2°·t).
    fn step_generic_wings(&mut self, level: &Level) {
        let t = 0.3 * self.dir as f32;
        self.vx += (t - self.vx).clamp(-0.003, 0.003);
        self.vy = 0.15 * (self.age as f32 * 2.0).to_radians().sin();
        let before = self.x;
        self.collide(level);
        if self.wall || (self.x - before).abs() < 1e-4 && self.vx.abs() > 0.05 {
            self.dir = -self.dir;
        }
    }

    /// Paragoomba hops (small, small, big; 80-frame pause after the big hop); green Paratroopa
    /// hops 3.5625 on every landing; red Paratroopa bobs around its spawn height.
    fn step_wings(&mut self, level: &Level, player_x: f32) {
        match self.kind {
            Kind::Goomba => {
                self.vx = WALK_SPEED * self.dir as f32;
                if self.pause > 0 {
                    self.pause -= 1;
                }
                if self.on_ground && self.vy <= 0.0 {
                    let behind = (player_x - self.x).signum() as i32 != self.dir;
                    if self.timer > 64 && behind {
                        self.turn();
                        self.timer = 0;
                        self.hops = 0;
                        self.pause = 0;
                    } else if self.pause == 0 {
                        self.hops = (self.hops + 1) % 3;
                        if self.hops == 0 {
                            self.pause = 80;
                        }
                        self.vy = if self.hops == 0 { 3.0 } else { 1.25 };
                        self.on_ground = false;
                    }
                }
                self.timer += 1;
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Koopa { red: false } => {
                self.vx = 0.6875 * self.dir as f32;
                if self.on_ground && self.vy <= 0.0 {
                    self.vy = 3.5625;
                    self.on_ground = false;
                }
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                self.collide(level);
                if self.wall && self.turn_lock == 0 {
                    self.turn();
                }
            }
            Kind::Koopa { red: true } => {
                // vertical speed eases toward ±0.5 by 0.01/frame between home−32 and home+48
                let above = self.y + 8.0 > self.home_y + 48.0;
                let below = self.y + 8.0 < self.home_y - 32.0;
                let target = if above {
                    -0.5
                } else if below || self.vy >= 0.0 {
                    0.5
                } else {
                    -0.5
                };
                self.vy = if self.vy < target {
                    (self.vy + 0.01).min(target)
                } else {
                    (self.vy - 0.01).max(target)
                };
                self.vx = 0.0;
                self.y += self.vy;
                if self.timer <= 0 {
                    self.dir = if player_x <= self.x { -1 } else { 1 };
                    self.timer = 10;
                }
                self.timer -= 1;
            }
            _ => {}
        }
    }

    /// Axis-separated movement against the tile grid and slopes. Sets on_ground and wall.
    pub fn collide(&mut self, level: &Level) {
        let (hw, h) = (self.half_w(), self.height());
        let was_ground = self.on_ground;
        let solid = |px: f32, py: f32| level.cell_at(px, py).is_solid();

        // horizontal
        self.x += self.vx;
        if self.vx != 0.0 {
            let side = self.vx.signum();
            let wx = self.x + side * hw;
            let ys = [self.y + 2.0, self.y + (h - 2.0).max(2.0)];
            // one-way walls block only motion into them from their blocking side
            let prev_wx = wx - self.vx;
            let oneway = ys.iter().any(|&wy| {
                let c = level.cell_at(wx, wy);
                let cell_x = (wx / TILE).floor() * TILE;
                (side < 0.0 && c == Cell::PassRight && prev_wx >= cell_x + TILE - 0.01)
                    || (side > 0.0 && c == Cell::PassLeft && prev_wx <= cell_x + 0.01)
            });
            if (ys.iter().any(|&wy| solid(wx, wy)) || oneway) && level.slope_at(wx).is_none() {
                let edge = if side > 0.0 {
                    (wx / TILE).floor() * TILE
                } else {
                    ((wx / TILE).floor() + 1.0) * TILE
                };
                self.x = edge - side * (hw + 0.001);
                self.wall = true;
            }
        }
        if self.x - hw < level.left {
            self.x = level.left + hw;
            self.wall = self.vx < 0.0 || self.wall;
        }

        // vertical
        let prev_y = self.y;
        self.y += self.vy;
        let fx = (hw - 3.0).max(1.0);
        if self.vy > 0.0 {
            let hy = self.y + h;
            if [-fx, fx].iter().any(|&dx| solid(self.x + dx, hy)) {
                self.y = (hy / TILE).floor() * TILE - h - 0.001;
                self.vy = match self.kind {
                    Kind::Mushroom | Kind::OneUp => -1.0,
                    Kind::Star => -2.0,
                    _ => 0.0,
                };
            }
            self.on_ground = false;
        } else {
            let mut landed: Option<f32> = None;
            for dx in [-fx, fx] {
                let (px, py) = (self.x + dx, self.y);
                let top = ((py / TILE).floor() + 1.0) * TILE;
                let cell = level.cell_at(px, py);
                if cell.is_solid() || (cell == Cell::SemiTop && prev_y >= top - 0.01) {
                    landed = Some(landed.map_or(top, |t: f32| t.max(top)));
                }
            }
            for dx in [-fx, 0.0, fx] {
                if let Some((surf, _)) = level.slope_at(self.x + dx) {
                    let reach = if was_ground { 6.0 } else { 0.0 };
                    if self.y <= surf + reach
                        && prev_y >= surf - 6.0
                        && landed.is_none_or(|t| surf >= t)
                    {
                        landed = Some(surf);
                    }
                }
            }
            match landed {
                Some(top) => {
                    self.y = top;
                    self.vy = 0.0;
                    self.on_ground = true;
                }
                None => self.on_ground = false,
            }
        }
    }
}

/// Non-interacting effects ("simple objects").
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Coin popped from a block (B10): credited when its life ends.
    CoinPop {
        x: f32,
        y: f32,
        vy: f32,
        g: f32,
        cap: f32,
        life: i32,
    },
    /// Brick debris (B8): 4 per brick, 127 frames.
    Debris {
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        life: i32,
    },
    /// Score popup rising above its source.
    Score {
        x: f32,
        y: f32,
        points: u32,
        life: i32,
    },
}

impl Effect {
    pub fn coin_pop(x: f32, block_y: f32, down: bool) -> Self {
        if down {
            Effect::CoinPop {
                x,
                y: block_y - 16.0,
                vy: 0.0,
                g: -0.0625,
                cap: -2.0,
                life: 28,
            }
        } else {
            Effect::CoinPop {
                x,
                y: block_y + 16.0,
                vy: 3.0,
                g: -0.1875,
                cap: -8.0,
                life: 28,
            }
        }
    }

    /// Four debris pieces for a brick broken at block origin (bx, by). `h`, `r` are the shared
    /// random offsets (B8: h ∈ {0} ∪ [0, 0.5), r ∈ [0, 0.2)).
    pub fn debris(bx: f32, by: f32, h: f32, r: f32) -> [Effect; 4] {
        let starts = [(-4.0, 8.0), (4.0, 8.0), (-4.0, 4.0), (4.0, 4.0)];
        let vxs = [-1.0, 1.0, -1.0, 1.0];
        let vys = [4.5, 4.5, 3.0, 3.0];
        std::array::from_fn(|i| Effect::Debris {
            x: bx + starts[i].0,
            y: by + starts[i].1,
            vx: vxs[i] + if vxs[i] > 0.0 { h } else { -h },
            vy: vys[i] + r,
            life: 127,
        })
    }

    /// Advance; returns false when finished.
    pub fn step(&mut self) -> bool {
        match self {
            Effect::CoinPop {
                y,
                vy,
                g,
                cap,
                life,
                ..
            } => {
                *vy = (*vy + *g).max(*cap);
                *y += *vy;
                *life -= 1;
                *life > 0
            }
            Effect::Debris {
                x, y, vx, vy, life, ..
            } => {
                *vy = (*vy - 0.1875).max(-4.0);
                *y += *vy;
                *x += *vx;
                *life -= 1;
                *life > 0
            }
            Effect::Score { y, life, .. } => {
                if *life > 20 {
                    *y += 1.0;
                }
                *life -= 1;
                *life > 0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground_actor(kind: Kind) -> (Level, Actor) {
        let level = Level::flat(100);
        let mut a = Actor::new(kind, 20.0 * TILE, 32.0);
        a.on_ground = true;
        a.dir = 1;
        (level, a)
    }

    #[test]
    fn goomba_walks_half_pixel_per_frame() {
        let (level, mut a) = ground_actor(Kind::Goomba);
        let x0 = a.x;
        for _ in 0..60 {
            a.step(&level, 0.0);
        }
        assert!((a.x - x0 - 30.0).abs() < 1e-3, "moved {}", a.x - x0);
        assert!(a.on_ground);
    }

    #[test]
    fn goomba_turns_at_wall() {
        let mut level = Level::flat(100);
        level.set_solid(25, 2);
        let mut a = Actor::new(Kind::Goomba, 22.0 * TILE, 32.0);
        a.on_ground = true;
        for _ in 0..200 {
            a.step(&level, 0.0);
        }
        assert_eq!(a.dir, -1);
        assert!(a.x < 25.0 * TILE);
    }

    #[test]
    fn red_koopa_turns_at_ledge_green_walks_off() {
        let level = Level::flat(30); // ground ends at x = 480
        let mut red = Actor::new(Kind::Koopa { red: true }, 28.0 * TILE, 32.0);
        red.on_ground = true;
        let mut green = red.clone();
        green.kind = Kind::Koopa { red: false };
        for _ in 0..200 {
            red.step(&level, 0.0);
            green.step(&level, 0.0);
        }
        assert_eq!(red.dir, -1);
        assert!(red.on_ground);
        assert!(!green.on_ground || green.mode == Mode::Dead);
    }

    #[test]
    fn stomped_goomba_lasts_30_frames() {
        let (level, mut a) = ground_actor(Kind::Goomba);
        assert!(a.stomp());
        for i in 0..30 {
            assert_eq!(a.mode, Mode::Stomped, "frame {i}");
            a.step(&level, 0.0);
        }
        assert_eq!(a.mode, Mode::Dead);
    }

    #[test]
    fn koopa_shell_wakes_after_511() {
        let (level, mut a) = ground_actor(Kind::Koopa { red: false });
        a.stomp();
        assert_eq!(a.kind, Kind::Shell { red: false });
        for _ in 0..510 {
            a.step(&level, 0.0);
        }
        assert_eq!(a.mode, Mode::ShellWait);
        a.step(&level, 0.0);
        assert_eq!(a.kind, Kind::Koopa { red: false });
    }

    #[test]
    fn kicked_shell_keeps_speed_and_bounces() {
        let mut level = Level::flat(100);
        level.set_solid(30, 2);
        let mut a = Actor::new(Kind::Shell { red: false }, 20.0 * TILE, 32.0);
        a.on_ground = true;
        a.mode = Mode::ShellWait;
        a.kick(1, 1.5);
        assert!((a.vx - 3.25).abs() < 1e-6);
        let mut reversed = false;
        for _ in 0..120 {
            a.step(&level, 0.0);
            if a.vx < 0.0 {
                reversed = true;
            }
        }
        assert!(reversed);
        assert!((a.vx.abs() - 3.25).abs() < 1e-6);
    }

    #[test]
    fn mushroom_emerges_in_32_frames_then_walks() {
        let level = Level::flat(100);
        let mut a = Actor::new(Kind::Mushroom, 20.0 * TILE + 8.0, 4.0 * TILE);
        a.vy = 0.5;
        a.mode = Mode::Emerge { until: 5.0 * TILE };
        let mut n = 0;
        while matches!(a.mode, Mode::Emerge { .. }) {
            a.step(&level, 0.0);
            n += 1;
        }
        assert_eq!(n, 32);
        assert_eq!(a.vx, ITEM_SPEED);
    }

    #[test]
    fn coin_pop_peaks_near_38_5() {
        let mut e = Effect::coin_pop(0.0, 0.0, false);
        let mut top: f32 = 0.0;
        let mut n = 0;
        while e.step() {
            n += 1;
            if let Effect::CoinPop { y, .. } = e {
                top = top.max(y);
            }
        }
        assert_eq!(n + 1, 28);
        assert!((top - 38.5).abs() < 0.2, "peak {top}");
    }

    #[test]
    fn skewer_cycle() {
        let level = Level::flat(40);
        let mut s = Actor::new(Kind::Skewer { dir: 0 }, 200.0, 100.0);
        let mut max_ext: f32 = 0.0;
        let mut first_full = None;
        for f in 0..400 {
            s.step(&level, 0.0);
            max_ext = max_ext.max(s.heading);
            if s.heading >= 144.0 && first_full.is_none() {
                first_full = Some(f);
            }
        }
        assert_eq!(max_ext, 144.0);
        // 100 wait + wind-back (~25) + 40 hold + thrust (~30)
        let f = first_full.unwrap();
        assert!((180..=210).contains(&f), "extended at frame {f}");
        assert!(s.heading <= 144.0);
    }

    #[test]
    fn fireball_bounces_and_dies_at_wall() {
        let mut level = Level::flat(100);
        level.set_solid(26, 2);
        let mut f = Actor::new(Kind::Fireball, 20.0 * TILE, 40.0);
        f.vx = FIREBALL_SPEED;
        f.vy = -3.0;
        f.timer = FIREBALL_LIFE;
        let mut bounced = false;
        for _ in 0..60 {
            f.step(&level, 0.0);
            if f.vy > 3.0 {
                bounced = true;
            }
            if f.mode == Mode::Dead {
                break;
            }
        }
        assert!(bounced);
        assert_eq!(f.mode, Mode::Dead);
    }
}
