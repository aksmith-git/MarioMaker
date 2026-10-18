//! Fades between scenes with SMM2's own fade layout (`Cmn_Fade_00`), the way the game's scene
//! fade sequence uses it (the Swift app's `SceneFade.swift`):
//! - a fade has a kind that picks a pair of the layout's animations: black (FadeOutB / FadeInB,
//!   30 + 30 frames), white (FadeOutW / FadeInW), black circle (OutCircleB / InCircleB, 60 + 60),
//!   white circle, blocks (OutBlock / InBlock, 56 + 56) [obs];
//! - the Out animation covers the old scene; when it has ended the scene changes, and once the
//!   new one is up the In animation uncovers it [obs];
//! - a scene change without fade parameters uses the black fade at rate 1 [obs]; the boot opening
//!   hands over to the title with the black circle [obs].
//!
//! The covered screen stays black for two ticks (one frame of the new screen drawn under the
//! cover) [inf]. The host's [`crate::Front`] runs the scene change when [`SceneFade::step`]
//! reports the screen covered.

use crate::layout::{Layout, Ui};
use crate::{DrawItem, push_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeKind {
    Black,
    White,
    CircleBlack,
    CircleWhite,
    Block,
}

impl FadeKind {
    const ALL: [FadeKind; 5] = [
        FadeKind::Black,
        FadeKind::White,
        FadeKind::CircleBlack,
        FadeKind::CircleWhite,
        FadeKind::Block,
    ];

    pub fn out(self) -> &'static str {
        match self {
            FadeKind::Black => "FadeOutB",
            FadeKind::White => "FadeOutW",
            FadeKind::CircleBlack => "OutCircleB",
            FadeKind::CircleWhite => "OutCircleW",
            FadeKind::Block => "OutBlock",
        }
    }

    pub fn into(self) -> &'static str {
        match self {
            FadeKind::Black => "FadeInB",
            FadeKind::White => "FadeInW",
            FadeKind::CircleBlack => "InCircleB",
            FadeKind::CircleWhite => "InCircleW",
            FadeKind::Block => "InBlock",
        }
    }

    /// The panes this kind draws (the others stay hidden; the layout at rest is all black).
    fn panes(self) -> &'static [&'static str] {
        match self {
            FadeKind::Black => &["P_FadeB_00"],
            FadeKind::White => &["P_FadeW_00"],
            FadeKind::CircleBlack => &["N_Pos_00", "W_CircleB_00"],
            FadeKind::CircleWhite => &["N_Pos_00", "W_CircleW_00"],
            FadeKind::Block => &["N_Block_00"],
        }
    }
}

const ALL_PANES: [&str; 7] = [
    "P_FadeB_00",
    "P_FadeW_00",
    "N_Pos_00",
    "W_CircleB_00",
    "W_CircleW_00",
    "N_Block_00",
    "N_CaptureFade_00",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Out,
    Covered,
    Into,
}

pub struct SceneFade {
    layout: Option<Layout>,
    phase: Phase,
    kind: FadeKind,
    phase_start: i32,
    /// Ticks since the fade started.
    pub tick: i32,
    /// Debug captures: scene changes happen at once (the Swift app's frozen captures).
    pub instant: bool,
}

/// What [`SceneFade::run`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeStart {
    /// The fade is covering the screen: change the scene when `step` says so.
    Started,
    /// Change the scene now (a fade already runs, or there is no fade layout).
    Now,
}

impl SceneFade {
    pub fn new(ui: Option<&Ui>) -> Self {
        Self {
            layout: ui.and_then(|u| u.layout("Cmn_Fade_00")),
            phase: Phase::Idle,
            kind: FadeKind::Black,
            phase_start: 0,
            tick: 0,
            instant: false,
        }
    }

    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// How much of the screen is covered (0–1).
    pub fn cover(&mut self) -> f32 {
        let since = (self.tick - self.phase_start) as f32;
        let Some(l) = &mut self.layout else {
            return 0.0;
        };
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Out => (since / l.length(self.kind.out()).max(1.0)).min(1.0),
            Phase::Covered => 1.0,
            Phase::Into => (1.0 - since / l.length(self.kind.into()).max(1.0)).max(0.0),
        }
    }

    /// Cover the screen with `kind` (circles centred on `center`, layout units).
    pub fn run(&mut self, kind: FadeKind, center: [f32; 2]) -> FadeStart {
        if self.phase != Phase::Idle || self.instant || !self.begin(kind, center) {
            return FadeStart::Now;
        }
        if let Some(l) = &mut self.layout {
            l.play(kind.out(), 0);
        }
        self.phase = Phase::Out;
        FadeStart::Started
    }

    /// Uncover only: a screen that starts covered plays the In animation of `kind`.
    pub fn reveal(&mut self, kind: FadeKind, center: [f32; 2]) {
        if self.phase != Phase::Idle || self.instant || !self.begin(kind, center) {
            return;
        }
        if let Some(l) = &mut self.layout {
            l.hold_end(kind.out());
        }
        self.phase = Phase::Covered;
    }

    fn begin(&mut self, kind: FadeKind, center: [f32; 2]) -> bool {
        let Some(l) = &mut self.layout else {
            return false;
        };
        self.kind = kind;
        for p in ALL_PANES {
            l.show(p, false);
        }
        for p in kind.panes() {
            l.show(p, true);
        }
        l.set_translate("N_Pos_00", center);
        for k in FadeKind::ALL {
            l.stop(k.into());
            l.stop(k.out());
        }
        self.tick = 0;
        self.phase_start = 0;
        true
    }

    /// One tick; true when the screen has just become covered (change the scene now).
    pub fn step(&mut self) -> bool {
        if self.phase == Phase::Idle {
            return false;
        }
        let Some(l) = &mut self.layout else {
            self.phase = Phase::Idle;
            return false;
        };
        self.tick += 1;
        let since = self.tick - self.phase_start;
        match self.phase {
            Phase::Out => {
                let n = l.length(self.kind.out()) as i32;
                if since >= n {
                    self.phase = Phase::Covered;
                    self.phase_start = self.tick;
                    return true;
                }
            }
            Phase::Covered => {
                if since >= 2 {
                    l.stop(self.kind.out());
                    l.play(self.kind.into(), self.tick);
                    self.phase = Phase::Into;
                    self.phase_start = self.tick;
                }
            }
            Phase::Into => {
                let n = l.length(self.kind.into()) as i32;
                if since >= n {
                    self.phase = Phase::Idle;
                    l.stop(self.kind.into());
                }
            }
            Phase::Idle => {}
        }
        false
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        if self.phase != Phase::Idle
            && let Some(l) = &mut self.layout
        {
            push_layout(out, l, self.tick);
        }
    }
}
