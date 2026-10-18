# SMCraft

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
scripts/make-app.sh        # Rust core + Swift release build → build/SMCraft.app (ad-hoc signed, own icon)
open build/SMCraft.app
scripts/run-app.sh         # or: build and run without bundling
```

The app is called SMCraft; the name, bundle id and version live only in
`app/Sources/MMKApp/Identity.swift` (the bundle script reads them from there).

- **First launch:** if no asset cache is found the app asks for the **RomFS** and **ExeFS** folders
  extracted from your own dump (plus, optionally, the update's RomFS/ExeFS) and imports them into
  `~/Library/Application Support/SMCraft/assets/<build>/`. Or choose **Use placeholder art** to play
  without a dump (coloured boxes; encrypted `.bcd` courses need imported data for their key).
- **Library** (home screen): every course file in `~/Library/Application Support/SMCraft/courses/`.
  **Add courses…** (⌘O) copies `.course.zlib`, `.bcd` or raw course files, or whole folders of
  them, into it. Search, filter (playable / cleared), sort; double-click or **Play**. Courses in
  styles the engine doesn't support yet are marked **Not yet supported**.
- **In game:** Esc / P / controller + pauses: Continue, Restart course, Exit to library. Clearing
  a course or losing all lives returns to the library. With the owner's SMM2 UI import
  (`mmk import-smm2-ui`) the screens are SMM2's own layouts: the HUD per game style, the course
  card (start, after each death), the pause menu (✕ Continue, Start Over / Retry, Exit Course →
  "You gave up..." with Exit / Keep playing), course clear (Start Over / Quit), "TIME'S UP!"
  and game over; ↑/↓ or ←/→ choose, Return / A confirms, Esc / B goes back.
  `MMK_UI_SMM2=off` keeps the drawn HUD and the plain menus.
- **100 Mario Challenge:** with the owner's SMM1 UI import (`mmk import-smm1`, found in
  `MMK_UI_CACHE`, `…/SMCraft/assets/smm1-*` or `~/smm-work/cache/smm1-*`) the screens are SMM1's own
  layouts: difficulty tabs over the world map (←/→ difficulty, ↑/↓ Continue / New Game), Mario's
  walk, the intro card, pause, course clear, game over, ending and credits. Hold **−** or **Tab**
  (controller −; Settings "Skip course") through the 3-step countdown to skip a course; letting go
  cancels. Without the import, simple stand-in screens are used (`MMK_UI_SMM1=off` forces them).
- **Save data:** best time, clears and attempts per course in
  `~/Library/Application Support/SMCraft/saves.json`, keyed by a hash of the course data.

### Settings (UserDefaults)

| Key | Type | Default | Used by |
|---|---|---|---|
| `volume.master`, `volume.music`, `volume.sfx` | Double 0…1 | 1 | audio output |
| `display.integerScale` | Bool | true | renderer (whole-number scaling, letterboxed) |
| `display.fullscreen` | Bool | false | window (kept in sync both ways) |
| `input.keys` | JSON `{action: [key code]}` | arrows/WASD, Z/Space jump, X/Shift run, Esc/P pause, −/Tab skip (100 Mario) | input |
| `input.pad` | JSON `{action: [button]}` | A/B jump, X/Y run, Menu pause, Options (−) skip | input |
| `assets.placeholder` | Bool | false | user chose to run without a dump |
| `input.touch` | Bool | true | iOS: on-screen controls (hidden while a controller is connected) |

Read them with `UserDefaults.standard.double(forKey: "volume.music")` etc.; defaults are
registered at launch (`SettingsKey.registerDefaults`).

### Environment variables (development, smoke tests)

| Variable | Effect |
|---|---|
| `MMK_COURSE=<file>` | skip the menus and play this course |
| `MMK_SCREENSHOT=<png>` [`MMK_SCREENSHOT_TICKS=<n>`] | scripted run, save the frame after n ticks (default 150), quit; skips the menus |
| `MMK_START_X/Y=<px>` | start the player somewhere else (with the two above) |
| `MMK_ASSETS=<dir>` | asset cache to use; `placeholder` = none |
| `MMK_SUPPORT_DIR=<dir>` | data folder instead of `~/Library/Application Support/SMCraft` |
| `MMK_UI_SCREENSHOT=<png>` [`MMK_UI_SCREENSHOT_DELAY=<s>`] [`MMK_UI_SCREEN=import\|settings`] | capture the window (UI included) and quit; with `MMK_COURSE`, `MMK_UI_PAUSE_TICKS=<n>` pauses after n ticks and `MMK_UI_SCRIPTED=1` uses the scripted run. More screens: `challenge`, `challenge-intro`/`-map`, `-walk`, `-pause`, `-clear`, `-skip`, `-card`, `-gameover`, `-ending` (`DebugCapture.swift`); `MMK_UI_TICKS=<n>` holds SMM1 layout screens at tick n; `MMK_UI_DIFFICULTY=<0-3>`, `MMK_UI_CLEARED=<n>`. SMM2 in-course screens over `MMK_COURSE`: `course-clear`, `course-gameover`, `course-timeup`, `course-retire`, `course-death` shown `MMK_UI_COURSE_AT=<s>` seconds in; `MMK_UI_INTRO=0` skips the course card |
| `MMK_UI2_CACHE=<dir>` | SMM2 UI import to use; `MMK_UI_SMM2=off` = no SMM2 title / in-course layouts |
| `MMK_UI_CACHE=<dir>` | SMM1 UI import to use; `MMK_UI_SMM1=off` = stand-in 100 Mario screens |
| `MMK_DEBUG_MISSING_ART=1` | draw boxes for sprites whose art isn't in the cache |
| `MMK_CPU_TEXTURES=1` | decode BC textures on the CPU, as on GPUs without BC support |
| `MMK_TOUCH=on`, `MMK_TOUCH_HOLD=right,jump,…` | iOS: show the touch controls even with a controller connected; hold controls from launch |
| `MMK_ORIENTATION=landscape\|portrait` | iOS: rotate at launch (Simulator captures) |

## The iOS / iPadOS app

The same Swift package and Rust core, built for iOS 17+ (DECISIONS.md, "iOS packaging"). Needs
the Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim` (the script adds them with
rustup).

```sh
scripts/make-ios-app.sh                 # Simulator build → build/ios-sim/SMCraft.app (ad-hoc signed)
xcrun simctl boot "iPad Pro 11-inch (M5)" && xcrun simctl install booted build/ios-sim/SMCraft.app
xcrun simctl launch booted dev.smcraft.app
SIMCTL_CHILD_MMK_COURSE=/path/course.zlib xcrun simctl launch booted dev.smcraft.app   # env vars: SIMCTL_CHILD_ prefix
xcrun simctl io booted screenshot shot.png

MMK_IOS_SIGN_IDENTITY="Apple Development: …" MMK_IOS_PROFILE=SMCraft.mobileprovision \
  scripts/make-ios-app.sh --device      # device build → build/ios-device/SMCraft.app
```

- **Game data:** the importer runs on the Mac. Copy the cache folder it made (`smm2-3.0.3`, and
  `smm1-…` / `smm2-ui-…` for the menus) with the Files app into **On My iPad › SMCraft › assets**, or
  choose it on the set-up screen from anywhere Files reaches and the app copies it in. Without
  one, **Use placeholder art**. The Simulator also finds the Mac's `~/smm-work/cache`.
- **Courses:** **+** in the library (Files picker: course files or a folder), or copy them into
  **On My iPad › SMCraft › courses**.
- **Controls:** on-screen d-pad, Jump / Run / Spin, **+** pause (back in menus), **−** held to
  skip a 100 Mario course; any GameController pad (they hide the touch controls); hardware
  keyboards with the default (or Mac-made) key bindings. Settings: **More › Settings…**.
- **Installing on the owner's iPad/iPhone over Tailscale**, once signed:
  `ios-install-tailscale build/ios-device/SMCraft.app --device ipad` (the owner's tool; it installs,
  it doesn't sign).

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

### In a browser

```sh
scripts/build-web.sh                          # needs wasm-bindgen-cli at the Cargo.lock version
python3 -m http.server -b 127.0.0.1 -d core/target/web 8000   # then open http://127.0.0.1:8000
```

Desktop Chromium (Chrome, Edge) or Firefox; WebGPU, or WebGL2 where it's missing. The page
starts on the demo strip with placeholder art. **Import game data…** takes a folder:

- an asset cache made by `mmk import` (e.g. `~/smm-work/cache/smm2-3.0.3`): copied in about
  15 s; or
- a folder holding your dump's extracted `romfs/` and `exefs/`: imported in the page by the same
  importer as `mmk import` (about 35 s, needs about 1.5 GB of memory while it runs).

Either way the result goes into the browser's own storage for this site (the origin private
file system, about 1.3 GB) and stays there for the next visit; nothing is uploaded anywhere.
**Remove game data** deletes it. Then pick a built-in course from the list, or **Open course
file…** (`.bcd`, `.course.zlib` or raw course data). Sound starts on the first click or key
press (browsers block it before that).

### Course Maker (`mmk-play --edit`)

```sh
cargo run --release -p mmk-shell -- --edit my-course.bin            # edit (a new course if the file doesn't exist)
cargo run --release -p mmk-shell -- --edit new.bin --style M3       # new course in SMB3 style
```

A debug-grade UI until the SMM2 maker layouts are drawn; the strip at the bottom says what
happened (F1 lists the keys).

| Keys / mouse | Does |
|---|---|
| left click / drag | place the selected part, paint ground while dragging |
| right click / drag | erase (a part, then a track piece, then ground); cancels a two-click action |
| 1–9, 0, Tab / Shift+Tab | pick the part (the palette also has Select, Track, Door, Warp Box, Scroll Node) |
| arrows | scroll |
| Door / Warp Box / Track tool | click one end, then the other (tracks: straight or 45°, even length) |
| W / F / B | wings / alternate form / big, on the part under the cursor |
| I | put the selected palette part inside the part under the cursor (block item, pipe contents, key) |
| = / − (Shift: height) | resize the part under the cursor (semisolids, platforms, bridges, pipes, belts, lifts, slopes, vines …) |
| R / C | turn / recolour the pipe under the cursor (R on empty space turns new pipes) |
| L | link pipes: L on a pipe, A to the other area, L on its pipe |
| K | door kind under the cursor: normal → P door → key door |
| G | put the part under the cursor on the nearest track (or take it off) |
| O | open / close the free track end under the cursor |
| H | draw the path of a snake block, ! block, track block or Piranha Creeper: arrows add steps, Backspace removes one, H again saves it |
| Select tool | drag a rectangle; Ctrl+C copy, Ctrl+X cut, Ctrl+V paste at the cursor, Delete / Backspace delete, Shift+arrows move |
| A / V / [ ] | other area / sub-area vertical or horizontal / area smaller or bigger |
| T / N | theme / night |
| Q | autoscroll: none, slow, normal, fast, custom (Scroll Node tool adds the nodes) |
| , / . (Shift: ×10) | timer −/+ 10 s |
| ; | clear condition (none, defeat the Goombas, collect the coins, as Super Mario, no damage, no landing) |
| ' | liquid (castle lava, forest water): stays, rises or falls, rises and falls |
| Ctrl+Z / Ctrl+Y / Ctrl+S / Ctrl+U | undo / redo / save / upload (`--server`) |
| Esc | drop the selection or a half-done action |
| Enter / E | test-play / back to editing |

The editor follows the game's placement rules: the start and goal areas stay clear, parts don't
overlap ground or each other (semisolids and platforms may), containers only take what the game
allows, part limits and style-only parts are enforced. The editing core is `core/crates/editor`;
`cargo run -p mmk-editor --example sample_course -- out.bin` writes a course that uses most of it.

## Online: the open-source course server

Our own self-hostable server; it never talks to Nintendo's network.

```sh
cargo run --release -p mmk-cli -- server --dir ./server-data --addr 0.0.0.0:8640
cargo run --release -p mmk-cli -- upload my-course.bin --server http://localhost:8640 --maker me
cargo run --release -p mmk-cli -- search "castle" --server http://localhost:8640 --sort popular
cargo run --release -p mmk-cli -- download ABC-DEF-GHJ --server http://localhost:8640
```

The API is described at the top of `core/crates/server/src/lib.rs`. Clients use
`core/crates/online`, which also feeds 100 Mario runs from a server.
