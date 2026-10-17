//! One tick of player input. Every run is a sequence of these, so every run is a replay.

pub const LEFT: u32 = 1 << 0;
pub const RIGHT: u32 = 1 << 1;
pub const UP: u32 = 1 << 2;
pub const DOWN: u32 = 1 << 3;
pub const JUMP: u32 = 1 << 4;
pub const RUN: u32 = 1 << 5;
pub const START: u32 = 1 << 6;
/// Spin jump (SMW, NSMBU, 3D World).
pub const SPIN: u32 = 1 << 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub buttons: u32,
}

impl Input {
    pub fn held(self, b: u32) -> bool {
        self.buttons & b != 0
    }
    /// -1 left, 0 none/both, +1 right.
    pub fn dir_x(self) -> i32 {
        match (self.held(LEFT), self.held(RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        }
    }
}
