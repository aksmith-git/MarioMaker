//! The window and frame loop (winit), shared by the native and web builds: one redraw per display
//! frame; each redraw advances the session by the host time elapsed (fixed 60 Hz ticks, see
//! `session`) and draws the latest state.
//!
//! Graphics start asynchronously (the browser can't block): native builds wait for the device
//! inside `resumed`; the web build spawns the setup and receives it as a user event.

use std::sync::Arc;
use std::time::Duration;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use mmk_host::audio::AudioOut;

use crate::game::Game;
use crate::input::{Keyboard, Pads, combine};
use crate::render::{Gpu, Renderer, VIEW};

/// Everything that needs the window: surface, device, renderer.
pub struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Format of the views we render into (the sRGB form of the surface format).
    view_format: wgpu::TextureFormat,
    gpu: Gpu,
    renderer: Renderer,
    /// A surface problem was reported (once).
    warned: bool,
}

impl Gfx {
    pub async fn new(
        instance: wgpu::Instance,
        window: Arc<Window>,
        game: &GameInfo,
    ) -> Result<Self, String> {
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("window surface: {e}"))?;
        let gpu = Gpu::new(&instance, Some(&surface), game.allow_bc).await?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("the window surface supports no format")?;
        let view_format = format.add_srgb_suffix();
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("the window surface is not supported by the adapter")?;
        config.format = format;
        config.view_formats = if view_format != format {
            vec![view_format]
        } else {
            vec![]
        };
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        let mut renderer = Renderer::new(&gpu, view_format, &game.cache);
        renderer.draw_missing = game.draw_missing;
        log(&format!(
            "graphics: {} ({:?}), BC textures: {}, surface {format:?}{}",
            gpu.info.name,
            gpu.info.backend,
            if gpu.bc { "GPU" } else { "CPU decode" },
            if renderer.skipped_textures > 0 {
                format!(", {} textures skipped", renderer.skipped_textures)
            } else {
                String::new()
            }
        ));
        Ok(Self {
            window,
            surface,
            config,
            view_format,
            gpu,
            renderer,
            warned: false,
        })
    }

    /// Draw another game's assets from now on.
    fn set_game(&mut self, info: &GameInfo) {
        self.renderer.set_cache(&info.cache);
        self.renderer.draw_missing = info.draw_missing;
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.gpu.device, &self.config);
        }
    }

    /// Draw and present one frame; false if the surface had no image to draw into (window
    /// hidden or occluded, surface being reconfigured).
    fn draw(&mut self, game: &mut Game) -> bool {
        self.draw_with(|gpu, renderer, enc, view, _, size| {
            let scene = game.scene();
            renderer.encode(gpu, enc, view, size, &scene);
        })
    }

    /// Draw and present one frame recorded by `encode` (gpu, renderer, encoder, target view, its
    /// format, its size); false as `draw`.
    fn draw_with(
        &mut self,
        encode: impl FnOnce(
            &Gpu,
            &mut Renderer,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            wgpu::TextureFormat,
            (u32, u32),
        ),
    ) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.gpu.device, &self.config);
                return false;
            }
            other => {
                if !self.warned {
                    self.warned = true;
                    log(&format!("surface: no image ({other:?}), waiting"));
                }
                return false;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        encode(
            &self.gpu,
            &mut self.renderer,
            &mut enc,
            &view,
            self.view_format,
            (self.config.width, self.config.height),
        );
        self.gpu.queue.submit([enc.finish()]);
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        true
    }
}

/// What graphics setup needs to know about the game (owned, for the async setup).
pub struct GameInfo {
    pub cache: Arc<mmk_assets::Cache>,
    pub allow_bc: bool,
    pub draw_missing: bool,
}

/// Events sent to the loop from async setup and (web) from the page.
pub enum UserEvent {
    Gfx(Result<Box<Gfx>, String>),
    /// Play this game instead (the web page opened the owner's assets or picked a course).
    Game(Box<Game>),
    /// Sound output for this and later games (the web page got its first user gesture, or new
    /// assets brought a new sound bank).
    Sound(Arc<AudioOut>),
    /// Play the current course online from its start: the link, players, our player (0-based).
    Net(Box<dyn mmk_netplay::Link>, usize, usize),
    /// Run this on the app (the web page starting or changing the front end).
    Run(Box<dyn FnOnce(&mut App)>),
}

pub struct App {
    pub game: Game,
    title: String,
    keys: Keyboard,
    pads: Pads,
    gfx: Option<Box<Gfx>>,
    starting: bool,
    last: Option<f64>,
    allow_bc: bool,
    proxy: Option<EventLoopProxy<UserEvent>>,
    /// Smoke tests: close after this many seconds of drawing and report frames and ticks.
    pub quit_after: Option<f64>,
    first_frame: Option<f64>,
    frames: u64,
    /// The last frame couldn't present; redraw when the wait times out.
    idle: bool,
    /// Mouse position (window px) and Ctrl held, for the maker.
    mouse: (f64, f64),
    ctrl: bool,
    shift: bool,
    /// Keeps sound playing (native builds).
    #[cfg(not(target_arch = "wasm32"))]
    pub audio: Option<crate::audio_out::AudioStream>,
    /// Sound output handed to games that arrive later (`UserEvent::Game`).
    sound: Option<Arc<AudioOut>>,
    /// The SMM2 front end (menus, Course World, 100 Mario, in-course screens), when it runs.
    pub front: Option<Box<crate::front_app::FrontApp>>,
}

pub fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&msg.into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("mmk-play: {msg}");
}

impl App {
    pub fn new(game: Game, title: String, allow_bc: bool) -> Self {
        Self {
            game,
            title,
            keys: Keyboard::default(),
            pads: Pads::new(),
            gfx: None,
            starting: false,
            last: None,
            allow_bc,
            proxy: None,
            quit_after: None,
            first_frame: None,
            frames: 0,
            idle: false,
            mouse: (0.0, 0.0),
            ctrl: false,
            shift: false,
            #[cfg(not(target_arch = "wasm32"))]
            audio: None,
            sound: None,
            front: None,
        }
    }

    /// Switch to `game` (keeping the window, GPU and sound output).
    pub fn replace_game(&mut self, mut game: Game) {
        if self.sound.is_some() {
            game.session.set_audio(self.sound.clone());
        }
        self.game = game;
        self.keys.clear();
        self.last = None;
        let info = self.info();
        if let Some(g) = &mut self.gfx {
            g.set_game(&info);
            g.window.request_redraw();
        }
    }

    /// Needed by the web build to hand the async graphics setup back to the loop.
    pub fn set_proxy(&mut self, proxy: EventLoopProxy<UserEvent>) {
        self.proxy = Some(proxy);
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            cache: self.game.cache.clone(),
            allow_bc: self.allow_bc,
            draw_missing: self.game.placeholder
                || std::env::var("MMK_DEBUG_MISSING_ART").as_deref() == Ok("1"),
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = crate::clock::now();
        if let Some(limit) = self.quit_after {
            let first = *self.first_frame.get_or_insert(now);
            if now - first >= limit {
                log(&format!(
                    "drew {} frames in {:.2} s, sim tick {}",
                    self.frames,
                    now - first,
                    self.game.session.world.tick
                ));
                event_loop.exit();
                return;
            }
        }
        let dt = self.last.map_or(0.0, |l| now - l);
        self.last = Some(now);
        // local multiplayer: keyboard + the first pad are player 1, further pads players 2–4
        let each = self.pads.poll_each();
        let pads = if self.game.session.world.others.is_empty() {
            each.iter().fold(0, |a, b| a | b)
        } else {
            each.first().copied().unwrap_or(0)
        };
        self.game.session.extra_inputs = each
            .iter()
            .skip(1)
            .map(|&b| mmk_sim::Input { buttons: b })
            .collect();
        let input = combine(&self.keys, pads);
        let actions = self.keys.take_actions();
        // the browser reads texture files asynchronously: start the reads the last frame asked
        // for, and hold the game (briefly) while they are on their way
        #[cfg(target_arch = "wasm32")]
        let loading = crate::web::pump_textures(&self.game.cache)
            | crate::web::pump_ui(self.front.as_deref_mut());
        #[cfg(not(target_arch = "wasm32"))]
        let loading = false;
        if self.front_advance(if loading { 0.0 } else { dt }, pads) {
            // the front end ran its ticks (and the course's)
        } else if self.game.editing {
            // arrows / WASD scroll the maker view
            if let Some(m) = self.game.maker.as_mut() {
                let b = input.buttons;
                let speed = 4.0;
                let dx = (b & mmk_sim::input::RIGHT != 0) as i32
                    - (b & mmk_sim::input::LEFT != 0) as i32;
                let dy =
                    (b & mmk_sim::input::UP != 0) as i32 - (b & mmk_sim::input::DOWN != 0) as i32;
                if dx != 0 || dy != 0 {
                    m.scroll(dx as f32 * speed, dy as f32 * speed);
                }
            }
        } else if !loading {
            self.game.session.frame(dt, input, actions);
            if let Some(r) = self.game.session.take_cleared_replay()
                && let Some(f) = &mut self.game.on_clear
            {
                f(r);
            }
        }
        if let Some(gfx) = &mut self.gfx {
            let drawn = match &mut self.front {
                Some(f) => {
                    let game = &mut self.game;
                    gfx.draw_with(|gpu, r, enc, view, format, size| {
                        f.encode(gpu, enc, view, format, size, r, game)
                    })
                }
                None => gfx.draw(&mut self.game),
            };
            if drawn {
                // presenting waits for the display, which paces the loop
                self.frames += 1;
                gfx.window.request_redraw();
            } else {
                // nothing on screen (hidden window): keep the game going at about 60 Hz
                // without spinning
                self.idle = true;
                event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(16)));
            }
        }
    }

    /// Advance the front end when it runs (true: it did).
    fn front_advance(&mut self, dt: f64, pads: u32) -> bool {
        match &mut self.front {
            Some(f) => {
                f.advance(dt, &self.keys, pads, &mut self.game);
                true
            }
            None => false,
        }
    }

    /// Maker keys (editing: palette, undo/redo, save, play; playing: E edits again). True when
    /// the key was used.
    fn maker_key(&mut self, code: KeyCode) -> bool {
        if self.game.maker.is_none() {
            return false;
        }
        if !self.game.editing {
            if code == KeyCode::KeyE {
                self.game.edit_again();
                return true;
            }
            return false;
        }
        let (ctrl, shift) = (self.ctrl, self.shift);
        let m = self.game.maker.as_mut().expect("checked");
        let digit = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
        ]
        .iter()
        .position(|&k| k == code);
        let arrow = match code {
            KeyCode::ArrowLeft => Some((-1, 0)),
            KeyCode::ArrowRight => Some((1, 0)),
            KeyCode::ArrowUp => Some((0, 1)),
            KeyCode::ArrowDown => Some((0, -1)),
            _ => None,
        };
        // arrows draw a path while one is being drawn, and move the selection with Shift
        if let Some(d) = arrow {
            if m.drawing_path() {
                return m.path_key(Some(d));
            }
            if shift && m.has_selection() {
                return m.selection_op('m', d.0, d.1);
            }
            return false;
        }
        match code {
            _ if digit.is_some() => m.select(digit.unwrap_or(0)),
            KeyCode::Tab if shift => m.prev_tool(),
            KeyCode::Tab => m.next_tool(),
            KeyCode::KeyZ if ctrl => m.undo(),
            KeyCode::KeyY if ctrl => m.redo(),
            KeyCode::KeyS if ctrl => m.save(),
            #[cfg(not(target_arch = "wasm32"))]
            KeyCode::KeyU if ctrl => m.upload(),
            KeyCode::KeyC if ctrl => return m.selection_op('c', 0, 0),
            KeyCode::KeyX if ctrl => return m.selection_op('x', 0, 0),
            KeyCode::KeyV if ctrl => return m.selection_op('v', 0, 0),
            KeyCode::Delete => return m.selection_op('d', 0, 0),
            KeyCode::Backspace if m.drawing_path() => return m.path_key(None),
            KeyCode::Backspace => return m.selection_op('d', 0, 0),
            KeyCode::Escape => m.cancel(),
            KeyCode::KeyW if !ctrl => m.modify('w'),
            KeyCode::KeyB => m.modify('b'),
            KeyCode::KeyF => m.modify('f'),
            KeyCode::KeyI => m.modify('i'),
            KeyCode::KeyR => m.modify('r'),
            KeyCode::KeyC => m.modify('c'),
            KeyCode::KeyK => m.modify('k'),
            KeyCode::KeyG => m.modify('g'),
            KeyCode::KeyO => m.modify('o'),
            KeyCode::KeyL => m.modify('l'),
            KeyCode::KeyH => m.modify('h'),
            KeyCode::Equal => m.modify(if shift { '^' } else { '+' }),
            KeyCode::Minus => m.modify(if shift { 'v' } else { '-' }),
            KeyCode::KeyT => m.setting('t'),
            KeyCode::KeyN => m.setting('n'),
            KeyCode::KeyA if !ctrl => m.setting('a'),
            KeyCode::KeyV => m.setting('V'),
            KeyCode::BracketLeft => m.setting('['),
            KeyCode::BracketRight => m.setting(']'),
            KeyCode::KeyQ => m.setting('q'),
            KeyCode::Comma => m.setting(if shift { '<' } else { ',' }),
            KeyCode::Period => m.setting(if shift { '>' } else { '.' }),
            KeyCode::Semicolon => m.setting(';'),
            KeyCode::Quote => m.setting('\''),
            KeyCode::F1 => m.message = crate::maker::HELP.to_string(),
            KeyCode::Enter => {
                self.game.play_maker_course();
                self.keys.clear();
            }
            _ => return false,
        }
        true
    }

    /// The maker's cursor tile under the mouse.
    fn maker_hover(&mut self) {
        let (Some(g), true) = (&self.gfx, self.game.editing) else {
            return;
        };
        let size = (g.config.width, g.config.height);
        let (vx, vy, vw, vh, _) = crate::render::viewport(size.0, size.1, true);
        let u = (self.mouse.0 as f32 - vx) / vw * mmk_sim::world::VIEW_W;
        let v = (self.mouse.1 as f32 - vy) / vh * mmk_sim::world::VIEW_H;
        let inside = (0.0..mmk_sim::world::VIEW_W).contains(&u)
            && (0.0..mmk_sim::world::VIEW_H).contains(&v);
        if let Some(m) = self.game.maker.as_mut() {
            m.hover(inside.then(|| m.tile_at_view(u, v)));
        }
    }

    fn installed(&mut self, gfx: Result<Box<Gfx>, String>, event_loop: &ActiveEventLoop) {
        match gfx {
            Ok(g) => {
                g.window.request_redraw();
                self.gfx = Some(g);
            }
            Err(e) => {
                log(&format!("graphics unavailable: {e}"));
                event_loop.exit();
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if self.idle && matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.idle = false;
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() || self.starting {
            return;
        }
        self.starting = true;
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(VIEW.0 * 3, VIEW.1 * 3))
            .with_min_inner_size(LogicalSize::new(VIEW.0, VIEW.1));
        #[cfg(target_arch = "wasm32")]
        let attrs = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs.with_canvas(crate::web::canvas()).with_append(true)
        };
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log(&format!("window: {e}"));
                event_loop.exit();
                return;
            }
        };
        let info = self.info();
        let display = event_loop.owned_display_handle();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
                Box::new(display),
            ));
            let gfx = pollster::block_on(Gfx::new(instance, window, &info)).map(Box::new);
            self.installed(gfx, event_loop);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone().expect("web build sets the proxy");
            wasm_bindgen_futures::spawn_local(async move {
                let desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
                let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
                let gfx = Gfx::new(instance, window, &info).await.map(Box::new);
                let _ = proxy.send_event(UserEvent::Gfx(gfx));
            });
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Gfx(g) => self.installed(g, event_loop),
            UserEvent::Game(g) => self.replace_game(*g),
            UserEvent::Sound(out) => {
                self.game.session.set_audio(Some(out.clone()));
                self.sound = Some(out);
            }
            UserEvent::Net(link, players, me) => {
                self.game.session.go_online(link, players, me);
                log(&format!("online: player {} of {players}", me + 1));
            }
            UserEvent::Run(f) => {
                f(self);
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gfx {
                    g.resize(size.width, size.height);
                    g.window.request_redraw();
                }
            }
            WindowEvent::Focused(false) => self.keys.clear(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    if matches!(
                        code,
                        KeyCode::ControlLeft
                            | KeyCode::ControlRight
                            | KeyCode::SuperLeft
                            | KeyCode::SuperRight
                    ) {
                        self.ctrl = pressed;
                    }
                    if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                        self.shift = pressed;
                    }
                    // a text field of the front end takes the keys as typed text
                    #[cfg(not(target_arch = "wasm32"))]
                    if pressed
                        && let Some(f) = &mut self.front
                        && f.key_text(code, event.text.as_deref())
                    {
                        return;
                    }
                    if pressed && self.maker_key(code) {
                        return;
                    }
                    self.keys.key(code, pressed);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = (position.x, position.y);
                self.maker_hover();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed
                    && let (Some(f), Some(g)) = (&mut self.front, &self.gfx)
                {
                    f.click(self.mouse, (g.config.width, g.config.height));
                }
                if self.game.editing
                    && let Some(m) = self.game.maker.as_mut()
                {
                    if state == ElementState::Pressed {
                        m.press(button == MouseButton::Right);
                    } else {
                        m.release();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}
