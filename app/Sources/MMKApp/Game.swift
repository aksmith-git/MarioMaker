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
        /// 100 Mario with the SMM1 layouts: the intro card after a death, the skip's Pass animation.
        case card, skipping
    }

    enum PauseItem: String, CaseIterable {
        case resume = "Continue"
        case skip = "Skip this course"
        case restart = "Restart course"
        case saveQuit = "Save & Quit"
        case exit = "Exit to library"
    }

    /// Pause menu entries: a challenge run can skip and Save & Quit, and a restart costs a life.
    var pauseItems: [PauseItem] {
        guard let run else { return [.resume, .restart, .exit] }
        // SMM1 pause menu: close, Start Over, Save & Quit (skipping is the held skip key)
        if overlay != nil { return [.resume, .restart, .saveQuit] }
        return run.canSkip ? [.resume, .skip, .restart, .saveQuit] : [.resume, .restart, .saveQuit]
    }

    func label(_ item: PauseItem) -> String {
        item == .restart && run != nil ? "Start over (costs a life)" : item.rawValue
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
    /// The 100 Mario run this course belongs to: it owns lives, and decides what a death means.
    weak var run: ChallengeFlow?
    /// SMM1 layouts over the game for a run (pause, clear, skip countdown, intro card after a
    /// death); nil for the stand-in SwiftUI menus. Drawn by the Renderer.
    var overlay: RunOverlay?
    /// Debug captures: behave as if the skip key were held.
    var debugSkipHeld = false
    private var uiAccumulator = 0.0
    private var skipDoneTick: Int?
    private var livesGained: UInt32 = 0
    /// Debug/smoke tests: replaces the player's input; given the engine tick.
    var scriptedInput: ((UInt64) -> MmkInput)?
    /// A layout screen drawn over the course and stepped at 60 Hz with the menu input (the SMM2
    /// title over its scripted demo course). Drawn by the Renderer.
    var sceneOverlay: (scene: UIScene, ui: LayoutUI)?
    /// No HUD strip (title demo).
    var hideHud = false
    /// Title demo: called instead of the clear / death handling when the course ends (the
    /// owner loads the next course with `loadDemoCourse`).
    var onDemoEnd: (() -> Void)?
    private var sceneAccumulator = 0.0
    private var sceneHeld: MenuInput = []
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
                // (self is fully initialised here, so deinit frees the engine)
                throw EngineError(lastEngineError())
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
        resignObserver = NotificationCenter.default.addObserver(forName: PlatformApp.resignActive, object: nil, queue: .main) { [weak self] _ in
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
        // (a title demo's scene reads the menu input itself, below)
        let live = scriptedInput == nil ? input.current : MmkInput(buttons: 0)
        let startDown = live.buttons & UInt32(MMK_BTN_START) != 0
        let startPressed = startDown && !heldStart
        heldStart = startDown
        let menu = scriptedInput == nil ? input.menu : []
        let pressed = menu.subtracting(heldMenu)
        heldMenu = menu
        phaseFrames += 1
        if let o = overlay {
            uiAccumulator = min(uiAccumulator + dt, 0.25)
            while uiAccumulator >= Self.tick {
                uiAccumulator -= Self.tick
                overlayTick(o)
            }
        }
        if let (scene, _) = sceneOverlay {
            if let freeze = DebugCapture.freezeTicks {
                while scene.tick < freeze { scene.step(UIInput()) }
            } else {
                sceneAccumulator = min(sceneAccumulator + dt, 0.25)
                while sceneAccumulator >= Self.tick {
                    sceneAccumulator -= Self.tick
                    let m = input.menu
                    scene.step(UIInput(pressed: m.subtracting(sceneHeld), held: m, skip: false))
                    sceneHeld = m
                }
            }
        }

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
            if !pressed.intersection([.up, .down]).isEmpty { overlay?.pauseSelect(pauseSelection) }
            if pressed.contains(.back) { resume() }
            else if pressed.contains(.confirm) { choose(items[pauseSelection]) }
        case .cleared where overlay != nil:
            guard let o = overlay, o.clearReady else { break }
            if pressed.contains(.left) { o.clearSelect(0) }
            if pressed.contains(.right) { o.clearSelect(1) }
            if pressed.contains(.confirm) {
                if o.clearFocus == 0 { run?.saveAndQuitAfterClear() }
                o.pressClear()
                exit()
            }
        case .gameOver where overlay != nil && run != nil:
            exit()   // the run shows Cmn_GameOver_00 as its own screen
        case .card, .skipping:
            break    // driven by overlayTick
        case .cleared, .gameOver:
            // a short beat so a held jump doesn't skip the screen
            if phaseFrames > 30 && !pressed.intersection([.confirm, .back]).isEmpty { exit() }
        }
    }

    private func observeTick() {
        attemptTicks += 1
        let h = hud
        if h.events & (1 << 13) != 0 && goalTicks == nil { goalTicks = attemptTicks } // Event::Goal
        if h.lives_gained > livesGained {
            run?.extraLives(Int(h.lives_gained - livesGained))
        }
        livesGained = h.lives_gained
        let dying = h.flags & 2 != 0
        if let end = onDemoEnd {
            // title demo: the end of a death sequence, or a clear, moves on to the next course
            if (wasDying && !dying) || h.flags & 1 != 0 {
                wasDying = dying
                end()
                return
            }
            wasDying = dying
            return
        }
        if wasDying && !dying, let run {
            // the engine restarted the course; the run decides whether it continues
            if run.died() { newAttempt(); showRetryCard() } else { setPhase(.gameOver) }
        } else if wasDying && !dying {
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
            run?.cleared(score: h.score, coins: h.coins)
            setPhase(.cleared)
            showClearOverlay()
        }
    }

    private func showClearOverlay() {
        guard let o = overlay, let run else { return }
        o.showClear(course: run.course?.name ?? title, maker: run.course?.maker ?? "", ticks: clearTicks,
                    worldRecordMs: run.courseWorldRecordMs, last: run.clearEndsRun)
    }

    /// In a run with the SMM1 layouts, the intro card (lives after the death) before the retry.
    private func showRetryCard() {
        guard let o = overlay, let run else { return }
        o.showCard(run.cardInfo())
        setPhase(.card)
    }

    /// One 60 Hz tick of the overlay: animations, the skip hold, the end of the card / Pass.
    private func overlayTick(_ o: RunOverlay) {
        if let f = DebugCapture.freezeTicks, o.mode != .none, o.sinceMode >= f { return }
        o.step(UIInput())
        switch phase {
        case .playing:
            guard let run, run.canSkip else { break }
            let held = debugSkipHeld || (scriptedInput == nil && input.skipHeld)
            if held {
                if o.mode == .none { o.skipBegin() }
                if o.mode == .skip && o.sinceMode >= RunOverlay.skipHold {
                    skipDoneTick = o.tick + o.skipPass()
                    setPhase(.skipping)
                }
            } else if o.mode == .skip {
                o.skipCancel()
            }
        case .skipping:
            if let t = skipDoneTick, o.tick >= t {
                skipDoneTick = nil
                run?.skipped()
                exit()
            }
        case .card:
            if o.cardDone {
                o.hideCard()
                lastTime = CACurrentMediaTime()
                accumulator = 0
                setPhase(.playing)
                heldStart = true
            }
        default:
            break
        }
    }

    /// Title demo: replace the course (the scripted input starts over).
    func loadDemoCourse(_ data: Data) {
        let ok = data.withUnsafeBytes { mmk_engine_load_course(engine, $0.bindMemory(to: UInt8.self).baseAddress, data.count) }
        if !ok { mmk_engine_restart(engine) }
        engineTick = 0
        accumulator = 0
        wasDying = false
        livesGained = 0
    }

    /// Debug captures: show the clear overlay / the retry card without finishing the course.
    func debugClear() {
        clearTicks = 2345
        setPhase(.cleared)
        showClearOverlay()
    }

    func debugCard() { showRetryCard() }

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
        if let o = overlay, let run {
            if o.mode == .skip { o.skipCancel() }
            o.showPause(modeLine: run.pauseLine, course: run.course?.name ?? title,
                        maker: run.course?.maker ?? "", selection: 0)
        }
        setPhase(.paused)
    }

    func resume() {
        guard phase == .paused else { return }
        overlay?.hidePause()
        lastTime = CACurrentMediaTime()
        accumulator = 0
        setPhase(.playing)
        heldStart = true
    }

    func restart() {
        // in a run, starting over counts as a death
        overlay?.clearAll()
        if let run, !run.died() {
            setPhase(.gameOver)
            return
        }
        mmk_engine_restart(engine)
        run?.applyCounters(to: engine)
        livesGained = 0
        wasDying = false
        newAttempt()
        lastTime = CACurrentMediaTime()
        accumulator = 0
        setPhase(.playing)
        heldStart = true
        showRetryCard()
    }

    func exit() { onExit?() }

    func choose(_ item: PauseItem) {
        switch item {
        case .resume: resume()
        case .restart: restart()
        case .exit: exit()
        case .skip: run?.skipped(); exit()
        case .saveQuit: run?.saveAndQuit(); exit()
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
            case .playing, .card, .skipping: EmptyView()
            case _ where session.overlay != nil: EmptyView()   // SMM1 layouts, drawn by the Renderer
            case .paused: PauseMenu(session: session)
            case .cleared: ResultCard(title: "Course clear!", detail: clearDetail, action: session.run == nil ? "Back to library" : "Next", session: session)
            case .gameOver: ResultCard(title: "Game over", detail: "Out of lives.", action: session.run == nil ? "Back to library" : "Continue", session: session)
            }
            if let run = session.run, session.phase == .playing, session.overlay == nil { RunBanner(run: run) }
        }
    }

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
                if let run = session.run { Text(run.pauseLine).font(.subheadline.weight(.semibold)) }
                Text(session.title).font(.headline).foregroundStyle(.white.opacity(0.75)).lineLimit(1)
                VStack(spacing: 8) {
                    ForEach(Array(session.pauseItems.enumerated()), id: \.offset) { i, item in
                        Button { session.choose(item) } label: {
                            Text(session.label(item))
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
