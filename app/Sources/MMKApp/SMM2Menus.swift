import CMmk
import Foundation

// Pieces the SMM2 menu screens share: the game's selection frame and scripted debug input.

/// The game's selection frame (`Cmn_Cursor_00`): `W_SelectFrame_00` sized to the selected
/// button's bounding pane, faded with it [inf: as on the title].
@MainActor
final class MenuCursor {
    let layout: UILayout?
    private(set) var shown = false

    init(ui: LayoutUI) {
        layout = ui.layout("Cmn_Cursor_00")
        layout?.play("Loop", at: 0, loop: true)
    }

    var layers: [UILayout] { shown ? layout.map { [$0] } ?? [] : [] }

    /// Frame pane `pane` of `l` (evaluated at `tick`); hidden when it isn't drawn.
    @discardableResult
    func place(on l: UILayout, pane: String, tick: Int, alpha: Float = 1) -> Bool {
        shown = false
        guard let c = layout else { return false }
        l.apply(tick: tick)
        guard let r = l.rect(pane), r.visible, r.alpha > 0.01 else { return false }
        c.setPane("W_SelectFrame_00", translate: r.center, size: r.size)
        c.setPane("N_All_00", alpha: 255 * r.alpha * alpha)
        shown = true
        return true
    }

    func hide() { shown = false }
}

/// Debug captures across screens: `MMK_UI_SCRIPT=<tick>:<button>[,…]` presses menu buttons at
/// global UI ticks (every SMM2 menu screen's step counts one), so a capture can walk through
/// several screens. Buttons: confirm, back, up, down, left, right.
@MainActor
enum DebugInput {
    private(set) static var tick = 0
    private static let script: [Int: MenuInput] = {
        var out: [Int: MenuInput] = [:]
        let names: [String: MenuInput] = ["confirm": .confirm, "back": .back, "up": .up, "down": .down,
                                          "left": .left, "right": .right]
        for item in (ProcessInfo.processInfo.environment["MMK_UI_SCRIPT"] ?? "").split(separator: ",") {
            let kv = item.split(separator: ":")
            if kv.count == 2, let t = Int(kv[0]), let b = names[String(kv[1])] { out[t, default: []].insert(b) }
        }
        return out
    }()

    /// One menu step: the scripted presses due now.
    static func step() -> MenuInput {
        tick += 1
        return script[tick] ?? []
    }
}

/// The nearest of `targets` (centres) from `from` in the pressed direction, for menus laid out
/// on a grid: candidates must lie in that direction (within 60° of it).
func nearest(from: SIMD2<Float>, toward dir: SIMD2<Float>, among targets: [(Int, SIMD2<Float>)]) -> Int? {
    var best: (Int, Float)?
    for (i, c) in targets {
        let d = c - from
        let along = d.x * dir.x + d.y * dir.y
        guard along > 1 else { continue }
        let across = abs(d.x * dir.y - d.y * dir.x)
        guard across <= along * 1.8 else { continue }
        let cost = along + across * 2
        if best.map({ cost < $0.1 }) ?? true { best = (i, cost) }
    }
    return best?.0
}

extension MenuInput {
    /// The direction pressed (layout units, y up), if any.
    var direction: SIMD2<Float>? {
        if contains(.up) { return SIMD2(0, 1) }
        if contains(.down) { return SIMD2(0, -1) }
        if contains(.left) { return SIMD2(-1, 0) }
        if contains(.right) { return SIMD2(1, 0) }
        return nil
    }
}
