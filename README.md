# MarioMaker (working title)

An open-source, native macOS (Apple Silicon) engine that plays Super Mario Maker 2 courses and
adds Super Mario Maker 1's 100 Mario Challenge.

**Status:** Phase 5 (vertical slice, SMB1 style) and 5b (native app shell) in progress: Rust core +
Swift/Metal app that imports your own dump, lists your courses and plays SMB1-style ones.
See [PROGRESS.md](PROGRESS.md).

## What is and isn't in this repository

This repo contains only original code, original placeholder art, docs and tooling.

- No Nintendo code, assets, text, audio, executables or keys are committed, ever.
  `scripts/check_no_proprietary.py` enforces this in CI and as a pre-commit hook.
- The original game's assets are extracted at first launch from **your own** dump of the games.
- Without a dump, the engine runs on placeholder assets.

Reverse-engineering notes live in a private, gitignored `research/` folder. They are used as a
behavioural reference only; the engine is a clean implementation.

## Development setup

```sh
scripts/install-hooks.sh                         # pre-commit guard against proprietary files
python3 -m unittest scripts/test_check_no_proprietary.py
pip install -r requirements.txt && python -m unittest discover -s tests -t .
python -m smmfmt path/to/course   # summarise a course (.bcd needs --table, see docs/formats/smm2-course.md)
```

Tooling for working with your own dumps is described in [docs/TOOLCHAIN.md](docs/TOOLCHAIN.md).

## The macOS app

Requires macOS 14+, Apple Silicon, Xcode 27 (Swift 6.4) and Rust 1.99.0 (`core/rust-toolchain.toml`).

```sh
scripts/make-app.sh        # Rust core + Swift release build → build/MMK.app (ad-hoc signed, own icon)
open build/MMK.app
scripts/run-app.sh         # or: build and run without bundling
```

"MMK" is a working name; the name, bundle id and version live only in
`app/Sources/MMKApp/Identity.swift` (the bundle script reads them from there).

- **First launch:** if no asset cache is found the app asks for the **RomFS** and **ExeFS** folders
  extracted from your own dump (plus, optionally, the update's RomFS/ExeFS) and imports them into
  `~/Library/Application Support/MMK/assets/<build>/`. Or choose **Use placeholder art** to play
  without a dump (coloured boxes; encrypted `.bcd` courses need imported data for their key).
- **Library** (home screen): every course file in `~/Library/Application Support/MMK/courses/`.
  **Add courses…** (⌘O) copies `.course.zlib`, `.bcd` or raw course files, or whole folders of
  them, into it. Search, filter (playable / cleared), sort; double-click or **Play**. Courses in
  styles the engine doesn't support yet are marked **Not yet supported**.
- **In game:** Esc / P / controller + pauses: Continue, Restart course, Exit to library. Clearing
  a course or losing all lives returns to the library.
- **Save data:** best time, clears and attempts per course in
  `~/Library/Application Support/MMK/saves.json`, keyed by a hash of the course data.

### Settings (UserDefaults)

| Key | Type | Default | Used by |
|---|---|---|---|
| `volume.master`, `volume.music`, `volume.sfx` | Double 0…1 | 1 | audio output |
| `display.integerScale` | Bool | true | renderer (whole-number scaling, letterboxed) |
| `display.fullscreen` | Bool | false | window (kept in sync both ways) |
| `input.keys` | JSON `{action: [key code]}` | arrows/WASD, Z/Space jump, X/Shift run, Esc/P pause | input |
| `input.pad` | JSON `{action: [button]}` | A/B jump, X/Y run, Menu pause | input |
| `assets.placeholder` | Bool | false | user chose to run without a dump |

Read them with `UserDefaults.standard.double(forKey: "volume.music")` etc.; defaults are
registered at launch (`SettingsKey.registerDefaults`).

### Environment variables (development, smoke tests)

| Variable | Effect |
|---|---|
| `MMK_COURSE=<file>` | skip the menus and play this course |
| `MMK_SCREENSHOT=<png>` [`MMK_SCREENSHOT_TICKS=<n>`] | scripted run, save the frame after n ticks (default 150), quit; skips the menus |
| `MMK_START_X/Y=<px>` | start the player somewhere else (with the two above) |
| `MMK_ASSETS=<dir>` | asset cache to use; `placeholder` = none |
| `MMK_SUPPORT_DIR=<dir>` | data folder instead of `~/Library/Application Support/MMK` |
| `MMK_UI_SCREENSHOT=<png>` [`MMK_UI_SCREENSHOT_DELAY=<s>`] [`MMK_UI_SCREEN=import\|settings`] | capture the window (UI included) and quit; with `MMK_COURSE`, `MMK_UI_PAUSE_TICKS=<n>` pauses after n ticks and `MMK_UI_SCRIPTED=1` uses the scripted run |
| `MMK_DEBUG_MISSING_ART=1` | draw boxes for sprites whose art isn't in the cache |

## Portable shell (`mmk-play`: Windows, Linux, web, macOS)

A second front end written in Rust (winit + wgpu + gilrs + cpal) that plays a course with the
same core; see [docs/architecture.md](docs/architecture.md) §5b. Linux needs `libasound2-dev` and
`libudev-dev` to build.

```sh
cd core
cargo run --release -p mmk-shell -- path/to/course.zlib     # window; assets: $MMK_ASSETS or ~/smm-work/cache/smm2-3.0.3
cargo run --release -p mmk-shell -- --assets placeholder    # demo strip with placeholder art
cargo run --release -p mmk-shell -- course.zlib --screenshot out.png --ticks 150   # headless smoke test
cargo run --release -p mmk-shell -- --check-textures        # GPU vs CPU BCn decoding
../scripts/build-web.sh                                     # wasm build into core/target/web
```

Keys: arrows/WASD move, Z/Space jump, X/Shift run, Esc/P/Enter pause, R restart. Gamepads:
d-pad/left stick, south/east jump, west/north run, Start pause.
