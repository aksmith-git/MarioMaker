//! Test courses for the parts SMM2 added in its 2.0 / 3.0 updates (the sample course corpus is
//! from June 2019, so it never contains them): each course is built with the editor, saved,
//! reopened and played in the engine. Set `MMK_WRITE_COURSES=<dir>` to also write the courses
//! there (raw course data) for `mmk coverage`, `mmk-play --map` and `mmk-play`.

use mmk_editor::{DEFAULT_FLAGS, Editor};
use mmk_sim::World;
use mmk_sim::actors::Kind;
use mmk_sim::input::{self, Input};
use mmk_sim::level::Cell;

/// A part to place: id, tile x, tile y, size, extra flag bits.
struct Part {
    id: i16,
    x: i32,
    y: i32,
    w: u8,
    h: u8,
    flags: u32,
    /// A part inside it (a Key for a Koopaling), or −1.
    child: i16,
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
        child: -1,
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
    ed.set_start_goal(2, 2);
    for x in 0..60 {
        for y in 0..2 {
            // the start and goal zones have their own ground (protected)
            match ed.paint_ground(x, y) {
                Ok(()) | Err(mmk_editor::EditError::Protected) => {}
                Err(e) => panic!("ground at ({x}, {y}): {e:?}"),
            }
        }
    }
    for p in parts {
        let i = ed
            .place_sized(p.id, p.x, p.y, p.w, p.h, DEFAULT_FLAGS | p.flags)
            .unwrap_or_else(|e| panic!("part {}: {e:?}", p.id));
        if p.child >= 0 {
            ed.set_child(i, p.child, 0);
        }
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
    press(w, 0, frames);
}

fn press(w: &mut World, buttons: u32, frames: usize) {
    for _ in 0..frames {
        w.step(Input { buttons });
    }
}

fn first(w: &World, f: impl Fn(Kind) -> bool) -> &mmk_sim::actors::Actor {
    w.actors.iter().find(|a| f(a.kind)).expect("actor")
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

fn with_flags(mut p: Part, flags: u32) -> Part {
    p.flags = flags;
    p
}

fn sized(mut p: Part, w: u8, h: u8) -> Part {
    p.w = w;
    p.h = h;
    p
}

// ---------------------------------------------------------------- P Block

#[test]
fn p_blocks_swap_while_a_p_switch_runs() {
    let c = course(
        "p_block",
        "3W",
        0,
        &[
            part(79, 10, 4),
            with_flags(part(79, 12, 4), 0x4),
            part(18, 20, 2),
        ],
    );
    let mut w = world(&c);
    assert_eq!(
        w.level.cell(10, 4),
        Cell::Solid,
        "PBlockDisappear: solid while no P"
    );
    assert_eq!(
        w.level.cell(12, 4),
        Cell::Empty,
        "PBlockAppear: outline while no P"
    );
    // drop the player onto the P Switch
    let sw = first(&w, |k| k == Kind::PSwitch);
    let (sx, sy) = (sw.x, sw.y);
    w.player.x = sx;
    w.player.y = sy + 24.0;
    run(&mut w, 30);
    assert!(w.p_timer > 0, "pressed");
    assert_eq!(
        mmk_sim::world::P_FRAMES,
        540,
        "the P timer [obs 0x71016c4a48]"
    );
    assert_eq!(w.level.cell(10, 4), Cell::Empty);
    assert_eq!(w.level.cell(12, 4), Cell::Solid);
    run(&mut w, mmk_sim::world::P_FRAMES as usize);
    assert_eq!(w.p_timer, 0);
    assert_eq!(
        w.level.cell(10, 4),
        Cell::Solid,
        "back the same frame the P ends"
    );
    assert_eq!(w.level.cell(12, 4), Cell::Empty);
}

// ---------------------------------------------------------------- Dash Block

#[test]
fn dash_block_boosts_to_4_px_per_frame_for_120_frames() {
    let c = course("dash_block", "3W", 0, &[part(80, 8, 2)]);
    let mut w = world(&c);
    assert_eq!(w.level.v3.dash_blocks, vec![(8, 2, 3)]);
    assert!(
        (8..11).all(|x| w.level.cell(x, 2) == Cell::Solid),
        "48 × 16 solid"
    );
    // walk onto it from the left: up onto the block
    w.player.x = 8.0 * 16.0 + 4.0;
    w.player.y = 3.0 * 16.0;
    run(&mut w, 2);
    assert_eq!(w.v3.dash, mmk_sim::world::v3_dash_frames());
    press(&mut w, input::RIGHT, 1);
    let x0 = w.player.x;
    press(&mut w, input::RIGHT, 1);
    assert!(
        (w.player.x - x0 - 4.0).abs() < 0.01,
        "dash speed {}",
        w.player.x - x0
    );
    // off the block the timer runs down, the speed stays 4.0 without holding a direction
    press(&mut w, input::RIGHT, 20);
    assert!(w.player.x > 11.0 * 16.0);
    let left = w.v3.dash;
    assert!(left > 0 && left < 120, "{left}");
    let x1 = w.player.x;
    run(&mut w, 1);
    assert!((w.player.x - x1 - 4.0).abs() < 0.01, "no input: still 4.0");
    run(&mut w, left as usize + 2);
    assert_eq!(w.v3.dash, 0);
    let x2 = w.player.x;
    run(&mut w, 30);
    assert!(
        w.player.x - x2 < 30.0 * 3.0,
        "slows down once the dash ends"
    );
}

// ---------------------------------------------------------------- ON/OFF Trampoline

#[test]
fn on_off_trampolines_bounce_only_in_their_colour_s_state() {
    let c = course(
        "on_off_trampoline",
        "3W",
        0,
        &[
            part(132, 10, 2),
            with_flags(part(132, 20, 2), 0x4),
            part(99, 30, 6),
        ],
    );
    let mut w = world(&c);
    let tramps: Vec<_> = w
        .actors
        .iter()
        .filter(|a| matches!(a.kind, Kind::OnOffTrampoline { .. }))
        .map(|a| (a.x, a.y))
        .collect();
    assert_eq!(tramps.len(), 2);
    let drop_on = |w: &mut World, (x, y): (f32, f32)| -> f32 {
        w.player.x = x;
        w.player.y = y + 40.0;
        w.player.vy = 0.0;
        w.player.ride = None;
        let mut top: f32 = 0.0;
        for _ in 0..60 {
            run(w, 1);
            top = top.max(w.player.vy);
        }
        top
    };
    // ON (the start state): the red one bounces (vy 6.0, seen after this frame's gravity),
    // the blue one is a plain platform
    let bounce = |v: f32| v > 5.5 && v <= 6.0;
    assert!(bounce(drop_on(&mut w, tramps[0])));
    assert!(drop_on(&mut w, tramps[1]) < 0.5, "blue is solid while ON");
    w.on_off = false;
    assert!(drop_on(&mut w, tramps[0]) < 0.5, "red is solid while OFF");
    assert!(bounce(drop_on(&mut w, tramps[1])));
}

// ---------------------------------------------------------------- Spike

/// Frames until the first ball leaves a Spike, and the ball's x speed a few frames later.
fn first_throw(w: &mut World) -> (usize, f32) {
    for f in 0..400 {
        run(w, 1);
        if count(w, |k| matches!(k, Kind::SpikeBall { .. })) > 0 {
            run(w, 30);
            let b = first(w, |k| matches!(k, Kind::SpikeBall { .. }));
            return (f, b.vx);
        }
    }
    panic!("no throw");
}

#[test]
fn nsmbu_spike_faces_the_player_and_throws_rolling_balls() {
    // the player (x 56) is left of the Spike: it turns left and throws within its first Wait
    // (0–59) + lift (50) + release (52)
    let c = course("spike_wu", "WU", 0, &[part(74, 14, 2)]);
    let mut w = world(&c);
    let (f, vx) = first_throw(&mut w);
    assert!((100..=165).contains(&f), "first ball after {f} frames");
    assert!((vx + 1.2).abs() < 0.01, "rolls left at 1.2: {vx}");
    let s = first(&w, |k| k == Kind::Spike);
    assert_eq!(s.dir, -1);
    assert_eq!(s.vx, 0.0, "GabonWU stands");
}

#[test]
fn smb1_spike_walks_and_its_ball_flies_level() {
    let c = course(
        "spike_m1",
        "M1",
        0,
        // (the start zone, columns 0–6, is protected: the wall goes just past it)
        &[part(74, 18, 2), sized(part(6, 7, 2), 1, 3)],
    );
    let mut w = world(&c);
    // the player is 32–288 px in front: it stops, lifts and throws
    let (f, vx) = first_throw(&mut w);
    assert_eq!(f, 35 + 14, "lift 35, release at throw frame 14");
    assert!((vx.abs() - 1.5).abs() < 0.01, "flies at 1.5: {vx}");
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    let y = b.y;
    run(&mut w, 10);
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    assert_eq!(b.y, y, "no gravity in Fly");
    // bursts of two: a second lift follows the 35-frame stand; then it walks (0.5 px/f) with
    // the player behind it
    let sx = first(&w, |k| k == Kind::Spike).x;
    w.player.x = sx + 20.0;
    run(&mut w, 100);
    // (the first ball has flown into the wall at tile 7 and broken)
    assert_eq!(count(&w, |k| matches!(k, Kind::SpikeBall { .. })), 1);
    let x0 = first(&w, |k| k == Kind::Spike).x;
    run(&mut w, 10);
    let x1 = first(&w, |k| k == Kind::Spike).x;
    assert!(
        ((x1 - x0).abs() - 5.0).abs() < 0.01,
        "walks 0.5 px/f: {}",
        x1 - x0
    );
}

#[test]
fn the_ball_leaves_the_spike_s_generator_joint() {
    // SMB1: the joint is 3 px behind and 24 px above the feet at the release frame; the ball's
    // centre is on it (radius 8)
    let c = course("spike_hand", "M1", 0, &[part(74, 18, 2)]);
    let mut w = world(&c);
    for _ in 0..400 {
        run(&mut w, 1);
        if count(&w, |k| matches!(k, Kind::SpikeBall { .. })) > 0 {
            break;
        }
    }
    let s = first(&w, |k| k == Kind::Spike);
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    let ahead = (b.x - s.x) * s.dir as f32;
    assert!(
        (-3.01..=-1.49).contains(&ahead),
        "3 px behind (+1.5 flown): {ahead}"
    );
    assert_eq!(b.y - s.y, 16.0);
}

#[test]
fn snowballs_in_the_snow_theme_are_kicked_from_the_side() {
    let c = course("spike_snow", "MW", 6, &[with_flags(part(74, 10, 2), 0x4)]);
    let mut w = world(&c);
    assert_eq!(count(&w, |k| k == Kind::SpikeBall { snow: true }), 1);
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    let bx = b.x;
    w.player.x = bx - 20.0;
    w.player.invuln = 0;
    press(&mut w, input::RIGHT, 20);
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    assert!(b.vx >= 1.7, "kicked right: {}", b.vx);
    assert!(w.player.alive());
}

#[test]
fn the_iron_ball_hurts_even_from_above() {
    let c = course("spike_ball", "WU", 0, &[with_flags(part(74, 10, 2), 0x4)]);
    let mut w = world(&c);
    w.player.invuln = 0;
    let b = first(&w, |k| matches!(k, Kind::SpikeBall { .. }));
    let (bx, by) = (b.x, b.y);
    w.player.x = bx;
    w.player.y = by + 24.0;
    run(&mut w, 20);
    assert!(
        w.player.invuln > 0 || !w.player.alive(),
        "a stomp is Invalid: the spikes hurt"
    );
}

// ---------------------------------------------------------------- Pokey

fn pokey(w: &World) -> &mmk_sim::actors::Actor {
    first(w, |k| matches!(k, Kind::Pokey { .. }))
}

#[test]
fn a_winged_pokey_hops_small_small_big() {
    let c = course("pokey_wing", "M3", 0, &[with_flags(part(78, 16, 2), 0x2)]);
    let mut w = world(&c);
    let mut hops = Vec::new();
    let mut last = pokey(&w).vy;
    for _ in 0..600 {
        run(&mut w, 1);
        let vy = pokey(&w).vy;
        if vy > 2.0 && last <= 0.0 {
            hops.push(vy);
        }
        last = vy;
        if hops.len() == 6 {
            break;
        }
    }
    let r = |v: f32| (v * 10.0).round() / 10.0;
    let hops: Vec<f32> = hops.into_iter().map(r).collect();
    // (its first hop comes during the setup frames)
    let big: Vec<usize> = (0..hops.len()).filter(|&i| hops[i] == 4.7).collect();
    assert_eq!(big.len(), 2, "{hops:?}");
    assert_eq!(big[1] - big[0], 3, "{hops:?}");
    assert!(hops.iter().all(|&v| v == 4.7 || v == 2.5), "{hops:?}");
}

#[test]
fn pokey_walks_a_quarter_pixel_and_sways() {
    let c = course("pokey", "M3", 0, &[part(78, 16, 2)]);
    let mut w = world(&c);
    let p = pokey(&w);
    assert_eq!(
        p.kind,
        Kind::Pokey {
            snow: false,
            height: 4
        }
    );
    assert_eq!(p.v3.segs.len(), 4, "the default stack: head + 3");
    assert_eq!(p.height(), 64.0);
    let x0 = p.x;
    run(&mut w, 40);
    assert!(((pokey(&w).x - x0).abs() - 10.0).abs() < 0.01, "0.25 px/f");
    // segments sway ±1.5 px, odd ones the other way
    let segs = pokey(&w).pokey_segments();
    assert!(
        segs.iter()
            .all(|&(sx, _)| (sx - pokey(&w).x).abs() <= 1.5 + 1e-3)
    );
}

#[test]
fn a_fireball_knocks_out_one_segment_and_a_head_stomp_the_whole_stack() {
    let c = course("pokey_hits", "M1", 0, &[part(78, 16, 2)]);
    let mut w = world(&c);
    // a fireball at the second segment
    let (px, py) = (pokey(&w).x, pokey(&w).y);
    let mut f = mmk_sim::actors::Actor::new(Kind::Fireball, px - 6.0, py + 20.0);
    f.vx = 3.0;
    f.timer = 600;
    w.actors.push(f);
    run(&mut w, 2);
    assert_eq!(pokey(&w).v3.segs.len(), 3, "one segment gone");
    assert!(pokey(&w).alive());
    // the ones above hop back down into place
    run(&mut w, 40);
    assert!(pokey(&w).v3.segs.iter().all(|s| s.drop == 0.0));
    assert_eq!(pokey(&w).height(), 48.0);
    // a stomp on the head defeats it
    let (px, top) = (pokey(&w).x, pokey(&w).y + pokey(&w).height());
    w.player.x = px;
    w.player.y = top + 4.0;
    w.player.vy = -2.0;
    // teleported into the air: falling, not standing where it was
    w.player.on_ground = false;
    run(&mut w, 3);
    assert!(!pokey(&w).alive(), "DieStamp on the head");
}

#[test]
fn snow_pokey_is_picked_up_as_a_snowball() {
    let c = course("pokey_snow", "WU", 6, &[part(78, 10, 2)]);
    let mut w = world(&c);
    assert_eq!(
        pokey(&w).kind,
        Kind::Pokey {
            snow: true,
            height: 4
        }
    );
    let px = pokey(&w).x;
    w.player.x = px - 10.0;
    w.player.invuln = 0;
    press(&mut w, input::RUN, 3);
    assert!(w.player.held.is_some(), "holding a snowball");
    assert_eq!(count(&w, |k| matches!(k, Kind::Pokey { .. })), 0);
    // let go of Run: thrown forward at 2.5 + 0.35·|vx|, vy 1.0
    run(&mut w, 1);
    let b = first(&w, |k| k == Kind::SpikeBall { snow: true });
    assert!(!b.held && b.vx >= 2.45, "thrown: {}", b.vx);
}

// ---------------------------------------------------------------- Mechakoopa

fn mecha(w: &World) -> &mmk_sim::actors::Actor {
    first(w, |k| matches!(k, Kind::Mechakoopa { .. }))
}

/// Mechakoopa kind attribute: (flags >> 18) & 3.
fn mecha_part(x: i32, weapon: u32) -> Part {
    with_flags(part(111, x, 2), weapon << 18)
}

#[test]
fn a_stomp_knocks_a_mechakoopa_down_for_480_frames() {
    let c = course("mecha", "M1", 0, &[mecha_part(16, 0)]);
    let mut w = world(&c);
    assert_eq!(mecha(&w).kind, Kind::Mechakoopa { weapon: 0 });
    let x0 = mecha(&w).x;
    run(&mut w, 20);
    assert!(
        ((mecha(&w).x - x0).abs() - 10.0).abs() < 0.6,
        "walks 0.5 px/f"
    );
    let (mx, my) = (mecha(&w).x, mecha(&w).y);
    w.player.x = mx;
    w.player.y = my + 14.0;
    w.player.vy = -2.0;
    // teleported into the air: falling, not standing where it was
    w.player.on_ground = false;
    run(&mut w, 2);
    assert!(mecha(&w).mecha_down(), "knocked down, not defeated");
    w.player.x = 40.0;
    run(&mut w, 470);
    assert!(mecha(&w).mecha_down());
    assert!(
        mecha(&w).mecha_shaking(),
        "the shake warning below 61 frames"
    );
    run(&mut w, 12 + 18);
    assert!(
        !mecha(&w).mecha_down(),
        "back up after 480 + the 18-frame revival"
    );
    assert!(mecha(&w).alive());
}

#[test]
fn a_downed_mechakoopa_is_kicked_and_fire_does_nothing() {
    let c = course("mecha_kick", "M1", 0, &[mecha_part(16, 0)]);
    let mut w = world(&c);
    // fire: Invalid
    let (mx, my) = (mecha(&w).x, mecha(&w).y);
    let mut f = mmk_sim::actors::Actor::new(Kind::Fireball, mx - 10.0, my + 4.0);
    f.vx = 3.0;
    f.timer = 600;
    w.actors.push(f);
    run(&mut w, 3);
    assert!(mecha(&w).alive() && !mecha(&w).mecha_down());
    // knock it down, then walk into it: kicked at 2.3 + 0.35·|vx|
    w.actors
        .iter_mut()
        .find(|a| matches!(a.kind, Kind::Mechakoopa { .. }))
        .unwrap()
        .mecha_knock(0.0, 0.0, 0.0, 0.03125);
    let mx = mecha(&w).x;
    w.player.x = mx - 14.0;
    w.player.invuln = 0;
    press(&mut w, input::RIGHT, 6);
    assert!(mecha(&w).vx > 2.0, "kicked: {}", mecha(&w).vx);
    assert!(
        w.player.alive() && w.player.invuln == 0,
        "a downed one is harmless"
    );
}

#[test]
fn blasta_fires_a_homing_missile_that_explodes_at_300_frames() {
    let c = course("mecha_blasta", "M3", 0, &[mecha_part(14, 1)]);
    let mut w = world(&c);
    assert_eq!(mecha(&w).kind, Kind::Mechakoopa { weapon: 1 });
    let mut fired = None;
    for f in 0..300 {
        run(&mut w, 1);
        if count(&w, |k| k == Kind::MechaMissile) > 0 {
            fired = Some(f);
            break;
        }
    }
    // first shot 30 + 0…59, then fire_st 30 and Charge 60
    let f = fired.expect("missile");
    assert!((118..=182).contains(&f), "{f}");
    let m = first(&w, |k| k == Kind::MechaMissile);
    assert!((m.speed - 1.2).abs() < 0.02);
    let id = m.id;
    // it turns toward the player (left, behind the launch direction if it faced right)
    w.player.y = 32.0;
    run(&mut w, 200);
    let m = first(&w, |k| k == Kind::MechaMissile);
    assert!(m.vx < 0.0, "homes on the player to the left");
    run(&mut w, 98);
    assert!(
        w.actors.iter().any(|a| a.id == id),
        "still flying at frame 299"
    );
    run(&mut w, 3);
    assert!(w.actors.iter().all(|a| a.id != id), "exploded at frame 300");
}

#[test]
fn zappa_charges_and_fires_a_horizontal_laser() {
    let c = course("mecha_zappa", "WU", 0, &[mecha_part(14, 2)]);
    let mut w = world(&c);
    let mut laser = None;
    for f in 0..300 {
        run(&mut w, 1);
        if count(&w, |k| k == Kind::MechaLaser) > 0 {
            laser = Some(f);
            break;
        }
    }
    // first shot after 30 + 0…59, fire_st 28 (NSMBU), the beam appears as the Charge starts
    let f = laser.expect("laser");
    assert!((56..=120).contains(&f), "{f}");
    let m = first(&w, |k| k == Kind::MechaLaser);
    assert!(m.laser_box().is_none(), "telegraph: no damage");
    // no robot model has a "shot" joint: the beam starts at the robot's feet
    let r = mecha(&w);
    assert_eq!((m.x, m.y), (r.x, r.y));
    run(&mut w, 69 + 20);
    let m = first(&w, |k| k == Kind::MechaLaser);
    let b = m.laser_box().expect("firing");
    assert!(b.2 - b.0 > 100.0, "the beam grows 10 px/f");
    assert!((b.3 - b.1 - 8.0).abs() < 0.01, "8 px tall");
    run(&mut w, 40);
    assert_eq!(count(&w, |k| k == Kind::MechaLaser), 0);
}

// ---------------------------------------------------------------- Koopalings

fn koop(w: &World) -> &mmk_sim::actors::Actor {
    first(w, |k| matches!(k, Kind::Koopaling { .. }))
}

fn koop_mut(w: &mut World) -> &mut mmk_sim::actors::Actor {
    w.actors
        .iter_mut()
        .find(|a| matches!(a.kind, Kind::Koopaling { .. }))
        .expect("koopaling")
}

/// Drop the player onto the Koopaling's head once it can be hurt; true if it lost HP.
fn stomp_koopaling(w: &mut World) -> bool {
    for _ in 0..600 {
        if !koop(w).koopaling_guarded() && koop(w).on_ground && !koop(w).koopaling_hidden() {
            break;
        }
        run(w, 1);
    }
    let hp = koop(w).koopaling_hp();
    let (x, top) = (koop(w).x, koop(w).y + koop(w).height());
    w.player.x = x;
    w.player.y = top + 2.0;
    w.player.vy = -2.0;
    // teleported into the air: falling, not standing where it was
    w.player.on_ground = false;
    run(w, 1);
    w.player.x = 40.0;
    w.player.y = 32.0;
    koop(w).koopaling_hp() < hp || !koop(w).alive()
}

#[test]
fn larry_takes_three_stomps_and_drops_his_key() {
    let mut p = part(122, 14, 2);
    p.child = 95;
    let c = course("larry", "M1", 0, &[p]);
    let mut w = world(&c);
    assert_eq!(
        koop(&w).kind,
        Kind::Koopaling {
            who: 2,
            key: true,
            clown: false
        }
    );
    assert_eq!(koop(&w).height(), 32.0, "32 × 32 body");
    for hp in [2, 1] {
        assert!(stomp_koopaling(&mut w));
        assert_eq!(koop(&w).koopaling_hp(), hp);
        assert!(koop(&w).koopaling_guarded(), "into the shell");
        // stomps bounce off the shell
        let (x, top) = (koop(&w).x, koop(&w).y + koop(&w).height());
        w.player.x = x;
        w.player.y = top + 2.0;
        w.player.vy = -2.0;
        w.player.on_ground = false;
        // teleported into the air: falling, not standing where it was
        w.player.on_ground = false;
        run(&mut w, 1);
        assert_eq!(koop(&w).koopaling_hp(), hp, "no HP lost in the shell");
        w.player.x = 40.0;
    }
    assert!(stomp_koopaling(&mut w));
    assert!(
        !koop(&w).alive(),
        "the third stomp defeats him (SMB1: at once)"
    );
    run(&mut w, 2);
    // (dropped where he was knocked out; the player may already have picked it up)
    assert_eq!(
        count(&w, |k| k == Kind::Key) + w.keys as usize,
        1,
        "his Key drops"
    );
}

#[test]
fn five_fireballs_cost_a_koopaling_one_hp() {
    let c = course("larry_fire", "MW", 0, &[part(122, 14, 2)]);
    let mut w = world(&c);
    let fire = |w: &mut World| {
        for _ in 0..600 {
            if !koop(w).koopaling_guarded() {
                break;
            }
            run(w, 1);
        }
        let (x, y) = (koop(w).x, koop(w).y);
        let mut f = mmk_sim::actors::Actor::new(Kind::Fireball, x - 20.0, y + 10.0);
        f.vx = 3.625;
        f.timer = 600;
        w.actors.push(f);
        run(w, 10);
    };
    for _ in 0..4 {
        fire(&mut w);
    }
    assert_eq!(koop(&w).koopaling_hp(), 3, "four fireballs only flash him");
    fire(&mut w);
    assert_eq!(koop(&w).koopaling_hp(), 2, "the fifth costs 1 HP");
}

/// Run until the Koopaling has fired `n` projectiles of kind `what` (or give up).
fn wait_shots(w: &mut World, n: usize, frames: usize) -> usize {
    for f in 0..frames {
        run(w, 1);
        if count(w, |k| matches!(k, Kind::KoopalingShot { .. })) >= n {
            return f;
        }
    }
    frames
}

#[test]
fn larry_jumps_between_volleys_of_wand_magic() {
    let c = course("larry_magic", "M3", 0, &[part(122, 14, 2)]);
    let mut w = world(&c);
    let f = wait_shots(&mut w, 1, 600);
    assert!(f < 600, "fires");
    let m = first(&w, |k| k == Kind::KoopalingShot { what: 0 });
    // aimed at the player (left of him), within 60° of level, at 1.2 plus a fading drift
    assert!(m.vx < 0.0);
    assert!((m.speed - 1.2).abs() < 1e-3);
    // at 1 HP a volley is 3 shots
    koop_mut(&mut w).v3.hp = 1;
    w.actors
        .retain(|a| !matches!(a.kind, Kind::KoopalingShot { .. }));
    let mut most = 0;
    for _ in 0..900 {
        run(&mut w, 1);
        most = most.max(count(&w, |k| matches!(k, Kind::KoopalingShot { .. })));
    }
    assert!(most >= 3, "volleys of 3 at 1 HP ({most})");
}

#[test]
fn wendy_rings_bounce_at_45_degrees() {
    let c = course("wendy", "M1", 0, &[part(123, 14, 2)]);
    let mut w = world(&c);
    assert!(wait_shots(&mut w, 1, 600) < 600);
    let r = first(&w, |k| k == Kind::KoopalingShot { what: 2 });
    assert!((r.vx.abs() - 0.8485).abs() < 1e-3 && (r.vy.abs() - 0.8485).abs() < 1e-3);
    run(&mut w, 120);
    let r = first(&w, |k| k == Kind::KoopalingShot { what: 2 });
    assert!(r.y >= 31.0, "reflected off the floor: y {}", r.y);
    assert!((r.vx.abs() - 0.8485).abs() < 1e-3, "speed kept");
}

#[test]
fn morton_ground_pounds_stun_the_player_and_send_out_two_flames() {
    let c = course("morton", "M1", 0, &[part(121, 14, 2)]);
    let mut w = world(&c);
    let mut stunned = false;
    let mut flames = 0;
    for _ in 0..900 {
        // a player with damage invincibility is not stunned (0x71015a6880): drop the test's
        // invincibility while Morton drops and lands (QuakeDrop 23, Quake 24)
        let landing = matches!(koop(&w).phase, 23 | 24);
        w.player.invuln = if landing { 0 } else { 1_000_000 };
        if landing {
            // keep his wand magic off the unprotected player (only the quake is under test;
            // the flames, kind 4, are counted)
            w.actors
                .retain(|a| !matches!(a.kind, Kind::KoopalingShot { what } if what != 4));
        }
        run(&mut w, 1);
        stunned |= w.player.stun > 0;
        flames = flames.max(count(&w, |k| k == Kind::KoopalingShot { what: 4 }));
    }
    assert!(stunned, "the quake stuns a grounded player");
    assert_eq!(flames, 2);
}

#[test]
fn lemmy_rides_his_ball_and_throws_bouncing_balls() {
    let c = course("lemmy", "M1", 0, &[part(120, 14, 2)]);
    let mut w = world(&c);
    assert!(koop(&w).lemmy_ball().is_some());
    assert!(wait_shots(&mut w, 1, 600) < 600);
    let mut top: f32 = 0.0;
    for _ in 0..120 {
        run(&mut w, 1);
        if let Some(b) = w
            .actors
            .iter()
            .find(|a| a.kind == (Kind::KoopalingShot { what: 3 }))
        {
            top = top.max(b.vy);
            assert!((b.vx.abs() - 0.6).abs() < 1e-3 || b.mode != mmk_sim::actors::Mode::Normal);
        }
    }
    assert!((top - 3.3).abs() < 0.2, "bounces at 3.3: {top}");
}

#[test]
fn ludwig_hovers_and_fans_three_shots() {
    let c = course("ludwig", "M1", 0, &[part(126, 14, 2)]);
    let mut w = world(&c);
    let mut hovered = false;
    let mut most = 0;
    for _ in 0..900 {
        run(&mut w, 1);
        hovered |= !koop(&w).on_ground && koop(&w).vy.abs() < 0.5 && koop(&w).y > 60.0;
        most = most.max(count(&w, |k| matches!(k, Kind::KoopalingShot { .. })));
    }
    assert!(hovered, "JumpKeep");
    assert!(most >= 3, "a fan of 3: {most}");
}

#[test]
fn roy_dives_into_the_ground_and_comes_up_elsewhere() {
    let c = course("roy", "M1", 0, &[part(125, 14, 2)]);
    let mut w = world(&c);
    let mut hid = false;
    let mut moved = false;
    let x0 = koop(&w).x;
    for _ in 0..900 {
        run(&mut w, 1);
        hid |= koop(&w).koopaling_hidden();
        if hid && !koop(&w).koopaling_hidden() && (koop(&w).x - x0).abs() > 1.0 {
            moved = true;
        }
    }
    assert!(hid && moved);
}

#[test]
fn iggy_walks_instead_of_hopping() {
    let c = course("iggy", "M1", 0, &[part(124, 14, 2)]);
    let mut w = world(&c);
    let mut walked = false;
    for _ in 0..300 {
        run(&mut w, 1);
        walked |= koop(&w).on_ground && koop(&w).vx.abs() > 0.5;
    }
    assert!(walked);
}

#[test]
fn nsmbu_koopalings_flinch_before_the_shell() {
    let c = course("koopaling_wu", "WU", 0, &[part(122, 14, 2)]);
    let mut w = world(&c);
    assert!(stomp_koopaling(&mut w));
    assert_eq!(koop(&w).koopaling_state(), 8, "Damage");
    run(&mut w, 59);
    assert_eq!(koop(&w).koopaling_state(), 8);
    run(&mut w, 2);
    assert!(koop(&w).koopaling_guarded() && koop(&w).koopaling_state() != 8);
}

// ---------------------------------------------------------------- every part, every style

#[test]
fn every_new_part_loads_and_plays_in_each_style() {
    for (style, theme) in [("M1", 0), ("M3", 6), ("MW", 0), ("WU", 6), ("3W", 0)] {
        let mut parts = vec![
            part(74, 7, 2),
            with_flags(part(74, 8, 2), 0x4),
            part(78, 10, 2),
        ];
        if style == "3W" {
            parts.extend([
                part(79, 13, 5),
                with_flags(part(79, 14, 5), 0x4),
                part(80, 16, 2),
                part(132, 20, 2),
                with_flags(part(132, 25, 2), 0x4),
            ]);
        } else {
            parts.extend([
                mecha_part(13, 0),
                mecha_part(15, 1),
                mecha_part(17, 2),
                part(120, 20, 2),
                part(121, 23, 2),
                part(122, 26, 2),
                part(123, 29, 2),
                part(124, 32, 2),
                part(125, 35, 2),
                part(126, 38, 2),
            ]);
        }
        let c = course(&format!("all_{style}"), style, theme, &parts);
        let mut w = World::from_course(&c);
        w.player.invuln = 1_000_000;
        press(&mut w, input::RIGHT | input::RUN, 600);
        assert!(w.player.alive(), "{style}");
    }
}

#[test]
fn mechakoopas_hop_and_hover_in_the_night_ground_theme() {
    let mut c = course("mecha_night", "M1", 0, &[mecha_part(16, 0)]);
    c.main.header.flags |= 2; // night
    let mut w = world(&c);
    let mut top: f32 = 0.0;
    for _ in 0..300 {
        run(&mut w, 1);
        top = top.max(mecha(&w).y);
    }
    // vy 0.8 braking 0.008/f to 0.4, then a hover: about 30 px up
    assert!(
        top > 32.0 + 20.0 && top < 32.0 + 60.0,
        "hop height {}",
        top - 32.0
    );
}
