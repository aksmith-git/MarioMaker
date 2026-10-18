//! Test courses for the parts SMM2 added in its 2.0 / 3.0 updates (the sample course corpus is
//! from June 2019, so it never contains them): each course is built with the editor, saved,
//! reopened and played in the engine. Set `MMK_WRITE_COURSES=<dir>` to also write the courses
//! there (raw course data) for `mmk coverage`, `mmk-play --map` and `mmk-play`.
#![allow(dead_code)]

use mmk_editor::{DEFAULT_FLAGS, Editor};
use mmk_sim::World;
use mmk_sim::actors::Kind;
use mmk_sim::input::Input;

/// A part to place: id, tile x, tile y, size, extra flag bits.
struct Part {
    id: i16,
    x: i32,
    y: i32,
    w: u8,
    h: u8,
    flags: u32,
}

fn part(id: i16, x: i32, y: i32) -> Part {
    let (w, h) = mmk_editor::default_size(id);
    Part {
        id,
        x,
        y,
        w,
        h,
        flags: 0,
    }
}

/// A 60-tile course in `style` / `theme` with a two-tile floor and the parts; saved and
/// reopened (so it goes through the course file), optionally written to $MMK_WRITE_COURSES.
fn course(name: &str, style: &str, theme: u8, parts: &[Part]) -> mmk_formats::smm2::Course {
    let mut ed = Editor::new(style);
    ed.set_name(name);
    ed.set_theme(theme);
    ed.set_timer(500);
    ed.set_length(60);
    for x in 0..60 {
        for y in 0..2 {
            ed.paint_ground(x, y).unwrap();
        }
    }
    for p in parts {
        ed.place_sized(p.id, p.x, p.y, p.w, p.h, DEFAULT_FLAGS | p.flags)
            .unwrap_or_else(|e| panic!("part {}: {e:?}", p.id));
    }
    let bytes = ed.save().unwrap();
    if let Ok(dir) = std::env::var("MMK_WRITE_COURSES") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/v3_{name}.bin"), &bytes).unwrap();
    }
    let back = Editor::open(&bytes).unwrap();
    assert!(
        mmk_sim::level::unsupported_features(&back.course).is_empty(),
        "{name}: {:?}",
        mmk_sim::level::unsupported_features(&back.course)
    );
    back.course
}

fn run(w: &mut World, frames: usize) {
    for _ in 0..frames {
        w.step(Input::default());
    }
}

/// The world with the player parked at the start, run until the actors near it are active.
fn world(c: &mmk_formats::smm2::Course) -> World {
    let mut w = World::from_course(c);
    w.player.invuln = 1_000_000;
    run(&mut w, 2);
    w
}

fn count(w: &World, f: impl Fn(Kind) -> bool) -> usize {
    w.actors.iter().filter(|a| f(a.kind)).count()
}
