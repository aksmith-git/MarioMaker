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
    // the player flag Skipsqueak watches (+0x488 bit 14) is set by a jump launched off the
    // ground with a fresh press (also the long jump, the water-exit jump and a spring's high
    // jump) and by each swim stroke, never by stomp bounces or wall jumps [obs 0x71009f0710,
    // 0x7100a104a0, 0x71009d3020]; the player's pre-update clears it the next frame, so it lasts
    // one frame (two for the spring's high jump) [obs 0x71009bc1a0]. Here: airborne and rising
    // after standing, or a stroke's kick.
    let launched = !p.on_ground && p.vy > 0.0 && a.w3.player_ground_prev;
    let stroke = p.in_water && p.vy > 0.0 && p.vy > a.w3.player_vy_prev + 0.5;
    a.w3.player_rising = p.alive() && (launched || stroke);
    a.w3.player_ground_prev = p.on_ground;
    a.w3.player_vy_prev = p.vy;
}

/// Spiky on top: a stomp (or a ground pound) hurts the player, a spin jump bounces off safely.
/// Spiny Skipsqueak and Porcupuffer: stamp Invalid [obs EnemyDB]. The horned Ant Trooper: its
/// EnemyDB row says ReactOnly, but while it walks on the floor its horn flag (+0x510 bit 18)
/// turns stamps and ground pounds into Pass (a plain body touch: hurt, no bounce) and spin jumps
/// into Invalid (a safe bounce, the ant untouched); on a wall or ceiling the flag is off and it
/// takes stomps like the plain one [obs 0x71007610e0, state enters/exits 0x7100603160…
/// 0x7100603850]. Only the big horned one's "Needle" collider (8×8 on the horn) is live, and it
/// hurts on any touch, spin jumps included [obs 0x7100603bc0, 0x71006042c0, callback
/// 0x7100600920].
fn spiky(a: &Actor) -> bool {
    match a.kind {
        Kind::AntTrooper { horned } => horned && a.ant_side() == 0,
        Kind::Skipsqueak { spiny } => spiny,
        Kind::Porcupuffer => true,
        _ => false,
    }
}

/// The big horned Ant Trooper's live Needle on top: even a spin jump is hurt.
fn needle(a: &Actor) -> bool {
    a.kind == (Kind::AntTrooper { horned: true }) && a.big && a.ant_side() == 0
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
        // a creeper head's touch callback does nothing for 16 frames after a hit; otherwise it
        // hurts whatever state it is in, trampled too [obs 0x710071e460]
        if matches!(kind, Kind::PiranhaCreeper { .. }) && a.creeper_shielded() {
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
        let (horn, needle) = (spiky(a), needle(a));
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
                    // a stomp or spin tramples the head 2 pieces back, a ground pound 4; the
                    // player bounces; a Goomba Shoe (or Yoshi) stomp does nothing at all [obs
                    // 0x710071e460, stomp test 0x710076b510]
                    if shoe {
                        return false;
                    }
                    if self.actors[i].creeper_trample(pound) {
                        self.events.push(Event::Stomp);
                    }
                    self.player.bounce(jump_held);
                    return false;
                }
                Kind::AntTrooper { .. } if !shoe && !pound && !horn => {
                    // stamp / spin: ReactOnly, it only squashes for a moment and walks on
                    // [obs EnemyDB, 0x7100601700]; the player bounces (a winged one keeps its
                    // wings)
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
                _ if horn => {
                    if spin && !needle {
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
        if self.player.sliding && matches!(kind, Kind::PiranhaCreeper { .. }) {
            // a slide tramples a creeper head [obs 0x710071e460]
            if self.actors[i].creeper_trample(false) {
                self.events.push(Event::Stomp);
            }
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

    /// The area's Charvaargh list, before the actors step [obs 0x71007a00a0, 0x71007a07c0,
    /// 0x71007a1d60; the list hangs off the area at +0xC0]: every Charvaargh learns the other ones'
    /// side and vertical leap spots on record (for spacing its leaps) and whether another one holds
    /// the turn; with nobody holding it, the first one waiting (in placement order, the order the
    /// game updates them) takes it. Needs calling once a frame ahead of the actor loop in
    /// `World::step`; without that call each Charvaargh leaps on its own.
    pub fn w3_charvaarghs(&mut self) {
        let live: Vec<usize> = (0..self.actors.len())
            .filter(|&i| {
                self.actors[i].kind == Kind::Charvaargh && self.actors[i].mode == Mode::Normal
            })
            .collect();
        if live.is_empty() {
            return;
        }
        let recs: Vec<_> = live
            .iter()
            .map(|&i| self.actors[i].charvaargh_record())
            .collect();
        let mut holder = recs.iter().position(|r| r.2);
        if holder.is_none() {
            holder = recs.iter().position(|r| r.3);
            if let Some(k) = holder {
                self.actors[live[k]].charvaargh_take_turn();
            }
        }
        for (k, &i) in live.iter().enumerate() {
            let others = || recs.iter().enumerate().filter(move |(j, _)| *j != k);
            let side: Vec<f32> = others().filter_map(|(_, r)| r.0).collect();
            let vert: Vec<f32> = others().filter_map(|(_, r)| r.1).collect();
            let busy = holder.is_some_and(|h| h != k);
            self.actors[i].charvaargh_sense(&side, &vert, busy);
        }
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
/// Porcupuffer: one of its 4 HP [obs]; Piranha Creeper: trampled 2 pieces back [obs
/// 0x710071e460]; the rest: DieFall.
pub(super) fn fire_hit(e: &mut Actor, from_x: f32) -> Option<bool> {
    match e.kind {
        Kind::Bully => {
            e.bully_push(bully_push::LIGHT, from_x);
            Some(false)
        }
        Kind::PiranhaCreeper { .. } => {
            e.creeper_trample(false);
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
/// component takes the shell as a heavy push (2.5 px/f) [obs 0x7100649cb0]. The shell is
/// pushed back by the Bully's pusher flag (+0x523 bit 1, Bully builder 0x7100645c20): the
/// generic handler's push path (0x710072bca0 → 0x7100730370, 4-frame cooldown 0x710078c330)
/// calls the shell's push hook 0x71005f7fb0, which faces it away from the Bully, sets its speed
/// to max(4, |v| + 2) with the slide's target speed |v| clamped to 2.5…4, and a 2.4 hop when its
/// vy is below 2.4 [obs v1]. [simplified: the engine's slide has no speed easing, so the shell
/// leaves at the target speed]
/// A sliding shell meeting a Piranha Creeper head tramples it 2 pieces back and is bounced back
/// [obs 0x710071e460; inf: 4 pieces for shells whose collider has flag +0x91 bit 1, not
/// identified]. True when handled.
pub(super) fn shell_hit(a: &mut Actor, b: &mut Actor) -> bool {
    let creeper = |x: &Actor| matches!(x.kind, Kind::PiranhaCreeper { .. });
    if (a.mode == Mode::ShellSlide && creeper(b)) || (b.mode == Mode::ShellSlide && creeper(a)) {
        let (shell, head) = if creeper(b) { (a, b) } else { (b, a) };
        if (head.x - shell.x) * shell.vx > 0.0 && head.creeper_trample(false) {
            shell.dir = -shell.dir;
            shell.vx = -shell.vx;
        }
        return true;
    }
    let (shell, bully) = if a.mode == Mode::ShellSlide && b.kind == Kind::Bully {
        (a, b)
    } else if b.mode == Mode::ShellSlide && a.kind == Kind::Bully {
        (b, a)
    } else {
        return false;
    };
    if (bully.x - shell.x) * shell.vx > 0.0 {
        bully.bully_push_by(bully_push::HEAVY, shell.x, false);
        let away = if shell.x < bully.x { -1 } else { 1 };
        shell.dir = away;
        shell.vx = shell.vx.abs().clamp(2.5, 4.0) * away as f32;
        if shell.vy < 2.4 {
            shell.vy = 2.4;
            shell.on_ground = false;
        }
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
    fn a_horned_ant_trooper_takes_a_spin_jump_safely_unless_it_is_big() {
        let kind = Kind::AntTrooper { horned: true };
        let mut w = world_with(kind);
        w.player.spinning = true;
        assert!(stomp(&mut w, kind), "a spin jump bounces off");
        assert!(w.player.alive());
        assert_eq!(enemy(&w, kind).mode, Mode::Normal, "the ant is untouched");
        // a ground pound is a plain touch: hurt
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
        assert!(!w.player.alive(), "the horn hurts a ground pound");
        // the big one's needle hurts even a spin jump
        let mut lvl = Level::flat(60);
        let mut sp = spawn(kind, 200.0, 32.0);
        sp.big = true;
        lvl.spawns.push(sp);
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        w.player.spinning = true;
        stomp(&mut w, kind);
        assert!(!w.player.alive(), "the needle hurts");
    }

    #[test]
    fn a_stomped_winged_ant_trooper_keeps_its_wings() {
        let kind = Kind::AntTrooper { horned: false };
        let mut lvl = Level::flat(60);
        let mut sp = spawn(kind, 200.0, 80.0);
        sp.wings = true;
        lvl.spawns.push(sp);
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        run(&mut w, 0, 2);
        assert!(stomp(&mut w, kind));
        let a = enemy(&w, kind);
        assert!(a.wings && a.mode == Mode::Normal, "ReactOnly: still flying");
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
        assert!(
            (shell.vx + 2.5).abs() < 1e-4,
            "at the slide's target speed (2.5…4): {}",
            shell.vx
        );
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
    fn charvaarghs_take_turns_to_warn_and_keep_their_leaps_apart() {
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
        lvl.spawns.push(spawn(Kind::Charvaargh, 220.0, 12.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        let (mut both_signing, mut starts) = (false, Vec::new());
        let mut was = [false; 2];
        for _ in 0..700 {
            w.player.y = 200.0; // out of reach
            w.player.vy = 0.0;
            w.w3_charvaarghs();
            run(&mut w, 0, 1);
            let cs: Vec<&Actor> = w
                .actors
                .iter()
                .filter(|a| a.kind == Kind::Charvaargh)
                .collect();
            if cs.len() < 2 {
                continue;
            }
            let signing: Vec<bool> = cs
                .iter()
                .map(|a| a.charvaargh_attacking() && a.charvaargh_segments().is_empty())
                .collect();
            both_signing |= signing[0] && signing[1];
            for k in 0..2 {
                let leaping = !cs[k].charvaargh_segments().is_empty();
                if leaping && !was[k] {
                    starts.push(cs[k].x);
                }
                was[k] = leaping;
            }
        }
        assert!(!both_signing, "one warning at a time");
        assert!(starts.len() >= 2, "both leapt: {starts:?}");
        // 48 px from where the first one was when the second one's warning began
        assert!((starts[0] - starts[1]).abs() > 47.5, "spaced: {starts:?}");
    }

    /// A creeper on a 7-piece path straight up from x 200, grown out past its 4th piece.
    fn creeper_out() -> (World, Kind) {
        let mut lvl = Level::flat(60);
        lvl.creepers.push((0, vec![4, 4, 4, 4, 4, 4, 15]));
        let kind = Kind::PiranhaCreeper {
            path: 0,
            sleeps: false,
        };
        lvl.spawns.push(spawn(kind, 200.0, 32.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        for _ in 0..400 {
            run(&mut w, 0, 1);
            if w.actors.iter().any(|a| a.kind == kind && a.y > 170.0) {
                break;
            }
        }
        assert!(enemy(&w, kind).y > 170.0, "out along its path");
        (w, kind)
    }

    /// Put the player beside the head (touching, not from above) for one frame; true if hurt.
    fn brush(w: &mut World, kind: Kind) -> bool {
        let (x, y) = {
            let a = enemy(w, kind);
            (a.x, a.y)
        };
        w.player.x = x - 14.0;
        w.player.y = y + 4.0;
        w.player.vy = 0.0;
        w.player.invuln = 0;
        run(w, 0, 1);
        !w.player.alive()
    }

    #[test]
    fn creeper_head_is_trampled_by_a_stomp_and_still_bites() {
        let (mut w, kind) = creeper_out();
        // the stalk is drawn behind it
        let stalk = w
            .sprites()
            .iter()
            .filter(|s| s.image.model == "M1_Enemy_packunpipe_body_straight")
            .count();
        assert!(stalk >= 1);
        assert!(stomp(&mut w, kind), "bounced");
        assert!(enemy(&w, kind).creeper_trampled());
        assert!(w.player.alive());
        // for 16 frames after the hit its touch does nothing; then the flat head hurts
        assert!(!brush(&mut w, kind), "shielded");
        w.player.x = 40.0;
        run(&mut w, 0, 16);
        assert!(enemy(&w, kind).creeper_trampled());
        assert!(brush(&mut w, kind), "a trampled head still hurts");
    }

    #[test]
    fn a_creeper_stalk_can_be_stood_on() {
        let mut lvl = Level::flat(60);
        // out of its base to the right, along the ground at 2 tiles up
        lvl.creepers.push((0, vec![2, 2, 2, 2, 2, 14]));
        let kind = Kind::PiranhaCreeper {
            path: 0,
            sleeps: false,
        };
        lvl.spawns.push(spawn(kind, 200.0, 64.0));
        let mut w = World::with_level(lvl);
        w.player.x = 40.0;
        // stop while it's out (it retracts after reaching the end)
        for _ in 0..400 {
            run(&mut w, 0, 1);
            if w.level
                .solids
                .iter()
                .filter(|s| s.owner >= CREEPER_OWNER)
                .count()
                >= 6
            {
                break;
            }
        }
        let tops: Vec<f32> = w
            .level
            .solids
            .iter()
            .filter(|s| s.owner >= CREEPER_OWNER)
            .map(|s| s.y1)
            .collect();
        assert!(tops.len() >= 4, "stalk boxes: {tops:?}");
        let top = tops[0];
        // drop the player onto the stalk, behind the head
        w.player.x = 232.0;
        w.player.y = top + 3.0;
        w.player.vy = 0.0;
        w.player.invuln = 999;
        run(&mut w, 0, 4);
        assert!(
            (w.player.y - top).abs() < 0.5,
            "stands at {} on {top}",
            w.player.y
        );
    }

    #[test]
    fn a_block_bumped_under_a_creeper_head_tramples_it() {
        let (mut w, kind) = creeper_out();
        let i = w.actors.iter().position(|a| a.kind == kind).unwrap();
        // line the head's bottom up with the top of a block row
        let (_, ay0, _, _) = w.actors[i].bbox();
        let ty = (ay0 / TILE).round() as i32 - 1;
        w.actors[i].y += (ty + 1) as f32 * TILE - ay0;
        let tx = (w.actors[i].x / TILE).floor() as i32;
        assert!(!w.actors[i].creeper_trampled());
        w.bump_top(tx, ty);
        assert!(w.actors[i].creeper_trampled());
    }

    #[test]
    fn fire_and_shells_trample_a_creeper_head_and_a_star_defeats_it() {
        let (mut w, kind) = creeper_out();
        shoot(&mut w, kind);
        let a = enemy(&w, kind);
        assert_eq!(a.mode, Mode::Normal, "not burnt");
        assert!(a.creeper_trampled(), "trampled by the fireball");

        let (mut w, kind) = creeper_out();
        let (x, y) = (enemy(&w, kind).x, enemy(&w, kind).y);
        let mut s = Actor::new(Kind::Shell { red: false }, x - 30.0, y);
        s.mode = Mode::ShellWait;
        s.kick(1, 0.0);
        s.kick_lock = 0;
        w.add_actor(s);
        run(&mut w, 0, 8);
        assert!(enemy(&w, kind).creeper_trampled(), "trampled by the shell");
        let shell = w
            .actors
            .iter()
            .find(|a| matches!(a.kind, Kind::Shell { .. }))
            .unwrap();
        assert!(shell.vx < 0.0, "the shell bounced back");

        let (mut w, kind) = creeper_out();
        w.player.star = 600;
        brush(&mut w, kind);
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
