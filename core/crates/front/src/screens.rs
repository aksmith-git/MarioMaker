//! SMM2's in-course screens drawn with the game's own layouts, over a course (the Swift app's
//! `SMM2Course.swift`): the HUD (`Play_PlayScene_00`), the course card (`Play_CourseIn_00` on a
//! black `Cmn_Fade_00`), the pause menu (`Cmn_PauseMenu_00`) and its settings window
//! (`Cmn_OptionWindow_00`), course clear (`Play_CourseClear_00`), "You gave up..."
//! (`Play_CourseRetire_00`), "TIME'S UP!" (`Play_TimeUp_00`) and game over
//! (`Play_GameOver_00`). Texts come from the import's message tables (the clear-condition
//! sentences from ReferenceMsg `GoalCondition` + `PartsIconName`), the clear-condition icons
//! from `ui/textures/MissionIcon/`.
//!
//! The layouts and their animations are the game's; how the game strings them together is not
//! in the data. Inferred [inf] (DECISIONS.md "SMM2 in-course screens"): which Type frame each
//! screen uses, the lives HUD variant and the course card after each death in a run with lives,
//! which buttons the clear / give-up screens offer, the number widths of the HUD's value tags,
//! the condition → icon / name table. Departures: keys held are shown as `Play_KeyIcon_00` icons
//! under the score, nothing that needs the network (likes, comments, Miis, flags, tags).

use crate::host::{Condition, CourseStatus};
use crate::layout::{Layout, Ui};
use crate::sound::Event;
use crate::{DrawItem, push_layout};

/// Style prefixes of the import's names (`<style>_Lyt_M_<icon>` …), in `CourseStatus::style`
/// order.
pub const STYLES: [&str; 5] = ["M1", "M3", "MW", "WU", "3W"];

/// Defeat conditions: condition-list index → (part name label, icon stem) [inf: matched by the
/// internal names of the condition list, the part-name table and the icon textures].
fn enemy(target: u32) -> Option<(&'static str, &'static str)> {
    Some(match target {
        1 => ("Kuribo", "Kuribo_00"),
        2 => ("Kuribo_01", "Kuribo_01"),
        3 => ("Nokonoko", "Nokonoko_00"),
        4 => ("Met_00", "Met_00"),
        5 => ("Togezo_00", "Togezo_00"),
        6 => ("Pakkun_00", "Pakkun_00"),
        7 => ("Pakkun_01", "Pakkun_01"),
        8 => ("BlackPakkun", "BlackPakkun_00"),
        10 => ("Karon", "Karon_00"),
        11 => ("FishBone", "FishBone_00"),
        12 => ("Poo", "Poo_00"),
        13 => ("ChoroPoo", "ChoroPoo_00"),
        14 => ("Bros", "Bros_00"),
        15 => ("MegaBros", "MegaBros_00"),
        16 => ("Jugem", "Jugem_00"),
        17 => ("Gesso_00", "Gesso_00"),
        18 => ("Pukupuku", "Pukupuku_00"),
        19 => ("TogeMet", "TogeMet_00"),
        20 => ("Arihei_00", "Arihei_00"),
        21 => ("Bubble", "Bubble_00"),
        22 => ("Wanwan_00", "Wanwan_00"),
        23 => ("Hanachan_00", "Hanachan_00"),
        24 => ("Bombhei_00", "Bombhei_00"),
        25 => ("Dossun", "Dossun_00"),
        26 => ("Teresa_00", "Teresa_00"),
        27 => ("Teresa_01", "Teresa_01"),
        28 => ("MagnumKiller", "MagnumKiller_00"),
        29 => ("Kameck", "Kameck_00"),
        30 => ("SunMoon", "SunMoon_00"),
        31 => ("Pyonchu", "Pyonchu_00"),
        32 => ("Hacchin", "Hacchin_00"),
        33 => ("Donketsu", "Donketsu_00"),
        34 => ("MagmaFish", "MagmaFish_00"),
        35 => ("Fugumannen", "Fugumannen_00"),
        36 => ("Hopper", "Hopper_00"),
        37 => ("Koopa", "Koopa_00"),
        38 => ("KoopaJr", "KoopaJr_00"),
        39 => ("Bunbun", "Bunbun_00"),
        40 => ("Bunbun_01", "Bunbun_01"),
        41 => ("Killer", "Killer_00"),
        _ => return None,
    })
}

/// (GoalCondition label, part name label, icon stem) of a condition.
fn entry(c: &Condition) -> Option<(&'static str, Option<&'static str>, Option<&'static str>)> {
    Some(match c.kind {
        1 => {
            let (p, i) = enemy(c.target)?;
            ("EnemyKill_02", Some(p), Some(i))
        }
        2 => ("ItemGet_02", Some("1upKinoko"), Some("1upKinoko_00")),
        3 => {
            let (p, i) = match c.target {
                10 => ("10Coin", "10Coin_00"),
                30 => ("10Coin_01", "10Coin_01"),
                50 => ("10Coin_02", "10Coin_02"),
                _ => ("Coin", "Coin_00"),
            };
            ("CoinGet_02", Some(p), Some(i))
        }
        4 => ("PowBlockShock_02", Some("PowBlock"), Some("PowBlock_00")),
        5 => ("PSwitchPress_02", Some("PSwitch"), Some("PSwitch_00")),
        6 => ("PlayerOnStar", None, Some("SuperStar_00")),
        7 => ("PlayerMode_00", None, Some("SuperKinoko_00")),
        8 => ("PlayerMode_01", None, Some("FireFlower_00")),
        9 => ("PlayerNoDamage", None, None),
        10 => ("PlayerNoLand", None, None),
        11 => (
            "PlayerOnShoe",
            Some("KutsuKuribo_00"),
            Some("KutsuKuribo_00"),
        ),
        12 => (
            "PlayerOnItem_00",
            Some("KoopaClown_00"),
            Some("KoopaClown_00"),
        ),
        13 => ("PlayerMode_04", None, Some("SuperKonoha_00")),
        14 => ("PlayerMode_03", None, Some("MantleWing_00")),
        15 => ("PlayerMode_02", None, Some("PropellerKinoko_00")),
        16 => (
            "PlayerCarry",
            Some("NokonokoShell"),
            Some("NokonokoShell_00"),
        ),
        17 => ("PlayerOnItem_01", Some("KutsuKuribo"), Some("YosshiEgg_00")),
        18 => ("PlayerMode_06", None, Some("SuperBell_00")),
        19 => ("PlayerOnItem_02", Some("KoopaCar"), Some("KoopaCar_00")),
        20 => ("PlayerMode_07", None, Some("SuperHammer_00")),
        _ => return None,
    })
}

/// Count conditions show "count/needed"; the others only the icon.
pub fn counted(c: &Condition) -> bool {
    (1..=5).contains(&c.kind)
}

/// Our own short wording of a condition (when the import has no table for it).
pub fn describe(c: &Condition) -> String {
    let mark = if c.met { "✓" } else { "⚑" };
    let progress = format!(" {}/{}", c.count.min(c.needed), c.needed);
    let enemies = |t: u32| match t {
        1 => "Goombas",
        3 => "Koopas",
        4 => "Buzzy Beetles",
        5 => "Spinies",
        6 => "Piranha Plants",
        7 => "Fire Piranhas",
        8 => "Munchers",
        10 => "Dry Bones",
        12 => "Rocky Wrenches",
        13 => "Monty Moles",
        14 => "Hammer Bros",
        16 => "Lakitus",
        17 => "Bloopers",
        18 => "Cheep Cheeps",
        19 => "Spike Tops",
        21 => "Lava Bubbles",
        22 => "Chain Chomps",
        23 => "Wigglers",
        24 => "Bob-ombs",
        25 => "Thwomps",
        26 => "Boos",
        28 => "Banzai Bills",
        29 => "Magikoopas",
        30 => "Angry Suns",
        37 => "Bowsers",
        38 => "Bowser Jrs",
        39 => "Boom Booms",
        40 => "Pom Poms",
        41 => "Bullet Bills",
        _ => "enemies",
    };
    let s = match c.kind {
        1 => format!("{}{progress}", enemies(c.target)),
        2 => format!("1-Ups{progress}"),
        3 if c.target == 0 => format!("coins{progress}"),
        3 => format!("{}-coins{progress}", c.target),
        4 => format!("POW{progress}"),
        5 => format!("P Switch{progress}"),
        6 => "goal invincible".into(),
        7 => "goal as Super".into(),
        8 => "goal as Fire".into(),
        9 => "no damage".into(),
        10 => "no landing".into(),
        11 => "goal in a shoe".into(),
        12 => "goal in a Clown Car".into(),
        13 => "goal as Raccoon".into(),
        14 => "goal with a cape".into(),
        15 => "goal as Propeller".into(),
        16 => "goal holding a shell".into(),
        17 => "goal on Yoshi".into(),
        18 => "goal as Cat Mario".into(),
        19 => "goal in a Koopa Troopa Car".into(),
        20 => "goal as Builder Mario".into(),
        21 => "goal on a Lakitu's Cloud".into(),
        22 => "goal holding the stone".into(),
        23 => format!("trees{progress}"),
        24 => "no claw".into(),
        25 => "no water".into(),
        26 if c.target == 2 => "goal in a Spiny helmet".into(),
        26 => "goal in a Buzzy helmet".into(),
        27 => "goal in a Dry Bones Shell".into(),
        28 => "goal as Superball Mario".into(),
        29 => "goal as Big Mario".into(),
        _ => return "⚑ condition not supported".into(),
    };
    format!("{mark} {s}")
}

/// The condition's sentence from the import, or our own wording.
pub fn sentence(ui: &Ui, c: &Condition, style: usize) -> String {
    let s = STYLES[style.min(4)];
    if let Some((goal, part, _)) = entry(c)
        && let Some(template) = ui.message("GoalCondition", goal)
    {
        let mut args = Vec::new();
        if counted(c) {
            args.push(c.needed.to_string());
        }
        if let Some(p) = part {
            let table = if counted(c) {
                "PartsIconName_plural"
            } else {
                "PartsIconName"
            };
            args.push(
                ui.message(table, &format!("{s}_{p}"))
                    .or_else(|| ui.message(table, p))
                    .unwrap_or_default(),
            );
        }
        return crate::layout::fill(&template, &args).replace('\n', " ");
    }
    describe(c)
}

/// Put the condition's icon into picture `pane` (the game style's icon, else another style's).
pub fn set_icon(l: &mut Layout, c: &Condition, pane: &str, style: usize) -> bool {
    let Some((_, _, Some(icon))) = entry(c) else {
        return false;
    };
    let own = STYLES[style.min(4)];
    std::iter::once(own)
        .chain(STYLES)
        .any(|s| l.picture(pane, "MissionIcon", Some(&format!("{s}_Lyt_M_{icon}"))))
}

/// Everything the course card, pause menu and clear screen show about the course.
#[derive(Clone, Debug, Default)]
pub struct CardInfo {
    pub name: String,
    pub maker: Option<String>,
    /// "Story Mode" for the built-in courses, else "Course World" (pause menu T_Mode_*).
    pub built_in: bool,
    pub best_ticks: Option<i64>,
    /// The play has lives (100 Mario / Endless): the HUD's lives counter, the card's "× lives".
    pub lives: bool,
    /// Pause menu mode label (`T_Mode_*`); None: Story Mode / Course World by `built_in`.
    pub mode_label: Option<String>,
    /// The mode line's own text instead of a label (the 100 Mario Challenge's "n/total").
    pub mode_text: Option<String>,
    /// A course of the course server (by code): the clear screen offers Like / Boo.
    pub server_code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Play,
    Intro,
    Pause,
    Options,
    Clear,
    Retire,
    GameOver,
}

/// Number text for an SMM2 value tag (2, 3 or 9 digits) [inf].
pub fn digits(v: i64, width: usize) -> String {
    format!("{:0width$}", v.max(0))
}

pub const PAUSE_PARTS: [&str; 4] = [
    "L_CloseBtn_00",
    "L_RetryBtn_00",
    "L_ExitBtn_00",
    "L_OptionBtn_00",
];
pub const OPTION_PARTS: [&str; 5] = [
    "L_CharacterBtn_00",
    "L_CharacterBtn_01",
    "L_CharacterBtn_02",
    "L_CharacterBtn_03",
    "L_OKBtn_00",
];
/// The judge buttons (Boo, Like) of the pause menu and the clear screen.
pub const JUDGE_PARTS: [&str; 2] = ["L_NegativeBtn_00", "L_GoodBtn_00"];

/// Show the judge buttons of `l` (the layout's own texts), with the vote `voted` (true like)
/// keeping its pressed look (the end of its Decide) [inf].
fn judge_buttons(l: &mut Layout, voted: Option<bool>) {
    for (k, (b, t)) in [
        ("L_NegativeBtn_00", "T_Negative"),
        ("L_GoodBtn_00", "T_Good"),
    ]
    .iter()
    .enumerate()
    {
        for n in ["00", "01"] {
            l.text_from(&format!("{b}/{t}_{n}"), &format!("{b}-{t}_00"));
        }
        let a = format!("{b}/Decide");
        if voted == Some(k == 1) {
            l.hold_end(&a);
        } else {
            l.stop(&a);
        }
    }
}

/// 0 Start Over (L_Btn_01, left), 1 Quit (L_Btn_00, right) [inf]; a server course's clear
/// adds 2 Boo (`L_NegativeBtn_00`) and 3 Like (`L_GoodBtn_00`), the game's judge buttons left
/// of them.
pub const CLEAR_PARTS: [&str; 4] = ["L_Btn_01", "L_Btn_00", "L_NegativeBtn_00", "L_GoodBtn_00"];
/// The clear screen's buttons from left to right (indices of [`CLEAR_PARTS`]).
pub const CLEAR_ORDER: [usize; 4] = [2, 3, 0, 1];

/// Pause button `i`: [`PAUSE_PARTS`], then 4 Boo and 5 Like ([`JUDGE_PARTS`]).
pub fn pause_part(i: usize) -> &'static str {
    match i {
        0..4 => PAUSE_PARTS[i],
        _ => JUDGE_PARTS[(i - 4).min(1)],
    }
}
/// 0 Exit (L_Btn_00, left), 1 Keep playing (L_Btn_02, right).
pub const RETIRE_PARTS: [&str; 2] = ["L_Btn_00", "L_Btn_02"];
/// Ticks the skip key must be held: three countdown steps of 32 frames [inf, as SMM1].
pub const SKIP_HOLD: i32 = 96;

/// The SMM2 layouts over a course. The owner steps it at 60 Hz and feeds it the engine's state
/// (`update`); [`crate::play::CoursePlay`] decides when each screen appears.
pub struct CourseScreens {
    ui: Ui,
    pub hud: Layout,
    pub tick: i32,
    pub mode: Mode,
    pub mode_start: i32,
    pub info: CardInfo,
    status: CourseStatus,
    key_icons: Vec<Layout>,
    pink_got: Vec<bool>,
    pink_shown: i32,
    condition_key: Option<(u32, u32, u32)>,
    last_style: Option<u32>,
    time_caution: bool,
    hud_shown: bool,
    fade: Option<Layout>,
    intro: Option<Layout>,
    pause_l: Option<Layout>,
    clear_l: Option<Layout>,
    retire_l: Option<Layout>,
    time_up_l: Option<Layout>,
    game_over_l: Option<Layout>,
    options_l: Option<Layout>,
    cursor: Option<Layout>,
    cursor_shown: bool,
    /// A screen still playing its Out animation: (which, until tick).
    closing: Option<(Which, i32)>,
    time_up_until: i32,
    fade_in_until: i32,
    iris: Option<Layout>,
    iris_until: i32,
    iris_closed: bool,
    skip_key: Option<Layout>,
    skip_icon: Option<Layout>,
    pub skipping: bool,
    skip_shown_until: i32,
    pub skip_start: i32,
    pub pause_selection: usize,
    pub option_selection: usize,
    pub character: usize,
    pub clear_selection: usize,
    pub retire_selection: usize,
    /// This play's vote on a server course (true like), shown on the pause and clear screens.
    pub voted: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Pause,
    Options,
    Retire,
}

impl CourseScreens {
    pub fn new(ui: &Ui) -> Option<Self> {
        let mut h = ui.layout("Play_PlayScene_00")?;
        // our play has lives: the HUD variant with the lives counter (Type 1) [inf]
        h.hold("Type", 1.0);
        h.show("N_1UP_00", false); // the Endless Challenge's 1-Up progress
        // pop-ups and Ninji / event panes the engine has no data for
        for p in [
            "L_PointGet_00",
            "L_MissionClearMark_00",
            "N_EventFirst_00",
            "N_EventGhostTime_00",
        ] {
            h.show(p, false);
        }
        h.hold("In", 0.0);
        let mut iris = ui.layout("Cmn_Fade_00");
        if let Some(f) = &mut iris {
            for p in [
                "P_FadeB_00",
                "P_FadeW_00",
                "W_CircleW_00",
                "N_Block_00",
                "N_CaptureFade_00",
            ] {
                f.show(p, false);
            }
        }
        let mut cursor = ui.layout("Cmn_Cursor_00");
        if let Some(c) = &mut cursor {
            c.play_loop("Loop", 0);
        }
        Some(Self {
            ui: ui.clone(),
            hud: h,
            tick: 0,
            mode: Mode::Play,
            mode_start: 0,
            info: CardInfo::default(),
            status: CourseStatus::default(),
            key_icons: Vec::new(),
            pink_got: vec![false; 10],
            pink_shown: -1,
            condition_key: None,
            last_style: None,
            time_caution: false,
            hud_shown: false,
            fade: ui.layout("Cmn_Fade_00"),
            intro: ui.layout("Play_CourseIn_00"),
            pause_l: ui.layout("Cmn_PauseMenu_00"),
            clear_l: ui.layout("Play_CourseClear_00"),
            retire_l: ui.layout("Play_CourseRetire_00"),
            time_up_l: ui.layout("Play_TimeUp_00"),
            game_over_l: ui.layout("Play_GameOver_00"),
            options_l: ui.layout("Cmn_OptionWindow_00"),
            cursor,
            cursor_shown: false,
            closing: None,
            time_up_until: 0,
            fade_in_until: 0,
            iris,
            iris_until: 0,
            iris_closed: false,
            skip_key: ui.layout("Play_SlidePassIconKey_00"),
            skip_icon: ui.layout("Play_SlidePassIcon_00"),
            skipping: false,
            skip_shown_until: 0,
            skip_start: 0,
            pause_selection: 0,
            option_selection: 0,
            character: 0,
            clear_selection: 1,
            retire_selection: 1,
            voted: None,
        })
    }

    pub fn since_mode(&self) -> i32 {
        self.tick - self.mode_start
    }

    pub fn style(&self) -> usize {
        self.status.style.min(4) as usize
    }

    fn skin(&self) -> &'static str {
        STYLES[self.style()]
    }

    fn which(&mut self, w: Which) -> Option<&mut Layout> {
        match w {
            Which::Pause => self.pause_l.as_mut(),
            Which::Options => self.options_l.as_mut(),
            Which::Retire => self.retire_l.as_mut(),
        }
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        let t = self.tick;
        if self.mode != Mode::GameOver {
            push_layout(out, &mut self.hud, t);
        }
        let keys = self.status.keys as usize;
        for k in self.key_icons.iter_mut().take(keys) {
            push_layout(out, k, t);
        }
        if t < self.time_up_until
            && let Some(l) = &mut self.time_up_l
        {
            push_layout(out, l, t);
        }
        if let Some((w, until)) = self.closing
            && t < until
            && let Some(l) = self.which(w)
        {
            push_layout(out, l, t);
        }
        let mut layers: Vec<&mut Layout> = Vec::new();
        match self.mode {
            Mode::Play => {
                if t < self.fade_in_until
                    && let Some(f) = &mut self.fade
                {
                    layers.push(f);
                }
                if (self.iris_closed || t < self.iris_until)
                    && let Some(i) = &mut self.iris
                {
                    layers.push(i);
                }
                if (self.skipping || t < self.skip_shown_until)
                    && let Some(k) = &mut self.skip_key
                {
                    layers.push(k);
                }
                if let Some(p) = &mut self.skip_icon
                    && p.is_playing("Pass")
                {
                    layers.push(p);
                }
            }
            Mode::Intro => layers.extend(
                [self.fade.as_mut(), self.intro.as_mut()]
                    .into_iter()
                    .flatten(),
            ),
            Mode::Pause => layers.extend(
                [self.fade.as_mut(), self.pause_l.as_mut()]
                    .into_iter()
                    .flatten(),
            ),
            Mode::Options => layers.extend(
                [
                    self.fade.as_mut(),
                    self.pause_l.as_mut(),
                    self.options_l.as_mut(),
                ]
                .into_iter()
                .flatten(),
            ),
            Mode::Clear => layers.extend(self.clear_l.as_mut()),
            Mode::Retire => layers.extend(self.retire_l.as_mut()),
            Mode::GameOver => layers.extend(self.game_over_l.as_mut()),
        }
        for l in layers {
            push_layout(out, l, t);
        }
        if self.cursor_shown
            && let Some(c) = &mut self.cursor
        {
            push_layout(out, c, t);
        }
    }

    pub fn step(&mut self) {
        self.tick += 1;
        self.place_cursor();
    }

    fn place_cursor(&mut self) {
        self.cursor_shown = false;
        let clear_ready = self.clear_ready();
        let retire_ready = self.retire_ready();
        let (l, pane) = match self.mode {
            Mode::Pause => (
                self.pause_l.as_mut(),
                format!("{}/B_Btn_00", pause_part(self.pause_selection)),
            ),
            Mode::Options => (
                self.options_l.as_mut(),
                format!("{}/B_Btn_00", OPTION_PARTS[self.option_selection]),
            ),
            Mode::Clear if clear_ready => (
                self.clear_l.as_mut(),
                format!("{}/B_Btn_00", CLEAR_PARTS[self.clear_selection]),
            ),
            Mode::Retire if retire_ready => (
                self.retire_l.as_mut(),
                format!("{}/B_Btn_00", RETIRE_PARTS[self.retire_selection]),
            ),
            _ => return,
        };
        let (Some(l), Some(c)) = (l, self.cursor.as_mut()) else {
            return;
        };
        l.apply(self.tick);
        let Some(r) = l.rect(&pane) else { return };
        if !r.visible || r.alpha <= 0.01 {
            return;
        }
        c.set_pane(
            "W_SelectFrame_00",
            mmk_ui::PaneOverride {
                translate: Some(r.center),
                size: Some(r.size),
                ..Default::default()
            },
        );
        c.set_alpha("N_All_00", 255.0 * r.alpha);
        self.cursor_shown = true;
    }

    /// A click on one of the current screen's buttons: its index.
    pub fn hit(&mut self, p: [f32; 2]) -> Option<usize> {
        let clear = self.clear_buttons();
        let pause: Vec<&str> = (0..self.pause_buttons()).map(pause_part).collect();
        let (l, parts): (Option<&mut Layout>, &[&str]) = match self.mode {
            Mode::Pause => (self.pause_l.as_mut(), &pause),
            Mode::Options => (self.options_l.as_mut(), &OPTION_PARTS),
            Mode::Clear => (self.clear_l.as_mut(), &CLEAR_PARTS[..clear]),
            Mode::Retire => (self.retire_l.as_mut(), &RETIRE_PARTS),
            _ => return None,
        };
        let l = l?;
        l.apply(self.tick);
        parts.iter().position(|b| {
            l.rect(&format!("{b}/B_Btn_00"))
                .is_some_and(|r| r.visible && r.alpha > 0.01 && r.contains(p))
        })
    }

    /// The black fade layer: full (course card) or a dim behind the pause panel [inf].
    fn set_fade(&mut self, dim: bool) {
        let Some(f) = &mut self.fade else { return };
        f.stop("FadeInB");
        f.hold_end("FadeOutB");
        for p in ["P_FadeW_00", "N_Pos_00", "N_Block_00", "N_CaptureFade_00"] {
            f.show(p, false);
        }
        if dim {
            f.set_alpha("P_FadeB_00", 210.0);
        } else {
            f.clear_pane("P_FadeB_00");
        }
    }

    fn set_mode(&mut self, m: Mode) {
        self.mode = m;
        self.mode_start = self.tick;
    }

    // MARK: HUD

    /// The engine's state for this tick: counters, timer, condition, pink coins, keys.
    pub fn update(&mut self, s: &CourseStatus) {
        self.status = *s;
        if self.last_style != Some(s.style) {
            self.last_style = Some(s.style);
            let st = self.style() as f32;
            self.hud.hold("Skin", st);
            for k in &mut self.key_icons {
                k.hold("Skin", st);
            }
            self.condition_key = None;
            self.pink_shown = -1;
        }
        let skin = self.skin();
        let h = &mut self.hud;
        h.hold("TypeCharacter", s.character.min(3) as f32);
        h.text(&format!("T_{skin}_Coin_00"), &digits(s.coins as i64, 2));
        h.text(&format!("T_{skin}_Zanki_00"), &digits(s.lives as i64, 2));
        h.text(&format!("T_{skin}_Score_00"), &digits(s.score as i64, 9));
        let timed = s.time >= 0;
        h.visible("N_Time_00", if timed { None } else { Some(false) });
        if timed {
            h.text(&format!("T_{skin}_Time_00"), &digits(s.time as i64, 3));
        }
        // the timer pulses in the last 100 seconds (the game's "hurry") [inf: threshold]
        let caution = timed && s.time <= 100 && !s.cleared;
        if caution != self.time_caution {
            self.time_caution = caution;
            if caution {
                h.play_loop("TimeCaution", self.tick);
            } else {
                h.stop("TimeCaution");
            }
        }
        self.update_condition();
        self.update_pink();
        self.update_keys();
    }

    fn update_condition(&mut self) {
        let style = self.style();
        let h = &mut self.hud;
        let Some(c) = self.status.condition else {
            h.show("N_Mission_00", false);
            return;
        };
        h.visible("N_Mission_00", None);
        let key = (c.kind, c.target, style as u32);
        if self.condition_key != Some(key) {
            self.condition_key = Some(key);
            let ok = set_icon(h, &c, "L_MissionIcon_00/P_DummyIcon_00", style);
            h.visible(
                "L_MissionIcon_00/P_DummyIcon_00",
                if ok { None } else { Some(false) },
            );
        }
        // TypeMission 0: counter window + icon, 1: icon alone
        h.hold("TypeMission", if counted(&c) { 0.0 } else { 1.0 });
        if counted(&c) {
            let n = c.count.min(c.needed);
            let s = h
                .format("Mission_Num", &[n.to_string(), c.needed.to_string()])
                .unwrap_or_else(|| format!("{n}/{}", c.needed));
            h.text("T_MissionNum_00", &s);
        }
        // met: the clear frame on; broken (no damage / no landing): the failed cross [inf]
        h.set_alpha(
            "W_MissionClear_00",
            if c.met && !c.violated { 255.0 } else { 0.0 },
        );
        if c.violated {
            if !h.is_playing("InMissionFailed") {
                h.play("InMissionFailed", self.tick);
            }
        } else if h.is_playing("InMissionFailed") {
            h.stop("InMissionFailed");
        }
    }

    /// One slot per pink coin of the course, the rightmost ones; collected ones in colour
    /// (`Play_PinkCoin_00` Type<style> frame 1) with the Get pop.
    fn update_pink(&mut self) {
        let skin = self.skin();
        let h = &mut self.hud;
        let total = self.status.pink_total.min(10) as usize;
        if total == 0 {
            h.show("N_PinkCoinScore_00", false);
            return;
        }
        h.visible("N_PinkCoinScore_00", None);
        let got = self.status.pink_coins as usize;
        if self.pink_shown != total as i32 {
            self.pink_shown = total as i32;
            self.pink_got = vec![false; 10];
            h.apply(self.tick);
            for i in 0..10 {
                h.visible(
                    &format!("L_PinkCoin_{i:02}"),
                    if i >= 10 - total { None } else { Some(false) },
                );
            }
            // the frame's left end before the first shown coin (as authored for two coins) [inf]
            if let (Some(first), Some(left)) = (
                h.pane(&format!("L_PinkCoin_{:02}", 10 - total)),
                h.pane("P_PinkFrameL_00"),
            ) {
                h.set_translate(
                    "P_PinkFrameL_00",
                    [first.translate[0] - 7.0, left.translate[1]],
                );
            }
        }
        for k in 0..total {
            let part = format!("L_PinkCoin_{:02}", 10 - total + k);
            let collected = k < got;
            h.hold(
                &format!("{part}/Type{skin}"),
                if collected { 1.0 } else { 0.0 },
            );
            if collected && !self.pink_got[k] {
                self.pink_got[k] = true;
                h.play(&format!("{part}/Get"), self.tick);
            } else if !collected && self.pink_got[k] {
                self.pink_got[k] = false;
                h.stop(&format!("{part}/Get"));
            }
        }
    }

    /// Keys held: one `Play_KeyIcon_00` per key in a row under the score [departure].
    fn update_keys(&mut self) {
        let n = (self.status.keys as usize).min(10);
        let y = if self.status.pink_total > 0 {
            210.0
        } else {
            262.0
        };
        while self.key_icons.len() < n {
            let Some(mut k) = self.ui.layout("Play_KeyIcon_00") else {
                break;
            };
            k.hold("Skin", self.style() as f32);
            let i = self.key_icons.len() as f32;
            k.set_translate("P_Key_00", [600.0 - i * 34.0, y]);
            k.play("Get", self.tick);
            self.key_icons.push(k);
        }
        for (i, k) in self.key_icons.iter_mut().enumerate() {
            k.set_translate("P_Key_00", [600.0 - i as f32 * 34.0, y]);
        }
    }

    fn hud_in(&mut self) {
        if self.hud_shown {
            return;
        }
        self.hud_shown = true;
        self.hud.stop("Out");
        self.hud.play("In", self.tick);
    }

    fn hud_out(&mut self) {
        if !self.hud_shown {
            return;
        }
        self.hud_shown = false;
        self.hud.stop("In");
        self.hud.play("Out", self.tick);
    }

    /// HUD variants for the play's rules: Type 1 with the lives counter when the play has lives,
    /// Type 0 (coins, score, time) when it hasn't.
    pub fn apply_rules(&mut self) {
        self.hud
            .hold("Type", if self.info.lives { 1.0 } else { 0.0 });
    }

    // MARK: skip (100 Mario: hold the skip key)

    pub fn skip_begin(&mut self) {
        let Some(s) = &mut self.skip_key else { return };
        for a in ["CountDown_00", "CountDown_01", "CountDown_02"] {
            s.stop(a);
        }
        let t = self.tick;
        s.play("In", t);
        s.play("CountDown_00", t);
        s.play("CountDown_01", t + 32);
        s.play("CountDown_02", t + 64);
        self.skipping = true;
        self.skip_start = t;
    }

    pub fn skip_cancel(&mut self) {
        if !self.skipping {
            return;
        }
        self.skipping = false;
        self.skip_shown_until = 0;
    }

    /// The hold is complete: "Skip!" (Pass). Returns its length in ticks.
    pub fn skip_pass(&mut self) -> i32 {
        self.skipping = false;
        let Some(p) = &mut self.skip_icon else {
            return 1;
        };
        p.text_from("T_Pass_00", "T_Pass_00");
        let t = self.tick;
        p.play("In", t);
        p.play_loop("Loop", t);
        p.play("Pass", t);
        p.ticks("Pass", 1)
    }

    // MARK: death iris

    /// Layout position of a point of the 384 × 216 view (px from its bottom-left).
    pub fn layout_point(x: f32, y: f32) -> [f32; 2] {
        let cx = x.clamp(0.0, 384.0);
        let cy = y.clamp(0.0, 216.0);
        [(cx - 192.0) * 1280.0 / 384.0, (cy - 108.0) * 720.0 / 216.0]
    }

    /// The black circle closes on `center` (layout units) [inf].
    pub fn iris_out(&mut self, center: [f32; 2]) {
        let Some(i) = &mut self.iris else { return };
        i.show("N_Pos_00", true);
        i.show("W_CircleB_00", true);
        i.set_translate("N_Pos_00", center);
        i.stop("InCircleB");
        i.play("OutCircleB", self.tick);
        self.iris_until = self.tick + i.ticks("OutCircleB", 1);
        self.iris_closed = true;
    }

    /// The course restarted: the circle opens on the player.
    pub fn iris_in(&mut self, center: [f32; 2]) {
        let Some(i) = &mut self.iris else { return };
        i.show("N_Pos_00", true);
        i.show("W_CircleB_00", true);
        i.set_translate("N_Pos_00", center);
        i.stop("OutCircleB");
        i.play("InCircleB", self.tick);
        self.iris_until = self.tick + i.ticks("InCircleB", 1);
        self.iris_closed = false;
    }

    // MARK: course card

    fn intro_anim(&self) -> &'static str {
        if self.info.lives {
            "AnimEndlessMode"
        } else {
            "AnimDefault"
        }
    }

    /// The course card on black: name, maker, condition, and lives when the play has them.
    pub fn show_intro(&mut self, lives: u32) {
        if self.intro.is_none() || self.fade.is_none() {
            return;
        }
        self.hud_out();
        self.closing = None;
        self.set_fade(false);
        let style = self.style();
        let cond = self.status.condition;
        let anim = self.intro_anim();
        let zanki = self
            .ui
            .format("Play_CourseIn_00", "T_Zanki_Number", &[lives.to_string()])
            .unwrap_or_else(|| lives.to_string());
        let sentence = cond.map(|c| sentence(&self.ui, &c, style));
        let c = self.intro.as_mut().expect("checked");
        c.text("T_CourseName_00", &self.info.name);
        match self.info.maker.as_deref() {
            Some(m) if !m.is_empty() => {
                c.hold("Maker", 1.0);
                c.text("T_PlayerName_00", m);
            }
            _ => c.hold("Maker", 0.0),
        }
        for p in [
            "N_Align_00",
            "L_MiiIcon_00",
            "L_NationalFlag_00",
            "T_CourseID_00",
            "L_LoadingGhost_00",
            "N_EventAlign_00",
            "N_EventFirst_00",
            "P_GhostVolumenone_00",
            "N_ClearCheckAll_00",
        ] {
            c.show(p, false);
        }
        if let (Some(cond), Some(s)) = (cond, sentence) {
            c.hold("Mission", 1.0);
            c.text("T_Mission_00", &s);
            let ok = set_icon(c, &cond, "L_MissionIcon_00/P_DummyIcon_00", style);
            c.visible("L_MissionIcon_00", if ok { None } else { Some(false) });
            // the icon just left of the centred sentence, the pair centred [inf]
            if ok && let Some(w) = c.text_width("T_Mission_00") {
                let shift = 38.0;
                c.set_translate("T_Mission_00", [shift, 0.0]);
                c.set_translate(
                    "L_MissionIcon_00",
                    [(shift - w.min(900.0) / 2.0 - 45.0).max(-440.0), 0.0],
                );
            }
        } else {
            c.hold("Mission", 0.0);
        }
        c.text("T_Zanki_00", &zanki);
        for a in ["AnimEndlessMode", "AnimDefault"] {
            c.stop(a);
        }
        c.play(anim, self.tick);
        self.ui.sound_anim("Play_CourseIn_00", anim, None);
        self.iris_closed = false;
        self.iris_until = 0;
        self.set_mode(Mode::Intro);
    }

    pub fn intro_length(&mut self) -> i32 {
        let a = self.intro_anim();
        self.intro.as_mut().map_or(1, |i| i.ticks(a, 1))
    }

    /// The card is gone: fade the black away, bring the HUD in.
    pub fn end_intro(&mut self) {
        if self.mode != Mode::Intro {
            return;
        }
        if let Some(f) = &mut self.fade {
            f.stop("FadeOutB");
            f.play("FadeInB", self.tick);
            self.fade_in_until = self.tick + f.ticks("FadeInB", 1);
        }
        self.set_mode(Mode::Play);
        self.hud_in();
    }

    /// Straight into play (no card): the HUD fades in.
    pub fn start_playing(&mut self) {
        self.set_mode(Mode::Play);
        self.hud_in();
    }

    // MARK: pause

    pub fn show_pause(&mut self) {
        if self.pause_l.is_none() {
            return;
        }
        let style = self.style();
        let skin = self.skin();
        let cond = self.status.condition;
        let sentence = cond.map(|c| sentence(&self.ui, &c, style));
        let none = self.ui.message("GoalCondition", "None").unwrap_or_default();
        let checkpoint = self.status.checkpoint;
        let info = self.info.clone();
        let voted = self.voted;
        let p = self.pause_l.as_mut().expect("checked");
        // Type 3: Start Over + Exit Course, no Edit / likes / comments; a server course: Type 2,
        // with the judge buttons (Boo / Like); TypeCourseInfo 1: the Course World information
        // block [inf]
        let judge = info.server_code.is_some();
        p.hold("Type", if judge { 2.0 } else { 3.0 });
        if judge {
            judge_buttons(p, voted);
        }
        p.hold("TypeCourseInfo", 1.0);
        let label = info.mode_label.clone().unwrap_or_else(|| {
            if info.built_in {
                "T_Mode_Quest".into()
            } else {
                "T_Mode_Network".into()
            }
        });
        match &info.mode_text {
            Some(t) => p.text("T_Mode_00", t),
            None => p.text_from("T_Mode_00", &label),
        }
        // a run's exit is "Quit" (save and quit) [inf]
        if info.lives {
            p.text_from("L_ExitBtn_00/T_Btn_00", "L_ExitSaveBtn_00-T_Btn_00");
        }
        p.text("T_CourseTitle_00", &info.name);
        for n in [
            "L_MiiIcon_00",
            "P_GoodIcon_00",
            "L_GoodCount_00",
            "N_TagAll_00",
            "L_KeyBtn_00",
            "L_Flag_00",
            "L_BadgeList_00",
        ] {
            p.show(n, false);
        }
        p.text_from("L_OptionBtn_00/T_Btn_00", "L_OptionBtn_00-T_Btn_00");
        p.show("L_OptionBtn_00/L_NewIcon_00", false);
        match info.maker.as_deref() {
            Some(m) if !m.is_empty() => {
                p.visible("N_PlayerInfo_00", None);
                p.text("T_PlayerName_00", m);
            }
            _ => p.show("N_PlayerInfo_00", false),
        }
        match info.best_ticks {
            Some(t) => {
                let ms = t * 1000 / 60;
                let a = p
                    .format("0001", &[digits(ms / 60000, 2), digits(ms / 1000 % 60, 2)])
                    .unwrap_or_default();
                let b = p
                    .format("0002", &[digits(ms % 1000, 3)])
                    .unwrap_or_default();
                p.text("T_Time_01", &a);
                p.text("T_Time_02", &b);
            }
            None => {
                let a = p.message("0003").unwrap_or_default();
                let b = p.message("0004").unwrap_or_default();
                p.text("T_Time_01", &a);
                p.text("T_Time_02", &b);
            }
        }
        // the clear condition line (Cmn_Mission_00: frame 0 icon + counter, 1 icon, 3 none)
        let m = "L_Mission_00";
        p.show(&format!("{m}/N_MainMenu_00"), false);
        if let (Some(c), Some(s)) = (cond, sentence) {
            p.hold(
                &format!("{m}/TypeMission"),
                if counted(&c) { 0.0 } else { 1.0 },
            );
            p.text(&format!("{m}/T_Mission_00"), &s);
            let pane = format!("{m}/L_MissionIcon_00/P_DummyIcon_00");
            let ok = set_icon(p, &c, &pane, style);
            p.visible(&pane, if ok { None } else { Some(false) });
            if counted(&c) {
                p.text(
                    &format!("{m}/T_MissionNum_00"),
                    &format!("{}/{}", c.count.min(c.needed), c.needed),
                );
            }
            p.visible(
                &format!("{m}/W_MissionFailed_00"),
                if c.violated { Some(true) } else { None },
            );
        } else {
            p.hold(&format!("{m}/TypeMission"), 3.0);
            p.text(&format!("{m}/T_Mission_00"), &none);
        }
        // "Retry" once a checkpoint was reached, else "Start Over"
        let retry = if checkpoint {
            "L_RestartBtn_CheckPoint"
        } else {
            "L_RestartBtn_Start"
        };
        p.text_from("L_RetryBtn_00/T_Btn_00", retry);
        p.stop("Out");
        p.play("In", self.tick);
        self.closing = None;
        self.set_fade(true);
        self.hud_out();
        // the pause jingle of the course's game style
        let ctx = format!("In_Layout_{skin}");
        self.ui
            .sound_key("Cmn_PauseMenu_00", "In_Layout", &[ctx.as_str()]);
        self.set_mode(Mode::Pause);
        self.pause_select(0, false);
    }

    /// How many pause buttons there are (4, or 6 with a server course's judge buttons: 4 Boo,
    /// 5 Like).
    pub fn pause_buttons(&self) -> usize {
        if self.info.server_code.is_some() {
            6
        } else {
            4
        }
    }

    /// A vote in the pause menu (see [`CourseScreens::clear_voted`]).
    pub fn pause_voted(&mut self, like: bool) {
        self.voted = Some(like);
        let t = self.tick;
        let Some(p) = &mut self.pause_l else { return };
        let (on, off) = if like {
            ("L_GoodBtn_00", "L_NegativeBtn_00")
        } else {
            ("L_NegativeBtn_00", "L_GoodBtn_00")
        };
        p.stop(&format!("{off}/Decide"));
        p.play(&format!("{on}/Decide"), t);
    }

    /// The pause button `i`'s decide sound alone.
    pub fn pause_decide_sound(&self, i: usize) {
        self.ui
            .sound_button("Cmn_PauseMenu_00", pause_part(i), Event::Decide);
    }

    pub fn pause_select(&mut self, i: usize, sound: bool) {
        self.pause_selection = i.min(self.pause_buttons() - 1);
        if sound {
            self.ui.sound_button(
                "Cmn_PauseMenu_00",
                pause_part(self.pause_selection),
                Event::Select,
            );
        }
        let (sel, t, n) = (self.pause_selection, self.tick, self.pause_buttons());
        let Some(p) = &mut self.pause_l else { return };
        for (k, part) in (0..n).map(pause_part).enumerate() {
            let a = format!("{part}/Select");
            if k == sel {
                p.play(&a, t);
            } else {
                p.hold(&a, 0.0);
            }
        }
    }

    /// The chosen button's Decide; returns its length in ticks.
    pub fn pause_decide(&mut self) -> i32 {
        let part = pause_part(self.pause_selection);
        self.ui
            .sound_button("Cmn_PauseMenu_00", part, Event::Decide);
        let t = self.tick;
        let Some(p) = &mut self.pause_l else { return 0 };
        let a = format!("{part}/Decide");
        p.play(&a, t);
        p.ticks(&a, 0)
    }

    pub fn hide_pause(&mut self) {
        if self.mode != Mode::Pause || self.pause_l.is_none() {
            return;
        }
        self.ui.sound_key("Cmn_PauseMenu_00", "Out_Layout", &[]);
        let t = self.tick;
        let p = self.pause_l.as_mut().expect("checked");
        p.stop("In");
        p.play("Out", t);
        self.closing = Some((Which::Pause, t + p.ticks("Out", 1)));
        self.hud_in();
        self.set_mode(Mode::Play);
    }

    // MARK: settings window

    /// The settings window over the pause menu: `Type` 1, the offline variant; its character row
    /// [obs layout]; the course-making and player-controls rows are hidden [departure].
    pub fn show_options(&mut self, character: usize) {
        let Some(o) = &mut self.options_l else { return };
        self.character = character.min(3);
        o.hold("Type", 1.0);
        for n in [
            "N_EditMode_00",
            "N_PlayerSetting_00",
            "L_HelpBtn_00",
            "L_RuleBtn_00",
        ] {
            o.show(n, false);
        }
        for label in [
            "L_Window_00-T_Index_00",
            "T_Character_00",
            "L_OKBtn_00-T_Btn_00",
            "L_CharacterBtn_00-T_Btn_00",
            "L_CharacterBtn_01-T_Btn_00",
            "L_CharacterBtn_02-T_Btn_00",
            "L_CharacterBtn_03-T_Btn_00",
        ] {
            o.text_from(&label.replace('-', "/"), label);
        }
        for (k, part) in OPTION_PARTS.iter().take(4).enumerate() {
            let a = format!("{part}/Toggle");
            let f = if k == self.character {
                o.length(&a)
            } else {
                0.0
            };
            o.hold(&a, f);
        }
        o.stop("Out");
        o.play("In", self.tick);
        self.set_mode(Mode::Options);
        self.options_select(self.character, false);
    }

    pub fn options_select(&mut self, i: usize, sound: bool) {
        let i = i.min(OPTION_PARTS.len() - 1);
        let changed = self.option_selection != i;
        self.option_selection = i;
        if sound && changed {
            self.ui
                .sound_button("Cmn_OptionWindow_00", OPTION_PARTS[i], Event::Select);
        }
        let t = self.tick;
        let Some(o) = &mut self.options_l else { return };
        for (k, part) in OPTION_PARTS.iter().enumerate() {
            let a = format!("{part}/Select");
            if k == i {
                o.play(&a, t);
            } else {
                o.hold(&a, 0.0);
            }
        }
    }

    /// Confirm on the selected item: a character becomes the chosen one (Toggle); returns the
    /// Decide's length, and whether it was OK (the window closes).
    pub fn options_decide(&mut self) -> (i32, bool) {
        let sel = self.option_selection;
        let part = OPTION_PARTS[sel];
        self.ui
            .sound_button("Cmn_OptionWindow_00", part, Event::Decide);
        let t = self.tick;
        let Some(o) = &mut self.options_l else {
            return (0, true);
        };
        o.play(&format!("{part}/Decide"), t);
        if sel < 4 && sel != self.character {
            let old = format!("{}/Toggle", OPTION_PARTS[self.character]);
            o.stop(&old);
            let from = o.length(&format!("{part}/Toggle"));
            o.play_ex(&old, t, from, Some(0.0), false, -1.0);
            self.character = sel;
            o.play(&format!("{part}/Toggle"), t);
        }
        (o.ticks(&format!("{part}/Decide"), 1), sel == 4)
    }

    /// Back to the pause menu.
    pub fn hide_options(&mut self) {
        if self.mode != Mode::Options {
            return;
        }
        let t = self.tick;
        let Some(o) = &mut self.options_l else { return };
        o.stop("In");
        o.play("Out", t);
        self.closing = Some((Which::Options, t + o.ticks("Out", 1)));
        self.set_mode(Mode::Pause);
        self.pause_select(PAUSE_PARTS.len() - 1, true);
    }

    // MARK: course clear

    /// `run`: a 100 Mario run's clear: Quit (save and quit) / Next instead of Start Over / Quit.
    pub fn show_clear(&mut self, ticks: Option<i64>, run: bool) {
        if self.clear_l.is_none() {
            return;
        }
        self.hud_out();
        let info = self.info.clone();
        let voted = self.voted;
        let c = self.clear_l.as_mut().expect("checked");
        // Type 2 with the network parts hidden; the banner (N_Cap_00) drawn in place,
        // undistorted (the game shows a distorted capture of it) [inf]
        c.hold("Type", 2.0);
        c.hold("RegionLanguageType", 1.0);
        let judge = info.server_code.is_some();
        for n in [
            "PF_Space_00",
            "PF_Claer_00",
            "L_CommentBtn_00",
            "L_TagBtn_00",
            "N_CountDown_00",
            "N_Comment_00",
        ] {
            c.show(n, false);
        }
        // a server course: Like / Boo, the game's own judge buttons [obs layout]
        c.show("N_JudgeBtn_00", judge);
        c.show("L_OrdinaryBtn_00", false);
        judge_buttons(c, voted);
        c.set_translate("N_Cap_00", [0.0, 0.0]);
        c.show("P_InNetBG_00", true);
        // the glow lettering under the card's letter holes stays white (no frame capture) [inf]
        c.material_white("P_ClearMax_00", Some([255, 255, 255, 255]));
        // without the Comment / Tag buttons on its left the pair is centred [departure]; with the
        // judge buttons it keeps the layout's place
        if judge {
            c.clear_pane("N_Btn_00");
        } else if let Some(b) = c.pane("N_Btn_00") {
            c.set_translate("N_Btn_00", [-201.0, b.translate[1]]);
        }
        c.text_from("L_Btn_01/T_Btn_00", if run { "0001" } else { "0000" });
        c.text_from("L_Btn_00/T_Btn_00", if run { "0004" } else { "0001" });
        fill_clear_info(c, &info, ticks);
        c.stop("OutContents");
        c.play("InNet", self.tick);
        self.set_mode(Mode::Clear);
        self.clear_select(1, false);
    }

    /// Buttons take input once InNet has brought them on screen.
    pub fn clear_ready(&mut self) -> bool {
        let n = self
            .clear_l
            .as_mut()
            .map_or(0, |c| c.length("InNet") as i32);
        self.mode == Mode::Clear && self.since_mode() >= n
    }

    /// How many of [`CLEAR_PARTS`] the clear screen shows (the judge buttons for a server
    /// course).
    pub fn clear_buttons(&self) -> usize {
        if self.info.server_code.is_some() {
            4
        } else {
            2
        }
    }

    /// The clear screen's button left (`right` false) or right of `i`.
    pub fn clear_neighbour(&self, i: usize, right: bool) -> usize {
        let order: Vec<usize> = CLEAR_ORDER
            .iter()
            .copied()
            .filter(|&k| k < self.clear_buttons())
            .collect();
        let at = order.iter().position(|&k| k == i).unwrap_or(0);
        let to = if right {
            (at + 1).min(order.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        order[to]
    }

    /// A vote on the clear screen: the button keeps its pressed look (the end of its Decide),
    /// the other one its plain look [inf].
    pub fn clear_voted(&mut self, like: bool) {
        self.voted = Some(like);
        let Some(c) = &mut self.clear_l else { return };
        let (on, off) = if like {
            ("L_GoodBtn_00", "L_NegativeBtn_00")
        } else {
            ("L_NegativeBtn_00", "L_GoodBtn_00")
        };
        c.stop(&format!("{off}/Decide"));
        let t = self.tick;
        c.play(&format!("{on}/Decide"), t);
    }

    pub fn clear_select(&mut self, i: usize, sound: bool) {
        let i = i.min(self.clear_buttons() - 1);
        if sound && self.clear_selection != i {
            self.ui
                .sound_button("Play_CourseClear_00", CLEAR_PARTS[i], Event::Select);
        }
        self.clear_selection = i;
        let t = self.tick;
        let Some(c) = &mut self.clear_l else { return };
        for (k, part) in CLEAR_PARTS.iter().enumerate() {
            let a = format!("{part}/Select");
            if !c.has(&a) {
                continue;
            }
            if k == i {
                c.play(&a, t);
            } else {
                c.hold(&a, 0.0);
            }
        }
    }

    /// The clear screen button `i`'s decide sound alone.
    pub fn ui_decide_sound(&self, i: usize) {
        self.ui
            .sound_button("Play_CourseClear_00", CLEAR_PARTS[i], Event::Decide);
    }

    pub fn clear_decide(&mut self) -> i32 {
        let part = CLEAR_PARTS[self.clear_selection];
        self.ui
            .sound_button("Play_CourseClear_00", part, Event::Decide);
        let t = self.tick;
        let Some(c) = &mut self.clear_l else { return 0 };
        let a = format!("{part}/Decide");
        c.play(&a, t);
        c.ticks(&a, 0)
    }

    // MARK: give up

    pub fn show_retire(&mut self) {
        if self.retire_l.is_none() {
            return;
        }
        self.closing = None;
        let info = self.info.clone();
        let r = self.retire_l.as_mut().expect("checked");
        r.hold("Type", 0.0);
        // like the clear banner: the captured N_Cap_00 in place of the waves [inf]
        for n in [
            "N_JudgeBtn_00",
            "N_CountDown_00",
            "P_LoseBG_000",
            "P_Wave_00",
            "P_Wave_01",
            "P_wave_02",
        ] {
            r.show(n, false);
        }
        r.set_translate("N_Cap_00", [0.0, 0.0]);
        for n in RETIRE_PARTS {
            r.show(n, true);
        }
        r.text_from("T_Retire_00", "0002");
        r.text_from("L_Btn_00/T_Btn_00", "0004");
        fill_clear_info(r, &info, None);
        r.stop("OutContents");
        r.play("InNet", self.tick);
        self.set_mode(Mode::Retire);
        self.retire_select(1, false);
    }

    pub fn retire_ready(&mut self) -> bool {
        let n = self
            .retire_l
            .as_mut()
            .map_or(0, |r| r.length("InNet") as i32);
        self.mode == Mode::Retire && self.since_mode() >= n / 2
    }

    pub fn retire_select(&mut self, i: usize, sound: bool) {
        let i = i.min(1);
        if sound && self.retire_selection != i {
            self.ui
                .sound_button("Play_CourseRetire_00", RETIRE_PARTS[i], Event::Select);
        }
        self.retire_selection = i;
        let t = self.tick;
        let Some(r) = &mut self.retire_l else { return };
        for (k, part) in RETIRE_PARTS.iter().enumerate() {
            let a = format!("{part}/Select");
            if k == i {
                r.play(&a, t);
            } else {
                r.hold(&a, 0.0);
            }
        }
    }

    pub fn retire_decide(&mut self) -> i32 {
        let part = RETIRE_PARTS[self.retire_selection];
        self.ui
            .sound_button("Play_CourseRetire_00", part, Event::Decide);
        let t = self.tick;
        let Some(r) = &mut self.retire_l else {
            return 0;
        };
        let a = format!("{part}/Decide");
        r.play(&a, t);
        r.ticks(&a, 0)
    }

    /// "Keep playing": the screen leaves, play goes on.
    pub fn hide_retire(&mut self) {
        if self.mode != Mode::Retire {
            return;
        }
        let t = self.tick;
        let Some(r) = &mut self.retire_l else { return };
        r.stop("InNet");
        r.hold_end("InNet");
        r.play("OutContents", t);
        self.closing = Some((Which::Retire, t + r.ticks("OutContents", 1)));
        self.hud_in();
        self.set_mode(Mode::Play);
    }

    // MARK: time up, game over

    /// "TIME'S UP!" over the player's death when the timer runs out.
    pub fn show_time_up(&mut self) {
        let t = self.tick;
        let Some(l) = &mut self.time_up_l else { return };
        l.stop("OneTime");
        l.play("OneTime", t);
        self.time_up_until = t + l.ticks("OneTime", 1);
    }

    pub fn show_game_over(&mut self) {
        if self.game_over_l.is_none() {
            return;
        }
        self.hud_out();
        self.closing = None;
        let t = self.tick;
        let g = self.game_over_l.as_mut().expect("checked");
        g.text_from("T_Text_00", "T_GameOver");
        g.play("OneTime", t);
        self.set_mode(Mode::GameOver);
    }

    pub fn game_over_length(&mut self) -> i32 {
        self.game_over_l
            .as_mut()
            .map_or(60, |g| g.ticks("OneTime", 60))
    }

    pub fn clear_all(&mut self) {
        self.closing = None;
        self.time_up_until = 0;
        self.set_mode(Mode::Play);
    }
}

fn fill_clear_info(l: &mut Layout, info: &CardInfo, ticks: Option<i64>) {
    let i = "L_ClearInfo_00";
    l.text(&format!("{i}/T_CourseName_00"), &info.name);
    match info.maker.as_deref() {
        Some(m) if !m.is_empty() => {
            l.visible(&format!("{i}/N_CourseMaker_00"), None);
            l.text(&format!("{i}/T_PlayerName_00"), m);
        }
        _ => l.show(&format!("{i}/N_CourseMaker_00"), false),
    }
    for n in [
        "L_MiiIcon_00",
        "L_MiiIcon_01",
        "L_MiiIcon_02",
        "L_NationalFlag_00",
        "N_Align_00",
        "N_FirstTime_00",
    ] {
        l.show(&format!("{i}/{n}"), false);
    }
    let Some(ticks) = ticks else {
        l.show(&format!("{i}/N_AllTime_00"), false);
        return;
    };
    let ms = ticks * 1000 / 60;
    let main = l
        .format("0010", &[digits(ms / 60000, 2), digits(ms / 1000 % 60, 2)])
        .unwrap_or_default();
    let frac = l
        .format("0011", &[digits(ms % 1000, 3)])
        .unwrap_or_default();
    l.text(&format!("{i}/T_Time_00"), &main);
    l.text(&format!("{i}/T_Time_01"), &frac);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click on the give-up screen's buttons hits them once the screen is up (with the owner's
    /// SMM2 UI import; skipped without it).
    #[test]
    fn retire_buttons_take_clicks() {
        let Some(ui) = crate::testing::smm2() else {
            return;
        };
        let mut s = CourseScreens::new(&ui).unwrap();
        s.show_retire();
        while !s.retire_ready() {
            s.step();
        }
        for (i, part) in RETIRE_PARTS.iter().enumerate() {
            let tick = s.tick;
            let l = s.retire_l.as_mut().unwrap();
            l.apply(tick);
            let r = l.rect(&format!("{part}/B_Btn_00")).unwrap();
            assert_eq!(s.hit(r.center), Some(i), "{part}");
        }
        assert_eq!(s.hit([0.0, -340.0]), None);
    }
}
