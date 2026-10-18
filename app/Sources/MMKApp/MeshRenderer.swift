import CMmk
import Metal

/// Draws the engine's mesh draws (NSMBU / 3D World models, mmk.h "meshes") with Metal: GPU
/// skinning from `mmk_mesh_pose`'s matrices, an orthographic side view sharing the sprites'
/// world → NDC transform, one albedo texture per shape, simple directional lighting (none for
/// pre-rendered cards).
///
/// Depth: draws come in sprite order (`MmkMeshDraw.order`); consecutive draws of one layer form a
/// depth group with real depth between them, and every later group sits entirely in front of the
/// earlier ones, so meshes and sprites still stack like painter's order between layers. Sprites
/// never write depth.
@MainActor
final class MeshRenderer {
    let device: MTLDevice
    private let assets: OpaquePointer
    private let pipeline: MTLRenderPipelineState
    private let depthState: MTLDepthStencilState
    private var samplers: [UInt32: MTLSamplerState] = [:]
    private lazy var white: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        var px: [UInt8] = [200, 200, 200, 255]
        t?.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        return t
    }()
    /// MMK_MESH_DEBUG=1: log what each frame poses.
    static let debug = ProcessInfo.processInfo.environment["MMK_MESH_DEBUG"] == "1"
    /// Texture index (mmk_assets_texture) → GPU texture.
    var texture: (UInt32) -> MTLTexture? = { _ in nil }

    private struct GPUModel {
        let vertices: MTLBuffer
        let indices: MTLBuffer
        let bones: Int
        let shapes: [MmkMeshShape]
    }
    private var models: [Int64: GPUModel] = [:]
    private var missing = Set<Int64>()

    /// One posed draw of this frame.
    struct Posed {
        var draw: MmkMeshDraw
        var skinOffset: Int        // bytes into the frame's skin buffer
        var textures: [UInt32]
        var visible: [UInt8]
        var group: Int
    }

    /// Everything one frame's draws need: poses taken, skin matrices in one buffer.
    struct Frame {
        var posed: [Posed] = []
        var skin: MTLBuffer?
        var groups = 0
    }

    /// Matches `U` in the shader.
    private struct Uniforms {
        var m0: SIMD4<Float>, m1: SIMD4<Float>, m2: SIMD4<Float>
        var view: SIMD4<Float>
        var depth: SIMD2<Float>
        var flags: UInt32
        var pad: UInt32 = 0
    }

    private static let shaderSource = """
    #include <metal_stdlib>
    using namespace metal;
    struct V { packed_float3 pos; packed_float3 nrm; float2 uv; ushort4 joints; packed_float4 weights; };
    struct U { float4 m0, m1, m2; float4 view; float2 depth; uint flags; uint pad; };
    struct O { float4 pos [[position]]; float2 uv; float3 n; };
    vertex O mesh_vertex(const device V* v [[buffer(0)]], const device float4* skin [[buffer(1)]],
                         constant U& u [[buffer(2)]], uint vid [[vertex_id]]) {
        V x = v[vid];
        float4 P = float4(float3(x.pos), 1);
        float3 N = float3(x.nrm);
        float3 p = 0, n = 0;
        for (int k = 0; k < 4; k++) {
            float w = x.weights[k];
            if (w <= 0) continue;
            uint b = uint(x.joints[k]) * 3;
            float4 r0 = skin[b], r1 = skin[b + 1], r2 = skin[b + 2];
            p += w * float3(dot(r0, P), dot(r1, P), dot(r2, P));
            n += w * float3(dot(r0.xyz, N), dot(r1.xyz, N), dot(r2.xyz, N));
        }
        float4 Q = float4(p, 1);
        float3 wp = float3(dot(u.m0, Q), dot(u.m1, Q), dot(u.m2, Q));
        O o;
        o.pos = float4(wp.xy * u.view.xy + u.view.zw, u.depth.x - wp.z * u.depth.y, 1);
        o.uv = x.uv;
        o.n = float3(dot(u.m0.xyz, n), dot(u.m1.xyz, n), dot(u.m2.xyz, n));
        return o;
    }
    fragment float4 mesh_fragment(O in [[stage_in]], texture2d<float> t [[texture(0)]],
                                  sampler s [[sampler(0)]], constant U& u [[buffer(0)]]) {
        float4 c = t.sample(s, in.uv);
        // cards (pre-rendered sprites) are cut out by alpha; the 3D materials' albedo alpha is
        // not opacity in general (render state not read): opaque [inf]
        if ((u.flags & 1) != 0 && c.a < 0.1) discard_fragment();
        if ((u.flags & 1) == 0) {
            c.a = 1;
            float3 n = normalize(in.n);
            if (n.z < 0) n = -n;      // two-sided
            float3 L = normalize(float3(-0.35, 0.6, 0.75));
            c.rgb *= 0.5 + 0.6 * max(dot(n, L), 0.0);
        }
        return c;
    }
    """

    init(device: MTLDevice, assets: OpaquePointer, colorFormat: MTLPixelFormat, depthFormat: MTLPixelFormat) throws {
        self.device = device
        self.assets = assets
        let lib = try device.makeLibrary(source: Self.shaderSource, options: nil)
        let desc = MTLRenderPipelineDescriptor()
        desc.vertexFunction = lib.makeFunction(name: "mesh_vertex")
        desc.fragmentFunction = lib.makeFunction(name: "mesh_fragment")
        desc.colorAttachments[0].pixelFormat = colorFormat
        desc.depthAttachmentPixelFormat = depthFormat
        let ca = desc.colorAttachments[0]!
        ca.isBlendingEnabled = true
        ca.sourceRGBBlendFactor = .sourceAlpha
        ca.destinationRGBBlendFactor = .oneMinusSourceAlpha
        ca.sourceAlphaBlendFactor = .one
        ca.destinationAlphaBlendFactor = .oneMinusSourceAlpha
        pipeline = try device.makeRenderPipelineState(descriptor: desc)
        let ds = MTLDepthStencilDescriptor()
        ds.depthCompareFunction = .less
        ds.isDepthWriteEnabled = true
        depthState = device.makeDepthStencilState(descriptor: ds)!
    }

    /// Depth state for sprites drawn in the same pass: no test, no write.
    static func spriteDepthState(_ device: MTLDevice) -> MTLDepthStencilState? {
        let ds = MTLDepthStencilDescriptor()
        ds.depthCompareFunction = .always
        ds.isDepthWriteEnabled = false
        return device.makeDepthStencilState(descriptor: ds)
    }

    private func model(_ mesh: Int32, _ index: UInt32) -> GPUModel? {
        let key = Int64(mesh) << 32 | Int64(index)
        if let m = models[key] { return m }
        if missing.contains(key) { return nil }
        var info = MmkMeshModel()
        guard mmk_mesh_model(assets, mesh, Int(index), &info), info.vertex_count > 0, info.index_count > 0,
              let v = device.makeBuffer(bytes: info.vertices, length: info.vertex_count * MemoryLayout<MmkMeshVertex>.stride),
              let i = device.makeBuffer(bytes: info.indices, length: info.index_count * 4)
        else {
            missing.insert(key)
            return nil
        }
        let shapes: [MmkMeshShape] = (0..<Int(info.shape_count)).map { s in
            var sh = MmkMeshShape()
            _ = mmk_mesh_shape(assets, mesh, Int(index), s, &sh)
            return sh
        }
        if Self.debug, let vp = info.vertices, let ip = info.indices {
            let v0 = vp[0]
            FileHandle.standardError.write("model \(mesh)/\(index): \(info.vertex_count) verts, \(info.index_count) indices (first \(ip[0]) \(ip[1]) \(ip[2])), v0 pos \(v0.position) w \(v0.weights) j \(v0.joints), shapes \(shapes.map { ($0.first_index, $0.index_count, $0.flags) })\n".data(using: .utf8)!)
        }
        let m = GPUModel(vertices: v, indices: i, bones: Int(info.bone_count), shapes: shapes)
        models[key] = m
        return m
    }

    /// Pose every draw and gather the skin matrices (call once per frame, before encoding).
    func prepare(_ draws: UnsafeBufferPointer<MmkMeshDraw>) -> Frame {
        var frame = Frame()
        var floats: [Float] = []
        var lastLayer: UInt32?
        for d in draws {
            guard let m = model(d.mesh, d.model) else { continue }
            var pose = MmkMeshPose()
            guard mmk_mesh_pose(assets, d.mesh, Int(d.model), d.anim, d.time, d.variant, &pose),
                  Int(pose.bone_count) == m.bones, let skin = pose.skin
            else { continue }
            if lastLayer != d.layer {
                frame.groups += 1
                lastLayer = d.layer
            }
            let offset = floats.count * 4
            floats.append(contentsOf: UnsafeBufferPointer(start: skin, count: m.bones * 12))
            let n = Int(pose.shape_count)
            frame.posed.append(Posed(
                draw: d, skinOffset: offset,
                textures: Array(UnsafeBufferPointer(start: pose.shape_texture, count: n)),
                visible: Array(UnsafeBufferPointer(start: pose.shape_visible, count: n)),
                group: frame.groups - 1))
        }
        if !floats.isEmpty {
            frame.skin = device.makeBuffer(bytes: floats, length: floats.count * 4)
        }
        if Self.debug {
            FileHandle.standardError.write("meshes: \(draws.count) draws, \(frame.posed.count) posed, \(frame.groups) groups, skin[0..12] \(floats.prefix(12)), textures \(frame.posed.first?.textures ?? []), visible \(frame.posed.first?.visible ?? [])\n".data(using: .utf8)!)
        }
        return frame
    }

    private func sampler(_ flags: UInt32) -> MTLSamplerState? {
        let key = (flags >> 8) & 0xF
        if let s = samplers[key] { return s }
        func mode(_ w: UInt32) -> MTLSamplerAddressMode {
            switch w { case 1: .mirrorRepeat; case 2: .clampToEdge; default: .repeat }
        }
        let d = MTLSamplerDescriptor()
        d.minFilter = .linear
        d.magFilter = .linear
        d.sAddressMode = mode(key & 3)
        d.tAddressMode = mode(key >> 2)
        let s = device.makeSamplerState(descriptor: d)
        samplers[key] = s
        return s
    }

    /// Encode posed draws `range` of `frame`. `view` maps world px to NDC (x' = x·view.x +
    /// view.z, y' = y·view.y + view.w), as for the sprites.
    func encode(_ enc: MTLRenderCommandEncoder, frame: Frame, range: Range<Int>, view: SIMD4<Float>) {
        guard let skin = frame.skin, !range.isEmpty else { return }
        enc.setRenderPipelineState(pipeline)
        enc.setDepthStencilState(depthState)
        enc.setCullMode(.none)
        let slice = 1 / Float(frame.groups + 1)
        for p in frame.posed[range] {
            guard let m = model(p.draw.mesh, p.draw.model) else { continue }
            let t = p.draw.transform
            let center = 1 - Float(p.group + 1) * slice
            var u = Uniforms(m0: SIMD4(t.0, t.1, t.2, t.3), m1: SIMD4(t.4, t.5, t.6, t.7), m2: SIMD4(t.8, t.9, t.10, t.11),
                             view: view, depth: SIMD2(center, slice / 400), flags: 0)
            enc.setVertexBuffer(m.vertices, offset: 0, index: 0)
            enc.setVertexBuffer(skin, offset: p.skinOffset, index: 1)
            for (i, sh) in m.shapes.enumerated() where i < p.visible.count && p.visible[i] != 0 && sh.index_count > 0 {
                u.flags = sh.flags & UInt32(MMK_MESH_UNLIT)
                enc.setVertexBytes(&u, length: MemoryLayout<Uniforms>.stride, index: 2)
                enc.setFragmentBytes(&u, length: MemoryLayout<Uniforms>.stride, index: 0)
                let tex = p.textures[i] == MMK_TEXTURE_NONE ? nil : texture(p.textures[i])
                enc.setFragmentTexture(tex ?? white, index: 0)
                enc.setFragmentSamplerState(sampler(sh.flags), index: 0)
                enc.drawIndexedPrimitives(type: .triangle, indexCount: Int(sh.index_count), indexType: .uint32,
                                          indexBuffer: m.indices, indexBufferOffset: Int(sh.first_index) * 4)
            }
        }
    }
}

/// GPU copy of an asset-cache or UI texture (mtex formats are Metal pixel formats). GPUs without
/// BC support (iPhones, iPads before M1) get BCn decoded to RGBA8 by the core
/// (`mmk_texture_decode_rgba`); MMK_CPU_TEXTURES=1 forces that path for testing.
enum GPUTexture {
    static let forceCPU = ProcessInfo.processInfo.environment["MMK_CPU_TEXTURES"] == "1"

    static func make(_ device: MTLDevice, _ t: MmkTextureInfo) -> MTLTexture? {
        guard t.width > 0, t.height > 0 else { return nil }
        if t.format >= 2 && (forceCPU || !device.supportsBCTextureCompression) { return decoded(device, t) }
        let srgb = t.variant == 2
        let (format, blockBytes, blockSize): (MTLPixelFormat, Int, Int) = switch t.format {
        case 1: (srgb ? .rgba8Unorm_srgb : .rgba8Unorm, 4, 1)
        case 2: (srgb ? .bc1_rgba_srgb : .bc1_rgba, 8, 4)
        case 3: (srgb ? .bc2_rgba_srgb : .bc2_rgba, 16, 4)
        case 4: (srgb ? .bc3_rgba_srgb : .bc3_rgba, 16, 4)
        case 5: (t.variant == 1 ? .bc4_rSnorm : .bc4_rUnorm, 8, 4)
        case 6: (t.variant == 1 ? .bc5_rgSnorm : .bc5_rgUnorm, 16, 4)
        case 7: (.bc6H_rgbuFloat, 16, 4)
        case 8: (srgb ? .bc7_rgbaUnorm_srgb : .bc7_rgbaUnorm, 16, 4)
        default: (.invalid, 0, 0)
        }
        guard format != .invalid, let data = t.data else { return nil }
        let w = Int(t.width), h = Int(t.height)
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: format, width: w, height: h, mipmapped: false)
        desc.usage = .shaderRead
        desc.swizzle = swizzle(t.channel_select)
        guard let tex = device.makeTexture(descriptor: desc) else { return nil }
        let bytesPerRow = (w + blockSize - 1) / blockSize * blockBytes
        tex.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: data, bytesPerRow: bytesPerRow)
        return tex
    }

    /// BCn decoded on the CPU into an RGBA8 texture (BC4/BC5 decode to linear values).
    private static func decoded(_ device: MTLDevice, _ t: MmkTextureInfo) -> MTLTexture? {
        let w = Int(t.width), h = Int(t.height)
        var px = [UInt8](repeating: 0, count: w * h * 4)
        var info = t
        guard mmk_texture_decode_rgba(&info, &px, px.count) else { return nil }
        let srgb = t.variant == 2 && t.format != 5 && t.format != 6
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: srgb ? .rgba8Unorm_srgb : .rgba8Unorm,
                                                            width: w, height: h, mipmapped: false)
        desc.usage = .shaderRead
        desc.swizzle = swizzle(t.channel_select)
        guard let tex = device.makeTexture(descriptor: desc) else { return nil }
        tex.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: px, bytesPerRow: w * 4)
        return tex
    }

    /// Channel select (0 zero, 1 one, 2 R, 3 G, 4 B, 5 A) → Metal swizzle.
    static func swizzle(_ sel: (UInt8, UInt8, UInt8, UInt8)) -> MTLTextureSwizzleChannels {
        func swz(_ c: UInt8) -> MTLTextureSwizzle {
            switch c { case 0: .zero; case 1: .one; case 2: .red; case 3: .green; case 4: .blue; default: .alpha }
        }
        return MTLTextureSwizzleChannels(red: swz(sel.0), green: swz(sel.1), blue: swz(sel.2), alpha: swz(sel.3))
    }
}
