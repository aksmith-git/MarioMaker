#if os(iOS)
import CMmk
import GameController
import SwiftUI
import UIKit

/// What the on-screen controls hold. One set of controls serves every screen, so this is
/// shared; every `InputState` adds it in (main thread only).
enum TouchInput {
    /// MMK_BTN_* bits held by touches.
    static var buttons: UInt32 = 0
    /// The − button is held (100 Mario skip).
    static var skip = false
}

/// On-screen controls over the game screens (iOS): a d-pad bottom left, Jump / Run / Spin bottom
/// right, + (pause, back in menus) and − (skip, held, in a 100 Mario run) top right. Hidden
/// while a game controller is connected, or when turned off in Settings.
///
/// Debug: MMK_TOUCH=on shows them even with a controller connected (the Simulator usually has
/// one); MMK_TOUCH_HOLD=<control>[,…] (left, right, up, down, jump, run, spin, pause, skip) holds
/// those controls from launch, for screenshots and to drive the touch input path.
struct TouchControls: UIViewRepresentable {
    @AppStorage(SettingsKey.touchControls) private var enabled = true

    func makeUIView(context: Context) -> TouchControlsView { TouchControlsView() }
    func updateUIView(_ view: TouchControlsView, context: Context) { view.enabled = enabled }
}

/// Every finger is tracked on its own (multitouch): a finger on the d-pad steers in 8 directions
/// wherever it moves, a finger on an action button can slide to another one (hold Run, roll onto
/// Jump). Touches that start outside the controls fall through to the views below (the pause
/// menu's buttons).
final class TouchControlsView: UIView {
    enum Control: CaseIterable { case dpad, jump, run, spin, pause, skip }

    private static let actions: [Control] = [.jump, .run, .spin]
    private static let labels: [Control: String] = [.jump: "JUMP", .run: "RUN", .spin: "SPIN", .pause: "+", .skip: "−"]

    /// Circle of each control (centre, radius), from `layoutSubviews`.
    private var circles: [Control: (c: CGPoint, r: CGFloat)] = [:]
    /// Live touches → the control they press (nil: an action finger slid off every button) and
    /// where they are.
    private var tracked: [ObjectIdentifier: (control: Control?, at: CGPoint)] = [:]
    /// MMK_TOUCH_HOLD (debug).
    private var debugHold: UInt32 = 0
    private var debugSkip = false
    private let forceShow = ProcessInfo.processInfo.environment["MMK_TOUCH"] == "on"
    private var observers: [NSObjectProtocol] = []
    private let haptics = UIImpactFeedbackGenerator(style: .light)

    var enabled = true { didSet { if enabled != oldValue { refreshVisibility() } } }

    init() {
        super.init(frame: .zero)
        isOpaque = false
        backgroundColor = .clear
        isMultipleTouchEnabled = true
        contentMode = .redraw
        for name in ProcessInfo.processInfo.environment["MMK_TOUCH_HOLD"]?.split(separator: ",") ?? [] {
            switch name {
            case "left": debugHold |= UInt32(MMK_BTN_LEFT)
            case "right": debugHold |= UInt32(MMK_BTN_RIGHT)
            case "up": debugHold |= UInt32(MMK_BTN_UP)
            case "down": debugHold |= UInt32(MMK_BTN_DOWN)
            case "jump": debugHold |= UInt32(MMK_BTN_JUMP)
            case "run": debugHold |= UInt32(MMK_BTN_RUN)
            case "spin": debugHold |= UInt32(MMK_BTN_SPIN)
            case "pause": debugHold |= UInt32(MMK_BTN_START)
            case "skip": debugSkip = true
            default: break
            }
        }
        let c = NotificationCenter.default
        for name in [Notification.Name.GCControllerDidConnect, .GCControllerDidDisconnect] {
            observers.append(c.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.refreshVisibility() }
            })
        }
        refreshVisibility()
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    deinit {
        observers.forEach(NotificationCenter.default.removeObserver)
        TouchInput.buttons = 0
        TouchInput.skip = false
    }

    private func refreshVisibility() {
        let pad = !forceShow && GCController.controllers().contains { $0.extendedGamepad != nil }
        isHidden = !enabled || pad
        if isHidden { tracked.removeAll() }
        publish()
    }

    // MARK: layout and drawing

    override func layoutSubviews() {
        super.layoutSubviews()
        let safe = bounds.inset(by: safeAreaInsets)
        // one size unit from the short side: 44 pt on an iPhone in landscape, 64 on an iPad
        let u = max(44, min(min(bounds.width, bounds.height) * 0.11, 64))
        let m = u * 0.45
        let dr = u * 1.55
        circles[.dpad] = (CGPoint(x: safe.minX + m + dr, y: safe.maxY - m - dr), dr)
        let br = u * 0.8
        let jump = CGPoint(x: safe.maxX - m - br, y: safe.maxY - m - br - u * 0.35)
        circles[.jump] = (jump, br)
        circles[.run] = (CGPoint(x: jump.x - br * 2.45, y: jump.y + br * 0.5), br)
        circles[.spin] = (CGPoint(x: jump.x - br * 0.55, y: jump.y - br * 2.35), br * 0.85)
        let sr = u * 0.4
        let pause = CGPoint(x: safe.maxX - m - sr, y: safe.minY + m + sr)
        circles[.pause] = (pause, sr)
        circles[.skip] = (CGPoint(x: pause.x - sr * 2.8, y: pause.y), sr)
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        let held = TouchInput.buttons
        let idle = UIColor(white: 0, alpha: 0.28), lit = UIColor(white: 1, alpha: 0.5), edge = UIColor(white: 1, alpha: 0.7)
        for (control, (c, r)) in circles {
            let path = UIBezierPath(arcCenter: c, radius: r, startAngle: 0, endAngle: .pi * 2, clockwise: true)
            path.lineWidth = 2
            if control == .dpad {
                idle.setFill()
                path.fill()
                edge.setStroke()
                path.stroke()
                drawArms(c, r, held: held, lit: lit, edge: edge)
                continue
            }
            (isHeld(control, held) ? lit : idle).setFill()
            path.fill()
            edge.setStroke()
            path.stroke()
            if let label = Self.labels[control] {
                let size = control == .pause || control == .skip ? r * 1.3 : r * 0.42
                let attrs: [NSAttributedString.Key: Any] = [
                    .font: UIFont.systemFont(ofSize: size, weight: .heavy),
                    .foregroundColor: UIColor(white: 1, alpha: 0.9),
                ]
                let s = NSAttributedString(string: label, attributes: attrs)
                let b = s.size()
                s.draw(at: CGPoint(x: c.x - b.width / 2, y: c.y - b.height / 2))
            }
        }
    }

    /// The d-pad cross; held directions light up.
    private func drawArms(_ c: CGPoint, _ r: CGFloat, held: UInt32, lit: UIColor, edge: UIColor) {
        let w = r * 0.42, l = r * 0.78
        let arms: [(UInt32, CGRect)] = [
            (UInt32(MMK_BTN_LEFT), CGRect(x: c.x - l, y: c.y - w / 2, width: l - w / 2, height: w)),
            (UInt32(MMK_BTN_RIGHT), CGRect(x: c.x + w / 2, y: c.y - w / 2, width: l - w / 2, height: w)),
            (UInt32(MMK_BTN_UP), CGRect(x: c.x - w / 2, y: c.y - l, width: w, height: l - w / 2)),
            (UInt32(MMK_BTN_DOWN), CGRect(x: c.x - w / 2, y: c.y + w / 2, width: w, height: l - w / 2)),
        ]
        for (bit, rect) in arms {
            let p = UIBezierPath(roundedRect: rect, cornerRadius: w * 0.25)
            (held & bit != 0 ? lit : UIColor(white: 1, alpha: 0.16)).setFill()
            p.fill()
            edge.withAlphaComponent(0.45).setStroke()
            p.lineWidth = 1
            p.stroke()
        }
    }

    private func isHeld(_ control: Control, _ held: UInt32) -> Bool {
        switch control {
        case .dpad: held & UInt32(MMK_BTN_LEFT | MMK_BTN_RIGHT | MMK_BTN_UP | MMK_BTN_DOWN) != 0
        case .jump: held & UInt32(MMK_BTN_JUMP) != 0
        case .run: held & UInt32(MMK_BTN_RUN) != 0
        case .spin: held & UInt32(MMK_BTN_SPIN) != 0
        case .pause: held & UInt32(MMK_BTN_START) != 0
        case .skip: TouchInput.skip
        }
    }

    // MARK: touches

    /// The control under `p`, with some slack around each circle (fingers are wide).
    private func control(at p: CGPoint, among list: [Control] = Control.allCases, slack: CGFloat = 1.25) -> Control? {
        var best: (Control, CGFloat)?
        for k in list {
            guard let (c, r) = circles[k] else { continue }
            let d = hypot(p.x - c.x, p.y - c.y) / r
            if d <= slack && d < (best?.1 ?? .infinity) { best = (k, d) }
        }
        return best?.0
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        guard !isHidden, control(at: point) != nil else { return nil }   // fall through elsewhere
        return self
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches {
            let p = t.location(in: self)
            if let k = control(at: p) { tracked[ObjectIdentifier(t)] = (k, p) }
        }
        publish()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches {
            let id = ObjectIdentifier(t)
            guard let (k, _) = tracked[id] else { continue }
            let p = t.location(in: self)
            if k == .dpad || k == .pause || k == .skip {
                tracked[id] = (k, p)   // the d-pad steers from anywhere; + and − stay pressed
            } else {
                // action buttons: slide between them; too far from all of them releases
                tracked[id] = (control(at: p, among: Self.actions, slack: 1.6), p)
            }
        }
        publish()
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches { tracked[ObjectIdentifier(t)] = nil }
        publish()
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        touchesEnded(touches, with: event)
    }

    /// Held controls → `TouchInput`, a tap of haptics for each new press, redraw on change.
    private func publish() {
        var b = debugHold
        var skip = debugSkip
        for (k, p) in tracked.values {
            switch k {
            case .dpad?: b |= direction(p)
            case .jump?: b |= UInt32(MMK_BTN_JUMP)
            case .run?: b |= UInt32(MMK_BTN_RUN)
            case .spin?: b |= UInt32(MMK_BTN_SPIN)
            case .pause?: b |= UInt32(MMK_BTN_START)
            case .skip?: skip = true
            case nil: break
            }
        }
        if isHidden { b = 0; skip = false }
        let pressed = b & ~TouchInput.buttons
        if pressed & UInt32(MMK_BTN_JUMP | MMK_BTN_RUN | MMK_BTN_SPIN | MMK_BTN_START) != 0 || (skip && !TouchInput.skip) {
            haptics.impactOccurred()
        }
        if b != TouchInput.buttons || skip != TouchInput.skip {
            TouchInput.buttons = b
            TouchInput.skip = skip
            setNeedsDisplay()
        }
    }

    /// D-pad direction bits for a finger at `p`: 8 sectors of 45°, a small dead zone in the
    /// middle.
    private func direction(_ p: CGPoint) -> UInt32 {
        guard let (c, r) = circles[.dpad] else { return 0 }
        let dx = p.x - c.x, dy = c.y - p.y   // y up
        let len = hypot(dx, dy)
        guard len > r * 0.2 else { return 0 }
        let cosA = dx / len, sinA = dy / len
        let edge: CGFloat = 0.383   // cos 67.5°: each direction covers 135°, diagonals 45°
        var b: UInt32 = 0
        if cosA > edge { b |= UInt32(MMK_BTN_RIGHT) }
        if cosA < -edge { b |= UInt32(MMK_BTN_LEFT) }
        if sinA > edge { b |= UInt32(MMK_BTN_UP) }
        if sinA < -edge { b |= UInt32(MMK_BTN_DOWN) }
        return b
    }
}
#endif
