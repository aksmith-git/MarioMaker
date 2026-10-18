//! 100 Mario Challenge screens drawn with the SMM1 layouts (the owner's SMM1 import; the
//! Swift app's `ChallengeScreens.swift`), for the "SMM1 layouts" presentation: difficulty tabs
//! over the world map, the walk and the intro card, game over, the ending and credits, and the
//! in-course overlays. Choreography the data does not pin down is marked [inf] (DECISIONS.md
//! "100 Mario screens: SMM1 layouts").

use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, UiInput, hit};
use crate::{DrawItem, push_layout};

/// What the intro card shows.
#[derive(Clone, Debug)]
pub struct RunCardInfo {
    pub name: String,
    pub maker: String,
    pub code: Option<String>,
    pub lives: u32,
    pub number: u32,
    pub total: u32,
}

impl Default for RunCardInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            maker: String::new(),
            code: None,
            lives: 100,
            number: 1,
            total: 8,
        }
    }
}

pub const RUN_LENGTHS: [usize; 4] = [8, 16, 16, 6];

/// Fill a `Play_CourseIn100Mario_00` instance: texts, and one small Mario per life (the
/// formation is 100 Marios; the one that steps forward in AnimB is #99, so the hidden ones are
/// the first).
pub fn configure_card(card: &mut Layout, info: &RunCardInfo) {
    card.text("T_CourseName_00", &info.name);
    card.text("T_PlayerName_00", &info.maker);
    match &info.code {
        Some(code) => {
            card.text("T_IDNumber_00", code);
            card.visible("N_CourseID_00", None);
        }
        None => card.show("N_CourseID_00", false),
    }
    card.text("T_Zanki_00", &info.lives.to_string());
    card.text("T_CourseNum_00", &format!("{}/{}", info.number, info.total));
    card.show("L_NF_00", false); // national flag: no region data
    card.show("P_NFBase_00", false);
    let shown = info.lives.min(100) as usize;
    for i in 0..100 {
        card.visible(
            &format!("P_MarioSml_{i:02}"),
            if i >= 100 - shown { None } else { Some(false) },
        );
    }
}

// MARK: - world map

const SUFFIX: [&str; 4] = ["Esy", "Nml", "Exp", "SpExp"];
const MARIO: &str = "N_WorldMapMario_00";
const MARIO_PART: &str = "L_WorldMapMario_00";

/// One difficulty's world map (`Nwk_WorldMap{Esy,Nml,Exp,SpExp}_00`). Mario's stops are found
/// from the data: the MoveMario frame at which his pane reaches each course dot.
pub struct WorldMap {
    pub layout: Layout,
    pub difficulty: usize,
    pub count: usize,
    stops: Vec<f32>,
}

impl WorldMap {
    pub fn new(ui: &Ui, d: usize) -> Option<Self> {
        let l = ui.layout(&format!("Nwk_WorldMap{}_00", SUFFIX[d]))?;
        let mut m = Self {
            layout: l,
            difficulty: d,
            count: RUN_LENGTHS[d],
            stops: Vec::new(),
        };
        m.stops = m.compute_stops();
        let l = &mut m.layout;
        // resting loops; the beat bounces run on a fixed period here (the game syncs them to
        // the music) [inf]
        for a in ["Default", "Beat_00_00", "Beat_00_01"] {
            l.play_loop(a, 0);
        }
        for c in ["L_Castle_00", "L_Castle_01"] {
            l.play_loop(&format!("{c}/Default"), 0);
        }
        l.play_loop("L_WorldMapKuribo_00/Default", 0);
        l.hold(&format!("{MARIO_PART}/Stand"), 0.0);
        Some(m)
    }

    pub fn dot(&self, i: usize) -> String {
        format!("L_WorldMapCourse{}_{i:02}", SUFFIX[self.difficulty])
    }

    fn compute_stops(&mut self) -> Vec<f32> {
        let len = self.layout.length("MoveMario");
        if len <= 0.0 {
            return Vec::new();
        }
        let dots: Vec<[f32; 2]> = (0..self.count)
            .filter_map(|i| self.layout.pane(&self.dot(i)))
            .map(|p| [p.translate[0], p.translate[1]])
            .collect();
        let mut out = Vec::new();
        let mut f = 0.0;
        while f <= len && out.len() < dots.len() {
            self.layout.player.play("MoveMario", f);
            if let Some(p) = self.layout.pane(MARIO) {
                let d = dots[out.len()];
                if (p.translate[0] - d[0]).abs() < 1.5 && (p.translate[1] - d[1]).abs() < 1.5 {
                    out.push(f);
                }
            }
            f += 1.0;
        }
        self.layout.stop("MoveMario");
        while out.len() < self.count {
            out.push(len);
        }
        out
    }

    pub fn stop(&self, i: usize) -> f32 {
        if self.stops.is_empty() {
            0.0
        } else {
            self.stops[i.min(self.stops.len() - 1)]
        }
    }

    /// Dots before `n` show cleared (Type frame 1 shows the "clear" disc) [inf].
    pub fn set_cleared(&mut self, n: usize) {
        for i in 0..self.count {
            let d = self.dot(i);
            self.layout
                .hold(&format!("{d}/Type"), if i < n { 1.0 } else { 0.0 });
        }
    }

    pub fn set_counters(&mut self, lives: u32, cleared: usize) {
        self.layout.text("T_RestMarioNum_00", &lives.to_string());
        self.layout
            .text("T_ClearCourseNum_00", &format!("{cleared}/{}", self.count));
    }

    /// Mario standing on dot `i`, or still inside the start castle (`None`).
    pub fn place_mario(&mut self, i: Option<usize>) {
        let f = i.map_or(0.0, |i| self.stop(i));
        self.layout.hold("MoveMario", f);
        self.mario_stand();
    }

    pub fn mario_stand(&mut self) {
        self.layout.stop(&format!("{MARIO_PART}/Walk"));
        self.layout.stop(&format!("{MARIO_PART}/Jump"));
        self.layout.hold(&format!("{MARIO_PART}/Stand"), 0.0);
    }

    /// Walk along MoveMario from frame `from` to `to` starting at `tick`; returns the arrival.
    pub fn walk(&mut self, from: f32, to: f32, tick: i32) -> i32 {
        self.layout
            .play_ex("MoveMario", tick, from, Some(to), false, 1.0);
        self.layout.stop(&format!("{MARIO_PART}/Stand"));
        if from < 28.0 {
            self.layout.play(&format!("{MARIO_PART}/Jump"), tick);
        }
        let w = if from < 28.0 { 28 } else { 0 };
        self.layout
            .play_loop(&format!("{MARIO_PART}/Walk"), tick + w);
        tick + (to - from).ceil() as i32
    }
}

/// What the run's data says about a difficulty (asked by the select screen).
pub trait RunQuery {
    fn unlocked(&self, d: usize) -> bool;
    fn can_continue(&self, d: usize) -> bool;
    /// The Continue record (lives, courses cleared).
    fn continue_info(&self, d: usize) -> Option<(u32, usize)>;
}

/// What the SMM1 screens ask of the run.
#[derive(Debug)]
pub enum Smm1Event {
    Exit,
    /// Start a run (New Game / Continue), handing the map and title bar on.
    Start {
        difficulty: usize,
        resume: bool,
    },
    Locked(usize),
    BackToSelect,
    Play,
    EndingFinished,
}

// MARK: - difficulty select

pub const TABS: [&str; 4] = [
    "L_Sub_00/L_ListSelectBtnSub_10",
    "L_Sub_00/L_ListSelectBtnSub_00",
    "L_Sub_00/L_ListSelectBtnSub_01",
    "L_Sub_00/L_ListSelectBtnSub_08",
];

/// `Nwk_ListTitle_00` (difficulty tabs) over the selected world map in its pre-MapIn state,
/// with New Game / Continue.
pub struct ChallengeMenuScene {
    ui: Ui,
    pub title: Layout,
    maps: Vec<Option<WorldMap>>,
    leaving: Option<(WorldMap, i32)>,
    pub difficulty: usize,
    /// 0 Continue (upper button), 1 New Game.
    focus: usize,
    pub tick: i32,
}

impl ChallengeMenuScene {
    pub fn new(ui: &Ui, difficulty: usize, q: &dyn RunQuery) -> Option<Self> {
        let mut t = ui.layout("Nwk_ListTitle_00")?;
        t.play("In", 0);
        t.play_loop("Default", 0);
        t.hold("MapIn", 0.0);
        t.hold("L_Sub_00/ListTitleType", 0.0); // frame 0: the 100 Mario list
        // OpenType 0/1/2 shows 2/3/4 tabs; all four are shown, locked ones greyed
        t.hold("L_Sub_00/OpenType", 2.0);
        let mut s = Self {
            ui: ui.clone(),
            title: t,
            maps: (0..4).map(|_| None).collect(),
            leaving: None,
            difficulty,
            focus: 1,
            tick: 0,
        };
        s.prepare(difficulty, q)?;
        if let Some(m) = &mut s.maps[difficulty] {
            m.layout.play("In", 0);
        }
        s.update_tabs(q);
        Some(s)
    }

    /// The current map and the title bar, for the MapIn that follows (it continues them).
    pub fn into_handoff(mut self) -> Option<(WorldMap, Layout)> {
        let m = self.maps[self.difficulty].take()?;
        Some((m, self.title))
    }

    fn prepare(&mut self, d: usize, q: &dyn RunQuery) -> Option<()> {
        if self.maps[d].is_none() {
            let m = WorldMap::new(&self.ui, d)?;
            self.maps[d] = Some(m);
            self.refresh(d, q);
        }
        Some(())
    }

    /// Selection state of a map: before MapIn, counters and Mario from the Continue record.
    fn refresh(&mut self, d: usize, q: &dyn RunQuery) {
        let cont = q.continue_info(d);
        let Some(m) = &mut self.maps[d] else { return };
        m.layout.hold("MapIn", 0.0);
        m.set_cleared(cont.map_or(0, |c| c.1));
        m.set_counters(cont.map_or(100, |c| c.0), cont.map_or(0, |c| c.1));
        if let Some(c) = cont {
            m.place_mario(Some(c.1));
            m.layout.hold_end("AnimB"); // Peach already on the castle
        } else {
            m.place_mario(None);
            m.layout.stop("AnimB");
        }
        self.update_buttons(q);
    }

    fn state(l: &mut Layout, tick: i32, part: &str, anim: &str, frame: Option<f32>) {
        for a in ["OnOff", "Valid", "Invalid", "Decide", "Cancel"] {
            if a != anim {
                l.stop(&format!("{part}/{a}"));
            }
        }
        let p = format!("{part}/{anim}");
        match frame {
            Some(f) => l.hold(&p, f),
            None => l.play(&p, tick),
        }
    }

    fn update_tabs(&mut self, q: &dyn RunQuery) {
        for (d, tab) in TABS.iter().enumerate() {
            if d == self.difficulty {
                let f = self.title.length(&format!("{tab}/Decide"));
                Self::state(&mut self.title, self.tick, tab, "Decide", Some(f));
            } else {
                let a = if q.unlocked(d) { "Valid" } else { "Invalid" };
                Self::state(&mut self.title, self.tick, tab, a, Some(1.0));
            }
        }
    }

    fn update_buttons(&mut self, q: &dyn RunQuery) {
        let d = self.difficulty;
        let open = q.unlocked(d);
        let cont = open && q.can_continue(d);
        if !cont && self.focus == 0 {
            self.focus = 1;
        }
        let (focus, tick) = (self.focus, self.tick);
        let Some(m) = &mut self.maps[d] else { return };
        // L_StartBtn_01 = Continue (upper), L_StartBtn_00 = New Game
        for (i, part) in ["L_StartBtn_01", "L_StartBtn_00"].iter().enumerate() {
            let enabled = open && (i == 1 || cont);
            if !enabled {
                Self::state(&mut m.layout, tick, part, "Invalid", Some(1.0));
            } else if i == focus {
                Self::state(&mut m.layout, tick, part, "OnOff", Some(3.0));
            } else {
                Self::state(&mut m.layout, tick, part, "Valid", Some(1.0));
            }
        }
    }

    pub fn step(&mut self, input: &UiInput, q: &dyn RunQuery) -> Option<Smm1Event> {
        self.tick += 1;
        if self.leaving.as_ref().is_some_and(|l| self.tick >= l.1) {
            self.leaving = None;
        }
        self.maps[self.difficulty].as_ref()?;
        let mut p = input.pressed;
        if let Some(c) = input.click {
            p = p | self.click(c, q);
        }
        if p.has(Buttons::LEFT) || p.has(Buttons::RIGHT) {
            let right = p.has(Buttons::RIGHT);
            let d = if right {
                (self.difficulty + 1).min(3)
            } else {
                self.difficulty.saturating_sub(1)
            };
            self.switch_to(d, q);
            return None;
        }
        let d = self.difficulty;
        let can_continue = q.can_continue(d);
        if p.has(Buttons::UP) && can_continue {
            self.focus = 0;
            self.update_buttons(q);
        }
        if p.has(Buttons::DOWN) {
            self.focus = 1;
            self.update_buttons(q);
        }
        if p.has(Buttons::BACK) {
            return Some(Smm1Event::Exit);
        }
        if p.has(Buttons::CONFIRM) {
            if !q.unlocked(d) {
                return Some(Smm1Event::Locked(d));
            }
            let resume = self.focus == 0 && can_continue;
            if let Some(m) = &mut self.maps[d] {
                for a in ["OutL", "OutR", "InL", "InR"] {
                    m.layout.stop(a);
                }
            }
            return Some(Smm1Event::Start {
                difficulty: d,
                resume,
            });
        }
        None
    }

    /// Tests: where start button `part` of the current map is drawn.
    #[cfg(test)]
    pub(crate) fn start_button_center(&mut self, part: &str) -> Option<[f32; 2]> {
        let m = self.maps[self.difficulty].as_mut()?;
        m.layout.apply(self.tick);
        m.layout.rect(&format!("{part}/B_Btn_00")).map(|r| r.center)
    }

    /// Slide over to difficulty `d`'s map (the maps' Out / In towards the tab).
    fn switch_to(&mut self, d: usize, q: &dyn RunQuery) {
        if d == self.difficulty || self.prepare(d, q).is_none() {
            return;
        }
        let right = d > self.difficulty;
        let tick = self.tick;
        if let Some(mut m) = self.maps[self.difficulty].take() {
            m.layout.play(if right { "OutL" } else { "OutR" }, tick);
            self.leaving = Some((m, tick + 32));
        }
        self.difficulty = d;
        self.refresh(d, q);
        if let Some(n) = &mut self.maps[d] {
            n.layout.play(if right { "InR" } else { "InL" }, tick);
        }
        self.update_tabs(q);
    }

    /// A click: a difficulty tab (its map slides in), Continue / New Game (focuses and
    /// decides it). The buttons to add as if pressed.
    fn click(&mut self, c: [f32; 2], q: &dyn RunQuery) -> Buttons {
        let tick = self.tick;
        let tabs: Vec<String> = TABS.iter().map(|t| format!("{t}/B_Btn_00")).collect();
        if let Some(d) = hit(&mut self.title, tick, &tabs, c) {
            self.switch_to(d, q);
            return Buttons::NONE;
        }
        let d = self.difficulty;
        let starts = [
            "L_StartBtn_01/B_Btn_00".to_string(),
            "L_StartBtn_00/B_Btn_00".to_string(),
        ];
        let Some(m) = &mut self.maps[d] else {
            return Buttons::NONE;
        };
        match hit(&mut m.layout, tick, &starts, c) {
            Some(0) if q.can_continue(d) => {
                self.focus = 0;
                self.update_buttons(q);
                Buttons::CONFIRM
            }
            Some(1) => {
                self.focus = 1;
                self.update_buttons(q);
                Buttons::CONFIRM
            }
            _ => Buttons::NONE,
        }
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if let Some((m, _)) = &mut self.leaving {
            push_layout(out, &mut m.layout, t);
        }
        if let Some(m) = &mut self.maps[self.difficulty] {
            push_layout(out, &mut m.layout, t);
        }
        push_layout(out, &mut self.title, t);
    }
}

// MARK: - world map walk + course intro card

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    NewGame,
    Resume,
    AfterClear,
    Direct,
}

/// Between courses: the world map (MapIn after New Game / Continue, or the map sliding in after
/// a clear while Mario walks to the next dot), then the intro card `Play_CourseIn100Mario_00`.
/// Confirm skips ahead (the game skips these with A since 1.20).
pub struct MapIntroScene {
    map: Option<WorldMap>,
    title: Option<Layout>,
    card: Layout,
    pub tick: i32,
    card_start: Option<i32>,
    out_at: Option<i32>,
    walk_end: Option<i32>,
    walk_start: Option<i32>,
    walk_from: f32,
    walk_to: f32,
    finished: bool,
}

/// The card's "Out" animation length: a timer [inf].
const CARD_HOLD: i32 = 150;

impl MapIntroScene {
    /// `cleared`: courses cleared before this one (= index of this course's dot).
    pub fn new(
        ui: &Ui,
        entry: Entry,
        difficulty: usize,
        info: &RunCardInfo,
        cleared: usize,
        handoff: Option<(WorldMap, Layout)>,
    ) -> Option<Self> {
        let mut card = ui.layout("Play_CourseIn100Mario_00")?;
        configure_card(&mut card, info);
        let usable = handoff.filter(|h| h.0.difficulty == difficulty);
        let (map, title, card_start) = match entry {
            Entry::Direct => (None, None, Some(0)),
            Entry::NewGame | Entry::Resume => match usable {
                Some((m, t)) => (Some(m), Some(t), None),
                None => (WorldMap::new(ui, difficulty), None, None),
            },
            Entry::AfterClear => (WorldMap::new(ui, difficulty), None, None),
        };
        let mut s = Self {
            map,
            title,
            card,
            tick: 0,
            card_start,
            out_at: None,
            walk_end: None,
            walk_start: None,
            walk_from: 0.0,
            walk_to: 0.0,
            finished: false,
        };
        let Some(m) = &mut s.map else {
            if s.card_start.is_none() {
                s.card_start = Some(0);
            }
            s.schedule_card();
            return Some(s);
        };
        m.set_counters(info.lives, cleared);
        match entry {
            Entry::NewGame => {
                m.layout.play("MapIn", 0);
                if let Some(t) = &mut s.title {
                    t.play("MapIn", 0);
                }
                m.set_cleared(0);
                m.place_mario(None);
                let l = &mut m.layout;
                l.stop("AnimB");
                // the Goombas carry Peach to the castle under the banner, she appears on it,
                // then Mario jumps out of the start castle and walks to the first dot [inf]
                l.play("Telop", 200);
                l.play("MoveKuribo", 200);
                let b = 200 + l.length("Telop").max(l.length("MoveKuribo")) as i32;
                l.play("AnimB", b);
                s.walk_start = Some(b + 20);
                s.walk_from = 0.0;
                s.walk_to = m.stop(0);
            }
            Entry::Resume => {
                m.layout.play("MapIn", 0);
                if let Some(t) = &mut s.title {
                    t.play("MapIn", 0);
                }
                m.set_cleared(cleared);
                m.place_mario(Some(cleared));
                m.layout.hold_end("AnimB");
                s.out_at = Some(230);
            }
            Entry::AfterClear => {
                m.layout.hold_end("MapIn");
                m.layout.play("In", 0);
                m.layout.hold_end("AnimB");
                m.set_cleared(cleared);
                if cleared > 0 {
                    let d = m.dot(cleared - 1);
                    m.layout.play(&format!("{d}/Clear"), 40);
                }
                m.place_mario(Some(cleared.saturating_sub(1)));
                s.walk_start = Some(70);
                s.walk_from = m.stop(cleared.saturating_sub(1));
                s.walk_to = m.stop(cleared);
            }
            Entry::Direct => {}
        }
        Some(s)
    }

    fn schedule_card(&mut self) {
        let Some(c) = self.card_start else { return };
        self.card.play("AnimA", c);
        let a = self.card.length("AnimA") as i32;
        self.card.play("AnimB", c + a);
    }

    fn card_end(&mut self) -> Option<i32> {
        let c = self.card_start?;
        Some(c + (self.card.length("AnimA") + self.card.length("AnimB")) as i32 + CARD_HOLD)
    }

    pub fn step(&mut self, input: &UiInput) -> Option<Smm1Event> {
        self.tick += 1;
        if self.finished {
            return None;
        }
        let tick = self.tick;
        if let Some(m) = &mut self.map {
            if self.walk_start == Some(tick) {
                self.walk_end = Some(m.walk(self.walk_from, self.walk_to, tick));
            }
            if self.walk_end == Some(tick) {
                m.mario_stand();
                m.layout.hold("MoveMario", self.walk_to);
                self.out_at = Some(tick + 30);
            }
            if self.out_at == Some(tick) && self.card_start.is_none() {
                m.layout.play("Out", tick);
                self.card_start = Some(tick + m.layout.ticks("Out", 1));
                self.schedule_card();
            }
        }
        if input.pressed.has(Buttons::BACK) {
            // leave before the course starts (the run stays saved for Continue)
            self.finished = true;
            return Some(Smm1Event::BackToSelect);
        }
        // a click / tap skips ahead as confirm does [inf]
        if input.pressed.has(Buttons::CONFIRM) || input.click.is_some() {
            if self.card_start.is_some_and(|c| tick >= c) {
                self.finished = true;
                return Some(Smm1Event::Play);
            }
            self.card_start = Some(tick);
            self.schedule_card();
            return None;
        }
        if self.card_end().is_some_and(|e| tick >= e) {
            self.finished = true;
            return Some(Smm1Event::Play);
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if self.card_start.is_some_and(|c| t >= c) {
            push_layout(out, &mut self.card, t);
            return;
        }
        if let Some(m) = &mut self.map {
            push_layout(out, &mut m.layout, t);
        }
        if let Some(l) = &mut self.title {
            push_layout(out, l, t);
        }
    }
}

// MARK: - game over, ending

/// `Cmn_GameOver_00` (OneTime), then back to the difficulty select.
pub struct GameOverScene {
    layout: Layout,
    pub tick: i32,
    done: bool,
}

impl GameOverScene {
    pub fn new(ui: &Ui) -> Option<Self> {
        let mut l = ui.layout("Cmn_GameOver_00")?;
        l.play("OneTime", 0);
        Some(Self {
            layout: l,
            tick: 0,
            done: false,
        })
    }

    pub fn step(&mut self, input: &UiInput) -> Option<Smm1Event> {
        self.tick += 1;
        if self.done {
            return None;
        }
        let end = self.layout.ticks("OneTime", 60);
        let p = input.pressed;
        let go = p.has(Buttons::CONFIRM) || p.has(Buttons::BACK) || input.click.is_some();
        if self.tick >= end || (self.tick > 60 && go) {
            self.done = true;
            return Some(Smm1Event::BackToSelect);
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
    }
}

/// After the run's last course: the world map with Mario at the castle and the message balloon
/// `Play_EndingText_00`, then the staff credits `Nwk_StaffCredit_*` listing the cleared courses
/// and their makers. The rescue scene itself is not a layout, so the map stands in for it.
pub struct EndingScene {
    map: Option<WorldMap>,
    text: Layout,
    credits: Option<Layout>,
    pub tick: i32,
    credits_start: i32,
    done: bool,
}

const TEXT_HOLD: i32 = 240;

impl EndingScene {
    pub fn new(ui: &Ui, d: usize, lives: u32, list: &[(String, String)]) -> Option<Self> {
        let mut t = ui.layout("Play_EndingText_00")?;
        let mut map = WorldMap::new(ui, d);
        if let Some(m) = &mut map {
            m.layout.hold_end("MapIn");
            m.layout.hold_end("AnimB");
            m.set_cleared(m.count);
            m.set_counters(lives, m.count);
            m.place_mario(Some(m.count - 1));
        }
        // character variants _x1.._x4 are for Mystery Mushroom costumes; plain Mario is _x0
        let label = format!(
            "T_ThankYou_{}",
            match d {
                0 => "10",
                1 => "20",
                _ => "30",
            }
        );
        t.text_from("T_ThankYou_00", &label);
        t.play("In", 20);
        let wait_at = 20 + t.length("In") as i32;
        t.play_loop("Wait", wait_at);
        let out_at = wait_at + TEXT_HOLD;
        t.play("Out", out_at);
        let credits_start = out_at + t.ticks("Out", 1) + 20;
        // 01: 8 entries + "TO BE CONTINUED" (Easy), 00: 16 entries (Normal, Expert), 03: 6
        // entries + "THE END" (Super Expert) [inf from the entry counts]
        let mut credits = ui.layout(&format!(
            "Nwk_StaffCredit_{}",
            match d {
                0 => "01",
                3 => "03",
                _ => "00",
            }
        ));
        if let Some(c) = &mut credits {
            c.play("InOut", credits_start);
            for i in 0..16 {
                let part = format!("L_MiiInfo_{i:02}");
                if c.pane(&part).is_none() {
                    continue;
                }
                if let Some((name, maker)) = list.get(i) {
                    c.visible(&part, None);
                    c.text(&format!("{part}/T_CourseName_00"), name);
                    c.text(&format!("{part}/T_MiiName_00"), maker);
                    c.show(&format!("{part}/L_NationalFlag_00"), false);
                    c.show(&format!("{part}/P_Frame_00"), false);
                } else {
                    c.show(&part, false);
                }
            }
        }
        Some(Self {
            map,
            text: t,
            credits,
            tick: 0,
            credits_start,
            done: false,
        })
    }

    pub fn step(&mut self, input: &UiInput) -> Option<Smm1Event> {
        self.tick += 1;
        if self.done {
            return None;
        }
        let confirm = input.pressed.has(Buttons::CONFIRM)
            || input.pressed.has(Buttons::BACK)
            || input.click.is_some();
        if self.tick < self.credits_start {
            if confirm && self.tick > 30 {
                self.credits_start = self.tick;
                if let Some(c) = &mut self.credits {
                    c.play("InOut", self.tick);
                }
            }
            return None;
        }
        let len = self.credits.as_mut().map_or(1, |c| c.ticks("InOut", 1));
        let end = self.credits_start + len;
        if self.tick >= end || (confirm && self.tick > self.credits_start + 30) {
            self.done = true;
            return Some(Smm1Event::EndingFinished);
        }
        None
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if t >= self.credits_start
            && let Some(c) = &mut self.credits
        {
            push_layout(out, c, t);
            return;
        }
        if let Some(m) = &mut self.map {
            push_layout(out, &mut m.layout, t);
        }
        push_layout(out, &mut self.text, t);
    }
}

// MARK: - in-course overlays

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOverlayMode {
    None,
    Pause,
    Clear,
    Card,
    Skip,
}

/// Ticks the skip key must be held: three countdown steps of 32 frames [inf].
pub const SKIP_HOLD: i32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Pause,
    Skip,
}

/// Layouts over a 100 Mario course: pause menu, course clear, the skip countdown and the intro
/// card after a death.
pub struct RunOverlay {
    ui: Ui,
    pub tick: i32,
    pub mode: RunOverlayMode,
    mode_start: i32,
    pause_l: Option<Layout>,
    clear_l: Option<Layout>,
    skip_l: Option<Layout>,
    card_l: Option<Layout>,
    closing: Option<(Which, i32)>,
    pub clear_focus: usize,
    clear_last: bool,
}

pub const PAUSE_PARTS: [&str; 3] = ["L_CloseBtn_00", "L_RetryBtn_00", "L_ExitBtn_00"];

impl RunOverlay {
    pub fn new(ui: &Ui) -> Self {
        Self {
            ui: ui.clone(),
            tick: 0,
            mode: RunOverlayMode::None,
            mode_start: 0,
            pause_l: None,
            clear_l: None,
            skip_l: None,
            card_l: None,
            closing: None,
            clear_focus: 1,
            clear_last: false,
        }
    }

    pub fn since_mode(&self) -> i32 {
        self.tick - self.mode_start
    }

    pub fn step(&mut self) {
        self.tick += 1;
    }

    fn set_mode(&mut self, m: RunOverlayMode) {
        self.mode = m;
        self.mode_start = self.tick;
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if let Some((w, until)) = self.closing
            && t < until
        {
            let l = match w {
                Which::Pause => self.pause_l.as_mut(),
                Which::Skip => self.skip_l.as_mut(),
            };
            if let Some(l) = l {
                push_layout(out, l, t);
            }
        }
        let l = match self.mode {
            RunOverlayMode::None => None,
            RunOverlayMode::Pause => self.pause_l.as_mut(),
            RunOverlayMode::Clear => self.clear_l.as_mut(),
            RunOverlayMode::Card => self.card_l.as_mut(),
            RunOverlayMode::Skip => self.skip_l.as_mut(),
        };
        if let Some(l) = l {
            push_layout(out, l, t);
        }
    }

    pub fn show_pause(&mut self, mode_line: &str, course: &str, maker: &str, selection: usize) {
        if self.pause_l.is_none()
            && let Some(mut p) = self.ui.layout("Cmn_PauseMenu_00")
        {
            // Type 6: the two play buttons without the network ones; the exit button reads
            // "Save & Quit" (T_ExitBtn_01 of the same table) [inf]
            p.hold("Type", 6.0);
            p.text_from("L_ExitBtn_00/T_Btn_00", "T_ExitBtn_01");
            p.hold("TypeCourseInfo", 2.0); // course-world icon, maker line
            for n in [
                "N_NetworkBtn_00",
                "P_YeahCountIcon_00",
                "P_PlayerCountIcon_00",
                "L_NationalFlag_00",
                "L_OptionBtn_00",
            ] {
                p.show(n, false);
            }
            self.pause_l = Some(p);
        }
        let t = self.tick;
        let Some(p) = &mut self.pause_l else { return };
        p.text("T_Mode_00", mode_line);
        p.text("T_CourseName_00", course);
        p.text("T_MiiName_00", maker);
        p.stop("Out");
        p.play("In", t);
        self.closing = None;
        self.set_mode(RunOverlayMode::Pause);
        self.pause_select(selection);
    }

    pub fn pause_select(&mut self, i: usize) {
        let t = self.tick;
        let Some(p) = &mut self.pause_l else { return };
        for (k, part) in PAUSE_PARTS.iter().enumerate() {
            let a = format!("{part}/OnOff");
            if k == i {
                p.play(&a, t);
            } else {
                p.hold(&a, 0.0);
            }
        }
    }

    pub fn hide_pause(&mut self) {
        if self.mode != RunOverlayMode::Pause {
            return;
        }
        let t = self.tick;
        let Some(p) = &mut self.pause_l else { return };
        p.stop("In");
        p.play("Out", t);
        self.closing = Some((Which::Pause, t + p.ticks("Out", 1)));
        self.set_mode(RunOverlayMode::None);
    }

    pub fn show_clear(
        &mut self,
        course: &str,
        maker: &str,
        ticks: Option<i64>,
        world_record_ms: Option<i64>,
        last: bool,
    ) {
        if self.clear_l.is_none() {
            self.clear_l = self.ui.layout("Play_CourseClear_00");
        }
        let t = self.tick;
        let Some(c) = &mut self.clear_l else { return };
        self.clear_last = last;
        // 100 Mario variant: Save & Quit + Next; ClearType centres Next alone (the run's last
        // course) [inf]
        if last {
            c.hold("ClearType", 1.0);
        } else {
            c.hold("ModeType", 0.0);
        }
        for n in [
            "N_MiiverseComment_01",
            "N_RcmText_00",
            "N_RcmCourseBtn_02",
            "N_CommentGoodBtn_00",
        ] {
            c.show(n, false);
        }
        c.show("N_Player_00/L_NF_00", false);
        c.show("L_NF_00", false);
        c.text("T_CourseName_00", course);
        c.text("T_PlayerName_00", maker);
        let put = |c: &mut Layout, main: &str, frac: &str, ms: Option<i64>| match ms {
            None => {
                c.text(main, "--’--”");
                c.text(frac, "---");
            }
            Some(ms) => {
                c.text(main, &format!("{:02}’{:02}”", ms / 60000, ms / 1000 % 60));
                c.text(frac, &format!("{:03}", ms % 1000));
            }
        };
        put(c, "T_Time_02", "T_Time_03", ticks.map(|t| t * 1000 / 60));
        put(c, "T_Time_00", "T_Time_01", world_record_ms);
        c.play("In", t);
        c.play_loop("Default", t);
        self.set_mode(RunOverlayMode::Clear);
        self.clear_focus = 1;
        self.clear_select(1);
    }

    pub fn clear_select(&mut self, i: usize) {
        self.clear_focus = if self.clear_last { 1 } else { i.min(1) };
        let (f, t) = (self.clear_focus, self.tick);
        let Some(c) = &mut self.clear_l else { return };
        for (k, part) in ["L_ExitBtn_00", "L_NextBtn_00"].iter().enumerate() {
            let a = format!("{part}/OnOff");
            if k == f {
                c.play(&a, t);
            } else {
                c.hold(&a, 0.0);
            }
        }
    }

    /// The buttons take input once the In animation has brought them on screen.
    pub fn clear_ready(&mut self) -> bool {
        let n = self.clear_l.as_mut().map_or(0, |c| c.length("In") as i32);
        self.mode == RunOverlayMode::Clear && self.since_mode() >= n / 2
    }

    pub fn press_clear(&mut self) {
        let part = if self.clear_focus == 0 {
            "L_ExitBtn_00"
        } else {
            "L_NextBtn_00"
        };
        let t = self.tick;
        if let Some(c) = &mut self.clear_l {
            c.play(&format!("{part}/Decide"), t);
        }
    }

    pub fn skip_begin(&mut self) {
        if self.skip_l.is_none() {
            self.skip_l = self.ui.layout("Play_SlidePassIconKey_00");
        }
        let t = self.tick;
        let Some(s) = &mut self.skip_l else { return };
        self.closing = None;
        for a in [
            "Out",
            "Pass",
            "CountDown_00",
            "CountDown_01",
            "CountDown_02",
        ] {
            s.stop(a);
        }
        s.play_loop("Default", t);
        s.play("In", t);
        s.play("CountDown_00", t);
        s.play("CountDown_01", t + 32);
        s.play("CountDown_02", t + 64);
        self.set_mode(RunOverlayMode::Skip);
    }

    pub fn skip_cancel(&mut self) {
        if self.mode != RunOverlayMode::Skip {
            return;
        }
        let t = self.tick;
        let Some(s) = &mut self.skip_l else { return };
        s.play("Out", t);
        self.closing = Some((Which::Skip, t + s.ticks("Out", 1)));
        self.set_mode(RunOverlayMode::None);
    }

    /// The hold is complete: play Pass. Returns the tick count it takes.
    pub fn skip_pass(&mut self) -> i32 {
        let t = self.tick;
        let Some(s) = &mut self.skip_l else { return 1 };
        s.play("Pass", t);
        s.ticks("Pass", 1)
    }

    pub fn show_card(&mut self, info: &RunCardInfo) {
        if self.card_l.is_none() {
            self.card_l = self.ui.layout("Play_CourseIn100Mario_00");
        }
        let t = self.tick;
        let Some(c) = &mut self.card_l else { return };
        configure_card(c, info);
        c.hold_end("AnimA");
        c.play("AnimB", t);
        let b = c.length("AnimB") as i32;
        c.play("OutMiss", t + b);
        self.set_mode(RunOverlayMode::Card);
    }

    pub fn card_done(&mut self) -> bool {
        if self.mode != RunOverlayMode::Card {
            return true;
        }
        let since = self.since_mode();
        let Some(c) = &mut self.card_l else {
            return true;
        };
        since >= (c.length("AnimB") + c.length("OutMiss").max(1.0)) as i32
    }

    pub fn hide_card(&mut self) {
        if self.mode == RunOverlayMode::Card {
            self.set_mode(RunOverlayMode::None);
        }
    }

    pub fn clear_all(&mut self) {
        self.closing = None;
        self.set_mode(RunOverlayMode::None);
    }
}
