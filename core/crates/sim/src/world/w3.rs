//! World-side rules for the 3D World enemies (actors/w3.rs): what the world senses for them each
//! frame, the player touching them, fireballs and shells hitting them, and the Piranha Creeper's
//! stalk. Reactions follow each spec's EnemyDB rows and the enemies' own touch callbacks; the
//! few the binary does not settle are marked [inf] with the reason.

use super::{Event, World, overlap};
use crate::actors::{Actor, Kind, Mode, STOMP_FRAMES, bully_push, creeper_corner};
use crate::liquid::Liquid;
use crate::player::Player;
use crate::render::Sprite;

/// Walking into a Bully puts the player in its Pushed state: thrown back at 3.0 px/f, and every
/// 4th push also 4.0 up [obs player state 0x4f, 0x71009d5b90]. The Pushed state's
/// control lock (until 20 frames of its animation have played) lives in the player code and is
/// not modelled here.
const BULLY_SHOVE: f32 = 3.0;
const BULLY_SHOVE_HOP: f32 = 4.0;
/// Charvaargh's 8 body touch boxes are 40×40 round their centres [obs 0x710079e290].
const SEGMENT_HALF: f32 = 20.0;

/// Before each step: the liquid and the nearest player, as the 3D World enemies see them.
pub(super) fn sense(a: &mut Actor, liquid: Option<Liquid>, p: &Player) {
    if !a.kind.is_w3() {
        return;
    }
    a.w3.flooded = liquid.is_some_and(|l| l.spec.full);
    a.w3.liquid = liquid.filter(|l| !l.spec.full).map(|l| (l.y, l.water()));
    a.w3.player_wet = p.in_water;
    a.w3.player_h = p.hurt_box().3 - p.y;
    // the player flag Skipsqueak watches (+0x488 bit 14) is set when a jump is launched and on
    // each swim stroke [obs 0x71009f0710, 0x7100a104a0]; it lasts that one frame [inf: its
    // clearing was not found]. Here: airborne and rising after standing, or a stroke's kick.
    let launched = !p.on_ground && p.vy > 0.0 && a.w3.player_ground_prev;
    let stroke = p.in_water && p.vy > 0.0 && p.vy > a.w3.player_vy_prev + 0.5;
    a.w3.player_rising = p.alive() && (launched || stroke);
    a.w3.player_ground_prev = p.on_ground;
    a.w3.player_vy_prev = p.vy;
}

/// Spiky on top: a stomp hurts the player, a spin jump bounces off safely. Spiny Skipsqueak and
/// Porcupuffer: stamp Invalid [obs EnemyDB]; the horned Ant Trooper's stamp row is ReactOnly,
/// its top hurts through the "Needle" collider on its horn [inf: that collider's callback was
/// not traced].
fn spiky(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::AntTrooper { horned: true } | Kind::Skipsqueak { spiny: true } | Kind::Porcupuffer
    )
}

impl World {
    /// The player touching a 3D World enemy at `i`. Returns true when the player was hurt (the
    /// caller stops checking this frame).
    pub(super) fn w3_contact(&mut self, i: usize, jump_held: bool) -> bool {
        let hurt = self.player.hurt_box();
        let a = &self.actors[i];
        let kind = a.kind;
        // a Charvaargh touches only through its 8 body boxes while it leaps [obs 0x710079e290]
        let body = kind != Kind::Charvaargh && overlap(hurt, a.bbox());
        let segment = kind == Kind::Charvaargh
            && a.charvaargh_segments().iter().any(|&(sx, sy)| {
                let h = SEGMENT_HALF;
                overlap(hurt, (sx - h, sy - h, sx + h, sy + h))
            });
        if !body && !segment {
            return false;
        }
        // a trampled creeper head is flat and harmless [inf: whether its touch callback hurts
        // in Damage / DamageWait was not traced]
        if matches!(kind, Kind::PiranhaCreeper { .. }) && a.creeper_trampled() {
            return false;
        }
        let (ax, ay, ah) = (a.x, a.y, a.height());
        let p = &self.player;
        let px = p.x;
        if p.star > 0 {
            // a Super Star knocks them all off, Charvaargh too [obs 0x710079c5f0]
            self.actors[i].die_fall(px);
            self.w3_defeated(ax, ay);
            return false;
        }
        if kind == Kind::Charvaargh {
            // anything else touching it is hurt: no stomp, no spin bounce [obs 0x710079c5f0]
            return self.w3_hurt();
        }
        let above = body && p.vy < 0.0 && p.y >= ay + ah * 0.5;
        let (shoe, spin, pound) = (p.shoe.is_some(), p.spinning, p.pound == -1);
        if above {
            match kind {
                Kind::Bully => {
                    // ReactOnly: pushed back (a ground pound hardest), the player bounces
                    // [obs 0x7100649cb0]
                    let s = if pound {
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
                    // a stomp tramples the head (Damage → DamageWait) [inf: which hits set the
                    // head's damage flags was not traced; the Trampled animations say stomps]
                    if self.actors[i].creeper_trample() {
                        self.events.push(Event::Stomp);
                    }
                    self.player.bounce(jump_held);
                    return false;
                }
                Kind::AntTrooper { horned } if !shoe && !pound && (!horned || spin) => {
                    // stamp / spin: ReactOnly, it only squashes for a moment and walks on
                    // [obs EnemyDB, 0x7100601700]; the player bounces
                    self.player.bounce(jump_held);
                    self.events.push(Event::Bump);
                    return false;
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
                        // hipatk / stamp_spin: DieFall (Hop-Chops' spin: DieVanish) [obs EnemyDB]
                        a.die_fall(px);
                    } else {
                        // DieStamp: flattened [obs EnemyDB]
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
            // the touch never hurts: the Bully is pushed (light, 7 frames apart) and the
            // player thrown back [obs 0x7100649cb0, player Pushed 0x71009d5b90]
            let b = &mut self.actors[i];
            if b.bully_body_push(px) {
                let side = if px < ax { -1.0 } else { 1.0 };
                self.player.vx = BULLY_SHOVE * side;
                if b.bully_shove_count().is_multiple_of(4) {
                    self.player.vy = BULLY_SHOVE_HOP;
                    self.player.on_ground = false;
                }
                self.events.push(Event::Bump);
            }
            return false;
        }
        self.w3_hurt()
    }

    /// The player touched something harmful: true when hurt.
    fn w3_hurt(&mut self) -> bool {
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
    /// path turns). They are drawn unrotated: the sprite list carries no rotation.
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

/// A sliding shell meeting a Bully: the shell attack is Invalid in its EnemyDB row, but its push
/// component takes the shell as a heavy push (2.5 px/f) [obs 0x7100649cb0]; the shell bounces
/// back [inf: the shell's side is the generic enemy-vs-enemy handler 0x710072bca0, not traced].
/// True when handled.
pub(super) fn shell_hit(a: &mut Actor, b: &mut Actor) -> bool {
    let (shell, bully) = if a.mode == Mode::ShellSlide && b.kind == Kind::Bully {
        (a, b)
    } else if b.mode == Mode::ShellSlide && a.kind == Kind::Bully {
        (b, a)
    } else {
        return false;
    };
    if (bully.x - shell.x) * shell.vx > 0.0 {
        bully.bully_push_by(bully_push::HEAVY, shell.x, false);
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
            alt: false,
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
        for kind in [Kind::Skipsqueak { spiny: false }, Kind::HopChops] {
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
    fn an_ant_trooper_shrugs_off_a_stomp_but_not_a_ground_pound() {
        let kind = Kind::AntTrooper { horned: false };
        let mut w = world_with(kind);
        assert!(stomp(&mut w, kind), "bounced");
        assert!(w.player.alive());
        assert_eq!(
            enemy(&w, kind).mode,
            Mode::Normal,
            "ReactOnly: still walking"
        );
        let mut w = world_with(kind);
        let (x, y, h) = {
            let a = enemy(&w, kind);
            (a.x, a.y, a.height())
        };
        w.player.x = x;
        w.player.y = y + h - 2.0;
        w.player.vy = -1.0;
        w.player.pound = -1;
        w.player.on_ground = false;
        run(&mut w, 0, 1);
        let e = enemy(&w, kind);
        assert_eq!(
            e.mode,
            Mode::DieFall,
            "hipatk: DieFall (player y {}, alive {})",
            w.player.y,
            w.player.alive()
        );
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
        // walking into it shoves the player back unhurt, and pushes the Bully lightly
        run(&mut w, 0, 80);
        let (bx, by) = {
            let b = enemy(&w, Kind::Bully);
            (b.x, b.y)
        };
        w.player.x = bx - 18.0;
        w.player.y = by;
        w.player.vy = 0.0;
        w.player.on_ground = true;
        w.player.invuln = 0;
        run(&mut w, 0, 1);
        assert!(w.player.alive());
        assert!(w.player.vx < -2.0, "shoved left: {}", w.player.vx);
        let b = enemy(&w, Kind::Bully);
        assert!(b.bully_pushed() && b.vx > 0.5, "pushed right: {}", b.vx);
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
        let b = enemy(&w, Kind::Bully);
        assert!(b.alive() && b.bully_pushed(), "a shell pushes it hard");
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

    /// A castle course with lava at 44 and a Charvaargh in it; runs until it leaps.
    fn leaping_charvaargh() -> World {
        let mut lvl = Level::flat(60);
        lvl.theme = 2;
        lvl.liquid = Some(LiquidSpec {
            kind: 2,
            start: 44.0,
            end: 44.0,
            mode: 0,
            speed: 0,
            full: false,
        });
        lvl.spawns.push(spawn(Kind::Charvaargh, 200.0, 12.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        w.player.y = 200.0; // out of reach
        run(&mut w, 0, 2);
        assert!(
            enemy(&w, Kind::Charvaargh).charvaargh_segments().is_empty(),
            "harmless while it waits"
        );
        for _ in 0..300 {
            if enemy(&w, Kind::Charvaargh).charvaargh_segments().len() == 8 {
                break;
            }
            w.player.y = 200.0;
            w.player.vy = 0.0;
            run(&mut w, 0, 1);
        }
        // 100 frames in, the head and the front of the body are well above the lava
        for _ in 0..100 {
            w.player.y = 200.0;
            w.player.vy = 0.0;
            run(&mut w, 0, 1);
        }
        w
    }

    #[test]
    fn a_leaping_charvaargh_hurts_through_its_body_and_only_a_star_beats_it() {
        let mut w = leaping_charvaargh();
        let (sx, sy) = enemy(&w, Kind::Charvaargh).charvaargh_segments()[2];
        assert!(sy > 80.0, "above the lava: {sy}");
        w.player.x = sx;
        w.player.y = sy - 8.0;
        w.player.vy = -1.0;
        w.player.spinning = true; // no spin bounce off it
        w.player.invuln = 0;
        run(&mut w, 0, 1);
        assert!(!w.player.alive(), "hurt by its body");

        let mut w = leaping_charvaargh();
        let (sx, sy) = enemy(&w, Kind::Charvaargh).charvaargh_segments()[1];
        w.player.star = 600;
        w.player.x = sx;
        w.player.y = sy - 8.0;
        run(&mut w, 0, 1);
        assert_eq!(enemy(&w, Kind::Charvaargh).mode, Mode::DieFall);
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
