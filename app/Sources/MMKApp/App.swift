import AppKit
import CMmk
import SwiftUI

@main
struct MMKApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate
    @StateObject private var model = AppModel()

    init() {
        SettingsKey.registerDefaults()
        // Running as a plain executable (swift run) rather than a bundle: become a regular,
        // focused app so the window appears in front and receives keyboard input.
        NSApplication.shared.setActivationPolicy(.regular)
        DispatchQueue.main.async { NSApplication.shared.activate(ignoringOtherApps: true) }
    }

    var body: some Scene {
        Window(AppIdentity.name, id: "main") {
            if UIPreview.requested {
                UIPreviewView().frame(minWidth: 768, minHeight: 432)   // MMK_UI_LAYOUT debug view
            } else {
                RootView()
                    .environmentObject(model)
                    .frame(minWidth: 768, minHeight: 432)
            }
        }
        .defaultSize(width: 1024, height: 640)
        .commands {
            CommandGroup(after: .newItem) {
                Button("Add Courses…") { model.showLibrary(); model.addCoursesRequested = true }
                    .keyboardShortcut("o")
                    .disabled(model.isPlaying || model.assets == nil)
                Button("Import Game Data…") { model.showImporter() }
                    .disabled(model.isPlaying)
            }
        }
        Settings {
            SettingsView()
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// One main window: closing it quits.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

/// Which screen the window shows, and the objects shared between screens.
@MainActor
final class AppModel: ObservableObject {
    enum Screen {
        case importer, library, game(GameSession), challenge(ChallengeFlow)
    }

    @Published var screen: Screen = .library
    @Published private(set) var assets: EngineAssets?
    @Published var errorMessage: String?
    @Published var addCoursesRequested = false
    let library = CourseLibrary()
    let saves = SaveStore()

    var isPlaying: Bool {
        switch screen {
        case .game, .challenge: true
        default: false
        }
    }

    private let env = ProcessInfo.processInfo.environment

    init() {
        AppPaths.ensureDirectories()
        if UIPreview.requested { return }   // MMK_UI_LAYOUT: the layout preview replaces the app
        // Smoke tests and debugging: MMK_SCREENSHOT / MMK_COURSE skip the menus and go straight
        // into the game (see Renderer for MMK_SCREENSHOT, MMK_START_X/Y below).
        if env["MMK_SCREENSHOT"] != nil || env["MMK_COURSE"] != nil {
            startDirect()
            return
        }
        if let url = AppPaths.findAssetCache(), let a = try? EngineAssets.open(url) {
            assets = a
        } else if UserDefaults.standard.bool(forKey: SettingsKey.usePlaceholder) || AppPaths.forcePlaceholder {
            assets = .placeholder()
        }
        if assets == nil {
            screen = .importer
        } else {
            showLibrary()
        }
        DebugCapture.scheduleIfRequested(model: self)
    }

    // MARK: navigation

    func showLibrary() {
        screen = .library
        library.scan(assets: assets)
    }

    func showImporter() { screen = .importer }

    /// 100 Mario Challenge over the library's playable courses.
    func showChallenge() {
        guard let assets else { return }
        let flow = ChallengeFlow(assets: assets, library: library.entries, saves: saves)
        flow.onExit = { [weak self] in self?.showLibrary() }
        screen = .challenge(flow)
    }

    func usePlaceholder() {
        UserDefaults.standard.set(true, forKey: SettingsKey.usePlaceholder)
        if assets == nil { assets = .placeholder() }
        showLibrary()
    }

    func importFinished() {
        guard let url = AppPaths.findAssetCache() else {
            errorMessage = "The import finished but no asset cache was found in \(AppPaths.assetsRoot.path)."
            return
        }
        do {
            assets = try EngineAssets.open(url)
            UserDefaults.standard.set(false, forKey: SettingsKey.usePlaceholder)
            showLibrary()
        } catch {
            errorMessage = "\(error)"
        }
    }

    func play(_ entry: CourseEntry) {
        guard let assets else { return }
        do {
            let data = try Data(contentsOf: entry.url)
            start(try GameSession(assets: assets, course: data, title: entry.name, courseKey: entry.info.key, saves: saves))
        } catch {
            errorMessage = "Couldn't start \(entry.name): \(error)"
        }
    }

    func playDemo() {
        guard let assets else { return }
        do {
            start(try GameSession(assets: assets, course: nil, title: "Demo strip", courseKey: nil, saves: nil))
        } catch {
            errorMessage = "\(error)"
        }
    }

    private func start(_ session: GameSession) {
        session.onExit = { [weak self] in self?.showLibrary() }
        screen = .game(session)
    }

    /// MMK_COURSE=<file> and/or MMK_SCREENSHOT=<png>: open the asset cache and play at once.
    private func startDirect() {
        if let url = AppPaths.findAssetCache() {
            do { assets = try EngineAssets.open(url) } catch { errorMessage = "\(error)" }
        } else if !AppPaths.forcePlaceholder {
            FileHandle.standardError.write("no asset cache found (looked in \(AppPaths.assetCandidates().map(\.path))); using placeholder art\n".data(using: .utf8)!)
        }
        let assets = assets ?? .placeholder()
        self.assets = assets
        do {
            var data: Data?
            var title = "Demo strip"
            var key: String?
            if let path = env["MMK_COURSE"] {
                let d = try Data(contentsOf: URL(fileURLWithPath: path))
                data = d
                title = URL(fileURLWithPath: path).lastPathComponent
                if case .success(let info) = CourseInfo.read(d, assets: assets) {
                    title = info.name
                    key = info.key
                }
            }
            let session = try GameSession(assets: assets, course: data, title: title, courseKey: key,
                                          saves: env["MMK_SCREENSHOT"] == nil ? saves : nil)
            // MMK_START_X=<px> [MMK_START_Y=<px>]: debug, start somewhere else (smoke tests)
            if let x = env["MMK_START_X"].flatMap(Float.init) {
                mmk_engine_debug_teleport(session.engine, x, env["MMK_START_Y"].flatMap(Float.init) ?? 32)
            }
            if let t = env["MMK_UI_PAUSE_TICKS"].flatMap(UInt64.init) { session.autoPauseTick = t }
            // MMK_UI_SCRIPTED=1: the smoke test's scripted run, with the menus live (UI captures)
            if env["MMK_UI_SCRIPTED"] == "1" { session.scriptedInput = Renderer.scriptedInput }
            start(session)
        } catch {
            errorMessage = "\(env["MMK_COURSE"] ?? "course"): \(error)"
            screen = .library
        }
        DebugCapture.scheduleIfRequested(model: self)
    }
}

struct RootView: View {
    @EnvironmentObject private var model: AppModel
    @AppStorage(SettingsKey.fullscreen) private var fullscreen = false
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        Group {
            switch model.screen {
            case .importer: ImportView()
            case .library: LibraryView(library: model.library, saves: model.saves)
            case .game(let session): GameView(session: session).id(session.id)
            case .challenge(let flow): ChallengeView(flow: flow).id(flow.id)
            }
        }
        .alert("Error", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
        .background(WindowAccessor(fullscreen: $fullscreen))
        .onAppear {
            if DebugCapture.requested && ProcessInfo.processInfo.environment["MMK_UI_SCREEN"] == "settings" { openSettings() }
        }
    }
}

/// Keeps the window's full-screen state and the `display.fullscreen` setting in sync, both ways.
private struct WindowAccessor: NSViewRepresentable {
    @Binding var fullscreen: Bool

    final class Coordinator {
        weak var window: NSWindow?
        /// Last state applied to the window (so view updates during a transition don't re-toggle).
        var applied = false
        var observers: [NSObjectProtocol] = []
        deinit { observers.forEach(NotificationCenter.default.removeObserver) }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSView {
        let v = NSView()
        let coord = context.coordinator
        let binding = $fullscreen
        DispatchQueue.main.async {
            guard let w = v.window else { return }
            coord.window = w
            let c = NotificationCenter.default
            coord.observers = [
                c.addObserver(forName: NSWindow.didEnterFullScreenNotification, object: w, queue: .main) { _ in
                    coord.applied = true
                    if !binding.wrappedValue { binding.wrappedValue = true }
                },
                c.addObserver(forName: NSWindow.didExitFullScreenNotification, object: w, queue: .main) { _ in
                    coord.applied = false
                    if binding.wrappedValue { binding.wrappedValue = false }
                },
            ]
            coord.applied = w.styleMask.contains(.fullScreen)
            Self.apply(binding.wrappedValue, coord)
        }
        return v
    }

    func updateNSView(_ v: NSView, context: Context) {
        Self.apply(fullscreen, context.coordinator)
    }

    private static func apply(_ want: Bool, _ c: Coordinator) {
        guard let w = c.window, want != c.applied else { return }
        c.applied = want
        if w.styleMask.contains(.fullScreen) != want { w.toggleFullScreen(nil) }
    }
}
