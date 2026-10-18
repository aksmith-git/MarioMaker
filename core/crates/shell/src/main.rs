//! `mmk-play`: play a course in a window, or render a smoke-test frame offscreen.

#[cfg(target_arch = "wasm32")]
fn main() {
    mmk_shell::web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    match native::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mmk-play: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;
    use std::sync::Arc;

    use clap::Parser;
    use mmk_shell::app::{App, UserEvent, log};
    use mmk_shell::game::Game;
    use mmk_shell::source::{self, AssetSource, DirSource};
    use winit::event_loop::EventLoop;

    #[derive(Parser)]
    #[command(
        name = "mmk-play",
        about = "Play a Super Mario Maker 2 course (portable shell)",
        after_help = "Keys: arrows/WASD move, Z/Space jump, X/Shift run, Esc/P/Enter pause, R restart.\n\
                      Gamepads: d-pad/left stick, south/east jump, west/north run, Start pause."
    )]
    struct Args {
        /// Course file: SMM2 course data, zlib-compressed (.course.zlib) or encrypted (.bcd).
        /// Without one, the built-in demo strip plays.
        course: Option<PathBuf>,
        /// Asset cache imported from the owner's dump ("placeholder" for none). Default: $MMK_ASSETS,
        /// else ~/smm-work/cache/smm2-3.0.3 if it exists, else placeholder art.
        #[arg(long)]
        assets: Option<String>,
        /// Headless: render one frame offscreen after --ticks ticks, save it as PNG and exit.
        #[arg(long, value_name = "PNG")]
        screenshot: Option<String>,
        /// Headless: render a whole course area (no HUD, actors as placed) to a PNG and exit.
        #[arg(long, value_name = "PNG")]
        map: Option<String>,
        /// Headless: render this imported layout (SMM1 or SMM2 UI) at 1280×720 to --screenshot,
        /// like the Swift app's MMK_UI_LAYOUT preview.
        #[arg(long)]
        layout: Option<String>,
        /// With --layout: the UI import (default $MMK_UI_CACHE, else ~/smm-work/cache/smm1-v208).
        #[arg(long)]
        ui_cache: Option<PathBuf>,
        /// With --layout: animations, comma-separated: Name (from 0, holding the last frame),
        /// Name~ (looping), Name@frame (fixed); --ticks frames in.
        #[arg(long, default_value = "")]
        ui_anim: String,
        /// With --layout: text replacements, pane=text;pane=text.
        #[arg(long, default_value = "")]
        ui_text: String,
        /// With --map: 0 main area, 1 sub-area.
        #[arg(long, default_value_t = 0)]
        map_area: usize,
        /// Ticks to run before the screenshot (scripted run right + jumps, as the Swift app's
        /// MMK_SCREENSHOT).
        #[arg(long, default_value_t = 150)]
        ticks: u64,
        /// Screenshot without the scripted input (the player stands still).
        #[arg(long)]
        idle: bool,
        /// Screenshot size as a multiple of the 384×216 view.
        #[arg(long, default_value_t = 3)]
        scale: u32,
        /// Screenshot size in pixels, WxH (overrides --scale; the view is fitted, e.g. 1280x720
        /// to compare with the console's frames).
        #[arg(long)]
        size: Option<String>,
        /// Screenshot: draw the view with its bottom-left at X,Y world px instead of where the
        /// camera is (to line frames up with reference captures; nothing else changes).
        #[arg(long, value_name = "X,Y")]
        view_at: Option<String>,
        /// Decode BC textures on the CPU even if the GPU samples them (tests the fallback).
        #[arg(long)]
        cpu_textures: bool,
        /// Compare the GPU's BC sampling with the CPU decoder for every BC texture, then exit.
        #[arg(long)]
        check_textures: bool,
        /// No sound.
        #[arg(long)]
        mute: bool,
        /// Smoke test: close the window after this many seconds and report frames and ticks.
        /// With --screenshot's script (--scripted), the player runs right and jumps.
        #[arg(long, value_name = "SECONDS")]
        quit_after: Option<f64>,
        /// Play the smoke-test script in the window instead of reading input.
        #[arg(long)]
        scripted: bool,
        /// Course server (open-source mmk-server) for --code and the maker's Ctrl+U upload.
        #[arg(long)]
        server: Option<String>,
        /// Download this course code from --server and play it.
        #[arg(long)]
        code: Option<String>,
        /// Maker name for uploads.
        #[arg(long, default_value = "anonymous")]
        maker: String,
        /// With --code: race the course record's run as a ghost (or a replay file's, --ghost-file).
        #[arg(long)]
        ghost: bool,
        /// Save your run as a replay file (`.mmkr`): on each clear, or with --screenshot, the
        /// scripted run when it's taken.
        #[arg(long)]
        record_replay: Option<PathBuf>,
        /// Race this replay file (`.mmkr`) as a ghost.
        #[arg(long)]
        ghost_file: Option<PathBuf>,
        /// Account token for the course server (default $MMK_TOKEN): with --code, each clear is
        /// sent as a replay the server verifies (world records).
        #[arg(long)]
        token: Option<String>,
        /// Online play (rollback over UDP) with known addresses: ours, e.g. 0.0.0.0:7000.
        #[arg(long)]
        net_bind: Option<String>,
        /// Online play: another peer's address, e.g. 192.168.1.20:7000 (repeat for 3–4 players).
        #[arg(long)]
        net_peer: Vec<String>,
        /// Online play through a course server's relay (works behind NAT): its UDP address,
        /// e.g. example.org:8640, with --net-room.
        #[arg(long)]
        net_relay: Option<String>,
        /// Online play: the relay room name every player uses (1–16 characters).
        #[arg(long, default_value = "SMCRAFT")]
        net_room: String,
        /// Online play: how many players (2–4; with --net-peer, one more than the peers).
        #[arg(long)]
        net_players: Option<usize>,
        /// Online play: which player we are (0 = player 1 … 3 = player 4).
        #[arg(long, default_value_t = 0)]
        net_player: usize,
        /// Local multiplayer: 1–4 players (keyboard + first pad, then one pad each).
        #[arg(long, default_value_t = 1)]
        players: usize,
        /// Course Maker: edit the course file (a new course if it doesn't exist yet); Enter
        /// test-plays, E returns to editing, Ctrl+S saves back to the file.
        #[arg(long)]
        edit: bool,
        /// Course Maker style for a new course (M1, M3, MW, WU, 3W).
        #[arg(long, default_value = "M1")]
        style: String,
        /// SMM2 front end (the default without a course when the SMM2 UI import is found):
        /// course folder(s) for Course World and 100 Mario (default $MMK_LIBRARY, else
        /// ~/smm-work/courses).
        #[arg(long)]
        library: Vec<PathBuf>,
        /// Play the demo strip instead of the menus.
        #[arg(long)]
        no_menus: bool,
        /// Front end without the opening.
        #[arg(long)]
        no_opening: bool,
        /// Front end: play this opening (InDefault, InCharacter, InKuribo, InKoopa, InCoopMode,
        /// InLink, InKoopaling).
        #[arg(long)]
        opening: Option<String>,
        /// Front end: start with this title demo course (0-based).
        #[arg(long)]
        title_course: Option<usize>,
        /// Front end: 100 Mario with SMM2's Endless screens (smm2) or SMM1's (smm1, needs
        /// --smm1-ui).
        #[arg(long, default_value = "smm2")]
        hundred: String,
        /// Front end: the SMM1 UI import (default $MMK_UI_SMM1, else ~/smm-work/cache/smm1-v208).
        #[arg(long)]
        smm1_ui: Option<PathBuf>,
        /// Front end: where save data and settings go (default $MMK_DATA, else ~/.mmk-play).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Headless front end capture (with --screenshot, --ticks): boot, title, title-main,
        /// title-play, courseworld, courseworld-list, storymode, with --server (and
        /// MMK_FRONT_REALTIME=1): account, courseworld-leaderboards, courseworld-course-page,
        /// courseworld-comments, courseworld-post, courseworld-maker, courseworld-play,
        /// courseworld-pause, courseworld-clear; challenge, challenge-map,
        /// challenge-walk, challenge-gameover, challenge-ending, challenge-pause,
        /// challenge-clear, challenge-skip, challenge-card; with --front-course: course-intro,
        /// course-pause, course-clear, course-gameover, course-timeup, course-retire,
        /// course-settings, course-death, course-play (--ticks of play with the HUD).
        #[arg(long, value_name = "NAME")]
        front_screen: Option<String>,
        /// Capture size.
        #[arg(long, default_value = "1280x720")]
        front_size: String,
        /// The course for course-* captures.
        #[arg(long)]
        front_course: Option<PathBuf>,
        /// Captures: difficulty (0–3) and courses cleared (challenge-walk).
        #[arg(long, default_value_t = 0)]
        front_difficulty: usize,
        #[arg(long, default_value_t = 3)]
        front_cleared: usize,
        /// Captures: Play-menu button selected on title-play.
        #[arg(long)]
        front_select: Option<usize>,
        /// Captures: menu presses at global UI ticks, tick:button,… (the Swift app's
        /// MMK_UI_SCRIPT); title presses at title ticks (MMK_UI_INPUT).
        #[arg(long, default_value = "")]
        front_script: String,
        #[arg(long, default_value = "")]
        front_input: String,
        /// Debug, with --screenshot: drive the player with this input script ("N BUTTONS…" per
        /// line: N ticks holding JUMP / RUN / SPIN / LEFT / RIGHT / UP / DOWN, "-" for none)
        /// and save a screenshot at each of --shot-ticks (`<png stem>_<tick>.png`).
        #[arg(long)]
        input_script: Option<PathBuf>,
        /// With --input-script: the ticks to save (comma-separated).
        #[arg(long, default_value = "")]
        shot_ticks: String,
        /// Debug, with --screenshot: play these emitter sets (comma-separated, imported effect
        /// names) side by side over the course and save their frame --ticks frames in.
        #[arg(long)]
        fx_set: Option<String>,
        /// Debug: start the player at this x (px), and y (default 32).
        #[arg(long)]
        start_x: Option<f32>,
        #[arg(long, default_value_t = 32.0)]
        start_y: f32,
        /// SMM1 extra: SMM1 shell physics (mid-air shell kicks, shell jumps).
        #[arg(long)]
        smm1_shell_kicks: bool,
        /// SMM1 extra: SMM1 draw order (the player travels between blocks and pipes, coins behind
        /// blocks, tracks in front of pipes, …).
        #[arg(long)]
        smm1_layering: bool,
        /// SMM1 extra: Super Mushrooms in SMB1-style courses become Mystery Mushrooms
        /// (costumes) or Weird Mushrooms (Lanky Mario): off, mystery, weird.
        #[arg(long, value_name = "off|mystery|weird")]
        smb1_mushroom: Option<String>,
        /// SMM1 extra: the Mystery Mushroom costume (an id of the SMM1 costume table, or
        /// "random", the default).
        #[arg(long, value_name = "ID|random")]
        costume: Option<String>,
        /// Any engine option as NAME=VALUE (repeatable; see `mmk_sim::Options`).
        #[arg(long = "option", value_name = "NAME=VALUE")]
        options: Vec<String>,
        /// SMM1 extras art: costumes, Lanky Mario, Weird/Mystery Mushrooms (`mmk import-smm1`
        /// output's sprites/ folder; "none" to skip). Default: $MMK_SMM1_SPRITES, else
        /// ~/smm-work/cache/smm1-v208/sprites if it exists.
        #[arg(long, value_name = "DIR")]
        smm1_sprites: Option<String>,
    }

    fn default_smm1_sprites() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("MMK_SMM1_SPRITES") {
            return (!p.is_empty() && p != "none").then(|| PathBuf::from(p));
        }
        let p = mmk_host::paths::smm1_ui()?.join("sprites");
        p.join("manifest.json").is_file().then_some(p)
    }

    /// The engine options from the command line.
    fn engine_options(args: &Args) -> Result<mmk_sim::Options, String> {
        let mut o = mmk_sim::Options {
            smm1_shell_kicks: args.smm1_shell_kicks,
            smm1_layering: args.smm1_layering,
            ..Default::default()
        };
        if let Some(m) = &args.smb1_mushroom {
            o.apply(&format!("smb1_mushroom={m}"))?;
        }
        if let Some(c) = &args.costume {
            o.apply(&format!("costume={c}"))?;
        }
        for a in &args.options {
            o.apply(a)?;
        }
        Ok(o)
    }

    fn default_assets() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("MMK_ASSETS") {
            return (!p.is_empty() && p != "placeholder").then(|| PathBuf::from(p));
        }
        mmk_host::paths::game_cache()
    }

    fn ui_game(dir: &std::path::Path) -> Option<String> {
        let m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
        m.get("game")?.as_str().map(str::to_string)
    }

    /// The SMM2 UI import for the front end: --ui-cache, $MMK_UI_CACHE / $MMK_UI2_CACHE when
    /// they are SMM2 imports, else ~/smm-work/cache/smm2-ui-3.0.3.
    fn front_ui_dir(args: &Args) -> Option<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        [
            args.ui_cache.clone(),
            env("MMK_UI_CACHE"),
            env("MMK_UI2_CACHE"),
            mmk_host::paths::smm2_ui(),
        ]
        .into_iter()
        .flatten()
        .find(|d| ui_game(d).as_deref() == Some("SMM2"))
    }

    fn run_front(args: &Args, ui_dir: PathBuf, assets_dir: Option<PathBuf>) -> Result<(), String> {
        use mmk_front::{Front, Options, Ui, UiKind};
        use mmk_shell::front_app::FrontApp;
        use mmk_shell::front_host::{HostData, ShellHost};
        let capture = args.front_screen.clone();
        let smm2 = Ui::open(UiKind::Smm2, &ui_dir)?;
        let smm1_dir = args
            .smm1_ui
            .clone()
            .or_else(|| std::env::var_os("MMK_UI_SMM1").map(PathBuf::from))
            .or_else(mmk_host::paths::smm1_ui)
            .filter(|d| ui_game(d).as_deref() == Some("SMM1"));
        let smm1 = smm1_dir.and_then(|d| Ui::open(UiKind::Smm1, &d).ok());
        let source = assets_dir.as_ref().map(|d| DirSource::new(d.clone()));
        let mut game = Game::load(source.as_ref().map(|s| s as &dyn AssetSource), None)?;
        game.no_hud = true;
        let mut data = HostData::new(assets_dir.as_ref().map(|d| DirSource::new(d.clone())));
        data.ui_dir = Some(ui_dir.clone());
        data.server = args
            .server
            .clone()
            .or_else(|| std::env::var("MMK_SERVER").ok().filter(|s| !s.is_empty()));
        data.library_dirs = if !args.library.is_empty() {
            args.library.clone()
        } else if let Some(l) = std::env::var_os("MMK_LIBRARY") {
            std::env::split_paths(&l).collect()
        } else {
            mmk_host::paths::library().into_iter().collect()
        };
        // the library is read on a thread while the opening plays
        data.start_scan();
        // captures keep their settings in memory (nothing read or written)
        data.data_dir = if capture.is_some() && args.data_dir.is_none() {
            None
        } else {
            args.data_dir
                .clone()
                .or_else(|| std::env::var_os("MMK_DATA").map(PathBuf::from))
                .or_else(mmk_host::paths::play_data)
        };
        let opts = Options {
            boot: !args.no_opening && capture.is_none(),
            opening: args.opening.clone(),
            title_select: args.front_select,
            title_course: args.title_course,
            hundred: if args.hundred == "smm1" {
                mmk_front::challenge::Presentation::Smm1
            } else {
                mmk_front::challenge::Presentation::Smm2
            },
            // (captures change scenes at once, as the Swift app's frozen captures; MMK_FADES=1
            // keeps the fades to capture them)
            instant_fades: capture.is_some() && std::env::var("MMK_FADES").as_deref() != Ok("1"),
            script: args.front_script.clone(),
            title_input: args.front_input.clone(),
            ..Default::default()
        };
        let front = {
            let mut host = ShellHost {
                game: &mut game,
                data: &mut data,
            };
            Front::new(smm2, smm1, opts, &mut host)
        };
        let mut app = FrontApp::new(front, data);
        log(&format!(
            "front end: {} (library {:?}, server {})",
            ui_dir.display(),
            app.data.library_dirs,
            app.data.server.as_deref().unwrap_or("none")
        ));
        if let Some(screen) = capture {
            let out = args
                .screenshot
                .as_deref()
                .ok_or("--front-screen needs --screenshot <png>")?;
            let (w, h) = args
                .front_size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .ok_or("--front-size: WxH")?;
            let course_screen = screen.starts_with("course-");
            if course_screen {
                let path = args
                    .front_course
                    .clone()
                    .ok_or("course-* captures need --front-course <course file>")?;
                let mut host = ShellHost {
                    game: &mut game,
                    data: &mut app.data,
                };
                let loaded = mmk_front::Host::load_course(
                    &mut host,
                    &mmk_front::Source::Library(path.to_string_lossy().into_owned()),
                )?;
                let name = if loaded.name.is_empty() {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                } else {
                    loaded.name.clone()
                };
                // course-pause: straight into play, paused after one tick (the Swift app's
                // MMK_UI_INTRO=0 MMK_UI_PAUSE_TICKS=1); the others over the course card
                let pause = screen == "course-pause";
                let play = screen == "course-play";
                app.front
                    .play_course(&mut host, &name, loaded.key.clone(), !pause && !play);
                let what = screen.clone();
                app.front.with_cx(&mut host, |cx, s| {
                    if let mmk_front::Screen::Course(p) = s {
                        if pause {
                            p.auto_pause_tick = Some(1);
                        } else if !play {
                            p.debug(&what, cx, None);
                        }
                    }
                });
            } else {
                let mut host = ShellHost {
                    game: &mut game,
                    data: &mut app.data,
                };
                if !app.front.capture(
                    &mut host,
                    &screen,
                    args.front_difficulty,
                    args.front_cleared,
                ) {
                    return Err(format!("unknown front screen {screen}"));
                }
            }
            let msg = mmk_shell::headless::front(
                &mut app,
                &mut game,
                args.ticks,
                course_screen && screen != "course-play",
                args.scripted,
                (w, h),
                out,
                !args.cpu_textures,
            )?;
            println!("{msg}");
            return Ok(());
        }
        let mute = args.mute || std::env::var("MMK_MUTE").as_deref() == Ok("1");
        if !mute {
            let bank = source.as_ref().and_then(|s| source::open_bank(s).ok());
            let music = source.as_ref().and_then(|s| source::music_reader(s));
            match mmk_shell::ui_sound::start(bank, music, Some(ui_dir.join("ui/sound"))) {
                Ok(a) => {
                    app.data.audio = a.game.clone();
                    app.ui_audio = Some(a.ui.clone());
                    app.audio_stream = Some(a);
                }
                Err(e) => log(&format!("sound off: {e}")),
            }
        }
        // the title course loaded before the sound started: give it the sound now
        game.session.set_audio(app.data.audio.clone());
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .map_err(|e| format!("event loop: {e}"))?;
        let mut shell = App::new(game, "SMCraft".to_string(), !args.cpu_textures);
        shell.quit_after = args.quit_after;
        shell.front = Some(Box::new(app));
        shell.set_proxy(event_loop.create_proxy());
        event_loop
            .run_app(&mut shell)
            .map_err(|e| format!("event loop: {e}"))
    }

    pub fn run() -> Result<(), String> {
        let args = Args::parse();
        if let Some(name) = &args.layout {
            let dir = args
                .ui_cache
                .clone()
                .or_else(|| std::env::var_os("MMK_UI_CACHE").map(PathBuf::from))
                .or_else(mmk_host::paths::smm1_ui)
                .ok_or("--layout needs --ui-cache (an SMM1 or SMM2 UI import)")?;
            let out = args
                .screenshot
                .as_deref()
                .ok_or("--layout needs --screenshot <png>")?;
            let msg = mmk_shell::headless::layout(
                &dir,
                name,
                &args.ui_anim,
                &args.ui_text,
                args.ticks,
                out,
                !args.cpu_textures,
            )?;
            println!("{msg}");
            return Ok(());
        }
        let assets_dir = match args.assets.as_deref() {
            Some("placeholder") => None,
            Some(p) => Some(PathBuf::from(p)),
            None => default_assets(),
        };
        let menus = args.course.is_none()
            && !args.edit
            && args.code.is_none()
            && args.net_bind.is_none()
            && args.net_relay.is_none()
            && args.map.is_none()
            && !args.check_textures
            && !args.no_menus
            && args.screenshot.is_none();
        if menus || args.front_screen.is_some() {
            match front_ui_dir(&args) {
                Some(ui) => return run_front(&args, ui, assets_dir),
                None if args.front_screen.is_some() => {
                    return Err("--front-screen needs the SMM2 UI import (--ui-cache)".into());
                }
                None => {}
            }
        }
        let source = assets_dir.as_ref().map(|d| DirSource::new(d.clone()));
        let source_ref = source.as_ref().map(|s| s as &dyn AssetSource);
        let downloaded = match (&args.code, &args.server) {
            (Some(code), Some(server)) => Some(
                mmk_online::Client::new(server.clone())
                    .download(code)
                    .map_err(|e| format!("{code}: {e}"))?,
            ),
            (Some(_), None) => return Err("--code needs --server".into()),
            _ => None,
        };
        let course = match &args.course {
            _ if downloaded.is_some() => downloaded.clone(),
            Some(p) if args.edit && !p.exists() => None,
            Some(p) => Some(std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?),
            None => None,
        };
        let mut game = Game::load(source_ref, if args.edit { None } else { course.as_deref() })?;
        let smm1_sprites = match args.smm1_sprites.as_deref() {
            Some("none") => None,
            Some(p) => Some(PathBuf::from(p)),
            None => default_smm1_sprites(),
        };
        if let Some(dir) = smm1_sprites.clone().filter(|_| assets_dir.is_some())
            && let Some(cache) = Arc::get_mut(&mut game.cache)
        {
            match mmk_assets::Cache::open(&dir) {
                Ok(c) => {
                    cache.merge(c);
                    log(&format!(
                        "SMM1 sprites: {} ({} costumes)",
                        dir.display(),
                        cache.costumes().len()
                    ));
                }
                Err(e) => log(&format!("SMM1 sprites: {e}")),
            }
        }
        let ghost = match (&args.ghost_file, args.ghost, &args.code, &args.server) {
            (Some(p), _, _, _) => {
                Some(std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?)
            }
            (None, true, Some(code), Some(server)) => Some(
                mmk_online::Client::new(server.clone())
                    .record_replay(code)
                    .map_err(|e| format!("ghost for {code}: {e}"))?,
            ),
            (None, true, _, _) => return Err("--ghost needs --code and --server".into()),
            _ => None,
        };
        if let Some(bytes) = ghost {
            let r = mmk_modes::replay::Replay::decode(&bytes).ok_or("the ghost isn't a replay")?;
            if r.sim_version != mmk_modes::replay::SIM_VERSION {
                log("ghost: recorded with another engine version; it may not follow the course");
            }
            log(&format!("ghost: {} frames", r.inputs.len()));
            game.session.set_ghost(r);
        }
        // a server course: each clear goes to the server as a replay it verifies (records)
        if let (Some(code), Some(server)) = (&args.code, &args.server) {
            let token = args
                .token
                .clone()
                .or_else(|| std::env::var("MMK_TOKEN").ok().filter(|t| !t.is_empty()));
            if let Some(token) = token {
                game.session.record_replays();
                let (code, server) = (code.clone(), server.clone());
                game.on_clear = Some(Box::new(move |r: mmk_modes::replay::Replay| {
                    let client =
                        mmk_online::Client::new(server.clone()).with_token(Some(token.clone()));
                    let code = code.clone();
                    std::thread::spawn(move || match client.submit_replay(&code, &r.encode()) {
                        Ok(l) => log(&format!(
                            "clear sent: record {} ms by {}{}",
                            l.record_ms,
                            l.record_holder,
                            if l.record_verified { " (verified)" } else { "" }
                        )),
                        Err(e) => log(&format!("clear not sent: {e}")),
                    });
                }));
            }
        }
        if let Some(p) = args.record_replay.clone() {
            game.session.record_replays();
            let mut next = game.on_clear.take();
            game.on_clear = Some(Box::new(move |r: mmk_modes::replay::Replay| {
                match std::fs::write(&p, r.encode()) {
                    Ok(()) => log(&format!("replay saved: {}", p.display())),
                    Err(e) => log(&format!("{}: {e}", p.display())),
                }
                if let Some(f) = &mut next {
                    f(r);
                }
            }));
        }
        if args.edit {
            let editor = match &course {
                Some(bytes) => {
                    let c = mmk_host::course::decode(bytes, || source::key_table(source_ref))?;
                    mmk_editor::Editor::open(&c.to_bytes().map_err(|e| e.to_string())?)
                        .map_err(|e| format!("{e:?}"))?
                }
                None => {
                    let mut e = mmk_editor::Editor::new(&args.style);
                    for x in 0..240 {
                        let _ = e.paint_ground(x, 0);
                    }
                    e
                }
            };
            let mut m = mmk_shell::maker::Maker::new(editor, args.course.clone());
            m.server = args.server.clone();
            m.maker_name = args.maker.clone();
            game.maker = Some(m);
            game.editing = true;
        }
        match &assets_dir {
            Some(d) => log(&format!(
                "assets: {} ({} textures)",
                d.display(),
                game.cache.textures.len()
            )),
            None => log("assets: none, drawing placeholder boxes"),
        }
        let mut options = engine_options(&args)?;
        let costumes = game.cache.costumes();
        if !costumes.is_empty() {
            options.set_costume_pool(costumes.iter().map(|(id, _)| *id));
        }
        game.session.set_options(options);
        if args.players > 1 {
            game.session.world.set_players(args.players);
        }
        let link: Option<(Box<dyn mmk_netplay::Link>, usize, String)> =
            if let Some(relay) = &args.net_relay {
                use std::net::ToSocketAddrs;
                let at = relay
                    .to_socket_addrs()
                    .map_err(|e| format!("--net-relay: {e}"))?
                    .next()
                    .ok_or("--net-relay: no address")?;
                let l = mmk_netplay::relay::RelayLink::connect(
                    at,
                    &args.net_room,
                    args.net_player.min(3) as u8,
                )
                .map_err(|e| format!("--net-relay: {e}"))?;
                let n = args.net_players.unwrap_or(2);
                Some((Box::new(l), n, format!("room {} on {at}", args.net_room)))
            } else if let Some(bind) = &args.net_bind
                && !args.net_peer.is_empty()
            {
                let peers = args
                    .net_peer
                    .iter()
                    .map(|p| p.parse().map_err(|e| format!("--net-peer {p}: {e}")))
                    .collect::<Result<Vec<std::net::SocketAddr>, String>>()?;
                let n = args.net_players.unwrap_or(peers.len() + 1);
                let l = mmk_netplay::UdpLink::bind(bind, peers.clone())
                    .map_err(|e| format!("--net-bind: {e}"))?;
                Some((Box::new(l), n, format!("peers {peers:?}")))
            } else {
                None
            };
        if let Some((link, n, how)) = link {
            game.session.go_online(link, n, args.net_player);
            let s = &game.session.net.as_ref().unwrap().0;
            log(&format!(
                "online: player {} of {}, {how}",
                s.local + 1,
                s.players()
            ));
        }
        if let Some(x) = args.start_x {
            let w = &mut game.session.world;
            w.player.x = x;
            w.player.y = args.start_y;
            w.player.vy = 0.0;
            w.reset_camera();
        }

        if args.check_textures {
            let report = mmk_shell::headless::check_textures(&game);
            return match report {
                Ok(r) => {
                    println!("{r}\ntextures: GPU and CPU decoding agree");
                    Ok(())
                }
                Err(r) => Err(format!("texture check failed:\n{r}")),
            };
        }
        if let Some(path) = &args.map {
            let msg = mmk_shell::headless::map(
                &mut game,
                path,
                args.map_area,
                args.scale,
                !args.cpu_textures,
            )?;
            println!("{msg}");
            return Ok(());
        }
        if let (Some(path), Some(script)) = (&args.screenshot, &args.input_script) {
            let text = std::fs::read_to_string(script)
                .map_err(|e| format!("{}: {e}", script.display()))?;
            let inputs = mmk_shell::headless::parse_input_script(&text)?;
            let mut ticks: Vec<u64> = args
                .shot_ticks
                .split(',')
                .filter_map(|t| t.trim().parse().ok())
                .collect();
            ticks.sort_unstable();
            let stem = path.strip_suffix(".png").unwrap_or(path);
            let mut done = 0u64;
            for t in ticks {
                while done < t {
                    let input = inputs.get(done as usize).copied().unwrap_or_default();
                    game.session.tick(input);
                    done += 1;
                }
                let out = format!("{stem}_{t}.png");
                let msg = mmk_shell::headless::screenshot(
                    &mut game,
                    &out,
                    0,
                    args.scale,
                    None,
                    None,
                    false,
                    !args.cpu_textures,
                )?;
                println!("{msg}");
            }
            return Ok(());
        }
        if let (Some(path), Some(sets)) = (&args.screenshot, &args.fx_set) {
            let msg = mmk_shell::headless::fx_preview(
                &mut game,
                path,
                sets,
                args.ticks,
                args.scale,
                !args.cpu_textures,
            )?;
            println!("{msg}");
            return Ok(());
        }
        if let Some(path) = &args.screenshot {
            if args.record_replay.is_some() {
                game.session.record_replays();
            }
            let size = match args.size.as_deref().map(|s| {
                s.split_once('x')
                    .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            }) {
                Some(None) => return Err("--size: expected WxH, e.g. 1280x720".into()),
                Some(Some(wh)) => Some(wh),
                None => None,
            };
            let view_at = match args.view_at.as_deref().map(|s| {
                s.split_once(',')
                    .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
            }) {
                Some(None) => return Err("--view-at: expected X,Y, e.g. 0,0".into()),
                Some(Some(p)) => Some(p),
                None => None,
            };
            let msg = mmk_shell::headless::screenshot(
                &mut game,
                path,
                args.ticks,
                args.scale,
                size,
                view_at,
                !args.idle,
                !args.cpu_textures,
            )?;
            if let Some(p) = &args.record_replay {
                std::fs::write(p, game.session.replay().encode())
                    .map_err(|e| format!("{}: {e}", p.display()))?;
            }
            println!("{msg}");
            return Ok(());
        }

        let title = match &args.course {
            Some(p) => format!(
                "SMCraft — {}",
                p.file_name().unwrap_or_default().to_string_lossy()
            ),
            None => "SMCraft — demo".to_string(),
        };
        let mute = args.mute || std::env::var("MMK_MUTE").as_deref() == Ok("1");
        let audio = match (&source, mute) {
            (Some(s), false) => match source::open_bank(s).and_then(|bank| {
                mmk_shell::audio_out::start(bank, source::music_reader(s), smm1_sprites.clone())
            }) {
                Ok(a) => Some(a),
                Err(e) => {
                    log(&format!("sound off: {e}"));
                    None
                }
            },
            _ => None,
        };
        game.session
            .set_audio(audio.as_ref().map(|a| Arc::clone(&a.out)));
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .map_err(|e| format!("event loop: {e}"))?;
        if args.scripted {
            game.session.scripted = Some(mmk_shell::session::scripted_run);
        }
        let mut app = App::new(game, title, !args.cpu_textures);
        app.audio = audio;
        app.quit_after = args.quit_after;
        app.set_proxy(event_loop.create_proxy());
        event_loop
            .run_app(&mut app)
            .map_err(|e| format!("event loop: {e}"))
    }
}
