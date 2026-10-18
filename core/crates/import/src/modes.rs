//! Import stage "modes": the data Story Mode and Yamamura's Dojo run on, from the owner's RomFS.
//!
//! - `eventflow/<name>.bfevfl`: the 23 Story flows of `Pack/QuestEventFlow.pack` and the lesson
//!   flows (and `quest_Staffroll`) of `EventFlow/`, copied as they are after checking they parse;
//! - `story/jobs.json`, `builds.json`, `flags.json`: `Mush/QuestCourseManager`,
//!   `QuestBuildManager`, `QuestFlagList` with English field names, the job type / difficulty /
//!   reward decoded, and each job's clear condition from `CourseInfo/QuestCourseGoalCondition`;
//! - `story/main_field.json`: the hub map `Map/MainField` (objects, gates, flows);
//! - `story/index.json`: the flow lists;
//! - `lessons/index.json`: the Dojo catalogue from `Mush/Lesson*` (Maker Lessons with their level
//!   from the sorting table, the power-up lessons with their form, Maker Basics, the demo replay
//!   names), listing only lessons whose course and flow exist.
//!
//! The tables' Japanese column names and enum values are matched by their CRC32, so no game text
//! is in this source. The format of the output is read by `mmk_story::data` and
//! `mmk_modes::dojo`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use mmk_formats::archive::{self, Sarc};
use mmk_formats::{bfevfl, byml};
use serde_json::{Value, json};

use crate::{ImportError, Result, fmt_err, is_file, open_sarc, read, read_dir, write};

/// Version of the files this stage writes.
pub const MODES_VERSION: u32 = 1;

/// What the stage wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModesReport {
    pub story_flows: usize,
    pub lesson_flows: usize,
    pub jobs: usize,
    pub builds: usize,
    pub flags: usize,
    pub hub_objects: usize,
    pub lessons: usize,
}

// QuestCourseManager columns (CRC32 of the column name)
const JOB_TYPE: u32 = 0xd0f3_3999;
const JOB_TITLE: u32 = 0x95d8_33ea;
const JOB_SKIN: u32 = 0x4cd4_2d15;
const JOB_DETAIL: u32 = 0x9cc0_6c53;
const JOB_CLIENT: u32 = 0xd2ce_4add;
const JOB_REWARD: u32 = 0x1d05_f519;
const DAYS: u32 = 0x17a7_5d40;
const JOB_RETURN_TAG: u32 = 0x5e53_5ce9;
const JOB_DIFFICULTY: u32 = 0xc042_b345;
// job type values: normal, hidden, key
const TYPE_VALUES: [(u32, &str); 3] = [
    (0xa51d_57fe, "normal"),
    (0xb103_0c49, "hidden"),
    (0x2ce8_b073, "key"),
];
// difficulty values: Easy … Very Hard
const DIFFICULTY_VALUES: [u32; 4] = [0x12b3_0bc4, 0x5bd6_d379, 0x1e9e_de42, 0x5a67_abef];
// QuestBuildManager columns
const BUILD_LABEL: u32 = 0x2c40_a8d7;
const BUILD_COINS: u32 = 0x766f_3cb1;
// QuestFlagList columns and kinds
const FLAG_NAME: u32 = 0x8a86_0d66;
const FLAG_KIND: u32 = 0x51f8_e83c;
const FLAG_KINDS: [(u32, &str); 4] = [
    (0x5f8f_1940, "talk"),
    (0x45cc_3484, "move"),
    (0x7ad1_47d0, "appear"),
    (0xc42a_ea71, "daily"),
];
/// Power-up lesson forms by lesson number (Raccoon, Cape, Propeller, Cat; coursebot.md §B.2).
const PLAYER_LESSON_FORMS: [i64; 4] = [5, 4, 3, 7];

fn crc(s: &str) -> u32 {
    crc32fast::hash(s.as_bytes())
}

/// The value of the column whose name has CRC32 `key`.
fn col(row: &Value, key: u32) -> &Value {
    row.as_object()
        .and_then(|o| o.iter().find(|(k, _)| crc(k) == key).map(|(_, v)| v))
        .unwrap_or(&Value::Null)
}

fn col_str(row: &Value, key: u32) -> String {
    match col(row, key) {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn rows(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// A table of `Mush/` in `Pack/Static.pack`.
fn mush(pack: &Sarc, name: &str, ctx: &Path) -> Result<Value> {
    let path = format!("Mush/{name}.byml");
    let data = pack
        .get(&path)
        .ok_or_else(|| ImportError::Invalid(format!("Static.pack has no {path}")))?;
    byml::parse(data).map_err(fmt_err(&ctx.join(path)))
}

/// The first BYML of a Yaz0 / SARC wrapped file (`.szs`), or the file itself.
fn byml_file(path: &Path) -> Result<Value> {
    let data = archive::unwrap(read(path)?).map_err(fmt_err(path))?;
    if data.starts_with(b"SARC") {
        let sarc = Sarc::parse(data).map_err(fmt_err(path))?;
        let (_, inner) = sarc
            .files()
            .find(|(n, _)| n.ends_with(".byml") || n.ends_with(".byaml"))
            .ok_or_else(|| ImportError::Invalid(format!("{}: no BYML inside", path.display())))?;
        return byml::parse(inner).map_err(fmt_err(path));
    }
    byml::parse(&data).map_err(fmt_err(path))
}

fn style(skin: &str) -> &str {
    match skin {
        "MU" => "WU",
        "3DW" => "3W",
        s => s,
    }
}

fn reward(text: &str) -> Value {
    match text {
        "LAND_IN" => json!({ "land": "in" }),
        "LAND_OUT" => json!({ "land": "out" }),
        t => match t.parse::<i64>() {
            Ok(n) => json!({ "coins": n }),
            Err(_) => json!({ "gear": t }),
        },
    }
}

fn jobs(table: &Value, goals: &Value) -> Vec<Value> {
    let goal_of = |q: i64| {
        rows(goals)
            .iter()
            .find(|g| g["QuestNo"].as_i64() == Some(q))
            .map(|g| {
                json!({
                    "category": g["Category"].as_str().unwrap_or("None"),
                    "target": g["Target"].as_str().unwrap_or("None"),
                    "count": g["SuccessfulCount"].as_i64().unwrap_or(0),
                    "full": g["IsFull"].as_bool().unwrap_or(false),
                })
            })
            .unwrap_or_else(|| {
                json!({ "category": "None", "target": "None", "count": 0,
                                       "full": false })
            })
    };
    let mut out: Vec<Value> = rows(table)
        .iter()
        .map(|r| {
            let no = r["QuestNo"].as_i64().unwrap_or(0);
            let course_no = r["CourseNo"].as_i64().unwrap_or(0);
            let kind_crc = crc(&col_str(r, JOB_TYPE));
            let kind = TYPE_VALUES
                .iter()
                .find(|(c, _)| *c == kind_crc)
                .map_or("normal", |(_, k)| k);
            let diff_crc = crc(&col_str(r, JOB_DIFFICULTY));
            let difficulty = DIFFICULTY_VALUES
                .iter()
                .position(|&c| c == diff_crc)
                .unwrap_or(1);
            json!({
                "no": no,
                "course_no": course_no,
                "course": format!("quest_{course_no:03}"),
                "kind": kind,
                "style": style(&col_str(r, JOB_SKIN)),
                "difficulty": difficulty,
                "days": col(r, DAYS).as_i64().unwrap_or(1),
                "reward": reward(&col_str(r, JOB_REWARD)),
                "giver": r["NPCName"].as_str().unwrap_or(""),
                "return_tag": col_str(r, JOB_RETURN_TAG),
                "title": col_str(r, JOB_TITLE),
                "client": col_str(r, JOB_CLIENT),
                "detail": col_str(r, JOB_DETAIL),
                "goal": goal_of(no),
            })
        })
        .collect();
    out.sort_by_key(|j| j["no"].as_i64());
    out
}

fn builds(table: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = rows(table)
        .iter()
        .map(|r| {
            json!({
                "no": r["BuildNo"].as_i64().unwrap_or(0),
                "cost": col(r, BUILD_COINS).as_i64().unwrap_or(0),
                "days": col(r, DAYS).as_i64().unwrap_or(1),
                "label": col_str(r, BUILD_LABEL),
            })
        })
        .collect();
    out.sort_by_key(|b| b["no"].as_i64());
    out
}

fn flags(table: &Value) -> Vec<Value> {
    rows(table)
        .iter()
        .filter_map(|r| {
            let name = col_str(r, FLAG_NAME);
            let k = crc(&col_str(r, FLAG_KIND));
            let kind = FLAG_KINDS.iter().find(|(c, _)| *c == k)?.1;
            (!name.is_empty()).then(|| json!({ "name": name, "kind": kind }))
        })
        .collect()
}

/// Maker Lessons with their level (group of the sorting table), power-up lessons, Maker Basics.
fn lessons(
    pack: &Sarc,
    ctx: &Path,
    have_course: &dyn Fn(i64) -> bool,
    have_flow: &dyn Fn(&str) -> bool,
) -> Result<(Value, usize)> {
    let technique = mush(pack, "LessonTechnique", ctx)?;
    let sorting = mush(pack, "LessonTechniqueSorting", ctx)?;
    let player = mush(pack, "LessonPlayerOperation", ctx)?;
    let tutorial = mush(pack, "LessonFirstTutorial", ctx)?;
    let replays = mush(pack, "LessonKeyReplay", ctx)?;
    // an empty name starts the next group
    let mut level = BTreeMap::new();
    let mut group = -1i64;
    for r in rows(&sorting) {
        match r["name"].as_str().unwrap_or("") {
            "" => group += 1,
            n => {
                level.insert(n.to_string(), group.max(0));
            }
        }
    }
    let entry = |r: &Value, flow: String| {
        let course_no = r["course_no"].as_i64().unwrap_or(-1);
        json!({
            "lesson_no": r["lesson_no"].as_i64().unwrap_or(0),
            "course_no": course_no,
            "course": format!("lesson_{course_no:03}"),
            "flow": flow,
        })
    };
    let usable = |e: &Value| {
        have_course(e["course_no"].as_i64().unwrap_or(-1))
            && have_flow(e["flow"].as_str().unwrap_or(""))
    };
    let mut tech = Vec::new();
    for r in rows(&technique) {
        let no = r["lesson_no"].as_i64().unwrap_or(0);
        let mut e = entry(r, format!("lesson_technique{no:03}"));
        let Some(&lv) = level.get(r["name"].as_str().unwrap_or("")) else {
            continue; // not on the list (the unused rows past 44)
        };
        if !usable(&e) {
            continue;
        }
        e["title_label"] = json!(r["data_name"].as_str().unwrap_or(""));
        e["level"] = json!(lv);
        tech.push(e);
    }
    let mut play = Vec::new();
    for r in rows(&player) {
        let no = r["lesson_no"].as_i64().unwrap_or(0);
        let mut e = entry(r, format!("lesson_player{no:03}"));
        if !usable(&e) {
            continue;
        }
        e["form"] = json!(PLAYER_LESSON_FORMS.get(no as usize).copied().unwrap_or(0));
        play.push(e);
    }
    let tut: Vec<Value> = rows(&tutorial)
        .iter()
        .map(|r| {
            let no = r["lesson_no"].as_i64().unwrap_or(0);
            entry(r, format!("lesson_tutorial{no:03}"))
        })
        .filter(|e| usable(e))
        .collect();
    let keys: serde_json::Map<String, Value> = rows(&replays)
        .iter()
        .filter_map(|r| {
            Some((
                r["key_replay_no"].as_i64()?.to_string(),
                json!(r["key_replay_name"].as_str()?),
            ))
        })
        .collect();
    let count = tech.len() + play.len() + tut.len();
    Ok((
        json!({ "version": MODES_VERSION, "technique": tech, "player": play, "tutorial": tut,
                "key_replays": keys }),
        count,
    ))
}

fn flow_name(member: &str) -> Option<String> {
    let file = member.rsplit('/').next()?;
    file.strip_suffix(".bfevfl").map(str::to_string)
}

/// Run the stage: read from `romfs`, write into the cache folder `out` (`<root>/smm2-<build>`).
pub fn import_modes(romfs: &Path, out: &Path) -> Result<ModesReport> {
    let static_path = romfs.join("Pack/Static.pack");
    let pack = open_sarc(&static_path)?;
    let mut report = ModesReport::default();

    // event flows
    let mut story_flows = Vec::new();
    let mut lesson_flows = Vec::new();
    let mut add_flow = |name: String, data: &[u8], ctx: &Path| -> Result<()> {
        bfevfl::parse(data).map_err(fmt_err(ctx))?;
        write(&out.join(format!("eventflow/{name}.bfevfl")), data)?;
        if name.starts_with("lesson_") {
            lesson_flows.push(name);
        } else {
            story_flows.push(name);
        }
        Ok(())
    };
    let qpack = romfs.join("Pack/QuestEventFlow.pack");
    if is_file(&qpack) {
        for (member, data) in open_sarc(&qpack)?.files() {
            if let Some(name) = flow_name(member) {
                add_flow(name, data, &qpack.join(member))?;
            }
        }
    }
    let dir = romfs.join("EventFlow");
    if is_file(&dir.join("lesson_subflows.bfevfl")) || is_file(&dir.join("quest_Staffroll.bfevfl"))
    {
        for p in read_dir(&dir)? {
            let name = p.file_name().and_then(|f| flow_name(&f.to_string_lossy()));
            if let Some(name) = name {
                add_flow(name, &read(&p)?, &p)?;
            }
        }
    }
    story_flows.sort();
    story_flows.dedup();
    lesson_flows.sort();
    lesson_flows.dedup();
    if story_flows.is_empty() {
        return Err(ImportError::Invalid(
            "no Story event flows (Pack/QuestEventFlow.pack)".into(),
        ));
    }
    report.story_flows = story_flows.len();
    report.lesson_flows = lesson_flows.len();
    let index = json!({ "version": MODES_VERSION, "flows": story_flows,
                        "lesson_flows": lesson_flows });
    let pretty = |v: &Value| serde_json::to_vec_pretty(v).expect("serialise");
    write(&out.join("story/index.json"), &pretty(&index))?;

    // tables
    let goal_path = romfs.join("CourseInfo/QuestCourseGoalCondition.szs");
    let goals = if is_file(&goal_path) {
        byml_file(&goal_path)?
    } else {
        Value::Null
    };
    let j = jobs(&mush(&pack, "QuestCourseManager", &static_path)?, &goals);
    let bl = builds(&mush(&pack, "QuestBuildManager", &static_path)?);
    let fl = flags(&mush(&pack, "QuestFlagList", &static_path)?);
    (report.jobs, report.builds, report.flags) = (j.len(), bl.len(), fl.len());
    write(&out.join("story/jobs.json"), &pretty(&json!(j)))?;
    write(&out.join("story/builds.json"), &pretty(&json!(bl)))?;
    write(&out.join("story/flags.json"), &pretty(&json!(fl)))?;

    // hub map
    let map_path = romfs.join("Map/MainField.szs");
    if is_file(&map_path) {
        let map = byml_file(&map_path)?;
        report.hub_objects = map["Objs"].as_array().map_or(0, Vec::len);
        write(
            &out.join("story/main_field.json"),
            &serde_json::to_vec(&map).expect("serialise"),
        )?;
    }

    // lessons
    let courses: HashSet<i64> = read_dir(&romfs.join("Course"))?
        .iter()
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            stem.strip_prefix("lesson_")?.parse().ok()
        })
        .collect();
    let flows: HashSet<&str> = index["lesson_flows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let (lessons, n) = lessons(&pack, &static_path, &|c| courses.contains(&c), &|f| {
        flows.contains(f)
    })?;
    report.lessons = n;
    write(&out.join("lessons/index.json"), &pretty(&lessons))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_by_crc_and_values() {
        let row = json!({ "QuestNo": 2, "CourseNo": 29, "x": "y" });
        assert_eq!(col(&row, crc("x")), "y");
        assert_eq!(col(&row, 1), &Value::Null);
        assert_eq!(reward("150"), json!({ "coins": 150 }));
        assert_eq!(reward("LAND_IN"), json!({ "land": "in" }));
        assert_eq!(reward("Some_Gear"), json!({ "gear": "Some_Gear" }));
        assert_eq!((style("MU"), style("3DW"), style("M1")), ("WU", "3W", "M1"));
        assert_eq!(
            flow_name("EventFlow/quest_Frog.bfevfl").as_deref(),
            Some("quest_Frog")
        );
    }

    /// The owner's RomFS (skipped without one): a scratch import has the expected shape.
    #[test]
    fn owner_import() {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let Some(romfs) = std::env::var_os("MMK_ROMFS")
            .map(std::path::PathBuf::from)
            .into_iter()
            .chain([
                "/Volumes/SSD/smm-work/extracted/smm2-update-v303/romfs".into(),
                home.join("smm-work/extracted/smm2-v303-romfs"),
            ])
            .find(|p| p.join("Pack/QuestEventFlow.pack").is_file())
        else {
            eprintln!("skipped: no RomFS");
            return;
        };
        let out = std::env::temp_dir().join(format!("mmk-modes-import-{}", std::process::id()));
        let r = import_modes(&romfs, &out).unwrap();
        assert_eq!((r.jobs, r.builds), (120, 16));
        assert!(r.story_flows >= 23, "{r:?}");
        assert_eq!(r.lesson_flows, 51);
        assert_eq!(
            r.lessons, 50,
            "45 Maker Lessons + 4 power-up lessons + Maker Basics"
        );
        assert_eq!(r.flags, 232);
        let jobs: Value =
            serde_json::from_slice(&std::fs::read(out.join("story/jobs.json")).unwrap()).unwrap();
        let kinds = |k: &str| rows(&jobs).iter().filter(|j| j["kind"] == k).count();
        assert_eq!(
            (kinds("normal"), kinds("hidden"), kinds("key")),
            (90, 24, 6)
        );
        let first_clear: i64 = rows(&jobs)
            .iter()
            .filter_map(|j| j["reward"]["coins"].as_i64())
            .sum();
        assert_eq!(first_clear, 15_800);
        let builds: Value =
            serde_json::from_slice(&std::fs::read(out.join("story/builds.json")).unwrap()).unwrap();
        let cost: i64 = rows(&builds)
            .iter()
            .map(|b| b["cost"].as_i64().unwrap())
            .sum();
        let days: i64 = rows(&builds)
            .iter()
            .map(|b| b["days"].as_i64().unwrap())
            .sum();
        assert_eq!((cost, days), (14_100, 61));
        let lessons: Value =
            serde_json::from_slice(&std::fs::read(out.join("lessons/index.json")).unwrap())
                .unwrap();
        let per_level = |l: i64| {
            rows(&lessons["technique"])
                .iter()
                .filter(|e| e["level"] == l)
                .count()
        };
        assert_eq!((per_level(0), per_level(1), per_level(2)), (15, 15, 15));
        let _ = std::fs::remove_dir_all(&out);
    }
}
