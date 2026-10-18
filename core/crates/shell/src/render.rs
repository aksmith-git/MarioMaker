//! wgpu renderer: the port of the Swift app's `Renderer.swift` (Metal) to every wgpu backend
//! (Metal, Vulkan, DX12, GL/WebGL2, WebGPU).
//!
//! Per frame: clear black, letterbox the 16:9 game view into the target (whole-number scale when
//! it fits), fill the view with the theme's clear colour, draw the sim's sprites in layer order
//! (one draw per run of sprites sharing a texture; positions snapped to whole pixels like the
//! Swift renderer; underground night drawn upside down), then the HUD image on top.

use std::sync::Arc;

use mmk_assets::Cache;
use mmk_host::DrawSprite;
use mmk_sim::world::{VIEW_H, VIEW_W};
use wgpu::util::DeviceExt;

use crate::hud::{HudText, Image};

/// View size in pixels.
pub const VIEW: (u32, u32) = (VIEW_W as u32, VIEW_H as u32);

/// An adapter's device and queue, with what the renderer needs to know about them.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The device samples BC1-7 directly.
    pub bc: bool,
    pub info: wgpu::AdapterInfo,
    pub adapter: wgpu::Adapter,
}

impl Gpu {
    /// Pick an adapter (compatible with `surface` if given) and open a device. `allow_bc`: use
    /// BC textures when the adapter has them (false forces the CPU decode path).
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        allow_bc: bool,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no graphics adapter: {e}"))?;
        let info = adapter.get_info();
        let bc = allow_bc
            && adapter
                .features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let base = if info.backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mmk"),
                required_features: if bc {
                    wgpu::Features::TEXTURE_COMPRESSION_BC
                } else {
                    wgpu::Features::empty()
                },
                required_limits: base.using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("graphics device: {e}"))?;
        Ok(Self {
            device,
            queue,
            bc,
            info,
            adapter,
        })
    }
}

/// What one frame shows.
pub struct Scene<'a> {
    /// Sprites in draw order (see `mmk_host::SpriteResolver`).
    pub sprites: &'a [DrawSprite],
    /// Bottom-left of the view in world pixels.
    pub camera: (f32, f32),
    /// Draw the course upside down (underground night).
    pub upside_down: bool,
    /// 0xRRGGBBAA behind everything in the view.
    pub clear: u32,
    pub hud: &'a HudText,
    /// The 3D styles' meshes, skinned (`crate::mesh`).
    pub meshes: &'a crate::mesh::MeshFrame,
}

/// A cache texture on the GPU: not uploaded yet, unusable on this path, or ready.
enum TexState {
    Pending,
    Skipped,
    Ready(wgpu::BindGroup, wgpu::TextureView),
}

pub const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@location(0) v: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(v.xy, 0.0, 1.0);
    o.uv = v.zw;
    return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv);
    if (c.a < 0.01) {
        discard;
    }
    return c;
}

// texture checks: the texel as is, no blending or discard
@fragment
fn fs_copy(i: VOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, i.uv);
}
"#;

/// Which bind group a batch of vertices uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bind {
    Texture(u32),
    Missing(usize),
    Sky,
    Hud,
    /// Mesh group `i` of the frame's `MeshFrame`.
    Mesh(usize),
}

/// The meshes' shader: skinned vertices arrive in NDC with their depth; lit unless a card.
pub const MESH_SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) n: vec3<f32>,
    @location(2) flags: f32,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) n: vec3<f32>,
      @location(3) flags: f32) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(pos, 1.0);
    o.uv = uv;
    o.n = n;
    o.flags = flags;
    return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    var c = textureSample(tex, samp, i.uv);
    if (i.flags > 0.5) {
        // cards (pre-rendered sprites) are cut out by alpha
        if (c.a < 0.1) {
            discard;
        }
        return c;
    }
    // 3D materials: opaque [inf: render state not read], simple directional light
    var n = normalize(i.n);
    if (n.z < 0.0) {
        n = -n;
    }
    let l = normalize(vec3<f32>(-0.35, 0.6, 0.75));
    return vec4<f32>(c.rgb * (0.5 + 0.6 * max(dot(n, l), 0.0)), 1.0);
}
"#;

/// Depth buffer format of the frame pass (meshes test and write it, sprites ignore it).
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Placeholder colours by draw layer (terrain, blocks, coins, actors, player, effects), as in the
/// Swift app: sprites without art are drawn as these boxes in placeholder mode.
const MISSING: [(u32, [u8; 3]); 6] = [
    (11, [0xB0, 0x6A, 0x3A]),
    (20, [0xE0, 0xA8, 0x30]),
    (30, [0xF8, 0xD8, 0x40]),
    (50, [0x50, 0xA0, 0x50]),
    (60, [0xE0, 0x40, 0x40]),
    (u32::MAX, [0xF0, 0xF0, 0xF0]),
];

fn missing_bucket(layer: u32) -> usize {
    MISSING
        .iter()
        .position(|(below, _)| layer < *below)
        .unwrap_or(5)
}

/// An 8×8 box with a darker 1-px border.
fn missing_box(rgb: [u8; 3]) -> Vec<u8> {
    let mut px = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let edge = x == 0 || y == 0 || x == 7 || y == 7;
            let k = if edge { 0.55 } else { 1.0 };
            px.extend(rgb.map(|c| (c as f32 * k) as u8));
            px.push(0xFF);
        }
    }
    px
}

/// Letterboxed viewport of the game view in a `w × h` target: (x, y, width, height, scale).
pub fn viewport(w: u32, h: u32, integer: bool) -> (f32, f32, f32, f32, f32) {
    let (dw, dh) = (w as f32, h as f32);
    let fit = (dw / VIEW_W).min(dh / VIEW_H);
    let scale = if fit >= 1.0 && integer {
        fit.floor()
    } else {
        fit
    };
    let (vw, vh) = (VIEW_W * scale, VIEW_H * scale);
    (
        ((dw - vw) / 2.0).floor(),
        ((dh - vh) / 2.0).floor(),
        vw,
        vh,
        scale,
    )
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Cache texture index → bind group (None: not uploadable, e.g. BC6H without GPU support).
    textures: Vec<TexState>,
    cache: Arc<Cache>,
    missing: Vec<wgpu::BindGroup>,
    sky: (wgpu::Texture, wgpu::BindGroup),
    sky_color: Option<u32>,
    hud: (wgpu::Texture, wgpu::BindGroup),
    hud_text: Option<HudText>,
    vbuf: wgpu::Buffer,
    verts: Vec<[f32; 4]>,
    batches: Vec<(u32, u32, Bind)>,
    /// Draw sprites without art as coloured boxes (placeholder mode).
    pub draw_missing: bool,
    /// Whole-number scaling of the view (letterboxed).
    pub integer_scale: bool,
    /// Textures the GPU couldn't take (count, for the log).
    pub skipped_textures: usize,
    mesh_pipeline: wgpu::RenderPipeline,
    /// Linear samplers by wrap mode (bits 0–1 u, 2–3 v).
    mesh_samplers: std::collections::HashMap<u32, wgpu::Sampler>,
    /// (texture or u32::MAX for plain grey, wrap) → bind group.
    mesh_binds: std::collections::HashMap<(u32, u32), wgpu::BindGroup>,
    grey: wgpu::TextureView,
    mesh_vbuf: wgpu::Buffer,
    mesh_ibuf: wgpu::Buffer,
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
}

fn make_texture(
    gpu: &Gpu,
    label: &str,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_rgba(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32, data: &[u8]) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

impl Renderer {
    /// Build the pipeline for targets of `format` and upload every texture of `cache`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, cache: &Arc<Cache>) -> Self {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline = Self::pipeline(
            device,
            &layout,
            format,
            "fs",
            true,
            Some(Self::depth_state(false)),
        );
        let mesh_pipeline = Self::mesh_pipeline(device, &layout, format);
        // nearest sampling, clamped (SMB1 style is pixel-exact)
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind = |tex: &wgpu::Texture| {
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };

        // cache textures upload when a sprite first uses one (see `ensure`)
        let textures = (0..cache.textures.len())
            .map(|_| TexState::Pending)
            .collect();
        let skipped = 0;

        let srgb = wgpu::TextureFormat::Rgba8UnormSrgb;
        let missing = MISSING
            .iter()
            .map(|(_, rgb)| {
                let t = make_texture(gpu, "missing", srgb, 8, 8);
                write_rgba(gpu, &t, 8, 8, &missing_box(*rgb));
                bind(&t)
            })
            .collect();
        let sky = make_texture(gpu, "sky", srgb, 1, 1);
        let sky_bind = bind(&sky);
        let hud = make_texture(gpu, "hud", srgb, VIEW.0, VIEW.1);
        let hud_bind = bind(&hud);
        let vbuf = Self::vertex_buffer(device, 4096);
        let grey_tex = make_texture(gpu, "grey", srgb, 1, 1);
        write_rgba(gpu, &grey_tex, 1, 1, &[200, 200, 200, 255]);
        let grey = grey_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mesh_vbuf = Self::buffer(device, "mesh vertices", 1 << 16, wgpu::BufferUsages::VERTEX);
        let mesh_ibuf = Self::buffer(device, "mesh indices", 1 << 16, wgpu::BufferUsages::INDEX);
        Self {
            mesh_pipeline,
            mesh_samplers: Default::default(),
            mesh_binds: Default::default(),
            grey,
            mesh_vbuf,
            mesh_ibuf,
            depth: None,
            pipeline,
            layout,
            sampler,
            textures,
            cache: cache.clone(),
            missing,
            sky: (sky, sky_bind),
            sky_color: None,
            hud: (hud, hud_bind),
            hud_text: None,
            vbuf,
            verts: Vec::new(),
            batches: Vec::new(),
            draw_missing: false,
            integer_scale: true,
            skipped_textures: skipped,
        }
    }

    fn buffer(
        device: &wgpu::Device,
        label: &str,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Meshes test and write depth; sprites (and the sky / HUD) do neither.
    fn depth_state(mesh: bool) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(mesh),
            depth_compare: Some(if mesh {
                wgpu::CompareFunction::Less
            } else {
                wgpu::CompareFunction::Always
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    }

    fn mesh_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh"),
            source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<crate::mesh::MeshVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(Self::depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Bind group for a mesh piece: its texture (uploaded on first use) with a linear sampler of
    /// its wrap mode.
    fn mesh_bind(&mut self, gpu: &Gpu, texture: Option<u32>, wrap: u32) -> Option<()> {
        let key = (texture.unwrap_or(u32::MAX), wrap);
        if self.mesh_binds.contains_key(&key) {
            return Some(());
        }
        if let Some(t) = texture {
            self.ensure(gpu, t as usize);
        }
        let view = match texture.map(|t| self.textures.get(t as usize)) {
            None => &self.grey,
            Some(Some(TexState::Ready(_, v))) => v,
            Some(_) => &self.grey,
        };
        let mode = |w: u32| match w {
            1 => wgpu::AddressMode::MirrorRepeat,
            2 => wgpu::AddressMode::ClampToEdge,
            _ => wgpu::AddressMode::Repeat,
        };
        let sampler = self.mesh_samplers.entry(wrap).or_insert_with(|| {
            gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("mesh"),
                address_mode_u: mode(wrap & 3),
                address_mode_v: mode(wrap >> 2),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        self.mesh_binds.insert(key, group);
        Some(())
    }

    fn vertex_buffer(device: &wgpu::Device, verts: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprites"),
            size: (verts * 16) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// The sprite pipeline (`fs`, alpha blended) or the texture-check pipeline (`fs_copy`).
    pub fn pipeline(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        fragment: &str,
        blend: bool,
        depth: Option<wgpu::DepthStencilState>,
    ) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: blend.then_some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }
    /// Bind group of cache texture `i` (None if it isn't uploaded: not used yet, or a format
    /// this GPU path can't take).
    pub fn texture(&self, i: usize) -> Option<&wgpu::BindGroup> {
        match self.textures.get(i)? {
            TexState::Ready(b, _) => Some(b),
            _ => None,
        }
    }

    /// Draw from another asset cache (the web build opens the owner's after starting on
    /// placeholder art); its textures upload when first used, as at start.
    pub fn set_cache(&mut self, cache: &Arc<Cache>) {
        if Arc::ptr_eq(&self.cache, cache) {
            return;
        }
        self.cache = cache.clone();
        self.textures = (0..cache.textures.len())
            .map(|_| TexState::Pending)
            .collect();
        self.skipped_textures = 0;
        self.mesh_binds.clear();
    }

    /// Upload cache texture `i` if it hasn't been yet (pixels are read from the cache now).
    pub fn ensure(&mut self, gpu: &Gpu, i: usize) {
        if !matches!(self.textures.get(i), Some(TexState::Pending)) {
            return;
        }
        let entry = &self.cache.textures[i];
        let up = self
            .cache
            .texture(i)
            .and_then(|t| crate::texture::plan(t, gpu.bc));
        let Some(up) = up else {
            if self.cache.texture_pending(i) {
                // a browser cache: the pixels are on their way (`web::pump_textures`)
                return;
            }
            self.textures[i] = TexState::Skipped;
            self.skipped_textures += 1;
            return;
        };
        let tex = make_texture(gpu, &entry.name, up.format, up.width, up.height);
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &up.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(up.bytes_per_row),
                rows_per_image: Some(up.rows),
            },
            wgpu::Extent3d {
                width: up.width,
                height: up.height,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.textures[i] = TexState::Ready(group, view);
    }

    fn push(&mut self, bind: Bind, quad: [[f32; 4]; 6]) {
        let start = self.verts.len() as u32;
        self.verts.extend_from_slice(&quad);
        match self.batches.last_mut() {
            Some((s, n, b)) if *b == bind && *s + *n == start => *n += 6,
            _ => self.batches.push((start, 6, bind)),
        }
    }

    /// Full-view quad (NDC −1…1) with the texture's top row at the top.
    fn full_quad() -> [[f32; 4]; 6] {
        [
            [-1.0, -1.0, 0.0, 1.0],
            [1.0, -1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0, 0.0],
        ]
    }

    /// Record one frame into `encoder`, drawing into `target` (`size` pixels).
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        scene: &Scene,
    ) {
        // per-frame resources
        if self.sky_color != Some(scene.clear) {
            self.sky_color = Some(scene.clear);
            let c = scene.clear.to_be_bytes();
            write_rgba(gpu, &self.sky.0, 1, 1, &[c[0], c[1], c[2], 0xFF]);
        }
        if self.hud_text.as_ref() != Some(scene.hud) {
            let img: Image = scene.hud.draw(VIEW.0 as usize, VIEW.1 as usize);
            write_rgba(gpu, &self.hud.0, VIEW.0, VIEW.1, &img.rgba);
            self.hud_text = Some(scene.hud.clone());
        }

        self.verts.clear();
        self.batches.clear();
        self.push(Bind::Sky, Self::full_quad());

        // world px → NDC (camera snapped to whole pixels)
        let (cam_x, cam_y) = (scene.camera.0.round(), scene.camera.1.round());
        let (sx, sy) = (2.0 / VIEW_W, 2.0 / VIEW_H);
        let flip = if scene.upside_down { -1.0 } else { 1.0 };
        let ndc = |x: f32, y: f32| [(x - cam_x) * sx - 1.0, flip * ((y - cam_y) * sy - 1.0)];
        for s in scene.sprites {
            if let Some(t) = s.texture {
                self.ensure(gpu, t as usize);
            }
        }
        let meshes = scene.meshes;
        for p in &meshes.pieces {
            self.mesh_bind(gpu, p.texture, p.wrap);
        }
        let mut next_group = 0;
        for (k, s) in scene.sprites.iter().enumerate() {
            // mesh groups drawn before this sprite
            while next_group < meshes.groups.len() && meshes.groups[next_group].0 as usize <= k {
                self.batches.push((0, 0, Bind::Mesh(next_group)));
                next_group += 1;
            }
            let bind = match s.texture {
                Some(t) if self.texture(t as usize).is_some() => Bind::Texture(t),
                Some(_) => continue,
                None if self.draw_missing => Bind::Missing(missing_bucket(s.layer)),
                None => continue,
            };
            let (mut u0, mut u1) = (s.u0, s.u1);
            if s.flip_x {
                std::mem::swap(&mut u0, &mut u1);
            }
            let (x0, y0) = (s.x.round(), s.y.round());
            let (x1, y1) = (x0 + s.w, y0 + s.h);
            let [ax, ay] = ndc(x0, y0);
            let [bx, by] = ndc(x1, y1);
            // two triangles; v is top-down in the texture, y is up in the world
            self.push(
                bind,
                [
                    [ax, ay, u0, s.v1],
                    [bx, ay, u1, s.v1],
                    [bx, by, u1, s.v0],
                    [ax, ay, u0, s.v1],
                    [bx, by, u1, s.v0],
                    [ax, by, u0, s.v0],
                ],
            );
        }
        while next_group < meshes.groups.len() {
            self.batches.push((0, 0, Bind::Mesh(next_group)));
            next_group += 1;
        }
        self.push(Bind::Hud, Self::full_quad());

        // meshes: world px → NDC like the sprites, depth as skinned
        if !meshes.vertices.is_empty() {
            let verts: Vec<crate::mesh::MeshVertex> = meshes
                .vertices
                .iter()
                .map(|v| {
                    let [x, y] = ndc(v.pos[0], v.pos[1]);
                    crate::mesh::MeshVertex {
                        pos: [x, y, v.pos[2]],
                        ..*v
                    }
                })
                .collect();
            let vb: &[u8] = bytemuck::cast_slice(&verts);
            if vb.len() as u64 > self.mesh_vbuf.size() {
                self.mesh_vbuf = Self::buffer(
                    &gpu.device,
                    "mesh vertices",
                    (vb.len() as u64).next_power_of_two(),
                    wgpu::BufferUsages::VERTEX,
                );
            }
            gpu.queue.write_buffer(&self.mesh_vbuf, 0, vb);
            let ib: &[u8] = bytemuck::cast_slice(&meshes.indices);
            if ib.len() as u64 > self.mesh_ibuf.size() {
                self.mesh_ibuf = Self::buffer(
                    &gpu.device,
                    "mesh indices",
                    (ib.len() as u64).next_power_of_two(),
                    wgpu::BufferUsages::INDEX,
                );
            }
            gpu.queue.write_buffer(&self.mesh_ibuf, 0, ib);
        }
        if self
            .depth
            .as_ref()
            .is_none_or(|(t, _)| t.width() != size.0.max(1) || t.height() != size.1.max(1))
        {
            let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth = Some((t, v));
        }

        let bytes: &[u8] = bytemuck::cast_slice(&self.verts);
        if bytes.len() as u64 > self.vbuf.size() {
            self.vbuf = Self::vertex_buffer(&gpu.device, self.verts.len().next_power_of_two());
        }
        gpu.queue.write_buffer(&self.vbuf, 0, bytes);

        let (vx, vy, vw, vh, _) = viewport(size.0, size.1, self.integer_scale);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: self.depth.as_ref().map(|(_, v)| {
                wgpu::RenderPassDepthStencilAttachment {
                    view: v,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }
            }),
            ..Default::default()
        });
        if vw < 1.0 || vh < 1.0 {
            return;
        }
        pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        for &(start, n, bind) in &self.batches {
            if let Bind::Mesh(g) = bind {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, self.mesh_vbuf.slice(..));
                pass.set_index_buffer(self.mesh_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                for p in &meshes.pieces[meshes.groups[g].1.clone()] {
                    let key = (p.texture.unwrap_or(u32::MAX), p.wrap);
                    if let Some(b) = self.mesh_binds.get(&key) {
                        pass.set_bind_group(0, b, &[]);
                        pass.draw_indexed(p.first..p.first + p.count, 0, 0..1);
                    }
                }
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.vbuf.slice(..));
                continue;
            }
            let group = match bind {
                Bind::Texture(t) => self.texture(t as usize).expect("ensured above"),
                Bind::Missing(i) => &self.missing[i],
                Bind::Sky => &self.sky.1,
                Bind::Hud => &self.hud.1,
                Bind::Mesh(_) => continue,
            };
            pass.set_bind_group(0, group, &[]);
            pass.draw(start..start + n, 0..1);
        }
    }

    /// A vertex buffer holding one quad that fills the target (texture checks).
    pub fn quad_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad"),
            contents: bytemuck::cast_slice(&Self::full_quad()),
            usage: wgpu::BufferUsages::VERTEX,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_letterboxes_at_whole_scales() {
        assert_eq!(viewport(1152, 648, true), (0.0, 0.0, 1152.0, 648.0, 3.0));
        // wider window: centred, still ×3
        assert_eq!(viewport(1300, 700, true), (74.0, 26.0, 1152.0, 648.0, 3.0));
        // smaller than the view: fractional
        let (_, _, w, _, s) = viewport(192, 108, true);
        assert_eq!((w, s), (192.0, 0.5));
        assert_eq!(viewport(1300, 700, false).4, 700.0 / 216.0);
    }

    #[test]
    fn missing_art_colours_follow_layers() {
        assert_eq!(missing_bucket(0), 0);
        assert_eq!(missing_bucket(15), 1);
        assert_eq!(missing_bucket(55), 4);
        assert_eq!(missing_bucket(90), 5);
        assert_eq!(missing_box([100, 0, 0])[..4], [55, 0, 0, 255]);
    }
}
