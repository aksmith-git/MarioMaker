//! Parts added in SMM2's 2.0 / 3.0 updates (they exist only in the v3.0.3 executable): Spike
//! (74-spike.md), Pokey (78-pokey.md), Mechakoopa (111-mechakoopa.md) and the ON/OFF Trampoline
//! (132-on-off-trampoline.md). The Koopalings are in actors/koopalings.rs; the world-side rules
//! (touching them, sprites, the P Block and Dash Block tiles) in world/v3.rs.

use super::{Actor, Kind};
use crate::level::Level;

/// A sprite choice: model, texture (animation frame), width, height, mirror.
pub type V3Look = (&'static str, &'static str, f32, f32, bool);

impl Kind {
    /// One of the 2.0 / 3.0 parts of this module or actors/koopalings.rs.
    pub fn is_v3(self) -> bool {
        matches!(
            self,
            Kind::Spike
                | Kind::SpikeBall { .. }
                | Kind::Pokey { .. }
                | Kind::Mechakoopa { .. }
                | Kind::MechaMissile
                | Kind::MechaLaser
                | Kind::OnOffTrampoline { .. }
                | Kind::Koopaling { .. }
                | Kind::KoopalingShot { .. }
        )
    }
}

impl Actor {
    /// Half width and height of the body box.
    pub(super) fn v3_size(&self) -> (f32, f32) {
        let s = if self.big { 2.0 } else { 1.0 };
        let (hw, h) = match self.kind {
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => return self.koopaling_size(),
            _ => (8.0, 16.0),
        };
        (hw * s, h * s)
    }

    /// One frame of a 2.0 / 3.0 part (from `step_normal`).
    pub(super) fn step_v3(&mut self, level: &Level) -> Option<Actor> {
        match self.kind {
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => self.step_koopaling(level),
            _ => None,
        }
    }

    /// Model and animation frame (`M1_` names; the front ends remap them per style).
    pub fn v3_look(&self) -> Option<V3Look> {
        match self.kind {
            Kind::Koopaling { .. } | Kind::KoopalingShot { .. } => self.koopaling_look(),
            _ => None,
        }
    }
}
