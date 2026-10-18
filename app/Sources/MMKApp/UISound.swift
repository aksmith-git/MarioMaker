import AVFoundation
import Foundation

// The SMM2 layouts' own sounds (`mmk import-smm2-ui` → `ui/sound/sounds.json` + WAVs), played the
// way the game's data says (import/src/smm2_ui_sound.rs):
// - a layout names its sound user and gives some buttons a sound type (`L_PlayBtn_00` →
//   `makeplaybtn`); selecting a button plays the user's `UB_Select_<type>` (plain `UB_Select`
//   without a type), deciding it `UB_Decide_<type>`; an animation with a sound plays
//   `Anm_<Animation>` (`Anm_<Animation>_<type>` on a typed button);
// - a container key plays its unconditional children (sequences, with their delays), or the one
//   conditional child the context picks: the controller's `Select_Key`, the title's
//   `…_backmove_title`, the game style's `In_Layout_M1` …; volumes and delays (frames) are the
//   data's.
// Which context picks which child (the game's conditions are not read) is ours [inf].

@MainActor
final class UISound {
    static let shared = UISound()

    enum Event { case select, decide }

    private struct LayoutInfo {
        var user: String
        var buttons: [String: String]
        var anims: Set<String>
    }

    private var layouts: [String: LayoutInfo] = [:]
    private var users: [String: [String: Any]] = [:]
    private let dir: URL?
    private var players: [AVAudioPlayer] = []
    private var cache: [String: Data] = [:]

    private init() {
        dir = LayoutUI.smm2?.dir.appendingPathComponent("ui/sound", isDirectory: true)
        guard let dir, let data = try? Data(contentsOf: dir.appendingPathComponent("sounds.json")),
              let doc = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        for (name, v) in doc["layouts"] as? [String: [String: Any]] ?? [:] {
            guard let user = v["user"] as? String else { continue }
            layouts[name] = LayoutInfo(user: user, buttons: v["buttons"] as? [String: String] ?? [:],
                                       anims: Set(v["anims"] as? [String] ?? []))
        }
        users = doc["users"] as? [String: [String: Any]] ?? [:]
    }

    /// The sound type of button `pane` (a part path) in `layout`: its own entry, else the longest
    /// listed prefix of the path (`L_WorldMap_00/L_WorldMapCourse` covers every dot).
    private func type(_ info: LayoutInfo, _ pane: String) -> String? {
        if let t = info.buttons[pane] { return t }
        let last = pane.split(separator: "/").last.map(String.init) ?? pane
        if let t = info.buttons[last] { return t }
        return info.buttons.keys.filter { pane.hasPrefix($0) || last.hasPrefix($0) }.max { $0.count < $1.count }
            .flatMap { info.buttons[$0] }
    }

    /// A button of `layout` was selected / decided. `context` picks a conditional child (e.g.
    /// "title" for the title's back).
    func button(_ layout: String, _ pane: String, _ event: Event, context: [String] = []) {
        guard let info = layouts[layout] else { return }
        let verb = event == .select ? "UB_Select" : "UB_Decide"
        let key = type(info, pane).map { "\(verb)_\($0)" } ?? verb
        play(user: info.user, key: users[info.user]?[key] != nil ? key : verb,
             context: context + (event == .select ? ["Select_Key"] : []))
    }

    /// A "back" decided on `layout` (the user's backmove, by context: title / normal).
    func back(_ layout: String, context: String = "normal") {
        guard let info = layouts[layout] else { return }
        play(user: info.user, key: "UB_Decide_backmove", context: ["UB_Decide_backmove_\(context)"])
    }

    /// Animation `anim` of `layout` started (of button `pane` when given).
    func anim(_ layout: String, _ anim: String, pane: String? = nil, context: [String] = []) {
        guard let info = layouts[layout] else { return }
        var key = "Anm_\(anim)"
        if let pane, let t = type(info, pane) { key += "_\(t)" }
        play(user: info.user, key: key, context: context)
    }

    /// Any key of `layout`'s user (e.g. the pause menu's `In_Layout`).
    func key(_ layout: String, _ key: String, context: [String] = []) {
        guard let info = layouts[layout] else { return }
        play(user: info.user, key: key, context: context)
    }

    private func play(user: String, key: String, context: [String]) {
        guard let node = users[user]?[key] as? [String: Any] else { return }
        play(node, context: context, delay: 0)
    }

    private func play(_ node: [String: Any], context: [String], delay: Double) {
        let d = delay + ((node["delay"] as? NSNumber)?.doubleValue ?? 0)
        if let sound = node["sound"] as? String {
            start(sound, volume: (node["volume"] as? NSNumber)?.floatValue ?? 1, delayFrames: d)
            return
        }
        guard let children = node["children"] as? [[String: Any]] else { return }
        let conditional = children.filter { $0["conditional"] as? Bool == true }
        if let pick = conditional.first(where: { c in context.contains { ($0 == c["key"] as? String) } }) {
            play(pick, context: context, delay: d)
            return
        }
        for c in children where c["conditional"] as? Bool != true {
            play(c, context: context, delay: d)
        }
    }

    /// Debug: `MMK_DEBUG_SOUND=1` logs every UI sound to stderr.
    private static let log = ProcessInfo.processInfo.environment["MMK_DEBUG_SOUND"] == "1"

    private func start(_ sound: String, volume: Float, delayFrames: Double) {
        guard sound != "@Blank", let dir else { return }
        if Self.log {
            FileHandle.standardError.write("UISound \(sound) volume \(volume) delay \(delayFrames)\n".data(using: .utf8)!)
        }
        let data: Data
        if let c = cache[sound] {
            data = c
        } else {
            guard let d = try? Data(contentsOf: dir.appendingPathComponent("\(sound).wav")) else { return }
            cache[sound] = d
            data = d
        }
        guard let p = try? AVAudioPlayer(data: data) else { return }
        let ud = UserDefaults.standard
        func vol(_ k: String) -> Float { ud.object(forKey: k) == nil ? 1 : Float(ud.double(forKey: k)) }
        p.volume = volume * vol(SettingsKey.volumeMaster) * vol(SettingsKey.volumeSfx)
        p.prepareToPlay()
        // keep each player until its sound has played out
        let now = p.deviceCurrentTime
        players.removeAll { $0.deviceCurrentTime > ends[ObjectIdentifier($0)] ?? 0 }
        ends = ends.filter { id, _ in players.contains { ObjectIdentifier($0) == id } }
        players.append(p)
        ends[ObjectIdentifier(p)] = now + delayFrames / 60 + p.duration + 0.5
        if delayFrames > 0 {
            p.play(atTime: now + delayFrames / 60)
        } else {
            p.play()
        }
    }

    private var ends: [ObjectIdentifier: TimeInterval] = [:]
}
