import CMmk
import SwiftUI

// The SMM2 title screen and its Play menu, drawn with the SMM2 layouts (`mmk import-smm2-ui`)
// over one of the game's title courses running a scripted demo.
//
// Layout `Play_Title_00` holds every state; its own animations move between them [obs data]:
//   top       logo + "Press [L] + [R]" prompt blinking (LoopPressKey)
//   PressKey  top → main: the prompt fades out, the Make / Play clapperboards fade in
//   main      L_EditBtn_00 "Make", L_PlayBtn_00 "Play" (Cmn_ClapperboardBtn_00: Select = bar
//             open, Decide = clap)
//   DecidePlayBtn   main → category; DecideBackBtn  category → main;
//   DecideBackBtnTop main → top
//   category  L_QuestBtn_00 "Story Mode", L_NetworkBtn_00 "Course World",
//             L_CourseRobotPlayBtn_00 "Coursebot" (Cmn_MainMenuBtn_00: Select = grow 3%,
//             Decide = flash)
//   Type      logo letters black (frame 0) or white (frame 1)
// From the game's title page (private research/specs/smm2-frontend.md "Title") [obs]: the four
// layout states are an animation graph (prompt →PressKey→ Make/Play →DecidePlayBtn→ Play menu,
// back by DecideBackBtnTop / DecideBackBtn, Make →DecideEditBtn→ editor); the prompt puts the
// cursor on Make; the Play menu opens with the cursor on Story Mode and going back from it puts
// it on Play; the choices change scene with the black fade (AppModel).
// Inferred [inf]: which logo colour goes with which course (by the course's sky colour), the
// cursor (`Cmn_Cursor_00`, sized to the selected button's N_Carsor_00 / B_Btn_00 as the panes'
// names suggest), how long a button's Decide plays before its choice acts. Departures: a
// fourth Play-menu button "100 Mario Challenge" (a copy of the Coursebot button, 116 below it,
// with the castle icon); Make plays its clap and says "coming soon" (no editor in the app yet).

enum TitleChoice {
    case storyMode, courseWorld, coursebot, hundredMario
}

@MainActor
final class TitleScene: UIScene {
    enum State { case top, main, category, leaving }

    let title: UILayout
    let cursor: UILayout?
    private(set) var tick = 0
    private(set) var state: State
    /// 0 Make, 1 Play; the prompt puts the cursor on Make [obs].
    private(set) var mainSelection = 0
    private(set) var categorySelection = 0
    private var busyUntil = 0
    private var pending: (choice: TitleChoice, at: Int)?
    var onChoice: ((TitleChoice) -> Void)?
    /// Make was chosen (the Course Maker isn't there yet).
    var onComingSoon: (() -> Void)?

    static let mainButtons = ["L_EditBtn_00", "L_PlayBtn_00"]
    static let categoryButtons = ["L_QuestBtn_00", "L_NetworkBtn_00", "L_CourseRobotPlayBtn_00", "L_HundredBtn_00"]
    static let categoryChoices: [TitleChoice] = [.storyMode, .courseWorld, .coursebot, .hundredMario]
    /// Animations that move between the top / main / category states.
    private static let transitions = ["PressKey", "DecideBackBtnTop", "DecidePlayBtn", "DecideBackBtn"]

    var layers: [UILayout] { [title] + (cursor.map { [$0] } ?? []) }

    init?(ui: LayoutUI, start: State, version: String) {
        guard let t = ui.layout("Play_Title_00") else { return nil }
        title = t
        cursor = ui.layout("Cmn_Cursor_00")
        state = start == .leaving ? .main : start
        t.text("T_Num_00", version)
        // the fourth Play-menu button (departure, see the file notes)
        if t.clone("L_CourseRobotPlayBtn_00", as: "L_HundredBtn_00", offset: SIMD2(0, -116)) {
            t.text("L_HundredBtn_00/T_Btn_00", "100 Mario Challenge")
            t.visible("L_HundredBtn_00/N_RobotIcon_00", false)
            t.visible("L_HundredBtn_00/N_QuestIcon_01", true)
            // four buttons don't fit between the logo (bottom y 80) and the copyright line
            // (top y −313) at full size: the list at 80%, its top at y 70 (layout units)
            t.setPane("N_CategoryPlay_00", translate: SIMD2(0, 34), scale: SIMD2(0.8, 0.8))
        }
        // "new" badges: nothing is new here
        for b in Self.categoryButtons { t.visible("\(b)/L_NewIcon_00", false) }
        setDark(false)
        t.play("LoopPressKey", at: 0, loop: true)
        for b in Self.mainButtons { t.hold("\(b)/Select", 0) }
        for b in Self.categoryButtons { t.hold("\(b)/Select", 0) }
        cursor?.play("Loop", at: 0, loop: true)
        switch state {
        case .main:
            t.hold("PressKey", t.length("PressKey"))
        case .category:
            t.hold("PressKey", t.length("PressKey"))
            t.hold("DecidePlayBtn", t.length("DecidePlayBtn"))
        default:
            break
        }
        selectMain(mainSelection, animate: false)
        selectCategory(categorySelection, animate: false)
        placeCursor()
    }

    /// Logo letters white over a dark course, black over a light one.
    func setDark(_ dark: Bool) { title.hold("Type", dark ? 1 : 0) }

    /// Debug captures: start the Play menu with button `i` selected.
    func debugSelectCategory(_ i: Int) { selectCategory(i, animate: false) }

    // MARK: step

    func step(_ input: UIInput) {
        tick += 1
        if let p = pending, tick >= p.at {
            pending = nil
            onChoice?(p.choice)
        }
        var pressed = input.pressed
        if let s = Self.scripted[tick] { pressed.formUnion(s) }
        if tick >= busyUntil {
            handle(pressed)
        }
        placeCursor()
    }

    /// Debug captures: `MMK_UI_INPUT=<tick>:<button>[,…]` (confirm, back, up, down, left, right)
    /// presses those buttons at those scene ticks.
    private static let scripted: [Int: MenuInput] = {
        var out: [Int: MenuInput] = [:]
        let names: [String: MenuInput] = ["confirm": .confirm, "back": .back, "up": .up, "down": .down, "left": .left, "right": .right]
        for item in (ProcessInfo.processInfo.environment["MMK_UI_INPUT"] ?? "").split(separator: ",") {
            let kv = item.split(separator: ":")
            if kv.count == 2, let t = Int(kv[0]), let b = names[String(kv[1])] { out[t, default: []].insert(b) }
        }
        return out
    }()

    private func handle(_ pressed: MenuInput) {
        switch state {
        case .top:
            // "Press L + R": any menu button brings the menu up
            if !pressed.isEmpty { toMain(from: .top) }
        case .main:
            if pressed.contains(.left) || pressed.contains(.right) {
                selectMain(pressed.contains(.left) ? 0 : 1, animate: true)
            } else if pressed.contains(.confirm) {
                let b = Self.mainButtons[mainSelection]
                title.play("\(b)/Decide", at: tick)
                if mainSelection == 0 {
                    busyUntil = tick + Int(title.length("\(b)/Decide"))
                    onComingSoon?()
                } else {
                    // the clap, then the menu swaps to the category buttons; the Play menu
                    // opens on Story Mode [obs]
                    let clap = Int(title.length("\(b)/Decide"))
                    restart("DecidePlayBtn", at: tick + clap)
                    state = .category
                    selectCategory(0, animate: false)
                    busyUntil = tick + clap + Int(title.length("DecidePlayBtn"))
                }
            } else if pressed.contains(.back) {
                title.stop("PressKey")
                restart("DecideBackBtnTop", at: tick)
                state = .top
                busyUntil = tick + Int(title.length("DecideBackBtnTop"))
                mainSelection = 0    // Wait: the cursor goes back to Make [obs]
            }
        case .category:
            let n = Self.categoryButtons.count
            if pressed.contains(.up) {
                selectCategory((categorySelection + n - 1) % n, animate: true)
            } else if pressed.contains(.down) {
                selectCategory((categorySelection + 1) % n, animate: true)
            } else if pressed.contains(.confirm) {
                let b = Self.categoryButtons[categorySelection]
                title.play("\(b)/Decide", at: tick)
                state = .leaving
                // the button's flash (frames 0–8 of Decide), then the next screen [inf]
                pending = (Self.categoryChoices[categorySelection], tick + 24)
            } else if pressed.contains(.back) {
                mainSelection = 1    // back from the Play menu: the cursor on Play [obs]
                toMain(from: .category)
            }
        case .leaving:
            break
        }
    }

    private func toMain(from: State) {
        if from == .top {
            title.stop("DecideBackBtnTop")
            restart("PressKey", at: tick)
            busyUntil = tick + Int(title.length("PressKey"))
        } else {
            restart("DecideBackBtn", at: tick)
            busyUntil = tick + Int(title.length("DecideBackBtn"))
        }
        state = .main
        selectMain(mainSelection, animate: false)
    }

    /// Re-bind `anim` last, so it wins over the transitions bound before it.
    private func restart(_ anim: String, at t: Int) {
        title.stop(anim)
        title.play(anim, at: t)
    }

    private func selectMain(_ i: Int, animate: Bool) {
        mainSelection = i
        for (k, b) in Self.mainButtons.enumerated() {
            // Select's last frame: the clapper bar open
            title.hold("\(b)/Select", k == i ? title.length("\(b)/Select") : 0)
        }
    }

    private func selectCategory(_ i: Int, animate: Bool) {
        categorySelection = i
        for (k, b) in Self.categoryButtons.enumerated() {
            if k == i {
                if animate { title.play("\(b)/Select", at: tick) } else { title.hold("\(b)/Select", title.length("\(b)/Select")) }
            } else {
                title.hold("\(b)/Select", 0)
            }
        }
    }

    /// The cursor frames the selected button and fades with it.
    private func placeCursor() {
        guard let cursor else { return }
        let target: String? = switch state {
        case .top: nil
        case .main: "\(Self.mainButtons[mainSelection])/N_Carsor_00"
        case .category, .leaving: "\(Self.categoryButtons[categorySelection])/B_Btn_00"
        }
        title.apply(tick: tick)
        guard let target, let r = title.rect(target), r.visible else {
            cursor.setPane("N_All_00", alpha: 0)
            return
        }
        cursor.setPane("W_SelectFrame_00", translate: r.center, size: r.size)
        cursor.setPane("N_All_00", alpha: 255 * r.alpha * (state == .leaving ? 0 : 1))
    }
}

/// The title screen: the scene, and the demo course running behind it (when the SMM2 asset cache
/// has the title courses).
@MainActor
final class TitleFlow: ObservableObject, Identifiable {
    nonisolated let id = UUID()
    let ui: LayoutUI
    let scene: TitleScene
    let session: GameSession?
    @Published var toast: String?
    var onChoice: ((TitleChoice) -> Void)?
    private var courses: [URL] = []
    private var courseIndex = 0
    private var toastTask: DispatchWorkItem?
    private var switching = false

    /// Demo length per course before moving on (ticks).
    static let demoTicks: UInt64 = 60 * 40

    init?(ui: LayoutUI, assets: EngineAssets?, start: TitleScene.State) {
        guard let scene = TitleScene(ui: ui, start: start, version: Self.version(assets)) else { return nil }
        self.ui = ui
        self.scene = scene
        let env = ProcessInfo.processInfo.environment
        // the title courses: the asset cache's built-in courses, else the UI import's copies
        var list: [URL] = []
        for dir in [assets?.url, ui.dir].compactMap({ $0?.appendingPathComponent("courses") }) where list.isEmpty {
            let files = (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
            list = files.filter { $0.lastPathComponent.hasPrefix("title_course_data_") }
                .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        }
        // only the ones the engine can draw and play (NSMBU / 3D World wait for the mesh renderer)
        let playable = list.filter { url in
            guard let d = try? Data(contentsOf: url), case .success(let info) = CourseInfo.read(d, assets: assets) else { return false }
            return info.playable
        }
        if !playable.isEmpty { list = playable }
        // MMK_TITLE_COURSE=<n>: start with title course n (captures)
        let n = max(1, list.count)
        let index = env["MMK_TITLE_COURSE"].flatMap(Int.init).map { max(0, $0) % n } ?? Int.random(in: 0..<n)
        var session: GameSession?
        if let assets, !list.isEmpty, let data = try? Data(contentsOf: list[index]) {
            session = try? GameSession(assets: assets, course: data, title: "Title", courseKey: nil, saves: nil)
        }
        courses = list
        courseIndex = index
        self.session = session
        scene.onChoice = { [weak self] c in self?.onChoice?(c) }
        scene.onComingSoon = { [weak self] in self?.show("The Course Maker is coming soon.") }
        guard let session else { return }
        session.hideHud = true
        session.scriptedInput = { [weak self] tick in
            if tick == Self.demoTicks { self?.requestNextCourse() }
            return Self.demoInput(tick)
        }
        session.onDemoEnd = { [weak self] in self?.requestNextCourse() }
        session.sceneOverlay = (scene, ui)
        updateLogo()
    }

    /// Scripted demo: walk right, a jump every two seconds [inf: the game lets the player move
    /// on the title course; this stands in for a player].
    static func demoInput(_ tick: UInt64) -> MmkInput {
        var b = UInt32(MMK_BTN_RIGHT)
        if tick < 60 { b = 0 }   // stand still for a second first
        if tick % 120 >= 90 && tick % 120 < 115 { b |= UInt32(MMK_BTN_JUMP) }
        return MmkInput(buttons: b)
    }

    /// Called from inside the engine step: switch once, after the step.
    private func requestNextCourse() {
        guard !switching else { return }
        switching = true
        DispatchQueue.main.async { [weak self] in
            self?.nextCourse()
            self?.switching = false
        }
    }

    private func nextCourse() {
        guard let session, !courses.isEmpty else { return }
        courseIndex = (courseIndex + 1) % courses.count
        if let data = try? Data(contentsOf: courses[courseIndex]) { session.loadDemoCourse(data) }
        updateLogo()
    }

    /// White letters over a dark sky (underground, castle, night…), black over a light one.
    private func updateLogo() {
        guard let session else { return }
        let c = mmk_engine_clear_color(session.engine)
        let r = Double(c >> 24) / 255, g = Double((c >> 16) & 0xff) / 255, b = Double((c >> 8) & 0xff) / 255
        scene.setDark(0.2126 * r + 0.7152 * g + 0.0722 * b < 0.35)
    }

    private func show(_ message: String) {
        toast = message
        toastTask?.cancel()
        let task = DispatchWorkItem { [weak self] in self?.toast = nil }
        toastTask = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.5, execute: task)
    }

    /// The game build of the SMM2 import, for the version box ("3.0.3").
    private static func version(_ assets: EngineAssets?) -> String {
        if let dir = LayoutUI.smm2?.dir,
           let data = try? Data(contentsOf: dir.appendingPathComponent("manifest.json")),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let build = json["build"] as? String, build != "unknown" {
            return build
        }
        return ""
    }
}

struct TitleView: View {
    @ObservedObject var flow: TitleFlow

    var body: some View {
        ZStack {
            if let s = flow.session {
                MetalView(session: s).id(s.id)
            } else {
                UIScreenView(scene: flow.scene, ui: flow.ui)
            }
            if let t = flow.toast {
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
            }
        }
        .animation(.easeInOut(duration: 0.2), value: flow.toast)
    }
}
