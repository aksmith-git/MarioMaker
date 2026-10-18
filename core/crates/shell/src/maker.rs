//! Course Maker mode of the portable shell (Phase 9): edit a course with the `editor` crate and
//! see it drawn by the engine; Enter test-plays it, E (in play) returns to editing.
//!
//! A debug-grade UI until the SMM2 maker layouts draw: the mouse places / erases, keys change
//! the part under the cursor or the course; the text strip says what happened. The controls are
//! listed in README.md (Course Maker) and in `HELP` below.

use std::path::PathBuf;

use mmk_editor::links::DoorKind;
use mmk_editor::parts::Dir;
use mmk_editor::paths::Step;
use mmk_editor::settings::Liquid;
use mmk_editor::{DEFAULT_FLAGS, EditError, Editor, Rect};
use mmk_formats::smm2::clear;
use mmk_sim::World;
use mmk_sim::level::Level;
use mmk_sim::world::{VIEW_H, VIEW_W};

/// What the cursor puts down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Ground,
    Part(i16),
    /// Two clicks: a pair of doors / warp boxes, or a track from one end point to the other.
    Doors,
    WarpBoxes,
    Track,
    /// Drag a rectangle.
    Select,
    /// Custom autoscroll node.
    ScrollNode,
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
    ("Select", Tool::Select),
    ("Fire Flower", Tool::Part(34)),
    ("Piranha Plant", Tool::Part(2)),
    ("Spring", Tool::Part(10)),
    ("Semisolid", Tool::Part(16)),
    ("Mushroom Platform", Tool::Part(14)),
    ("Bridge", Tool::Part(17)),
    ("Conveyor Belt", Tool::Part(53)),
    ("Lift", Tool::Part(11)),
    ("Steep Slope", Tool::Part(88)),
    ("Gentle Slope", Tool::Part(87)),
    ("Vine", Tool::Part(64)),
    ("Track", Tool::Track),
    ("Door", Tool::Doors),
    ("Warp Box", Tool::WarpBoxes),
    ("Key", Tool::Part(95)),
    ("Snake Block", Tool::Part(84)),
    ("! Block", Tool::Part(119)),
    ("Piranha Creeper", Tool::Part(107)),
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
    ("P Switch", Tool::Part(18)),
    ("Scroll Node", Tool::ScrollNode),
];

/// The control summary (also in README.md).
pub const HELP: &str = "click place / drag paint / right erase · Tab, 1-0 part · A area · \
V vertical · [ ] size · W wings F form B big I inside · R turn C colour -/= size (Shift: height) \
· L link pipes · K door kind · G on/off track · O track end · H path (arrows, H done) · \
Select: drag, Ctrl+C/X/V, Del, Shift+arrows · T theme N night Q scroll , . timer ; clear \
' liquid · Ctrl+Z/Y/S · Enter play";

/// A two-step action in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    /// First door / warp box / track end point chosen.
    First(Tool, (i32, i32)),
    /// A pipe chosen for linking: (area, index).
    Link(usize, usize),
}

/// Preset clear conditions cycled by `;`: (category, list index).
const CONDITIONS: &[(u8, u8)] = &[
    (clear::NONE, 0),
    (clear::PARTS, 1),    // defeat Goombas
    (clear::PARTS, 66),   // collect coins
    (clear::STATUS, 42),  // as Super Mario
    (clear::ACTIONS, 70), // without taking damage
    (clear::ACTIONS, 71), // without landing
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
    /// The selected rectangle (Select tool).
    pub selection: Option<Rect>,
    /// Direction new pipes face.
    pub pipe_dir: Dir,
    pending: Option<Pending>,
    /// Mouse held: where the drag started and whether it erases.
    drag: Option<((i32, i32), bool)>,
    /// Path being drawn: the part and its steps.
    path_edit: Option<(usize, Vec<Step>)>,
    condition: usize,
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
            selection: None,
            pipe_dir: Dir::Up,
            pending: None,
            drag: None,
            path_edit: None,
            condition: 0,
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
        self.pending = None;
        self.dirty = true;
    }

    pub fn next_tool(&mut self) {
        self.select(self.tool + 1);
    }

    pub fn prev_tool(&mut self) {
        self.select(self.tool + PALETTE.len() - 1);
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
            if let (Some((start, false)), Tool::Select, Some(t)) = (self.drag, self.tool().1, tile)
            {
                self.selection = Some(Rect::spanning(start, t));
            }
            // dragging paints ground / erases along the way
            if let (Some(((_, _), erase)), Some(_)) = (self.drag, tile)
                && (erase || self.tool().1 == Tool::Ground)
            {
                self.apply(erase, false);
            }
        }
    }

    /// Run an edit as one undo step; a failed or empty edit leaves no step.
    fn step(&mut self, f: impl FnOnce(&mut Editor) -> Result<String, EditError>) {
        self.editor.checkpoint();
        match f(&mut self.editor) {
            Ok(msg) => self.message = msg,
            Err(e) => {
                self.editor.undo();
                self.editor.redo_clear();
                self.message = describe(&e);
            }
        }
        self.dirty = true;
    }

    /// Mouse button pressed at the cursor (left places, right erases).
    pub fn press(&mut self, erase: bool) {
        let Some(t) = self.cursor else { return };
        self.drag = Some((t, erase));
        if !erase && self.tool().1 == Tool::Select {
            self.selection = Some(Rect::new(t.0, t.1, 1, 1));
            return;
        }
        self.apply(erase, true);
    }

    /// Mouse button released: finishes a selection drag.
    pub fn release(&mut self) {
        if let (Some((start, false)), Tool::Select, Some(end)) =
            (self.drag, self.tool().1, self.cursor)
        {
            let r = Rect::spanning(start, end);
            self.selection = Some(r);
            self.message = format!("selected {}×{} at ({},{})", r.w, r.h, r.x, r.y);
        }
        self.drag = None;
    }

    /// Mouse click at the cursor: place (left) or erase (right).
    pub fn click(&mut self, erase: bool) {
        self.press(erase);
        self.release();
    }

    fn apply(&mut self, erase: bool, first: bool) {
        let Some((tx, ty)) = self.cursor else { return };
        if erase {
            if self.pending.take().is_some() {
                self.message = "cancelled".into();
                return;
            }
            self.step(|e| {
                if e.erase(tx, ty) {
                    Ok(String::new())
                } else {
                    Err(EditError::Missing)
                }
            });
            return;
        }
        let tool = self.tool().1;
        match tool {
            Tool::Ground => self.step(|e| e.paint_ground(tx, ty).map(|_| String::new())),
            Tool::Part(9) if first => {
                let dir = self.pipe_dir;
                self.step(|e| e.place_pipe(tx, ty, 2, dir, 0).map(|_| String::new()))
            }
            Tool::Part(p) if first => self.step(|e| e.place(p, tx, ty).map(|_| String::new())),
            Tool::ScrollNode if first => self.step(|e| {
                e.add_scroll_node(tx, ty, 0)
                    .map(|_| "scroll node added (Q: custom autoscroll)".into())
            }),
            Tool::Doors | Tool::WarpBoxes | Tool::Track if first => match self.pending {
                Some(Pending::First(t, a)) if t == tool => {
                    self.pending = None;
                    self.step(|e| match tool {
                        Tool::Doors => e
                            .place_doors(a, (tx, ty), DoorKind::Normal)
                            .map(|_| "door pair".into()),
                        Tool::WarpBoxes => e
                            .place_warp_boxes(a, (tx, ty), false)
                            .map(|_| "warp box pair".into()),
                        _ => e
                            .draw_track(a, (tx, ty))
                            .map(|v| format!("{} track pieces", v.len())),
                    });
                }
                _ => {
                    self.pending = Some(Pending::First(tool, (tx, ty)));
                    self.message = "now click the other end".into();
                }
            },
            _ => {}
        }
    }

    /// Edit the part under the cursor: 'w' wings, 'b' big, 'f' alternate form, 'i' put the
    /// selected palette part inside it (a block's item, a pipe's contents), 'r' turn a pipe,
    /// 'c' pipe colour, 'k' door kind, 'g' on / off a track, '+' / '-' wider / narrower, '^' /
    /// 'v' taller / shorter, 'l' link pipes, 'h' draw a path.
    pub fn modify(&mut self, what: char) {
        let Some((tx, ty)) = self.cursor else { return };
        if what == 'o' {
            let open = !self.editor.track_end_open((tx, ty)).unwrap_or(false);
            self.step(|e| {
                e.set_track_end((tx, ty), open).map(|_| {
                    if open {
                        "track end opened"
                    } else {
                        "track end closed"
                    }
                    .into()
                })
            });
            return;
        }
        let Some(i) = self.editor.object_at(tx, ty) else {
            if what == 'r' {
                self.pipe_dir = turn(self.pipe_dir);
                self.message = format!("new pipes face {:?}", self.pipe_dir);
            } else {
                self.message = "no part under the cursor".into();
            }
            return;
        };
        let o = self.editor.object(i).cloned().expect("found");
        let tool = self.tool().1;
        match what {
            'w' => self.step(|e| {
                e.toggle_flags(i, 0x2);
                Ok("wings".into())
            }),
            'f' => self.step(|e| {
                e.toggle_flags(i, 0x4);
                Ok("form".into())
            }),
            'b' => self.step(|e| e.toggle_big(i).map(|_| "size".into())),
            'i' => self.step(|e| match tool {
                Tool::Part(p) if o.id == 9 => e.put_in_pipe(i, p).map(|_| "in the pipe".into()),
                Tool::Part(p) => e
                    .try_set_child(i, p, DEFAULT_FLAGS)
                    .map(|_| "inside".into()),
                _ if o.id == 9 => {
                    e.empty_pipe(i);
                    Ok("pipe emptied".into())
                }
                _ => e.try_set_child(i, -1, 0).map(|_| "emptied".into()),
            }),
            'r' | 'c' if o.id == 9 => {
                let dir = Dir::from_flags(o.flags);
                let colour = ((o.flags >> 18) & 3) as u8;
                let (dir, colour) = if what == 'r' {
                    (turn(dir), colour)
                } else {
                    (dir, (colour + 1) % 4)
                };
                self.step(|e| {
                    e.set_pipe(i, o.height, dir, colour)
                        .map(|_| format!("pipe {dir:?}"))
                });
            }
            '+' | '-' | '^' | 'v' => {
                let (dw, dh): (i32, i32) = match what {
                    '+' => (1, 0),
                    '-' => (-1, 0),
                    '^' => (0, 1),
                    _ => (0, -1),
                };
                let (w, h) = (
                    (o.width as i32 + dw).max(1) as u8,
                    (o.height as i32 + dh).max(1) as u8,
                );
                self.step(|e| e.resize(i, w, h).map(|_| format!("{w}×{h}")));
            }
            'k' if o.id == 55 => {
                let kind = match (o.flags >> 18) & 3 {
                    0 => DoorKind::PSwitch,
                    1 => DoorKind::Key,
                    _ => DoorKind::Normal,
                };
                self.step(|e| e.set_door_kind(i, kind).map(|_| format!("{kind:?} door")));
            }
            'g' => self.step(|e| {
                if o.flags & 0x400 != 0 {
                    e.detach_from_track(i);
                    Ok("off the track".into())
                } else {
                    e.attach_to_track(i).map(|_| "on the track".into())
                }
            }),
            'l' if o.id == 9 => match self.pending {
                Some(Pending::Link(area, j)) if area != self.editor.area => {
                    self.pending = None;
                    let (main, sub) = if area == 0 { (j, i) } else { (i, j) };
                    self.step(|e| {
                        e.link_pipes(main, sub)
                            .map(|id| format!("pipes linked ({id})"))
                    });
                }
                _ => {
                    self.pending = Some(Pending::Link(self.editor.area, i));
                    self.message = "pipe chosen: A to the other area, L on its pipe".into();
                }
            },
            'h' => match self.path_edit.take() {
                Some((j, steps)) => {
                    let heading = if self.editor.object(j).is_some_and(|o| o.id == 107) {
                        Step::Up
                    } else {
                        Step::Right
                    };
                    self.step(|e| {
                        e.set_path(j, heading, &steps)
                            .map(|_| format!("path of {} steps", steps.len()))
                    });
                }
                None if mmk_editor::paths::has_path(o.id) => {
                    self.path_edit = Some((i, Vec::new()));
                    self.message =
                        "path: arrows add steps, Backspace undoes one, H finishes".into();
                }
                None => self.message = "this part has no path".into(),
            },
            _ => self.message = "not for this part".into(),
        }
    }

    /// Arrow keys while a path is being drawn (true = used).
    pub fn path_key(&mut self, dir: Option<(i32, i32)>) -> bool {
        let Some((_, steps)) = self.path_edit.as_mut() else {
            return false;
        };
        match dir {
            Some((1, 0)) => steps.push(Step::Right),
            Some((-1, 0)) => steps.push(Step::Left),
            Some((0, 1)) => steps.push(Step::Up),
            Some((0, -1)) => steps.push(Step::Down),
            None => {
                steps.pop();
            }
            _ => {}
        }
        self.message = format!("path: {} steps (H finishes)", steps.len());
        true
    }

    /// Area and course settings: 't' next theme, 'n' night, 'a' other area, 'V' vertical
    /// sub-area, '[' / ']' smaller / bigger area, 'q' autoscroll, ',' / '.' timer, ';' clear
    /// condition, '\'' liquid.
    pub fn setting(&mut self, what: char) {
        let (theme, night) = self.editor.theme();
        match what {
            't' => self.step(|e| {
                e.set_theme((theme + 1) % 10);
                Ok(String::new())
            }),
            'n' => self.step(|e| {
                e.set_night(!night);
                Ok(String::new())
            }),
            'a' => {
                let to = 1 - self.editor.area;
                self.editor.set_area(to);
                self.selection = None;
                self.cam = (0.0, 0.0);
                self.message = if to == 0 { "main area" } else { "sub-area" }.into();
                self.dirty = true;
                return;
            }
            'V' => {
                let v = self.editor.area().header.orientation == 0;
                self.step(|e| {
                    e.set_vertical(v)
                        .map(|_| if v { "vertical" } else { "horizontal" }.into())
                });
                self.cam = (0.0, 0.0);
                return;
            }
            '[' | ']' => {
                let vertical = self.editor.area().header.orientation != 0;
                let (w, h) = self.editor.size();
                let step = if vertical { 7 } else { 12 };
                let d = if what == ']' { step } else { -step };
                let n = if vertical { h + d } else { w + d };
                self.step(|e| e.set_area_width(n).map(|_| String::new()));
                let (w, h) = self.editor.size();
                self.message = format!("area {w}×{h}");
                self.scroll(0.0, 0.0);
                return;
            }
            'q' => {
                let k = (self.editor.area().header.autoscroll_type + 1) % 5;
                self.step(|e| {
                    e.set_autoscroll(k);
                    Ok(String::new())
                });
                const NAMES: [&str; 5] = ["no autoscroll", "slow", "normal", "fast", "custom"];
                self.message = format!(
                    "autoscroll: {}",
                    NAMES[self.editor.area().header.autoscroll_type as usize % 5]
                );
                return;
            }
            ',' | '.' | '<' | '>' => {
                let d = match what {
                    ',' => -10,
                    '.' => 10,
                    '<' => -100,
                    _ => 100,
                };
                let t = self.editor.course.header.timer + d;
                self.step(|e| {
                    e.set_timer(t);
                    Ok(String::new())
                });
                self.message = format!("timer {}", self.editor.course.header.timer);
                return;
            }
            ';' => {
                self.condition = (self.condition + 1) % CONDITIONS.len();
                let (cat, idx) = CONDITIONS[self.condition];
                // a count condition counts what's placed
                let n = match idx {
                    1 => self
                        .editor
                        .area()
                        .objects
                        .iter()
                        .filter(|o| o.id == 0)
                        .count()
                        .max(1),
                    66 => self
                        .editor
                        .area()
                        .objects
                        .iter()
                        .filter(|o| o.id == 8)
                        .count()
                        .max(1),
                    _ => 0,
                } as i16;
                self.step(|e| e.set_clear_condition(cat, idx, n).map(|_| String::new()));
                self.message = match idx {
                    0 => "no clear condition".into(),
                    _ => format!(
                        "clear condition: {} {}{}",
                        ["", "get / defeat", "as / with", "without"][cat as usize],
                        clear::LABELS[idx as usize - 1],
                        if n > 0 {
                            format!(" ×{n}")
                        } else {
                            String::new()
                        }
                    ),
                };
                return;
            }
            '\'' => {
                let l = self.editor.liquid();
                let mode = (l.mode + 1) % 3;
                let (_, h) = self.editor.size();
                let start = if l.start == 0 { 2 } else { l.start };
                let end = (start as i32 + 6).min(h - 1) as u8;
                self.step(|e| {
                    e.set_liquid(Liquid {
                        mode,
                        speed: 1,
                        start,
                        end,
                    });
                    Ok(String::new())
                });
                self.message = format!(
                    "liquid: {}",
                    ["stays", "rises or falls", "rises and falls"][mode as usize]
                );
                return;
            }
            _ => return,
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
    }

    /// Selection keys: 'c' copy, 'x' cut, 'v' paste at the cursor, 'd' delete, and moves by
    /// (dx, dy) with 'm'. True when there was something to act on.
    pub fn selection_op(&mut self, what: char, dx: i32, dy: i32) -> bool {
        match (what, self.selection) {
            ('c', Some(r)) => {
                self.editor.copy(r);
                self.message = "copied".into();
            }
            ('x', Some(r)) => {
                self.step(|e| {
                    e.cut(r);
                    Ok("cut".into())
                });
                self.selection = None;
            }
            ('d', Some(r)) => {
                self.step(|e| Ok(format!("deleted {}", e.delete_rect(r))));
                self.selection = None;
            }
            ('v', _) => {
                let Some((tx, ty)) = self.cursor else {
                    return false;
                };
                self.step(|e| e.paste_clipboard(tx, ty).map(|_| "pasted".into()));
                if let Some(c) = &self.editor.clipboard {
                    self.selection = Some(Rect::new(tx, ty, c.size.0, c.size.1));
                }
            }
            ('m', Some(r)) => {
                self.step(|e| e.move_rect(r, dx, dy).map(|_| String::new()));
                if self.message.is_empty() {
                    self.selection = Some(Rect::new(r.x + dx, r.y + dy, r.w, r.h));
                }
            }
            _ => return false,
        }
        true
    }

    /// Forget the selection and any half-done action (Esc).
    pub fn cancel(&mut self) {
        self.selection = None;
        self.pending = None;
        self.path_edit = None;
        self.message = "cancelled".into();
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn drawing_path(&self) -> bool {
        self.path_edit.is_some()
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

    fn build(editor: &Editor) -> World {
        if editor.area == 0 {
            World::from_course(&editor.course)
        } else {
            World::with_level(Level::from_course(&editor.course, 1))
        }
    }

    /// The world to draw: the current area as built, with the selected part shown at the cursor.
    pub fn world(&mut self) -> &World {
        if self.dirty {
            self.dirty = false;
            let mut preview = None;
            if let (Some((tx, ty)), (_, tool)) = (self.cursor, self.tool())
                && matches!(tool, Tool::Ground | Tool::Part(_))
                && let Ok(bytes) = self.editor.save()
                && let Ok(mut e) = Editor::open(&bytes)
            {
                e.set_area(self.editor.area);
                let placed = match tool {
                    Tool::Ground => e.paint_ground(tx, ty).is_ok(),
                    Tool::Part(9) => e.place_pipe(tx, ty, 2, self.pipe_dir, 0).is_ok(),
                    Tool::Part(p) => e.place(p, tx, ty).is_ok(),
                    _ => false,
                };
                if placed {
                    preview = Some(Self::build(&e));
                }
            }
            self.world = preview.unwrap_or_else(|| Self::build(&self.editor));
            // nothing moves while editing: the player waits at the start
            self.world.player.hidden = self.editor.area != 0;
        }
        self.world.camera_x = self.cam.0;
        self.world.camera_y = self.cam.1;
        &self.world
    }

    /// The text strip shown while editing.
    pub fn status(&self) -> String {
        let (name, _) = self.tool();
        let area = if self.editor.area == 0 { "" } else { " SUB" };
        let cur = self
            .cursor
            .map_or(String::new(), |(x, y)| format!("  ({x},{y})"));
        let sel = self
            .selection
            .map_or(String::new(), |r| format!("  [{}×{}]", r.w, r.h));
        let msg = if self.message.is_empty() {
            "click place / right erase / Tab part / Enter play / F1 keys".to_string()
        } else {
            self.message.clone()
        };
        format!("MAKER{area}  {name}{cur}{sel}  {msg}")
    }
}

fn turn(d: Dir) -> Dir {
    match d {
        Dir::Up => Dir::Right,
        Dir::Right => Dir::Down,
        Dir::Down => Dir::Left,
        Dir::Left => Dir::Up,
    }
}

fn describe(e: &EditError) -> String {
    match e {
        EditError::OutOfBounds => "outside the area".into(),
        EditError::Occupied => "something is in the way".into(),
        EditError::Full => "the area is full".into(),
        EditError::Limit(what) => format!("part limit reached ({what})"),
        EditError::Protected => "the start / goal area stays clear".into(),
        EditError::NotAllowed(why) => (*why).into(),
        EditError::Missing => "nothing there".into(),
        EditError::Format(e) => e.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(m: &mut Maker, tool: Tool) {
        let i = PALETTE.iter().position(|p| p.1 == tool).unwrap();
        m.select(i);
    }

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

    #[test]
    fn two_click_tools_areas_and_links() {
        let mut m = Maker::new(Editor::new("M1"), None);
        pick(&mut m, Tool::Doors);
        m.hover(Some((10, 0)));
        m.click(false);
        m.hover(Some((20, 0)));
        m.click(false);
        assert_eq!(
            m.editor
                .area()
                .objects
                .iter()
                .filter(|o| o.id == 55)
                .count(),
            2
        );
        pick(&mut m, Tool::Track);
        m.hover(Some((10, 6)));
        m.click(false);
        m.hover(Some((16, 6)));
        m.click(false);
        assert_eq!(m.editor.area().tracks.len(), 3);
        // a pipe in each area, linked with L … A … L
        pick(&mut m, Tool::Part(9));
        m.hover(Some((30, 0)));
        m.click(false);
        m.modify('l');
        m.setting('a');
        assert_eq!(m.editor.area, 1);
        m.hover(Some((5, 0)));
        m.click(false);
        m.modify('l');
        assert!(m.message.contains("linked"), "{}", m.message);
        assert!(m.status().contains("SUB"));
        let _ = m.world();
        // resize and turn
        m.modify('^');
        assert_eq!(
            m.editor
                .object(m.editor.object_at(5, 0).unwrap())
                .unwrap()
                .height,
            3
        );
        m.setting('V');
        assert_eq!(m.editor.size().0, 48, "the sub-area turned vertical");
    }

    #[test]
    fn select_copy_paste_and_paths() {
        let mut m = Maker::new(Editor::new("M1"), None);
        m.select(1); // brick
        for x in 10..13 {
            m.hover(Some((x, 2)));
            m.click(false);
        }
        pick(&mut m, Tool::Select);
        m.hover(Some((10, 2)));
        m.press(false);
        m.hover(Some((12, 2)));
        m.release();
        assert_eq!(m.selection, Some(Rect::new(10, 2, 3, 1)));
        assert!(m.selection_op('c', 0, 0));
        m.hover(Some((20, 4)));
        assert!(m.selection_op('v', 0, 0));
        assert!(m.editor.object_at(22, 4).is_some());
        assert!(m.selection_op('m', 1, 1));
        assert!(m.editor.object_at(23, 5).is_some());
        assert!(m.selection_op('d', 0, 0));
        assert!(m.editor.object_at(23, 5).is_none());
        // a snake block's path drawn with the arrows
        pick(&mut m, Tool::Part(84));
        m.hover(Some((30, 6)));
        m.click(false);
        m.modify('h');
        assert!(m.drawing_path());
        for d in [(1, 0), (1, 0), (0, 1), (1, 0)] {
            m.path_key(Some(d));
        }
        m.modify('h');
        let s = m.editor.object_at(30, 6).unwrap();
        assert_eq!(m.editor.path(s), Some(vec![2, 2, 11, 12, 13]));
        // settings
        m.setting(';');
        assert!(m.message.contains("Goomba"), "{}", m.message);
        m.setting('.');
        assert_eq!(m.editor.course.header.timer, 310);
    }
}
