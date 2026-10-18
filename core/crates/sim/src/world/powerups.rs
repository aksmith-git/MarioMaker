//! The 2.0 / 3.0 update power-ups, world side: collecting the items (part 81 by style, the
//! Master Sword, the wearables), the attacks the forms make (Link's sword, shield, down-thrust,
//! arrows and bombs; the boomerang; the Cannon Box; the Red POW Box; the Bullet Bill Mask's
//! break) and the SMB2 Mushroom's lifting and throwing. Player states: player/powerups.rs;
//! actors: actors/powerups.rs. Rules from 81-style-powerup-2.md and 127-headdress-items.md
//! (v3.0.3) [obs] unless marked.

use super::*;
use crate::actors::{PuAttack as Attack, PuReaction as Reaction, STYLE2, SWORD, WEAR};
use crate::player::{Blade, Headdress, Link};
use crate::player::{CUE_BALLOON_WALL, CUE_BOW, CUE_BOX_SPIN, CUE_CHARGE, CUE_DASH_WALL};
use crate::player::{CUE_FROG_SPLASH, CUE_KILLER_BREAK, CUE_SLASH, CUE_SPIN_ONCE, CUE_THRUST};

/// LinkBomb fuse [inf: the action's fuse length (vfunc +0x88) was not read; the Bob-omb's 240
/// frames is used].
pub const LINK_BOMB_FUSE: i32 = 240;

/// World state of the update power-ups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PuWorld {
    /// Enemies the current sword swing has already struck (one hit per swing).
    struck: Vec<u32>,
    /// (boomerang, enemy) pairs already hit by a flight.
    boom_hits: Vec<(u32, u32)>,
    prev_run: bool,
}

/// The Goomba Mask hides the player from the Goombas' and the generic player searches
/// (0x71011aad20, 0x7100d7f160 with the v3 flag; Boom Boom / Pom Pom wrappers) [obs]: those
/// enemies keep to their own way [inf: what they do instead].
pub(super) fn mask_target(a: &mut Actor, masked: bool) {
    if masked
        && matches!(
            a.kind,
            Kind::Goomba | Kind::Goombrat | Kind::BoomBoom { .. } | Kind::ClownCar { .. }
        )
    {
        a.target = (a.x + a.dir as f32 * 256.0, a.y);
    }
}

/// The player sprite's size for the update forms: their textures are 32 × 32 (the 3D forms
/// draw meshes, sized by Super Mario's art).
pub(super) fn sprite_size(form: Form) -> Option<(f32, f32)> {
    matches!(form, Form::Link | Form::Frog | Form::Balloon | Form::Smb2).then_some((32.0, 32.0))
}

impl World {
    fn style_ix(&self) -> u8 {
        match self.style {
            crate::player::Style::M1 => 0,
            crate::player::Style::M3 => 1,
            crate::player::Style::MW => 2,
            crate::player::Style::WU => 3,
            crate::player::Style::W3 => 4,
        }
    }

    /// The block swap to a Super Mushroom happens only for a small (form 0) player
    /// [obs 0x7100e4f720] (an SMM1 costume counts as small).
    pub(super) fn pu_small(&self) -> bool {
        matches!(self.player.form, Form::Small | Form::Costume)
    }

    /// The actor an update item becomes (the Master Sword is SMB1's; elsewhere the part is the
    /// Super Mushroom).
    pub(super) fn pu_item(&self, what: u8) -> Kind {
        if what == SWORD && self.style != crate::player::Style::M1 {
            return Kind::Mushroom;
        }
        Kind::PowerItem { what }
    }

    /// Out of a block: the part-81 items are a Super Mushroom for a small player (mask 0x7d7d2,
    /// kinds 14–18); the Master Sword and the wearables are not swapped [obs].
    pub(super) fn pu_block_item(&self, what: u8) -> Kind {
        if what == STYLE2 && self.pu_small() {
            Kind::Mushroom
        } else {
            self.pu_item(what)
        }
    }

    /// Collect an update item (0x710134ffd0): part 81 gives the style's form (0x71023d5d20 =
    /// SMB2 Mushroom, Frog, Balloon, Squirrel, Boomerang), the Triforce gives Link; a wearable
    /// is put on (3D World only, not while riding) [obs 0x71015c53e0].
    pub(super) fn collect_power_item(&mut self, what: u8, x: f32, y: f32) {
        if what >= WEAR {
            // not while riding or carried (0x71015a7960); carrying an item doesn't matter
            let free = self.player.yoshi.is_none()
                && self.player.clown.is_none()
                && self.player.car.is_none();
            if self.style == crate::player::Style::W3
                && free
                && let Some(h) = Headdress::from_kind(what - WEAR)
            {
                self.player.pu.wear(h);
                self.events.push(Event::Wearing);
            }
            return;
        }
        let form = if what == SWORD {
            Form::Link
        } else {
            match self.style {
                crate::player::Style::M1 => Form::Smb2,
                crate::player::Style::M3 => Form::Frog,
                crate::player::Style::MW => Form::Balloon,
                crate::player::Style::WU => Form::Squirrel,
                crate::player::Style::W3 => Form::Boomerang,
            }
        };
        if self.player.power_up(form) && form == Form::Balloon && self.player.pu.headdress.is_some()
        {
            // the change to the balloon removes a wearable (0x71015a8990) [obs]
            self.player.pu.take_off();
            self.events.push(Event::HeadDressOff);
        }
        self.events.push(Event::PowerUp);
        self.add_score(ITEM_SCORE, x, y + 16.0);
    }

    /// Before the player's step: what the power-up states sense.
    pub(super) fn pu_sense(&mut self) {
        let water = self.liquid.filter(|l| l.water()).map(|l| l.y);
        self.player.pu.water_top = water;
        self.player.pu.boomerang_out = self
            .actors
            .iter()
            .any(|a| a.kind == Kind::PlayerBoomerang && a.mode != Mode::Dead);
    }

    /// After the player's step, before the actors move and touch: the requests and attacks of
    /// the update power-ups.
    pub(super) fn step_powerups(&mut self) {
        if !self.player.alive() {
            return;
        }
        self.pu_cues();
        self.pu_spawns();
        self.pu_stand_on_enemies();
        self.pu_pick();
        self.pu_sword();
        self.pu_shield();
        self.pu_projectiles();
        self.pu_thrown();
        self.pu_red_pow();
    }

    fn pu_cues(&mut self) {
        let c = self.player.pu.cues;
        for (bit, e) in [
            (CUE_SLASH, Event::SwordSlash),
            (CUE_DASH_WALL, Event::SwordDashHit),
            (CUE_BOW, Event::ArrowShoot),
            (CUE_CHARGE, Event::SquatCharge),
            (CUE_BALLOON_WALL, Event::BalloonBounce),
            (CUE_BOX_SPIN, Event::PropellerBox),
            (CUE_SPIN_ONCE, Event::PropellerBox),
            (CUE_KILLER_BREAK, Event::KillerBreak),
            (CUE_THRUST, Event::SwordThrust),
            (CUE_FROG_SPLASH, Event::Bump),
        ] {
            if c & bit != 0 {
                self.events.push(e);
            }
        }
    }

    /// Hit an enemy with a power-up attack by its reaction column. Returns the reaction (the
    /// caller decides whether its projectile is spent).
    fn pu_hit(&mut self, j: usize, attack: Attack, from_x: f32) -> Reaction {
        let style = self.style_ix();
        let a = &mut self.actors[j];
        let r = a.kind.pu_reaction(attack, a.big, style);
        let (ax, ay) = (a.x, a.y);
        let mut down = false;
        match r {
            Reaction::Invalid | Reaction::Pass => return r,
            Reaction::CapBound => {
                // turned back as off a helmet [inf: the CapBound context's motion]
                let away = if a.x < from_x { -1 } else { 1 };
                if a.dir != away && a.kind.is_walker() {
                    a.turn();
                }
                a.dir = away;
                return r;
            }
            Reaction::DieVanish => {
                a.mode = Mode::Dead;
                down = true;
            }
            Reaction::DieStamp if matches!(a.kind, Kind::Goomba | Kind::Goombrat) => {
                a.stomp();
                down = true;
            }
            Reaction::DieFall | Reaction::DieStamp | Reaction::Explode => {
                if a.kind.is_v3() {
                    a.v3_tail_hit(from_x, ay);
                } else {
                    a.die_fall(from_x);
                }
                down = true;
            }
            Reaction::Damage => match a.kind {
                // into the shell / collapsed / lit [obs: the enemy's own damage handler]
                Kind::Koopa { .. } | Kind::Buzzy | Kind::DryBones | Kind::BobOmb => {
                    a.stomp();
                }
                Kind::Shell { .. } => {
                    let dir = if a.x < from_x { -1 } else { 1 };
                    a.kick(dir, 0.0);
                }
                k if k.is_v3() => a.v3_tail_hit(from_x, ay),
                // [inf] no shell of its own in the engine
                _ => {
                    a.die_fall(from_x);
                    down = true;
                }
            },
            Reaction::ReactOnly => {
                if a.kind.is_boss() {
                    // [inf] the sword hits as hard as a shell, arrows and boomerangs as a
                    // fireball
                    let hit = if matches!(attack, Attack::Sword | Attack::SwordUnder) {
                        crate::actors::BossAttack::Shell
                    } else {
                        crate::actors::BossAttack::Fire
                    };
                    down = a.boss_hit(hit, from_x) == crate::actors::BossHit::Defeated;
                } else if a.kind.is_v3() {
                    a.v3_tail_hit(from_x, ay);
                } else if a.kind == Kind::Wiggler {
                    a.stomp();
                } else if let Some(d) = super::w3::fire_hit(a, from_x) {
                    down = d;
                }
            }
        }
        if down {
            self.add_score(Self::chain_score(0), ax, ay + 24.0);
            self.events.push(Event::EnemyDown);
        }
        r
    }

    /// Requests from the player's states: arrows, the bomb, the boomerang, Cannon Box shots,
    /// the Bullet Bill Mask's break.
    fn pu_spawns(&mut self) {
        let p = self.player.clone();
        let f = p.facing as f32;
        let (px, py, pvx) = (p.x, p.y, p.vx);
        if let Some(aim) = p.pu.arrow {
            // PlayerArrowShoot (0x71015cd8e0): at the hand + (2·dir, +10) [obs]
            let (vx, vy) = crate::actors::ARROW_LAUNCH[(aim as usize).min(2)];
            let mut a = Actor::new(Kind::PlayerArrow, px + 2.0 * f, py + 8.0);
            a.vx = vx * f;
            a.vy = vy;
            a.dir = p.facing;
            a.v3.flags = p.in_water as u8;
            self.add_actor(a);
        }
        if p.pu.boomerang {
            // PlayerBoomerang at the model root + (8·dir, +16) [obs 0x71015c0e40]
            let mut a = Actor::new(Kind::PlayerBoomerang, px + 8.0 * f, py + 8.0);
            a.dir = p.facing;
            a.v3.count = 1;
            a.v3.cool = crate::actors::BOOMERANG_STOP_FRAMES;
            a.v3.clock = 300;
            a.target = (px, py);
            self.add_actor(a);
            self.events.push(Event::BoomerangThrow);
        }
        if p.pu.bomb && p.held.is_none() {
            // LinkBomb at (x, y − 16) in the game's y-down units: over Link's head [obs]
            let mut b = Actor::new(Kind::LinkBomb, px, py + 16.0);
            b.dir = p.facing;
            b.phase = 1;
            b.timer = LINK_BOMB_FUSE;
            b.held = true;
            let id = self.next_id;
            self.add_actor(b);
            self.player.held = Some(id);
            self.pu.prev_run = true;
        }
        if let Some(charged) = p.pu.cannon {
            // BulletPlayerHoudai (0x7100df3450): 3.0 (charged 5.5) plus a forward-moving
            // player's speed, capped 5.5 (8.0); from x + 20·dir, 14 up (small) / 17 (else),
            // 9 crouched [obs]
            let (base, cap) = if charged { (5.5, 8.0) } else { (3.0, 5.5) };
            let carry = if pvx * f > 0.0 { pvx } else { 0.0 };
            let vx = (base * f + carry).clamp(-cap, cap);
            let up = if p.crouch {
                9.0
            } else if p.big() {
                17.0
            } else {
                14.0
            };
            let mut s = Actor::new(Kind::CannonShot, px + 20.0 * f, py + up - 4.0);
            s.vx = vx;
            s.dir = p.facing;
            s.timer = crate::actors::CANNON_SHOT_LIFE;
            s.v3.flags = charged as u8;
            self.add_actor(s);
            self.events.push(Event::CannonShot);
        }
        if self.player.pu.killer_break {
            // KillerBreak with a wall: the blocks from x to x + 16·dir, ±16 around 0.75 of the
            // body's height [obs 0x71015c56a0]
            let p = &self.player;
            let h = if p.big() { 32.0 } else { 16.0 };
            let cy = p.y + 0.75 * h;
            let x1 = p.x + 16.0 * f + f;
            let tx = (x1 / TILE).floor() as i32;
            let rows = [
                ((cy - 15.0) / TILE).floor() as i32,
                (cy / TILE).floor() as i32,
                ((cy + 15.0) / TILE).floor() as i32,
            ];
            for ty in rows {
                if self.blocks.get(tx, ty).is_some() {
                    self.hit_block(tx, ty, true, HitPower::Break);
                }
            }
        }
        // the down-thrust lands: a ground quake under Link [inf: as a small ground pound]
        if self.player.pu.thrust_land {
            let p = &self.player;
            let ty = ((p.y - 1.0) / TILE).floor() as i32;
            let mut cols = vec![
                ((p.x - 4.0) / TILE).floor() as i32,
                ((p.x + 4.0) / TILE).floor() as i32,
            ];
            cols.dedup();
            for tx in cols {
                if self.blocks.get(tx, ty).is_some() {
                    self.hit_block(tx, ty, false, HitPower::Bump);
                }
            }
            self.events.push(Event::Bump);
        }
    }

    /// The SMB2 Mushroom stands on what it can carry [ext: SMB2's rule; the binary's "touching
    /// from above" flag +0x496 bit 6 was not traced]: falling onto a carryable enemy lands on
    /// its top instead of stomping it.
    fn pu_stand_on_enemies(&mut self) {
        let p = &self.player;
        if p.form != Form::Smb2 || p.vy > 0.0 || !p.alive() || p.yoshi.is_some() {
            return;
        }
        let held = p.held;
        let (x, y, vy) = (p.x, p.y, p.vy);
        let style = self.style_ix();
        let _ = style;
        let top = self
            .actors
            .iter()
            .filter(|a| {
                a.alive()
                    && a.solid_contact()
                    && !a.held
                    && Some(a.id) != held
                    && a.kind.usa_carry(a.big).is_some()
                    && (a.x - x).abs() < a.half_w() + 4.0
            })
            .map(|a| (a.y + a.height(), a.moved.0))
            .filter(|&(t, _)| y <= t + 0.01 && y >= t - 4.0 + vy.min(0.0))
            .fold(None, |best: Option<(f32, f32)>, c| match best {
                Some(b) if b.0 >= c.0 => Some(b),
                _ => Some(c),
            });
        if let Some((t, dx)) = top {
            let p = &mut self.player;
            p.y = t + 0.01;
            p.vy = 0.0;
            p.x += dx;
            if !p.on_ground {
                p.on_ground = true;
                p.pose = Pose::Wait;
            }
        }
    }

    /// SMB2 Mushroom: Down + action standing on a carryable object picks it up (LiftUp).
    fn pu_pick(&mut self) {
        if !self.player.pu.pick || self.player.held.is_some() {
            return;
        }
        let p = &self.player;
        let (x, y) = (p.x, p.y);
        let found = self.actors.iter().position(|a| {
            a.alive()
                && a.solid_contact()
                && !a.held
                && a.kind.usa_carry(a.big).is_some()
                && (a.x - x).abs() < a.half_w() + 4.0
                && (y - (a.y + a.height())).abs() < 2.0
        });
        let Some(j) = found else {
            // LiftUpSnowBall: pulled out of snow ground (theme 6), at most 4 out [obs
            // 0x7101598090; inf: any ground tile counts]
            let snowballs = self
                .actors
                .iter()
                .filter(|a| a.kind == (Kind::SpikeBall { snow: true }))
                .count();
            let on_ground_tile = self.level.cell_at(x, y - 1.0) == Cell::Solid
                && self
                    .blocks
                    .get((x / TILE).floor() as i32, ((y - 1.0) / TILE).floor() as i32)
                    .is_none();
            if self.level.theme == 6 && snowballs <= 4 && self.player.on_ground && on_ground_tile {
                let mut b = Actor::new(Kind::SpikeBall { snow: true }, x, y);
                b.held = true;
                let id = self.next_id;
                self.add_actor(b);
                self.player.held = Some(id);
                let pu = &mut self.player.pu;
                pu.lift = 1;
                pu.lift_delay = crate::player::LIFT_DELAY_BIG;
                pu.lift_from = -12.0;
                pu.carry_h = -12.0;
                self.events.push(Event::Pull);
            }
            return;
        };
        let a = &mut self.actors[j];
        a.held = true;
        a.vx = 0.0;
        a.vy = 0.0;
        let id = a.id;
        let big = a.big;
        let from = if matches!(a.kind, Kind::BanzaiBill { .. }) {
            -60.0
        } else if big {
            -28.0
        } else {
            -12.0
        };
        self.player.held = Some(id);
        let pu = &mut self.player.pu;
        pu.lift = 1;
        pu.lift_delay = if big {
            crate::player::LIFT_DELAY_BIG
        } else {
            crate::player::LIFT_DELAY
        };
        pu.lift_from = from;
        pu.carry_h = from;
        self.events.push(Event::Pull);
    }

    /// Carrying (World::carry_held hook): the SMB2 Mushroom's lifted object and Link's bomb.
    /// True when handled.
    pub(super) fn pu_carry(&mut self, i: usize) -> bool {
        let p = self.player.clone();
        let link_bomb = self.actors[i].kind == Kind::LinkBomb;
        if p.form != Form::Smb2 && !link_bomb {
            return false;
        }
        let f = p.facing as f32;
        let run = self.run_held;
        let run_pressed = run && !self.pu.prev_run;
        self.pu.prev_run = run;
        if link_bomb && p.form != Form::Smb2 {
            let (x, y) = (p.x, p.y + if p.big() { 28.0 } else { 16.0 });
            if run_pressed || !p.alive() {
                // thrown forward (0x7101597e90): at least 3.0, from |speed| + 1 [obs, partly
                // read; inf: vy 2.0]
                let speed = (p.vx.abs() + 1.0).max(3.0);
                let a = &mut self.actors[i];
                a.held = false;
                a.x = x;
                a.y = y;
                a.vx = speed * f;
                a.vy = 2.0;
                a.dir = p.facing;
                self.player.held = None;
                self.events.push(Event::Throw);
                return true;
            }
            let a = &mut self.actors[i];
            a.x = x;
            a.y = y;
            a.vx = 0.0;
            a.vy = 0.0;
            return true;
        }
        let pu = p.pu.clone();
        if pu.lift > 0 && pu.lift < pu.lift_delay {
            // not up yet: it waits where it is
            return true;
        }
        let (x, y) = (p.x, p.y + pu.carry_h);
        let lifting = pu.lift > 0;
        if !run && !lifting {
            let crouch = p.crouch;
            let pvx = p.vx;
            let a = &mut self.actors[i];
            a.held = false;
            self.player.held = None;
            if crouch || !self.player.alive() {
                // set down in front [inf: 0x7101597b20]
                a.x = x + f * 12.0;
                a.y = self.player.y;
                a.vx = 0.0;
                a.vy = 0.0;
                return true;
            }
            // the SMB2 throw (cState_ThrownUSA): 2.2 px/f, more when running forward
            let mut speed = crate::actors::THROW_SPEED_USA;
            if pvx * f > 0.0 {
                speed = (pvx.abs() + speed).min(crate::actors::THROW_MAX_USA);
            }
            let k = crate::actors::THROW_SPEED_USA / speed;
            a.x = x;
            a.y = y;
            a.vx = speed * f;
            a.vy = -0.5 * k * k;
            a.dir = p.facing;
            a.mode = Mode::Thrown;
            a.kick_lock = 50;
            self.events.push(Event::Throw);
            return true;
        }
        let a = &mut self.actors[i];
        a.x = x;
        a.y = y;
        a.vx = 0.0;
        a.vy = 0.0;
        if a.kind == (Kind::Shell { red: false }) || matches!(a.kind, Kind::Shell { .. }) {
            a.timer = crate::actors::SHELL_WAKE;
        }
        true
    }

    /// Link's sword (slash, dash, down-thrust): enemies once per swing by their reaction,
    /// blocks at the tip.
    fn pu_sword(&mut self) {
        let p = &self.player;
        if p.form != Form::Link {
            self.pu.struck.clear();
            return;
        }
        let Some((b, blade)) = p.pu.sword_box(p.x, p.y, p.facing) else {
            self.pu.struck.clear();
            return;
        };
        let (px, py, f) = (p.x, p.y, p.facing as f32);
        let probe = p.pu.sword_probe();
        let attack = if blade == Blade::Under {
            Attack::SwordUnder
        } else {
            Attack::Sword
        };
        let mut bounce = false;
        for j in 0..self.actors.len() {
            let a = &self.actors[j];
            if !(a.alive() && a.solid_contact() && !a.held)
                || self.pu.struck.contains(&a.id)
                || !overlap(b, a.bbox())
            {
                continue;
            }
            let id = a.id;
            let r = self.pu_hit(j, attack, px);
            if r != Reaction::Invalid {
                self.pu.struck.push(id);
            }
            if blade == Blade::Under && !matches!(r, Reaction::Invalid | Reaction::Pass) {
                bounce = true;
            }
        }
        if bounce {
            // the enemy's sword_under reaction bounces Link (the generic jump request) [inf:
            // the stomp bounce's speed]
            self.player.pu.link = Link::None;
            self.player.bounce(true);
            self.events.push(Event::Stomp);
        }
        if probe {
            // the tip probes the block grid at (x + 20·dir, y + 8): the shared side hit [obs];
            // bricks break, ? blocks give their item [inf: as the tail]
            let tx = ((px + crate::player::SWORD_REACH * f) / TILE).floor() as i32;
            let ty = ((py + 8.0) / TILE).floor() as i32;
            if self.blocks.get(tx, ty).is_some_and(|b| b.bump.is_none()) {
                self.hit_block(tx, ty, true, HitPower::Break);
            }
        }
    }

    /// The shield (ShieldGuard): projectiles touching it are blocked ("shieldHit", Link
    /// pushed back 1 px/f); enemies take their reaction_shield [obs].
    fn pu_shield(&mut self) {
        let p = &self.player;
        let Some(b) = p.pu.shield_box(p.x, p.y, p.facing) else {
            return;
        };
        let (px, f) = (p.x, p.facing as f32);
        let mut blocked = false;
        for j in 0..self.actors.len() {
            let a = &self.actors[j];
            if !(a.alive() && a.solid_contact()) || !overlap(b, a.bbox()) {
                continue;
            }
            // only what comes at the shield from the front
            let from_front = (a.x - px) * f > 0.0;
            if !from_front {
                continue;
            }
            if matches!(
                a.kind,
                Kind::EnemyFire
                    | Kind::Hammer
                    | Kind::Magic
                    | Kind::Wrench
                    | Kind::BowserFire
                    | Kind::Shuriken
                    | Kind::Cannonball
                    | Kind::KoopalingShot { .. }
                    | Kind::MechaMissile
            ) {
                // the projectile's blocked state ("r_hit") [obs; inf: it is destroyed]
                self.actors[j].mode = Mode::Dead;
                blocked = true;
                continue;
            }
            let r = self.pu_hit(j, Attack::Shield, px);
            if r == Reaction::CapBound {
                blocked = true;
            }
        }
        if blocked {
            self.player.vx = -f;
            self.events.push(Event::ShieldHit);
        }
    }

    /// Arrows, boomerangs, bombs and Cannon Box shots.
    fn pu_projectiles(&mut self) {
        let style = self.style_ix();
        let contact = self.player.contact_box();
        let owner_alive = self.player.alive();
        let mut blasts: Vec<(f32, f32, bool)> = Vec::new();
        let n = self.actors.len();
        for i in 0..n {
            let k = self.actors[i].kind;
            if !matches!(
                k,
                Kind::PlayerArrow | Kind::PlayerBoomerang | Kind::LinkBomb | Kind::CannonShot
            ) || self.actors[i].mode == Mode::Dead
            {
                continue;
            }
            let sb = self.actors[i].bbox();
            let sx = self.actors[i].x;
            match k {
                Kind::PlayerArrow if self.actors[i].phase == 0 => {
                    for j in 0..n {
                        let e = &self.actors[j];
                        if j == i
                            || !(e.alive() && e.solid_contact() && e.kind.is_enemy())
                            || !overlap(sb, e.bbox())
                        {
                            continue;
                        }
                        match self.pu_hit(j, Attack::Arrow, sx) {
                            Reaction::Pass => {}
                            // a hard enemy: the arrow glances off ("r_hit", FlipFall) [inf]
                            Reaction::Invalid | Reaction::CapBound => {
                                let water = self.actors[i].v3.flags & 1 != 0;
                                self.actors[i].arrow_flip(water);
                                self.events.push(Event::ArrowHit);
                                break;
                            }
                            // used up ("r_hit_enemy") [obs]
                            _ => {
                                self.actors[i].mode = Mode::Dead;
                                self.events.push(Event::ArrowHit);
                                break;
                            }
                        }
                    }
                }
                Kind::PlayerBoomerang => {
                    let id = self.actors[i].id;
                    if !owner_alive {
                        self.actors[i].mode = Mode::Dead;
                        continue;
                    }
                    // caught by its thrower after the 15-frame lock, or from the third pass [obs]
                    let b = &self.actors[i];
                    if overlap(sb, contact) && (b.v3.cool == 0 || b.v3.count > 2) {
                        self.actors[i].mode = Mode::Dead;
                        continue;
                    }
                    if self.actors[i].v3.shots > 0 {
                        self.actors[i].v3.shots = 0;
                        self.events.push(Event::Rebound);
                    }
                    for j in 0..n {
                        let e = &self.actors[j];
                        if j == i || !(e.alive() && e.solid_contact()) || !overlap(sb, e.bbox()) {
                            continue;
                        }
                        let eid = e.id;
                        if e.kind.is_item() {
                            // items hop (vy 3.0) and are not carried back; 16-frame lockout [obs
                            // 0x71013520a0]
                            if e.kick_lock == 0 && e.mode == Mode::Normal {
                                let e = &mut self.actors[j];
                                e.vy = 3.0;
                                e.on_ground = false;
                                e.kick_lock = 16;
                                self.events.push(Event::BoomerangReaction);
                            }
                            continue;
                        }
                        if !e.kind.is_enemy() && e.kind != Kind::Pow && e.kind != Kind::PSwitch {
                            continue;
                        }
                        if self.pu.boom_hits.contains(&(id, eid)) {
                            continue;
                        }
                        self.pu.boom_hits.push((id, eid));
                        let bound = boomerang_bounds(e.kind, e.big, style);
                        if e.kind.is_enemy() {
                            self.pu_hit(j, Attack::Boomerang, sx);
                        }
                        if bound {
                            // bound_boomerang: knocked back [inf: it turns for home]
                            let b = &mut self.actors[i];
                            if b.phase != 2 {
                                b.phase = 1;
                                b.timer = crate::actors::BOOMERANG_STOP_FRAMES;
                            }
                        }
                    }
                }
                Kind::LinkBomb if self.actors[i].phase == 2 => {
                    let a = &mut self.actors[i];
                    a.mode = Mode::Dead;
                    blasts.push((a.x, a.y + 8.0, true));
                }
                Kind::CannonShot => {
                    let mut hit = self.actors[i].phase == 1;
                    if !hit {
                        hit = self.actors.iter().enumerate().any(|(j, e)| {
                            j != i
                                && e.alive()
                                && e.solid_contact()
                                && e.kind.is_enemy()
                                && !houdai_immune(e.kind)
                                && overlap(sb, e.bbox())
                        });
                    }
                    if hit {
                        let a = &mut self.actors[i];
                        a.mode = Mode::Dead;
                        blasts.push((a.x, a.y + 4.0, false));
                    }
                }
                _ => {}
            }
        }
        if !self
            .actors
            .iter()
            .any(|a| a.kind == Kind::PlayerBoomerang && a.mode != Mode::Dead)
        {
            self.pu.boom_hits.clear();
        }
        for (x, y, bomb) in blasts {
            if bomb {
                self.pu_link_bomb_blast(x, y);
            } else {
                self.pu_cannon_blast(x, y);
            }
        }
    }

    /// The LinkBomb's blast (FUN_71011bcd90 → the shared blast 0x7101081e20): the Bob-omb's
    /// blast and terrain pattern, "p_LinkBombExp" [obs].
    fn pu_link_bomb_blast(&mut self, bx: f32, by: f32) {
        for a in &mut self.actors {
            let (cx, cy) = (a.x, a.y + a.height() / 2.0);
            if a.kind.is_enemy() && a.alive() && (cx - bx).abs() < 32.0 && (cy - by).abs() < 32.0 {
                if a.kind.is_boss() {
                    a.boss_hit(crate::actors::BossAttack::Blast, bx);
                } else {
                    a.die_fall(bx);
                }
            }
        }
        let (tx, ty) = ((bx / TILE).floor() as i32, (by / TILE).floor() as i32);
        for (dx, dy) in [
            (0, 0),
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
            (2, 0),
            (-2, 0),
            (0, 2),
            (0, -2),
        ] {
            let (x, y) = (tx + dx, ty + dy);
            if self
                .blocks
                .get(x, y)
                .is_some_and(|b| matches!(b.kind, BlockKind::Brick | BlockKind::Hard))
            {
                self.blocks.map.remove(&(x, y));
                self.level.set(x, y, Cell::Empty);
                let (ox, oy) = blocks::origin(x, y);
                self.effects.extend(Effect::debris(ox, oy, 0.25, 0.1));
            }
        }
        // [inf] the blast hurts the player as the Bob-omb's does
        let p = &self.player;
        if (p.x - bx).abs() < 32.0 && (p.y + 8.0 - by).abs() < 32.0 && self.player.hurt() {
            self.events.push(if self.player.alive() {
                Event::PowerDown
            } else {
                Event::Death
            });
        }
        self.events.push(Event::LinkBombExplode);
    }

    /// The Cannon Box shot's explosion (SnuffOut 0x7100df3d00): a 32 × 32 square on the shot
    /// hits the blocks and the actors in it (not Boos or Lava Bubbles,
    /// `invalid_explode_player_houdai`) [obs; inf: blocks break as from a shell, enemies fall].
    fn pu_cannon_blast(&mut self, x: f32, y: f32) {
        let area = (x - 16.0, y - 16.0, x + 16.0, y + 16.0);
        let mut downs = 0;
        for a in &mut self.actors {
            if a.kind.is_enemy() && a.alive() && !houdai_immune(a.kind) && overlap(area, a.bbox()) {
                if a.kind.is_boss() {
                    a.boss_hit(crate::actors::BossAttack::Blast, x);
                } else {
                    a.die_fall(x);
                    downs += 1;
                }
            }
        }
        if downs > 0 {
            self.events.push(Event::EnemyDown);
        }
        let (t0, t1) = (
            ((x - 15.9) / TILE).floor() as i32,
            ((x + 15.9) / TILE).floor() as i32,
        );
        let (r0, r1) = (
            ((y - 15.9) / TILE).floor() as i32,
            ((y + 15.9) / TILE).floor() as i32,
        );
        for tx in t0..=t1 {
            for ty in r0..=r1 {
                if self.blocks.get(tx, ty).is_some_and(|b| b.bump.is_none()) {
                    self.hit_block(tx, ty, true, HitPower::Break);
                }
            }
        }
        self.events.push(Event::Explode);
    }

    /// Enemies thrown by the SMB2 Mushroom knock out what they hit (r_damage_usathrow) [obs
    /// name; inf: both fall].
    fn pu_thrown(&mut self) {
        let n = self.actors.len();
        let mut downs = Vec::new();
        for i in 0..n {
            if self.actors[i].mode != Mode::Thrown {
                continue;
            }
            let tb = self.actors[i].bbox();
            if let Some(j) = (0..n).find(|&j| {
                j != i && {
                    let e = &self.actors[j];
                    e.alive() && e.solid_contact() && e.kind.is_enemy() && overlap(tb, e.bbox())
                }
            }) {
                let (tx, ex) = (self.actors[i].x, self.actors[j].x);
                downs.push((j, tx));
                downs.push((i, ex));
            }
        }
        for (j, from) in downs {
            let a = &mut self.actors[j];
            if a.kind.is_boss() {
                a.boss_hit(crate::actors::BossAttack::Shell, from);
            } else if a.alive() {
                a.die_fall(from);
                let (x, y) = (a.x, a.y);
                self.add_score(Self::chain_score(0), x, y + 24.0);
                self.events.push(Event::EnemyDown);
            }
        }
    }

    /// Red POW Box: an upward head bump sets off a POW quake (kind 4) and the block-breaking
    /// ring (MapObjPowBlockRedQuakeGenerator) [obs 0x71015c5a90]; three uses, then 127 frames
    /// of invincibility and the box is gone.
    fn pu_red_pow(&mut self) {
        // the ring grows: blocks and enemies inside it [obs geometry; inf: enemies fall]
        let rings: Vec<(f32, f32, f32)> = self
            .actors
            .iter()
            .filter(|a| a.kind == Kind::RedPowRing && a.mode != Mode::Dead)
            .map(|a| (a.home_x, a.home_y, a.speed))
            .collect();
        for (cx, cy, r) in rings {
            let (t0, t1) = (
                ((cx - r) / TILE).floor() as i32,
                ((cx + r) / TILE).floor() as i32,
            );
            let (r0, r1) = (
                ((cy - r) / TILE).floor() as i32,
                ((cy + r) / TILE).floor() as i32,
            );
            for tx in t0..=t1 {
                for ty in r0..=r1 {
                    let (bx, by) = ((tx as f32 + 0.5) * TILE, (ty as f32 + 0.5) * TILE);
                    if (bx - cx).dhypot(by - cy) > r {
                        continue;
                    }
                    if self.blocks.get(tx, ty).is_some_and(|b| b.bump.is_none()) {
                        self.hit_block(tx, ty, true, HitPower::Break);
                    }
                }
            }
            let px = self.player.x;
            for a in &mut self.actors {
                let (ax, ay) = (a.x, a.y + a.height() / 2.0);
                if a.kind.is_enemy() && a.alive() && (ax - cx).dhypot(ay - cy) <= r {
                    if a.kind.is_boss() {
                        a.boss_hit(crate::actors::BossAttack::Blast, px);
                    } else {
                        a.die_fall(px);
                    }
                }
            }
        }
        for a in &mut self.actors {
            if a.kind == Kind::RedPowRing && a.phase == 1 {
                a.mode = Mode::Dead;
            }
        }
        let p = &self.player;
        let bumped = p.head_hit.is_some() || p.head_solid.is_some();
        if p.pu.headdress != Some(Headdress::RedPow) || !bumped || p.pu.red_uses == 0 {
            return;
        }
        let h = if p.big() { 32.0 } else { 16.0 };
        let (x, y) = (p.x, p.y + 0.5 * h);
        let mut ring = Actor::new(Kind::RedPowRing, x, y);
        // the ring centre snaps to the 16-px grid + 8 [obs]
        ring.home_x = (x / TILE).floor() * TILE + 8.0;
        ring.home_y = (y / TILE).floor() * TILE + 8.0;
        self.add_actor(ring);
        // the POW Land quake: every live enemy standing on something falls [obs 0x7100e65470,
        // kind 4 as the POW block]
        let px = self.player.x;
        for a in &mut self.actors {
            if a.kind.is_enemy() && a.alive() && a.on_ground {
                if a.kind.is_boss() {
                    a.boss_hit(crate::actors::BossAttack::Blast, px);
                } else {
                    a.die_fall(px);
                }
            }
        }
        self.events.push(Event::Break);
        let pu = &mut self.player.pu;
        pu.red_uses -= 1;
        if pu.red_uses == 0 {
            pu.take_off();
            self.player.invuln = crate::player::INVULN_DAMAGE;
            self.events.push(Event::HeadDressOff);
        }
    }

    /// The player's model and frame for the update forms (`model`, `frame` as chosen by the
    /// usual rules). `who` picks the character variant.
    pub(super) fn pu_player_look(
        &self,
        p: &Player,
        form: Form,
        who: usize,
        frame: &'static str,
    ) -> Option<(&'static str, &'static str)> {
        let k3 = |v: &[&'static str; 3], rate: u64| v[(self.tick / rate) as usize % 3];
        let model = |names: [&'static str; 4]| names[who.min(3)];
        Some(match form {
            Form::Link => {
                let m = model([
                    "M1_Player_Link",
                    "M1_Player_Link_2",
                    "M1_Player_Link_3",
                    "M1_Player_Link_4",
                ]);
                let f = match p.pu.link {
                    Link::Slash(t) => {
                        ["attack.0", "attack.1", "attack.2", "attack.3"][(t as usize / 2).min(3)]
                    }
                    Link::Dash { phase: 0, t } => {
                        ["attack.0", "attack.1", "attack.2", "attack.3"][(t as usize / 2).min(3)]
                    }
                    Link::Dash { phase: 1, .. } => "attack_dash.0",
                    Link::Dash { .. } => {
                        k3(&["attack_dash.0", "attack_dash.1", "attack_dash.2"], 2)
                    }
                    Link::Shield => "guard_walk.0",
                    Link::Bow { t, .. } if t < crate::player::BOW_DRAW => {
                        ["charge.0", "charge.1", "charge.2", "charge.3", "charge.4"]
                            [(t as usize / 5).min(4)]
                    }
                    Link::Bow { .. } if !p.on_ground => {
                        if p.vy >= 0.0 {
                            "charge_jump.0"
                        } else {
                            "charge_jump_fall.0"
                        }
                    }
                    Link::Bow { .. } if p.vx.abs() > 1e-5 => {
                        k3(&["charge_walk.0", "charge_walk.1", "charge_walk.2"], 5)
                    }
                    Link::Bow { .. } => "charge.4",
                    Link::Thrust { phase: 0 | 1, t } => [
                        "attack_under_start.0",
                        "attack_under_start.1",
                        "attack_under_start.2",
                        "attack_under_start.3",
                    ][(t as usize / 3).min(3)],
                    Link::Thrust { .. } => "attack_under.0",
                    // a fall plays "jump2"; walking off a ledge "jump" / "jump2" [obs]
                    Link::None if !p.on_ground && p.vy < 0.0 && frame == "jump.0" => "jump2.0",
                    // (Link's swim stroke has 5 frames)
                    Link::None if frame == "swim.5" => "swim.4",
                    Link::None => frame,
                };
                (m, f)
            }
            Form::Smb2 => {
                let m = model([
                    "M1_Player_UsaMarioMdl",
                    "M1_Player_UsaMarioMdl_2",
                    "M1_Player_UsaMarioMdl_3",
                    "M1_Player_UsaMarioMdl_4",
                ]);
                let f = if p.pu.lift > 0 {
                    if p.pu.lift < p.pu.lift_delay {
                        "pull.0"
                    } else {
                        "pull.1"
                    }
                } else if p.crouch && p.pu.charge >= crate::player::CHARGE_FRAMES {
                    if (self.tick / 4).is_multiple_of(2) {
                        "stoop.0"
                    } else {
                        "stoop.1"
                    }
                } else if frame.starts_with("swim.") {
                    // the SMB2 Mushroom's swim art is "swim_up" / "swim_down"
                    let k = (p.walk_phase / 5.0) as usize % 3;
                    if p.vy > 0.0 {
                        ["swim_up.0", "swim_up.1", "swim_up.2"][k]
                    } else {
                        ["swim_down.0", "swim_down.1", "swim_down.2"][k]
                    }
                } else {
                    frame
                };
                (m, f)
            }
            Form::Frog => {
                let m = model([
                    "M3_Player_FrogMarioMdl",
                    "M3_Player_FrogMarioMdl_2",
                    "M3_Player_FrogMarioMdl_3",
                    "M3_Player_FrogMarioMdl_4",
                ]);
                let swim = |v: [&'static str; 3]| v[(p.walk_phase / 5.0) as usize % 3];
                let f = if p.in_water {
                    let up = p.vy > 0.3;
                    let down = p.vy < -0.3;
                    if p.vx.abs() > 0.2 {
                        swim(["swim.0", "swim.1", "swim.2"])
                    } else if up {
                        swim(["swim_up.0", "swim_up.1", "swim_up.2"])
                    } else if down {
                        swim(["swim_down.0", "swim_down.1", "swim_down.2"])
                    } else {
                        ["swim_wait.0", "swim_wait.1"][(self.tick / 8) as usize % 2]
                    }
                } else if !p.on_ground {
                    // "jump" rising, "jump2" falling [obs 0x71015c91a0]
                    if p.vy >= 0.0 { "jump.0" } else { "jump2.0" }
                } else if p.pu.hop {
                    ["walk.0", "walk.1", "walk.2", "walk.3"][(p.walk_phase / 5.0) as usize % 4]
                } else if frame == "turn.0" {
                    "walk.0"
                } else {
                    frame
                };
                (m, f)
            }
            Form::Balloon => {
                let m = model([
                    "MW_Player_BalloonMarioMdl",
                    "MW_Player_BalloonMarioMdl_2",
                    "MW_Player_BalloonMarioMdl_3",
                    "MW_Player_BalloonMarioMdl_4",
                ]);
                // "balloon_wait" / "balloon_move" / "balloon_jet" are float 30 / 12 / 8 frames
                let rate = if p.pu.jet { 1 } else { 4 };
                (m, k3(&["float.0", "float.1", "float.2"], rate))
            }
            Form::Squirrel => {
                let right = p.facing > 0;
                let f = match p.pu.sq {
                    crate::player::Squirrel::Glide { mode: 1 } => {
                        if right {
                            "M_flutterR.0"
                        } else {
                            "M_flutterL.0"
                        }
                    }
                    crate::player::Squirrel::Glide { .. }
                    | crate::player::Squirrel::Flying { .. } => {
                        if right {
                            "M_glideR.0"
                        } else {
                            "M_glideL.0"
                        }
                    }
                    crate::player::Squirrel::Cling { .. } => {
                        if right {
                            "M_wsld_L.0"
                        } else {
                            "M_wsld_R.0"
                        }
                    }
                    crate::player::Squirrel::None => frame,
                };
                ("M1_Player_SquirrelMarioMdl", f)
            }
            Form::Boomerang => {
                let f = if p.pu.throw_t > 0 {
                    if p.on_ground {
                        "Boomerang.0"
                    } else {
                        "BoomerangFloat.0"
                    }
                } else {
                    frame
                };
                ("M1_Player_BoomerangMarioMdl", f)
            }
            _ => return None,
        })
    }

    /// Sprites of the worn boxes (on the head) [inf: drawn as their own model above the head].
    pub(super) fn pu_sprites(&self, out: &mut Vec<Sprite>) {
        let p = &self.player;
        let Some(h) = p.pu.headdress else { return };
        if p.hidden || !p.alive() {
            return;
        }
        let model = match h {
            Headdress::Propeller => "M1_Player_BoxPropeller",
            Headdress::Cannon => "M1_Player_BoxKiller",
            Headdress::Goomba => "M1_Player_BoxKuribo",
            Headdress::RedPow => "M1_Player_BoxPow",
            Headdress::Bullet => "M1_Player_BoxKillerPlayer",
        };
        let head = if p.big() && !p.crouch { 24.0 } else { 12.0 };
        let frame = match h {
            Headdress::Propeller if p.pu.prop > 0 => "Attach.0",
            Headdress::Bullet if p.pu.killer => "Fly.0",
            Headdress::Cannon if self.player.pu.cannon.is_some() => "Shot.0",
            _ => "Wait.0",
        };
        out.push(Sprite {
            image: SpriteImage {
                model,
                texture: frame,
                rect: None,
            },
            x: p.x - 8.0,
            y: p.y + head,
            w: 16.0,
            h: 16.0,
            flip_x: p.facing < 0,
            flip_y: false,
            layer: 51,
        });
    }
}

/// EnemyDB `bound_boomerang` [obs]: the boomerang is knocked back by these.
fn boomerang_bounds(k: Kind, big: bool, style: u8) -> bool {
    match k {
        Kind::Koopa { .. } | Kind::Shell { .. } => style == 4,
        Kind::AntTrooper { .. } => big,
        Kind::Bully | Kind::KoopaCar | Kind::Pow | Kind::PSwitch => true,
        Kind::BoomBoom { .. } => style == 4,
        Kind::SpikeBall { snow: false } => true,
        _ => false,
    }
}

/// EnemyDB `invalid_explode_player_houdai`: Boos and Lava Bubbles shrug off the Cannon Box blast.
fn houdai_immune(k: Kind) -> bool {
    matches!(k, Kind::Boo | Kind::RingBoo { .. } | Kind::LavaBubble)
}
