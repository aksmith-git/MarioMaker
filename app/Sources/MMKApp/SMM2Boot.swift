import Foundation

// The SMM2 opening (`Boot_BootScene_00`) at launch, before the title, as the game's boot page
// runs it (private research/specs/smm2-frontend.md "Boot"):
// - Appear picks one opening: the newest one an update added that hasn't been seen yet
//   (Koopalings, then Link, then co-op; seeing a newer one marks the older ones seen too), else at
//   random: 66% InDefault, 8% InCharacter, 8% InKuribo, 2% InCoopMode, 8% InLink, 4% InKoopaling,
//   4% InKoopa [obs]; it plays at rate 1 to its end;
// - InMain (200 frames) brings the characters and the logo to the final picture [obs];
// - Disp holds it; the boot sequence then changes to the title with the black circle fade [obs].
// The game reads no input there [obs]; the opening isn't skippable (Settings can turn it off).
// `Loop` (the characters' idle) is not bound by the page; played from the start [inf].

@MainActor
final class BootScene: UIScene {
    /// The openings in the page's order (index = the binary's choice).
    static let openings = ["InDefault", "InCharacter", "InKuribo", "InKoopa", "InCoopMode", "InLink", "InKoopaling"]

    enum State { case appear, inMain, disp }

    let layout: UILayout
    let opening: String
    private(set) var tick = 0
    private(set) var state = State.appear
    private var stateStart = 0
    /// The final picture is up (Disp): the host changes to the title.
    var onFinished: (() -> Void)?

    var layers: [UILayout] { [layout] }

    /// `opening`: force one (captures, `MMK_BOOT_OPENING`); nil chooses like the game.
    init?(ui: LayoutUI, opening forced: String? = nil) {
        guard let l = ui.layout("Boot_BootScene_00") else { return nil }
        layout = l
        let o = forced.flatMap { Self.openings.contains($0) ? $0 : nil } ?? Self.openings[Self.choose()]
        opening = o
        l.play("Loop", at: 0, loop: true)
        l.play(o, at: 0)
        // the opening's own sound sequence (BootScene's Anm_<opening>) [obs data]
        UISound.shared.anim("Boot_BootScene_00", o)
    }

    /// The opening to play, with the "seen" flags of the update openings kept in UserDefaults
    /// (bit 0 co-op, bit 1 Link, bit 2 Koopalings) like the game's save flags.
    static func choose() -> Int {
        let key = "boot.openingsSeen"
        let d = UserDefaults.standard
        let seen = d.integer(forKey: key)
        if seen & 4 == 0 {
            d.set(seen | 7, forKey: key)
            return 6
        }
        if seen & 2 == 0 {
            d.set(seen | 3, forKey: key)
            return 5
        }
        if seen & 1 == 0 {
            d.set(seen | 1, forKey: key)
            return 4
        }
        let r = Int.random(in: 0..<100)
        switch r {
        case ..<66: return 0
        case ..<74: return 1
        case ..<82: return 2
        case ..<84: return 4
        case ..<92: return 5
        case ..<96: return 6
        default: return 3
        }
    }

    func step(_ input: UIInput) {
        tick += 1
        switch state {
        case .appear:
            if layout.finished(opening, at: tick) {
                state = .inMain
                stateStart = tick
                layout.play("InMain", at: tick)
                UISound.shared.anim("Boot_BootScene_00", "InMain")   // the title call
            }
        case .inMain:
            if layout.finished("InMain", at: tick) {
                state = .disp
                stateStart = tick
                onFinished?()
            }
        case .disp:
            break
        }
    }
}
