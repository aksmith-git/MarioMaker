import CMmk
import Metal

/// Draws layout-player output (`mmk_ui_draw_list`, `mmk_front_frame`) with Metal.
///
/// Owns the GPU copies of the UI textures (uploaded once each, as `mmk_ui_texture` lists them),
/// one pipeline per blend mode and one sampler per wrap/filter combination. Shading follows
/// mmk.h: t = combine(tex0, tex1), c = mix(black, white, t) · vertex colour. Material and vertex
/// colours are authored sRGB values and are linearised in the shader; the target is sRGB.
@MainActor
final class UILayer {
    let device: MTLDevice
    /// The import's texture list so far (it only grows) and one entry of it.
    private let textureCount: () -> Int
    private let textureInfo: (Int, inout MmkTextureInfo) -> Bool
    private let library: MTLLibrary
    private let pixelFormat: MTLPixelFormat
    private var textures: [MTLTexture?] = []
    private var pipelines: [UInt32: MTLRenderPipelineState] = [:]
    private var samplers: [UInt32: MTLSamplerState] = [:]
    private lazy var white: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm, width: 1, height: 1, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        var px: [UInt8] = [255, 255, 255, 255]
        t?.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        return t
    }()

    /// Matches the vertex struct in the shader (40 bytes).
    private struct Vertex {
        var pos: SIMD2<Float>
        var uv0: SIMD2<Float>
        var uv1: SIMD2<Float>
        var color: UInt32
        var black: UInt32
        var white: UInt32
        var flags: UInt32
    }

    private static let shaderSource = """
    #include <metal_stdlib>
    using namespace metal;
    struct V { float2 pos; float2 uv0; float2 uv1; uchar4 color; uchar4 black; uchar4 white; uint flags; };
    struct O { float4 pos [[position]]; float2 uv0; float2 uv1; float4 color; float4 black; float4 white; uint flags [[flat]]; };
    static float3 lin(float3 c) { return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045); }
    static float4 lin4(uchar4 c) { float4 f = float4(c) / 255.0; return float4(lin(f.rgb), f.a); }
    vertex O ui_vertex(const device V* v [[buffer(0)]], constant float4& xf [[buffer(1)]], uint i [[vertex_id]]) {
        O o;
        o.pos = float4(v[i].pos * xf.xy + xf.zw, 0, 1);
        o.uv0 = v[i].uv0; o.uv1 = v[i].uv1;
        o.color = lin4(v[i].color); o.black = lin4(v[i].black); o.white = lin4(v[i].white);
        o.flags = v[i].flags;
        return o;
    }
    static float comb(float a, float b, uint m) {
        switch (m) {
            case 0: return b;
            case 1: return a * b;
            case 2: return min(a + b, 1.0);
            case 5: return max(a - b, 0.0);
            default: return a;
        }
    }
    fragment float4 ui_fragment(O in [[stage_in]], texture2d<float> t0 [[texture(0)]], texture2d<float> t1 [[texture(1)]],
                                sampler s0 [[sampler(0)]], sampler s1 [[sampler(1)]]) {
        float4 a = (in.flags & 1u) ? t0.sample(s0, in.uv0) : float4(1);
        float4 t = a;
        if (in.flags & 2u) {
            float4 b = t1.sample(s1, in.uv1);
            uint cm = (in.flags >> 12) & 15u, am = (in.flags >> 16) & 15u;
            t = float4(comb(a.r, b.r, cm), comb(a.g, b.g, cm), comb(a.b, b.b, cm), comb(a.a, b.a, am));
        }
        return mix(in.black, in.white, t) * in.color;
    }
    """

    /// Textures from a texture list (`mmk_front_texture_count` / `mmk_front_texture`).
    init(device: MTLDevice, pixelFormat: MTLPixelFormat, textureCount: @escaping () -> Int,
         textureInfo: @escaping (Int, inout MmkTextureInfo) -> Bool) throws {
        self.device = device
        self.textureCount = textureCount
        self.textureInfo = textureInfo
        self.pixelFormat = pixelFormat
        library = try device.makeLibrary(source: Self.shaderSource, options: nil)
    }

    /// Textures of an `mmk_ui_open` handle.
    convenience init(device: MTLDevice, ui: OpaquePointer, pixelFormat: MTLPixelFormat) throws {
        try self.init(device: device, pixelFormat: pixelFormat, textureCount: { mmk_ui_texture_count(ui) },
                      textureInfo: { i, info in mmk_ui_texture(ui, i, &info) })
    }

    /// Upload textures added since the last call (call after opening a layout).
    func syncTextures() {
        let count = textureCount()
        while textures.count < count {
            var info = MmkTextureInfo()
            textures.append(textureInfo(textures.count, &info) ? Self.makeTexture(device, info) : nil)
        }
    }

    static func makeTexture(_ device: MTLDevice, _ t: MmkTextureInfo) -> MTLTexture? { GPUTexture.make(device, t) }

    private func pipeline(blend: UInt32) -> MTLRenderPipelineState? {
        if let p = pipelines[blend] { return p }
        let desc = MTLRenderPipelineDescriptor()
        desc.vertexFunction = library.makeFunction(name: "ui_vertex")
        desc.fragmentFunction = library.makeFunction(name: "ui_fragment")
        desc.colorAttachments[0].pixelFormat = pixelFormat
        let op = blend & 15, src = (blend >> 4) & 15, dst = (blend >> 8) & 15
        let ca = desc.colorAttachments[0]!
        if op != 0 {
            func factor(_ f: UInt32) -> MTLBlendFactor {
                switch f {
                case 0: .zero
                case 1: .one
                case 2: .destinationColor
                case 3: .oneMinusDestinationColor
                case 4: .sourceAlpha
                case 5: .oneMinusSourceAlpha
                case 6: .destinationAlpha
                case 7: .oneMinusDestinationAlpha
                case 8: .sourceColor
                default: .oneMinusSourceColor
                }
            }
            let mop: MTLBlendOperation = switch op {
            case 2: .subtract
            case 3: .reverseSubtract
            case 4: .min
            case 5: .max
            default: .add
            }
            ca.isBlendingEnabled = true
            ca.rgbBlendOperation = mop
            ca.alphaBlendOperation = mop
            ca.sourceRGBBlendFactor = factor(src)
            ca.destinationRGBBlendFactor = factor(dst)
            ca.sourceAlphaBlendFactor = factor(src)
            ca.destinationAlphaBlendFactor = factor(dst)
        }
        let p = try? device.makeRenderPipelineState(descriptor: desc)
        pipelines[blend] = p
        return p
    }

    /// Sampler for wrap s/t (2 bits each) + linear flag.
    private func sampler(_ key: UInt32) -> MTLSamplerState? {
        if let s = samplers[key] { return s }
        func mode(_ m: UInt32) -> MTLSamplerAddressMode {
            switch m { case 1: .repeat; case 2: .mirrorRepeat; default: .clampToEdge }
        }
        let d = MTLSamplerDescriptor()
        d.sAddressMode = mode(key & 3)
        d.tAddressMode = mode((key >> 2) & 3)
        let linear = key & 16 != 0
        d.minFilter = linear ? .linear : .nearest
        d.magFilter = linear ? .linear : .nearest
        let s = device.makeSamplerState(descriptor: d)
        samplers[key] = s
        return s
    }

    private func texture(_ id: UInt32) -> MTLTexture? {
        id != MMK_TEXTURE_NONE && Int(id) < textures.count ? textures[Int(id)] : nil
    }

    /// Encode `quads` into `enc`. `layoutSize` maps layout units to the current viewport
    /// (origin centre, y up).
    func encode(_ enc: MTLRenderCommandEncoder, quads: UnsafeBufferPointer<MmkUiQuad>, layoutSize: SIMD2<Float>) {
        guard !quads.isEmpty else { return }
        syncTextures()
        var verts: [Vertex] = []
        verts.reserveCapacity(quads.count * 6)
        struct Batch { var start: Int; var count: Int; var key: (UInt32, UInt32, UInt32, UInt32, UInt32) }
        var batches: [Batch] = []
        for q in quads {
            let t0 = texture(q.tex0), t1 = texture(q.tex1)
            let has: UInt32 = (t0 != nil ? 1 : 0) | (t1 != nil ? 2 : 0)
            let shaderFlags = (q.flags & 0x000F_F000) | has
            let s0 = (q.flags & 0xF) | (q.flags & 0x100 != 0 ? 16 : 0)
            let s1 = ((q.flags >> 4) & 0xF) | (q.flags & 0x100 != 0 ? 16 : 0)
            let blend = (q.flags >> 20) & 0xFFF
            let key = (t0 != nil ? q.tex0 : MMK_TEXTURE_NONE, t1 != nil ? q.tex1 : MMK_TEXTURE_NONE, s0, s1, blend)
            withUnsafeBytes(of: q) { raw in
                func f2(_ off: Int, _ i: Int) -> SIMD2<Float> {
                    SIMD2(raw.load(fromByteOffset: off + i * 8, as: Float.self), raw.load(fromByteOffset: off + i * 8 + 4, as: Float.self))
                }
                func c(_ off: Int) -> UInt32 { raw.load(fromByteOffset: off, as: UInt32.self) }
                let black = c(112), white = c(116)
                let v = (0..<4).map { i in
                    Vertex(pos: f2(0, i), uv0: f2(32, i), uv1: f2(64, i), color: c(96 + i * 4), black: black, white: white, flags: shaderFlags)
                }
                // corners TL, TR, BL, BR → two triangles
                verts += [v[0], v[2], v[1], v[1], v[2], v[3]]
            }
            if let last = batches.last, last.key == key, last.start + last.count == verts.count - 6 {
                batches[batches.count - 1].count += 6
            } else {
                batches.append(Batch(start: verts.count - 6, count: 6, key: key))
            }
        }
        let length = verts.count * MemoryLayout<Vertex>.stride
        guard let buffer = verts.withUnsafeBytes({ device.makeBuffer(bytes: $0.baseAddress!, length: length) }) else { return }
        var xf = SIMD4<Float>(2 / layoutSize.x, 2 / layoutSize.y, 0, 0)
        enc.setVertexBuffer(buffer, offset: 0, index: 0)
        enc.setVertexBytes(&xf, length: 16, index: 1)
        for b in batches {
            guard let p = pipeline(blend: b.key.4) else { continue }
            enc.setRenderPipelineState(p)
            enc.setFragmentTexture(texture(b.key.0) ?? white, index: 0)
            enc.setFragmentTexture(texture(b.key.1) ?? white, index: 1)
            enc.setFragmentSamplerState(sampler(b.key.2), index: 0)
            enc.setFragmentSamplerState(sampler(b.key.3), index: 1)
            enc.drawPrimitives(type: .triangle, vertexStart: b.start, vertexCount: b.count)
        }
    }
}
