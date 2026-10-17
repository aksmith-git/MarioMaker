import AppKit

/// Debug: screenshot the app's own window without Screen Recording permission.
///
///     MMK_UI_SCREENSHOT=<png> [MMK_UI_SCREENSHOT_DELAY=<s>, default 2] [MMK_UI_SCREEN=import|settings]
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

    static func scheduleIfRequested(model: AppModel) {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["MMK_UI_SCREENSHOT"] else { return }
        let delay = env["MMK_UI_SCREENSHOT_DELAY"].flatMap(Double.init) ?? 2
        let settings = env["MMK_UI_SCREEN"] == "settings"
        if env["MMK_UI_SCREEN"] == "import" { model.showImporter() }
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
