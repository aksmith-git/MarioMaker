//! The SMM2 title screen and its Play menu (`Play_Title_00`), over one of the game's title
//! courses running a scripted demo (the Swift app's `SMM2Title.swift`).
//!
//! The layout holds every state; its own animations move between them [obs data]:
//!   top       logo + "Press [L] + [R]" prompt blinking (LoopPressKey)
//!   PressKey  top → main: the prompt fades out, the Make / Play clapperboards fade in
//!   main      L_EditBtn_00 "Make", L_PlayBtn_00 "Play"
//!   DecidePlayBtn   main → category; DecideBackBtn  category → main;
//!   DecideBackBtnTop main → top
//!   category  L_QuestBtn_00, L_NetworkBtn_00, L_CourseRobotPlayBtn_00
//!   Type      logo letters black (frame 0) or white (frame 1)
//! From the game's title page [obs]: the prompt puts the cursor on Make; the Play menu opens with
//! the cursor on Story Mode and going back from it puts it on Play; the choices change scene
//! with the black fade. Inferred [inf]: the logo colour by the course's sky, the cursor
//! (`Cmn_Cursor_00` sized to the selected button's N_Carsor_00 / B_Btn_00), how long a
//! button's Decide plays before its choice acts. Departures: a fourth Play-menu button "100
//! Mario Challenge" (a copy of the Coursebot button, 116 below it, with the castle icon); Make
//! plays its clap and says "coming soon".

use crate::host::{Host, buttons};
use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, Script, UiInput};
use crate::sound::Event;
use crate::{DrawItem, push_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleChoice {
    StoryMode,
    CourseWorld,
    Coursebot,
    HundredMario,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Top,
    Main,
    Category,
    Leaving,
}

/// What a title step asks of its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleEvent {
    Choice(TitleChoice),
    ComingSoon,
}

pub const MAIN_BUTTONS: [&str; 2] = ["L_EditBtn_00", "L_PlayBtn_00"];
pub const CATEGORY_BUTTONS: [&str; 4] = [
    "L_QuestBtn_00",
    "L_NetworkBtn_00",
    "L_CourseRobotPlayBtn_00",
    "L_HundredBtn_00",
];
const CATEGORY_CHOICES: [TitleChoice; 4] = [
    TitleChoice::StoryMode,
    TitleChoice::CourseWorld,
    TitleChoice::Coursebot,
    TitleChoice::HundredMario,
];
const LAYOUT: &str = "Play_Title_00";

/// The pane whose sound type a button uses (the added 100 Mario button sounds like the
/// Coursebot one it copies).
fn sound_pane(b: &str) -> &str {
    if b == "L_HundredBtn_00" {
        "L_CourseRobotPlayBtn_00"
    } else {
        b
    }
}

pub struct TitleScene {
    ui: Ui,
    pub title: Layout,
    pub cursor: Option<Layout>,
    pub tick: i32,
    pub state: State,
    /// 0 Make, 1 Play; the prompt puts the cursor on Make [obs].
    pub main_selection: usize,
    pub category_selection: usize,
    busy_until: i32,
    pending: Option<(TitleChoice, i32)>,
    /// Captures: presses at scene ticks (the Swift app's `MMK_UI_INPUT`).
    pub script: Script,
}

impl TitleScene {
    pub fn new(ui: &Ui, start: State, version: &str) -> Option<Self> {
        let mut t = ui.layout(LAYOUT)?;
        let cursor = ui.layout("Cmn_Cursor_00");
        let state = if start == State::Leaving {
            State::Main
        } else {
            start
        };
        t.text("T_Num_00", version);
        // the fourth Play-menu button (departure, see the module notes)
        if t.clone_part("L_CourseRobotPlayBtn_00", "L_HundredBtn_00", [0.0, -116.0]) {
            t.text("L_HundredBtn_00/T_Btn_00", "100 Mario Challenge");
            t.show("L_HundredBtn_00/N_RobotIcon_00", false);
            t.show("L_HundredBtn_00/N_QuestIcon_01", true);
            // four buttons don't fit between the logo (bottom y 80) and the copyright line
            // (top y −313) at full size: the list at 80%, its top at y 70 (layout units)
            t.set_pane(
                "N_CategoryPlay_00",
                mmk_ui::PaneOverride {
                    translate: Some([0.0, 34.0]),
                    scale: Some([0.8, 0.8]),
                    ..Default::default()
                },
            );
        }
        // "new" badges: nothing is new here
        for b in CATEGORY_BUTTONS {
            t.show(&format!("{b}/L_NewIcon_00"), false);
        }
        let mut s = Self {
            ui: ui.clone(),
            title: t,
            cursor,
            tick: 0,
            state,
            main_selection: 0,
            category_selection: 0,
            busy_until: 0,
            pending: None,
            script: Script::default(),
        };
        s.set_dark(false);
        s.title.play_loop("LoopPressKey", 0);
        for b in MAIN_BUTTONS.iter().chain(&CATEGORY_BUTTONS) {
            s.title.hold(&format!("{b}/Select"), 0.0);
        }
        if let Some(c) = &mut s.cursor {
            c.play_loop("Loop", 0);
        }
        match state {
            State::Main => s.title.hold_end("PressKey"),
            State::Category => {
                s.title.hold_end("PressKey");
                s.title.hold_end("DecidePlayBtn");
            }
            _ => {}
        }
        s.select_main(s.main_selection);
        s.select_category(s.category_selection, false);
        s.place_cursor();
        Some(s)
    }

    /// Logo letters white over a dark course, black over a light one.
    pub fn set_dark(&mut self, dark: bool) {
        self.title.hold("Type", if dark { 1.0 } else { 0.0 });
    }

    /// Start the Play menu with button `i` selected (coming back from a mode, captures).
    pub fn select_category_now(&mut self, i: usize) {
        self.select_category(i.min(CATEGORY_BUTTONS.len() - 1), false);
        self.place_cursor();
    }

    pub fn step(&mut self, input: &UiInput) -> Option<TitleEvent> {
        self.tick += 1;
        let mut ev = None;
        if let Some((c, at)) = self.pending
            && self.tick >= at
        {
            self.pending = None;
            ev = Some(TitleEvent::Choice(c));
        }
        let pressed = input.pressed | self.script.at(self.tick);
        if self.tick >= self.busy_until
            && let Some(e) = self.handle(pressed, input.click)
        {
            ev = Some(e);
        }
        self.place_cursor();
        ev
    }

    fn handle(&mut self, mut pressed: Buttons, click: Option<[f32; 2]>) -> Option<TitleEvent> {
        // a click on a button selects it and decides it
        if let Some(p) = click {
            self.title.apply(self.tick);
            match self.state {
                State::Top => pressed = pressed | Buttons::CONFIRM,
                State::Main => {
                    if let Some(i) = (0..2).find(|&i| {
                        self.title
                            .rect(&format!("{}/N_Carsor_00", MAIN_BUTTONS[i]))
                            .is_some_and(|r| r.visible && r.contains(p))
                    }) {
                        self.select_main(i);
                        pressed = pressed | Buttons::CONFIRM;
                    }
                }
                State::Category => {
                    if let Some(i) = (0..CATEGORY_BUTTONS.len()).find(|&i| {
                        self.title
                            .rect(&format!("{}/B_Btn_00", CATEGORY_BUTTONS[i]))
                            .is_some_and(|r| r.visible && r.contains(p))
                    }) {
                        self.select_category(i, false);
                        pressed = pressed | Buttons::CONFIRM;
                    }
                }
                State::Leaving => {}
            }
        }
        let tick = self.tick;
        match self.state {
            State::Top => {
                // "Press L + R": any menu button brings the menu up
                if !pressed.is_empty() {
                    self.enter_main(State::Top);
                }
            }
            State::Main => {
                if pressed.has(Buttons::LEFT) || pressed.has(Buttons::RIGHT) {
                    let i = if pressed.has(Buttons::LEFT) { 0 } else { 1 };
                    if i != self.main_selection {
                        self.ui.sound_button(LAYOUT, MAIN_BUTTONS[i], Event::Select);
                    }
                    self.select_main(i);
                } else if pressed.has(Buttons::CONFIRM) {
                    let b = MAIN_BUTTONS[self.main_selection];
                    self.ui.sound_button(LAYOUT, b, Event::Decide);
                    let decide = format!("{b}/Decide");
                    self.title.play(&decide, tick);
                    let clap = self.title.length(&decide) as i32;
                    if self.main_selection == 0 {
                        self.busy_until = tick + clap;
                        return Some(TitleEvent::ComingSoon);
                    }
                    // the clap, then the menu swaps to the category buttons; the Play menu
                    // opens on Story Mode [obs]
                    self.restart("DecidePlayBtn", tick + clap);
                    self.state = State::Category;
                    self.select_category(0, false);
                    self.busy_until = tick + clap + self.title.length("DecidePlayBtn") as i32;
                } else if pressed.has(Buttons::BACK) {
                    self.ui.sound_back(LAYOUT, "title");
                    self.title.stop("PressKey");
                    self.restart("DecideBackBtnTop", tick);
                    self.state = State::Top;
                    self.busy_until = tick + self.title.length("DecideBackBtnTop") as i32;
                    self.main_selection = 0; // Wait: the cursor goes back to Make [obs]
                }
            }
            State::Category => {
                let n = CATEGORY_BUTTONS.len();
                if pressed.has(Buttons::UP) || pressed.has(Buttons::DOWN) {
                    let step = if pressed.has(Buttons::UP) { n - 1 } else { 1 };
                    let i = (self.category_selection + step) % n;
                    self.ui
                        .sound_button(LAYOUT, sound_pane(CATEGORY_BUTTONS[i]), Event::Select);
                    self.select_category(i, true);
                } else if pressed.has(Buttons::CONFIRM) {
                    let b = CATEGORY_BUTTONS[self.category_selection];
                    self.ui.sound_button(LAYOUT, sound_pane(b), Event::Decide);
                    self.title.play(&format!("{b}/Decide"), tick);
                    self.state = State::Leaving;
                    // the button's flash (frames 0–8 of Decide), then the next screen [inf]
                    self.pending = Some((CATEGORY_CHOICES[self.category_selection], tick + 24));
                } else if pressed.has(Buttons::BACK) {
                    self.ui.sound_back(LAYOUT, "title");
                    self.main_selection = 1; // back from the Play menu: the cursor on Play [obs]
                    self.enter_main(State::Category);
                }
            }
            State::Leaving => {}
        }
        None
    }

    fn enter_main(&mut self, from: State) {
        let tick = self.tick;
        if from == State::Top {
            self.ui.sound_anim(LAYOUT, "PressKey", None);
            self.title.stop("DecideBackBtnTop");
            self.restart("PressKey", tick);
            self.busy_until = tick + self.title.length("PressKey") as i32;
        } else {
            self.restart("DecideBackBtn", tick);
            self.busy_until = tick + self.title.length("DecideBackBtn") as i32;
        }
        self.state = State::Main;
        self.select_main(self.main_selection);
    }

    /// Re-bind `anim` last, so it wins over the transitions bound before it.
    fn restart(&mut self, anim: &str, t: i32) {
        self.title.stop(anim);
        self.title.play(anim, t);
    }

    fn select_main(&mut self, i: usize) {
        self.main_selection = i;
        for (k, b) in MAIN_BUTTONS.iter().enumerate() {
            // Select's last frame: the clapper bar open
            let a = format!("{b}/Select");
            let f = if k == i { self.title.length(&a) } else { 0.0 };
            self.title.hold(&a, f);
        }
    }

    fn select_category(&mut self, i: usize, animate: bool) {
        self.category_selection = i;
        for (k, b) in CATEGORY_BUTTONS.iter().enumerate() {
            let a = format!("{b}/Select");
            if k == i {
                if animate {
                    self.title.play(&a, self.tick);
                } else {
                    self.title.hold_end(&a);
                }
            } else {
                self.title.hold(&a, 0.0);
            }
        }
    }

    /// The cursor frames the selected button and fades with it.
    fn place_cursor(&mut self) {
        let Some(cursor) = &mut self.cursor else {
            return;
        };
        let target = match self.state {
            State::Top => None,
            State::Main => Some(format!("{}/N_Carsor_00", MAIN_BUTTONS[self.main_selection])),
            State::Category | State::Leaving => Some(format!(
                "{}/B_Btn_00",
                CATEGORY_BUTTONS[self.category_selection]
            )),
        };
        self.title.apply(self.tick);
        let r = target
            .and_then(|t| self.title.rect(&t))
            .filter(|r| r.visible);
        let Some(r) = r else {
            cursor.set_alpha("N_All_00", 0.0);
            return;
        };
        cursor.set_pane(
            "W_SelectFrame_00",
            mmk_ui::PaneOverride {
                translate: Some(r.center),
                size: Some(r.size),
                ..Default::default()
            },
        );
        let k = if self.state == State::Leaving {
            0.0
        } else {
            1.0
        };
        cursor.set_alpha("N_All_00", 255.0 * r.alpha * k);
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.title, self.tick);
        if let Some(c) = &mut self.cursor {
            push_layout(out, c, self.tick);
        }
    }
}

/// Demo length per course before moving on (engine ticks).
pub const DEMO_TICKS: u64 = 60 * 40;

/// Scripted demo: walk right, a jump every two seconds [inf: stands in for a player].
pub fn demo_input(tick: u64) -> u32 {
    let mut b = buttons::RIGHT;
    if tick < 60 {
        b = 0; // stand still for a second first
    }
    if tick % 120 >= 90 && tick % 120 < 115 {
        b |= buttons::JUMP;
    }
    b
}

/// The title screen: the scene and the demo course running behind it (when the host has the
/// title courses).
pub struct Title {
    pub scene: TitleScene,
    demo: Option<Demo>,
}

struct Demo {
    courses: usize,
    index: usize,
    tick: u64,
    was_dying: bool,
}

impl Title {
    /// `course`: start with title course n (captures), else a random one.
    pub fn new(ui: &Ui, start: State, course: Option<usize>, host: &mut dyn Host) -> Option<Self> {
        let scene = TitleScene::new(ui, start, &ui.build())?;
        let n = host.title_courses();
        let mut demo = None;
        if n > 0 {
            let index = course.unwrap_or_else(|| host.random() as usize) % n;
            if host.load_title_course(index) {
                demo = Some(Demo {
                    courses: n,
                    index,
                    tick: 0,
                    was_dying: false,
                });
            }
        }
        let mut t = Self { scene, demo };
        t.update_logo(host);
        Some(t)
    }

    /// A course plays behind the title.
    pub fn has_course(&self) -> bool {
        self.demo.is_some()
    }

    pub fn step(&mut self, input: &UiInput, host: &mut dyn Host) -> Option<TitleEvent> {
        let ev = self.scene.step(input);
        if let Some(d) = &mut self.demo {
            host.step_course(Some(demo_input(d.tick)));
            d.tick += 1;
            let s = host.course_status();
            // the end of a death sequence, a clear or the demo's length moves on
            let ended = (d.was_dying && !s.dying) || s.cleared || d.tick >= DEMO_TICKS;
            d.was_dying = s.dying;
            if ended {
                d.index = (d.index + 1) % d.courses;
                if !host.load_title_course(d.index) {
                    host.restart_course();
                }
                d.tick = 0;
                d.was_dying = false;
                self.update_logo(host);
            }
        }
        ev
    }

    /// White letters over a dark sky (underground, castle, night…), black over a light one.
    fn update_logo(&mut self, host: &mut dyn Host) {
        if self.demo.is_none() {
            return;
        }
        let c = host.course_status().clear_color;
        let ch = |s: u32| ((c >> s) & 0xff) as f64 / 255.0;
        let lum = 0.2126 * ch(24) + 0.7152 * ch(16) + 0.0722 * ch(8);
        self.scene.set_dark(lum < 0.35);
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        if self.demo.is_some() {
            out.push(DrawItem::Course);
        }
        self.scene.draw(out);
    }
}
