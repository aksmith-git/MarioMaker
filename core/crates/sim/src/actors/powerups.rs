//! The 2.0 / 3.0 update power-ups, actor side: the items (part 81 per style, the SMB1 Master
//! Sword, the 3D World wearables 127–131), the projectiles the forms make (Link's arrow and
//! bomb, the boomerang, the Cannon Box shot, the Red POW Box ring) and an enemy thrown by the
//! SMB2 Mushroom. From research/specs/objects/81-style-powerup-2.md and 127-headdress-items.md
//! (v3.0.3); [obs] values unless marked. The world side is world/powerups.rs.
//!
//! State layout: `phase` is the state, `timer` its timer; the rest in `Actor::v3` (`count` the
//! boomerang's pass, `cool` its catch lock, `clock` a life counter, `home` a target x, `speed` a
//! radius / launch speed, `owner` the player index, `flags` the charged / water bits).

use super::{Actor, ENEMY_GRAVITY, ITEM_SPEED, Kind, MAX_FALL, Mode};
use crate::fmath::DetMath;
use crate::level::{Level, TILE};

/// `Kind::PowerItem::what`: the style's part-81 item, the Master Sword, or a wearable
/// (`WEAR + kind`, kind 0 Propeller … 4 Bullet Bill, the player's +0x4bc numbering).
pub const STYLE2: u8 = 0;
pub const SWORD: u8 = 1;
pub const WEAR: u8 = 2;

/// The item a part is (part 81 in every style, 127–131 the wearables), or None.
pub fn item_for_part(id: i16) -> Option<u8> {
    Some(match id {
        81 => STYLE2,
        // 128 Propeller (kind 0), 127 Cannon (1), 129 Goomba (2), 131 Red POW (3), 130 Bullet
        // Bill (4) [obs item-kind table _INIT_1222]
        128 => WEAR,
        127 => WEAR + 1,
        129 => WEAR + 2,
        131 => WEAR + 3,
        130 => WEAR + 4,
        _ => return None,
    })
}

/// The boomerang's legs: out 160 px, then 80, 60, 40 and 20 past the owner [obs 0x71023c8410].
pub const BOOMERANG_LEGS: [f32; 5] = [160.0, 80.0, 60.0, 40.0, 20.0];
pub const BOOMERANG_SPEED: f32 = 5.0;
/// Stops 15 frames at each end; may be caught after 15 frames (or from the third pass) [obs].
pub const BOOMERANG_STOP: i32 = 15;
/// Arrow launch by aim (straight, up, down) [obs 0x7100dedee0].
pub const ARROW_LAUNCH: [(f32, f32); 3] = [(5.0, 0.9), (3.5, 4.8), (3.5, -2.0)];
/// Cannon Box shot: 3.0 px/f (charged 5.5), 30 frames of life [obs 0x7100df3450].
pub const CANNON_LIFE: i32 = 30;
/// The Red POW Box ring grows 5.333 px a frame to 80 px [obs 0x71013d65d0].
pub const RING_GROWTH: f32 = 16.0 / 3.0;
pub const RING_RADIUS: f32 = 80.0;
/// The SMB2 throw: 2.2 px/f, or |thrower speed| + 2.2 up to 4.0; vy −0.5 (less when faster);
/// gravity −0.1875 [obs cState_ThrownUSA 0x7100dd07c0].
pub const THROW_SPEED: f32 = 2.2;
pub const THROW_MAX: f32 = 4.0;
pub const THROW_GRAVITY: f32 = -0.1875;

/// What an enemy does about a power-up attack (its EnemyDB reaction column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    DieFall,
    DieStamp,
    DieVanish,
    /// Its own damage handler (into the shell, lit, stunned, a hit on a boss).
    Damage,
    /// Its own handler without dying (bosses lose HP).
    ReactOnly,
    /// Bounced back as off a helmet (the shield).
    CapBound,
    /// The attack goes through and is not spent.
    Pass,
    /// Bullet Bills burst (the boomerang).
    Explode,
    Invalid,
}

/// The attacks with their own EnemyDB columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    Sword,
    SwordUnder,
    Shield,
    Arrow,
    Boomerang,
}

impl Kind {
    /// The power-up actors (this module steps and draws them).
    pub fn is_pu(self) -> bool {
        matches!(
            self,
            Kind::PowerItem { .. }
                | Kind::PlayerArrow
                | Kind::PlayerBoomerang
                | Kind::LinkBomb
                | Kind::CannonShot
                | Kind::RedPowRing
        )
    }

    /// The EnemyDB reaction of this enemy to a power-up attack (the reaction_sword /
    /// sword_under / shield / arrow / boomerang columns) [obs EnemyDB v3]. `big` picks the big
    /// rows, `style` the style's own row where they differ (0 SMB1 … 4 3D World).
    pub fn pu_reaction(self, attack: Attack, big: bool, style: u8) -> Reaction {
        use Attack as A;
        use Reaction as R;
        let w3 = style == 4;
        // (sword, sword_under, shield, arrow, boomerang)
        let row: [R; 5] = match self {
            Kind::Goomba | Kind::Goombrat | Kind::ShoeGoomba { .. } => {
                let boom = if big && !w3 { R::DieStamp } else { R::DieFall };
                [R::DieFall, R::DieFall, R::CapBound, R::DieFall, boom]
            }
            // SMB1 / SMB3 / NSMBU Koopas go into their shells; SMW's and 3D World's fall
            Kind::Koopa { .. } if matches!(style, 2 | 4) => {
                [R::DieFall, R::DieFall, R::CapBound, R::DieFall, R::DieFall]
            }
            Kind::Koopa { .. } | Kind::Shell { .. } | Kind::Spiny | Kind::SpinyEgg => {
                [R::Damage, R::DieFall, R::CapBound, R::DieFall, R::Damage]
            }
            Kind::Buzzy => [R::Damage, R::DieFall, R::CapBound, R::Invalid, R::Damage],
            Kind::Piranha { .. } if big && w3 => {
                [R::DieFall, R::Damage, R::CapBound, R::Damage, R::DieFall]
            }
            Kind::Piranha { .. } => [R::DieFall, R::DieFall, R::CapBound, R::DieFall, R::DieFall],
            Kind::Muncher => [R::Invalid, R::Invalid, R::CapBound, R::Invalid, R::Invalid],
            Kind::HammerBro | Kind::Magikoopa | Kind::MontyMole | Kind::CheepCheep { .. } => {
                [R::DieFall, R::DieFall, R::CapBound, R::DieFall, R::DieFall]
            }
            Kind::BulletBill | Kind::BanzaiBill { .. } => {
                [R::DieFall, R::DieFall, R::DieFall, R::Invalid, R::Explode]
            }
            Kind::BobOmb => [R::Damage, R::DieFall, R::CapBound, R::Damage, R::Damage],
            Kind::SpikeTop { .. } => [R::DieFall, R::DieFall, R::CapBound, R::Invalid, R::DieFall],
            Kind::Lakitu => [R::DieFall, R::DieFall, R::DieFall, R::DieFall, R::DieFall],
            Kind::Boo | Kind::RingBoo { .. } => {
                [R::Invalid, R::Invalid, R::Invalid, R::Pass, R::Pass]
            }
            Kind::Thwomp { .. } => [R::Invalid, R::Invalid, R::CapBound, R::Invalid, R::Invalid],
            Kind::DryBones => [R::DieFall, R::Damage, R::CapBound, R::Invalid, R::DieFall],
            Kind::Blooper => [R::DieFall, R::DieStamp, R::CapBound, R::DieFall, R::DieFall],
            Kind::BabyBlooper { .. } => {
                [R::DieFall, R::DieStamp, R::DieFall, R::DieFall, R::DieFall]
            }
            Kind::LavaBubble => [R::Invalid, R::Invalid, R::CapBound, R::Invalid, R::Pass],
            Kind::ChainChomp { .. } => {
                [R::DieFall, R::DieFall, R::CapBound, R::Invalid, R::DieFall]
            }
            Kind::RockyWrench => [
                R::DieStamp,
                R::DieFall,
                R::CapBound,
                R::DieStamp,
                R::DieStamp,
            ],
            Kind::Wiggler => [
                R::DieFall,
                R::DieFall,
                R::CapBound,
                R::ReactOnly,
                R::DieFall,
            ],
            Kind::FishBone => [
                R::DieVanish,
                R::DieVanish,
                R::DieVanish,
                R::Invalid,
                R::DieVanish,
            ],
            Kind::Bowser { .. } | Kind::BowserJr { .. } | Kind::Koopaling { .. } => [
                R::ReactOnly,
                R::ReactOnly,
                R::CapBound,
                R::ReactOnly,
                R::ReactOnly,
            ],
            Kind::BoomBoom { .. } => [
                R::ReactOnly,
                R::ReactOnly,
                R::ReactOnly,
                R::ReactOnly,
                R::ReactOnly,
            ],
            Kind::AntTrooper { horned } if big => {
                let _ = horned;
                [
                    R::ReactOnly,
                    R::ReactOnly,
                    R::Invalid,
                    R::ReactOnly,
                    R::ReactOnly,
                ]
            }
            Kind::AntTrooper { .. } => {
                [R::DieFall, R::DieStamp, R::Invalid, R::DieFall, R::DieFall]
            }
            Kind::Stingby => [
                R::DieFall,
                R::DieVanish,
                R::CapBound,
                R::DieFall,
                R::DieFall,
            ],
            Kind::Skipsqueak { .. } => [
                R::DieFall,
                R::DieVanish,
                R::CapBound,
                R::DieFall,
                R::DieFall,
            ],
            Kind::HopChops => [R::DieFall, R::DieFall, R::CapBound, R::DieFall, R::DieFall],
            Kind::Porcupuffer => [R::DieFall, R::DieFall, R::DieFall, R::ReactOnly, R::DieFall],
            Kind::Bully => [
                R::ReactOnly,
                R::ReactOnly,
                R::CapBound,
                R::ReactOnly,
                R::ReactOnly,
            ],
            Kind::Spike => [R::DieFall, R::DieFall, R::CapBound, R::DieFall, R::DieFall],
            Kind::SpikeBall { snow: false } => [
                R::ReactOnly,
                R::ReactOnly,
                R::CapBound,
                R::Invalid,
                R::Damage,
            ],
            Kind::SpikeBall { snow: true } => [
                R::DieStamp,
                R::DieStamp,
                R::CapBound,
                R::DieStamp,
                R::Damage,
            ],
            Kind::Pokey { .. } => [R::DieFall, R::DieStamp, R::CapBound, R::DieFall, R::DieFall],
            Kind::Mechakoopa { .. } => [R::Damage, R::DieFall, R::CapBound, R::Invalid, R::Damage],
            Kind::LinkBomb => [
                R::ReactOnly,
                R::ReactOnly,
                R::CapBound,
                R::ReactOnly,
                R::ReactOnly,
            ],
            // not in the table (or no row): no interaction
            _ => [R::Invalid; 5],
        };
        row[match attack {
            A::Sword => 0,
            A::SwordUnder => 1,
            A::Shield => 2,
            A::Arrow => 3,
            A::Boomerang => 4,
        }]
    }

    /// EnemyDB `enable_carry_usa` and `linebg_height_usa`: what the SMB2 Mushroom can pick up
    /// and how high it rides (None: not carryable) [obs EnemyDB v3, SMB1 rows].
    pub fn usa_carry(self, big: bool) -> Option<f32> {
        let h = match self {
            Kind::Goomba | Kind::ShoeGoomba { .. } => 14.0,
            Kind::Koopa { .. } | Kind::Shell { .. } | Kind::Buzzy | Kind::Spiny => 14.0,
            Kind::HammerBro => 20.0,
            Kind::BulletBill => 14.0,
            Kind::BanzaiBill { .. } => return Some(60.0),
            Kind::BobOmb | Kind::DryBones | Kind::CheepCheep { .. } | Kind::MontyMole => 14.0,
            Kind::SpikeTop { .. } | Kind::RockyWrench | Kind::FishBone | Kind::Pow => 15.9,
            Kind::Lakitu => 26.0,
            Kind::Magikoopa => 20.0,
            Kind::Blooper => 18.0,
            Kind::Spike | Kind::Mechakoopa { .. } => 14.0,
            Kind::SpikeBall { snow: true } | Kind::Pokey { snow: true, .. } => 15.9,
            Kind::LinkBomb | Kind::SideSpring => 15.9,
            _ => return None,
        };
        Some(if big { h * 2.0 } else { h })
    }
}

/// Does a walking item (part 81 outside 3D World walks like the mushroom; the Boomerang Flower,
/// the Master Sword and the wearables stay put [obs vtable slot 194]).
pub fn walks(what: u8, style: u8) -> bool {
    what == STYLE2 && style != 4
}

impl Actor {
    pub(super) fn pu_size(&self) -> (f32, f32) {
        match self.kind {
            Kind::PowerItem { .. } => (8.0, 16.0),
            // the arrow's sensors reach ±4 along it [obs 0x7100dedee0]
            Kind::PlayerArrow => (4.0, 4.0),
            // hit body half 8 × 8 [obs _INIT_353]
            Kind::PlayerBoomerang => (8.0, 16.0),
            Kind::LinkBomb => (7.0, 14.0),
            Kind::CannonShot => (6.0, 8.0),
            Kind::RedPowRing => (0.0, 0.0),
            _ => (8.0, 16.0),
        }
    }

    /// One frame of a power-up actor.
    pub(super) fn step_pu(&mut self, level: &Level) {
        let style = super::v3::style_ix(level) as u8;
        match self.kind {
            Kind::PowerItem { what } => {
                self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
                if walks(what, style) {
                    if self.on_ground {
                        self.vx = ITEM_SPEED * self.dir as f32;
                    }
                    self.collide(level);
                    if self.wall {
                        self.dir = -self.dir;
                        self.vx = ITEM_SPEED * self.dir as f32;
                    }
                } else {
                    if self.on_ground {
                        self.vx = 0.0;
                    }
                    self.collide(level);
                }
            }
            Kind::PlayerArrow => self.step_arrow(level),
            Kind::PlayerBoomerang => self.step_boomerang(level),
            Kind::LinkBomb => self.step_link_bomb(level),
            Kind::CannonShot => {
                // a straight line for 30 frames; the world blows it up (walls, enemies, the
                // end of its life) [obs 0x7100df3b20]
                self.x += self.vx;
                self.timer -= 1;
                let ahead = self.x + self.dir as f32 * 6.0;
                if self.timer <= 0 || level.cell_at(ahead, self.y + 4.0).is_solid() {
                    self.phase = 1;
                }
            }
            Kind::RedPowRing => {
                self.speed = (self.speed + RING_GROWTH).min(RING_RADIUS);
                self.timer += 1;
                if self.speed >= RING_RADIUS {
                    self.phase = 1;
                }
            }
            _ => {}
        }
    }

    /// BulletPlayerArrow Move / FlipFall (0x7100dee620 / 0x7100deea50).
    fn step_arrow(&mut self, level: &Level) {
        let water = self.v3.flags & 1 != 0;
        if self.phase == 0 {
            self.vy = (self.vy - 0.12).max(-3.5);
            self.vx = crate::player::approach(self.vx, 0.0, 0.038);
            self.x += self.vx;
            self.y += self.vy;
            let tip = (
                self.x + self.vx.signum() * 4.0,
                self.y + 2.0 + self.vy.signum() * 2.0,
            );
            if level.cell_at(tip.0, tip.1).is_solid() {
                self.arrow_flip(water);
            }
        } else {
            let (g, cap) = if water { (-0.05, -1.5) } else { (-0.12, -3.5) };
            self.vy = (self.vy + g).max(cap);
            self.x += self.vx;
            self.y += self.vy;
            self.heading += if water { 8.0 } else { 24.0 };
        }
        let (cx, cy) = self.camera;
        if self.x < cx - 32.0 || self.x > cx + 24.0 * TILE + 32.0 || self.y < cy - 32.0 {
            self.mode = Mode::Dead;
        }
    }

    /// FlipFall begin: it bounces back off what it hit and tumbles away.
    pub fn arrow_flip(&mut self, water: bool) {
        self.phase = 1;
        self.vy = if water { 1.0 } else { 2.0 };
        self.vx *= if water { -0.2 } else { -0.3 };
        if self.vx != 0.0 {
            self.dir = self.vx.signum() as i32;
        }
        self.heading += 180.0;
    }

    /// The arrow's heading for drawing (degrees, 0 = right).
    pub fn arrow_angle(&self) -> f32 {
        if self.phase == 0 {
            self.vy.datan2(self.vx).to_degrees()
        } else {
            self.heading
        }
    }

    /// BulletPlayerBoomerang (0x7100df0320 …): out to a point, a 15-frame stop, back past the
    /// owner to the next leg's point, five legs in all. `target` is the owner's position.
    fn step_boomerang(&mut self, level: &Level) {
        let (ox, oy) = self.target;
        self.v3.clock -= 1;
        if self.v3.cool > 0 {
            self.v3.cool -= 1;
        }
        self.heading += 22.5;
        let cx = self.x;
        let cy = self.y + 8.0;
        let solid = |x: f32, y: f32| level.cell_at(x, y).is_solid();
        match self.phase {
            // Move
            0 => {
                if self.age <= 1 {
                    self.boomerang_target(ox);
                }
                self.vx = BOOMERANG_SPEED * self.dir as f32;
                self.vy = if self.v3.count >= 2 && cy > oy + 16.0 {
                    -1.0
                } else {
                    0.0
                };
                if self.vy != 0.0 && solid(cx, cy - 7.8) {
                    self.mode = Mode::Dead;
                    return;
                }
                if solid(cx + self.dir as f32 * 4.0 + self.vx, cy) {
                    // "Rebound": turn back now
                    self.v3.shots += 1;
                    self.boomerang_return(ox);
                    return;
                }
                let next = self.x + self.vx;
                if (next - self.v3.home) * self.dir as f32 >= 0.0 {
                    self.x = self.v3.home;
                    self.y += self.vy;
                    self.phase = 1;
                    self.timer = 0;
                    if self.v3.count > 4 {
                        self.mode = Mode::Dead;
                    }
                    return;
                }
                self.x = next;
                self.y += self.vy;
            }
            // Stop
            1 => {
                self.vx = 0.0;
                self.vy = 0.0;
                self.timer += 1;
                if self.timer >= BOOMERANG_STOP {
                    self.boomerang_return(ox);
                }
            }
            // Return
            _ => {
                self.vx = BOOMERANG_SPEED * self.dir as f32;
                self.vy = if cy > oy + 16.0 { -1.0 } else { 0.0 };
                let next = self.x + self.vx;
                if (next - ox) * self.dir as f32 >= 0.0 {
                    // past the owner: on to the far side's point (or back again at a wall)
                    self.x = next;
                    self.y += self.vy;
                    if solid(cx + self.dir as f32 * 4.0 + self.vx, cy) {
                        self.v3.shots += 1;
                        self.boomerang_return(ox);
                    } else {
                        self.phase = 0;
                        self.boomerang_target(ox);
                    }
                    return;
                }
                if solid(cx + self.dir as f32 * 4.0 + self.vx, cy)
                    || (self.vy != 0.0 && solid(cx, cy - 7.8))
                {
                    self.mode = Mode::Dead;
                    return;
                }
                self.x = next;
                self.y += self.vy;
            }
        }
        if self.v3.clock <= 0 {
            self.mode = Mode::Dead;
        }
    }

    fn boomerang_target(&mut self, ox: f32) {
        let k = (self.v3.count.max(1) as usize - 1).min(4);
        self.v3.home = ox + BOOMERANG_LEGS[k] * self.dir as f32;
    }

    /// Return.begin: the next pass back toward the owner (after five, it breaks).
    fn boomerang_return(&mut self, ox: f32) {
        if self.v3.count > 4 {
            self.mode = Mode::Dead;
            return;
        }
        let old = self.v3.count;
        self.v3.count += 1;
        self.dir = -self.dir;
        // already beyond the next point in the new direction (the owner ran past it): carry on
        // the old way instead [obs 0x7100df0840]
        let k = (old as usize).min(4);
        let far = ox + BOOMERANG_LEGS[k] * self.dir as f32;
        if (self.x - far) * self.dir as f32 >= 0.0 {
            self.dir = -self.dir;
            self.phase = 0;
            self.boomerang_target(ox);
            return;
        }
        self.phase = 2;
    }

    /// LinkBomb: falls and slides to a stop; the fuse counts down while it is lit (`timer`);
    /// phase 1 = lit, 2 = blowing up (the world handles the blast).
    fn step_link_bomb(&mut self, level: &Level) {
        if self.phase == 1 {
            self.timer -= 1;
            if self.timer <= 0 {
                self.phase = 2;
            }
        }
        self.vy = (self.vy + ENEMY_GRAVITY).max(MAX_FALL);
        if self.on_ground {
            // [inf] it slows like a kicked-then-dropped object on the ground
            self.vx = crate::player::approach(self.vx, 0.0, 0.0625);
        }
        self.collide(level);
        if self.wall {
            self.vx = -self.vx * 0.5;
        }
    }

    /// An enemy thrown by the SMB2 Mushroom (cState_ThrownUSA): ballistic at gravity −0.1875
    /// down to −4.0 [obs]; back to its own state on landing [inf: FallUSA]; a wall turns it
    /// back [inf: BoundHitUSA].
    pub(super) fn step_thrown(&mut self, level: &Level) {
        self.vy = (self.vy + THROW_GRAVITY).max(MAX_FALL);
        let vx = self.vx;
        self.collide(level);
        if self.on_ground {
            self.vx = 0.0;
            if matches!(self.kind, Kind::Shell { .. }) {
                self.enter_shell_wait();
            } else {
                self.mode = Mode::Normal;
            }
        } else if self.wall {
            self.vx = -vx * 0.5;
            self.dir = -self.dir;
        }
        if self.kick_lock > 0 {
            self.kick_lock -= 1;
        }
        if self.y < -4.0 * TILE {
            self.mode = Mode::Dead;
        }
    }

    /// Model and frame of a power-up actor (`M1_` names remap per style), width, height,
    /// mirrored.
    pub fn pu_look(&self) -> Option<(&'static str, &'static str, f32, f32, bool)> {
        let face = self.dir > 0;
        Some(match self.kind {
            Kind::PowerItem { what: SWORD } => {
                let f = ["wait.0", "wait.1", "wait.2", "wait.3"][(self.age as usize / 8) % 4];
                ("M1_Item_triforce", f, 16.0, 16.0, false)
            }
            // the part-81 item of each style (M1/M3/MW sprites, the NSMBU / 3D World models)
            Kind::PowerItem { what: STYLE2 } => ("M1_Item_specialEX", "wait.0", 16.0, 16.0, false),
            Kind::PowerItem { what } => (
                match what - WEAR {
                    0 => "M1_Item_BoxPropeller",
                    1 => "M1_Item_BoxKiller",
                    2 => "M1_Item_BoxKuribo",
                    3 => "M1_Item_BoxPow",
                    _ => "M1_Item_BoxKillerPlayer",
                },
                "wait.0",
                16.0,
                16.0,
                false,
            ),
            Kind::PlayerArrow => ("M1_Object_arrow", "arrow", 16.0, 8.0, !face),
            Kind::PlayerBoomerang => ("M1_Player_Boomerang", "Spin.0", 16.0, 16.0, false),
            Kind::LinkBomb => {
                let f = if self.phase == 1 && (self.timer / 4) % 2 == 0 {
                    "walk.0"
                } else {
                    "wait.0"
                };
                ("M1_Enemy_link_bomb", f, 16.0, 16.0, !face)
            }
            Kind::CannonShot => ("M1_Player_BoxKillerBullet", "wait.0", 16.0, 16.0, !face),
            Kind::RedPowRing => return None,
            _ => return None,
        })
    }
}
