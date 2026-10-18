#if os(macOS)
import AppKit
#else
import UIKit
#endif
import CMmk
import Combine
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
        /// A game session: the SMM2 front end (opening, title, Course World, 100 Mario, course
        /// play with the SMM2 screens) when the SMM2 UI import is there, else plain play.
        case importer, library, game(GameSession)
    }

    /// The SMM2 UI import (`mmk import-smm2-ui`) the front end draws from; `MMK_UI_SMM2=off`
    /// ignores it (the library and plain play then).
    static let smm2UIDir: URL? =
        ProcessInfo.processInfo.environment["MMK_UI_SMM2"] == "off" ? nil : AppPaths.findSMM2UICache()

    @Published var screen: Screen = .library
    @Published private(set) var assets: EngineAssets?
    @Published var errorMessage: String?
    @Published var addCoursesRequested = false
    /// iOS: the Settings sheet (macOS has a Settings window).
    @Published var showSettings = false
    /// Filter the library opens with next.
    @Published var libraryStartFilter: LibraryView.Filter?
    let library = CourseLibrary()
    let saves = SaveStore()
    private var librarySink: AnyCancellable?

    var isPlaying: Bool {
        if case .game(let s) = screen { return s.front == nil || s.playingCourse }
        return false
    }

    private let env = ProcessInfo.processInfo.environment

    init() {
        AppPaths.ensureDirectories()
        if UIPreview.requested || MeshPreview.requested { return }   // debug views replace the app
        // the front end sees library changes (a scan finishing, courses added)
        librarySink = library.$entries.dropFirst().sink { [weak self] entries in
            if case .game(let s) = self?.screen { s.setLibrary(entries) }
        }
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
        } else if Self.smm2UIDir != nil && env["MMK_TITLE"] != "off" {
            // SMM2's opening, then the title. MMK_UI_SCREEN=title / title-main / title-play
            // captures start on the title; MMK_UI_SCREEN=boot or MMK_BOOT=on force the opening,
            // MMK_BOOT=off (or Settings) leaves it out
            library.scan(assets: assets)
            let forced = env["MMK_BOOT"] == "on" || env["MMK_UI_SCREEN"] == "boot"
            let boot = forced || (env["MMK_BOOT"] != "off" && !DebugCapture.requested
                                  && UserDefaults.standard.bool(forKey: SettingsKey.bootOpening))
            var start = frontStart()
            start.boot = boot
            start.titleStart = switch env["MMK_UI_SCREEN"] {
            case "title-main": 1
            case "title-play": 2
            default: 0
            }
            if !showFront(start) { showLibrary() }
        } else {
            showLibrary()
        }
        DebugCapture.scheduleIfRequested(model: self)
    }

    /// The front end's start options from the environment (captures).
    private func frontStart() -> GameSession.FrontStart {
        var s = GameSession.FrontStart()
        s.titleSelect = env["MMK_UI_SELECT"].flatMap(Int32.init) ?? -1
        s.titleCourse = env["MMK_TITLE_COURSE"].flatMap(Int32.init) ?? -1
        s.opening = env["MMK_BOOT_OPENING"]
        s.script = env["MMK_UI_SCRIPT"]
        s.titleInput = env["MMK_UI_INPUT"]
        // captures change scenes at once (MMK_FADES=1 keeps the fades)
        s.instantFades = DebugCapture.requested && env["MMK_FADES"] != "1"
        return s
    }

    /// A new front end session on screen (false: it couldn't start).
    @discardableResult
    private func showFront(_ start: GameSession.FrontStart) -> Bool {
        guard let assets else { return false }
        do {
            let s = try GameSession(assets: assets, front: start, library: library.entries)
            s.onExit = { [weak self] in self?.showLibrary() }
            screen = .game(s)
            return true
        } catch {
            errorMessage = "\(error)"
            return false
        }
    }

    /// The front end session on screen, if any.
    var frontSession: GameSession? {
        if case .game(let s) = screen, s.front != nil { return s }
        return nil
    }

    // MARK: navigation

    func showLibrary() {
        screen = .library
        library.scan(assets: assets)
    }

    /// The SMM2 title screen (needs the SMM2 UI import; the library otherwise). `start` 1 Make /
    /// Play, 2 the Play menu (on button `select`).
    func showTitle(start: UInt32 = 1, select: Int32 = -1) {
        var s = frontStart()
        s.titleStart = start
        if select >= 0 { s.titleSelect = select }
        if !showFront(s) { showLibrary() }
    }

    /// Course World with SMM2's layouts (the library without the SMM2 UI import).
    func showCourseWorld() {
        guard showFront(frontStart()), let s = frontSession else { return showLibrary() }
        s.capture("courseworld")
    }

    func showImporter() { screen = .importer }

    /// 100 Mario Challenge over the library's playable courses (needs the SMM2 UI import).
    func showChallenge() {
        guard showFront(frontStart()), let s = frontSession else { return showLibrary() }
        s.capture("challenge", difficulty: Int(env["MMK_UI_DIFFICULTY"] ?? "") ?? 0)
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

    /// Play a library course: with the SMM2 screens when the import is there (no lives,
    /// unlimited retries, as Course World), else plain play.
    func play(_ entry: CourseEntry) {
        guard let assets else { return }
        if Self.smm2UIDir != nil, showFront(frontStart()), let s = frontSession {
            if !s.playCourse(path: entry.url.path, name: entry.name, intro: true) {
                errorMessage = "Couldn't start \(entry.name): \(lastEngineError())"
                showLibrary()
            }
            return
        }
        do {
            let data = try Data(contentsOf: entry.url)
            let session = try GameSession(assets: assets, course: data, title: entry.name, courseKey: entry.info.key, saves: saves)
            session.unlimitedLives = true
            start(session)
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
        let screenshot = env["MMK_SCREENSHOT"] != nil
        if let path = env["MMK_COURSE"], Self.smm2UIDir != nil {
            // the SMM2 screens: the smoke test's MMK_SCREENSHOT runs straight into play; UI
            // captures start with the course card unless MMK_UI_INTRO=0
            guard showFront(frontStart()), let s = frontSession else { return }
            let intro = !screenshot && env["MMK_UI_INTRO"] != "0"
            if !s.playCourse(path: path, name: nil, intro: intro) {
                errorMessage = "\(path): \(lastEngineError())"
                screen = .library
                return
            }
            // MMK_START_X=<px> [MMK_START_Y=<px>]: debug, start somewhere else (smoke tests)
            if let x = env["MMK_START_X"].flatMap(Float.init) {
                mmk_engine_debug_teleport(s.engine, x, env["MMK_START_Y"].flatMap(Float.init) ?? 32)
            }
            // MMK_UI_SCRIPTED=1: the smoke test's scripted run, with the menus live (UI captures)
            if env["MMK_UI_SCRIPTED"] == "1" { s.scriptedInput = Renderer.scriptedInput }
            DebugCapture.scheduleIfRequested(model: self)
            return
        }
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
                                          saves: screenshot ? nil : saves)
            if let x = env["MMK_START_X"].flatMap(Float.init) {
                mmk_engine_debug_teleport(session.engine, x, env["MMK_START_Y"].flatMap(Float.init) ?? 32)
            }
            if let t = env["MMK_UI_PAUSE_TICKS"].flatMap(UInt64.init) { session.autoPauseTick = t }
            session.unlimitedLives = env["MMK_LIVES"] != "on"
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
        case .game: true
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
