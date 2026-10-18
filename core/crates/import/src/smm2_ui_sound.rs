//! SMM2 UI sounds for the layout screens: which sound each layout's buttons and animations play,
//! as the game's own data says, and the sounds themselves.
//!
//! - `Pack/Static.pack` → `Param/Layout/<Layout>.byml` (one per layout with sounds): the layout's
//!   SLink user name, its buttons (part pane → sound type, e.g. `L_PlayBtn_00` → `makeplaybtn`)
//!   and the animations that have a sound. The keys are hashed names; the ones used here are
//!   constants below.
//! - `SLink2/SLink2DB.szs` (`mmk_formats::xlink`): per user, event keys → sound assets. Buttons
//!   use `UB_Select_<type>` / `UB_Decide_<type>` (plain `UB_Select` / `UB_Decide` without a
//!   type), animations `Anm_<Animation>`; some keys are containers whose child is chosen by a
//!   condition (controller vs touch: `Select_Key`; the scene for "back").
//! - `Sound/Resource/*.bars`: the sounds, decoded to 16-bit WAV.
//!
//! Output, next to the layouts: `ui/sound/sounds.json` and `ui/sound/<sound>.wav`.
//!
//! ```text
//! { "layouts": { "<Layout>": { "user": "UITitle", "buttons": {"<pane>": "<type>"},
//!                              "anims": ["PressKey", …] } },
//!   "users": { "<user>": { "<key>": {"sound": "se_…", "volume": 0.9, "delay": 10},
//!                          "<container key>": {"children": {"<key>": {…}}, "default": "<key>"} } } }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mmk_formats::archive::{self, Sarc};
use mmk_formats::audio::bars::Bars;
use mmk_formats::byml;
use mmk_formats::xlink::{Call, Xlnk};
use serde_json::{Map, Value, json};

use crate::{Result, read, write};

/// Hashed BYML keys of the layout parameter files.
const K_ROOT: &str = "799b9ac4";
const K_BUTTONS: &str = "213f7f84";
const K_PANE: &str = "d17c5197";
const K_TYPE: &str = "eedd76eb";
const K_USER: &str = "4309561a";
const K_ANIMS: &str = "a8e79f6f";
const K_ANIM: &str = "2c49113d";

/// What the import found.
#[derive(Default, Debug)]
pub struct SoundReport {
    pub layouts: usize,
    pub users: usize,
    pub sounds: usize,
    pub skipped: Vec<String>,
}

fn layout_params(romfs: &Path) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Ok(data) = read(&romfs.join("Pack/Static.pack")) else {
        return out;
    };
    let Ok(sarc) = archive::unwrap(data).and_then(Sarc::parse) else {
        return out;
    };
    for (name, d) in sarc.files() {
        let Some(layout) = name
            .strip_prefix("Param/Layout/")
            .and_then(|n| n.strip_suffix(".byml"))
        else {
            continue;
        };
        let Ok(v) = byml::parse(d) else { continue };
        let root = &v[K_ROOT];
        let Some(user) = root[K_USER].as_str() else {
            continue;
        };
        let mut buttons = Map::new();
        for b in root[K_BUTTONS].as_array().into_iter().flatten() {
            if let (Some(p), Some(t)) = (b[K_PANE].as_str(), b[K_TYPE].as_str()) {
                buttons.insert(p.to_string(), json!(t));
            }
        }
        let anims: Vec<Value> = root[K_ANIMS]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a[K_ANIM].as_str().map(|s| json!(s)))
            .collect();
        out.insert(
            layout.to_string(),
            json!({"user": user, "buttons": buttons, "anims": anims}),
        );
    }
    out
}

fn asset_json(c: &Call) -> Option<Value> {
    let sound = c.params.get("RuntimeAssetName").and_then(|p| p.as_str())?;
    let mut v = json!({"sound": sound});
    if let Some(vol) = c.params.get("Volume").and_then(|p| p.as_f32()) {
        v["volume"] = json!(vol);
    }
    if let Some(delay) = c.params.get("Delay").and_then(|p| p.as_f32()) {
        v["delay"] = json!(delay);
    }
    Some(v)
}

/// Call `i` as JSON: an asset, or a container with its children (in call order, each with its
/// key and whether a condition chooses it; nested containers kept).
fn node_json(calls: &[Call], i: usize, depth: usize) -> Option<Value> {
    let c = &calls[i];
    if !c.container {
        return asset_json(c);
    }
    if depth > 8 {
        return None;
    }
    let children: Vec<Value> = calls
        .iter()
        .enumerate()
        .filter(|(_, k)| k.parent == Some(i))
        .filter_map(|(j, k)| {
            let mut v = node_json(calls, j, depth + 1)?;
            v["key"] = json!(k.key);
            if k.conditional {
                v["conditional"] = json!(true);
            }
            Some(v)
        })
        .collect();
    (!children.is_empty()).then(|| json!({ "children": children }))
}

/// A user's calls as JSON: the top-level calls (assets or containers) by key.
fn user_json(calls: &[Call], sounds: &mut BTreeSet<String>) -> Value {
    let mut out = Map::new();
    for (i, c) in calls.iter().enumerate() {
        if c.parent.is_none()
            && let Some(v) = node_json(calls, i, 0)
        {
            out.insert(c.key.clone(), v);
        }
    }
    // every sound the kept calls name
    fn collect(v: &Value, s: &mut BTreeSet<String>) {
        if let Some(n) = v["sound"].as_str() {
            s.insert(n.to_string());
        }
        for c in v["children"].as_array().into_iter().flatten() {
            collect(c, s);
        }
    }
    for v in out.values() {
        collect(v, sounds);
    }
    Value::Object(out)
}

/// 16-bit PCM WAV.
fn wav(rate: u32, channels: &[Vec<i16>]) -> Vec<u8> {
    let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let ch = channels.len() as u16;
    let data_len = n * channels.len() * 2;
    let mut w = Vec::with_capacity(44 + data_len);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&ch.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(ch * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..n {
        for c in channels {
            w.extend_from_slice(&c[i].to_le_bytes());
        }
    }
    w
}

/// Write the sounds of BARS `data` that are still `left` as WAV files.
fn export_from(
    data: &[u8],
    ui: &Path,
    left: &mut BTreeSet<String>,
    report: &mut SoundReport,
) -> Result<()> {
    let Ok(bars) = Bars::parse(data) else {
        return Ok(());
    };
    for e in &bars.entries {
        if !left.contains(&e.name) {
            continue;
        }
        let Some(b) = bars.bwav(e) else { continue };
        left.remove(&e.name);
        if !b.is_complete() {
            report.skipped.push(format!("{}: streamed", e.name));
            continue;
        }
        let chans: std::result::Result<Vec<Vec<i16>>, _> =
            (0..b.channels.len()).map(|c| b.decode_channel(c)).collect();
        match chans {
            Ok(ch) => {
                write(
                    &ui.join(format!("sound/{}.wav", e.name)),
                    &wav(b.sample_rate(), &ch),
                )?;
                report.sounds += 1;
            }
            Err(err) => report.skipped.push(format!("{}: {err}", e.name)),
        }
    }
    Ok(())
}

/// Import the UI sounds into `<ui>/sound/`.
pub fn run(romfs: &Path, ui: &Path) -> Result<SoundReport> {
    let mut report = SoundReport::default();
    let layouts = layout_params(romfs);
    report.layouts = layouts.len();
    let users: BTreeSet<String> = layouts
        .values()
        .filter_map(|v| v["user"].as_str().map(String::from))
        .collect();
    let mut users_json = Map::new();
    let mut sounds = BTreeSet::new();
    let slink = read(&romfs.join("SLink2/SLink2DB.szs")).ok().and_then(|d| {
        let d = archive::unwrap(d).ok()?;
        let s = Sarc::parse(d).ok()?;
        s.files()
            .find(|(n, _)| n.ends_with(".bslnk"))
            .map(|(_, b)| b.to_vec())
    });
    match slink.as_deref().map(Xlnk::parse) {
        Some(Ok(x)) => {
            for u in &users {
                match x.user_calls(u) {
                    Ok(Some(calls)) => {
                        users_json.insert(u.clone(), user_json(&calls, &mut sounds));
                    }
                    Ok(None) => report.skipped.push(format!("SLink user {u}: not found")),
                    Err(e) => report.skipped.push(format!("SLink user {u}: {e}")),
                }
            }
        }
        Some(Err(e)) => report.skipped.push(format!("SLink2DB: {e}")),
        None => report.skipped.push("SLink2/SLink2DB.szs: missing".into()),
    }
    report.users = users_json.len();
    sounds.remove("@Blank");
    // the sounds: wherever they are among Sound/Resource/*.bars and the bars packs
    // (Pack/*_bars.pack: the styles' and the worlds' sounds)
    let mut left = sounds.clone();
    let listing = |dir: &Path, ext: &str| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = crate::read_dir(dir).unwrap_or_default();
        v.retain(|p: &std::path::PathBuf| p.to_string_lossy().ends_with(ext));
        v.sort();
        v
    };
    for path in listing(&romfs.join("Sound/Resource"), ".bars") {
        if left.is_empty() {
            break;
        }
        if let Ok(data) = read(&path) {
            export_from(&data, ui, &mut left, &mut report)?;
        }
    }
    for path in listing(&romfs.join("Pack"), "_bars.pack") {
        if left.is_empty() {
            break;
        }
        let Ok(sarc) = read(&path)
            .map_err(|_| ())
            .and_then(|d| archive::unwrap(d).and_then(Sarc::parse).map_err(|_| ()))
        else {
            continue;
        };
        for (name, data) in sarc.files() {
            if name.ends_with(".bars") {
                export_from(data, ui, &mut left, &mut report)?;
            }
        }
    }
    for s in &left {
        report.skipped.push(format!("sound {s}: not found"));
    }
    let doc = json!({"layouts": layouts, "users": users_json});
    write(
        &ui.join("sound/sounds.json"),
        serde_json::to_string_pretty(&doc)
            .unwrap_or_default()
            .as_bytes(),
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_44_bytes_and_interleaves() {
        let w = wav(48000, &[vec![1, 2], vec![3, 4]]);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(&w[44..52], &[1, 0, 3, 0, 2, 0, 4, 0]);
    }
}
