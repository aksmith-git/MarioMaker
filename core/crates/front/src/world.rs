//! Course World with SMM2's own layouts, over the local library and the open-source course
//! server (never Nintendo's network; the Swift app's `SMM2CourseWorld.swift`):
//! - the hub `Nwk_Title_00` (space background with its LoopRandom events, the buttons Courses,
//!   Leaderboards, Network Play, Endless Challenge, Ninji Speedruns, Super Worlds) [obs layout];
//! - Courses → `Nwk_CourseListGeneral_00`: the tabs (Hot / Popular / New), rows
//!   `Nwk_CourseData_00` (name, style logo, maker, plays, likes, date, clear mark); confirm opens
//!   a row (its Toggle animation: clear rate, clears / attempts, best time, course ID, Play) and
//!   Play starts the course with the black fade;
//! - Endless Challenge → the Endless menu (100 Mario rules, see `endless`).
//!
//! Scene changes use the black fade [obs]. What each tab lists, the cursor's start, which
//! buttons are unavailable without Nintendo's network (a message), and the hidden parts without
//! data (Mii, flags, badges, thumbnails, comments, world ranks, ID search, the player card) are
//! ours [inf / departure]. Departure of the portable front end: Story Mode and Coursebot open
//! this course list over the built-in Story Mode courses / the library (the Swift app shows its
//! library there).

use crate::fade::FadeKind;
use crate::host::{CourseEntry, Source, WorldCourse};
use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, MenuCursor, Script, UiInput, nearest};
use crate::play::{CoursePlay, PlayEvent, PlayInput};
use crate::screens::{CardInfo, CourseScreens};
use crate::sound::Event;
use crate::{Cx, DrawItem, Go, civil, push_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubChoice {
    Courses,
    Endless,
    Unavailable,
}

/// The buttons (parts of `N_Btn_00`) and what they open.
pub const HUB_BUTTONS: [(&str, HubChoice); 6] = [
    ("L_Course_00", HubChoice::Courses),
    ("L_Maker_00", HubChoice::Unavailable),
    ("L_MultiPlay_00", HubChoice::Unavailable),
    ("L_EndlessMode_00", HubChoice::Endless),
    ("L_EventCourseBtn_00", HubChoice::Unavailable),
    ("L_MyWBtn_00", HubChoice::Unavailable),
];

const HUB: &str = "Nwk_Title_00";

/// The Course World hub (`Nwk_Title_00`).
pub struct HubScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    pub selection: usize,
    pending: Option<(HubChoice, i32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubEvent {
    Choice(HubChoice),
    Back,
}

impl HubScene {
    pub fn new(ui: &Ui, selection: usize) -> Option<Self> {
        let mut l = ui.layout(HUB)?;
        // the "Course World" title art and the Super Worlds sign in English (frame 1) [inf]
        l.hold("RegionLanguageType", 1.0);
        l.hold("L_MyWBtn_00/RegionLanguageType", 1.0);
        l.play_loop("LoopRandom", 0);
        l.play_loop("L_MyWBtn_00/SelectLoop", 0);
        // no event running: the Ninji button without its period line [inf]
        l.hold("L_EventCourseBtn_00/TypePeriod", 1.0);
        // the player card (Mii), the ID search: no data / no keyboard here [departure]
        l.show("L_PlayerBody_00", false);
        l.show("L_IDSearchBtn_00", false);
        for (b, _) in HUB_BUTTONS {
            l.hold(&format!("{b}/Select"), 0.0);
        }
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            selection: selection.min(HUB_BUTTONS.len() - 1),
            pending: None,
        };
        s.select(s.selection, false);
        Some(s)
    }

    pub fn step(&mut self, input: &UiInput, script: &mut Script) -> Option<HubEvent> {
        self.tick += 1;
        let pressed = input.pressed | script.step();
        let mut ev = None;
        if let Some((c, at)) = self.pending
            && self.tick >= at
        {
            self.pending = None;
            ev = Some(HubEvent::Choice(c));
        }
        if self.pending.is_none() && ev.is_none() {
            ev = self.handle(pressed, input.click);
        }
        let part = HUB_BUTTONS[self.selection].0;
        let alpha = if self.pending.is_none() { 1.0 } else { 0.0 };
        self.cursor.place(
            &mut self.layout,
            &format!("{part}/B_Btn_00"),
            self.tick,
            alpha,
        );
        ev
    }

    fn handle(&mut self, mut pressed: Buttons, click: Option<[f32; 2]>) -> Option<HubEvent> {
        if let Some(p) = click {
            self.layout.apply(self.tick);
            if let Some(i) = (0..HUB_BUTTONS.len()).find(|&i| {
                self.layout
                    .rect(&format!("{}/B_Btn_00", HUB_BUTTONS[i].0))
                    .is_some_and(|r| r.visible && r.contains(p))
            }) {
                self.select(i, false);
                pressed = pressed | Buttons::CONFIRM;
            }
        }
        if let Some(dir) = pressed.direction() {
            self.layout.apply(self.tick);
            let here = self
                .layout
                .rect(&format!("{}/B_Btn_00", HUB_BUTTONS[self.selection].0))
                .map_or([0.0, 0.0], |r| r.center);
            let targets: Vec<(usize, [f32; 2])> = HUB_BUTTONS
                .iter()
                .enumerate()
                .filter_map(|(i, (b, _))| {
                    self.layout
                        .rect(&format!("{b}/B_Btn_00"))
                        .map(|r| (i, r.center))
                })
                .collect();
            if let Some(n) = nearest(here, dir, &targets) {
                self.ui.sound_button(HUB, HUB_BUTTONS[n].0, Event::Select);
                self.select(n, true);
            }
        } else if pressed.has(Buttons::CONFIRM) {
            let (b, c) = HUB_BUTTONS[self.selection];
            self.ui.sound_button(HUB, b, Event::Decide);
            let a = format!("{b}/Decide");
            self.layout.play(&a, self.tick);
            self.pending = Some((c, self.tick + self.layout.ticks(&a, 1)));
        } else if pressed.has(Buttons::BACK) {
            self.ui.sound_back(HUB, "normal");
            return Some(HubEvent::Back);
        }
        None
    }

    fn select(&mut self, i: usize, animate: bool) {
        for (k, (b, _)) in HUB_BUTTONS.iter().enumerate() {
            if k != i {
                self.layout.hold(&format!("{b}/Select"), 0.0);
            }
        }
        self.selection = i;
        let a = format!("{}/Select", HUB_BUTTONS[i].0);
        if animate {
            self.layout.play(&a, self.tick);
        } else {
            self.layout.hold_end(&a);
        }
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
        self.cursor.draw(self.tick, out);
    }
}

// MARK: - course list

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Hot,
    Popular,
    New,
}

const TABS: [&str; 3] = [
    "L_PickUpBtn_00",
    "L_CoursePointRankBtn_00",
    "L_LatestBtn_00",
];
const SLOTS: usize = 5;
const LIST: &str = "L_CourseList_00";
pub const LIST_LAYOUT: &str = "Nwk_CourseListGeneral_00";
/// Lines on screen: the five slots are a ring whose first and last are the off-screen rows
/// above and below the list's scissor (`N_Scissor_00`, 580 high) [inf from the layout]; the
/// player doesn't clip, so those two stay hidden.
const VISIBLE: i64 = 3;

#[derive(Clone, Debug, PartialEq)]
pub enum ListEvent {
    Play(Box<WorldCourse>),
    Back,
}

/// The course list (`Nwk_CourseListGeneral_00` with `Nwk_CourseList_00`'s five rows).
pub struct CourseListScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    pub tab: Tab,
    all: Vec<WorldCourse>,
    pub courses: Vec<WorldCourse>,
    pub selection: i64,
    first: i64,
    /// The list's slide when it scrolls by a line (from this offset to 0) [inf].
    slide: Option<(f32, i32)>,
    pub open: bool,
    busy_until: i32,
    pending: Option<(WorldCourse, i32)>,
}

impl CourseListScene {
    pub fn new(ui: &Ui) -> Option<Self> {
        let mut l = ui.layout(LIST_LAYOUT)?;
        l.play("In", 0);
        // no sort / search window, no "Detailed Search" and no timeline [departure]
        for p in [
            "L_SortBtn_00",
            "N_SortBtn_00",
            "N_CourseList_00/L_AdvancedBtn_00",
            "N_CourseList_00/L_TimelineBtn_00",
            "L_TimelineBtn_00",
            "L_AdvancedBtn_00",
            "N_PageTopBtn_00",
            "L_PageBtns_00/N_PageBtns_00",
        ] {
            l.show(p, false);
        }
        l.text_from("L_PageBtns_00/T_Title_00", "L_PageBtns_00-T_Title_00");
        for t in TABS {
            l.hold(&format!("{t}/Select"), 0.0);
        }
        l.hold(&format!("{LIST}/TypeContents"), 0.0);
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            tab: Tab::Hot,
            all: Vec::new(),
            courses: Vec::new(),
            selection: 0,
            first: 0,
            slide: None,
            open: false,
            busy_until: 0,
            pending: None,
        };
        s.select_tab(Tab::Hot);
        s.set_loading(true);
        Some(s)
    }

    /// The courses arrived (library first, then the server's when it answers).
    pub fn set_courses(&mut self, list: Vec<WorldCourse>, loading: bool) {
        self.all = list;
        self.set_loading(loading);
        self.apply_tab(true);
    }

    fn set_loading(&mut self, on: bool) {
        let show = on && self.all.is_empty();
        self.layout
            .visible("L_LoadingIcon_00", if show { None } else { Some(false) });
        if show {
            self.layout.play_loop("L_LoadingIcon_00/Loop", self.tick);
        }
    }

    fn apply_tab(&mut self, keep_selection: bool) {
        let id = (keep_selection && (self.selection as usize) < self.courses.len())
            .then(|| self.courses[self.selection as usize].id.clone());
        let mut c = self.all.clone();
        match self.tab {
            // the library first, then the server's courses, each newest first [inf]
            Tab::Hot => c.sort_by(|a, b| {
                a.is_server()
                    .cmp(&b.is_server())
                    .then(b.date.unwrap_or(i64::MIN).cmp(&a.date.unwrap_or(i64::MIN)))
            }),
            Tab::Popular => c.sort_by(|a, b| {
                (b.likes.unwrap_or(0), b.plays.unwrap_or(0))
                    .cmp(&(a.likes.unwrap_or(0), a.plays.unwrap_or(0)))
            }),
            Tab::New => {
                c.sort_by(|a, b| b.date.unwrap_or(i64::MIN).cmp(&a.date.unwrap_or(i64::MIN)))
            }
        }
        self.courses = c;
        self.selection = id
            .and_then(|i| self.courses.iter().position(|c| c.id == i))
            .unwrap_or(0) as i64;
        self.first = self
            .selection
            .min(self.courses.len() as i64 - VISIBLE)
            .max(0);
        self.open = false;
        self.fill();
    }

    fn select_tab(&mut self, t: Tab) {
        self.tab = t;
        for (k, p) in TABS.iter().enumerate() {
            // the current tab keeps its Decide look (dark), the others their plain one [inf]
            let a = format!("{p}/Decide");
            if k == t as usize {
                self.layout.hold_end(&a);
            } else {
                self.layout.stop(&a);
            }
        }
    }

    fn slot_pane(k: i64) -> String {
        format!("{LIST}/L_CourseData_{k:02}")
    }

    /// The slot showing course `i` (line 1 is the first visible one).
    fn row_pane(&self, i: i64) -> String {
        Self::slot_pane(i - self.first + 1)
    }

    /// Put the courses of the window into the three visible slots (an open row takes the space
    /// of the two lines under it).
    fn fill(&mut self) {
        let l = &mut self.layout;
        l.show(&format!("{LIST}/N_CourseCountAlign_00"), false);
        for k in 0..SLOTS as i64 {
            let row = Self::slot_pane(k);
            let i = self.first + k - 1;
            // an open row (on the middle line) covers the lines above and below it
            let covered = self.open && i != self.selection;
            if !(1..=VISIBLE).contains(&k) || i < 0 || i >= self.courses.len() as i64 || covered {
                l.show(&row, false);
                continue;
            }
            l.visible(&row, None);
            let c = &self.courses[i as usize];
            l.hold(&format!("{row}/Type"), 1.0); // no world rank column
            // tags: TypeTag 0 two, 1 one, 2 none [obs keys]
            let tags: Vec<String> = c
                .tags
                .iter()
                .take(2)
                .filter_map(|t| l.message(&(1000 + t).to_string()))
                .collect();
            l.hold(&format!("{row}/TypeTag"), (2 - tags.len()) as f32);
            if let Some(t) = tags.first() {
                l.text(&format!("{row}/T_Tag_00"), t);
            }
            if tags.len() == 2 {
                l.text(&format!("{row}/T_Tag_01"), &tags[1]);
            }
            l.hold(&format!("{row}/TypeMission"), 0.0);
            l.show(&format!("{row}/P_MissionIcon_00"), false);
            l.text(&format!("{row}/T_CourseName_00"), &c.name);
            let logo = format!("{}_Lyt_Logo_00", c.style);
            if !l.picture(
                &format!("{row}/L_GameSkin_00/P_DummyGameSkin_00"),
                "GameSkin",
                Some(&logo),
            ) {
                l.show(&format!("{row}/L_GameSkin_00"), false);
            }
            for p in [
                "L_Thumbnail_00",
                "L_GoodCheck_00",
                "L_MyWIcon_00",
                "L_MiiBtn_00",
                "L_BadgeList_00",
                "L_NationalFlag_00",
                "L_MiiIcon_00",
                "L_CommentBtn_00",
                "L_ReportBtn_00",
                "L_DeleteBtn_00",
                "L_DpointBtn_00",
                "L_CoopModeBtn_00",
                "L_DetailBtn_00",
                "L_Rank_00",
                "L_DownloadPlayBtn_00",
            ] {
                l.show(&format!("{row}/{p}"), false);
            }
            // the rows' fixed texts: the screen's table labels them for the first row only
            for p in [
                "T_ClearRate_00",
                "T_Challenge_00",
                "T_Fast_00",
                "T_CourseIDText_00",
                "L_PlayBtn_00-T_Btn_00",
            ] {
                if let Some(s) = l.message(&format!("L_CourseList_00-L_CourseData_00-{p}")) {
                    l.text(&format!("{row}/{}", p.replace('-', "/")), &s);
                }
            }
            let maker = c.maker.clone().unwrap_or_default();
            l.text(&format!("{row}/T_PlayerName_00"), &maker);
            l.visible(
                &format!("{row}/N_BadgeListAlign_00"),
                if maker.is_empty() { Some(false) } else { None },
            );
            let n = |v: Option<i64>| v.map_or("-".to_string(), |v| v.to_string());
            l.text(&format!("{row}/L_GoodCount_00/T_Count_00"), &n(c.likes));
            l.text(&format!("{row}/L_PlayerCount_00/T_Count_00"), &n(c.plays));
            l.hold(
                &format!("{row}/L_ClearCheck_00/ClearCheck"),
                if c.cleared { 1.0 } else { 0.0 },
            );
            l.visible(
                &format!("{row}/L_ClearCheck_00"),
                if c.cleared { None } else { Some(false) },
            );
            match c.date {
                Some(d) => {
                    let (y, mo, da, h, mi) = civil(d + c.date_offset);
                    let day = l
                        .format(
                            "Day_0000",
                            &[format!("{mo:02}"), format!("{da:02}"), y.to_string()],
                        )
                        .unwrap_or_default();
                    let time = l
                        .format("Time_0000", &[format!("{h:02}"), format!("{mi:02}")])
                        .unwrap_or_default();
                    l.text(&format!("{row}/T_Day_00"), &day);
                    l.text(&format!("{row}/T_Time_00"), &time);
                    l.visible(&format!("{row}/P_DayTimeBase_00"), None);
                }
                None => l.show(&format!("{row}/P_DayTimeBase_00"), false),
            }
            let c = c.clone();
            Self::fill_detail(l, &row, &c);
            // Play downloads a server course first (no separate Download button) [departure]
            l.hold(&format!("{row}/TypeBtn"), 0.0);
            let toggle = format!("{row}/Toggle");
            if i == self.selection && self.open {
                l.hold_end(&toggle);
            } else {
                l.stop(&toggle);
            }
            let sel = format!("{row}/Select");
            let f = if i == self.selection && !self.open {
                l.length(&sel)
            } else {
                0.0
            };
            l.hold(&sel, f);
            l.play_loop(&format!("{row}/Loop"), 0);
        }
        self.layout_rows();
    }

    fn fill_detail(l: &mut Layout, row: &str, c: &WorldCourse) {
        l.wrapped_text(&format!("{row}/T_CourseInfo_00"), &c.description, 560.0, 2);
        let rate = match c.clear_rate {
            Some(r) => {
                let whole = r as i64;
                let frac = ((r - whole as f64) * 100.0) as i64;
                l.format(
                    "ClearRatePercent_0000",
                    &[whole.to_string(), format!("{frac:02}")],
                )
                .unwrap_or_default()
            }
            None => "--.-- %".to_string(),
        };
        l.text(&format!("{row}/T_ClearRatePercent_00"), &rate);
        l.text(
            &format!("{row}/L_ClearCount_00/T_Count_00"),
            &c.clears.unwrap_or(0).to_string(),
        );
        l.text(
            &format!("{row}/L_TryCount_00/T_Count_00"),
            &c.attempts.unwrap_or(0).to_string(),
        );
        // the world record when the index knows it, else this player's best time [inf]
        let ms = c.world_record_ms.or(c.best_ticks.map(|t| t * 1000 / 60));
        let (a, b) = match ms {
            Some(ms) => (
                l.format(
                    "Time_0001",
                    &[
                        format!("{:02}", ms / 60000),
                        format!("{:02}", ms / 1000 % 60),
                    ],
                ),
                l.format("Time_0002", &[format!("{:03}", ms % 1000)]),
            ),
            None => (l.message("Time_0011"), l.message("Time_0012")),
        };
        l.text(&format!("{row}/T_Time_01"), &a.unwrap_or_default());
        l.text(&format!("{row}/T_Time_02"), &b.unwrap_or_default());
        l.text(&format!("{row}/T_PlayerName_01"), "");
        l.text(
            &format!("{row}/T_CourseID_00"),
            c.code.as_deref().unwrap_or("-"),
        );
    }

    /// The slots at their lines, the whole list slid while it scrolls.
    fn layout_rows(&mut self) {
        let mut dy = 0.0;
        if let Some((from, start)) = self.slide {
            let t = (self.tick - start) as f32 / 6.0;
            if t >= 1.0 {
                self.slide = None;
            } else {
                dy = from * (1.0 - t) * (1.0 - t);
            }
        }
        self.layout
            .set_translate(&format!("{LIST}/N_TransRoot_00"), [0.0, dy]);
    }

    pub fn step(&mut self, input: &UiInput, script: &mut Script) -> Option<ListEvent> {
        self.tick += 1;
        let pressed = input.pressed | script.step();
        let mut ev = None;
        if let Some((c, at)) = &self.pending
            && self.tick >= *at
        {
            ev = Some(ListEvent::Play(Box::new(c.clone())));
            self.pending = None;
        }
        if self.tick >= self.busy_until
            && let Some(e) = self.handle(pressed)
        {
            ev = Some(e);
        }
        self.layout_rows();
        self.place_cursor();
        ev
    }

    fn handle(&mut self, pressed: Buttons) -> Option<ListEvent> {
        let tick = self.tick;
        if pressed.has(Buttons::BACK) {
            if self.open {
                self.toggle(false);
            } else {
                self.ui.sound_back(LIST_LAYOUT, "normal");
                self.layout.play("Out", tick);
                self.busy_until = i32::MAX;
                return Some(ListEvent::Back);
            }
            return None;
        }
        if self.open {
            if pressed.has(Buttons::CONFIRM) {
                let row = self.row_pane(self.selection);
                self.ui
                    .sound_button(LIST_LAYOUT, "L_PlayBtn_00", Event::Decide);
                let a = format!("{row}/L_PlayBtn_00/Decide");
                self.layout.play(&a, tick);
                let c = self.courses[self.selection as usize].clone();
                self.busy_until = i32::MAX;
                let wait = self.layout.ticks(&a, 1);
                self.pending = Some((c, tick + wait));
            } else if pressed.has(Buttons::UP) || pressed.has(Buttons::DOWN) {
                self.toggle(false);
                self.move_by(if pressed.has(Buttons::UP) { -1 } else { 1 });
            }
            return None;
        }
        if pressed.has(Buttons::LEFT) || pressed.has(Buttons::RIGHT) {
            let tabs = [Tab::Hot, Tab::Popular, Tab::New];
            let k = self.tab as usize;
            let t = tabs[if pressed.has(Buttons::LEFT) {
                (k + 2) % 3
            } else {
                (k + 1) % 3
            }];
            self.ui
                .sound_button(LIST_LAYOUT, TABS[t as usize], Event::Decide);
            self.layout
                .play(&format!("{}/Decide", TABS[t as usize]), tick);
            self.select_tab(t);
            self.apply_tab(false);
        } else if pressed.has(Buttons::UP) {
            self.move_by(-1);
        } else if pressed.has(Buttons::DOWN) {
            self.move_by(1);
        } else if pressed.has(Buttons::CONFIRM) && (self.selection as usize) < self.courses.len() {
            let row = self.row_pane(self.selection);
            self.layout.play(&format!("{row}/Decide"), tick);
            self.toggle(true);
        }
        None
    }

    fn move_by(&mut self, d: i64) {
        if self.courses.is_empty() {
            return;
        }
        let n = (self.selection + d).clamp(0, self.courses.len() as i64 - 1);
        if n == self.selection {
            return;
        }
        self.selection = n;
        self.ui.sound_button(LIST_LAYOUT, LIST, Event::Select);
        let old = self.first;
        if self.selection < self.first {
            self.first = self.selection;
        }
        if self.selection > self.first + VISIBLE - 1 {
            self.first = self.selection - VISIBLE + 1;
        }
        if self.first != old {
            self.slide = Some(((self.first - old) as f32 * -180.0, self.tick));
        }
        self.fill();
        let row = self.row_pane(self.selection);
        self.layout.play(&format!("{row}/Select"), self.tick);
    }

    fn toggle(&mut self, on: bool) {
        if self.selection as usize >= self.courses.len() {
            return;
        }
        self.open = on;
        if on {
            // the open row on the middle line: it grows 180 up and down, over its neighbours,
            // and stays inside the list's scissor [inf]
            let mid = self.selection - 1;
            if self.first != mid {
                self.slide = Some(((mid - self.first) as f32 * -180.0, self.tick));
            }
            self.first = mid;
        } else if self.first < 0 || self.first > (self.courses.len() as i64 - VISIBLE).max(0) {
            self.first = self.first.min(self.courses.len() as i64 - VISIBLE).max(0);
        }
        self.fill();
        let row = self.row_pane(self.selection);
        let key = format!(
            "Anm_Toggle_showcoursedetail_{}",
            if on { "Forward" } else { "Backward" }
        );
        self.ui.sound_key(LIST_LAYOUT, &key, &[]);
        let t = self.tick;
        if on {
            self.layout.play(&format!("{row}/Toggle"), t);
            self.layout
                .play(&format!("{row}/L_PlayBtn_00/Select"), t + 10);
        } else {
            self.layout.play(&format!("{row}/Select"), t);
        }
        self.busy_until = t + 10;
    }

    fn place_cursor(&mut self) {
        if self.selection as usize >= self.courses.len() {
            self.cursor.hide();
            return;
        }
        let row = self.row_pane(self.selection);
        let pane = if self.open {
            format!("{row}/L_PlayBtn_00/B_Btn_00")
        } else {
            format!("{row}/B_Btn_00")
        };
        let alpha = if self.busy_until == i32::MAX {
            0.0
        } else {
            1.0
        };
        self.cursor.place(&mut self.layout, &pane, self.tick, alpha);
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
        self.cursor.draw(self.tick, out);
    }
}

// MARK: - flow

/// Which courses a list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    /// Course World: the library (not the built-in courses) and the server.
    World,
    /// Story Mode: the built-in Story Mode courses.
    Story,
    /// Coursebot: the library.
    Coursebot,
}

/// Deferred work the flow asks the front end to run (behind a fade).
#[derive(Clone, Debug, PartialEq)]
pub enum WorldGo {
    List,
    Hub,
    Start(Box<WorldCourse>),
}

enum Stage {
    Hub(Box<HubScene>),
    List(Box<CourseListScene>),
    Playing(Box<CoursePlay>, Box<WorldCourse>),
}

/// What a step tells the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldEvent {
    /// Back to the title.
    Exit,
    /// Endless Challenge (the owner shows the Endless menu and comes back here).
    Endless,
}

/// Course World: hub → course list → play, and the Endless menu; owns the course being played.
pub struct WorldFlow {
    ui: Ui,
    pub kind: ListKind,
    stage: Stage,
    library: Vec<CourseEntry>,
    hub_selection: usize,
    waiting_server: bool,
    local: Vec<WorldCourse>,
    leaving: bool,
    /// Captures: menu presses at global UI ticks.
    pub script: Script,
}

impl WorldFlow {
    /// The hub (Course World) or straight the list (Story Mode, Coursebot).
    pub fn new(
        ui: &Ui,
        kind: ListKind,
        library: Vec<CourseEntry>,
        selection: usize,
        cx: &mut Cx,
    ) -> Option<Self> {
        let mut f = Self {
            ui: ui.clone(),
            kind,
            stage: Stage::Hub(Box::new(HubScene::new(ui, selection)?)),
            library,
            hub_selection: selection,
            waiting_server: false,
            local: Vec::new(),
            leaving: false,
            script: Script::default(),
        };
        if kind != ListKind::World {
            f.show_list(cx);
        }
        Some(f)
    }

    pub fn show_hub(&mut self) {
        if let Some(h) = HubScene::new(&self.ui, self.hub_selection) {
            self.stage = Stage::Hub(Box::new(h));
        }
    }

    pub fn show_list(&mut self, cx: &mut Cx) {
        let Some(mut s) = CourseListScene::new(&self.ui) else {
            return;
        };
        let kind = self.kind;
        self.local = self
            .library
            .iter()
            .filter(|e| match kind {
                ListKind::World | ListKind::Coursebot => !e.built_in,
                ListKind::Story => e.collection.as_deref() == Some("Story Mode"),
            })
            .map(|e| {
                let mut c = WorldCourse::library(e, &cx.saves.record(&e.key));
                c.date_offset = c.date.map_or(0, |d| cx.host.local_offset(d));
                c
            })
            .collect();
        self.waiting_server = kind == ListKind::World && cx.host.request_server_courses();
        s.set_courses(self.local.clone(), self.waiting_server);
        self.stage = Stage::List(Box::new(s));
    }

    pub fn go(&mut self, cx: &mut Cx, g: WorldGo) {
        if matches!(self.stage, Stage::Playing(..)) {
            cx.host.unload_course();
        }
        match g {
            WorldGo::List => self.show_list(cx),
            WorldGo::Hub => self.show_hub(),
            WorldGo::Start(c) => self.start(cx, *c),
        }
    }

    fn start(&mut self, cx: &mut Cx, c: WorldCourse) {
        let loaded = match cx.host.load_course(&c.source) {
            Ok(l) => l,
            Err(e) => {
                cx.toast(&format!("Couldn't start {}: {e}", c.name));
                self.show_list(cx);
                return;
            }
        };
        if let Source::Server(code) = &c.source {
            cx.host.report(code, "play");
        }
        let title = if matches!(c.source, Source::Library(_)) || loaded.name.is_empty() {
            c.name.clone()
        } else {
            loaded.name.clone()
        };
        let key = loaded.key.clone().or(c.key.clone());
        let built_in = self.library.iter().any(|e| {
            Source::Library(e.id.clone()) == c.source
                && e.collection.as_deref() == Some("Story Mode")
        });
        let info = CardInfo {
            name: title.clone(),
            maker: c.maker.clone(),
            best_ticks: key.as_ref().and_then(|k| cx.saves.record(k).best_ticks),
            built_in,
            mode_label: (self.kind == ListKind::World).then(|| "T_Mode_Network".to_string()),
            ..Default::default()
        };
        let screens = CourseScreens::new(&self.ui);
        self.leaving = false;
        let p = CoursePlay::new(cx, title, key, screens, None, info, true, true, false);
        self.stage = Stage::Playing(Box::new(p), Box::new(c));
    }

    pub fn step(&mut self, input: &PlayInput, cx: &mut Cx) -> Option<WorldEvent> {
        match &mut self.stage {
            Stage::Hub(h) => match h.step(&input.ui, &mut self.script)? {
                HubEvent::Back => return Some(WorldEvent::Exit),
                HubEvent::Choice(c) => {
                    self.hub_selection = h.selection;
                    match c {
                        HubChoice::Courses => cx.fade_to(FadeKind::Black, Go::World(WorldGo::List)),
                        HubChoice::Endless => return Some(WorldEvent::Endless),
                        HubChoice::Unavailable => {
                            cx.toast("This needs Nintendo's network, which this engine never uses.")
                        }
                    }
                }
            },
            Stage::List(s) => {
                if self.waiting_server
                    && let Some(found) = cx.host.poll_server_courses()
                {
                    self.waiting_server = false;
                    let mut all = self.local.clone();
                    all.extend(found);
                    s.set_courses(all, false);
                }
                match s.step(&input.ui, &mut self.script)? {
                    ListEvent::Back => {
                        if self.kind == ListKind::World {
                            cx.fade_to(FadeKind::Black, Go::World(WorldGo::Hub));
                        } else {
                            return Some(WorldEvent::Exit);
                        }
                    }
                    ListEvent::Play(c) => {
                        if !c.playable {
                            cx.toast(&format!(
                                "{} uses parts the engine doesn't support yet.",
                                c.name
                            ));
                            self.show_list(cx);
                        } else {
                            cx.fade_to(FadeKind::Black, Go::World(WorldGo::Start(c)));
                        }
                    }
                }
            }
            Stage::Playing(p, c) => match p.step(input, cx, None) {
                // (the course keeps animating under the fade out)
                _ if self.leaving => {}
                Some(PlayEvent::Exit) => {
                    // the course stays on screen under the fade
                    self.leaving = true;
                    cx.fade_to(FadeKind::Black, Go::World(WorldGo::List));
                }
                Some(PlayEvent::Cleared) => {
                    if let Source::Server(code) = &c.source {
                        let code = code.clone();
                        cx.host.report(&code, "clear");
                    }
                }
                None => {}
            },
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        match &mut self.stage {
            Stage::Hub(h) => h.draw(out),
            Stage::List(s) => s.draw(out),
            Stage::Playing(p, _) => p.draw(out),
        }
    }

    pub fn hub_selection(&self) -> usize {
        self.hub_selection
    }

    pub fn playing(&mut self) -> Option<&mut CoursePlay> {
        match &mut self.stage {
            Stage::Playing(p, _) => Some(p),
            _ => None,
        }
    }
}
