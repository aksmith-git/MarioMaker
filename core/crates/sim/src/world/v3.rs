//! World-side rules for the parts added in SMM2's 2.0 / 3.0 updates (actors/v3.rs,
//! actors/koopalings.rs): the player touching them, fireballs and shells hitting them, carrying
//! a snowball or a knocked-down Mechakoopa, the missile's blast, the P Block and the Dash Block,
//! and their extra sprites. Reactions follow the v3.0.3 EnemyDB rows and the parts' own touch
//! callbacks (cited in the specs); what the binary does not settle is marked [inf] / [ext].

use super::{Event, World, overlap};
use crate::actors::{Actor, Effect, Kind, Mode};
use crate::blocks::{self, BlockKind};
use crate::level::{Cell, TILE};
use crate::render::Sprite;

/// World state of these parts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3World {
    /// Dash Block boost frames left and its direction (+1 right) [obs 0x71015f1d20].
    pub dash: i32,
    pub dash_dir: i32,
}

/// Dash Block [obs 0x71015f1d20, 0x71015d2840, 0x710158d3f0]: standing on one sets a 120-frame
/// timer along the player's facing; while it runs the ground (and air) speed is the star row's
/// run maximum, 4.0 px/f, set outright every frame; it runs out 120 frames after leaving the
/// block or stops at once on a wall, a turn or a loss of control.
pub const DASH_FRAMES: i32 = 120;
pub const DASH_SPEED: f32 = 4.0;
/// The 3D World run meter is held full while dashing (75 frames; 60 as Cat) [obs 0x71015f19f0].
const DASH_METER: i32 = 75;

/// Kick a snowball from the side [obs 0x71010f78d0]: max(1.7, min(5.2, |relative vx| + 0.6)),
/// vy 2.0 (big 1.25), 10 frames without touching.
const SNOW_KICK_MIN: f32 = 1.7;
const SNOW_KICK_MAX: f32 = 5.2;

/// Dash Block boost length (for tests and front ends).
pub fn v3_dash_frames() -> i32 {
    DASH_FRAMES
}

impl World {
    /// The player touching a 2.0 / 3.0 part at `i`. Returns true when the player was hurt (the
    /// caller stops checking this frame).
    pub(super) fn v3_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let a = &self.actors[i];
        if a.held {
            return false;
        }
        match a.kind {
            Kind::Pokey { snow, .. } => self.pokey_contact(i, snow, jump_held),
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => {
                self.koopaling_contact(i, jump_held)
            }
            Kind::MechaLaser => {
                let hit = a
                    .laser_box()
                    .is_some_and(|b| overlap(self.player.hurt_box(), b));
                hit && self.player.star == 0 && self.v3_hurt()
            }
            Kind::OnOffTrampoline { .. } => false,
            _ => {
                if !overlap(self.player.hurt_box(), a.bbox()) {
                    return false;
                }
                match a.kind {
                    Kind::Spike => self.spike_contact(i, jump_held),
                    Kind::SpikeBall { snow } => self.ball_contact(i, snow, jump_held),
                    Kind::Mechakoopa { .. } => self.mecha_contact(i, jump_held),
                    Kind::MechaMissile => self.missile_contact(i, jump_held),
                    _ => false,
                }
            }
        }
    }

    /// The player touched something harmful: true when hurt.
    pub(super) fn v3_hurt(&mut self) -> bool {
        if self.player.hurt() {
            self.events.push(if self.player.alive() {
                Event::PowerDown
            } else {
                Event::Death
            });
            return true;
        }
        false
    }

    pub(super) fn v3_defeated(&mut self, ax: f32, ay: f32, stomp: bool) {
        let n = self.player.stomp_chain;
        self.player.stomp_chain += 1;
        self.add_score(Self::chain_score(n), ax, ay + 24.0);
        self.events.push(if stomp {
            Event::Stomp
        } else {
            Event::EnemyDown
        });
    }

    /// Feet coming down on the top half of the box.
    fn v3_from_above(&self, i: usize) -> bool {
        let a = &self.actors[i];
        self.player.vy < 0.0 && self.player.y >= a.y + a.height() * 0.5
    }

    /// Spike (EnemyDB GabonWU / Gabon3rd): stomps, the shoe, a slide, the Star knock it out
    /// (DieFall; a spin stomp DieVanish); touching its side hurts.
    fn spike_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let (ax, ay) = (self.actors[i].x, self.actors[i].y);
        let px = self.player.x;
        if self.player.star > 0 || self.player.sliding {
            self.actors[i].die_fall(px);
            self.v3_defeated(ax, ay, false);
            return false;
        }
        if self.v3_from_above(i) {
            let a = &mut self.actors[i];
            if a.wings {
                a.wings = false;
                a.vy = a.vy.min(0.0);
            } else if self.player.spinning {
                a.mode = Mode::Dead; // DieVanish
            } else {
                a.die_fall(px);
            }
            if self.player.pound != -1 {
                self.player.bounce(jump_held);
            }
            self.v3_defeated(ax, ay, true);
            return false;
        }
        self.v3_hurt()
    }

    /// A Spike's ball. The stomp result follows the generic handler: an Invalid reaction to a
    /// plain stomp, a shoe stomp or a ground pound (attack types 0, 3, 34 …) turns into Pass, an
    /// ordinary touch, while other Invalid stomps bounce the player unharmed [obs v1
    /// 0x71007610e0 mask 0x580000009, 0x710072bca0]. The iron ball (EnemyDB GabonIronBallWU):
    /// plain stomps and ground pounds are Invalid → Pass, so it hurts from every side; a spin
    /// (stamp_spin Invalid) bounces off [?: the game hurts instead when the collider's class is
    /// the spiky one (5) and a ceiling is within 20 px (+ held item) above; the ball's class is
    /// not mapped]; the shoe and the Star break it. The snowball: a plain stomp is Invalid →
    /// Pass, the same touch as from the side, which kicks it [obs 0x71010f78d0] — or picks it up
    /// with Run held; a spin, the shoe, a ground pound or the Star break it.
    fn ball_contact(&mut self, i: usize, snow: bool, jump_held: bool) -> bool {
        let (ax, ay) = (self.actors[i].x, self.actors[i].y);
        let px = self.player.x;
        if self.actors[i].kick_lock > 0 && self.actors[i].v3.owner == 0 {
            return false; // just kicked or thrown by the player
        }
        if self.player.star > 0 {
            self.actors[i].die_fall(px);
            self.v3_defeated(ax, ay, false);
            return false;
        }
        let above = self.v3_from_above(i);
        let shoe = self.player.shoe.is_some();
        if above && shoe {
            self.actors[i].die_fall(px);
            self.player.bounce(jump_held);
            self.v3_defeated(ax, ay, true);
            return false;
        }
        if !snow {
            if above && self.player.spinning {
                self.player.bounce(jump_held);
                self.events.push(Event::Bump);
                return false;
            }
            return self.v3_hurt();
        }
        if above && (self.player.spinning || self.player.pound == -1) {
            self.actors[i].mode = Mode::Dead; // DieStamp: it bursts
            if self.player.pound != -1 {
                self.player.bounce(jump_held);
            }
            self.v3_defeated(ax, ay, true);
            return false;
        }
        // a plain stomp (Invalid → Pass) is an ordinary touch: carried or kicked as below
        if self.style.can_carry() && self.run_held && self.player.held.is_none() {
            let id = self.actors[i].id;
            self.actors[i].held = true;
            self.actors[i].v3.owner = 0;
            self.player.held = Some(id);
            return false;
        }
        let dir = if ax >= px { 1 } else { -1 };
        let rel = (self.player.vx - self.actors[i].vx).abs();
        let speed = (rel + 0.6).clamp(SNOW_KICK_MIN, SNOW_KICK_MAX);
        let a = &mut self.actors[i];
        let vy = if a.big { 1.25 } else { 2.0 };
        a.ball_launch(dir, speed, vy);
        a.kick_lock = 10;
        a.v3.owner = 0;
        self.events.push(Event::Kick);
        false
    }

    /// Pokey: each segment is its own collider [obs 0x71012597c0]. Stomps count only on the head
    /// and defeat the whole stack (DieStamp); a ground pound is Invalid (it hurts) except on a
    /// Snow Pokey, which it defeats; a slide knocks out the lowest segment; a touch elsewhere
    /// hurts. A Snow Pokey grabbed with Run held gives the player a snowball and is gone
    /// [obs: the grab hits the head; the snowball in hand inf].
    fn pokey_contact(&mut self, i: usize, snow: bool, jump_held: bool) -> bool {
        let hurt = self.player.hurt_box();
        let segs = self.actors[i].pokey_segments();
        let Some(k) = segs
            .iter()
            .position(|&(cx, cy)| overlap(hurt, (cx - 7.8, cy - 7.8, cx + 7.8, cy + 7.8)))
        else {
            return false;
        };
        let (ax, ay) = (self.actors[i].x, self.actors[i].y);
        let px = self.player.x;
        if self.player.star > 0 {
            self.actors[i].die_fall(px);
            self.v3_defeated(ax, ay, false);
            return false;
        }
        let head = k + 1 == segs.len();
        let (_, hy) = segs[k];
        let above = head && self.player.vy < 0.0 && self.player.y >= hy;
        if above {
            if self.player.pound == -1 && !snow {
                return self.v3_hurt(); // hipatk: Invalid [obs]
            }
            if self.actors[i].wings {
                self.actors[i].wings = false;
            } else {
                self.actors[i].die_fall(px);
            }
            if self.player.pound != -1 {
                self.player.bounce(jump_held);
            }
            self.v3_defeated(ax, ay, true);
            return false;
        }
        if self.player.sliding {
            let down = self.actors[i].pokey_hit(0, px);
            self.v3_defeated(ax, ay, false);
            let _ = down;
            return false;
        }
        if snow && self.style.can_carry() && self.run_held && self.player.held.is_none() {
            self.actors[i].mode = Mode::Dead;
            let mut b = Actor::new(Kind::SpikeBall { snow: true }, self.player.x, self.player.y);
            b.held = true;
            b.phase = 3;
            self.add_actor(b);
            self.player.held = self.actors.last().map(|b| b.id);
            self.events.push(Event::Kick);
            return false;
        }
        self.v3_hurt()
    }

    /// Mechakoopa (EnemyDB MechaKoopaGreen/Red/Black): stomp-family hits and ground pounds knock
    /// it down instead of defeating it; a slide or the Star defeats it; while it is down it is
    /// harmless — a touch from the side kicks it, Run picks it up, a stomp knocks it down again
    /// [obs 0x71011cbad0; speeds in 111-mechakoopa.md].
    fn mecha_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let (ax, ay) = (self.actors[i].x, self.actors[i].y);
        let (px, pvx) = (self.player.x, self.player.vx);
        if self.actors[i].kick_lock > 0 {
            return false; // the player ignores it for 20–25 frames after a hit [obs]
        }
        if self.player.star > 0 || self.player.sliding {
            self.actors[i].die_fall(px);
            self.v3_defeated(ax, ay, false);
            return false;
        }
        let above = self.v3_from_above(i);
        let down = self.actors[i].mecha_down();
        let push = (2.3 + 0.35 * pvx.abs()).min(4.0);
        if above {
            let a = &mut self.actors[i];
            if a.wings && !down {
                a.wings = false;
                a.phase = 0;
            } else if self.player.pound == -1 {
                let speed = (3.5 + 0.35 * pvx.abs()).min(4.0);
                a.mecha_knock(px, if down { 0.0 } else { speed }, 2.0, 0.03125);
                a.kick_lock = 25;
            } else if self.player.spinning || self.player.shoe.is_some() {
                if !down {
                    a.mecha_knock(px, push, 1.7, 0.03125);
                }
                a.kick_lock = 20;
            } else {
                // a plain stomp: it falls over where it stands (down again: the timer restarts)
                a.mecha_knock(px, 0.0, 0.0, 0.03125);
                a.kick_lock = 20;
            }
            if self.player.pound != -1 {
                self.player.bounce(jump_held);
            }
            self.events.push(Event::Stomp);
            return false;
        }
        if down {
            if self.style.can_carry() && self.run_held && self.player.held.is_none() {
                let id = self.actors[i].id;
                self.actors[i].held = true;
                self.player.held = Some(id);
                return false;
            }
            let a = &mut self.actors[i];
            a.mecha_knock(px, push, 1.7, 0.03125);
            a.kick_lock = 25;
            self.events.push(Event::Kick);
            return false;
        }
        self.v3_hurt()
    }

    /// Blasta's missile: a stomp destroys it (the player bounces), any other touch hurts
    /// [obs flags; inf].
    fn missile_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let (ax, ay) = (self.actors[i].x, self.actors[i].y);
        if self.player.star > 0 {
            self.actors[i].mode = Mode::Dead;
            self.v3_defeated(ax, ay, false);
            return false;
        }
        if self.v3_from_above(i) {
            self.actors[i].mode = Mode::Dead;
            self.player.bounce(jump_held);
            self.v3_defeated(ax, ay, true);
            return false;
        }
        self.v3_hurt()
    }

    /// Carrying a snowball or a knocked-down Mechakoopa (from `carry_held`). Let go of Run, it
    /// is thrown: the snowball at 2.5 + 0.35·|vx| (≤ 4) with vy 1.0 [obs 0x71010f7d80], the
    /// robot at 3.7 + 0.9·|vx| with vy 1.2 [obs]. Carried, it is an attacker (carried flag →
    /// attack 6, the `shell` column): an enemy that takes shells reacts as to a shell and the
    /// carried thing dies itself (DieFall, no score); one that does not (Bully) is ignored
    /// [obs v1 0x7100732640, 0x710072bca0; the v3 parts share the enemy base].
    pub(super) fn v3_carry(&mut self, i: usize) {
        let p = &self.player;
        let (x, y, facing, pvx) = (p.x + p.facing as f32 * 11.0, p.y + 2.0, p.facing, p.vx);
        let id = self.actors[i].id;
        let a = &self.actors[i];
        let still = match a.kind {
            Kind::SpikeBall { snow: true } => a.mode == Mode::Normal,
            Kind::Mechakoopa { .. } => a.mecha_down(),
            _ => false,
        };
        if !self.run_held || !self.player.alive() || !still {
            let a = &mut self.actors[i];
            a.held = false;
            self.player.held = None;
            if !still || !self.player.alive() {
                return;
            }
            a.x = x;
            match a.kind {
                Kind::SpikeBall { .. } => {
                    let speed = (2.5 + 0.35 * pvx.abs()).min(4.0);
                    a.ball_launch(facing, speed, 1.0);
                    a.kick_lock = 10;
                }
                _ => {
                    let speed = 3.7 + 0.9 * pvx.abs();
                    let from = x - facing as f32;
                    let t = a.timer;
                    a.mecha_knock(from, speed, 1.2, 0.03125);
                    a.timer = t;
                    a.kick_lock = 20;
                }
            }
            self.events.push(Event::Kick);
            return;
        }
        {
            let a = &mut self.actors[i];
            a.x = x;
            a.y = y;
            a.vx = 0.0;
            a.vy = 0.0;
        }
        let sb = self.actors[i].bbox();
        let hit = self
            .actors
            .iter()
            .position(|e| e.id != id && e.alive() && e.kind.is_enemy() && overlap(sb, e.bbox()));
        if let Some(j) = hit
            && shell_attack(&mut self.actors[j], x, y)
        {
            self.actors[i].held = false;
            self.actors[i].die_fall(x - facing as f32);
            self.player.held = None;
            self.events.push(Event::EnemyDown);
        }
    }

    /// Per-frame world rules of these parts (after the P Switch timer, before the actors move).
    pub(super) fn step_v3(&mut self) {
        // P Blocks follow the P Switch: the normal kind is solid while none runs, the
        // alternate one only while one does; the change is immediate both ways [obs
        // 0x7100e25b80]
        let p_on = self.p_timer > 0;
        for k in 0..self.level.v3.p_blocks.len() {
            let (x, y, alt) = self.level.v3.p_blocks[k];
            let solid = alt == p_on;
            self.level
                .set(x, y, if solid { Cell::Solid } else { Cell::Empty });
        }
        self.v3_lasers();
        self.v3_blasts();
    }

    /// Lasers stay on their robot's muzzle and go when it is knocked down or gone [obs].
    fn v3_lasers(&mut self) {
        for li in 0..self.actors.len() {
            if self.actors[li].kind != Kind::MechaLaser {
                continue;
            }
            let owner = self.actors[li].v3.owner;
            let robot = self
                .actors
                .iter()
                .find(|r| r.id == owner && r.alive() && !r.mecha_down() && !r.held)
                .map(|r| (r.mecha_muzzle(), r.dir));
            let l = &mut self.actors[li];
            match robot {
                Some(((mx, my), dir)) => {
                    l.x = mx;
                    l.y = my;
                    l.dir = dir;
                }
                None => l.mode = Mode::Dead,
            }
        }
    }

    /// A missile at the end of its time explodes [obs]: blocks break in a 3×3 square plus the
    /// four tiles two away (as a Bob-omb's), the blast (half 18, big 36) hurts the player and
    /// hits the enemies in it with the shell attack: its collider profile (0x7102a62d28, set up
    /// by _INIT_350, used by Explode enter 0x7100dea640) has attack type 6, the EnemyDB `shell`
    /// column, where the flying missile's has 0 [obs]. So it does what a sliding shell does:
    /// most enemies DieFall, a Koopaling loses 1 HP, a Pokey the touched segment, bosses take a
    /// shell hit. [simplified: one frame; the game's blast collider lives until the Explode
    /// counter passes 15 (0x7100deab10)]
    fn v3_blasts(&mut self) {
        let blasts: Vec<(f32, f32, f32)> = self
            .actors
            .iter()
            .filter(|a| a.missile_due() && a.mode == Mode::Normal)
            .map(|a| (a.x, a.y + a.half_w(), if a.big { 36.0 } else { 18.0 }))
            .collect();
        for &(bx, by, r) in &blasts {
            for a in &mut self.actors {
                if a.missile_due() {
                    a.mode = Mode::Dead;
                    continue;
                }
                let (cx, cy) = (a.x, a.y + a.height() / 2.0);
                if a.kind.is_enemy()
                    && a.alive()
                    && (cx - bx).abs() < r + a.half_w()
                    && (cy - by).abs() < r + a.height() / 2.0
                {
                    blast_hit(a, bx, by);
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
            let hb = self.player.hurt_box();
            if overlap(hb, (bx - r, by - r, bx + r, by + r)) && self.player.star == 0 {
                self.v3_hurt();
            }
            self.events.push(Event::Break);
        }
    }

    /// The Dash Block (from `step`, right after the player moved from `prev_x`).
    pub(super) fn v3_dash(&mut self, prev_x: f32) {
        use crate::player::Control;
        let p = &self.player;
        if !p.alive() || p.control != Control::Normal || p.clown.is_some() || p.car.is_some() {
            self.v3.dash = 0;
            return;
        }
        if p.on_ground && self.level.v3.dash_under(p.x, p.y) {
            self.v3.dash = DASH_FRAMES;
            self.v3.dash_dir = p.facing;
        } else if self.v3.dash > 0 {
            self.v3.dash -= 1;
        }
        if self.v3.dash <= 0 {
            return;
        }
        let dir = self.v3.dash_dir;
        let p = &self.player;
        // a turn, a reversed speed or a wall on the dash side ends it at once [obs]
        let wall_x = p.x + dir as f32 * 7.0;
        let blocked = [p.y + 2.0, p.y + 12.0]
            .iter()
            .any(|&y| self.level.cell_at(wall_x, y).is_solid());
        if p.facing != dir || p.vx * (dir as f32) < 0.0 || blocked {
            self.v3.dash = 0;
            self.player.dash = 0;
            return;
        }
        // the speed is set, not reached: move the rest of the way this frame where it is free
        let want = prev_x + DASH_SPEED * dir as f32;
        let ahead = want + dir as f32 * 7.0;
        let free = [p.y + 2.0, p.y + 12.0]
            .iter()
            .all(|&y| !self.level.cell_at(ahead, y).is_solid());
        if free && (want - p.x) * dir as f32 > 0.0 {
            self.player.x = want;
        }
        self.player.vx = DASH_SPEED * dir as f32;
        self.player.dash = DASH_METER;
    }

    /// Extra sprites: Pokey segments, the held spike ball, Zappa's beam, P Blocks, Dash Blocks
    /// and ON/OFF Trampolines.
    pub(super) fn v3_sprites(&self, out: &mut Vec<Sprite>) {
        let mesh = matches!(self.level.style.as_str(), "WU" | "3W");
        for a in &self.actors {
            match a.kind {
                Kind::Pokey { snow, .. } if a.alive() || a.mode == Mode::DieFall => {
                    let model = if snow {
                        "M1_Enemy_sambosnow"
                    } else {
                        "M1_Enemy_sambo"
                    };
                    let segs = a.pokey_segments();
                    let n = segs.len();
                    for (k, (cx, cy)) in segs.into_iter().enumerate() {
                        // 2D: the textures "head" / "body"; the meshes are picked by model name
                        let tex = match (k + 1 == n, mesh, snow) {
                            (true, _, _) => "head.0",
                            (false, false, _) => "body.0",
                            (false, true, false) => "sambo_body.0",
                            (false, true, true) => "sambosnow_body.0",
                        };
                        // the 2D art is 32 × 32; a mesh frame without SMB1 art is sized
                        // against 16 px (the mesh planner's reference), i.e. game scale
                        let w = if mesh && k + 1 != n { 16.0 } else { 32.0 };
                        let mut s =
                            Self::model_sprite(model, tex, cx - w / 2.0, cy - 8.0, w, w, false, 40);
                        if a.mode == Mode::DieFall {
                            s.flip_y = true;
                            s.layer = 70;
                        }
                        out.push(s);
                    }
                }
                Kind::Spike if a.spike_holding() => {
                    let s = a.v3_scale();
                    let model = if self.level.theme == 6 {
                        "M1_Enemy_gabonballsnow"
                    } else {
                        "M1_Enemy_gabonball"
                    };
                    // [simplified: drawn where the hand joint is at the release frame; the game's
                    // ball follows the joint through the lift]
                    let (hx, hy) = a.spike_hand(a.v3.style as usize);
                    out.push(Self::model_sprite(
                        model,
                        "gabonball_model",
                        hx - 8.0 * s,
                        hy - 8.0 * s,
                        16.0 * s,
                        16.0 * s,
                        false,
                        41,
                    ));
                }
                Kind::MechaLaser => {
                    // [art: the beam is an effect (p_Spark / Beam, textures WU_laser…) that the
                    // importer does not export; drawn under its own name for now]
                    let (len, tex) = if a.timer <= crate::actors::LASER_SIGN {
                        (a.v3.speed, "rangeline.0")
                    } else {
                        (a.v3.len, "beam.0")
                    };
                    let mut x = 0.0;
                    while x < len {
                        let cx = a.x + (x + 8.0) * a.dir as f32;
                        out.push(Self::model_sprite(
                            "M1_Effect_laser",
                            tex,
                            cx - 8.0,
                            a.y - 6.0,
                            16.0,
                            8.0,
                            a.dir < 0,
                            42,
                        ));
                        x += 16.0;
                    }
                }
                Kind::OnOffTrampoline { width, blue } => {
                    let live = blue != self.on_off;
                    let x0 = a.x - width as f32 * 8.0;
                    for i in 0..width {
                        let piece = match (width == 1, i == 0, i + 1 == width) {
                            (true, ..) | (false, false, false) => "unit",
                            (false, true, _) => "left",
                            (false, false, true) => "right",
                        };
                        let tex = match (live, piece) {
                            (true, "left") => "tranpoline_left.0",
                            (true, "right") => "tranpoline_right.0",
                            (true, _) => "tranpoline_unit.0",
                            (false, "left") => "tranpoline_hold_left.0",
                            (false, "right") => "tranpoline_hold_right.0",
                            (false, _) => "tranpoline_hold_unit.0",
                        };
                        out.push(Self::model_sprite(
                            "M1_Object_trampoline_onoff",
                            tex,
                            x0 + i as f32 * TILE,
                            a.y + 8.0,
                            16.0,
                            16.0,
                            false,
                            15,
                        ));
                    }
                }
                _ => {}
            }
        }
        let p_on = self.p_timer > 0;
        for &(x, y, alt) in &self.level.v3.p_blocks {
            let solid = alt == p_on;
            out.push(Self::model_sprite(
                "M1_Object_pblock",
                if solid { "wait.0" } else { "pblock_dot.0" },
                x as f32 * TILE,
                y as f32 * TILE,
                16.0,
                16.0,
                false,
                20,
            ));
        }
        for &(l, row, w) in &self.level.v3.dash_blocks {
            let cx = (l as f32 + w as f32 / 2.0) * TILE;
            out.push(Self::model_sprite(
                "M1_Object_block_dash",
                "wait.0",
                cx - 8.0,
                row as f32 * TILE,
                16.0,
                16.0,
                false,
                20,
            ));
        }
    }
}

/// The shell attack (EnemyDB `shell` column, attack type 6) from (`bx`, `by`) on enemy `e`,
/// through the same rules as a sliding shell (with a still stand-in shell, so nothing is pushed
/// by it): false when `e` does not take it (the Bully's `shell` reaction is Invalid, so the
/// generic handler clears the attack from its accepted mask, v1 0x7100732640).
fn shell_attack(e: &mut Actor, bx: f32, by: f32) -> bool {
    if e.kind == Kind::Bully {
        return false;
    }
    let mut shell = Actor::new(Kind::MechaMissile, bx, by - 8.0);
    shell.mode = Mode::ShellSlide;
    if super::bosses::boss_shell_hit(&mut shell, e).is_some() || super::w3::shell_hit(&mut shell, e)
    {
        return true;
    }
    if e.kind.is_v3() && shell_hit(&mut shell, e).is_some() {
        return true;
    }
    e.die_fall(bx);
    true
}

fn blast_hit(e: &mut Actor, bx: f32, by: f32) {
    shell_attack(e, bx, by);
}

/// A player fireball hitting a 2.0 / 3.0 part at height `fy`: None when `e` is not one (the
/// generic rules apply), else whether it was knocked off. Spike, a snowball: DieFall /
/// DieStamp; the iron ball and Mechakoopas: Invalid (the fireball is spent); a Pokey loses the
/// segment it hits (the head takes the stack) [obs EnemyDB, 0x71012597c0].
pub(super) fn fire_hit(e: &mut Actor, fx: f32, fy: f32) -> Option<bool> {
    if !e.kind.is_v3() {
        return None;
    }
    Some(match e.kind {
        Kind::Spike | Kind::SpikeBall { snow: true } => {
            e.die_fall(fx);
            true
        }
        Kind::Pokey { .. } => {
            let k = e.pokey_segment_at(fy);
            e.pokey_hit(k, fx)
        }
        Kind::Koopaling { .. } => e.koopaling_fire_hit(fx),
        _ => false,
    })
}

/// A sliding shell, a rolling ball or a moving knocked-down Mechakoopa meeting another actor:
/// true when one of them is a 2.0 / 3.0 part and the meeting was handled. Shells knock out
/// these parts (DieFall; a Pokey loses the touched segment; a Koopaling loses 1 HP and the
/// shell is spent [obs reactions]); rolling balls and thrown robots knock out the enemies they
/// run into [ext: as in the game; the balls' enemy side was not traced].
pub(super) fn shell_hit(a: &mut Actor, b: &mut Actor) -> Option<Option<(f32, f32)>> {
    for swap in [false, true] {
        let (s, v) = if swap {
            (&mut *b, &mut *a)
        } else {
            (&mut *a, &mut *b)
        };
        if s.mode == Mode::ShellSlide && v.kind.is_v3() {
            let sx = s.x;
            match v.kind {
                Kind::Pokey { .. } => {
                    let k = v.pokey_segment_at(s.y + 8.0);
                    v.pokey_hit(k, sx);
                }
                Kind::Koopaling { .. } => {
                    v.koopaling_damage(sx, false);
                    s.die_fall(v.x);
                    return Some(None);
                }
                _ => v.die_fall(sx),
            }
            s.chain += 1;
            return Some(Some((v.x, v.y)));
        }
        let rolling = matches!(s.kind, Kind::SpikeBall { .. }) && !s.ball_resting();
        let thrown = s.mecha_down() && s.vx.abs() >= 1.5;
        if (rolling || thrown)
            && v.kind.is_enemy()
            && !v.kind.is_v3()
            && !v.kind.is_boss()
            && v.mode != Mode::ShellSlide
        {
            let sx = s.x;
            v.die_fall(sx);
            return Some(Some((v.x, v.y)));
        }
        if thrown && v.kind.is_v3() {
            // a thrown enemy carries the shell attack to every enemy, 2.0 / 3.0 parts included
            // (attacker flags → attack 6, v1 0x7100732640)
            let (sx, sy) = (s.x, s.y + 8.0);
            let (vx, vy) = (v.x, v.y);
            return Some(shell_attack(v, sx, sy).then_some((vx, vy)));
        }
        if rolling && v.kind.is_v3() {
            // a Spike's rolling ball and a 2.0 / 3.0 part pass each other [?: the v1 handler
            // makes an enemy an attacker only through flags (ShellSlide, carried, released,
            // a fixed attacker box; v1 0x7100732640) and the ball's states set none of their
            // v3 counterparts (the flag block moved by 0x10), which would make it harmless to
            // every enemy; the v3 attacker test itself was not found, so the old rule stays]
            return Some(None);
        }
    }
    if a.kind.is_v3() || b.kind.is_v3() {
        return Some(None);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::Seg;

    #[test]
    fn a_missile_blast_hits_like_a_shell() {
        // a Pokey loses only the segment at the blast's height; a Goomba is knocked out
        let mut p = Actor::new(
            Kind::Pokey {
                snow: false,
                height: 4,
            },
            100.0,
            0.0,
        );
        p.phase = 1;
        p.v3.segs = vec![Seg::default(); 4];
        blast_hit(&mut p, 100.0, 8.0);
        assert!(p.alive());
        assert_eq!(p.v3.segs.len(), 3);
        let mut g = Actor::new(Kind::Goomba, 100.0, 0.0);
        blast_hit(&mut g, 100.0, 8.0);
        assert_eq!(g.mode, Mode::DieFall);
    }

    #[test]
    fn a_bully_does_not_take_the_shell_attack() {
        // (so a snowball carried into it survives)
        let mut b = Actor::new(Kind::Bully, 100.0, 0.0);
        assert!(!shell_attack(&mut b, 90.0, 8.0));
        assert!(b.alive());
    }
}
