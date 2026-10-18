//! Course Maker mode of the portable shell (Phase 9): edit a course with the `editor` crate and
//! see it drawn by the engine; Enter test-plays it, E (in play) returns to editing.
//!
//! Controls: mouse left places the selected part (or paints ground), right erases; 1–9 / 0 and
//! Tab pick the part; arrows scroll; Ctrl+Z / Ctrl+Y undo / redo; Ctrl+S saves; Enter plays.
//! On the part under the cursor: W wings, B big, F alternate form, I put the selected part
//! inside. T cycles the area theme, N toggles night. [placeholder UI: a text strip until the SMM2 maker layouts draw]

use std::path::PathBuf;

use mmk_editor::Editor;
use mmk_sim::World;
use mmk_sim::world::{VIEW_H, VIEW_W};

/// What the cursor puts down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Ground,
    Part(i16),
}

/// The palette (key 1 = first entry … 0 = tenth; Tab cycles through all).
pub const PALETTE: &[(&str, Tool)] = &[
    ("Ground", Tool::Ground),
    ("Brick", Tool::Part(4)),
    ("? Block", Tool::Part(5)),
    ("Hard Block", Tool::Part(6)),
    ("Coin", Tool::Part(8)),
    ("Goomba", Tool::Part(0)),
    ("Koopa", Tool::Part(1)),
    ("Pipe", Tool::Part(9)),
    ("Super Mushroom", Tool::Part(20)),
    ("Fire Flower", Tool::Part(34)),
    ("Piranha Plant", Tool::Part(2)),
    ("Spring", Tool::Part(10)),
    ("Semisolid", Tool::Part(16)),
    ("Lift", Tool::Part(11)),
    ("Thwomp", Tool::Part(12)),
    ("Bill Blaster", Tool::Part(13)),
    ("Spiny", Tool::Part(25)),
    ("Hammer Bro", Tool::Part(3)),
    ("Bob-omb", Tool::Part(15)),
    ("Boo", Tool::Part(41)),
    ("1-Up", Tool::Part(33)),
    ("Super Star", Tool::Part(35)),
    ("Spike Trap", Tool::Part(43)),
    ("Checkpoint", Tool::Part(90)),
];

pub struct Maker {
    pub editor: Editor,
    /// Bottom-left of the view, world px.
    pub cam: (f32, f32),
    pub tool: usize,
    pub cursor: Option<(i32, i32)>,
    pub path: Option<PathBuf>,
    pub message: String,
    /// Course server for Ctrl+U uploads (`--server`).
    pub server: Option<String>,
    pub maker_name: String,
    world: World,
    dirty: bool,
}

impl Maker {
    pub fn new(editor: Editor, path: Option<PathBuf>) -> Self {
        let world = World::from_course(&editor.course);
        Self {
            editor,
            cam: (0.0, 0.0),
            tool: 0,
            cursor: None,
            path,
            message: String::new(),
            server: None,
            maker_name: "anonymous".into(),
            world,
            dirty: true,
        }
    }

    pub fn tool(&self) -> (&'static str, Tool) {
        PALETTE[self.tool % PALETTE.len()]
    }

    pub fn select(&mut self, i: usize) {
        self.tool = i % PALETTE.len();
        self.message.clear();
    }

    pub fn next_tool(&mut self) {
        self.select(self.tool + 1);
    }

    /// Scroll the view (px), kept inside the area.
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        let (w, h) = self.editor.size();
        self.cam.0 = (self.cam.0 + dx).clamp(0.0, (w as f32 * 16.0 - VIEW_W).max(0.0));
        self.cam.1 = (self.cam.1 + dy).clamp(0.0, (h as f32 * 16.0 - VIEW_H).max(0.0));
    }

    /// Tile under a view position (u right, v down, in view px 0…384 × 0…216).
    pub fn tile_at_view(&self, u: f32, v: f32) -> (i32, i32) {
        let x = self.cam.0 + u;
        let y = self.cam.1 + (VIEW_H - v);
        ((x / 16.0).floor() as i32, (y / 16.0).floor() as i32)
    }

    pub fn hover(&mut self, tile: Option<(i32, i32)>) {
        if tile != self.cursor {
            self.cursor = tile;
            self.dirty = true;
        }
    }

    /// Mouse button at the cursor: place (left) or erase (right).
    pub fn click(&mut self, erase: bool) {
        let Some((tx, ty)) = self.cursor else { return };
        self.editor.checkpoint();
        let ok = if erase {
            self.editor.erase(tx, ty)
        } else {
            match self.tool().1 {
                Tool::Ground => self.editor.paint_ground(tx, ty).is_ok(),
                Tool::Part(p) => self.editor.place(p, tx, ty).is_ok(),
            }
        };
        if !ok {
            // nothing changed: drop the checkpoint
            self.editor.undo();
            self.editor.redo_clear();
        }
        self.dirty = true;
    }

    /// Edit the part under the cursor: `what` = 'w' wings, 'b' big, 'f' alternate form, 'i' put
    /// the selected palette part inside it (a block's item, a pipe's enemy).
    pub fn modify(&mut self, what: char) {
        let Some((tx, ty)) = self.cursor else { return };
        let Some(i) = self.editor.object_at(tx, ty) else {
            self.message = "no part under the cursor".into();
            return;
        };
        self.editor.checkpoint();
        match what {
            'w' => self.editor.toggle_flags(i, 0x2),
            'f' => self.editor.toggle_flags(i, 0x4),
            'b' => {
                let _ = self.editor.toggle_big(i);
            }
            'i' => match self.tool().1 {
                Tool::Part(p) => self.editor.set_child(i, p, 0),
                Tool::Ground => self.editor.set_child(i, -1, 0),
            },
            _ => {}
        }
        self.dirty = true;
    }

    /// Area settings: 't' next theme, 'n' night on/off.
    pub fn setting(&mut self, what: char) {
        self.editor.checkpoint();
        let (theme, night) = self.editor.theme();
        match what {
            't' => self.editor.set_theme((theme + 1) % 10),
            'n' => self.editor.set_night(!night),
            _ => {}
        }
        let (theme, night) = self.editor.theme();
        const NAMES: [&str; 10] = [
            "Ground",
            "Underground",
            "Castle",
            "Airship",
            "Underwater",
            "Ghost House",
            "Snow",
            "Desert",
            "Sky",
            "Forest",
        ];
        self.message = format!(
            "{}{}",
            NAMES[theme as usize % 10],
            if night { " (night)" } else { "" }
        );
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        self.dirty |= self.editor.undo();
    }

    pub fn redo(&mut self) {
        self.dirty |= self.editor.redo();
    }

    /// Save to the course path (raw course data); a new course goes to `course.bin`.
    pub fn save(&mut self) {
        let path = self
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("course.bin"));
        self.message = match self.editor.save().map(|b| std::fs::write(&path, b)) {
            Ok(Ok(())) => {
                self.path = Some(path.clone());
                format!("saved {}", path.display())
            }
            Ok(Err(e)) => format!("save failed: {e}"),
            Err(e) => format!("save failed: {e:?}"),
        };
    }

    /// Upload the course to the course server (Ctrl+U); the message shows its code.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn upload(&mut self) {
        let Some(server) = self.server.clone() else {
            self.message = "no --server given".into();
            return;
        };
        self.message = match self.editor.save() {
            Ok(b) => match mmk_online::Client::new(server).upload(&b, &self.maker_name) {
                Ok(l) => format!("uploaded: {}", l.code),
                Err(e) => format!("upload failed: {e}"),
            },
            Err(e) => format!("upload failed: {e:?}"),
        };
    }

    /// The course bytes for test play.
    pub fn course_bytes(&self) -> Option<Vec<u8>> {
        self.editor.save().ok()
    }

    /// The world to draw: the course as built, with the selected part shown at the cursor.
    pub fn world(&mut self) -> &World {
        if self.dirty {
            self.dirty = false;
            let mut preview = None;
            if let (Some((tx, ty)), (_, tool)) = (self.cursor, self.tool())
                && let Ok(bytes) = self.editor.save()
                && let Ok(mut e) = Editor::open(&bytes)
            {
                let placed = match tool {
                    Tool::Ground => e.paint_ground(tx, ty).is_ok(),
                    Tool::Part(p) => e.place(p, tx, ty).is_ok(),
                };
                if placed {
                    preview = Some(World::from_course(&e.course));
                }
            }
            self.world = preview.unwrap_or_else(|| World::from_course(&self.editor.course));
            // nothing moves while editing: the player waits at the start
            self.world.player.hidden = false;
        }
        self.world.camera_x = self.cam.0;
        self.world.camera_y = self.cam.1;
        &self.world
    }

    /// The text strip shown while editing.
    pub fn status(&self) -> String {
        let (name, _) = self.tool();
        let cur = self
            .cursor
            .map_or(String::new(), |(x, y)| format!("  ({x},{y})"));
        let msg = if self.message.is_empty() {
            "click place / right erase / Tab part / Enter play / Ctrl+S save".to_string()
        } else {
            self.message.clone()
        };
        format!("MAKER  {name}{cur}  {msg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_places_erases_and_undoes() {
        let mut m = Maker::new(Editor::new("M1"), None);
        // view position → tile: the bottom row of the view is tile row 0
        assert_eq!(m.tile_at_view(8.0, 215.0), (0, 0));
        m.select(5); // Goomba
        m.hover(Some((10, 3)));
        m.click(false);
        assert!(m.editor.object_at(10, 3).is_some());
        m.click(false); // occupied: no change, no extra undo step
        m.undo();
        assert!(m.editor.object_at(10, 3).is_none());
        m.redo();
        m.click(true);
        assert!(m.editor.object_at(10, 3).is_none());
        // the drawn world shows the ghost of the selected part under the cursor
        m.select(1);
        m.hover(Some((12, 4)));
        let w = m.world();
        assert!(w.blocks.get(12, 4).is_some());
        assert!(m.status().contains("Brick"));
    }
}
