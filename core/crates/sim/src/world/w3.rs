//! World-side rules for the 3D World enemies (actors/w3.rs): what the world senses for them each
//! frame, the player touching them, fireballs and shells hitting them, and the Piranha Creeper's
//! stalk. Reactions follow each spec's EnemyDB rows; the inferred ones are marked [inf].

use super::{Event, World, overlap};
use crate::actors::{Actor, Kind, Mode, STOMP_FRAMES, bully_push, creeper_corner};
use crate::liquid::Liquid;
use crate::player::Player;
use crate::render::Sprite;

/// [inf] a Bully's touch shoves the player away (2.0 px/f, a 2.0 hop) instead of hurting.
const BULLY_SHOVE: f32 = 2.0;
/// [inf] Charvaargh's body segments touch within a 32×32 box round their centres.
const SEGMENT_HALF: f32 = 16.0;

/// Before each step: the liquid and the nearest player, as the 3D World enemies see them.
pub(super) fn sense(a: &mut Actor, liquid: Option<Liquid>, p: &Player) {
    if !a.kind.is_w3() {
        return;
    }
    a.w3.flooded = liquid.is_some_and(|l| l.spec.full);
    a.w3.liquid = liquid.filter(|l| !l.spec.full).map(|l| (l.y, l.water()));
    a.w3.player_wet = p.in_water;
    a.w3.player_h = p.hurt_box().3 - p.y;
    // [?] the spec's "jumping" player flag: read as airborne and rising
    a.w3.player_rising = p.alive() && !p.on_ground && p.vy > 0.0;
}

/// Spiky on top: a stomp hurts the player, a spin jump bounces off safely.
fn spiky(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::AntTrooper { horned: true }
            | Kind::Skipsqueak { spiny: true }
            | Kind::Porcupuffer
            | Kind::Charvaargh
    )
}

impl World {
    /// The player touching a 3D World enemy at `i`. Returns true when the player was hurt (the
    /// caller stops checking this frame).
    pub(super) fn w3_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let hurt = self.player.hurt_box();
        let a = &self.actors[i];
        let kind = a.kind;
        let body = overlap(hurt, a.bbox());
        let segment = kind == Kind::Charvaargh
            && a.charvaargh_segments().iter().any(|&(sx, sy)| {
                let b = (
                    sx - SEGMENT_HALF,
                    sy - SEGMENT_HALF,
                    sx + SEGMENT_HALF,
                    sy + SEGMENT_HALF,
                );
                overlap(hurt, b)
            });
        if !body && !segment {
            return false;
        }
        // a trampled creeper head is flat and harmless [inf]
        if matches!(kind, Kind::PiranhaCreeper { .. }) && a.creeper_trampled() {
            return false;
        }
        let (ax, ay, ah) = (a.x, a.y, a.height());
        let p = &self.player;
        let px = p.x;
        if p.star > 0 {
            // a Super Star knocks them off (Charvaargh shrugs it off [inf])
            if kind != Kind::Charvaargh {
                self.actors[i].die_fall(px);
                self.w3_defeated(ax, ay);
            }
            return false;
        }
        let above = body && p.vy < 0.0 && p.y >= ay + ah * 0.5;
        let (shoe, spin, pound) = (p.shoe.is_some(), p.spinning, p.pound == -1);
        if above {
            match kind {
                Kind::Bully => {
                    // ReactOnly: pushed back, the player bounces
                    let s = if pound || shoe {
                        bully_push::HEAVY
                    } else {
                        bully_push::STOMP
                    };
                    self.actors[i].bully_push(s, px);
                    self.player.bounce(jump_held);
                    self.events.push(Event::Bump);
                    return false;
                }
                Kind::PiranhaCreeper { .. } => {
                    // [inf] a stomp tramples the head (Damage → DamageWait)
                    if self.actors[i].creeper_trample() {
                        self.events.push(Event::Stomp);
                    }
                    self.player.bounce(jump_held);
                    return false;
                }
                Kind::Charvaargh => {
                    if spin {
                        self.player.bounce(jump_held);
                        self.events.push(Event::Bump);
                        return false;
                    }
                }
                _ if shoe => {
                    // stamp_shoe defeats them all (DieVanish / DieStamp in the rows)
                    self.actors[i].die_fall(px);
                    self.player.bounce(jump_held);
                    self.w3_defeated(ax, ay);
                    self.events.push(Event::Stomp);
                    return false;
                }
                k if spiky(k) => {
                    if spin {
                        self.player.bounce(jump_held);
                        self.events.push(Event::Bump);
                        return false;
                    }
                }
                _ => {
                    let a = &mut self.actors[i];
                    if a.wings {
                        // a stomp takes the wings first
                        a.wings = false;
                        a.vy = a.vy.min(0.0);
                    } else if pound || spin {
                        // hipatk / stamp_spin: DieFall (Stingby, Skipsqueak) [obs]; the same
                        // for the others [inf]
                        a.die_fall(px);
                    } else {
                        // DieStamp: flattened (Ant Trooper: the rows say ReactOnly, but in the
                        // game a stomp defeats the plain one [inf])
                        a.mode = Mode::Stomped;
                        a.timer = STOMP_FRAMES;
                        a.vx = 0.0;
                        a.vy = 0.0;
                    }
                    if !pound {
                        self.player.bounce(jump_held);
                    }
                    let n = self.player.stomp_chain;
                    self.player.stomp_chain += 1;
                    self.add_score(Self::chain_score(n), ax, ay + 24.0);
                    self.events.push(Event::Stomp);
                    return false;
                }
            }
        }
        // a slide knocks the small ones over
        let small = matches!(
            kind,
            Kind::AntTrooper { .. } | Kind::Stingby | Kind::Skipsqueak { .. } | Kind::HopChops
        );
        if body && self.player.sliding && small {
            self.actors[i].die_fall(px);
            self.w3_defeated(ax, ay);
            return false;
        }
        if kind == Kind::Bully {
            // [inf] a Bully shoves instead of hurting
            let side = if self.player.x < ax { -1.0 } else { 1.0 };
            self.player.vx = BULLY_SHOVE * side + self.actors[i].vx;
            if self.player.on_ground {
                self.player.vy = BULLY_SHOVE;
                self.player.on_ground = false;
            }
            self.events.push(Event::Bump);
            return false;
        }
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

    fn w3_defeated(&mut self, ax: f32, ay: f32) {
        let n = self.player.stomp_chain;
        self.player.stomp_chain += 1;
        self.add_score(Self::chain_score(n), ax, ay + 24.0);
        self.events.push(Event::EnemyDown);
    }

    /// Piranha Creeper stalks: one piece per path cell behind the head (a corner piece where the
    /// path turns). [inf] the pieces are drawn unrotated: sprites carry no rotation.
    pub(super) fn w3_sprites(&self, out: &mut Vec<Sprite>) {
        for a in &self.actors {
            let Kind::PiranhaCreeper { path, .. } = a.kind else {
                continue;
            };
            if a.mode != Mode::Normal {
                continue;
            }
            let Some((_, codes)) = self.level.creepers.iter().find(|(n, _)| *n == path) else {
                continue;
            };
            for (x, y, code) in a.creeper_stalk(codes) {
                let model = if creeper_corner(code) {
                    "M1_Enemy_packunpipe_body_corner"
                } else {
                    "M1_Enemy_packunpipe_body_straight"
                };
                // the stalk models are centred on their origin and authored at game scale
                out.push(Self::model_sprite(
                    model,
                    "move.0",
                    x - 8.0,
                    y,
                    16.0,
                    16.0,
                    false,
                    39,
                ));
            }
        }
    }
}

/// A player fireball hitting a 3D World enemy: None when `e` is not one (the generic rules
/// apply), else whether it was knocked off. Bully: pushed back (light) [obs ReactOnly];
/// Porcupuffer: one of its 4 HP [obs]; the rest: DieFall.
pub(super) fn fire_hit(e: &mut Actor, from_x: f32) -> Option<bool> {
    match e.kind {
        Kind::Bully => {
            e.bully_push(bully_push::LIGHT, from_x);
            Some(false)
        }
        Kind::Porcupuffer => Some(e.porcupuffer_hit(from_x)),
        k if k.is_w3() => {
            e.die_fall(from_x);
            Some(true)
        }
        _ => None,
    }
}

/// A sliding shell meeting a Bully: shells are Invalid on it [obs], so the shell bounces back
/// [inf]. True when handled.
pub(super) fn shell_hit(a: &mut Actor, b: &mut Actor) -> bool {
    let (shell, bully) = if a.mode == Mode::ShellSlide && b.kind == Kind::Bully {
        (a, b)
    } else if b.mode == Mode::ShellSlide && a.kind == Kind::Bully {
        (b, a)
    } else {
        return false;
    };
    if (bully.x - shell.x) * shell.vx > 0.0 {
        shell.dir = -shell.dir;
        shell.vx = -shell.vx;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::level::LiquidSpec;

    fn spawn(kind: Kind, x: f32, y: f32) -> Spawn {
        Spawn {
            kind,
            x,
            y,
            big: false,
            wings: false,
            pipe: None,
            parachute: false,
            rail: None,
            link: -1,
            clown: false,
        }
    }

    fn run(w: &mut World, buttons: u32, frames: usize) {
        for _ in 0..frames {
            w.step(Input { buttons });
        }
    }

    /// A flat course with one enemy at x = 200, the player parked at x = 40; run so it is active.
    fn world_with(kind: Kind) -> World {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(kind, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        assert!(w.actors.iter().any(|a| a.kind == kind), "spawned");
        w
    }

    fn enemy(w: &World, kind: Kind) -> &Actor {
        w.actors.iter().find(|a| a.kind == kind).expect("enemy")
    }

    /// Drop the player onto the enemy's head from just above; true if the player bounced.
    fn stomp(w: &mut World, kind: Kind) -> bool {
        let (x, y, h) = {
            let a = enemy(w, kind);
            (a.x, a.y, a.height())
        };
        w.player.x = x;
        w.player.y = y + h - 2.0;
        w.player.vy = -1.0;
        w.player.on_ground = false;
        run(w, 0, 1);
        w.player.vy > 2.0
    }

    fn shoot(w: &mut World, kind: Kind) {
        let (x, y) = {
            let a = enemy(w, kind);
            (a.x, a.y)
        };
        let mut f = Actor::new(Kind::Fireball, x, y + 4.0);
        f.timer = 600;
        w.add_actor(f);
        run(w, 0, 1);
    }

    #[test]
    fn plain_ones_are_stomped_spiny_ones_hurt() {
        for kind in [
            Kind::AntTrooper { horned: false },
            Kind::Skipsqueak { spiny: false },
            Kind::HopChops,
        ] {
            let mut w = world_with(kind);
            assert!(stomp(&mut w, kind), "{kind:?}: bounced");
            assert!(w.player.alive());
            assert!(
                w.actors
                    .iter()
                    .all(|a| a.kind != kind || a.mode == Mode::Stomped),
                "{kind:?}: flattened"
            );
        }
        for kind in [
            Kind::AntTrooper { horned: true },
            Kind::Skipsqueak { spiny: true },
        ] {
            let mut w = world_with(kind);
            stomp(&mut w, kind);
            assert!(!w.player.alive(), "{kind:?}: a stomp hurts a small player");
        }
    }

    #[test]
    fn a_stingby_is_stomped_in_the_air() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Stingby, 200.0, 80.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        assert!(stomp(&mut w, Kind::Stingby));
        assert_eq!(enemy(&w, Kind::Stingby).mode, Mode::Stomped);
    }

    #[test]
    fn fireballs_knock_the_small_ones_off() {
        for kind in [
            Kind::AntTrooper { horned: true },
            Kind::Skipsqueak { spiny: true },
            Kind::HopChops,
        ] {
            let mut w = world_with(kind);
            shoot(&mut w, kind);
            assert_eq!(enemy(&w, kind).mode, Mode::DieFall, "{kind:?}");
        }
    }

    #[test]
    fn a_bully_is_pushed_by_stomps_and_fire_and_shoves_the_player() {
        let mut w = world_with(Kind::Bully);
        assert!(stomp(&mut w, Kind::Bully));
        let b = enemy(&w, Kind::Bully);
        assert!(b.alive() && b.bully_pushed());
        run(&mut w, 0, 80);
        shoot(&mut w, Kind::Bully);
        let b = enemy(&w, Kind::Bully);
        assert!(b.alive() && b.bully_pushed());
        assert!(b.vx.abs() > 0.5, "pushed by the fireball");
        // walking into it shoves the player back unhurt
        let (bx, by) = (b.x, b.y);
        w.player.x = bx - 18.0;
        w.player.y = by;
        w.player.vy = 0.0;
        w.player.on_ground = true;
        w.player.invuln = 0;
        run(&mut w, 0, 1);
        assert!(w.player.alive());
        assert!(w.player.vx < -1.0, "shoved left: {}", w.player.vx);
    }

    #[test]
    fn a_star_or_a_block_hit_defeats_a_bully() {
        let mut w = world_with(Kind::Bully);
        w.player.star = 600;
        let (x, y) = (enemy(&w, Kind::Bully).x, enemy(&w, Kind::Bully).y);
        w.player.x = x;
        w.player.y = y;
        run(&mut w, 0, 1);
        assert_eq!(enemy(&w, Kind::Bully).mode, Mode::DieFall);
        assert!(Actor::new(Kind::Bully, 0.0, 0.0).w3_block_hit());
    }

    #[test]
    fn a_shell_bounces_off_a_bully() {
        let mut w = world_with(Kind::Bully);
        let (x, y) = (enemy(&w, Kind::Bully).x, enemy(&w, Kind::Bully).y);
        let mut s = Actor::new(Kind::Shell { red: false }, x - 30.0, y);
        s.mode = Mode::ShellWait;
        s.kick(1, 0.0);
        s.kick_lock = 0;
        w.add_actor(s);
        run(&mut w, 0, 12);
        let shell = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::Shell { .. }))
            .unwrap();
        assert_eq!(shell.mode, Mode::ShellSlide);
        assert!(shell.vx < 0.0, "bounced back");
        assert!(enemy(&w, Kind::Bully).alive());
    }

    #[test]
    fn porcupuffer_takes_four_fireballs_and_cannot_be_stomped() {
        let mut w = world_with(Kind::Porcupuffer);
        for n in 1..=3 {
            shoot(&mut w, Kind::Porcupuffer);
            assert_eq!(enemy(&w, Kind::Porcupuffer).porcupuffer_hp(), 4 - n);
            run(&mut w, 0, 30);
        }
        shoot(&mut w, Kind::Porcupuffer);
        assert_eq!(enemy(&w, Kind::Porcupuffer).mode, Mode::DieFall);

        let mut w = world_with(Kind::Porcupuffer);
        stomp(&mut w, Kind::Porcupuffer);
        assert!(!w.player.alive(), "spiky: a stomp hurts");
    }

    #[test]
    fn porcupuffer_and_charvaargh_live_in_lava() {
        let mut lvl = Level::flat(60);
        lvl.liquid = Some(LiquidSpec {
            kind: 2,
            start: 44.0,
            end: 44.0,
            mode: 0,
            speed: 0,
            full: false,
        });
        lvl.spawns.push(spawn(Kind::Charvaargh, 200.0, 12.0));
        lvl.spawns.push(spawn(Kind::Porcupuffer, 260.0, 80.0));
        lvl.spawns.push(spawn(Kind::HopChops, 300.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        w.player.y = 200.0; // out of reach
        run(&mut w, 0, 60);
        assert!(w.actors.iter().any(|a| a.kind == Kind::Charvaargh));
        assert!(w.actors.iter().any(|a| a.kind == Kind::Porcupuffer));
        assert!(
            w.actors.iter().all(|a| a.kind != Kind::HopChops),
            "lava kills"
        );
    }

    #[test]
    fn charvaargh_hurts_even_starred_players_cannot_beat_it() {
        let mut lvl = Level::flat(60);
        lvl.spawns.push(spawn(Kind::Charvaargh, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        let (x, y) = (enemy(&w, Kind::Charvaargh).x, enemy(&w, Kind::Charvaargh).y);
        w.player.star = 600;
        w.player.x = x;
        w.player.y = y + 8.0;
        run(&mut w, 0, 1);
        assert!(enemy(&w, Kind::Charvaargh).alive());
        w.player.star = 0;
        w.player.invuln = 0;
        let (x, y) = (enemy(&w, Kind::Charvaargh).x, enemy(&w, Kind::Charvaargh).y);
        w.player.x = x;
        w.player.y = y + 8.0;
        run(&mut w, 0, 1);
        assert!(!w.player.alive());
    }

    #[test]
    fn creeper_head_is_trampled_by_a_stomp_and_burnt_by_fire() {
        let mut lvl = Level::flat(60);
        lvl.creepers.push((0, vec![4, 4, 15]));
        let kind = Kind::PiranhaCreeper {
            path: 0,
            sleeps: false,
        };
        lvl.spawns.push(spawn(kind, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 200);
        // out along its path, the stalk drawn behind it
        assert!(enemy(&w, kind).y > 60.0);
        let stalk = w
            .sprites()
            .iter()
            .filter(|s| s.image.model == "M1_Enemy_packunpipe_body_straight")
            .count();
        assert!(stalk >= 1);
        assert!(stomp(&mut w, kind));
        assert!(enemy(&w, kind).creeper_trampled());
        assert!(w.player.alive());
        shoot(&mut w, kind);
        assert_eq!(enemy(&w, kind).mode, Mode::DieFall);
    }

    #[test]
    fn skipsqueak_jumps_when_the_player_jumps() {
        let mut w = world_with(Kind::Skipsqueak { spiny: false });
        w.player.x = 120.0;
        run(&mut w, 0, 10);
        let y0 = enemy(&w, Kind::Skipsqueak { spiny: false }).y;
        run(&mut w, input::JUMP, 4);
        let mut top: f32 = 0.0;
        for _ in 0..40 {
            run(&mut w, input::JUMP, 1);
            top = top.max(enemy(&w, Kind::Skipsqueak { spiny: false }).y - y0);
        }
        assert!(top > 10.0, "jumped {top}");
    }

    #[test]
    fn three_d_world_parts_load_from_course_ids() {
        assert!(Kind::Charvaargh.is_w3() && !Kind::Charvaargh.is_enemy());
        assert!(Kind::Bully.is_enemy() && Kind::Bully.is_w3());
        for id in [50, 51, 65, 86, 96, 107, 114, 117] {
            assert!(crate::level::SUPPORTED_PARTS.contains(&id), "part {id}");
        }
    }
}
