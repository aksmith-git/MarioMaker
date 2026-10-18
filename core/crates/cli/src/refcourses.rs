//! `mmk ref-courses`: small deterministic test courses, one per style × theme (× night), for
//! comparing our renderer with reference frames from the owner's own copy of the game. Each
//! course has the same first screen: a standard set of terrain shapes, blocks, a pipe, coins and
//! two enemies penned in so they barely move. The player stands still at the start.
//!
//! Output per course: `<name>.course` (raw course data, for `mmk-play`) and, with a key table,
//! `<name>.szs` (the encrypted course wrapped like the game's own built-in course files, for an
//! emulator's file-replacement mods).

use std::path::Path;

use mmk_editor::Editor;
use mmk_formats::archive::sarc;
use mmk_formats::smm2::crypto::{self, KeyTable};

pub const THEMES: [&str; 10] = [
    "ground",
    "underground",
    "castle",
    "airship",
    "water",
    "ghost",
    "snow",
    "desert",
    "sky",
    "forest",
];

/// (name, style, theme, night) of every test course.
pub fn list() -> Vec<(String, &'static str, u8, bool)> {
    let mut out = Vec::new();
    for style in ["WU", "3W", "M1"] {
        for (t, name) in THEMES.iter().enumerate() {
            out.push((format!("{style}_{name}"), style, t as u8, false));
            // 3D World has no night versions
            if style == "WU" {
                out.push((format!("{style}_{name}_night"), style, t as u8, true));
            }
        }
    }
    out
}

/// Build one test course.
pub fn build(style: &str, theme: u8, night: bool) -> Result<Vec<u8>, String> {
    const WIDTH: i32 = 60;
    let mut e = Editor::new(style);
    e.set_name("Reference");
    e.set_theme(theme);
    e.set_night(night);
    e.set_timer(500);
    e.set_length(WIDTH);
    // the start (x −3…6) and goal ground come from these heights, the player stands at y 2
    e.set_start_goal(2, 2);
    let err = |x: mmk_editor::EditError| format!("{x:?}");
    let ground = |e: &mut Editor, x0: i32, x1: i32, y0: i32, y1: i32| -> Result<(), String> {
        for x in x0..=x1 {
            for y in y0..=y1 {
                e.paint_ground(x, y).map_err(err)?;
            }
        }
        Ok(())
    };
    // floor from the start ground, ending well before the goal ground (whose exact extent the
    // game decides: ground running into it makes the course fail to load)
    ground(&mut e, 7, WIDTH - 20, 0, 1)?;
    // a 4×3 raised block, a one-wide pillar, a floating ledge, three steps
    ground(&mut e, 9, 12, 2, 4)?;
    ground(&mut e, 14, 14, 2, 6)?;
    ground(&mut e, 16, 20, 8, 8)?;
    ground(&mut e, 21, 21, 2, 2)?;
    ground(&mut e, 22, 22, 2, 3)?;
    ground(&mut e, 23, 24, 2, 4)?;
    // blocks: brick, ?, hard, note, donut, cloud, ice
    // (3D World has no note blocks: skip parts the style lacks, as the game's editor does)
    for (i, part) in [4i16, 5, 6, 23, 21, 22, 63].iter().enumerate() {
        if mmk_editor::rules::style_allows(style, *part) {
            e.place(*part, 6 + i as i32, 8).map_err(err)?;
        }
    }
    // coins
    for x in 7..10 {
        e.place(8, x, 11).map_err(err)?;
    }
    // pipe (2 wide, 3 tall) under the ledge
    e.place_sized(9, 16, 2, 2, 3, mmk_editor::DEFAULT_FLAGS)
        .map_err(err)?;
    // Goomba and Koopa penned in by hard blocks on top of the raised block
    e.place(6, 9, 5).map_err(err)?;
    e.place(6, 12, 5).map_err(err)?;
    e.place(0, 10, 5).map_err(err)?;
    e.place_sized(1, 11, 5, 1, 2, mmk_editor::DEFAULT_FLAGS)
        .map_err(err)?;
    // object fields as the game writes them: child flags default like the flags, pipes get a
    // link id and flag 0x80 [obs: every pipe of the owner's sample]
    let mut links = 0i16;
    for o in e.course.main.objects.items.iter_mut() {
        o.child_flags = mmk_editor::DEFAULT_FLAGS;
        if o.id == 9 {
            o.flags |= 0x80;
            o.link_id = links;
            links += 1;
        }
    }
    // header fields the game itself expects of a saved course (values as in the owner's
    // built-in and downloaded courses; the game crashes on loading the all-zero defaults)
    let c = &mut e.course;
    c.header.game_version = 1;
    c.header.management_flags = 1;
    c.header.clear_check_time = u32::MAX;
    c.header.year = 2019;
    c.header.month = 1;
    c.header.day = 1;
    c.header.creation_id = 0x534D_4352;
    // the goal sits 9.5 tiles before the right edge in every course of the owner's dump
    c.header.goal_x = (WIDTH * 10 - 95) as i16;
    for a in [&mut c.main, &mut c.sub] {
        a.header.liquid_start_height = 1;
        a.header.liquid_end_height = 1;
    }
    c.sub.header.theme = theme;
    c.sub.header.flags = c.main.header.flags;
    c.sub.header.boundary_type = 1;
    c.sub.header.boundary_right = 1344;
    retile(&mut e.course, WIDTH);
    e.save().map_err(err)
}

/// Ground ids as the game expects them: neighbours include the start ground (x −3…6 up to
/// start_y), the goal ground (11 tiles from 10 before the right edge, up to goal_y) and
/// everything below the area. The game crashes on loading ids that contradict their
/// neighbourhood (e.g. a corner piece next to the start ground).
fn retile(c: &mut mmk_formats::smm2::course::Course, width: i32) {
    const N: [(i32, i32); 8] = [
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
    ];
    let mut solid: std::collections::HashSet<(i32, i32)> = c
        .main
        .ground
        .iter()
        .map(|g| (g.x as i32, g.y as i32))
        .collect();
    for x in -3..7 {
        for y in 0..c.header.start_y as i32 {
            solid.insert((x, y));
        }
    }
    for x in width - 10..width + 1 {
        for y in 0..c.header.goal_y as i32 {
            solid.insert((x, y));
        }
    }
    for g in c.main.ground.items.iter_mut() {
        let (x, y) = (g.x as i32, g.y as i32);
        let mut mask = 0usize;
        for (k, (dx, dy)) in N.iter().enumerate() {
            if y + dy < 0 || solid.contains(&(x + dx, y + dy)) {
                mask |= 1 << k;
            }
        }
        g.id = mmk_editor::autotile::GROUND_ID[normalise(mask)];
    }
}

/// Drop diagonal neighbours whose two orthogonal neighbours aren't both ground (they don't change
/// the piece).
fn normalise(mut m: usize) -> usize {
    for k in [1, 3, 5, 7] {
        let (a, b) = ((k + 7) % 8, (k + 1) % 8);
        if m >> a & 1 == 0 || m >> b & 1 == 0 {
            m &= !(1 << k);
        }
    }
    m
}

/// Encrypt a course and wrap it like the game's built-in courses (Yaz0 → SARC → `<stem>.bcd`).
pub fn szs(course: &[u8], table: &KeyTable, stem: &str) -> Result<Vec<u8>, String> {
    let iv = [0x5Au8; 16];
    let state = [0x1234_5678, 0x9ABC_DEF0, 0x0F1E_2D3C, 0x4B5A_6978];
    let bcd = crypto::encrypt_bcd(course, table, crypto::default_header(), iv, state)
        .map_err(|e| e.to_string())?;
    let name = format!("{stem}.bcd");
    let arc = sarc::build(&[(name.as_str(), &bcd)]);
    Ok(yaz0_store(&arc))
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

/// Write the selected courses to `out`; with `romfs` (needs the key table), also the replacement
/// built-in title courses `romfs/Course/title_course_data_00…09.szs` of the last one selected
/// (or of `course`, any course data, when given).
pub fn run(
    out: &Path,
    table: Option<&KeyTable>,
    only: Option<&str>,
    romfs: Option<&Path>,
    course: Option<Vec<u8>>,
) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut last = None;
    let all = list();
    let exact = only.is_some_and(|o| all.iter().any(|c| c.0 == o));
    for (name, style, theme, night) in all {
        if only.is_some_and(|o| if exact { name != o } else { !name.contains(o) }) {
            continue;
        }
        let course = build(style, theme, night)?;
        std::fs::write(out.join(format!("{name}.course")), &course).map_err(|e| e.to_string())?;
        println!("{name}");
        last = Some(course);
    }
    if let Some(c) = course {
        std::fs::write(out.join("wrapped.course"), &c).map_err(|e| e.to_string())?;
        last = Some(c);
    }
    if let Some(dir) = romfs {
        let table = table.ok_or("--romfs needs --table")?;
        let course = last.ok_or("no course selected")?;
        let dir = dir.join("Course");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for i in 0..10 {
            let stem = format!("title_course_data_{i:02}");
            let z = szs(&course, table, &stem)?;
            std::fs::write(dir.join(format!("{stem}.szs")), z).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reference_course_builds_and_parses() {
        for (name, style, theme, night) in list() {
            let c = build(style, theme, night).unwrap_or_else(|e| panic!("{name}: {e}"));
            let e = Editor::open(&c).unwrap();
            assert_eq!(e.theme(), (theme, night), "{name}");
        }
    }

    #[test]
    fn yaz0_store_round_trips() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let z = yaz0_store(&data);
        assert_eq!(mmk_formats::archive::yaz0::decompress(&z).unwrap(), data);
    }
}
