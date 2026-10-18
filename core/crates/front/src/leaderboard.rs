//! Leaderboards over the open-source course server, on SMM2's own leaderboard screen
//! `Nwk_CourseListMaker_00` (the hub's Leaderboards button): its tabs Course Makers (our
//! `maker_points`) and Players (courses cleared, first clears, world records, Endless Challenge
//! high scores per difficulty, the categories of its sort button `Nwk_SortBtn_01`
//! TypeClearCourse), rows `Nwk_MakerData_00` (rank, name, the score under its label). The
//! Endless categories show the 100 Mario boards (`hundred_<d>`, completed runs): the runs played
//! on SMCraft's Endless screens follow SMM1's rules, so SMM2's own Endless boards (`endless_<d>`,
//! the recomp's) would be empty here [departure].
//!
//! ←/→ step through the boards (the Players tab's categories in the game's order) and ↑/↓ move
//! along the rows; confirm opens the maker's profile. The game picks a Players category in a
//! sort window; here the sort button cycles it [inf]. Versus, followed makers, friends and
//! official makers have no data on our server and are hidden, as are Miis, flags, medals and the
//! last-played date [departure]. The own row uses the rows' TypeOwn look [inf].

use serde_json::{Value, json};

use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, Loading, MenuCursor, UiInput, hit, set_loading};
use crate::sound::Event;
use crate::{Cx, DrawItem, push_layout};

pub const LAYOUT: &str = "Nwk_CourseListMaker_00";

/// The boards: server kind, tab (0 Course Makers, 1 Players), Players category (the sort
/// button's TypeClearCourse frame / `ClearCourse_000n` message), score label message.
pub const KINDS: [(&str, usize, Option<usize>, &str); 8] = [
    ("maker_points", 0, None, "1000"),
    ("clears", 1, Some(0), "1001"),
    ("first_clears", 1, Some(1), "1001"),
    ("records", 1, Some(2), "1001"),
    ("hundred_easy", 1, Some(3), "1003"),
    ("hundred_normal", 1, Some(4), "1003"),
    ("hundred_expert", 1, Some(5), "1003"),
    ("hundred_super_expert", 1, Some(6), "1003"),
];

const TABS: [&str; 2] = ["L_CoursePointRankBtn_00", "L_CourseRankBtn_00"];
const SORT: &str = "L_SortBtn_00";
const SLOTS: i64 = 7;
/// The scissor (580 high) shows the five middle lines of the seven [inf from the layout].
const VISIBLE: i64 = 5;
/// Rows asked for per board.
pub const LIMIT: u64 = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rank: u32,
    pub score: i64,
    pub maker_id: String,
    pub name: String,
}

impl Row {
    pub fn parse(v: &Value) -> Vec<Row> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Row {
                    rank: r["rank"].as_u64()? as u32,
                    score: r["score"].as_i64().unwrap_or(0),
                    maker_id: r["maker_id"].as_str()?.to_string(),
                    name: r["name"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BoardEvent {
    Back,
    /// A maker's profile (maker ID, name).
    Maker(String, String),
}

pub struct LeaderboardScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    pub kind: usize,
    pub rows: Vec<Row>,
    pub selection: i64,
    first: i64,
    waiting: Option<u32>,
    busy_until: i32,
    leaving: bool,
    me: Option<String>,
}

impl LeaderboardScene {
    pub fn new(ui: &Ui, cx: &mut Cx) -> Option<Self> {
        let mut l = ui.layout(LAYOUT)?;
        l.play("In", 0);
        l.text_from("L_PageBtns_00/T_Title_00", "L_PageBtns_00-T_Title_00");
        for p in [
            "L_BattleModeBtn_00",
            "L_FolloweeBtn_00",
            "L_FridendBtn_00",
            "L_OfficialUserBtn_00",
            "L_PageBtns_00/N_PageBtns_00",
            "N_PageTopBtn_00",
            "L_UpdateBtn_00",
            "L_ScrollBar_00",
        ] {
            l.show(p, false);
        }
        for t in TABS {
            l.text_from(&format!("{t}/T_Btn_00"), &format!("{t}-T_Btn_00"));
            l.hold(&format!("{t}/Select"), 0.0);
        }
        l.text_from(&format!("{SORT}/T_Sort_00"), "L_SortBtn_00-T_Sort_00");
        let busy = l.ticks("In", 1);
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            kind: 0,
            rows: Vec::new(),
            selection: 0,
            first: 0,
            waiting: None,
            busy_until: busy,
            leaving: false,
            me: cx.online.maker_id().map(str::to_string),
        };
        s.load(cx);
        Some(s)
    }

    /// Ask the server for the current board.
    fn load(&mut self, cx: &mut Cx) {
        self.rows.clear();
        self.selection = 0;
        self.first = 0;
        let kind = KINDS[self.kind].0;
        self.waiting = cx
            .online
            .call(cx.host, "rankings", json!({ "kind": kind, "limit": LIMIT }));
        self.show_kind();
        if self.waiting.is_none() {
            self.set_status(Some("No course server is set (--server or MMK_SERVER)."));
        } else {
            self.set_status(None);
        }
        self.fill();
    }

    /// The loading icon while waiting, the screen's "no makers" message (or `text`) when empty.
    fn set_status(&mut self, text: Option<&str>) {
        let state = if self.waiting.is_some() {
            Loading::Waiting
        } else if self.rows.is_empty() {
            let msg = text
                .map(str::to_string)
                .or_else(|| self.layout.message("NoData_0005"))
                .unwrap_or_default();
            Loading::Message(msg)
        } else {
            Loading::Hidden
        };
        set_loading(&mut self.layout, "L_LoadingIcon_00", &state, self.tick);
    }

    /// Tabs and the sort button for the current board.
    fn show_kind(&mut self) {
        let (_, tab, cat, _) = KINDS[self.kind];
        let l = &mut self.layout;
        for (k, t) in TABS.iter().enumerate() {
            let a = format!("{t}/Decide");
            if k == tab {
                l.hold_end(&a);
            } else {
                l.stop(&a);
            }
        }
        match cat {
            Some(c) => {
                l.hold(&format!("{SORT}/TypeForm"), 2.0);
                l.hold(&format!("{SORT}/TypeClearCourse"), c as f32);
                l.text_from(
                    &format!("{SORT}/T_ClearCourse_00"),
                    &format!("ClearCourse_{c:04}"),
                );
            }
            None => {
                l.hold(&format!("{SORT}/TypeForm"), 0.0);
                l.hold(&format!("{SORT}/TypePeriod"), 0.0);
                l.text_from(&format!("{SORT}/T_Period_00"), "Period_0000");
            }
        }
    }

    fn slot(k: i64) -> String {
        format!("L_MakerData_{k:02}")
    }

    fn row_pane(&self, i: i64) -> String {
        Self::slot(i - self.first + 1)
    }

    fn fill(&mut self) {
        let label = self.layout.message(KINDS[self.kind].3).unwrap_or_default();
        for k in 0..SLOTS {
            let pane = Self::slot(k);
            let i = self.first + k - 1;
            let l = &mut self.layout;
            if !(1..=VISIBLE).contains(&k) || i < 0 || i >= self.rows.len() as i64 {
                l.show(&pane, false);
                continue;
            }
            let r = self.rows[i as usize].clone();
            l.visible(&pane, None);
            l.hold(&format!("{pane}/Type"), 0.0);
            for p in [
                "L_MiiIcon_00",
                "L_NationalFlag_00",
                "N_BadgeList_00",
                "N_Day_00",
                "P_Follow_00",
            ] {
                l.show(&format!("{pane}/{p}"), false);
            }
            l.text(
                &format!("{pane}/L_RankCheck_00/T_Ranking_00"),
                &r.rank.to_string(),
            );
            l.text(&format!("{pane}/T_PlayerName_00"), &r.name);
            l.text(&format!("{pane}/T_Points_00"), &label);
            l.text(
                &format!("{pane}/L_Count_00/T_Count_00"),
                &r.score.to_string(),
            );
            let own = self.me.as_deref() == Some(r.maker_id.as_str());
            l.hold(&format!("{pane}/TypeOwn"), if own { 1.0 } else { 0.0 });
            let sel = format!("{pane}/Select");
            let f = if i == self.selection {
                l.length(&sel)
            } else {
                0.0
            };
            l.hold(&sel, f);
        }
    }

    pub fn step(&mut self, input: &UiInput, cx: &mut Cx) -> Option<BoardEvent> {
        self.tick += 1;
        if let Some(id) = self.waiting
            && let Some(a) = cx.online.take(id)
        {
            self.waiting = None;
            match a {
                Ok(v) => {
                    self.rows = Row::parse(&v);
                    self.set_status(None);
                }
                Err(e) => {
                    let msg = format!("The server couldn't be reached: {e}");
                    self.set_status(Some(&msg));
                }
            }
            self.fill();
        }
        let mut ev = None;
        if !self.leaving && self.tick >= self.busy_until {
            let mut pressed = input.pressed;
            if let Some(p) = input.click {
                pressed = pressed | self.click(p, cx);
            }
            ev = self.handle(pressed, cx);
        }
        if self.leaving || self.rows.is_empty() {
            self.cursor.hide();
        } else {
            let pane = format!("{}/B_Btn_00", self.row_pane(self.selection));
            self.cursor.place(&mut self.layout, &pane, self.tick, 1.0);
        }
        ev
    }

    /// A click: a tab, the sort button, or a row (selects it; on the selected one, confirms).
    fn click(&mut self, p: [f32; 2], cx: &mut Cx) -> Buttons {
        let tabs: Vec<String> = TABS.iter().map(|t| format!("{t}/B_Btn_00")).collect();
        if let Some(t) = hit(&mut self.layout, self.tick, &tabs, p) {
            let k = KINDS.iter().position(|k| k.1 == t).unwrap_or(0);
            if KINDS[self.kind].1 != t {
                self.set_kind(k, cx);
            }
            return Buttons::NONE;
        }
        let sort = [format!("{SORT}/B_Btn_00")];
        if hit(&mut self.layout, self.tick, &sort, p).is_some() {
            self.ui.sound_button(LAYOUT, SORT, Event::Decide);
            self.layout.play(&format!("{SORT}/Decide"), self.tick);
            if KINDS[self.kind].1 == 1 {
                // cycle the Players categories [inf: the game opens a sort window]
                let k = if self.kind + 1 < KINDS.len() {
                    self.kind + 1
                } else {
                    1
                };
                self.set_kind(k, cx);
            }
            return Buttons::NONE;
        }
        let rows: Vec<(i64, String)> = (self.first..self.first + VISIBLE)
            .filter(|&i| i >= 0 && (i as usize) < self.rows.len())
            .map(|i| (i, format!("{}/B_Btn_00", self.row_pane(i))))
            .collect();
        let panes: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
        if let Some(k) = hit(&mut self.layout, self.tick, &panes, p) {
            let i = rows[k].0;
            if i == self.selection {
                return Buttons::CONFIRM;
            }
            self.move_by(i - self.selection);
        }
        Buttons::NONE
    }

    fn set_kind(&mut self, k: usize, cx: &mut Cx) {
        let tab = KINDS[k].1;
        self.ui.sound_button(LAYOUT, TABS[tab], Event::Decide);
        self.layout
            .play(&format!("{}/Decide", TABS[tab]), self.tick);
        self.kind = k;
        self.load(cx);
    }

    fn handle(&mut self, pressed: Buttons, cx: &mut Cx) -> Option<BoardEvent> {
        if pressed.has(Buttons::BACK) {
            self.ui.sound_back(LAYOUT, "normal");
            self.layout.play("Out", self.tick);
            self.leaving = true;
            return Some(BoardEvent::Back);
        }
        if pressed.has(Buttons::LEFT) {
            let k = (self.kind + KINDS.len() - 1) % KINDS.len();
            self.set_kind(k, cx);
        } else if pressed.has(Buttons::RIGHT) {
            let k = (self.kind + 1) % KINDS.len();
            self.set_kind(k, cx);
        } else if pressed.has(Buttons::UP) {
            self.move_by(-1);
        } else if pressed.has(Buttons::DOWN) {
            self.move_by(1);
        } else if pressed.has(Buttons::CONFIRM)
            && let Some(r) = self.rows.get(self.selection as usize)
        {
            let pane = self.row_pane(self.selection);
            self.ui.sound_button(LAYOUT, &pane, Event::Decide);
            self.layout.play(&format!("{pane}/Decide"), self.tick);
            return Some(BoardEvent::Maker(r.maker_id.clone(), r.name.clone()));
        }
        None
    }

    fn move_by(&mut self, d: i64) {
        if self.rows.is_empty() {
            return;
        }
        let n = (self.selection + d).clamp(0, self.rows.len() as i64 - 1);
        if n == self.selection {
            return;
        }
        self.selection = n;
        if self.selection < self.first {
            self.first = self.selection;
        }
        if self.selection > self.first + VISIBLE - 1 {
            self.first = self.selection - VISIBLE + 1;
        }
        self.ui
            .sound_button(LAYOUT, "L_MakerData_00", Event::Select);
        self.fill();
        let pane = self.row_pane(self.selection);
        self.layout.play(&format!("{pane}/Select"), self.tick);
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
    fn kinds_match_the_server() {
        // mmk_server::Store::rankings' kinds, in the game's category order
        let kinds: Vec<&str> = KINDS.iter().map(|k| k.0).collect();
        assert_eq!(kinds[0], "maker_points");
        assert!(kinds[4..].iter().all(|k| k.starts_with("hundred_")));
        let rows = Row::parse(&json!([
            {"rank": 1, "score": 12, "maker_id": "A", "name": "Ada"},
            {"rank": 2, "maker_id": "B"},
            {"score": 3}
        ]));
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].rank, rows[0].score, rows[1].score), (1, 12, 0));
    }
}
