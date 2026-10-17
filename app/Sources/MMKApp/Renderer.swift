import CMmk
import ImageIO
import Metal
import CoreText
import MetalKit
import QuartzCore
import SwiftUI

/// Hosts an MTKView inside SwiftUI, drawing one game session.
struct MetalView: NSViewRepresentable {
    let session: GameSession

    func makeCoordinator() -> Renderer { Renderer(session: session) }

    func makeNSView(context: Context) -> MTKView {
        let view = GameMTKView()
        view.device = context.coordinator.device
        view.colorPixelFormat = .bgra8Unorm_srgb
        view.preferredFramesPerSecond = 120
        view.delegate = context.coordinator
        view.framebufferOnly = context.coordinator.screenshot == nil && !DebugCapture.requested
        view.onWindow = { [weak session] w in session?.input.window = w }
        if DebugCapture.requested { DebugCapture.gameView = view }
        return view
    }

    func updateNSView(_ nsView: MTKView, context: Context) {}
}

/// Tells the session's input which window is the game's.
final class GameMTKView: MTKView {
    var onWindow: ((NSWindow?) -> Void)?
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        onWindow?(window)
        window?.makeFirstResponder(self)
    }
    override var acceptsFirstResponder: Bool { true }
}

/// Draws a game session's sprite snapshot (the session steps the engine, see GameSession.frame).
@MainActor
final class Renderer: NSObject, MTKViewDelegate {
    let device = MTLCreateSystemDefaultDevice()!
    private lazy var queue = device.makeCommandQueue()!
    private var pipeline: MTLRenderPipelineState?
    private var sampler: MTLSamplerState?
    private var textures: [MTLTexture?] = []
    private lazy var skyTexture: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        var px: [UInt8] = [0x5C, 0x94, 0xFC, 0xFF]
        t?.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        return t
    }()
    private let session: GameSession
    private var engine: OpaquePointer { session.engine }
    /// Sprites without art (MMK_TEXTURE_NONE) are drawn as coloured boxes in placeholder mode
    /// (or with MMK_DEBUG_MISSING_ART=1); otherwise skipped as before.
    private let drawMissing: Bool
    private var fallbackTextures: [UInt32: MTLTexture] = [:]
    // placeholder HUD (text rendered with CoreText until the game's own HUD layout is imported)
    private var hudText = ""
    private var hudTexture: MTLTexture?
    private var hudSize = (0, 0)

    /// Debug/smoke-test mode: MMK_SCREENSHOT=<png path> [MMK_SCREENSHOT_TICKS=<n>] runs a scripted
    /// input sequence (run right, jump), saves the frame after n ticks and quits.
    let screenshot = ProcessInfo.processInfo.environment["MMK_SCREENSHOT"]
    private let screenshotTicks = UInt64(ProcessInfo.processInfo.environment["MMK_SCREENSHOT_TICKS"] ?? "") ?? 150

    init(session: GameSession) {
        self.session = session
        drawMissing = session.assets.isPlaceholder || ProcessInfo.processInfo.environment["MMK_DEBUG_MISSING_ART"] == "1"
        super.init()
        do {
            try buildPipeline()
        } catch {
            fatalError("Metal pipeline: \(error)")
        }
        if session.assets.gpuTextures == nil {
            let a = session.assets.handle
            session.assets.gpuTextures = (0..<mmk_assets_texture_count(a)).map { i in
                var info = MmkTextureInfo()
                guard mmk_assets_texture(a, i, &info) else { return nil }
                return makeTexture(info)
            }
        }
        textures = session.assets.gpuTextures ?? []
        if screenshot != nil { session.scriptedInput = Self.scriptedInput }
    }

    // MARK: setup

    private func makeTexture(_ t: MmkTextureInfo) -> MTLTexture? {
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
        let sel = t.channel_select
        func swz(_ c: UInt8) -> MTLTextureSwizzle {
            switch c { case 0: .zero; case 1: .one; case 2: .red; case 3: .green; case 4: .blue; default: .alpha }
        }
        desc.swizzle = MTLTextureSwizzleChannels(red: swz(sel.0), green: swz(sel.1), blue: swz(sel.2), alpha: swz(sel.3))
        guard let tex = device.makeTexture(descriptor: desc) else { return nil }
        let bytesPerRow = (w + blockSize - 1) / blockSize * blockBytes
        tex.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: data, bytesPerRow: bytesPerRow)
        return tex
    }

    private static let shaderSource = """
    #include <metal_stdlib>
    using namespace metal;
    struct V { float2 pos; float2 uv; };
    struct O { float4 pos [[position]]; float2 uv; };
    vertex O vmain(const device V* v [[buffer(0)]], constant float4& xform [[buffer(1)]], uint i [[vertex_id]]) {
        O o; o.pos = float4(v[i].pos * xform.xy + xform.zw, 0, 1); o.uv = v[i].uv; return o;
    }
    fragment float4 fmain(O in [[stage_in]], texture2d<float> t [[texture(0)]], sampler s [[sampler(0)]]) {
        float4 c = t.sample(s, in.uv);
        if (c.a < 0.01) discard_fragment();
        return c;
    }
    """

    private func buildPipeline() throws {
        let lib = try device.makeLibrary(source: Self.shaderSource, options: nil)
        let desc = MTLRenderPipelineDescriptor()
        desc.vertexFunction = lib.makeFunction(name: "vmain")
        desc.fragmentFunction = lib.makeFunction(name: "fmain")
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        let ca = desc.colorAttachments[0]!
        ca.isBlendingEnabled = true
        ca.sourceRGBBlendFactor = .sourceAlpha
        ca.destinationRGBBlendFactor = .oneMinusSourceAlpha
        ca.sourceAlphaBlendFactor = .one
        ca.destinationAlphaBlendFactor = .oneMinusSourceAlpha
        pipeline = try device.makeRenderPipelineState(descriptor: desc)
        let s = MTLSamplerDescriptor()
        s.minFilter = .nearest
        s.magFilter = .nearest
        s.sAddressMode = .clampToEdge
        s.tAddressMode = .clampToEdge
        sampler = device.makeSamplerState(descriptor: s)
    }

    // MARK: frame

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        guard let pipeline, let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable else { return }

        // fixed 60 Hz simulation (or the pause menu), independent of the display rate
        session.frame(now: CACurrentMediaTime())

        var snap = MmkSnapshot()
        mmk_engine_snapshot(engine, &snap)

        // black bars outside the game view; the sky is drawn inside it below
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        pass.colorAttachments[0].loadAction = .clear
        guard let cmd = queue.makeCommandBuffer(), let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }

        // letterbox the 16:9 view into the drawable, at an integer scale when it fits
        let dw = Double(view.drawableSize.width), dh = Double(view.drawableSize.height)
        let fit = min(dw / Double(snap.view_w), dh / Double(snap.view_h))
        let integer = UserDefaults.standard.bool(forKey: SettingsKey.integerScale)
        let scale = fit >= 1 && integer ? floor(fit) : fit
        let vw = Double(snap.view_w) * scale, vh = Double(snap.view_h) * scale
        enc.setViewport(MTLViewport(originX: (dw - vw) / 2, originY: (dh - vh) / 2, width: vw, height: vh, znear: 0, zfar: 1))

        // world px → NDC: x' = x * sx + tx (camera snapped to whole pixels)
        let camX = Float(snap.camera_x.rounded()), camY = Float(snap.camera_y.rounded())
        var xform = SIMD4<Float>(2 / snap.view_w, 2 / snap.view_h, -1 - 2 * camX / snap.view_w, -1 - 2 * camY / snap.view_h)

        enc.setRenderPipelineState(pipeline)
        enc.setFragmentSamplerState(sampler, index: 0)

        // sky: SMB1 overworld blue (#5C94FC) until background models are drawn
        if let sky = skyTexture {
            var ident = SIMD4<Float>(1, 1, 0, 0)
            enc.setVertexBytes(&ident, length: 16, index: 1)
            let quad: [SIMD4<Float>] = [SIMD4(-1, -1, 0, 0), SIMD4(1, -1, 1, 0), SIMD4(1, 1, 1, 1), SIMD4(-1, -1, 0, 0), SIMD4(1, 1, 1, 1), SIMD4(-1, 1, 0, 1)]
            enc.setFragmentTexture(sky, index: 0)
            quad.withUnsafeBytes { enc.setVertexBytes($0.baseAddress!, length: $0.count, index: 0) }
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6)
        }
        enc.setVertexBytes(&xform, length: MemoryLayout<SIMD4<Float>>.size, index: 1)

        let sprites = UnsafeBufferPointer(start: snap.sprites, count: snap.sprite_count)
        var verts: [SIMD4<Float>] = []
        verts.reserveCapacity(sprites.count * 6)
        var batchStart = 0
        var batchTexture: UInt32 = .max

        func flush() {
            let n = verts.count - batchStart
            guard n > 0, let tex = texture(forBatch: batchTexture) else { batchStart = verts.count; return }
            enc.setFragmentTexture(tex, index: 0)
            verts.withUnsafeBytes { raw in
                let slice = UnsafeRawBufferPointer(rebasing: raw[(batchStart * 16)...])
                if slice.count <= 4096 {
                    enc.setVertexBytes(slice.baseAddress!, length: n * 16, index: 0)
                } else if let buf = device.makeBuffer(bytes: slice.baseAddress!, length: n * 16) {
                    enc.setVertexBuffer(buf, offset: 0, index: 0)
                }
            }
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: n)
            batchStart = verts.count
        }

        for s in sprites {
            let key = s.texture == MMK_TEXTURE_NONE ? (drawMissing ? Self.fallbackKey | min(s.layer, 0xFFFF) : MMK_TEXTURE_NONE) : s.texture
            if key != batchTexture { flush(); batchTexture = key }
            var (u0, u1) = (s.u0, s.u1)
            if s.flags & 1 != 0 { swap(&u0, &u1) }
            let x0 = s.x.rounded(), y0 = s.y.rounded(), x1 = x0 + s.w, y1 = y0 + s.h
            // two triangles; v is top-down in the texture, y is up in the world
            verts += [SIMD4(x0, y0, u0, s.v1), SIMD4(x1, y0, u1, s.v1), SIMD4(x1, y1, u1, s.v0),
                      SIMD4(x0, y0, u0, s.v1), SIMD4(x1, y1, u1, s.v0), SIMD4(x0, y1, u0, s.v0)]
        }
        flush()

        // HUD strip across the top of the view, drawn at drawable resolution
        var hud = MmkHud()
        mmk_engine_hud(engine, &hud)
        let time = hud.time < 0 ? "---" : String(format: "%03d", hud.time)
        var text = String(format: "MARIO ×%02d    ⦿×%02d    %09d    TIME %@", hud.lives, hud.coins, hud.score, time)
        if hud.flags & 1 != 0 { text += "    COURSE CLEAR!" }
        let stripH = 18.0
        let tw = Int(vw), th = Int(stripH * scale)
        if text != hudText || hudSize != (tw, th) {
            hudText = text
            hudSize = (tw, th)
            hudTexture = makeTextTexture(text, width: tw, height: th)
        }
        if let tex = hudTexture {
            var ident = SIMD4<Float>(1, 1, 0, 0)
            enc.setVertexBytes(&ident, length: 16, index: 1)
            let y0 = Float(1 - 2 * stripH / Double(snap.view_h))
            let quad: [SIMD4<Float>] = [SIMD4(-1, y0, 0, 1), SIMD4(1, y0, 1, 1), SIMD4(1, 1, 1, 0),
                                        SIMD4(-1, y0, 0, 1), SIMD4(1, 1, 1, 0), SIMD4(-1, 1, 0, 0)]
            enc.setFragmentTexture(tex, index: 0)
            quad.withUnsafeBytes { enc.setVertexBytes($0.baseAddress!, length: $0.count, index: 0) }
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6)
        }

        enc.endEncoding()
        if let path = screenshot, snap.tick >= screenshotTicks {
            cmd.commit()
            cmd.waitUntilCompleted()
            saveTexture(drawable.texture, to: path)
            exit(0)
        }
        cmd.present(drawable)
        cmd.commit()
        if DebugCapture.requested && snap.tick % 10 == 0 {
            // keep a copy of the frame: window snapshots can't see Metal layers
            cmd.waitUntilCompleted()
            DebugCapture.lastFrame = image(of: drawable.texture)
        }
    }

    private func makeTextTexture(_ text: String, width: Int, height: Int) -> MTLTexture? {
        guard width > 0, height > 0,
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        let font = CTFontCreateWithName("Menlo-Bold" as CFString, CGFloat(height) * 0.62, nil)
        func draw(_ color: CGColor, dx: CGFloat, dy: CGFloat) {
            let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attrs))
            ctx.textPosition = CGPoint(x: CGFloat(height) * 0.5 + dx, y: CGFloat(height) * 0.28 + dy)
            CTLineDraw(line, ctx)
        }
        let px = max(1, CGFloat(height) / 18)
        draw(CGColor(red: 0, green: 0, blue: 0, alpha: 1), dx: px, dy: -px)
        draw(CGColor(red: 1, green: 1, blue: 1, alpha: 1), dx: 0, dy: 0)
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: width, height: height, mipmapped: false)
        guard let tex = device.makeTexture(descriptor: d), let data = ctx.data else { return nil }
        tex.replace(region: MTLRegionMake2D(0, 0, width, height), mipmapLevel: 0, withBytes: data, bytesPerRow: width * 4)
        return tex
    }

    // MARK: missing art

    /// Batch keys at or above this are placeholder boxes for layer (key & 0xFFFF).
    private static let fallbackKey: UInt32 = 0xF000_0000

    private func texture(forBatch key: UInt32) -> MTLTexture? {
        if key < Self.fallbackKey { return Int(key) < textures.count ? textures[Int(key)] : nil }
        if key == MMK_TEXTURE_NONE { return nil }
        if let t = fallbackTextures[key] { return t }
        let t = makeFallbackTexture(layer: key & 0xFFFF)
        fallbackTextures[key] = t
        return t
    }

    /// An 8x8 box with a darker 1-px border, coloured by draw layer (terrain, blocks, coins,
    /// actors, player, effects), so a course is readable and playable without any game art.
    private func makeFallbackTexture(layer: UInt32) -> MTLTexture? {
        let rgb: (UInt8, UInt8, UInt8) = switch layer {
        case ..<11: (0xB0, 0x6A, 0x3A)    // terrain
        case ..<20: (0xE0, 0xA8, 0x30)    // blocks
        case ..<30: (0xF8, 0xD8, 0x40)    // coins
        case ..<50: (0x50, 0xA0, 0x50)    // actors
        case ..<60: (0xE0, 0x40, 0x40)    // player
        default: (0xF0, 0xF0, 0xF0)       // effects
        }
        var px = [UInt8]()
        for y in 0..<8 {
            for x in 0..<8 {
                let edge = x == 0 || y == 0 || x == 7 || y == 7
                let k: Double = edge ? 0.55 : 1
                px += [UInt8(Double(rgb.0) * k), UInt8(Double(rgb.1) * k), UInt8(Double(rgb.2) * k), 0xFF]
            }
        }
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 8, height: 8, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        t?.replace(region: MTLRegionMake2D(0, 0, 8, 8), mipmapLevel: 0, withBytes: px, bytesPerRow: 32)
        return t
    }

    // MARK: smoke test

    static func scriptedInput(tick: UInt64) -> MmkInput {
        var b = UInt32(MMK_BTN_RIGHT | MMK_BTN_RUN)
        if tick % 90 >= 60 && tick % 90 < 80 { b |= UInt32(MMK_BTN_JUMP) }
        return MmkInput(buttons: b)
    }

    private func saveTexture(_ tex: MTLTexture, to path: String) {
        guard let img = image(of: tex),
              let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil)
        else { return }
        CGImageDestinationAddImage(dest, img, nil)
        CGImageDestinationFinalize(dest)
    }

    private func image(of tex: MTLTexture) -> CGImage? {
        let w = tex.width, h = tex.height
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        tex.getBytes(&bytes, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        guard let ctx = CGContext(data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: info.rawValue)
        else { return nil }
        return ctx.makeImage()
    }
}
