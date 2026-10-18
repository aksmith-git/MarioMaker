//! Yamamura's Dojo: the lesson list (from the owner's imported data), what a lesson button
//! starts, and the saved Dojo state (completion marks, Maker Basics, Nina's introduction).
//!
//! Written from the private behaviour spec `research/specs/coursebot.md` §B. The catalogue is
//! never shipped: the importer writes `lessons/index.json` into the asset cache and
//! [`LessonCatalogue::from_json`] reads it:
//!
//! ```json
//! {"version":1,
//!  "technique":[{"lesson_no":0,"course_no":5,"course":"lesson_005","flow":"lesson_technique000",
//!                "title_label":"0000","level":0}],
//!  "player":[{"lesson_no":0,"course_no":0,"course":"lesson_000","flow":"lesson_player000","form":5}],
//!  "tutorial":[{"lesson_no":0,"course_no":4,"course":"lesson_004","flow":"lesson_tutorial000"}],
//!  "key_replays":{"1":"<replay name>"}}
//! ```
//!
//! **Starting a lesson** is scene change [`REASON_TO_LESSON`] with the course
//! `0x12F + course_no` and a [`LessonDescriptor`] {lesson number, kind, extra}; the extra word is
//! the power-up form for the Mario's Moves lessons and 0 otherwise (§B.2).
//!
//! **Ending** (`EndLesson` at the end of every flow): a Maker Lesson sets its completion bit, Maker
//! Basics sets "Maker Basics done" (which also grows the title-course rotation from 8 to 10),
//! power-up lessons set nothing; then the dialog offers "watch again" (the same start again) or
//! "leave" (scene change [`REASON_TO_LESSON_MENU`]). Leaving mid-lesson with the Exit button
//! marks nothing. Everything is unlocked from the start.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::coursebot::{CourseNumber, RAW_LESSON};

/// Scene-change reason: main menu → Dojo menu, and "leave" after a lesson (0x37).
pub const REASON_TO_LESSON_MENU: u8 = 0x37;
/// Scene-change reason: start a lesson (0x38).
pub const REASON_TO_LESSON: u8 = 0x38;
/// Maker Lessons per level tab.
pub const LESSONS_PER_LEVEL: usize = 15;
/// Rows visible at once in the Maker Lessons list.
pub const VISIBLE_ROWS: usize = 7;
/// Title-screen demo courses before / after Maker Basics.
pub const TITLE_COURSES: u32 = 8;
pub const TITLE_COURSES_AFTER_BASICS: u32 = 10;
/// Power-up forms of the four Mario's Moves lessons (lesson_no 0–3; spec table {5, 4, 3, 7}).
pub const POWER_UP_FORMS: [u8; 4] = [5, 4, 3, 7];

/// Lesson kind (the game's enum; −1 Invalid has no variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LessonKind {
    Tutorial = 0,
    Technique = 1,
    Player = 2,
    FirstSeq = 3,
}

impl LessonKind {
    /// Event-flow name stem: `lesson_<kind><NNN>`.
    pub fn flow_stem(self) -> &'static str {
        match self {
            LessonKind::Tutorial => "tutorial",
            LessonKind::Technique => "technique",
            LessonKind::Player => "player",
            LessonKind::FirstSeq => "firstseq",
        }
    }

    pub fn flow_name(self, lesson_no: u32) -> String {
        format!("lesson_{}{lesson_no:03}", self.flow_stem())
    }
}

/// Maker Lessons level tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    #[default]
    Beginner = 0,
    Intermediate = 1,
    Advanced = 2,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Beginner, Level::Intermediate, Level::Advanced];

    pub fn from_index(i: u64) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }
}

/// The three Dojo title-page buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    MariosMoves,
    MakerLessons,
    MakerBasics,
}

/// Mario's Moves pages (one per game style); four carry a power-up lesson button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuideStyle {
    M1 = 0,
    M3 = 1,
    MW = 2,
    WU = 3,
    W3 = 4,
}

impl GuideStyle {
    pub const ALL: [GuideStyle; 5] = [
        GuideStyle::M1,
        GuideStyle::M3,
        GuideStyle::MW,
        GuideStyle::WU,
        GuideStyle::W3,
    ];

    /// Game style code.
    pub fn code(self) -> &'static str {
        match self {
            GuideStyle::M1 => "M1",
            GuideStyle::M3 => "M3",
            GuideStyle::MW => "MW",
            GuideStyle::WU => "WU",
            GuideStyle::W3 => "3W",
        }
    }

    /// The power-up lesson on this style's page.
    pub fn power_up(self) -> Option<PowerUpLesson> {
        match self {
            GuideStyle::M1 => None,
            GuideStyle::M3 => Some(PowerUpLesson::Raccoon),
            GuideStyle::MW => Some(PowerUpLesson::Cape),
            GuideStyle::WU => Some(PowerUpLesson::Propeller),
            GuideStyle::W3 => Some(PowerUpLesson::Cat),
        }
    }
}

/// The four Mario's Moves lessons (player lessons 0–3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerUpLesson {
    Raccoon = 0,
    Cape = 1,
    Propeller = 2,
    Cat = 3,
}

impl PowerUpLesson {
    pub const ALL: [PowerUpLesson; 4] = [
        PowerUpLesson::Raccoon,
        PowerUpLesson::Cape,
        PowerUpLesson::Propeller,
        PowerUpLesson::Cat,
    ];

    pub fn lesson_no(self) -> u32 {
        self as u32
    }

    pub fn style(self) -> GuideStyle {
        match self {
            PowerUpLesson::Raccoon => GuideStyle::M3,
            PowerUpLesson::Cape => GuideStyle::MW,
            PowerUpLesson::Propeller => GuideStyle::WU,
            PowerUpLesson::Cat => GuideStyle::W3,
        }
    }

    /// The form the lesson's descriptor carries.
    pub fn form(self) -> u8 {
        POWER_UP_FORMS[self as usize]
    }

    /// UI event of the button (0x157–0x15A).
    pub fn event(self) -> u32 {
        0x157 + self as u32
    }
}

/// One lesson of the catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub kind: LessonKind,
    pub lesson_no: u32,
    /// Index of `lesson_%03d` (the course number is 0x12F + this).
    pub course_no: u32,
    /// Course file stem, `lesson_NNN`.
    pub course: String,
    /// Event flow, `lesson_<kind>NNN`.
    pub flow: String,
    /// Maker Lessons: title label in the list's message file ("0000"…).
    pub title_label: String,
    /// Maker Lessons: level tab.
    pub level: Level,
    /// Mario's Moves: power-up form (descriptor extra).
    pub form: u8,
}

impl Lesson {
    /// The course to load.
    pub fn course_number(&self) -> Option<CourseNumber> {
        CourseNumber::lesson(self.course_no)
    }

    pub fn raw_course_number(&self) -> u16 {
        RAW_LESSON + self.course_no as u16
    }

    pub fn descriptor(&self) -> LessonDescriptor {
        LessonDescriptor {
            lesson_no: self.lesson_no,
            kind: self.kind,
            extra: if self.kind == LessonKind::Player {
                self.form as u32
            } else {
                0
            },
        }
    }

    pub fn start(&self) -> Option<LessonStart> {
        Some(LessonStart {
            course: self.course_number()?,
            descriptor: self.descriptor(),
            flow: self.flow.clone(),
        })
    }
}

/// The 12-byte lesson descriptor the lesson scene receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LessonDescriptor {
    pub lesson_no: u32,
    pub kind: LessonKind,
    /// Power-up form for player lessons, else 0.
    pub extra: u32,
}

/// What a lesson button asks for: scene change [`REASON_TO_LESSON`] with this course and
/// descriptor; the lesson scene runs `flow`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonStart {
    pub course: CourseNumber,
    pub descriptor: LessonDescriptor,
    pub flow: String,
}

/// The lesson dialog's two answers at the end of a lesson.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndChoice {
    /// "Watch again": the same course and descriptor (event 9).
    WatchAgain,
    /// Back to the Dojo menu (event 8).
    Leave,
}

/// Where a lesson's end dialog goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterLesson {
    /// Scene change [`REASON_TO_LESSON`] again.
    Replay(LessonStart),
    /// Scene change [`REASON_TO_LESSON_MENU`].
    ToMenu,
}

pub fn after_lesson(start: &LessonStart, choice: EndChoice) -> AfterLesson {
    match choice {
        EndChoice::WatchAgain => AfterLesson::Replay(start.clone()),
        EndChoice::Leave => AfterLesson::ToMenu,
    }
}

/// The lesson catalogue, from the owner's imported data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LessonCatalogue {
    /// Maker Lessons, sorted by level then lesson_no (display order).
    pub technique: Vec<Lesson>,
    /// Mario's Moves power-up lessons, by lesson_no.
    pub player: Vec<Lesson>,
    /// Maker Basics (and any other tutorial lessons), by lesson_no.
    pub tutorial: Vec<Lesson>,
    /// Demo replay names by key-replay number (`Replay/Lesson/<name>.dat`).
    pub key_replays: BTreeMap<u32, String>,
}

/// Errors reading `lessons/index.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogueError {
    NotJson(String),
    UnsupportedVersion(u64),
}

impl std::fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogueError::NotJson(e) => write!(f, "lessons index: {e}"),
            CatalogueError::UnsupportedVersion(v) => {
                write!(f, "lessons index: unsupported version {v}")
            }
        }
    }
}

impl std::error::Error for CatalogueError {}

fn parse_lessons(v: &Value, key: &str, kind: LessonKind) -> Vec<Lesson> {
    let mut out: Vec<Lesson> = v
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let lesson_no = e.get("lesson_no")?.as_u64()? as u32;
            let course_no = e.get("course_no")?.as_u64()? as u32;
            let s = |k: &str| e.get(k).and_then(Value::as_str).map(String::from);
            let course = s("course").unwrap_or_else(|| format!("lesson_{course_no:03}"));
            let flow = s("flow").unwrap_or_else(|| kind.flow_name(lesson_no));
            let form = match kind {
                LessonKind::Player => e
                    .get("form")
                    .and_then(Value::as_u64)
                    .map(|f| f as u8)
                    .or_else(|| POWER_UP_FORMS.get(lesson_no as usize).copied())?,
                _ => 0,
            };
            Some(Lesson {
                kind,
                lesson_no,
                course_no,
                course,
                flow,
                title_label: s("title_label").unwrap_or_else(|| format!("{lesson_no:04}")),
                level: e
                    .get("level")
                    .and_then(Value::as_u64)
                    .and_then(Level::from_index)
                    .unwrap_or_default(),
                form,
            })
        })
        .filter(|l| CourseNumber::lesson(l.course_no).is_some())
        .filter(|l| kind != LessonKind::Technique || l.lesson_no < 128)
        .collect();
    out.sort_by_key(|l| (l.level, l.lesson_no));
    out.dedup_by_key(|l| l.lesson_no);
    out
}

impl LessonCatalogue {
    pub fn from_json(v: &Value) -> Result<Self, CatalogueError> {
        let version = v.get("version").and_then(Value::as_u64).unwrap_or(1);
        if version != 1 {
            return Err(CatalogueError::UnsupportedVersion(version));
        }
        let key_replays = v
            .get("key_replays")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(k, n)| Some((k.parse().ok()?, n.as_str()?.to_string())))
            .collect();
        Ok(Self {
            technique: parse_lessons(v, "technique", LessonKind::Technique),
            player: parse_lessons(v, "player", LessonKind::Player),
            tutorial: parse_lessons(v, "tutorial", LessonKind::Tutorial),
            key_replays,
        })
    }

    pub fn from_json_str(s: &str) -> Result<Self, CatalogueError> {
        let v: Value =
            serde_json::from_str(s).map_err(|e| CatalogueError::NotJson(e.to_string()))?;
        Self::from_json(&v)
    }

    pub fn is_empty(&self) -> bool {
        self.technique.is_empty() && self.player.is_empty() && self.tutorial.is_empty()
    }

    /// Whether a title-page section has anything to show.
    pub fn has_section(&self, s: Section) -> bool {
        match s {
            // the move reference itself is always there; only its lesson buttons need data
            Section::MariosMoves => true,
            Section::MakerLessons => !self.technique.is_empty(),
            Section::MakerBasics => self.basics().is_some(),
        }
    }

    /// Maker Lessons of one level tab, in display order.
    pub fn level(&self, level: Level) -> Vec<&Lesson> {
        self.technique.iter().filter(|l| l.level == level).collect()
    }

    /// Row `row` of a level tab.
    pub fn technique_at(&self, level: Level, row: usize) -> Option<&Lesson> {
        self.technique.iter().filter(|l| l.level == level).nth(row)
    }

    /// Level tab and row of a Maker Lesson.
    pub fn position_of(&self, lesson_no: u32) -> Option<(Level, usize)> {
        let l = self.technique.iter().find(|l| l.lesson_no == lesson_no)?;
        let row = self
            .technique
            .iter()
            .filter(|x| x.level == l.level)
            .position(|x| x.lesson_no == lesson_no)?;
        Some((l.level, row))
    }

    pub fn power_up(&self, p: PowerUpLesson) -> Option<&Lesson> {
        self.player.iter().find(|l| l.lesson_no == p.lesson_no())
    }

    /// The power-up lesson button of a Mario's Moves page.
    pub fn guide_lesson(&self, style: GuideStyle) -> Option<&Lesson> {
        self.power_up(style.power_up()?)
    }

    /// Maker Basics (tutorial lesson 0).
    pub fn basics(&self) -> Option<&Lesson> {
        self.tutorial.iter().find(|l| l.lesson_no == 0)
    }

    /// The lesson a descriptor names.
    pub fn find(&self, d: &LessonDescriptor) -> Option<&Lesson> {
        let list = match d.kind {
            LessonKind::Technique => &self.technique,
            LessonKind::Player => &self.player,
            LessonKind::Tutorial => &self.tutorial,
            LessonKind::FirstSeq => return None,
        };
        list.iter().find(|l| l.lesson_no == d.lesson_no)
    }

    pub fn replay_name(&self, key: u32) -> Option<&str> {
        self.key_replays.get(&key).map(String::as_str)
    }

    /// Start for a Maker Lessons row (UI event 0x156).
    pub fn start_technique(&self, level: Level, row: usize) -> Option<LessonStart> {
        self.technique_at(level, row)?.start()
    }

    /// Start for a Mario's Moves lesson button (UI events 0x157–0x15A).
    pub fn start_power_up(&self, p: PowerUpLesson) -> Option<LessonStart> {
        self.power_up(p)?.start()
    }

    /// Start for Maker Basics (UI event 0x152, and the first-time editor prompt).
    pub fn start_basics(&self) -> Option<LessonStart> {
        self.basics()?.start()
    }
}

/// Saved Dojo state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DojoSave {
    /// Maker Lessons completion marks, bit = lesson_no (save +0xC8, 128 bits).
    pub completed: u128,
    /// Maker Basics finished (save +0x100 bit 2).
    pub basics_done: bool,
    /// Nina has introduced herself (save +0x100 bit 10; one flag for every lesson kind).
    pub nina_intro_done: bool,
    /// The Maker Lessons list reopens on these.
    pub last_level: Level,
    pub last_row: u32,
    /// Lessons started from the Maker Lessons list (the menu's play counter).
    pub technique_plays: u32,
}

impl DojoSave {
    pub fn is_complete(&self, lesson_no: u32) -> bool {
        lesson_no < 128 && self.completed >> lesson_no & 1 != 0
    }

    pub fn complete_count(&self) -> u32 {
        self.completed.count_ones()
    }

    /// Check marks shown on a level tab, by row.
    pub fn marks(&self, catalogue: &LessonCatalogue, level: Level) -> Vec<bool> {
        catalogue
            .level(level)
            .iter()
            .map(|l| self.is_complete(l.lesson_no))
            .collect()
    }

    /// Title-screen demo courses in the rotation (8, or 10 once Maker Basics is done).
    pub fn title_course_count(&self) -> u32 {
        if self.basics_done {
            TITLE_COURSES_AFTER_BASICS
        } else {
            TITLE_COURSES
        }
    }

    /// The list remembers the row the player opened.
    pub fn remember(&mut self, level: Level, row: usize) {
        self.last_level = level;
        self.last_row = row as u32;
    }

    /// A Maker Lessons row was chosen: remember it, count the play, and return its start.
    pub fn choose_technique(
        &mut self,
        catalogue: &LessonCatalogue,
        level: Level,
        row: usize,
    ) -> Option<LessonStart> {
        let s = catalogue.start_technique(level, row)?;
        self.remember(level, row);
        self.technique_plays = self.technique_plays.saturating_add(1);
        Some(s)
    }

    /// `EndLesson` reached: set the marks this lesson earns. Returns whether anything changed.
    pub fn lesson_finished(&mut self, d: &LessonDescriptor) -> bool {
        let before = self.clone();
        match d.kind {
            LessonKind::Technique if d.lesson_no < 128 => self.completed |= 1u128 << d.lesson_no,
            LessonKind::Tutorial if d.lesson_no == 0 => self.basics_done = true,
            _ => {}
        }
        *self != before
    }

    /// `FinishSelfIntroduction` ran.
    pub fn finish_self_introduction(&mut self) {
        self.nina_intro_done = true;
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": 1,
            "completed": format!("{:032x}", self.completed),
            "basics_done": self.basics_done,
            "nina_intro_done": self.nina_intro_done,
            "last_level": self.last_level as u8,
            "last_row": self.last_row,
            "technique_plays": self.technique_plays,
        })
    }

    /// Robust to missing fields. `completed` is a hex string (128 bits do not fit a JSON
    /// number); a list of lesson numbers is accepted too.
    pub fn from_json(v: &Value) -> Self {
        let completed = match v.get("completed") {
            Some(Value::String(s)) => u128::from_str_radix(s, 16).unwrap_or(0),
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(Value::as_u64)
                .filter(|&n| n < 128)
                .fold(0, |acc, n| acc | 1u128 << n),
            _ => 0,
        };
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        Self {
            completed,
            basics_done: b("basics_done"),
            nina_intro_done: b("nina_intro_done"),
            last_level: Level::from_index(n("last_level")).unwrap_or_default(),
            last_row: n("last_row").min(u32::MAX as u64) as u32,
            technique_plays: n("technique_plays").min(u32::MAX as u64) as u32,
        }
    }

    pub fn from_json_str(s: &str) -> Self {
        serde_json::from_str::<Value>(s)
            .map(|v| Self::from_json(&v))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic catalogue shaped like the importer's output (no game data).
    fn fixture() -> Value {
        let technique: Vec<Value> = (0..45u32)
            .map(|n| {
                json!({
                    "lesson_no": n, "course_no": 5 + n, "course": format!("lesson_{:03}", 5 + n),
                    "flow": format!("lesson_technique{n:03}"), "title_label": format!("{n:04}"),
                    "level": n / 15,
                })
            })
            .rev() // order in the file must not matter
            .collect();
        let player: Vec<Value> = (0..4u32)
            .map(|n| {
                json!({
                    "lesson_no": n, "course_no": n, "course": format!("lesson_{n:03}"),
                    "flow": format!("lesson_player{n:03}"), "form": POWER_UP_FORMS[n as usize],
                })
            })
            .collect();
        json!({
            "version": 1,
            "technique": technique,
            "player": player,
            "tutorial": [{"lesson_no": 0, "course_no": 4, "course": "lesson_004",
                          "flow": "lesson_tutorial000"}],
            "key_replays": {"1": "replay_a", "18": "replay_b", "x": "bad"},
        })
    }

    fn cat() -> LessonCatalogue {
        LessonCatalogue::from_json(&fixture()).unwrap()
    }

    #[test]
    fn parses_the_index() {
        let c = cat();
        assert_eq!(c.technique.len(), 45);
        assert_eq!(c.player.len(), 4);
        assert_eq!(c.tutorial.len(), 1);
        assert_eq!(c.key_replays.len(), 2);
        assert_eq!(c.replay_name(18), Some("replay_b"));
        for level in Level::ALL {
            assert_eq!(c.level(level).len(), LESSONS_PER_LEVEL);
        }
        assert_eq!(c.technique_at(Level::Beginner, 0).unwrap().lesson_no, 0);
        assert_eq!(
            c.technique_at(Level::Intermediate, 0).unwrap().lesson_no,
            15
        );
        assert_eq!(c.technique_at(Level::Advanced, 14).unwrap().lesson_no, 44);
        assert_eq!(c.technique_at(Level::Advanced, 15), None);
        assert_eq!(c.position_of(17), Some((Level::Intermediate, 2)));
        assert!(Section::ALL_FOR_TEST.iter().all(|&s| c.has_section(s)));
        let empty = LessonCatalogue::from_json(&json!({})).unwrap();
        assert!(empty.is_empty());
        assert!(!empty.has_section(Section::MakerLessons));
        assert_eq!(
            LessonCatalogue::from_json(&json!({"version": 2})),
            Err(CatalogueError::UnsupportedVersion(2))
        );
        assert!(LessonCatalogue::from_json_str("{").is_err());
    }

    impl Section {
        const ALL_FOR_TEST: [Section; 3] = [
            Section::MariosMoves,
            Section::MakerLessons,
            Section::MakerBasics,
        ];
    }

    #[test]
    fn skips_incomplete_entries() {
        let c = LessonCatalogue::from_json(&json!({
            "technique": [
                {"lesson_no": 1, "course_no": 6},          // defaults filled
                {"course_no": 7},                          // no lesson_no
                {"lesson_no": 2, "course_no": 500},        // no such course number
                {"lesson_no": 1, "course_no": 6},          // duplicate
            ],
            "player": [{"lesson_no": 9, "course_no": 1}],  // no form, none in the table
        }))
        .unwrap();
        assert_eq!(c.technique.len(), 1);
        assert_eq!(c.technique[0].flow, "lesson_technique001");
        assert_eq!(c.technique[0].course, "lesson_006");
        assert_eq!(c.technique[0].title_label, "0001");
        assert!(c.player.is_empty());
    }

    #[test]
    fn lesson_starts() {
        let c = cat();
        let s = c.start_technique(Level::Beginner, 3).unwrap();
        assert_eq!(s.course, CourseNumber::Lesson(8));
        assert_eq!(s.course.raw(), 0x12F + 8);
        assert_eq!(
            s.descriptor,
            LessonDescriptor {
                lesson_no: 3,
                kind: LessonKind::Technique,
                extra: 0
            }
        );
        assert_eq!(s.flow, "lesson_technique003");

        let b = c.start_basics().unwrap();
        assert_eq!(b.course.raw(), 0x133);
        assert_eq!(b.descriptor.kind, LessonKind::Tutorial);

        let forms: Vec<u32> = PowerUpLesson::ALL
            .iter()
            .map(|&p| c.start_power_up(p).unwrap().descriptor.extra)
            .collect();
        assert_eq!(forms, vec![5, 4, 3, 7]);
        assert_eq!(
            c.guide_lesson(GuideStyle::MW).unwrap().flow,
            "lesson_player001"
        );
        assert!(c.guide_lesson(GuideStyle::M1).is_none());
        assert_eq!(PowerUpLesson::Cat.style().code(), "3W");
        assert_eq!(PowerUpLesson::Propeller.event(), 0x159);
        assert_eq!(c.find(&s.descriptor).unwrap().lesson_no, 3);
        assert_eq!(c.find(&b.descriptor).unwrap().course, "lesson_004");

        assert_eq!(
            after_lesson(&s, EndChoice::WatchAgain),
            AfterLesson::Replay(s.clone())
        );
        assert_eq!(after_lesson(&s, EndChoice::Leave), AfterLesson::ToMenu);
    }

    #[test]
    fn completion_rules() {
        let c = cat();
        let mut save = DojoSave::default();
        assert_eq!(save.title_course_count(), 8);
        let s = save.choose_technique(&c, Level::Advanced, 2).unwrap();
        assert_eq!((save.last_level, save.last_row), (Level::Advanced, 2));
        assert_eq!(save.technique_plays, 1);
        assert!(save.lesson_finished(&s.descriptor));
        assert!(save.is_complete(32));
        assert!(!save.lesson_finished(&s.descriptor), "already set");
        assert_eq!(save.complete_count(), 1);
        let marks = save.marks(&c, Level::Advanced);
        assert_eq!(marks.iter().filter(|&&m| m).count(), 1);
        assert!(marks[2]);

        // power-up lessons set no mark
        let p = c.start_power_up(PowerUpLesson::Raccoon).unwrap();
        assert!(!save.lesson_finished(&p.descriptor));
        assert!(!save.is_complete(0));

        // Maker Basics grows the title rotation
        let b = c.start_basics().unwrap();
        assert!(save.lesson_finished(&b.descriptor));
        assert!(save.basics_done);
        assert_eq!(save.title_course_count(), 10);

        save.finish_self_introduction();
        assert!(save.nina_intro_done);
        assert!(!save.is_complete(200));
    }

    #[test]
    fn save_json_round_trip() {
        let mut save = DojoSave {
            basics_done: true,
            nina_intro_done: true,
            last_level: Level::Intermediate,
            last_row: 6,
            technique_plays: 9,
            ..DojoSave::default()
        };
        for n in [0, 44, 127] {
            save.lesson_finished(&LessonDescriptor {
                lesson_no: n,
                kind: LessonKind::Technique,
                extra: 0,
            });
        }
        let back = DojoSave::from_json(&save.to_json());
        assert_eq!(back, save);
        assert!(back.is_complete(127));
        assert_eq!(DojoSave::from_json_str("nope"), DojoSave::default());
        assert_eq!(DojoSave::from_json(&json!({})), DojoSave::default());
        let listed = DojoSave::from_json(&json!({"completed": [1, 3, 300]}));
        assert_eq!(listed.completed, 0b1010);
    }
}
