import AVFoundation
import Foundation

/// The duck's speaker.
///
/// The sound is the robot's own synth — `sounds/` compiled into the core — so
/// the app plays what the physical duck plays, not a recording. The core
/// decides *when* (a greet as the loop comes up, a peck before the servos go
/// quiet, a chirp when asked); this renders the PCM and hands it to
/// `AVAudioEngine`.
///
/// Everything that touches the core runs on the main thread, from the same
/// 50 Hz tick that steps MuJoCo: the core has no lock, and a background read
/// of MuJoCo's state while it is stepping is a data race. Only the finished
/// PCM buffer crosses to `AVAudioEngine`.
final class Voice {
    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var format: AVAudioFormat?
    /// Main-thread only: whether a buffer is still sounding.
    private var playing = false

    func start(core: OpaquePointer) {
        let rate = Double(duck_core_voice_rate())
        guard let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: rate,
            channels: 1,
            interleaved: false
        ) else { return }
        self.format = format
        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: format)
        do {
            // Playback only: a duck that quacks must not silence the user's
            // music, and it has no microphone to claim.
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.ambient, mode: .default, options: [.mixWithOthers])
            try session.setActive(true)
            try engine.start()
            player.play()
        } catch {
            NSLog("duck: voice unavailable: %@", error.localizedDescription)
            return
        }
        NSLog("duck: voice ready at %.0f Hz", rate)
    }

    func stop() {
        player.stop()
        engine.stop()
    }

    /// Play the next queued sound, if any. Called from the main-thread tick.
    func poll(core: OpaquePointer) {
        if playing { return }
        guard let pointer = duck_core_next_sound(core) else { return }
        let tag = String(cString: pointer)
        guard let pcm = Voice.render(core: core, tag: tag) else { return }
        schedule(pcm)
    }

    /// Ask for a tag now (the shell's quack). It goes through the core's
    /// queue, so it is the same path as a greet and each press is a new
    /// variant rather than the same sound twice.
    func play(core: OpaquePointer, tag: String) {
        _ = tag.withCString { duck_core_queue_sound(core, $0) }
    }

    private static func render(core: OpaquePointer, tag: String) -> [Float]? {
        let count = tag.withCString { duck_core_render_sound(core, $0, nil, 0) }
        guard count > 0 else { return nil }
        var samples = [Float](repeating: 0, count: Int(count))
        let written = tag.withCString { pointer in
            samples.withUnsafeMutableBufferPointer { buffer in
                duck_core_render_sound(core, pointer, buffer.baseAddress, count)
            }
        }
        guard written == count else { return nil }
        return samples
    }

    /// Main thread only.
    private func schedule(_ pcm: [Float]) {
        guard let format, let buffer = AVAudioPCMBuffer(
            pcmFormat: format,
            frameCapacity: AVAudioFrameCount(pcm.count)
        ) else { return }
        buffer.frameLength = AVAudioFrameCount(pcm.count)
        if let channel = buffer.floatChannelData?[0] {
            pcm.withUnsafeBufferPointer { source in
                guard let base = source.baseAddress else { return }
                channel.update(from: base, count: pcm.count)
            }
        }
        playing = true
        player.scheduleBuffer(buffer, at: nil, options: []) { [weak self] in
            // Back to the main thread: `playing` is read there by `poll`, and
            // a flag written on the audio thread would be a race.
            DispatchQueue.main.async { self?.playing = false }
        }
    }
}
