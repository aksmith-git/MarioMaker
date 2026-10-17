import AppKit
import CMmk
import SwiftUI

/// The 100 Mario Challenge: difficulty select → course intro → course → result, until the run is
/// cleared, lost or saved. Rules, pools and save data live in the Rust session
/// (`mmk_challenge_*`); this class sequences the screens and owns the course being played.
///
/// These screens are a functional stand-in: the SMM1 layout player will draw the original world
/// map, intro card and ending from the owner's SMM1 data when it is imported.
@MainActor
final class ChallengeFlow: ObservableObject, Identifiable {
    enum Stage {
        case select
        case intro
        case playing(GameSession)
        case gameOver
        case complete
    }

    struct Course {
        var path: String
        var name: String
        var maker: String
        var difficulty: Int
    }

    static let difficulties = ["Easy", "Normal", "Expert", "Super Expert"]
    static let runLengths = [8, 16, 16, 6]

    nonisolated let id = UUID()
    @Published private(set) var stage: Stage = .select
    @Published private(set) var status = MmkRunStatus()
    @Published private(set) var course: Course?
    @Published var message: String?
    @Published private(set) var difficulty = 0
    /// Shown on the ending screen: a difficulty this run unlocked.
    @Published private(set) var unlocked: Int?
    @Published private(set) var credits: [Course] = []

    private let handle: OpaquePointer
    private let assets: EngineAssets
    private weak var saves: SaveStore?
    private let entries: [String: CourseEntry]
    var onExit: (() -> Void)?

    static var saveURL: URL { AppPaths.support.appendingPathComponent("hundred.json") }

    /// `library`: the playable courses offered to the pools.
    init(assets: EngineAssets, library: [CourseEntry], saves: SaveStore?) {
        self.assets = assets
        self.saves = saves
        let json = try? String(contentsOf: Self.saveURL, encoding: .utf8)
        handle = mmk_challenge_new(json, UInt64.random(in: 1...UInt64.max))
        var map: [String: CourseEntry] = [:]
        for e in library where e.info.playable {
            map[e.url.path] = e
            mmk_challenge_add_course(handle, e.url.path, e.name, e.meta.maker ?? "",
                                     UInt8(e.meta.difficultyValue), e.info.theme == 2)
        }
        entries = map
    }

    deinit { mmk_challenge_free(handle) }

    // MARK: queries for the select screen

    func unlocked(_ d: Int) -> Bool { mmk_challenge_unlocked(handle, UInt8(d)) }
    func canContinue(_ d: Int) -> Bool { mmk_challenge_can_continue(handle, UInt8(d)) }
    func available(_ d: Int) -> Int { Int(mmk_challenge_available(handle, UInt8(d))) }
    func clears(_ d: Int) -> Int { Int(mmk_challenge_clears(handle, UInt8(d))) }
    var canSkip: Bool { true }
    var pauseLine: String { "100 Mario Challenge (\(status.course_number)/\(status.total))" }

    // MARK: flow

    func start(_ d: Int, resume: Bool) {
        difficulty = d
        unlocked = nil
        guard mmk_challenge_start(handle, UInt8(d), resume) else {
            message = available(d) == 0
                ? "There aren't any playable \(Self.difficulties[d]) courses in your library yet."
                : "Couldn't start the run: \(lastEngineError())."
            return
        }
        persist()
        showIntro()
    }

    private func showIntro() {
        refresh()
        var c = MmkRunCourse()
        guard mmk_challenge_course(handle, &c) else {
            message = "The library ran out of courses for this run. Your progress is saved."
            stage = .select
            return
        }
        course = Course(path: Self.str(c.id), name: Self.str(c.name), maker: Self.str(c.maker),
                        difficulty: Int(c.difficulty))
        stage = .intro
    }

    /// Leave the intro card: load and play the course (an unreadable one is replaced).
    func play() {
        guard let course else { return }
        let entry = entries[course.path]
        do {
            let data = try Data(contentsOf: URL(fileURLWithPath: course.path))
            let s = try GameSession(assets: assets, course: data, title: entry?.name ?? course.name,
                                    courseKey: entry?.info.key, saves: saves)
            s.run = self
            applyCounters(to: s.engine)
            s.onExit = { [weak self] in self?.sessionEnded() }
            mmk_challenge_course_started(handle)
            persist()
            stage = .playing(s)
        } catch {
            if mmk_challenge_unloadable(handle) {
                message = "\(course.name) couldn't be loaded. The next course will be loaded instead."
                showIntro()
            } else {
                message = "\(course.name) couldn't be loaded and no other course is left."
                stage = .select
            }
        }
    }

    /// Skip from the intro card.
    func skipIntro() {
        next(mmk_challenge_finish(handle, UInt8(MMK_OUTCOME_SKIPPED), 0, 0))
    }

    func backToSelect() {
        mmk_challenge_end(handle)
        refresh()
        stage = .select
    }

    func exit() {
        mmk_challenge_end(handle)
        onExit?()
    }

    // MARK: called by the GameSession

    func applyCounters(to engine: OpaquePointer) {
        refresh()
        mmk_engine_set_run_counters(engine, status.score, status.coins, status.lives)
    }

    func extraLives(_ n: Int) {
        mmk_challenge_extra_lives(handle, UInt32(n))
        refresh()
    }

    private var pendingNext: UInt8?

    /// A death (or a start-over): true if the run continues on this course.
    func died() -> Bool {
        let n = mmk_challenge_finish(handle, UInt8(MMK_OUTCOME_DIED), 0, 0)
        persist()
        if n == UInt8(MMK_NEXT_RETRY) {
            if case .playing(let s) = stage { applyCounters(to: s.engine) }
            return true
        }
        pendingNext = n
        refresh()
        return false
    }

    func cleared(score: UInt32, coins: UInt32) {
        pendingNext = mmk_challenge_finish(handle, UInt8(MMK_OUTCOME_CLEARED), score, coins)
        persist()
        refresh()
        if pendingNext == UInt8(MMK_NEXT_COMPLETE) { collectCredits() }
    }

    func skipped() {
        pendingNext = mmk_challenge_finish(handle, UInt8(MMK_OUTCOME_SKIPPED), 0, 0)
        persist()
    }

    func saveAndQuit() {
        pendingNext = mmk_challenge_finish(handle, UInt8(MMK_OUTCOME_SAVE_QUIT), 0, 0)
        persist()
    }

    /// The course screen closed (after its result card, a skip or Save & Quit).
    private func sessionEnded() {
        let n = pendingNext ?? UInt8(MMK_NEXT_QUIT)
        pendingNext = nil
        next(n)
    }

    private func next(_ n: UInt8) {
        persist()
        switch Int32(n) {
        case Int32(MMK_NEXT_PLAY): showIntro()
        case Int32(MMK_NEXT_GAME_OVER): refresh(); stage = .gameOver
        case Int32(MMK_NEXT_COMPLETE):
            let u = mmk_challenge_take_unlock(handle)
            unlocked = u >= 0 ? Int(u) : nil
            persist()
            stage = .complete
        default: backToSelect()
        }
    }

    private func collectCredits() {
        var out: [Course] = []
        for i in 0..<mmk_challenge_cleared_count(handle) {
            var c = MmkRunCourse()
            if mmk_challenge_cleared(handle, i, &c) {
                out.append(Course(path: Self.str(c.id), name: Self.str(c.name), maker: Self.str(c.maker),
                                  difficulty: Int(c.difficulty)))
            }
        }
        credits = out
    }

    private func refresh() {
        var s = MmkRunStatus()
        mmk_challenge_status(handle, &s)
        status = s
    }

    private func persist() {
        let n = mmk_challenge_save_json(handle, nil, 0)
        var buf = [CChar](repeating: 0, count: n + 1)
        guard mmk_challenge_save_json(handle, &buf, buf.count) == n else { return }
        let json = String(cString: buf)
        try? FileManager.default.createDirectory(at: AppPaths.support, withIntermediateDirectories: true)
        try? json.write(to: Self.saveURL, atomically: true, encoding: .utf8)
    }

    private static func str<T>(_ tuple: T) -> String {
        withUnsafeBytes(of: tuple) { String(cString: $0.bindMemory(to: CChar.self).baseAddress!) }
    }
}

// MARK: - views

struct ChallengeView: View {
    @ObservedObject var flow: ChallengeFlow

    var body: some View {
        Group {
            switch flow.stage {
            case .select: DifficultySelect(flow: flow)
            case .intro: CourseIntro(flow: flow)
            case .playing(let s): GameView(session: s).id(s.id)
            case .gameOver: RunEnd(flow: flow, won: false)
            case .complete: RunEnd(flow: flow, won: true)
            }
        }
        .alert(flow.message ?? "", isPresented: Binding(get: { flow.message != nil }, set: { if !$0 { flow.message = nil } })) {
            Button("OK", role: .cancel) {}
        }
    }
}

/// Backdrop shared by the challenge screens.
private struct ChallengeBackdrop<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        ZStack {
            LinearGradient(colors: [Color(red: 0.36, green: 0.58, blue: 0.99), Color(red: 0.55, green: 0.80, blue: 1.0)],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea()
            content.foregroundStyle(.white)
        }
    }
}

private struct DifficultySelect: View {
    @ObservedObject var flow: ChallengeFlow

    var body: some View {
        ChallengeBackdrop {
            VStack(spacing: 22) {
                Text("100 Mario Challenge").font(.system(size: 40, weight: .heavy, design: .rounded))
                Text("100 lives. Random courses from your library. Skip any course for free.")
                    .font(.title3.weight(.medium)).opacity(0.9)
                HStack(spacing: 16) {
                    ForEach(0..<4, id: \.self) { d in card(d) }
                }
                Button("Back to library") { flow.exit() }
                    .keyboardShortcut(.cancelAction)
                    .controlSize(.large)
            }
            .padding(32)
        }
    }

    private func card(_ d: Int) -> some View {
        let open = flow.unlocked(d)
        let n = flow.available(d)
        return VStack(spacing: 10) {
            Text(ChallengeFlow.difficulties[d]).font(.system(size: 22, weight: .heavy, design: .rounded))
            Text("\(ChallengeFlow.runLengths[d]) courses").font(.headline)
            if open {
                Text("\(n) playable in library").font(.caption).opacity(0.85)
                if flow.clears(d) > 0 { Text("Cleared ×\(flow.clears(d))").font(.caption.weight(.bold)) }
                Spacer(minLength: 4)
                Button("New Game") { flow.start(d, resume: false) }
                    .buttonStyle(.borderedProminent)
                    .disabled(n == 0)
                Button("Continue") { flow.start(d, resume: true) }
                    .disabled(!flow.canContinue(d))
            } else {
                Image(systemName: "lock.fill").font(.system(size: 30))
                Text(d == 2 ? "Clear a Normal run to unlock" : "Clear an Expert run to unlock")
                    .font(.caption).multilineTextAlignment(.center)
                Spacer(minLength: 4)
            }
        }
        .frame(width: 170, height: 220)
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 16).fill(.black.opacity(open ? 0.35 : 0.55)))
    }
}

private struct CourseIntro: View {
    @ObservedObject var flow: ChallengeFlow

    var body: some View {
        ChallengeBackdrop {
            VStack(spacing: 16) {
                Text("\(ChallengeFlow.difficulties[flow.difficulty]) · Course \(flow.status.course_number)/\(flow.status.total)")
                    .font(.title2.weight(.bold))
                if let c = flow.course {
                    Text(c.name).font(.system(size: 36, weight: .heavy, design: .rounded))
                        .multilineTextAlignment(.center).lineLimit(2)
                    if !c.maker.isEmpty { Text("by \(c.maker)").font(.title3) }
                    Text("Difficulty \(c.difficulty)").font(.caption).opacity(0.8)
                }
                Label("× \(flow.status.lives)", systemImage: "person.fill")
                    .font(.system(size: 28, weight: .heavy, design: .rounded).monospacedDigit())
                HStack(spacing: 12) {
                    Button("Start") { flow.play() }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                    Button("Skip") { flow.skipIntro() }
                        .keyboardShortcut("s", modifiers: [])
                    Button("Save & Quit") { flow.backToSelect() }
                        .keyboardShortcut(.cancelAction)
                }
                .controlSize(.large)
            }
            .padding(40)
            .background(RoundedRectangle(cornerRadius: 22).fill(.black.opacity(0.4)))
        }
    }
}

private struct RunEnd: View {
    @ObservedObject var flow: ChallengeFlow
    let won: Bool

    var body: some View {
        ChallengeBackdrop {
            VStack(spacing: 16) {
                Text(won ? "Run cleared!" : "Game over")
                    .font(.system(size: 44, weight: .heavy, design: .rounded))
                if won {
                    Text("\(ChallengeFlow.difficulties[flow.difficulty]) · \(flow.status.total) courses · \(flow.status.lives) lives left")
                        .font(.title3)
                    if let u = flow.unlocked {
                        Text("\(ChallengeFlow.difficulties[u]) is now open!")
                            .font(.title2.weight(.heavy)).foregroundStyle(.yellow)
                    }
                    if !flow.credits.isEmpty {
                        Text("Course design").font(.headline).padding(.top, 8)
                        ScrollView {
                            VStack(spacing: 6) {
                                ForEach(Array(flow.credits.enumerated()), id: \.offset) { _, c in
                                    VStack(spacing: 0) {
                                        Text(c.name).font(.body.weight(.semibold))
                                        if !c.maker.isEmpty { Text(c.maker).font(.caption).opacity(0.8) }
                                    }
                                }
                            }
                        }
                        .frame(maxHeight: 260)
                    }
                } else {
                    Text("Course \(flow.status.course_number)/\(flow.status.total) · \(flow.status.deaths) lives lost on it")
                        .font(.title3)
                }
                Button("Back") { flow.backToSelect() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
            }
            .padding(40)
            .background(RoundedRectangle(cornerRadius: 22).fill(.black.opacity(0.4)))
        }
    }
}

/// Top banner while a run's course is played: course n/total and lives (banked extras).
struct RunBanner: View {
    @ObservedObject var run: ChallengeFlow

    var body: some View {
        VStack {
            HStack(spacing: 14) {
                Text("\(ChallengeFlow.difficulties[run.difficulty]) \(run.status.course_number)/\(run.status.total)")
                if let c = run.course { Text(c.name).lineLimit(1) }
                if run.status.banked > 0 { Text("+\(run.status.banked) banked").foregroundStyle(.yellow) }
            }
            .font(.system(size: 13, weight: .bold, design: .rounded))
            .padding(.horizontal, 12).padding(.vertical, 5)
            .background(Capsule().fill(.black.opacity(0.45)))
            .foregroundStyle(.white)
            .padding(.top, 34)
            Spacer()
        }
        .allowsHitTesting(false)
    }
}
