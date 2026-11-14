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
/// keeps it usable for playback in both configurations — and puts it back
/// after the OS takes it away (a call, a route change, the app returning
/// from the background).
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
    /// The session/engine notifications we keep; removed in `deinit`.
    private var observers: [NSObjectProtocol] = []
    /// Main-thread only: stops the configuration-change notification raised
    /// by a restart from recursively calling `resume()` again.
    private var resuming = false
    /// Main-thread only: the last fail-safe restart, so a missing route does
    /// not turn the 50 Hz tick into a retry storm.
    private var lastRecovery = Date.distantPast
    /// Main-thread only: `setActive(true)` last returned without throwing.
    /// `AVAudioSession` has no public `isActive`, and printing a fabricated
    /// one would make the diagnostic line lie.
    private var sessionActive = false
    private var lastError = "-"

    /// The options each configuration must carry, in ONE place.
    ///
    /// `.defaultToSpeaker` is load-bearing, not cosmetic: a `.playAndRecord`
    /// session without it routes playback to the receiver (the earpiece), so
    /// the duck is inaudible from across the room even though every call
    /// succeeds. This was the mute regression: the room mic and the speech
    /// ear are persisted in `@AppStorage` and resumed at launch, so a phone
    /// that had ever enabled either switched this one session to
    /// play-and-record — and every sound went to the receiver. The simulator
    /// keeps the speaker regardless of the option, which is why CI stayed
    /// green while the phone was mute.
    static let playbackOptions: AVAudioSession.CategoryOptions = [.mixWithOthers]
    static let recordOptions: AVAudioSession.CategoryOptions = [.mixWithOthers, .defaultToSpeaker]

    /// Main-thread diagnostic counters. `scheduledSounds` counts the sounds
    /// that really reached `scheduleBuffer`, which is the difference between
    /// "the core queued a tag" and "the duck spoke".
    private(set) var scheduledSounds = 0
    private(set) var playedSounds = 0
    private(set) var droppedSounds = 0
    private(set) var lastScheduledTag = ""
    private(set) var lastDropReason = "-"

    /// `-duck.voiceDiagnostics YES` in DEBUG opens the diagnostic line. The
    /// product UI and the release build never carry it.
    static let diagnosticsEnabled: Bool = {
        #if DEBUG
        return UserDefaults.standard.bool(forKey: "duck.voiceDiagnostics")
        #else
        return false
        #endif
    }()

    /// `-duck.voiceForceRecordPath YES` in DEBUG takes the play-and-record
    /// branch without installing an input tap and without asking for the
    /// microphone. CI has no automatable microphone permission, and the
    /// simulator's input format can be 0 Hz, so this is the only way the
    /// receiver-routing path can be asserted there.
    static let forceRecordPath: Bool = {
        #if DEBUG
        return UserDefaults.standard.bool(forKey: "duck.voiceForceRecordPath")
        #else
        return false
        #endif
    }()

    /// The session is record-capable when the microphone tap is live or when
    /// a test asks for the production record path explicitly.
    private var recordPathActive: Bool { micTapInstalled || Self.forceRecordPath }

    enum VoiceError: Error { case invalidFormat }

    func start() {
        observeSession()
        do {
            try activate()
            NSLog("duck: voice ready at %.0f Hz", format?.sampleRate ?? 0)
        } catch {
            sessionActive = false
            lastError = error.localizedDescription
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

    private func activate() throws {
        try configureIfNeeded()
        try applySession()
        try ensureRunning()
    }

    /// The session category follows whether the mic is live. `.playback` (not
    /// `.ambient`) for a duck that only quacks: an ambient session dies with
    /// the Ring/Silent switch, and a companion toy must still be heard when
    /// the phone is on silent. `.mixWithOthers` keeps the original promise
    /// never to stop the user's music. `.playAndRecord` once the room is
    /// being listened to, with `.defaultToSpeaker` so the duck still talks to
    /// the room instead of the earpiece. A second engine or an
    /// `AVAudioRecorder` would race Voice for this same session, so there is
    /// only this one.
    private func applySession() throws {
        let session = AVAudioSession.sharedInstance()
        if recordPathActive {
            // Changing an active session's category can invalidate the input
            // node's format, so startMicTap stops the engine first; the error
            // path below restores the old category and restarts playback.
            try session.setCategory(.playAndRecord, mode: .default, options: Self.recordOptions)
        } else {
            try session.setCategory(.playback, mode: .default, options: Self.playbackOptions)
        }
        try session.setActive(true)
        sessionActive = true
        lastError = "-"
        NSLog(
            "duck: audio session category=%@ options=%lu mic=%d forced=%d route=%@",
            session.category.rawValue,
            session.categoryOptions.rawValue,
            micTapInstalled ? 1 : 0,
            Self.forceRecordPath ? 1 : 0,
            Self.routeName()
        )
    }

    private func ensureRunning() throws {
        // An engine stop (interruption, route change, background) can swallow
        // the completion handler that clears `playing`; a stale `true` would
        // refuse every future sound.
        if playing && !player.isPlaying {
            playing = false
        }
        guard !engine.isRunning else {
            if !player.isPlaying { player.play() }
            return
        }
        try engine.start()
        player.play()
    }

    /// Put the voice back after the OS took it away. Called by the session
    /// observers below and by the shell when the app becomes active again;
    /// safe to call at any time.
    func resume() {
        guard !resuming else { return }
        resuming = true
        defer { resuming = false }
        do {
            try activate()
            NSLog("duck: voice resumed category=%@", AVAudioSession.sharedInstance().category.rawValue)
        } catch {
            sessionActive = false
            lastError = error.localizedDescription
            NSLog("duck: voice resume failed: %@", error.localizedDescription)
        }
    }

    /// The OS can stop the engine without asking. Before this, one phone call
    /// or one unplugged headset turned the duck silent until the next launch.
    private func observeSession() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        let session = AVAudioSession.sharedInstance()

        observers.append(center.addObserver(
            forName: AVAudioSession.interruptionNotification,
            object: session,
            queue: .main
        ) { [weak self] note in
            guard let self,
                  let rawType = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  let type = AVAudioSession.InterruptionType(rawValue: rawType)
            else { return }
            guard type == .ended else { return }
            let options = AVAudioSession.InterruptionOptions(
                rawValue: note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            )
            if options.contains(.shouldResume) {
                self.resume()
            }
        })

        observers.append(center.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: session,
            queue: .main
        ) { [weak self] _ in
            // A headset leaving can stop the engine; iOS re-routes an engine
            // that is still running, so only a stopped one needs us.
            guard let self, !self.engine.isRunning else { return }
            self.resume()
        })

        observers.append(center.addObserver(
            forName: AVAudioEngine.configurationChangeNotification,
            object: engine,
            queue: .main
        ) { [weak self] _ in
            guard let self, !self.engine.isRunning else { return }
            self.resume()
        })

        observers.append(center.addObserver(
            forName: AVAudioSession.mediaServicesWereResetNotification,
            object: session,
            queue: .main
        ) { [weak self] _ in
            self?.resume()
        })
    }

    deinit {
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    func stop() {
        stopMicTap()
        player.stop()
        engine.stop()
        playing = false
        sessionActive = false
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
                playing = false
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
            lastError = error.localizedDescription
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
        playing = false
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        micTapInstalled = false
        try? applySession()
        try? ensureRunning()
    }

    /// Play the next queued sound, if any. Called from the main-thread tick.
    func poll(core: OpaquePointer) {
        if playing {
            // The completion handler is the normal way this clears; if the
            // engine swallowed it, do not let the latch silence the duck.
            if !player.isPlaying { playing = false } else { return }
        }
        if !engine.isRunning {
            // A fail-safe for an engine the notifications did not explain:
            // at most one attempt every two seconds, so a missing route does
            // not turn the 50 Hz tick into a retry storm.
            let now = Date()
            if now.timeIntervalSince(lastRecovery) >= 2 {
                lastRecovery = now
                resume()
            }
            guard engine.isRunning else { return }
        }
        guard let pointer = duck_core_next_sound(core) else { return }
        let tag = String(cString: pointer)
        guard let pcm = Voice.render(core: core, tag: tag) else {
            recordDrop("render", tag: tag)
            return
        }
        schedule(pcm, tag: tag)
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
    private func schedule(_ pcm: [Float], tag: String) {
        guard let format, let buffer = AVAudioPCMBuffer(
            pcmFormat: format,
            frameCapacity: AVAudioFrameCount(pcm.count)
        ) else {
            recordDrop("buffer", tag: tag)
            return
        }
        buffer.frameLength = AVAudioFrameCount(pcm.count)
        if let channel = buffer.floatChannelData?[0] {
            pcm.withUnsafeBufferPointer { source in
                guard let base = source.baseAddress else { return }
                channel.update(from: base, count: pcm.count)
            }
        }
        guard engine.isRunning else {
            recordDrop("engine", tag: tag)
            return
        }
        playing = true
        scheduledSounds += 1
        lastScheduledTag = tag
        NSLog("duck: voice scheduled tag=%@ frames=%d total=%d", tag, pcm.count, scheduledSounds)
        player.scheduleBuffer(buffer, at: nil, options: []) { [weak self] in
            // Back to the main thread: `playing` is read there by `poll`, and
            // a flag written on the audio thread would be a race.
            DispatchQueue.main.async {
                guard let self else { return }
                self.playing = false
                self.playedSounds += 1
            }
        }
    }

    /// A silent return is evidence, not an absence: keep the reason so a mute
    /// report can say whether the core rendered nothing, the engine was
    /// stopped or the buffer could not be built.
    private func recordDrop(_ reason: String, tag: String) {
        droppedSounds += 1
        lastDropReason = reason
        NSLog("duck: voice dropped reason=%@ tag=%@ total=%d", reason, tag, droppedSounds)
    }

    /// One space-separated line for the UI test and the field log. Values are
    /// sanitized so the line can be parsed by splitting on spaces.
    var diagnostics: String {
        let session = AVAudioSession.sharedInstance()
        return [
            "ready=\(configured ? 1 : 0)",
            "cat=\(session.category.rawValue)",
            "opts=\(Self.optionNames(session.categoryOptions))",
            "recOpts=\(Self.optionNames(Self.recordOptions))",
            "mic=\(micTapInstalled ? 1 : 0)",
            "forced=\(Self.forceRecordPath ? 1 : 0)",
            "active=\(sessionActive ? 1 : 0)",
            "engine=\(engine.isRunning ? 1 : 0)",
            "player=\(player.isPlaying ? 1 : 0)",
            "sched=\(scheduledSounds)",
            "played=\(playedSounds)",
            "drops=\(droppedSounds)",
            "drop=\(lastDropReason)",
            "tag=\(lastScheduledTag.isEmpty ? "-" : lastScheduledTag)",
            "route=\(Self.routeName())",
            "error=\(lastError.replacingOccurrences(of: " ", with: "_"))",
        ].joined(separator: " ")
    }

    private static func optionNames(_ options: AVAudioSession.CategoryOptions) -> String {
        var names: [String] = []
        if options.contains(.mixWithOthers) { names.append("mixWithOthers") }
        if options.contains(.defaultToSpeaker) { names.append("defaultToSpeaker") }
        return names.isEmpty ? "-" : names.joined(separator: ",")
    }

    private static func routeName() -> String {
        let outputs = AVAudioSession.sharedInstance().currentRoute.outputs
        guard !outputs.isEmpty else { return "-" }
        return outputs.map { $0.portType.rawValue }.joined(separator: ",")
    }

    /// The computed record-path options, exposed for the regression test: the
    /// simulator cannot prove the routing, but it can prove the option that
    /// decides it is still there.
    static var recordOptionNames: String { optionNames(recordOptions) }
}
