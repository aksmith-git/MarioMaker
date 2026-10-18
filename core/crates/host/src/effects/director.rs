//! World → effects: the sim's effect cues (`World::fx`) and the player's actions, played
//! through the course objects' effect links (the game's asset-link data, `library`).
//!
//! The links are keyed by user (an object's name per game style, "M1_Kuribo", or shared,
//! "Kuribo") and event key; containers choose by the game style, theme, the object's size, the
//! liquid, the player's form. The player's looping effects come from the link's action
//! triggers: while an animation or state named there is active (the full-speed run, the skid,
//! …) its keys play, stopped when it ends.

use std::collections::HashMap;
use std::sync::Arc;

use mmk_sim::actors::{Kind, Mode};
use mmk_sim::fx::{Fx, FxCue, FxSource, Liquid};
use mmk_sim::player::{Form, Pose, RUN_MAX};
use mmk_sim::world::{VIEW_H, VIEW_W};
use mmk_sim::{Event, World};

use super::library::{Asset, EffectLibrary};
use super::system::{EffectPlayer, FxHandle, FxQuad};

/// Course themes by id, as the links' theme conditions spell them.
const THEMES: [&str; 10] = [
    "plain",
    "underground",
    "castle",
    "airship",
    "water",
    "hauntedhouse",
    "snow",
    "desert",
    "athletic",
    "woods",
];

/// The link user (without the style prefix) of an actor kind; None: no effects linked.
pub fn actor_user(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Goomba => "Kuribo",
        Kind::Goombrat => "Kakibo",
        Kind::Koopa { red: false } | Kind::Shell { red: false } | Kind::EmptyShell { .. } => {
            "Nokonoko"
        }
        Kind::Koopa { red: true } | Kind::Shell { red: true } => "NokonokoRed",
        Kind::Mushroom => "SuperKinoko",
        Kind::FireFlower => "FireFlower",
        Kind::Star => "SuperStar",
        Kind::OneUp => "1upKinoko",
        Kind::Fireball => "PlayerFire",
        Kind::Piranha { fire: false } => "Pakkun",
        Kind::Piranha { fire: true } => "FirePakkun",
        Kind::BigCoin { value: 0 } => "Coin10",
        Kind::BigCoin { value: 1 } => "Coin30",
        Kind::BigCoin { .. } => "Coin50",
        Kind::EnemyFire => "EnmyFire",
        Kind::Thwomp { .. } => "Dossun",
        Kind::Lift { .. } => "Lift",
        Kind::Spring | Kind::Trampoline { .. } => "JumpStep",
        Kind::SideSpring => "JumpStepSide",
        Kind::BulletBill => "Killer",
        Kind::PSwitch => "PSwitch",
        Kind::Muncher => "BlackPakkun",
        Kind::Donut => "ChikuwaBlock",
        Kind::Spiny | Kind::SpinyEgg => "Togezo",
        Kind::Buzzy => "Met",
        Kind::SpikeTop { .. } => "Togemet",
        Kind::HammerBro => "Bros",
        Kind::Hammer => "Hammer",
        Kind::BobOmb => "Bombhei",
        Kind::DryBones => "Karon",
        Kind::Boo | Kind::RingBoo { .. } => "Teresa",
        Kind::BabyBlooper { .. } => "GessoChild",
        Kind::BanzaiBill { .. } => "MagnumKiller",
        Kind::Bumper => "Marumaru",
        Kind::Grinder => "Garigari",
        Kind::Cannonball => "BulletSenkanBall",
        Kind::Lakitu => "Jugem",
        Kind::Cloud => "JugemCloud",
        Kind::Pow => "PowBlock",
        Kind::Twister => "Tornado",
        Kind::CheepCheep { .. } => "Pukupuku",
        Kind::Blooper => "Gesso",
        Kind::Magikoopa => "Kameck",
        Kind::Magic => "KameckMagic",
        Kind::RockyWrench => "Poo",
        Kind::Wrench => "BulletSpanner",
        Kind::Wiggler => "Hanachan",
        Kind::ShoeGoomba { .. } => "KutsuKuribo",
        Kind::ClownCar { .. } => "KoopaClown",
        Kind::Stone => "Stone",
        Kind::YoshiEgg { .. } => "YoshiEgg",
        Kind::Yoshi { .. } => "Yoshi",
        Kind::FishBone => "FishBone",
        Kind::LavaBubble => "Bubble",
        Kind::MontyMole => "Choropoo",
        Kind::AngrySun { .. } => "Sun",
        Kind::ChainChomp { .. } => "Wanwan",
        Kind::LavaLift { .. } => "YouganLift",
        Kind::Bowser { .. } => "Koopa",
        Kind::BowserFire => "BulletKoopaFire",
        Kind::BoomBoom { .. } => "Bunbun",
        Kind::BowserJr { .. } => "KoopaJr",
        Kind::Superball => "PlayerSuperBall",
        Kind::Spike => "Gabon",
        Kind::SpikeBall { .. } => "GabonBall",
        Kind::Pokey { .. } => "Sanbo",
        Kind::Mechakoopa { .. } => "Mechakoopa",
        Kind::MechaMissile => "Missile",
        Kind::MechaLaser => "LaserBeam",
        _ => return None,
    })
}

/// The link key a cue plays (on its own user), the user it belongs to when not the source's.
fn cue_keys(cue: FxCue) -> &'static [(&'static str, Option<&'static str>)] {
    match cue {
        FxCue::Stomped => &[("r_damage", None), ("DieStamp", None)],
        FxCue::Defeated => &[("r_damage", None)],
        FxCue::Vanish => &[("DieVanish", None)],
        FxCue::Kicked => &[("p_kick", None)],
        FxCue::Bounced => &[("r_hit_metcap", None)],
        FxCue::FireballHit => &[("r_hit", Some("PlayerFire"))],
        FxCue::SplashIn(_) => &[("r_splash_in", None)],
        FxCue::SplashOut(_) => &[("r_splash_out", None)],
        FxCue::CoinGet => &[("p_Coin_Get", Some("ObjectCommon"))],
        FxCue::PowerUp => &[("r_power_up", None)],
        FxCue::PipeIn => &[("p_DrawDokan_In", Some("ObjectCommon"))],
        FxCue::PipeOut => &[("p_DrawDokan_Out", Some("ObjectCommon"))],
        FxCue::SwitchOn => &[("On", None)],
        FxCue::Checkpoint => &[("r_MiddleFlag_On", Some("ObjectCommon"))],
        FxCue::Explosion => &[("p_Bombhei_Explosion", Some("Game_OneEmitterMgr"))],
    }
}

fn player_mode(form: Form) -> &'static str {
    match form {
        Form::Mega => "Mega",
        Form::Cat => "Neko",
        Form::Raccoon => "Sippo",
        Form::Cape => "Mant",
        _ => "Normal",
    }
}

/// What an actor was doing last tick (to see it get stomped, knocked out, kicked, vanish).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Alive,
    Stomped,
    Falling,
    ShellWait,
    ShellSlide,
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    kind: Kind,
    state: State,
    x: f32,
    y: f32,
}

fn state_of(mode: &Mode) -> State {
    match mode {
        Mode::Stomped => State::Stomped,
        Mode::DieFall | Mode::Dead => State::Falling,
        Mode::ShellWait => State::ShellWait,
        Mode::ShellSlide => State::ShellSlide,
        _ => State::Alive,
    }
}

/// Plays the effects of a running course. Step it once per world tick.
#[derive(Debug, Default)]
pub struct EffectDirector {
    pub lib: Arc<EffectLibrary>,
    pub player: EffectPlayer,
    /// The player's looping effects by action name.
    held: HashMap<&'static str, Vec<FxHandle>>,
    last_tick: u64,
    pick: u32,
    /// Actors by id as of the last tick, the coin count and the player in water.
    seen: HashMap<u32, Seen>,
    coins: u32,
    coins_known: bool,
    in_water: bool,
}

impl EffectDirector {
    pub fn new(lib: Arc<EffectLibrary>) -> Self {
        Self {
            lib,
            ..Default::default()
        }
    }

    fn next_pick(&mut self) -> f32 {
        self.pick = self.pick.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        (self.pick >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Play the assets of `key` on user `base` at (x, y); returns their handles.
    #[allow(clippy::too_many_arguments)]
    fn play_key(
        &mut self,
        world: &World,
        base: &str,
        key: &str,
        x: f32,
        y: f32,
        flip: bool,
        big: bool,
        liquid: Option<Liquid>,
        held: bool,
    ) -> Vec<FxHandle> {
        let style = world.level.style.clone();
        let theme = THEMES
            .get(world.level.theme as usize)
            .copied()
            .unwrap_or("plain");
        let night = world.level.night;
        let mode = player_mode(world.player.form);
        let props = move |p: &str| -> Option<String> {
            Some(
                match p {
                    "style" => style.as_str(),
                    "theme" => theme,
                    "time" => {
                        if night {
                            "cNight"
                        } else {
                            "cDay"
                        }
                    }
                    "size" => {
                        if big {
                            "Big"
                        } else {
                            "Normal"
                        }
                    }
                    "liquid" => match liquid? {
                        Liquid::Water => "Water",
                        Liquid::Lava => "Lava",
                        Liquid::Poison => "Poison",
                    },
                    "reaction" => "Die",
                    "player_mode" => mode,
                    "player" => "Mario",
                    _ => return None,
                }
                .to_string(),
            )
        };
        let pick = self.next_pick();
        let assets: Vec<Asset> = self
            .lib
            .resolve(&world.level.style, base, key, &props, pick);
        let mut out = Vec::new();
        for a in assets {
            let Some(set) = self.lib.sets.get(&a.set).cloned() else {
                continue;
            };
            out.push(self.player.play(set, &a, x, y, flip, held));
        }
        out
    }

    fn play_cue(&mut self, world: &World, fx: &Fx) {
        let source = match fx.source {
            FxSource::Actor(k) => actor_user(k),
            FxSource::Player => Some("PlayerMario"),
            FxSource::Course => Some("ObjectCommon"),
        };
        let liquid = match fx.cue {
            FxCue::SplashIn(l) | FxCue::SplashOut(l) => Some(l),
            _ => None,
        };
        for &(key, user) in cue_keys(fx.cue) {
            let Some(base) = user.or(source) else {
                continue;
            };
            // the player's style users are "<style>_Player"
            let base = if base == "PlayerMario" && fx.source == FxSource::Player {
                "Player"
            } else {
                base
            };
            self.play_key(world, base, key, fx.x, fx.y, fx.flip, fx.big, liquid, false);
        }
    }

    /// Cues the sim doesn't push but its state shows: actors stomped flat, knocked out, turned
    /// into or kicked as shells, vanishing in view (fireballs bursting, Bob-ombs exploding,
    /// enemies gone in a puff); coins taken; the player entering or leaving water; this tick's
    /// power-up, checkpoint and P Switch events.
    fn observe(&mut self, world: &World, out: &mut Vec<Fx>) {
        let p = &world.player;
        let (vx0, vy0) = (world.camera_x, world.camera_y);
        let in_view =
            |x: f32, y: f32| x >= vx0 && x <= vx0 + VIEW_W && y >= vy0 - 16.0 && y <= vy0 + VIEW_H;
        let mut now = HashMap::with_capacity(world.actors.len());
        for a in &world.actors {
            let s = Seen {
                kind: a.kind,
                state: state_of(&a.mode),
                x: a.x,
                y: a.y,
            };
            if let Some(old) = self.seen.get(&a.id) {
                let cue = match (old.state, s.state) {
                    (State::Alive, State::Stomped) => Some(FxCue::Stomped),
                    (State::Alive | State::ShellWait | State::ShellSlide, State::Falling) => {
                        Some(FxCue::Defeated)
                    }
                    (State::Alive, State::ShellWait) | (State::ShellSlide, State::ShellWait) => {
                        Some(FxCue::Stomped)
                    }
                    (State::ShellWait, State::ShellSlide) => Some(FxCue::Kicked),
                    _ => None,
                };
                if let Some(cue) = cue {
                    out.push(Fx::actor(old.kind, cue, a.x, a.y));
                }
            }
            now.insert(a.id, s);
        }
        for (id, old) in &self.seen {
            if now.contains_key(id) || !in_view(old.x, old.y) {
                continue;
            }
            let cue = match old.kind {
                Kind::Fireball => Some(FxCue::FireballHit),
                Kind::BobOmb => Some(FxCue::Explosion),
                k if k.is_enemy() && old.state == State::Alive => Some(FxCue::Vanish),
                _ => None,
            };
            if let Some(cue) = cue {
                out.push(Fx::actor(old.kind, cue, old.x, old.y));
            }
        }
        self.seen = now;
        if world.coins > self.coins && self.coins_known {
            out.push(Fx::course(FxCue::CoinGet, p.x, p.y + 8.0));
        }
        self.coins = world.coins;
        self.coins_known = true;
        if p.in_water != self.in_water {
            let cue = if p.in_water {
                FxCue::SplashIn(Liquid::Water)
            } else {
                FxCue::SplashOut(Liquid::Water)
            };
            out.push(Fx::player(cue, p.x, p.y));
            self.in_water = p.in_water;
        }
        for ev in &world.events {
            match ev {
                Event::PowerUp => out.push(Fx::player(FxCue::PowerUp, p.x, p.y)),
                Event::Checkpoint => out.push(Fx::course(FxCue::Checkpoint, p.x, p.y)),
                Event::Switch => {
                    let sw = world
                        .actors
                        .iter()
                        .filter(|a| a.kind == Kind::PSwitch)
                        .min_by(|a, b| {
                            let d =
                                |a: &mmk_sim::actors::Actor| (a.x - p.x).abs() + (a.y - p.y).abs();
                            d(a).total_cmp(&d(b))
                        });
                    if let Some(a) = sw {
                        out.push(Fx::actor(Kind::PSwitch, FxCue::SwitchOn, a.x, a.y));
                    }
                }
                _ => {}
            }
        }
    }

    /// The player's actions now active, by the names the links' action triggers use.
    fn player_actions(world: &World) -> Vec<&'static str> {
        let p = &world.player;
        let mut out = Vec::new();
        if !p.alive() || p.hidden {
            return out;
        }
        if p.on_ground && p.vx.abs() >= RUN_MAX - 0.01 && p.pose == Pose::Walk {
            out.push("b_dash2");
        }
        if p.pose == Pose::Skid {
            out.push("cState_Turn");
        }
        if p.wall_slide != 0 {
            out.push("cState_WallSlide");
        }
        if p.pound_landed {
            out.push("cState_HipAttackEnd");
        }
        if p.spinning && !p.on_ground {
            out.push("cState_SpinJump");
        }
        out
    }

    /// One world tick: play this tick's cues, start / stop / move the player's looping
    /// effects, step every effect.
    pub fn step(&mut self, world: &World) {
        if world.tick == self.last_tick {
            return;
        }
        if world.tick < self.last_tick {
            // a restart
            self.player.clear();
            self.held.clear();
            self.seen.clear();
            self.coins_known = false;
        }
        self.last_tick = world.tick;
        if self.lib.is_empty() {
            return;
        }
        let mut cues = world.fx.clone();
        self.observe(world, &mut cues);
        for fx in cues {
            self.play_cue(world, &fx);
        }
        let p = &world.player;
        let (px, py, flip) = (p.x, p.y, p.facing < 0);
        let now = Self::player_actions(world);
        let ended: Vec<&'static str> = self
            .held
            .keys()
            .copied()
            .filter(|a| !now.contains(a))
            .collect();
        for a in ended {
            for h in self.held.remove(a).unwrap_or_default() {
                self.player.stop(h);
            }
        }
        for a in now {
            if let Some(hs) = self.held.get(a) {
                for &h in hs {
                    self.player.move_to(h, px, py, flip);
                }
                continue;
            }
            let keys: Vec<String> = self
                .lib
                .user(&world.level.style, "Player")
                .and_then(|u| u.actions.get(a))
                .cloned()
                .unwrap_or_default();
            let mut hs = Vec::new();
            for k in keys {
                hs.extend(self.play_key(world, "Player", &k, px, py, flip, false, None, true));
            }
            self.held.insert(a, hs);
        }
        self.player.step();
    }

    /// The quads to draw this frame.
    pub fn quads(&mut self, out: &mut Vec<FxQuad>) {
        self.player.quads(out);
    }
}
