//! Offscreen rendering for smoke tests: run N scripted ticks, draw one frame into a texture and
//! save it as PNG; and a texture check comparing the GPU's BCn sampling with the CPU decoder.

use crate::game::Game;
use crate::render::{Gpu, Renderer, VIEW};
use crate::session::scripted_run;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

fn target(gpu: &Gpu, w: u32, h: u32) -> wgpu::Texture {
    target_as(gpu, w, h, FORMAT)
}

fn target_as(gpu: &Gpu, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Copy an RGBA8 texture back to the CPU (tightly packed rows, top row first).
fn read_back(gpu: &Gpu, mut enc: wgpu::CommandEncoder, tex: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (tex.width(), tex.height());
    let row = w * 4;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |r| {
        r.expect("map readback buffer");
    });
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("wait for the GPU");
    let mapped = buf.slice(..).get_mapped_range().expect("mapped readback");
    let mut out = Vec::with_capacity((row * h) as usize);
    for y in 0..h {
        let o = (y * padded) as usize;
        out.extend_from_slice(&mapped[o..o + row as usize]);
    }
    out
}

pub fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut wr = enc.write_header().map_err(|e| format!("{path}: {e}"))?;
    wr.write_image_data(rgba)
        .map_err(|e| format!("{path}: {e}"))?;
    wr.finish().map_err(|e| format!("{path}: {e}"))
}

/// Run `ticks` ticks of the smoke-test script (or idle input), then save the frame as a PNG of
/// the view at `scale`× (or fitted into `size` pixels).
#[allow(clippy::too_many_arguments)]
pub fn screenshot(
    game: &mut Game,
    path: &str,
    ticks: u64,
    scale: u32,
    size: Option<(u32, u32)>,
    view_at: Option<(f32, f32)>,
    scripted: bool,
    allow_bc: bool,
) -> Result<String, String> {
    // the maker view is drawn as it is (nothing ticks while editing)
    let ticks = if game.editing { 0 } else { ticks };
    for _ in 0..ticks {
        let input = if scripted {
            scripted_run(game.session.world.tick)
        } else {
            mmk_sim::Input::default()
        };
        game.session.tick(input);
    }
    if let Some((x, y)) = view_at {
        let world = &mut game.session.world;
        world.camera_x = x;
        world.camera_y = y;
    }
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    renderer.draw_missing = game.placeholder;
    let (w, h) = size.unwrap_or((VIEW.0 * scale.max(1), VIEW.1 * scale.max(1)));
    renderer.integer_scale = size.is_none();
    let tex = target(&gpu, w, h);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let scene = game.scene();
    let sprites = scene.sprites.len();
    let effects = scene.effects.quads.len();
    renderer.encode(&gpu, &mut enc, &view, (w, h), &scene);
    let rgba = read_back(&gpu, enc, &tex);
    write_png(path, w, h, &rgba)?;
    Ok(format!(
        "{path}: {w}x{h}, tick {}, {sprites} sprites, {effects} effect quads, {} ({:?}), BC textures {}",
        game.session.world.tick,
        gpu.info.name,
        gpu.info.backend,
        if gpu.bc { "on GPU" } else { "CPU-decoded" }
    ))
}

/// Parse an input script: per line "N BUTTONS…" (N ticks holding the buttons: JUMP, RUN,
/// SPIN, LEFT, RIGHT, UP, DOWN; "-" for none; `#` starts a comment) → one input per tick.
pub fn parse_input_script(text: &str) -> Result<Vec<mmk_sim::Input>, String> {
    use mmk_sim::input;
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut t = line.split_whitespace();
        let count: usize = t
            .next()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| format!("line {}: no tick count", n + 1))?;
        let mut buttons = 0;
        for b in t {
            buttons |= match b.to_ascii_uppercase().as_str() {
                "JUMP" => input::JUMP,
                "RUN" => input::RUN,
                "SPIN" => input::SPIN,
                "LEFT" => input::LEFT,
                "RIGHT" => input::RIGHT,
                "UP" => input::UP,
                "DOWN" => input::DOWN,
                "-" => 0,
                other => return Err(format!("line {}: unknown button {other}", n + 1)),
            };
        }
        out.extend(std::iter::repeat_n(mmk_sim::Input { buttons }, count));
    }
    Ok(out)
}

/// Effect check: play `sets` (comma-separated emitter set names) side by side at mid-height of
/// the course's first view and save the frame `frames` frames in.
pub fn fx_preview(
    game: &mut Game,
    path: &str,
    sets: &str,
    frames: u64,
    scale: u32,
    allow_bc: bool,
) -> Result<String, String> {
    game.session.tick(mmk_sim::Input::default());
    let names: Vec<&str> = sets.split(',').filter(|s| !s.is_empty()).collect();
    let fx = game.session.effects.as_mut().ok_or("no effect director")?;
    let w = &game.session.world;
    let (vw, vh) = (VIEW.0 as f32, VIEW.1 as f32);
    let n = names.len().max(1) as f32;
    let mut missing = Vec::new();
    for (k, name) in names.iter().enumerate() {
        let Some(set) = fx.lib.sets.get(*name).cloned() else {
            missing.push(*name);
            continue;
        };
        let asset = mmk_host::effects::library::Asset {
            set: name.to_string(),
            scale: 1.0,
            pos: [0.0; 3],
            rot: [0.0; 3],
            color: [1.0; 4],
            rate: 1.0,
            life: 1.0,
            dir_vel: 1.0,
            delay: 0,
            duration: 0,
        };
        let x = w.camera_x + vw * (k as f32 + 0.5) / n;
        fx.player
            .play(set, &asset, x, w.camera_y + vh * 0.5, false, true);
    }
    for _ in 0..frames {
        fx.player.step();
    }
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    let (w, h) = (VIEW.0 * scale.max(1), VIEW.1 * scale.max(1));
    let tex = target(&gpu, w, h);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let no_hud = crate::hud::HudText {
        strip: String::new(),
        banner: None,
    };
    let scene = game.scene();
    let quads = scene.effects.quads.len();
    let scene = crate::render::Scene {
        hud: &no_hud,
        ..scene
    };
    renderer.encode(&gpu, &mut enc, &view, (w, h), &scene);
    let rgba = read_back(&gpu, enc, &tex);
    write_png(path, w, h, &rgba)?;
    Ok(format!(
        "{path}: {frames} frames, {quads} effect quads{}",
        if missing.is_empty() {
            String::new()
        } else {
            format!(", not in the cache: {}", missing.join(", "))
        }
    ))
}

/// Render a whole course area (0 main, 1 sub) at `scale`× into one PNG: the view is moved over
/// the area in view-sized steps (actors spawned as the camera reaches them, nothing ticks) and
/// the frames are pasted together, without the HUD.
pub fn map(
    game: &mut Game,
    path: &str,
    area: usize,
    scale: u32,
    allow_bc: bool,
) -> Result<String, String> {
    let scale = scale.max(1);
    let world = &mut game.session.world;
    world.show_area(area);
    let (left, bottom, right, top) = world.area_bounds();
    let (vw, vh) = (VIEW.0 as f32, VIEW.1 as f32);
    let (cols, rows) = (
        ((right - left) / vw).ceil().max(1.0) as u32,
        ((top - bottom) / vh).ceil().max(1.0) as u32,
    );
    let (fw, fh) = (VIEW.0 * scale, VIEW.1 * scale);
    let (w, h) = (
        ((right - left) * scale as f32) as u32,
        ((top - bottom) * scale as f32) as u32,
    );
    let mut out = vec![0u8; (w * h * 4) as usize];
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    renderer.draw_missing = game.placeholder;
    let tex = target(&gpu, fw, fh);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let no_hud = crate::hud::HudText {
        strip: String::new(),
        banner: None,
    };
    for row in 0..rows {
        for col in 0..cols {
            let world = &mut game.session.world;
            world.look_at(left + col as f32 * vw, bottom + row as f32 * vh);
            let (cx, cy) = (world.camera_x, world.camera_y);
            let scene = game.scene();
            let scene = crate::render::Scene {
                hud: &no_hud,
                ..scene
            };
            let mut enc = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            renderer.encode(&gpu, &mut enc, &view, (fw, fh), &scene);
            let rgba = read_back(&gpu, enc, &tex);
            // paste: frame pixel (fx, fy) (fy from the top) → world (cx + fx/s, cy + vh − fy/s)
            let ox = ((cx - left) * scale as f32) as i64;
            let oy = ((top - (cy + vh)) * scale as f32) as i64;
            for fy in 0..fh as i64 {
                let y = oy + fy;
                if y < 0 || y >= h as i64 {
                    continue;
                }
                for fx in 0..fw as i64 {
                    let x = ox + fx;
                    if x < 0 || x >= w as i64 {
                        continue;
                    }
                    let s = ((fy * fw as i64 + fx) * 4) as usize;
                    let d = ((y * w as i64 + x) * 4) as usize;
                    out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
                }
            }
        }
    }
    write_png(path, w, h, &out)?;
    Ok(format!(
        "{path}: {w}x{h} ({cols}×{rows} views), area {area}"
    ))
}

/// Draw every block-compressed texture of the cache at 1:1 with the GPU sampling BCn directly
/// and compare with the CPU decoder (`texture::decode_rgba`). Returns a report; Err when the GPU
/// has no BC support or a texture differs by more than rounding.
pub fn check_textures(game: &Game) -> Result<String, String> {
    use mmk_assets::MtexFormat;
    let gpu = pollster::block_on(Gpu::new(&instance(), None, true))?;
    if !gpu.bc {
        return Err(format!("{}: no BC texture support", gpu.info.name));
    }
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    // sRGB textures go through an sRGB target (decoded, then encoded again), linear ones
    // through a linear target, so the texel bytes come back unchanged
    const LINEAR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    let copy = |format| {
        Renderer::pipeline(
            &gpu.device,
            renderer.bind_group_layout(),
            format,
            "fs_copy",
            false,
            None,
        )
    };
    let (srgb_pipeline, linear_pipeline) = (copy(FORMAT), copy(LINEAR));
    let quad = Renderer::quad_buffer(&gpu.device);
    let mut report = Vec::new();
    let mut worst_all = 0u8;
    for (i, t) in game.cache.textures.iter().enumerate() {
        let Some(m) = game.cache.texture(i) else {
            report.push(format!("{}/{}: unreadable", t.model, t.name));
            continue;
        };
        if m.format == MtexFormat::Rgba8 {
            continue;
        }
        renderer.ensure(&gpu, i);
        let Some(group) = renderer.texture(i) else {
            report.push(format!("{}/{}: not uploaded", t.model, t.name));
            continue;
        };
        let Some(cpu) = crate::texture::decode_rgba(m) else {
            report.push(format!("{}/{}: no CPU decoder", t.model, t.name));
            continue;
        };
        let srgb = m.variant == mmk_assets::MtexVariant::Srgb
            && !matches!(m.format, MtexFormat::Bc4 | MtexFormat::Bc5);
        let (format, pipeline) = if srgb {
            (FORMAT, &srgb_pipeline)
        } else {
            (LINEAR, &linear_pipeline)
        };
        let tex = target_as(&gpu, m.width, m.height, format);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, quad.slice(..));
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, 0..1);
        }
        let gpu_px = read_back(&gpu, enc, &tex);
        let worst = gpu_px
            .iter()
            .zip(&cpu)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        worst_all = worst_all.max(worst);
        report.push(format!(
            "{}/{} {}x{} {}: max channel difference {worst}",
            t.model,
            t.name,
            m.width,
            m.height,
            m.format.name()
        ));
    }
    let text = report.join("\n");
    // BC1-3 colour interpolation is only specified to within a few steps (GPUs round
    // differently from `mmk_formats::bcn`); BC4, BC5 and BC7 match exactly on Apple GPUs
    if worst_all > 3 { Err(text) } else { Ok(text) }
}

/// Render layout `name` from the UI import in `dir` at 1280×720 after `ticks` frames of the
/// animations in `anims` (see `--ui-anim`), texts from `texts` (`pane=text;…`), to a PNG.
pub fn layout(
    dir: &std::path::Path,
    name: &str,
    anims: &str,
    texts: &str,
    ticks: u64,
    path: &str,
    allow_bc: bool,
) -> Result<String, String> {
    let mut cache = mmk_ui::UiCache::open(dir).map_err(|e| format!("{}: {e:?}", dir.display()))?;
    let res = cache
        .layout(name)
        .map_err(|e| format!("layout {name}: {e:?}"))?;
    let mut player = mmk_ui::LayoutPlayer::new(res);
    for pair in texts.split(';').filter(|p| !p.is_empty()) {
        if let Some((pane, text)) = pair.split_once('=') {
            player.set_text(pane, text);
        }
    }
    let t = ticks as f32;
    for spec in anims.split(',').filter(|s| !s.is_empty()) {
        let (anim, frame) = if let Some((a, f)) = spec.split_once('@') {
            (a, f.parse::<f32>().map_err(|e| format!("{spec}: {e}"))?)
        } else if let Some(a) = spec.strip_suffix('~') {
            let len = player
                .animation_length(a)
                .ok_or(format!("no animation {a}"))?;
            (a, if len > 0.0 { t % len } else { 0.0 })
        } else {
            let len = player
                .animation_length(spec)
                .ok_or(format!("no animation {spec}"))?;
            (spec, t.min(len))
        };
        player.play(anim, frame);
    }
    let size = player.size();
    let quads = player.draw_list();
    let (w, h) = (1280, 720);
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut layer = crate::ui_layer::UiLayer::new(&gpu, FORMAT);
    layer.sync(&gpu, &cache);
    let tex = target(&gpu, w, h);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
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
    // letterbox the layout's aspect into the target
    let s = (w as f32 / size[0]).min(h as f32 / size[1]);
    let (vw, vh) = (size[0] * s, size[1] * s);
    let vp = ((w as f32 - vw) / 2.0, (h as f32 - vh) / 2.0, vw, vh);
    layer.encode(&gpu, &mut enc, &view, vp, &quads, size);
    let rgba = read_back(&gpu, enc, &tex);
    write_png(path, w, h, &rgba)?;
    Ok(format!(
        "{path}: {name}, {} quads, {}×{} layout units",
        quads.len(),
        size[0],
        size[1]
    ))
}

/// Render the front end after it has run: `ticks` front ticks, or for a course screen
/// (`play`), until that screen has been up `ticks` ticks (as the Swift app's frozen captures:
/// `MMK_UI_TICKS`). Saves a `size` PNG.
#[allow(clippy::too_many_arguments)]
pub fn front(
    app: &mut crate::front_app::FrontApp,
    game: &mut Game,
    ticks: u64,
    course_screen: bool,
    scripted: bool,
    size: (u32, u32),
    path: &str,
    allow_bc: bool,
) -> Result<String, String> {
    let input = mmk_front::FrontInput::default();
    if course_screen {
        // (a screen that comes up later, e.g. the pause after some play, is waited for)
        for _ in 0..ticks + 3600 {
            let done = match &app.front.screen {
                mmk_front::Screen::Course(p) => p.menu_up() && p.since_mode() as u64 >= ticks,
                _ => true,
            };
            if done {
                break;
            }
            app.tick(&input, 0, game);
        }
    } else {
        // MMK_FRONT_REALTIME=1: one tick per 1/60 s (gives a server listing time to arrive)
        let realtime = std::env::var("MMK_FRONT_REALTIME").as_deref() == Ok("1");
        // MMK_FRONT_TYPE=TICK:TEXT types TEXT into an open text field, a key per tick from
        // tick TICK ("\n" is Return)
        let mut typed: Vec<char> = Vec::new();
        let mut type_at = u64::MAX;
        if let Ok(t) = std::env::var("MMK_FRONT_TYPE")
            && let Some((at, text)) = t.split_once(':')
        {
            type_at = at.parse().unwrap_or(u64::MAX);
            typed = text.replace("\\n", "\n").chars().rev().collect();
        }
        for i in 0..ticks {
            let mut input = input;
            if i >= type_at {
                input.text = typed.pop();
            }
            // the smoke-test script as the player's input (the Swift app's MMK_SCREENSHOT)
            let b = if scripted {
                scripted_run(game.session.world.tick).buttons
            } else {
                0
            };
            app.tick(&input, b, game);
            if realtime {
                std::thread::sleep(std::time::Duration::from_micros(16_667));
            }
        }
    }
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    renderer.draw_missing = game.placeholder;
    let (w, h) = size;
    let tex = target(&gpu, w, h);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    app.encode(&gpu, &mut enc, &view, FORMAT, size, &mut renderer, game);
    let rgba = read_back(&gpu, enc, &tex);
    write_png(path, w, h, &rgba)?;
    Ok(format!("{path}: {w}x{h}, front tick {}", app.front.tick))
}
