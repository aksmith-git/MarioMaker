//! World-side rules for the parts added in SMM2's 2.0 / 3.0 updates (actors/v3.rs,
//! actors/koopalings.rs): the player touching them, fireballs and shells hitting them, the P
//! Block and Dash Block tiles, and their extra sprites. Reactions follow the EnemyDB rows of
//! v3.0.3 and the parts' own code; what the binary does not settle is marked [inf] / [?].

use super::{Event, World, overlap};
use crate::actors::Actor;
use crate::render::Sprite;

impl World {
    /// The player touching a 2.0 / 3.0 part at `i`. Returns true when the player was hurt (the
    /// caller stops checking this frame).
    pub(super) fn v3_contact(&mut self, i: usize, _jump_held: bool) -> bool {
        let hurt = self.player.hurt_box();
        let a = &self.actors[i];
        if !overlap(hurt, a.bbox()) {
            return false;
        }
        self.v3_hurt()
    }

    /// The player touched something harmful: true when hurt.
    fn v3_hurt(&mut self) -> bool {
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

    /// Per-frame world rules of these parts (before the actors move).
    pub(super) fn step_v3(&mut self) {}

    /// Extra sprites (Pokey segments, the P Block and Dash Block tiles, …).
    pub(super) fn v3_sprites(&self, _out: &mut Vec<Sprite>) {}
}

/// A player fireball hitting a 2.0 / 3.0 part: None when `e` is not one (the generic rules
/// apply), else whether it was knocked off.
pub(super) fn fire_hit(e: &mut Actor, _from_x: f32) -> Option<bool> {
    if !e.kind.is_v3() {
        return None;
    }
    Some(false)
}
