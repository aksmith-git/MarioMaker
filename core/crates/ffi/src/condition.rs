//! `mmk_engine_clear_condition`: the course's clear condition and its progress, for the HUD.

use mmk_sim::clear::Goal;

use crate::{MmkEngine, guard};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkCondition {
    /// 0 none, 1 defeat N (target = enemy class), 2 collect N 1-Ups, 3 collect N coins (target =
    /// coin value, 0 any), 4 use N POW blocks, 5 press N P Switches, 6 goal while invincible,
    /// 7 goal as Super, 8 goal as Fire, 9 no damage, 10 no landing, 11 goal in a Goomba's Shoe, 12 in a Koopa Clown Car, 13 as Raccoon, 14 as Cape, 15 as Propeller, 16 holding a shell, 17 on Yoshi, 18 as Cat, 19 in a Koopa Troopa Car, 20 as Builder, 21 on a Lakitu's Cloud, 22 holding the stone, 23 shake N trees, 24 not held by a claw, 25 no water, 26 wearing a helmet (target 1 Buzzy, 2 Spiny), 27 in a Dry Bones Shell, 28 as Superball Mario, 29 as Big Mario, 29 as Big Mario,
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
        let Some((kind, target)) = goal_code(c.goal) else {
            return false;
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

/// The condition's (kind, target) codes as `MmkCondition` gives them; None without one.
pub(crate) fn goal_code(goal: Goal) -> Option<(u32, u32)> {
    Some(match goal {
        Goal::None => return None,
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
        Goal::Cat => (18, 0),
        Goal::Builder => (20, 0),
        Goal::Cloud => (21, 0),
        Goal::HoldingStone => (22, 0),
        Goal::BellTrees => (23, 0),
        Goal::NoClaw => (24, 0),
        Goal::NoWater => (25, 0),
        Goal::Helmet(h) => (26, h as u32),
        Goal::DryBonesShell => (27, 0),
        Goal::Superball => (28, 0),
        Goal::Mega => (29, 0),
        Goal::HoldingShell => (16, 0),
        Goal::Yoshi => (17, 0),
        Goal::ClownCar => (12, 0),
        Goal::KoopaCar => (19, 0),
        Goal::NoDamage => (9, 0),
        Goal::NoLanding => (10, 0),
        Goal::Unsupported(i) => (255, i as u32),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<super::MmkCondition>(), 20);
    }
}
