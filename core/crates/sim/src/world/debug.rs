//! Tools for course renders and tests: point the camera anywhere and wake what's there.

use super::World;
use crate::world::{VIEW_H, VIEW_W};

impl World {
    /// Put the view's bottom-left at (`x`, `y`) (clamped to the area) and spawn the actors whose
    /// homes are in view, as if the camera had scrolled there; nothing else moves.
    pub fn look_at(&mut self, x: f32, y: f32) {
        self.map_view = true;
        let l = &self.level;
        self.camera_x = x.clamp(l.left, (l.right - VIEW_W).max(l.left));
        self.camera_y = y.clamp(l.bottom, (l.top - VIEW_H).max(l.bottom));
        self.activate();
    }

    /// Show area `area` (0 main, 1 sub) as a warp would, for renders.
    pub fn show_area(&mut self, area: usize) {
        if area != self.area && area < self.area_initial.len() {
            self.switch_area(area);
        }
    }

    /// The area's bounds in pixels: (left, bottom, right, top).
    pub fn area_bounds(&self) -> (f32, f32, f32, f32) {
        let l = &self.level;
        (l.left, l.bottom, l.right, l.top)
    }
}
