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
        /// Online play (2 players, rollback over UDP): our address, e.g. 0.0.0.0:7000.
        #[arg(long)]
        net_bind: Option<String>,
        /// Online play: the other peer's address, e.g. 192.168.1.20:7000.
        #[arg(long)]
        net_peer: Option<String>,
        /// Online play: which player we are (0 = player 1, 1 = player 2).
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
        /// Debug: start the player at this x (px), and y (default 32).
        #[arg(long)]
        start_x: Option<f32>,
        #[arg(long, default_value_t = 32.0)]
        start_y: f32,
    }

    fn default_assets() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("MMK_ASSETS") {
            return (!p.is_empty() && p != "placeholder").then(|| PathBuf::from(p));
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        let p = PathBuf::from(home).join("smm-work/cache/smm2-3.0.3");
        p.join("manifest.json").is_file().then_some(p)
    }

    pub fn run() -> Result<(), String> {
        let args = Args::parse();
        let assets_dir = match args.assets.as_deref() {
            Some("placeholder") => None,
            Some(p) => Some(PathBuf::from(p)),
            None => default_assets(),
        };
        let source = assets_dir.as_ref().map(|d| DirSource::new(d.clone()));
        let source_ref = source.as_ref().map(|s| s as &dyn AssetSource);
        let course = match &args.course {
            Some(p) if args.edit && !p.exists() => None,
            Some(p) => Some(std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?),
            None => None,
        };
        let mut game = Game::load(source_ref, if args.edit { None } else { course.as_deref() })?;
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
            game.maker = Some(mmk_shell::maker::Maker::new(editor, args.course.clone()));
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
        if args.players > 1 {
            game.session.world.set_players(args.players);
        }
        if let (Some(bind), Some(peer)) = (&args.net_bind, &args.net_peer) {
            let peer: std::net::SocketAddr =
                peer.parse().map_err(|e| format!("--net-peer: {e}"))?;
            let link = mmk_netplay::UdpLink::bind(bind, vec![peer])
                .map_err(|e| format!("--net-bind: {e}"))?;
            game.session.world.set_players(2);
            let net =
                mmk_netplay::Session::new(game.session.world.clone(), 2, args.net_player.min(1));
            game.session.net = Some((net, link));
            log(&format!(
                "online: player {} of 2, peer {peer}",
                args.net_player.min(1) + 1
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
        if let Some(path) = &args.screenshot {
            let msg = mmk_shell::headless::screenshot(
                &mut game,
                path,
                args.ticks,
                args.scale,
                !args.idle,
                !args.cpu_textures,
            )?;
            println!("{msg}");
            return Ok(());
        }

        let title = match &args.course {
            Some(p) => format!(
                "mmk-play — {}",
                p.file_name().unwrap_or_default().to_string_lossy()
            ),
            None => "mmk-play — demo".to_string(),
        };
        let mute = args.mute || std::env::var("MMK_MUTE").as_deref() == Ok("1");
        let audio = match (&source, mute) {
            (Some(s), false) => match source::open_bank(s).and_then(mmk_shell::audio_out::start) {
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
