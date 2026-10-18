//! Checks against the owner's imported asset cache. Skipped when there is none (CI): set
//! `MMK_ASSETS=<cache dir>` or import into `~/smm-work/cache/smm2-3.0.3`.

use std::path::PathBuf;
use std::sync::Arc;

use mmk_audio::{Bank, Command, Mixer, OUTPUT_RATE};

fn cache() -> Option<PathBuf> {
    let p = std::env::var_os("MMK_ASSETS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("smm-work/cache/smm2-3.0.3"))
        })?;
    p.join("audio/index.json").is_file().then_some(p)
}

#[test]
fn imported_sounds_decode_and_play() {
    let Some(root) = cache() else {
        eprintln!("skipped: no asset cache with audio");
        return;
    };
    let bank = Bank::load_cache(&root).expect("load audio bank");
    for name in [
        "jump_small",
        "coin",
        "bgm_plain",
        "bgm_plain_hurry",
        "fanfare_clear",
    ] {
        assert!(bank.id(name).is_some(), "{name} missing");
    }
    for s in &bank.sounds {
        assert!(s.frames > 0 && s.pcm().unwrap().len() == s.frames as usize * s.channels as usize);
        if let Some((a, b)) = s.loop_range {
            assert!(
                a < b && b <= s.frames,
                "{}: loop {a}..{b} of {}",
                s.name,
                s.frames
            );
        }
        // decoded ADPCM must not be noise: neighbouring samples are close compared with the level
        let ch = s.channels as usize;
        let x: Vec<f64> = s
            .pcm()
            .unwrap()
            .iter()
            .step_by(ch)
            .map(|&v| v as f64)
            .collect();
        let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        let diff = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / x.len() as f64;
        assert!(rms > 50.0, "{} is silent", s.name);
        assert!(
            diff < rms * 0.6,
            "{} looks like noise ({diff:.0} vs {rms:.0})",
            s.name
        );
    }
    let bgm = &bank.sounds[bank.id("bgm_plain").unwrap() as usize];
    assert_eq!(bgm.channels, 2);
    assert!(bgm.loop_range.is_some(), "ground music loops");

    // the ground music keeps playing past its loop end
    let mut m = Mixer::new(Arc::new(bank.clone()), OUTPUT_RATE);
    let id = bank.id("bgm_plain").unwrap();
    m.apply(Command::PlayMusic {
        sound: id,
        then: None,
    });
    let mut buf = vec![0.0f32; 48_000 * 2];
    let seconds = bgm.frames as usize / bgm.rate as usize + 2;
    for _ in 0..seconds {
        m.render(&mut buf);
    }
    assert_eq!(m.current_music(), Some(id));
    assert!(buf.iter().any(|&s| s != 0.0));
}
