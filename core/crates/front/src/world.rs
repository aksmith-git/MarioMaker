//! Course World with SMM2's own layouts, over the local library and the open-source course
//! server (never Nintendo's network; the Swift app's `SMM2CourseWorld.swift`):
//! - the hub `Nwk_Title_00` (space background with its LoopRandom events, the buttons Courses,
//!   Leaderboards, Network Play, Endless Challenge, Ninji Speedruns, Super Worlds, and the player
//!   card) [obs layout];
//! - Courses → `Nwk_CourseListGeneral_00`: the tabs (Hot / Popular / New), rows
//!   `Nwk_CourseData_00` (name, style logo, maker, plays, likes, date, clear mark); confirm opens
//!   a row (its Toggle animation: clear rate, clears / attempts, best time, course ID, Play, and
//!   for server courses "More info" and "View comments") and Play starts the course with the
//!   black fade;
//! - Leaderboards → `leaderboard` (`Nwk_CourseListMaker_00`); the player card → `account`
//!   (`Nwk_PlayerInfo_00`, over the hub); "More info" → the course page and "View comments" →
//!   the comments (`coursepage`); a maker's profile is `Nwk_CourseListWindow_00` (Type 0) with
//!   their uploaded courses as course rows;
//! - Endless Challenge → the Endless menu (100 Mario rules, see `endless`).
//!
//! Scene changes use the black fade [obs]; the windows (course page, profile, comments, the
//! comment form) open over the screen under them with their own In [inf: as the game's
//! windows]. What each tab lists, the cursor's start, which buttons are unavailable without
//! Nintendo's network (a message), and the hidden parts without data (Mii, flags, badges,
//! thumbnails, world ranks, ID search) are ours [inf / departure]. Departure of the portable
//! front end: Story Mode and Coursebot open this course list over the built-in Story Mode courses
//! / the library (the Swift app shows its library there).
//!
//! A server course's clear is sent when signed in: its replay (`submit_replay`, which the server
//! re-simulates; a plain `clear` with the time if that fails), or the clear without a time when
//! the engine's play options change play; signed out, an anonymous clear report.
//!
//! Clicks / taps (layout units) select and decide what they land on: hub buttons, the player
//! card, the list's tabs, rows (a click on the selected row opens it) and an open row's buttons.

use serde_json::{Value, json};

use crate::account::{AccountEvent, AccountScene};
use crate::coursepage::{
    CommentsEvent, CommentsScene, CoursePageScene, PageEvent, PostEvent, PostScene, WINDOW,
};
use crate::fade::FadeKind;
use crate::host::{CourseEntry, Source, WorldCourse};
use crate::layout::{Layout, Ui};
use crate::leaderboard::{BoardEvent, LeaderboardScene};
use crate::menu::{Buttons, Loading, MenuCursor, Script, UiInput, hit, nearest, set_loading};
use crate::play::{CoursePlay, PlayEvent, PlayInput};
use crate::screens::{CardInfo, CourseScreens};
use crate::sound::Event;
use crate::{Cx, DrawItem, Go, civil, push_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubChoice {
    Courses,
    Leaderboards,
    Endless,
    Account,
    Unavailable,
}

/// The buttons (parts of `N_Btn_00`, then the player card) and what they open.
pub const HUB_BUTTONS: [(&str, HubChoice); 7] = [
    ("L_Course_00", HubChoice::Courses),
    ("L_Maker_00", HubChoice::Leaderboards),
    ("L_MultiPlay_00", HubChoice::Unavailable),
    ("L_EndlessMode_00", HubChoice::Endless),
    ("L_EventCourseBtn_00", HubChoice::Unavailable),
    ("L_MyWBtn_00", HubChoice::Unavailable),
    ("L_PlayerBody_00", HubChoice::Account),
];

const HUB: &str = "Nwk_Title_00";

/// A hub button's bounding pane (the player card's is `B_bounding_00`).
pub fn hub_bound(part: &str) -> String {
    if part == "L_PlayerBody_00" {
        format!("{part}/B_bounding_00")
    } else {
        format!("{part}/B_Btn_00")
    }
}

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
    /// `account`: the signed-in account's name, shown on the player card [inf].
    pub fn new(ui: &Ui, selection: usize, account: Option<&str>) -> Option<Self> {
        let mut l = ui.layout(HUB)?;
        // the "Course World" title art and the Super Worlds sign in English (frame 1) [inf]
        l.hold("RegionLanguageType", 1.0);
        l.hold("L_MyWBtn_00/RegionLanguageType", 1.0);
        l.play_loop("LoopRandom", 0);
        l.play_loop("L_MyWBtn_00/SelectLoop", 0);
        // no event running: the Ninji button without its period line [inf]
        l.hold("L_EventCourseBtn_00/TypePeriod", 1.0);
        // the ID search: no keyboard search here; the player card without a Mii [departure]
        l.show("L_IDSearchBtn_00", false);
        l.show("L_PlayerBody_00/N_MiiScissor_00", false);
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
        s.set_account(account);
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
        self.cursor
            .place(&mut self.layout, &hub_bound(part), self.tick, alpha);
        ev
    }

    fn handle(&mut self, mut pressed: Buttons, click: Option<[f32; 2]>) -> Option<HubEvent> {
        if let Some(p) = click {
            let panes: Vec<String> = HUB_BUTTONS.iter().map(|b| hub_bound(b.0)).collect();
            if let Some(i) = hit(&mut self.layout, self.tick, &panes, p) {
                self.select(i, false);
                pressed = pressed | Buttons::CONFIRM;
            }
        }
        if let Some(dir) = pressed.direction() {
            self.layout.apply(self.tick);
            let here = self
                .layout
                .rect(&hub_bound(HUB_BUTTONS[self.selection].0))
                .map_or([0.0, 0.0], |r| r.center);
            let targets: Vec<(usize, [f32; 2])> = HUB_BUTTONS
                .iter()
                .enumerate()
                .filter_map(|(i, (b, _))| {
                    self.layout
                        .rect(&hub_bound(b))
                        .filter(|r| r.visible)
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

    /// The player info opens over the hub (InPlayerInfo fades its buttons) or closes.
    pub fn player_info(&mut self, open: bool) {
        self.layout.stop("InPlayerInfo");
        self.layout.stop("OutPlayerInfo");
        let a = if open {
            "InPlayerInfo"
        } else {
            "OutPlayerInfo"
        };
        self.layout.play(a, self.tick);
    }

    /// The player card's label: the account's name, else the layout's own [inf].
    pub fn set_account(&mut self, account: Option<&str>) {
        let pane = "L_PlayerBody_00/T_PlayerInfo_00";
        match account {
            Some(name) if !name.is_empty() => self.layout.text(pane, name),
            _ => self
                .layout
                .text_from(pane, "L_PlayerBody_00-T_PlayerInfo_00"),
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
/// An open row's buttons: Play, then (server courses) View comments and More info.
pub const OPEN_BUTTONS: [&str; 3] = ["L_PlayBtn_00", "L_CommentBtn_00", "L_DetailBtn_00"];

/// Which screen a course list is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListVariant {
    /// `Nwk_CourseListGeneral_00` (Courses, Story Mode, Coursebot).
    General,
    /// A maker's profile: `Nwk_CourseListWindow_00` Type 0 with the uploaded courses.
    Maker,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListEvent {
    Play(Box<WorldCourse>),
    /// "More info": the course page.
    Detail(Box<WorldCourse>),
    /// "View comments".
    Comments(Box<WorldCourse>),
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
    /// The open row's button (an index of [`OPEN_BUTTONS`]).
    pub button: usize,
    busy_until: i32,
    pending: Option<(ListEvent, i32)>,
    pub variant: ListVariant,
    name: &'static str,
    /// A maker's profile: the server call bringing their courses.
    pub source_call: Option<u32>,
}

impl CourseListScene {
    pub fn new(ui: &Ui) -> Option<Self> {
        Self::with(ui, ListVariant::General, "")
    }

    /// A maker's profile with their uploaded courses (`set_courses` when they arrive).
    pub fn maker(ui: &Ui, maker: &str) -> Option<Self> {
        Self::with(ui, ListVariant::Maker, maker)
    }

    fn with(ui: &Ui, variant: ListVariant, maker: &str) -> Option<Self> {
        let name = match variant {
            ListVariant::General => LIST_LAYOUT,
            ListVariant::Maker => WINDOW,
        };
        let mut l = ui.layout(name)?;
        l.play("In", 0);
        match variant {
            ListVariant::General => {
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
            }
            ListVariant::Maker => {
                // another maker's profile (Type 0) on its uploaded courses (contents 1); the
                // overview, liked courses, first clears and records tabs, follow, Mii and flag
                // have no data on our server [departure]
                l.hold("Type", 0.0);
                l.hold("TypeContents", 1.0);
                for p in [
                    "L_FollowBtn_00",
                    "L_PlayInfoBtn_00",
                    "L_PositiveRatedBtn_00",
                    "L_ClearFirstBtn_00",
                    "L_BestTimeCourseBtn_00",
                    "L_MiiIcon_00",
                    "L_NationalFlag_00",
                    "L_UpLoadInfo_00",
                    "L_UpdateBtn_00",
                    "L_CloseBtn_00",
                ] {
                    l.show(p, false);
                }
                l.text("T_PlayerName_00", maker);
                l.text_from(
                    "L_OwnedCourseBtn_00/T_Btn_00",
                    "L_MyOwnedCourseBtn_00-T_Btn_00",
                );
                l.hold_end("L_OwnedCourseBtn_00/Decide");
            }
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
            button: 0,
            busy_until: 0,
            pending: None,
            variant,
            name,
            source_call: None,
        };
        if variant == ListVariant::General {
            s.select_tab(Tab::Hot);
        } else {
            s.tab = Tab::New;
        }
        s.set_loading(true);
        Some(s)
    }

    /// The courses arrived (library first, then the server's when it answers).
    pub fn set_courses(&mut self, list: Vec<WorldCourse>, loading: bool) {
        self.all = list;
        self.set_loading(loading);
        self.apply_tab(true);
    }

    /// The loading icon while waiting with nothing to show; the screen's "no courses" message
    /// when the list stays empty.
    fn set_loading(&mut self, on: bool) {
        let state = if !self.all.is_empty() {
            Loading::Hidden
        } else if on {
            Loading::Waiting
        } else {
            let label = match self.variant {
                ListVariant::General => "NoData_0000",
                ListVariant::Maker => "NoData_0009",
            };
            Loading::Message(self.layout.message(label).unwrap_or_default())
        };
        set_loading(&mut self.layout, "L_LoadingIcon_00", &state, self.tick);
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

    /// Clear marks and best times again from the save data (after a play).
    pub fn refresh_records(&mut self, saves: &crate::Saves) {
        for c in self.all.iter_mut().chain(self.courses.iter_mut()) {
            if let Some(k) = &c.key {
                let r = saves.record(k);
                c.cleared |= r.cleared;
                c.best_ticks = r.best_ticks.or(c.best_ticks);
            }
        }
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
    pub(crate) fn row_pane(&self, i: i64) -> String {
        Self::slot_pane(i - self.first + 1)
    }

    /// The open row's buttons drawn for course `i` (indices of [`OPEN_BUTTONS`]).
    fn open_buttons(&self, i: i64) -> Vec<usize> {
        let server = self
            .courses
            .get(i as usize)
            .is_some_and(WorldCourse::is_server);
        if server { vec![0, 1, 2] } else { vec![0] }
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
                "L_ReportBtn_00",
                "L_DeleteBtn_00",
                "L_DpointBtn_00",
                "L_CoopModeBtn_00",
                "L_Rank_00",
                "L_DownloadPlayBtn_00",
                "L_CommentBtn_00/L_Count_00",
            ] {
                l.show(&format!("{row}/{p}"), false);
            }
            // server courses: "View comments" and "More info" on the open row
            let server = c.is_server();
            for p in ["L_CommentBtn_00", "L_DetailBtn_00"] {
                l.visible(
                    &format!("{row}/{p}"),
                    if server { None } else { Some(false) },
                );
            }
            // the rows' fixed texts: the screen's table labels them for the first row only
            for p in [
                "T_ClearRate_00",
                "T_Challenge_00",
                "T_Fast_00",
                "T_CourseIDText_00",
                "L_PlayBtn_00-T_Btn_00",
                "L_CommentBtn_00-T_Btn_00",
                "L_DetailBtn_00-T_Btn_00",
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
            for (b, p) in OPEN_BUTTONS.iter().enumerate() {
                let a = format!("{row}/{p}/Select");
                let on = i == self.selection && self.open && b == self.button;
                let f = if on { l.length(&a) } else { 0.0 };
                l.hold(&a, f);
            }
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
        let mut pressed = input.pressed | script.step();
        let mut ev = None;
        if let Some((_, at)) = &self.pending
            && self.tick >= *at
        {
            ev = self.pending.take().map(|p| p.0);
        }
        if self.tick >= self.busy_until {
            if let Some(p) = input.click {
                pressed = pressed | self.click(p);
            }
            if let Some(e) = self.handle(pressed) {
                ev = Some(e);
            }
        }
        self.layout_rows();
        self.place_cursor();
        ev
    }

    /// A click: an open row's button (selects and decides it), a tab, a row (selects it; on
    /// the selected row, opens it). The buttons to add as if pressed.
    fn click(&mut self, p: [f32; 2]) -> Buttons {
        let tick = self.tick;
        if self.open {
            let row = self.row_pane(self.selection);
            let shown = self.open_buttons(self.selection);
            let panes: Vec<String> = shown
                .iter()
                .map(|&b| format!("{row}/{}/B_Btn_00", OPEN_BUTTONS[b]))
                .collect();
            if let Some(k) = hit(&mut self.layout, tick, &panes, p) {
                self.select_button(shown[k]);
                return Buttons::CONFIRM;
            }
            // elsewhere on the open row: nothing; outside it, close it
            if hit(&mut self.layout, tick, &[format!("{row}/B_Btn_00")], p).is_none() {
                self.toggle(false);
            }
            return Buttons::NONE;
        }
        if self.variant == ListVariant::General {
            let tabs: Vec<String> = TABS.iter().map(|t| format!("{t}/B_Btn_00")).collect();
            if let Some(k) = hit(&mut self.layout, tick, &tabs, p) {
                let t = [Tab::Hot, Tab::Popular, Tab::New][k];
                if t != self.tab {
                    self.ui.sound_button(self.name, TABS[k], Event::Decide);
                    self.layout.play(&format!("{}/Decide", TABS[k]), tick);
                    self.select_tab(t);
                    self.apply_tab(false);
                }
                return Buttons::NONE;
            }
        }
        let rows: Vec<i64> = (self.first..self.first + VISIBLE)
            .filter(|&i| i >= 0 && (i as usize) < self.courses.len())
            .collect();
        let panes: Vec<String> = rows
            .iter()
            .map(|&i| format!("{}/B_Btn_00", self.row_pane(i)))
            .collect();
        if let Some(k) = hit(&mut self.layout, tick, &panes, p) {
            let i = rows[k];
            if i == self.selection {
                return Buttons::CONFIRM;
            }
            self.move_by(i - self.selection);
        }
        Buttons::NONE
    }

    fn select_button(&mut self, b: usize) {
        if b == self.button {
            return;
        }
        let row = self.row_pane(self.selection);
        self.layout
            .hold(&format!("{row}/{}/Select", OPEN_BUTTONS[self.button]), 0.0);
        self.button = b;
        self.ui
            .sound_button(self.name, OPEN_BUTTONS[b], Event::Select);
        self.layout
            .play(&format!("{row}/{}/Select", OPEN_BUTTONS[b]), self.tick);
    }

    fn handle(&mut self, pressed: Buttons) -> Option<ListEvent> {
        let tick = self.tick;
        if pressed.has(Buttons::BACK) {
            if self.open {
                self.toggle(false);
            } else {
                self.ui.sound_back(self.name, "normal");
                self.layout.play("Out", tick);
                self.busy_until = i32::MAX;
                return Some(ListEvent::Back);
            }
            return None;
        }
        if self.open {
            let row = self.row_pane(self.selection);
            if pressed.has(Buttons::CONFIRM) {
                let part = OPEN_BUTTONS[self.button];
                self.ui.sound_button(self.name, part, Event::Decide);
                let a = format!("{row}/{part}/Decide");
                self.layout.play(&a, tick);
                let c = Box::new(self.courses[self.selection as usize].clone());
                self.busy_until = i32::MAX;
                let wait = self.layout.ticks(&a, 1);
                let e = match self.button {
                    1 => ListEvent::Comments(c),
                    2 => ListEvent::Detail(c),
                    _ => ListEvent::Play(c),
                };
                self.pending = Some((e, tick + wait));
            } else if let Some(dir) = pressed.direction() {
                // between the open row's buttons; up / down past them closes it and moves
                self.layout.apply(tick);
                let at = |l: &Layout, b: usize| {
                    l.rect(&format!("{row}/{}/B_Btn_00", OPEN_BUTTONS[b]))
                        .filter(|r| r.visible)
                        .map(|r| r.center)
                };
                let here = at(&self.layout, self.button).unwrap_or([0.0, 0.0]);
                let targets: Vec<(usize, [f32; 2])> = self
                    .open_buttons(self.selection)
                    .into_iter()
                    .filter_map(|b| at(&self.layout, b).map(|c| (b, c)))
                    .collect();
                if let Some(b) = nearest(here, dir, &targets) {
                    self.select_button(b);
                } else if pressed.has(Buttons::UP) || pressed.has(Buttons::DOWN) {
                    self.toggle(false);
                    self.move_by(if pressed.has(Buttons::UP) { -1 } else { 1 });
                }
            }
            return None;
        }
        if self.variant == ListVariant::General
            && (pressed.has(Buttons::LEFT) || pressed.has(Buttons::RIGHT))
        {
            let tabs = [Tab::Hot, Tab::Popular, Tab::New];
            let k = self.tab as usize;
            let t = tabs[if pressed.has(Buttons::LEFT) {
                (k + 2) % 3
            } else {
                (k + 1) % 3
            }];
            self.ui
                .sound_button(self.name, TABS[t as usize], Event::Decide);
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
        self.ui.sound_button(self.name, LIST, Event::Select);
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
        self.button = 0;
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
        self.ui.sound_key(self.name, &key, &[]);
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
            format!("{row}/{}/B_Btn_00", OPEN_BUTTONS[self.button])
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

    /// Back on this list after a window over it closed.
    pub fn resume(&mut self) {
        self.busy_until = self.tick + 1;
        self.pending = None;
        self.fill();
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
    /// Back from a play to the screen it started from.
    AfterPlay,
    Leaderboards,
}

enum Stage {
    Hub(Box<HubScene>),
    List(Box<CourseListScene>),
    Playing(Box<CoursePlay>, Box<WorldCourse>),
    Account(Box<AccountScene>),
    Board(Box<LeaderboardScene>),
    Page(Box<CoursePageScene>),
    Comments(Box<CommentsScene>),
    Post(Box<PostScene>),
}

impl Stage {
    /// Drawn over the stage under it (the windows, the player info).
    fn overlay(&self) -> bool {
        matches!(
            self,
            Stage::Account(_) | Stage::Page(_) | Stage::Comments(_) | Stage::Post(_)
        ) || matches!(self, Stage::List(s) if s.variant == ListVariant::Maker)
    }

    /// Keep the animations of a stage drawn under another running.
    fn idle(&mut self) {
        match self {
            Stage::Hub(h) => h.tick += 1,
            Stage::List(s) => s.tick += 1,
            Stage::Board(b) => b.tick += 1,
            Stage::Page(p) => p.tick += 1,
            Stage::Comments(c) => c.tick += 1,
            _ => {}
        }
    }

    fn draw(&mut self, out: &mut Vec<DrawItem>) {
        match self {
            Stage::Hub(h) => h.draw(out),
            Stage::List(s) => s.draw(out),
            Stage::Playing(p, _) => p.draw(out),
            Stage::Account(a) => a.draw(out),
            Stage::Board(b) => b.draw(out),
            Stage::Page(p) => p.draw(out),
            Stage::Comments(c) => c.draw(out),
            Stage::Post(p) => p.draw(out),
        }
    }
}

/// What a step tells the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldEvent {
    /// Back to the title.
    Exit,
    /// Endless Challenge (the owner shows the Endless menu and comes back here).
    Endless,
}

/// A clear being sent: the `submit_replay` call, the course code and the time (a plain
/// `clear` follows when the replay is refused).
#[derive(Clone, Debug, PartialEq)]
struct Sending {
    id: u32,
    code: String,
    ms: i64,
    replay: bool,
    /// The record before this clear (ms), to tell a new one.
    before: Option<i64>,
}

/// The clears of server courses on their way to the server (see the module notes).
#[derive(Debug, Default)]
pub struct ClearSender {
    sending: Vec<Sending>,
}

impl ClearSender {
    /// Answers to the clears being sent: a refused replay falls back to a plain clear.
    pub fn collect(&mut self, cx: &mut Cx) {
        let sends = std::mem::take(&mut self.sending);
        for s in sends {
            let Some(a) = cx.online.take(s.id) else {
                self.sending.push(s);
                continue;
            };
            match a {
                Ok(v) => {
                    let mine = cx
                        .online
                        .maker_id()
                        .is_some_and(|m| !m.is_empty() && v["record_holder"].as_str() == Some(m));
                    // (the server times a replay to the end of the clear, the clear screen to
                    // the goal: compare with the record before, not with our time)
                    let ms = v["record_ms"].as_i64().unwrap_or(0);
                    if mine && ms > 0 && s.before.is_none_or(|b| ms < b) {
                        cx.toast(if v["record_verified"].as_bool() == Some(true) {
                            "New world record! (verified from your replay)"
                        } else {
                            "New world record!"
                        });
                    }
                }
                Err(_) if s.replay => {
                    // the server couldn't verify the run: report the clear with its time
                    if let Some(id) =
                        cx.online
                            .call(cx.host, "clear", json!({ "code": s.code, "time_ms": s.ms }))
                    {
                        self.sending.push(Sending {
                            id,
                            replay: false,
                            ..s
                        });
                    }
                }
                Err(_) => {}
            }
        }
    }

    /// Tell the server about a clear of a server course (see the module notes).
    /// `before`: the course's record when the play started (ms).
    pub fn send(&mut self, cx: &mut Cx, code: &str, ticks: Option<i64>, before: Option<i64>) {
        let ms = ticks.map_or(0, |t| t * 1000 / 60);
        if !cx.online.signed_in() {
            cx.host.report(code, "clear");
            return;
        }
        let affects = cx.host.play_options_affect_play();
        let (action, args) = if affects {
            // times by other rules aren't records [inf]
            ("clear", json!({ "code": code, "time_ms": 0 }))
        } else {
            ("submit_replay", json!({ "code": code }))
        };
        match cx.online.call(cx.host, action, args) {
            Some(id) => self.sending.push(Sending {
                id,
                code: code.to_string(),
                ms,
                replay: !affects,
                before,
            }),
            None => cx.host.report(code, "clear"),
        }
    }
}

/// Course World: hub → course list → play, the windows over them, the leaderboards, the
/// account, and the Endless menu; owns the course being played.
pub struct WorldFlow {
    ui: Ui,
    pub kind: ListKind,
    stage: Stage,
    /// The stages under the current one (back returns to them).
    under: Vec<Stage>,
    library: Vec<CourseEntry>,
    hub_selection: usize,
    waiting_server: bool,
    local: Vec<WorldCourse>,
    leaving: bool,
    sending: ClearSender,
    /// The dim drawn under the windows.
    veil: Option<Option<Layout>>,
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
        let name = cx.online.account.as_ref().map(|a| a.name.clone());
        let mut f = Self {
            ui: ui.clone(),
            kind,
            stage: Stage::Hub(Box::new(HubScene::new(ui, selection, name.as_deref())?)),
            under: Vec::new(),
            library,
            hub_selection: selection,
            waiting_server: false,
            local: Vec::new(),
            leaving: false,
            sending: ClearSender::default(),
            veil: None,
            script: Script::default(),
        };
        if kind != ListKind::World {
            f.show_list(cx);
        }
        Some(f)
    }

    fn push(&mut self, s: Stage) {
        let old = std::mem::replace(&mut self.stage, s);
        self.under.push(old);
    }

    /// Back to the stage under the current one (false: none).
    fn pop(&mut self) -> bool {
        let Some(s) = self.under.pop() else {
            return false;
        };
        self.stage = s;
        if let Stage::List(l) = &mut self.stage {
            l.resume();
        }
        true
    }

    pub fn show_hub(&mut self, cx: &Cx) {
        self.under.clear();
        let name = cx.online.account.as_ref().map(|a| a.name.clone());
        if let Some(h) = HubScene::new(&self.ui, self.hub_selection, name.as_deref()) {
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
        // the list replaces whatever was over the hub
        self.under
            .retain(|st| matches!(st, Stage::Hub(_)) && kind == ListKind::World);
        if !matches!(self.stage, Stage::Hub(_)) || kind != ListKind::World {
            self.stage = Stage::List(Box::new(s));
        } else {
            self.push(Stage::List(Box::new(s)));
        }
    }

    fn show_maker(&mut self, cx: &mut Cx, id: &str, name: &str) {
        let Some(mut s) = CourseListScene::maker(&self.ui, name) else {
            return;
        };
        s.source_call = cx.online.call(cx.host, "maker", json!({ "maker_id": id }));
        if s.source_call.is_none() {
            s.set_courses(Vec::new(), false);
        }
        self.push(Stage::List(Box::new(s)));
    }

    pub fn go(&mut self, cx: &mut Cx, g: WorldGo) {
        if matches!(self.stage, Stage::Playing(..)) {
            cx.host.unload_course();
        }
        match g {
            WorldGo::List => self.show_list(cx),
            WorldGo::Hub => self.show_hub(cx),
            WorldGo::Start(c) => self.start(cx, *c),
            WorldGo::AfterPlay => {
                if !self.pop() {
                    self.show_list(cx);
                }
                if let Stage::List(s) = &mut self.stage {
                    s.refresh_records(cx.saves);
                }
            }
            WorldGo::Leaderboards => {
                if let Some(b) = LeaderboardScene::new(&self.ui, cx) {
                    self.under.clear();
                    self.stage = Stage::Board(Box::new(b));
                }
            }
        }
    }

    fn start(&mut self, cx: &mut Cx, c: WorldCourse) {
        let loaded = match cx.host.load_course(&c.source) {
            Ok(l) => l,
            Err(e) => {
                cx.toast(&format!("Couldn't start {}: {e}", c.name));
                match &mut self.stage {
                    Stage::Playing(..) => self.go(cx, WorldGo::AfterPlay),
                    Stage::List(s) => s.resume(),
                    _ => {}
                }
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
            server_code: c.server_code().map(str::to_string),
            ..Default::default()
        };
        let screens = CourseScreens::new(&self.ui);
        self.leaving = false;
        let p = CoursePlay::new(cx, title, key, screens, None, info, true, true, false);
        let playing = Stage::Playing(Box::new(p), Box::new(c));
        if matches!(self.stage, Stage::Playing(..)) {
            self.stage = playing;
        } else {
            self.push(playing);
        }
    }

    /// A text field is open (the host sends typed keys).
    pub fn typing(&self) -> bool {
        match &self.stage {
            Stage::Account(a) => a.typing(),
            Stage::Post(p) => p.typing(),
            _ => false,
        }
    }

    pub fn step(&mut self, input: &PlayInput, cx: &mut Cx) -> Option<WorldEvent> {
        self.sending.collect(cx);
        if self.stage.overlay()
            && let Some(u) = self.under.last_mut()
        {
            u.idle();
        }
        // captures: the scripted presses for the screens that don't read the script themselves
        let mut ui = input.ui;
        if !matches!(
            self.stage,
            Stage::Hub(_) | Stage::List(_) | Stage::Playing(..)
        ) {
            ui.pressed = ui.pressed | self.script.step();
        }
        let ui = &ui;
        match &mut self.stage {
            Stage::Hub(h) => match h.step(&input.ui, &mut self.script)? {
                HubEvent::Back => return Some(WorldEvent::Exit),
                HubEvent::Choice(c) => {
                    self.hub_selection = h.selection;
                    match c {
                        HubChoice::Courses => cx.fade_to(FadeKind::Black, Go::World(WorldGo::List)),
                        HubChoice::Leaderboards => {
                            cx.fade_to(FadeKind::Black, Go::World(WorldGo::Leaderboards))
                        }
                        HubChoice::Endless => return Some(WorldEvent::Endless),
                        HubChoice::Account => {
                            h.player_info(true);
                            if let Some(a) = AccountScene::new(&self.ui, cx) {
                                self.push(Stage::Account(Box::new(a)));
                            }
                        }
                        HubChoice::Unavailable => {
                            cx.toast("This needs Nintendo's network, which this engine never uses.")
                        }
                    }
                }
            },
            Stage::List(s) => {
                if s.variant == ListVariant::General
                    && self.waiting_server
                    && let Some(found) = cx.host.poll_server_courses()
                {
                    self.waiting_server = false;
                    let mut all = self.local.clone();
                    all.extend(found);
                    s.set_courses(all, false);
                }
                if let Some(id) = s.source_call
                    && let Some(a) = cx.online.take(id)
                {
                    s.source_call = None;
                    let list = match a {
                        Ok(v) => maker_courses(&v, cx),
                        Err(e) => {
                            cx.toast(&format!("The server couldn't be reached: {e}"));
                            Vec::new()
                        }
                    };
                    s.set_courses(list, false);
                }
                let variant = s.variant;
                match s.step(&input.ui, &mut self.script)? {
                    ListEvent::Back => {
                        if variant == ListVariant::Maker {
                            self.pop();
                        } else if self.kind == ListKind::World {
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
                            s.resume();
                        } else {
                            cx.fade_to(FadeKind::Black, Go::World(WorldGo::Start(c)));
                        }
                    }
                    ListEvent::Detail(c) => match CoursePageScene::new(&self.ui, *c, cx) {
                        Some(p) => self.push(Stage::Page(Box::new(p))),
                        None => s.resume(),
                    },
                    ListEvent::Comments(c) => {
                        let code = c.server_code().unwrap_or_default().to_string();
                        match CommentsScene::new(&self.ui, &code, cx) {
                            Some(v) => self.push(Stage::Comments(Box::new(v))),
                            None => s.resume(),
                        }
                    }
                }
            }
            Stage::Account(a) => match a.step(ui, cx)? {
                AccountEvent::Back => {
                    self.pop();
                    let name = cx.online.account.as_ref().map(|a| a.name.clone());
                    if let Stage::Hub(h) = &mut self.stage {
                        h.player_info(false);
                        h.set_account(name.as_deref());
                    }
                }
            },
            Stage::Board(b) => match b.step(ui, cx)? {
                BoardEvent::Back => cx.fade_to(FadeKind::Black, Go::World(WorldGo::Hub)),
                BoardEvent::Maker(id, name) => self.show_maker(cx, &id, &name),
            },
            Stage::Page(p) => match p.step(ui, cx)? {
                PageEvent::Back => {
                    self.pop();
                }
                PageEvent::Maker(id, name) => self.show_maker(cx, &id, &name),
                PageEvent::Ghost(c) => {
                    cx.fade_to(FadeKind::Black, Go::World(WorldGo::Start(c)));
                }
            },
            Stage::Comments(c) => match c.step(ui, cx)? {
                CommentsEvent::Back => {
                    self.pop();
                }
                CommentsEvent::Post => {
                    let code = c.code.clone();
                    if let Some(p) = PostScene::new(&self.ui, &code) {
                        self.push(Stage::Post(Box::new(p)));
                    }
                }
            },
            Stage::Post(p) => match p.step(ui, cx)? {
                PostEvent::Cancel => {
                    self.pop();
                }
                PostEvent::Posted => {
                    self.pop();
                    if let Stage::Comments(c) = &mut self.stage {
                        c.reload(cx);
                    }
                }
            },
            Stage::Playing(p, c) => match p.step(input, cx, None) {
                // (the course keeps animating under the fade out)
                _ if self.leaving => {}
                Some(PlayEvent::Exit) => {
                    // the course stays on screen under the fade
                    self.leaving = true;
                    cx.fade_to(FadeKind::Black, Go::World(WorldGo::AfterPlay));
                }
                Some(PlayEvent::Cleared) => {
                    if let Source::Server(code) = &c.source {
                        let (code, ticks) = (code.clone(), p.clear_ticks);
                        self.sending.send(cx, &code, ticks, c.world_record_ms);
                    }
                }
                None => {}
            },
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        if self.stage.overlay()
            && let Some(u) = self.under.last_mut()
        {
            u.draw(out);
            // the windows dim the screen under them [inf: as the pause menu's veil]
            if !matches!(self.stage, Stage::Account(_)) {
                let veil = self.veil.get_or_insert_with(|| {
                    let mut f = self.ui.layout("Cmn_Fade_00");
                    if let Some(f) = &mut f {
                        f.hold_end("FadeOutB");
                        for p in ["P_FadeW_00", "N_Pos_00", "N_Block_00", "N_CaptureFade_00"] {
                            f.show(p, false);
                        }
                        f.set_alpha("P_FadeB_00", 200.0);
                    }
                    f
                });
                if let Some(v) = veil {
                    push_layout(out, v, 0);
                }
            }
        }
        self.stage.draw(out);
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

    /// Captures: the stage shown ("hub", "list", "maker", "playing", "account", "leaderboards",
    /// "page", "comments", "post").
    pub fn stage_name(&self) -> &'static str {
        match &self.stage {
            Stage::Hub(_) => "hub",
            Stage::List(s) if s.variant == ListVariant::Maker => "maker",
            Stage::List(_) => "list",
            Stage::Playing(..) => "playing",
            Stage::Account(_) => "account",
            Stage::Board(_) => "leaderboards",
            Stage::Page(_) => "page",
            Stage::Comments(_) => "comments",
            Stage::Post(_) => "post",
        }
    }

    /// Captures and tests: open a screen directly ("account", "leaderboards", the course page /
    /// comments / comment form of the list's first server course, "maker" of its uploader,
    /// "play" it, its "pause" menu or its "clear" screen).
    pub fn debug_open(&mut self, cx: &mut Cx, what: &str) -> bool {
        match what {
            "account" => {
                if let Stage::Hub(h) = &mut self.stage {
                    h.player_info(true);
                }
                match AccountScene::new(&self.ui, cx) {
                    Some(a) => self.push(Stage::Account(Box::new(a))),
                    None => return false,
                }
            }
            "leaderboards" => self.go(cx, WorldGo::Leaderboards),
            "clear" | "play" | "pause" => {
                let Some(c) = self.first_server_course() else {
                    return false;
                };
                self.start(cx, c);
                if what == "clear"
                    && let Stage::Playing(p, _) = &mut self.stage
                {
                    p.debug("course-clear", cx, None);
                    // the capture's presses go to the clear screen (menu ticks)
                    p.script = Some(std::mem::take(&mut self.script));
                }
                if what == "pause"
                    && let Stage::Playing(p, _) = &mut self.stage
                {
                    p.auto_pause_tick = Some(1);
                    p.script = Some(std::mem::take(&mut self.script));
                }
            }
            "page" | "comments" | "maker" | "post" => {
                let Some(c) = self.first_server_course() else {
                    return false;
                };
                let code = c.server_code().unwrap_or_default().to_string();
                match what {
                    "page" => match CoursePageScene::new(&self.ui, c, cx) {
                        Some(p) => self.push(Stage::Page(Box::new(p))),
                        None => return false,
                    },
                    "maker" => {
                        let id = c.maker_id.clone().unwrap_or_default();
                        let name = c.maker.clone().unwrap_or_default();
                        self.show_maker(cx, &id, &name);
                    }
                    _ => {
                        match CommentsScene::new(&self.ui, &code, cx) {
                            Some(v) => self.push(Stage::Comments(Box::new(v))),
                            None => return false,
                        }
                        if what == "post" {
                            match PostScene::new(&self.ui, &code) {
                                Some(p) => self.push(Stage::Post(Box::new(p))),
                                None => return false,
                            }
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// The course list's first server course (waits for the server's list).
    fn first_server_course(&mut self) -> Option<WorldCourse> {
        let Stage::List(s) = &mut self.stage else {
            return None;
        };
        s.all.iter().find(|c| c.is_server()).cloned()
    }

    /// Captures: wait for the server's course list (at most `ticks` polls of 10 ms).
    pub fn wait_server(&mut self, cx: &mut Cx, ticks: u32) {
        for _ in 0..ticks {
            if !self.waiting_server {
                return;
            }
            if let Stage::List(s) = &mut self.stage
                && let Some(found) = cx.host.poll_server_courses()
            {
                self.waiting_server = false;
                let mut all = self.local.clone();
                all.extend(found);
                s.set_courses(all, false);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// A `maker` answer's courses as list entries (cleared marks from the save data when known).
fn maker_courses(v: &Value, cx: &mut Cx) -> Vec<WorldCourse> {
    v["courses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(WorldCourse::server)
        .map(|mut c| {
            c.date_offset = c.date.map_or(0, |d| cx.host.local_offset(d));
            if let Some(k) = &c.key {
                c.cleared = cx.saves.record(k).cleared;
            }
            c
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Saves;
    use crate::online::{Account, Online};
    use crate::play::tests::{MockHost, with_online};

    #[test]
    fn server_listing_fields() {
        let c = WorldCourse::server(&json!({
            "code": "ABC-DEF-GHJ", "name": "Sky", "maker": "Ada", "maker_id": "M1",
            "plays": 10, "clears": 4, "attempts": 20, "likes": 3, "uploaded": 1_700_000_000,
            "record_ms": 42_000, "record_holder": "M2", "first_clear": "",
            "record_verified": true, "description": "hi"
        }))
        .unwrap();
        assert_eq!(c.server_code(), Some("ABC-DEF-GHJ"));
        assert_eq!(c.clear_rate, Some(20.0));
        assert_eq!(c.world_record_ms, Some(42_000));
        assert_eq!(c.maker_id.as_deref(), Some("M1"));
        assert_eq!(c.first_clear, None);
        assert!(c.record_verified);
        assert_eq!(c.date, Some(1_700_000_000));
    }

    pub fn signed_in() -> Online {
        let mut o = Online::default();
        o.account = Some(Account {
            name: "Ada".into(),
            maker_id: "M1".into(),
            token: "t".into(),
        });
        o
    }

    #[test]
    fn clears_go_to_the_server_by_account_and_options() {
        let mut host = MockHost {
            server: true,
            ..Default::default()
        };
        let mut saves = Saves::default();
        let mut sender = ClearSender::default();
        // signed out: an anonymous clear report
        let mut anon = Online::default();
        with_online(&mut host, &mut saves, &mut anon, |cx| {
            sender.send(cx, "AAA", Some(600), None)
        });
        assert_eq!(host.reports, [("AAA".to_string(), "clear".to_string())]);
        assert!(host.calls.is_empty());
        // signed in: the replay; refused, a plain clear with the time (600 ticks = 10 s)
        let mut me = signed_in();
        with_online(&mut host, &mut saves, &mut me, |cx| {
            sender.send(cx, "AAA", Some(600), None)
        });
        assert_eq!(host.calls[0].1, "submit_replay");
        assert_eq!(host.calls[0].3.as_deref(), Some("t"));
        host.answers
            .push((host.calls[0].0, Err("not verified".into())));
        with_online(&mut host, &mut saves, &mut me, |cx| {
            cx.online.poll(cx.host);
            sender.collect(cx);
        });
        assert_eq!(host.calls[1].1, "clear");
        assert_eq!(host.calls[1].2, json!({"code": "AAA", "time_ms": 10_000}));
        // a refused plain clear isn't retried
        host.answers.push((host.calls[1].0, Err("no".into())));
        with_online(&mut host, &mut saves, &mut me, |cx| {
            cx.online.poll(cx.host);
            sender.collect(cx);
        });
        assert_eq!(host.calls.len(), 2);
        // SMM1 play options: a clear without a time, never a replay
        host.affects_play = true;
        with_online(&mut host, &mut saves, &mut me, |cx| {
            sender.send(cx, "BBB", Some(60), None)
        });
        assert_eq!(host.calls[2].1, "clear");
        assert_eq!(host.calls[2].2["time_ms"], 0);
    }
}
