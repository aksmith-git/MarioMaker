//! One play of one course with the front end's screens over it (the Swift app's `GameSession`
//! minus the engine itself, which the host owns): it decides when the engine steps (playing, not
//! paused), runs the pause / course clear / give up / game over states on the SMM2 layouts
//! ([`CourseScreens`]) or, for an SMM1-layout 100 Mario run, the SMM1 overlays
//! ([`RunOverlay`]), and reports to the 100 Mario run ([`RunLink`]) what each attempt ended
//! with.

use crate::host::CourseStatus;
use crate::menu::{Buttons, Script, UiInput};
use crate::screens::{CardInfo, CourseScreens, Mode, SKIP_HOLD};
use crate::smm1::{RunCardInfo, RunOverlay, RunOverlayMode};
use crate::{Cx, DrawItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Paused,
    Cleared,
    GameOver,
    /// SMM1 overlays: the intro card after a death, the skip's Pass animation.
    Card,
    Skipping,
    /// SMM2 layouts: the course card, "You gave up...", the pause menu's settings window.
    Intro,
    Retire,
    Options,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseItem {
    Resume,
    Skip,
    Restart,
    SaveQuit,
    Exit,
    Settings,
}

/// What the 100 Mario run behind a course does with its results (the Swift app's
/// `ChallengeFlow` as seen from `GameSession`).
pub trait RunLink {
    /// A death (or a start-over): true if the run continues on this course.
    fn died(&mut self, cx: &mut Cx) -> bool;
    fn cleared(&mut self, score: u32, coins: u32);
    fn skipped(&mut self);
    fn save_and_quit(&mut self);
    /// Save & Quit on the course-clear screen (the clear is already saved).
    fn save_and_quit_after_clear(&mut self);
    fn extra_lives(&mut self, n: u32);
    /// Put the run's counters into the loaded course.
    fn apply_counters(&mut self, cx: &mut Cx);
    fn can_skip(&self) -> bool;
    /// "100 Mario Challenge (n/total)".
    fn pause_line(&self) -> String;
    fn course_name(&self) -> String;
    fn course_maker(&self) -> String;
    /// The last clear completed the run.
    fn clear_ends_run(&self) -> bool;
    fn world_record_ms(&self) -> Option<i64>;
    /// The SMM1 intro card's contents.
    fn card_info(&self) -> RunCardInfo;
}

/// The run behind a course, if any.
pub type Run<'a> = Option<&'a mut (dyn RunLink + 'static)>;

/// What a step tells the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayEvent {
    /// The player leaves the course (exit, after a clear, game over, skip, Save & Quit).
    Exit,
    /// The course was cleared (Course World reports it to the server).
    Cleared,
}

/// Input for one tick of play.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayInput {
    pub ui: UiInput,
    /// The pause button (START) is held.
    pub start: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    SkipDone,
    Choose(PauseItem),
    OptionsClose,
    Clear { left: bool },
    Retire { leave: bool },
}

pub struct CoursePlay {
    pub phase: Phase,
    pub screens: Option<CourseScreens>,
    pub overlay: Option<RunOverlay>,
    pub title: String,
    pub course_key: Option<String>,
    /// No lives: deaths restart the course without a game over (library, Course World, Story
    /// Mode — SMM2 has no lives there).
    pub unlimited_lives: bool,
    /// Part of a 100 Mario run.
    pub in_run: bool,
    pending: Option<(i32, Pending)>,
    attempt_ticks: i64,
    goal_ticks: Option<i64>,
    was_dying: bool,
    time_up_shown: bool,
    death_center: Option<[f32; 2]>,
    iris_shown: bool,
    lives_gained: u32,
    held_start: bool,
    phase_frames: u32,
    pub pause_selection: usize,
    skip_done_tick: Option<i32>,
    pub clear_ticks: Option<i64>,
    pub new_best: bool,
    /// Captures: behave as if the skip key were held.
    pub debug_skip_held: bool,
    /// Captures: menu presses at global UI ticks while a menu is up.
    pub script: Option<Script>,
    /// Captures: pause after this many engine ticks.
    pub auto_pause_tick: Option<u64>,
    engine_tick: u64,
    /// The player's character (the settings window).
    pub character: usize,
}

const RESULT_GRACE: u32 = 30;

/// Like (or boo) server course `code` as the signed-in account; false (with a message) when
/// signed out or without a server.
fn vote(cx: &mut Cx, code: Option<&str>, like: bool) -> bool {
    let Some(code) = code else {
        return false;
    };
    if !cx.online.signed_in() {
        cx.toast("Sign in from the Course World player card to vote.");
        return false;
    }
    let args = serde_json::json!({ "code": code, "like": like });
    cx.online.call(cx.host, "vote", args).is_some()
}

impl CoursePlay {
    /// A course just loaded into the engine. With `screens`, the SMM2 layouts (`info`; the card
    /// first when `intro`); with `overlay`, the SMM1 run overlays.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cx: &mut Cx,
        title: String,
        course_key: Option<String>,
        screens: Option<CourseScreens>,
        overlay: Option<RunOverlay>,
        info: CardInfo,
        intro: bool,
        unlimited_lives: bool,
        in_run: bool,
    ) -> Self {
        let character = cx
            .host
            .load_value("player.character")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0usize)
            .min(3);
        cx.host.set_character(character as u32);
        if let Some(k) = &course_key {
            cx.saves.add_attempt(cx.host, k);
        }
        let mut p = Self {
            phase: Phase::Playing,
            screens,
            overlay,
            title,
            course_key,
            unlimited_lives,
            in_run,
            pending: None,
            attempt_ticks: 0,
            goal_ticks: None,
            was_dying: false,
            time_up_shown: false,
            death_center: None,
            iris_shown: false,
            lives_gained: 0,
            held_start: true,
            phase_frames: 0,
            pause_selection: 0,
            skip_done_tick: None,
            clear_ticks: None,
            new_best: false,
            debug_skip_held: false,
            script: None,
            auto_pause_tick: None,
            engine_tick: 0,
            character,
        };
        let status = cx.host.course_status();
        if let Some(s) = &mut p.screens {
            s.info = info;
            s.apply_rules();
            s.update(&status);
            if intro {
                s.show_intro(status.lives);
                p.set_phase(Phase::Intro);
            } else {
                s.start_playing();
            }
        }
        p
    }

    fn set_phase(&mut self, p: Phase) {
        self.phase = p;
        self.phase_frames = 0;
    }

    /// The course view, then the layouts over it.
    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        out.push(DrawItem::Course);
        if let Some(o) = &mut self.overlay {
            o.draw(out);
        } else if let Some(s) = &mut self.screens {
            s.draw(out);
        }
    }

    /// The pause menu's items (an SMM1 run: close, Start Over, Save & Quit).
    pub fn pause_items(&self, run: Option<&dyn RunLink>) -> Vec<PauseItem> {
        match run {
            None => vec![PauseItem::Resume, PauseItem::Restart, PauseItem::Exit],
            Some(_) if self.overlay.is_some() => {
                vec![PauseItem::Resume, PauseItem::Restart, PauseItem::SaveQuit]
            }
            Some(r) if r.can_skip() => vec![
                PauseItem::Resume,
                PauseItem::Skip,
                PauseItem::Restart,
                PauseItem::SaveQuit,
            ],
            Some(_) => vec![PauseItem::Resume, PauseItem::Restart, PauseItem::SaveQuit],
        }
    }

    /// One 60 Hz tick.
    pub fn step(&mut self, input: &PlayInput, cx: &mut Cx, run: Run<'_>) -> Option<PlayEvent> {
        let was = self.phase == Phase::Paused;
        let ev = self.step_inner(input, cx, run);
        let now = self.phase == Phase::Paused;
        if was != now {
            cx.host.set_paused(now);
        }
        ev
    }

    fn step_inner(
        &mut self,
        input: &PlayInput,
        cx: &mut Cx,
        mut run: Run<'_>,
    ) -> Option<PlayEvent> {
        let mut ev = None;
        let start_pressed = input.start && !self.held_start;
        self.held_start = input.start;
        let pressed = input.ui.pressed;
        self.phase_frames = self.phase_frames.saturating_add(1);
        if self.overlay.is_some()
            && let Some(e) = self.overlay_tick(input, cx, run.as_deref_mut())
        {
            return Some(e);
        }
        if self.screens.is_some() {
            if let Some(e) = self.smm2_tick(input, pressed, cx, run.as_deref_mut()) {
                return Some(e);
            }
            let status = cx.host.course_status();
            if let Some(s) = &mut self.screens {
                s.update(&status);
            }
            let mut p = pressed;
            if self.screens.as_ref().is_some_and(|s| s.mode != Mode::Play)
                && let Some(sc) = &mut self.script
            {
                p = p | sc.step();
            }
            if let Some(e) = self.smm2_menu(p, input.ui.click, cx, run.as_deref_mut()) {
                return Some(e);
            }
        }
        match self.phase {
            Phase::Playing => {
                if start_pressed {
                    self.pause(run.as_deref());
                    return None;
                }
                cx.host.step_course(None);
                self.engine_tick += 1;
                if let Some(e) = self.observe_tick(cx, run.as_deref_mut()) {
                    ev = Some(e);
                }
                if let Some(t) = self.auto_pause_tick
                    && self.engine_tick >= t
                {
                    self.auto_pause_tick = None;
                    self.pause(run.as_deref());
                }
            }
            _ if self.screens.is_some() => {}
            Phase::Paused => {
                let items = self.pause_items(run.as_deref());
                let n = items.len();
                if pressed.has(Buttons::UP) {
                    self.pause_selection = (self.pause_selection + n - 1) % n;
                }
                if pressed.has(Buttons::DOWN) {
                    self.pause_selection = (self.pause_selection + 1) % n;
                }
                if pressed.has(Buttons::UP) || pressed.has(Buttons::DOWN) {
                    let i = self.pause_selection;
                    if let Some(o) = &mut self.overlay {
                        o.pause_select(i);
                    }
                }
                if pressed.has(Buttons::BACK) {
                    self.resume();
                } else if pressed.has(Buttons::CONFIRM) {
                    let item = items[self.pause_selection.min(n - 1)];
                    return self.choose(item, cx, run);
                }
            }
            Phase::Cleared if self.overlay.is_some() => {
                let o = self.overlay.as_mut().expect("checked");
                if o.clear_ready() {
                    if pressed.has(Buttons::LEFT) {
                        o.clear_select(0);
                    }
                    if pressed.has(Buttons::RIGHT) {
                        o.clear_select(1);
                    }
                    if pressed.has(Buttons::CONFIRM) {
                        if o.clear_focus == 0
                            && let Some(r) = run.as_deref_mut()
                        {
                            r.save_and_quit_after_clear();
                        }
                        o.press_clear();
                        return Some(PlayEvent::Exit);
                    }
                }
            }
            Phase::GameOver if self.overlay.is_some() && run.is_some() => {
                return Some(PlayEvent::Exit);
            }
            Phase::Card | Phase::Skipping | Phase::Intro | Phase::Retire | Phase::Options => {}
            Phase::Cleared | Phase::GameOver => {
                // a short beat so a held jump doesn't skip the screen
                if self.phase_frames > RESULT_GRACE
                    && (pressed.has(Buttons::CONFIRM) || pressed.has(Buttons::BACK))
                {
                    return Some(PlayEvent::Exit);
                }
            }
        }
        ev
    }

    /// One tick of the SMM2 screens: animations, the card's end, pending button actions.
    fn smm2_tick(
        &mut self,
        input: &PlayInput,
        pressed: Buttons,
        cx: &mut Cx,
        mut run: Run<'_>,
    ) -> Option<PlayEvent> {
        let s = self.screens.as_mut()?;
        s.step();
        let tick = s.tick;
        if let Some((at, p)) = self.pending
            && tick >= at
        {
            self.pending = None;
            return self.pending_action(p, cx, run);
        }
        if self.pending.is_some() {
            return None;
        }
        let s = self.screens.as_mut()?;
        match self.phase {
            Phase::Playing => {
                // 100 Mario: hold the skip key through the countdown to skip the course
                if !run.as_deref().is_some_and(|r| r.can_skip()) {
                    return None;
                }
                let held = self.debug_skip_held || input.ui.skip;
                if held {
                    if !s.skipping {
                        s.skip_begin();
                    }
                    if s.skipping && s.tick - s.skip_start >= SKIP_HOLD {
                        let n = s.skip_pass();
                        let t = s.tick;
                        self.set_phase(Phase::Skipping);
                        self.pending = Some((t + n, Pending::SkipDone));
                    }
                } else if s.skipping {
                    s.skip_cancel();
                }
            }
            Phase::Intro => {
                // the card runs its length; confirm skips it once it has been up half a second
                let len = s.intro_length();
                if s.since_mode() >= len || (s.since_mode() > 30 && pressed.has(Buttons::CONFIRM)) {
                    s.end_intro();
                    self.set_phase(Phase::Playing);
                    self.held_start = true;
                }
            }
            Phase::GameOver => {
                let len = s.game_over_length();
                if s.since_mode() >= len
                    || (s.since_mode() > 60
                        && (pressed.has(Buttons::CONFIRM) || pressed.has(Buttons::BACK)))
                {
                    return Some(PlayEvent::Exit);
                }
            }
            _ => {}
        }
        let _ = (&mut run, cx);
        None
    }

    fn pending_action(&mut self, p: Pending, cx: &mut Cx, run: Run<'_>) -> Option<PlayEvent> {
        match p {
            Pending::SkipDone => {
                if let Some(r) = run {
                    r.skipped();
                }
                Some(PlayEvent::Exit)
            }
            Pending::Choose(item) => self.choose(item, cx, run),
            Pending::OptionsClose => {
                if let Some(s) = &mut self.screens {
                    s.hide_options();
                }
                self.set_phase(Phase::Paused);
                None
            }
            Pending::Clear { left } => {
                if let Some(r) = run {
                    // Quit (the clear is saved) / Next
                    if left {
                        r.save_and_quit_after_clear();
                    }
                    Some(PlayEvent::Exit)
                } else if left {
                    self.restart(cx, None);
                    None
                } else {
                    Some(PlayEvent::Exit)
                }
            }
            Pending::Retire { leave } => {
                if leave {
                    Some(PlayEvent::Exit)
                } else {
                    self.keep_playing();
                    None
                }
            }
        }
    }

    /// Menu input on the SMM2 pause / settings / clear / give-up screens.
    fn smm2_menu(
        &mut self,
        mut pressed: Buttons,
        click: Option<[f32; 2]>,
        cx: &mut Cx,
        run: Run<'_>,
    ) -> Option<PlayEvent> {
        if self.pending.is_some() {
            return None;
        }
        let s = self.screens.as_mut()?;
        // a click on a button selects it and decides it
        if let Some(p) = click
            && let Some(i) = s.hit(p)
        {
            match s.mode {
                Mode::Pause => s.pause_select(i, false),
                Mode::Options => s.options_select(i, false),
                Mode::Clear => s.clear_select(i, false),
                Mode::Retire => s.retire_select(i, false),
                _ => {}
            }
            pressed = pressed | Buttons::CONFIRM;
        }
        let t = s.tick;
        match self.phase {
            Phase::Paused => {
                // top to bottom: close, (a server course) the judge row, Start Over, Exit,
                // Settings; left / right between Boo and Like [inf]
                let judge = s.pause_buttons() > 4;
                let order: &[usize] = if judge {
                    &[0, 5, 1, 2, 3]
                } else {
                    &[0, 1, 2, 3]
                };
                let n = order.len();
                let row = if s.pause_selection == 4 {
                    5
                } else {
                    s.pause_selection
                };
                let at = order.iter().position(|&k| k == row).unwrap_or(0);
                if pressed.has(Buttons::UP) {
                    s.pause_select(order[(at + n - 1) % n], true);
                }
                if pressed.has(Buttons::DOWN) {
                    s.pause_select(order[(at + 1) % n], true);
                }
                if judge && s.pause_selection >= 4 {
                    if pressed.has(Buttons::LEFT) {
                        s.pause_select(4, true);
                    }
                    if pressed.has(Buttons::RIGHT) {
                        s.pause_select(5, true);
                    }
                }
                if pressed.has(Buttons::BACK) {
                    self.resume();
                } else if pressed.has(Buttons::CONFIRM) && s.pause_selection >= 4 {
                    let like = s.pause_selection == 5;
                    s.pause_decide_sound(s.pause_selection);
                    if vote(cx, s.info.server_code.as_deref(), like) {
                        s.pause_voted(like);
                    }
                } else if pressed.has(Buttons::CONFIRM) {
                    // a run's exit is Save & Quit
                    let exit = if run.is_none() {
                        PauseItem::Exit
                    } else {
                        PauseItem::SaveQuit
                    };
                    let item = [
                        PauseItem::Resume,
                        PauseItem::Restart,
                        exit,
                        PauseItem::Settings,
                    ][s.pause_selection];
                    let wait = s.pause_decide();
                    self.pending = Some((t + wait.max(1), Pending::Choose(item)));
                }
            }
            Phase::Options => {
                let i = s.option_selection;
                if pressed.has(Buttons::LEFT) {
                    let c = if i == 4 {
                        s.character
                    } else {
                        i.saturating_sub(1)
                    };
                    s.options_select(c, true);
                }
                if pressed.has(Buttons::RIGHT) {
                    let c = if i == 4 { s.character } else { (i + 1).min(3) };
                    s.options_select(c, true);
                }
                if pressed.has(Buttons::DOWN) {
                    s.options_select(4, true);
                }
                if pressed.has(Buttons::UP) && i == 4 {
                    let c = s.character;
                    s.options_select(c, true);
                }
                if pressed.has(Buttons::BACK) {
                    s.hide_options();
                    self.set_phase(Phase::Paused);
                } else if pressed.has(Buttons::CONFIRM) {
                    let before = s.character;
                    let (wait, close) = s.options_decide();
                    if s.character != before {
                        self.character = s.character;
                        cx.host
                            .store_value("player.character", &s.character.to_string());
                        cx.host.set_character(s.character as u32);
                    }
                    if close {
                        self.pending = Some((t + wait, Pending::OptionsClose));
                    }
                }
            }
            Phase::Cleared => {
                if !s.clear_ready() {
                    return None;
                }
                if pressed.has(Buttons::LEFT) {
                    let n = s.clear_neighbour(s.clear_selection, false);
                    s.clear_select(n, true);
                }
                if pressed.has(Buttons::RIGHT) {
                    let n = s.clear_neighbour(s.clear_selection, true);
                    s.clear_select(n, true);
                }
                if pressed.has(Buttons::CONFIRM) {
                    if s.clear_selection >= 2 {
                        // Like / Boo a server course (an account's vote replaces its last one)
                        let like = s.clear_selection == 3;
                        s.ui_decide_sound(s.clear_selection);
                        if vote(cx, s.info.server_code.as_deref(), like) {
                            s.clear_voted(like);
                        }
                        return None;
                    }
                    let left = s.clear_selection == 0;
                    let wait = s.clear_decide();
                    self.pending = Some((t + wait.max(1), Pending::Clear { left }));
                }
            }
            Phase::Retire => {
                if !s.retire_ready() {
                    return None;
                }
                if pressed.has(Buttons::LEFT) {
                    s.retire_select(0, true);
                }
                if pressed.has(Buttons::RIGHT) {
                    s.retire_select(1, true);
                }
                if pressed.has(Buttons::BACK) {
                    self.keep_playing();
                } else if pressed.has(Buttons::CONFIRM) {
                    let leave = s.retire_selection == 0;
                    let wait = s.retire_decide();
                    self.pending = Some((t + wait.max(1), Pending::Retire { leave }));
                }
            }
            _ => {}
        }
        None
    }

    /// "Keep playing" on the give-up screen: back into the course.
    fn keep_playing(&mut self) {
        if let Some(s) = &mut self.screens {
            s.hide_retire();
        }
        self.set_phase(Phase::Playing);
        self.held_start = true;
    }

    fn new_attempt(&mut self, cx: &mut Cx) {
        self.attempt_ticks = 0;
        self.goal_ticks = None;
        if let Some(k) = &self.course_key {
            cx.saves.add_attempt(cx.host, k);
        }
    }

    /// After each engine tick: goal time, extra lives, deaths, the clear.
    fn observe_tick(&mut self, cx: &mut Cx, mut run: Run<'_>) -> Option<PlayEvent> {
        self.attempt_ticks += 1;
        let h = cx.host.course_status();
        if h.goal_event && self.goal_ticks.is_none() {
            self.goal_ticks = Some(self.attempt_ticks);
        }
        if h.lives_gained > self.lives_gained
            && let Some(r) = run.as_deref_mut()
        {
            r.extra_lives(h.lives_gained - self.lives_gained);
        }
        self.lives_gained = h.lives_gained;
        let dying = h.dying;
        if self.screens.is_some() {
            return self.observe_smm2(&h, dying, cx, run);
        }
        if self.was_dying && !dying {
            if let Some(r) = run.as_deref_mut() {
                // the engine restarted the course; the run decides whether it continues
                if r.died(cx) {
                    self.new_attempt(cx);
                    self.show_retry_card(run.as_deref());
                } else {
                    self.set_phase(Phase::GameOver);
                }
            } else if h.lives == 0 && !self.unlimited_lives {
                self.set_phase(Phase::GameOver);
            } else {
                self.new_attempt(cx);
            }
        }
        self.was_dying = dying;
        if h.cleared {
            let ticks = self.goal_ticks.unwrap_or(self.attempt_ticks);
            self.record_clear(cx, ticks);
            if let Some(r) = run.as_deref_mut() {
                r.cleared(h.score, h.coins);
            }
            self.set_phase(Phase::Cleared);
            self.show_clear_overlay(run.as_deref());
            return Some(PlayEvent::Cleared);
        }
        None
    }

    fn record_clear(&mut self, cx: &mut Cx, ticks: i64) {
        self.clear_ticks = Some(ticks);
        if let Some(k) = &self.course_key {
            self.new_best = cx.saves.add_clear(cx.host, k, ticks);
        }
        cx.host.cleared(ticks);
    }

    fn observe_smm2(
        &mut self,
        h: &CourseStatus,
        dying: bool,
        cx: &mut Cx,
        mut run: Run<'_>,
    ) -> Option<PlayEvent> {
        let s = self.screens.as_mut()?;
        if dying && h.time_up && !self.time_up_shown {
            self.time_up_shown = true;
            s.show_time_up();
        }
        let center = CourseScreens::layout_point(h.player_x, h.player_y + 12.0);
        if self.unlimited_lives && dying {
            // the death iris: the circle closes on where the player died, ending as the course
            // restarts (the death sequence is 45 + 140 frames) [inf]
            if self.death_center.is_none() {
                self.death_center = Some(center);
            }
            if !self.iris_shown
                && (0..=60).contains(&h.restart_in)
                && let Some(c) = self.death_center
            {
                self.iris_shown = true;
                s.iris_out(c);
            }
        }
        if self.was_dying && !dying && self.unlimited_lives {
            // restarted (at the checkpoint when one was reached): no lives, no card; the circle
            // opens on the player
            self.time_up_shown = false;
            self.was_dying = dying;
            self.death_center = None;
            self.iris_shown = false;
            self.new_attempt(cx);
            if let Some(s) = &mut self.screens {
                s.iris_in(center);
            }
            return None;
        }
        if self.was_dying
            && !dying
            && let Some(r) = run.as_deref_mut()
        {
            // a run's death: the run decides (a life spent → the card again; none → game over)
            self.time_up_shown = false;
            self.was_dying = dying;
            if r.died(cx) {
                self.new_attempt(cx);
                let st = cx.host.course_status();
                let s = self.screens.as_mut()?;
                s.update(&st);
                s.show_intro(st.lives);
                self.set_phase(Phase::Intro);
            } else {
                self.screens.as_mut()?.show_game_over();
                self.set_phase(Phase::GameOver);
            }
            return None;
        }
        if self.was_dying && !dying {
            // the engine restarted the course after the death sequence
            self.time_up_shown = false;
            self.was_dying = dying;
            if h.lives == 0 {
                self.screens.as_mut()?.show_game_over();
                self.set_phase(Phase::GameOver);
            } else {
                self.new_attempt(cx);
                self.screens.as_mut()?.show_intro(h.lives);
                self.set_phase(Phase::Intro);
            }
            return None;
        }
        self.was_dying = dying;
        if h.cleared {
            let ticks = self.goal_ticks.unwrap_or(self.attempt_ticks);
            self.record_clear(cx, ticks);
            if let Some(r) = run.as_deref_mut() {
                r.cleared(h.score, h.coins);
            }
            self.set_phase(Phase::Cleared);
            let in_run = run.is_some();
            self.screens.as_mut()?.show_clear(Some(ticks), in_run);
            return Some(PlayEvent::Cleared);
        }
        None
    }

    fn show_clear_overlay(&mut self, run: Option<&dyn RunLink>) {
        let (Some(o), Some(r)) = (&mut self.overlay, run) else {
            return;
        };
        let name = r.course_name();
        o.show_clear(
            if name.is_empty() { &self.title } else { &name },
            &r.course_maker(),
            self.clear_ticks,
            r.world_record_ms(),
            r.clear_ends_run(),
        );
    }

    /// In a run with the SMM1 layouts, the intro card (lives after the death) before the retry.
    fn show_retry_card(&mut self, run: Option<&dyn RunLink>) {
        let (Some(o), Some(r)) = (&mut self.overlay, run) else {
            return;
        };
        o.show_card(&r.card_info());
        self.set_phase(Phase::Card);
    }

    /// One tick of the SMM1 overlay: animations, the skip hold, the end of the card / Pass.
    fn overlay_tick(&mut self, input: &PlayInput, cx: &mut Cx, run: Run<'_>) -> Option<PlayEvent> {
        let o = self.overlay.as_mut()?;
        o.step();
        match self.phase {
            Phase::Playing => {
                let r = run?;
                if !r.can_skip() {
                    return None;
                }
                let held = self.debug_skip_held || input.ui.skip;
                if held {
                    if o.mode == RunOverlayMode::None {
                        o.skip_begin();
                    }
                    if o.mode == RunOverlayMode::Skip && o.since_mode() >= crate::smm1::SKIP_HOLD {
                        self.skip_done_tick = Some(o.tick + o.skip_pass());
                        self.set_phase(Phase::Skipping);
                    }
                } else if o.mode == RunOverlayMode::Skip {
                    o.skip_cancel();
                }
            }
            Phase::Skipping => {
                if let Some(t) = self.skip_done_tick
                    && o.tick >= t
                {
                    self.skip_done_tick = None;
                    if let Some(r) = run {
                        r.skipped();
                    }
                    return Some(PlayEvent::Exit);
                }
            }
            Phase::Card if o.card_done() => {
                o.hide_card();
                self.set_phase(Phase::Playing);
                self.held_start = true;
            }
            _ => {}
        }
        let _ = cx;
        None
    }

    pub fn pause(&mut self, run: Option<&dyn RunLink>) {
        if self.phase != Phase::Playing {
            return;
        }
        self.pause_selection = 0;
        if let Some(s) = &mut self.screens {
            s.show_pause();
        }
        if let (Some(o), Some(r)) = (&mut self.overlay, run) {
            if o.mode == RunOverlayMode::Skip {
                o.skip_cancel();
            }
            let name = r.course_name();
            o.show_pause(
                &r.pause_line(),
                if name.is_empty() { &self.title } else { &name },
                &r.course_maker(),
                0,
            );
        }
        self.set_phase(Phase::Paused);
    }

    pub fn resume(&mut self) {
        if self.phase != Phase::Paused {
            return;
        }
        if let Some(o) = &mut self.overlay {
            o.hide_pause();
        }
        if let Some(s) = &mut self.screens {
            s.hide_pause();
        }
        self.set_phase(Phase::Playing);
        self.held_start = true;
    }

    pub fn restart(&mut self, cx: &mut Cx, mut run: Run<'_>) {
        // in a run, starting over counts as a death
        if let Some(o) = &mut self.overlay {
            o.clear_all();
        }
        if let Some(r) = run.as_deref_mut()
            && !r.died(cx)
        {
            if let Some(s) = &mut self.screens {
                s.clear_all();
                s.show_game_over();
            }
            self.set_phase(Phase::GameOver);
            return;
        }
        cx.host.restart_course();
        if let Some(r) = run.as_deref_mut() {
            r.apply_counters(cx);
        }
        self.lives_gained = 0;
        self.was_dying = false;
        self.time_up_shown = false;
        self.new_attempt(cx);
        self.set_phase(Phase::Playing);
        self.held_start = true;
        self.show_retry_card(run.as_deref());
        if let Some(s) = &mut self.screens {
            // Start Over: the course card again
            let st = cx.host.course_status();
            s.clear_all();
            s.update(&st);
            s.show_intro(st.lives);
            self.set_phase(Phase::Intro);
        }
    }

    fn choose(&mut self, item: PauseItem, cx: &mut Cx, run: Run<'_>) -> Option<PlayEvent> {
        match item {
            PauseItem::Resume => self.resume(),
            PauseItem::Restart => self.restart(cx, run),
            PauseItem::Exit => {
                let Some(s) = &mut self.screens else {
                    return Some(PlayEvent::Exit);
                };
                // Exit Course: "You gave up..." with Exit / Keep playing first
                s.hide_pause();
                s.show_retire();
                self.set_phase(Phase::Retire);
            }
            PauseItem::Skip => {
                if let Some(r) = run {
                    r.skipped();
                }
                return Some(PlayEvent::Exit);
            }
            PauseItem::SaveQuit => {
                if let Some(r) = run {
                    r.save_and_quit();
                }
                return Some(PlayEvent::Exit);
            }
            PauseItem::Settings => {
                let c = self.character;
                if let Some(s) = &mut self.screens {
                    s.show_options(c);
                }
                self.set_phase(Phase::Options);
            }
        }
        None
    }

    // MARK: debug captures

    /// Show a screen without playing there (the Swift app's `debugSMM2` / `debugClear` /
    /// `debugCard`): course-clear, course-gameover, course-timeup, course-retire,
    /// course-settings, course-death, course-pause.
    pub fn debug(&mut self, screen: &str, cx: &mut Cx, run: Run<'_>) {
        let lives = cx.host.course_status().lives;
        match screen {
            "course-clear" | "clear" => {
                self.clear_ticks = Some(2345);
                self.set_phase(Phase::Cleared);
                let in_run = run.is_some();
                if let Some(s) = &mut self.screens {
                    s.show_clear(Some(2345), in_run);
                } else {
                    self.show_clear_overlay(run.as_deref());
                }
            }
            "course-gameover" => {
                if let Some(s) = &mut self.screens {
                    s.show_game_over();
                }
                self.set_phase(Phase::GameOver);
            }
            "course-timeup" => {
                if let Some(s) = &mut self.screens {
                    s.show_time_up();
                }
            }
            "course-retire" => {
                self.pause(run.as_deref());
                self.choose(PauseItem::Exit, cx, run);
            }
            "course-settings" => {
                self.pause(run.as_deref());
                self.choose(PauseItem::Settings, cx, run);
            }
            "course-pause" | "pause" => self.pause(run.as_deref()),
            "course-death" | "card" => {
                if let Some(s) = &mut self.screens {
                    s.show_intro(lives.saturating_sub(1));
                    self.set_phase(Phase::Intro);
                } else {
                    self.show_retry_card(run.as_deref());
                }
            }
            "skip" => self.debug_skip_held = true,
            _ => {}
        }
    }

    /// The screens' own tick (captures count from when a screen appeared).
    pub fn since_mode(&self) -> i32 {
        if let Some(s) = &self.screens {
            return s.since_mode();
        }
        self.overlay.as_ref().map_or(0, |o| o.since_mode())
    }

    /// A menu screen is up (not plain play).
    pub fn menu_up(&self) -> bool {
        if let Some(s) = &self.screens {
            return s.mode != Mode::Play;
        }
        self.overlay
            .as_ref()
            .is_some_and(|o| o.mode != RunOverlayMode::None)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::host::{CourseEntry, Host, LoadedCourse, Saves, Source, WorldCourse};

    /// An engine stand-in: a status the test sets, counted steps.
    #[derive(Default)]
    pub struct MockHost {
        pub status: CourseStatus,
        pub steps: u32,
        pub restarts: u32,
        pub values: HashMap<String, String>,
        pub paused: bool,
        /// A server is configured (`online` accepts calls).
        pub server: bool,
        /// Calls started: (id, action, args, token).
        pub calls: Vec<(u32, String, serde_json::Value, Option<String>)>,
        /// Answers `poll_online` hands out, first first.
        pub answers: Vec<(u32, Result<serde_json::Value, String>)>,
        pub affects_play: bool,
        pub loaded: Vec<Source>,
        pub reports: Vec<(String, String)>,
        pub library: Vec<CourseEntry>,
    }

    impl Host for MockHost {
        fn library(&mut self) -> Vec<CourseEntry> {
            self.library.clone()
        }
        fn request_server_courses(&mut self) -> bool {
            false
        }
        fn poll_server_courses(&mut self) -> Option<Vec<WorldCourse>> {
            None
        }
        fn load_course(&mut self, s: &Source) -> Result<LoadedCourse, String> {
            self.loaded.push(s.clone());
            Ok(LoadedCourse::default())
        }
        fn title_courses(&mut self) -> usize {
            0
        }
        fn load_title_course(&mut self, _: usize) -> bool {
            false
        }
        fn unload_course(&mut self) {}
        fn step_course(&mut self, _: Option<u32>) {
            self.steps += 1;
        }
        fn course_status(&self) -> CourseStatus {
            self.status
        }
        fn restart_course(&mut self) {
            self.restarts += 1;
        }
        fn set_character(&mut self, _: u32) {}
        fn set_run_counters(&mut self, _: u32, _: u32, _: u32) {}
        fn set_paused(&mut self, p: bool) {
            self.paused = p;
        }
        fn report(&mut self, code: &str, what: &str) {
            self.reports.push((code.into(), what.into()));
        }
        fn online(
            &mut self,
            id: u32,
            action: &str,
            args: &serde_json::Value,
            token: Option<&str>,
        ) -> bool {
            if self.server {
                self.calls
                    .push((id, action.into(), args.clone(), token.map(str::to_string)));
            }
            self.server
        }
        fn poll_online(&mut self) -> Option<(u32, Result<serde_json::Value, String>)> {
            (!self.answers.is_empty()).then(|| self.answers.remove(0))
        }
        fn play_options_affect_play(&self) -> bool {
            self.affects_play
        }
        fn load_value(&mut self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
        fn store_value(&mut self, key: &str, value: &str) {
            self.values.insert(key.into(), value.into());
        }
        fn random(&mut self) -> u64 {
            42
        }
    }

    fn input(start: bool, pressed: Buttons) -> PlayInput {
        PlayInput {
            ui: UiInput {
                pressed,
                ..Default::default()
            },
            start,
        }
    }

    /// Run `f` with a context over `host` and `saves`.
    pub fn with<R>(host: &mut MockHost, saves: &mut Saves, f: impl FnOnce(&mut Cx) -> R) -> R {
        let mut online = crate::online::Online::default();
        with_online(host, saves, &mut online, f)
    }

    /// Run `f` with a context over `host`, `saves` and the account state `online`.
    pub fn with_online<R>(
        host: &mut MockHost,
        saves: &mut Saves,
        online: &mut crate::online::Online,
        f: impl FnOnce(&mut Cx) -> R,
    ) -> R {
        let mut fade = crate::fade::SceneFade::new(None);
        let (mut pending, mut now, mut toast) = (None, Vec::new(), None);
        let mut cx = Cx {
            host,
            saves,
            online,
            fade: &mut fade,
            pending: &mut pending,
            now: &mut now,
            toast: &mut toast,
        };
        f(&mut cx)
    }

    #[test]
    fn plain_play_steps_pauses_and_records_the_clear() {
        let mut host = MockHost::default();
        let mut saves = Saves::default();
        let mut p = with(&mut host, &mut saves, |cx| {
            let info = CardInfo::default();
            let key = Some("k".to_string());
            CoursePlay::new(cx, "C".into(), key, None, None, info, false, true, false)
        });
        assert_eq!(saves.record("k").attempts, 1);
        let mut step = |host: &mut MockHost, saves: &mut Saves, start: bool, b: Buttons| {
            with(host, saves, |cx| p.step(&input(start, b), cx, None))
        };
        // the start button held from before must be released first
        step(&mut host, &mut saves, true, Buttons::NONE);
        step(&mut host, &mut saves, false, Buttons::NONE);
        assert_eq!(host.steps, 2);
        step(&mut host, &mut saves, true, Buttons::NONE);
        assert!(host.paused);
        step(&mut host, &mut saves, false, Buttons::NONE);
        assert_eq!(host.steps, 2, "no engine steps while paused");
        step(&mut host, &mut saves, false, Buttons::BACK);
        assert!(!host.paused);
        step(&mut host, &mut saves, false, Buttons::NONE);
        assert_eq!(host.steps, 3);
        // a death restarts with a new attempt (no lives); a clear records it
        host.status.dying = true;
        step(&mut host, &mut saves, false, Buttons::NONE);
        host.status.dying = false;
        host.status.cleared = true;
        let ev = step(&mut host, &mut saves, false, Buttons::NONE);
        assert_eq!(ev, Some(PlayEvent::Cleared));
        let r = saves.record("k");
        assert_eq!((r.attempts, r.clears, r.cleared), (2, 1, true));
        // the result screen waits half a second before confirm leaves
        host.status.cleared = false;
        for _ in 0..31 {
            step(&mut host, &mut saves, false, Buttons::NONE);
        }
        let ev = step(&mut host, &mut saves, false, Buttons::CONFIRM);
        assert_eq!(ev, Some(PlayEvent::Exit));
    }

    #[test]
    fn boot_marks_the_update_openings_seen_newest_first() {
        let mut host = MockHost::default();
        assert_eq!(crate::boot::choose(&mut host), 6);
        assert_eq!(crate::boot::choose(&mut host), 0); // 42 % 100 < 66: InDefault
        let mut fresh = MockHost::default();
        fresh.values.insert("boot.openingsSeen".into(), "4".into());
        assert_eq!(crate::boot::choose(&mut fresh), 5);
        assert_eq!(crate::boot::choose(&mut fresh), 0);
        fresh.values.insert("boot.openingsSeen".into(), "6".into());
        assert_eq!(crate::boot::choose(&mut fresh), 4);
    }

    #[test]
    fn front_without_layouts_still_ticks() {
        let ui = crate::Ui::new(crate::UiKind::Smm2, mmk_ui::UiCache::in_memory(), None);
        let mut host = MockHost::default();
        let mut f = crate::Front::new(ui, None, crate::Options::default(), &mut host);
        f.tick(&crate::FrontInput::default(), &mut host);
        assert!(f.frame().items.is_empty());
        assert!(!f.capture(&mut host, "nope", 0, 0));
    }
}
