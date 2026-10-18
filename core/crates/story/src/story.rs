//! Story Mode: the game's own flows run against our Story state.
//!
//! [`Story`] owns the save state and runs one flow at a time (a talk to a hub NPC, the scene
//! entry after a job, the opening). Each step hands the screen a [`Request`]: show a message,
//! open the job list or the build menu, show a reward window, enter a course, save, or a
//! presentation call (camera, animation, fade, wait, demo) the screen may play or skip. The
//! screen handles it and calls [`Story::step`] again; a yes/no question is answered with
//! [`Story::answer`] first, a list with [`Story::order_job`] / [`Story::order_build`].
//!
//! The rules behind the flows' queries and actions (`QuestSystemActor`) follow story-mode.md
//! §2–§6 and its binary notes:
//! - a job's **first** clear pays its coin reward; every clear adds the coins collected in the
//!   course and passes its days (1 per job); a failure pays nothing, loses them, passes no day;
//! - a build is paid in full when ordered and counts down by the days passed, all builds in
//!   parallel; one reaching 0 is finished and queued for its pop-up (21 = none queued);
//! - which jobs, builds, crises and the ending happen when is entirely in the flows.
//!
//! In a course ([`JobPlay`]): 5 lives; each retry costs one; out of lives → TOO BAD with Try
//! Again / Call Luigi / Give Up; a Luigi clear pays normally but marks the job until the player
//! clears it; after more than two attempts the job switches to the "struggling" play type for
//! good.

use std::collections::HashSet;
use std::sync::Arc;

use mmk_formats::bfevfl::Params;

use crate::data::StoryData;
use crate::flow::{Call, FlowError, Host, Runner, Status, Step};
use crate::state::{BUILDS, FlagKind, JOBS, MAX_COUNT, StoryState};

/// Lives at the start of a job (story-mode.md §3.1).
pub const JOB_LIVES: u8 = 5;
/// `checkBuildEndTrigger` / `checkBuildEndTriggerForBuildChief` answer when nothing is queued.
pub const NO_BUILD: i32 = 21;
/// The Stained Glass, whose completion is the ending.
pub const STAINED_GLASS: usize = 15;
/// Hub coin values by unit (`Coin`, `Coin10`, `Coin30`, `Coin50`).
pub const HUB_COINS: [(&str, i32); 4] =
    [("Coin", 1), ("Coin10", 10), ("Coin30", 30), ("Coin50", 50)];

/// What the screen must do before the flow goes on.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// A line of dialogue (`messageid` = message file path `:` label). When the next step is a
    /// question, answer with [`Story::answer`] (0 = first choice).
    Message {
        /// The speaker: the flow's NPC, or the named one for `showMessageWithNPCName`.
        speaker: String,
        message: String,
        /// A proximity line (balloon) rather than a talk window.
        balloon: bool,
    },
    /// The Taskmaster's job list (`only`: a giver's single job). Answer with
    /// [`Story::order_job`] (None = closed without taking a job).
    JobList { only: Option<u8> },
    /// The Chief's build menu, offering `options` (enabled and not started). Answer with
    /// [`Story::order_build`].
    BuildMenu { options: Vec<u8> },
    /// The reward window after a job (coins collected, payment) or a gear / all-clear reward.
    Reward {
        collected: i32,
        payment: i32,
        gear: Option<(String, i32)>,
        message: Option<String>,
    },
    /// Go into the ordered job's course; report the outcome with [`Story::finish_job`].
    EnterCourse { job: u8 },
    /// The staff roll (from the ending, or replayed from the Coursebot NPC).
    StaffRoll { from_robot: bool },
    /// Save [`Story::state`] now (`startAutoSave`).
    Save,
    /// Anything else: camera, animation, effects, fades, waits, demos, progress pop-ups.
    Present(Call),
}

/// Session values the game keeps in memory only.
#[derive(Clone, Debug, Default)]
struct Session {
    /// Build chosen in the menu (cleared by `startBuilding`).
    selected_build: Option<u8>,
    /// The build just started (the Chief's "construction has started" line).
    build_start: Option<u8>,
    /// Builds finished by the last day advance, not yet shown.
    ended_now: u32,
    /// The last job result: days to pass, coins collected, payment.
    result_days: i32,
    result_collected: i32,
    result_payment: i32,
    /// The ordered job had not been cleared before this clear.
    first_clear: bool,
    /// Coins collected in the course just played.
    collected: i32,
    /// Answer to the last question.
    choice: i32,
    /// NPC flows talked to since the hub was entered / since the game started.
    talked_visit: HashSet<String>,
    talked_power_on: HashSet<String>,
    /// The staff roll was started from the Coursebot NPC.
    staff_roll_from_robot: bool,
    rng: u64,
}

impl Session {
    fn random(&mut self, n: i32) -> i32 {
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        if n <= 1 { 0 } else { (x % n as u64) as i32 }
    }
}

/// Outcome of a job's course.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    /// Reached the goal (`coins` = the course's coin count at the goal; `luigi` = Luigi played).
    Clear { coins: i32, luigi: bool },
    /// Gave up (TOO BAD → Give Up, or quit from the pause menu).
    Fail,
}

/// The TOO BAD window's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooBadChoice {
    TryAgain,
    CallLuigi,
    GiveUp,
}

/// A job being played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobPlay {
    pub job: u8,
    pub lives: u8,
    /// Attempts at the course in this visit (1 = the first).
    pub attempts: u32,
    /// Play type "struggling" (assist parts), for good once set.
    pub struggling: bool,
    /// Out of lives: the TOO BAD window is up.
    pub too_bad: bool,
    /// Luigi is playing the course.
    pub luigi: bool,
}

impl JobPlay {
    /// A miss (fall, damage, time up) or Start Over: one life (story-mode.md §3.1). Returns
    /// true when that was the last one (TOO BAD).
    pub fn miss(&mut self) -> bool {
        self.lives = self.lives.max(1) - 1;
        if self.lives == 0 {
            self.too_bad = true;
        } else {
            self.retry();
        }
        self.too_bad
    }

    fn retry(&mut self) {
        self.attempts += 1;
        // more than two attempts: the course is rebuilt in the struggling play type
        if self.attempts > 2 {
            self.struggling = true;
        }
    }

    /// The TOO BAD window's answer. Try Again restarts with full lives [inf]; Call Luigi lets
    /// Luigi play (report his clear as `JobOutcome::Clear { luigi: true, .. }`); Give Up is a
    /// failure (report `JobOutcome::Fail`).
    pub fn too_bad(&mut self, choice: TooBadChoice) {
        self.too_bad = false;
        match choice {
            TooBadChoice::TryAgain => {
                self.lives = JOB_LIVES;
                self.retry();
            }
            TooBadChoice::CallLuigi => self.luigi = true,
            TooBadChoice::GiveUp => {}
        }
    }
}

/// A hub object that is present, with what talking to it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct HubThing {
    pub index: usize,
    pub unit: String,
    pub flow: String,
    pub entry: String,
    pub approach: String,
    pub pos: [f32; 3],
}

pub struct Story {
    pub data: Arc<StoryData>,
    pub state: StoryState,
    session: Session,
    runner: Option<Runner>,
    /// NPC (flowchart name) of the flow being run, for first-talk tracking.
    talking: Option<String>,
    pending: Option<Request>,
    play: Option<JobPlay>,
}

/// The flow host: the Story rules over the state, one step at a time.
struct Rules<'a> {
    data: &'a StoryData,
    state: &'a mut StoryState,
    session: &'a mut Session,
    request: &'a mut Option<Request>,
    play: &'a mut Option<JobPlay>,
    talking: Option<&'a str>,
}

fn job_index(call: &Call, key: &str) -> i32 {
    call.int(key).unwrap_or(-1)
}

fn b(v: bool) -> i32 {
    v as i32
}

impl Rules<'_> {
    fn job_flag(&self, q: i32, f: impl Fn(&crate::state::JobState) -> bool) -> i32 {
        b(self.state.job(q).is_some_and(f))
    }
    fn build_flag(&self, n: i32, f: impl Fn(&crate::state::BuildState) -> bool) -> i32 {
        b(self.state.build(n).is_some_and(f))
    }

    /// Builds the Chief could still start (enabled, not started).
    fn build_options(&self) -> Vec<u8> {
        (0..BUILDS)
            .filter(|&i| {
                let s = &self.state.builds[i];
                s.enabled && !s.started()
            })
            .map(|i| i as u8)
            .collect()
    }

    fn build_cost(&self, n: Option<u8>) -> i32 {
        n.and_then(|n| self.data.build(n as i32))
            .map_or(1, |b| b.cost)
    }

    /// Pass `d` days: the day count and every running build (story-mode.md §3.3).
    fn advance_days(&mut self, d: i32) {
        self.state.days = self.state.days.saturating_add(d).clamp(0, MAX_COUNT);
        for (i, s) in self.state.builds.iter_mut().enumerate() {
            if s.building && !s.finished {
                s.days_left = (s.days_left - d).max(0);
                if s.days_left == 0 {
                    s.building = false;
                    s.finished = true;
                    self.session.ended_now |= 1 << i;
                }
            }
        }
    }

    fn refresh_new_jobs(&mut self) {
        if !self
            .state
            .jobs
            .iter()
            .any(|j| j.activated && !j.displayed && !j.cleared)
        {
            self.state.new_jobs = false;
        }
    }

    fn give_gear(&mut self, call: &Call, kind: i32) {
        if (0..32).contains(&kind) {
            self.state.gear |= 1 << kind;
        }
        let name = call.str("cloth_name").unwrap_or_default().to_string();
        *self.request = Some(Request::Reward {
            collected: 0,
            payment: 0,
            gear: Some((name, kind)),
            message: call.str("messageid").map(str::to_string),
        });
    }

    fn lowest_unannounced_finished(&self) -> i32 {
        (0..BUILDS)
            .find(|&i| self.state.builds[i].finished && !self.state.builds[i].talked)
            .map_or(NO_BUILD, |i| i as i32)
    }
}

impl Host for Rules<'_> {
    fn action(&mut self, call: &Call) -> Step {
        let s = &mut *self.state;
        match (call.actor.as_str(), call.name.as_str()) {
            (_, "activateQuest") => {
                if let Some(j) = s.job_mut(job_index(call, "quest_index"))
                    && !j.activated
                {
                    j.activated = true;
                    if !j.displayed {
                        s.new_jobs = true;
                    }
                }
            }
            (_, "setDisplayFlagActivateQuest") => {
                if let Some(j) = s.job_mut(job_index(call, "quest_index")) {
                    j.displayed = true;
                }
                self.refresh_new_jobs();
            }
            (_, "setDisplayFlagAllActivateQuest") => {
                for j in s.jobs.iter_mut().filter(|j| j.activated) {
                    j.displayed = true;
                }
                self.refresh_new_jobs();
            }
            (_, "onTalkedAboutNewQuest") => s.new_jobs = false,
            (_, "openQuestUI") => {
                *self.request = Some(Request::JobList { only: None });
                return Step::Wait;
            }
            (_, "openSecretQuestUI") => {
                let q = job_index(call, "quest_idx");
                *self.request = Some(Request::JobList {
                    only: u8::try_from(q).ok().filter(|&q| (q as usize) < JOBS),
                });
                return Step::Wait;
            }
            (_, "sceneChangeToCourse") => {
                if let Some(q) = s.ordered {
                    let struggling = s.jobs[q as usize].struggling;
                    *self.play = Some(JobPlay {
                        job: q,
                        lives: JOB_LIVES,
                        attempts: 1,
                        struggling,
                        too_bad: false,
                        luigi: false,
                    });
                    *self.request = Some(Request::EnterCourse { job: q });
                    return Step::Wait;
                }
            }
            (_, "cancelOrderedQuest") => {
                if s.ordered.is_some() {
                    s.last_ordered = s.ordered.take();
                }
            }
            (_, "countUpOrderedQuest") => {
                if let Some(q) = s.ordered {
                    let j = &mut s.jobs[q as usize];
                    j.plays = j.plays.saturating_add(1);
                }
            }
            (_, "onOrderedQuestClear") => {
                if let Some(q) = s.ordered {
                    let j = &mut s.jobs[q as usize];
                    self.session.first_clear = !j.cleared;
                    j.cleared = true;
                    j.played = false;
                }
            }
            (_, "calcPlayCourseInfo") => {
                let job = s.ordered.and_then(|q| self.data.job(q as i32));
                if call.bool("is_clear").unwrap_or(false) {
                    self.session.result_days = job.map_or(100, |j| j.days);
                    self.session.result_collected = self.session.collected;
                    self.session.result_payment = if self.session.first_clear {
                        job.map_or(0, |j| j.reward.coins())
                    } else {
                        0
                    };
                } else {
                    self.session.result_days = 0;
                    self.session.result_collected = 0;
                    self.session.result_payment = 0;
                    if let Some(q) = s.ordered
                        && !s.jobs[q as usize].cleared
                    {
                        s.jobs[q as usize].played = true;
                    }
                }
            }
            (_, "addQuestCourseClearBuildCoin") => {
                let (d, c, p) = (
                    self.session.result_days,
                    self.session.result_collected,
                    self.session.result_payment,
                );
                self.advance_days(d);
                self.state.add_coins(c + p);
                *self.request = Some(Request::Reward {
                    collected: c,
                    payment: p,
                    gear: None,
                    message: None,
                });
                return Step::Wait;
            }
            (_, "getQuestMiiClothes") => {
                let kind = call.int("cloth_type").unwrap_or(-1);
                let (d, c) = (self.session.result_days, self.session.result_collected);
                self.advance_days(d);
                self.state.add_coins(c);
                self.give_gear(call, kind);
                if let Some(Request::Reward { collected, .. }) = self.request.as_mut() {
                    *collected = c;
                }
                return Step::Wait;
            }
            (_, "getQuestMiiClothesWithOutUI") => {
                if let Some(k) = call.int("cloth_type").filter(|k| (0..32).contains(k)) {
                    s.gear |= 1 << k;
                }
            }
            (_, "getQuestSuperBallMiiClothes") => {
                self.give_gear(call, 10);
                return Step::Wait;
            }
            (_, "getQuestRewardAtQuestAllClear") => {
                self.give_gear(call, 9);
                return Step::Wait;
            }
            (_, "showGetRewardWindow") => {
                *self.request = Some(Request::Reward {
                    collected: 0,
                    payment: 0,
                    gear: None,
                    message: call.str("messageid").map(str::to_string),
                });
                return Step::Wait;
            }
            (_, "onEnableBuild") => {
                if let Some(x) = s.build_mut(call.int("build_index").unwrap_or(-1)) {
                    x.enabled = true;
                }
            }
            (_, "onModelDisplay") => {
                if let Some(x) = s.build_mut(call.int("build_index").unwrap_or(-1)) {
                    x.model_shown = true;
                }
            }
            (_, "openBuildUI") => {
                for x in s.builds.iter_mut().filter(|x| x.enabled) {
                    x.displayed = true;
                }
                *self.request = Some(Request::Present(call.clone()));
                return Step::Wait;
            }
            (_, "openBuildOrderButton") => {
                self.session.selected_build = None;
                *self.request = Some(Request::BuildMenu {
                    options: self.build_options(),
                });
                return Step::Wait;
            }
            (_, "subQuestBuildCoinStartBuilding") => {
                let cost = self.build_cost(self.session.selected_build);
                self.state.add_coins(-cost);
            }
            (_, "startBuilding") => {
                if let Some(n) = self.session.selected_build.take() {
                    let days = self.data.build(n as i32).map_or(1, |b| b.days);
                    let x = &mut s.builds[n as usize];
                    x.building = true;
                    x.days_left = days;
                    self.session.build_start = Some(n);
                }
            }
            (_, "subQuestBuildCoin") => s.add_coins(-call.int("coin_num").unwrap_or(0)),
            (_, "addQuestBuildCoin") => s.add_coins(call.int("coin_num").unwrap_or(0)),
            (_, "shiftLastBuildingTriiger") => {
                let m = self.session.ended_now;
                self.session.ended_now = m & m.wrapping_sub(1);
            }
            (_, "onTalkedBuildChiefTriiger") => {
                for x in s.builds.iter_mut().filter(|x| x.finished) {
                    x.talked = true;
                }
            }
            (_, "onGeneralTalkFlag") => {
                s.set_talk_flag(call.int("npc").unwrap_or(-1), call.int("no").unwrap_or(-1));
            }
            (_, "onAppearFlag" | "onMoveFlag" | "onOneDayFlag") => {
                if let Some(f) = call.str("flag") {
                    s.set_flag(f, true);
                }
            }
            (_, "offAppearFlag") => {
                if let Some(f) = call.str("flag") {
                    s.set_flag(f, false);
                }
            }
            (_, "onPlayedOPDemo") => s.opening_seen = true,
            (_, "onPlayedEDDemo") => s.ending_seen = true,
            (_, "onPlayedStaffRoll") => s.staff_roll_seen = true,
            (_, "onUnlockedBuilderMario") => s.builder_mario = true,
            (_, "onUnlockedSuperBallMario") => s.superball_mario = true,
            (_, "startAutoSave") => {
                *self.request = Some(Request::Save);
                return Step::Wait;
            }
            (_, "sceneChangeToStaffRoll") => {
                let from_robot = call.bool("return_to_robot").unwrap_or(false);
                self.session.staff_roll_from_robot = from_robot;
                *self.request = Some(Request::StaffRoll { from_robot });
                return Step::Wait;
            }
            (_, "showMessage" | "showMessageWithNPCName" | "openMessageApproachWindow") => {
                let speaker = call
                    .str("name")
                    .map(str::to_string)
                    .or_else(|| self.talking.map(str::to_string))
                    .unwrap_or_else(|| call.chart.clone());
                *self.request = Some(Request::Message {
                    speaker,
                    message: call.str("messageid").unwrap_or_default().to_string(),
                    balloon: call.name == "openMessageApproachWindow",
                });
                return Step::Wait;
            }
            _ => {
                *self.request = Some(Request::Present(call.clone()));
                return Step::Wait;
            }
        }
        Step::Next
    }

    fn query(&mut self, call: &Call) -> i32 {
        let s = &*self.state;
        let q = || job_index(call, "quest_index");
        let build = || call.int("build_index").unwrap_or(-1);
        let ended = || s.builds.iter().filter(|x| x.finished).count() as i32;
        match call.name.as_str() {
            "checkQuestClear" => self.job_flag(q(), |j| j.cleared),
            "checkQuestActivate" => self.job_flag(q(), |j| j.activated),
            "checkQuestDisplayed" => self.job_flag(q(), |j| j.displayed),
            "checkAllQuestDisplayed" => b(s.jobs.iter().all(|j| !j.activated || j.displayed)),
            "checkTalkedAboutNewQuest" => b(s.new_jobs),
            "checkQuestOrdered" => b(s.ordered.is_some()),
            "checkOrderedQuestNo" => b(s.ordered.is_some_and(|o| o as i32 == q())),
            "getRewardKind" => s
                .ordered
                .and_then(|o| self.data.job(o as i32))
                .map_or(0, |j| j.reward.kind()),
            // these three answer 0 for "yes" (story-mode.md §9 binary notes)
            "checkAllQuestClear" => b(!s.jobs.iter().all(|j| j.cleared)),
            "checkAnyQuestSupportedClear" => b(!s.jobs.iter().any(|j| j.luigi)),
            "checkAllQuestClearNotSupported" => {
                b(!(s.jobs.iter().all(|j| j.cleared) && !s.jobs.iter().any(|j| j.luigi)))
            }
            "checkBuildOrdered" => b(self.session.selected_build.is_some()),
            "checkBuildCoin" => b(self.session.selected_build.is_some()
                && s.coins >= self.build_cost(self.session.selected_build)),
            "checkEnableBuild" => self.build_flag(build(), |x| x.enabled),
            "checkBuilding" => self.build_flag(build(), |x| x.building && !x.finished),
            "checkEndBuild" => self.build_flag(build(), |x| x.finished),
            "checkBuildEndNum" => ended(),
            "checkEnableOrderBuildStart" => b(!self.build_options().is_empty()),
            "checkQuestBuildCoinForBuild" => b(self
                .build_options()
                .iter()
                .any(|&n| s.coins >= self.build_cost(Some(n)))),
            "checkQuestBuildCoin" => b(s.coins >= call.int("coin_num").unwrap_or(0)),
            "checkAllBuildingDisplayed" => b(s.builds.iter().all(|x| !x.enabled || x.displayed)),
            "checkBuildStartTrigger" => self.session.build_start.map_or(0, |n| n as i32),
            "checkBuildEndTrigger" => {
                let m = self.session.ended_now;
                if m == 0 {
                    NO_BUILD
                } else {
                    m.trailing_zeros() as i32
                }
            }
            "checkBuildEndTriggerOtherExist" => b(self.session.ended_now.count_ones() > 1),
            "checkStainedGlassBuildEndOneDayLeft" => {
                let x = &s.builds[STAINED_GLASS];
                b(x.building && !x.finished && x.days_left == 1)
            }
            "checkQuestMiiClothesGet" => {
                let k = call.int("cloth_type").unwrap_or(-1);
                b((0..32).contains(&k) && s.gear >> k & 1 != 0)
            }
            "checkUnlockedBuilderMario" => b(s.builder_mario),
            "checkPlayedOPDemo" => b(s.opening_seen),
            "checkPlayedEDDemo" => b(s.ending_seen),
            "checkPlayedStaffRoll" => b(s.staff_roll_seen),
            "checkGeneralTalkFlag" => {
                b(s.talk_flag(call.int("npc").unwrap_or(-1), call.int("no").unwrap_or(-1)))
            }
            "checkMoveFlag" => {
                let v = call.str("flag").map_or(0, |f| s.flag(f));
                if v & 1 == 0 {
                    0
                } else if v & 2 != 0 {
                    2
                } else {
                    1
                }
            }
            "checkAppearFlag" | "checkOneDayFlag" => {
                b(call.str("flag").is_some_and(|f| s.flag(f) & 1 != 0))
            }
            "StaffRollCheckAppearanceFlagOfSaveDataQuest" => {
                b(call.str("flagname").is_some_and(|f| s.flag(f) & 1 != 0))
            }
            "StaffrollIsExecutedFromRobot" => b(self.session.staff_roll_from_robot),
            // QuestCharacterController: about the NPC whose flow is running
            "checkFirstTalked" => b(self
                .talking
                .is_some_and(|t| self.session.talked_visit.contains(t))),
            "checkFirstTalkedFromPowerOn" => b(self
                .talking
                .is_some_and(|t| self.session.talked_power_on.contains(t))),
            "checkBuildEndTriggerForBuildChief" => self.lowest_unannounced_finished(),
            "checkSameTimeEndBuilding" => {
                b(s.builds.iter().filter(|x| x.finished && !x.talked).count() > 1)
            }
            n if n.starts_with("random") && n.ends_with("branch") => {
                let k = n["random".len()..n.len() - "branch".len()]
                    .parse()
                    .unwrap_or(2);
                self.session.random(k)
            }
            n if n.starts_with("general") && n.ends_with("branch") => {
                let k: i32 = n["general".len()..n.len() - "branch".len()]
                    .parse()
                    .unwrap_or(2);
                self.session.choice.clamp(0, k - 1)
            }
            // camera already where the demo wants it; course-world counter; BGM stage [?]
            _ => 0,
        }
    }
}

/// Coins of a hub coin unit, if it is one.
pub fn hub_coin_value(unit: &str) -> Option<i32> {
    HUB_COINS.iter().find(|(u, _)| *u == unit).map(|(_, v)| *v)
}

impl Story {
    pub fn new(data: Arc<StoryData>, state: StoryState) -> Self {
        Self {
            data,
            state,
            session: Session {
                rng: 0x9E37_79B9_7F4A_7C15,
                ..Default::default()
            },
            runner: None,
            talking: None,
            pending: None,
            play: None,
        }
    }

    /// Seed the random branches (random2branch …) for reproducible runs.
    pub fn seed(&mut self, seed: u64) {
        self.session.rng = seed | 1;
    }

    /// A flow is running (or waiting on the screen).
    pub fn busy(&self) -> bool {
        self.runner.is_some()
    }

    /// Start `chart::entry` (the screen then calls [`step`](Self::step)).
    pub fn start(&mut self, chart: &str, entry: &str) -> Result<(), FlowError> {
        self.start_with(chart, entry, Params::default())
    }

    pub fn start_with(&mut self, chart: &str, entry: &str, args: Params) -> Result<(), FlowError> {
        self.runner = Some(Runner::start(&self.data.flows, chart, entry, args)?);
        self.pending = None;
        Ok(())
    }

    /// Run the current flow until it needs the screen; None when it has finished.
    pub fn step(&mut self) -> Result<Option<Request>, FlowError> {
        let Some(mut runner) = self.runner.take() else {
            return Ok(None);
        };
        let data = self.data.clone();
        let mut request = None;
        let status = {
            let mut rules = Rules {
                data: &data,
                state: &mut self.state,
                session: &mut self.session,
                request: &mut request,
                play: &mut self.play,
                talking: self.talking.as_deref(),
            };
            runner.run(&data.flows, &mut rules)?
        };
        match status {
            Status::Waiting => {
                self.runner = Some(runner);
                self.pending = request.clone();
                Ok(request)
            }
            Status::Finished => {
                if let Some(t) = self.talking.take() {
                    self.session.talked_visit.insert(t.clone());
                    self.session.talked_power_on.insert(t);
                }
                self.pending = None;
                Ok(None)
            }
        }
    }

    /// The request the flow is waiting on.
    pub fn pending(&self) -> Option<&Request> {
        self.pending.as_ref()
    }

    /// Answer the question just shown (0 = first choice), for the next `general*branch`.
    pub fn answer(&mut self, choice: i32) {
        self.session.choice = choice;
    }

    /// Close the job list: `Some(job)` = Start pressed on that job.
    pub fn order_job(&mut self, job: Option<u8>) {
        match job.filter(|&q| (q as usize) < JOBS && self.state.jobs[q as usize].activated) {
            Some(q) => self.state.ordered = Some(q),
            None => {
                if self.state.ordered.is_some() {
                    self.state.last_ordered = self.state.ordered.take();
                }
            }
        }
    }

    /// The job window was opened on `job` (it stops counting as new).
    pub fn mark_displayed(&mut self, job: u8) {
        if let Some(j) = self.state.jobs.get_mut(job as usize) {
            j.displayed = true;
        }
        if !self
            .state
            .jobs
            .iter()
            .any(|j| j.activated && !j.displayed && !j.cleared)
        {
            self.state.new_jobs = false;
        }
    }

    /// Close the build menu: `Some(build)` = that build chosen.
    pub fn order_build(&mut self, build: Option<u8>) {
        self.session.selected_build = build.filter(|&n| {
            let s = self.state.builds.get(n as usize);
            s.is_some_and(|s| s.enabled && !s.started())
        });
    }

    /// Talk to a hub NPC / object: run its flow (`flow::entry`).
    pub fn talk(&mut self, flow: &str, entry: &str) -> Result<(), FlowError> {
        self.start(flow, entry)?;
        self.talking = Some(flow.to_string());
        Ok(())
    }

    /// The hub was (re-)entered: talk-once lines start over.
    pub fn enter_hub(&mut self) {
        self.session.talked_visit.clear();
    }

    /// The hub scene was loaded (a saved game opened, or back from a course): move flags set
    /// before it count as "settled", so the NPCs that said they would move have moved.
    pub fn after_load(&mut self) {
        let data = self.data.clone();
        self.state.settle_move_flags(&|n| data.flag_kind(n));
    }

    /// The opening has not been seen: run it (`quest_DemoOP`).
    pub fn start_opening(&mut self) -> Result<bool, FlowError> {
        if self.state.opening_seen || !self.data.flows.has_entry("quest_DemoOP", "EntryPoint0") {
            return Ok(false);
        }
        self.start("quest_DemoOP", "EntryPoint0")?;
        Ok(true)
    }

    /// The job being played (after [`Request::EnterCourse`]).
    pub fn job_play(&mut self) -> Option<&mut JobPlay> {
        self.play.as_mut()
    }

    /// The job's course ended: back to the hub, then the scene-entry flow (`course_clear` or
    /// `course_cancel`), which pays, passes the day, counts builds down and saves.
    pub fn finish_job(&mut self, outcome: JobOutcome) -> Result<(), FlowError> {
        let play = self.play.take();
        let Some(q) = self.state.ordered.or(play.as_ref().map(|p| p.job)) else {
            return Ok(());
        };
        self.state.ordered = Some(q);
        let struggling = play.as_ref().is_some_and(|p| p.struggling);
        // the hub scene is built again from the save, as after a load
        self.after_load();
        self.enter_hub();
        let job = &mut self.state.jobs[q as usize];
        if struggling {
            job.struggling = true;
        }
        match outcome {
            JobOutcome::Clear { coins, luigi } => {
                let first = !job.cleared;
                if luigi {
                    if first {
                        job.luigi = true;
                    }
                } else {
                    job.luigi = false;
                }
                self.session.collected = coins.max(0);
                if first {
                    let tag = &self.data.jobs.get(q as usize).map(|j| j.return_tag.clone());
                    if let Some(t) = tag.as_ref().filter(|t| !t.is_empty()) {
                        self.state.return_tag = Some(t.clone());
                    }
                }
                // daily hub things come back after a cleared job
                let data = self.data.clone();
                for (name, v) in self.state.flags.iter_mut() {
                    if data.flag_kind(name) == Some(FlagKind::Daily) {
                        *v = 0;
                    }
                }
                self.start("quest_SceneEnter", "course_clear")
            }
            JobOutcome::Fail => {
                self.session.collected = 0;
                self.start("quest_SceneEnter", "course_cancel")
            }
        }
    }

    /// The staff roll ended: back to the hub (`quest_System::return_from_staffroll`, or the
    /// Coursebot NPC's return entry when it was a replay).
    pub fn staff_roll_done(&mut self) -> Result<(), FlowError> {
        if std::mem::take(&mut self.session.staff_roll_from_robot) {
            self.talk("quest_Robot", "ReturnFromStaffRoll")
        } else {
            self.start("quest_System", "return_from_staffroll")
        }
    }

    /// Picking up a hub coin object (`Coin`, `Coin10`, …) adds its value; a coin whose
    /// disappear gate is an appear or daily flag sets that flag.
    pub fn pick_up(&mut self, object: usize) -> i32 {
        let Some(o) = self.data.hub.get(object) else {
            return 0;
        };
        let v = hub_coin_value(&o.unit).unwrap_or(0);
        self.state.add_coins(v);
        if matches!(o.disappear.kind, 7 | 8) {
            let name = o.disappear.index.clone();
            self.state.set_flag(&name, true);
        }
        v
    }

    /// Whether a hub gate holds (story-mode.md §1.6).
    pub fn gate(&self, kind: i32, index: &str, appear: bool) -> bool {
        let s = &self.state;
        let all = |f: &dyn Fn(i32) -> bool| {
            let nums: Vec<i32> = index
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            !nums.is_empty() && nums.into_iter().all(f)
        };
        match kind {
            0 => appear,
            1 => all(&|q| s.job(q).is_some_and(|j| j.activated)),
            2 => all(&|q| s.job(q).is_some_and(|j| j.cleared)),
            3 => all(&|n| s.build(n).is_some_and(|x| x.enabled)),
            4 => all(&|n| s.build(n).is_some_and(|x| x.started())),
            5 if appear => all(&|n| s.build(n).is_some_and(|x| x.finished && x.model_shown)),
            5 => all(&|n| s.build(n).is_some_and(|x| x.model_shown)),
            6 => s.flag(index) & 3 == 3,
            7 | 8 => s.flag(index) & 1 != 0,
            9 => s.builder_mario,
            _ => false,
        }
    }

    /// Hub objects with a flow (NPCs, interactions) that are present now: appear gate holds,
    /// disappear gate does not. A screen without the 3D hub lists these as the places to talk.
    pub fn hub_things(&self) -> Vec<HubThing> {
        self.data
            .hub
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.flow.is_empty())
            .filter(|(_, o)| self.present(o))
            .map(|(i, o)| HubThing {
                index: i,
                unit: o.unit.clone(),
                flow: o.flow.clone(),
                entry: o.entry.clone(),
                approach: o.approach.clone(),
                pos: o.pos,
            })
            .collect()
    }

    /// Whether a hub object is there now.
    pub fn present(&self, o: &crate::data::HubObject) -> bool {
        self.gate(o.appear.kind, &o.appear.index, true)
            && !(o.disappear.kind != 0 && self.gate(o.disappear.kind, &o.disappear.index, false))
    }

    /// Jobs on the Taskmaster's list in board order (activated; `not_done` hides cleared ones).
    pub fn job_list(&self, not_done: bool) -> Vec<u8> {
        (0..JOBS)
            .filter(|&q| {
                let j = &self.state.jobs[q];
                j.activated && !(not_done && j.cleared)
            })
            .map(|q| q as u8)
            .collect()
    }

    /// The job window's stamp: 0 not cleared, 1 cleared, 2 cleared with Luigi.
    pub fn stamp(&self, job: u8) -> u8 {
        match self.state.jobs.get(job as usize) {
            Some(j) if j.luigi => 2,
            Some(j) if j.cleared => 1,
            _ => 0,
        }
    }

    /// The ending has happened (the Stained Glass is finished).
    pub fn castle_complete(&self) -> bool {
        self.state.builds[STAINED_GLASS].finished
    }
}

/// Split a flow message id (`CommonMsg/Quest/NPC/QstNPC_BuildChief:oyakata0019`) into the
/// message file path and the label.
pub fn split_message_id(id: &str) -> (&str, &str) {
    id.rsplit_once(':').unwrap_or((id, ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data::{Build, Gate, HubObject, Job, JobKind, Reward};
    use crate::flow::Flows;
    use mmk_formats::bfevfl::{Actor, EntryPoint, Event, EventKind, Flowchart, NONE, Value};

    type Act<'a> = (&'a str, Vec<(&'a str, Value)>);

    /// Flowcharts `quest_SceneEnter` and `t` with one actor and entry points that each run a
    /// straight list of actions, as the game's flows do around these rules.
    fn chain_flows(entries: Vec<(&str, Vec<Act>)>) -> Flows {
        let mut actions: Vec<String> = Vec::new();
        let mut events = Vec::new();
        let mut eps = Vec::new();
        for (entry, list) in &entries {
            eps.push(EntryPoint {
                name: entry.to_string(),
                main_event: events.len() as u16,
                ..Default::default()
            });
            for (k, (name, params)) in list.iter().enumerate() {
                let a = match actions.iter().position(|x| x == name) {
                    Some(a) => a,
                    None => {
                        actions.push(name.to_string());
                        actions.len() - 1
                    }
                };
                let next = if k + 1 == list.len() {
                    NONE
                } else {
                    events.len() as u16 + 1
                };
                events.push(Event {
                    name: format!("E{}", events.len()),
                    kind: EventKind::Action {
                        actor: 0,
                        action: a as u16,
                        params: Params(
                            params
                                .iter()
                                .map(|(k, v)| (k.to_string(), v.clone()))
                                .collect(),
                        ),
                        next,
                    },
                });
            }
        }
        let mut flows = Flows::new();
        for name in ["quest_SceneEnter", "t"] {
            flows.add(Flowchart {
                name: name.into(),
                actors: vec![Actor {
                    name: "QuestSystemActor".into(),
                    actions: actions.clone(),
                    ..Default::default()
                }],
                events: events.clone(),
                entry_points: eps.clone(),
            });
        }
        flows
    }

    fn data() -> StoryData {
        let job = |no: u8, reward: Reward| Job {
            no,
            course_no: no,
            course: format!("quest_{no:03}"),
            kind: JobKind::Normal,
            style: "M1".into(),
            difficulty: 0,
            days: 1,
            reward,
            giver: String::new(),
            return_tag: if no == 1 {
                "Tag1".into()
            } else {
                String::new()
            },
            title: String::new(),
            client: String::new(),
            detail: String::new(),
            goal: Default::default(),
        };
        let q = |n: i32| vec![("quest_index", Value::Int(n))];
        let bi = |n: i32| vec![("build_index", Value::Int(n))];
        let flows = chain_flows(vec![
            (
                "course_clear",
                vec![
                    ("onOrderedQuestClear", vec![]),
                    ("calcPlayCourseInfo", vec![("is_clear", Value::Bool(true))]),
                    ("countUpOrderedQuest", vec![]),
                    ("addQuestCourseClearBuildCoin", vec![]),
                    ("cancelOrderedQuest", vec![]),
                ],
            ),
            (
                "course_cancel",
                vec![
                    ("calcPlayCourseInfo", vec![("is_clear", Value::Bool(false))]),
                    ("countUpOrderedQuest", vec![]),
                    ("cancelOrderedQuest", vec![]),
                ],
            ),
            (
                "board",
                vec![
                    ("activateQuest", q(0)),
                    ("activateQuest", q(1)),
                    ("openQuestUI", vec![]),
                    ("sceneChangeToCourse", vec![]),
                ],
            ),
            (
                "enable",
                vec![
                    ("onEnableBuild", bi(0)),
                    ("onEnableBuild", bi(1)),
                    ("onEnableBuild", bi(15)),
                ],
            ),
            (
                "order",
                vec![
                    ("openBuildOrderButton", vec![]),
                    ("subQuestBuildCoinStartBuilding", vec![]),
                    ("startBuilding", vec![]),
                ],
            ),
        ]);
        let build = |no: u8, cost: i32, days: i32| Build {
            no,
            cost,
            days,
            label: String::new(),
        };
        StoryData {
            jobs: vec![job(0, Reward::Coins(100)), job(1, Reward::Gear("G".into()))],
            builds: [build(0, 100, 2), build(1, 50, 3)]
                .into_iter()
                .chain((2..16).map(|n| build(n, 10, 2)))
                .collect(),
            hub: vec![HubObject {
                unit: "Coin10".into(),
                disappear: Gate {
                    kind: 8,
                    index: "DailyCoin".into(),
                },
                ..Default::default()
            }],
            flags: [("DailyCoin".to_string(), FlagKind::Daily)]
                .into_iter()
                .collect(),
            flows,
            lesson_flows: Vec::new(),
        }
    }

    fn drive(st: &mut Story) -> Vec<Request> {
        let mut out = Vec::new();
        while let Some(r) = st.step().unwrap() {
            if let Request::JobList { .. } = r {
                let q = st.job_list(true).first().copied();
                st.order_job(q);
            }
            if let Request::BuildMenu { options } = &r {
                st.order_build(options.first().copied());
            }
            out.push(r);
        }
        out
    }

    fn take_job(st: &mut Story) -> u8 {
        st.start("t", "board").unwrap();
        let reqs = drive(st);
        match reqs.last() {
            Some(Request::EnterCourse { job }) => *job,
            other => panic!("no course: {other:?}"),
        }
    }

    fn clear(st: &mut Story, job: u8, coins: i32, luigi: bool) -> Vec<Request> {
        st.order_job(Some(job));
        st.finish_job(JobOutcome::Clear { coins, luigi }).unwrap();
        drive(st)
    }

    fn call(name: &str) -> Call {
        Call {
            chart: "t".into(),
            event: String::new(),
            actor: "QuestSystemActor".into(),
            actor_sub: String::new(),
            name: name.into(),
            params: Params::default(),
        }
    }

    /// Ask the rules a query (or run an action) directly.
    fn with_rules<R>(st: &mut Story, f: impl FnOnce(&mut Rules) -> R) -> R {
        let data = st.data.clone();
        let mut req = None;
        let mut play = None;
        let mut r = Rules {
            data: &data,
            state: &mut st.state,
            session: &mut st.session,
            request: &mut req,
            play: &mut play,
            talking: None,
        };
        f(&mut r)
    }

    #[test]
    fn first_clear_pays_replays_collect_failures_lose() {
        let mut st = Story::new(Arc::new(data()), StoryState::default());
        assert_eq!(take_job(&mut st), 0);
        assert_eq!(st.job_play().map(|p| p.lives), Some(JOB_LIVES));
        st.finish_job(JobOutcome::Clear {
            coins: 30,
            luigi: false,
        })
        .unwrap();
        let reqs = drive(&mut st);
        assert!(reqs.contains(&Request::Reward {
            collected: 30,
            payment: 100,
            gear: None,
            message: None
        }));
        assert_eq!((st.state.coins, st.state.days), (130, 1));
        assert_eq!((st.state.ordered, st.state.last_ordered), (None, Some(0)));
        // replay: the coins collected only, one more day
        clear(&mut st, 0, 7, false);
        assert_eq!((st.state.coins, st.state.days), (137, 2));
        // failure: nothing, no day, the play still counts
        st.order_job(Some(0));
        st.finish_job(JobOutcome::Fail).unwrap();
        drive(&mut st);
        assert_eq!(
            (st.state.coins, st.state.days, st.state.jobs[0].plays),
            (137, 2, 3)
        );
        // a first clear with a return tag puts Mario there; Luigi's clear stamps the job …
        clear(&mut st, 1, 0, true);
        assert_eq!(st.state.return_tag.as_deref(), Some("Tag1"));
        assert_eq!(st.stamp(1), 2);
        // … until Mario clears it himself
        clear(&mut st, 1, 0, false);
        assert_eq!(st.stamp(1), 1);
    }

    #[test]
    fn builds_are_paid_up_front_and_count_down_per_cleared_job() {
        let mut st = Story::new(Arc::new(data()), StoryState::default());
        st.state.coins = 160;
        st.start("t", "enable").unwrap();
        drive(&mut st);
        st.start("t", "order").unwrap(); // the first option: build 0 (100 coins, 2 days)
        drive(&mut st);
        st.start("t", "order").unwrap(); // then build 1 (50 coins, 3 days), in parallel
        drive(&mut st);
        assert_eq!(st.state.coins, 10);
        assert!(st.state.builds[0].building && st.state.builds[1].building);
        assert_eq!(
            (st.state.builds[0].days_left, st.state.builds[1].days_left),
            (2, 3)
        );
        assert_eq!(
            with_rules(&mut st, |r| r.query(&call("checkBuildEndTrigger"))),
            NO_BUILD
        );
        assert_eq!(
            with_rules(&mut st, |r| r.query(&call("checkEnableOrderBuildStart"))),
            1,
            "build 15 is still on offer"
        );
        st.state.jobs[0].activated = true;
        for day in 1..=3 {
            clear(&mut st, 0, 0, false);
            assert_eq!(st.state.days, day);
        }
        assert!(st.state.builds[0].finished && st.state.builds[1].finished);
        assert_eq!(st.state.builds[0].days_left, 0);
        // the Stained Glass one day from the end is the kidnapping trigger
        st.state.builds[15].building = true;
        st.state.builds[15].days_left = 1;
        with_rules(&mut st, |r| {
            // the pop-ups are queued lowest first until the flow shifts them off
            assert_eq!(r.query(&call("checkBuildEndTrigger")), 0);
            assert_eq!(r.query(&call("checkBuildEndTriggerOtherExist")), 1);
            r.action(&call("shiftLastBuildingTriiger"));
            assert_eq!(r.query(&call("checkBuildEndTrigger")), 1);
            r.action(&call("shiftLastBuildingTriiger"));
            assert_eq!(r.query(&call("checkBuildEndTrigger")), NO_BUILD);
            assert_eq!(r.query(&call("checkStainedGlassBuildEndOneDayLeft")), 1);
            assert_eq!(r.query(&call("checkBuildEndNum")), 2);
            assert_eq!(r.query(&call("checkSameTimeEndBuilding")), 1);
            assert_eq!(r.query(&call("checkBuildEndTriggerForBuildChief")), 0);
            r.action(&call("onTalkedBuildChiefTriiger"));
            assert_eq!(
                r.query(&call("checkBuildEndTriggerForBuildChief")),
                NO_BUILD
            );
            assert_eq!(
                r.query(&call("checkAllQuestClear")),
                1,
                "0 means all cleared"
            );
        });
        // the Stained Glass's last day
        clear(&mut st, 0, 0, false);
        assert!(st.castle_complete());
    }

    #[test]
    fn coins_and_days_stay_in_range() {
        let mut st = Story::new(Arc::new(data()), StoryState::default());
        st.state.coins = MAX_COUNT - 5;
        st.state.days = MAX_COUNT;
        st.state.jobs[0].activated = true;
        clear(&mut st, 0, 50, false);
        assert_eq!((st.state.coins, st.state.days), (MAX_COUNT, MAX_COUNT));
    }

    #[test]
    fn hub_gates_coins_and_daily_flags() {
        let mut st = Story::new(Arc::new(data()), StoryState::default());
        let coin = st.data.hub[0].clone();
        assert!(st.present(&coin));
        assert_eq!(st.pick_up(0), 10);
        assert_eq!(st.state.coins, 10);
        assert!(!st.present(&coin), "picked up for the day");
        st.state.jobs[0].activated = true;
        clear(&mut st, 0, 0, false);
        assert!(st.present(&coin), "back after a cleared job");
        assert!(st.gate(1, "0", true) && !st.gate(2, "0,1", true));
        assert!(st.gate(2, "0", true));
        st.state.set_flag("M", true);
        assert!(!st.gate(6, "M", true));
        st.state.settle_move_flags(&|_| Some(FlagKind::Move));
        assert!(st.gate(6, "M", true));
    }

    #[test]
    fn job_play_lives_too_bad_and_struggling() {
        let mut p = JobPlay {
            job: 3,
            lives: JOB_LIVES,
            attempts: 1,
            struggling: false,
            too_bad: false,
            luigi: false,
        };
        assert!(!p.miss());
        assert_eq!((p.lives, p.attempts, p.struggling), (4, 2, false));
        assert!(!p.miss());
        assert!(
            p.struggling,
            "a third attempt switches to the struggling play type"
        );
        for _ in 0..2 {
            assert!(!p.miss());
        }
        assert!(p.miss(), "the fifth miss is TOO BAD");
        assert_eq!(p.lives, 0);
        p.too_bad(TooBadChoice::TryAgain);
        assert_eq!((p.lives, p.too_bad), (JOB_LIVES, false));
        for _ in 0..5 {
            p.miss();
        }
        p.too_bad(TooBadChoice::CallLuigi);
        assert!(p.luigi);
    }

    #[test]
    fn message_ids_split() {
        assert_eq!(
            split_message_id("CommonMsg/Quest/NPC/QstNPC_X:line0001"),
            ("CommonMsg/Quest/NPC/QstNPC_X", "line0001")
        );
        assert_eq!(hub_coin_value("Coin30"), Some(30));
        assert_eq!(hub_coin_value("Coins"), None);
    }
}
