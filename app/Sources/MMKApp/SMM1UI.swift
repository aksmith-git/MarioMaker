import CMmk
import MetalKit
import QuartzCore
import SwiftUI

/// An imported UI (layouts, textures, fonts, text) opened once, with the GPU side shared by every
/// layout-driven screen:
/// - `LayoutUI.smm1` (alias `SMM1UI.shared`): the SMM1 import (`mmk import-smm1`) for the 100
///   Mario screens; where it is looked for: `AppPaths.findSMM1Cache()`; `MMK_UI_SMM1=off` ignores it.
/// - `LayoutUI.smm2`: the SMM2 import (`mmk import-smm2-ui`) for the title screen and menus;
///   `AppPaths.findSMM2UICache()`; `MMK_UI_SMM2=off` ignores it.
/// `nil` when there is no import: the app then uses its SwiftUI screens.
@MainActor
final class LayoutUI {
    static let smm1: LayoutUI? = open(ProcessInfo.processInfo.environment["MMK_UI_SMM1"] == "off" ? nil : AppPaths.findSMM1Cache())
    static let smm2: LayoutUI? = open(ProcessInfo.processInfo.environment["MMK_UI_SMM2"] == "off" ? nil : AppPaths.findSMM2UICache())
    /// The SMM1 import (100 Mario screens).
    static var shared: LayoutUI? { smm1 }

    let handle: OpaquePointer
    let device: MTLDevice
    let layer: UILayer
    let dir: URL

    private init(handle: OpaquePointer, device: MTLDevice, layer: UILayer, dir: URL) {
        self.handle = handle
        self.device = device
        self.layer = layer
        self.dir = dir
    }

    private static func open(_ dir: URL?) -> LayoutUI? {
        guard let dir, let device = MTLCreateSystemDefaultDevice(), let h = mmk_ui_open(dir.path) else { return nil }
        guard let layer = try? UILayer(device: device, ui: h, pixelFormat: .bgra8Unorm_srgb) else {
            mmk_ui_free(h)
            return nil
        }
        return LayoutUI(handle: h, device: device, layer: layer, dir: dir)
    }

    func layout(_ name: String) -> UILayout? { UILayout(ui: self, name: name) }

    /// Message `label` of text table `table` (a layout's, or a shared SMM2 one such as
    /// `GoalCondition`); value tags become U+FFFC.
    func message(table: String, _ label: String) -> String? {
        let n = mmk_ui_table_message(handle, table, label, nil, 0)
        guard n > 0 else { return nil }
        var buf = [CChar](repeating: 0, count: n + 1)
        _ = mmk_ui_table_message(handle, table, label, &buf, buf.count)
        return String(cString: buf)
    }

    /// Message `label` of `table` with its value tags filled in order from `args`.
    func format(table: String, _ label: String, _ args: [String]) -> String? {
        message(table: table, label).map { UILayout.fill($0, args) }
    }

    /// Draw `layers` (1280×720 layouts, back to front) into the current viewport.
    func encode(_ enc: MTLRenderCommandEncoder, layers: [UILayout]) {
        for l in layers {
            var ptr: UnsafePointer<MmkUiQuad>?
            let n = mmk_ui_draw_list(l.handle, &ptr)
            if let ptr, n > 0 { layer.encode(enc, quads: UnsafeBufferPointer(start: ptr, count: n), layoutSize: l.size) }
        }
    }
}

/// The SMM1 screens' name for the shared layout UI type.
typealias SMM1UI = LayoutUI

/// One playing layout plus a small timeline: animations are scheduled on the owner's tick
/// counter (60 per second) and evaluated by `apply(tick:)` before drawing.
@MainActor
final class UILayout {
    let handle: OpaquePointer
    let name: String
    /// The import this layout came from (the `LayoutUI`s live as long as the app).
    private let uiHandle: OpaquePointer
    private(set) var size = SIMD2<Float>(1280, 720)

    private struct Track {
        var start: Int
        var from: Float
        var to: Float?
        var loop: Bool
        var speed: Float
    }
    private var tracks: [(name: String, track: Track)] = []
    private var lengths: [String: Float] = [:]

    init?(ui: SMM1UI, name: String) {
        guard let h = mmk_ui_layout(ui.handle, name) else { return nil }
        handle = h
        self.name = name
        uiHandle = ui.handle
        var w: Float = 0, hgt: Float = 0
        mmk_ui_layout_size(h, &w, &hgt)
        if w > 0 && hgt > 0 { size = SIMD2(w, hgt) }
    }

    deinit { mmk_ui_layout_free(handle) }

    /// Length in frames (−1 when the layout has no such animation).
    func length(_ anim: String) -> Float {
        if let l = lengths[anim] { return l }
        let l = mmk_ui_anim_length(handle, anim)
        lengths[anim] = l
        return l
    }

    func has(_ anim: String) -> Bool { length(anim) >= 0 }

    /// Play `anim` (`[Part/…/]Name`) from frame `from` starting at tick `tick`, at `speed` frames
    /// per tick, until `to` (default: its end), holding there; `loop` wraps instead. Before `tick`
    /// the animation sits at `from`.
    func play(_ anim: String, at tick: Int, from: Float = 0, to: Float? = nil, loop: Bool = false, speed: Float = 1) {
        guard has(anim) else { return }
        let t = Track(start: tick, from: from, to: to, loop: loop, speed: speed)
        if let i = tracks.firstIndex(where: { $0.name == anim }) { tracks[i].track = t } else { tracks.append((anim, t)) }
    }

    /// Show `anim` at a fixed frame.
    func hold(_ anim: String, _ frame: Float) { play(anim, at: 0, from: frame, to: frame) }

    func stop(_ anim: String) {
        tracks.removeAll { $0.name == anim }
        mmk_ui_stop(handle, anim)
    }

    func isPlaying(_ anim: String) -> Bool { tracks.contains { $0.name == anim } }

    /// Frame of `anim` at `tick` (nil when not scheduled).
    func frame(_ anim: String, at tick: Int) -> Float? {
        guard let t = tracks.first(where: { $0.name == anim })?.track else { return nil }
        return Self.frame(t, length: length(anim), tick: tick)
    }

    /// True once `anim` has reached its last frame (or `to`) at `tick`.
    func finished(_ anim: String, at tick: Int) -> Bool {
        guard let t = tracks.first(where: { $0.name == anim })?.track, !t.loop else { return false }
        let end = t.to ?? length(anim)
        return Float(tick - t.start) * t.speed + t.from >= end
    }

    private static func frame(_ t: Track, length: Float, tick: Int) -> Float {
        let f = t.from + Float(max(0, tick - t.start)) * t.speed
        if t.loop {
            return length > 0 ? f.truncatingRemainder(dividingBy: length) : 0
        }
        let end = t.to ?? length
        return t.speed >= 0 ? min(f, end) : max(f, end)
    }

    /// Bind every scheduled animation at its frame for `tick`.
    func apply(tick: Int) {
        for (name, t) in tracks { _ = mmk_ui_play(handle, name, Self.frame(t, length: length(name), tick: tick)) }
    }

    func text(_ pane: String, _ s: String) { _ = mmk_ui_set_text(handle, pane, s) }

    /// Force picture / window `pane`'s material white colour (RGBA 0–255) over its animations;
    /// `nil` gives it back to them.
    func materialWhite(_ pane: String, _ rgba: [UInt8]?) {
        if let rgba { _ = mmk_ui_set_material_color(handle, pane, 1, rgba) } else { _ = mmk_ui_set_material_color(handle, pane, 1, nil) }
    }

    /// Width (pane units) of text pane `pane`'s widest line (nil: no such pane).
    func textWidth(_ pane: String) -> Float? {
        let w = mmk_ui_text_width(handle, pane)
        return w >= 0 ? w : nil
    }

    /// Draw picture `pane` with the import's texture `ui/textures/<group>/<name>.mtex` (`nil`:
    /// its own texture again). False when the texture or the picture is missing.
    @discardableResult
    func picture(_ pane: String, group: String, _ name: String?) -> Bool {
        mmk_ui_set_picture(uiHandle, handle, pane, group, name)
    }

    /// Force a pane shown / hidden (`nil`: back to its animations).
    func visible(_ pane: String, _ v: Bool?) { _ = mmk_ui_set_visible(handle, pane, v.map { $0 ? 1 : 0 } ?? -1) }

    func pane(_ name: String) -> MmkUiPane? {
        var p = MmkUiPane()
        return mmk_ui_pane(handle, name, &p) ? p : nil
    }

    /// Where pane `name` (`[Part/…/]Pane`) is drawn: centre and size of its rectangle in layout
    /// units (y up), its global alpha (0–1) and whether it is shown. Apply the timeline first.
    func rect(_ name: String) -> (center: SIMD2<Float>, size: SIMD2<Float>, alpha: Float, visible: Bool)? {
        var r = MmkUiRect()
        guard mmk_ui_pane_rect(handle, name, &r) else { return nil }
        let c = r.corners
        let xs = [c.0, c.2, c.4, c.6], ys = [c.1, c.3, c.5, c.7]
        let lo = SIMD2(xs.min()!, ys.min()!), hi = SIMD2(xs.max()!, ys.max()!)
        return ((lo + hi) / 2, hi - lo, r.alpha, r.visible != 0)
    }

    /// Force a pane's translate (parent space) / size / scale / alpha (0–255) over its
    /// animations; all nil gives it back to them.
    func setPane(_ name: String, translate: SIMD2<Float>? = nil, size: SIMD2<Float>? = nil,
                 scale: SIMD2<Float>? = nil, alpha: Float? = nil) {
        var flags: UInt32 = 0
        if translate != nil { flags |= 1 }
        if size != nil { flags |= 2 }
        if scale != nil { flags |= 4 }
        if alpha != nil { flags |= 8 }
        let t = translate ?? .zero, s = size ?? .zero, k = scale ?? SIMD2(1, 1)
        _ = mmk_ui_set_pane(handle, name, flags, t.x, t.y, s.x, s.y, k.x, k.y, alpha ?? 255)
    }

    /// Draw a copy of part pane `part` as `name`, moved by `offset`; it follows the original's
    /// animations and has its own part instance (`name/Anim`, `name/Pane`).
    @discardableResult
    func clone(_ part: String, as name: String, offset: SIMD2<Float>) -> Bool {
        mmk_ui_add_clone(handle, part, name, offset.x, offset.y)
    }

    /// A message of this layout's text table; value tags become U+FFFC.
    func message(_ label: String) -> String? {
        let n = mmk_ui_message(handle, label, nil, 0)
        guard n > 0 else { return nil }
        var buf = [CChar](repeating: 0, count: n + 1)
        _ = mmk_ui_message(handle, label, &buf, buf.count)
        return String(cString: buf)
    }

    /// Message `label` with its value tags filled in order from `args` (nil if there is none).
    func format(_ label: String, _ args: [String]) -> String? {
        message(label).map { Self.fill($0, args) }
    }

    /// A message template with its U+FFFC value slots filled in order from `args`.
    nonisolated static func fill(_ m: String, _ args: [String]) -> String {
        var out = "", i = 0
        for ch in m {
            if ch == "\u{FFFC}" {
                out += i < args.count ? args[i] : ""
                i += 1
            } else {
                out.append(ch)
            }
        }
        return out
    }
}

/// Menu buttons for one 60 Hz tick of a layout screen: `pressed` went down this tick.
struct UIInput {
    var pressed: MenuInput = []
    var held: MenuInput = []
    /// The skip key (Settings → Keyboard/Controller "Skip course") is down.
    var skip = false
}

/// A layout-driven screen: steps at 60 Hz and draws its layouts back to front.
@MainActor
protocol UIScene: AnyObject {
    var layers: [UILayout] { get }
    /// Ticks stepped so far (debug captures freeze at `MMK_UI_TICKS`).
    var tick: Int { get }
    func step(_ input: UIInput)
}

/// Hosts a `UIScene` full screen (16:9, letterboxed), with keyboard and controller input.
struct UIScreenView: MetalViewRepresentable {
    let scene: UIScene
    /// The imported UI the scene's layouts come from (SMM1 by default).
    var ui: LayoutUI? = LayoutUI.smm1

    func makeCoordinator() -> UIScreenRenderer { UIScreenRenderer(scene: scene, ui: ui) }

    func makeView(context: Context) -> MTKView {
        let view = GameMTKView()
        view.device = ui?.device ?? MTLCreateSystemDefaultDevice()
        view.colorPixelFormat = .bgra8Unorm_srgb
        view.preferredFramesPerSecond = 60
        view.framebufferOnly = !DebugCapture.requested
        view.delegate = context.coordinator
        #if os(macOS)
        let input = context.coordinator.input
        view.onWindow = { w in input.window = w }
        #endif
        if DebugCapture.requested { DebugCapture.gameView = view }
        return view
    }

    func updateView(_ view: MTKView, context: Context) { context.coordinator.scene = scene }
}

@MainActor
final class UIScreenRenderer: NSObject, MTKViewDelegate {
    var scene: UIScene
    let ui: LayoutUI?
    let input = InputState()
    private lazy var queue = ui?.device.makeCommandQueue()
    private var last = CACurrentMediaTime()
    private var acc = 0.0
    private var held: MenuInput = []
    private var frames = 0

    init(scene: UIScene, ui: LayoutUI?) {
        self.scene = scene
        self.ui = ui
        super.init()
        held = input.menu
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        guard let ui, let queue, let pass = view.currentRenderPassDescriptor,
              let drawable = view.currentDrawable else { return }
        let now = CACurrentMediaTime()
        acc = min(acc + now - last, 0.25)
        last = now
        if let freeze = DebugCapture.freezeTicks {
            // debug capture: run straight to the requested tick, then hold
            while scene.tick < freeze { scene.step(UIInput()) }
        } else {
            while acc >= 1.0 / 60 {
                acc -= 1.0 / 60
                let menu = input.menu
                scene.step(UIInput(pressed: menu.subtracting(held), held: menu, skip: input.skipHeld))
                held = menu
            }
        }
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        pass.colorAttachments[0].loadAction = .clear
        guard let cmd = queue.makeCommandBuffer(), let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }
        let dw = Double(view.drawableSize.width), dh = Double(view.drawableSize.height)
        let scale = min(dw / 1280, dh / 720)
        let vw = 1280 * scale, vh = 720 * scale
        enc.setViewport(MTLViewport(originX: (dw - vw) / 2, originY: (dh - vh) / 2, width: vw, height: vh, znear: 0, zfar: 1))
        let layers = scene.layers
        for l in layers { l.apply(tick: scene.tick) }
        ui.encode(enc, layers: layers)
        // a scene fade over the screen (SMM2's Cmn_Fade_00, its own import)
        let fade = SceneFade.shared.layers
        if !fade.isEmpty, let fui = LayoutUI.smm2 { fui.encode(enc, layers: fade) }
        enc.endEncoding()
        cmd.present(drawable)
        cmd.commit()
        frames += 1
        if DebugCapture.requested && frames % 10 == 0 {
            cmd.waitUntilCompleted()
            DebugCapture.lastFrame = DebugCapture.image(of: drawable.texture)
        }
    }
}
