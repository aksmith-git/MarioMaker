//! Story save state: what SMM2 keeps in `quest.dat` (story-mode.md §9), as our own JSON.
//!
//! Per job: times played and the flag bits (played, cleared, activated, displayed, Luigi mark,
//! "struggling" play type); per build: days left and flag bits (enabled, building, finished,
//! displayed, talked about by the Chief, finished model shown); the day count, coin total, the
//! ordered job, a few one-time bits (new-jobs pending, opening / ending / staff roll seen, assist
//! hint seen), gear bits, the 16 talk-flag words and the named move / appear / daily flags; plus
//! the two unlocks SMM2 writes into the main save (Builder Mario, Superball Mario).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// Jobs (QuestNo 0..=119) and builds (BuildNo 0..=15).
pub const JOBS: usize = 120;
pub const BUILDS: usize = 16;
/// Coin total and day count are kept in 0..=9999 (story-mode.md §3.2).
pub const MAX_COUNT: i32 = 9999;
/// Version of our JSON layout.
pub const STATE_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JobState {
    pub plays: i32,
    /// Played at least once without clearing (quest.dat bit 0).
    pub played: bool,
    pub cleared: bool,
    pub activated: bool,
    pub displayed: bool,
    /// Cleared by Luigi's Clear Support and not yet by the player.
    pub luigi: bool,
    /// Plays in the "struggling" play type (assist parts) from now on.
    pub struggling: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildState {
    pub days_left: i32,
    pub enabled: bool,
    pub building: bool,
    pub finished: bool,
    /// Shown in the Chief's build menu.
    pub displayed: bool,
    /// The Chief has announced its completion (`onTalkedBuildChiefTriiger`).
    pub talked: bool,
    /// Its finished model is shown on the hub (`onModelDisplay`).
    pub model_shown: bool,
}

impl BuildState {
    /// Started: building or finished (hub gate type 4).
    pub fn started(&self) -> bool {
        self.building || self.finished
    }
}

/// Named flag kinds of `QuestFlagList` (talk flags are stored as NPC words instead).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagKind {
    Talk,
    /// An NPC moved on: 0 off, 1 set this session, 2 settled after a reload.
    Move,
    /// Appear / disappear of hub things, rewards and seen-demo flags.
    Appear,
    /// Reset when the hub is entered after a cleared job.
    Daily,
}

impl FlagKind {
    pub fn name(self) -> &'static str {
        match self {
            FlagKind::Talk => "talk",
            FlagKind::Move => "move",
            FlagKind::Appear => "appear",
            FlagKind::Daily => "daily",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "talk" => FlagKind::Talk,
            "move" => FlagKind::Move,
            "appear" => FlagKind::Appear,
            "daily" => FlagKind::Daily,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoryState {
    pub jobs: [JobState; JOBS],
    pub builds: [BuildState; BUILDS],
    pub days: i32,
    pub coins: i32,
    /// The job taken at the Taskmaster (None: none).
    pub ordered: Option<u8>,
    /// The previous ordered job (`cancelOrderedQuest` keeps it).
    pub last_ordered: Option<u8>,
    pub new_jobs: bool,
    pub opening_seen: bool,
    pub ending_seen: bool,
    pub staff_roll_seen: bool,
    pub assist_hint_seen: bool,
    /// Gear bits by cloth type (0 Rocket … 10 Superball, story-mode.md §5).
    pub gear: u32,
    /// Talk flags: word per NPC number, flag `no` = bit `no + 1`.
    pub talk: [u32; 16],
    /// Named flags (move / appear / daily): bit 0 on; move flags get bit 1 on reload.
    pub flags: BTreeMap<String, u8>,
    pub builder_mario: bool,
    pub superball_mario: bool,
    /// Hub tag where Mario stands after the first clear of a job with a return tag.
    pub return_tag: Option<String>,
}

impl Default for StoryState {
    fn default() -> Self {
        Self {
            jobs: [JobState::default(); JOBS],
            builds: [BuildState::default(); BUILDS],
            days: 0,
            coins: 0,
            ordered: None,
            last_ordered: None,
            new_jobs: false,
            opening_seen: false,
            ending_seen: false,
            staff_roll_seen: false,
            assist_hint_seen: false,
            gear: 0,
            talk: [0; 16],
            flags: BTreeMap::new(),
            builder_mario: false,
            superball_mario: false,
            return_tag: None,
        }
    }
}

fn bits(list: &[bool]) -> u64 {
    list.iter()
        .enumerate()
        .fold(0, |acc, (i, &b)| acc | ((b as u64) << i))
}

fn bit(v: u64, i: usize) -> bool {
    v >> i & 1 != 0
}

impl StoryState {
    pub fn job(&self, q: i32) -> Option<&JobState> {
        usize::try_from(q).ok().and_then(|q| self.jobs.get(q))
    }
    pub fn job_mut(&mut self, q: i32) -> Option<&mut JobState> {
        usize::try_from(q).ok().and_then(|q| self.jobs.get_mut(q))
    }
    pub fn build(&self, b: i32) -> Option<&BuildState> {
        usize::try_from(b).ok().and_then(|b| self.builds.get(b))
    }
    pub fn build_mut(&mut self, b: i32) -> Option<&mut BuildState> {
        usize::try_from(b).ok().and_then(|b| self.builds.get_mut(b))
    }

    /// Add `n` (may be negative) to the coin total, kept in 0..=9999.
    pub fn add_coins(&mut self, n: i32) {
        self.coins = self.coins.saturating_add(n).clamp(0, MAX_COUNT);
    }

    pub fn flag(&self, name: &str) -> u8 {
        self.flags.get(name).copied().unwrap_or(0)
    }
    pub fn set_flag(&mut self, name: &str, on: bool) {
        let v = self.flags.entry(name.to_string()).or_insert(0);
        if on {
            *v |= 1;
        } else {
            *v = 0;
        }
    }

    pub fn talk_flag(&self, npc: i32, no: i32) -> bool {
        match (usize::try_from(npc), u32::try_from(no)) {
            (Ok(n), Ok(no)) if n < 16 && no < 31 => self.talk[n] >> (no + 1) & 1 != 0,
            _ => false,
        }
    }
    pub fn set_talk_flag(&mut self, npc: i32, no: i32) {
        if let (Ok(n), Ok(no)) = (usize::try_from(npc), u32::try_from(no))
            && n < 16
            && no < 31
        {
            self.talk[n] |= 1 << (no + 1);
        }
    }

    /// The state as it is after a reload: move flags set this session become "settled".
    pub fn settle_move_flags(&mut self, kinds: &dyn Fn(&str) -> Option<FlagKind>) {
        for (name, v) in self.flags.iter_mut() {
            if *v & 1 != 0 && kinds(name) == Some(FlagKind::Move) {
                *v |= 2;
            }
        }
    }

    pub fn to_json(&self) -> Value {
        let jobs: Vec<Value> = self
            .jobs
            .iter()
            .map(|j| {
                json!([
                    j.plays,
                    bits(&[
                        j.played,
                        j.cleared,
                        j.activated,
                        j.displayed,
                        j.luigi,
                        j.struggling
                    ])
                ])
            })
            .collect();
        let builds: Vec<Value> = self
            .builds
            .iter()
            .map(|b| {
                json!([
                    b.days_left,
                    bits(&[
                        b.enabled,
                        b.building,
                        b.finished,
                        b.displayed,
                        b.talked,
                        b.model_shown
                    ])
                ])
            })
            .collect();
        let flags: Map<String, Value> = self
            .flags
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        json!({
            "version": STATE_VERSION,
            "jobs": jobs,
            "builds": builds,
            "days": self.days,
            "coins": self.coins,
            "ordered": self.ordered,
            "last_ordered": self.last_ordered,
            "new_jobs": self.new_jobs,
            "opening_seen": self.opening_seen,
            "ending_seen": self.ending_seen,
            "staff_roll_seen": self.staff_roll_seen,
            "assist_hint_seen": self.assist_hint_seen,
            "gear": self.gear,
            "talk": self.talk.to_vec(),
            "flags": flags,
            "builder_mario": self.builder_mario,
            "superball_mario": self.superball_mario,
            "return_tag": self.return_tag,
        })
    }

    /// Read a state written by [`to_json`](Self::to_json); missing fields keep their defaults.
    pub fn from_json(v: &Value) -> Self {
        let mut s = Self::default();
        let int = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
        let flag = |k: &str| v[k].as_bool().unwrap_or(false);
        let job_no = |k: &str| v[k].as_u64().filter(|&q| q < JOBS as u64).map(|q| q as u8);
        if let Some(a) = v["jobs"].as_array() {
            for (j, e) in s.jobs.iter_mut().zip(a) {
                let f = e[1].as_u64().unwrap_or(0);
                *j = JobState {
                    plays: e[0].as_i64().unwrap_or(0) as i32,
                    played: bit(f, 0),
                    cleared: bit(f, 1),
                    activated: bit(f, 2),
                    displayed: bit(f, 3),
                    luigi: bit(f, 4),
                    struggling: bit(f, 5),
                };
            }
        }
        if let Some(a) = v["builds"].as_array() {
            for (b, e) in s.builds.iter_mut().zip(a) {
                let f = e[1].as_u64().unwrap_or(0);
                *b = BuildState {
                    days_left: e[0].as_i64().unwrap_or(0) as i32,
                    enabled: bit(f, 0),
                    building: bit(f, 1),
                    finished: bit(f, 2),
                    displayed: bit(f, 3),
                    talked: bit(f, 4),
                    model_shown: bit(f, 5),
                };
            }
        }
        s.days = int("days").clamp(0, MAX_COUNT);
        s.coins = int("coins").clamp(0, MAX_COUNT);
        s.ordered = job_no("ordered");
        s.last_ordered = job_no("last_ordered");
        s.new_jobs = flag("new_jobs");
        s.opening_seen = flag("opening_seen");
        s.ending_seen = flag("ending_seen");
        s.staff_roll_seen = flag("staff_roll_seen");
        s.assist_hint_seen = flag("assist_hint_seen");
        s.gear = v["gear"].as_u64().unwrap_or(0) as u32;
        if let Some(a) = v["talk"].as_array() {
            for (w, e) in s.talk.iter_mut().zip(a) {
                *w = e.as_u64().unwrap_or(0) as u32;
            }
        }
        if let Some(o) = v["flags"].as_object() {
            for (k, x) in o {
                s.flags.insert(k.clone(), x.as_u64().unwrap_or(0) as u8);
            }
        }
        s.builder_mario = flag("builder_mario");
        s.superball_mario = flag("superball_mario");
        s.return_tag = v["return_tag"].as_str().map(str::to_string);
        // the ordered job is kept only while it is on offer (quest.dat load rule)
        if let Some(q) = s.ordered
            && !s.jobs[q as usize].activated
        {
            s.last_ordered = s.ordered.take();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_clamps() {
        let mut s = StoryState::default();
        s.jobs[3] = JobState {
            plays: 4,
            played: true,
            cleared: true,
            activated: true,
            displayed: true,
            luigi: true,
            struggling: false,
        };
        s.builds[15] = BuildState {
            days_left: 1,
            enabled: true,
            building: true,
            ..Default::default()
        };
        s.ordered = Some(3);
        s.coins = 1234;
        s.days = 17;
        s.gear = 0b101;
        s.set_talk_flag(0, 6);
        s.set_flag("KinopikoHide", true);
        s.return_tag = Some("NPC01ReturnPosTag".into());
        let back = StoryState::from_json(&s.to_json());
        assert_eq!(back, s);
        assert!(back.talk_flag(0, 6) && !back.talk_flag(0, 5));
        let mut v = s.to_json();
        v["coins"] = json!(123456);
        v["ordered"] = json!(9); // not activated: dropped to "last ordered"
        let back = StoryState::from_json(&v);
        assert_eq!(back.coins, MAX_COUNT);
        assert_eq!((back.ordered, back.last_ordered), (None, Some(9)));
        assert_eq!(StoryState::from_json(&json!({})), StoryState::default());
    }

    #[test]
    fn coins_stay_in_range() {
        let mut s = StoryState::default();
        s.add_coins(-5);
        assert_eq!(s.coins, 0);
        s.add_coins(20_000);
        assert_eq!(s.coins, MAX_COUNT);
    }
}
