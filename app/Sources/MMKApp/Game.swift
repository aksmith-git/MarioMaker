import CMmk
import QuartzCore
import SwiftUI

/// One engine and what drives it, drawn by the Renderer, which calls `frame(now:)` once per
/// display frame:
/// - with the SMM2 UI import, the front end (`mmk-front` through `mmk_front_*`): the opening,
///   title, Course World, 100 Mario and the in-course screens, the same screens as `mmk-play`.
///   The front end loads, steps and restarts courses on this session's engine; the Renderer draws
///   the engine's picture where the frame has the course view, then the frame's layouts
///   (`encodeLayouts`);
/// - without it, plain play of one course: a fixed 60 Hz step, the drawn HUD strip and the SwiftUI
///   pause / clear / game over cards below.
@MainActor
final class GameSession: ObservableObject, Identifiable {
    enum Phase: Equatable { case playing, paused, cleared, gameOver }

    enum PauseItem: String, CaseIterable {
        case resume = "Continue"
        case restart = "Restart course"
        case exit = "Exit to library"
    }

    var pauseItems: [PauseItem] { PauseItem.allCases }

    nonisolated let id = UUID()
    let assets: EngineAssets
    let engine: OpaquePointer
    let title: String
    /// Save-data key (plain play; nil for the demo strip).
    let courseKey: String?
    let input = InputState()
    /// Sound output (nil when muted or the cache has no audio).
    private var audio: AudioOutput?

    @Published private(set) var phase: Phase = .playing
    @Published var pauseSelection = 0
    /// Clear time of the last clear (ticks) and whether it was a new best.
    @Published private(set) var clearTicks: Int?
    @Published private(set) var newBest = false
    /// The front end's message (toast) over the screen.
    @Published private(set) var toast: String?

    /// Called when the player leaves the session (plain play: exit, after a clear or game over;
    /// front end: leaving a course started with `playCourse`).
    var onExit: (() -> Void)?
    /// Debug/smoke tests: replaces the player's input; given the engine tick.
    var scriptedInput: ((UInt64) -> MmkInput)?
    /// No HUD strip (the front end draws SMM2's).
    var hideHud = false
    /// Debug: pause automatically after this many ticks (screenshots of the pause menu).
    var autoPauseTick: UInt64?
    /// No lives: deaths restart the course without a game over.
    var unlimitedLives = false

    private weak var saves: SaveStore?
    private static let tick = 1.0 / 60.0
    private var lastTime = CACurrentMediaTime()
    private var accumulator = 0.0
    private var engineTick: UInt64 = 0
    private var attemptTicks = 0
    private var goalTicks: Int?
    private var wasDying = false
    private var heldMenu: MenuInput = []
    private var heldStart = true      // the key that started the game must be released first
    private var phaseFrames = 0
    private var resignObserver: NSObjectProtocol?

    // MARK: front end state

    /// The front end (nil: plain play).
    private(set) var front: OpaquePointer?
    /// Captures: no more ticks (the screen holds where `MMK_UI_TICKS` left it).
    var frozen = false
    /// The last frame showed the course view.
    private(set) var showsCourse = true
    /// A course is being played (not the title's demo, not a menu).
    @Published private(set) var playingCourse = false
    /// A text field of the front end is open (an account name, a comment): macOS types into it
    /// from the keyboard, iOS asks with a text field.
    @Published private(set) var typing = false
    private var items: [MmkFrontItem] = []
    private var layers: [UInt32: UILayer] = [:]
    private var click: SIMD2<Float>?
    private var pausePending = false
    private var reportedDone = false

    // MARK: setup

    private init(assets: EngineAssets, title: String, courseKey: String?, saves: SaveStore?) throws {
        guard let e = mmk_engine_new(assets.handle) else { throw EngineError(lastEngineError()) }
        self.assets = assets
        self.engine = e
        self.title = title
        self.courseKey = courseKey
        self.saves = saves
        // sound: the mixer is fed by the engine's events; silent if the cache has no audio
        if let url = assets.url {
            audio = AudioOutput(cachePath: url.path)
            if let h = audio?.handle { mmk_engine_set_audio(e, h) }
        }
        // SMM1 extras from Settings (off by default)
        let d = UserDefaults.standard
        mmk_engine_set_option(e, "smm1_shell_kicks", d.bool(forKey: SettingsKey.smm1ShellKicks) ? 1 : 0)
        mmk_engine_set_option(e, "smm1_layering", d.bool(forKey: SettingsKey.smm1Layering) ? 1 : 0)
        mmk_engine_set_option(e, "smb1_mushroom", Int32(d.integer(forKey: SettingsKey.smb1Mushroom)))
        mmk_engine_set_option(e, "costume", d.object(forKey: SettingsKey.costume) == nil ? -1 : Int32(d.integer(forKey: SettingsKey.costume)))
        // player 1's character (the pause menu's Settings)
        mmk_engine_set_character(e, UInt32(max(0, min(3, d.integer(forKey: SettingsKey.character)))))
        // switching to another app pauses (not during scripted runs)
        resignObserver = NotificationCenter.default.addObserver(forName: PlatformApp.resignActive, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                if let self, self.scriptedInput == nil { self.pause() }
            }
        }
    }

    /// Plain play of one course (no SMM2 UI import): `course` nil plays the demo strip.
    convenience init(assets: EngineAssets, course: Data?, title: String, courseKey: String?, saves: SaveStore?) throws {
        try self.init(assets: assets, title: title, courseKey: courseKey, saves: saves)
        if let course {
            let ok = course.withUnsafeBytes { mmk_engine_load_course(engine, $0.bindMemory(to: UInt8.self).baseAddress, course.count) }
            if !ok { throw EngineError(lastEngineError()) }
        }
        if let courseKey { saves?.addAttempt(courseKey) }
        applyVolumes()
    }

    /// How the front end starts.
    struct FrontStart {
        var boot = false
        var titleStart: UInt32 = 0
        var titleSelect: Int32 = -1
        var titleCourse: Int32 = -1
        var instantFades = false
        var opening: String?
        var script: String?
        var titleInput: String?
    }

    /// The SMM2 front end over a new engine (needs the SMM2 UI import).
    convenience init(assets: EngineAssets, front start: FrontStart, library: [CourseEntry]) throws {
        try self.init(assets: assets, title: AppIdentity.name, courseKey: nil, saves: nil)
        guard let ui = AppModel.smm2UIDir else { throw EngineError("no SMM2 UI import") }
        hideHud = true
        let d = UserDefaults.standard
        let env = ProcessInfo.processInfo.environment
        let smm1 = env["MMK_UI_SMM1"] == "off" ? nil : AppPaths.findSMM1Cache()
        let hundred = env["MMK_HUNDRED"] ?? d.string(forKey: SettingsKey.hundredPresentation)
        var flags: UInt32 = 0
        if start.boot { flags |= UInt32(MMK_FRONT_BOOT) }
        if start.instantFades { flags |= UInt32(MMK_FRONT_INSTANT_FADES) }
        if hundred == "smm1" { flags |= UInt32(MMK_FRONT_HUNDRED_SMM1) }
        let server = d.string(forKey: SettingsKey.courseServer) ?? ""
        let downloads = AppPaths.courses.appendingPathComponent("Course World", isDirectory: true).path
        let host = MmkFrontHost(user: Unmanaged.passUnretained(self).toOpaque(),
                                load_value: frontLoadValue, store_value: frontStoreValue,
                                set_paused: frontSetPaused)
        // (C strings live for the call: the front end copies what it keeps)
        let f: OpaquePointer? = ui.path.withCString { uiPath in
            withOptionalCString(smm1?.path) { smm1Path in
                server.withCString { serverC in
                    downloads.withCString { dl in
                        withOptionalCString(start.opening) { opening in
                            withOptionalCString(start.script) { script in
                                withOptionalCString(start.titleInput) { titleInput in
                                    var o = MmkFrontOptions(smm2_ui: uiPath, smm1_ui: smm1Path, server: serverC,
                                                            downloads: dl, opening: opening, script: script,
                                                            title_input: titleInput, flags: flags,
                                                            title_start: start.titleStart,
                                                            title_select: start.titleSelect,
                                                            title_course: start.titleCourse)
                                    return mmk_front_new(&o, engine, assets.handle, audio?.handle, host)
                                }
                            }
                        }
                    }
                }
            }
        }
        guard let f else { throw EngineError(lastEngineError()) }
        front = f
        setLibrary(library)
        applyVolumes()
    }

    deinit {
        if let resignObserver { NotificationCenter.default.removeObserver(resignObserver) }
        if let front {
            mmk_front_free(front)
            InputState.typing = false
        }
        mmk_engine_set_audio(engine, nil)
        audio = nil
        mmk_engine_free(engine)
    }

    /// Volumes from Settings (UserDefaults, default 1); music is turned down while paused.
    func applyVolumes(paused: Bool? = nil) {
        guard let h = audio?.handle else { return }
        let d = UserDefaults.standard
        func vol(_ k: String) -> Float { d.object(forKey: k) == nil ? 1 : Float(d.double(forKey: k)) }
        let down = paused ?? (phase == .paused)
        let music = vol(SettingsKey.volumeMusic) * (down ? 0.3 : 1)
        mmk_audio_set_volumes(h, vol(SettingsKey.volumeMaster), music, vol(SettingsKey.volumeSfx))
    }

    var hud: MmkHud {
        var h = MmkHud()
        mmk_engine_hud(engine, &h)
        return h
    }

    // MARK: front end

    /// Give the front end the library (call when it changes).
    func setLibrary(_ entries: [CourseEntry]) {
        guard let front else { return }
        let list = entries.map(\.frontJSON)
        guard let data = try? JSONSerialization.data(withJSONObject: list),
              let text = String(data: data, encoding: .utf8) else { return }
        mmk_front_set_library(front, text)
    }

    /// Play a course file with the SMM2 screens (unlimited retries); `onExit` when the player
    /// leaves it.
    func playCourse(path: String, name: String?, intro: Bool) -> Bool {
        guard let front else { return false }
        reportedDone = false
        return withOptionalCString(name) { mmk_front_play_course(front, path, $0, intro) }
    }

    /// Captures: a screen as `mmk-play --front-screen` names it.
    @discardableResult
    func capture(_ screen: String, difficulty: Int = 0, cleared: Int = 3) -> Bool {
        guard let front else { return false }
        return mmk_front_capture(front, screen, UInt32(max(0, difficulty)), UInt32(max(0, cleared)))
    }

    /// Captures of a course play's screens (course-pause, course-clear, …).
    @discardableResult
    func debugCourse(_ screen: String) -> Bool {
        guard let front else { return false }
        return mmk_front_debug_course(front, screen)
    }

    /// Captures: run `ticks` ticks at once, then hold the screen there.
    func runAndFreeze(_ ticks: Int, courseScreen: Bool) {
        guard let front else { return }
        mmk_front_run_ticks(front, UInt64(max(0, ticks)), courseScreen, scriptedInput != nil)
        frozen = true
        refreshFrame()
    }

    /// A click / tap at `point` of a view `size` (points): the 16:9 view is letterboxed into it.
    func click(at point: CGPoint, in size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        let scale = min(size.width / 16, size.height / 9)
        let w = 16 * scale, h = 9 * scale
        let u = (point.x - (size.width - w) / 2) / w, v = (point.y - (size.height - h) / 2) / h
        guard (0...1).contains(u), (0...1).contains(v) else { return }
        click = SIMD2(Float((u - 0.5) * 1280), Float((0.5 - v) * 720))
    }

    private func frontTick(_ front: OpaquePointer, first: Bool) {
        let live = input.current
        var i = MmkFrontInput()
        if scriptedInput == nil {
            i.menu = UInt32(input.menu.rawValue)
            i.buttons = live.buttons & ~UInt32(MMK_BTN_START)
            if live.buttons & UInt32(MMK_BTN_START) != 0 { i.flags |= 1 }
            if input.skipHeld { i.flags |= 2 }
        } else {
            var st = MmkFrontStatus()
            mmk_front_status(front, &st)
            i.buttons = scriptedInput?(st.engine_tick).buttons ?? 0
        }
        if pausePending {
            // (another app came in front: one tick of the pause button)
            pausePending = false
            i.flags |= 1
        }
        if !InputState.typed.isEmpty { i.text = InputState.typed.removeFirst() }
        if first, let c = click {
            click = nil
            i.flags |= 4
            i.click_x = c.x
            i.click_y = c.y
        }
        mmk_front_tick(front, &i)
        var sounds: UnsafePointer<MmkFrontSound>?
        let n = mmk_front_sounds(front, &sounds)
        for s in UnsafeBufferPointer(start: sounds, count: n) {
            UISound.shared.play(String(cString: s.name), volume: s.volume, delayFrames: Double(s.delay))
        }
    }

    /// This frame's draw items, toast and status.
    private func refreshFrame() {
        guard let front else { return }
        var ptr: UnsafePointer<MmkFrontItem>?
        let n = mmk_front_frame(front, &ptr)
        items = Array(UnsafeBufferPointer(start: ptr, count: n))
        showsCourse = items.contains { $0.kind == 1 }
        let len = mmk_front_toast(front, nil, 0)
        var text: String?
        if len > 0 {
            var buf = [CChar](repeating: 0, count: len + 1)
            _ = mmk_front_toast(front, &buf, buf.count)
            text = String(cString: buf)
        }
        if text != toast { toast = text }
        var st = MmkFrontStatus()
        mmk_front_status(front, &st)
        let typingNow = st.flags & UInt32(MMK_FRONT_TYPING) != 0
        if typingNow != typing {
            typing = typingNow
            InputState.typing = typingNow
            if !typingNow { InputState.typed.removeAll() }
        }
        let playing = st.flags & UInt32(MMK_FRONT_PLAYING) != 0
        if playing != playingCourse { playingCourse = playing }
        if st.flags & UInt32(MMK_FRONT_COURSE_DONE) != 0 && !reportedDone {
            reportedDone = true
            DispatchQueue.main.async { [weak self] in self?.onExit?() }
        }
    }

    private func frontFrame(_ front: OpaquePointer, dt: Double) {
        if !frozen {
            accumulator = min(accumulator + dt, 0.25)
            var first = true
            while accumulator >= Self.tick {
                accumulator -= Self.tick
                frontTick(front, first: first)
                first = false
            }
        }
        refreshFrame()
    }

    /// Whether `encodeLayouts` has anything to draw.
    var hasLayouts: Bool { items.contains { $0.kind == 0 && $0.quad_count > 0 } }

    /// Draw this frame's layouts (back to front) into `enc`'s current viewport (the 16:9 view).
    func encodeLayouts(_ enc: MTLRenderCommandEncoder, device: MTLDevice) {
        guard let front else { return }
        for it in items where it.kind == 0 && it.quad_count > 0 {
            let layer: UILayer
            if let l = layers[it.ui] {
                layer = l
            } else {
                let which = it.ui
                guard let l = try? UILayer(device: device, pixelFormat: .bgra8Unorm_srgb,
                                           textureCount: { mmk_front_texture_count(front, which) },
                                           textureInfo: { i, info in mmk_front_texture(front, which, i, &info) })
                else { continue }
                layers[it.ui] = l
                layer = l
            }
            layer.encode(enc, quads: UnsafeBufferPointer(start: it.quads, count: it.quad_count),
                         layoutSize: SIMD2(it.width, it.height))
        }
    }

    // MARK: per frame

    /// Advance to `now`: the front end's ticks, or plain play (fixed-step the engine while
    /// playing, else drive the SwiftUI menus).
    func frame(now: CFTimeInterval) {
        let dt = now - lastTime
        lastTime = now
        if let front {
            frontFrame(front, dt: dt)
            return
        }
        let live = scriptedInput == nil ? input.current : MmkInput(buttons: 0)
        let startDown = live.buttons & UInt32(MMK_BTN_START) != 0
        let startPressed = startDown && !heldStart
        heldStart = startDown
        let menu = scriptedInput == nil ? input.menu : []
        let pressed = menu.subtracting(heldMenu)
        heldMenu = menu
        phaseFrames += 1

        switch phase {
        case .playing:
            if startPressed { pause(); return }
            accumulator = min(accumulator + dt, 0.25)
            while accumulator >= Self.tick && phase == .playing {
                var inp = scriptedInput?(engineTick) ?? live
                inp.buttons &= ~UInt32(MMK_BTN_START)
                mmk_engine_step(engine, &inp)
                engineTick += 1
                accumulator -= Self.tick
                observeTick()
                if let t = autoPauseTick, engineTick >= t { autoPauseTick = nil; pause() }
            }
        case .paused:
            let items = pauseItems
            if pressed.contains(.up) { pauseSelection = (pauseSelection + items.count - 1) % items.count }
            if pressed.contains(.down) { pauseSelection = (pauseSelection + 1) % items.count }
            if pressed.contains(.back) { resume() }
            else if pressed.contains(.confirm) { choose(items[pauseSelection]) }
        case .cleared, .gameOver:
            // a short beat so a held jump doesn't skip the screen
            if phaseFrames > 30 && !pressed.intersection([.confirm, .back]).isEmpty { exit() }
        }
    }

    private func observeTick() {
        attemptTicks += 1
        let h = hud
        if h.events & (1 << 13) != 0 && goalTicks == nil { goalTicks = attemptTicks } // Event::Goal
        let dying = h.flags & 2 != 0
        if wasDying && !dying {
            // death sequence over, the course restarted
            if h.lives == 0 && !unlimitedLives {
                setPhase(.gameOver)
            } else {
                newAttempt()
            }
        }
        wasDying = dying
        if h.flags & 1 != 0 {
            let ticks = goalTicks ?? attemptTicks
            clearTicks = ticks
            if let courseKey { newBest = saves?.addClear(courseKey, ticks: ticks) ?? false }
            setPhase(.cleared)
        }
    }

    private func newAttempt() {
        attemptTicks = 0
        goalTicks = nil
        if let courseKey { saves?.addAttempt(courseKey) }
    }

    private func setPhase(_ p: Phase) {
        phase = p
        phaseFrames = 0
        heldMenu = input.menu // don't treat buttons already down as presses
        applyVolumes()
    }

    // MARK: actions (menu, mouse)

    func pause() {
        if front != nil {
            pausePending = true
            return
        }
        guard phase == .playing else { return }
        pauseSelection = 0
        setPhase(.paused)
    }

    func resume() {
        guard phase == .paused else { return }
        lastTime = CACurrentMediaTime()
        accumulator = 0
        setPhase(.playing)
        heldStart = true
    }

    func restart() {
        mmk_engine_restart(engine)
        wasDying = false
        newAttempt()
        lastTime = CACurrentMediaTime()
        accumulator = 0
        setPhase(.playing)
        heldStart = true
    }

    func exit() { onExit?() }

    func choose(_ item: PauseItem) {
        switch item {
        case .resume: resume()
        case .restart: restart()
        case .exit: exit()
        }
    }
}

// MARK: - the front end's host callbacks

/// Values the front end keeps (`MmkFrontHost.load_value`): save data and the 100 Mario save as
/// the app's files, the openings seen and the character in UserDefaults as before, anything else
/// (the course server account) in UserDefaults under `front.<key>`.
@MainActor
enum FrontStore {
    static let hundredURL = AppPaths.support.appendingPathComponent("hundred.json")

    static func load(_ key: String) -> String? {
        let d = UserDefaults.standard
        switch key {
        case "saves": return try? String(contentsOf: AppPaths.saves, encoding: .utf8)
        case "hundred": return try? String(contentsOf: hundredURL, encoding: .utf8)
        case "boot.openingsSeen": return String(d.integer(forKey: key))
        case SettingsKey.character: return d.object(forKey: key).map { _ in String(d.integer(forKey: key)) }
        default: return d.string(forKey: "front." + key)
        }
    }

    static func store(_ key: String, _ value: String) {
        let d = UserDefaults.standard
        switch key {
        case "saves", "hundred":
            let url = key == "saves" ? AppPaths.saves : hundredURL
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? value.data(using: .utf8)?.write(to: url, options: .atomic)
            if key == "saves" { NotificationCenter.default.post(name: .mmkSavesChanged, object: nil) }
        case "boot.openingsSeen", SettingsKey.character: d.set(Int(value) ?? 0, forKey: key)
        default: d.set(value, forKey: "front." + key)
        }
    }
}

extension Notification.Name {
    /// The front end wrote the save data (the library's records reload).
    static let mmkSavesChanged = Notification.Name("mmk.savesChanged")
}

private func frontLoadValue(_ user: UnsafeMutableRawPointer?, _ key: UnsafePointer<CChar>?,
                            _ buf: UnsafeMutablePointer<CChar>?, _ cap: Int) -> Int64 {
    guard let key else { return -1 }
    let k = String(cString: key)
    let value = MainActor.assumeIsolated { FrontStore.load(k) }
    guard let value else { return -1 }
    let bytes = Array(value.utf8)
    if let buf, cap > bytes.count {
        for (i, b) in bytes.enumerated() { buf[i] = CChar(bitPattern: b) }
        buf[bytes.count] = 0
    }
    return Int64(bytes.count)
}

private func frontStoreValue(_ user: UnsafeMutableRawPointer?, _ key: UnsafePointer<CChar>?,
                             _ value: UnsafePointer<CChar>?) {
    guard let key, let value else { return }
    let k = String(cString: key), v = String(cString: value)
    MainActor.assumeIsolated { FrontStore.store(k, v) }
}

private func frontSetPaused(_ user: UnsafeMutableRawPointer?, _ paused: Bool) {
    guard let user else { return }
    MainActor.assumeIsolated {
        Unmanaged<GameSession>.fromOpaque(user).takeUnretainedValue().applyVolumes(paused: paused)
    }
}

/// `body` with a C string for `s`, or NULL.
func withOptionalCString<R>(_ s: String?, _ body: (UnsafePointer<CChar>?) -> R) -> R {
    guard let s else { return body(nil) }
    return s.withCString { body($0) }
}

extension CourseEntry {
    /// The front end's library entry (`mmk_front_set_library`).
    var frontJSON: [String: Any] {
        var meta: [String: Any] = ["tags": self.meta.tags]
        if let v = self.meta.clearRate { meta["clear_rate"] = v }
        if let v = self.meta.bucket { meta["bucket"] = v }
        if let v = self.meta.maker { meta["maker"] = v }
        if let id = self.meta.dataId {
            var buf = [CChar](repeating: 0, count: 16)
            _ = mmk_smm2_course_code(id, &buf, buf.count)
            meta["code"] = String(cString: buf)
        }
        if let v = self.meta.worldRecordMs { meta["world_record_ms"] = v }
        if let v = self.meta.plays { meta["plays"] = v }
        if let v = self.meta.likes { meta["likes"] = v }
        if let v = self.meta.clears { meta["clears"] = v }
        if let v = self.meta.attempts { meta["attempts"] = v }
        if let v = self.meta.uploaded { meta["uploaded"] = Int(v.timeIntervalSince1970) }
        var o: [String: Any] = [
            "id": url.path, "name": name, "description": info.description, "style": info.style,
            "theme": Int(info.theme), "playable": info.playable, "built_in": builtIn, "key": info.key,
            "meta": meta,
        ]
        if let c = collection { o["collection"] = c }
        if let m = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate {
            o["modified"] = Int(m.timeIntervalSince1970)
        }
        return o
    }
}

// MARK: - view

struct GameView: View {
    @ObservedObject var session: GameSession

    var body: some View {
        GeometryReader { geo in
            ZStack {
                MetalView(session: session)
                    .gesture(SpatialTapGesture().onEnded { session.click(at: $0.location, in: geo.size) },
                             including: session.front != nil ? .all : .none)
                if session.front == nil {
                    switch session.phase {
                    case .playing: EmptyView()
                    case .paused: PauseMenu(session: session)
                    case .cleared: ResultCard(title: "Course clear!", detail: clearDetail, action: "Back to library", session: session)
                    case .gameOver: ResultCard(title: "Game over", detail: "Out of lives.", action: "Back to library", session: session)
                    }
                }
                if let t = session.toast {
                    VStack {
                        Spacer()
                        Text(t)
                            .font(.system(size: 18, weight: .semibold, design: .rounded))
                            .padding(.horizontal, 22).padding(.vertical, 12)
                            .background(Capsule().fill(.black.opacity(0.75)))
                            .foregroundStyle(.white)
                            .padding(.bottom, 40)
                    }
                    .transition(.opacity)
                    .allowsHitTesting(false)
                }
            }
            .animation(.easeInOut(duration: 0.2), value: session.toast)
        }
        #if os(iOS)
        // no keyboard to type into the front end's text fields: ask with a text field
        .alert("Enter text", isPresented: Binding(get: { session.typing }, set: { _ in })) {
            TextField("", text: $typed)
            Button("OK") { send(typed, finish: 10) }
            Button("Cancel", role: .cancel) { send("", finish: 27) }
        }
        #endif
    }

    #if os(iOS)
    @State private var typed = ""

    private func send(_ text: String, finish: UInt32) {
        InputState.typed += text.unicodeScalars.map(\.value) + [finish]
        typed = ""
    }
    #endif

    private var clearDetail: String {
        guard let t = session.clearTicks else { return "" }
        return "Time \(CourseRecord.format(ticks: t))" + (session.newBest ? " — new best!" : "")
    }
}

private struct PauseMenu: View {
    @ObservedObject var session: GameSession

    #if os(macOS)
    private static let hint = "↑↓ choose · Return / A select · Esc / B continue"
    #else
    private static let hint = "Tap an item, or ↑↓ and Jump · + continues"
    #endif

    var body: some View {
        ZStack {
            Color.black.opacity(0.55)
            VStack(spacing: 18) {
                Text("Paused").font(.system(size: 34, weight: .heavy, design: .rounded))
                Text(session.title).font(.headline).foregroundStyle(.white.opacity(0.75)).lineLimit(1)
                VStack(spacing: 8) {
                    ForEach(Array(session.pauseItems.enumerated()), id: \.offset) { i, item in
                        Button { session.choose(item) } label: {
                            Text(item.rawValue)
                                .font(.system(size: 18, weight: .semibold, design: .rounded))
                                .frame(width: 240, height: 40)
                                .background(RoundedRectangle(cornerRadius: 10)
                                    .fill(i == session.pauseSelection ? Color.accentColor : Color.white.opacity(0.12)))
                        }
                        .buttonStyle(.plain)
                        .onHover { if $0 { session.pauseSelection = i } }
                    }
                }
                Text(Self.hint)
                    .font(.caption).foregroundStyle(.white.opacity(0.6))
            }
            .padding(32)
            .background(RoundedRectangle(cornerRadius: 18).fill(.black.opacity(0.6)))
            .foregroundStyle(.white)
        }
    }
}

private struct ResultCard: View {
    let title: String
    let detail: String
    let action: String
    @ObservedObject var session: GameSession

    var body: some View {
        ZStack {
            Color.black.opacity(0.4)
            VStack(spacing: 14) {
                Text(title).font(.system(size: 40, weight: .heavy, design: .rounded))
                if !detail.isEmpty { Text(detail).font(.title3.monospacedDigit()) }
                Button(action) { session.exit() }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                Text("Press Return / A to continue").font(.caption).foregroundStyle(.white.opacity(0.6))
            }
            .padding(36)
            .background(RoundedRectangle(cornerRadius: 18).fill(.black.opacity(0.65)))
            .foregroundStyle(.white)
        }
    }
}
