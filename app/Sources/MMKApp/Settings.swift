import CMmk
#if os(macOS)
import AppKit
#endif
import GameController
import SwiftUI

/// UserDefaults keys. Other parts of the app (and the audio engine) read these directly;
/// they are documented in README ("Settings") — keep both in sync.
enum SettingsKey {
    /// Double 0…1 each (default 1). Consumed by the audio output.
    static let volumeMaster = "volume.master"
    static let volumeMusic = "volume.music"
    static let volumeSfx = "volume.sfx"
    /// Bool (default true): scale the game view by whole numbers only (letterboxed).
    static let integerScale = "display.integerScale"
    /// Bool (default false): window is full screen. Kept in sync with the window.
    static let fullscreen = "display.fullscreen"
    /// JSON `{action: [macOS key code]}` (see `KeyBindings`).
    static let keyBindings = "input.keys"
    /// JSON `{action: [PadButton raw value]}` (see `PadBindings`).
    static let padBindings = "input.pad"
    /// Bool: the user chose to run without a game dump.
    static let usePlaceholder = "assets.placeholder"
    /// Bool (default true, iOS): on-screen d-pad and buttons while no controller is connected.
    static let touchControls = "input.touch"
    /// Bool (default true): play SMM2's opening (`Boot_BootScene_00`) at launch.
    static let bootOpening = "boot.opening"
    /// String (default "smm2"): the 100 Mario Challenge's screens: "smm2" (SMM2's Endless
    /// Challenge menus and in-course screens) or "smm1" (SMM1's own layouts).
    static let hundredPresentation = "hundred.presentation"
    /// String (default empty): the open-source course server Course World also lists courses
    /// from (e.g. "http://localhost:8640"); empty = the local library only.
    static let courseServer = "online.server"
    /// Int 0–3 (default 0): player 1's character, Mario / Luigi / Toad / Toadette (also set from
    /// the SMM2 pause menu's Settings).
    static let character = "player.character"
    /// SMM1 extras (Phase 11), off by default: shell physics, draw order, SMB1 mushroom swap
    /// (0 off / 1 Mystery / 2 Weird), costume (−1 random or an SMM1 costume id).
    static let smm1ShellKicks = "smm1.shellKicks"
    static let smm1Layering = "smm1.layering"
    static let smb1Mushroom = "smm1.mushroom"
    static let costume = "smm1.costume"

    static func registerDefaults() {
        UserDefaults.standard.register(defaults: [
            volumeMaster: 1.0, volumeMusic: 1.0, volumeSfx: 1.0,
            integerScale: true, fullscreen: false, usePlaceholder: false, touchControls: true,
            bootOpening: true, hundredPresentation: "smm2", courseServer: "", character: 0,
        ])
    }
}

extension Notification.Name {
    static let mmkBindingsChanged = Notification.Name("mmk.bindingsChanged")
}

/// Game actions the player can bind.
enum GameAction: String, CaseIterable, Codable, Identifiable {
    case left, right, up, down, jump, run, spin, pause, skip
    var id: String { rawValue }
    var label: String {
        switch self {
        case .left: "Left"
        case .right: "Right"
        case .up: "Up"
        case .down: "Down"
        case .jump: "Jump"
        case .run: "Run / fire"
        case .spin: "Spin jump"
        case .pause: "Pause"
        case .skip: "Skip course (hold, 100 Mario)"
        }
    }
}

struct KeyBindings: Codable, Equatable {
    var keys: [GameAction: [UInt16]]

    static let defaults = KeyBindings(keys: [
        .left: [123, 0], .right: [124, 2], .up: [126, 13], .down: [125, 1], // arrows, WASD
        .jump: [6, 49], .run: [7, 56],                                      // Z/Space, X/Shift
        .spin: [8],                                                         // C
        .pause: [53, 35],                                                   // Esc, P
        .skip: [27, 48],                                                    // − (minus), Tab
    ])

    static func load() -> KeyBindings {
        guard let data = UserDefaults.standard.data(forKey: SettingsKey.keyBindings),
              let raw = try? JSONDecoder().decode([String: [UInt16]].self, from: data)
        else { return .defaults }
        var b = defaults
        for (k, v) in raw { if let a = GameAction(rawValue: k) { b.keys[a] = v } }
        return b
    }

    func save() {
        let raw = Dictionary(uniqueKeysWithValues: keys.map { ($0.key.rawValue, $0.value) })
        UserDefaults.standard.set(try? JSONEncoder().encode(raw), forKey: SettingsKey.keyBindings)
        NotificationCenter.default.post(name: .mmkBindingsChanged, object: nil)
    }
}

/// Controller buttons that can be bound (directions are always the d-pad and left stick).
enum PadButton: String, CaseIterable, Codable {
    case a, b, x, y, leftShoulder, rightShoulder, leftTrigger, rightTrigger, menu, options
    case leftStick, rightStick

    var label: String {
        switch self {
        case .a: "A"
        case .b: "B"
        case .x: "X"
        case .y: "Y"
        case .leftShoulder: "L"
        case .rightShoulder: "R"
        case .leftTrigger: "ZL / L2"
        case .rightTrigger: "ZR / R2"
        case .menu: "Menu (+ / Start)"
        case .options: "Options (− / Select)"
        case .leftStick: "Left stick press"
        case .rightStick: "Right stick press"
        }
    }

    func input(_ g: GCExtendedGamepad) -> GCControllerButtonInput? {
        switch self {
        case .a: g.buttonA
        case .b: g.buttonB
        case .x: g.buttonX
        case .y: g.buttonY
        case .leftShoulder: g.leftShoulder
        case .rightShoulder: g.rightShoulder
        case .leftTrigger: g.leftTrigger
        case .rightTrigger: g.rightTrigger
        case .menu: g.buttonMenu
        case .options: g.buttonOptions
        case .leftStick: g.leftThumbstickButton
        case .rightStick: g.rightThumbstickButton
        }
    }
}

struct PadBindings: Codable, Equatable {
    static let actions: [GameAction] = [.jump, .run, .spin, .pause, .skip]
    var buttons: [GameAction: [PadButton]]

    static let defaults = PadBindings(buttons: [.jump: [.a, .b], .run: [.x, .y], .spin: [.rightShoulder, .rightTrigger],
                                                .pause: [.menu], .skip: [.options]])

    static func load() -> PadBindings {
        guard let data = UserDefaults.standard.data(forKey: SettingsKey.padBindings),
              let raw = try? JSONDecoder().decode([String: [String]].self, from: data)
        else { return .defaults }
        var b = defaults
        for (k, v) in raw {
            if let a = GameAction(rawValue: k) { b.buttons[a] = v.compactMap(PadButton.init(rawValue:)) }
        }
        return b
    }

    func save() {
        let raw = Dictionary(uniqueKeysWithValues: buttons.map { ($0.key.rawValue, $0.value.map(\.rawValue)) })
        UserDefaults.standard.set(try? JSONEncoder().encode(raw), forKey: SettingsKey.padBindings)
        NotificationCenter.default.post(name: .mmkBindingsChanged, object: nil)
    }
}

/// Display names for macOS virtual key codes (ANSI layout).
enum KeyName {
    private static let names: [UInt16: String] = [
        0: "A", 1: "S", 2: "D", 3: "F", 4: "H", 5: "G", 6: "Z", 7: "X", 8: "C", 9: "V", 11: "B",
        12: "Q", 13: "W", 14: "E", 15: "R", 16: "Y", 17: "T", 18: "1", 19: "2", 20: "3", 21: "4",
        22: "6", 23: "5", 24: "=", 25: "9", 26: "7", 27: "-", 28: "8", 29: "0", 30: "]", 31: "O",
        32: "U", 33: "[", 34: "I", 35: "P", 36: "Return", 37: "L", 38: "J", 39: "'", 40: "K",
        41: ";", 42: "\\", 43: ",", 44: "/", 45: "N", 46: "M", 47: ".", 48: "Tab", 49: "Space",
        50: "`", 51: "Delete", 53: "Esc", 54: "Right ⌘", 55: "⌘", 56: "Shift", 57: "Caps Lock",
        58: "Option", 59: "Control", 60: "Right Shift", 61: "Right Option", 62: "Right Control",
        76: "Enter", 96: "F5", 97: "F6", 98: "F7", 99: "F3", 100: "F8", 101: "F9", 103: "F11",
        109: "F10", 111: "F12", 118: "F4", 120: "F2", 122: "F1", 123: "←", 124: "→", 125: "↓", 126: "↑",
    ]

    static func of(_ code: UInt16) -> String { names[code] ?? "Key \(code)" }

    /// macOS key code → USB HID usage (`GCKeyCode` raw value), so iOS hardware keyboards use the
    /// same stored bindings.
    static let hid: [UInt16: Int] = [
        0: 4, 1: 22, 2: 7, 3: 9, 4: 11, 5: 10, 6: 29, 7: 27, 8: 6, 9: 25, 11: 5,              // A S D F H G Z X C V B
        12: 20, 13: 26, 14: 8, 15: 21, 16: 28, 17: 23,                                      // Q W E R Y T
        18: 30, 19: 31, 20: 32, 21: 33, 22: 35, 23: 34, 24: 46, 25: 38, 26: 36, 27: 45,     // 1 2 3 4 6 5 = 9 7 -
        28: 37, 29: 39, 30: 48, 31: 18, 32: 24, 33: 47, 34: 12, 35: 19, 36: 40, 37: 15,     // 8 0 ] O U [ I P Return L
        38: 13, 39: 52, 40: 14, 41: 51, 42: 49, 43: 54, 44: 56, 45: 17, 46: 16, 47: 55,     // J ' K ; \ , / N M .
        48: 43, 49: 44, 50: 53, 51: 42, 53: 41,                                             // Tab Space ` Delete Esc
        54: 231, 55: 227, 56: 225, 57: 57, 58: 226, 59: 224, 60: 229, 61: 230, 62: 228,     // modifiers
        76: 88, 122: 58, 120: 59, 99: 60, 118: 61, 96: 62, 97: 63, 98: 64, 100: 65,         // Enter F1…F8
        101: 66, 109: 67, 103: 68, 111: 69, 123: 80, 124: 79, 125: 81, 126: 82,             // F9…F12 ← → ↓ ↑
    ]
}

// MARK: - Settings window (macOS) / sheet (iOS)

struct SettingsView: View {
    #if os(macOS)
    var body: some View {
        TabView {
            KeyboardSettings().tabItem { Label("Keyboard", systemImage: "keyboard") }
            ControllerSettings().tabItem { Label("Controller", systemImage: "gamecontroller") }
            DisplaySettings().tabItem { Label("Display", systemImage: "display") }
            AudioSettings().tabItem { Label("Audio", systemImage: "speaker.wave.2") }
            GameSettings().tabItem { Label("Game", systemImage: "flag.checkered") }
        }
        .frame(width: 520, height: 420)
    }
    #else
    @Environment(\.dismiss) private var dismiss

    // (hardware keyboards use the default bindings, or ones made on a Mac; no key capture here)
    var body: some View {
        NavigationStack {
            TabView {
                ControllerSettings().tabItem { Label("Controller", systemImage: "gamecontroller") }
                DisplaySettings().tabItem { Label("Display", systemImage: "display") }
                AudioSettings().tabItem { Label("Audio", systemImage: "speaker.wave.2") }
                GameSettings().tabItem { Label("Game", systemImage: "flag.checkered") }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }
    #endif
}

#if os(macOS)
private struct KeyboardSettings: View {
    @State private var bindings = KeyBindings.load()
    @State private var capturing: GameAction?
    @State private var monitor: Any?

    var body: some View {
        Form {
            Section {
                ForEach(GameAction.allCases) { action in
                    LabeledContent(action.label) {
                        HStack {
                            ForEach(bindings.keys[action] ?? [], id: \.self) { code in
                                KeyChip(text: KeyName.of(code)) { remove(code, from: action) }
                            }
                            Button(capturing == action ? "Press a key…" : "Add") { capture(action) }
                                .buttonStyle(.borderless)
                        }
                    }
                }
            } footer: {
                Text("Menus also accept Return to confirm. Click a key to remove it.")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            Button("Restore defaults") { bindings = .defaults; bindings.save() }
        }
        .formStyle(.grouped)
        .onDisappear(perform: stopCapture)
    }

    private func remove(_ code: UInt16, from action: GameAction) {
        bindings.keys[action]?.removeAll { $0 == code }
        bindings.save()
    }

    private func capture(_ action: GameAction) {
        stopCapture()
        capturing = action
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
            // flagsChanged fires on release too; take modifier keys on press only
            if event.type == .flagsChanged && event.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty {
                return nil
            }
            let code = event.keyCode
            for a in GameAction.allCases { bindings.keys[a]?.removeAll { $0 == code } }
            bindings.keys[action, default: []].append(code)
            bindings.save()
            stopCapture()
            return nil
        }
    }

    private func stopCapture() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        capturing = nil
    }
}

#endif

private struct ControllerSettings: View {
    @State private var bindings = PadBindings.load()
    @State private var capturing: GameAction?
    @State private var timer: Timer?
    @State private var controllers: [String] = []

    var body: some View {
        Form {
            Section("Connected") {
                if controllers.isEmpty {
                    Text(Self.pairHint)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(controllers, id: \.self) { Text($0) }
                }
            }
            Section {
                ForEach(PadBindings.actions) { action in
                    LabeledContent(action.label) {
                        HStack {
                            ForEach(bindings.buttons[action] ?? [], id: \.self) { b in
                                KeyChip(text: b.label) {
                                    bindings.buttons[action]?.removeAll { $0 == b }
                                    bindings.save()
                                }
                            }
                            Button(capturing == action ? "Press a button…" : "Add") { capture(action) }
                                .buttonStyle(.borderless)
                        }
                    }
                }
            } header: {
                Text("Buttons")
            } footer: {
                Text("Movement is always the d-pad and left stick. In menus, A confirms and B goes back.")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            Button("Restore defaults") { bindings = .defaults; bindings.save() }
        }
        .formStyle(.grouped)
        .onAppear(perform: refresh)
        .onReceive(NotificationCenter.default.publisher(for: .GCControllerDidConnect)) { _ in refresh() }
        .onReceive(NotificationCenter.default.publisher(for: .GCControllerDidDisconnect)) { _ in refresh() }
        .onDisappear { timer?.invalidate(); timer = nil; capturing = nil }
    }

    #if os(macOS)
    private static let pairHint = "No controller connected. Pair one in System Settings › Bluetooth."
    #else
    private static let pairHint = "No controller connected. Pair one in Settings › Bluetooth; until then the on-screen controls are shown."
    #endif

    private func refresh() {
        controllers = GCController.controllers().map { $0.vendorName ?? "Controller" }
    }

    private func capture(_ action: GameAction) {
        timer?.invalidate()
        capturing = action
        // ignore buttons already held when "Add" was clicked
        let held = Set(GCController.controllers().compactMap(\.extendedGamepad).flatMap { g in
            PadButton.allCases.filter { $0.input(g)?.isPressed == true }
        })
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { t in
            for g in GCController.controllers().compactMap(\.extendedGamepad) {
                if let b = PadButton.allCases.first(where: { $0.input(g)?.isPressed == true && !held.contains($0) }) {
                    for a in PadBindings.actions { bindings.buttons[a]?.removeAll { $0 == b } }
                    bindings.buttons[action, default: []].append(b)
                    bindings.save()
                    t.invalidate()
                    capturing = nil
                    return
                }
            }
        }
    }
}

private struct KeyChip: View {
    let text: String
    let remove: () -> Void

    var body: some View {
        Button(action: remove) {
            Text(text)
                .font(.system(.callout, design: .rounded).weight(.medium))
                .padding(.horizontal, 8).padding(.vertical, 2)
                .background(RoundedRectangle(cornerRadius: 5).fill(Color.secondary.opacity(0.18)))
        }
        .buttonStyle(.plain)
        .help("Remove \(text)")
    }
}

private struct DisplaySettings: View {
    @AppStorage(SettingsKey.fullscreen) private var fullscreen = false
    @AppStorage(SettingsKey.integerScale) private var integerScale = true
    @AppStorage(SettingsKey.touchControls) private var touchControls = true

    var body: some View {
        Form {
            #if os(macOS)
            Toggle("Full screen", isOn: $fullscreen)
            #endif
            Toggle("Integer scaling (sharp pixels, letterboxed)", isOn: $integerScale)
            #if os(macOS)
            Text("Full screen can also be toggled with ⌃⌘F.").font(.footnote).foregroundStyle(.secondary)
            #else
            Toggle("On-screen controls (hidden while a controller is connected)", isOn: $touchControls)
            #endif
        }
        .formStyle(.grouped)
    }
}

/// Front-end choices: the opening at launch, how 100 Mario is presented, the course server.
private struct GameSettings: View {
    @AppStorage(SettingsKey.bootOpening) private var opening = true
    @AppStorage(SettingsKey.hundredPresentation) private var hundred = "smm2"
    @AppStorage(SettingsKey.courseServer) private var server = ""
    @AppStorage(SettingsKey.character) private var character = 0
    @AppStorage(SettingsKey.smm1ShellKicks) private var shellKicks = false
    @AppStorage(SettingsKey.smm1Layering) private var layering = false
    @AppStorage(SettingsKey.smb1Mushroom) private var mushroom = 0
    @AppStorage(SettingsKey.costume) private var costume = -1

    var body: some View {
        Form {
            Picker("Character", selection: $character) {
                Text("Mario").tag(0)
                Text("Luigi").tag(1)
                Text("Toad").tag(2)
                Text("Toadette").tag(3)
            }
            Toggle("Opening at launch (SMM2 logo scene)", isOn: $opening)
            Picker("100 Mario Challenge screens", selection: $hundred) {
                Text("Super Mario Maker 2 (Endless Challenge style)").tag("smm2")
                Text("Super Mario Maker (original)").tag("smm1")
            }
            TextField("Course server", text: $server, prompt: Text("http://localhost:8640"))
            Text("Course World lists your library and, when set, an open-source course server (mmk server). It never talks to Nintendo's network.")
                .font(.footnote).foregroundStyle(.secondary)
            Section("Super Mario Maker extras") {
                Toggle("SMM1 shell physics (mid-air kicks, shell jumps)", isOn: $shellKicks)
                Toggle("SMM1 draw order", isOn: $layering)
                Picker("Mushrooms in SMB1 courses", selection: $mushroom) {
                    Text("Super Mushroom").tag(0)
                    Text("Mystery Mushroom (costumes)").tag(1)
                    Text("Weird Mushroom (Lanky Mario)").tag(2)
                }
                if mushroom == 1 {
                    Picker("Costume", selection: $costume) {
                        Text("Random").tag(-1)
                        ForEach(CostumeList.entries, id: \.id) { c in Text(c.name).tag(c.id) }
                    }
                }
                Text("Costumes and Lanky Mario need the SMM1 import (mmk import-smm1). Courses cleared with shell physics or a mushroom swap aren't sent as records, and online play uses SMM2 rules.")
                    .font(.footnote).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

private struct AudioSettings: View {
    @AppStorage(SettingsKey.volumeMaster) private var master = 1.0
    @AppStorage(SettingsKey.volumeMusic) private var music = 1.0
    @AppStorage(SettingsKey.volumeSfx) private var sfx = 1.0

    var body: some View {
        Form {
            slider("Master", $master)
            slider("Music", $music)
            slider("Sound effects", $sfx)
        }
        .formStyle(.grouped)
    }

    private func slider(_ label: String, _ value: Binding<Double>) -> some View {
        LabeledContent(label) {
            HStack {
                Slider(value: value, in: 0...1)
                Text("\(Int((value.wrappedValue * 100).rounded())) %").monospacedDigit().frame(width: 48, alignment: .trailing)
            }
        }
    }
}

/// The costumes the opened assets have art for (filled when game data opens), for Settings.
@MainActor
enum CostumeList {
    struct Entry: Hashable, Sendable { var id: Int; var name: String }
    static var entries: [Entry] = []

    nonisolated static func read(_ assets: OpaquePointer) -> [Entry] {
        let n = mmk_assets_costume_count(assets)
        return (0..<n).map { i in
            let name = mmk_assets_costume_name(assets, i).map { String(cString: $0) } ?? "Costume \(i)"
            return Entry(id: Int(mmk_assets_costume_id(assets, i)), name: name)
        }
    }
}
