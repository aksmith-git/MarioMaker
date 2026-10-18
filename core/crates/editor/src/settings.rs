//! Course and area settings: name, style, timer, theme / night, area size and orientation,
//! start / goal heights, clear condition, autoscroll (with custom-scroll nodes), liquid.

use mmk_formats::smm2::clear;

use crate::{DEFAULT_FLAGS, EditError, Editor, Rect, parts};

/// Width of a new sub-area: every unused sub-area of the sample is 84 tiles wide [corp].
pub const SUB_DEFAULT_WIDTH: i32 = 84;
/// Horizontal area widths in tiles [corp: 24–240 in the sample].
pub const WIDTH_MIN: i32 = 24;
pub const WIDTH_MAX: i32 = 240;
/// Horizontal areas are 27 tiles tall [corp: every one].
pub const HEIGHT_H: i32 = 27;
/// Vertical sub-areas: 48 tiles wide, 28–168 tiles tall in steps of 7 [corp]; new ones 42.
pub const VERT_W: i32 = 48;
pub const VERT_MIN: i32 = 28;
pub const VERT_MAX: i32 = 168;
pub const VERT_STEP: i32 = 7;
pub const VERT_DEFAULT: i32 = 42;
/// Timer range in seconds [obs themes.md: 10–500; corp: all multiples of 10].
pub const TIMER_MIN: i16 = 10;
pub const TIMER_MAX: i16 = 500;
/// Custom autoscroll nodes per area (the editor's counter limit) [obs].
pub const SCROLL_NODES_MAX: usize = 10;

/// Liquid behaviour (area header +5 / +6): mode 0 stays, 1 rises or falls, 2 rises and falls;
/// speed 1 slow – 3 fast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liquid {
    pub mode: u8,
    pub speed: u8,
    /// Heights in tiles.
    pub start: u8,
    pub end: u8,
}

impl Editor {
    pub fn set_name(&mut self, name: &str) {
        self.course
            .header
            .name
            .set(&name.chars().take(32).collect::<String>());
    }

    pub fn set_description(&mut self, text: &str) {
        self.course
            .header
            .description
            .set(&text.chars().take(75).collect::<String>());
    }

    /// Game style code: "M1", "M3", "MW", "WU", "3W". Parts the new style lacks stay in the file
    /// (the game asks before converting; here the caller decides).
    pub fn set_style(&mut self, style: &str) {
        self.course.header.game_style.set(style);
    }

    /// Time limit in seconds, 10–500 in steps of 10.
    pub fn set_timer(&mut self, seconds: i16) {
        let s = seconds.clamp(TIMER_MIN, TIMER_MAX);
        self.course.header.timer = (s + 5) / 10 * 10;
    }

    /// Theme of the current area: 0 ground … 9 forest.
    pub fn set_theme(&mut self, theme: u8) {
        self.area_mut().header.theme = theme.min(9);
    }

    /// Night version of the current area's theme (not in 3D World).
    pub fn set_night(&mut self, night: bool) {
        let night = night && self.course.header.game_style.as_str_lossy() != "3W";
        let h = &mut self.area_mut().header;
        h.flags = if night { h.flags | 2 } else { h.flags & !2 };
    }

    /// Height (tiles) of the start and goal ground: start 1–25, goal 1–15 (both from 2 in 3D
    /// World) [obs: course validator 0x710056d490].
    pub fn set_start_goal(&mut self, start_y: u8, goal_y: u8) {
        let low = if self.course.header.game_style.as_str_lossy() == "3W" {
            2
        } else {
            1
        };
        self.course.header.start_y = start_y.clamp(low, 25);
        self.course.header.goal_y = goal_y.clamp(low, 15);
    }

    /// Main-area width in tiles (kept for older callers): `set_area_width` on area 0.
    pub fn set_length(&mut self, tiles: i32) {
        let area = self.area;
        self.area = 0;
        let _ = self.set_area_width(tiles);
        self.area = area;
    }

    /// Width (horizontal areas, 24–240 tiles) or height (vertical sub-areas, 28–168 in steps of 7)
    /// of the current area. Fails if parts, ground or tracks would end up outside. In the main
    /// area the goal moves with the right edge: goal_x = (right − 152 px) / 1.6 [course-loading
    /// §1.5].
    pub fn set_area_width(&mut self, tiles: i32) -> Result<(), EditError> {
        let vertical = self.area().header.orientation != 0;
        let (w, h) = if vertical {
            let t = ((tiles.clamp(VERT_MIN, VERT_MAX) + VERT_STEP / 2) / VERT_STEP) * VERT_STEP;
            (VERT_W, t)
        } else {
            (tiles.clamp(WIDTH_MIN, WIDTH_MAX), HEIGHT_H)
        };
        self.resize_area(w, h)
    }

    /// Make the sub-area vertical (48 tiles wide, 42 tall) or horizontal (84 × 27). The main area
    /// is always horizontal.
    pub fn set_vertical(&mut self, vertical: bool) -> Result<(), EditError> {
        if self.area == 0 {
            return Err(EditError::NotAllowed("the main area is horizontal"));
        }
        let (w, h) = if vertical {
            (VERT_W, VERT_DEFAULT)
        } else {
            (SUB_DEFAULT_WIDTH, HEIGHT_H)
        };
        self.resize_area(w, h)?;
        let a = self.area_mut();
        a.header.orientation = vertical as u8;
        // a sub-area has no custom autoscroll
        if a.header.autoscroll_type == 4 {
            a.header.autoscroll_type = 0;
        }
        self.course.header.management_flags |= 0x40;
        Ok(())
    }

    fn resize_area(&mut self, w: i32, h: i32) -> Result<(), EditError> {
        let inside = Rect::new(0, 0, w, h);
        let a = self.area();
        let outside = a
            .objects
            .iter()
            .any(|o| !crate::rules::is_rider(o) && !rect_within(&parts::footprint(o), &inside))
            || a.ground
                .iter()
                .any(|g| !inside.contains(g.x as i32, g.y as i32))
            || a.tracks.iter().any(|t| {
                let (cx, cy) = crate::tracks::centre(t);
                !inside.contains(cx, cy)
            });
        if outside {
            return Err(EditError::Occupied);
        }
        let main = self.area == 0;
        let hd = &mut self.area_mut().header;
        hd.boundary_right = w * 16;
        hd.boundary_top = h * 16;
        if main {
            self.course.header.goal_x = (((w * 16 - 152) * 10) as f32 / 16.0).round() as i16;
        }
        Ok(())
    }

    // ------------------------------------------------------------ clear condition

    /// Clear condition: `category` 1 parts (defeat / collect / use `magnitude`), 2 status, 3
    /// actions; `index` in the game's condition list (formats `smm2::clear`); 0 for none.
    pub fn set_clear_condition(
        &mut self,
        category: u8,
        index: u8,
        magnitude: i16,
    ) -> Result<(), EditError> {
        let h = &mut self.course.header;
        if category == clear::NONE || index == 0 {
            h.clear_condition_category = 0;
            h.clear_condition = 0;
            h.clear_condition_magnitude = 0;
            h.management_flags &= !0x20;
            return Ok(());
        }
        if !clear::categories(index).contains(&category) {
            return Err(EditError::NotAllowed("clear condition"));
        }
        let hash = clear::hash(index).ok_or(EditError::NotAllowed("clear condition"))?;
        let n = if category == clear::PARTS {
            magnitude.clamp(1, 9999)
        } else {
            0
        };
        h.clear_condition_category = category;
        h.clear_condition = hash;
        h.clear_condition_magnitude = n;
        // management bit 5: the condition has an amount [ext smm2-course.md; corp: 58 of 73]
        if n > 0 {
            h.management_flags |= 0x20;
        } else {
            h.management_flags &= !0x20;
        }
        Ok(())
    }

    // ------------------------------------------------------------ autoscroll

    /// Autoscroll of the current area: 0 none, 1 slow, 2 normal, 3 fast, 4 custom (main area
    /// only, the route is the custom-scroll nodes) [obs: the course validator allows 0–3 in the
    /// sub-area].
    pub fn set_autoscroll(&mut self, kind: u8) {
        let kind = if kind >= 4 && self.area != 0 {
            0
        } else {
            kind.min(4)
        };
        self.area_mut().header.autoscroll_type = kind;
    }

    /// Custom autoscroll speed (course header 0x0E): 0 ×1, 1 ×2, 2 ×3.
    pub fn set_custom_scroll_speed(&mut self, speed: u8) {
        self.course.header.autoscroll_speed = speed.min(2);
    }

    /// Add a custom-scroll node (part 89, 3×1, centred on (tx, ty)): numbered in order, speed
    /// (flags >> 18) & 3 for the leg after it [sim 89-autoscroll-camera.md].
    pub fn add_scroll_node(&mut self, tx: i32, ty: i32, speed: u8) -> Result<usize, EditError> {
        let n = self.area().objects.iter().filter(|o| o.id == 89).count();
        if self.area != 0 {
            return Err(EditError::NotAllowed("custom autoscroll is main-area only"));
        }
        if n >= SCROLL_NODES_MAX {
            return Err(EditError::Limit("custom scroll nodes"));
        }
        if !self.in_bounds(tx, ty) {
            return Err(EditError::OutOfBounds);
        }
        let flags = DEFAULT_FLAGS | (n as u32) << 20 | (speed.min(2) as u32) << 18;
        let o = parts::new_object(89, tx - 1, ty, 3, 1, flags);
        Ok(self.push_object(o))
    }

    // ------------------------------------------------------------ liquid

    /// Water / lava / poison level of the current area (castle: lava, forest: water, poison at
    /// night; underwater areas are always full). Heights clamp to the area.
    pub fn set_liquid(&mut self, l: Liquid) {
        let (_, h) = self.size();
        let top = (h - 1).clamp(0, 255) as u8;
        let mode = l.mode.min(2);
        let speed = if mode == 0 { 0 } else { l.speed.clamp(1, 3) };
        let hd = &mut self.area_mut().header;
        hd.liquid_mode = mode;
        hd.liquid_speed = speed;
        hd.liquid_start_height = l.start.min(top);
        hd.liquid_end_height = if mode == 0 {
            l.start.min(top)
        } else {
            l.end.min(top)
        };
    }

    pub fn liquid(&self) -> Liquid {
        let h = &self.area().header;
        Liquid {
            mode: h.liquid_mode,
            speed: h.liquid_speed,
            start: h.liquid_start_height,
            end: h.liquid_end_height,
        }
    }
}

fn rect_within(r: &Rect, outer: &Rect) -> bool {
    r.x >= outer.x
        && r.y >= outer.y
        && r.x + r.w <= outer.x + outer.w
        && r.y + r.h <= outer.y + outer.h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;
    use mmk_sim::level::Level;

    #[test]
    fn both_areas_and_vertical_sub_areas() {
        let mut e = Editor::new("M1");
        for x in 7..100 {
            e.paint_ground(x, 0).unwrap();
        }
        e.set_area(1);
        e.set_theme(1);
        e.set_night(true);
        e.set_vertical(true).unwrap();
        assert_eq!(e.size(), (48, 42));
        e.set_area_width(100).unwrap();
        assert_eq!(e.size(), (48, 98), "steps of 7");
        for x in 0..48 {
            e.paint_ground(x, 0).unwrap();
        }
        e.place(0, 10, 60).unwrap();
        assert_eq!(
            e.set_area_width(28),
            Err(EditError::Occupied),
            "a Goomba would be outside"
        );
        e.set_area(0);
        e.set_area_width(120).unwrap();
        assert_eq!(e.course.header.goal_x, ((120 * 16 - 152) * 10 / 16) as i16);
        assert!(e.set_vertical(true).is_err());
        let back = round_trip(&e);
        assert!(back.header.management_flags & 0x40 != 0);
        let sub = Level::from_course(&back, 1);
        assert!(sub.vertical && sub.night && sub.theme == 1);
        assert_eq!(sub.top, 98.0 * 16.0);
        let main = Level::from_course(&back, 0);
        assert_eq!(main.right, 120.0 * 16.0);
        assert!(main.goal.is_some());
    }

    #[test]
    fn course_settings() {
        let mut e = flat("M1");
        e.set_timer(333);
        assert_eq!(e.course.header.timer, 330);
        e.set_timer(2);
        assert_eq!(e.course.header.timer, 10);
        // defeat 3 Goombas
        e.set_clear_condition(clear::PARTS, 1, 3).unwrap();
        assert_eq!(e.course.header.clear_condition, 0x7f07acbf);
        assert!(
            e.set_clear_condition(clear::STATUS, 1, 0).is_err(),
            "Goombas are a count"
        );
        for x in [20, 24, 28] {
            e.place(0, x, 2).unwrap();
        }
        let back = round_trip(&e);
        let w = mmk_sim::World::from_course(&back);
        assert_eq!(w.clear.goal, mmk_sim::clear::Goal::Defeat(1));
        assert_eq!(w.clear.n, 3);
        // reach the goal without taking damage
        e.set_clear_condition(clear::ACTIONS, 70, 5).unwrap();
        assert_eq!(e.course.header.clear_condition_magnitude, 0);
        let w = mmk_sim::World::from_course(&round_trip(&e));
        assert_eq!(w.clear.goal, mmk_sim::clear::Goal::NoDamage);
        e.set_clear_condition(0, 0, 0).unwrap();
        assert_eq!(e.course.header.management_flags & 0x20, 0);
    }

    #[test]
    fn autoscroll_and_liquid() {
        let mut e = flat("M1");
        e.set_autoscroll(4);
        e.set_custom_scroll_speed(1);
        e.add_scroll_node(12, 6, 0).unwrap();
        e.add_scroll_node(40, 12, 2).unwrap();
        e.set_theme(2);
        e.set_liquid(Liquid {
            mode: 2,
            speed: 3,
            start: 2,
            end: 8,
        });
        let back = round_trip(&e);
        let lvl = Level::from_course(&back, 0);
        assert_eq!(lvl.autoscroll, 4);
        // the two nodes (numbered 0 and 1, so the first replaces the start node) and the end
        assert_eq!(lvl.scroll_nodes.len(), 3);
        assert_eq!(
            lvl.scroll_nodes[1],
            (1, 40.0 * 16.0 + 8.0, 12.0 * 16.0 + 8.0, 2)
        );
        let l = lvl.liquid.unwrap();
        assert_eq!(
            (l.kind, l.mode, l.speed),
            (2, 2, 3),
            "lava, rising and falling, fast"
        );
        e.set_liquid(Liquid {
            mode: 0,
            speed: 3,
            start: 4,
            end: 9,
        });
        assert_eq!(
            e.liquid(),
            Liquid {
                mode: 0,
                speed: 0,
                start: 4,
                end: 4
            }
        );
    }
}
