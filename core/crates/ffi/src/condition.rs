//! `mmk_engine_clear_condition`: the course's clear condition and its progress, for the HUD.

use mmk_sim::clear::Goal;

use crate::{MmkEngine, guard};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkCondition {
    /// 0 none, 1 defeat N (target = enemy class), 2 collect N 1-Ups, 3 collect N coins (target =
    /// coin value, 0 any), 4 use N POW blocks, 5 press N P Switches, 6 goal while invincible,
    /// 7 goal as Super, 8 goal as Fire, 9 no damage, 10 no landing, 11 goal in a Goomba's Shoe, 12 in a Koopa Clown Car, 13 as Raccoon, 14 as Cape, 15 as Propeller, 16 holding a shell, 17 on Yoshi,
    /// 255 not supported.
    pub kind: u32,
    pub target: u32,
    pub count: u32,
    pub needed: u32,
    /// The goal would accept the player now (status conditions: in the current state).
    pub met: u8,
    pub violated: u8,
    pub _pad: [u8; 2],
}

/// False when the course has no clear condition.
///
/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_clear_condition(
    engine: *const MmkEngine,
    out: *mut MmkCondition,
) -> bool {
    guard(false, || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return false;
        };
        let c = &e.world.clear;
        let (kind, target) = match c.goal {
            Goal::None => return false,
            Goal::Defeat(t) => (1, t as u32),
            Goal::OneUps => (2, 0),
            Goal::Coins(v) => (3, v),
            Goal::Pow => (4, 0),
            Goal::PSwitch => (5, 0),
            Goal::Star => (6, 0),
            Goal::Super => (7, 0),
            Goal::Fire => (8, 0),
            Goal::Shoe => (11, 0),
            Goal::Raccoon => (13, 0),
            Goal::Cape => (14, 0),
            Goal::Propeller => (15, 0),
            Goal::HoldingShell => (16, 0),
            Goal::Yoshi => (17, 0),
            Goal::ClownCar => (12, 0),
            Goal::NoDamage => (9, 0),
            Goal::NoLanding => (10, 0),
            Goal::Unsupported(i) => (255, i as u32),
        };
        let p = &e.world.player;
        *out = MmkCondition {
            kind,
            target,
            count: c.count,
            needed: c.n,
            met: c.met(p.at_goal()) as u8,
            violated: c.violated as u8,
            _pad: [0; 2],
        };
        true
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<super::MmkCondition>(), 20);
    }
}
