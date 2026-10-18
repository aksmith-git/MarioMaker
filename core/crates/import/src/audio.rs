//! Audio stage: copy the sounds the engine uses out of the user's RomFS into `<cache>/audio/`.
//!
//! The engine refers to sounds by its own short ids; [`M1_SOUNDS`] maps each id to the name of
//! the game's wave, with the playback gain and exclusive group the engine uses for it. Each wave
//! is looked up by name: first as a complete stream file (`Sound/Resource/Stream/<name>.bwav`),
//! then inside the BARS archives (loose ones in `Sound/Resource/`, then those in the style's and
//! the static sound packs). The BWAV bytes are written unchanged as `audio/<id>.bwav`, and
//! `audio/index.json` records the id → file mapping and playback parameters.

use std::path::{Path, PathBuf};

use mmk_formats::archive::{self, Sarc};
use mmk_formats::audio::{Bars, Bwav};
use serde_json::{Map, Value, json};

use crate::{ImportError, Progress, Result, fmt_err, is_file, read, write};

/// Exclusive playback groups: a new sound in a group cuts the one playing.
pub const GROUP_NONE: u8 = 0;
pub const GROUP_SQUARE_A: u8 = 1;
pub const GROUP_SQUARE_B: u8 = 2;
pub const GROUP_NOISE: u8 = 3;

#[derive(Clone, Copy, Debug)]
pub struct SoundDef {
    /// Engine sound id (also the cache file stem).
    pub id: &'static str,
    /// Name of the game's wave.
    pub game: &'static str,
    pub gain: f32,
    pub group: u8,
}

const fn sfx(id: &'static str, game: &'static str, gain: f32, group: u8) -> SoundDef {
    SoundDef {
        id,
        game,
        gain,
        group,
    }
}

/// Sounds for the SMB1 style slice. Gains are the game's per-sound volumes.
pub const M1_SOUNDS: &[SoundDef] = &[
    sfx("jump_small", "M1_SmallMarioJump", 0.21, GROUP_SQUARE_A),
    sfx("jump_big", "M1_BigMarioJump", 0.21, GROUP_SQUARE_A),
    sfx("coin", "M1_Coin", 0.2, GROUP_NONE),
    sfx("stomp", "M1_StepNokonoko", 0.15, GROUP_SQUARE_A),
    sfx("kick_a", "M1_KickNokonoko_a", 0.3, GROUP_SQUARE_A),
    sfx("kick_b", "M1_KickNokonoko_b", 0.3, GROUP_SQUARE_A),
    sfx("bump", "M1_HitFloor", 0.2, GROUP_NONE),
    sfx("break", "M1_BreakBlock", 0.2, GROUP_NOISE),
    sfx("item_appear", "M1_PowerUpItemAppear", 0.22, GROUP_NONE),
    sfx("power_up", "M1_PowerUp", 0.2, GROUP_SQUARE_B),
    sfx("power_down", "M1_PowerDown", 0.18, GROUP_SQUARE_A),
    sfx("one_up", "M1_OneUp", 0.2, GROUP_SQUARE_B),
    sfx("fireball", "M1_FireBall", 0.25, GROUP_SQUARE_A),
    sfx("pole", "M1_FlagScoreUp", 0.15, GROUP_NONE),
    sfx("timer_count", "M1_TimerCountDown", 0.4, GROUP_NONE),
    // music
    sfx("fanfare_down", "M1_Fanfare_PlayerDown", 0.38, GROUP_NONE),
    sfx("fanfare_clear", "M1_Fanfare_CourseClear", 0.22, GROUP_NONE),
    sfx("fanfare_hurry", "M1_Fanfare_HurryUp", 0.22, GROUP_NONE),
    sfx("bgm_plain", "M1_BGM_Plain_Play", 1.0, GROUP_NONE),
    sfx("bgm_plain_hurry", "M1_BGM_Plain_PlayHurry", 1.0, GROUP_NONE),
    sfx("bgm_star", "M1_BGM_Star", 0.26, GROUP_NONE),
    sfx("bgm_star_hurry", "M1_BGM_Star_Hurry", 0.25, GROUP_NONE),
];

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v = crate::read_dir(dir).unwrap_or_default();
    v.retain(|p| p.extension().is_some_and(|x| x == ext));
    v
}

/// BARS archives to search, in order, as (label, bytes) loaded on demand.
fn bars_sources(romfs: &Path, styles: &[&str]) -> Vec<PathBuf> {
    let mut v = sorted_files(&romfs.join("Sound/Resource"), "bars");
    for s in styles {
        v.push(romfs.join(format!("Pack/{s}_bars.pack")));
    }
    v.push(romfs.join("Pack/StaticHighFreqSound.pack"));
    v.push(romfs.join("Pack/StaticSound.pack"));
    v.retain(|p| is_file(p));
    v
}

/// BARS blobs in a file: the file itself, or every BARS member of a SARC pack.
fn bars_blobs(path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let data = archive::unwrap(read(path)?).map_err(fmt_err(path))?;
    if data.starts_with(b"SARC") {
        let sarc = Sarc::parse(data).map_err(fmt_err(path))?;
        Ok(sarc
            .files()
            .filter(|(_, d)| d.starts_with(b"BARS"))
            .map(|(n, d)| (format!("{}:{n}", path.display()), d.to_vec()))
            .collect())
    } else if data.starts_with(b"BARS") {
        Ok(vec![(path.display().to_string(), data)])
    } else {
        Ok(Vec::new())
    }
}

/// Export `defs` from `romfs` into `out/audio/`. Returns the number of sounds written.
pub fn import_audio(
    romfs: &Path,
    out: &Path,
    styles: &[&str],
    defs: &[SoundDef],
    progress: Progress,
) -> Result<usize> {
    let mut found: Vec<Option<(String, Vec<u8>)>> = vec![None; defs.len()];
    // 1. complete stream files
    for (i, d) in defs.iter().enumerate() {
        let p = romfs.join(format!("Sound/Resource/Stream/{}.bwav", d.game));
        if is_file(&p) {
            let bytes = read(&p)?;
            if Bwav::parse(&bytes).is_ok_and(|w| w.is_complete()) {
                found[i] = Some((p.display().to_string(), bytes));
            }
        }
    }
    // 2. BARS archives, until every sound is found
    let sources = bars_sources(romfs, styles);
    for (k, src) in sources.iter().enumerate() {
        if found.iter().all(Option::is_some) {
            break;
        }
        progress("audio", k, sources.len());
        for (label, blob) in bars_blobs(src)? {
            let bars = Bars::parse(&blob).map_err(fmt_err(src))?;
            for (i, d) in defs.iter().enumerate() {
                if found[i].is_some() {
                    continue;
                }
                let Some(w) = bars.find(d.game).and_then(|e| bars.bwav(e)) else {
                    continue;
                };
                if w.is_complete()
                    && let Ok(bytes) = w.bytes()
                {
                    found[i] = Some((label.clone(), bytes.to_vec()));
                }
            }
        }
    }
    progress("audio", sources.len(), sources.len());
    // ids with a style prefix ("M3.coin") are optional variants: absent ones are skipped
    let optional = |d: &SoundDef| d.id.contains('.');
    let missing: Vec<&str> = defs
        .iter()
        .zip(&found)
        .filter(|(d, f)| f.is_none() && !optional(d))
        .map(|(d, _)| d.game)
        .collect();
    if !missing.is_empty() {
        return Err(ImportError::Invalid(format!(
            "sounds not found in the RomFS: {}",
            missing.join(", ")
        )));
    }
    let mut index = Map::new();
    for (d, f) in defs.iter().zip(found) {
        let Some((source, bytes)) = f else { continue };
        let w = Bwav::parse(&bytes).map_err(fmt_err(Path::new(&source)))?;
        let file = format!("{}.bwav", d.id);
        write(&out.join("audio").join(&file), &bytes)?;
        index.insert(
            d.id.to_string(),
            json!({
                "file": file,
                "game": d.game,
                "gain": d.gain,
                "group": d.group,
                "rate": w.sample_rate(),
                "channels": w.channels.len(),
                "samples": w.samples(),
                "loop": w.loop_range().map(|(s, e)| vec![s, e]),
            }),
        );
    }
    let n = index.len();
    write(
        &out.join("audio/index.json"),
        serde_json::to_string_pretty(&Value::Object(index))
            .unwrap()
            .as_bytes(),
    )?;
    Ok(n)
}

/// [`M1_SOUNDS`] plus each other style's variant of every sound effect and fanfare
/// (`<style>.<id>`, the game's `M1_` name with the style's prefix). Music comes from the sound
/// database stage (`crate::sound`); variants a style doesn't have are skipped.
pub fn all_style_sounds(styles: &[&str]) -> Vec<SoundDef> {
    let mut v = M1_SOUNDS.to_vec();
    for s in styles.iter().filter(|s| **s != "M1") {
        for d in M1_SOUNDS.iter().filter(|d| !d.id.starts_with("bgm_")) {
            let Some(rest) = d.game.strip_prefix("M1_") else {
                continue;
            };
            v.push(SoundDef {
                id: Box::leak(format!("{s}.{}", d.id).into_boxed_str()),
                game: Box::leak(format!("{s}_{rest}").into_boxed_str()),
                gain: d.gain,
                group: d.group,
            });
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmk_formats::audio::bars;
    use mmk_formats::audio::bwav::{self, BuildChannel, Codec};
    use std::fs;

    fn wave(samples: &[i16], rate: u32, loop_range: Option<(u32, u32)>) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        bwav::build(&[BuildChannel {
            codec: Codec::Pcm16,
            pan: 2,
            sample_rate: rate,
            samples: samples.len() as u32,
            coefs: [0; 16],
            loop_range,
            data: &data,
        }])
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("mmk-import-audio-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// Synthetic RomFS: one loose BARS, one stream file, one SARC pack; all sounds synthetic.
    #[test]
    fn finds_sounds_in_streams_loose_bars_and_packs() {
        let root = temp_dir("ok");
        let romfs = root.join("romfs");
        let a = wave(&[1, 2, 3], 32000, None);
        let b = wave(&[4, 5, 6, 7], 44100, Some((1, 4)));
        let c = wave(&[8; 10], 48000, None);
        write(
            &romfs.join("Sound/Resource/Test_Player.bars"),
            &bars::build(&[("Synth_A", &a)]),
        )
        .unwrap();
        write(&romfs.join("Sound/Resource/Stream/Synth_B.bwav"), &b).unwrap();
        // a pack: a SARC holding a BARS
        let inner = bars::build(&[("Synth_C", &c), ("Synth_Unused", &a)]);
        write(
            &romfs.join("Pack/T1_bars.pack"),
            &mmk_formats::archive::sarc::build(&[("Sound/Resource/T1_Object.bars", &inner)]),
        )
        .unwrap();
        let defs = [
            sfx("a", "Synth_A", 0.5, GROUP_SQUARE_A),
            sfx("b", "Synth_B", 1.0, GROUP_NONE),
            sfx("c", "Synth_C", 0.25, GROUP_NOISE),
        ];
        let out = root.join("cache");
        let n = import_audio(&romfs, &out, &["T1"], &defs, &mut |_, _, _| {}).unwrap();
        assert_eq!(n, 3);
        let index: Value =
            serde_json::from_slice(&fs::read(out.join("audio/index.json")).unwrap()).unwrap();
        assert_eq!(index["b"]["loop"], json!([1, 4]));
        assert_eq!(index["a"]["group"], json!(1));
        assert_eq!(index["c"]["rate"], json!(48000));
        assert_eq!(fs::read(out.join("audio/a.bwav")).unwrap(), a);
        assert_eq!(fs::read(out.join("audio/b.bwav")).unwrap(), b);
        assert_eq!(fs::read(out.join("audio/c.bwav")).unwrap(), c);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_sound_is_an_error() {
        let root = temp_dir("missing");
        let romfs = root.join("romfs");
        fs::create_dir_all(romfs.join("Sound/Resource")).unwrap();
        let err = import_audio(
            &romfs,
            &root.join("cache"),
            &[],
            &[sfx("x", "Synth_Missing", 1.0, 0)],
            &mut |_, _, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("Synth_Missing"));
        let _ = fs::remove_dir_all(&root);
    }
}
