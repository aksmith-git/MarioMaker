//! A server course's page and its comments, on SMM2's own layouts:
//! - the course page: `Nwk_CourseListWindow_00` in its course mode (Type 2, contents 2 =
//!   `Nwk_CourseDetail_00`, opened by "More info" on an open course row, over the list as the
//!   game's window): the name in the title, the maker (its Mii button opens the maker's profile),
//!   likes and plays, the first clearer and the world-record holder with the time. The record
//!   holder's button races the record's ghost (`ghost`, then play) [inf: the game opens the
//!   holder's profile there; ghosts of records are ours]; the first clearer's opens their
//!   profile. Refreshed from the server (`info`) when it opens. The "Cleared by" / "Liked by"
//!   column shows the layout's "no one yet" message: the server doesn't list players [departure].
//! - the comments: `Nwk_CommentViewer_00` ("View comments" on an open row): text comments
//!   (`Nwk_CommentHistory_00` Type 1, left and right in turn) with the poster's name and the
//!   date (absolute: the host gives no clock for the game's "n hours ago") [inf]; confirm opens
//!   `Nwk_CommentPosting_00` in its text form to type a comment (signed in) [inf: the game posts
//!   from the course's pause and clear screens].
//!
//! Thumbnails, Miis, flags, medals, tags (the server has none: the layout's "none" marks),
//! drawings, stamps, spoilers and reporting are hidden [departure].

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::host::{Source, WorldCourse};
use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, Loading, MenuCursor, UiInput, hit, nearest, set_loading};
use crate::online::{edit, time_text};
use crate::sound::Event;
use crate::{Cx, DrawItem, civil, push_layout};

pub const WINDOW: &str = "Nwk_CourseListWindow_00";
const DETAIL: &str = "L_CourseDetail_00";

/// What the course page's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageButton {
    Maker,
    FirstClear,
    Record,
    Back,
}

pub const PAGE_BUTTONS: [(&str, PageButton); 4] = [
    ("L_CourseDetail_00/L_MiiBtn_00/B_Btn_00", PageButton::Maker),
    (
        "L_CourseDetail_00/L_RecordBtn_00/B_Btn_00",
        PageButton::FirstClear,
    ),
    (
        "L_CourseDetail_00/L_RecordBtn_01/B_Btn_00",
        PageButton::Record,
    ),
    ("L_BackBtn_00/B_Btn_00", PageButton::Back),
];

#[derive(Clone, Debug, PartialEq)]
pub enum PageEvent {
    Back,
    /// A maker's profile (maker ID, name).
    Maker(String, String),
    /// The record's ghost is ready: play the course racing it.
    Ghost(Box<WorldCourse>),
}

pub struct CoursePageScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    pub course: WorldCourse,
    pub selection: usize,
    /// Names of maker IDs (record holder, first clearer) as they arrive.
    names: HashMap<String, String>,
    calls: Vec<(u32, Call)>,
    busy_until: i32,
    leaving: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Info,
    Name(String),
    Ghost,
}

impl CoursePageScene {
    pub fn new(ui: &Ui, course: WorldCourse, cx: &mut Cx) -> Option<Self> {
        let mut l = ui.layout(WINDOW)?;
        l.play("In", 0);
        l.hold("Type", 2.0);
        l.hold("TypeContents", 2.0);
        l.show("L_UpLoadInfo_00", false);
        l.show("L_LoadingIcon_00", false);
        let d = DETAIL;
        for p in [
            "L_Thumbnail_00",
            "L_Thumbnail_01",
            "P_DummyThumbnailBG_00",
            "T_MakerClearTime_00",
            "L_NationalFlag_00",
            "N_BadgeList_00",
            "N_Count_02",
            "L_TagBtn_00",
            "L_ScrollBar_00",
            "N_Scissor_00",
            "L_SortBtnFootprint_01",
            "L_MiiBtn_00/L_MiiIcon_00",
            "L_RecordBtn_00/L_MiiIcon_00",
            "L_RecordBtn_01/L_MiiIcon_00",
            "L_RecordBtn_00/N_NationalFlag_00",
            "L_RecordBtn_01/N_NationalFlag_00",
            "L_RecordBtn_00/L_NationalFlag_00",
            "L_RecordBtn_01/L_NationalFlag_00",
        ] {
            l.show(&format!("{d}/{p}"), false);
        }
        for (pane, label) in [
            ("T_Good_00", "L_CourseDetail_00-T_Good_00"),
            ("T_Play_00", "L_CourseDetail_00-T_Play_00"),
            ("T_First_00", "L_CourseDetail_00-T_First_00"),
            ("T_Fast_00", "L_CourseDetail_00-T_Fast_00"),
            (
                "L_SortBtnFootprint_00/T_BtnText_00",
                "L_CourseDetail_00-L_SortBtnFootprint_00-T_BtnText_00",
            ),
        ] {
            l.text_from(&format!("{d}/{pane}"), label);
        }
        // no tags on the server: the layout's "none" marks (TypeTag 2)
        l.hold(&format!("{d}/TypeTag"), 2.0);
        // the players column: the layout's own "no one yet" [departure: no player lists]
        let none = l.message("NoData_0014").unwrap_or_default();
        set_loading(
            &mut l,
            &format!("{d}/L_LoadingIcon_01"),
            &Loading::Message(none),
            0,
        );
        let busy = l.ticks("In", 1);
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            course,
            selection: 0,
            names: HashMap::new(),
            calls: Vec::new(),
            busy_until: busy,
            leaving: false,
        };
        if let Some(code) = s.code()
            && let Some(id) = cx.online.call(cx.host, "info", json!({ "code": code }))
        {
            s.calls.push((id, Call::Info));
        }
        s.ask_names(cx);
        s.fill();
        Some(s)
    }

    fn code(&self) -> Option<String> {
        match &self.course.source {
            Source::Server(c) => Some(c.clone()),
            Source::Library(_) => self.course.code.clone(),
        }
    }

    /// Ask for the names behind the record holder's and first clearer's maker IDs.
    fn ask_names(&mut self, cx: &mut Cx) {
        let ids = [
            self.course.record_holder.clone(),
            self.course.first_clear.clone(),
        ];
        for id in ids.into_iter().flatten() {
            if id.is_empty()
                || self.names.contains_key(&id)
                || self.calls.iter().any(|c| c.1 == Call::Name(id.clone()))
            {
                continue;
            }
            if let Some(k) = cx.online.call(cx.host, "maker", json!({ "maker_id": id })) {
                self.calls.push((k, Call::Name(id)));
            }
        }
    }

    fn name_of(&self, id: &Option<String>) -> Option<String> {
        let id = id.as_ref().filter(|i| !i.is_empty())?;
        Some(self.names.get(id).cloned().unwrap_or_else(|| id.clone()))
    }

    fn fill(&mut self) {
        let c = self.course.clone();
        let first = self.name_of(&c.first_clear);
        let holder = self.name_of(&c.record_holder);
        let l = &mut self.layout;
        let d = DETAIL;
        l.text("N_Title_01/T_CourseName_00", &c.name);
        l.text("T_CourseName_00", &c.name);
        l.text(
            &format!("{d}/T_PlayerName_00"),
            c.maker.as_deref().unwrap_or("-"),
        );
        let n = |v: Option<i64>| v.map_or("-".to_string(), |v| v.to_string());
        l.text(&format!("{d}/L_GoodCount_00/T_Count_00"), &n(c.likes));
        l.text(&format!("{d}/L_PlayerCount_00/T_Count_00"), &n(c.plays));
        for (btn, who) in [("L_RecordBtn_00", &first), ("L_RecordBtn_01", &holder)] {
            let b = format!("{d}/{btn}");
            match who {
                Some(name) => {
                    l.stop(&format!("{b}/Invalid"));
                    l.text(&format!("{b}/T_PlayerName_00"), name);
                }
                // nobody yet: the button's blank face [inf]
                None => l.hold(&format!("{b}/Invalid"), 1.0),
            }
        }
        match c.world_record_ms.filter(|&ms| ms > 0) {
            Some(ms) => {
                let (a, b) = time_text(l, ms);
                l.visible(&format!("{d}/N_Time_00"), None);
                l.text(&format!("{d}/T_Time_00"), &a);
                l.text(&format!("{d}/T_Time_01"), &b);
            }
            None => l.show(&format!("{d}/N_Time_00"), false),
        }
    }

    /// The buttons drawn now (indices into [`PAGE_BUTTONS`]).
    fn visible_buttons(&mut self) -> Vec<usize> {
        self.layout.apply(self.tick);
        (0..PAGE_BUTTONS.len())
            .filter(|&i| {
                self.layout
                    .rect(PAGE_BUTTONS[i].0)
                    .is_some_and(|r| r.visible && r.alpha > 0.01)
            })
            .collect()
    }

    pub fn step(&mut self, input: &UiInput, cx: &mut Cx) -> Option<PageEvent> {
        self.tick += 1;
        let mut ev = self.collect(cx);
        if ev.is_none() && !self.leaving && self.tick >= self.busy_until {
            let mut pressed = input.pressed;
            if let Some(p) = input.click {
                let panes: Vec<String> = PAGE_BUTTONS.iter().map(|b| b.0.to_string()).collect();
                if let Some(i) = hit(&mut self.layout, self.tick, &panes, p) {
                    self.selection = i;
                    pressed = pressed | Buttons::CONFIRM;
                }
            }
            ev = self.handle(pressed, cx);
        }
        let waiting = self.calls.iter().any(|c| c.1 == Call::Ghost);
        let alpha = if self.leaving || waiting { 0.0 } else { 1.0 };
        let pane = PAGE_BUTTONS[self.selection].0;
        self.cursor.place(&mut self.layout, pane, self.tick, alpha);
        ev
    }

    fn collect(&mut self, cx: &mut Cx) -> Option<PageEvent> {
        let mut ev = None;
        let calls = std::mem::take(&mut self.calls);
        for (id, call) in calls {
            let Some(a) = cx.online.take(id) else {
                self.calls.push((id, call));
                continue;
            };
            match (call, a) {
                (Call::Info, Ok(v)) => {
                    if let Some(mut fresh) = WorldCourse::server(&v) {
                        fresh.cleared = self.course.cleared;
                        fresh.best_ticks = self.course.best_ticks;
                        fresh.key = self.course.key.clone();
                        self.course = fresh;
                        self.ask_names(cx);
                    }
                }
                (Call::Name(mid), Ok(v)) => {
                    if let Some(n) = v["maker"]["name"].as_str() {
                        self.names.insert(mid, n.to_string());
                    }
                }
                (Call::Ghost, Ok(_)) => ev = Some(PageEvent::Ghost(Box::new(self.course.clone()))),
                (Call::Ghost, Err(e)) => cx.toast(&format!("Couldn't get the record's run: {e}")),
                (_, Err(_)) => {}
            }
            self.fill();
        }
        ev
    }

    fn handle(&mut self, pressed: Buttons, cx: &mut Cx) -> Option<PageEvent> {
        if pressed.has(Buttons::BACK) {
            return self.leave();
        }
        if let Some(dir) = pressed.direction() {
            let vis = self.visible_buttons();
            let here = self
                .layout
                .rect(PAGE_BUTTONS[self.selection].0)
                .map_or([0.0, 0.0], |r| r.center);
            let targets: Vec<(usize, [f32; 2])> = vis
                .iter()
                .filter_map(|&i| self.layout.rect(PAGE_BUTTONS[i].0).map(|r| (i, r.center)))
                .collect();
            if let Some(n) = nearest(here, dir, &targets) {
                self.selection = n;
                self.ui.sound_button(WINDOW, DETAIL, Event::Select);
                let part = PAGE_BUTTONS[n].0.trim_end_matches("/B_Btn_00");
                self.layout.play(&format!("{part}/Select"), self.tick);
            }
            return None;
        }
        if !pressed.has(Buttons::CONFIRM) {
            return None;
        }
        let part = PAGE_BUTTONS[self.selection]
            .0
            .trim_end_matches("/B_Btn_00")
            .to_string();
        self.layout.play(&format!("{part}/Decide"), self.tick);
        self.ui.sound_button(WINDOW, &part, Event::Decide);
        let c = self.course.clone();
        match PAGE_BUTTONS[self.selection].1 {
            PageButton::Back => return self.leave(),
            PageButton::Maker => match c.maker_id.filter(|m| !m.is_empty()) {
                Some(id) => return Some(PageEvent::Maker(id, c.maker.unwrap_or_default())),
                None => cx.toast("This course was uploaded without an account."),
            },
            PageButton::FirstClear => {
                if let Some(id) = c.first_clear.clone().filter(|m| !m.is_empty()) {
                    let name = self.name_of(&c.first_clear).unwrap_or_default();
                    return Some(PageEvent::Maker(id, name));
                }
            }
            PageButton::Record => {
                if c.world_record_ms.unwrap_or(0) <= 0 {
                    cx.toast("Nobody holds the record yet.");
                } else if !c.record_verified {
                    cx.toast("The record wasn't sent as a replay, so there's no run to race.");
                } else if cx.host.play_options_affect_play() {
                    cx.toast("Ghosts race by SMM2's rules: turn off the SMM1 play options.");
                } else if let Some(code) = self.code() {
                    match cx.online.call(cx.host, "ghost", json!({ "code": code })) {
                        Some(id) => self.calls.push((id, Call::Ghost)),
                        None => cx.toast("No course server is set (--server or MMK_SERVER)."),
                    }
                }
            }
        }
        None
    }

    fn leave(&mut self) -> Option<PageEvent> {
        self.ui.sound_back(WINDOW, "normal");
        self.layout.play("Out", self.tick);
        self.leaving = true;
        Some(PageEvent::Back)
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
        self.cursor.draw(self.tick, out);
    }
}

// MARK: - comments

pub const VIEWER: &str = "Nwk_CommentViewer_00";
const COMMENT_SLOTS: i64 = 5;
/// The lines drawn: the balloons sit 170 above their line, so the scissor (650 high) shows
/// those of lines 2 and 3 [inf from the layout].
const COMMENT_VISIBLE: i64 = 2;
const COMMENT_FIRST_LINE: i64 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub maker_id: String,
    pub name: String,
    pub text: String,
    pub posted: i64,
    /// Local time minus UTC at `posted` (from the host), for the date shown.
    pub offset: i64,
}

impl Comment {
    pub fn parse(v: &Value) -> Vec<Comment> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(Comment {
                    maker_id: c["maker_id"].as_str().unwrap_or_default().to_string(),
                    name: c["name"].as_str().unwrap_or_default().to_string(),
                    text: c["text"].as_str()?.to_string(),
                    posted: c["posted"].as_i64().unwrap_or(0),
                    offset: 0,
                })
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommentsEvent {
    Back,
    /// Open the posting screen.
    Post,
}

pub struct CommentsScene {
    ui: Ui,
    pub layout: Layout,
    pub tick: i32,
    pub code: String,
    pub comments: Vec<Comment>,
    /// The first comment on screen.
    pub first: i64,
    waiting: Option<u32>,
    busy_until: i32,
    leaving: bool,
}

impl CommentsScene {
    pub fn new(ui: &Ui, code: &str, cx: &mut Cx) -> Option<Self> {
        let mut l = ui.layout(VIEWER)?;
        l.play("In", 0);
        l.text_from("L_PageBtns_00/T_Title_00", "L_PageBtns_00-T_Title_00");
        l.text_from("L_CloseBtn_00/T_Btn_00", "L_CloseBtn_00-T_Btn_00");
        // the key glyph: B, the back button (the layout's own glyph is the game's) [inf]
        l.text("L_CloseBtn_00/T_KeyBtn_00", "\u{E0E1}");
        for p in [
            "L_StampFilterBtn_00",
            "N_PageTopBtn_00",
            "L_PageBtns_00/N_PageBtns_00",
            "L_ScrollBar_00",
        ] {
            l.show(p, false);
        }
        let busy = l.ticks("In", 1);
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            tick: 0,
            code: code.to_string(),
            comments: Vec::new(),
            first: 0,
            waiting: None,
            busy_until: busy,
            leaving: false,
        };
        s.reload(cx);
        Some(s)
    }

    pub fn reload(&mut self, cx: &mut Cx) {
        self.waiting = cx
            .online
            .call(cx.host, "comments", json!({ "code": self.code }));
        self.fill();
    }

    fn slot(k: i64) -> String {
        format!("L_Comment_{k:02}")
    }

    fn fill(&mut self) {
        let state = if self.waiting.is_some() {
            Loading::Waiting
        } else if self.comments.is_empty() {
            // [ours] the server's empty list (the game's tables have no such line here)
            Loading::Message("No comments yet.".to_string())
        } else {
            Loading::Hidden
        };
        set_loading(&mut self.layout, "L_LoadingIcon_00", &state, self.tick);
        let l = &mut self.layout;
        let ui = self.ui.clone();
        for k in 0..COMMENT_SLOTS {
            let pane = Self::slot(k);
            let i = self.first + k - COMMENT_FIRST_LINE;
            let shown = COMMENT_FIRST_LINE..COMMENT_FIRST_LINE + COMMENT_VISIBLE;
            if !shown.contains(&k) || i < 0 || i >= self.comments.len() as i64 {
                l.show(&pane, false);
                continue;
            }
            let c = &self.comments[i as usize];
            l.visible(&pane, None);
            l.hold(&format!("{pane}/Type"), 1.0);
            l.hold(&format!("{pane}/TypeLR"), (i % 2) as f32);
            l.hold(&format!("{pane}/TypeOwn"), 0.0);
            let side = if i % 2 == 0 { "00" } else { "01" };
            for p in [
                "N_TypeReportBtn_00",
                "L_DeleteBtn_00",
                "L_MiiBtn_00",
                "L_MiiBtn_01",
                "L_NationlFlag_00",
                "L_NationlFlag_01",
            ] {
                l.show(&format!("{pane}/{p}"), false);
            }
            l.wrapped_text(&format!("{pane}/T_TextComment_00"), &c.text, 300.0, 6);
            l.text(&format!("{pane}/T_PlayerName_{side}"), &c.name);
            let (y, mo, d, h, mi) = civil(c.posted + c.offset);
            let day = ui
                .format(
                    "Nwk_CourseListGeneral_00",
                    "Day_0000",
                    &[format!("{mo:02}"), format!("{d:02}"), y.to_string()],
                )
                .unwrap_or_default();
            let time = ui
                .format(
                    "Nwk_CourseListGeneral_00",
                    "Time_0000",
                    &[format!("{h:02}"), format!("{mi:02}")],
                )
                .unwrap_or_default();
            l.text(
                &format!("{pane}/T_DateTime_{side}"),
                &format!("{day} {time}"),
            );
        }
    }

    pub fn step(&mut self, input: &UiInput, cx: &mut Cx) -> Option<CommentsEvent> {
        self.tick += 1;
        if let Some(id) = self.waiting
            && let Some(a) = cx.online.take(id)
        {
            self.waiting = None;
            match a {
                Ok(v) => {
                    // newest first [inf]
                    let mut c = Comment::parse(&v);
                    c.sort_by_key(|m| std::cmp::Reverse(m.posted));
                    for m in &mut c {
                        m.offset = cx.host.local_offset(m.posted);
                    }
                    self.comments = c;
                }
                Err(e) => cx.toast(&format!("The server couldn't be reached: {e}")),
            }
            self.first = 0;
            self.fill();
        }
        if self.leaving || self.tick < self.busy_until {
            return None;
        }
        let mut pressed = input.pressed;
        if let Some(p) = input.click {
            let close = ["L_CloseBtn_00/B_Btn_00".to_string()];
            if hit(&mut self.layout, self.tick, &close, p).is_some() {
                pressed = pressed | Buttons::BACK;
            } else if self.comments.len() as i64 <= COMMENT_VISIBLE {
                // nothing to scroll: a click opens the form [inf]
                pressed = pressed | Buttons::CONFIRM;
            } else if p[1] > 0.0 {
                // the upper balloon scrolls back, the lower one on [inf]
                pressed = pressed | Buttons::UP;
            } else {
                pressed = pressed | Buttons::DOWN;
            }
        }
        if pressed.has(Buttons::BACK) {
            self.ui.sound_back(VIEWER, "normal");
            self.layout.play("L_CloseBtn_00/Decide", self.tick);
            self.layout.play("Out", self.tick);
            self.leaving = true;
            return Some(CommentsEvent::Back);
        }
        let max_first = (self.comments.len() as i64 - COMMENT_VISIBLE).max(0);
        if pressed.has(Buttons::UP) && self.first > 0 {
            self.first -= 1;
            self.ui.sound_button(VIEWER, "L_Comment_00", Event::Select);
            self.fill();
        } else if pressed.has(Buttons::DOWN) && self.first < max_first {
            self.first += 1;
            self.ui.sound_button(VIEWER, "L_Comment_00", Event::Select);
            self.fill();
        } else if pressed.has(Buttons::CONFIRM) {
            if cx.online.signed_in() {
                return Some(CommentsEvent::Post);
            }
            cx.toast("Sign in from the Course World player card to comment.");
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
    }
}

// MARK: - posting a comment

pub const POSTING: &str = "Nwk_CommentPosting_00";
/// `mmk_server::MAX_COMMENT`.
pub const MAX_COMMENT: usize = 75;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostEvent {
    /// Left without posting.
    Cancel,
    /// The comment was accepted.
    Posted,
}

pub struct PostScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    pub code: String,
    pub text: String,
    waiting: Option<u32>,
    leaving: bool,
}

impl PostScene {
    pub fn new(ui: &Ui, code: &str) -> Option<Self> {
        let mut l = ui.layout(POSTING)?;
        l.play("In", 0);
        l.hold("Type", 1.0);
        l.text_from("T_Title_00", "T_Title_00");
        l.text_from("L_PostBtn_00/T_Btn_00", "L_PostBtn_00-T_Btn_00");
        for p in [
            "L_TextBtn_00",
            "L_DrawingBtn_00",
            "L_StampBtn_00",
            "L_SpoilerBtn_00",
            "P_DummyThumbnail_00",
            "N_Comment_00",
        ] {
            l.show(p, false);
        }
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            code: code.to_string(),
            text: String::new(),
            waiting: None,
            leaving: false,
        };
        s.show_text();
        Some(s)
    }

    pub fn typing(&self) -> bool {
        self.waiting.is_none() && !self.leaving
    }

    fn show_text(&mut self) {
        let caret = if self.waiting.is_none() && self.tick / 30 % 2 == 0 {
            "_"
        } else {
            ""
        };
        let t = format!("{}{caret}", self.text);
        self.layout.wrapped_text("T_TextComment_00", &t, 300.0, 4);
    }

    pub fn step(&mut self, input: &UiInput, cx: &mut Cx) -> Option<PostEvent> {
        self.tick += 1;
        if let Some(id) = self.waiting
            && let Some(a) = cx.online.take(id)
        {
            self.waiting = None;
            match a {
                Ok(_) => {
                    cx.toast("Comment posted.");
                    return self.leave(PostEvent::Posted);
                }
                Err(e) => cx.toast(&format!("The comment wasn't posted: {e}")),
            }
        }
        if self.leaving || self.waiting.is_some() {
            return None;
        }
        let mut send = input.pressed.has(Buttons::CONFIRM);
        let mut cancel = input.pressed.has(Buttons::BACK);
        if let Some(p) = input.click {
            let post = ["L_PostBtn_00/B_Btn_00".to_string()];
            send |= hit(&mut self.layout, self.tick, &post, p).is_some();
        }
        match input.text {
            Some('\u{1b}') => cancel = true,
            Some(c) => send |= edit(&mut self.text, c, MAX_COMMENT),
            None => {}
        }
        if cancel {
            return self.leave(PostEvent::Cancel);
        }
        if send && !self.text.trim().is_empty() {
            self.ui.sound_button(POSTING, "L_PostBtn_00", Event::Decide);
            self.layout.play("L_PostBtn_00/Decide", self.tick);
            let text = self.text.trim().to_string();
            self.waiting = cx.online.call(
                cx.host,
                "comment",
                json!({ "code": self.code, "text": text }),
            );
            if self.waiting.is_none() {
                cx.toast("No course server is set (--server or MMK_SERVER).");
            }
        }
        self.show_text();
        self.cursor
            .place(&mut self.layout, "L_PostBtn_00/B_Btn_00", self.tick, 1.0);
        None
    }

    fn leave(&mut self, e: PostEvent) -> Option<PostEvent> {
        self.ui.sound_back(POSTING, "normal");
        self.layout.play("Out", self.tick);
        self.leaving = true;
        Some(e)
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
        self.cursor.draw(self.tick, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_parse() {
        let c = Comment::parse(&json!([
            {"maker_id": "A", "name": "Ada", "text": "hi", "posted": 5},
            {"maker_id": "B"}
        ]));
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].name.as_str(), c[0].posted), ("Ada", 5));
    }
}
