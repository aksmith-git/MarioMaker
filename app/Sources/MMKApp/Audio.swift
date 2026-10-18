import AVFoundation
import CMmk

/// Plays the engine's mixer through AVAudioEngine: an AVAudioSourceNode pulls interleaved stereo
/// float from `mmk_audio_render` on the audio thread and splits it into the node's channel buffers.
///
/// MMK_MUTE=1 disables audio entirely (no mixer, no output device).
final class AudioOutput {
    private(set) var handle: OpaquePointer?
    private let engine = AVAudioEngine()
    private var source: AVAudioSourceNode?
    /// Interleaved scratch for one render call, allocated once (the render block must not allocate).
    private let scratch: UnsafeMutablePointer<Float>
    private let scratchFrames = 8192

    /// Opens the cache's sounds and starts output. Returns nil when muted or when the cache has no
    /// audio (the game then runs silently).
    init?(cachePath: String) {
        if ProcessInfo.processInfo.environment["MMK_MUTE"] == "1" { return nil }
        guard let h = mmk_audio_open(cachePath) else {
            NSLog("audio disabled: %@", String(cString: mmk_last_error()))
            return nil
        }
        handle = h
        if let smm1 = AppPaths.findSMM1Cache() {
            mmk_audio_add_smm1_sprites(h, smm1.appendingPathComponent("sprites").path)
        }
        scratch = .allocate(capacity: scratchFrames * 2)
        scratch.initialize(repeating: 0, count: scratchFrames * 2)

        let rate = Double(mmk_audio_sample_rate())
        guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2) else {
            mmk_audio_free(h)
            scratch.deallocate()
            return nil
        }
        let scratch = self.scratch, maxFrames = scratchFrames
        let node = AVAudioSourceNode(format: format) { _, _, frameCount, bufferList -> OSStatus in
            let buffers = UnsafeMutableAudioBufferListPointer(bufferList)
            var done = 0
            let total = Int(frameCount)
            while done < total {
                let n = min(total - done, maxFrames)
                mmk_audio_render(h, scratch, n)
                for (c, buf) in buffers.enumerated() {
                    guard let out = buf.mData?.assumingMemoryBound(to: Float.self) else { continue }
                    if buffers.count == 1 && buf.mNumberChannels == 2 {
                        // interleaved destination
                        (out + done * 2).update(from: scratch, count: n * 2)
                    } else {
                        let ch = min(c, 1)
                        for i in 0..<n { out[done + i] = scratch[i * 2 + ch] }
                    }
                }
                done += n
            }
            return noErr
        }
        source = node
        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        do {
            try engine.start()
        } catch {
            NSLog("audio output unavailable: %@", "\(error)")
            engine.detach(node)
            source = nil
        }
    }

    func setVolumes(master: Float, music: Float, sfx: Float) {
        if let handle { mmk_audio_set_volumes(handle, master, music, sfx) }
    }

    deinit {
        engine.stop()
        if let source { engine.detach(source) }
        if let handle { mmk_audio_free(handle) }
        scratch.deallocate()
    }
}
