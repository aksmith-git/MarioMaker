//! Tests of the data-driven sound director with a synthetic sound database and waves.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use mmk_audio::mixer::{MusicClock, SET_BASE, SharedSet, shared_set};
use mmk_audio::{Bus, Command, command_channel};
use mmk_formats::audio::bwav::{self, BuildChannel, Codec};
use mmk_sim::World;
use mmk_sim::actors::{Actor, Kind, Mode};
use mmk_sim::player::Form;
use mmk_sim::world::{Event, GoalPhase};
use serde_json::json;

use crate::sound::{FileReader, Library, STAR_END_SIGN, Sounds};

fn wave(n: u32, looped: bool) -> Vec<u8> {
    let data: Vec<u8> = (0..n)
        .flat_map(|i| ((i % 100) as i16).to_le_bytes())
        .collect();
    bwav::build(&[BuildChannel {
        codec: Codec::Pcm16,
        pan: 2,
        sample_rate: 48000,
        samples: n,
        coefs: [0; 16],
        loop_range: looped.then_some((0, n)),
        data: &data,
    }])
}

fn db() -> serde_json::Value {
    let asset = |key: &str, wave: &str| json!({"key": key, "wave": wave, "volume": 0.5});
    let bgm = vec![
        asset("BGM_Plain_Day", "PlainDay"),
        asset("BGM_Plain_Day_Hurry", "PlainHurry"),
        asset("BGM_Plain_Night", "PlainNight"),
        asset("BGM_Castle_Day", "CastleDay"),
        asset("BGM_Hurry", "HurryJingle"),
        asset("BGM_Star", "Star"),
        asset("BGM_StarHurry", "StarHurry"),
        asset("BGM_Switch", "Switch"),
        asset("BGM_SwitchHurry", "SwitchHurry"),
        asset("FanfareDown", "Down"),
        json!({"key": "FanfareGoal", "kind": "switch", "watch": "@theme", "children": [11, 12]}),
        json!({"key": "g_castle", "parent": 10, "if": ["==", "castle"], "wave": "CastleClear"}),
        json!({"key": "g_default", "parent": 10, "wave": "Clear"}),
    ];
    json!({
        "version": 1,
        "groups": [{"name": "Root"}],
        "attenuation": {},
        "effects": {},
        "waves": {},
        "users": {
            "CourseSceneBGM_M1": {"calls": bgm},
            "CourseSceneBGM_Common": {"calls": [
                {"key": "play1UpSe", "kind": "switch", "watch": "@style", "global": true, "children": [1, 2]},
                {"key": "u_m1", "parent": 0, "if": ["==", "M1"], "wave": "OneUpM1"},
                {"key": "u_wu", "parent": 0, "if": ["==", "WU"], "wave": "OneUpWU"},
                asset("playStarEndSign", "StarEnd")
            ]},
            "M1_Player": {"calls": [
                {"key": "jump", "kind": "switch", "watch": "PlayerMode", "children": [1, 2]},
                {"key": "jump_0", "parent": 0, "if": ["==", "Normal"], "wave": "JumpSmall"},
                {"key": "jump_1", "parent": 0, "if": ["==", "Super"], "wave": "JumpBig"},
                asset("pole", "Pole"),
                asset("swim", "Swim"),
                asset("kabekick", "WallKick")
            ]},
            "M1_JumpStep": {"calls": [asset("jump", "Spring"), asset("high_jump", "SpringHigh")]},
            "M1_PowBlock": {"calls": [asset("pow", "Pow")]},
            "M1_Bombhei": {"calls": [asset("ignite", "Fuse"), asset("explode", "Boom")]},
            "M1_ObjectCommon": {"calls": [asset("playCoin", "Coin"), asset("block_hit", "Bump")]},
            "M1_Kuribo": {"calls": [
                {"key": "r_damage_stamp", "kind": "switch", "watch": "ScaleLevel", "children": [1, 2]},
                {"key": "s_big", "parent": 0, "if": ["==", "Big"], "wave": "Split"},
                {"key": "s_normal", "parent": 0, "if": ["==", "Normal"], "wave": "Stomp"}
            ]},
            "M1_koura": {"calls": [asset("kick", "Kick")]}
        }
    })
}

struct Rig {
    s: Sounds,
    rx: Receiver<Command>,
    shared: SharedSet,
    clock: MusicClock,
}

fn rig() -> Rig {
    let mut doc = db();
    let mut waves = serde_json::Map::new();
    for u in doc["users"].as_object().unwrap().values() {
        for c in u["calls"].as_array().unwrap() {
            if let Some(w) = c["wave"].as_str() {
                let music = w.contains("Day") || w.contains("Night") || w == "Star";
                let dir = if music { "music" } else { "se" };
                waves.insert(w.into(), json!(format!("{dir}/{w}.bwav")));
            }
        }
    }
    doc["waves"] = serde_json::Value::Object(waves);
    let bytes = serde_json::to_vec(&doc).unwrap();
    let read: FileReader = Box::new(move |rel| {
        if rel == "sound.json" {
            Some(bytes.clone())
        } else {
            let looped = rel.starts_with("music/");
            Some(wave(if looped { 4800 } else { 480 }, looped))
        }
    });
    let lib = Arc::new(Library::load(read).unwrap());
    let (tx, rx) = command_channel(4096);
    let shared = shared_set();
    let clock: MusicClock = Arc::new(AtomicU64::new(u64::MAX));
    let s = Sounds::new(tx, lib, shared.clone(), clock.clone(), 48000);
    Rig {
        s,
        rx,
        shared,
        clock,
    }
}

impl Rig {
    /// Commands since the last call: "sfx:Wave", "music:Wave(>Then)(@start)", …
    fn played(&self) -> Vec<String> {
        let set = self.shared.lock().unwrap().clone().unwrap_or_default();
        let name = |id: u16| set.sounds[(id - SET_BASE) as usize].name.clone();
        self.rx
            .try_iter()
            .filter_map(|c| match c {
                Command::Play(p) if p.bus == Bus::Sfx => Some(format!("sfx:{}", name(p.sound))),
                Command::Play(p) => Some(format!(
                    "music:{}{}{}",
                    name(p.sound),
                    p.then.map(|t| format!(">{}", name(t))).unwrap_or_default(),
                    if p.start > 0 {
                        format!("@{}", p.start)
                    } else {
                        String::new()
                    }
                )),
                Command::StopMusic { .. } => Some("stop_music".into()),
                Command::SetMusicLayer { gain, .. } => Some(format!("layer:{gain}")),
                _ => None,
            })
            .collect()
    }

    /// The mixer reports music `wave` at `frame`.
    fn music_at(&self, wave: &str, frame: u32) {
        let set = self.shared.lock().unwrap().clone().unwrap();
        let id = set.sounds.iter().position(|s| s.name == wave).unwrap() as u64;
        self.clock.store(
            ((SET_BASE as u64 + id) << 32) | frame as u64,
            Ordering::Relaxed,
        );
    }
}

fn world() -> World {
    let mut w = World::new();
    w.level.style = "M1".into();
    w.level.theme = 0;
    w.level.night = false;
    w.time = 300 * 60;
    w.actors.clear();
    w
}

#[test]
fn course_music_per_theme_and_night() {
    let mut r = rig();
    let mut w = world();
    r.s.start_course(&w);
    assert_eq!(r.played(), ["layer:0", "music:PlainDay"]);
    w.level.night = true;
    r.s.start_course(&w);
    assert_eq!(r.played(), ["layer:0", "music:PlainNight"]);
    w.level.night = false;
    w.time = 90 * 60;
    r.s.start_course(&w);
    assert_eq!(
        r.played(),
        ["layer:0", "music:PlainHurry"],
        "a course starting in hurry time"
    );
}

#[test]
fn player_and_object_events_follow_the_data() {
    let mut r = rig();
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    w.events = vec![Event::Jump, Event::Coin];
    w.coins = 1;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:JumpSmall", "sfx:Coin"]);
    w.player.form = Form::Super;
    w.events = vec![Event::Jump];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:JumpBig"]);
    w.player.in_water = true;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:Swim"], "a jump in water is a swim stroke");
    w.player.in_water = false;
    w.events = vec![Event::OneUp];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:OneUpM1"], "the global style switch");
}

#[test]
fn stomps_and_kicks_come_from_the_actor_that_changed() {
    let mut r = rig();
    let mut w = world();
    let mut g = Actor::new(Kind::Goomba, 100.0, 32.0);
    g.id = 7;
    g.big = true;
    w.actors.push(g);
    let mut s = Actor::new(Kind::Shell { red: false }, 150.0, 32.0);
    s.id = 8;
    s.mode = Mode::ShellWait;
    w.actors.push(s);
    r.s.start_course(&w);
    r.played();
    w.actors[0].mode = Mode::Stomped;
    w.actors[1].mode = Mode::ShellSlide;
    w.events = vec![Event::Stomp, Event::Kick];
    r.s.after_step(&w);
    assert_eq!(
        r.played(),
        ["sfx:Split", "sfx:Kick"],
        "the big Goomba's stomp and the kick, once each"
    );
}

#[test]
fn hurry_star_resume_and_switch() {
    let mut r = rig();
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    // star: the theme track's position is kept and resumed (aligned to 1152 samples)
    r.music_at("PlainDay", 3000);
    w.player.star = 600;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:Star"]);
    w.player.star = STAR_END_SIGN;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:StarEnd"]);
    w.player.star = 0;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:PlainDay@2304"]);
    // hurry: jingle, then the hurry track from the top
    w.events = vec![Event::Hurry];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:HurryJingle>PlainHurry"]);
    w.events.clear();
    // a P switch after hurry-up: its hurry variant; the theme track restarts after it
    w.p_timer = 100;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:SwitchHurry"]);
    w.p_timer = 0;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:PlainHurry"]);
}

#[test]
fn death_goal_fanfares_and_yoshi_layer() {
    let mut r = rig();
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    w.player.yoshi = Some(3);
    r.s.after_step(&w);
    assert_eq!(r.played(), ["layer:1"]);
    w.player.yoshi = None;
    w.events = vec![Event::Goal];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["stop_music", "sfx:Pole", "layer:0"]);
    w.events.clear();
    w.goal_phase = GoalPhase::Walk;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:Clear"]);
    // a castle course plays the castle clear
    let mut w = world();
    w.level.theme = 2;
    r.s.start_course(&w);
    r.played();
    w.goal_phase = GoalPhase::Walk;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:CastleClear"]);
    // death
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    w.events = vec![Event::Death];
    w.player.kill();
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:Down"]);
}

#[test]
fn springs_wall_kicks_pow_and_bob_ombs() {
    let mut r = rig();
    let mut w = world();
    let mut spring = Actor::new(Kind::Spring, 100.0, 32.0);
    spring.id = 1;
    let mut pow = Actor::new(Kind::Pow, 200.0, 32.0);
    pow.id = 2;
    let mut bomb = Actor::new(Kind::BobOmb, 300.0, 32.0);
    bomb.id = 3;
    w.actors = vec![spring, pow, bomb];
    w.player.ride = Some(1);
    r.s.start_course(&w);
    r.played();
    // launched off the spring: its sound, not the jump
    w.player.ride = None;
    w.player.vy = 6.0;
    w.events = vec![Event::Jump];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:SpringHigh"]);
    // off a wall: the wall kick
    w.player.wall_slide = 5;
    w.events.clear();
    r.s.after_step(&w);
    w.player.wall_slide = 0;
    w.events = vec![Event::Jump];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:WallKick"]);
    // the POW goes off; the Bob-omb is lit, then explodes
    w.events.clear();
    w.actors[1].mode = Mode::Dead;
    w.actors[2].phase = 1;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:Pow", "sfx:Fuse"]);
    w.actors[2].phase = 9;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:Boom"]);
}

#[test]
fn costumes_keep_their_sounds() {
    let mut r = rig();
    let read: crate::costume_sound::CostumeReader = Arc::new(|n: &str| {
        matches!(
            n,
            "costume_045_jump" | "costume_045_jump2" | "costume_045_goal"
        )
        .then(|| wave(480, false))
    });
    r.s.set_costumes(Some(read));
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    w.player.form = Form::Costume;
    w.player.costume = 45;
    w.events = vec![Event::Jump];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:costume_045_jump"]);
    r.s.after_step(&w);
    assert_eq!(
        r.played(),
        ["sfx:costume_045_jump2"],
        "the second take every other jump"
    );
    // the costume's clear jingle replaces the clear fanfare
    w.events.clear();
    w.goal_phase = GoalPhase::Walk;
    r.s.after_step(&w);
    assert_eq!(r.played(), ["music:costume_045_goal"]);
    // without the costume: the style's jump
    let mut w = world();
    r.s.start_course(&w);
    r.played();
    w.events = vec![Event::Jump];
    r.s.after_step(&w);
    assert_eq!(r.played(), ["sfx:JumpSmall"]);
}
