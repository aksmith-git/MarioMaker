//! World-side rules for the Koopalings (actors/koopalings.rs): the player touching them and
//! their projectiles, the volleys they fire, Morton's and Roy's ground quake, Wendy's ring
//! limit, the projectiles vanishing with their owner, and the Key a Koopaling drops.

use super::{Event, World, overlap};
use crate::actors::{Actor, Kind, Mode};
use crate::render::Sprite;

/// Players in view of a quake are stunned for 80 frames (player state 0x58 [obs
/// 0x71015a6880]; grounded players only [inf]).
const QUAKE_STUN: i32 = 80;

impl World {
    /// The player touching a Koopaling or one of its projectiles at `i`; true when hurt.
    pub(super) fn koopaling_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let hurt = self.player.hurt_box();
        let a = &self.actors[i];
        if a.koopaling_hidden() || !overlap(hurt, a.bbox()) {
            return false;
        }
        let (ax, ay) = (a.x, a.y);
        let px = self.player.x;
        let star = self.player.star > 0;
        if let Kind::KoopalingShot { what } = a.kind {
            if star {
                return false;
            }
            // a player can bounce on Lemmy's balls [inf from its bounce callbacks]
            if what == crate::actors::koopaling_shot::BALL && self.v3_from_above_pub(i) {
                self.player.bounce(jump_held);
                self.events.push(Event::Bump);
                return false;
            }
            return self.v3_hurt();
        }
        if star {
            // star: DieFall at once [obs EnemyDB]
            self.actors[i].die_fall(px);
            self.v3_defeated(ax, ay, false);
            return false;
        }
        if self.actors[i].kick_lock > 0 && !self.actors[i].koopaling_shell_moving() {
            return false;
        }
        let above = self.v3_from_above_pub(i);
        if above {
            let pound = self.player.pound == -1;
            if self.actors[i].koopaling_guarded() {
                // in the shell: stomps bounce off [obs]
                if !pound {
                    self.player.bounce(jump_held);
                }
                self.events.push(Event::Bump);
                return false;
            }
            let (_, down) = self.actors[i].koopaling_damage(px, true);
            self.actors[i].kick_lock = 20;
            if !pound {
                self.player.bounce(jump_held);
            }
            if down {
                self.v3_defeated(ax, ay, true);
                self.events.push(Event::EnemyDown);
            } else {
                self.events.push(Event::Stomp);
            }
            return false;
        }
        if self.player.sliding && !self.actors[i].koopaling_guarded() {
            let (_, down) = self.actors[i].koopaling_damage(px, false);
            self.actors[i].kick_lock = 20;
            if down {
                self.v3_defeated(ax, ay, false);
            }
            return false;
        }
        self.v3_hurt()
    }

    pub(super) fn v3_from_above_pub(&self, i: usize) -> bool {
        let a = &self.actors[i];
        self.player.vy < 0.0 && self.player.y >= a.y + a.height() * 0.5
    }

    /// Per frame: volleys, Morton's flames, quakes, Wendy's ring limit, orphaned projectiles,
    /// dropped Keys.
    pub(super) fn koopaling_world(&mut self) {
        let mut born: Vec<Actor> = Vec::new();
        let mut quake = false;
        let mut keys = Vec::new();
        let v = self.view_rect(0.0);
        for i in 0..self.actors.len() {
            if !matches!(self.actors[i].kind, Kind::Koopaling { .. }) {
                continue;
            }
            if self.actors[i].v3.shots > 0 {
                self.actors[i].v3.shots = 0;
                let level = &self.level;
                born.extend(self.actors[i].koopaling_shots(level));
            }
            let a = &mut self.actors[i];
            if a.v3.flames {
                a.v3.flames = false;
                born.extend(a.morton_flames());
            }
            if a.v3.quake {
                a.v3.quake = false;
                quake |= a.x > v.0 && a.x < v.2 && a.y > v.1 && a.y < v.3;
            }
            if !a.alive() && a.koopaling_key() {
                keys.push((a.x, a.y + a.height() / 2.0 - 8.0));
            }
        }
        if quake {
            if self.player.on_ground && self.player.star == 0 {
                self.player.stun = QUAKE_STUN;
                self.player.vx = 0.0;
            }
            self.events.push(Event::Bump);
        }
        for a in born {
            self.add_actor(a);
        }
        for (x, y) in keys {
            self.add_actor(Actor::new(Kind::Key, x, y));
        }
        // projectiles go when their Koopaling is defeated or gone [obs 0x7100dd9770]
        let live: Vec<(u32, u8)> = self
            .actors
            .iter()
            .filter(|a| matches!(a.kind, Kind::Koopaling { .. }) && a.alive() && a.v3.hp > 0)
            .map(|a| (a.id, a.v3.hp))
            .collect();
        let ring = crate::actors::koopaling_shot::RING;
        for (id, hp) in &live {
            // Wendy: 1 / 2 / 3 rings at 3 / 2 / 1 HP; beyond that the oldest goes 60 frames
            // later [obs 0x71012bf260]
            let limit = (4 - hp) as usize;
            let rings: Vec<usize> = (0..self.actors.len())
                .filter(|&k| {
                    let r = &self.actors[k];
                    r.kind == (Kind::KoopalingShot { what: ring })
                        && r.v3.owner == *id
                        && r.mode == Mode::Normal
                })
                .collect();
            if rings.len() > limit {
                let k = rings[0];
                if self.actors[k].timer > 60 {
                    self.actors[k].timer = 60;
                }
            }
        }
        for a in &mut self.actors {
            if let Kind::KoopalingShot { .. } = a.kind
                && a.mode == Mode::Normal
                && !live.iter().any(|(id, _)| *id == a.v3.owner)
            {
                a.mode = Mode::Dead;
            }
        }
    }

    /// Lemmy's ball under him while he rides it.
    pub(super) fn koopaling_sprites(&self, out: &mut Vec<Sprite>) {
        for a in &self.actors {
            if let Some((x, cy, r)) = a.lemmy_ball()
                && a.mode == Mode::Normal
            {
                out.push(Self::model_sprite(
                    "M1_Enemy_lemmy",
                    "ball.0",
                    x - r,
                    cy,
                    2.0 * r,
                    2.0 * r,
                    false,
                    39,
                ));
                // Lemmy himself stands on top of it
                if let Some((model, tex, w, h, flip)) = a.koopaling_body_look() {
                    out.push(Self::model_sprite(
                        model,
                        tex,
                        x - w / 2.0,
                        cy + r,
                        w,
                        h,
                        flip,
                        40,
                    ));
                }
            }
        }
    }
}
