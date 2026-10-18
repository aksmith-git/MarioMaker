#if os(macOS)
import AppKit
#endif
import CMmk
import GameController

/// Menu navigation buttons (pause menu, overlays).
struct MenuInput: OptionSet {
    let rawValue: UInt8
    static let up = MenuInput(rawValue: 1)
    static let down = MenuInput(rawValue: 2)
    static let confirm = MenuInput(rawValue: 4)
    static let back = MenuInput(rawValue: 8)
    static let left = MenuInput(rawValue: 16)
    static let right = MenuInput(rawValue: 32)
}

/// Keyboard and game controllers (Switch Pro, Joy-Con, DualSense, Xbox via GameController) →
/// engine button mask, using the bindings from Settings. On macOS only keys pressed while
/// `window` is the key window count, so typing in other windows (Settings) doesn't move the
/// player. On iOS hardware keyboards are read through GameController (`GCKeyboard`, the same
/// bindings mapped to HID codes) and the on-screen controls (`TouchInput`) are added in.
final class InputState {
    /// The front end has a text field open (macOS: keys go to `typed` instead of the game).
    nonisolated(unsafe) static var typing = false
    /// Typed keys waiting to be sent, one per tick (Unicode scalars; 8 erase, 10 finish, 27 cancel).
    nonisolated(unsafe) static var typed: [UInt32] = []

    private var observers: [NSObjectProtocol] = []
    private var keys = KeyBindings.load()
    private var pad = PadBindings.load()

    /// Return / keypad Enter always confirm in menus.
    private static let confirmKeys: Set<UInt16> = [36, 76]

    #if os(macOS)
    private var keysDown = Set<UInt16>()
    private var monitor: Any?
    weak var window: NSWindow?

    /// Modifier key code → its flag (modifiers arrive as flagsChanged, not keyDown).
    private static let modifierFlags: [UInt16: NSEvent.ModifierFlags] = [
        56: .shift, 60: .shift, 59: .control, 62: .control, 58: .option, 61: .option, 55: .command, 54: .command,
    ]

    init() {
        let m = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
            guard let self else { return event }
            // only the game window; keep ⌘-shortcuts working
            guard event.window != nil, event.window === self.window else { return event }
            if event.type == .keyDown && event.modifierFlags.contains(.command) { return event }
            // a text field of the front end is open: keys type (Return finishes, Esc cancels,
            // Delete erases) instead of playing
            if Self.typing && event.type == .keyDown {
                switch event.keyCode {
                case 36, 76: Self.typed.append(10)
                case 51: Self.typed.append(8)
                case 53: Self.typed.append(27)
                default:
                    for u in (event.characters ?? "").unicodeScalars where u.value >= 32 && u.value != 127 {
                        Self.typed.append(u.value)
                    }
                }
                return nil
            }
            switch event.type {
            case .keyDown: self.keysDown.insert(event.keyCode)
            case .keyUp: self.keysDown.remove(event.keyCode)
            case .flagsChanged:
                if let flag = Self.modifierFlags[event.keyCode] {
                    if event.modifierFlags.contains(flag) { self.keysDown.insert(event.keyCode) } else { self.keysDown.remove(event.keyCode) }
                }
                return event
            default: break
            }
            // swallow game keys so the window doesn't beep
            return self.isGameKey(event.keyCode) ? nil : event
        }
        monitor = m
        observeBindings()
        // keys released while another app or window is in front never arrive: forget them
        observers.append(NotificationCenter.default.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            self?.keysDown.removeAll()
        })
        observers.append(NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: nil, queue: .main) { [weak self] note in
            if let self, note.object as? NSWindow === self.window { self.keysDown.removeAll() }
        })
    }

    deinit {
        if let monitor { NSEvent.removeMonitor(monitor) }
        observers.forEach(NotificationCenter.default.removeObserver)
    }

    private func anyDown(_ codes: some Sequence<UInt16>) -> Bool { !keysDown.isDisjoint(with: codes) }
    #else
    init() { observeBindings() }

    deinit { observers.forEach(NotificationCenter.default.removeObserver) }

    private func anyDown(_ codes: some Sequence<UInt16>) -> Bool {
        guard let kb = GCKeyboard.coalesced?.keyboardInput else { return false }
        return codes.contains { code in
            KeyName.hid[code].map { kb.button(forKeyCode: GCKeyCode(rawValue: $0))?.isPressed == true } ?? false
        }
    }
    #endif

    private func observeBindings() {
        observers.append(NotificationCenter.default.addObserver(forName: .mmkBindingsChanged, object: nil, queue: .main) { [weak self] _ in
            self?.keys = KeyBindings.load()
            self?.pad = PadBindings.load()
        })
    }

    private func isGameKey(_ code: UInt16) -> Bool {
        Self.confirmKeys.contains(code) || keys.keys.values.contains { $0.contains(code) }
    }

    private func held(_ action: GameAction) -> Bool {
        if let codes = keys.keys[action], anyDown(codes) { return true }
        return false
    }

    private func padHeld(_ action: GameAction, _ g: GCExtendedGamepad) -> Bool {
        (pad.buttons[action] ?? []).contains { $0.input(g)?.isPressed == true }
    }

    /// Engine buttons. The pause action is reported as MMK_BTN_START (the shell consumes it).
    var current: MmkInput {
        var b: UInt32 = 0
        if held(.left) { b |= UInt32(MMK_BTN_LEFT) }
        if held(.right) { b |= UInt32(MMK_BTN_RIGHT) }
        if held(.up) { b |= UInt32(MMK_BTN_UP) }
        if held(.down) { b |= UInt32(MMK_BTN_DOWN) }
        if held(.jump) { b |= UInt32(MMK_BTN_JUMP) }
        if held(.run) { b |= UInt32(MMK_BTN_RUN) }
        if held(.spin) { b |= UInt32(MMK_BTN_SPIN) }
        if held(.pause) { b |= UInt32(MMK_BTN_START) }

        for c in GCController.controllers() {
            guard let g = c.extendedGamepad else { continue }
            let x = g.leftThumbstick.xAxis.value, y = g.leftThumbstick.yAxis.value
            if g.dpad.left.isPressed || x < -0.4 { b |= UInt32(MMK_BTN_LEFT) }
            if g.dpad.right.isPressed || x > 0.4 { b |= UInt32(MMK_BTN_RIGHT) }
            if g.dpad.up.isPressed || y > 0.4 { b |= UInt32(MMK_BTN_UP) }
            if g.dpad.down.isPressed || y < -0.4 { b |= UInt32(MMK_BTN_DOWN) }
            if padHeld(.jump, g) { b |= UInt32(MMK_BTN_JUMP) }
            if padHeld(.run, g) { b |= UInt32(MMK_BTN_RUN) }
            if padHeld(.spin, g) { b |= UInt32(MMK_BTN_SPIN) }
            if padHeld(.pause, g) { b |= UInt32(MMK_BTN_START) }
        }
        #if os(iOS)
        b |= TouchInput.buttons
        #endif
        return MmkInput(buttons: b)
    }

    /// The 100 Mario skip key is held (Settings: "Skip course", default − / Tab, controller −).
    var skipHeld: Bool {
        if held(.skip) { return true }
        #if os(iOS)
        if TouchInput.skip { return true }
        #endif
        return GCController.controllers().contains { c in c.extendedGamepad.map { padHeld(.skip, $0) } ?? false }
    }

    /// Buttons held for menu navigation.
    var menu: MenuInput {
        var m: MenuInput = []
        if held(.up) { m.insert(.up) }
        if held(.down) { m.insert(.down) }
        if held(.left) { m.insert(.left) }
        if held(.right) { m.insert(.right) }
        if anyDown(Self.confirmKeys) || held(.jump) { m.insert(.confirm) }
        if held(.pause) { m.insert(.back) }
        for c in GCController.controllers() {
            guard let g = c.extendedGamepad else { continue }
            let x = g.leftThumbstick.xAxis.value, y = g.leftThumbstick.yAxis.value
            if g.dpad.up.isPressed || y > 0.5 { m.insert(.up) }
            if g.dpad.down.isPressed || y < -0.5 { m.insert(.down) }
            if g.dpad.left.isPressed || x < -0.5 { m.insert(.left) }
            if g.dpad.right.isPressed || x > 0.5 { m.insert(.right) }
            if g.buttonA.isPressed { m.insert(.confirm) }
            if g.buttonB.isPressed || padHeld(.pause, g) { m.insert(.back) }
        }
        #if os(iOS)
        // on-screen controls: the d-pad moves, Jump confirms, Run and + go back
        let t = TouchInput.buttons
        if t & UInt32(MMK_BTN_UP) != 0 { m.insert(.up) }
        if t & UInt32(MMK_BTN_DOWN) != 0 { m.insert(.down) }
        if t & UInt32(MMK_BTN_LEFT) != 0 { m.insert(.left) }
        if t & UInt32(MMK_BTN_RIGHT) != 0 { m.insert(.right) }
        if t & UInt32(MMK_BTN_JUMP) != 0 { m.insert(.confirm) }
        if t & UInt32(MMK_BTN_RUN | MMK_BTN_START) != 0 { m.insert(.back) }
        #endif
        // a key bound to both (e.g. Return as pause) confirms
        if m.contains(.confirm) { m.remove(.back) }
        return m
    }
}
