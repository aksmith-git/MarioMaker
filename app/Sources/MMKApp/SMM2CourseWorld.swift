import CMmk
import SwiftUI

// Course World with SMM2's own layouts, over the local library and the open-source course
// server (`mmk_online_*`, never Nintendo's network):
// - the hub `Nwk_Title_00` (space background with its LoopRandom events, the buttons Courses,
//   Leaderboards, Network Play, Endless Challenge, Ninji Speedruns, Super Worlds) [obs layout];
// - Courses → `Nwk_CourseListGeneral_00`: the tabs (Hot / Popular / New Courses), rows
//   `Nwk_CourseData_00` (name, style logo, maker, plays, likes, date, clear mark); confirm opens
//   a row (its Toggle animation: clear rate, clears / attempts, best time, course ID, Play) and
//   Play starts the course with the black fade;
// - Endless Challenge → the Endless menu (100 Mario Challenge rules, see SMM2Endless.swift).
// Scene changes use the black fade (the game passes no fade parameters there [obs]). What each
// tab lists, the cursor's start, which buttons are unavailable without Nintendo's network
// (Leaderboards, Network Play, Ninji Speedruns, Super Worlds: a message), and the hidden parts
// without data (Mii, flags, badges, thumbnails, tags, comments, world ranks, ID search, the
// player card) are ours [inf / departure] (private research/specs/smm2-frontend.md).

/// A course offered by Course World: a library file or a server listing.
struct WorldCourse: Identifiable {
    enum Source {
        case library(CourseEntry)
        case server(code: String)
    }

    var id: String
    var source: Source
    var name: String
    var maker: String?
    /// Game style "M1", "M3", "MW", "WU", "3W".
    var style: String
    var description = ""
    var plays: Int?
    var likes: Int?
    var clears: Int?
    var attempts: Int?
    var clearRate: Double?
    var code: String?
    var date: Date?
    var cleared = false
    var bestTicks: Int?
    var worldRecordMs: Int?
    var playable = true
    /// SMM2 tag ids (1 Standard … 15 Link; the course list's message labels are 1000 + id).
    var tags: [Int] = []

    @MainActor static func library(_ e: CourseEntry, saves: SaveStore) -> WorldCourse {
        let r = saves.record(e.info.key)
        var c = WorldCourse(id: e.url.path, source: .library(e), name: e.name, maker: e.meta.maker,
                            style: e.info.style, description: e.info.description)
        // the dataset's statistics when the index has them, else this player's own
        c.plays = e.meta.plays ?? r.attempts
        c.likes = e.meta.likes
        c.clears = e.meta.clears ?? r.clears
        c.attempts = e.meta.attempts ?? r.attempts
        c.tags = e.meta.tags
        c.cleared = r.cleared
        c.bestTicks = r.bestTicks
        c.clearRate = e.meta.clearRate
        c.worldRecordMs = e.meta.worldRecordMs
        c.playable = e.info.playable
        if let id = e.meta.dataId {
            var buf = [CChar](repeating: 0, count: 16)
            if mmk_smm2_course_code(id, &buf, buf.count) > 0 { c.code = String(cString: buf) }
        }
        c.date = e.meta.uploaded ?? (try? e.url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        return c
    }

    static func server(_ j: [String: Any]) -> WorldCourse? {
        guard let code = j["code"] as? String else { return nil }
        func n(_ k: String) -> Int? { (j[k] as? NSNumber)?.intValue }
        var c = WorldCourse(id: "server:\(code)", source: .server(code: code), name: j["name"] as? String ?? code,
                            maker: j["maker"] as? String, style: j["style"] as? String ?? "M1")
        c.plays = n("plays")
        c.likes = n("likes")
        c.clears = n("clears")
        c.attempts = n("plays")
        if let p = c.plays, p > 0, let cl = c.clears { c.clearRate = Double(cl) * 100 / Double(p) }
        c.code = code
        return c
    }
}

enum CourseWorldChoice {
    case courses, endless, unavailable
}

// MARK: - hub

/// The Course World hub (`Nwk_Title_00`).
@MainActor
final class CourseWorldScene: UIScene {
    let layout: UILayout
    let cursor: MenuCursor
    private(set) var tick = 0
    private(set) var selection = 0
    private var pending: (choice: CourseWorldChoice, at: Int)?
    var onChoice: ((CourseWorldChoice, String) -> Void)?
    var onBack: (() -> Void)?

    /// The buttons (parts of `N_Btn_00`) and what they open.
    static let buttons: [(part: String, choice: CourseWorldChoice)] = [
        ("L_Course_00", .courses), ("L_Maker_00", .unavailable), ("L_MultiPlay_00", .unavailable),
        ("L_EndlessMode_00", .endless), ("L_EventCourseBtn_00", .unavailable), ("L_MyWBtn_00", .unavailable),
    ]

    var layers: [UILayout] { [layout] + cursor.layers }

    init?(ui: LayoutUI, selection start: Int = 0) {
        guard let l = ui.layout("Nwk_Title_00") else { return nil }
        layout = l
        cursor = MenuCursor(ui: ui)
        selection = max(0, min(Self.buttons.count - 1, start))
        // the "Course World" title art and the Super Worlds sign in English (frame 1, as the
        // clear banner) [inf]
        l.hold("RegionLanguageType", 1)
        l.hold("L_MyWBtn_00/RegionLanguageType", 1)
        l.play("LoopRandom", at: 0, loop: true)
        l.play("L_MyWBtn_00/SelectLoop", at: 0, loop: true)
        // no event running: the Ninji button without its period line [inf]
        l.hold("L_EventCourseBtn_00/TypePeriod", 1)
        // the player card (Mii), the ID search: no data / no keyboard here [departure]
        l.visible("L_PlayerBody_00", false)
        l.visible("L_IDSearchBtn_00", false)
        for b in Self.buttons { l.hold("\(b.part)/Select", 0) }
        select(selection, animate: false)
    }

    func step(_ input: UIInput) {
        tick += 1
        let pressed = input.pressed.union(DebugInput.step())
        if let p = pending, tick >= p.at {
            pending = nil
            onChoice?(p.choice, Self.buttons[selection].part)
        }
        if pending == nil { handle(pressed) }
        let part = Self.buttons[selection].part
        cursor.place(on: layout, pane: "\(part)/B_Btn_00", tick: tick, alpha: pending == nil ? 1 : 0)
    }

    private func handle(_ pressed: MenuInput) {
        if let dir = pressed.direction {
            layout.apply(tick: tick)
            let here = layout.rect("\(Self.buttons[selection].part)/B_Btn_00")?.center ?? .zero
            let targets: [(Int, SIMD2<Float>)] = Self.buttons.enumerated().compactMap { i, b in
                layout.rect("\(b.part)/B_Btn_00").map { (i, $0.center) }
            }
            if let n = nearest(from: here, toward: dir, among: targets) {
                UISound.shared.button("Nwk_Title_00", Self.buttons[n].part, .select)
                select(n, animate: true)
            }
        } else if pressed.contains(.confirm) {
            let b = Self.buttons[selection]
            UISound.shared.button("Nwk_Title_00", b.part, .decide)
            layout.play("\(b.part)/Decide", at: tick)
            pending = (b.choice, tick + Int(max(1, layout.length("\(b.part)/Decide"))))
        } else if pressed.contains(.back) {
            UISound.shared.back("Nwk_Title_00")
            onBack?()
        }
    }

    private func select(_ i: Int, animate: Bool) {
        for (k, b) in Self.buttons.enumerated() where k != i { layout.hold("\(b.part)/Select", 0) }
        selection = i
        let part = Self.buttons[i].part
        if animate { layout.play("\(part)/Select", at: tick) } else { layout.hold("\(part)/Select", layout.length("\(part)/Select")) }
    }
}

// MARK: - course list

/// The course list (`Nwk_CourseListGeneral_00` with `Nwk_CourseList_00`'s five rows).
@MainActor
final class CourseListScene: UIScene {
    enum Tab: Int, CaseIterable { case hot, popular, new }

    let layout: UILayout
    let cursor: MenuCursor
    private let ui: LayoutUI
    private(set) var tick = 0
    private(set) var tab = Tab.hot
    private var all: [WorldCourse] = []
    private(set) var courses: [WorldCourse] = []
    /// Selected course, the course in the first visible line, whether the selected row is open.
    private(set) var selection = 0
    private var first = 0
    /// The list's slide when it scrolls by a line (from this offset to 0) [inf].
    private var slide: (from: Float, start: Int)?
    private(set) var open = false
    private var focusPlay = true
    private var busyUntil = 0
    private var loading = false
    var onPlay: ((WorldCourse) -> Void)?
    var onBack: (() -> Void)?

    static let slots = 5
    static let list = "L_CourseList_00"
    static let layoutName = "Nwk_CourseListGeneral_00"
    static let tabs = ["L_PickUpBtn_00", "L_CoursePointRankBtn_00", "L_LatestBtn_00"]
    static let styles = ["M1", "M3", "MW", "WU", "3W"]

    var layers: [UILayout] { [layout] + cursor.layers }

    init?(ui: LayoutUI) {
        guard let l = ui.layout("Nwk_CourseListGeneral_00") else { return nil }
        self.ui = ui
        layout = l
        cursor = MenuCursor(ui: ui)
        l.play("In", at: 0)
        // no sort / search window, no "Detailed Search" and no timeline [departure]
        for p in ["L_SortBtn_00", "N_SortBtn_00", "N_CourseList_00/L_AdvancedBtn_00", "N_CourseList_00/L_TimelineBtn_00",
                  "L_TimelineBtn_00", "L_AdvancedBtn_00", "N_PageTopBtn_00", "L_PageBtns_00/N_PageBtns_00"] {
            l.visible(p, false)
        }
        if let t = l.message("L_PageBtns_00-T_Title_00") { l.text("L_PageBtns_00/T_Title_00", t) }
        for t in Self.tabs { l.hold("\(t)/Select", 0) }
        l.hold("\(Self.list)/TypeContents", 0)
        selectTab(.hot)
        setLoading(true)
    }

    /// The courses arrived (library first, then the server's when it answers).
    func setCourses(_ list: [WorldCourse], loading: Bool) {
        all = list
        setLoading(loading)
        applyTab(keepSelection: true)
    }

    private func setLoading(_ on: Bool) {
        loading = on
        layout.visible("L_LoadingIcon_00", on && all.isEmpty ? nil : false)
        if on && all.isEmpty { layout.play("L_LoadingIcon_00/Loop", at: tick, loop: true) }
    }

    private func applyTab(keepSelection: Bool) {
        let id = keepSelection && selection < courses.count ? courses[selection].id : nil
        switch tab {
        case .hot:
            // the library first, then the server's courses, each newest first [inf]
            courses = all.sorted { a, b in
                if a.isServer != b.isServer { return !a.isServer }
                return (a.date ?? .distantPast) > (b.date ?? .distantPast)
            }
        case .popular:
            courses = all.sorted { ($0.likes ?? 0, $0.plays ?? 0) > ($1.likes ?? 0, $1.plays ?? 0) }
        case .new:
            courses = all.sorted { ($0.date ?? .distantPast) > ($1.date ?? .distantPast) }
        }
        selection = id.flatMap { i in courses.firstIndex { $0.id == i } } ?? 0
        first = max(0, min(selection, courses.count - Self.visible))
        open = false
        fill()
    }

    private func selectTab(_ t: Tab) {
        tab = t
        for (k, p) in Self.tabs.enumerated() {
            // the current tab keeps its Decide look (dark), the others their plain one [inf]
            if k == t.rawValue { layout.hold("\(p)/Decide", layout.length("\(p)/Decide")) } else { layout.stop("\(p)/Decide") }
        }
    }

    // MARK: rows

    /// Lines on screen: the five slots are a ring whose first and last are the off-screen rows
    /// above and below the list's scissor (`N_Scissor_00`, 580 high) [inf from the layout]; the
    /// player doesn't clip, so those two stay hidden.
    static let visible = 3

    private func slotPane(_ k: Int) -> String { String(format: "%@/L_CourseData_%02d", Self.list, k) }

    /// The slot showing course `i` (line 1 is the first visible one).
    private func rowPane(_ i: Int) -> String { slotPane(i - first + 1) }

    /// Put the courses of the window into the three visible slots (an open row takes the
    /// space of the two lines under it).
    private func fill() {
        let l = layout
        l.visible("\(Self.list)/N_CourseCountAlign_00", false)
        for k in 0..<Self.slots {
            let row = slotPane(k)
            let i = first + k - 1
            // an open row (on the middle line) covers the lines above and below it
            let covered = open && i != selection
            guard k >= 1, k <= Self.visible, i >= 0, i < courses.count, !covered else {
                l.visible(row, false)
                continue
            }
            l.visible(row, nil)
            let c = courses[i]
            l.hold("\(row)/Type", 1)                // no world rank column
            // tags: TypeTag 0 two, 1 one, 2 none (the "–" marks) [obs keys]
            let tags = c.tags.prefix(2).compactMap { l.message("\(1000 + $0)") }
            l.hold("\(row)/TypeTag", Float(2 - tags.count))
            if let t = tags.first { l.text("\(row)/T_Tag_00", t) }
            if tags.count == 2 { l.text("\(row)/T_Tag_01", tags[1]) }
            l.hold("\(row)/TypeMission", 0)
            l.visible("\(row)/P_MissionIcon_00", false)
            l.text("\(row)/T_CourseName_00", c.name)
            if !l.picture("\(row)/L_GameSkin_00/P_DummyGameSkin_00", group: "GameSkin", "\(c.style)_Lyt_Logo_00") {
                l.visible("\(row)/L_GameSkin_00", false)
            }
            for p in ["L_Thumbnail_00", "L_GoodCheck_00", "L_MyWIcon_00", "L_MiiBtn_00", "L_BadgeList_00",
                      "L_NationalFlag_00", "L_MiiIcon_00", "L_CommentBtn_00", "L_ReportBtn_00",
                      "L_DeleteBtn_00", "L_DpointBtn_00", "L_CoopModeBtn_00", "L_DetailBtn_00", "L_Rank_00",
                      "L_DownloadPlayBtn_00"] {
                l.visible("\(row)/\(p)", false)
            }
            // the rows' fixed texts: the screen's table labels them for the first row only
            for p in ["T_ClearRate_00", "T_Challenge_00", "T_Fast_00", "T_CourseIDText_00", "L_PlayBtn_00-T_Btn_00"] {
                if let s = l.message("L_CourseList_00-L_CourseData_00-\(p)") {
                    l.text("\(row)/\(p.replacingOccurrences(of: "-", with: "/"))", s)
                }
            }
            l.text("\(row)/T_PlayerName_00", c.maker ?? "")
            l.visible("\(row)/N_BadgeListAlign_00", (c.maker ?? "").isEmpty ? false : nil)
            l.text("\(row)/L_GoodCount_00/T_Count_00", c.likes.map(String.init) ?? "-")
            l.text("\(row)/L_PlayerCount_00/T_Count_00", c.plays.map(String.init) ?? "-")
            l.hold("\(row)/L_ClearCheck_00/ClearCheck", c.cleared ? 1 : 0)
            l.visible("\(row)/L_ClearCheck_00", c.cleared ? nil : false)
            if let d = c.date {
                let cal = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute], from: d)
                l.text("\(row)/T_Day_00", l.format("Day_0000", [String(format: "%02d", cal.month ?? 0), String(format: "%02d", cal.day ?? 0), "\(cal.year ?? 0)"]) ?? "")
                l.text("\(row)/T_Time_00", l.format("Time_0000", [String(format: "%02d", cal.hour ?? 0), String(format: "%02d", cal.minute ?? 0)]) ?? "")
                l.visible("\(row)/P_DayTimeBase_00", nil)
            } else {
                l.visible("\(row)/P_DayTimeBase_00", false)
            }
            fillDetail(row, c)
            // Play downloads a server course first (no separate Download button) [departure]
            l.hold("\(row)/TypeBtn", 0)
            if i == selection && open {
                l.hold("\(row)/Toggle", l.length("\(row)/Toggle"))
            } else {
                l.stop("\(row)/Toggle")
            }
            l.hold("\(row)/Select", i == selection && !open ? l.length("\(row)/Select") : 0)
            l.play("\(row)/Loop", at: 0, loop: true)
        }
        layoutRows()
    }

    private func fillDetail(_ row: String, _ c: WorldCourse) {
        let l = layout
        l.wrappedText("\(row)/T_CourseInfo_00", c.description, width: 560, lines: 2)
        if let r = c.clearRate {
            let whole = Int(r), frac = Int((r - Double(whole)) * 100)
            l.text("\(row)/T_ClearRatePercent_00", l.format("ClearRatePercent_0000", ["\(whole)", String(format: "%02d", frac)]) ?? "")
        } else {
            l.text("\(row)/T_ClearRatePercent_00", "--.-- %")
        }
        l.text("\(row)/L_ClearCount_00/T_Count_00", "\(c.clears ?? 0)")
        l.text("\(row)/L_TryCount_00/T_Count_00", "\(c.attempts ?? 0)")
        // the world record when the index knows it, else this player's best time [inf]
        let ms = c.worldRecordMs ?? c.bestTicks.map { $0 * 1000 / 60 }
        if let ms {
            l.text("\(row)/T_Time_01", l.format("Time_0001", [String(format: "%02d", ms / 60000), String(format: "%02d", ms / 1000 % 60)]) ?? "")
            l.text("\(row)/T_Time_02", l.format("Time_0002", [String(format: "%03d", ms % 1000)]) ?? "")
        } else {
            l.text("\(row)/T_Time_01", l.message("Time_0011") ?? "")
            l.text("\(row)/T_Time_02", l.message("Time_0012") ?? "")
        }
        l.text("\(row)/T_PlayerName_01", "")
        l.text("\(row)/T_CourseID_00", c.code ?? "-")
    }

    /// The slots at their lines, the whole list slid while it scrolls.
    private func layoutRows() {
        var dy: Float = 0
        if let s = slide {
            let t = Float(tick - s.start) / 6
            if t >= 1 { slide = nil } else { dy = s.from * (1 - t) * (1 - t) }
        }
        layout.setPane("\(Self.list)/N_TransRoot_00", translate: SIMD2(0, dy))
    }

    // MARK: input

    func step(_ input: UIInput) {
        tick += 1
        let pressed = input.pressed.union(DebugInput.step())
        if tick >= busyUntil { handle(pressed) }
        layoutRows()
        placeCursor()
    }

    private func handle(_ pressed: MenuInput) {
        if pressed.contains(.back) {
            if open {
                toggle(false)
            } else {
                UISound.shared.back(Self.layoutName)
                layout.play("Out", at: tick)
                busyUntil = .max
                onBack?()
            }
            return
        }
        if open {
            if pressed.contains(.confirm) {
                let row = rowPane(selection)
                UISound.shared.button(Self.layoutName, "L_PlayBtn_00", .decide)
                layout.play("\(row)/L_PlayBtn_00/Decide", at: tick)
                let c = courses[selection]
                busyUntil = .max
                let wait = Int(max(1, layout.length("\(row)/L_PlayBtn_00/Decide")))
                DispatchQueue.main.asyncAfter(deadline: .now() + Double(wait) / 60) { [weak self] in self?.onPlay?(c) }
            } else if pressed.contains(.up) || pressed.contains(.down) {
                toggle(false)
                move(pressed.contains(.up) ? -1 : 1)
            }
            return
        }
        if pressed.contains(.left) || pressed.contains(.right) {
            let n = Tab.allCases.count
            let t = Tab(rawValue: (tab.rawValue + (pressed.contains(.left) ? n - 1 : 1)) % n)!
            UISound.shared.button(Self.layoutName, Self.tabs[t.rawValue], .decide)
            layout.play("\(Self.tabs[t.rawValue])/Decide", at: tick)
            selectTab(t)
            applyTab(keepSelection: false)
        } else if pressed.contains(.up) {
            move(-1)
        } else if pressed.contains(.down) {
            move(1)
        } else if pressed.contains(.confirm), selection < courses.count {
            layout.play("\(rowPane(selection))/Decide", at: tick)
            toggle(true)
        }
    }

    private func move(_ d: Int) {
        guard !courses.isEmpty else { return }
        let n = max(0, min(courses.count - 1, selection + d))
        guard n != selection else { return }
        selection = n
        UISound.shared.button(Self.layoutName, Self.list, .select)
        let old = first
        if selection < first { first = selection }
        if selection > first + Self.visible - 1 { first = selection - Self.visible + 1 }
        if first != old { slide = (Float(first - old) * -180, tick) }
        fill()
        layout.play("\(rowPane(selection))/Select", at: tick)
    }

    private func toggle(_ on: Bool) {
        guard selection < courses.count else { return }
        open = on
        if on {
            // the open row on the middle line: it grows 180 up and down, over its neighbours,
            // and stays inside the list's scissor [inf]
            let mid = selection - 1
            if first != mid { slide = (Float(mid - first) * -180, tick) }
            first = mid
        } else if first < 0 || first > max(0, courses.count - Self.visible) {
            first = max(0, min(first, courses.count - Self.visible))
        }
        fill()
        let row = rowPane(selection)
        UISound.shared.key(Self.layoutName, "Anm_Toggle_showcoursedetail_\(on ? "Forward" : "Backward")")
        if on {
            layout.play("\(row)/Toggle", at: tick)
            layout.play("\(row)/L_PlayBtn_00/Select", at: tick + 10)
        } else {
            layout.play("\(row)/Select", at: tick)
        }
        busyUntil = tick + 10
    }

    private func placeCursor() {
        guard selection < courses.count else {
            cursor.hide()
            return
        }
        let row = rowPane(selection)
        let pane = open ? "\(row)/L_PlayBtn_00/B_Btn_00" : "\(row)/B_Btn_00"
        cursor.place(on: layout, pane: pane, tick: tick, alpha: busyUntil == .max ? 0 : 1)
    }
}

private extension WorldCourse {
    var isServer: Bool { if case .server = source { true } else { false } }
}

// MARK: - flow

/// Course World: hub → course list → play, and the Endless menu; owns the course being played.
@MainActor
final class CourseWorldFlow: ObservableObject, Identifiable {
    enum Stage {
        case hub, list
        case playing(GameSession)
    }

    nonisolated let id = UUID()
    let ui: LayoutUI
    @Published private(set) var stage = Stage.hub
    @Published private(set) var scene: UIScene?
    @Published var toast: String?
    private let assets: EngineAssets
    private let library: [CourseEntry]
    private let saves: SaveStore
    private var hubSelection = 0
    private var toastTask: DispatchWorkItem?
    /// Back to the title.
    var onExit: (() -> Void)?
    /// Endless Challenge (the app shows the Endless menu and comes back here).
    var onEndless: (() -> Void)?

    init?(ui: LayoutUI, assets: EngineAssets, library: [CourseEntry], saves: SaveStore, selection: Int = 0) {
        self.ui = ui
        self.assets = assets
        self.library = library
        self.saves = saves
        hubSelection = selection
        guard showHub() else { return nil }
    }

    @discardableResult
    func showHub() -> Bool {
        guard let s = CourseWorldScene(ui: ui, selection: hubSelection) else { return false }
        s.onChoice = { [weak self] c, _ in self?.hubChoice(c, selection: s.selection) }
        s.onBack = { [weak self] in self?.onExit?() }
        scene = s
        stage = .hub
        return true
    }

    private func hubChoice(_ c: CourseWorldChoice, selection: Int) {
        hubSelection = selection
        switch c {
        case .courses:
            SceneFade.shared.run(.black) { [weak self] in self?.showList() }
        case .endless:
            onEndless?()
        case .unavailable:
            show("This needs Nintendo's network, which this engine never uses.")
        }
    }

    func showList() {
        guard let s = CourseListScene(ui: ui) else { return }
        s.onBack = { [weak self] in SceneFade.shared.run(.black) { self?.showHub() } }
        s.onPlay = { [weak self] c in self?.play(c) }
        scene = s
        stage = .list
        let local = library.filter { !$0.builtIn }.map { WorldCourse.library($0, saves: saves) }
        let server = UserDefaults.standard.string(forKey: SettingsKey.courseServer) ?? ""
        s.setCourses(local, loading: !server.isEmpty)
        guard !server.isEmpty else { return }
        DispatchQueue.global(qos: .userInitiated).async {
            let found = Self.search(server)
            DispatchQueue.main.async { [weak s] in s?.setCourses(local + found, loading: false) }
        }
    }

    /// The server's courses (popular first, 100), empty when it can't be reached.
    nonisolated static func search(_ server: String) -> [WorldCourse] {
        var buf = [CChar](repeating: 0, count: 1 << 20)
        let n = mmk_online_search(server, "", "popular", 0, 100, &buf, buf.count)
        guard n > 0, n < buf.count,
              let arr = try? JSONSerialization.jsonObject(with: Data(String(cString: buf).utf8)) as? [[String: Any]]
        else { return [] }
        return arr.compactMap(WorldCourse.server)
    }

    private func play(_ c: WorldCourse) {
        guard c.playable else {
            show("\(c.name) uses parts the engine doesn't support yet.")
            showList()
            return
        }
        SceneFade.shared.run(.black) { [weak self] in self?.start(c) }
    }

    private func start(_ c: WorldCourse) {
        var url: URL
        var entry: CourseEntry?
        switch c.source {
        case .library(let e):
            url = e.url
            entry = e
        case .server(let code):
            let server = UserDefaults.standard.string(forKey: SettingsKey.courseServer) ?? ""
            let dir = AppPaths.courses.appendingPathComponent("Course World", isDirectory: true)
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            url = dir.appendingPathComponent("\(code).bin")
            if !FileManager.default.fileExists(atPath: url.path) && !mmk_online_download(server, code, url.path) {
                show("Couldn't download \(c.name): \(lastEngineError())")
                showList()
                return
            }
            DispatchQueue.global(qos: .utility).async { _ = mmk_online_report(server, code, "play") }
        }
        guard let data = try? Data(contentsOf: url),
              case .success(let info) = CourseInfo.read(data, assets: assets),
              let s = try? GameSession(assets: assets, course: data, title: entry?.name ?? (info.name.isEmpty ? c.name : info.name),
                                       courseKey: info.key, saves: saves) else {
            show("Couldn't start \(c.name).")
            showList()
            return
        }
        s.unlimitedLives = true
        if let screens = CourseScreens(ui: ui) {
            var card = CourseCardInfo()
            card.name = s.title
            card.maker = c.maker
            card.bestTicks = saves.record(info.key).bestTicks
            card.modeLabel = "T_Mode_Network"
            s.attachSMM2(screens, info: card, intro: true)
        }
        if case .server(let code) = c.source {
            let server = UserDefaults.standard.string(forKey: SettingsKey.courseServer) ?? ""
            s.onClear = { DispatchQueue.global(qos: .utility).async { _ = mmk_online_report(server, code, "clear") } }
        }
        s.onExit = { [weak self] in SceneFade.shared.run(.black) { self?.showList() } }
        scene = nil
        stage = .playing(s)
    }

    private func show(_ message: String) {
        toast = message
        toastTask?.cancel()
        let task = DispatchWorkItem { [weak self] in self?.toast = nil }
        toastTask = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 3, execute: task)
    }
}

struct CourseWorldView: View {
    @ObservedObject var flow: CourseWorldFlow

    var body: some View {
        ZStack {
            if case .playing(let s) = flow.stage {
                GameView(session: s).id(s.id)
            } else if let scene = flow.scene {
                UIScreenView(scene: scene, ui: flow.ui).id(ObjectIdentifier(scene))
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
