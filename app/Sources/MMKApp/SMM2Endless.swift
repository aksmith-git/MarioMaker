import CMmk
import Foundation

// The 100 Mario Challenge presented with SMM2's Endless Challenge screens (Settings > Game, the
// default): the rules are SMM1's (core/crates/modes: Easy / Normal / Expert / Super Expert runs
// of 8 / 16 / 16 / 6 courses, 100 lives, Continue, unlocks); the screens are SMM2's own
// layouts, read at run time:
// - `Nwk_EndlessModeMenu_00`: difficulty tabs (`Nwk_MCDifflculityBtn_00` Easy … Super Expert)
//   over the map part `Nwk_WorldMap_00` (TypeDifficulty per tab), New Game / Continue
//   (`TypeStartBtn` 0 both, 1 New Game alone with the rule text); In (25) [obs layout];
// - starting a run plays ChangeMap (the frame opens up to the whole map), then on a new run the
//   Telop ("Mario embarks on a journey!") with StartMario (he pops out of the castle), then he
//   walks to the first dot (MoveMario_00); dots: cleared ones blue (Type 0), the next one orange
//   (Type 2, InPlay pop), the rest hidden [inf]; after a clear the dot's InClear, then the walk to
//   the next (MoveMario_n; Scroll_00 brings the map's second half at dot 16);
// - at the dot Mario's "Play" balloon (`Nwk_WorldMapMario_00` Select) waits for confirm, then his
//   Decide jump and the black fade into the course (the course card `Play_CourseIn_00`
//   AnimEndlessMode with the lives, the HUD with lives, pause / clear / give-up from
//   SMM2Course.swift); back on the map is Save & Quit (`L_EndBtn_00`);
// - the map's counters: lives (T_RestMarioNum_00) and courses cleared; the high score and the
//   world ranking (network) are hidden [departure];
// - SMM2's Endless Challenge has no locked tiers: Expert / Super Expert show their tab greyed
//   until SMM1's unlock rule opens them, and a run's last clear goes back to the menu with the
//   unlock message (SMM2 has no ending for an endless run) [departure].
// The sequence timings between those animations are not in the data [inf]; the Endless
// sequence's own states are in the private research/specs/smm2-frontend.md.

/// The Endless menu layout and its map part, shared by the menu and the run's map.
@MainActor
final class EndlessBoard {
    let layout: UILayout
    /// The New Game / Continue buttons drawn from a second instance over the first: in the
    /// layout they sit under the map frame's bottom band (`W_Frame_00`, drawn after them, opaque
    /// there), where the game shows them [inf: how the game orders them isn't in the data].
    let buttons: UILayout?
    let ui: LayoutUI
    private(set) var difficulty = 0

    var layers: [UILayout] { [layout] + (buttons.map { [$0] } ?? []) }

    /// Both instances (animations of the whole screen).
    func both(_ f: (UILayout) -> Void) {
        f(layout)
        if let b = buttons { f(b) }
    }

    static let map = "L_WorldMap_00"
    static let tabs = ["L_Btn_00", "L_Btn_01", "L_Btn_02", "L_Btn_03"]
    static let mario = "L_WorldMap_00/L_WorldMapMario_00"
    /// Dots on the first half of the map (Scroll_00 shows the second).
    static let halfDots = 16

    init?(ui: LayoutUI) {
        guard let l = ui.layout("Nwk_EndlessModeMenu_00") else { return nil }
        self.ui = ui
        layout = l
        buttons = ui.layout("Nwk_EndlessModeMenu_00")
        if let b = buttons {
            for p in ["P_BG_00", "N_Scissor_00", "N_ChangeMap_00", "N_ChangeMap_01"] { b.visible(p, false) }
        }
        l.visible("N_StartBtnMap_00", false)
        l.text("T_Title_00", "100 Mario Challenge")
        l.play("\(Self.map)/Loop", at: 0, loop: true)
        l.play("\(Self.map)/LoopRandom", at: 0, loop: true)
        // the network's record and ranking panes; the map's own Quit key while choosing
        for p in ["\(Self.map)/N_Ranking_00", "\(Self.map)/N_BreakRecord_00", "\(Self.map)/N_BestClearCount_00",
                  "\(Self.map)/T_Ranking_00", "\(Self.map)/L_WorldMapBtn_00"] {
            l.visible(p, false)
        }
        for t in Self.tabs { l.hold("\(t)/Select", 0) }
    }

    func dot(_ i: Int) -> String { String(format: "%@/L_WorldMapCourse_%02d", Self.map, i) }
    func info(_ i: Int) -> String { String(format: "%@/L_WorldMapInfo_%02d", Self.map, i) }

    func setDifficulty(_ d: Int) {
        difficulty = d
        layout.hold("\(Self.map)/TypeDifficulty", Float(d))
    }

    /// Cleared dots blue and small, the next one orange, later ones hidden [inf].
    func setDots(cleared: Int, total: Int, current: Bool = true) {
        for i in 0..<32 {
            let d = dot(i)
            layout.visible(info(i), false)
            if i < cleared {
                layout.visible(d, nil)
                layout.stop("\(d)/InPlay")
                layout.stop("\(d)/InClear")
                layout.hold("\(d)/Type", 0)
            } else if i == cleared && i < total && current {
                layout.visible(d, nil)
                layout.hold("\(d)/Type", 2)
            } else {
                layout.visible(d, false)
            }
        }
    }

    /// Mario standing on dot `i` (nil: still in the castle), the map scrolled to its half.
    func placeMario(_ i: Int?) {
        let l = layout
        for k in 0...32 { l.stop("\(Self.map)/MoveMario_\(String(format: "%02d", k))") }
        l.stop("\(Self.map)/StartMario")
        if let i {
            let a = "\(Self.map)/MoveMario_\(String(format: "%02d", min(i, 31)))"
            l.hold(a, l.length(a))
            l.hold("\(Self.mario)/Stop", 0)
        } else {
            l.hold("\(Self.map)/StartMario", 0)
        }
        scroll(to: i ?? 0, animate: false, at: 0)
    }

    func scroll(to i: Int, animate: Bool, at tick: Int) {
        let a = "\(Self.map)/Scroll_00"
        if i >= Self.halfDots {
            if animate { layout.play(a, at: tick) } else { layout.hold(a, layout.length(a)) }
        } else {
            layout.stop(a)
        }
    }

    /// The map's counters: lives left and courses cleared.
    func setCounters(lives: Int, cleared: Int) {
        layout.text("\(Self.map)/T_RestMarioNum_00", "\(lives)")
        layout.text("\(Self.map)/L_ClearCourseCount_00/T_Count_00", "\(cleared)")
        layout.text("\(Self.map)/L_ClearCourseCount_01/T_Count_00", "\(cleared)")
    }
}

// MARK: - difficulty select

/// Difficulty tabs, New Game / Continue over the map. ←/→ change the difficulty, ↑/↓ move
/// between the tab row and the buttons, confirm starts, back leaves.
@MainActor
final class EndlessMenuScene: UIScene {
    private weak var flow: ChallengeFlow?
    let board: EndlessBoard
    let cursor: MenuCursor
    private(set) var tick = 0
    private(set) var difficulty: Int
    /// 0 New Game, 1 Continue.
    private var button = 0
    private var onTabs = false
    private var busyUntil = 0
    private var started = false

    var layers: [UILayout] { board.layers + cursor.layers }

    init?(flow: ChallengeFlow, ui: LayoutUI, difficulty: Int) {
        guard let b = EndlessBoard(ui: ui) else { return nil }
        self.flow = flow
        board = b
        cursor = MenuCursor(ui: ui)
        self.difficulty = difficulty
        b.both { $0.play("In", at: 0) }
        b.layout.visible("\(EndlessBoard.map)/L_EndBtn_00", false)
        busyUntil = Int(b.layout.length("In"))
        refresh()
    }

    /// The layer holding the start buttons.
    private var buttonLayer: UILayout { board.buttons ?? board.layout }

    private var unlocked: Bool { flow?.unlocked(difficulty) ?? false }
    private var canContinue: Bool { unlocked && (flow?.canContinue(difficulty) ?? false) }

    /// Tabs, buttons and the map for the current difficulty (the Continue record's dots).
    private func refresh() {
        let l = board.layout
        board.setDifficulty(difficulty)
        for (d, t) in EndlessBoard.tabs.enumerated() {
            if d == difficulty {
                l.hold("\(t)/Decide", l.length("\(t)/Decide"))   // the chosen tab stays dark [inf]
            } else {
                l.stop("\(t)/Decide")
            }
            // locked tiers greyed (SMM1's unlock rule) [departure]
            l.setPane(t, alpha: (flow?.unlocked(d) ?? false) ? 255 : 110)
        }
        let cont = flow?.continueInfo(difficulty)
        let bl = buttonLayer
        board.both { $0.hold("TypeStartBtn", canContinue ? 0 : 1) }
        if !canContinue { button = 0 }
        bl.text("T_Rule_00", flow.map { "\(ChallengeFlow.runLengths[difficulty]) courses, \($0.unlocked(difficulty) ? "100 lives" : ChallengeFlow.lockedMessage(difficulty))" } ?? "")
        board.setDots(cleared: cont?.cleared ?? 0, total: ChallengeFlow.runLengths[difficulty], current: cont != nil)
        board.placeMario(cont.map { max(0, $0.cleared - 1) })
        board.setCounters(lives: cont?.lives ?? 100, cleared: cont?.cleared ?? 0)
        for (i, p) in ["L_StartBtn_00", "L_StartBtn_01"].enumerated() {
            bl.hold("\(p)/Select", i == button && !onTabs ? bl.length("\(p)/Select") : 0)
            // locked: the buttons' Invalid look (bound last, over Select) [departure]
            if unlocked { bl.stop("\(p)/Invalid") } else { bl.hold("\(p)/Invalid", 0) }
        }
    }

    func step(_ input: UIInput) {
        tick += 1
        let p = input.pressed.union(DebugInput.step())
        if tick >= busyUntil && !started { handle(p) }
        if onTabs {
            cursor.place(on: board.layout, pane: "\(EndlessBoard.tabs[difficulty])/B_Btn_00", tick: tick, alpha: started ? 0 : 1)
        } else {
            cursor.place(on: buttonLayer, pane: "L_StartBtn_0\(button)/B_Btn_00", tick: tick, alpha: started ? 0 : 1)
        }
    }

    private func handle(_ p: MenuInput) {
        let l = board.layout
        if p.contains(.back) {
            flow?.exit()
            return
        }
        if p.contains(.up) { onTabs = true; refresh(); return }
        if p.contains(.down) { onTabs = false; refresh(); return }
        if p.contains(.left) || p.contains(.right) {
            let right = p.contains(.right)
            if onTabs {
                let d = max(0, min(3, difficulty + (right ? 1 : -1)))
                guard d != difficulty else { return }
                difficulty = d
                l.play("\(EndlessBoard.tabs[d])/Select", at: tick)
                // the map fades over to the other difficulty's (ChangeOut / ChangeIn, 5 + 5) [inf]
                l.play("ChangeOut", at: tick)
                l.play("ChangeIn", at: tick + 5)
                refresh()
            } else if canContinue {
                button = right ? 1 : 0
                buttonLayer.play("L_StartBtn_0\(button)/Select", at: tick)
                refresh()
            }
            return
        }
        if p.contains(.confirm) {
            if onTabs {
                onTabs = false
                refresh()
                return
            }
            guard unlocked else {
                flow?.message = ChallengeFlow.lockedMessage(difficulty)
                return
            }
            let resume = button == 1 && canContinue
            buttonLayer.play("L_StartBtn_0\(button)/Decide", at: tick)
            started = true
            flow?.endlessHandoff = board
            let wait = Int(buttonLayer.length("L_StartBtn_00/Decide"))
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(wait) / 60) { [weak self] in
                guard let self else { return }
                self.flow?.start(self.difficulty, resume: resume)
            }
        }
    }
}

// MARK: - the run's map

/// Before each course: the map (ChangeMap from the menu, the Telop and Mario out of the castle
/// on a new run, or the clear pop and the walk to the next dot), Mario's "Play" balloon, confirm
/// → his jump and the black fade into the course. Back = Save & Quit.
@MainActor
final class EndlessMapScene: UIScene {
    enum Entry { case newGame, resume, afterClear }

    private weak var flow: ChallengeFlow?
    let board: EndlessBoard
    private(set) var tick = 0
    private let cleared: Int
    private var balloonAt: Int?
    private var waiting = false
    private var leaving = false
    private var actions: [(at: Int, run: () -> Void)] = []

    var layers: [UILayout] { board.layers }

    /// `cleared`: courses cleared before this one (this course's dot).
    init?(flow: ChallengeFlow, ui: LayoutUI, entry: Entry, difficulty: Int, lives: Int, cleared: Int,
          theme: Int?, handoff: EndlessBoard?) {
        guard let b = (handoff?.difficulty == difficulty ? handoff : nil) ?? EndlessBoard(ui: ui) else { return nil }
        self.flow = flow
        board = b
        self.cleared = cleared
        let l = b.layout
        let total = ChallengeFlow.runLengths[difficulty]
        b.setDifficulty(difficulty)
        b.setCounters(lives: lives, cleared: cleared)
        l.visible("\(EndlessBoard.map)/L_EndBtn_00", nil)
        if let s = l.message("L_WorldMap_00-L_EndBtn_00-T_Btn_00") { l.text("\(EndlessBoard.map)/L_EndBtn_00/T_Btn_00", s) }
        if let s = l.message("L_WorldMap_00-L_WorldMapMario_00-T_Challenge_00") { l.text("\(EndlessBoard.mario)/T_Challenge_00", s) }
        if let s = l.message("L_WorldMap_00-T_Telop_00") { l.text("\(EndlessBoard.map)/T_Telop_00", s) }
        let mapIn = Int(l.length("ChangeMap"))
        switch entry {
        case .newGame:
            b.both { $0.play("ChangeMap", at: 0) }
            b.setDots(cleared: 0, total: total, current: false)
            b.placeMario(nil)
            // the Telop with Mario popping out of the castle (StartMario's jump is 130 frames
            // in), then the walk to the first dot [inf: order]
            l.play("\(EndlessBoard.map)/Telop", at: mapIn)
            l.play("\(EndlessBoard.map)/StartMario", at: mapIn)
            let walk = mapIn + Int(l.length("\(EndlessBoard.map)/Telop"))
            at(walk) { [weak self] in self?.arrive(at: 0, theme: theme, walkFrom: nil) }
        case .resume:
            b.both { $0.play("ChangeMap", at: 0) }
            b.setDots(cleared: cleared, total: total, current: false)
            b.placeMario(cleared > 0 ? cleared - 1 : nil)
            at(mapIn + 10) { [weak self] in self?.arrive(at: cleared, theme: theme, walkFrom: cleared > 0 ? cleared - 1 : nil) }
        case .afterClear:
            b.both { $0.hold("ChangeMap", $0.length("ChangeMap")) }
            b.setDots(cleared: cleared - 1, total: total, current: true)
            b.placeMario(max(0, cleared - 1))
            // the cleared dot pops to blue, then Mario walks on [inf: timing]
            let d = b.dot(cleared - 1)
            l.stop("\(d)/Type")
            l.play("\(d)/InClear", at: 30)
            at(30 + Int(l.length("\(d)/InClear")) + 10) { [weak self] in
                self?.arrive(at: cleared, theme: theme, walkFrom: cleared - 1)
            }
        }
    }

    private func at(_ t: Int, _ f: @escaping () -> Void) { actions.append((t, f)) }

    /// Walk to dot `i` (MoveMario_i), the dot's InPlay pop and the theme above it, then the
    /// "Play" balloon.
    private func arrive(at i: Int, theme: Int?, walkFrom: Int?) {
        let l = board.layout
        var t = tick
        if i == EndlessBoard.halfDots {
            board.scroll(to: i, animate: true, at: t)
            t += Int(l.length("\(EndlessBoard.map)/Scroll_00"))
        }
        let d = board.dot(i)
        l.visible(d, nil)
        l.hold("\(d)/Type", 2)
        l.play("\(d)/InPlay", at: t)
        let move = "\(EndlessBoard.map)/MoveMario_\(String(format: "%02d", min(i, 31)))"
        if walkFrom != nil || i == 0 {
            l.play("\(EndlessBoard.mario)/Walk", at: t, loop: true)
            l.play(move, at: t)
            t += Int(l.length(move))
        } else {
            l.hold(move, l.length(move))
        }
        if let theme {
            let info = board.info(i)
            l.visible(info, nil)
            l.hold("\(info)/SceneSkin", Float(max(0, min(9, theme))))
            l.play("\(info)/InPlay", at: t)
        }
        at(t) { [weak self] in
            guard let self else { return }
            l.stop("\(EndlessBoard.mario)/Walk")
            l.play("\(EndlessBoard.mario)/Select", at: self.tick)
            self.waiting = true
        }
    }

    func step(_ input: UIInput) {
        tick += 1
        let p = input.pressed.union(DebugInput.step())
        let due = actions.filter { $0.at <= tick }
        actions.removeAll { $0.at <= tick }
        for a in due { a.run() }
        guard !leaving else { return }
        if p.contains(.back) {
            // Quit on the map: the run stays saved for Continue
            leaving = true
            board.layout.play("\(EndlessBoard.map)/L_EndBtn_00/Decide", at: tick)
            SceneFade.shared.run(.black) { [weak self] in self?.flow?.backToSelect() }
            return
        }
        if waiting && p.contains(.confirm) {
            waiting = false
            leaving = true
            let l = board.layout
            l.play("\(EndlessBoard.mario)/Decide", at: tick)
            // Mario's jump (Decide, 130 frames), then the fade into the course [inf]
            let jump = Int(l.length("\(EndlessBoard.mario)/Decide"))
            at(tick + max(30, jump - 40)) { [weak self] in
                SceneFade.shared.run(.black) { self?.flow?.play() }
            }
        }
    }
}
