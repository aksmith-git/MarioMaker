//! Test courses for the power-ups SMM2 added in its 2.0 / 3.0 updates: the second style
//! power-up (part 81 per style), the SMB1 Master Sword (part 20 with flag 0x4) and the 3D World
//! wearables (127–131). Each course is built with the editor, saved, reopened and played; the
//! checks are the specs' key numbers (research/specs/objects/81-style-powerup-2.md,
//! 127-headdress-items.md). `MMK_WRITE_COURSES=<dir>` also writes the courses there.

use mmk_editor::{DEFAULT_FLAGS, Editor};
use mmk_sim::World;
use mmk_sim::actors::{Kind, Mode};
use mmk_sim::input::{self, Input};
use mmk_sim::player::{self, Control, Form, Headdress, Link, Squirrel};

struct Part {
    id: i16,
    x: i32,
    y: i32,
    flags: u32,
    child: i16,
    child_flags: u32,
}

fn part(id: i16, x: i32, y: i32) -> Part {
    Part {
        id,
        x,
        y,
        flags: 0,
        child: -1,
        child_flags: 0,
    }
}

/// A 60-tile course in `style` / `theme` with a two-tile floor (start zone columns 0–6 kept
/// clear), saved and reopened.
fn course(name: &str, style: &str, theme: u8, parts: &[Part]) -> mmk_formats::smm2::Course {
    let mut ed = Editor::new(style);
    ed.set_name(name);
    ed.set_theme(theme);
    ed.set_timer(500);
    ed.set_length(60);
    ed.set_start_goal(2, 2);
    for x in 0..60 {
        for y in 0..2 {
            match ed.paint_ground(x, y) {
                Ok(()) | Err(mmk_editor::EditError::Protected) => {}
                Err(e) => panic!("ground at ({x}, {y}): {e:?}"),
            }
        }
    }
    for p in parts {
        let (w, h) = mmk_editor::default_size(p.id);
        let i = ed
            .place_sized(p.id, p.x, p.y, w, h, DEFAULT_FLAGS | p.flags)
            .unwrap_or_else(|e| panic!("part {}: {e:?}", p.id));
        if p.child >= 0 {
            ed.set_child(i, p.child, p.child_flags);
        }
    }
    let bytes = ed.save().unwrap();
    if let Ok(dir) = std::env::var("MMK_WRITE_COURSES") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/v3pu_{name}.bin"), &bytes).unwrap();
    }
    let back = Editor::open(&bytes).unwrap();
    assert!(
        mmk_sim::level::unsupported_features(&back.course).is_empty(),
        "{name}: {:?}",
        mmk_sim::level::unsupported_features(&back.course)
    );
    back.course
}

fn press(w: &mut World, buttons: u32, frames: usize) {
    for _ in 0..frames {
        w.step(Input { buttons });
    }
}

fn run(w: &mut World, frames: usize) {
    press(w, 0, frames);
}

fn world(c: &mmk_formats::smm2::Course) -> World {
    let mut w = World::from_course(c);
    run(&mut w, 2);
    w
}

fn count(w: &World, f: impl Fn(Kind) -> bool) -> usize {
    w.actors.iter().filter(|a| a.alive() && f(a.kind)).count()
}

fn first(w: &World, f: impl Fn(Kind) -> bool) -> &mmk_sim::actors::Actor {
    w.actors.iter().find(|a| f(a.kind)).expect("actor")
}

/// Walk right until the form changes (or `frames` run out), then let the change finish.
fn collect(w: &mut World, frames: usize) {
    let before = w.player.form;
    for _ in 0..frames {
        w.step(Input {
            buttons: input::RIGHT,
        });
        if w.player.form != before || w.player.pu.headdress.is_some() {
            break;
        }
    }
    run(w, 40);
}

/// A world whose player already has `form` (on the flat floor of a `style` course).
fn with_form(style: &str, theme: u8, form: Form, parts: &[Part]) -> World {
    let c = course(&format!("form_{form:?}_{style}"), style, theme, parts);
    let mut w = world(&c);
    w.player.form = form;
    run(&mut w, 2);
    w
}

// ---------------------------------------------------------------- shared rules

#[test]
fn part_81_gives_each_styles_form() {
    for (style, form) in [
        ("M1", Form::Smb2),
        ("M3", Form::Frog),
        ("MW", Form::Balloon),
        ("WU", Form::Squirrel),
        ("3W", Form::Boomerang),
    ] {
        let c = course(&format!("p81_{style}"), style, 0, &[part(81, 9, 2)]);
        let mut w = world(&c);
        assert_eq!(
            count(&w, |k| k
                == Kind::PowerItem {
                    what: mmk_sim::actors::STYLE2
                }),
            1,
            "{style}: the part-81 item"
        );
        collect(&mut w, 600);
        assert_eq!(w.player.form, form, "{style}");
    }
}

#[test]
fn blocks_give_small_mario_a_mushroom_for_part_81_but_the_sword_as_is() {
    let mut block81 = part(5, 10, 5);
    block81.child = 81;
    let mut sword = part(5, 16, 5);
    sword.child = 20;
    sword.child_flags = 0x4;
    let c = course("block_swap", "M1", 0, &[block81, sword]);
    let mut w = world(&c);
    let mut out = Vec::new();
    for bx in [10.0, 16.0] {
        w.player.x = bx * 16.0 + 8.0;
        w.player.vx = 0.0;
        let before: Vec<u32> = w.actors.iter().map(|a| a.id).collect();
        press(&mut w, input::JUMP, 12);
        // out of the way before it comes out
        w.player.x = 3.0 * 16.0;
        run(&mut w, 40);
        out.extend(
            w.actors
                .iter()
                .filter(|a| !before.contains(&a.id))
                .map(|a| a.kind),
        );
    }
    assert_eq!(
        out,
        vec![
            Kind::Mushroom,
            Kind::PowerItem {
                what: mmk_sim::actors::SWORD
            }
        ],
        "81 → mushroom for small Mario; the Master Sword as is"
    );
    // a big player gets the real thing
    let mut block = part(5, 10, 5);
    block.child = 81;
    let c = course("block_big", "M1", 0, &[block]);
    let mut w = world(&c);
    w.player.form = Form::Super;
    w.player.x = 10.0 * 16.0 + 8.0;
    press(&mut w, input::JUMP, 12);
    w.player.x = 3.0 * 16.0;
    run(&mut w, 40);
    assert_eq!(
        count(&w, |k| k
            == Kind::PowerItem {
                what: mmk_sim::actors::STYLE2
            }),
        1
    );
}

#[test]
fn every_update_form_drops_to_super_when_hit() {
    for form in [
        Form::Link,
        Form::Frog,
        Form::Balloon,
        Form::Squirrel,
        Form::Boomerang,
        Form::Smb2,
    ] {
        let mut p = mmk_sim::player::Player::new(50.0, 32.0);
        p.form = form;
        assert!(p.hurt());
        assert_eq!(p.form, Form::Super, "{form:?}");
        assert!(matches!(p.control, Control::Change { .. }));
        assert_eq!(p.invuln, player::INVULN_DAMAGE);
    }
    // Link and the balloon have the small body
    let mut p = mmk_sim::player::Player::new(50.0, 32.0);
    p.form = Form::Link;
    assert!(!p.big());
    p.form = Form::Balloon;
    assert!(!p.big());
    assert_eq!(p.sensors().head_y, 15.9);
    p.form = Form::Frog;
    assert!(p.big());
}

// ---------------------------------------------------------------- Master Sword (Link)

fn link_world(parts: &[Part]) -> World {
    let mut sword = part(20, 9, 2);
    sword.flags = 0x4;
    let mut all = vec![sword];
    all.extend(parts.iter().map(|p| Part {
        id: p.id,
        x: p.x,
        y: p.y,
        flags: p.flags,
        child: p.child,
        child_flags: p.child_flags,
    }));
    let c = course("link", "M1", 0, &all);
    let mut w = world(&c);
    collect(&mut w, 200);
    assert_eq!(w.player.form, Form::Link, "the Master Sword makes Link");
    w
}

#[test]
fn link_slashes_for_8_frames_and_cuts_a_goomba_in_front() {
    let mut w = link_world(&[part(0, 20, 2)]);
    // stand 16 px left of the Goomba, facing it
    let g = first(&w, |k| k == Kind::Goomba).x;
    w.player.x = g - 20.0;
    w.player.facing = 1;
    w.step(Input {
        buttons: input::RUN,
    });
    assert!(matches!(w.player.pu.link, Link::Slash(_)));
    let mut frames = 1;
    while matches!(w.player.pu.link, Link::Slash(_)) {
        w.step(Input::default());
        frames += 1;
    }
    assert_eq!(frames, player::SLASH_FRAMES);
    assert_eq!(
        count(&w, |k| k == Kind::Goomba),
        0,
        "the slash knocked it out"
    );
}

#[test]
fn link_dash_winds_up_60_frames_then_runs_at_3() {
    let mut w = link_world(&[]);
    w.player.vx = 0.0;
    press(&mut w, input::DOWN, 3);
    assert_eq!(
        w.player.pu.link,
        Link::Shield,
        "Down on the ground is the shield"
    );
    press(&mut w, input::DOWN | input::RUN, 1);
    assert!(matches!(w.player.pu.link, Link::Dash { phase: 0, .. }));
    press(&mut w, input::RUN, player::SLASH_FRAMES as usize - 1);
    assert!(matches!(w.player.pu.link, Link::Dash { phase: 1, .. }));
    press(&mut w, input::RUN, player::DASH_WINDUP as usize);
    assert!(matches!(w.player.pu.link, Link::Dash { phase: 2, .. }));
    press(&mut w, input::RUN, 2);
    assert_eq!(w.player.vx, player::DASH_SPEED * w.player.facing as f32);
    press(&mut w, 0, 1);
    assert_eq!(w.player.pu.link, Link::None, "letting go ends it");
}

#[test]
fn link_shield_blocks_a_fireball() {
    let mut w = link_world(&[]);
    press(&mut w, input::DOWN, 2);
    assert_eq!(w.player.pu.link, Link::Shield);
    let (px, py) = (w.player.x, w.player.y);
    let mut f = mmk_sim::actors::Actor::new(Kind::EnemyFire, px + 30.0, py + 2.0);
    f.vx = -1.0;
    f.dir = -1;
    w.actors.push(f);
    w.player.facing = 1;
    let form = w.player.form;
    press(&mut w, input::DOWN, 40);
    assert_eq!(count(&w, |k| k == Kind::EnemyFire), 0, "blocked");
    assert_eq!(w.player.form, form, "not hurt");
}

#[test]
fn link_bow_draws_24_frames_and_shoots_a_5_px_arrow() {
    let mut w = link_world(&[]);
    w.step(Input {
        buttons: input::SPIN,
    });
    assert!(matches!(w.player.pu.link, Link::Bow { .. }));
    // a tap: the arrow still flies once the draw is done
    let mut frames = 1;
    while count(&w, |k| k == Kind::PlayerArrow) == 0 && frames < 60 {
        w.step(Input::default());
        frames += 1;
    }
    assert_eq!(frames, player::BOW_DRAW, "minimum time to the shot");
    let a = first(&w, |k| k == Kind::PlayerArrow);
    assert!((a.vx.abs() - (5.0 - 0.038)).abs() < 1e-4, "vx {}", a.vx);
}

#[test]
fn link_down_thrust_hangs_then_drops_at_6() {
    let mut w = link_world(&[]);
    press(&mut w, input::JUMP, 12);
    w.step(Input {
        buttons: input::DOWN,
    });
    assert!(matches!(w.player.pu.link, Link::Thrust { .. }));
    let y = w.player.y;
    run(
        &mut w,
        player::THRUST_HANG as usize + player::THRUST_PAUSE as usize - 2,
    );
    assert_eq!(w.player.y, y, "hangs in place");
    run(&mut w, 2);
    assert!(w.player.vy <= -6.0 + 0.35, "drops: {}", w.player.vy);
}

#[test]
fn link_pulls_a_bomb_that_blows_up_bricks() {
    let mut w = link_world(&[]);
    press(&mut w, input::UP | input::RUN, 1);
    assert_eq!(count(&w, |k| k == Kind::LinkBomb), 1);
    assert!(w.player.held.is_some());
    run(&mut w, 2);
    press(&mut w, input::RUN, 1);
    assert!(w.player.held.is_none(), "thrown");
    run(&mut w, 200);
    // bricks beside the resting bomb
    let b = first(&w, |k| k == Kind::LinkBomb);
    let (tx, ty) = ((b.x / 16.0) as i32 + 1, (b.y / 16.0) as i32 + 1);
    w.level.set(tx, ty, mmk_sim::level::Cell::Solid);
    w.blocks.map.insert(
        (tx, ty),
        mmk_sim::blocks::Block::new(mmk_sim::blocks::BlockKind::Brick, None),
    );
    run(&mut w, 60);
    assert_eq!(count(&w, |k| k == Kind::LinkBomb), 0, "exploded");
    assert!(w.blocks.get(tx, ty).is_none(), "the blast broke the brick");
}

// ---------------------------------------------------------------- SMB2 Mushroom

#[test]
fn smb2_mushroom_charges_a_4_1_jump() {
    let mut w = with_form("M1", 0, Form::Smb2, &[]);
    press(&mut w, input::DOWN, player::CHARGE_FRAMES as usize + 2);
    assert_eq!(w.player.pu.charge, player::CHARGE_FRAMES);
    let y0 = w.player.y;
    w.step(Input {
        buttons: input::DOWN | input::JUMP,
    });
    // vy 4.1, then the held-jump gravity of the top band
    assert!(
        (w.player.vy - (player::CHARGE_VY - 0.06)).abs() < 1e-4,
        "{}",
        w.player.vy
    );
    let mut peak = 0.0f32;
    for _ in 0..80 {
        w.step(Input::default());
        peak = peak.max(w.player.y - y0);
    }
    // ≈ 98 px with the held gravity forced for the whole rise (spec §2.4)
    assert!(peak > 90.0 && peak < 106.0, "peak {peak}");
}

#[test]
fn smb2_mushroom_stands_on_lifts_and_throws_a_goomba() {
    let mut w = with_form("M1", 0, Form::Smb2, &[part(0, 14, 2), part(0, 18, 2)]);
    let g = first(&w, |k| k == Kind::Goomba);
    let (gx, gid) = (g.x, g.id);
    w.player.x = gx;
    w.player.y = 70.0;
    w.player.on_ground = false;
    run(&mut w, 30);
    assert_eq!(
        count(&w, |k| k == Kind::Goomba),
        2,
        "stood on it, not stomped"
    );
    press(&mut w, input::DOWN | input::RUN, 1);
    assert_eq!(w.player.held, Some(gid));
    assert!(w.player.pu.lift > 0, "LiftUp");
    press(&mut w, input::RUN, 20);
    assert_eq!(w.player.pu.lift, 0);
    assert_eq!(w.player.pu.carry_h, player::CARRY_HEIGHT);
    w.player.facing = 1;
    press(&mut w, 0, 1);
    let t = w.actors.iter().find(|a| a.id == gid).unwrap();
    assert_eq!(t.mode, Mode::Thrown);
    assert!((t.vx - 2.2).abs() < 0.2, "thrown at 2.2: {}", t.vx);
    run(&mut w, 200);
    assert!(
        count(&w, |k| k == Kind::Goomba) < 2,
        "the thrown Goomba took the other one out"
    );
}

// ---------------------------------------------------------------- Frog Suit

#[test]
fn frog_hops_20_frames_waits_10_and_jumps_3_928() {
    let mut w = with_form("M3", 0, Form::Frog, &[]);
    let mut hops = Vec::new();
    for _ in 0..90 {
        w.step(Input {
            buttons: input::RIGHT,
        });
        hops.push(w.player.pu.hop);
    }
    // the first hop starts once the 10-frame wait is over, and lasts 20 frames
    let start = hops.iter().position(|&h| h).unwrap();
    let len = hops[start..].iter().take_while(|&&h| h).count();
    assert_eq!(len, player::FROG_HOP as usize);
    let wait = hops[start + len..].iter().take_while(|&&h| !h).count();
    assert_eq!(wait, player::FROG_WAIT as usize);
    run(&mut w, 30);
    w.step(Input {
        buttons: input::JUMP,
    });
    assert!((w.player.vy - (player::FROG_JUMP - 0.06)).abs() < 1e-4);
}

#[test]
fn frog_swims_freely_with_no_gravity() {
    let c = course("frog_water", "M3", 9, &[]);
    let mut c = c;
    c.main.header.liquid_start_height = 8;
    c.main.header.liquid_end_height = 8;
    let mut w = world(&c);
    w.player.form = Form::Frog;
    run(&mut w, 80);
    assert!(w.player.in_water);
    let y = w.player.y;
    run(&mut w, 30);
    assert!((w.player.y - y).abs() < 0.01, "no gravity: stays put");
    press(&mut w, input::RIGHT, 30);
    assert!((w.player.vx - 1.0).abs() < 1e-4, "swim speed 1.0");
    press(&mut w, input::RIGHT | input::RUN, 30);
    assert!((w.player.vx - 2.0).abs() < 1e-4, "dash swim 2.0");
}

// ---------------------------------------------------------------- Power Balloon

#[test]
fn balloon_floats_drifts_at_0_5_and_jets_at_2() {
    let mut w = with_form("MW", 0, Form::Balloon, &[]);
    press(&mut w, input::UP, 60);
    assert!(
        (w.player.vy - 0.5).abs() < 1e-3,
        "floats up at 0.5: {}",
        w.player.vy
    );
    run(&mut w, 60);
    assert!(w.player.vy.abs() < 1e-3, "no gravity: hovers");
    press(&mut w, input::UP | input::RUN, 60);
    assert!(w.player.pu.jet);
    let s = w.player.vx.hypot(w.player.vy);
    assert!((s - 2.0).abs() < 0.05, "jet speed {s}");
}

#[test]
fn balloon_jet_bounces_off_a_wall() {
    let mut w = with_form("MW", 0, Form::Balloon, &[]);
    let wx = (w.player.x / 16.0) as i32 + 5;
    for y in 2..12 {
        w.level.set(wx, y, mmk_sim::level::Cell::Solid);
    }
    // up a little, then jet right into the wall
    press(&mut w, input::UP, 30);
    let mut bounced = false;
    for _ in 0..120 {
        w.step(Input {
            buttons: input::RIGHT | input::RUN,
        });
        if w.player.vx < 0.0 {
            bounced = true;
            break;
        }
    }
    assert!(bounced, "reflected off the wall");
    assert!(
        w.player.vx <= -0.5,
        "at least 0.5 px/f back: {}",
        w.player.vx
    );
    assert!(!w.player.pu.jet, "a head-on bounce ends the jet");
}

#[test]
fn squirrel_clings_to_a_wall_for_90_frames() {
    let mut w = with_form("WU", 0, Form::Squirrel, &[]);
    let wx = (w.player.x / 16.0) as i32 + 1;
    for y in 2..14 {
        w.level.set(wx, y, mmk_sim::level::Cell::Solid);
    }
    press(&mut w, input::JUMP, 20);
    let mut frames = 0;
    for _ in 0..200 {
        w.step(Input {
            buttons: input::RIGHT,
        });
        if matches!(w.player.pu.sq, Squirrel::Cling { .. }) {
            frames += 1;
        } else if frames > 0 {
            break;
        }
    }
    assert_eq!(frames, player::CLING_FRAMES as usize);
    assert_eq!(w.player.wall_slide, 1, "then the ordinary wall slide");
}

#[test]
fn boomerang_makes_items_hop() {
    let mut w = with_form("3W", 0, Form::Boomerang, &[part(20, 12, 2)]);
    w.player.facing = 1;
    run(&mut w, 30);
    let m = w
        .actors
        .iter()
        .position(|a| a.kind == Kind::Mushroom)
        .unwrap();
    w.actors[m].vx = 0.0;
    w.actors[m].x = w.player.x + 60.0;
    press(&mut w, input::RUN, 1);
    let mut hopped = false;
    for _ in 0..30 {
        w.step(Input::default());
        hopped |= w.events.contains(&mmk_sim::Event::BoomerangReaction);
    }
    assert!(hopped, "BoomerangReaction: the item hops");
    assert_eq!(count(&w, |k| k == Kind::Mushroom), 1, "not carried off");
}

#[test]
fn link_arrow_aims_up_and_down() {
    for (b, vx, vy) in [(input::UP, 3.5, 4.8), (input::DOWN, 3.5, -2.0)] {
        let mut w = link_world(&[]);
        press(&mut w, input::JUMP, 6);
        press(&mut w, input::SPIN | b, 30);
        press(&mut w, b, 1);
        let a = first(&w, |k| k == Kind::PlayerArrow);
        assert!((a.vx.abs() - (vx - 0.038)).abs() < 0.05, "{} {}", a.vx, vx);
        assert!((a.vy - (vy - 0.12)).abs() < 0.13, "{} {}", a.vy, vy);
    }
}

// ---------------------------------------------------------------- Super Acorn

#[test]
fn squirrel_boosts_once_per_air_time_and_glides() {
    let mut w = with_form("WU", 0, Form::Squirrel, &[]);
    press(&mut w, input::JUMP, 10);
    w.step(Input {
        buttons: input::JUMP | input::SPIN,
    });
    assert!(matches!(w.player.pu.sq, Squirrel::Flying { .. }));
    assert!(
        (w.player.vy - 2.0).abs() < 0.31,
        "boost vy 2.0: {}",
        w.player.vy
    );
    let mut top = w.player.vy;
    for _ in 0..60 {
        w.step(Input {
            buttons: input::JUMP,
        });
        top = top.max(w.player.vy);
    }
    assert!(top > 3.0, "the boost climbs toward 67.5°: {top}");
    // a second spin in the same air time does nothing
    press(&mut w, 0, 1);
    w.step(Input {
        buttons: input::SPIN,
    });
    assert!(!matches!(w.player.pu.sq, Squirrel::Flying { .. }));
    // boost used: it flutters at ≥ −1.5
    run(&mut w, 60);
    if !w.player.on_ground {
        assert!(w.player.vy >= -1.5 - 1e-3, "flutter cap: {}", w.player.vy);
    }
}

// ---------------------------------------------------------------- Boomerang Flower

#[test]
fn boomerang_one_at_a_time_out_160_px() {
    let mut w = with_form("3W", 0, Form::Boomerang, &[]);
    w.player.facing = 1;
    let x0 = w.player.x;
    press(&mut w, input::RUN, 1);
    run(&mut w, 5);
    assert_eq!(count(&w, |k| k == Kind::PlayerBoomerang), 1);
    press(&mut w, input::RUN, 1);
    run(&mut w, 8);
    assert_eq!(count(&w, |k| k == Kind::PlayerBoomerang), 1, "only one out");
    let mut far = 0.0f32;
    for _ in 0..40 {
        w.step(Input::default());
        if let Some(b) = w.actors.iter().find(|a| a.kind == Kind::PlayerBoomerang) {
            far = far.max(b.x - x0);
        }
    }
    assert!(
        (far - 160.0).abs() < 0.01,
        "first leg 160 px from the player: {far}"
    );
    // it comes back and is caught
    run(&mut w, 200);
    assert_eq!(count(&w, |k| k == Kind::PlayerBoomerang), 0);
}

// ---------------------------------------------------------------- wearables

fn wear(part_id: i16) -> World {
    let c = course(&format!("wear_{part_id}"), "3W", 0, &[part(part_id, 9, 2)]);
    let mut w = world(&c);
    collect(&mut w, 200);
    w
}

#[test]
fn wearables_are_put_on_and_absorb_one_hit() {
    for (id, h) in [
        (127, Headdress::Cannon),
        (128, Headdress::Propeller),
        (129, Headdress::Goomba),
        (130, Headdress::Bullet),
        (131, Headdress::RedPow),
    ] {
        let mut w = wear(id);
        assert_eq!(w.player.pu.headdress, Some(h), "part {id}");
        assert_eq!(w.player.form, Form::Small, "small Mario can wear one");
        assert!(w.player.hurt());
        assert_eq!(w.player.pu.headdress, None);
        assert_eq!(w.player.form, Form::Small, "the box took the hit");
        assert!(w.player.alive());
    }
}

#[test]
fn cannon_box_fires_every_30_frames_and_a_charged_shot() {
    let mut w = wear(127);
    w.player.facing = 1;
    press(&mut w, input::RUN, 1);
    let s = first(&w, |k| k == Kind::CannonShot);
    assert!((s.vx - 3.0).abs() < 1e-4, "{}", s.vx);
    // held 60 frames, released: the charged shot at 5.5
    press(&mut w, input::RUN, player::CANNON_CHARGE as usize);
    press(&mut w, 0, 1);
    assert!(
        w.actors
            .iter()
            .any(|a| a.kind == Kind::CannonShot && (a.vx - 5.5).abs() < 1e-4)
    );
    // mashing: one shot per 30 frames
    run(&mut w, 40);
    let mut shots = 0;
    for i in 0..60 {
        let b = if i % 2 == 0 { input::RUN } else { 0 };
        w.step(Input { buttons: b });
        shots += w
            .events
            .iter()
            .filter(|e| **e == mmk_sim::Event::CannonShot)
            .count();
    }
    assert_eq!(shots, 2, "30-frame cooldown");
}

#[test]
fn propeller_box_flies_three_times_per_air_time() {
    let mut w = wear(128);
    let mut flights = 0;
    for _ in 0..3 {
        press(&mut w, input::SPIN, 1);
        flights += 1;
        assert_eq!(w.player.pu.flights, flights);
        // a new flight starts from the fall (PropellerFall runs the spin handler) [obs]
        run(&mut w, 70);
        assert!(w.player.pu.prop_fall);
    }
    assert!(!w.player.on_ground);
    let f = w.player.pu.flights;
    press(&mut w, input::SPIN, 1);
    assert_eq!(w.player.pu.flights, f, "no fourth flight");
    run(&mut w, 300);
    assert!(w.player.on_ground);
    assert_eq!(w.player.pu.flights, 0, "refilled on landing");
}

#[test]
fn bullet_bill_mask_dashes_at_3_while_jump_is_held() {
    let mut w = wear(130);
    w.player.facing = 1;
    press(&mut w, input::JUMP, 60);
    assert!(
        w.player.pu.killer,
        "the dash starts as the jump turns to a fall"
    );
    assert!(w.player.vy.abs() < 0.5);
    press(&mut w, input::JUMP, 30);
    assert!((w.player.vx - 3.0).abs() < 1e-3, "{}", w.player.vx);
    press(&mut w, 0, 1);
    assert!(!w.player.pu.killer, "letting go of jump ends it");
}

#[test]
fn red_pow_box_has_three_head_bumps() {
    let mut w = wear(131);
    assert_eq!(w.player.pu.red_uses, player::RED_POW_USES);
    let x = (w.player.x / 16.0) as i32;
    w.level.set(x, 5, mmk_sim::level::Cell::Solid);
    press(&mut w, input::JUMP, 20);
    run(&mut w, 40);
    assert_eq!(w.player.pu.red_uses, player::RED_POW_USES - 1);
}

#[test]
fn goomba_mask_and_headdress_turn_off_fireballs() {
    let mut w = wear(129);
    w.player.form = Form::Fire;
    press(&mut w, input::RUN, 1);
    assert_eq!(count(&w, |k| k == Kind::Fireball), 0);
}
