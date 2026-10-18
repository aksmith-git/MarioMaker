import CMmk
import Metal
import simd

/// Draws the engine's mesh draws (NSMBU / 3D World models and background scenes, mmk.h "meshes")
/// with Metal: GPU skinning from `mmk_mesh_pose`'s matrices, the camera of the scene they belong
/// to (`mmk_engine_scene3d`: the play field's, orthographic or 3D World's slight perspective, or
/// the background scene's), the environment's lights and fogs, and each shape's material look
/// (`MmkMeshShape`: self-lit, fog, alpha test, blending, faces, texture SRT, vertex colours).
/// Mirrors the portable shell's `mesh.rs` / `render.rs`.
///
/// Depth: play-field draws come in sprite order (`MmkMeshDraw.order`); consecutive draws of one
/// layer form a depth group with real depth between them, and every later group sits entirely in
/// front of the earlier ones, so meshes and sprites still stack like painter's order between
/// layers. Sprites never write depth. The background scene keeps its own depth (own pass).
@MainActor
final class MeshRenderer {
    let device: MTLDevice
    private let assets: OpaquePointer
    /// Pipelines by blending (0 opaque, 1 alpha, 2 additive).
    private let pipelines: [MTLRenderPipelineState]
    private let depthWrite: MTLDepthStencilState
    private let depthTest: MTLDepthStencilState
    private var samplers: [UInt32: MTLSamplerState] = [:]
    private lazy var white: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        var px: [UInt8] = [200, 200, 200, 255]
        t?.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        return t
    }()
    private lazy var noColors: MTLBuffer? = device.makeBuffer(length: 16)
    /// MMK_MESH_DEBUG=1: log what each frame poses.
    static let debug = ProcessInfo.processInfo.environment["MMK_MESH_DEBUG"] == "1"
    /// Texture index (mmk_assets_texture) → GPU texture.
    var texture: (UInt32) -> MTLTexture? = { _ in nil }

    private struct GPUModel {
        let vertices: MTLBuffer
        let indices: MTLBuffer
        let colors: MTLBuffer?
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

    /// The camera and environment a set of draws is seen with.
    struct Camera {
        var viewProj: simd_float4x4
        /// Play field: depth squeezed into per-layer slices.
        var layered: Bool
        /// Underground night: drawn upside down.
        var flipY: Bool
        /// 30 rows (mmk.h MmkScene3d light uniforms).
        var light: [SIMD4<Float>]
    }

    /// Matches `U` in the shader.
    private struct Uniforms {
        var vp: simd_float4x4
        var m0: SIMD4<Float>, m1: SIMD4<Float>, m2: SIMD4<Float>
        var depth: SIMD4<Float>     // group centre, slice, layered, y flip
        var srt: SIMD4<Float>       // scale u, scale v, rotate, translate u
        var misc: SIMD4<Float>      // translate v, vertex-colour ratio, alpha ref, has colours
        var tint: SIMD4<Float>      // albedo tint rgb, emission strength
        var flags: UInt32           // MMK_MESH_* bits (bit 0 = card) as in MmkMeshShape.flags
        var pad0: UInt32 = 0, pad1: UInt32 = 0, pad2: UInt32 = 0
    }

    private static let shaderSource = """
    #include <metal_stdlib>
    using namespace metal;
    struct V { packed_float3 pos; packed_float3 nrm; float2 uv; ushort4 joints; packed_float4 weights; };
    struct U { float4x4 vp; float4 m0, m1, m2; float4 depth; float4 srt; float4 misc; float4 tint; uint flags; uint p0, p1, p2; };
    struct O { float4 pos [[position]]; float2 uv; float3 n; float3 world; float4 color; };
    vertex O mesh_vertex(const device V* v [[buffer(0)]], const device float4* skin [[buffer(1)]],
                         constant U& u [[buffer(2)]], const device uchar4* colors [[buffer(3)]],
                         uint vid [[vertex_id]]) {
        V x = v[vid];
        float4 P = float4(float3(x.pos), 1);
        float3 N = float3(x.nrm);
        float3 p = 0, n = 0;
        bool any = false;
        for (int k = 0; k < 4; k++) {
            float w = x.weights[k];
            if (w <= 0) continue;
            uint b = uint(x.joints[k]) * 3;
            float4 r0 = skin[b], r1 = skin[b + 1], r2 = skin[b + 2];
            p += w * float3(dot(r0, P), dot(r1, P), dot(r2, P));
            n += w * float3(dot(r0.xyz, N), dot(r1.xyz, N), dot(r2.xyz, N));
            any = true;
        }
        if (!any) { p = P.xyz; n = N; }
        float4 Q = float4(p, 1);
        float3 wp = float3(dot(u.m0, Q), dot(u.m1, Q), dot(u.m2, Q));
        float4 clip = u.vp * float4(wp, 1);
        if (u.depth.z > 0.5 && abs(clip.w) > 1e-6) {
            float zn = clamp(clip.z / clip.w, 0.0, 1.0);
            clip.z = (u.depth.x + (zn - 0.5) * u.depth.y * 0.98) * clip.w;
        }
        clip.y *= u.depth.w;
        O o;
        o.pos = clip;
        // world-mapped textures: (x, -y) / 64 (MMK_MESH_WORLD_UV)
        float2 uv = ((u.flags & (1u << 16)) != 0 ? float2(wp.x, -wp.y) / 64.0 : x.uv) * u.srt.xy;
        float s = sin(u.srt.z), c = cos(u.srt.z);
        o.uv = float2(c * uv.x - s * uv.y + u.srt.w, s * uv.x + c * uv.y + u.misc.x);
        o.n = float3(dot(u.m0.xyz, n), dot(u.m1.xyz, n), dot(u.m2.xyz, n));
        o.world = wp;
        float4 vc = u.misc.w > 0.5 ? float4(colors[vid]) / 255.0 : float4(1);
        o.color = float4(mix(float3(1), vc.rgb, u.misc.y) * u.tint.rgb, vc.a);
        return o;
    }
    float3 lighting(float3 n, constant float4* L) {
        float3 c = L[13].xyz;
        if (L[10].w > 0.5) {
            float k = dot(n, L[12].xyz) * 0.5 + 0.5;
            c += mix(L[11].xyz, L[10].xyz, k);
        }
        for (int i = 0; i < 3; i++) {
            float4 d = L[1 + 3 * i];
            if (d.w > 0.5) {
                float ndl = dot(n, -d.xyz);
                c += L[2 + 3 * i].xyz * max(ndl, 0.0) + L[3 + 3 * i].xyz * max(-ndl, 0.0);
            }
        }
        return c;
    }
    float3 fogged(float3 c, float3 world, uint flags, constant float4* L) {
        float3 r = c;
        float3 eye = L[0].xyz;
        // slots: the distance fog (MMK_MESH_FOG), the second distance fog, the height fog
        bool on[3] = { (flags & 4) != 0, (flags & (1u << 14)) != 0, (flags & (1u << 15)) != 0 };
        for (int i = 0; i < 3; i++) {
            float4 p = L[14 + 3 * i], col = L[15 + 3 * i], dir = L[16 + 3 * i];
            if (dir.w > 0.5 && on[i]) {
                float d = p.w > 0.5 ? -dot(world, dir.xyz) : dot(world - eye, dir.xyz);
                float span = p.y - p.x;
                float t = abs(span) > 1e-4 ? clamp((d - p.x) / span, 0.0, 1.0) : 0.0;
                t = pow(t, max(p.z, 0.01));
                r = mix(r, col.rgb, t * col.a);
            }
        }
        return r;
    }
    fragment float4 mesh_fragment(O in [[stage_in]], texture2d<float> t [[texture(0)]],
                                  texture2d<float> extra [[texture(1)]],
                                  sampler s [[sampler(0)]], constant U& u [[buffer(0)]],
                                  constant float4* L [[buffer(1)]]) {
        float4 c = t.sample(s, in.uv) * in.color;
        float4 x = extra.sample(s, in.uv);
        uint mode = (u.flags >> 12) & 3;
        if (mode == 2) c.a *= x.a;
        // sky: the base gradient under the albedo's cloud layer
        if (mode == 3) {
            if (L[24].w > 0.5) {
                float3 sky = mix(L[29].rgb, L[28].rgb, x.r);
                float3 cloud = mix(L[25].rgb, L[24].rgb, c.r);
                c = float4(mix(sky, cloud, c.a) * in.color.rgb, 1);
            } else {
                c = float4(mix(x.rgb, c.rgb, c.a) * in.color.rgb, 1);
            }
        }
        // cards (pre-rendered sprites) are cut out by alpha
        if ((u.flags & 1) != 0) {
            if (c.a < 0.1) discard_fragment();
            return c;
        }
        if ((u.flags & 8) != 0 && c.a < u.misc.z) discard_fragment();
        float3 rgb = c.rgb;
        if ((u.flags & 2) == 0) rgb *= lighting(normalize(in.n), L) * L[23].x;
        if (mode == 1) rgb += x.rgb * u.tint.w;
        rgb = fogged(rgb, in.world, u.flags, L);
        return float4(rgb, c.a);
    }
    """

    init(device: MTLDevice, assets: OpaquePointer, colorFormat: MTLPixelFormat, depthFormat: MTLPixelFormat) throws {
        self.device = device
        self.assets = assets
        let lib = try device.makeLibrary(source: Self.shaderSource, options: nil)
        func make(_ blend: Int) throws -> MTLRenderPipelineState {
            let desc = MTLRenderPipelineDescriptor()
            desc.vertexFunction = lib.makeFunction(name: "mesh_vertex")
            desc.fragmentFunction = lib.makeFunction(name: "mesh_fragment")
            desc.colorAttachments[0].pixelFormat = colorFormat
            desc.depthAttachmentPixelFormat = depthFormat
            let ca = desc.colorAttachments[0]!
            ca.isBlendingEnabled = true
            ca.sourceRGBBlendFactor = .sourceAlpha
            ca.destinationRGBBlendFactor = blend == 2 ? .one : .oneMinusSourceAlpha
            ca.sourceAlphaBlendFactor = .one
            ca.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            return try device.makeRenderPipelineState(descriptor: desc)
        }
        pipelines = try [make(0), make(1), make(2)]
        let ds = MTLDepthStencilDescriptor()
        ds.depthCompareFunction = .lessEqual
        ds.isDepthWriteEnabled = true
        depthWrite = device.makeDepthStencilState(descriptor: ds)!
        ds.isDepthWriteEnabled = false
        depthTest = device.makeDepthStencilState(descriptor: ds)!
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
        let colors = info.colors.flatMap { device.makeBuffer(bytes: $0, length: info.vertex_count * 4) }
        let shapes: [MmkMeshShape] = (0..<Int(info.shape_count)).map { s in
            var sh = MmkMeshShape()
            _ = mmk_mesh_shape(assets, mesh, Int(index), s, &sh)
            return sh
        }
        if Self.debug, let vp = info.vertices, let ip = info.indices {
            let v0 = vp[0]
            FileHandle.standardError.write("model \(mesh)/\(index): \(info.vertex_count) verts, \(info.index_count) indices (first \(ip[0]) \(ip[1]) \(ip[2])), v0 pos \(v0.position) w \(v0.weights) j \(v0.joints), shapes \(shapes.map { ($0.first_index, $0.index_count, $0.flags) })\n".data(using: .utf8)!)
        }
        let m = GPUModel(vertices: v, indices: i, colors: colors, bones: Int(info.bone_count), shapes: shapes)
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

    /// Encode posed draws `range` of `frame` seen with `camera`: opaque shapes first, then the
    /// blended ones.
    func encode(_ enc: MTLRenderCommandEncoder, frame: Frame, range: Range<Int>, camera: Camera) {
        guard let skin = frame.skin, !range.isEmpty else { return }
        enc.setFrontFacing(.counterClockwise)
        var light = camera.light
        if light.count < 30 { light += Array(repeating: SIMD4<Float>(0, 0, 0, 0), count: 30 - light.count) }
        enc.setFragmentBytes(&light, length: 30 * 16, index: 1)
        let slice = 1 / Float(frame.groups + 1)
        for blended in [false, true] {
            for p in frame.posed[range] {
                guard let m = model(p.draw.mesh, p.draw.model) else { continue }
                let t = p.draw.transform
                let center = 1 - Float(p.group + 1) * slice
                // a mirroring transform (or the upside-down course) turns the winding around
                let det = t.0 * (t.5 * t.10 - t.6 * t.9) - t.1 * (t.4 * t.10 - t.6 * t.8) + t.2 * (t.4 * t.9 - t.5 * t.8)
                let mirrored = (det < 0) != camera.flipY
                let card = p.draw.flags & UInt32(MMK_MESH_DRAW_CARD) != 0
                var u = Uniforms(vp: camera.viewProj,
                                 m0: SIMD4(t.0, t.1, t.2, t.3), m1: SIMD4(t.4, t.5, t.6, t.7), m2: SIMD4(t.8, t.9, t.10, t.11),
                                 depth: SIMD4(center, slice, camera.layered ? 1 : 0, camera.flipY ? -1 : 1),
                                 srt: .zero, misc: .zero, tint: .one, flags: 0)
                enc.setVertexBuffer(m.vertices, offset: 0, index: 0)
                enc.setVertexBuffer(skin, offset: p.skinOffset, index: 1)
                enc.setVertexBuffer(m.colors ?? noColors, offset: 0, index: 3)
                for (i, sh) in m.shapes.enumerated() where i < p.visible.count && p.visible[i] != 0 && sh.index_count > 0 {
                    let blend = Int((sh.flags >> UInt32(MMK_MESH_BLEND_SHIFT)) & 3)
                    let faces = (sh.flags >> UInt32(MMK_MESH_FACES_SHIFT)) & 3
                    if (blend != 0) != blended || faces == 3 { continue }
                    let unlit = card || sh.flags & UInt32(MMK_MESH_UNLIT) != 0
                    let extraTex = sh.extra_texture == MMK_TEXTURE_NONE ? nil : texture(sh.extra_texture)
                    let keep = UInt32(MMK_MESH_SELF_LIT | MMK_MESH_FOG | MMK_MESH_ALPHA_TEST) | UInt32(MMK_MESH_FOG1)
                        | UInt32(MMK_MESH_FOG_Y) | UInt32(MMK_MESH_WORLD_UV) | (extraTex != nil ? 3 << 12 : 0)
                    u.flags = (unlit ? 1 : 0) | (sh.flags & keep)
                    u.tint = SIMD4(sh.tint.0, sh.tint.1, sh.tint.2, sh.emission)
                    enc.setFragmentTexture(extraTex ?? white, index: 1)
                    u.srt = SIMD4(sh.srt.0, sh.srt.1, sh.srt.2, sh.srt.3)
                    u.misc = SIMD4(sh.srt.4, sh.vertex_color_ratio, sh.alpha_ref, m.colors == nil ? 0 : 1)
                    enc.setRenderPipelineState(pipelines[blend])
                    enc.setDepthStencilState(blend == 0 ? depthWrite : depthTest)
                    if unlit || faces == 2 {
                        enc.setCullMode(.none)
                    } else {
                        enc.setCullMode((faces == 0) != mirrored ? .back : .front)
                    }
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
        enc.setCullMode(.none)
    }

    /// A plain key light plus ambient (model viewer; scenes without an environment).
    static let neutralLight: [SIMD4<Float>] = {
        var l = Array(repeating: SIMD4<Float>(0, 0, 0, 0), count: 30)
        l[0] = SIMD4(0, 0, 500, 1)
        l[1] = SIMD4(simd_normalize(SIMD3<Float>(0.35, -0.6, -0.75)), 1)
        l[2] = SIMD4(0.6, 0.6, 0.6, 0)
        l[13] = SIMD4(0.5, 0.5, 0.5, 0)
        l[23] = SIMD4(1, 0, 0, 0)
        return l
    }()

    /// The light rows of an `MmkScene3d` uniform block (96 floats).
    static func lightRows<T>(_ tuple: T) -> [SIMD4<Float>] {
        withUnsafeBytes(of: tuple) { raw in
            let f = raw.bindMemory(to: Float.self)
            return (0..<30).map { SIMD4(f[$0 * 4], f[$0 * 4 + 1], f[$0 * 4 + 2], f[$0 * 4 + 3]) }
        }
    }

    /// A column-major 16-float tuple as a matrix.
    static func matrix<T>(_ tuple: T) -> simd_float4x4 {
        withUnsafeBytes(of: tuple) { raw in
            let f = raw.bindMemory(to: Float.self)
            return simd_float4x4(columns: (SIMD4(f[0], f[1], f[2], f[3]), SIMD4(f[4], f[5], f[6], f[7]),
                                           SIMD4(f[8], f[9], f[10], f[11]), SIMD4(f[12], f[13], f[14], f[15])))
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
