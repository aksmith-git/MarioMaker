//! The Coursebot: 120 My Courses + 60 Downloaded slots, the course-number space shared with every
//! scene, and the scene-change reasons that enter and leave the Coursebot screens.
//!
//! Written from the private behaviour spec `research/specs/coursebot.md` (§A.2–A.5).
//!
//! **Slots and files.** Like the game, the visible slot order is only a table: each course number
//! 0–179 maps to a file number, and the files (`course_data_NNN`, `course_thumb_NNN`,
//! `course_replay_NNN`) are never renamed. A drag in My Courses swaps two table entries — the
//! occupied flag travels with the file number, so dropping on an empty slot is a move (§A.4;
//! settled from the v3.0.3 swap state, which swaps whole entries, My Courses only). Deleting
//! clears the occupied flag and keeps the slot's file number for the next save.
//!
//! **Metadata is authoritative for the list.** Name, description, style, theme, clear check,
//! upload code and Downloaded stats live in [`CourseMeta`], keyed by file number (the game keeps
//! the same facts in the course header and in `later.dat`, indexed by file). The course bytes
//! are opaque here: rename and description change the metadata only; hosts stamp
//! `name`/`description` into the course header when they hand a slot to the editor or the
//! uploader (see [`CourseMeta::header_text`]).
//!
//! The pure model is [`Coursebot`] (JSON via [`Coursebot::to_json`] / [`Coursebot::from_json`]);
//! [`CoursebotStore`] adds the files over any [`BlobStore`] ([`DirStore`] for `std::fs`,
//! [`MemStore`] for tests and web hosts). The Coursebot starts empty: the game ships no sample
//! courses.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

/// My Courses slots.
pub const MY_COURSES: usize = 120;
/// Downloaded Courses slots.
pub const DOWNLOADED: usize = 60;
/// All slots (the slot table length).
pub const SLOTS: usize = MY_COURSES + DOWNLOADED;
/// Course numbers of the built-in sources (§A.3).
pub const RAW_MEM: u16 = 0xB4;
pub const RAW_NEW: u16 = 0xB5;
pub const RAW_TITLE: u16 = 0xB6;
pub const RAW_QUEST: u16 = 0xB7;
pub const QUESTS: u16 = 120;
pub const RAW_LESSON: u16 = 0x12F;
pub const LESSONS: u16 = 100;
/// Frames of play after which a Downloaded attempt also counts in the long counter (30 s).
pub const LONG_ATTEMPT_FRAMES: u32 = 1800;
/// JSON version written by [`Coursebot::to_json`].
pub const JSON_VERSION: u64 = 1;

/// The two Coursebot tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    MyCourses,
    Downloaded,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::MyCourses, Tab::Downloaded];

    pub fn slots(self) -> usize {
        match self {
            Tab::MyCourses => MY_COURSES,
            Tab::Downloaded => DOWNLOADED,
        }
    }

    /// Rows of four slot buttons in the grid (30 / 15).
    pub fn rows(self) -> usize {
        self.slots() / 4
    }

    /// The `index`-th slot of this tab (display order).
    pub fn slot(self, index: usize) -> Option<CourseNumber> {
        (index < self.slots()).then_some(match self {
            Tab::MyCourses => CourseNumber::MyCourse(index as u8),
            Tab::Downloaded => CourseNumber::Downloaded(index as u8),
        })
    }

    /// First raw course number of the tab.
    pub fn base(self) -> u16 {
        match self {
            Tab::MyCourses => 0,
            Tab::Downloaded => MY_COURSES as u16,
        }
    }
}

/// A course number: what every scene change carries to say which course to load (§A.3).
/// `cInvalid` (−1) has no variant: [`CourseNumber::from_raw`] returns `None` for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CourseNumber {
    /// My Courses slot 0–119 (raw 0–119).
    MyCourse(u8),
    /// Downloaded slot 0–59 (raw 120–179).
    Downloaded(u8),
    /// Keep the course already in memory (raw 180).
    Mem,
    /// A blank new course (raw 181; "Make New Course").
    New,
    /// A title-screen demo course (raw 182).
    Title,
    /// Story Mode job course, `quest_%03d` (raw 183–302).
    Quest(u8),
    /// Dojo lesson course, `lesson_%03d` (raw 303–402).
    Lesson(u8),
}

impl CourseNumber {
    pub fn from_raw(n: i32) -> Option<Self> {
        let n: u16 = u16::try_from(n).ok()?;
        Some(match n {
            0..=119 => CourseNumber::MyCourse(n as u8),
            120..=179 => CourseNumber::Downloaded((n - 120) as u8),
            RAW_MEM => CourseNumber::Mem,
            RAW_NEW => CourseNumber::New,
            RAW_TITLE => CourseNumber::Title,
            _ if (RAW_QUEST..RAW_QUEST + QUESTS).contains(&n) => {
                CourseNumber::Quest((n - RAW_QUEST) as u8)
            }
            _ if (RAW_LESSON..RAW_LESSON + LESSONS).contains(&n) => {
                CourseNumber::Lesson((n - RAW_LESSON) as u8)
            }
            _ => return None,
        })
    }

    /// The raw number. Out-of-range payloads (e.g. `MyCourse(200)`) are clamped to the last
    /// valid number of their range, so the result always round-trips through `from_raw`.
    pub fn raw(self) -> u16 {
        match self {
            CourseNumber::MyCourse(i) => (i as u16).min(MY_COURSES as u16 - 1),
            CourseNumber::Downloaded(i) => 120 + (i as u16).min(DOWNLOADED as u16 - 1),
            CourseNumber::Mem => RAW_MEM,
            CourseNumber::New => RAW_NEW,
            CourseNumber::Title => RAW_TITLE,
            CourseNumber::Quest(i) => RAW_QUEST + (i as u16).min(QUESTS - 1),
            CourseNumber::Lesson(i) => RAW_LESSON + (i as u16).min(LESSONS - 1),
        }
    }

    /// The Coursebot tab and index of a slot number.
    pub fn slot(self) -> Option<(Tab, usize)> {
        match self {
            CourseNumber::MyCourse(i) if (i as usize) < MY_COURSES => {
                Some((Tab::MyCourses, i as usize))
            }
            CourseNumber::Downloaded(i) if (i as usize) < DOWNLOADED => {
                Some((Tab::Downloaded, i as usize))
            }
            _ => None,
        }
    }

    pub fn is_slot(self) -> bool {
        self.slot().is_some()
    }

    pub fn tab(self) -> Option<Tab> {
        self.slot().map(|(t, _)| t)
    }

    /// Index into the 180-entry slot table (= raw number) for slot numbers.
    fn table_index(self) -> Option<usize> {
        self.slot().map(|(t, i)| t.base() as usize + i)
    }

    /// RomFS course stem for built-in courses: `quest_007`, `lesson_004` (§A.3).
    pub fn builtin_stem(self) -> Option<String> {
        match self {
            CourseNumber::Quest(i) if (i as u16) < QUESTS => Some(format!("quest_{i:03}")),
            CourseNumber::Lesson(i) if (i as u16) < LESSONS => Some(format!("lesson_{i:03}")),
            _ => None,
        }
    }

    /// The lesson course for a Dojo `course_no` (0x12F + n).
    pub fn lesson(course_no: u32) -> Option<Self> {
        (course_no < LESSONS as u32).then_some(CourseNumber::Lesson(course_no as u8))
    }

    pub fn quest(index: u32) -> Option<Self> {
        (index < QUESTS as u32).then_some(CourseNumber::Quest(index as u8))
    }
}

/// Title-screen demo course file name: `title_course_data_%02d` (random among `count`).
pub fn title_course_stem(index: u32) -> String {
    format!("title_course_data_{index:02}")
}

/// The game's per-slot file names for a file number. The engine keeps the same stems; see
/// [`CoursebotStore`] for what each file holds.
pub fn course_data_name(file: u8) -> String {
    format!("course_data_{file:03}.bcd")
}
pub fn course_thumb_name(file: u8) -> String {
    format!("course_thumb_{file:03}.btl")
}
pub fn course_replay_name(file: u8) -> String {
    format!("course_replay_{file:03}.dat")
}

/// The scene-change reasons that touch the Coursebot (§A.2; the value is the enum order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneReason {
    /// Title → Play → Coursebot.
    TitleToRobo = 1,
    /// Coursebot → Course Maker (Make; "Make New Course" passes [`CourseNumber::New`]).
    RoboToEdit = 3,
    /// Play a My Courses slot.
    MyCourseToNormalPlay = 4,
    /// Play a Downloaded slot.
    LaterCourseToNormalPlay = 5,
    /// Play Together (local co-op).
    RoboToCoopNormalPlay = 6,
    /// Clear check before upload.
    RoboToClearCheck = 7,
    /// Empty Downloaded tab → "Go to Course World?".
    RoboToNetMainForSave = 8,
    RoboToNetMainTitle = 9,
    RoboToNetMainUserInfo = 10,
    /// Course World "Download" → choose a Downloaded slot.
    NetMainToRoboForSave = 0x0D,
    /// Course World download into My Courses [inf].
    NetMainToRoboForSaveMyCourse = 0x0E,
    /// Editor "Save" → choose a My Courses slot.
    ShortCutRobo = 0x1C,
    /// Editor "Load" → choose a course to load.
    ShortCutRoboLoad = 0x1D,
    /// Editor upload.
    ShortCutRoboUpload = 0x1E,
    /// Editor upload that needs a clear check first.
    ShortCutRobotClearCheck = 0x1F,
    /// Main menu "Coursebot".
    MenuToRobo = 0x22,
}

impl SceneReason {
    pub const ALL: [SceneReason; 16] = [
        SceneReason::TitleToRobo,
        SceneReason::RoboToEdit,
        SceneReason::MyCourseToNormalPlay,
        SceneReason::LaterCourseToNormalPlay,
        SceneReason::RoboToCoopNormalPlay,
        SceneReason::RoboToClearCheck,
        SceneReason::RoboToNetMainForSave,
        SceneReason::RoboToNetMainTitle,
        SceneReason::RoboToNetMainUserInfo,
        SceneReason::NetMainToRoboForSave,
        SceneReason::NetMainToRoboForSaveMyCourse,
        SceneReason::ShortCutRobo,
        SceneReason::ShortCutRoboLoad,
        SceneReason::ShortCutRoboUpload,
        SceneReason::ShortCutRobotClearCheck,
        SceneReason::MenuToRobo,
    ];

    pub fn value(self) -> u8 {
        self as u8
    }

    pub fn from_value(v: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.value() == v)
    }

    /// The list mode a reason opens the Coursebot in; `None` for reasons that leave it.
    pub fn list_mode(self) -> Option<ListMode> {
        Some(match self {
            SceneReason::TitleToRobo | SceneReason::MenuToRobo => ListMode::Browse,
            SceneReason::ShortCutRobo => ListMode::Save,
            SceneReason::ShortCutRoboLoad => ListMode::Load,
            SceneReason::ShortCutRoboUpload | SceneReason::ShortCutRobotClearCheck => {
                ListMode::Upload
            }
            SceneReason::NetMainToRoboForSave => ListMode::Download(Tab::Downloaded),
            SceneReason::NetMainToRoboForSaveMyCourse => ListMode::Download(Tab::MyCourses),
            _ => return None,
        })
    }

    /// Play reason for a slot's tab (Play button).
    pub fn play(tab: Tab) -> Self {
        match tab {
            Tab::MyCourses => SceneReason::MyCourseToNormalPlay,
            Tab::Downloaded => SceneReason::LaterCourseToNormalPlay,
        }
    }
}

/// What the Coursebot list is open for (§A.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListMode {
    /// Browse, play, edit, upload, rename, delete (both tabs).
    Browse,
    /// Editor "Save": choose a My Courses slot, confirm overwrite.
    Save,
    /// Editor "Load": choose a course, or "Make New Course".
    Load,
    /// The upload page (after the save).
    Upload,
    /// Course World download into the given tab.
    Download(Tab),
}

impl ListMode {
    /// Header label in `Lcl_CourseList_00` (label ids only; the text is the owner's data).
    pub fn header_label(self) -> Option<&'static str> {
        match self {
            ListMode::Browse => Some("0000"),
            ListMode::Save => Some("0001"),
            ListMode::Load => Some("0004"),
            ListMode::Upload => None,
            ListMode::Download(_) => Some("0008"),
        }
    }

    /// Labels shown while / after writing ("Saving..." / "Saving Complete", download likewise).
    pub fn progress_labels(self) -> Option<(&'static str, &'static str)> {
        match self {
            ListMode::Save => Some(("0002", "0003")),
            ListMode::Download(_) => Some(("0009", "0010")),
            _ => None,
        }
    }

    /// The tab the list opens on.
    pub fn initial_tab(self) -> Tab {
        match self {
            ListMode::Download(t) => t,
            _ => Tab::MyCourses,
        }
    }

    /// Whether the other tab can be selected in this mode.
    pub fn tabs_switchable(self) -> bool {
        matches!(self, ListMode::Browse | ListMode::Load)
    }

    /// Whether choosing an empty slot is meaningful (save targets, "Make New Course").
    pub fn picks_empty(self) -> bool {
        matches!(
            self,
            ListMode::Browse | ListMode::Save | ListMode::Load | ListMode::Download(_)
        )
    }
}

/// How a course played from the Coursebot ended: the game's exit reason mapped to an out-type
/// (table at 0x71023d73e0, §A.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayOutcome {
    /// Out-type 0 (quit, other).
    Quit = 0,
    /// Out-type 1 (slide pass).
    Pass = 1,
    /// Out-type 2 (death).
    Death = 2,
    /// Out-type 3 (goal, goal-retry).
    Goal = 3,
    /// Out-type 5 (retry from the pause menu).
    Retry = 5,
}

impl PlayOutcome {
    /// From the course exit reason (0x18 goal, 0x19 death, 0x1A retry, 0x1B goal-retry,
    /// 0x26 slide pass; others quit).
    pub fn from_exit_reason(reason: u32) -> Self {
        match reason {
            0x18 | 0x1B => PlayOutcome::Goal,
            0x19 => PlayOutcome::Death,
            0x1A => PlayOutcome::Retry,
            0x26 => PlayOutcome::Pass,
            _ => PlayOutcome::Quit,
        }
    }
}

/// Clear check of an own course: cleared from the start and from every checkpoint flag
/// (upload rule, §A.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClearCheck {
    pub from_start: bool,
    /// One entry per checkpoint flag in the course, true when cleared from it.
    pub checkpoints: Vec<bool>,
    pub attempts: u32,
    /// Clear-check time (the game stores 0xFFFFFFFF for none).
    pub time_ms: Option<u32>,
}

impl ClearCheck {
    /// A fresh check for a course with `checkpoints` checkpoint flags (after any edit).
    pub fn reset(checkpoints: usize) -> Self {
        Self {
            checkpoints: vec![false; checkpoints],
            ..Self::default()
        }
    }

    /// Upload allowed.
    pub fn passed(&self) -> bool {
        self.from_start && self.checkpoints.iter().all(|&c| c)
    }

    /// Record a clear-check run from `start` (`None` = the start, `Some(i)` = checkpoint i).
    pub fn record(&mut self, start: Option<usize>, cleared: bool, time_ms: Option<u32>) {
        self.attempts = self.attempts.saturating_add(1);
        if !cleared {
            return;
        }
        match start {
            None => self.from_start = true,
            Some(i) => {
                if let Some(c) = self.checkpoints.get_mut(i) {
                    *c = true;
                }
            }
        }
        if start.is_none()
            && let Some(t) = time_ms
        {
            self.time_ms = Some(self.time_ms.map_or(t, |old| old.min(t)));
        }
    }
}

/// World record of a downloaded course (server snapshot).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldRecord {
    pub maker_name: String,
    pub maker_id: String,
    pub time_ms: u32,
}

/// Server stats of a downloaded course, as of the download / last refresh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerStats {
    pub likes: u32,
    pub plays: u32,
    pub clears: u32,
    pub attempts: u32,
    /// 0 Easy, 1 Normal, 2 Expert, 3 Super Expert.
    pub difficulty: u8,
    pub tags: Vec<u8>,
    pub world_record: Option<WorldRecord>,
}

impl ServerStats {
    /// Clear rate in percent (0 with no attempts).
    pub fn clear_rate(&self) -> f32 {
        if self.attempts == 0 {
            0.0
        } else {
            self.clears as f32 * 100.0 / self.attempts as f32
        }
    }
}

/// Local play state of a downloaded course (the 0x18-byte `later.dat` record's state byte).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayState {
    #[default]
    Unplayed = 0,
    Played = 1,
    /// At least one failed attempt (death or retry).
    Attempted = 2,
    /// Cleared [inf: set by the goal path, which skips the stats update]; counters stop.
    Cleared = 3,
}

impl PlayState {
    fn from_u64(v: u64) -> Self {
        match v {
            1 => PlayState::Played,
            2 => PlayState::Attempted,
            3 => PlayState::Cleared,
            _ => PlayState::Unplayed,
        }
    }
}

/// Local play stats of a downloaded course (§A.3 "Downloaded play stats").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalPlayStats {
    /// Failed attempts (death, retry).
    pub attempts: u32,
    /// Plays that lasted more than [`LONG_ATTEMPT_FRAMES`] (single player).
    pub long_attempts: u32,
    pub state: PlayState,
    /// The record's "played" bit.
    pub played: bool,
}

impl LocalPlayStats {
    /// Update after a course played from a Downloaded slot ends.
    pub fn record(&mut self, outcome: PlayOutcome, frames: u32, players: u32) {
        self.played = true;
        if self.state == PlayState::Cleared {
            return;
        }
        if outcome == PlayOutcome::Goal {
            self.state = PlayState::Cleared;
            return;
        }
        if self.state == PlayState::Unplayed {
            self.state = PlayState::Played;
        }
        if matches!(outcome, PlayOutcome::Death | PlayOutcome::Retry) {
            self.state = PlayState::Attempted;
            self.attempts = self.attempts.saturating_add(1);
        }
        if players < 2 && frames > LONG_ATTEMPT_FRAMES {
            self.long_attempts = self.long_attempts.saturating_add(1);
        }
    }
}

/// What a Downloaded slot knows about the server course.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DownloadInfo {
    /// Our server's course code.
    pub course_code: String,
    pub maker_name: String,
    pub maker_id: String,
    pub server: ServerStats,
    pub local: LocalPlayStats,
}

/// Per-course metadata shown by the list and the info window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CourseMeta {
    pub name: String,
    pub description: String,
    /// Game style code: "M1", "M3", "MW", "WU", "3W".
    pub style: String,
    /// Main-area theme id.
    pub theme: u8,
    pub clear_check: ClearCheck,
    /// Our server's code once uploaded (own courses).
    pub uploaded: Option<String>,
    /// Save time, seconds since the Unix epoch (host clock).
    pub saved_at: u64,
    /// Super World this course belongs to (filter hook).
    pub world: Option<u8>,
    /// Downloaded courses only.
    pub download: Option<DownloadInfo>,
}

impl CourseMeta {
    pub fn new(name: &str, style: &str, theme: u8) -> Self {
        Self {
            name: name.into(),
            style: style.into(),
            theme,
            ..Self::default()
        }
    }

    /// Name and description clamped to what the course header holds (32 / 100 UTF-16 units
    /// plus a terminator), for stamping into the header.
    pub fn header_text(&self) -> (String, String) {
        (
            clamp_utf16(&self.name, 32),
            clamp_utf16(&self.description, 100),
        )
    }
}

/// Longest prefix of `s` that fits in `units` UTF-16 code units.
pub fn clamp_utf16(s: &str, units: usize) -> String {
    let mut n = 0;
    s.chars()
        .take_while(|c| {
            n += c.len_utf16();
            n <= units
        })
        .collect()
}

/// One slot as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotView<'a> {
    pub number: CourseNumber,
    /// Index within the tab (display order).
    pub index: usize,
    pub file: u8,
    pub meta: Option<&'a CourseMeta>,
}

/// Why a Coursebot operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoursebotError {
    /// Not a My Courses / Downloaded slot.
    NotASlot(CourseNumber),
    /// The slot is empty.
    Empty(CourseNumber),
    /// Saving onto an occupied slot needs the overwrite confirmation ("This data will be
    /// overwritten.").
    NeedsOverwriteConfirm(CourseNumber),
    /// The tab has no free slot.
    Full(Tab),
    /// Only My Courses slots can be swapped by dragging.
    NotSwappable(CourseNumber, CourseNumber),
    /// A downloaded course's metadata must carry [`DownloadInfo`], an own course's must not.
    WrongTab(CourseNumber),
}

impl fmt::Display for CoursebotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoursebotError::NotASlot(n) => write!(f, "{n:?} is not a Coursebot slot"),
            CoursebotError::Empty(n) => write!(f, "slot {n:?} is empty"),
            CoursebotError::NeedsOverwriteConfirm(n) => {
                write!(f, "slot {n:?} is occupied; confirm the overwrite")
            }
            CoursebotError::Full(t) => write!(f, "no free slot in {t:?}"),
            CoursebotError::NotSwappable(a, b) => write!(f, "cannot swap {a:?} and {b:?}"),
            CoursebotError::WrongTab(n) => write!(f, "metadata does not fit slot {n:?}"),
        }
    }
}

impl std::error::Error for CoursebotError {}

/// Why the Upload button is refused (§A.4 upload rules; the server adds its own limits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadBlock {
    Empty,
    /// Downloaded courses cannot be uploaded.
    NotOwn,
    /// Already uploaded (shows the code).
    AlreadyUploaded(String),
    /// Needs a clear check from the start and every checkpoint (→ [`SceneReason::RoboToClearCheck`]).
    NeedsClearCheck,
    /// The server's maximum is reached (offer to delete an uploaded course).
    LimitReached {
        uploaded: u32,
        max: u32,
    },
}

/// One entry of the slot table: the file the slot points at and whether it holds a course
/// (the game's byte 0 and byte 1 bit 0; the bit gates every load and rewrite of the slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotEntry {
    pub file: u8,
    pub occupied: bool,
}

/// The Coursebot slot model.
#[derive(Clone, Debug, PartialEq)]
pub struct Coursebot {
    table: [SlotEntry; SLOTS],
    /// Metadata by file number (valid while some occupied slot points at the file).
    files: BTreeMap<u8, CourseMeta>,
}

impl Default for Coursebot {
    fn default() -> Self {
        Self::new()
    }
}

impl Coursebot {
    /// Empty Coursebot, identity table (slot n → file n).
    pub fn new() -> Self {
        Self {
            table: std::array::from_fn(|i| SlotEntry {
                file: i as u8,
                occupied: false,
            }),
            files: BTreeMap::new(),
        }
    }

    fn entry(&self, n: CourseNumber) -> Result<&SlotEntry, CoursebotError> {
        n.table_index()
            .map(|i| &self.table[i])
            .ok_or(CoursebotError::NotASlot(n))
    }

    /// The slot table entry of a slot.
    pub fn slot_entry(&self, n: CourseNumber) -> Option<SlotEntry> {
        self.entry(n).ok().copied()
    }

    /// File number of a slot (for `course_data_NNN` etc.).
    pub fn file_of(&self, n: CourseNumber) -> Option<u8> {
        self.slot_entry(n).map(|e| e.file)
    }

    /// `later.dat` record index of a Downloaded slot (file − 120, clamped to 0–59).
    pub fn later_record(&self, n: CourseNumber) -> Option<usize> {
        match n.tab()? {
            Tab::Downloaded => self
                .file_of(n)
                .map(|f| (f as usize).saturating_sub(MY_COURSES).min(DOWNLOADED - 1)),
            Tab::MyCourses => None,
        }
    }

    pub fn is_occupied(&self, n: CourseNumber) -> bool {
        self.slot_entry(n).is_some_and(|e| e.occupied)
    }

    pub fn meta(&self, n: CourseNumber) -> Option<&CourseMeta> {
        let e = self.slot_entry(n)?;
        if e.occupied {
            self.files.get(&e.file)
        } else {
            None
        }
    }

    pub fn meta_mut(&mut self, n: CourseNumber) -> Option<&mut CourseMeta> {
        let e = self.slot_entry(n)?;
        if e.occupied {
            self.files.get_mut(&e.file)
        } else {
            None
        }
    }

    fn meta_or_err(&mut self, n: CourseNumber) -> Result<&mut CourseMeta, CoursebotError> {
        self.entry(n)?;
        self.meta_mut(n).ok_or(CoursebotError::Empty(n))
    }

    /// A tab's slots in display order, optionally filtered to one Super World (empty slots and
    /// courses of other worlds are left out when filtering).
    pub fn list(&self, tab: Tab, world: Option<u8>) -> Vec<SlotView<'_>> {
        (0..tab.slots())
            .filter_map(|i| {
                let number = tab.slot(i)?;
                let e = self.slot_entry(number)?;
                let meta = self.meta(number);
                if let Some(w) = world
                    && meta.and_then(|m| m.world) != Some(w)
                {
                    return None;
                }
                Some(SlotView {
                    number,
                    index: i,
                    file: e.file,
                    meta,
                })
            })
            .collect()
    }

    /// Occupied slots in a tab.
    pub fn count(&self, tab: Tab) -> usize {
        (0..tab.slots())
            .filter_map(|i| tab.slot(i))
            .filter(|&n| self.is_occupied(n))
            .count()
    }

    pub fn first_free(&self, tab: Tab) -> Option<CourseNumber> {
        (0..tab.slots())
            .filter_map(|i| tab.slot(i))
            .find(|&n| !self.is_occupied(n))
    }

    /// Whether saving into `n` must first ask "This data will be overwritten.".
    pub fn needs_overwrite_confirm(&self, n: CourseNumber) -> bool {
        self.is_occupied(n)
    }

    /// Save a course into a slot (editor save or download). Returns the file number the host
    /// writes the course / thumbnail / replay to. Overwriting needs `confirmed_overwrite`.
    pub fn save(
        &mut self,
        n: CourseNumber,
        meta: CourseMeta,
        confirmed_overwrite: bool,
    ) -> Result<u8, CoursebotError> {
        let e = *self.entry(n)?;
        let downloaded = n.tab() == Some(Tab::Downloaded);
        if downloaded != meta.download.is_some() {
            return Err(CoursebotError::WrongTab(n));
        }
        if e.occupied && !confirmed_overwrite {
            return Err(CoursebotError::NeedsOverwriteConfirm(n));
        }
        let i = n.table_index().expect("slot");
        self.table[i].occupied = true;
        self.files.insert(e.file, meta);
        Ok(e.file)
    }

    /// Save into the first free slot of a tab.
    pub fn save_new(
        &mut self,
        tab: Tab,
        meta: CourseMeta,
    ) -> Result<(CourseNumber, u8), CoursebotError> {
        let n = self.first_free(tab).ok_or(CoursebotError::Full(tab))?;
        self.save(n, meta, false).map(|f| (n, f))
    }

    /// Delete a slot's course; returns the freed file number (the host removes its files). The
    /// slot keeps its file number. Deleting a downloaded course leaves the server copy alone.
    pub fn delete(&mut self, n: CourseNumber) -> Result<u8, CoursebotError> {
        let e = *self.entry(n)?;
        if !e.occupied {
            return Err(CoursebotError::Empty(n));
        }
        let i = n.table_index().expect("slot");
        self.table[i].occupied = false;
        self.files.remove(&e.file);
        Ok(e.file)
    }

    /// Drag `a` onto `b` in My Courses: swap the two table entries (files stay put; an empty
    /// target makes it a move). Use [`follow_swap`] for course numbers held elsewhere (the
    /// course being edited, the selected slot).
    pub fn swap(&mut self, a: CourseNumber, b: CourseNumber) -> Result<(), CoursebotError> {
        let (Some((Tab::MyCourses, _)), Some((Tab::MyCourses, _))) = (a.slot(), b.slot()) else {
            return Err(CoursebotError::NotSwappable(a, b));
        };
        let (ia, ib) = (a.table_index().unwrap(), b.table_index().unwrap());
        self.table.swap(ia, ib);
        Ok(())
    }

    pub fn rename(&mut self, n: CourseNumber, name: &str) -> Result<(), CoursebotError> {
        self.meta_or_err(n)?.name = name.into();
        Ok(())
    }

    pub fn set_description(&mut self, n: CourseNumber, text: &str) -> Result<(), CoursebotError> {
        self.meta_or_err(n)?.description = text.into();
        Ok(())
    }

    /// Put a course in a Super World (or take it out).
    pub fn set_world(&mut self, n: CourseNumber, world: Option<u8>) -> Result<(), CoursebotError> {
        self.meta_or_err(n)?.world = world;
        Ok(())
    }

    /// Take every course out of a deleted Super World.
    pub fn clear_world(&mut self, world: u8) {
        for m in self.files.values_mut() {
            if m.world == Some(world) {
                m.world = None;
            }
        }
    }

    pub fn set_uploaded(
        &mut self,
        n: CourseNumber,
        code: Option<String>,
    ) -> Result<(), CoursebotError> {
        if n.tab() == Some(Tab::Downloaded) {
            return Err(CoursebotError::WrongTab(n));
        }
        self.meta_or_err(n)?.uploaded = code;
        Ok(())
    }

    /// Own courses uploaded to the server.
    pub fn uploaded_count(&self) -> usize {
        self.list(Tab::MyCourses, None)
            .iter()
            .filter(|s| s.meta.is_some_and(|m| m.uploaded.is_some()))
            .count()
    }

    /// The Upload button's gate. `limit` = (uploaded, max) from the server when known.
    pub fn upload_check(
        &self,
        n: CourseNumber,
        limit: Option<(u32, u32)>,
    ) -> Result<(), UploadBlock> {
        let Some(m) = self.meta(n) else {
            return Err(UploadBlock::Empty);
        };
        if n.tab() != Some(Tab::MyCourses) {
            return Err(UploadBlock::NotOwn);
        }
        if let Some(code) = &m.uploaded {
            return Err(UploadBlock::AlreadyUploaded(code.clone()));
        }
        if !m.clear_check.passed() {
            return Err(UploadBlock::NeedsClearCheck);
        }
        if let Some((uploaded, max)) = limit
            && uploaded >= max
        {
            return Err(UploadBlock::LimitReached { uploaded, max });
        }
        Ok(())
    }

    /// The Downloaded slot already holding a server course, if any.
    pub fn find_download(&self, course_code: &str) -> Option<CourseNumber> {
        self.list(Tab::Downloaded, None)
            .into_iter()
            .find(|s| {
                s.meta
                    .and_then(|m| m.download.as_ref())
                    .is_some_and(|d| d.course_code == course_code)
            })
            .map(|s| s.number)
    }

    /// A course played from a Downloaded slot ended: update its local play stats.
    pub fn record_downloaded_play(
        &mut self,
        n: CourseNumber,
        outcome: PlayOutcome,
        frames: u32,
        players: u32,
    ) -> Result<(), CoursebotError> {
        let m = self.meta_or_err(n)?;
        let d = m.download.as_mut().ok_or(CoursebotError::WrongTab(n))?;
        d.local.record(outcome, frames, players);
        Ok(())
    }

    // ---- JSON ----

    pub fn to_json(&self) -> Value {
        let table: Vec<Value> = self
            .table
            .iter()
            .map(|e| json!([e.file, e.occupied]))
            .collect();
        let files: Map<String, Value> = self
            .files
            .iter()
            .map(|(f, m)| (f.to_string(), meta_to_json(m)))
            .collect();
        json!({ "version": JSON_VERSION, "table": table, "files": files })
    }

    /// Robust to missing or bad fields: a table that is not a permutation of each tab's file
    /// numbers is replaced by the identity (metadata kept by file number); metadata of files no
    /// occupied slot points at is dropped, and occupied slots without metadata get a default.
    pub fn from_json(v: &Value) -> Self {
        let mut bot = Coursebot::new();
        if let Some(a) = v.get("table").and_then(Value::as_array)
            && a.len() == SLOTS
        {
            let mut table = bot.table;
            for (e, x) in table.iter_mut().zip(a) {
                if let Some(f) = x
                    .get(0)
                    .and_then(Value::as_u64)
                    .filter(|&f| f < SLOTS as u64)
                {
                    e.file = f as u8;
                }
                e.occupied = x.get(1).and_then(Value::as_bool).unwrap_or(false);
            }
            if table_valid(&table) {
                bot.table = table;
            } else {
                for (e, x) in bot.table.iter_mut().zip(table) {
                    e.occupied = x.occupied;
                }
            }
        }
        if let Some(files) = v.get("files").and_then(Value::as_object) {
            for (k, m) in files {
                if let Ok(f) = k.parse::<u8>()
                    && (f as usize) < SLOTS
                {
                    bot.files.insert(f, meta_from_json(m));
                }
            }
        }
        let live: Vec<(u8, bool)> = bot
            .table
            .iter()
            .enumerate()
            .filter(|(_, e)| e.occupied)
            .map(|(i, e)| (e.file, i >= MY_COURSES))
            .collect();
        bot.files.retain(|f, _| live.iter().any(|(lf, _)| lf == f));
        for (f, downloaded) in live {
            let m = bot.files.entry(f).or_default();
            if downloaded && m.download.is_none() {
                m.download = Some(DownloadInfo::default());
            }
            if !downloaded {
                m.download = None;
            }
        }
        bot
    }

    pub fn from_json_str(s: &str) -> Self {
        serde_json::from_str::<Value>(s)
            .map(|v| Self::from_json(&v))
            .unwrap_or_default()
    }
}

/// Where a course number held elsewhere (the course being edited, a selection) ends up after
/// `swap(a, b)`: the game's swap-end state makes "the current course" follow its course.
pub fn follow_swap(n: CourseNumber, a: CourseNumber, b: CourseNumber) -> CourseNumber {
    if n == a {
        b
    } else if n == b {
        a
    } else {
        n
    }
}

/// Each tab's entries must hold exactly that tab's file numbers (swaps never cross tabs).
fn table_valid(table: &[SlotEntry; SLOTS]) -> bool {
    Tab::ALL.iter().all(|&t| {
        let base = t.base() as usize;
        let mut seen = vec![false; t.slots()];
        table[base..base + t.slots()].iter().all(|e| {
            let f = e.file as usize;
            (base..base + t.slots()).contains(&f) && !std::mem::replace(&mut seen[f - base], true)
        })
    })
}

fn u32_of(v: &Value, k: &str) -> u32 {
    v.get(k)
        .and_then(Value::as_u64)
        .map_or(0, |x| x.min(u32::MAX as u64) as u32)
}
fn str_of(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
fn opt_u32(v: &Value, k: &str) -> Option<u32> {
    v.get(k)
        .and_then(Value::as_u64)
        .map(|x| x.min(u32::MAX as u64) as u32)
}

fn meta_to_json(m: &CourseMeta) -> Value {
    let mut o = json!({
        "name": m.name,
        "description": m.description,
        "style": m.style,
        "theme": m.theme,
        "clear_check": {
            "from_start": m.clear_check.from_start,
            "checkpoints": m.clear_check.checkpoints,
            "attempts": m.clear_check.attempts,
            "time_ms": m.clear_check.time_ms,
        },
        "uploaded": m.uploaded,
        "saved_at": m.saved_at,
        "world": m.world,
    });
    if let Some(d) = &m.download {
        let s = &d.server;
        o["download"] = json!({
            "course_code": d.course_code,
            "maker_name": d.maker_name,
            "maker_id": d.maker_id,
            "server": {
                "likes": s.likes, "plays": s.plays, "clears": s.clears, "attempts": s.attempts,
                "difficulty": s.difficulty, "tags": s.tags,
                "world_record": s.world_record.as_ref().map(|w| json!({
                    "maker_name": w.maker_name, "maker_id": w.maker_id, "time_ms": w.time_ms,
                })),
            },
            "local": {
                "attempts": d.local.attempts,
                "long_attempts": d.local.long_attempts,
                "state": d.local.state as u8,
                "played": d.local.played,
            },
        });
    }
    o
}

fn meta_from_json(v: &Value) -> CourseMeta {
    let cc = &v["clear_check"];
    let clear_check = ClearCheck {
        from_start: cc
            .get("from_start")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        checkpoints: cc
            .get("checkpoints")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
            .unwrap_or_default(),
        attempts: u32_of(cc, "attempts"),
        time_ms: opt_u32(cc, "time_ms"),
    };
    let download = v.get("download").filter(|d| d.is_object()).map(|d| {
        let s = &d["server"];
        let l = &d["local"];
        DownloadInfo {
            course_code: str_of(d, "course_code"),
            maker_name: str_of(d, "maker_name"),
            maker_id: str_of(d, "maker_id"),
            server: ServerStats {
                likes: u32_of(s, "likes"),
                plays: u32_of(s, "plays"),
                clears: u32_of(s, "clears"),
                attempts: u32_of(s, "attempts"),
                difficulty: u32_of(s, "difficulty").min(3) as u8,
                tags: s
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_u64())
                            .map(|t| t as u8)
                            .collect()
                    })
                    .unwrap_or_default(),
                world_record: s.get("world_record").filter(|w| w.is_object()).map(|w| {
                    WorldRecord {
                        maker_name: str_of(w, "maker_name"),
                        maker_id: str_of(w, "maker_id"),
                        time_ms: u32_of(w, "time_ms"),
                    }
                }),
            },
            local: LocalPlayStats {
                attempts: u32_of(l, "attempts"),
                long_attempts: u32_of(l, "long_attempts"),
                state: PlayState::from_u64(l.get("state").and_then(Value::as_u64).unwrap_or(0)),
                played: l.get("played").and_then(Value::as_bool).unwrap_or(false),
            },
        }
    });
    CourseMeta {
        name: str_of(v, "name"),
        description: str_of(v, "description"),
        style: str_of(v, "style"),
        theme: u32_of(v, "theme").min(255) as u8,
        clear_check,
        uploaded: v.get("uploaded").and_then(Value::as_str).map(String::from),
        saved_at: v.get("saved_at").and_then(Value::as_u64).unwrap_or(0),
        world: v
            .get("world")
            .and_then(Value::as_u64)
            .filter(|&w| w < 256)
            .map(|w| w as u8),
        download,
    }
}

// ---- storage ----

/// Named blobs: the Coursebot's index and per-file data. Implement this to keep the Coursebot
/// in browser storage, a database, etc.
pub trait BlobStore {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn write(&mut self, name: &str, data: &[u8]) -> io::Result<()>;
    /// Removing a missing blob is not an error.
    fn remove(&mut self, name: &str) -> io::Result<()>;
}

/// A directory on disk (writes go through a temporary file and a rename).
#[derive(Clone, Debug)]
pub struct DirStore {
    pub dir: PathBuf,
}

impl DirStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl BlobStore for DirStore {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.dir.join(name)) {
            Ok(d) => Ok(Some(d)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn write(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.dir.join(name))
    }
    fn remove(&mut self, name: &str) -> io::Result<()> {
        match std::fs::remove_file(self.dir.join(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// In-memory blobs (tests, web hosts that persist the map themselves).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemStore {
    pub blobs: BTreeMap<String, Vec<u8>>,
}

impl BlobStore for MemStore {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.blobs.get(name).cloned())
    }
    fn write(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        self.blobs.insert(name.into(), data.to_vec());
        Ok(())
    }
    fn remove(&mut self, name: &str) -> io::Result<()> {
        self.blobs.remove(name);
        Ok(())
    }
}

/// Index blob name.
pub const INDEX_NAME: &str = "index.json";

/// Engine file names. The course file keeps the game's name and holds the course bytes exactly
/// as the engine loads them (encrypted `.bcd` or decrypted course data — opaque here); the
/// thumbnail is a PNG; the replay is the clear-check run (our replay format).
pub fn store_data_name(file: u8) -> String {
    course_data_name(file)
}
pub fn store_thumb_name(file: u8) -> String {
    format!("course_thumb_{file:03}.png")
}
pub fn store_replay_name(file: u8) -> String {
    course_replay_name(file)
}

/// Errors from [`CoursebotStore`].
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Coursebot(CoursebotError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "coursebot storage: {e}"),
            StoreError::Coursebot(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}
impl From<CoursebotError> for StoreError {
    fn from(e: CoursebotError) -> Self {
        StoreError::Coursebot(e)
    }
}

/// The Coursebot plus its files. Every mutating call writes the index after the files.
pub struct CoursebotStore<S: BlobStore> {
    pub storage: S,
    bot: Coursebot,
}

impl CoursebotStore<DirStore> {
    /// Open (or start) a Coursebot in a directory.
    pub fn open_dir(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::open(DirStore::new(dir))
    }
}

impl<S: BlobStore> CoursebotStore<S> {
    pub fn open(storage: S) -> io::Result<Self> {
        let bot = match storage.read(INDEX_NAME)? {
            Some(d) => Coursebot::from_json_str(&String::from_utf8_lossy(&d)),
            None => Coursebot::new(),
        };
        Ok(Self { storage, bot })
    }

    pub fn bot(&self) -> &Coursebot {
        &self.bot
    }

    /// Change the model, then write the index.
    pub fn update<R>(
        &mut self,
        f: impl FnOnce(&mut Coursebot) -> Result<R, CoursebotError>,
    ) -> Result<R, StoreError> {
        let r = f(&mut self.bot)?;
        self.flush()?;
        Ok(r)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let s = serde_json::to_vec_pretty(&self.bot.to_json()).expect("json");
        self.storage.write(INDEX_NAME, &s)
    }

    /// Save course bytes (+ optional PNG thumbnail) into a slot. A save without a thumbnail
    /// removes the old one; any old clear-check replay is removed (the course changed).
    pub fn save(
        &mut self,
        n: CourseNumber,
        meta: CourseMeta,
        course: &[u8],
        thumb_png: Option<&[u8]>,
        confirmed_overwrite: bool,
    ) -> Result<u8, StoreError> {
        let mut probe = self.bot.clone();
        let file = probe.save(n, meta, confirmed_overwrite)?;
        self.storage.write(&store_data_name(file), course)?;
        match thumb_png {
            Some(t) => self.storage.write(&store_thumb_name(file), t)?,
            None => self.storage.remove(&store_thumb_name(file))?,
        }
        self.storage.remove(&store_replay_name(file))?;
        self.bot = probe;
        self.flush()?;
        Ok(file)
    }

    pub fn delete(&mut self, n: CourseNumber) -> Result<(), StoreError> {
        let file = self.bot.delete(n)?;
        self.flush()?;
        for name in [
            store_data_name(file),
            store_thumb_name(file),
            store_replay_name(file),
        ] {
            self.storage.remove(&name)?;
        }
        Ok(())
    }

    /// Drag swap: index only, no file moves.
    pub fn swap(&mut self, a: CourseNumber, b: CourseNumber) -> Result<(), StoreError> {
        self.update(|bot| bot.swap(a, b))
    }

    fn blob(&self, n: CourseNumber, name: fn(u8) -> String) -> io::Result<Option<Vec<u8>>> {
        match self.bot.slot_entry(n) {
            Some(e) if e.occupied => self.storage.read(&name(e.file)),
            _ => Ok(None),
        }
    }

    /// The slot's course bytes (`None` for an empty slot or a missing file — the game builds a
    /// blank course then).
    pub fn course(&self, n: CourseNumber) -> io::Result<Option<Vec<u8>>> {
        self.blob(n, store_data_name)
    }

    pub fn thumbnail(&self, n: CourseNumber) -> io::Result<Option<Vec<u8>>> {
        self.blob(n, store_thumb_name)
    }

    pub fn replay(&self, n: CourseNumber) -> io::Result<Option<Vec<u8>>> {
        self.blob(n, store_replay_name)
    }

    /// Store the clear-check replay of an occupied slot.
    pub fn write_replay(&mut self, n: CourseNumber, replay: &[u8]) -> Result<(), StoreError> {
        let e = self.bot.slot_entry(n).ok_or(CoursebotError::NotASlot(n))?;
        if !e.occupied {
            return Err(CoursebotError::Empty(n).into());
        }
        self.storage.write(&store_replay_name(e.file), replay)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(name: &str) -> CourseMeta {
        CourseMeta::new(name, "WU", 0)
    }

    fn dl(name: &str, code: &str) -> CourseMeta {
        CourseMeta {
            download: Some(DownloadInfo {
                course_code: code.into(),
                maker_name: "maker".into(),
                ..DownloadInfo::default()
            }),
            ..CourseMeta::new(name, "M1", 2)
        }
    }

    #[test]
    fn course_number_space() {
        for raw in 0..=402 {
            let n = CourseNumber::from_raw(raw).expect("valid");
            assert_eq!(n.raw() as i32, raw);
        }
        assert_eq!(CourseNumber::from_raw(-1), None);
        assert_eq!(CourseNumber::from_raw(403), None);
        assert_eq!(
            CourseNumber::from_raw(119),
            Some(CourseNumber::MyCourse(119))
        );
        assert_eq!(
            CourseNumber::from_raw(120),
            Some(CourseNumber::Downloaded(0))
        );
        assert_eq!(
            CourseNumber::from_raw(179),
            Some(CourseNumber::Downloaded(59))
        );
        assert_eq!(CourseNumber::from_raw(180), Some(CourseNumber::Mem));
        assert_eq!(CourseNumber::from_raw(181), Some(CourseNumber::New));
        assert_eq!(CourseNumber::from_raw(182), Some(CourseNumber::Title));
        assert_eq!(CourseNumber::from_raw(183), Some(CourseNumber::Quest(0)));
        assert_eq!(CourseNumber::from_raw(302), Some(CourseNumber::Quest(119)));
        assert_eq!(CourseNumber::from_raw(303), Some(CourseNumber::Lesson(0)));
        assert_eq!(CourseNumber::from_raw(0x133), Some(CourseNumber::Lesson(4)));
        assert_eq!(
            CourseNumber::Quest(7).builtin_stem().as_deref(),
            Some("quest_007")
        );
        assert_eq!(
            CourseNumber::Lesson(49).builtin_stem().as_deref(),
            Some("lesson_049")
        );
        assert_eq!(CourseNumber::MyCourse(3).builtin_stem(), None);
        assert_eq!(CourseNumber::MyCourse(200).raw(), 119);
        assert!(CourseNumber::MyCourse(200).slot().is_none());
        assert_eq!(course_data_name(7), "course_data_007.bcd");
        assert_eq!(course_thumb_name(120), "course_thumb_120.btl");
        assert_eq!(course_replay_name(0), "course_replay_000.dat");
        assert_eq!(title_course_stem(9), "title_course_data_09");
    }

    #[test]
    fn starts_empty_and_tabs() {
        let bot = Coursebot::new();
        assert_eq!(bot.count(Tab::MyCourses), 0);
        assert_eq!(bot.count(Tab::Downloaded), 0);
        let my = bot.list(Tab::MyCourses, None);
        assert_eq!(my.len(), 120);
        assert_eq!(Tab::MyCourses.rows(), 30);
        assert_eq!(Tab::Downloaded.rows(), 15);
        assert!(my.iter().all(|s| s.meta.is_none()));
        let d = bot.list(Tab::Downloaded, None);
        assert_eq!(d.len(), 60);
        assert_eq!(d[0].number.raw(), 120);
        assert_eq!(d[0].file, 120);
        assert_eq!(
            bot.first_free(Tab::MyCourses),
            Some(CourseNumber::MyCourse(0))
        );
        assert_eq!(
            bot.first_free(Tab::Downloaded),
            Some(CourseNumber::Downloaded(0))
        );
    }

    #[test]
    fn save_overwrite_needs_confirm() {
        let mut bot = Coursebot::new();
        let n = CourseNumber::MyCourse(5);
        assert!(!bot.needs_overwrite_confirm(n));
        assert_eq!(bot.save(n, own("a"), false), Ok(5));
        assert!(bot.needs_overwrite_confirm(n));
        assert_eq!(
            bot.save(n, own("b"), false),
            Err(CoursebotError::NeedsOverwriteConfirm(n))
        );
        assert_eq!(bot.meta(n).unwrap().name, "a");
        assert_eq!(bot.save(n, own("b"), true), Ok(5));
        assert_eq!(bot.meta(n).unwrap().name, "b");
        assert_eq!(
            bot.first_free(Tab::MyCourses),
            Some(CourseNumber::MyCourse(0))
        );
        // downloads need download info and a Downloaded slot
        assert_eq!(
            bot.save(CourseNumber::Downloaded(0), own("x"), false),
            Err(CoursebotError::WrongTab(CourseNumber::Downloaded(0)))
        );
        assert_eq!(
            bot.save(n, dl("x", "C"), true),
            Err(CoursebotError::WrongTab(n))
        );
        assert_eq!(
            bot.save(CourseNumber::Mem, own("x"), true),
            Err(CoursebotError::NotASlot(CourseNumber::Mem))
        );
    }

    #[test]
    fn save_new_fills_in_order_until_full() {
        let mut bot = Coursebot::new();
        for i in 0..DOWNLOADED {
            let (n, f) = bot
                .save_new(Tab::Downloaded, dl("d", &format!("C{i}")))
                .unwrap();
            assert_eq!(n, CourseNumber::Downloaded(i as u8));
            assert_eq!(f as usize, MY_COURSES + i);
        }
        assert_eq!(
            bot.save_new(Tab::Downloaded, dl("d", "X")),
            Err(CoursebotError::Full(Tab::Downloaded))
        );
        assert_eq!(bot.find_download("C17"), Some(CourseNumber::Downloaded(17)));
        assert_eq!(bot.later_record(CourseNumber::Downloaded(17)), Some(17));
        assert_eq!(bot.later_record(CourseNumber::MyCourse(1)), None);
    }

    #[test]
    fn delete_frees_slot_and_keeps_file_number() {
        let mut bot = Coursebot::new();
        let n = CourseNumber::MyCourse(0);
        bot.save(n, own("a"), false).unwrap();
        bot.save(CourseNumber::MyCourse(1), own("b"), false)
            .unwrap();
        assert_eq!(bot.delete(n), Ok(0));
        assert!(!bot.is_occupied(n));
        assert!(bot.meta(n).is_none());
        assert_eq!(bot.delete(n), Err(CoursebotError::Empty(n)));
        assert_eq!(bot.first_free(Tab::MyCourses), Some(n));
        assert_eq!(bot.file_of(n), Some(0));
        assert_eq!(bot.count(Tab::MyCourses), 1);
    }

    #[test]
    fn swap_moves_table_entries_not_files() {
        let mut bot = Coursebot::new();
        let (a, b, c) = (
            CourseNumber::MyCourse(0),
            CourseNumber::MyCourse(1),
            CourseNumber::MyCourse(9),
        );
        bot.save(a, own("A"), false).unwrap();
        bot.save(b, own("B"), false).unwrap();
        bot.swap(a, b).unwrap();
        assert_eq!(bot.meta(a).unwrap().name, "B");
        assert_eq!(bot.meta(b).unwrap().name, "A");
        assert_eq!(bot.file_of(a), Some(1));
        assert_eq!(bot.file_of(b), Some(0));
        // drop onto an empty slot = move
        bot.swap(a, c).unwrap();
        assert!(!bot.is_occupied(a));
        assert_eq!(bot.meta(c).unwrap().name, "B");
        assert_eq!(bot.file_of(c), Some(1));
        assert_eq!(bot.file_of(a), Some(9));
        // the next save into the vacated slot uses its (new) file number
        assert_eq!(bot.save(a, own("N"), false), Ok(9));
        // downloads are not dragged
        assert_eq!(
            bot.swap(a, CourseNumber::Downloaded(0)),
            Err(CoursebotError::NotSwappable(a, CourseNumber::Downloaded(0)))
        );
        assert_eq!(follow_swap(a, a, c), c);
        assert_eq!(follow_swap(c, a, c), a);
        assert_eq!(follow_swap(b, a, c), b);
    }

    #[test]
    fn rename_description_world_filter() {
        let mut bot = Coursebot::new();
        let (a, b) = (CourseNumber::MyCourse(2), CourseNumber::MyCourse(3));
        bot.save(a, own("A"), false).unwrap();
        bot.save(b, own("B"), false).unwrap();
        bot.rename(a, "Alpha").unwrap();
        bot.set_description(a, "desc").unwrap();
        assert_eq!(bot.meta(a).unwrap().name, "Alpha");
        assert_eq!(bot.meta(a).unwrap().description, "desc");
        assert_eq!(
            bot.rename(CourseNumber::MyCourse(4), "x"),
            Err(CoursebotError::Empty(CourseNumber::MyCourse(4)))
        );
        bot.set_world(b, Some(2)).unwrap();
        let w = bot.list(Tab::MyCourses, Some(2));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].number, b);
        assert_eq!(w[0].index, 3);
        bot.clear_world(2);
        assert!(bot.list(Tab::MyCourses, Some(2)).is_empty());
        let long = "x".repeat(40);
        let (n, d) = CourseMeta::new(&long, "M1", 0).header_text();
        assert_eq!((n.len(), d.len()), (32, 0));
    }

    #[test]
    fn upload_gate() {
        let mut bot = Coursebot::new();
        let n = CourseNumber::MyCourse(0);
        assert_eq!(bot.upload_check(n, None), Err(UploadBlock::Empty));
        let mut m = own("A");
        m.clear_check = ClearCheck::reset(2);
        bot.save(n, m, false).unwrap();
        assert_eq!(bot.upload_check(n, None), Err(UploadBlock::NeedsClearCheck));
        let cc = &mut bot.meta_mut(n).unwrap().clear_check;
        cc.record(None, true, Some(30_000));
        cc.record(Some(0), true, None);
        cc.record(Some(1), false, None);
        assert!(!cc.passed());
        cc.record(Some(1), true, None);
        cc.record(None, true, Some(25_000));
        assert!(cc.passed());
        assert_eq!(cc.attempts, 5);
        assert_eq!(cc.time_ms, Some(25_000));
        assert_eq!(
            bot.upload_check(n, Some((32, 32))),
            Err(UploadBlock::LimitReached {
                uploaded: 32,
                max: 32
            })
        );
        assert_eq!(bot.upload_check(n, Some((3, 32))), Ok(()));
        bot.set_uploaded(n, Some("ABC-123-DEF".into())).unwrap();
        assert_eq!(bot.uploaded_count(), 1);
        assert_eq!(
            bot.upload_check(n, None),
            Err(UploadBlock::AlreadyUploaded("ABC-123-DEF".into()))
        );
        let d = CourseNumber::Downloaded(0);
        bot.save(d, dl("D", "X"), false).unwrap();
        assert_eq!(bot.upload_check(d, None), Err(UploadBlock::NotOwn));
    }

    #[test]
    fn downloaded_play_stats() {
        let mut bot = Coursebot::new();
        let d = CourseNumber::Downloaded(4);
        bot.save(d, dl("D", "X"), false).unwrap();
        bot.record_downloaded_play(d, PlayOutcome::Quit, 100, 1)
            .unwrap();
        let l = &bot.meta(d).unwrap().download.as_ref().unwrap().local;
        assert_eq!(
            (l.state, l.attempts, l.long_attempts, l.played),
            (PlayState::Played, 0, 0, true)
        );
        bot.record_downloaded_play(d, PlayOutcome::from_exit_reason(0x19), 1801, 1)
            .unwrap();
        bot.record_downloaded_play(d, PlayOutcome::Retry, 1800, 1)
            .unwrap();
        bot.record_downloaded_play(d, PlayOutcome::Death, 5000, 2)
            .unwrap();
        let l = &bot.meta(d).unwrap().download.as_ref().unwrap().local;
        assert_eq!(
            (l.state, l.attempts, l.long_attempts),
            (PlayState::Attempted, 3, 1)
        );
        bot.record_downloaded_play(d, PlayOutcome::from_exit_reason(0x18), 9000, 1)
            .unwrap();
        bot.record_downloaded_play(d, PlayOutcome::Death, 9000, 1)
            .unwrap();
        let l = &bot.meta(d).unwrap().download.as_ref().unwrap().local;
        assert_eq!((l.state, l.attempts), (PlayState::Cleared, 3));
        assert_eq!(
            bot.record_downloaded_play(CourseNumber::MyCourse(0), PlayOutcome::Goal, 1, 1),
            Err(CoursebotError::Empty(CourseNumber::MyCourse(0)))
        );
    }

    #[test]
    fn scene_reasons_and_modes() {
        for r in SceneReason::ALL {
            assert_eq!(SceneReason::from_value(r.value()), Some(r));
        }
        assert_eq!(SceneReason::MyCourseToNormalPlay.value(), 4);
        assert_eq!(SceneReason::ShortCutRobo.value(), 0x1C);
        assert_eq!(SceneReason::from_value(2), None);
        assert_eq!(SceneReason::play(Tab::Downloaded).value(), 5);
        assert_eq!(SceneReason::ShortCutRobo.list_mode(), Some(ListMode::Save));
        assert_eq!(
            SceneReason::NetMainToRoboForSave.list_mode(),
            Some(ListMode::Download(Tab::Downloaded))
        );
        assert_eq!(SceneReason::RoboToEdit.list_mode(), None);
        assert_eq!(ListMode::Save.header_label(), Some("0001"));
        assert_eq!(
            ListMode::Download(Tab::Downloaded).initial_tab(),
            Tab::Downloaded
        );
        assert!(!ListMode::Save.tabs_switchable());
    }

    fn sample() -> Coursebot {
        let mut bot = Coursebot::new();
        let mut a = own("A");
        a.description = "first".into();
        a.clear_check = ClearCheck::reset(1);
        a.clear_check.record(None, true, Some(1234));
        a.uploaded = Some("CODE".into());
        a.saved_at = 1_700_000_000;
        a.world = Some(1);
        bot.save(CourseNumber::MyCourse(0), a, false).unwrap();
        bot.save(CourseNumber::MyCourse(7), own("B"), false)
            .unwrap();
        bot.swap(CourseNumber::MyCourse(0), CourseNumber::MyCourse(50))
            .unwrap();
        let mut d = dl("D", "XYZ");
        let info = d.download.as_mut().unwrap();
        info.server = ServerStats {
            likes: 3,
            plays: 10,
            clears: 2,
            attempts: 40,
            difficulty: 2,
            tags: vec![1, 5],
            world_record: Some(WorldRecord {
                maker_name: "wr".into(),
                maker_id: "id".into(),
                time_ms: 4567,
            }),
        };
        bot.save(CourseNumber::Downloaded(3), d, false).unwrap();
        bot.record_downloaded_play(CourseNumber::Downloaded(3), PlayOutcome::Death, 2000, 1)
            .unwrap();
        bot
    }

    #[test]
    fn json_round_trip() {
        let bot = sample();
        let back = Coursebot::from_json(&bot.to_json());
        assert_eq!(back, bot);
        assert_eq!(back.file_of(CourseNumber::MyCourse(50)), Some(0));
        assert_eq!(
            back.meta(CourseNumber::Downloaded(3))
                .unwrap()
                .download
                .as_ref()
                .unwrap()
                .server
                .clear_rate(),
            5.0
        );
        let s = serde_json::to_string(&bot.to_json()).unwrap();
        assert_eq!(Coursebot::from_json_str(&s), bot);
    }

    #[test]
    fn json_is_robust() {
        assert_eq!(Coursebot::from_json(&json!({})), Coursebot::new());
        assert_eq!(Coursebot::from_json_str("not json"), Coursebot::new());
        // a broken table (duplicate file) falls back to identity but keeps occupancy
        let mut v = sample().to_json();
        v["table"][1] = json!([50, false]);
        let bot = Coursebot::from_json(&v);
        assert_eq!(bot.file_of(CourseNumber::MyCourse(50)), Some(50));
        assert!(bot.is_occupied(CourseNumber::MyCourse(50)));
        // occupied slot without metadata gets a default; orphan metadata is dropped
        let v = json!({
            "table": (0..SLOTS).map(|i| json!([i, i == 2 || i == 130])).collect::<Vec<_>>(),
            "files": { "5": { "name": "orphan" }, "2": { "name": 7 } },
        });
        let bot = Coursebot::from_json(&v);
        assert_eq!(bot.meta(CourseNumber::MyCourse(2)).unwrap().name, "");
        assert!(
            bot.meta(CourseNumber::Downloaded(10))
                .unwrap()
                .download
                .is_some()
        );
        assert_eq!(bot.to_json()["files"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn store_keeps_files_by_number() {
        let mut st = CoursebotStore::open(MemStore::default()).unwrap();
        let (a, b) = (CourseNumber::MyCourse(0), CourseNumber::MyCourse(1));
        st.save(a, own("A"), b"course-a", Some(b"png-a"), false)
            .unwrap();
        st.save(b, own("B"), b"course-b", None, false).unwrap();
        st.write_replay(a, b"replay-a").unwrap();
        assert!(matches!(
            st.save(a, own("X"), b"x", None, false),
            Err(StoreError::Coursebot(
                CoursebotError::NeedsOverwriteConfirm(_)
            ))
        ));
        assert_eq!(st.course(a).unwrap().as_deref(), Some(&b"course-a"[..]));
        st.swap(a, b).unwrap();
        assert_eq!(st.course(a).unwrap().as_deref(), Some(&b"course-b"[..]));
        assert_eq!(st.course(b).unwrap().as_deref(), Some(&b"course-a"[..]));
        assert_eq!(st.thumbnail(b).unwrap().as_deref(), Some(&b"png-a"[..]));
        assert_eq!(st.replay(b).unwrap().as_deref(), Some(&b"replay-a"[..]));
        assert!(st.storage.blobs.contains_key("course_data_000.bcd"));
        assert!(st.storage.blobs.contains_key("course_thumb_000.png"));
        // overwrite drops the stale replay
        st.save(b, own("A2"), b"course-a2", None, true).unwrap();
        assert_eq!(st.replay(b).unwrap(), None);
        assert_eq!(st.thumbnail(b).unwrap(), None);
        st.delete(a).unwrap();
        assert_eq!(st.course(a).unwrap(), None);
        assert!(!st.storage.blobs.contains_key("course_data_001.bcd"));
        // reopen from the index
        let reopened = CoursebotStore::open(st.storage.clone()).unwrap();
        assert_eq!(reopened.bot(), st.bot());
        assert_eq!(reopened.bot().meta(b).unwrap().name, "A2");
    }

    #[test]
    fn dir_store_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "mmk-coursebot-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut st = CoursebotStore::open_dir(&dir).unwrap();
            st.save(CourseNumber::MyCourse(3), own("A"), b"data", None, false)
                .unwrap();
        }
        let st = CoursebotStore::open_dir(&dir).unwrap();
        assert_eq!(st.bot().meta(CourseNumber::MyCourse(3)).unwrap().name, "A");
        assert_eq!(
            st.course(CourseNumber::MyCourse(3)).unwrap().as_deref(),
            Some(&b"data"[..])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
