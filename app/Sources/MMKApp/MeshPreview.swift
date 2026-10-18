import CMmk
import ImageIO
import MetalKit
import SwiftUI

/// Debug model viewer for the NSMBU / 3D World meshes, side by side on a ground line.
///
///     MMK_MESH_PREVIEW=<item>[,<item>…]
///         <mesh>[#<model>][@<anim>]          a mesh file of the cache, e.g. 3W_Enemy_kuribo@Walk
///                                            (model: name or index, default 0; anim: by name)
///         <style>:<sim model>:<frame>        what the engine draws for a sim sprite, e.g.
///                                            3W:M1_Player_SpMarioMdl:walk.0 (players with their
///                                            head / face / hands, animation from the frame name)
///     [MMK_MESH_YAW=<deg>] [MMK_MESH_PITCH=<deg>]   turn the raw meshes (default 0: side view)
///     [MMK_MESH_FLIP=1]                      sim sprites face left
///     [MMK_MESH_TICK=<n>]                    animation time in frames (default: running clock,
///                                            or the screenshot tick)
///     [MMK_ASSETS=<cache dir>]               asset cache (default as the app)
///     [MMK_SCREENSHOT=<png> [MMK_SCREENSHOT_TICKS=<n>, default 60]]
///                                            render at 1280×720, one tick per frame, save, quit
enum MeshPreview {
    static let env = ProcessInfo.processInfo.environment
    static var requested: Bool { env["MMK_MESH_PREVIEW"] != nil }
}

struct MeshPreviewView: MetalViewRepresentable {
    func makeCoordinator() -> MeshPreviewRenderer { MeshPreviewRenderer() }

    func makeView(context: Context) -> MTKView {
        let view = MTKView()
        view.device = context.coordinator.device
        view.colorPixelFormat = .bgra8Unorm_srgb
        view.depthStencilPixelFormat = Renderer.depthFormat
        view.clearDepth = 1
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
final class MeshPreviewRenderer: NSObject, MTKViewDelegate {
    let device = MTLCreateSystemDefaultDevice()!
    private lazy var queue = device.makeCommandQueue()!
    private var assets: EngineAssets?
    private var meshes: MeshRenderer?
    private var gpuTextures: [UInt32: MTLTexture?] = [:]

    private enum Item {
        case raw(mesh: Int32, model: UInt32, anim: Int32, bounds: [Float])
        case sprite(style: String, model: String, frame: String)
    }
    private var items: [Item] = []
    private var tick = 0
    private var start = CACurrentMediaTime()
    let screenshot = MeshPreview.env["MMK_SCREENSHOT"]
    private let screenshotTicks = Int(MeshPreview.env["MMK_SCREENSHOT_TICKS"] ?? "") ?? 60

    override init() {
        super.init()
        let env = MeshPreview.env
        guard let url = AppPaths.findAssetCache() else { fail("no asset cache found (set MMK_ASSETS)") }
        do { assets = try EngineAssets.open(url) } catch { fail("\(url.path): \(error)") }
        guard let a = assets?.handle else { fail("no assets") }
        do {
            meshes = try MeshRenderer(device: device, assets: a, colorFormat: .bgra8Unorm_srgb, depthFormat: Renderer.depthFormat)
        } catch {
            fail("mesh pipeline: \(error)")
        }
        meshes?.texture = { [weak self] i in self?.texture(i) }
        for spec in (env["MMK_MESH_PREVIEW"] ?? "").split(separator: ",").map(String.init) where !spec.isEmpty {
            let parts = spec.split(separator: ":").map(String.init)
            if parts.count == 3 {
                items.append(.sprite(style: parts[0], model: parts[1], frame: parts[2]))
                continue
            }
            var name = spec, animName: String?, modelName: String?
            if let at = name.firstIndex(of: "@") {
                animName = String(name[name.index(after: at)...])
                name = String(name[..<at])
            }
            if let hash = name.firstIndex(of: "#") {
                modelName = String(name[name.index(after: hash)...])
                name = String(name[..<hash])
            }
            let mesh = mmk_mesh_find(a, name)
            if mesh < 0 { fail("no mesh '\(name)' in \(url.path)") }
            var model: Int32 = 0
            if let m = modelName { model = Int32(m) ?? mmk_mesh_model_index(a, mesh, m) }
            if model < 0 { fail("\(name): no model '\(modelName ?? "")'") }
            var anim: Int32 = -1
            if let n = animName {
                anim = mmk_mesh_anim(a, mesh, n)
                if anim < 0 {
                    var names: [String] = []
                    var buf = [CChar](repeating: 0, count: 256)
                    for i in 0..<mmk_mesh_anim_count(a, mesh) {
                        _ = mmk_mesh_anim_name(a, mesh, i, &buf, buf.count)
                        names.append(String(cString: buf))
                    }
                    FileHandle.standardError.write("MMK_MESH_PREVIEW: \(name) has no animation '\(n)'; has: \(names.joined(separator: " "))\n".data(using: .utf8)!)
                }
            }
            var info = MmkMeshModel()
            guard mmk_mesh_model(a, mesh, Int(model), &info) else { fail("\(name): bad model \(model)") }
            let b = info.bounds
            items.append(.raw(mesh: mesh, model: UInt32(model), anim: anim, bounds: [b.0, b.1, b.2, b.3, b.4, b.5]))
        }
        if items.isEmpty { fail("nothing to show") }
    }

    private func fail(_ msg: String) -> Never {
        FileHandle.standardError.write("MMK_MESH_PREVIEW: \(msg)\n".data(using: .utf8)!)
        exit(1)
    }

    private func texture(_ i: UInt32) -> MTLTexture? {
        if let t = gpuTextures[i] { return t }
        var info = MmkTextureInfo()
        let t = assets.flatMap { mmk_assets_texture($0.handle, Int(i), &info) ? GPUTexture.make(device, info) : nil }
        gpuTextures[i] = t
        return t
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    /// Rotation for raw meshes: pitch (about x) after yaw (about y), degrees.
    private func turn() -> (SIMD3<Float>, SIMD3<Float>, SIMD3<Float>) {
        let yaw = Float(MeshPreview.env["MMK_MESH_YAW"].flatMap(Double.init) ?? 0) * .pi / 180
        let pitch = Float(MeshPreview.env["MMK_MESH_PITCH"].flatMap(Double.init) ?? 0) * .pi / 180
        let (sy, cy, sp, cp) = (sin(yaw), cos(yaw), sin(pitch), cos(pitch))
        // R = Rx(pitch) · Ry(yaw), rows
        return (SIMD3(cy, 0, sy), SIMD3(sp * sy, cp, -sp * cy), SIMD3(-cp * sy, sp, cp * cy))
    }

    func draw(in view: MTKView) {
        guard let meshes, let a = assets?.handle, let pass = view.currentRenderPassDescriptor,
              let drawable = view.currentDrawable else { return }
        tick = screenshot != nil ? tick + 1 : Int((CACurrentMediaTime() - start) * 60)
        let time = Float(MeshPreview.env["MMK_MESH_TICK"].flatMap(Int.init) ?? tick)
        let flip = MeshPreview.env["MMK_MESH_FLIP"] == "1"

        // lay the items out left to right on y = 0, 16 px apart
        var draws: [MmkMeshDraw] = []
        var x: Float = 0
        var top: Float = 32
        let (r0, r1, r2) = turn()
        for item in items {
            switch item {
            case let .raw(mesh, model, anim, b):
                let w = max(b[3] - b[0], 16)
                top = max(top, b[4])
                let ox = x + w / 2 - (b[0] + b[3]) / 2
                var d = MmkMeshDraw()
                d.mesh = mesh
                d.model = model
                d.anim = anim
                d.time = time
                d.transform = (r0.x, r0.y, r0.z, ox, r1.x, r1.y, r1.z, 0, r2.x, r2.y, r2.z, 0)
                d.layer = UInt32(draws.count)
                draws.append(d)
                x += w + 16
            case let .sprite(style, model, frame):
                var ptr: UnsafePointer<MmkMeshDraw>?
                let n = mmk_mesh_sprite_draws(a, style, model, frame, x + 24, 0, 1, flip, time, &ptr)
                if n == 0 { FileHandle.standardError.write("MMK_MESH_PREVIEW: \(style):\(model):\(frame) has no mesh\n".data(using: .utf8)!) }
                let layer = UInt32(draws.count)
                for var d in UnsafeBufferPointer(start: ptr, count: n) {
                    d.layer = layer
                    draws.append(d)
                }
                top = max(top, 40)
                x += 48 + 16
            }
        }
        let worldW = max(x - 16, 32), worldH = top + 16
        // fit with a margin, keeping the 16:9 aspect
        let dw = Float(drawable.texture.width), dh = Float(drawable.texture.height)
        let scale = min(dw / (worldW + 32), dh / (worldH + 24))
        let viewW = dw / scale, viewH = dh / scale
        let left = -(viewW - worldW) / 2, bottom = -(viewH - worldH) / 2 - 8
        let xform = SIMD4<Float>(2 / viewW, 2 / viewH, -1 - 2 * left / viewW, -1 - 2 * bottom / viewH)

        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0.30, green: 0.36, blue: 0.46, alpha: 1)
        pass.colorAttachments[0].loadAction = .clear
        guard let cmd = queue.makeCommandBuffer(), let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }
        let frame = draws.withUnsafeBufferPointer { meshes.prepare($0) }
        let cam = MeshRenderer.Camera(viewProj: Renderer.orthoMatrix(xform), layered: true, flipY: false,
                                      light: MeshRenderer.neutralLight)
        meshes.encode(enc, frame: frame, range: 0..<frame.posed.count, camera: cam)
        enc.endEncoding()
        if let path = screenshot, tick >= screenshotTicks {
            cmd.commit()
            cmd.waitUntilCompleted()
            if let img = DebugCapture.image(of: drawable.texture),
               let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil) {
                CGImageDestinationAddImage(dest, img, nil)
                CGImageDestinationFinalize(dest)
            }
            exit(0)
        }
        cmd.present(drawable)
        cmd.commit()
    }
}
