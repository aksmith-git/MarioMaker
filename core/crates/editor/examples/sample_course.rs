//! Build a course with the Course Maker core and write it out (raw course data):
//! `cargo run -p mmk-editor --example sample_course -- out.bin`. Uses both areas, a warp pipe
//! pair, doors, a track with a lift, a snake block path, resized parts, a clear condition and
//! custom autoscroll — a quick end-to-end check with `mmk coverage out.bin`.

use mmk_editor::links::DoorKind;
use mmk_editor::parts::Dir;
use mmk_editor::paths::Step;
use mmk_editor::{DEFAULT_FLAGS, Editor};
use mmk_formats::smm2::clear;

fn main() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample_course.bin".into());
    let err = |e| format!("{e:?}");
    let mut e = Editor::new("MW");
    e.set_name("Editor sample");
    e.set_area_width(120).map_err(err)?;
    for x in 7..120 {
        for y in 0..2 {
            if !e.protected(x, y) {
                e.paint_ground(x, y).map_err(err)?;
            }
        }
    }
    let q = e.place(5, 12, 5).map_err(err)?;
    e.set_child(q, 44, DEFAULT_FLAGS);
    let semi = e.place(16, 20, 2).map_err(err)?;
    e.resize(semi, 6, 3).map_err(err)?;
    let pipe = e.place_pipe(30, 2, 3, Dir::Up, 0).map_err(err)?;
    e.put_in_pipe(pipe, 2).map_err(err)?;
    e.place_doors((40, 2), (60, 2), DoorKind::Normal)
        .map_err(err)?;
    e.draw_track((70, 6), (80, 6)).map_err(err)?;
    let lift = e.place(11, 72, 6).map_err(err)?;
    e.attach_to_track(lift).map_err(err)?;
    let snake = e.place(84, 86, 5).map_err(err)?;
    e.set_path(
        snake,
        Step::Right,
        &[Step::Right, Step::Right, Step::Up, Step::Right],
    )
    .map_err(err)?;
    for x in [50, 54, 58] {
        e.place(0, x, 2).map_err(err)?;
    }
    e.set_clear_condition(clear::PARTS, 1, 3).map_err(err)?;
    e.set_area(1);
    e.set_vertical(true).map_err(err)?;
    e.set_theme(1);
    for x in 0..48 {
        e.paint_ground(x, 0).map_err(err)?;
    }
    let back = e.place_pipe(10, 1, 2, Dir::Up, 1).map_err(err)?;
    e.link_pipes(pipe, back).map_err(err)?;
    let bytes = e.save().map_err(err)?;
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}
