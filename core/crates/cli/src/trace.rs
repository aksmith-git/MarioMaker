//! `mmk trace`: run a per-frame input script through the simulation and write the player's state
//! after every frame as CSV, for frame-by-frame comparison with traces recorded elsewhere (e.g. an
//! emulator). `mmk trace-courses`: the small physics test courses those scripts run on.
//!
//! Script format (one line per run of frames, `#` starts a comment):
//!
//! ```text
//! 20 -            # 20 frames, nothing held
//! 40 RIGHT RUN    # 40 frames of Right + Run
//! 1 RIGHT JUMP
//! ```
//!
//! Buttons: LEFT RIGHT UP DOWN JUMP RUN SPIN (pad names are accepted too: A/B = JUMP,
//! X/Y = RUN, ZL/ZR = SPIN).

use std::path::Path;

use mmk_editor::Editor;
use mmk_formats::archive::sarc;
use mmk_formats::smm2::crypto::{self, KeyTable};
use mmk_sim::input::{self, Input};
use mmk_sim::player::Form;

/// Parse a script into one input per frame.
pub fn parse_script(text: &str) -> Result<Vec<Input>, String> {
    let mut out = Vec::new();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let n: usize = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("line {}: expected a frame count", ln + 1))?;
        let mut b = 0u32;
        for tok in it {
            b |= match tok.to_ascii_uppercase().as_str() {
                "-" => 0,
                "LEFT" => input::LEFT,
                "RIGHT" => input::RIGHT,
                "UP" => input::UP,
                "DOWN" => input::DOWN,
                "JUMP" | "A" | "B" => input::JUMP,
                "RUN" | "X" | "Y" => input::RUN,
                "SPIN" | "ZL" | "ZR" => input::SPIN,
                other => return Err(format!("line {}: unknown button {other}", ln + 1)),
            };
        }
        out.extend(std::iter::repeat_n(Input { buttons: b }, n));
    }
    Ok(out)
}

pub fn parse_form(s: &str) -> Result<Form, String> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "small" => Form::Small,
        "super" => Form::Super,
        "fire" => Form::Fire,
        "raccoon" => Form::Raccoon,
        "cape" => Form::Cape,
        "propeller" => Form::Propeller,
        "cat" => Form::Cat,
        "builder" => Form::Builder,
        "superball" => Form::Superball,
        _ => return Err(format!("unknown form {s}")),
    })
}

pub struct Start {
    pub pos: Option<(f32, f32)>,
    pub facing: Option<i32>,
    pub form: Option<Form>,
    /// Start speed (px/frame, signed), e.g. when the recorded run starts in motion.
    pub vx: Option<f32>,
}

/// Run the script; one CSV row per frame (the state after that frame's step).
pub fn run(course: &[u8], script: &[Input], start: &Start) -> Result<String, String> {
    let c = mmk_formats::smm2::Course::parse(course).map_err(|e| e.to_string())?;
    let mut w = mmk_sim::World::from_course(&c);
    if let Some((x, y)) = start.pos {
        w.player.x = x;
        w.player.y = y;
    }
    if let Some(f) = start.facing {
        w.player.facing = f;
    }
    if let Some(f) = start.form {
        w.player.form = f;
    }
    if let Some(v) = start.vx {
        w.player.vx = v;
    }
    w.reset_camera();
    let mut csv = String::from("frame,x,y,vx,vy,facing,ground,pose,form,input\n");
    for (i, inp) in script.iter().enumerate() {
        w.step(*inp);
        let p = &w.player;
        csv.push_str(&format!(
            "{i},{},{},{},{},{},{},{:?},{:?},{}\n",
            p.x, p.y, p.vx, p.vy, p.facing, p.on_ground as u8, p.pose, p.form, inp.buttons
        ));
    }
    Ok(csv)
}

// ---------------------------------------------------------------------------------------------
// Physics test courses

pub const STYLES: [&str; 5] = ["M1", "M3", "MW", "WU", "3W"];
pub const KINDS: [&str; 6] = ["flat", "wall", "slope", "water", "items", "form"];

/// All test course names: `<style>_<kind>`.
pub fn course_names() -> Vec<String> {
    let mut v = Vec::new();
    for s in STYLES {
        for k in KINDS {
            v.push(format!("{s}_{k}"));
        }
    }
    v
}

/// Build one test course. Every course starts on the start ground with a long flat floor to
/// the right (one tile high; two in 3D World, whose start ground can't be lower); the kind adds
/// the feature under test further along.
pub fn build_course(name: &str) -> Result<Vec<u8>, String> {
    let (style, kind) = name.split_once('_').ok_or("name is <style>_<kind>")?;
    if !STYLES.contains(&style) || !KINDS.contains(&kind) {
        return Err(format!("unknown test course {name}"));
    }
    let err = |x: mmk_editor::EditError| format!("{x:?}");
    let len = 120;
    // floor height in tiles: the game rejects a 3D World course with start height 1
    let base: i32 = if style == "3W" { 2 } else { 1 };
    let mut e = Editor::new(style);
    e.set_name("Physics");
    e.set_theme(if kind == "water" { 4 } else { 0 });
    e.set_timer(500);
    e.set_length(len);
    e.set_start_goal(base as u8, base as u8);
    // header fields a course made in the game has (the game rejects a course without them and
    // plays a blank one instead) [obs: the game's own title courses, checked in an emulator]
    {
        let h = &mut e.course.header;
        (h.year, h.month, h.day, h.hour, h.minute) = (2019, 4, 16, 16, 5);
        h.game_version = 1;
        h.management_flags = 1;
        h.clear_check_time = u32::MAX;
        h.creation_id = 1;
        // goal x in tenths of a tile: 9.5 tiles left of the right boundary (as in the game's
        // own courses, e.g. 182 tiles → 1725)
        h.goal_x = (len * 10 - 95) as i16;
    }
    e.course.main.header.boundary_type = 1;
    if style == "3W" {
        // as in the game's own 3D World courses: both liquid heights 1, boundary types 0 / 1
        for a in [&mut e.course.main.header, &mut e.course.sub.header] {
            a.liquid_start_height = 1;
            a.liquid_end_height = 1;
        }
        e.course.main.header.boundary_type = 0;
        e.course.sub.header.boundary_type = 1;
    }
    // the start ground covers x < 7 and the goal ground the last 11 columns; ground overlapping
    // either makes the game reject the course
    let floor = |e: &mut Editor, x0: i32, x1: i32| -> Result<(), String> {
        for x in x0.max(7)..=x1.min(len - 12) {
            for y in 0..base {
                e.paint_ground(x, y).map_err(err)?;
            }
        }
        Ok(())
    };
    // rows above the floor
    let up = |y: i32| y + base - 1;
    match kind {
        "flat" => {
            // a 3-wide pit far to the right (x 90..92) for running off a ledge
            floor(&mut e, 0, 89)?;
            floor(&mut e, 93, len - 1)?;
        }
        "water" => floor(&mut e, 0, len - 1)?,
        "items" => {
            // a Super Mushroom on the floor just ahead of the start (walk into it to grow)
            floor(&mut e, 0, len - 1)?;
            e.place(20, 10, base).map_err(err)?;
        }
        "form" => {
            // the style's own power-up (part 44: leaf, feather, propeller, bell; a mushroom in
            // SMB1) just ahead of the start
            floor(&mut e, 0, len - 1)?;
            e.place(44, 10, base).map_err(err)?;
        }
        "wall" => {
            // floor, then a 12-tall wall at x 40 and a second one at x 46 (a 5-wide shaft for
            // wall jumps), a low ceiling at x 20..26, 3 tiles up
            floor(&mut e, 0, len - 1)?;
            for y in 1..=12 {
                e.paint_ground(40, up(y)).map_err(err)?;
                e.paint_ground(46, up(y)).map_err(err)?;
            }
            for x in 20..=26 {
                e.paint_ground(x, up(3)).map_err(err)?;
            }
        }
        "slope" => {
            // all four slope kinds with their bottom row in the floor's top row (a slope's low
            // end is a tile above its bottom row): floor to x 23, gentle up (8×4), plateau,
            // steep down (5×4), floor x 45..49, steep up, plateau, gentle down; orientation
            // (bits 20–21): 0 rises to the right, 3 falls
            let rise = mmk_editor::DEFAULT_FLAGS;
            let fall = mmk_editor::DEFAULT_FLAGS | (3 << 20);
            let row = base - 1;
            floor(&mut e, 0, 23)?;
            e.place_sized(87, 24, row, 8, 4, rise).map_err(err)?;
            for x in (32..=39).chain(55..=59) {
                for y in 0..=up(3) {
                    e.paint_ground(x, y).map_err(err)?;
                }
            }
            e.place_sized(88, 40, row, 5, 4, fall).map_err(err)?;
            floor(&mut e, 45, 49)?;
            e.place_sized(88, 50, row, 5, 4, rise).map_err(err)?;
            e.place_sized(87, 60, row, 8, 4, fall).map_err(err)?;
            floor(&mut e, 68, len - 1)?;
        }
        _ => unreachable!(),
    }
    // under a slope's bottom row in 3D World (base 2): fill the floor row below it
    if base == 2 && kind == "slope" {
        for x in (24..=31).chain(40..=44).chain(50..=54).chain(60..=67) {
            e.paint_ground(x, 0).map_err(err)?;
        }
    }
    e.save().map_err(err)
}

/// Encrypt a course and wrap it like the game's built-in courses (Yaz0 → SARC → `<stem>.bcd`).
pub fn szs(course: &[u8], table: &KeyTable, stem: &str) -> Result<Vec<u8>, String> {
    let iv = [0x5Au8; 16];
    let state = [0x1234_5678, 0x9ABC_DEF0, 0x0F1E_2D3C, 0x4B5A_6978];
    let bcd = crypto::encrypt_bcd(course, table, crypto::default_header(), iv, state)
        .map_err(|e| e.to_string())?;
    let name = format!("{stem}.bcd");
    let arc = sarc::build(&[(name.as_str(), &bcd)]);
    Ok(yaz0_store(&align_data(&arc, 0x100)))
}

/// Move a SARC's data section to `offset` (the game's own course archives keep it at 0x100).
fn align_data(arc: &[u8], offset: usize) -> Vec<u8> {
    let old = u32::from_le_bytes(arc[0x0C..0x10].try_into().unwrap()) as usize;
    if old >= offset {
        return arc.to_vec();
    }
    let mut out = arc[..old].to_vec();
    out.resize(offset, 0);
    out.extend_from_slice(&arc[old..]);
    let len = out.len() as u32;
    out[0x08..0x0C].copy_from_slice(&len.to_le_bytes());
    out[0x0C..0x10].copy_from_slice(&(offset as u32).to_le_bytes());
    out
}

/// Yaz0 without back-references (every group is 8 literal bytes): valid for any reader.
fn yaz0_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + data.len() * 9 / 8 + 1);
    out.extend_from_slice(b"Yaz0");
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0u8; 8]);
    for chunk in data.chunks(8) {
        out.push(0xFF);
        out.extend_from_slice(chunk);
    }
    out
}

/// Write `<name>.course` for each selected course; with `title_dir` (needs the key table), also
/// the last one as the ten replacement title courses `title_course_data_00…09.szs` (the title
/// screen plays one of them, so an emulator with a file-replacement mod boots straight into it).
pub fn write_courses(
    out: &Path,
    only: Option<&str>,
    table: Option<&KeyTable>,
    title_dir: Option<&Path>,
) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut last = None;
    // `only` may also name a raw course file (any course becomes the title course)
    if let Some(path) = only.filter(|o| Path::new(o).is_file()) {
        last = Some(std::fs::read(path).map_err(|e| e.to_string())?);
    }
    for name in course_names() {
        if last.is_some() && only.is_some_and(|o| Path::new(o).is_file()) {
            break;
        }
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let c = build_course(&name)?;
        std::fs::write(out.join(format!("{name}.course")), &c).map_err(|e| e.to_string())?;
        println!("{name}");
        last = Some(c);
    }
    if let Some(dir) = title_dir {
        let table = table.ok_or("--title needs --table")?;
        let c = last.ok_or("no course selected")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for i in 0..10 {
            let stem = format!("title_course_data_{i:02}");
            std::fs::write(dir.join(format!("{stem}.szs")), szs(&c, table, &stem)?)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_expand_per_frame() {
        let s = parse_script("2 -\n# comment\n3 RIGHT Y  # run\n1 right a\n").unwrap();
        assert_eq!(s.len(), 6);
        assert_eq!(s[0].buttons, 0);
        assert_eq!(s[2].buttons, input::RIGHT | input::RUN);
        assert_eq!(s[5].buttons, input::RIGHT | input::JUMP);
        assert!(parse_script("x RIGHT").is_err());
        assert!(parse_script("1 FLY").is_err());
    }

    #[test]
    fn every_test_course_builds_and_runs() {
        let script = parse_script("10 -\n30 RIGHT RUN\n20 RIGHT JUMP\n").unwrap();
        for name in course_names() {
            let c = build_course(&name).unwrap_or_else(|e| panic!("{name}: {e}"));
            let csv = run(
                &c,
                &script,
                &Start {
                    pos: None,
                    facing: None,
                    form: None,
                    vx: None,
                },
            )
            .unwrap();
            assert_eq!(csv.lines().count(), 61, "{name}");
        }
    }

    #[test]
    fn walking_right_moves_right() {
        let c = build_course("M1_flat").unwrap();
        let script = parse_script("40 RIGHT").unwrap();
        let csv = run(
            &c,
            &script,
            &Start {
                pos: None,
                facing: None,
                form: None,
                vx: None,
            },
        )
        .unwrap();
        let last = csv.lines().last().unwrap();
        let vx: f32 = last.split(',').nth(3).unwrap().parse().unwrap();
        assert!((vx - 1.5).abs() < 1e-4, "{last}");
    }

    #[test]
    fn yaz0_store_round_trips() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let z = yaz0_store(&data);
        assert_eq!(mmk_formats::archive::yaz0::decompress(&z).unwrap(), data);
    }
}
