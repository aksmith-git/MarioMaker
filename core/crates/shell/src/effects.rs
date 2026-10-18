//! The effects pass: particle quads (`mmk_host::effects`) drawn over the course's sprites and
//! meshes, under the HUD, one pipeline per blend mode. A separate step of the frame pass
//! (`Renderer::encode` calls `prepare` and, at its place in the batch list, `draw`); the Swift
//! app's `EffectsRenderer.swift` draws the same way.

use std::collections::HashMap;

use mmk_assets::Cache;
use mmk_host::effects::{Blend, FxQuad, TEXTURE_MODEL};

use crate::render::{DEPTH, Gpu};

/// What the effects pass draws this frame: the quads and, by the quads' texture name index,
/// the cache texture to sample (None: the cache lacks it; the quad is skipped).
#[derive(Debug, Default)]
pub struct EffectFrame {
    pub quads: Vec<FxQuad>,
    pub textures: Vec<Option<u32>>,
}

impl EffectFrame {
    /// Map the player's texture names to cache textures (`Effect/<name>`).
    pub fn resolve_textures(&mut self, names: &[String], cache: &Cache) {
        self.textures.clear();
        self.textures.extend(
            names
                .iter()
                .map(|n| cache.texture_index(TEXTURE_MODEL, n).map(|i| i as u32)),
        );
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) c: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.uv = uv;
    o.color = c;
    return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

// alpha, add, subtract, premultiplied: colour and alpha as they are
@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv) * i.color;
    return vec4<f32>(c.rgb, clamp(c.a, 0.0, 1.0));
}

// multiply: the colour faded towards white by (1 − alpha)
@fragment
fn fs_mul(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv) * i.color;
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(mix(vec3<f32>(1.0), c.rgb, a), 1.0);
}

// screen: the colour weighted by alpha
@fragment
fn fs_screen(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv) * i.color;
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(min(c.rgb * a, vec3<f32>(1.0)), a);
}
"#;

const BLENDS: [Blend; 6] = [
    Blend::Alpha,
    Blend::Add,
    Blend::Sub,
    Blend::Mul,
    Blend::Screen,
    Blend::Premul,
];

fn blend_state(b: Blend) -> (wgpu::BlendState, &'static str) {
    use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
    let comp = |src, dst, operation| C {
        src_factor: src,
        dst_factor: dst,
        operation,
    };
    let keep_alpha = comp(F::Zero, F::One, O::Add);
    let (color, fs) = match b {
        Blend::Alpha => (comp(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add), "fs"),
        Blend::Add => (comp(F::SrcAlpha, F::One, O::Add), "fs"),
        Blend::Sub => (comp(F::SrcAlpha, F::One, O::ReverseSubtract), "fs"),
        Blend::Mul => (comp(F::Zero, F::Src, O::Add), "fs_mul"),
        Blend::Screen => (comp(F::OneMinusDst, F::One, O::Add), "fs_screen"),
        Blend::Premul => (comp(F::One, F::OneMinusSrcAlpha, O::Add), "fs"),
    };
    (
        wgpu::BlendState {
            color,
            alpha: keep_alpha,
        },
        fs,
    )
}

pub struct EffectsPass {
    pipelines: Vec<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    /// Samplers by (wrap u, wrap v, nearest).
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    /// (cache texture, sampler) → bind group.
    binds: HashMap<BindKey, wgpu::BindGroup>,
    vbuf: wgpu::Buffer,
    verts: Vec<Vertex>,
    /// (first vertex, vertex count, blend index, bind key)
    draws: Vec<(u32, u32, usize, BindKey)>,
}

type SamplerKey = (u8, u8, bool);
type BindKey = (u32, SamplerKey);

/// The game's wrap (0 mirror, 1 repeat, 2 clamp, 3 mirror once) as a wgpu address mode.
fn address(wrap: u8) -> wgpu::AddressMode {
    match wrap {
        0 | 3 => wgpu::AddressMode::MirrorRepeat,
        1 => wgpu::AddressMode::Repeat,
        _ => wgpu::AddressMode::ClampToEdge,
    }
}

impl EffectsPass {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, layout: &wgpu::BindGroupLayout) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        let pipelines = BLENDS
            .iter()
            .map(|&b| {
                let (blend, fs) = blend_state(b);
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("effects"),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                            ],
                        })],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        Self {
            pipelines,
            layout: layout.clone(),
            samplers: HashMap::new(),
            binds: HashMap::new(),
            vbuf: Self::buffer(device, 1024),
            verts: Vec::new(),
            draws: Vec::new(),
        }
    }

    fn buffer(device: &wgpu::Device, verts: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects"),
            size: (verts * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Forget the bind groups (the cache changed).
    pub fn reset(&mut self) {
        self.binds.clear();
    }

    /// Build this frame's vertices. `view(i)` gives cache texture `i` uploaded (None: not
    /// drawable); `ndc` maps world pixels to clip space like the sprites.
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        frame: &EffectFrame,
        mut view: impl FnMut(u32) -> Option<wgpu::TextureView>,
        ndc: impl Fn(f32, f32) -> [f32; 2],
    ) {
        self.verts.clear();
        self.draws.clear();
        for q in &frame.quads {
            let Some(tex) = q
                .texture
                .and_then(|t| frame.textures.get(t as usize).copied().flatten())
            else {
                continue;
            };
            let skey = (q.wrap[0], q.wrap[1], q.nearest);
            let key = (tex, skey);
            if !self.binds.contains_key(&key) {
                let Some(v) = view(tex) else { continue };
                let sampler = self.samplers.entry(skey).or_insert_with(|| {
                    let filter = if q.nearest {
                        wgpu::FilterMode::Nearest
                    } else {
                        wgpu::FilterMode::Linear
                    };
                    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("effects"),
                        address_mode_u: address(q.wrap[0]),
                        address_mode_v: address(q.wrap[1]),
                        mag_filter: filter,
                        min_filter: filter,
                        ..Default::default()
                    })
                });
                let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("effect texture"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&v),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ],
                });
                self.binds.insert(key, group);
            }
            let blend = BLENDS.iter().position(|&b| b == q.blend).unwrap_or(0);
            let v = |k: usize| Vertex {
                pos: ndc(q.pos[k][0], q.pos[k][1]),
                uv: q.uv[k],
                color: q.color,
            };
            let start = self.verts.len() as u32;
            self.verts
                .extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
            match self.draws.last_mut() {
                Some((s, n, b, k)) if *b == blend && *k == key && *s + *n == start => *n += 6,
                _ => self.draws.push((start, 6, blend, key)),
            }
        }
        if self.verts.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&self.verts);
        if bytes.len() as u64 > self.vbuf.size() {
            self.vbuf = Self::buffer(&gpu.device, self.verts.len().next_power_of_two());
        }
        gpu.queue.write_buffer(&self.vbuf, 0, bytes);
    }

    /// Draw the prepared quads (the caller restores its own pipeline and buffers afterwards).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        let mut current = usize::MAX;
        for &(start, n, blend, key) in &self.draws {
            let Some(group) = self.binds.get(&key) else {
                continue;
            };
            if blend != current {
                pass.set_pipeline(&self.pipelines[blend]);
                current = blend;
            }
            pass.set_bind_group(0, group, &[]);
            pass.draw(start..start + n, 0..1);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.draws.is_empty()
    }
}
