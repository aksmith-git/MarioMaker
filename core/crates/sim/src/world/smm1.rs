//! SMM1 extras (Phase 11, `crate::options::Options`): rules from the original Super Mario Maker
//! that hosts can switch on over SMM2 play. Behaviour specs: research/specs/smm1-extras.md.

use super::{Event, KICK_SCORE, World};
use crate::actors::{Kind, Mode};
use crate::level::{Cell, TILE};
use crate::player::STOMP_BOUNCE;

/// What the SMM1 shell rules made of a player–shell contact.
pub(super) enum Smm1Shell {
    /// Handled (kicked, stomped, or no contact during the kicker's no-hit window).
    Done,
    /// A sliding shell ran into the player: the normal damage path.
    Hurt,
}

/// The mid-air wall assist's hop: 0.9 × the stomp bounce (shell spec §4.4).
pub const ASSIST_HOP: f32 = 0.9 * STOMP_BOUNCE;
/// Kick speed, the running bonus, the catch-up re-kick (§4.1).
const KICK_SPEED: f32 = 2.5;
const KICK_RUN: f32 = 0.5;
const REKICK_SPEED: f32 = 4.0;
/// The brake of an assisted shell after it bounced off the wall (px/frame²) (§4.4).
const ASSIST_BRAKE: f32 = 0.5;
/// No-hit window every contact leaves when none is running (contact epilogue, §3).
const CONTACT_WINDOW: i32 = 5;

impl World {
    /// The SMM1 rules for the player touching a Koopa / Buzzy / Spiny shell (shell spec §3–§4):
    /// no contact while the kicker's no-hit window runs; a still shell is kicked from the side or
    /// from above alike, without a bounce (or with the wall assist's hop); a sliding shell is
    /// stopped by a stomp (bounce 3.9095), re-kicked at 4.0 when caught from behind, and hurts
    /// otherwise. Every contact leaves a 5-frame window when none is running.
    pub(super) fn smm1_shell_contact(
        &mut self,
        i: usize,
        stomp: bool,
        jump_held: bool,
    ) -> Smm1Shell {
        let a = &self.actors[i];
        if a.held {
            return Smm1Shell::Done;
        }
        if a.kick_lock > 0 {
            return Smm1Shell::Done;
        }
        let (ax, ay) = (a.x, a.y);
        let p = &self.player;
        // kicks go away from the player (centre comparison)
        let dir = if ax >= p.x { 1 } else { -1 };
        let result = match a.mode {
            Mode::ShellWait => {
                if self.style.can_carry() && self.run_held && p.held.is_none() {
                    // pick it up (as in SMM2)
                    let id = a.id;
                    self.actors[i].held = true;
                    self.player.held = Some(id);
                    return Smm1Shell::Done;
                }
                self.smm1_kick(i, dir, false);
                Smm1Shell::Done
            }
            Mode::ShellSlide => {
                let rekick = !stomp
                    && a.assist.is_none()
                    && (a.on_ground || a.y < p.y + 4.0)
                    && dir == a.dir
                    && a.vx * p.vx > 0.0;
                if rekick {
                    self.smm1_kick(i, dir, true);
                    Smm1Shell::Done
                } else if stomp {
                    // a stomp stops it dead; the player bounces
                    self.actors[i].assist = None;
                    if self.actors[i].stomp() {
                        self.player.bounce(jump_held);
                        let n = self.player.stomp_chain;
                        self.player.stomp_chain += 1;
                        self.add_score(Self::chain_score(n), ax, ay + 24.0);
                        self.events.push(Event::Stomp);
                    }
                    Smm1Shell::Done
                } else {
                    Smm1Shell::Hurt
                }
            }
            _ => Smm1Shell::Hurt,
        };
        let s = &mut self.actors[i];
        if s.kick_lock == 0 {
            s.kick_lock = CONTACT_WINDOW;
        }
        result
    }

    /// Kick shell `i` in `dir` (kick handler, §4): the speed, the kicker's no-hit window, and
    /// the mid-air wall assist.
    fn smm1_kick(&mut self, i: usize, dir: i32, rekick: bool) {
        let p = &self.player;
        let (pvx, p_ground, p_in_water) = (p.vx, p.on_ground, p.in_water);
        let s = &self.actors[i];
        let speed = if rekick {
            REKICK_SPEED
        } else if pvx != 0.0 && pvx.signum() as i32 == dir {
            KICK_SPEED + KICK_RUN * pvx.abs()
        } else {
            KICK_SPEED
        };
        let assist = (!rekick).then(|| self.wall_assist(i, dir)).flatten();
        let (ax, ay) = (s.x, s.y);
        let window = {
            let s = &self.actors[i];
            let mut w = 16;
            if p_ground && s.on_ground {
                w = if (s.x - self.player.x).abs() < 8.0 {
                    8
                } else {
                    4
                };
            } else if p_in_water && !p_ground && self.player.y > s.y {
                // squeezed between two walls in the water [inf: the probes at +8 px, ±12 px]
                let probe = |dx: f32| self.level.cell_at(s.x + dx, s.y + 8.0).is_solid();
                if probe(-12.0) && probe(12.0) {
                    w = 4;
                }
            }
            if rekick {
                w += 4;
            }
            if p_in_water {
                w += 8;
            }
            w
        };
        let s = &mut self.actors[i];
        s.kick(dir, 0.0);
        s.vx = speed * dir as f32;
        s.kick_lock = if assist.is_some() { 0 } else { window };
        s.assist = assist.map(|stop| (stop, dir));
        if assist.is_some() {
            // the hop, through the same jump request as a stomp
            self.player.vy = ASSIST_HOP;
            self.player.on_ground = false;
            self.player.slope = None;
        }
        self.add_score(KICK_SCORE, ax, ay + 24.0);
        self.events.push(Event::Kick);
    }

    /// The mid-air wall assist (§4.4): the player falling (vy ≤ 0) with no floor within 16·k px
    /// under a point 1.5 px ahead of them, no floor within 16·k px under a point 16·k px behind
    /// the shell, and a wall 10·k … 16·k px ahead of the shell (k = 2 for a big shell). Returns the
    /// stop point (one tile short of the wall).
    fn wall_assist(&self, i: usize, dir: i32) -> Option<f32> {
        let p = &self.player;
        let s = &self.actors[i];
        if p.on_ground || p.vy > 0.0 {
            return None;
        }
        let k = if s.big { 2.0 } else { 1.0 };
        let reach = 16.0 * k;
        let d = dir as f32;
        let floor_below = |x: f32, y: f32| {
            let mut yy = y;
            while yy >= y - reach {
                if matches!(
                    self.level.cell_at(x, yy),
                    Cell::Solid | Cell::Spike | Cell::SemiTop
                ) {
                    return true;
                }
                yy -= 1.0;
            }
            false
        };
        if floor_below(p.x + 1.5 * d, p.y) {
            return None;
        }
        let cy = s.y + s.height() / 2.0;
        if floor_below(s.x - reach * d, cy) {
            return None;
        }
        let wall = (0..=reach as i32)
            .map(|n| n as f32)
            .find(|&n| self.level.cell_at(s.x + n * d, cy).is_solid())?;
        if wall < 10.0 * k {
            return None;
        }
        Some(s.x + wall * d - TILE * d)
    }

    /// Assisted shells (§4.4), before the actors move: while the shell is grounded and the
    /// player in the air, a shell that bounced off the wall brakes to a stop at 0.5 px/frame²
    /// (and then waits, still); the assist ends — the shell back at 2.5 px/frame — when the
    /// player lands, the shell is in the air, or it slid back past its stop point.
    pub(super) fn step_shell_assist(&mut self) {
        let (px, p_ground) = (self.player.x, self.player.on_ground);
        for s in &mut self.actors {
            let Some((stop, kick_dir)) = s.assist else {
                continue;
            };
            if s.mode != Mode::ShellSlide || !matches!(s.kind, Kind::Shell { .. }) {
                s.assist = None;
                continue;
            }
            let toward_player = (px - s.x).signum() as i32 == s.dir;
            let passed = ((s.x - stop) * kick_dir as f32) < 0.0;
            if p_ground || !s.on_ground || (toward_player && passed && s.dir != kick_dir) {
                s.assist = None;
                s.vx = KICK_SPEED * s.dir as f32;
                continue;
            }
            if s.dir != kick_dir {
                let v = s.vx.abs() - ASSIST_BRAKE;
                s.vx = v.max(0.0) * s.dir as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::{self, Input};
    use crate::level::{Cell, Level, PipeDir, Spawn};
    use crate::world::World;

    fn pipe_layers(w: &World) -> Vec<u32> {
        w.sprites()
            .iter()
            .filter(|s| s.image.rect.is_some() && s.layer != 12 && s.layer != 20)
            .filter(|s| {
                w.level
                    .tiles
                    .iter()
                    .any(|t| t.pipe && (t.x as f32 * 16.0, t.y as f32 * 16.0) == (s.x, s.y))
            })
            .map(|s| s.layer)
            .collect()
    }

    #[test]
    fn pipes_drawn_over_the_player_only_with_the_option() {
        let mut lvl = Level::flat(60);
        lvl.add_pipe(6, 2, 3, PipeDir::Up);
        let mut w = World::with_level(lvl);
        let player = w.player_sprite().unwrap().layer;
        let layers = pipe_layers(&w);
        assert_eq!(layers.len(), 6, "2 × 3 pipe tiles in view");
        assert!(
            layers.iter().all(|&l| l < player),
            "SMM2: the player is in front"
        );
        let mut o = w.options;
        o.pipes_over_player = true;
        w.set_options(o);
        assert!(
            pipe_layers(&w).iter().all(|&l| l > player),
            "SMM1: pipes in front"
        );
        // ground tiles keep their layer
        assert!(
            w.sprites()
                .iter()
                .any(|s| s.image.rect.is_some() && s.layer == 10)
        );
    }

    fn shell_spawn(x: f32, y: f32) -> Spawn {
        Spawn {
            kind: crate::actors::Kind::Shell { red: false },
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

    /// A world with a still shell on the ground at x = 120 and the player standing at x = 40.
    fn shell_world(smm1: bool) -> World {
        let mut lvl = Level::flat(100);
        lvl.spawns.push(shell_spawn(120.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut o = w.options;
        o.smm1_shell_kicks = smm1;
        w.set_options(o);
        w.step(Input::default());
        for a in &mut w.actors {
            a.mode = crate::actors::Mode::ShellWait;
            a.timer = 1000; // no Koopa inside wakes up during the test
        }
        w
    }

    fn shell(w: &World) -> &crate::actors::Actor {
        w.actors
            .iter()
            .find(|a| matches!(a.kind, crate::actors::Kind::Shell { .. }))
            .unwrap()
    }

    /// Drop the player onto the still shell from straight above: SMM2 (as the engine has it)
    /// kicks and bounces; SMM1 kicks without a bounce (0x0234c294).
    #[test]
    fn landing_on_a_still_shell() {
        for smm1 in [false, true] {
            let mut w = shell_world(smm1);
            let sx = shell(&w).x;
            w.player.x = sx - 2.0;
            w.player.y = 32.0 + 16.0 + 6.0;
            w.player.vy = -1.0;
            w.player.on_ground = false;
            let mut kicked = None;
            for _ in 0..4 {
                w.step(Input::default());
                if shell(&w).mode == crate::actors::Mode::ShellSlide && kicked.is_none() {
                    kicked = Some(w.player.vy);
                }
            }
            let vy = kicked.expect("kicked");
            if smm1 {
                assert!(vy < 1.0, "SMM1: no bounce ({vy})");
            } else {
                assert!(vy > 3.0, "SMM2 engine: bounce ({vy})");
            }
        }
    }

    /// The mid-air kick: a side touch while airborne kicks the shell in both modes, and the
    /// kicker's window is 16 frames in the air.
    #[test]
    fn mid_air_side_kick() {
        for smm1 in [false, true] {
            let mut w = shell_world(smm1);
            let sx = shell(&w).x;
            w.player.x = sx - 11.0;
            w.player.y = 34.0;
            w.player.vy = 1.0;
            w.player.on_ground = false;
            w.step(Input {
                buttons: input::RIGHT,
            });
            let s = shell(&w);
            assert_eq!(s.mode, crate::actors::Mode::ShellSlide, "smm1 = {smm1}");
            assert!(s.vx > 0.0);
            assert!(s.kick_lock >= 14, "airborne window ({})", s.kick_lock);
        }
    }

    /// SMM1's falling stomp window is a flat 10 px: a slow fall 9 px deep into a Goomba stomps
    /// it, where SMM2's 7 + |vy| does not.
    #[test]
    fn deep_slow_landing_is_a_stomp_only_in_smm1() {
        for smm1 in [false, true] {
            let mut lvl = Level::flat(100);
            let mut g = shell_spawn(120.0, 32.0);
            g.kind = crate::actors::Kind::Goomba;
            lvl.spawns.push(g);
            let mut w = World::with_level(lvl);
            let mut o = w.options;
            o.smm1_shell_kicks = smm1;
            w.set_options(o);
            w.step(Input::default());
            let gx = w.actors[0].x;
            w.player.x = gx;
            w.player.y = 32.0 + 16.0 - 8.5;
            w.player.vy = -0.5;
            w.player.on_ground = false;
            w.player_interactions(false);
            let stomped = w.actors[0].mode != crate::actors::Mode::Normal;
            assert_eq!(stomped, smm1, "smm1 = {smm1}");
        }
    }

    /// The shell jump: falling past a pit edge, a mid-air kick of a still shell toward a wall
    /// gives the 3.51855 hop and parks the shell short of the wall (SMM1 option only).
    #[test]
    fn wall_assist_shell_jump() {
        for smm1 in [false, true] {
            // floor up to x = 80 (tiles 0..5), a pit, and a wall at tiles 10..
            let mut lvl = Level::flat(30);
            for x in 5..10 {
                for y in 0..2 {
                    lvl.set(x, y, Cell::Empty);
                }
            }
            for y in 0..8 {
                lvl.set(10, y, Cell::Solid);
            }
            // a still shell floating over the pit at x = 146 (its wall 14 px ahead)
            lvl.spawns.push(shell_spawn(146.0, 40.0));
            let mut w = World::with_level(lvl);
            let mut o = w.options;
            o.smm1_shell_kicks = smm1;
            w.set_options(o);
            w.step(Input::default());
            let i = w
                .actors
                .iter()
                .position(|a| matches!(a.kind, crate::actors::Kind::Shell { .. }))
                .unwrap();
            w.actors[i].x = 146.0;
            w.actors[i].y = 40.0;
            w.actors[i].vy = 0.0;
            w.actors[i].on_ground = false;
            w.actors[i].mode = crate::actors::Mode::ShellWait;
            w.player.x = 146.0 - 11.0;
            w.player.y = 41.0;
            w.player.vy = -0.5;
            w.player.on_ground = false;
            w.player_interactions(false);
            let s = &w.actors[i];
            assert_eq!(s.mode, crate::actors::Mode::ShellSlide, "smm1 = {smm1}");
            if smm1 {
                assert!(
                    (w.player.vy - super::ASSIST_HOP).abs() < 1e-4,
                    "hop {}",
                    w.player.vy
                );
                assert_eq!(s.assist, Some((160.0 - 16.0, 1)));
                assert_eq!(s.kick_lock, 5, "the contact window");
            } else {
                assert!(w.player.vy < 0.0, "no hop in the SMM2 engine");
                assert!(s.assist.is_none());
            }
        }
    }

    /// After the assist the shell bounces off the wall and, while the player is still in the
    /// air, brakes to a stop about a tile short of the wall: something to land on.
    #[test]
    fn assisted_shell_parks_short_of_the_wall() {
        let mut lvl = Level::flat(40);
        for y in 2..8 {
            lvl.set(12, y, Cell::Solid);
        }
        lvl.spawns.push(shell_spawn(178.0, 32.0));
        let mut w = World::with_level(lvl);
        let mut o = w.options;
        o.smm1_shell_kicks = true;
        w.set_options(o);
        w.step(Input::default());
        let i = w
            .actors
            .iter()
            .position(|a| matches!(a.kind, crate::actors::Kind::Shell { .. }))
            .unwrap();
        // a shell kicked toward the wall with the assist on, the player high above
        let wall_x = 192.0;
        let s = &mut w.actors[i];
        s.mode = crate::actors::Mode::ShellSlide;
        s.on_ground = true;
        s.vx = 2.5;
        s.dir = 1;
        s.assist = Some((wall_x - 16.0, 1));
        for _ in 0..40 {
            w.player.x = 100.0;
            w.player.y = 150.0;
            w.player.vy = 0.0;
            w.player.on_ground = false;
            w.step(Input::default());
        }
        let s = &w.actors[i];
        assert_eq!(s.mode, crate::actors::Mode::ShellWait, "parked");
        assert!(s.x < wall_x - 8.0 && s.x > wall_x - 32.0, "x {}", s.x);
    }
}
