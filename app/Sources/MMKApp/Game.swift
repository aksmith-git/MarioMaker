import AppKit
import CMmk
import QuartzCore
import SwiftUI

/// One play session of one course: owns the engine, steps it at a fixed 60 Hz while playing, and
/// runs the pause / course clear / game over states. The Renderer calls `frame(now:)` once per
/// display frame and draws whatever the engine shows; it knows nothing about menus.
@MainActor
final class GameSession: ObservableObject, Identifiable {
    enum Phase: Equatable {
        case playing, paused, cleared, gameOver
    }

    enum PauseItem: String, CaseIterable {
        case resume = "Continue"
        case restart = "Restart course"
        case exit = "Exit to library"
    }

    nonisolated let id = UUID()
    let assets: EngineAssets
    let engine: OpaquePointer
    let title: String
    /// Save-data key (nil for the demo strip).
    let courseKey: String?
    let input = InputState()
    /// Sound output (nil when muted or the cache has no audio).
    private var audio: AudioOutput?

    @Published private(set) var phase: Phase = .playing
    @Published var pauseSelection = 0
    /// Clear time of the last clear (ticks) and whether it was a new best.
    @Published private(set) var clearTicks: Int?
    @Published private(set) var newBest = false

    /// Called when the player leaves the session (exit, after a clear or game over).
    var onExit: (() -> Void)?
    /// Debug/smoke tests: replaces the player's input; given the engine tick.
    var scriptedInput: ((UInt64) -> MmkInput)?
    /// Debug: pause automatically after this many ticks (screenshots of the pause menu).
    var autoPauseTick: UInt64?

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

    init(assets: EngineAssets, course: Data?, title: String, courseKey: String?, saves: SaveStore?) throws {
        guard let e = mmk_engine_new(assets.handle) else { throw EngineError(lastEngineError()) }
        self.assets = assets
        self.engine = e
        self.title = title
        self.courseKey = courseKey
        self.saves = saves
        if let course {
            let ok = course.withUnsafeBytes { mmk_engine_load_course(e, $0.bindMemory(to: UInt8.self).baseAddress, course.count) }
            if !ok {
                let msg = lastEngineError()
                mmk_engine_free(e)
                throw EngineError(msg)
            }
        }
        if let courseKey { saves?.addAttempt(courseKey) }
        // sound: the mixer is fed by the engine's events; silent if the cache has no audio
        if let url = assets.url {
            audio = AudioOutput(cachePath: url.path)
            if let h = audio?.handle { mmk_engine_set_audio(e, h) }
        }
        applyVolumes()
        // switching to another app pauses (not during scripted runs)
        resignObserver = NotificationCenter.default.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                if let self, self.scriptedInput == nil { self.pause() }
            }
        }
    }

    deinit {
        if let resignObserver { NotificationCenter.default.removeObserver(resignObserver) }
        mmk_engine_set_audio(engine, nil)
        audio = nil
        mmk_engine_free(engine)
    }

    /// Volumes from Settings (UserDefaults, default 1); music is turned down while paused.
    func applyVolumes() {
        guard let h = audio?.handle else { return }
        let d = UserDefaults.standard
        func vol(_ k: String) -> Float { d.object(forKey: k) == nil ? 1 : Float(d.double(forKey: k)) }
        let music = vol(SettingsKey.volumeMusic) * (phase == .paused ? 0.3 : 1)
        mmk_audio_set_volumes(h, vol(SettingsKey.volumeMaster), music, vol(SettingsKey.volumeSfx))
    }

    var hud: MmkHud {
        var h = MmkHud()
        mmk_engine_hud(engine, &h)
        return h
    }

    // MARK: per frame

    /// Advance to `now`: fixed-step the engine while playing, else drive the menus.
    func frame(now: CFTimeInterval) {
        let dt = now - lastTime
        lastTime = now
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
            if pressed.contains(.up) { pauseSelection = (pauseSelection + PauseItem.allCases.count - 1) % PauseItem.allCases.count }
            if pressed.contains(.down) { pauseSelection = (pauseSelection + 1) % PauseItem.allCases.count }
            if pressed.contains(.back) { resume() }
            else if pressed.contains(.confirm) { choose(PauseItem.allCases[pauseSelection]) }
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
            if h.lives == 0 {
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

// MARK: - view

struct GameView: View {
    @ObservedObject var session: GameSession

    var body: some View {
        ZStack {
            MetalView(session: session)
            switch session.phase {
            case .playing: EmptyView()
            case .paused: PauseMenu(session: session)
            case .cleared: ResultCard(title: "Course clear!", detail: clearDetail, action: "Back to library", session: session)
            case .gameOver: ResultCard(title: "Game over", detail: "Out of lives.", action: "Back to library", session: session)
            }
        }
    }

    private var clearDetail: String {
        guard let t = session.clearTicks else { return "" }
        return "Time \(CourseRecord.format(ticks: t))" + (session.newBest ? " — new best!" : "")
    }
}

private struct PauseMenu: View {
    @ObservedObject var session: GameSession

    var body: some View {
        ZStack {
            Color.black.opacity(0.55)
            VStack(spacing: 18) {
                Text("Paused").font(.system(size: 34, weight: .heavy, design: .rounded))
                Text(session.title).font(.headline).foregroundStyle(.white.opacity(0.75)).lineLimit(1)
                VStack(spacing: 8) {
                    ForEach(Array(GameSession.PauseItem.allCases.enumerated()), id: \.offset) { i, item in
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
                Text("↑↓ choose · Return / A select · Esc / B continue")
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
