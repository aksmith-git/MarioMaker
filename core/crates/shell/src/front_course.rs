//! What the front end's hosts (`front_host` natively, `web_front` in the browser) share about
//! courses and the engine: decoding a course file with its save-data key, the library entry of a
//! course, and the engine's [`CourseStatus`] after a tick.

use mmk_formats::smm2::Course;
use mmk_front::CourseEntry;
use mmk_front::CourseStatus;
use mmk_front::host::Condition;
use mmk_sim::player::{Control, DOWN_FALL_FRAMES};
use mmk_sim::world::Event;

use crate::game::Game;

/// A course file (raw, zlib or an encrypted `.bcd`, which needs the key table): the course,
/// its decoded data and its save-data key.
pub fn decode(
    bytes: &[u8],
    key_table: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<(Course, Vec<u8>, String), String> {
    let (data, _) = mmk_host::course::decode_data(bytes, key_table)?;
    let c = Course::parse(&data).map_err(|e| e.to_string())?;
    let key = format!("{:016x}", fnv1a64(&data));
    Ok((c, data, key))
}

/// FNV-1a 64 of the decoded course data (the Swift app's save-data key).
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The engine plays this course's style and everything in it.
pub fn playable(c: &Course) -> bool {
    let style = c.header.game_style.as_str_lossy();
    mmk_sim::level::PLAYABLE_STYLES.contains(&style.as_str())
        && mmk_sim::level::unsupported_features(c).is_empty()
}

/// The library entry of course `c` (file `id`, named `stem` without its extension); built-in
/// courses (the owner's game data) get their collection and, without a name of their own, one
/// from it ("Story Mode 3").
pub fn entry(
    id: String,
    stem: &str,
    built_in: bool,
    c: &Course,
    key: String,
    modified: Option<i64>,
) -> CourseEntry {
    let collection = built_in.then(|| {
        if stem.starts_with("quest") {
            "Story Mode"
        } else if stem.starts_with("lesson") {
            "Lesson"
        } else if stem.starts_with("title") {
            "Title"
        } else {
            "Built-in"
        }
        .to_string()
    });
    let own = c.header.name.to_string_lossy();
    let name = if !own.is_empty() {
        own
    } else if let (Some(col), Some(n)) = (
        &collection,
        stem.rsplit('_').next().and_then(|n| n.parse::<u32>().ok()),
    ) {
        format!("{col} {}", n + 1)
    } else {
        stem.to_string()
    };
    CourseEntry {
        id,
        name,
        description: c.header.description.to_string_lossy(),
        style: c.header.game_style.as_str_lossy(),
        theme: c.main.header.theme,
        playable: playable(c),
        built_in,
        collection,
        key,
        modified,
        meta: Default::default(),
    }
}

/// Numbers inside names compare by value ("course 2" before "course 10").
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut text = String::new();
        let mut num = String::new();
        for ch in s.chars() {
            if ch.is_ascii_digit() {
                num.push(ch);
            } else {
                if !num.is_empty() {
                    out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                    num.clear();
                }
                text.extend(ch.to_lowercase());
            }
        }
        out.push((text, num.parse().unwrap_or(0)));
        out
    };
    key(a).cmp(&key(b))
}

/// What the screens read from the engine after a tick.
pub fn status(game: &Game) -> CourseStatus {
    let w = &game.session.world;
    let h = w.hud();
    let dying = !w.player.alive();
    let restart_in = match w.player.control {
        Control::DownPose { timer } => timer + DOWN_FALL_FRAMES,
        Control::DownFall { timer } => timer,
        _ => -1,
    };
    CourseStatus {
        score: h.score,
        coins: h.coins,
        lives: h.lives,
        time: h.time,
        cleared: h.cleared,
        dying,
        goal_event: w.events.contains(&Event::Goal),
        lives_gained: h.lives_gained,
        keys: w.keys,
        pink_coins: w.pink.len() as u32,
        pink_total: w.pink_total as u32,
        style: w.style as u32,
        character: w.main_char as u32,
        checkpoint: w.checkpoint.is_some(),
        time_up: dying && w.time == 0,
        player_x: w.player.x - w.camera_x,
        player_y: w.player.y - w.camera_y,
        restart_in,
        clear_color: mmk_host::clear_color(w, &game.cache),
        condition: condition(w),
    }
}

fn condition(w: &mmk_sim::World) -> Option<Condition> {
    use mmk_sim::clear::Goal;
    let c = &w.clear;
    let (kind, target) = match c.goal {
        Goal::None => return None,
        Goal::Defeat(t) => (1, t as u32),
        Goal::OneUps => (2, 0),
        Goal::Coins(v) => (3, v),
        Goal::Pow => (4, 0),
        Goal::PSwitch => (5, 0),
        Goal::Star => (6, 0),
        Goal::Super => (7, 0),
        Goal::Fire => (8, 0),
        Goal::Shoe => (11, 0),
        Goal::Raccoon => (13, 0),
        Goal::Cape => (14, 0),
        Goal::Propeller => (15, 0),
        Goal::Cat => (18, 0),
        Goal::Builder => (20, 0),
        Goal::Cloud => (21, 0),
        Goal::HoldingStone => (22, 0),
        Goal::BellTrees => (23, 0),
        Goal::NoClaw => (24, 0),
        Goal::NoWater => (25, 0),
        Goal::Helmet(h) => (26, h as u32),
        Goal::DryBonesShell => (27, 0),
        Goal::Superball => (28, 0),
        Goal::Mega => (29, 0),
        Goal::HoldingShell => (16, 0),
        Goal::Yoshi => (17, 0),
        Goal::ClownCar => (12, 0),
        Goal::KoopaCar => (19, 0),
        Goal::NoDamage => (9, 0),
        Goal::NoLanding => (10, 0),
        Goal::Unsupported(i) => (255, i as u32),
    };
    Some(Condition {
        kind,
        target,
        count: c.count,
        needed: c.n,
        met: c.met(w.player.at_goal()),
        violated: c.violated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["c10", "c2", "C1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["C1", "c2", "c10"]);
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    }
}
