//! `mmk-play`'s SMM2 front end (`mmk_front`): ties the platform-neutral screens to the shell's
//! window loop, renderer and sound: keyboard / pads → menu buttons, a fixed 60 Hz tick, the
//! layout draw lists through [`UiLayer`] (one per UI import) with the course view drawn by the
//! game [`Renderer`] where the front end puts it, the UI sounds into the output stream, and the
//! front end's messages as a small text box.

use std::collections::VecDeque;
use std::sync::Arc;

use mmk_front::{Buttons, DrawItem, Front, FrontInput, UiKind};
use mmk_ui::{
    BLEND_NORMAL, COMBINE_NONE, Quad, SHIFT_COMBINE_ALPHA, SHIFT_COMBINE_COLOR, TEXTURE_NONE,
};
use winit::keyboard::KeyCode;

// the host: files, threads and the course server natively; the browser's storage on the web
#[cfg(not(target_arch = "wasm32"))]
use crate::front_host::{HostData, ShellHost};
use crate::game::Game;
use crate::input::Keyboard;
use crate::render::{Gpu, Renderer};
use crate::session::FixedStep;
use crate::ui_layer::UiLayer;
use crate::ui_sound::UiMixer;
#[cfg(target_arch = "wasm32")]
use crate::web_front::{HostData, ShellHost};

pub struct FrontApp {
    pub front: Front,
    pub data: HostData,
    layers: Vec<(UiKind, UiLayer)>,
    step: FixedStep,
    pub ui_audio: Option<Arc<UiMixer>>,
    click: Option<[f32; 2]>,
    /// The message shown and its texture in the SMM2 import's cache.
    toast: Option<(String, Option<mmk_ui::TexRef>)>,
    toasts: u32,
    /// Keys typed into an open text field, one handed to the front end per tick.
    typed: VecDeque<char>,
    /// Keeps the sound playing.
    pub audio_stream: Option<crate::ui_sound::FrontAudio>,
}

/// Menu buttons, pause and skip from the keyboard and the pads' engine buttons, as in the Swift
/// app: arrows / WASD move, Return / Z / Space confirm, Esc / P / Backspace go back (a key that
/// does both confirms); pads: the d-pad, jump confirms, run and Start go back. Skip: − or Tab.
pub fn menu_input(keys: &Keyboard, pad: u32) -> FrontInput {
    use mmk_sim::input as b;
    let k = |codes: &[KeyCode]| codes.iter().any(|&c| keys.held(c));
    let mut m = Buttons::NONE;
    let mut add = |on: bool, x: Buttons| {
        if on {
            m = m | x;
        }
    };
    add(
        k(&[KeyCode::ArrowUp, KeyCode::KeyW]) || pad & b::UP != 0,
        Buttons::UP,
    );
    add(
        k(&[KeyCode::ArrowDown, KeyCode::KeyS]) || pad & b::DOWN != 0,
        Buttons::DOWN,
    );
    add(
        k(&[KeyCode::ArrowLeft, KeyCode::KeyA]) || pad & b::LEFT != 0,
        Buttons::LEFT,
    );
    add(
        k(&[KeyCode::ArrowRight, KeyCode::KeyD]) || pad & b::RIGHT != 0,
        Buttons::RIGHT,
    );
    let confirm = k(&[
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::KeyZ,
        KeyCode::Space,
    ]) || pad & b::JUMP != 0;
    let back =
        k(&[KeyCode::Escape, KeyCode::KeyP, KeyCode::Backspace]) || pad & (b::RUN | b::START) != 0;
    add(confirm, Buttons::CONFIRM);
    add(back && !confirm, Buttons::BACK);
    FrontInput {
        menu: m,
        start: keys.buttons() & b::START != 0 || pad & b::START != 0,
        skip: k(&[KeyCode::Minus, KeyCode::Tab]),
        click: None,
        text: None,
    }
}

impl FrontApp {
    pub fn new(front: Front, data: HostData) -> Self {
        Self {
            front,
            data,
            layers: Vec::new(),
            step: FixedStep::default(),
            ui_audio: None,
            click: None,
            toast: None,
            toasts: 0,
            typed: VecDeque::new(),
            audio_stream: None,
        }
    }

    /// One 60 Hz tick of the front end (and of the course when it plays).
    pub fn tick(&mut self, input: &FrontInput, game_buttons: u32, game: &mut Game) {
        self.data.input = game_buttons;
        let mut host = ShellHost {
            game,
            data: &mut self.data,
        };
        self.front.tick(input, &mut host);
        if self.front.course_done {
            // a course played on its own (the web page's course picker): back to the menus
            self.front.course_done = false;
            self.front.capture(&mut host, "title-play", 0, 0);
        }
        let sounds = self.front.take_sounds();
        if let Some(m) = &self.ui_audio {
            for s in &sounds {
                m.play(s);
            }
        }
    }

    /// Host time `dt` with the keyboard and pads: the ticks it is worth.
    pub fn advance(&mut self, dt: f64, keys: &Keyboard, pad: u32, game: &mut Game) {
        let n = self.step.advance(dt);
        let game_buttons = crate::input::combine(keys, pad).buttons;
        let mut input = menu_input(keys, pad);
        for i in 0..n {
            input.click = if i == 0 { self.click.take() } else { None };
            input.text = self.typed.pop_front();
            self.tick(&input, game_buttons, game);
        }
    }

    /// A key pressed while the front end has a text field open: queued as typed text
    /// (Backspace erases, Return finishes, Esc cancels). False when no field is open (the key
    /// is a menu key then).
    pub fn key_text(&mut self, code: KeyCode, text: Option<&str>) -> bool {
        if !self.front.typing() {
            return false;
        }
        match code {
            KeyCode::Backspace => self.typed.push_back('\u{8}'),
            KeyCode::Enter | KeyCode::NumpadEnter => self.typed.push_back('\n'),
            KeyCode::Escape => self.typed.push_back('\u{1b}'),
            _ => self
                .typed
                .extend(text.unwrap_or_default().chars().filter(|c| !c.is_control())),
        }
        true
    }

    /// A click at window pixel `at` in a `size` window.
    pub fn click(&mut self, at: (f64, f64), size: (u32, u32)) {
        let (vx, vy, vw, vh, _) = crate::render::viewport(size.0, size.1, false);
        let u = (at.0 as f32 - vx) / vw;
        let v = (at.1 as f32 - vy) / vh;
        if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
            self.click = Some([(u - 0.5) * 1280.0, (0.5 - v) * 720.0]);
        }
    }

    fn layer(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, kind: UiKind) -> usize {
        if let Some(i) = self.layers.iter().position(|(k, _)| *k == kind) {
            return i;
        }
        self.layers.push((kind, UiLayer::new(gpu, format)));
        self.layers.len() - 1
    }

    /// The toast's quads (a dark box with the text), its texture in the SMM2 import's cache.
    fn toast_quads(&mut self, text: Option<String>) -> Vec<Quad> {
        let Some(text) = text else {
            self.toast = None;
            return Vec::new();
        };
        if self.toast.as_ref().is_none_or(|t| t.0 != text) {
            let scale = 3;
            let w = crate::hud::text_width(&text, scale) + 2 * scale;
            let h = (crate::hud::GLYPH_H + 2) * scale;
            let mut img = crate::hud::Image::new(w.max(1), h);
            img.text(&text, 0, 0, scale, [255, 255, 255, 255]);
            self.toasts += 1;
            let tex = mmk_assets::Mtex {
                format: mmk_assets::MtexFormat::Rgba8,
                variant: mmk_assets::MtexVariant::Srgb,
                width: img.width as u32,
                height: img.height as u32,
                channel_select: [2, 3, 4, 5],
                data: img.rgba,
            };
            let r = self.front.smm2.cache_mut().add_texture_file(
                "toast",
                &self.toasts.to_string(),
                tex,
            );
            self.toast = Some((text, Some(r)));
        }
        let Some((_, Some(t))) = &self.toast else {
            return Vec::new();
        };
        let (w, h) = (t.width as f32, t.height as f32);
        let y = -360.0 + 60.0;
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| [[x0, y1], [x1, y1], [x0, y0], [x1, y0]];
        let flags = (COMBINE_NONE << SHIFT_COMBINE_COLOR)
            | (COMBINE_NONE << SHIFT_COMBINE_ALPHA)
            | BLEND_NORMAL;
        let quad = |pos, tex, color: [u8; 4]| Quad {
            pos,
            uv0: [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            uv1: [[0.0; 2]; 4],
            color: [color; 4],
            black: [0, 0, 0, 0],
            white: [255; 4],
            tex0: tex,
            tex1: TEXTURE_NONE,
            flags,
        };
        let pad = 18.0;
        vec![
            quad(
                rect(-w / 2.0 - pad, y - pad, w / 2.0 + pad, y + h + pad),
                TEXTURE_NONE,
                [0, 0, 0, 190],
            ),
            quad(rect(-w / 2.0, y, w / 2.0, y + h), t.id, [255; 4]),
        ]
    }

    /// Draw the front end into `target` (`size` px): the course view and the layouts in the
    /// letterboxed 16:9 view, then the message.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        renderer: &mut Renderer,
        game: &mut Game,
    ) {
        let frame = self.front.frame();
        let toast = self.toast_quads(frame.toast.clone());
        let (vx, vy, vw, vh, _) = crate::render::viewport(size.0, size.1, false);
        if !matches!(frame.items.first(), Some(DrawItem::Course)) {
            clear(enc, target);
        }
        renderer.integer_scale = false;
        for item in frame.items {
            match item {
                DrawItem::Course => {
                    game.no_hud = true;
                    let scene = game.scene();
                    renderer.encode(gpu, enc, target, size, &scene);
                }
                DrawItem::Layout {
                    ui,
                    size: lsize,
                    quads,
                } => {
                    let i = self.layer(gpu, format, ui);
                    let cache_ui = match ui {
                        UiKind::Smm2 => Some(&self.front.smm2),
                        UiKind::Smm1 => self.front.smm1.as_ref(),
                    };
                    if let Some(u) = cache_ui {
                        self.layers[i].1.sync(gpu, &u.cache());
                    }
                    self.layers[i]
                        .1
                        .encode(gpu, enc, target, (vx, vy, vw, vh), &quads, lsize);
                }
            }
        }
        if !toast.is_empty() {
            let i = self.layer(gpu, format, UiKind::Smm2);
            self.layers[i].1.sync(gpu, &self.front.smm2.cache());
            self.layers[i]
                .1
                .encode(gpu, enc, target, (vx, vy, vw, vh), &toast, [1280.0, 720.0]);
        }
    }
}

fn clear(enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("front clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}
