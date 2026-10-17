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
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

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
        let scene = game.scene();
        self.renderer.encode(
            &self.gpu,
            &mut enc,
            &view,
            (self.config.width, self.config.height),
            &scene,
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

/// Events sent to the loop from async setup.
pub enum UserEvent {
    Gfx(Result<Box<Gfx>, String>),
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
    /// Keeps sound playing (native builds).
    #[cfg(not(target_arch = "wasm32"))]
    pub audio: Option<crate::audio_out::AudioStream>,
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
            #[cfg(not(target_arch = "wasm32"))]
            audio: None,
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
        let pads = self.pads.poll();
        let input = combine(&self.keys, pads);
        let actions = self.keys.take_actions();
        self.game.session.frame(dt, input, actions);
        if let Some(gfx) = &mut self.gfx {
            if gfx.draw(&mut self.game) {
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
                    self.keys.key(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}
