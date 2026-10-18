//! Keyboard (winit key codes) and gamepads (gilrs) → `mmk_sim::Input`, with the Swift app's
//! default bindings: arrows/WASD move, Z/Space jump, X/Shift run, Esc/P pause (Start), R restart.

use std::collections::HashSet;

use mmk_sim::{Input, input};
use winit::keyboard::KeyCode;

use crate::session::Actions;

/// Keys currently held.
#[derive(Debug, Default)]
pub struct Keyboard {
    down: HashSet<KeyCode>,
    restart: bool,
}

/// Engine button of a key (None: not a game key).
pub fn key_button(k: KeyCode) -> Option<u32> {
    Some(match k {
        KeyCode::ArrowLeft | KeyCode::KeyA => input::LEFT,
        KeyCode::ArrowRight | KeyCode::KeyD => input::RIGHT,
        KeyCode::ArrowUp | KeyCode::KeyW => input::UP,
        KeyCode::ArrowDown | KeyCode::KeyS => input::DOWN,
        KeyCode::KeyZ | KeyCode::Space => input::JUMP,
        KeyCode::KeyX | KeyCode::ShiftLeft | KeyCode::ShiftRight => input::RUN,
        KeyCode::Escape | KeyCode::KeyP | KeyCode::Enter => input::START,
        _ => return None,
    })
}

impl Keyboard {
    pub fn key(&mut self, k: KeyCode, pressed: bool) {
        if pressed {
            if k == KeyCode::KeyR && !self.down.contains(&k) {
                self.restart = true;
            }
            self.down.insert(k);
        } else {
            self.down.remove(&k);
        }
    }

    /// Key `k` is held.
    pub fn held(&self, k: KeyCode) -> bool {
        self.down.contains(&k)
    }

    /// Forget everything held (the window lost focus: the key-up events will never arrive).
    pub fn clear(&mut self) {
        self.down.clear();
    }

    pub fn buttons(&self) -> u32 {
        self.down
            .iter()
            .filter_map(|&k| key_button(k))
            .fold(0, |b, k| b | k)
    }

    /// Actions pressed since the last call.
    pub fn take_actions(&mut self) -> Actions {
        Actions {
            restart: std::mem::take(&mut self.restart),
        }
    }
}

/// Stick deflection that counts as a direction (as in the Swift app).
const STICK: f32 = 0.4;

/// Connected gamepads (gilrs). Missing gamepad support (no backend, no permission) leaves it
/// empty rather than failing.
pub struct Pads {
    gilrs: Option<gilrs::Gilrs>,
}

impl Pads {
    pub fn new() -> Self {
        Self {
            gilrs: gilrs::Gilrs::new().ok(),
        }
    }

    /// Process pending gamepad events and return the buttons held on any pad.
    pub fn poll(&mut self) -> u32 {
        self.poll_each().iter().fold(0, |a, b| a | b)
    }

    /// Process pending gamepad events and return each connected pad's buttons (in id order).
    pub fn poll_each(&mut self) -> Vec<u32> {
        use gilrs::{Axis, Button};
        let Some(g) = &mut self.gilrs else {
            return Vec::new();
        };
        while g.next_event().is_some() {}
        let mut out = Vec::new();
        for (_, pad) in g.gamepads() {
            let mut b = 0;
            let x = pad.value(Axis::LeftStickX);
            let y = pad.value(Axis::LeftStickY);
            let held = |btn: Button| pad.is_pressed(btn);
            if held(Button::DPadLeft) || x < -STICK {
                b |= input::LEFT;
            }
            if held(Button::DPadRight) || x > STICK {
                b |= input::RIGHT;
            }
            if held(Button::DPadUp) || y > STICK {
                b |= input::UP;
            }
            if held(Button::DPadDown) || y < -STICK {
                b |= input::DOWN;
            }
            // positions, not labels: south/east jump, west/north run (Nintendo B/A, Y/X)
            if held(Button::South) || held(Button::East) {
                b |= input::JUMP;
            }
            if held(Button::West) || held(Button::North) {
                b |= input::RUN;
            }
            if held(Button::Start) {
                b |= input::START;
            }
            out.push(b);
        }
        out
    }
}

impl Default for Pads {
    fn default() -> Self {
        Self::new()
    }
}

/// Keyboard and pads together.
pub fn combine(keys: &Keyboard, pads: u32) -> Input {
    Input {
        buttons: keys.buttons() | pads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_buttons_and_restart_is_an_edge() {
        let mut k = Keyboard::default();
        k.key(KeyCode::ArrowRight, true);
        k.key(KeyCode::ShiftLeft, true);
        k.key(KeyCode::KeyZ, true);
        assert_eq!(k.buttons(), input::RIGHT | input::RUN | input::JUMP);
        k.key(KeyCode::KeyZ, false);
        assert_eq!(k.buttons(), input::RIGHT | input::RUN);
        k.key(KeyCode::KeyR, true);
        k.key(KeyCode::KeyR, true); // key repeat
        assert!(k.take_actions().restart);
        assert!(!k.take_actions().restart);
        k.clear();
        assert_eq!(k.buttons(), 0);
    }
}
