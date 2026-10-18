import AVFoundation
import Foundation

/// Plays the SMM2 UI sounds the front end asks for (`mmk_front_sounds`: a WAV of the import's
/// `ui/sound/`, a volume, a delay in frames). Which sound a button or animation plays is decided
/// by `mmk-front` (core/crates/front/src/sound.rs) from the import's `sounds.json`.
@MainActor
final class UISound {
    static let shared = UISound()

    private let dir: URL?
    private var players: [AVAudioPlayer] = []
    private var ends: [ObjectIdentifier: TimeInterval] = [:]
    private var cache: [String: Data] = [:]

    /// Debug: `MMK_DEBUG_SOUND=1` logs every UI sound to stderr.
    private static let log = ProcessInfo.processInfo.environment["MMK_DEBUG_SOUND"] == "1"
    private static let muted = ProcessInfo.processInfo.environment["MMK_MUTE"] == "1"

    private init() {
        dir = AppModel.smm2UIDir?.appendingPathComponent("ui/sound", isDirectory: true)
    }

    func play(_ sound: String, volume: Float, delayFrames: Double) {
        guard sound != "@Blank", let dir, !Self.muted else { return }
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
}
