//! The Koopalings (120-koopalings.md): Lemmy, Morton, Larry, Wendy, Iggy, Roy and Ludwig, boss
//! enemies on one shared action with per-Koopaling overrides, and their projectiles.

use super::Actor;
use super::v3::V3Look;
use crate::level::Level;

impl Actor {
    pub(super) fn koopaling_size(&self) -> (f32, f32) {
        let s = if self.big { 2.0 } else { 1.0 };
        (8.0 * s, 16.0 * s)
    }

    pub(super) fn step_koopaling(&mut self, _level: &Level) -> Option<Actor> {
        None
    }

    pub(super) fn koopaling_look(&self) -> Option<V3Look> {
        None
    }
}
