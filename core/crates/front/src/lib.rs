//! The SMM2 front end, platform-neutral: the screens of the Swift app's SMM2 presentation
//! (opening, scene fades, title and Play menu, Course World over a local library and the course
//! server, 100 Mario Challenge in SMM2's Endless Challenge screens or SMM1's, the in-course HUD /
//! course card / pause / clear / give up / time up / game over screens, and the UI sounds) as
//! state machines over [`mmk_ui::LayoutPlayer`]s.
//!
//! A host (the portable shell, later the Swift app through the FFI) owns the window, the engine,
//! files and audio. Each 60 Hz tick it hands [`Front::tick`] an abstract input ([`FrontInput`]:
//! menu buttons, pause, skip, a click) and a [`Host`] for the course hooks, then draws
//! [`Front::frame`]: layouts in order (quads + layout size, see [`DrawItem`]), with the course
//! view where [`DrawItem::Course`] stands, and plays the frame's [`SoundCue`]s (WAVs of the SMM2
//! UI import's `ui/sound/`).

pub mod account;
pub mod boot;
pub mod challenge;
pub mod coursepage;
pub mod endless;
pub mod fade;
pub mod host;
pub mod layout;
pub mod leaderboard;
pub mod menu;
pub mod online;
pub mod play;
pub mod screens;
pub mod smm1;
pub mod sound;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests_online;
pub mod title;
pub mod world;

pub use host::{CourseEntry, CourseStatus, Host, Saves, Source, WorldCourse};
pub use layout::{Layout, Ui, UiKind};
pub use menu::{Buttons, UiInput};
pub use sound::{SoundCue, UiSounds};

use challenge::{ChallengeEvent, ChallengeFlow, ChallengeGo, Presentation};
use fade::{FadeKind, FadeStart, SceneFade};
use menu::Script;
use play::PlayInput;
use title::{Title, TitleChoice, TitleEvent};
use world::{ListKind, WorldEvent, WorldFlow, WorldGo};

/// One thing to draw, back to front.
#[derive(Clone, Debug)]
pub enum DrawItem {
    /// A layout's draw list: `size` layout units (origin centre, y up) mapped onto the 16:9
    /// view; texture ids are the [`Ui`]'s of `ui`.
    Layout {
        ui: UiKind,
        size: [f32; 2],
        quads: Vec<mmk_ui::Quad>,
    },
    /// The course view (the engine's picture, the whole 16:9 view).
    Course,
}

/// Append `l`'s draw list at `tick`.
pub fn push_layout(out: &mut Vec<DrawItem>, l: &mut Layout, tick: i32) {
    let mut quads = Vec::new();
    l.draw(tick, &mut quads);
    out.push(DrawItem::Layout {
        ui: l.ui().kind(),
        size: l.size(),
        quads,
    });
}

/// What to show now.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub items: Vec<DrawItem>,
    /// A short message to show over the screen (the Swift app's toasts and alerts).
    pub toast: Option<String>,
}

/// The input for one tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrontInput {
    /// Menu buttons held.
    pub menu: Buttons,
    /// Pause (START) held.
    pub start: bool,
    /// The skip key held (100 Mario).
    pub skip: bool,
    /// A click / tap this tick, layout units (1280 × 720 view, origin centre, y up).
    pub click: Option<[f32; 2]>,
    /// A key typed this tick while a text field is open ([`Front::typing`]): a character,
    /// '\u{8}' to erase the last one, '\n' to finish, '\u{1b}' to cancel. Hosts without a
    /// keyboard leave it None (one key per tick; queue the rest).
    pub text: Option<char>,
}

/// A deferred scene change (run when its fade has covered the screen).
#[derive(Clone, Debug, PartialEq)]
pub enum Go {
    Title {
        start: title::State,
        select: Option<usize>,
    },
    Enter(TitleChoice),
    CourseWorld {
        selection: usize,
    },
    ShowChallenge {
        presentation: Option<Presentation>,
        back: Back,
    },
    World(WorldGo),
    Challenge(ChallengeGo),
}

/// Where the 100 Mario Challenge returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Title,
    CourseWorld,
}

/// What a screen's step may use: the host, the save store, the fade and the toast.
pub struct Cx<'a> {
    pub host: &'a mut dyn Host,
    pub saves: &'a mut Saves,
    /// The server account and the answers of server calls.
    pub online: &'a mut online::Online,
    fade: &'a mut SceneFade,
    pending: &'a mut Option<Go>,
    now: &'a mut Vec<Go>,
    toast: &'a mut Option<(String, i32)>,
}

impl Cx<'_> {
    /// Change the scene behind the fade `kind` (at once when a fade already runs).
    pub fn fade_to(&mut self, kind: FadeKind, go: Go) {
        match self.fade.run(kind, [0.0, 0.0]) {
            FadeStart::Started => *self.pending = Some(go),
            FadeStart::Now => self.now.push(go),
        }
    }

    /// Show a short message.
    pub fn toast(&mut self, text: &str) {
        *self.toast = Some((text.to_string(), TOAST_TICKS));
    }
}

const TOAST_TICKS: i32 = 180;

/// Seconds since 1970 → (year, month, day, hour, minute), UTC.
pub fn civil(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Howard Hinnant's civil-from-days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (
        if m <= 2 { y + 1 } else { y },
        m,
        d,
        (rem / 3600) as u32,
        (rem / 60 % 60) as u32,
    )
}

/// How the front end starts.
#[derive(Clone, Debug)]
pub struct Options {
    /// Play the opening first.
    pub boot: bool,
    /// Force this opening (InDefault, InLink …).
    pub opening: Option<String>,
    /// The title screen's starting state when there is no opening.
    pub title_start: title::State,
    /// Start the Play menu on this button.
    pub title_select: Option<usize>,
    /// Start with this title demo course.
    pub title_course: Option<usize>,
    /// 100 Mario presentation.
    pub hundred: Presentation,
    /// Captures: scene changes happen at once (as the Swift app's frozen captures do).
    pub instant_fades: bool,
    /// Captures: menu presses at global UI ticks (`MMK_UI_SCRIPT`).
    pub script: String,
    /// Captures: title presses at title ticks (`MMK_UI_INPUT`).
    pub title_input: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            boot: true,
            opening: None,
            title_start: title::State::Top,
            title_select: None,
            title_course: None,
            hundred: Presentation::Smm2,
            instant_fades: false,
            script: String::new(),
            title_input: String::new(),
        }
    }
}

pub enum Screen {
    Boot(Box<boot::BootScene>),
    Title(Box<Title>),
    World(Box<WorldFlow>),
    Challenge(Box<ChallengeFlow>, Back),
    /// One course with the SMM2 screens (the Swift app's library play / `MMK_COURSE`).
    Course(Box<play::CoursePlay>),
    None,
}

/// The whole front end.
pub struct Front {
    pub smm2: Ui,
    pub smm1: Option<Ui>,
    pub screen: Screen,
    pub fade: SceneFade,
    pending: Option<Go>,
    now: Vec<Go>,
    pub saves: Saves,
    pub online: online::Online,
    toast: Option<(String, i32)>,
    held: Buttons,
    pub opts: Options,
    /// Global UI ticks.
    pub tick: i64,
    /// Set when the player left a direct course play (`Screen::Course`).
    pub course_done: bool,
}

impl Front {
    pub fn new(smm2: Ui, smm1: Option<Ui>, opts: Options, host: &mut dyn Host) -> Self {
        let mut fade = SceneFade::new(Some(&smm2));
        fade.instant = opts.instant_fades;
        let mut f = Self {
            saves: Saves::load(host),
            online: online::Online::load(host),
            smm2,
            smm1,
            screen: Screen::None,
            fade,
            pending: None,
            now: Vec::new(),
            toast: None,
            held: Buttons::NONE,
            opts,
            tick: 0,
            course_done: false,
        };
        let boot = if f.opts.boot {
            boot::BootScene::new(&f.smm2, f.opts.opening.as_deref(), host)
        } else {
            None
        };
        match boot {
            Some(b) => f.screen = Screen::Boot(Box::new(b)),
            None => {
                let (start, select) = (f.opts.title_start, f.opts.title_select);
                f.show_title(host, start, select);
            }
        }
        f
    }

    fn cx<'a>(&'a mut self, host: &'a mut dyn Host) -> (Cx<'a>, &'a mut Screen) {
        (
            Cx {
                host,
                saves: &mut self.saves,
                online: &mut self.online,
                fade: &mut self.fade,
                pending: &mut self.pending,
                now: &mut self.now,
                toast: &mut self.toast,
            },
            &mut self.screen,
        )
    }

    /// Run `f` with a context (for hosts and captures that drive a screen directly).
    pub fn with_cx<R>(
        &mut self,
        host: &mut dyn Host,
        f: impl FnOnce(&mut Cx, &mut Screen) -> R,
    ) -> R {
        let (mut cx, screen) = self.cx(host);
        let r = f(&mut cx, screen);
        self.run_now(host);
        r
    }

    fn show_title(&mut self, host: &mut dyn Host, start: title::State, select: Option<usize>) {
        host.unload_course();
        match Title::new(&self.smm2, start, self.opts.title_course, host) {
            Some(mut t) => {
                if let Some(i) = select {
                    t.scene.select_category_now(i);
                }
                t.scene.script = Script::parse(&self.opts.title_input);
                self.screen = Screen::Title(Box::new(t));
            }
            None => self.screen = Screen::None,
        }
    }

    fn show_world(&mut self, host: &mut dyn Host, kind: ListKind, selection: usize) {
        // (the title's demo course stops with the title)
        host.unload_course();
        let lib = host.library();
        let smm2 = self.smm2.clone();
        let script = self.opts.script.clone();
        let (mut cx, screen) = self.cx(host);
        *screen = match WorldFlow::new(&smm2, kind, lib, selection, &mut cx) {
            Some(mut f) => {
                f.script = Script::parse(&script);
                Screen::World(Box::new(f))
            }
            None => Screen::None,
        };
    }

    fn show_challenge(
        &mut self,
        host: &mut dyn Host,
        presentation: Option<Presentation>,
        back: Back,
    ) {
        host.unload_course();
        let lib = host.library();
        let (smm2, smm1) = (self.smm2.clone(), self.smm1.clone());
        let wanted = presentation.unwrap_or(self.opts.hundred);
        let (mut cx, screen) = self.cx(host);
        let flow = ChallengeFlow::new(&mut cx, &smm2, smm1.as_ref(), &lib, wanted);
        *screen = Screen::Challenge(Box::new(flow), back);
    }

    /// Play one course with the SMM2 screens (already loaded by the host): `intro` the card
    /// first. Unlimited retries, as library play.
    pub fn play_course(
        &mut self,
        host: &mut dyn Host,
        name: &str,
        key: Option<String>,
        intro: bool,
    ) {
        let info = screens::CardInfo {
            name: name.to_string(),
            best_ticks: key.as_ref().and_then(|k| self.saves.record(k).best_ticks),
            ..Default::default()
        };
        let s = screens::CourseScreens::new(&self.smm2);
        let (mut cx, screen) = self.cx(host);
        let p = play::CoursePlay::new(
            &mut cx,
            name.to_string(),
            key,
            s,
            None,
            info,
            intro,
            true,
            false,
        );
        *screen = Screen::Course(Box::new(p));
    }

    fn apply(&mut self, host: &mut dyn Host, g: Go) {
        match g {
            Go::Title { start, select } => self.show_title(host, start, select),
            Go::Enter(c) => match c {
                TitleChoice::StoryMode => self.show_world(host, ListKind::Story, 0),
                TitleChoice::CourseWorld => self.show_world(host, ListKind::World, 0),
                TitleChoice::Coursebot => self.show_world(host, ListKind::Coursebot, 0),
                TitleChoice::HundredMario => self.show_challenge(host, None, Back::Title),
            },
            Go::CourseWorld { selection } => self.show_world(host, ListKind::World, selection),
            Go::ShowChallenge { presentation, back } => {
                self.show_challenge(host, presentation, back)
            }
            Go::World(w) => {
                let (mut cx, screen) = self.cx(host);
                if let Screen::World(f) = screen {
                    f.go(&mut cx, w);
                }
            }
            Go::Challenge(c) => {
                let (mut cx, screen) = self.cx(host);
                if let Screen::Challenge(f, _) = screen {
                    f.go(&mut cx, c);
                }
            }
        }
    }

    fn run_now(&mut self, host: &mut dyn Host) {
        while !self.now.is_empty() {
            let g = self.now.remove(0);
            self.apply(host, g);
        }
    }

    fn fade_to(&mut self, host: &mut dyn Host, kind: FadeKind, go: Go) {
        let (mut cx, _) = self.cx(host);
        cx.fade_to(kind, go);
        self.run_now(host);
    }

    /// One 60 Hz tick.
    pub fn tick(&mut self, input: &FrontInput, host: &mut dyn Host) {
        self.tick += 1;
        let pressed = input.menu.minus(self.held);
        self.held = input.menu;
        let ui = UiInput {
            pressed,
            held: input.menu,
            skip: input.skip,
            click: input.click,
            text: input.text,
        };
        let play = PlayInput {
            ui,
            start: input.start,
        };
        self.online.poll(host);
        if let Some((_, t)) = &mut self.toast {
            *t -= 1;
            if *t <= 0 {
                self.toast = None;
            }
        }
        let mut after: Option<(FadeKind, Go)> = None;
        let (mut cx, screen) = self.cx(host);
        match screen {
            Screen::Boot(b) => {
                if b.step() {
                    // the boot sequence changes to the title with the black circle [obs]
                    after = Some((
                        FadeKind::CircleBlack,
                        Go::Title {
                            start: title::State::Top,
                            select: None,
                        },
                    ));
                }
            }
            Screen::Title(t) => match t.step(&ui, cx.host) {
                Some(TitleEvent::Choice(c)) => after = Some((FadeKind::Black, Go::Enter(c))),
                Some(TitleEvent::ComingSoon) => cx.toast("The Course Maker is coming soon."),
                None => {}
            },
            Screen::World(f) => match f.step(&play, &mut cx) {
                Some(WorldEvent::Exit) => {
                    // back to the title's Play menu on the button that led here [inf]
                    let select = match f.kind {
                        ListKind::Story => 0,
                        ListKind::World => 1,
                        ListKind::Coursebot => 2,
                    };
                    after = Some((
                        FadeKind::Black,
                        Go::Title {
                            start: title::State::Category,
                            select: Some(select),
                        },
                    ));
                }
                Some(WorldEvent::Endless) => {
                    after = Some((
                        FadeKind::Black,
                        Go::ShowChallenge {
                            presentation: Some(Presentation::Smm2),
                            back: Back::CourseWorld,
                        },
                    ))
                }
                None => {}
            },
            Screen::Challenge(f, back) => {
                if f.step(&play, &mut cx) == Some(ChallengeEvent::Exit) {
                    after = Some((
                        FadeKind::Black,
                        match back {
                            // back to the title's Play menu, on the 100 Mario button [inf]
                            Back::Title => Go::Title {
                                start: title::State::Category,
                                select: Some(3),
                            },
                            Back::CourseWorld => Go::CourseWorld { selection: 3 },
                        },
                    ));
                }
            }
            Screen::Course(p) => {
                if p.step(&play, &mut cx, None) == Some(play::PlayEvent::Exit) {
                    self.course_done = true;
                }
            }
            Screen::None => {}
        }
        self.run_now(host);
        if let Some((k, g)) = after {
            self.fade_to(host, k, g);
        }
        if self.fade.step()
            && let Some(g) = self.pending.take()
        {
            self.apply(host, g);
        }
    }

    /// A text field is open: the host should send typed keys ([`FrontInput::text`]) and keep
    /// the keyboard's letter keys from moving the menus.
    pub fn typing(&self) -> bool {
        match &self.screen {
            Screen::World(f) => f.typing(),
            _ => false,
        }
    }

    /// What to draw and play now.
    pub fn frame(&mut self) -> Frame {
        let mut items = Vec::new();
        match &mut self.screen {
            Screen::Boot(b) => b.draw(&mut items),
            Screen::Title(t) => t.draw(&mut items),
            Screen::World(f) => f.draw(&mut items),
            Screen::Challenge(f, _) => f.draw(&mut items),
            Screen::Course(p) => p.draw(&mut items),
            Screen::None => {}
        }
        self.fade.draw(&mut items);
        Frame {
            items,
            toast: self.toast.as_ref().map(|t| t.0.clone()),
        }
    }

    /// The UI sounds started since the last call (take them every tick).
    pub fn take_sounds(&mut self) -> Vec<SoundCue> {
        let mut sounds = self.smm2.take_cues();
        if let Some(u) = &self.smm1 {
            sounds.extend(u.take_cues());
        }
        sounds
    }

    /// Captures: jump to a screen the way the Swift app's `MMK_UI_SCREEN` does: boot, title,
    /// title-main, title-play, courseworld, courseworld-list, the online screens (account,
    /// courseworld-leaderboards, courseworld-course-page / -comments / -post / -maker / -play /
    /// -pause / -clear of the list's first server course), challenge, challenge-map,
    /// challenge-walk, challenge-gameover, challenge-ending (`difficulty`, `cleared` for the
    /// walk). Returns false for an unknown name.
    pub fn capture(
        &mut self,
        host: &mut dyn Host,
        screen: &str,
        difficulty: usize,
        cleared: usize,
    ) -> bool {
        match screen {
            "boot" => {
                if let Some(b) =
                    boot::BootScene::new(&self.smm2, self.opts.opening.as_deref(), host)
                {
                    self.screen = Screen::Boot(Box::new(b));
                }
            }
            "title" | "title-main" | "title-play" => {
                let start = match screen {
                    "title-main" => title::State::Main,
                    "title-play" => title::State::Category,
                    _ => title::State::Top,
                };
                let sel = self.opts.title_select;
                self.show_title(host, start, sel);
            }
            "courseworld" => self.show_world(host, ListKind::World, 0),
            "courseworld-list" => {
                self.show_world(host, ListKind::World, 0);
                self.apply(host, Go::World(WorldGo::List));
            }
            // the online screens (a server: --server / MMK_SERVER; MMK_FRONT_REALTIME=1 gives
            // its answers time to arrive)
            "account" | "courseworld-leaderboards" => {
                self.show_world(
                    host,
                    ListKind::World,
                    if screen == "account" { 6 } else { 1 },
                );
                let what = if screen == "account" {
                    "account"
                } else {
                    "leaderboards"
                };
                let (mut cx, scr) = self.cx(host);
                let Screen::World(f) = scr else {
                    return false;
                };
                if !f.debug_open(&mut cx, what) {
                    return false;
                }
                self.run_now(host);
            }
            "courseworld-course-page"
            | "courseworld-comments"
            | "courseworld-post"
            | "courseworld-maker"
            | "courseworld-play"
            | "courseworld-pause"
            | "courseworld-clear" => {
                self.show_world(host, ListKind::World, 0);
                self.apply(host, Go::World(WorldGo::List));
                let what = &screen["courseworld-".len()..];
                let what = if what == "course-page" { "page" } else { what };
                let (mut cx, scr) = self.cx(host);
                let Screen::World(f) = scr else {
                    return false;
                };
                f.wait_server(&mut cx, 500);
                if !f.debug_open(&mut cx, what) {
                    return false;
                }
                self.run_now(host);
            }
            "storymode" => self.show_world(host, ListKind::Story, 0),
            s if s.starts_with("challenge") => {
                self.show_challenge(host, None, Back::Title);
                let (mut cx, scr) = self.cx(host);
                let Screen::Challenge(f, _) = scr else {
                    return false;
                };
                match s {
                    "challenge" => f.debug_select(difficulty),
                    "challenge-map" | "challenge-intro" => f.start(&mut cx, difficulty, false),
                    "challenge-walk" => f.debug_map(difficulty, false, cleared),
                    "challenge-gameover" => f.debug_game_over(),
                    "challenge-ending" => {
                        let lib = cx.host.library();
                        f.debug_ending(difficulty, &lib);
                    }
                    "challenge-pause" | "challenge-clear" | "challenge-skip" | "challenge-card" => {
                        f.start(&mut cx, difficulty, false);
                        f.play(&mut cx);
                        let what = &s["challenge-".len()..];
                        f.debug_play(what, &mut cx);
                    }
                    _ => return false,
                }
                self.run_now(host);
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0));
        assert_eq!(civil(1_700_000_000), (2023, 11, 14, 22, 13));
    }
}
