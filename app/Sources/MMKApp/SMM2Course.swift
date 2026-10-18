import CMmk
import Foundation

// SMM2's in-course screens drawn with the game's own layouts (`mmk import-smm2-ui`), over a
// library course: the HUD (`Play_PlayScene_00`), the course card (`Play_CourseIn_00` on a black
// `Cmn_Fade_00`), the pause menu (`Cmn_PauseMenu_00`), course clear (`Play_CourseClear_00` with
// `Play_ClearInfo_00`), "You gave up..." (`Play_CourseRetire_00`), "TIME'S UP!" (`Play_TimeUp_00`)
// and game over (`Play_GameOver_00`). Texts come from the import's message tables (the
// clear-condition sentences from ReferenceMsg `GoalCondition` + `PartsIconName`), the
// clear-condition icons from `ui/textures/MissionIcon/`.
//
// The layouts and their animations are the game's; how the game strings them together is not in
// the data. Inferred [inf] (DECISIONS.md "SMM2 in-course screens"): which Type frame each screen
// uses for library play, the engine's lives shown with the Endless-Challenge HUD variant and the
// course card after each death (Course World has no lives), which buttons the clear / give-up
// screens offer, the number widths of the HUD's value tags, the condition → icon / name table.
// Departures: keys held are shown as `Play_KeyIcon_00` icons under the score (the game shows
// them with the player), nothing that needs the network (likes, comments, Miis, flags, tags).

/// A course's clear condition as the SMM2 screens show it: icon, sentence, counter.
struct SMM2Condition {
    let kind: Int
    let target: Int
    let count: Int
    let needed: Int
    let met: Bool
    let violated: Bool

    /// Style prefixes of the import's names (`<style>_Lyt_M_<icon>`, `MW_Kuribo` …), in the
    /// order of `MmkPlayStatus.style`.
    static let styles = ["M1", "M3", "MW", "WU", "3W"]

    init?(_ c: MmkCondition) {
        guard c.kind != 0 else { return nil }
        kind = Int(c.kind)
        target = Int(c.target)
        count = Int(c.count)
        needed = Int(c.needed)
        met = c.met != 0
        violated = c.violated != 0
    }

    /// Defeat conditions: condition-list index → (part name label, icon stem) [inf: matched by
    /// the internal names of the condition list, the part-name table and the icon textures].
    static let enemies: [Int: (part: String, icon: String)] = [
        1: ("Kuribo", "Kuribo_00"), 2: ("Kuribo_01", "Kuribo_01"), 3: ("Nokonoko", "Nokonoko_00"),
        4: ("Met_00", "Met_00"), 5: ("Togezo_00", "Togezo_00"), 6: ("Pakkun_00", "Pakkun_00"),
        7: ("Pakkun_01", "Pakkun_01"), 8: ("BlackPakkun", "BlackPakkun_00"), 10: ("Karon", "Karon_00"),
        11: ("FishBone", "FishBone_00"), 12: ("Poo", "Poo_00"), 13: ("ChoroPoo", "ChoroPoo_00"),
        14: ("Bros", "Bros_00"), 15: ("MegaBros", "MegaBros_00"), 16: ("Jugem", "Jugem_00"),
        17: ("Gesso_00", "Gesso_00"), 18: ("Pukupuku", "Pukupuku_00"), 19: ("TogeMet", "TogeMet_00"),
        20: ("Arihei_00", "Arihei_00"), 21: ("Bubble", "Bubble_00"), 22: ("Wanwan_00", "Wanwan_00"),
        23: ("Hanachan_00", "Hanachan_00"), 24: ("Bombhei_00", "Bombhei_00"), 25: ("Dossun", "Dossun_00"),
        26: ("Teresa_00", "Teresa_00"), 27: ("Teresa_01", "Teresa_01"), 28: ("MagnumKiller", "MagnumKiller_00"),
        29: ("Kameck", "Kameck_00"), 30: ("SunMoon", "SunMoon_00"), 31: ("Pyonchu", "Pyonchu_00"),
        32: ("Hacchin", "Hacchin_00"), 33: ("Donketsu", "Donketsu_00"), 34: ("MagmaFish", "MagmaFish_00"),
        35: ("Fugumannen", "Fugumannen_00"), 36: ("Hopper", "Hopper_00"), 37: ("Koopa", "Koopa_00"),
        38: ("KoopaJr", "KoopaJr_00"), 39: ("Bunbun", "Bunbun_00"), 40: ("Bunbun_01", "Bunbun_01"),
        41: ("Killer", "Killer_00"),
    ]

    /// (GoalCondition label, part name label, icon stem) for `MmkCondition.kind`.
    var entry: (goal: String, part: String?, icon: String?)? {
        switch kind {
        case 1:
            guard let e = Self.enemies[target] else { return nil }
            return ("EnemyKill_02", e.part, e.icon)
        case 2: return ("ItemGet_02", "1upKinoko", "1upKinoko_00")
        case 3:
            let (p, i) = switch target {
            case 10: ("10Coin", "10Coin_00")
            case 30: ("10Coin_01", "10Coin_01")
            case 50: ("10Coin_02", "10Coin_02")
            default: ("Coin", "Coin_00")
            }
            return ("CoinGet_02", p, i)
        case 4: return ("PowBlockShock_02", "PowBlock", "PowBlock_00")
        case 5: return ("PSwitchPress_02", "PSwitch", "PSwitch_00")
        case 6: return ("PlayerOnStar", nil, "SuperStar_00")
        case 7: return ("PlayerMode_00", nil, "SuperKinoko_00")
        case 8: return ("PlayerMode_01", nil, "FireFlower_00")
        case 9: return ("PlayerNoDamage", nil, nil)
        case 10: return ("PlayerNoLand", nil, nil)
        case 11: return ("PlayerOnShoe", "KutsuKuribo_00", "KutsuKuribo_00")
        case 12: return ("PlayerOnItem_00", "KoopaClown_00", "KoopaClown_00")
        case 13: return ("PlayerMode_04", nil, "SuperKonoha_00")
        case 14: return ("PlayerMode_03", nil, "MantleWing_00")
        case 15: return ("PlayerMode_02", nil, "PropellerKinoko_00")
        case 16: return ("PlayerCarry", "NokonokoShell", "NokonokoShell_00")
        case 17: return ("PlayerOnItem_01", "KutsuKuribo", "YosshiEgg_00")
        case 18: return ("PlayerMode_06", nil, "SuperBell_00")
        case 19: return ("PlayerOnItem_02", "KoopaCar", "KoopaCar_00")
        case 20: return ("PlayerMode_07", nil, "SuperHammer_00")
        default: return nil
        }
    }

    /// Count conditions show "count/needed"; the others only the icon.
    var counted: Bool { (1...5).contains(kind) }

    /// The condition's sentence from the import ("Reach the goal after …"), or our own short
    /// wording when the import has no table for it.
    @MainActor func sentence(_ ui: LayoutUI, style: Int) -> String {
        let s = Self.styles[max(0, min(4, style))]
        if let e = entry, let template = ui.message(table: "GoalCondition", e.goal) {
            var args: [String] = []
            if counted { args.append("\(needed)") }
            if let p = e.part {
                let table = counted ? "PartsIconName_plural" : "PartsIconName"
                args.append(ui.message(table: table, "\(s)_\(p)") ?? ui.message(table: table, p) ?? "")
            }
            return UILayout.fill(template, args).replacingOccurrences(of: "\n", with: " ")
        }
        var c = MmkCondition()
        c.kind = UInt32(kind)
        c.target = UInt32(target)
        c.count = UInt32(count)
        c.needed = UInt32(needed)
        return ClearConditionText.describe(c)
    }

    /// Put the icon into picture `pane` (the game style's icon, else another style's).
    @discardableResult
    @MainActor func setIcon(_ l: UILayout, pane: String, style: Int) -> Bool {
        guard let icon = entry?.icon else { return false }
        let own = Self.styles[max(0, min(4, style))]
        for s in [own] + Self.styles where l.picture(pane, group: "MissionIcon", "\(s)_Lyt_M_\(icon)") {
            return true
        }
        return false
    }
}

/// What a session tells the screens each tick.
struct CourseSnapshot {
    var hud = MmkHud()
    var status = MmkPlayStatus()
    var condition: SMM2Condition?
}

/// Everything the course card, pause menu and clear screen show about the course.
struct CourseCardInfo {
    var name = ""
    var maker: String?
    /// "Story Mode" for the built-in courses, else "Course World" (`Cmn_PauseMenu_00` T_Mode_*).
    var builtIn = false
    var bestTicks: Int?
    /// The play has lives (100 Mario / Endless Challenge): the HUD's lives counter and the
    /// course card with "× lives". Library / Course World / Story Mode play has none: unlimited
    /// retries, no game over (DECISIONS.md "Course World without lives").
    var lives = false
    /// `Cmn_PauseMenu_00` mode label (`T_Mode_*`); nil: Story Mode / Course World by `builtIn`.
    var modeLabel: String?
}

/// The SMM2 layouts over a library course. Hosts step it at 60 Hz (`step`) and feed it the
/// engine's state (`update`); `GameSession` decides when each screen appears.
@MainActor
final class CourseScreens: UIScene {
    enum Mode: Equatable { case play, intro, pause, clear, retire, gameOver }

    let ui: LayoutUI
    let hud: UILayout
    private(set) var tick = 0
    private(set) var mode: Mode = .play
    private(set) var modeStart = 0
    var sinceMode: Int { tick - modeStart }
    var info = CourseCardInfo()
    private var snap = CourseSnapshot()
    private var keyIcons: [UILayout] = []
    private var pinkGot: Set<Int> = []
    private var pinkShown = -1
    private var conditionKey: (Int, Int, Int)?
    private var lastStyle: UInt32?
    private var timeCaution = false
    private var hudShown = false

    private lazy var fade = ui.layout("Cmn_Fade_00")
    private lazy var intro = ui.layout("Play_CourseIn_00")
    private lazy var pauseL = ui.layout("Cmn_PauseMenu_00")
    private lazy var clearL = ui.layout("Play_CourseClear_00")
    private lazy var retireL = ui.layout("Play_CourseRetire_00")
    private lazy var timeUpL = ui.layout("Play_TimeUp_00")
    private lazy var gameOverL = ui.layout("Play_GameOver_00")
    /// The game's selection frame, sized to the selected button's bounding pane (as on the title).
    private lazy var cursor: UILayout? = {
        let c = ui.layout("Cmn_Cursor_00")
        c?.play("Loop", at: 0, loop: true)
        return c
    }()
    private var cursorShown = false
    /// A screen still playing its Out animation: (layout, until tick).
    private var closing: (UILayout, Int)?
    private var timeUpUntil = 0
    private var fadeInUntil = 0
    /// The death iris (`Cmn_Fade_00` black circle on the player): its own instance, apart from
    /// the course card's black.
    private lazy var iris: UILayout? = {
        let f = ui.layout("Cmn_Fade_00")
        for p in ["P_FadeB_00", "P_FadeW_00", "W_CircleW_00", "N_Block_00", "N_CaptureFade_00"] { f?.visible(p, false) }
        return f
    }()
    private var irisUntil = 0
    private var irisClosed = false

    var style: Int { Int(min(snap.status.style, 4)) }
    private var skin: String { SMM2Condition.styles[style] }

    var layers: [UILayout] {
        var l: [UILayout] = []
        if mode != .gameOver { l.append(hud) }
        l += keyIcons.prefix(Int(snap.status.keys))
        if tick < timeUpUntil, let t = timeUpL { l.append(t) }
        if let c = closing, tick < c.1 { l.append(c.0) }
        switch mode {
        case .play:
            if tick < fadeInUntil, let f = fade { l.append(f) }
            if irisClosed || tick < irisUntil, let i = iris { l.append(i) }
        case .intro: l += [fade, intro].compactMap { $0 }
        case .pause: l += [fade, pauseL].compactMap { $0 }
        case .clear: if let c = clearL { l.append(c) }
        case .retire: if let r = retireL { l.append(r) }
        case .gameOver: if let g = gameOverL { l.append(g) }
        }
        if cursorShown, let c = cursor { l.append(c) }
        return l
    }

    init?(ui: LayoutUI) {
        guard let h = ui.layout("Play_PlayScene_00") else { return nil }
        self.ui = ui
        hud = h
        // our play has lives: the HUD variant with the lives counter (Type 1) [inf]
        h.hold("Type", 1)
        h.visible("N_1UP_00", false)          // the Endless Challenge's 1-Up progress
        // pop-ups and Ninji / event panes the engine has no data for
        for p in ["L_PointGet_00", "L_MissionClearMark_00", "N_EventFirst_00", "N_EventGhostTime_00"] {
            h.visible(p, false)
        }
        h.hold("In", 0)
    }

    func step(_ input: UIInput) {
        tick += 1
        placeCursor()
    }

    /// The selected button's bounding pane on the current screen.
    private var cursorTarget: (UILayout, String)? {
        switch mode {
        case .pause: pauseL.map { ($0, "\(Self.pauseParts[pauseSelection])/B_Btn_00") }
        case .clear where clearReady: clearL.map { ($0, "\(Self.clearParts[clearSelection])/B_Btn_00") }
        case .retire where retireReady: retireL.map { ($0, "\(Self.retireParts[retireSelection])/B_Btn_00") }
        default: nil
        }
    }

    private func placeCursor() {
        cursorShown = false
        guard let cursor, let (l, pane) = cursorTarget else { return }
        l.apply(tick: tick)
        guard let r = l.rect(pane), r.visible, r.alpha > 0.01 else { return }
        cursor.setPane("W_SelectFrame_00", translate: r.center, size: r.size)
        cursor.setPane("N_All_00", alpha: 255 * r.alpha)
        cursorShown = true
    }

    /// The black fade layer: full (course card) or a dim behind the pause panel (the game blurs
    /// a capture of the course there) [inf].
    private func setFade(dim: Bool) {
        guard let f = fade else { return }
        f.stop("FadeInB")
        f.hold("FadeOutB", f.length("FadeOutB"))
        for p in ["P_FadeW_00", "N_Pos_00", "N_Block_00", "N_CaptureFade_00"] { f.visible(p, false) }
        if dim { f.setPane("P_FadeB_00", alpha: 210) } else { f.setPane("P_FadeB_00") }
    }

    private func setMode(_ m: Mode) {
        mode = m
        modeStart = tick
    }

    /// Number text for an SMM2 value tag: its width follows the tag's kind (9: 2 digits, 10: 3,
    /// 16: 9; the HUD's samples are "99", "999", "123456789") [inf].
    static func digits(_ v: Int, _ width: Int) -> String {
        String(format: "%0\(width)d", max(0, v))
    }

    // MARK: HUD

    /// The engine's state for this tick: counters, timer, condition, pink coins, keys.
    func update(_ s: CourseSnapshot) {
        snap = s
        let h = hud
        if lastStyle != s.status.style {
            lastStyle = s.status.style
            h.hold("Skin", Float(style))
            for k in keyIcons { k.hold("Skin", Float(style)) }
            conditionKey = nil
            pinkShown = -1
        }
        h.hold("TypeCharacter", Float(min(s.status.character, 3)))
        h.text("T_\(skin)_Coin_00", Self.digits(Int(s.hud.coins), 2))
        h.text("T_\(skin)_Zanki_00", Self.digits(Int(s.hud.lives), 2))
        h.text("T_\(skin)_Score_00", Self.digits(Int(s.hud.score), 9))
        let timed = s.hud.time >= 0
        h.visible("N_Time_00", timed ? nil : false)
        if timed { h.text("T_\(skin)_Time_00", Self.digits(Int(s.hud.time), 3)) }
        // the timer pulses in the last 100 seconds (the game's "hurry") [inf: threshold]
        let caution = timed && s.hud.time <= 100 && s.hud.flags & 1 == 0
        if caution != timeCaution {
            timeCaution = caution
            if caution { h.play("TimeCaution", at: tick, loop: true) } else { h.stop("TimeCaution") }
        }
        updateCondition()
        updatePink()
        updateKeys()
    }

    private func updateCondition() {
        let h = hud
        guard let c = snap.condition else {
            h.visible("N_Mission_00", false)
            return
        }
        h.visible("N_Mission_00", nil)
        let key = (c.kind, c.target, style)
        if conditionKey.map({ $0 != key }) ?? true {
            conditionKey = key
            let iconOK = c.setIcon(h, pane: "L_MissionIcon_00/P_DummyIcon_00", style: style)
            h.visible("L_MissionIcon_00/P_DummyIcon_00", iconOK ? nil : false)
        }
        // TypeMission 0: counter window + icon, 1: icon alone
        h.hold("TypeMission", c.counted ? 0 : 1)
        if c.counted {
            let n = h.format("Mission_Num", ["\(min(c.count, c.needed))", "\(c.needed)"]) ?? "\(min(c.count, c.needed))/\(c.needed)"
            h.text("T_MissionNum_00", n)
        }
        // met: the clear frame on; broken (no damage / no landing): the failed cross [inf]
        h.setPane("W_MissionClear_00", alpha: c.met && !c.violated ? 255 : 0)
        if c.violated {
            if !h.isPlaying("InMissionFailed") { h.play("InMissionFailed", at: tick) }
        } else if h.isPlaying("InMissionFailed") {
            h.stop("InMissionFailed")
        }
    }

    /// One slot per pink coin of the course, the rightmost ones; collected ones in colour
    /// (`Play_PinkCoin_00` Type<style> frame 1) with the Get pop.
    private func updatePink() {
        let h = hud
        let total = min(10, Int(snap.status.pink_total))
        guard total > 0 else {
            h.visible("N_PinkCoinScore_00", false)
            return
        }
        h.visible("N_PinkCoinScore_00", nil)
        let got = Int(snap.status.pink_coins)
        if pinkShown != total {
            pinkShown = total
            pinkGot = []
            h.apply(tick: tick)
            for i in 0..<10 { h.visible(String(format: "L_PinkCoin_%02d", i), i >= 10 - total ? nil : false) }
            // the frame's left end before the first shown coin (as authored for two coins) [inf]
            if let first = h.pane(String(format: "L_PinkCoin_%02d", 10 - total)),
               let left = h.pane("P_PinkFrameL_00") {
                h.setPane("P_PinkFrameL_00", translate: SIMD2(first.translate.0 - 7, left.translate.1))
            }
        }
        for k in 0..<total {
            let part = String(format: "L_PinkCoin_%02d", 10 - total + k)
            let collected = k < got
            h.hold("\(part)/Type\(skin)", collected ? 1 : 0)
            if collected && !pinkGot.contains(k) {
                pinkGot.insert(k)
                h.play("\(part)/Get", at: tick)
            } else if !collected && pinkGot.contains(k) {
                pinkGot.remove(k)
                h.stop("\(part)/Get")
            }
        }
    }

    /// Keys held: one `Play_KeyIcon_00` per key in a row under the score [departure: the game
    /// shows them with the player].
    private func updateKeys() {
        let n = min(Int(snap.status.keys), 10)
        while keyIcons.count < n, let k = ui.layout("Play_KeyIcon_00") {
            k.hold("Skin", Float(style))
            let i = keyIcons.count
            k.setPane("P_Key_00", translate: SIMD2(600 - Float(i) * 34, snap.status.pink_total > 0 ? 210 : 262))
            k.play("Get", at: tick)
            keyIcons.append(k)
        }
        for (i, k) in keyIcons.enumerated() {
            k.setPane("P_Key_00", translate: SIMD2(600 - Float(i) * 34, snap.status.pink_total > 0 ? 210 : 262))
        }
    }

    private func hudIn() {
        guard !hudShown else { return }
        hudShown = true
        hud.stop("Out")
        hud.play("In", at: tick)
    }

    private func hudOut() {
        guard hudShown else { return }
        hudShown = false
        hud.stop("In")
        hud.play("Out", at: tick)
    }

    /// HUD and card variants for the play's rules: Type 1 with the lives counter when the play
    /// has lives, Type 0 (coins, score, time) when it hasn't.
    func applyRules() {
        hud.hold("Type", info.lives ? 1 : 0)
    }

    // MARK: death iris

    /// Layout position of a point of the 384 × 216 view (px from its bottom-left).
    static func layoutPoint(viewX x: Float, viewY y: Float) -> SIMD2<Float> {
        let cx = max(0, min(384, x)), cy = max(0, min(216, y))
        return SIMD2((cx - 192) * 1280 / 384, (cy - 108) * 720 / 216)
    }

    /// The black circle closes on `center` (layout units) [inf: the circle and its timing are
    /// read from the fade layout; that a death ends with it is inferred].
    func irisOut(at center: SIMD2<Float>) {
        guard let i = iris else { return }
        i.visible("N_Pos_00", true)
        i.visible("W_CircleB_00", true)
        i.setPane("N_Pos_00", translate: center)
        i.stop("InCircleB")
        i.play("OutCircleB", at: tick)
        irisUntil = tick + Int(max(1, i.length("OutCircleB")))
        irisClosed = true
    }

    /// The course restarted: the circle opens on the player.
    func irisIn(at center: SIMD2<Float>) {
        guard let i = iris else { return }
        i.visible("N_Pos_00", true)
        i.visible("W_CircleB_00", true)
        i.setPane("N_Pos_00", translate: center)
        i.stop("OutCircleB")
        i.play("InCircleB", at: tick)
        irisUntil = tick + Int(max(1, i.length("InCircleB")))
        irisClosed = false
    }

    // MARK: course card

    /// The card's animation: `AnimEndlessMode` ("× lives") when the play has lives, else
    /// `AnimDefault`.
    private var introAnim: String { info.lives ? "AnimEndlessMode" : "AnimDefault" }

    /// The course card on black: name, maker, condition, and lives when the play has them, at
    /// the start (and after each death in a play with lives) [inf]. `lives` is the count the
    /// attempt starts with.
    func showIntro(lives: Int) {
        guard let c = intro, fade != nil else { return }
        hudOut()
        closing = nil
        setFade(dim: false)
        c.text("T_CourseName_00", info.name)
        if let m = info.maker, !m.isEmpty {
            c.hold("Maker", 1)
            c.text("T_PlayerName_00", m)
        } else {
            c.hold("Maker", 0)
        }
        for p in ["N_Align_00", "L_MiiIcon_00", "L_NationalFlag_00", "T_CourseID_00", "L_LoadingGhost_00",
                  "N_EventAlign_00", "N_EventFirst_00", "P_GhostVolumenone_00", "N_ClearCheckAll_00"] {
            c.visible(p, false)
        }
        if let cond = snap.condition {
            c.hold("Mission", 1)
            c.text("T_Mission_00", cond.sentence(ui, style: style))
            let ok = cond.setIcon(c, pane: "L_MissionIcon_00/P_DummyIcon_00", style: style)
            c.visible("L_MissionIcon_00", ok ? nil : false)
            // the icon just left of the centred sentence, the pair centred (the game aligns them;
            // the layout leaves the icon at a fixed place for its longest sentence) [inf]
            if ok, let w = c.textWidth("T_Mission_00") {
                let shift: Float = 38
                c.setPane("T_Mission_00", translate: SIMD2(shift, 0))
                c.setPane("L_MissionIcon_00", translate: SIMD2(max(-440, shift - min(w, 900) / 2 - 45), 0))
            }
        } else {
            c.hold("Mission", 0)
        }
        c.text("T_Zanki_00", ui.format(table: "Play_CourseIn_00", "T_Zanki_Number", ["\(lives)"]) ?? "\(lives)")
        for a in ["AnimEndlessMode", "AnimDefault"] { c.stop(a) }
        c.play(introAnim, at: tick)
        irisClosed = false
        irisUntil = 0
        setMode(.intro)
    }

    var introLength: Int { Int(max(1, intro?.length(introAnim) ?? 1)) }

    /// The card is gone: fade the black away, bring the HUD in.
    func endIntro() {
        guard mode == .intro else { return }
        if let f = fade {
            f.stop("FadeOutB")
            f.play("FadeInB", at: tick)
            fadeInUntil = tick + Int(max(1, f.length("FadeInB")))
        }
        setMode(.play)
        hudIn()
    }

    /// Straight into play (no card): the HUD fades in.
    func startPlaying() {
        setMode(.play)
        hudIn()
    }

    // MARK: pause (`Cmn_PauseMenu_00`)

    /// Pause items top to bottom: the close button (continue), Start Over / Retry, Exit Course.
    static let pauseParts = ["L_CloseBtn_00", "L_RetryBtn_00", "L_ExitBtn_00"]
    private(set) var pauseSelection = 0

    func showPause() {
        guard let p = pauseL else { return }
        // Type 3: Start Over + Exit Course, no Edit / likes / comments; TypeCourseInfo 1: the
        // Course World information block [inf]
        p.hold("Type", 3)
        p.hold("TypeCourseInfo", 1)
        let modeLabel = info.modeLabel ?? (info.builtIn ? "T_Mode_Quest" : "T_Mode_Network")
        if let m = p.message(modeLabel) { p.text("T_Mode_00", m) }
        p.text("T_CourseTitle_00", info.name)
        for n in ["L_MiiIcon_00", "P_GoodIcon_00", "L_GoodCount_00", "N_TagAll_00", "L_KeyBtn_00",
                  "L_OptionBtn_00", "N_PlayerInfo_01/L_Flag_00", "N_PlayerInfo_01/L_BadgeList_00"] {
            p.visible(n, false)
        }
        if let m = info.maker, !m.isEmpty {
            p.visible("N_PlayerInfo_00", nil)
            p.text("T_PlayerName_00", m)
        } else {
            p.visible("N_PlayerInfo_00", false)
        }
        if let t = info.bestTicks {
            let ms = t * 1000 / 60
            p.text("T_Time_01", p.format("0001", [Self.digits(ms / 60000, 2), Self.digits(ms / 1000 % 60, 2)]) ?? "")
            p.text("T_Time_02", p.format("0002", [Self.digits(ms % 1000, 3)]) ?? "")
        } else {
            p.text("T_Time_01", p.message("0003") ?? "")
            p.text("T_Time_02", p.message("0004") ?? "")
        }
        // the clear condition line (Cmn_Mission_00: frame 0 icon + counter, 1 icon, 3 none)
        let m = "L_Mission_00"
        p.visible("\(m)/N_MainMenu_00", false)
        if let c = snap.condition {
            p.hold("\(m)/TypeMission", c.counted ? 0 : 1)
            p.text("\(m)/T_Mission_00", c.sentence(ui, style: style))
            let ok = c.setIcon(p, pane: "\(m)/L_MissionIcon_00/P_DummyIcon_00", style: style)
            p.visible("\(m)/L_MissionIcon_00/P_DummyIcon_00", ok ? nil : false)
            if c.counted { p.text("\(m)/T_MissionNum_00", "\(min(c.count, c.needed))/\(c.needed)") }
            p.visible("\(m)/W_MissionFailed_00", c.violated ? true : nil)
        } else {
            p.hold("\(m)/TypeMission", 3)
            p.text("\(m)/T_Mission_00", ui.message(table: "GoalCondition", "None") ?? "")
        }
        // "Retry" once a checkpoint was reached, else "Start Over"
        let retry = snap.status.flags & 1 != 0 ? "L_RestartBtn_CheckPoint" : "L_RestartBtn_Start"
        if let s = p.message(retry) { p.text("L_RetryBtn_00/T_Btn_00", s) }
        closing = nil
        setFade(dim: true)
        hudOut()
        p.stop("Out")
        p.play("In", at: tick)
        setMode(.pause)
        pauseSelect(0)
    }

    func pauseSelect(_ i: Int) {
        guard let p = pauseL else { return }
        pauseSelection = max(0, min(Self.pauseParts.count - 1, i))
        for (k, part) in Self.pauseParts.enumerated() {
            if k == pauseSelection { p.play("\(part)/Select", at: tick) } else { p.hold("\(part)/Select", 0) }
        }
    }

    /// The chosen button's Decide; returns its length in ticks.
    @discardableResult
    func pauseDecide() -> Int {
        guard let p = pauseL else { return 0 }
        let part = "\(Self.pauseParts[pauseSelection])/Decide"
        p.play(part, at: tick)
        return Int(max(0, p.length(part)))
    }

    func hidePause() {
        guard mode == .pause, let p = pauseL else { return }
        p.stop("In")
        p.play("Out", at: tick)
        closing = (p, tick + Int(max(1, p.length("Out"))))
        hudIn()
        setMode(.play)
    }

    // MARK: course clear (`Play_CourseClear_00`)

    /// 0 Start Over (L_Btn_01, left), 1 Quit (L_Btn_00, right) [inf].
    static let clearParts = ["L_Btn_01", "L_Btn_00"]
    private(set) var clearSelection = 1

    func showClear(ticks: Int?) {
        guard let c = clearL else { return }
        hudOut()
        // Type 2 with the network parts hidden. The banner (N_Cap_00: orange band, COURSE CLEAR)
        // is authored off screen and shown through PF_* panes that distort a capture of it
        // (indirect texturing of a frame copy); here it is drawn in place, undistorted [inf]
        c.hold("Type", 2)
        c.hold("RegionLanguageType", 1)    // USen uses the English "COURSE CLEAR" (as JP) [inf]
        for n in ["PF_Space_00", "PF_Claer_00", "N_JudgeBtn_00", "L_CommentBtn_00", "L_TagBtn_00",
                  "N_CountDown_00", "N_Comment_00"] {
            c.visible(n, false)
        }
        c.setPane("N_Cap_00", translate: .zero)
        c.visible("P_InNetBG_00", true)
        // P_ClearA_00 is an orange card with letter-shaped holes whose colour comes from the
        // frame capture (its second texture); without the capture the holes would only show the
        // band, so the glow lettering under it (P_ClearMax_00, authored to fade to 5%) stays
        // white [inf]
        c.materialWhite("P_ClearMax_00", [255, 255, 255, 255])
        // without the Comment / Tag buttons on its left the pair is centred [departure]
        if let b = c.pane("N_Btn_00") { c.setPane("N_Btn_00", translate: SIMD2(-201, b.translate.1)) }
        if let s = c.message("0000") { c.text("L_Btn_01/T_Btn_00", s) }
        if let s = c.message("0001") { c.text("L_Btn_00/T_Btn_00", s) }
        fillClearInfo(c, ticks: ticks)
        c.stop("OutContents")
        c.play("InNet", at: tick)
        setMode(.clear)
        clearSelect(1)
    }

    private func fillClearInfo(_ l: UILayout, ticks: Int?) {
        let i = "L_ClearInfo_00"
        l.text("\(i)/T_CourseName_00", info.name)
        if let m = info.maker, !m.isEmpty {
            l.visible("\(i)/N_CourseMaker_00", nil)
            l.text("\(i)/T_PlayerName_00", m)
        } else {
            l.visible("\(i)/N_CourseMaker_00", false)
        }
        for n in ["L_MiiIcon_00", "L_MiiIcon_01", "L_MiiIcon_02", "L_NationalFlag_00", "N_Align_00", "N_FirstTime_00"] {
            l.visible("\(i)/\(n)", false)
        }
        guard let ticks else {
            l.visible("\(i)/N_AllTime_00", false)
            return
        }
        let ms = ticks * 1000 / 60
        let main = l.format("0010", [Self.digits(ms / 60000, 2), Self.digits(ms / 1000 % 60, 2)])
        let frac = l.format("0011", [Self.digits(ms % 1000, 3)])
        l.text("\(i)/T_Time_00", main ?? "")
        l.text("\(i)/T_Time_01", frac ?? "")
    }

    /// Buttons take input once InNet has brought them on screen.
    var clearReady: Bool { mode == .clear && sinceMode >= Int(clearL?.length("InNet") ?? 0) }

    func clearSelect(_ i: Int) {
        guard let c = clearL else { return }
        clearSelection = max(0, min(1, i))
        for (k, part) in Self.clearParts.enumerated() {
            if k == clearSelection { c.play("\(part)/Select", at: tick) } else { c.hold("\(part)/Select", 0) }
        }
    }

    @discardableResult
    func clearDecide() -> Int {
        guard let c = clearL else { return 0 }
        let part = "\(Self.clearParts[clearSelection])/Decide"
        c.play(part, at: tick)
        return Int(max(0, c.length(part)))
    }

    // MARK: give up (`Play_CourseRetire_00`)

    /// 0 Exit (L_Btn_00, left), 1 Keep playing (L_Btn_02, right).
    static let retireParts = ["L_Btn_00", "L_Btn_02"]
    private(set) var retireSelection = 1

    func showRetire() {
        guard let r = retireL else { return }
        closing = nil
        r.hold("Type", 0)
        // like the clear banner: the captured N_Cap_00 (purple, "You gave up...") in place of
        // the waves that show a distorted copy of it [inf]
        for n in ["N_JudgeBtn_00", "N_CountDown_00", "P_LoseBG_000", "P_Wave_00", "P_Wave_01", "P_wave_02"] {
            r.visible(n, false)
        }
        r.setPane("N_Cap_00", translate: .zero)
        for n in Self.retireParts { r.visible(n, true) }
        if let s = r.message("0002") { r.text("T_Retire_00", s) }
        if let s = r.message("0004") { r.text("L_Btn_00/T_Btn_00", s) }
        fillClearInfo(r, ticks: nil)
        r.stop("OutContents")
        r.play("InNet", at: tick)
        setMode(.retire)
        retireSelect(1)
    }

    var retireReady: Bool { mode == .retire && sinceMode >= Int(retireL?.length("InNet") ?? 0) / 2 }

    func retireSelect(_ i: Int) {
        guard let r = retireL else { return }
        retireSelection = max(0, min(1, i))
        for (k, part) in Self.retireParts.enumerated() {
            if k == retireSelection { r.play("\(part)/Select", at: tick) } else { r.hold("\(part)/Select", 0) }
        }
    }

    @discardableResult
    func retireDecide() -> Int {
        guard let r = retireL else { return 0 }
        let part = "\(Self.retireParts[retireSelection])/Decide"
        r.play(part, at: tick)
        return Int(max(0, r.length(part)))
    }

    /// "Keep playing": the screen leaves, play goes on.
    func hideRetire() {
        guard mode == .retire, let r = retireL else { return }
        r.stop("InNet")
        r.hold("InNet", r.length("InNet"))
        r.play("OutContents", at: tick)
        closing = (r, tick + Int(max(1, r.length("OutContents"))))
        hudIn()
        setMode(.play)
    }

    // MARK: time up, game over

    /// "TIME'S UP!" over the player's death when the timer runs out.
    func showTimeUp() {
        guard let t = timeUpL else { return }
        t.stop("OneTime")
        t.play("OneTime", at: tick)
        timeUpUntil = tick + Int(max(1, t.length("OneTime")))
    }

    func showGameOver() {
        guard let g = gameOverL else { return }
        hudOut()
        closing = nil
        if let s = g.message("T_GameOver") { g.text("T_Text_00", s) }
        g.play("OneTime", at: tick)
        setMode(.gameOver)
    }

    var gameOverLength: Int { Int(max(60, gameOverL?.length("OneTime") ?? 60)) }

    func clearAll() {
        closing = nil
        timeUpUntil = 0
        setMode(.play)
    }
}
