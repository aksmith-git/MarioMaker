//! `mmk_engine_play_status`: what the SMM2 in-course screens show besides the HUD counters
//! (keys, pink coins, game style, character, checkpoint, why the player died, where the player
//! is on screen and when a death restarts the course — for the death iris).

use mmk_sim::player::{Control, DOWN_FALL_FRAMES};
use mmk_sim::world::GoalPhase;

use crate::{MmkEngine, guard};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MmkPlayStatus {
    /// Keys held.
    pub keys: u32,
    /// Pink coins collected this attempt, and how many the course has.
    pub pink_coins: u32,
    pub pink_total: u32,
    /// Game style: 0 SMB1, 1 SMB3, 2 SMW, 3 NSMBU, 4 3D World (the order of the HUD skins).
    pub style: u32,
    /// Player 1's character: 0 Mario, 1 Luigi, 2 Toad, 3 Toadette.
    pub character: u32,
    /// Frames left on the timer, −1 = no timer.
    pub time_frames: i32,
    /// bit 0: a checkpoint was reached (restarts begin there); bit 1: the player is dying because
    /// the time ran out; bit 2: the goal sequence is running.
    pub flags: u32,
    /// Current area (0 main, 1 sub).
    pub area: u32,
    /// Player 1 in the view: px from its bottom-left corner (feet).
    pub player_x: f32,
    pub player_y: f32,
    /// Frames until the death sequence restarts the course; −1 while alive.
    pub restart_in: i32,
    pub _pad: u32,
}

/// # Safety
/// `engine` must be a live handle and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmk_engine_play_status(engine: *const MmkEngine, out: *mut MmkPlayStatus) {
    guard((), || {
        let (Some(e), Some(out)) = (unsafe { engine.as_ref() }, unsafe { out.as_mut() }) else {
            return;
        };
        let w = &e.world;
        let dying = !w.player.alive();
        let restart_in = match w.player.control {
            Control::DownPose { timer } => timer + DOWN_FALL_FRAMES,
            Control::DownFall { timer } => timer,
            _ => -1,
        };
        *out = MmkPlayStatus {
            keys: w.keys,
            pink_coins: w.pink.len() as u32,
            pink_total: w.pink_total as u32,
            style: w.style as u32,
            character: w.main_char as u32,
            time_frames: w.time,
            flags: w.checkpoint.is_some() as u32
                | ((dying && w.time == 0) as u32) << 1
                | ((w.goal_phase != GoalPhase::None) as u32) << 2,
            area: w.area as u32,
            player_x: w.player.x - w.camera_x,
            player_y: w.player.y - w.camera_y,
            restart_in,
            _pad: 0,
        };
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_matches_header() {
        assert_eq!(size_of::<super::MmkPlayStatus>(), 48);
    }
}
