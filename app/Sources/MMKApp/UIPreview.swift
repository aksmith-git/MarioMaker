import CMmk
import ImageIO
import MetalKit
import SwiftUI

/// Debug: show one imported layout full-screen with the layout player (SMM1, or SMM2 with
/// MMK_UI_CACHE=<smm2-ui import>).
///
///     MMK_UI_LAYOUT=<name>                 e.g. Nwk_WorldMapEsy_00, Play_Title_00
///     [MMK_UI_ANIM=<spec>[,<spec>…]]       spec: Name (plays from 0, holds the last frame),
///                                          Name~ (loops), Name@<frame> (fixed frame);
///                                          parts: L_Part_00/Name
///     [MMK_UI_TEXT=<pane>=<text>[;…]]      replace texts
///     [MMK_UI_CACHE=<dir>]                 SMM1 import (default: <support>/assets/smm1-*,
///                                          ~/smm-work/cache/smm1-*)
///     [MMK_SCREENSHOT=<png> [MMK_SCREENSHOT_TICKS=<n>, default 60]]
///                                          render at 1280×720, one tick per frame, save, quit
enum UIPreview {
    static let env = ProcessInfo.processInfo.environment
    static var requested: Bool { env["MMK_UI_LAYOUT"] != nil }

    /// The SMM1 UI import to use.
    static func cacheDir() -> URL? {
        AppPaths.findSMM1Cache()
    }
}

struct UIPreviewView: MetalViewRepresentable {
    func makeCoordinator() -> UIPreviewRenderer { UIPreviewRenderer() }

    func makeView(context: Context) -> MTKView {
        let view = MTKView()
        view.device = context.coordinator.device
        view.colorPixelFormat = .bgra8Unorm_srgb
        view.framebufferOnly = false
        view.preferredFramesPerSecond = 60
        if context.coordinator.screenshot != nil {
            view.autoResizeDrawable = false
            view.drawableSize = CGSize(width: 1280, height: 720)
        }
        view.delegate = context.coordinator
        return view
    }

    func updateView(_ view: MTKView, context: Context) {}
}

@MainActor
final class UIPreviewRenderer: NSObject, MTKViewDelegate {
    let device = MTLCreateSystemDefaultDevice()!
    private lazy var queue = device.makeCommandQueue()!
    private var ui: OpaquePointer?
    private var layout: OpaquePointer?
    private var layer: UILayer?
    private var size = SIMD2<Float>(1280, 720)
    private struct Anim { var name: String; var fixed: Float?; var loop: Bool; var length: Float }
    private var anims: [Anim] = []
    private var tick = 0
    private var start = CACurrentMediaTime()
    let screenshot = UIPreview.env["MMK_SCREENSHOT"]
    private let screenshotTicks = Int(UIPreview.env["MMK_SCREENSHOT_TICKS"] ?? "") ?? 60

    override init() {
        super.init()
        let env = UIPreview.env
        guard let dir = UIPreview.cacheDir() else { fail("no SMM1 UI import found (set MMK_UI_CACHE)") }
        guard let ui = mmk_ui_open(dir.path) else { fail("mmk_ui_open: \(lastEngineError())") }
        self.ui = ui
        let name = env["MMK_UI_LAYOUT"] ?? ""
        guard let layout = mmk_ui_layout(ui, name) else { fail("layout \(name): \(lastEngineError())") }
        self.layout = layout
        var w: Float = 0, h: Float = 0
        mmk_ui_layout_size(layout, &w, &h)
        if w > 0 && h > 0 { size = SIMD2(w, h) }
        for spec in (env["MMK_UI_ANIM"] ?? "").split(separator: ",").map(String.init) where !spec.isEmpty {
            var a = Anim(name: spec, fixed: nil, loop: false, length: 0)
            if let at = spec.firstIndex(of: "@") {
                a.name = String(spec[..<at])
                a.fixed = Float(spec[spec.index(after: at)...])
            } else if spec.hasSuffix("~") {
                a.name = String(spec.dropLast())
                a.loop = true
            }
            a.length = mmk_ui_anim_length(layout, a.name)
            if a.length < 0 { FileHandle.standardError.write("MMK_UI_ANIM: no animation '\(a.name)' in \(name)\n".data(using: .utf8)!) }
            anims.append(a)
        }
        for pair in (env["MMK_UI_TEXT"] ?? "").split(separator: ";") {
            let kv = pair.split(separator: "=", maxSplits: 1).map(String.init)
            if kv.count == 2 { _ = mmk_ui_set_text(layout, kv[0], kv[1]) }
        }
        do {
            layer = try UILayer(device: device, ui: ui, pixelFormat: .bgra8Unorm_srgb)
        } catch {
            fail("UI pipeline: \(error)")
        }
    }

    private func fail(_ msg: String) -> Never {
        FileHandle.standardError.write("MMK_UI_LAYOUT: \(msg)\n".data(using: .utf8)!)
        exit(1)
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        guard let layout, let layer, let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable else { return }
        // one tick per frame for screenshots (deterministic), otherwise 60 Hz wall clock
        tick = screenshot != nil ? tick + 1 : Int((CACurrentMediaTime() - start) * 60)
        for a in anims where a.length >= 0 {
            var f = a.fixed ?? Float(tick)
            if a.fixed == nil {
                f = a.loop && a.length > 0 ? f.truncatingRemainder(dividingBy: a.length) : min(f, a.length)
            }
            _ = mmk_ui_play(layout, a.name, f)
        }
        var ptr: UnsafePointer<MmkUiQuad>?
        let n = mmk_ui_draw_list(layout, &ptr)

        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        pass.colorAttachments[0].loadAction = .clear
        guard let cmd = queue.makeCommandBuffer(), let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }
        // letterbox the layout's aspect ratio into the drawable
        let dw = Double(view.drawableSize.width), dh = Double(view.drawableSize.height)
        let scale = min(dw / Double(size.x), dh / Double(size.y))
        let vw = Double(size.x) * scale, vh = Double(size.y) * scale
        enc.setViewport(MTLViewport(originX: (dw - vw) / 2, originY: (dh - vh) / 2, width: vw, height: vh, znear: 0, zfar: 1))
        if let ptr { layer.encode(enc, quads: UnsafeBufferPointer(start: ptr, count: n), layoutSize: size) }
        enc.endEncoding()
        if let path = screenshot, tick >= screenshotTicks {
            cmd.commit()
            cmd.waitUntilCompleted()
            save(drawable.texture, to: path)
            exit(0)
        }
        cmd.present(drawable)
        cmd.commit()
    }

    private func save(_ tex: MTLTexture, to path: String) {
        let w = tex.width, h = tex.height
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        tex.getBytes(&bytes, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        guard let ctx = CGContext(data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: info.rawValue),
              let img = ctx.makeImage(),
              let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil)
        else { return }
        CGImageDestinationAddImage(dest, img, nil)
        CGImageDestinationFinalize(dest)
    }
}
