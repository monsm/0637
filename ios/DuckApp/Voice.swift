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
///
/// The app has exactly ONE `AVAudioSession`, so this engine is also where the
/// room's microphone tap lives (see `MicFeed`). Voice owns the session and
/// keeps it usable for playback in both configurations.
final class Voice {
    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var format: AVAudioFormat?
    /// `start()` may be called again after the mic was enabled early;
    /// attaching the same node twice is an exception, not a no-op.
    private var configured = false
    /// Main-thread only: whether a buffer is still sounding.
    private var playing = false
    /// Main-thread only: the mic tap is installed on `engine.inputNode`.
    private var micTapInstalled = false

    enum VoiceError: Error { case invalidFormat }

    func start() {
        do {
            try configureIfNeeded()
            try applySession()
            try ensureRunning()
            NSLog("duck: voice ready at %.0f Hz", format?.sampleRate ?? 0)
        } catch {
            NSLog("duck: voice unavailable: %@", error.localizedDescription)
        }
    }

    /// Attach the player once. The rate comes from the compiled-in core ABI,
    /// so this needs no `DuckCore` pointer and can run before it is loaded.
    private func configureIfNeeded() throws {
        guard !configured else { return }
        let rate = Double(duck_core_voice_rate())
        // A zero rate would make an invalid audio format and throw from the tap.
        guard rate > 0 else { throw VoiceError.invalidFormat }
        guard let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: rate,
            channels: 1,
            interleaved: false
        ) else { throw VoiceError.invalidFormat }
        self.format = format
        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: format)
        configured = true
    }

    /// The session category follows whether the mic is live: `.ambient` for a
    /// duck that only quacks (it must not silence the user's music, and it has
    /// no microphone to claim), `.playAndRecord` once the room is being
    /// listened to. A second engine or an `AVAudioRecorder` would race Voice
    /// for this same session, so there is only this one.
    private func applySession() throws {
        let session = AVAudioSession.sharedInstance()
        if micTapInstalled {
            // Changing an active session's category can invalidate the input
            // node's format, so startMicTap stops the engine first; the error
            // path below restores the old category and restarts playback.
            try session.setCategory(.playAndRecord, mode: .default, options: [.mixWithOthers])
        } else {
            try session.setCategory(.ambient, mode: .default, options: [.mixWithOthers])
        }
        try session.setActive(true)
    }

    private func ensureRunning() throws {
        guard !engine.isRunning else { return }
        try engine.start()
        player.play()
    }

    func stop() {
        stopMicTap()
        player.stop()
        engine.stop()
    }

    /// The room's microphone, on this same engine.
    ///
    /// Input is NOT connected to the output: the tap only reads it, so there
    /// is no feedback loop. Called only after record permission was granted,
    /// and never at launch. The tap is installed while the engine is stopped,
    /// because switching categories can restart it and invalidate the input
    /// node's format; starting the engine last keeps the tap's format valid.
    func startMicTap(onBuffer: @escaping (AVAudioPCMBuffer) -> Void) throws {
        guard !micTapInstalled else { return }
        // Set the flag first so `applySession` picks the record-capable
        // category; roll the whole setup back if any step below fails.
        micTapInstalled = true
        do {
            if engine.isRunning {
                player.stop()
                engine.stop()
            }
            try configureIfNeeded()
            try applySession()
            let input = engine.inputNode
            // Force a mono Float32 tap: the hardware's input format may be
            // interleaved or multi-channel, but MicFeed only reads channel 0
            // as Float32. Passing nil would install a tap in the hardware's
            // own format and make that assumption wrong.
            let mono = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: input.outputFormat(forBus: 0).sampleRate,
                channels: 1,
                interleaved: false
            )
            // 4096 frames is a ~90 ms window at 48 kHz. The tap callback only
            // reduces it to two scalars; the 1 Hz POST reads the last window.
            input.installTap(onBus: 0, bufferSize: 4096, format: mono) { buffer, _ in
                onBuffer(buffer)
            }
            try ensureRunning()
        } catch {
            micTapInstalled = false
            // A partly installed tap would keep the input node alive with a
            // callback whose owner has gone away.
            engine.inputNode.removeTap(onBus: 0)
            try? applySession()
            try? ensureRunning()
            throw error
        }
        NSLog(
            "duck: microphone tap ready at %.0f Hz",
            engine.inputNode.outputFormat(forBus: 0).sampleRate
        )
    }

    /// The user switched the mic off. Give the session back its playback-only
    /// category, stopping the engine first because the category switch can
    /// invalidate the input node; then restart for the duck's own sounds.
    /// `try?` on purpose: a failure here must not break the UI toggle.
    func stopMicTap() {
        guard micTapInstalled else { return }
        player.stop()
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        micTapInstalled = false
        try? applySession()
        try? ensureRunning()
    }

    /// Play the next queued sound, if any. Called from the main-thread tick.
    func poll(core: OpaquePointer) {
        if playing || !engine.isRunning { return }
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
        guard engine.isRunning else { return }
        playing = true
        player.scheduleBuffer(buffer, at: nil, options: []) { [weak self] in
            // Back to the main thread: `playing` is read there by `poll`, and
            // a flag written on the audio thread would be a race.
            DispatchQueue.main.async { self?.playing = false }
        }
    }
}
