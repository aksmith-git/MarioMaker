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
/// the view at `scale`×.
pub fn screenshot(
    game: &mut Game,
    path: &str,
    ticks: u64,
    scale: u32,
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
    let gpu = pollster::block_on(Gpu::new(&instance(), None, allow_bc))?;
    let mut renderer = Renderer::new(&gpu, FORMAT, &game.cache);
    renderer.draw_missing = game.placeholder;
    let (w, h) = (VIEW.0 * scale.max(1), VIEW.1 * scale.max(1));
    let tex = target(&gpu, w, h);
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let scene = game.scene();
    let sprites = scene.sprites.len();
    renderer.encode(&gpu, &mut enc, &view, (w, h), &scene);
    let rgba = read_back(&gpu, enc, &tex);
    write_png(path, w, h, &rgba)?;
    Ok(format!(
        "{path}: {w}x{h}, tick {}, {sprites} sprites, {} ({:?}), BC textures {}",
        game.session.world.tick,
        gpu.info.name,
        gpu.info.backend,
        if gpu.bc { "on GPU" } else { "CPU-decoded" }
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
    let renderer = Renderer::new(&gpu, FORMAT, &game.cache);
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
        )
    };
    let (srgb_pipeline, linear_pipeline) = (copy(FORMAT), copy(LINEAR));
    let quad = Renderer::quad_buffer(&gpu.device);
    let mut report = Vec::new();
    let mut worst_all = 0u8;
    for (i, t) in game.cache.textures.iter().enumerate() {
        let m = &t.texture;
        if m.format == MtexFormat::Rgba8 {
            continue;
        }
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
