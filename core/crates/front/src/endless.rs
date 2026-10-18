//! The 100 Mario Challenge presented with SMM2's Endless Challenge screens (the Swift app's
//! `SMM2Endless.swift`): the rules are SMM1's (`mmk_modes`: Easy / Normal / Expert / Super
//! Expert runs of 8 / 16 / 16 / 6 courses, 100 lives, Continue, unlocks); the screens are
//! SMM2's own layouts:
//! - `Nwk_EndlessModeMenu_00`: difficulty tabs over the map part `Nwk_WorldMap_00`
//!   (TypeDifficulty per tab), New Game / Continue (`TypeStartBtn` 0 both, 1 New Game alone
//!   with the rule text); In (25) [obs layout];
//! - starting a run plays ChangeMap, then on a new run the Telop with StartMario, then the walk
//!   to the first dot (MoveMario_00); cleared dots blue (Type 0), the next one orange (Type 2,
//!   InPlay pop), the rest hidden [inf]; after a clear the dot's InClear, then the walk to the
//!   next (MoveMario_n; Scroll_00 brings the map's second half at dot 16);
//! - at the dot Mario's "Play" balloon waits for confirm, then his Decide jump and the black
//!   fade into the course; back on the map is Save & Quit (`L_EndBtn_00`);
//! - the map's counters: lives and courses cleared; the high score and the world ranking are
//!   hidden [departure];
//! - Expert / Super Expert show their tab greyed until SMM1's unlock rule opens them, and a
//!   run's last clear goes back to the menu with the unlock message [departure].

use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, MenuCursor, UiInput, hit};
use crate::smm1::{RUN_LENGTHS, RunQuery};
use crate::sound::Event;
use crate::{DrawItem, push_layout};

pub const MAP: &str = "L_WorldMap_00";
pub const TABS: [&str; 4] = ["L_Btn_00", "L_Btn_01", "L_Btn_02", "L_Btn_03"];
pub const MARIO: &str = "L_WorldMap_00/L_WorldMapMario_00";
/// Dots on the first half of the map (Scroll_00 shows the second).
pub const HALF_DOTS: usize = 16;
const NAME: &str = "Nwk_EndlessModeMenu_00";

pub fn locked_message(d: usize) -> &'static str {
    if d == 2 {
        "Clear a Normal run to unlock Expert."
    } else {
        "Clear an Expert run to unlock Super Expert."
    }
}

/// The Endless menu layout and its map part, shared by the menu and the run's map.
pub struct EndlessBoard {
    pub layout: Layout,
    /// The New Game / Continue buttons drawn from a second instance over the first: in the
    /// layout they sit under the map frame's bottom band, drawn after them [inf].
    pub buttons: Option<Layout>,
    pub difficulty: usize,
}

impl EndlessBoard {
    pub fn new(ui: &Ui) -> Option<Self> {
        let mut l = ui.layout(NAME)?;
        let mut buttons = ui.layout(NAME);
        if let Some(b) = &mut buttons {
            for p in [
                "P_BG_00",
                "N_Scissor_00",
                "N_ChangeMap_00",
                "N_ChangeMap_01",
            ] {
                b.show(p, false);
            }
        }
        l.show("N_StartBtnMap_00", false);
        l.text("T_Title_00", "100 Mario Challenge");
        l.play_loop(&format!("{MAP}/Loop"), 0);
        l.play_loop(&format!("{MAP}/LoopRandom"), 0);
        // the network's record and ranking panes; the map's own Quit key while choosing
        for p in [
            "N_Ranking_00",
            "N_BreakRecord_00",
            "N_BestClearCount_00",
            "T_Ranking_00",
            "L_WorldMapBtn_00",
        ] {
            l.show(&format!("{MAP}/{p}"), false);
        }
        for t in TABS {
            l.hold(&format!("{t}/Select"), 0.0);
        }
        Some(Self {
            layout: l,
            buttons,
            difficulty: 0,
        })
    }

    /// Both instances (animations of the whole screen).
    pub fn both(&mut self, mut f: impl FnMut(&mut Layout)) {
        f(&mut self.layout);
        if let Some(b) = &mut self.buttons {
            f(b);
        }
    }

    /// The layer holding the start buttons.
    pub fn button_layer(&mut self) -> &mut Layout {
        match &mut self.buttons {
            Some(b) => b,
            None => &mut self.layout,
        }
    }

    pub fn draw(&mut self, tick: i32, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, tick);
        if let Some(b) = &mut self.buttons {
            push_layout(out, b, tick);
        }
    }

    pub fn dot(i: usize) -> String {
        format!("{MAP}/L_WorldMapCourse_{i:02}")
    }

    pub fn info(i: usize) -> String {
        format!("{MAP}/L_WorldMapInfo_{i:02}")
    }

    pub fn set_difficulty(&mut self, d: usize) {
        self.difficulty = d;
        self.layout.hold(&format!("{MAP}/TypeDifficulty"), d as f32);
    }

    /// Cleared dots blue and small, the next one orange, later ones hidden [inf].
    pub fn set_dots(&mut self, cleared: usize, total: usize, current: bool) {
        let l = &mut self.layout;
        for i in 0..32 {
            let d = Self::dot(i);
            l.show(&Self::info(i), false);
            if i < cleared {
                l.visible(&d, None);
                l.stop(&format!("{d}/InPlay"));
                l.stop(&format!("{d}/InClear"));
                l.hold(&format!("{d}/Type"), 0.0);
            } else if i == cleared && i < total && current {
                l.visible(&d, None);
                l.hold(&format!("{d}/Type"), 2.0);
            } else {
                l.show(&d, false);
            }
        }
    }

    /// Mario standing on dot `i` (None: still in the castle), the map scrolled to its half.
    pub fn place_mario(&mut self, i: Option<usize>) {
        let l = &mut self.layout;
        for k in 0..=32 {
            l.stop(&format!("{MAP}/MoveMario_{k:02}"));
        }
        l.stop(&format!("{MAP}/StartMario"));
        match i {
            Some(i) => {
                let a = format!("{MAP}/MoveMario_{:02}", i.min(31));
                l.hold_end(&a);
                l.hold(&format!("{MARIO}/Stop"), 0.0);
            }
            None => l.hold(&format!("{MAP}/StartMario"), 0.0),
        }
        self.scroll(i.unwrap_or(0), false, 0);
    }

    pub fn scroll(&mut self, i: usize, animate: bool, tick: i32) {
        let a = format!("{MAP}/Scroll_00");
        if i >= HALF_DOTS {
            if animate {
                self.layout.play(&a, tick);
            } else {
                self.layout.hold_end(&a);
            }
        } else {
            self.layout.stop(&a);
        }
    }

    /// The map's counters: lives left and courses cleared.
    pub fn set_counters(&mut self, lives: u32, cleared: usize) {
        let l = &mut self.layout;
        l.text(&format!("{MAP}/T_RestMarioNum_00"), &lives.to_string());
        for c in ["L_ClearCourseCount_00", "L_ClearCourseCount_01"] {
            l.text(&format!("{MAP}/{c}/T_Count_00"), &cleared.to_string());
        }
    }
}

/// What the Endless screens ask of the run.
#[derive(Debug)]
pub enum EndlessEvent {
    Exit,
    Locked(usize),
    /// New Game / Continue decided (after the button's Decide): start the run.
    Start {
        difficulty: usize,
        resume: bool,
    },
    /// Save & Quit on the map: back to the menu (with the black fade).
    BackToSelect,
    /// Mario jumped: fade into the course.
    Play,
}

// MARK: - difficulty select

/// Difficulty tabs, New Game / Continue over the map. ←/→ change the difficulty, ↑/↓ move
/// between the tab row and the buttons, confirm starts, back leaves.
pub struct EndlessMenuScene {
    ui: Ui,
    pub board: Option<EndlessBoard>,
    cursor: MenuCursor,
    pub tick: i32,
    pub difficulty: usize,
    /// 0 New Game, 1 Continue.
    button: usize,
    on_tabs: bool,
    busy_until: i32,
    started: Option<(i32, bool)>,
}

impl EndlessMenuScene {
    pub fn new(ui: &Ui, difficulty: usize, q: &dyn RunQuery) -> Option<Self> {
        let mut b = EndlessBoard::new(ui)?;
        b.both(|l| l.play("In", 0));
        b.layout.show(&format!("{MAP}/L_EndBtn_00"), false);
        let busy = b.layout.length("In") as i32;
        let mut s = Self {
            ui: ui.clone(),
            board: Some(b),
            cursor: MenuCursor::new(ui),
            tick: 0,
            difficulty,
            button: 0,
            on_tabs: false,
            busy_until: busy,
            started: None,
        };
        s.refresh(q);
        Some(s)
    }

    fn board(&mut self) -> &mut EndlessBoard {
        self.board
            .as_mut()
            .expect("the board until the run takes it")
    }

    /// Tabs, buttons and the map for the current difficulty (the Continue record's dots).
    fn refresh(&mut self, q: &dyn RunQuery) {
        let d = self.difficulty;
        let unlocked = q.unlocked(d);
        let can_continue = unlocked && q.can_continue(d);
        let cont = if can_continue {
            q.continue_info(d)
        } else {
            None
        };
        if !can_continue {
            self.button = 0;
        }
        let (button, on_tabs) = (self.button, self.on_tabs);
        let b = self.board();
        b.set_difficulty(d);
        for (k, t) in TABS.iter().enumerate() {
            let a = format!("{t}/Decide");
            if k == d {
                b.layout.hold_end(&a); // the chosen tab stays dark [inf]
            } else {
                b.layout.stop(&a);
            }
            // locked tiers greyed (SMM1's unlock rule) [departure]
            b.layout
                .set_alpha(t, if q.unlocked(k) { 255.0 } else { 110.0 });
        }
        b.both(|l| l.hold("TypeStartBtn", if can_continue { 0.0 } else { 1.0 }));
        let rule = format!(
            "{} courses, {}",
            RUN_LENGTHS[d],
            if unlocked {
                "100 lives"
            } else {
                locked_message(d)
            }
        );
        let bl = b.button_layer();
        bl.text("T_Rule_00", &rule);
        for (i, p) in ["L_StartBtn_00", "L_StartBtn_01"].iter().enumerate() {
            let a = format!("{p}/Select");
            let f = if i == button && !on_tabs {
                bl.length(&a)
            } else {
                0.0
            };
            bl.hold(&a, f);
            // locked: the buttons' Invalid look (bound last, over Select) [departure]
            let inv = format!("{p}/Invalid");
            if unlocked {
                bl.stop(&inv);
            } else {
                bl.hold(&inv, 0.0);
            }
        }
        b.set_dots(cont.map_or(0, |c| c.1), RUN_LENGTHS[d], cont.is_some());
        b.place_mario(cont.map(|c| c.1.saturating_sub(1)));
        b.set_counters(cont.map_or(100, |c| c.0), cont.map_or(0, |c| c.1));
    }

    pub fn step(&mut self, input: &UiInput, q: &dyn RunQuery) -> Option<EndlessEvent> {
        self.tick += 1;
        let mut ev = None;
        if let Some((at, resume)) = self.started
            && self.tick == at
        {
            ev = Some(EndlessEvent::Start {
                difficulty: self.difficulty,
                resume,
            });
        }
        if self.tick >= self.busy_until && self.started.is_none() {
            let mut pressed = input.pressed;
            if let Some(p) = input.click {
                pressed = pressed | self.click(p, q);
            }
            ev = ev.or(self.handle(pressed, q));
        }
        let alpha = if self.started.is_some() { 0.0 } else { 1.0 };
        let tick = self.tick;
        if let Some(b) = &mut self.board {
            if self.on_tabs {
                let pane = format!("{}/B_Btn_00", TABS[self.difficulty]);
                self.cursor.place(&mut b.layout, &pane, tick, alpha);
            } else {
                let pane = format!("L_StartBtn_0{}/B_Btn_00", self.button);
                let l = match &mut b.buttons {
                    Some(x) => x,
                    None => &mut b.layout,
                };
                self.cursor.place(l, &pane, tick, alpha);
            }
        }
        ev
    }

    /// A click: a difficulty tab (chooses it), New Game / Continue (decides it). The buttons to
    /// add as if pressed.
    fn click(&mut self, p: [f32; 2], q: &dyn RunQuery) -> Buttons {
        let tick = self.tick;
        let tabs: Vec<String> = TABS.iter().map(|t| format!("{t}/B_Btn_00")).collect();
        if let Some(d) = hit(&mut self.board().layout, tick, &tabs, p) {
            self.on_tabs = true;
            self.choose(d, q);
            return Buttons::NONE;
        }
        let can_continue = q.unlocked(self.difficulty) && q.can_continue(self.difficulty);
        let starts = [
            "L_StartBtn_00/B_Btn_00".to_string(),
            "L_StartBtn_01/B_Btn_00".to_string(),
        ];
        if let Some(b) = hit(self.board().button_layer(), tick, &starts, p)
            && (b == 0 || can_continue)
        {
            self.on_tabs = false;
            self.button = b;
            self.refresh(q);
            return Buttons::CONFIRM;
        }
        Buttons::NONE
    }

    /// Change to difficulty `d` (the tab's Select, the map's change) [inf].
    fn choose(&mut self, d: usize, q: &dyn RunQuery) {
        if d == self.difficulty {
            self.refresh(q);
            return;
        }
        let tick = self.tick;
        self.difficulty = d;
        self.ui.sound_button(NAME, TABS[d], Event::Decide);
        let l = &mut self.board().layout;
        l.play(&format!("{}/Select", TABS[d]), tick);
        // the map fades over to the other difficulty's (ChangeOut / ChangeIn) [inf]
        l.play("ChangeOut", tick);
        l.play("ChangeIn", tick + 5);
        self.refresh(q);
    }

    fn handle(&mut self, p: Buttons, q: &dyn RunQuery) -> Option<EndlessEvent> {
        let ui = self.ui.clone();
        let tick = self.tick;
        if p.has(Buttons::BACK) {
            ui.sound_back(NAME, "normal");
            return Some(EndlessEvent::Exit);
        }
        if p.has(Buttons::UP) && !self.on_tabs {
            ui.sound_button(NAME, TABS[self.difficulty], Event::Select);
            self.on_tabs = true;
            self.refresh(q);
            return None;
        }
        if p.has(Buttons::DOWN) && self.on_tabs {
            ui.sound_button(NAME, &format!("L_StartBtn_0{}", self.button), Event::Select);
            self.on_tabs = false;
            self.refresh(q);
            return None;
        }
        let can_continue = q.unlocked(self.difficulty) && q.can_continue(self.difficulty);
        if p.has(Buttons::LEFT) || p.has(Buttons::RIGHT) {
            let right = p.has(Buttons::RIGHT);
            if self.on_tabs {
                let d = if right {
                    (self.difficulty + 1).min(3)
                } else {
                    self.difficulty.saturating_sub(1)
                };
                if d == self.difficulty {
                    return None;
                }
                self.choose(d, q);
            } else if can_continue {
                self.button = if right { 1 } else { 0 };
                let b = format!("L_StartBtn_0{}", self.button);
                ui.sound_button(NAME, &b, Event::Select);
                self.board()
                    .button_layer()
                    .play(&format!("{b}/Select"), tick);
                self.refresh(q);
            }
            return None;
        }
        if p.has(Buttons::CONFIRM) {
            if self.on_tabs {
                self.on_tabs = false;
                self.refresh(q);
                return None;
            }
            if !q.unlocked(self.difficulty) {
                return Some(EndlessEvent::Locked(self.difficulty));
            }
            let resume = self.button == 1 && can_continue;
            let b = format!("L_StartBtn_0{}", self.button);
            ui.sound_button(NAME, &b, Event::Decide);
            let bl = self.board().button_layer();
            bl.play(&format!("{b}/Decide"), tick);
            let wait = bl.length("L_StartBtn_00/Decide") as i32;
            self.started = Some((tick + wait.max(1), resume));
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if let Some(b) = &mut self.board {
            b.draw(t, out);
        }
        self.cursor.draw(t, out);
    }
}

// MARK: - the run's map

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapEntry {
    NewGame,
    Resume,
    AfterClear,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Sound(&'static str, Option<usize>),
    /// Walk to the dot.
    Arrive {
        at: usize,
        walk_from: Option<usize>,
    },
    Balloon,
    FadeIn,
}

/// Before each course: the map (ChangeMap from the menu, the Telop and Mario out of the castle
/// on a new run, or the clear pop and the walk to the next dot), Mario's "Play" balloon,
/// confirm → his jump and the black fade into the course. Back = Save & Quit.
pub struct EndlessMapScene {
    ui: Ui,
    pub board: EndlessBoard,
    pub tick: i32,
    theme: Option<u8>,
    waiting: bool,
    leaving: bool,
    actions: Vec<(i32, Action)>,
}

impl EndlessMapScene {
    /// `cleared`: courses cleared before this one (this course's dot).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        entry: MapEntry,
        difficulty: usize,
        lives: u32,
        cleared: usize,
        theme: Option<u8>,
        handoff: Option<EndlessBoard>,
    ) -> Option<Self> {
        let mut b = match handoff.filter(|h| h.difficulty == difficulty) {
            Some(h) => h,
            None => EndlessBoard::new(ui)?,
        };
        let total = RUN_LENGTHS[difficulty];
        b.set_difficulty(difficulty);
        b.set_counters(lives, cleared);
        let l = &mut b.layout;
        l.visible(&format!("{MAP}/L_EndBtn_00"), None);
        // the key glyph: B, the back button here (the layout shows X) [departure]
        l.text(&format!("{MAP}/L_EndBtn_00/T_KeyBtn_00"), "\u{E0E1}");
        l.text_from(
            &format!("{MAP}/L_EndBtn_00/T_Btn_00"),
            "L_WorldMap_00-L_EndBtn_00-T_Btn_00",
        );
        l.text_from(
            &format!("{MARIO}/T_Challenge_00"),
            "L_WorldMap_00-L_WorldMapMario_00-T_Challenge_00",
        );
        l.text_from(&format!("{MAP}/T_Telop_00"), "L_WorldMap_00-T_Telop_00");
        let map_in = l.length("ChangeMap") as i32;
        let mut s = Self {
            ui: ui.clone(),
            board: b,
            tick: 0,
            theme,
            waiting: false,
            leaving: false,
            actions: Vec::new(),
        };
        match entry {
            MapEntry::NewGame => {
                s.board.both(|l| l.play("ChangeMap", 0));
                s.board.set_dots(0, total, false);
                s.board.place_mario(None);
                // the Telop with Mario popping out of the castle, then the walk to the first
                // dot [inf: order]
                let l = &mut s.board.layout;
                l.play(&format!("{MAP}/Telop"), map_in);
                l.play(&format!("{MAP}/StartMario"), map_in);
                let walk = map_in + l.length(&format!("{MAP}/Telop")) as i32;
                s.at(map_in, Action::Sound("Telop", None));
                s.at(
                    walk,
                    Action::Arrive {
                        at: 0,
                        walk_from: None,
                    },
                );
            }
            MapEntry::Resume => {
                s.board.both(|l| l.play("ChangeMap", 0));
                s.board.set_dots(cleared, total, false);
                s.board
                    .place_mario(if cleared > 0 { Some(cleared - 1) } else { None });
                s.at(
                    map_in + 10,
                    Action::Arrive {
                        at: cleared,
                        walk_from: cleared.checked_sub(1),
                    },
                );
            }
            MapEntry::AfterClear => {
                s.board.both(|l| l.hold_end("ChangeMap"));
                s.board.set_dots(cleared.saturating_sub(1), total, true);
                s.board.place_mario(Some(cleared.saturating_sub(1)));
                // the cleared dot pops to blue, then Mario walks on [inf: timing]
                let d = EndlessBoard::dot(cleared.saturating_sub(1));
                let l = &mut s.board.layout;
                l.stop(&format!("{d}/Type"));
                l.play(&format!("{d}/InClear"), 30);
                let pop = l.length(&format!("{d}/InClear")) as i32;
                s.at(30, Action::Sound("InClear", None));
                s.at(
                    30 + pop + 10,
                    Action::Arrive {
                        at: cleared,
                        walk_from: cleared.checked_sub(1),
                    },
                );
            }
        }
        Some(s)
    }

    fn at(&mut self, t: i32, a: Action) {
        self.actions.push((t, a));
    }

    /// Walk to dot `i` (MoveMario_i), the dot's InPlay pop and the theme above it, then the
    /// "Play" balloon.
    fn arrive(&mut self, i: usize, walk_from: Option<usize>) {
        let mut t = self.tick;
        if i == HALF_DOTS {
            self.board.scroll(i, true, t);
            t += self.board.layout.length(&format!("{MAP}/Scroll_00")) as i32;
        }
        let d = EndlessBoard::dot(i);
        let l = &mut self.board.layout;
        l.visible(&d, None);
        l.hold(&format!("{d}/Type"), 2.0);
        l.play(&format!("{d}/InPlay"), t);
        let pop = t;
        let mv = format!("{MAP}/MoveMario_{:02}", i.min(31));
        if walk_from.is_some() || i == 0 {
            l.play_loop(&format!("{MARIO}/Walk"), t);
            l.play(&mv, t);
            t += l.length(&mv) as i32;
        } else {
            l.hold_end(&mv);
        }
        if let Some(theme) = self.theme {
            let info = EndlessBoard::info(i);
            l.visible(&info, None);
            l.hold(&format!("{info}/SceneSkin"), theme.min(9) as f32);
            l.play(&format!("{info}/InPlay"), t);
        }
        self.at(pop, Action::Sound("InPlay", Some(i)));
        self.at(t, Action::Balloon);
    }

    pub fn step(&mut self, input: &UiInput) -> Option<EndlessEvent> {
        self.tick += 1;
        let tick = self.tick;
        let mut due: Vec<Action> = Vec::new();
        self.actions.retain(|(at, a)| {
            if *at <= tick {
                due.push(*a);
                false
            } else {
                true
            }
        });
        let mut ev = None;
        for a in due {
            match a {
                Action::Sound(anim, dot) => {
                    let pane = dot.map_or(MAP.to_string(), EndlessBoard::dot);
                    self.ui.sound_anim(NAME, anim, Some(&pane));
                }
                Action::Arrive { at, walk_from } => self.arrive(at, walk_from),
                Action::Balloon => {
                    let l = &mut self.board.layout;
                    l.stop(&format!("{MARIO}/Walk"));
                    l.play(&format!("{MARIO}/Select"), tick);
                    self.ui.sound_button(NAME, MARIO, Event::Select);
                    self.waiting = true;
                }
                Action::FadeIn => ev = Some(EndlessEvent::Play),
            }
        }
        if ev.is_some() || self.leaving {
            return ev;
        }
        let mut p = input.pressed;
        if let Some(c) = input.click {
            let quit = [format!("{MAP}/L_EndBtn_00/B_Btn_00")];
            p = p | if hit(&mut self.board.layout, tick, &quit, c).is_some() {
                Buttons::BACK
            } else {
                Buttons::CONFIRM
            };
        }
        if p.has(Buttons::BACK) {
            // Quit on the map: the run stays saved for Continue
            self.leaving = true;
            let end = format!("{MAP}/L_EndBtn_00");
            self.ui.sound_button(NAME, &end, Event::Decide);
            self.board.layout.play(&format!("{end}/Decide"), tick);
            return Some(EndlessEvent::BackToSelect);
        }
        if self.waiting && p.has(Buttons::CONFIRM) {
            self.waiting = false;
            self.leaving = true;
            let l = &mut self.board.layout;
            l.play(&format!("{MARIO}/Decide"), tick);
            self.ui.sound_anim(NAME, "Decide", Some(MARIO));
            // Mario's jump (Decide, 130 frames), then the fade into the course [inf]
            let jump = l.length(&format!("{MARIO}/Decide")) as i32;
            self.at(tick + (jump - 40).max(30), Action::FadeIn);
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        self.board.draw(self.tick, out);
    }
}
