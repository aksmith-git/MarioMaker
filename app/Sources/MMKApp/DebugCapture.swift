import Metal
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Debug: screenshot the app's own window without Screen Recording permission.
///
///     MMK_UI_SCREENSHOT=<png> [MMK_UI_SCREENSHOT_DELAY=<s>, default 2] [MMK_UI_SCREEN=<screen>]
///
/// Screens: import, settings, and the front end's (`mmk-front`, the names of `mmk-play
/// --front-screen`): boot (MMK_BOOT_OPENING=<InDefault…> picks the opening), title, title-main,
/// title-play (MMK_UI_SELECT=<n> picks the Play menu button), courseworld, courseworld-list,
/// storymode, challenge (MMK_UI_DIFFICULTY=<0-3>), challenge-map, challenge-walk
/// (MMK_UI_CLEARED=<n>, default 3), challenge-gameover, challenge-ending, challenge-pause /
/// -clear / -skip / -card; with MMK_COURSE: course-intro (default), course-play
/// (MMK_UI_INTRO=0), course-pause (MMK_UI_PAUSE_TICKS), course-clear, course-gameover,
/// course-timeup, course-retire, course-settings, course-death.
/// MMK_UI_TICKS=<n>: run n front-end ticks at once and hold the screen there (course screens:
/// until the screen has been up n ticks), so captures are deterministic and match mmk-play's.
/// MMK_UI_INPUT=<tick>:<button>,… presses title buttons at title ticks, MMK_UI_SCRIPT=<tick>:
/// <button>,… menu buttons at global UI ticks; MMK_TITLE_COURSE=<n> picks the demo course.
///
/// Waits, renders the main window's content (or the Settings window) into a PNG and quits.
/// Window snapshots can't see Metal layers, so while this is set the renderer keeps a copy of its
/// latest frame and it is drawn underneath. Combine with MMK_COURSE and MMK_UI_PAUSE_TICKS=<n>
/// to capture the pause menu over the game. On iOS the key window is captured the same way
/// (`xcrun simctl io booted screenshot` also works there, Metal included).
@MainActor
enum DebugCapture {
    static let requested = ProcessInfo.processInfo.environment["MMK_UI_SCREENSHOT"] != nil
    static weak var gameView: PlatformView?
    static var lastFrame: CGImage?
    static let freezeTicks: Int? = requested ? ProcessInfo.processInfo.environment["MMK_UI_TICKS"].flatMap(Int.init) : nil

    /// Read back a BGRA texture (frame copies for captures).
    static func image(of tex: MTLTexture) -> CGImage? {
        let w = tex.width, h = tex.height
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        tex.getBytes(&bytes, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        guard let ctx = CGContext(data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: info.rawValue)
        else { return nil }
        return ctx.makeImage()
    }

    /// Run `body` once the library scan is over.
    private static func afterScan(_ model: AppModel, _ body: @escaping () -> Void) {
        if model.library.scanning {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { afterScan(model, body) }
        } else {
            body()
        }
    }

    static func scheduleIfRequested(model: AppModel) {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["MMK_UI_SCREENSHOT"] else { return }
        let delay = env["MMK_UI_SCREENSHOT_DELAY"].flatMap(Double.init) ?? 2
        let settings = env["MMK_UI_SCREEN"] == "settings"
        if env["MMK_UI_SCREEN"] == "import" { model.showImporter() }
        let screen = env["MMK_UI_SCREEN"] ?? ""
        if let s = model.frontSession {
            // the front end's screens, as `mmk-play --front-screen` captures them: MMK_UI_TICKS
            // runs that many ticks at once and holds the screen there
            if env["MMK_COURSE"] != nil {
                let name = screen.hasPrefix("course-") ? screen
                    : env["MMK_UI_PAUSE_TICKS"] != nil ? "course-pause"
                    : env["MMK_UI_INTRO"] == "0" ? "course-play" : "course-intro"
                s.debugCourse(name)
                if let t = freezeTicks { s.runAndFreeze(t, courseScreen: name != "course-play") }
            } else if screen.hasPrefix("challenge") || screen.hasPrefix("courseworld") || screen == "storymode" || screen == "account" {
                // over the library: after its scan
                afterScan(model) {
                    s.setLibrary(model.library.entries)
                    s.capture(screen, difficulty: env["MMK_UI_DIFFICULTY"].flatMap(Int.init) ?? 0,
                              cleared: env["MMK_UI_CLEARED"].flatMap(Int.init) ?? 3)
                    if let t = freezeTicks { s.runAndFreeze(t, courseScreen: false) }
                }
            } else {
                if ["boot", "title", "title-main", "title-play"].contains(screen) { s.capture(screen) }
                if let t = freezeTicks { s.runAndFreeze(t, courseScreen: false) }
            }
        }
        // (RootView opens the Settings window / sheet for MMK_UI_SCREEN=settings)
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
            #if os(macOS)
            let windows = NSApp.windows.filter { $0.isVisible && $0.contentView != nil }
            let target = settings ? windows.first { $0.title != AppIdentity.name } ?? windows.first
                                  : windows.first { $0.title == AppIdentity.name } ?? windows.first
            #else
            _ = settings   // the sheet is part of the key window
            let target = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                .flatMap(\.windows).first { $0.isKeyWindow }
            #endif
            guard let w = target, let png = capture(w) else {
                FileHandle.standardError.write("MMK_UI_SCREENSHOT: nothing to capture\n".data(using: .utf8)!)
                exit(1)
            }
            try? png.write(to: URL(fileURLWithPath: path))
            exit(0)
        }
    }

    #if os(iOS)
    private static func capture(_ w: UIWindow) -> Data? {
        let game = gameView.flatMap { $0.window === w ? $0 : nil }
        return UIGraphicsImageRenderer(bounds: w.bounds).pngData { ctx in
            UIColor.black.setFill()
            ctx.fill(w.bounds)
            if let game, let frame = lastFrame {
                UIImage(cgImage: frame).draw(in: game.convert(game.bounds, to: w))
            }
            // the UI over it (the Metal layer itself doesn't draw into a snapshot)
            game?.isHidden = true
            w.drawHierarchy(in: w.bounds, afterScreenUpdates: true)
            game?.isHidden = false
        }
    }
    #else

    private static func capture(_ w: NSWindow) -> Data? {
        guard let content = w.contentView else { return nil }
        let game = gameView.flatMap { $0.window === w ? $0 : nil }
        // the frame view (title bar, toolbar) paints an opaque background, so with a game showing
        // only the content view is captured
        let view = game == nil ? content.superview ?? content : content
        game?.isHidden = true
        defer { game?.isHidden = false }
        guard let ui = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: ui)
        guard let out = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: ui.pixelsWide, pixelsHigh: ui.pixelsHigh,
                                         bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                         colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return nil }
        out.size = view.bounds.size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: out)
        NSColor.black.setFill()
        view.bounds.fill()
        if let game, let frame = lastFrame {
            // game view rect in window coordinates (origin bottom-left) relative to `view`
            let r = game.convert(game.bounds, to: nil)
            let origin = view.convert(view.bounds, to: nil).origin
            NSImage(cgImage: frame, size: r.size).draw(in: r.offsetBy(dx: -origin.x, dy: -origin.y))
        }
        ui.draw(in: view.bounds, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        NSGraphicsContext.restoreGraphicsState()
        return out.representation(using: .png, properties: [:])
    }
    #endif
}
