import CMmk
import ImageIO
import Metal
import CoreText
import MetalKit
import QuartzCore
import SwiftUI

/// Hosts an MTKView inside SwiftUI, drawing one game session.
struct MetalView: MetalViewRepresentable {
    let session: GameSession

    func makeCoordinator() -> Renderer { Renderer(session: session) }

    func makeView(context: Context) -> MTKView {
        let view = GameMTKView()
        view.device = context.coordinator.device
        view.colorPixelFormat = .bgra8Unorm_srgb
        view.depthStencilPixelFormat = Renderer.depthFormat   // meshes (3D styles)
        view.clearDepth = 1
        view.preferredFramesPerSecond = 120
        view.delegate = context.coordinator
        view.framebufferOnly = context.coordinator.screenshot == nil && !DebugCapture.requested
        #if os(macOS)
        view.onWindow = { [weak session] w in session?.input.window = w }
        #endif
        if DebugCapture.requested { DebugCapture.gameView = view }
        return view
    }

    func updateView(_ view: MTKView, context: Context) {}
}

#if os(macOS)
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
#else
/// The game's MTKView (keyboards are read through GameController, touches by `TouchControls`).
final class GameMTKView: MTKView {}
#endif

/// Draws a game session's sprite snapshot (the session steps the engine, see GameSession.frame).
@MainActor
final class Renderer: NSObject, MTKViewDelegate {
    let device = MTLCreateSystemDefaultDevice()!
    private lazy var queue = device.makeCommandQueue()!
    private var pipeline: MTLRenderPipelineState?
    private var sampler: MTLSamplerState?
    static let depthFormat: MTLPixelFormat = .depth32Float
    private lazy var spriteDepth = MeshRenderer.spriteDepthState(device)
    /// NSMBU / 3D World models (created when a frame first has mesh draws).
    private var meshRenderer: MeshRenderer?
    private lazy var skyTexture: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        let t = device.makeTexture(descriptor: d)
        var px: [UInt8] = [0x5C, 0x94, 0xFC, 0xFF]
        t?.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        return t
    }()
    private var skyColor: UInt32 = 0x5C94FCFF
    private var debugFrames = 0
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
        if screenshot != nil { session.scriptedInput = Self.scriptedInput }
        // MMK_NO_MESH=1: draw the 3D styles with sprites only (comparison, debugging)
        if ProcessInfo.processInfo.environment["MMK_NO_MESH"] == "1" { mmk_engine_set_meshes(session.engine, false) }
    }

    // MARK: setup

    private func makeTexture(_ t: MmkTextureInfo) -> MTLTexture? { GPUTexture.make(device, t) }

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
        desc.depthAttachmentPixelFormat = Self.depthFormat
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

    /// The sprites' world → NDC transform (x' = x·sx + tx, y' = y·sy + ty) as a 4×4 matrix (the
    /// 2D styles' meshes, without a 3D scene). Depth: z = +500 … −500 → 0 … 1.
    static func orthoMatrix(_ x: SIMD4<Float>) -> simd_float4x4 {
        simd_float4x4(columns: (SIMD4(x.x, 0, 0, 0), SIMD4(0, x.y, 0, 0), SIMD4(0, 0, -0.001, 0), SIMD4(x.z, x.w, 0.5, 1)))
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        guard let pipeline, let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable else { return }

        // fixed 60 Hz simulation (or the pause menu), independent of the display rate
        session.frame(now: CACurrentMediaTime())
        // (the front end's menus show no course: only their layouts are drawn)
        let showCourse = session.showsCourse

        var snap = MmkSnapshot()
        mmk_engine_snapshot(engine, &snap)
        var meshPtr: UnsafePointer<MmkMeshDraw>?
        let meshCount = showCourse ? mmk_engine_meshes(engine, &meshPtr) : 0
        var bgPtr: UnsafePointer<MmkMeshDraw>?
        let bgCount = showCourse ? mmk_engine_background(engine, &bgPtr) : 0
        var scene3d = MmkScene3d()
        let has3d = mmk_engine_scene3d(engine, &scene3d)
        if meshCount + bgCount > 0 && meshRenderer == nil {
            meshRenderer = try? MeshRenderer(device: device, assets: session.assets.handle,
                                             colorFormat: .bgra8Unorm_srgb, depthFormat: Self.depthFormat)
            meshRenderer?.texture = { [weak self] i in self?.texture(forBatch: i) }
        }
        let meshFrame = meshCount > 0 ? meshRenderer?.prepare(UnsafeBufferPointer(start: meshPtr, count: meshCount)) : nil
        let bgFrame = bgCount > 0 && has3d && scene3d.has_background
            ? meshRenderer?.prepare(UnsafeBufferPointer(start: bgPtr, count: bgCount)) : nil
        let flipY = mmk_engine_view_flags(engine) & 1 != 0
        let fieldVP = has3d ? MeshRenderer.matrix(scene3d.field_view_proj) : nil

        // black bars outside the game view; the sky and the background scene are drawn inside it
        // in a pass of their own, the course over them with the depth cleared
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        pass.colorAttachments[0].loadAction = .clear
        guard let cmd = queue.makeCommandBuffer(), let bgEnc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }

        // letterbox the 16:9 view into the drawable, at an integer scale when it fits
        let dw = Double(view.drawableSize.width), dh = Double(view.drawableSize.height)
        let fit = min(dw / Double(snap.view_w), dh / Double(snap.view_h))
        let integer = showCourse && UserDefaults.standard.bool(forKey: SettingsKey.integerScale)
        let scale = fit >= 1 && integer ? floor(fit) : fit
        let vw = Double(snap.view_w) * scale, vh = Double(snap.view_h) * scale
        let viewport = MTLViewport(originX: (dw - vw) / 2, originY: (dh - vh) / 2, width: vw, height: vh, znear: 0, zfar: 1)
        bgEnc.setViewport(viewport)

        // world px → NDC: x' = x * sx + tx (camera snapped to whole pixels)
        let camX = Float(snap.camera_x.rounded()), camY = Float(snap.camera_y.rounded())
        var xform = SIMD4<Float>(2 / snap.view_w, 2 / snap.view_h, -1 - 2 * camX / snap.view_w, -1 - 2 * camY / snap.view_h)
        if mmk_engine_view_flags(engine) & 1 != 0 {
            // underground night: the course is shown upside down
            xform.y = -xform.y
            xform.w = -xform.w
        }

        bgEnc.setRenderPipelineState(pipeline)
        bgEnc.setFragmentSamplerState(sampler, index: 0)
        bgEnc.setDepthStencilState(spriteDepth)

        // clear colour behind the theme background (black for underground/castle)
        let clear = mmk_engine_clear_color(engine)
        if clear != skyColor, let sky = skyTexture {
            skyColor = clear
            var px: [UInt8] = [UInt8(clear >> 24), UInt8((clear >> 16) & 0xff), UInt8((clear >> 8) & 0xff), 0xff]
            sky.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: &px, bytesPerRow: 4)
        }
        if showCourse, let sky = skyTexture {
            var ident = SIMD4<Float>(1, 1, 0, 0)
            bgEnc.setVertexBytes(&ident, length: 16, index: 1)
            let quad: [SIMD4<Float>] = [SIMD4(-1, -1, 0, 0), SIMD4(1, -1, 1, 0), SIMD4(1, 1, 1, 1), SIMD4(-1, -1, 0, 0), SIMD4(1, 1, 1, 1), SIMD4(-1, 1, 0, 1)]
            bgEnc.setFragmentTexture(sky, index: 0)
            quad.withUnsafeBytes { bgEnc.setVertexBytes($0.baseAddress!, length: $0.count, index: 0) }
            bgEnc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6)
        }
        // the 3D styles' background scene, with its own camera and depth
        if let bgFrame, let meshRenderer {
            let cam = MeshRenderer.Camera(viewProj: MeshRenderer.matrix(scene3d.background_view_proj), layered: false,
                                          flipY: flipY, light: MeshRenderer.lightRows(scene3d.background_light))
            meshRenderer.encode(bgEnc, frame: bgFrame, range: 0..<bgFrame.posed.count, camera: cam)
        }
        bgEnc.endEncoding()

        pass.colorAttachments[0].loadAction = .load
        pass.depthAttachment.loadAction = .clear
        guard let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }
        enc.setViewport(viewport)
        enc.setRenderPipelineState(pipeline)
        enc.setFragmentSamplerState(sampler, index: 0)
        enc.setDepthStencilState(spriteDepth)
        // 3D World's slight perspective: sprites go through the field camera at z = 0 (projected
        // here, so the sprite shader keeps its plain transform)
        if fieldVP != nil { xform = SIMD4<Float>(1, 1, 0, 0) }
        enc.setVertexBytes(&xform, length: MemoryLayout<SIMD4<Float>>.size, index: 1)

        let sprites = UnsafeBufferPointer(start: snap.sprites, count: showCourse ? snap.sprite_count : 0)
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

        // meshes go before the sprite their `order` names (both lists follow the sim's layers)
        var nextMesh = 0
        func drawMeshes(before order: Int) {
            guard let meshFrame, let meshRenderer, nextMesh < meshFrame.posed.count,
                  Int(meshFrame.posed[nextMesh].draw.order) <= order else { return }
            flush()
            var end = nextMesh
            while end < meshFrame.posed.count && Int(meshFrame.posed[end].draw.order) <= order { end += 1 }
            let cam = MeshRenderer.Camera(viewProj: fieldVP ?? Self.orthoMatrix(xform), layered: true,
                                          flipY: flipY && fieldVP != nil,
                                          light: has3d ? MeshRenderer.lightRows(scene3d.field_light) : MeshRenderer.neutralLight)
            meshRenderer.encode(enc, frame: meshFrame, range: nextMesh..<end, camera: cam)
            nextMesh = end
            enc.setRenderPipelineState(pipeline)
            enc.setFragmentSamplerState(sampler, index: 0)
            enc.setDepthStencilState(spriteDepth)
            enc.setVertexBytes(&xform, length: MemoryLayout<SIMD4<Float>>.size, index: 1)
            batchTexture = .max
        }

        for (index, s) in sprites.enumerated() {
            drawMeshes(before: index)
            let key = s.texture == MMK_TEXTURE_NONE ? (drawMissing ? Self.fallbackKey | min(s.layer, 0xFFFF) : MMK_TEXTURE_NONE) : s.texture
            if key != batchTexture { flush(); batchTexture = key }
            var (u0, u1) = (s.u0, s.u1)
            if s.flags & 1 != 0 { swap(&u0, &u1) }
            var x0 = s.x.rounded(), y0 = s.y.rounded(), x1 = x0 + s.w, y1 = y0 + s.h
            if let vp = fieldVP {
                func ndc(_ x: Float, _ y: Float) -> SIMD2<Float> {
                    let c = vp * SIMD4<Float>(x, y, 0, 1)
                    return SIMD2(c.x / c.w, (flipY ? -1 : 1) * c.y / c.w)
                }
                let a = ndc(x0, y0), b = ndc(x1, y1)
                (x0, y0, x1, y1) = (a.x, a.y, b.x, b.y)
            }
            // two triangles; v is top-down in the texture, y is up in the world
            verts += [SIMD4(x0, y0, u0, s.v1), SIMD4(x1, y0, u1, s.v1), SIMD4(x1, y1, u1, s.v0),
                      SIMD4(x0, y0, u0, s.v1), SIMD4(x1, y1, u1, s.v0), SIMD4(x0, y1, u0, s.v0)]
        }
        flush()
        drawMeshes(before: Int.max)

        // HUD strip across the top of the view, drawn at drawable resolution
        var hud = MmkHud()
        mmk_engine_hud(engine, &hud)
        let time = hud.time < 0 ? "---" : String(format: "%03d", hud.time)
        var text = session.unlimitedLives
            ? String(format: "⦿×%02d    %09d    TIME %@", hud.coins, hud.score, time)
            : String(format: "LIVES ×%02d    ⦿×%02d    %09d    TIME %@", hud.lives, hud.coins, hud.score, time)
        if hud.flags & 1 != 0 { text += "    COURSE CLEAR!" }
        var cond = MmkCondition()
        if mmk_engine_clear_condition(engine, &cond) { text += "    " + ClearConditionText.describe(cond) }
        let stripH = 18.0
        let tw = Int(vw), th = Int(stripH * scale)
        let drawnHud = !session.hideHud   // (the front end draws the SMM2 HUD layout)
        if drawnHud && (text != hudText || hudSize != (tw, th)) {
            hudText = text
            hudSize = (tw, th)
            hudTexture = makeTextTexture(text, width: tw, height: th)
        }
        if drawnHud, let tex = hudTexture {
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

        // the front end's layouts (HUD, menus, scene fades …) over the course in a pass of their
        // own (the layout pipelines have no depth attachment)
        if session.hasLayouts {
            let pass2 = MTLRenderPassDescriptor()
            pass2.colorAttachments[0].texture = drawable.texture
            pass2.colorAttachments[0].loadAction = .load
            pass2.colorAttachments[0].storeAction = .store
            if let enc2 = cmd.makeRenderCommandEncoder(descriptor: pass2) {
                enc2.setViewport(MTLViewport(originX: (dw - vw) / 2, originY: (dh - vh) / 2, width: vw, height: vh, znear: 0, zfar: 1))
                session.encodeLayouts(enc2, device: device)
                enc2.endEncoding()
            }
        }

        if let path = screenshot, snap.tick >= screenshotTicks {
            cmd.commit()
            cmd.waitUntilCompleted()
            saveTexture(drawable.texture, to: path)
            exit(0)
        }
        cmd.present(drawable)
        cmd.commit()
        debugFrames += 1
        if DebugCapture.requested && debugFrames % 10 == 0 {   // also while paused (menus, overlays)
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
        if key < Self.fallbackKey {
            let assets = session.assets
            let i = Int(key)
            if let t = assets.gpuTextures[i] { return t }
            var info = MmkTextureInfo()
            let t = mmk_assets_texture(assets.handle, i, &info) ? makeTexture(info) : nil
            assets.gpuTextures[i] = t
            return t
        }
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

    private func image(of tex: MTLTexture) -> CGImage? { DebugCapture.image(of: tex) }
}

/// Short HUD text for a course's clear condition (our own wording).
enum ClearConditionText {
    static let enemies: [UInt32: String] = [
        1: "Goombas", 3: "Koopas", 4: "Buzzy Beetles", 5: "Spinies", 6: "Piranha Plants",
        7: "Fire Piranhas", 8: "Munchers", 10: "Dry Bones", 12: "Rocky Wrenches", 13: "Monty Moles",
        14: "Hammer Bros", 16: "Lakitus", 17: "Bloopers", 18: "Cheep Cheeps", 19: "Spike Tops",
        21: "Lava Bubbles", 22: "Chain Chomps", 23: "Wigglers", 24: "Bob-ombs", 25: "Thwomps",
        26: "Boos", 28: "Banzai Bills", 29: "Magikoopas", 30: "Angry Suns", 37: "Bowsers",
        38: "Bowser Jrs", 39: "Boom Booms", 40: "Pom Poms", 41: "Bullet Bills",
    ]

    static func describe(_ c: MmkCondition) -> String {
        let mark = c.met != 0 ? "✓" : "⚑"
        let progress = " \(min(c.count, c.needed))/\(c.needed)"
        switch c.kind {
        case 1: return "\(mark) \(enemies[c.target] ?? "enemies")" + progress
        case 2: return "\(mark) 1-Ups" + progress
        case 3: return "\(mark) " + (c.target == 0 ? "coins" : "\(c.target)-coins") + progress
        case 4: return "\(mark) POW" + progress
        case 5: return "\(mark) P Switch" + progress
        case 6: return "\(mark) goal invincible"
        case 7: return "\(mark) goal as Super"
        case 8: return "\(mark) goal as Fire"
        case 9: return "\(mark) no damage"
        case 10: return "\(mark) no landing"
        case 11: return "\(mark) goal in a shoe"
        case 12: return "\(mark) goal in a Clown Car"
        case 13: return "\(mark) goal as Raccoon"
        case 14: return "\(mark) goal with a cape"
        case 15: return "\(mark) goal as Propeller"
        case 16: return "\(mark) goal holding a shell"
        case 17: return "\(mark) goal on Yoshi"
        case 18: return "\(mark) goal as Cat Mario"
        case 19: return "\(mark) goal in a Koopa Troopa Car"
        case 20: return "\(mark) goal as Builder Mario"
        case 21: return "\(mark) goal on a Lakitu's Cloud"
        case 22: return "\(mark) goal holding the stone"
        case 23: return "\(mark) trees" + progress
        case 24: return "\(mark) no claw"
        case 25: return "\(mark) no water"
        case 26: return "\(mark) goal in a " + (c.target == 2 ? "Spiny" : "Buzzy") + " helmet"
        case 27: return "\(mark) goal in a Dry Bones Shell"
        case 28: return "\(mark) goal as Superball Mario"
        case 29: return "\(mark) goal as Big Mario"
        default: return "⚑ condition not supported"
        }
    }
}
