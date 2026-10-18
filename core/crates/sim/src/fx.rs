//! Effect cues: things that happened this tick that the game shows with a particle effect, and
//! where. The world pushes them (`World::fx`, cleared every tick like `World::events`); the host
//! turns each into the effect the game links to it for the object, cue and game style.

use crate::actors::Kind;

/// Who the effect belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxSource {
    Actor(Kind),
    Player,
    /// The course's shared objects (coins, blocks, pipes, the common smoke puffs).
    Course,
}

/// The liquid a splash is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liquid {
    Water,
    Lava,
    Poison,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxCue {
    /// Stomped flat (the stomp kill).
    Stomped,
    /// Knocked out (fire, shell, star, block from below, …).
    Defeated,
    /// Vanished in a puff of smoke (spin jump, Yoshi, lava, a cleared course's enemies).
    Vanish,
    /// Kicked or bumped (the hit spark).
    Kicked,
    /// A hit that does nothing (a spike top's shell, a stomp bounce).
    Bounced,
    /// The player's fireball hit something or burst on a wall.
    FireballHit,
    SplashIn(Liquid),
    SplashOut(Liquid),
    CoinGet,
    PowerUp,
    PipeIn,
    PipeOut,
    SwitchOn,
    Checkpoint,
    Explosion,
}

/// One cue: position in world pixels (x centre, y bottom of the source), facing left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fx {
    pub source: FxSource,
    pub cue: FxCue,
    pub x: f32,
    pub y: f32,
    pub flip: bool,
    /// The big form of the object (ScaleLevel Big).
    pub big: bool,
}

impl Fx {
    pub fn actor(kind: Kind, cue: FxCue, x: f32, y: f32) -> Self {
        Self {
            source: FxSource::Actor(kind),
            cue,
            x,
            y,
            flip: false,
            big: false,
        }
    }

    pub fn player(cue: FxCue, x: f32, y: f32) -> Self {
        Self {
            source: FxSource::Player,
            cue,
            x,
            y,
            flip: false,
            big: false,
        }
    }

    pub fn course(cue: FxCue, x: f32, y: f32) -> Self {
        Self {
            source: FxSource::Course,
            cue,
            x,
            y,
            flip: false,
            big: false,
        }
    }
}
