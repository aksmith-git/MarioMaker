import CMmk
import Foundation

// 100 Mario Challenge screens drawn with the SMM1 layouts (owner's import). Plain classes that
// sequence layout animations on a 60 Hz tick; the host (UIScreenView, or the game Renderer for the
// in-course overlays) only steps them and draws `layers`. Choreography that the data does not pin
// down is marked [inf] here and in DECISIONS.md ("100 Mario screens: SMM1 layouts").

/// What the intro card shows.
struct RunCardInfo {
    var name = ""
    var maker = ""
    var code: String?
    var lives = 100
    var number = 1
    var total = 8
}

/// Fill a `Play_CourseIn100Mario_00` instance: texts, and one small Mario per life (the formation
/// is 100 Marios; the one that steps forward in AnimB is #99, so the hidden ones are the first).
@MainActor
func configureCard(_ card: UILayout, _ info: RunCardInfo) {
    card.text("T_CourseName_00", info.name)
    card.text("T_PlayerName_00", info.maker)
    if let code = info.code {
        card.text("T_IDNumber_00", code)
        card.visible("N_CourseID_00", nil)
    } else {
        card.visible("N_CourseID_00", false)
    }
    card.text("T_Zanki_00", "\(info.lives)")
    card.text("T_CourseNum_00", "\(info.number)/\(info.total)")
    card.visible("L_NF_00", false)        // national flag: no region data
    card.visible("P_NFBase_00", false)
    let shown = max(0, min(100, info.lives))
    for i in 0..<100 { card.visible(String(format: "P_MarioSml_%02d", i), i >= 100 - shown ? nil : false) }
}

// MARK: - world map

/// One difficulty's world map (`Nwk_WorldMap{Esy,Nml,Exp,SpExp}_00`) with helpers for the course
/// dots, the counters and Mario's walk. Mario's stops are found from the data: the MoveMario frame
/// at which his pane reaches each course dot.
@MainActor
final class WorldMap {
    static let suffix = ["Esy", "Nml", "Exp", "SpExp"]
    let layout: UILayout
    let difficulty: Int
    let count: Int
    private(set) var stops: [Float] = []
    private static let mario = "N_WorldMapMario_00"
    private static let marioPart = "L_WorldMapMario_00"

    init?(ui: SMM1UI, difficulty d: Int) {
        guard let l = ui.layout("Nwk_WorldMap\(Self.suffix[d])_00") else { return nil }
        layout = l
        difficulty = d
        count = ChallengeFlow.runLengths[d]
        stops = computeStops()
        // resting loops: clouds, birds, flags, Peach bobbing; the beat bounces run on a fixed
        // period here (the game syncs them to the music) [inf]
        l.play("Default", at: 0, loop: true)
        l.play("Beat_00_00", at: 0, loop: true)
        l.play("Beat_00_01", at: 0, loop: true)
        for c in ["L_Castle_00", "L_Castle_01"] { l.play("\(c)/Default", at: 0, loop: true) }
        l.play("L_WorldMapKuribo_00/Default", at: 0, loop: true)
        l.hold("\(Self.marioPart)/Stand", 0)
    }

    func dot(_ i: Int) -> String { "L_WorldMapCourse\(Self.suffix[difficulty])_" + String(format: "%02d", i) }

    private func computeStops() -> [Float] {
        let len = layout.length("MoveMario")
        guard len > 0 else { return [] }
        let dots = (0..<count).compactMap { layout.pane(dot($0)) }.map { SIMD2($0.translate.0, $0.translate.1) }
        var out: [Float] = []
        var f: Float = 0
        while f <= len && out.count < dots.count {
            _ = mmk_ui_play(layout.handle, "MoveMario", f)
            if let p = layout.pane(Self.mario) {
                let d = dots[out.count]
                if abs(p.translate.0 - d.x) < 1.5 && abs(p.translate.1 - d.y) < 1.5 { out.append(f) }
            }
            f += 1
        }
        layout.stop("MoveMario")
        while out.count < count { out.append(len) }
        return out
    }

    func stop(_ i: Int) -> Float { stops.isEmpty ? 0 : stops[max(0, min(stops.count - 1, i))] }

    /// Dots before `n` show cleared (Type frame 1 shows the "clear" disc) [inf].
    func setCleared(_ n: Int) {
        for i in 0..<count { layout.hold("\(dot(i))/Type", i < n ? 1 : 0) }
    }

    func setCounters(lives: Int, cleared: Int) {
        layout.text("T_RestMarioNum_00", "\(lives)")
        layout.text("T_ClearCourseNum_00", "\(cleared)/\(count)")
    }

    /// Mario standing on dot `i`, or still inside the start castle (`nil`, MoveMario frame 0).
    func placeMario(_ i: Int?) {
        layout.hold("MoveMario", i.map { stop($0) } ?? 0)
        marioStand()
    }

    func marioStand() {
        layout.stop("\(Self.marioPart)/Walk")
        layout.stop("\(Self.marioPart)/Jump")
        layout.hold("\(Self.marioPart)/Stand", 0)
    }

    /// Walk along MoveMario from frame `from` to `to` starting at `tick`; returns the arrival tick.
    @discardableResult
    func walk(from: Float, to: Float, at tick: Int) -> Int {
        layout.play("MoveMario", at: tick, from: from, to: to)
        layout.stop("\(Self.marioPart)/Stand")
        if from < 28 { layout.play("\(Self.marioPart)/Jump", at: tick) }
        layout.play("\(Self.marioPart)/Walk", at: tick + (from < 28 ? 28 : 0), loop: true)
        return tick + Int((to - from).rounded(.up))
    }
}

// MARK: - difficulty select (title bar over the world map)

/// The 100 Mario entry screen: `Nwk_ListTitle_00` (difficulty tabs) over the selected world map in
/// its pre-MapIn state, with New Game / Continue. Left/right change difficulty, up/down choose the
/// button, confirm starts, back leaves.
@MainActor
final class ChallengeMenuScene: UIScene {
    private weak var flow: ChallengeFlow?
    private let ui: SMM1UI
    let title: UILayout
    private var maps: [Int: WorldMap] = [:]
    private var leaving: (map: WorldMap, until: Int)?
    private(set) var difficulty: Int
    /// 0 Continue (upper button), 1 New Game.
    private var focus = 1
    private(set) var tick = 0

    /// Tab part for each difficulty (the 100 Mario list of `Nwk_ListTitleSub_00`).
    static let tabs = ["L_ListSelectBtnSub_10", "L_ListSelectBtnSub_00", "L_ListSelectBtnSub_01", "L_ListSelectBtnSub_08"]
        .map { "L_Sub_00/\($0)" }

    var map: WorldMap? { maps[difficulty] }

    var layers: [UILayout] {
        var l: [UILayout] = []
        if let leaving { l.append(leaving.map.layout) }
        if let map { l.append(map.layout) }
        return l + [title]
    }

    init?(flow: ChallengeFlow, ui: SMM1UI, difficulty: Int) {
        guard let t = ui.layout("Nwk_ListTitle_00") else { return nil }
        self.flow = flow
        self.ui = ui
        title = t
        self.difficulty = difficulty
        t.play("In", at: 0)
        t.play("Default", at: 0, loop: true)
        t.hold("MapIn", 0)
        t.hold("L_Sub_00/ListTitleType", 0)    // frame 0: the 100 Mario list
        // OpenType 0/1/2 shows 2/3/4 tabs (the game adds tabs as they unlock [obs data]); all four
        // are shown here, locked ones greyed (DECISIONS.md)
        t.hold("L_Sub_00/OpenType", 2)
        guard let m = prepare(difficulty) else { return nil }
        m.layout.play("In", at: 0)
        updateTabs()
    }

    private func prepare(_ d: Int) -> WorldMap? {
        if let m = maps[d] { return m }
        guard let m = WorldMap(ui: ui, difficulty: d) else { return nil }
        maps[d] = m
        refresh(m)
        return m
    }

    /// Selection state of a map: before MapIn, counters and Mario from the Continue record.
    private func refresh(_ m: WorldMap) {
        m.layout.hold("MapIn", 0)
        let cont = flow?.continueInfo(m.difficulty)
        m.setCleared(cont?.cleared ?? 0)
        m.setCounters(lives: cont?.lives ?? 100, cleared: cont?.cleared ?? 0)
        if let c = cont {
            m.placeMario(c.cleared)
            m.layout.hold("AnimB", m.layout.length("AnimB"))   // Peach already on the castle
        } else {
            m.placeMario(nil)
            m.layout.stop("AnimB")
        }
        updateButtons(m)
    }

    private var canContinue: Bool { flow?.canContinue(difficulty) ?? false }
    private var unlocked: Bool { flow?.unlocked(difficulty) ?? false }

    private func state(_ l: UILayout, _ part: String, _ anim: String, _ frame: Float? = nil) {
        for a in ["OnOff", "Valid", "Invalid", "Decide", "Cancel"] where a != anim { l.stop("\(part)/\(a)") }
        if let frame { l.hold("\(part)/\(anim)", frame) } else { l.play("\(part)/\(anim)", at: tick) }
    }

    private func updateTabs() {
        for (d, tab) in Self.tabs.enumerated() {
            if d == difficulty {
                state(title, tab, "Decide", title.length("\(tab)/Decide"))   // pointer under the tab [inf]
            } else {
                state(title, tab, (flow?.unlocked(d) ?? false) ? "Valid" : "Invalid", 1)
            }
        }
    }

    private func updateButtons(_ m: WorldMap) {
        let open = flow?.unlocked(m.difficulty) ?? false
        let cont = open && (flow?.canContinue(m.difficulty) ?? false)
        if !cont && focus == 0 { focus = 1 }
        // L_StartBtn_01 = Continue (upper), L_StartBtn_00 = New Game
        for (i, part) in ["L_StartBtn_01", "L_StartBtn_00"].enumerated() {
            let enabled = open && (i == 1 || cont)
            if !enabled { state(m.layout, part, "Invalid", 1) } else if i == focus { state(m.layout, part, "OnOff", 3) } else { state(m.layout, part, "Valid", 1) }
        }
    }

    func step(_ input: UIInput) {
        tick += 1
        if let l = leaving, tick >= l.until { leaving = nil }
        guard let m = map else { return }
        let p = input.pressed
        if p.contains(.left) || p.contains(.right) {
            let d = max(0, min(3, difficulty + (p.contains(.right) ? 1 : -1)))
            if d != difficulty, let next = prepare(d) {
                m.layout.play(p.contains(.right) ? "OutL" : "OutR", at: tick)
                leaving = (m, tick + 32)
                difficulty = d
                refresh(next)
                next.layout.play(p.contains(.right) ? "InR" : "InL", at: tick)
                updateTabs()
            }
            return
        }
        if p.contains(.up) && canContinue { focus = 0; updateButtons(m) }
        if p.contains(.down) { focus = 1; updateButtons(m) }
        if p.contains(.back) { flow?.exit(); return }
        if p.contains(.confirm) {
            guard unlocked else {
                flow?.message = ChallengeFlow.lockedMessage(difficulty)
                return
            }
            let resume = focus == 0 && canContinue
            m.layout.stop("OutL"); m.layout.stop("OutR"); m.layout.stop("InL"); m.layout.stop("InR")
            flow?.handoff = (m, title)
            flow?.start(difficulty, resume: resume)
        }
    }
}

// MARK: - world map walk + course intro card

/// Between courses: the world map (MapIn after New Game / Continue, or the map sliding in after a
/// clear while Mario walks to the next dot), then the intro card `Play_CourseIn100Mario_00`
/// (AnimA: the Marios run in; AnimB: one steps forward and the lives count pops; then a hold).
/// Confirm skips ahead (the game skips these with A since 1.20).
@MainActor
final class MapIntroScene: UIScene {
    enum Entry { case newGame, resume, afterClear, direct }

    private weak var flow: ChallengeFlow?
    private let map: WorldMap?
    private let title: UILayout?
    private let card: UILayout
    private(set) var tick = 0
    private var cardStart: Int?
    private var outAt: Int?
    private var walkEnd: Int?
    private var finished = false
    private static let cardHold = 150   // the card's "Out" animation length: a timer [inf]

    var layers: [UILayout] {
        if let c = cardStart, tick >= c { return [card] }
        return [map?.layout, title].compactMap { $0 }
    }

    /// `cleared`: courses cleared before this one (= index of this course's dot).
    init?(flow: ChallengeFlow?, ui: SMM1UI, entry: Entry, difficulty: Int, info: RunCardInfo, cleared: Int,
          handoff: (WorldMap, UILayout)?) {
        guard let card = ui.layout("Play_CourseIn100Mario_00") else { return nil }
        self.flow = flow
        self.card = card
        configureCard(card, info)
        let usable = handoff.flatMap { $0.0.difficulty == difficulty ? $0 : nil }
        switch entry {
        case .direct:
            map = nil
            title = nil
            cardStart = 0
        case .newGame, .resume:
            map = usable?.0 ?? WorldMap(ui: ui, difficulty: difficulty)
            title = usable?.1
        case .afterClear:
            map = WorldMap(ui: ui, difficulty: difficulty)
            title = nil
        }
        guard let m = map else {
            if cardStart == nil { cardStart = 0 }
            scheduleCard()
            return
        }
        let l = m.layout
        m.setCounters(lives: info.lives, cleared: cleared)
        switch entry {
        case .newGame:
            l.play("MapIn", at: 0)
            title?.play("MapIn", at: 0)
            m.setCleared(0)
            m.placeMario(nil)
            l.stop("AnimB")
            // the Goombas carry Peach to the castle under the banner, she appears on it, then
            // Mario jumps out of the start castle and walks to the first dot [inf timing]
            l.play("Telop", at: 200)
            l.play("MoveKuribo", at: 200)
            let b = 200 + Int(max(l.length("Telop"), l.length("MoveKuribo")))
            l.play("AnimB", at: b)
            walkStart = b + 20
            walkFrom = 0
            walkTo = m.stop(0)
        case .resume:
            l.play("MapIn", at: 0)
            title?.play("MapIn", at: 0)
            m.setCleared(cleared)
            m.placeMario(cleared)
            l.hold("AnimB", l.length("AnimB"))
            outAt = 230
        case .afterClear:
            l.hold("MapIn", l.length("MapIn"))
            l.play("In", at: 0)
            l.hold("AnimB", l.length("AnimB"))
            m.setCleared(cleared)
            if cleared > 0 { l.play("\(m.dot(cleared - 1))/Clear", at: 40) }
            m.placeMario(max(0, cleared - 1))
            walkStart = 70
            walkFrom = m.stop(cleared - 1)
            walkTo = m.stop(cleared)
        case .direct:
            break
        }
    }

    private var walkStart: Int?
    private var walkFrom: Float = 0
    private var walkTo: Float = 0

    private func scheduleCard() {
        guard let c = cardStart else { return }
        card.play("AnimA", at: c)
        card.play("AnimB", at: c + Int(card.length("AnimA")))
    }

    private var cardEnd: Int? {
        cardStart.map { $0 + Int(card.length("AnimA") + card.length("AnimB")) + Self.cardHold }
    }

    func step(_ input: UIInput) {
        tick += 1
        guard !finished else { return }
        if let m = map {
            if let w = walkStart, tick == w {
                walkEnd = m.walk(from: walkFrom, to: walkTo, at: tick)
            }
            if let e = walkEnd, tick == e {
                m.marioStand()
                m.layout.hold("MoveMario", walkTo)
                outAt = tick + 30
            }
            if let o = outAt, tick == o, cardStart == nil {
                m.layout.play("Out", at: tick)
                cardStart = tick + Int(max(1, m.layout.length("Out")))
                scheduleCard()
            }
        }
        if input.pressed.contains(.back) {
            // leave before the course starts (the run stays saved for Continue)
            finished = true
            flow?.backToSelect()
            return
        }
        if input.pressed.contains(.confirm) {
            if let c = cardStart, tick >= c {
                end()
            } else {
                cardStart = tick
                scheduleCard()
            }
            return
        }
        if let e = cardEnd, tick >= e { end() }
    }

    private func end() {
        finished = true
        flow?.play()
    }
}

// MARK: - game over, ending

/// `Cmn_GameOver_00` (OneTime), then back to the difficulty select.
@MainActor
final class GameOverScene: UIScene {
    private weak var flow: ChallengeFlow?
    private let layout: UILayout
    private(set) var tick = 0
    private var done = false
    var layers: [UILayout] { [layout] }

    init?(flow: ChallengeFlow, ui: SMM1UI) {
        guard let l = ui.layout("Cmn_GameOver_00") else { return nil }
        self.flow = flow
        layout = l
        l.play("OneTime", at: 0)
    }

    func step(_ input: UIInput) {
        tick += 1
        guard !done else { return }
        let end = Int(max(60, layout.length("OneTime")))
        if tick >= end || (tick > 60 && !input.pressed.intersection([.confirm, .back]).isEmpty) {
            done = true
            flow?.backToSelect()
        }
    }
}

/// After the run's last course: the world map with Mario at the castle and the message balloon
/// `Play_EndingText_00` (Easy: "taken to another castle"; Normal: the rescue line; Expert and Super
/// Expert: the "can't have been easy" line), then the staff credits `Nwk_StaffCredit_*` listing
/// the cleared courses and their makers. The rescue scene itself is not a layout (the game draws
/// it in-engine), so the map stands in for it.
@MainActor
final class EndingScene: UIScene {
    private weak var flow: ChallengeFlow?
    private let map: WorldMap?
    private let text: UILayout
    private let credits: UILayout?
    private(set) var tick = 0
    private var creditsStart: Int
    private var done = false
    private static let textHold = 240

    var layers: [UILayout] {
        if tick >= creditsStart, let credits { return [credits] }
        return [map?.layout, text].compactMap { $0 }
    }

    init?(flow: ChallengeFlow?, ui: SMM1UI, difficulty d: Int, lives: Int, credits list: [(name: String, maker: String)]) {
        guard let t = ui.layout("Play_EndingText_00") else { return nil }
        self.flow = flow
        text = t
        map = WorldMap(ui: ui, difficulty: d)
        if let m = map {
            m.layout.hold("MapIn", m.layout.length("MapIn"))
            m.layout.hold("AnimB", m.layout.length("AnimB"))
            m.setCleared(m.count)
            m.setCounters(lives: lives, cleared: m.count)
            m.placeMario(m.count - 1)
        }
        // character variants _x1.._x4 are for Mystery Mushroom costumes; plain Mario is _x0
        let label = "T_ThankYou_" + (d == 0 ? "10" : d == 1 ? "20" : "30")
        if let s = t.message(label) { t.text("T_ThankYou_00", s) }
        t.play("In", at: 20)
        let waitAt = 20 + Int(t.length("In"))
        t.play("Wait", at: waitAt, loop: true)
        let outAt = waitAt + Self.textHold
        t.play("Out", at: outAt)
        creditsStart = outAt + Int(max(1, t.length("Out"))) + 20
        // 01: 8 entries + "TO BE CONTINUED" (Easy), 00: 16 entries (Normal, Expert), 03: 6 entries +
        // "THE END" (Super Expert) [inf from the entry counts]
        let c = ui.layout("Nwk_StaffCredit_" + (d == 0 ? "01" : d == 3 ? "03" : "00"))
        credits = c
        if let c {
            c.play("InOut", at: creditsStart)
            for i in 0..<16 {
                let part = String(format: "L_MiiInfo_%02d", i)
                guard c.pane(part) != nil else { continue }
                if i < list.count {
                    c.visible(part, nil)
                    c.text("\(part)/T_CourseName_00", list[i].name)
                    c.text("\(part)/T_MiiName_00", list[i].maker)
                    c.visible("\(part)/L_NationalFlag_00", false)
                    c.visible("\(part)/P_Frame_00", false)
                } else {
                    c.visible(part, false)
                }
            }
        }
    }

    func step(_ input: UIInput) {
        tick += 1
        guard !done else { return }
        let confirm = !input.pressed.intersection([.confirm, .back]).isEmpty
        if tick < creditsStart {
            if confirm && tick > 30 { creditsStart = tick; credits?.play("InOut", at: tick) }
            return
        }
        let end = creditsStart + Int(credits.map { max(1, $0.length("InOut")) } ?? 1)
        if tick >= end || (confirm && tick > creditsStart + 30) {
            done = true
            flow?.endingFinished()
        }
    }
}

// MARK: - in-course overlays

/// Layouts over the game view during a 100 Mario course, drawn by the game Renderer and stepped by
/// the GameSession: pause menu, course clear, the skip countdown and the intro card after a death.
@MainActor
final class RunOverlay: UIScene {
    enum Mode: Equatable { case none, pause, clear, card, skip }

    private let ui: SMM1UI
    private(set) var tick = 0
    private(set) var mode: Mode = .none
    private(set) var modeStart = 0
    private var pauseL: UILayout?
    private var clearL: UILayout?
    private var skipL: UILayout?
    private var cardL: UILayout?
    /// A layout still playing its Out animation after its mode ended: (layout, until tick).
    private var closing: (UILayout, Int)?

    var sinceMode: Int { tick - modeStart }

    var layers: [UILayout] {
        var l: [UILayout] = []
        if let c = closing, tick < c.1 { l.append(c.0) }
        switch mode {
        case .none: break
        case .pause: if let p = pauseL { l.append(p) }
        case .clear: if let c = clearL { l.append(c) }
        case .card: if let c = cardL { l.append(c) }
        case .skip: if let s = skipL { l.append(s) }
        }
        return l
    }

    init(ui: SMM1UI) { self.ui = ui }

    func step(_ input: UIInput) { tick += 1 }

    private func setMode(_ m: Mode) {
        mode = m
        modeStart = tick
    }

    // MARK: pause (`Cmn_PauseMenu_00`)

    /// Selectable pause items, top to bottom: close (continue), Start Over, Save & Quit.
    static let pauseParts = ["L_CloseBtn_00", "L_RetryBtn_00", "L_ExitBtn_00"]

    func showPause(modeLine: String, course: String, maker: String, selection: Int) {
        if pauseL == nil, let p = ui.layout("Cmn_PauseMenu_00") {
            pauseL = p
            // Type 6: the two play buttons without the network ones (Start Over, Exit Course); the
            // exit button reads "Save & Quit" (T_ExitBtn_01 of the same table) [inf]
            p.hold("Type", 6)
            if let s = p.message("T_ExitBtn_01") { p.text("L_ExitBtn_00/T_Btn_00", s) }
            p.hold("TypeCourseInfo", 2)   // course-world icon, maker line
            for n in ["N_NetworkBtn_00", "P_YeahCountIcon_00", "P_PlayerCountIcon_00", "L_NationalFlag_00", "L_OptionBtn_00"] {
                p.visible(n, false)       // stars, comments, counts, region, options: no network data
            }
        }
        guard let p = pauseL else { return }
        p.text("T_Mode_00", modeLine)
        p.text("T_CourseName_00", course)
        p.text("T_MiiName_00", maker)
        closing = nil
        setMode(.pause)
        p.stop("Out")
        p.play("In", at: tick)
        pauseSelect(selection)
    }

    func pauseSelect(_ i: Int) {
        guard let p = pauseL else { return }
        for (k, part) in Self.pauseParts.enumerated() {
            if k == i { p.play("\(part)/OnOff", at: tick) } else { p.hold("\(part)/OnOff", 0) }
        }
    }

    func hidePause() {
        guard mode == .pause, let p = pauseL else { return }
        p.stop("In")
        p.play("Out", at: tick)
        closing = (p, tick + Int(max(1, p.length("Out"))))
        setMode(.none)
    }

    // MARK: course clear (`Play_CourseClear_00`)

    /// 0 Save & Quit, 1 Next.
    private(set) var clearFocus = 1
    private var clearLast = false

    func showClear(course: String, maker: String, ticks: Int?, worldRecordMs: Int?, last: Bool) {
        guard let c = clearL ?? ui.layout("Play_CourseClear_00") else { return }
        clearL = c
        clearLast = last
        // 100 Mario variant: Save & Quit + Next, no recommendations; ClearType centres Next alone
        // (used on the run's last course, which goes on to the ending) [inf]
        if last { c.hold("ClearType", 1) } else { c.hold("ModeType", 0) }
        for n in ["N_MiiverseComment_01", "N_RcmText_00", "N_RcmCourseBtn_02", "N_CommentGoodBtn_00"] {
            c.visible(n, false)   // comments, recommendations, Star: no network data
        }
        c.visible("N_Player_00/L_NF_00", false)
        c.visible("L_NF_00", false)
        c.text("T_CourseName_00", course)
        c.text("T_PlayerName_00", maker)
        func put(_ main: String, _ frac: String, _ ms: Int?) {
            guard let ms else {
                c.text(main, "--’--”")
                c.text(frac, "---")
                return
            }
            c.text(main, String(format: "%02d’%02d”", ms / 60000, ms / 1000 % 60))
            c.text(frac, String(format: "%03d", ms % 1000))
        }
        put("T_Time_02", "T_Time_03", ticks.map { $0 * 1000 / 60 })
        put("T_Time_00", "T_Time_01", worldRecordMs)
        c.play("In", at: tick)
        c.play("Default", at: tick, loop: true)
        setMode(.clear)
        clearFocus = 1
        clearSelect(1)
    }

    func clearSelect(_ i: Int) {
        guard let c = clearL else { return }
        clearFocus = clearLast ? 1 : max(0, min(1, i))
        for (k, part) in ["L_ExitBtn_00", "L_NextBtn_00"].enumerated() {
            if k == clearFocus { c.play("\(part)/OnOff", at: tick) } else { c.hold("\(part)/OnOff", 0) }
        }
    }

    /// The buttons take input once the In animation has brought them on screen.
    var clearReady: Bool { mode == .clear && sinceMode >= Int(clearL?.length("In") ?? 0) / 2 }

    func pressClear() {
        guard let c = clearL else { return }
        let part = clearFocus == 0 ? "L_ExitBtn_00" : "L_NextBtn_00"
        c.play("\(part)/Decide", at: tick)
    }

    // MARK: skip countdown (`Play_SlidePassIconKey_00`)

    /// Ticks the skip key must be held: three countdown steps of 32 frames [inf: each step waits
    /// for its animation, smm1-layouts survey].
    static let skipHold = 96

    func skipBegin() {
        guard let s = skipL ?? ui.layout("Play_SlidePassIconKey_00") else { return }
        skipL = s
        closing = nil
        for a in ["Out", "Pass", "CountDown_00", "CountDown_01", "CountDown_02"] { s.stop(a) }
        s.play("Default", at: tick, loop: true)
        s.play("In", at: tick)
        s.play("CountDown_00", at: tick)
        s.play("CountDown_01", at: tick + 32)
        s.play("CountDown_02", at: tick + 64)
        setMode(.skip)
    }

    func skipCancel() {
        guard mode == .skip, let s = skipL else { return }
        s.play("Out", at: tick)
        closing = (s, tick + Int(max(1, s.length("Out"))))
        setMode(.none)
    }

    /// The hold is complete: play Pass. Returns the tick count it takes.
    func skipPass() -> Int {
        guard let s = skipL else { return 1 }
        s.play("Pass", at: tick)
        return Int(max(1, s.length("Pass")))
    }

    // MARK: intro card after a death

    func showCard(_ info: RunCardInfo) {
        guard let c = cardL ?? ui.layout("Play_CourseIn100Mario_00") else { return }
        cardL = c
        configureCard(c, info)
        c.hold("AnimA", c.length("AnimA"))
        c.play("AnimB", at: tick)
        c.play("OutMiss", at: tick + Int(c.length("AnimB")))
        setMode(.card)
    }

    var cardDone: Bool {
        guard mode == .card, let c = cardL else { return true }
        return sinceMode >= Int(c.length("AnimB") + max(1, c.length("OutMiss")))
    }

    func hideCard() { if mode == .card { setMode(.none) } }

    func clearAll() {
        closing = nil
        setMode(.none)
    }
}
