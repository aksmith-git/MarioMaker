import AppKit
import Metal

/// Debug: screenshot the app's own window without Screen Recording permission.
///
///     MMK_UI_SCREENSHOT=<png> [MMK_UI_SCREENSHOT_DELAY=<s>, default 2] [MMK_UI_SCREEN=<screen>]
///
/// Screens: import, settings, challenge (difficulty select), challenge-intro / challenge-map (an
/// Easy New Game: MapIn, Mario's walk, the intro card), challenge-walk (the map after a clear,
/// MMK_UI_CLEARED=<n> courses done, default 3), challenge-gameover, challenge-ending, and over a
/// run's first course: challenge-pause, challenge-clear, challenge-skip, challenge-card (the intro
/// card after a death). MMK_UI_DIFFICULTY=<0-3> picks the difficulty (default 0).
/// MMK_UI_TICKS=<n>: SMM1 layout screens run straight to tick n and hold there (in-course overlays:
/// n ticks after they appear), so captures are deterministic.
/// SMM2 title (at launch; see SMM2Title.swift): title (prompt), title-main (Make / Play),
/// title-play (Play menu; MMK_UI_SELECT=<n> picks the button); MMK_UI_INPUT=<tick>:<button>,…
/// presses menu buttons at scene ticks; MMK_TITLE_COURSE=<n> picks the demo course.
/// SMM2 in-course screens (with MMK_COURSE; SMM2Course.swift): the course card shows first
/// (MMK_UI_INTRO=0 skips it); MMK_UI_PAUSE_TICKS=<n> pauses; MMK_UI_SCREEN=course-clear,
/// course-gameover, course-timeup, course-retire, course-death (the card after a death) shows
/// that screen MMK_UI_COURSE_AT=<s> seconds in (default half the delay).
///
/// Waits, renders the main window's content (or the Settings window) into a PNG and quits.
/// Window snapshots can't see Metal layers, so while this is set the renderer keeps a copy of its
/// latest frame and it is drawn underneath. Combine with MMK_COURSE and MMK_UI_PAUSE_TICKS=<n>
/// to capture the pause menu over the game.
@MainActor
enum DebugCapture {
    static let requested = ProcessInfo.processInfo.environment["MMK_UI_SCREENSHOT"] != nil
    static weak var gameView: NSView?
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

    static func scheduleIfRequested(model: AppModel) {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["MMK_UI_SCREENSHOT"] else { return }
        let delay = env["MMK_UI_SCREENSHOT_DELAY"].flatMap(Double.init) ?? 2
        let settings = env["MMK_UI_SCREEN"] == "settings"
        if env["MMK_UI_SCREEN"] == "import" { model.showImporter() }
        if let screen = env["MMK_UI_SCREEN"], screen.hasPrefix("challenge") {
            // after the library scan: the difficulty select, or an Easy run's first intro card
            DispatchQueue.main.asyncAfter(deadline: .now() + delay / 2) {
                model.showChallenge()
                guard case .challenge(let flow) = model.screen else { return }
                let d = env["MMK_UI_DIFFICULTY"].flatMap(Int.init) ?? 0
                switch screen {
                case "challenge": flow.debugSelect(d)
                case "challenge-intro", "challenge-map": flow.start(d, resume: false)
                case "challenge-walk": flow.debugMap(d, entry: .afterClear, cleared: env["MMK_UI_CLEARED"].flatMap(Int.init) ?? 3)
                case "challenge-gameover": flow.debugGameOver()
                case "challenge-ending": flow.debugEnding(d, library: model.library.entries)
                case "challenge-pause", "challenge-clear", "challenge-skip", "challenge-card":
                    flow.start(d, resume: false)
                    flow.play()
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        guard case .playing(let s) = flow.stage else { return }
                        switch screen {
                        case "challenge-pause": s.pause()
                        case "challenge-clear": s.debugClear()
                        case "challenge-skip": s.debugSkipHeld = true
                        default: s.debugCard()
                        }
                    }
                default: break
                }
            }
        }
        if let screen = env["MMK_UI_SCREEN"], screen.hasPrefix("course-") {
            // SMM2 in-course screens over MMK_COURSE (after MMK_UI_COURSE_AT seconds, default
            // half the delay)
            let at = env["MMK_UI_COURSE_AT"].flatMap(Double.init) ?? delay / 2
            DispatchQueue.main.asyncAfter(deadline: .now() + at) {
                if case .game(let s) = model.screen { s.debugSMM2(screen) }
            }
        }
        // (RootView opens the Settings window for MMK_UI_SCREEN=settings)
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
            let windows = NSApp.windows.filter { $0.isVisible && $0.contentView != nil }
            let target = settings ? windows.first { $0.title != AppIdentity.name } ?? windows.first
                                  : windows.first { $0.title == AppIdentity.name } ?? windows.first
            guard let w = target, let png = capture(w) else {
                FileHandle.standardError.write("MMK_UI_SCREENSHOT: nothing to capture\n".data(using: .utf8)!)
                exit(1)
            }
            try? png.write(to: URL(fileURLWithPath: path))
            exit(0)
        }
    }

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
}
