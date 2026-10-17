import AppKit
import CMmk
import GameController

/// Keyboard (arrows/WASD, Z/Space = jump, X/Shift = run) and game controllers
/// (Switch Pro, Joy-Con, DualSense, Xbox via GameController) → engine button mask.
final class InputState {
    private var keysDown = Set<UInt16>()
    private var monitor: Any?

    init() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
            guard let self else { return event }
            switch event.type {
            case .keyDown: self.keysDown.insert(event.keyCode)
            case .keyUp: self.keysDown.remove(event.keyCode)
            case .flagsChanged:
                // Shift keys (56 left, 60 right) arrive as modifier changes
                if event.modifierFlags.contains(.shift) { self.keysDown.insert(56) } else { self.keysDown.remove(56) }
            default: break
            }
            // swallow game keys so the window doesn't beep
            return Self.gameKeys.contains(event.keyCode) ? nil : event
        }
    }

    deinit {
        if let monitor { NSEvent.removeMonitor(monitor) }
    }

    // macOS virtual key codes
    private static let left: Set<UInt16> = [123, 0]      // ←, A
    private static let right: Set<UInt16> = [124, 2]     // →, D
    private static let up: Set<UInt16> = [126, 13]       // ↑, W
    private static let down: Set<UInt16> = [125, 1]      // ↓, S
    private static let jump: Set<UInt16> = [6, 49]       // Z, Space
    private static let run: Set<UInt16> = [7, 56]        // X, Shift
    private static let start: Set<UInt16> = [36, 53]     // Return, Esc
    private static let gameKeys = left.union(right).union(up).union(down).union(jump).union(run).union(start)

    var current: MmkInput {
        var b: UInt32 = 0
        func any(_ s: Set<UInt16>) -> Bool { !keysDown.isDisjoint(with: s) }
        if any(Self.left) { b |= UInt32(MMK_BTN_LEFT) }
        if any(Self.right) { b |= UInt32(MMK_BTN_RIGHT) }
        if any(Self.up) { b |= UInt32(MMK_BTN_UP) }
        if any(Self.down) { b |= UInt32(MMK_BTN_DOWN) }
        if any(Self.jump) { b |= UInt32(MMK_BTN_JUMP) }
        if any(Self.run) { b |= UInt32(MMK_BTN_RUN) }
        if any(Self.start) { b |= UInt32(MMK_BTN_START) }

        for pad in GCController.controllers() {
            guard let g = pad.extendedGamepad else { continue }
            let x = g.leftThumbstick.xAxis.value, y = g.leftThumbstick.yAxis.value
            if g.dpad.left.isPressed || x < -0.4 { b |= UInt32(MMK_BTN_LEFT) }
            if g.dpad.right.isPressed || x > 0.4 { b |= UInt32(MMK_BTN_RIGHT) }
            if g.dpad.up.isPressed || y > 0.4 { b |= UInt32(MMK_BTN_UP) }
            if g.dpad.down.isPressed || y < -0.4 { b |= UInt32(MMK_BTN_DOWN) }
            if g.buttonA.isPressed || g.buttonB.isPressed { b |= UInt32(MMK_BTN_JUMP) }
            if g.buttonX.isPressed || g.buttonY.isPressed { b |= UInt32(MMK_BTN_RUN) }
            if g.buttonMenu.isPressed { b |= UInt32(MMK_BTN_START) }
        }
        return MmkInput(buttons: b)
    }
}
