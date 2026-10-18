#if os(macOS)
import AppKit
#else
import UIKit
#endif
import CMmk
import SwiftUI

@main
struct MMKApp: App {
    #if os(macOS)
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate
    #endif
    @StateObject private var model = AppModel()

    init() {
        SettingsKey.registerDefaults()
        #if os(macOS)
        // Running as a plain executable (swift run) rather than a bundle: become a regular,
        // focused app so the window appears in front and receives keyboard input.
        NSApplication.shared.setActivationPolicy(.regular)
        DispatchQueue.main.async { NSApplication.shared.activate(ignoringOtherApps: true) }
        #endif
    }

    var body: some Scene {
        #if os(macOS)
        Window(AppIdentity.name, id: "main") {
            content
        }
        .defaultSize(width: 1024, height: 640)
        .commands { commands }
        Settings {
            SettingsView()
        }
        #else
        // one window: the app model is shared, so a second scene would mirror the first
        WindowGroup {
            content
        }
        .commands { commands }   // iPad hardware keyboard menu
        #endif
    }

    @ViewBuilder private var content: some View {
        if MeshPreview.requested {
            MeshPreviewView().minimumWindowSize()   // MMK_MESH_PREVIEW model viewer
        } else if UIPreview.requested {
            UIPreviewView().minimumWindowSize()   // MMK_UI_LAYOUT debug view
        } else {
            RootView()
                .environmentObject(model)
                .minimumWindowSize()
        }
    }

    @CommandsBuilder private var commands: some Commands {
        CommandGroup(after: .newItem) {
            Button("Add Courses…") { model.showLibrary(); model.addCoursesRequested = true }
                .keyboardShortcut("o")
                .disabled(model.isPlaying || model.assets == nil)
            Button("Import Game Data…") { model.showImporter() }
                .disabled(model.isPlaying)
        }
    }
}

#if os(macOS)
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// One main window: closing it quits.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
#endif

/// Which screen the window shows, and the objects shared between screens.
@MainActor
final class AppModel: ObservableObject {
    enum Screen {
        case importer, library, game(GameSession), challenge(ChallengeFlow), title(TitleFlow)
    }

    @Published var screen: Screen = .library
    @Published private(set) var assets: EngineAssets?
    @Published var errorMessage: String?
    @Published var addCoursesRequested = false
    /// iOS: the Settings sheet (macOS has a Settings window).
    @Published var showSettings = false
    /// Filter the library opens with next (the title's Story Mode → Built-in).
    @Published var libraryStartFilter: LibraryView.Filter?
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
        if UIPreview.requested || MeshPreview.requested { return }   // debug views replace the app
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
        } else if LayoutUI.smm2 != nil && env["MMK_TITLE"] != "off" {
            // the SMM2 title screen first (MMK_UI_SCREEN=title-main / title-play: captures)
            let start: TitleScene.State = switch env["MMK_UI_SCREEN"] {
            case "title-main": .main
            case "title-play": .category
            default: .top
            }
            library.scan(assets: assets)
            showTitle(start: start)
            if case .title(let flow) = screen, let i = env["MMK_UI_SELECT"].flatMap(Int.init) {
                flow.scene.debugSelectCategory(i)
            }
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

    /// The SMM2 title screen (needs the SMM2 UI import; the library otherwise).
    func showTitle(start: TitleScene.State = .main) {
        guard let ui = LayoutUI.smm2, let flow = TitleFlow(ui: ui, assets: assets, start: start) else {
            showLibrary()
            return
        }
        flow.onChoice = { [weak self] choice in self?.titleChoice(choice) }
        screen = .title(flow)
    }

    private func titleChoice(_ choice: TitleChoice) {
        switch choice {
        case .storyMode:
            // Story Mode's courses are the built-in ones (the jobs map isn't there yet)
            libraryStartFilter = .builtIn
            showLibrary()
        case .courseWorld, .coursebot:
            showLibrary()
        case .hundredMario:
            showChallenge()
        }
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

    /// After an import (macOS) or a cache copied in (iOS): open the best cache found.
    func importFinished() {
        guard let url = AppPaths.findAssetCache() else {
            #if os(macOS)
            errorMessage = "The import finished but no asset cache was found in \(AppPaths.assetsRoot.path)."
            #else
            errorMessage = "No asset cache was found in \(AppPaths.assetsRoot.path). Copy the folder made by the importer on your Mac (for example smm2-3.0.3) there."
            #endif
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
    #if os(macOS)
    @AppStorage(SettingsKey.fullscreen) private var fullscreen = false
    @Environment(\.openSettings) private var openSettings
    #endif

    var body: some View {
        Group {
            switch model.screen {
            case .importer: ImportView()
            case .library: libraryScreen
            case .game(let session): GameView(session: session).id(session.id)
            case .challenge(let flow): ChallengeView(flow: flow).id(flow.id)
            case .title(let flow): TitleView(flow: flow).id(flow.id)
            }
        }
        .alert("Error", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
        #if os(macOS)
        .background(WindowAccessor(fullscreen: $fullscreen))
        .onAppear {
            if DebugCapture.requested && ProcessInfo.processInfo.environment["MMK_UI_SCREEN"] == "settings" { openSettings() }
        }
        #else
        // game screens use the whole display, with the on-screen controls on top
        .overlay { if model.showsGameScreen { TouchControls().ignoresSafeArea() } }
        .ignoresSafeArea(edges: model.showsGameScreen ? .all : [])
        .statusBarHidden(model.showsGameScreen)
        .persistentSystemOverlays(model.showsGameScreen ? .hidden : .automatic)
        .sheet(isPresented: $model.showSettings) { SettingsView() }
        .onAppear {
            let env = ProcessInfo.processInfo.environment
            if DebugCapture.requested && env["MMK_UI_SCREEN"] == "settings" { model.showSettings = true }
            // MMK_ORIENTATION=landscape / portrait: rotate at launch (simulator captures)
            if let o = env["MMK_ORIENTATION"], let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene {
                scene.requestGeometryUpdate(.iOS(interfaceOrientations: o == "portrait" ? .portrait : .landscapeRight))
            }
        }
        #endif
    }

    @ViewBuilder private var libraryScreen: some View {
        #if os(macOS)
        LibraryView(library: model.library, saves: model.saves)
        #else
        NavigationStack { LibraryView(library: model.library, saves: model.saves) }   // (for its toolbar)
        #endif
    }
}

#if os(iOS)
extension AppModel {
    /// The game, a 100 Mario run or the title: full screen, touch controls shown.
    var showsGameScreen: Bool {
        switch screen {
        case .game, .challenge, .title: true
        case .importer, .library: false
        }
    }
}
#endif

#if os(macOS)
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
#endif
