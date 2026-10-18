//! SMM1 extras (Phase 11, `crate::options::Options`): rules from the original Super Mario Maker
//! that hosts can switch on over SMM2 play. Behaviour specs: research/specs/smm1-extras.md.

use super::{Event, World};
use crate::actors::{Kind, Mode};
use crate::level::{Cell, TILE};
use crate::player::{Form, STOMP_BOUNCE};

// --- SMM1 draw order (Options::smm1_layering; misc spec part A)
//
// SMM1 gives every object a depth by category (EditDB `ZposPriorityType`, table 0x02247f48),
// back to front: Bg (semisolids) < coins < blocks and ground < pipes < tracks < lifts < enemies
// and items < the player (13900, 0x026a4bb4) < wings. The player's depth changes with what he
// does (0x026a4b9c): 4300 while entering or leaving a pipe — in front of blocks, behind every
// pipe —, 6080 on a vine (behind lifts and enemies), −3347 on the goal pole (behind blocks),
// 14450 while dying (in front of everything). Pipe contents sit just behind their own pipe.
// The engine's layers (SMM2 order) map onto that ladder as below.

/// Pipe tiles (SMM2 order: with the ground at 10).
pub const PIPE_LAYER: u32 = 14;
/// Things inside a pipe: the player passing through, a Piranha Plant at home, items and enemies
/// coming out (SMM2 order: 5, behind all terrain).
const IN_PIPE_LAYER: u32 = 13;
/// Coins (SMM2 order: 20, in front of blocks): behind ground, blocks and pipes (Block 2301–3300,
/// Coin 1300–2300), in front of the background-type tiles (Bg −6000…, Bridge −5579…, vine
/// −5690, conveyor 1174) [obs: the EditDB `ZposPriorityType` ranges, 0x02247f48].
const COIN_LAYER: u32 = 11;
/// Ground and other terrain tiles in the SMM1 order: with the blocks, in front of coins.
const GROUND_LAYER: u32 = 12;

/// The layer of a terrain tile (SMM2 order: 10 for all of them).
pub fn tile_layer(t: &crate::level::TileDraw, smm1: bool) -> u32 {
    match (smm1, t.pipe, t.back) {
        (false, ..) => 10,
        (true, true, _) => PIPE_LAYER,
        (true, false, true) => 10,
        (true, false, false) => GROUND_LAYER,
    }
}
/// Tracks (SMM2 order: 9, behind terrain): in front of pipes, behind actors.
const TRACK_LAYER: u32 = 15;
/// The player on a vine: behind enemies and lifts (actors draw at 40).
const VINE_PLAYER_LAYER: u32 = 39;
/// The player on the goal pole: behind blocks.
const POLE_PLAYER_LAYER: u32 = 11;
/// The player dying: in front of everything.
const DYING_PLAYER_LAYER: u32 = 90;

/// An engine layer in the SMM1 order.
pub fn smm1_layer(layer: u32) -> u32 {
    match layer {
        5 => IN_PIPE_LAYER,
        9 => TRACK_LAYER,
        20 => COIN_LAYER,
        l => l,
    }
}

/// The player's layer in the SMM1 order (outside pipes).
pub fn player_layer(p: &crate::player::Player) -> u32 {
    if !p.alive() {
        DYING_PLAYER_LAYER
    } else if p.climbing.is_some() {
        VINE_PLAYER_LAYER
    } else if p.pose == crate::player::Pose::Pole {
        POLE_PLAYER_LAYER
    } else {
        50
    }
}

// --- costumes (Mystery Mushroom)

/// Model name of costume `id` in the imported SMM1 sprite cache (`SMM1_Costume_<id>`, three
/// digits; mmk_import::smm1_costumes). The engine knows costumes only by number.
pub fn costume_model(id: u16) -> &'static str {
    static NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        (0..crate::player::COSTUMES)
            .map(|i| &*Box::leak(format!("SMM1_Costume_{i:03}").into_boxed_str()))
            .collect()
    });
    names[(id as usize).min(names.len() - 1)]
}

/// A costumed player's sprite: one 24 × 24 frame per pose (`wait.0`, `walk.0-2`, `b_dash.0-2`,
/// `jump.0`, `jump_fall.0`, `turn.0`, `stoop.0`, `climb.0-1`, `pole.0-1`, `swim.0-5`,
/// `shoes_wait.0`, `appeal.0`), standing on the player's origin. The walk and dash cycles are
/// 15 frames (a texture per 5) in the costume rig's animations [obs]; a costume has no death
/// frame (the wait pose) [obs]; Sonic (id 78) spins through `jump.0-2` every 4 frames for the
/// whole jump [obs].
pub fn costume_sprite(p: &crate::player::Player, layer: u32, tick: u64) -> crate::render::Sprite {
    use crate::player::{Pose, WALK_MAX};
    let k3 = |phase: f32, step: f32| ((phase / step) as usize) % 3;
    let airborne = matches!(p.pose, Pose::Jump | Pose::Spin | Pose::Fire);
    let frame = match p.pose {
        _ if p.appeal > 0 => "appeal.0",
        _ if airborne && p.costume == SONIC => {
            ["jump.0", "jump.1", "jump.2"][(tick as usize / 4) % 3]
        }
        Pose::Wait if p.shoe.is_some() => "shoes_wait.0",
        Pose::Wait => "wait.0",
        Pose::Walk if p.vx.abs() > WALK_MAX + 0.01 => {
            ["b_dash.0", "b_dash.1", "b_dash.2"][k3(p.walk_phase, 6.0)]
        }
        Pose::Walk => ["walk.0", "walk.1", "walk.2"][k3(p.walk_phase, 6.0)],
        Pose::Skid => "turn.0",
        // the costume rig maps the falling state "jump2" to `jump_fall` (M1_PlayerAnmTable)
        // [obs]; [inf: that jump2 starts when the rise ends (vy < 0) — the SMM1 player's switch
        // to jump2 (0x026c1470) was traced only for the star-jump and spin cases]
        Pose::Jump | Pose::Spin | Pose::Fire if p.vy < 0.0 => "jump_fall.0",
        Pose::Jump | Pose::Spin | Pose::Fire => "jump.0",
        Pose::Dead => "wait.0",
        Pose::Crouch | Pose::Slide => "stoop.0",
        Pose::Pole => "pole.0",
        Pose::Climb => {
            if ((p.walk_phase / 8.0) as i32) % 2 == 0 {
                "climb.0"
            } else {
                "climb.1"
            }
        }
        Pose::Swim => ["swim.0", "swim.1", "swim.2", "swim.3", "swim.4", "swim.5"]
            [((p.walk_phase / 4.0) as usize).min(5)],
    };
    crate::render::Sprite {
        image: crate::render::SpriteImage {
            model: costume_model(p.costume),
            texture: frame,
            rect: None,
        },
        x: p.x - 12.0,
        y: p.y,
        w: 24.0,
        h: 24.0,
        flip_x: p.facing < 0,
        flip_y: false,
        layer,
    }
}

/// Sonic's costume id: the one costume whose jump is a spin animation (SMM1 `sonic_jump`).
const SONIC: u16 = 78;

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

/// The costume when nothing else can be drawn (SMM1 0x021f22e4 falls back to Link, id 45) [obs].
const FALLBACK_COSTUME: u16 = 45;

impl World {
    /// The player takes a Mystery Mushroom (costume spec §2): the costume set in the options,
    /// else one drawn uniformly from the costume pool without the one already worn (SMM1 draws
    /// from the owned costumes; SMM2 courses store none). Taking a costume knocks a worn shell
    /// helmet off.
    pub(super) fn take_mystery_mushroom(&mut self) {
        let id = match self.options.costume {
            Some(id) => id,
            None => {
                let worn = (self.player.form == Form::Costume).then_some(self.player.costume);
                let pool: Vec<u16> = self
                    .options
                    .costume_ids()
                    .into_iter()
                    .filter(|&i| Some(i) != worn)
                    .collect();
                if pool.is_empty() {
                    FALLBACK_COSTUME
                } else {
                    let k = (self.rand() * pool.len() as f32) as usize;
                    pool[k.min(pool.len() - 1)]
                }
            }
        };
        if self.player.put_on_costume(id) {
            self.player.helmet = None;
        }
    }

    /// The SMM1 forms' sound events of this frame: a costume's landing, skid and taunt, Lanky
    /// Mario's float cue (only these forms raise them, so SMM2 play is unchanged).
    pub(super) fn smm1_player_events(&mut self) {
        use crate::player::{CUE_APPEAL, CUE_LAND, CUE_SKID};
        let p = &self.player;
        if p.form == Form::Costume {
            for (cue, ev) in [
                (CUE_LAND, Event::Land),
                (CUE_SKID, Event::Skid),
                (CUE_APPEAL, Event::Taunt),
            ] {
                if p.cues & cue != 0 {
                    self.events.push(ev);
                }
            }
        }
        if self.player.float_cue {
            self.events.push(Event::LankyFloat);
        }
    }

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
        let (pvx, p_ground) = (p.vx, p.on_ground);
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
        // the no-hit window (0x0225c120) [obs]: the water tests use the shell's own water flag
        let window = {
            let s = &self.actors[i];
            let shell_in_water = self
                .liquid
                .is_some_and(|l| l.water() && l.covers(s.y + 8.0))
                || (self.level.theme == 2 && self.level.night);
            // a shell in the water squeezed between two walls, the kicker airborne above it:
            // probes from 8 px up the shell, 12 px to each side, both hit a wall → 4 flat
            let probe = |dx: f32| self.level.cell_at(s.x + dx, s.y + 8.0).is_solid();
            if shell_in_water && !p_ground && self.player.y > s.y && probe(-12.0) && probe(12.0) {
                4
            } else {
                let mut w = 16;
                if p_ground && s.on_ground {
                    w = if (s.x - self.player.x).abs() < 8.0 {
                        8
                    } else {
                        4
                    };
                }
                if rekick {
                    w += 4;
                }
                if shell_in_water {
                    w += 8;
                }
                w
            }
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
        // scored on the stomp chain, as the SMM2 kick (0x7100644610)
        let n = self.player.stomp_chain;
        self.player.stomp_chain += 1;
        self.add_score(Self::chain_score(n), ax, ay + 24.0);
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
    fn smm1_coins_go_between_background_tiles_and_ground() {
        use crate::level::TileDraw;
        let tile = |back| TileDraw {
            x: 0,
            y: 0,
            atlas: 0,
            flip: 0,
            pipe: false,
            back,
        };
        let (semi, ground) = (tile(true), tile(false));
        assert_eq!(
            super::tile_layer(&semi, false),
            super::tile_layer(&ground, false)
        );
        let coin = super::smm1_layer(20);
        assert!(super::tile_layer(&semi, true) < coin);
        assert!(coin < super::tile_layer(&ground, true));
        assert!(super::tile_layer(&ground, true) < super::PIPE_LAYER);
    }

    #[test]
    fn smm1_draw_order() {
        let mut lvl = Level::flat(60);
        lvl.add_pipe(6, 2, 3, PipeDir::Up);
        lvl.coins.push(crate::level::CoinTile { x: 9, y: 4 });
        let mut w = World::with_level(lvl);
        let player = w.player_sprite().unwrap().layer;
        let coin = |w: &World| {
            w.sprites()
                .iter()
                .find(|s| (s.x, s.y) == (9.0 * 16.0, 4.0 * 16.0))
                .unwrap()
                .layer
        };
        let layers = pipe_layers(&w);
        assert_eq!(layers.len(), 6, "2 × 3 pipe tiles in view");
        assert!(layers.iter().all(|&l| l < player));
        assert!(coin(&w) > 12, "SMM2: coins in front of blocks");
        let mut o = w.options;
        o.smm1_layering = true;
        w.set_options(o);
        // normal play: the player stays in front of pipes (13900 vs 4400…)
        let player = w.player_sprite().unwrap().layer;
        let pipes = pipe_layers(&w);
        assert!(
            pipes.iter().all(|&l| l < player && l > 12),
            "pipes above blocks"
        );
        assert!(coin(&w) < 12, "SMM1: coins behind blocks");
        // passing through a pipe: in front of blocks (12), behind the pipe
        let in_pipe = super::smm1_layer(5);
        assert!(in_pipe > 12 && in_pipe < super::PIPE_LAYER);
        assert!(
            super::smm1_layer(9) > super::PIPE_LAYER,
            "tracks in front of pipes"
        );
        // on a vine: behind the actors (40)
        w.player.climbing = Some((100.0, 1));
        assert!(w.player_sprite().unwrap().layer < 40);
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

    // --- Lanky Mario and costumes

    use crate::blocks::{BlockKind, Contents};
    use crate::level::BlockPlacement;
    use crate::options::MushroomSwap;
    use crate::player::Form;
    use crate::world::Event;

    /// Peak height (px above the start) of a standing jump with jump held for 60 frames.
    fn jump_peak(form: Form) -> f32 {
        let mut w = World::with_level(Level::flat(40));
        w.player.form = form;
        for _ in 0..5 {
            w.step(Input::default());
        }
        let y0 = w.player.y;
        let mut peak = y0;
        for _ in 0..60 {
            w.step(Input {
                buttons: input::JUMP,
            });
            peak = peak.max(w.player.y);
        }
        peak - y0
    }

    /// Lanky Mario's launch is 0.5 px/f higher (4.128 standing): about 101 px against Super
    /// Mario's 68 (lanky spec §3.3); gravity and the fall cap are unchanged.
    #[test]
    fn lanky_jump_is_higher_and_floatier() {
        let normal = jump_peak(Form::Super);
        let lanky = jump_peak(Form::Lanky);
        assert!((normal - 67.6).abs() < 2.0, "Super {normal}");
        assert!((lanky - 100.9).abs() < 2.0, "Lanky {lanky}");
    }

    /// SMM1's pickup and damage rules for the Weird Mushroom (0x026b34a4, 0x026b04e8).
    #[test]
    fn lanky_pickups_and_damage() {
        let mut p = crate::player::Player::new(0.0, 32.0);
        assert!(p.power_up(Form::Lanky), "small → Lanky");
        p.control = crate::player::Control::Normal;
        assert!(!p.power_up(Form::Super), "a Super Mushroom is refused");
        assert!(p.big() && p.sensors().head_y == 27.0, "Super Mario's body");
        assert!(p.hurt());
        assert_eq!(p.form, Form::Small, "Lanky → small");
        p.control = crate::player::Control::Normal;
        p.invuln = 0;
        p.form = Form::Fire;
        assert!(!p.power_up(Form::Lanky), "Fire keeps Fire");
        p.form = Form::Super;
        assert!(p.power_up(Form::Lanky), "Super → Lanky");
    }

    /// A course's Super Mushroom from a ? block, with the swap option.
    fn mushroom_world(swap: MushroomSwap, style: &str) -> World {
        let mut lvl = Level::flat(100);
        lvl.style = style.into();
        lvl.set(3, 5, Cell::Solid);
        lvl.blocks.push(BlockPlacement {
            x: 3,
            y: 5,
            kind: BlockKind::Question,
            contents: Some(Contents::Mushroom),
        });
        let mut w = World::with_level(lvl);
        let mut o = w.options;
        o.smb1_mushroom = swap;
        o.costume = Some(45);
        w.set_options(o);
        let run = |w: &mut World, b: u32, n: usize| {
            for _ in 0..n {
                w.step(Input { buttons: b });
            }
        };
        run(&mut w, input::JUMP, 30);
        run(&mut w, 0, 100);
        run(&mut w, input::RIGHT, 240);
        w
    }

    #[test]
    fn weird_and_mystery_mushrooms_replace_super_mushrooms_in_smb1() {
        assert_eq!(
            mushroom_world(MushroomSwap::Off, "M1").player.form,
            Form::Super
        );
        assert_eq!(
            mushroom_world(MushroomSwap::Weird, "M1").player.form,
            Form::Lanky
        );
        let w = mushroom_world(MushroomSwap::Mystery, "M1");
        assert_eq!((w.player.form, w.player.costume), (Form::Costume, 45));
        // SMB1 only
        assert_eq!(
            mushroom_world(MushroomSwap::Weird, "M3").player.form,
            Form::Super
        );
    }

    /// A costume is small Mario's body and physics: the same jump, one hit back to small, the
    /// costume art at 24 × 24.
    #[test]
    fn costume_form() {
        let small = jump_peak(Form::Small);
        let costume = jump_peak(Form::Costume);
        assert_eq!(small, costume);
        let mut w = World::with_level(Level::flat(40));
        w.player.form = Form::Costume;
        w.player.costume = 7;
        assert!(!w.player.big());
        let s = w.player_sprite().unwrap();
        assert_eq!(s.image.model, "SMM1_Costume_007");
        assert_eq!((s.w, s.h), (24.0, 24.0));
        assert!(w.player.hurt());
        assert_eq!(w.player.form, Form::Small);
    }

    /// Random costumes come from the pool, never the one already worn.
    #[test]
    fn random_costume_from_the_pool() {
        let mut w = World::with_level(Level::flat(40));
        let mut o = w.options;
        o.set_costume_pool([3, 9]);
        w.set_options(o);
        for _ in 0..10 {
            w.player.control = crate::player::Control::Normal;
            let before = (w.player.form == Form::Costume).then_some(w.player.costume);
            w.take_mystery_mushroom();
            assert!([3, 9].contains(&w.player.costume));
            assert_ne!(Some(w.player.costume), before);
        }
    }

    /// Holding Up standing still starts the costume taunt (120 frames) with its sound event.
    #[test]
    fn costume_taunt() {
        let mut w = World::with_level(Level::flat(40));
        w.player.form = Form::Costume;
        let mut taunts = 0;
        for _ in 0..20 {
            w.step(Input { buttons: input::UP });
            taunts += w.events.iter().filter(|e| **e == Event::Taunt).count();
        }
        assert_eq!(taunts, 1);
        assert!(w.player.appeal > 100);
        assert_eq!(w.player_sprite().unwrap().image.texture, "appeal.0");
    }

    /// The lanky float cue sounds once on a held jump, never on a tap.
    #[test]
    fn lanky_float_cue() {
        for (held, want) in [(60, 1), (2, 0)] {
            let mut w = World::with_level(Level::flat(40));
            w.player.form = Form::Lanky;
            w.step(Input::default());
            let mut cues = 0;
            for t in 0..80 {
                let b = if t < held { input::JUMP } else { 0 };
                w.step(Input { buttons: b });
                cues += w.events.iter().filter(|e| **e == Event::LankyFloat).count();
            }
            assert_eq!(cues, want, "held {held}");
        }
    }
}
