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
/// One player node, mono 48 kHz float: the synth's own rate, so nothing is
/// resampled and a quack is bit-for-bit what the robot would have played.
final class Voice {
    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var format: AVAudioFormat?
    private var timer: Timer?
    private var core: OpaquePointer?
    private let queue = DispatchQueue(label: "duck.voice")
    /// Read and written only on `queue`; the audio thread hops back to it.
    private var playing = false

    func start(core: OpaquePointer) {
        self.core = core
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
        // The core queues a greet when the loop comes up; poll for anything it
        // wants played, at a rate that never lets a quack wait on a frame.
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            self?.poll()
        }
        NSLog("duck: voice ready at %.0f Hz", rate)
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        player.stop()
        engine.stop()
    }

    /// Play a tag now, whatever the core's queue says (the shell's quack).
    func play(_ tag: String) {
        queue.async { [weak self] in
            guard let self, let core = self.core else { return }
            if let pcm = Voice.render(core: core, tag: tag) {
                self.schedule(pcm)
            }
        }
    }

    private func poll() {
        queue.async { [weak self] in
            guard let self, let core = self.core, !self.playing else { return }
            guard let pointer = duck_core_next_sound(core) else { return }
            let tag = String(cString: pointer)
            if let pcm = Voice.render(core: core, tag: tag) {
                self.schedule(pcm)
            }
        }
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
        // The completion handler runs on an audio thread; the flag is read by
        // `poll` on the voice queue, so hop back before touching it.
        player.scheduleBuffer(buffer, at: nil, options: []) { [weak self] in
            self?.queue.async { self?.playing = false }
        }
    }
}
