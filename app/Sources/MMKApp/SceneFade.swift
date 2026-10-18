import CMmk
import QuartzCore
import SwiftUI

// Fades between scenes with SMM2's own fade layout (`Cmn_Fade_00`), the way the game's scene
// fade sequence uses it (private research/specs/smm2-frontend.md "Scene fades"):
// - a fade has a kind that picks a pair of the layout's animations: black (FadeOutB / FadeInB,
//   30 + 30 frames), white (FadeOutW / FadeInW), black circle (OutCircleB / InCircleB, 60 + 60),
//   white circle, blocks (OutBlock / InBlock, 56 + 56) [obs];
// - the Out animation covers the old scene; when it has ended the scene changes, and once the new
//   one is up the In animation uncovers it [obs];
// - a scene change without fade parameters uses the black fade at rate 1 [obs]: the title's
//   choices, Course World's screens. The boot opening hands over to the title with the black
//   circle [obs].
// How many frames the covered screen stays black while the next scene loads is the loader's
// business; here it is two ticks (one frame of the new screen drawn under the cover) [inf].

/// The fade layer shared by every screen: Metal screens draw `layers` last (Renderer,
/// UIScreenRenderer); SwiftUI screens show `cover` as a black veil.
@MainActor
final class SceneFade: ObservableObject {
    static let shared = SceneFade()

    enum Kind: Int {
        case black = 1, white, circleBlack, circleWhite, block

        var out: String {
            switch self {
            case .black: "FadeOutB"
            case .white: "FadeOutW"
            case .circleBlack: "OutCircleB"
            case .circleWhite: "OutCircleW"
            case .block: "OutBlock"
            }
        }

        var `in`: String {
            switch self {
            case .black: "FadeInB"
            case .white: "FadeInW"
            case .circleBlack: "InCircleB"
            case .circleWhite: "InCircleW"
            case .block: "InBlock"
            }
        }

        /// The panes this kind draws (the others stay hidden; the layout at rest is all black).
        var panes: [String] {
            switch self {
            case .black: ["P_FadeB_00"]
            case .white: ["P_FadeW_00"]
            case .circleBlack: ["N_Pos_00", "W_CircleB_00"]
            case .circleWhite: ["N_Pos_00", "W_CircleW_00"]
            case .block: ["N_Block_00"]
            }
        }
    }

    private enum Phase { case idle, out, covered, into }

    private static let allPanes = ["P_FadeB_00", "P_FadeW_00", "N_Pos_00", "W_CircleB_00", "W_CircleW_00",
                                   "N_Block_00", "N_CaptureFade_00"]

    private(set) lazy var layout: UILayout? = LayoutUI.smm2?.layout("Cmn_Fade_00")
    private var phase = Phase.idle
    private var kind = Kind.black
    private var start = 0.0
    private var phaseStart = 0
    private var action: (() -> Void)?
    private var onDone: (() -> Void)?
    private var timer: Timer?
    /// Ticks (60 per second) since the fade started.
    private(set) var tick = 0
    /// How much of the screen is covered (0–1), for the SwiftUI screens' veil.
    @Published private(set) var cover: Double = 0

    var active: Bool { phase != .idle }

    /// The layout to draw over the current screen (empty while idle).
    var layers: [UILayout] {
        guard phase != .idle, let l = layout else { return [] }
        l.apply(tick: tick)
        return [l]
    }

    /// Cover the screen with `kind`, run `change` (the scene switch), uncover; `done` at the end.
    /// Without the SMM2 import (or in frozen debug captures) the switch happens at once.
    func run(_ kind: Kind, at center: SIMD2<Float> = .zero, change: @escaping () -> Void, done: (() -> Void)? = nil) {
        if phase != .idle {
            // a fade already runs: switch now, keep its uncovering
            change()
            return
        }
        guard DebugCapture.freezeTicks == nil, let l = begin(kind, center: center) else {
            change()
            done?()
            return
        }
        action = change
        onDone = done
        l.play(kind.out, at: 0)
        phase = .out
        cover = 0
    }

    /// Uncover only: a screen that starts covered (the title after the boot opening's circle)
    /// plays the In animation of `kind`.
    func reveal(_ kind: Kind, at center: SIMD2<Float> = .zero) {
        guard phase == .idle, DebugCapture.freezeTicks == nil, let l = begin(kind, center: center) else { return }
        l.hold(kind.out, l.length(kind.out))
        phase = .covered
        cover = 1
    }

    private func begin(_ kind: Kind, center: SIMD2<Float>) -> UILayout? {
        guard let l = layout else { return nil }
        self.kind = kind
        for p in Self.allPanes { l.visible(p, false) }
        for p in kind.panes { l.visible(p, true) }
        l.setPane("N_Pos_00", translate: center)
        for k in [Kind.black, .white, .circleBlack, .circleWhite, .block] {
            l.stop(k.in)
            l.stop(k.out)
        }
        start = CACurrentMediaTime()
        tick = 0
        phaseStart = 0
        let t = Timer(timeInterval: 1.0 / 120, repeats: true) { _ in
            MainActor.assumeIsolated { SceneFade.shared.advance() }
        }
        RunLoop.main.add(t, forMode: .common)
        timer?.invalidate()
        timer = t
        return l
    }

    private func advance() {
        guard phase != .idle, let l = layout else { return }
        tick = Int((CACurrentMediaTime() - start) * 60)
        let since = tick - phaseStart
        switch phase {
        case .out:
            let n = Int(l.length(kind.out))
            cover = min(1, Double(since) / Double(max(1, n)))
            if since >= n {
                phase = .covered
                phaseStart = tick
                cover = 1
                let a = action
                action = nil
                a?()
            }
        case .covered:
            if since >= 2 {
                l.stop(kind.out)
                l.play(kind.in, at: tick)
                phase = .into
                phaseStart = tick
            }
        case .into:
            let n = Int(l.length(kind.in))
            cover = max(0, 1 - Double(since) / Double(max(1, n)))
            if since >= n {
                phase = .idle
                cover = 0
                timer?.invalidate()
                timer = nil
                l.stop(kind.in)
                let d = onDone
                onDone = nil
                d?()
            }
        case .idle:
            break
        }
    }
}

/// The fade veil over SwiftUI screens (library, import): black at the fade's cover.
struct SceneFadeVeil: View {
    @ObservedObject var fade = SceneFade.shared

    var body: some View {
        Color.black.opacity(fade.cover)
            .ignoresSafeArea()
            .allowsHitTesting(fade.active)
    }
}
