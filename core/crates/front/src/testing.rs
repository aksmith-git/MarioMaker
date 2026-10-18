//! Test helpers: the owner's UI imports when this machine has them (the screens' layouts are
//! the game's data, never in the repository), so layout-driven tests run locally and are
//! skipped elsewhere (CI).

use std::path::PathBuf;

use crate::layout::{Ui, UiKind};
use crate::menu::UiInput;

/// `MMK_REQUIRE_UI=1` makes a missing import fail the test instead of skipping it.
fn import(env: &str, default: &str, game: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let found = [
        std::env::var_os(env).map(PathBuf::from),
        home.map(|h| h.join(default)),
    ]
    .into_iter()
    .flatten()
    .find(|d| {
        std::fs::read_to_string(d.join("manifest.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|m| m["game"] == game)
    });
    if found.is_none() && std::env::var("MMK_REQUIRE_UI").as_deref() == Ok("1") {
        panic!("no {game} UI import ({env})");
    }
    found
}

/// The SMM2 UI import (`$MMK_UI_CACHE`, else `~/smm-work/cache/smm2-ui-3.0.3`), if present.
pub fn smm2() -> Option<Ui> {
    let d = import("MMK_UI_CACHE", "smm-work/cache/smm2-ui-3.0.3", "SMM2")?;
    Ui::open(UiKind::Smm2, &d).ok()
}

/// The SMM1 UI import (`$MMK_UI_SMM1`, else `~/smm-work/cache/smm1-v208`), if present.
pub fn smm1() -> Option<Ui> {
    let d = import("MMK_UI_SMM1", "smm-work/cache/smm1-v208", "SMM1")?;
    Ui::open(UiKind::Smm1, &d).ok()
}

/// A click at `p` (layout units).
pub fn click(p: [f32; 2]) -> UiInput {
    UiInput {
        click: Some(p),
        ..Default::default()
    }
}

/// A key typed into a text field.
pub fn typed(c: char) -> UiInput {
    UiInput {
        text: Some(c),
        ..Default::default()
    }
}

/// Buttons pressed this tick.
pub fn press(b: crate::Buttons) -> UiInput {
    UiInput {
        pressed: b,
        ..Default::default()
    }
}
