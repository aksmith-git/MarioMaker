//! Layout-player output (`mmk_ui::Quad`, SMM1 / SMM2 screens) on wgpu: the port of the Swift
//! app's `UILayer.swift`, with the same shading (mmk.h): `t = combine(tex0, tex1)`,
//! `out = mix(black, white, t) · vertex colour`, colours authored as sRGB and linearised in the
//! shader; the target is an sRGB view. One pipeline per blend mode, one sampler per wrap/filter
//! combination, one draw per run of quads sharing textures, samplers and blend.

use std::collections::{HashMap, HashSet};

use mmk_ui::{Quad, TEXTURE_NONE, UiCache};
use wgpu::util::DeviceExt;

use crate::render::Gpu;

const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) black: vec4<f32>,
    @location(4) white: vec4<f32>,
    @location(5) @interpolate(flat) flags: u32,
};

fn lin(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn lin4(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(lin(c.rgb), c.a);
}

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) uv0: vec2<f32>, @location(2) uv1: vec2<f32>,
      @location(3) color: vec4<f32>, @location(4) black: vec4<f32>, @location(5) white: vec4<f32>,
      @location(6) flags: u32) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(pos, 0.0, 1.0);
    o.uv0 = uv0;
    o.uv1 = uv1;
    o.color = lin4(color);
    o.black = lin4(black);
    o.white = lin4(white);
    o.flags = flags;
    return o;
}

@group(0) @binding(0) var t0: texture_2d<f32>;
@group(0) @binding(1) var s0: sampler;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var s1: sampler;

fn comb(a: f32, b: f32, m: u32) -> f32 {
    switch m {
        case 0u: { return b; }
        case 1u: { return a * b; }
        case 2u: { return min(a + b, 1.0); }
        case 5u: { return max(a - b, 0.0); }
        default: { return a; }
    }
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // sample first (uniform control flow), then pick
    let a0 = textureSample(t0, s0, i.uv0);
    let b = textureSample(t1, s1, i.uv1);
    var a = vec4<f32>(1.0);
    if ((i.flags & 1u) != 0u) {
        a = a0;
    }
    var t = a;
    if ((i.flags & 2u) != 0u) {
        let cm = (i.flags >> 12u) & 15u;
        let am = (i.flags >> 16u) & 15u;
        t = vec4<f32>(comb(a.r, b.r, cm), comb(a.g, b.g, cm), comb(a.b, b.b, cm), comb(a.a, b.a, am));
    }
    return mix(i.black, i.white, t) * i.color;
}
"#;

/// Matches the shader's vertex input (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv0: [f32; 2],
    uv1: [f32; 2],
    color: [u8; 4],
    black: [u8; 4],
    white: [u8; 4],
    flags: u32,
}

/// (tex0, tex1, sampler 0, sampler 1): what one bind group holds.
type BindKey = (u32, u32, u32, u32);

pub struct UiLayer {
    format: wgpu::TextureFormat,
    module: wgpu::ShaderModule,
    bind_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// The cache's textures by id (None: unusable on this device).
    textures: Vec<Option<wgpu::TextureView>>,
    /// Textures whose pixels are on their way (the web build's cache): not drawn yet.
    waiting: HashSet<u32>,
    /// The cache's revision when `waiting` was last looked at.
    revision: u64,
    white: wgpu::TextureView,
    pipelines: HashMap<u32, wgpu::RenderPipeline>,
    samplers: HashMap<u32, wgpu::Sampler>,
    binds: HashMap<BindKey, wgpu::BindGroup>,
}

/// The wgpu blend state for the 12 blend bits (op, src, dst; None: overwrite).
pub fn blend_state(blend: u32) -> Option<wgpu::BlendState> {
    use wgpu::{BlendFactor as F, BlendOperation as O};
    let (op, src, dst) = (blend & 15, (blend >> 4) & 15, (blend >> 8) & 15);
    if op == 0 {
        return None;
    }
    let factor = |f: u32| match f {
        0 => F::Zero,
        1 => F::One,
        2 => F::Dst,
        3 => F::OneMinusDst,
        4 => F::SrcAlpha,
        5 => F::OneMinusSrcAlpha,
        6 => F::DstAlpha,
        7 => F::OneMinusDstAlpha,
        8 => F::Src,
        _ => F::OneMinusSrc,
    };
    let operation = match op {
        2 => O::Subtract,
        3 => O::ReverseSubtract,
        4 => O::Min,
        5 => O::Max,
        _ => O::Add,
    };
    // min / max ignore the factors (Metal); wgpu requires them to be One
    let (s, d) = if matches!(operation, O::Min | O::Max) {
        (F::One, F::One)
    } else {
        (factor(src), factor(dst))
    };
    let c = wgpu::BlendComponent {
        src_factor: s,
        dst_factor: d,
        operation,
    };
    Some(wgpu::BlendState { color: c, alpha: c })
}

impl UiLayer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let d = &gpu.device;
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let bind_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[tex(0), samp(1), tex(2), samp(3)],
        });
        let pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let white_tex = crate::render::make_texture(gpu, "ui white", format_rgba(), 1, 1);
        crate::render::write_rgba(gpu, &white_tex, 1, 1, &[255; 4]);
        Self {
            format,
            module,
            bind_layout,
            pipeline_layout,
            textures: Vec::new(),
            waiting: HashSet::new(),
            revision: 0,
            white: white_tex.create_view(&wgpu::TextureViewDescriptor::default()),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            binds: HashMap::new(),
        }
    }

    /// Upload the cache's textures added since the last call (after opening a layout or font),
    /// and those whose pixels arrived since.
    pub fn sync(&mut self, gpu: &Gpu, cache: &UiCache) {
        let upload = |e: &mmk_ui::TextureEntry| {
            crate::texture::plan(&e.texture, gpu.bc)
                .map(|up| crate::render::upload(gpu, &e.name, &up))
        };
        if cache.revision() != self.revision && !self.waiting.is_empty() {
            self.revision = cache.revision();
            let arrived: Vec<u32> = self
                .waiting
                .iter()
                .copied()
                .filter(|&id| cache.textures[id as usize].ready)
                .collect();
            for id in &arrived {
                self.waiting.remove(id);
                self.textures[*id as usize] = upload(&cache.textures[*id as usize]);
            }
            if !arrived.is_empty() {
                // bind groups made meanwhile hold the white stand-in
                self.binds.clear();
            }
        }
        while self.textures.len() < cache.textures.len() {
            let id = self.textures.len();
            let e = &cache.textures[id];
            if e.ready {
                self.textures.push(upload(e));
            } else {
                self.textures.push(None);
                self.waiting.insert(id as u32);
            }
        }
    }

    fn pipeline(&mut self, gpu: &Gpu, blend: u32) -> &wgpu::RenderPipeline {
        let (format, module, layout) = (self.format, &self.module, &self.pipeline_layout);
        self.pipelines.entry(blend).or_insert_with(|| {
            let attrs = wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x2,
                3 => Unorm8x4, 4 => Unorm8x4, 5 => Unorm8x4, 6 => Uint32
            ];
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("ui"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attrs,
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: blend_state(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        })
    }

    fn bind(&mut self, gpu: &Gpu, key: BindKey) -> &wgpu::BindGroup {
        let sampler = |samplers: &mut HashMap<u32, wgpu::Sampler>, k: u32| {
            samplers
                .entry(k)
                .or_insert_with(|| {
                    let mode = |m: u32| match m {
                        1 => wgpu::AddressMode::Repeat,
                        2 => wgpu::AddressMode::MirrorRepeat,
                        _ => wgpu::AddressMode::ClampToEdge,
                    };
                    let filter = if k & 16 != 0 {
                        wgpu::FilterMode::Linear
                    } else {
                        wgpu::FilterMode::Nearest
                    };
                    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("ui"),
                        address_mode_u: mode(k & 3),
                        address_mode_v: mode((k >> 2) & 3),
                        mag_filter: filter,
                        min_filter: filter,
                        ..Default::default()
                    })
                })
                .clone()
        };
        if !self.binds.contains_key(&key) {
            let (s0, s1) = (
                sampler(&mut self.samplers, key.2),
                sampler(&mut self.samplers, key.3),
            );
            let view = |id: u32| {
                self.textures
                    .get(id as usize)
                    .and_then(|t| t.as_ref())
                    .unwrap_or(&self.white)
            };
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ui"),
                layout: &self.bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view(key.0)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&s0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(view(key.1)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&s1),
                    },
                ],
            });
            self.binds.insert(key, group);
        }
        &self.binds[&key]
    }

    fn has(&self, id: u32) -> bool {
        id != TEXTURE_NONE && self.textures.get(id as usize).is_some_and(Option::is_some)
    }

    /// Draw `quads` into `target` (loaded, not cleared) inside the viewport `(x, y, w, h)` in
    /// target pixels; `size` is the layout's size in layout units (origin centre, y up).
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: (f32, f32, f32, f32),
        quads: &[Quad],
        size: [f32; 2],
    ) {
        if quads.is_empty() || size[0] <= 0.0 || size[1] <= 0.0 {
            return;
        }
        let mut verts: Vec<Vertex> = Vec::with_capacity(quads.len() * 6);
        // (first vertex, count, blend, bind key)
        let mut batches: Vec<(u32, u32, u32, BindKey)> = Vec::new();
        let (sx, sy) = (2.0 / size[0], 2.0 / size[1]);
        for q in quads {
            if self.waiting.contains(&q.tex0) || self.waiting.contains(&q.tex1) {
                continue;
            }
            let (h0, h1) = (self.has(q.tex0), self.has(q.tex1));
            let shader_flags = (q.flags & 0x000F_F000) | h0 as u32 | (h1 as u32) << 1;
            let linear = if q.flags & mmk_ui::FLAG_LINEAR != 0 {
                16
            } else {
                0
            };
            let key = (
                if h0 { q.tex0 } else { TEXTURE_NONE },
                if h1 { q.tex1 } else { TEXTURE_NONE },
                (q.flags & 0xF) | linear,
                ((q.flags >> 4) & 0xF) | linear,
            );
            let blend = (q.flags >> 20) & 0xFFF;
            let v: [Vertex; 4] = std::array::from_fn(|i| Vertex {
                pos: [q.pos[i][0] * sx, q.pos[i][1] * sy],
                uv0: q.uv0[i],
                uv1: q.uv1[i],
                color: q.color[i],
                black: q.black,
                white: q.white,
                flags: shader_flags,
            });
            // corners TL, TR, BL, BR → two triangles
            verts.extend([v[0], v[2], v[1], v[1], v[2], v[3]]);
            let end = verts.len() as u32;
            match batches.last_mut() {
                Some(b) if b.2 == blend && b.3 == key && b.0 + b.1 == end - 6 => b.1 += 6,
                _ => batches.push((end - 6, 6, blend, key)),
            }
        }
        if verts.is_empty() {
            return;
        }
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ui quads"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        // pipelines and bind groups first (the pass borrows them)
        for b in &batches {
            self.pipeline(gpu, b.2);
            self.bind(gpu, b.3);
        }
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let (x, y, w, h) = viewport;
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (first, count, blend, key) in batches {
            pass.set_pipeline(&self.pipelines[&blend]);
            pass.set_bind_group(0, &self.binds[&key], &[]);
            pass.draw(first..first + count, 0..1);
        }
    }
}

fn format_rgba() -> wgpu::TextureFormat {
    wgpu::TextureFormat::Rgba8Unorm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_bits_map_to_wgpu() {
        assert_eq!(blend_state(0), None);
        let normal = blend_state(mmk_ui::BLEND_NORMAL >> 20).unwrap();
        assert_eq!(normal.color, wgpu::BlendState::ALPHA_BLENDING.color);
        // max with odd factors still validates (factors forced to One)
        let m = blend_state(5 | 4 << 4 | 5 << 8).unwrap();
        assert_eq!(m.color.src_factor, wgpu::BlendFactor::One);
        assert_eq!(size_of::<Vertex>(), 40);
    }
}
