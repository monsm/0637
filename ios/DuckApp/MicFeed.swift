import AVFoundation
import Combine
import Foundation
import Speech

/// The room's microphone, as a Twin-mode sensor, and the duck's ear.
///
/// Twin semantics: this is the ROOM, never one of the duck's senses. Nothing
/// here feeds the 61-D observation; the RMS and a crude pitch estimate are
/// posted to `/sensor` for an AI client to react to, exactly as the camera's
/// metadata is. Only one summary per second leaves the phone, so the 50 Hz
/// control loop sees no per-frame audio callback.
///
/// The speech recogniser is a different thing entirely: the words a person
/// says are the duck's own input. "翻滚" becomes the roulade skill, and that
/// mapping is the core's (see `core/duck-mobile-core/src/speech.rs`); the
/// platform's job is only to turn sound into text and post it to `/heard`.
///
/// There is exactly one `AVAudioSession` per app, and `Voice` already owns it
/// for playback. A second engine with an input tap would race Voice for that
/// same session (and `.ambient` cannot record). `AVAudioRecorder` uses the
/// shared session too, so it has the same conflict. The robust path is to tap
/// `Voice`'s own engine: Voice switches the one session to `.playAndRecord`
/// once, keeps the speaker path, and this class only installs/removes an
/// input tap. The session becomes `.playAndRecord` while the mic is live and goes
/// back to Voice's `.ambient` when it stops; playback uses the same engine, so
/// Voice's setup is idempotent. No audio is written to disk.
///
/// The tap has two masters — the room's loudness and the duck's ear — so it
/// stays installed while either of them wants it. Nothing else in the app may
/// tap the input node.
final class MicFeed: ObservableObject {
    /// The tap is installed and the engine is delivering input buffers.
    @Published private(set) var available = false
    /// The user refused the microphone; do not ask again from the sheet.
    @Published private(set) var denied = false
    /// What the duck's ear is doing. Mirrored from the listener so the
    /// controller never has to import Speech to draw one row.
    @Published private(set) var speechState: SpeechState = .off
    /// The last thing the duck heard, in the user's own words.
    @Published private(set) var heardText: String = ""

    private let voice: Voice
    /// Serial queue for the 1 Hz POST, so a slow local server never touches
    /// the audio thread or the main thread.
    private let queue = DispatchQueue(label: "duck.microphone")
    /// The duck's ear. It reads the same buffers as the room's loudness:
    /// one engine, one session, one tap.
    private let listener = SpeechListener()
    private var postTimer: Timer?
    /// Written on the real-time audio thread, read on `queue`. Swift 5 mode
    /// has no strict-concurrency checking, and a one-buffer-old snapshot of a
    /// scalar is harmless at 1 Hz; a lock in a tap callback is not (it can
    /// priority-invert the audio thread against the main thread).
    private var latestRMS: Double = 0
    private var latestPeakHz: Double = 0
    /// The two masters of the one tap. The room's loudness and the duck's
    /// ear are separate switches; the tap lives while either wants it.
    private var rmsActive = false
    private var speechActive = false
    private var tapInstalled = false
    private var speechCancellable: AnyCancellable?
    private var heardCancellable: AnyCancellable?

    init(voice: Voice) {
        self.voice = voice
        // The ear answers on its own queue; the sheet reads the mirrors.
        speechCancellable = listener.$state
            .receive(on: RunLoop.main)
            .sink { [weak self] state in
                self?.speechState = state
            }
        heardCancellable = listener.$text
            .receive(on: RunLoop.main)
            .sink { [weak self] text in
                self?.heardText = text
            }
    }

    /// The user's toggle is off by default. Permission is asked only when the
    /// sheet calls `start()`, never at app launch. Like `CameraFeed`, the OS
    /// status is read first so returning to the sheet does not prompt again.
    func start() {
        withRecordPermission { [weak self] granted in
            guard let self, granted else { return }
            self.rmsActive = true
            self.installTap()
            // No tap, no room: posting a silence would be a lie.
            guard self.tapInstalled else { return }
            self.startPosting()
        }
    }

    /// The room's loudness is off. The duck's ear is a different switch, so
    /// the tap stays if the ear still wants it.
    func stop() {
        rmsActive = false
        stopPosting()
        releaseTapIfUnused()
    }

    /// The duck's ear, on. The speech permission is asked inside the
    /// listener, and only here — never at launch.
    func startSpeech() {
        withRecordPermission { [weak self] granted in
            guard let self else { return }
            guard granted else {
                // Without the microphone there is nothing to listen to.
                self.listener.microphoneRefused()
                return
            }
            self.speechActive = true
            self.installTap()
            // A recogniser with no audio is a session that ends in silence
            // every second; do not start one.
            guard self.tapInstalled else { return }
            self.listener.start()
        }
    }

    /// The duck's ear, off.
    func stopSpeech() {
        speechActive = false
        listener.stop()
        releaseTapIfUnused()
    }

    /// The ear comes back on the next launch, but only what the user has
    /// already granted. Nothing is asked here: a permission that was never
    /// given stays unasked.
    func resumeSpeech() {
        guard AVAudioApplication.shared.recordPermission == .granted else { return }
        speechActive = true
        installTap()
        guard tapInstalled else { return }
        listener.resume()
    }

    /// Everything off: the room's loudness and the duck's ear. Used when the
    /// controller goes away.
    func stopAll() {
        rmsActive = false
        speechActive = false
        stopPosting()
        listener.stop()
        releaseTapIfUnused()
    }

    /// The OS already refused on a previous launch. Reading the permission
    /// state never prompts, so publish the answer without re-asking.
    func setDenied() {
        denied = true
        // The tap is the truth about the room: a refused microphone has no
        // tap, but the ear may still hold one the room's feed never used.
        available = tapInstalled
    }

    /// The microphone permission, asked only from a user toggle. The
    /// completion handler is not on the main thread, so the answer comes back
    /// on it.
    private func withRecordPermission(_ body: @escaping (Bool) -> Void) {
        switch AVAudioApplication.shared.recordPermission {
        case .granted:
            body(true)
        case .undetermined:
            // iOS 17 is the deployment target; AVAudioSession's request API is
            // deprecated there.
            AVAudioApplication.requestRecordPermission { granted in
                DispatchQueue.main.async { body(granted) }
            }
        case .denied:
            setDenied()
            body(false)
        @unknown default:
            setDenied()
            body(false)
        }
    }

    /// The one input tap, shared by the room's loudness and the duck's ear.
    /// Voice owns the session and the engine; this only installs the tap.
    private func installTap() {
        guard !tapInstalled else { return }
        do {
            // Voice reconfigures the one shared session for play-and-record
            // and installs our tap without connecting input to output (that
            // would feed the speaker straight back into the microphone).
            try voice.startMicTap { [weak self] buffer in
                self?.handle(buffer: buffer)
            }
            tapInstalled = true
            available = true
        } catch {
            NSLog("duck: microphone unavailable: %@", error.localizedDescription)
        }
    }

    /// The tap is released only when neither master wants it any more.
    private func releaseTapIfUnused() {
        guard tapInstalled, !rmsActive, !speechActive else { return }
        voice.stopMicTap()
        tapInstalled = false
        available = false
    }

    /// The tap callback. This runs on the real-time audio thread, so it does
    /// the cheap scalar analysis and hands the buffer to the recogniser, and
    /// NOTHING else: no JSON, no HTTP, no UI, no allocation of our own. A
    /// slow call here starves the audio engine, and the duck's 50 Hz loop
    /// with it. The one lock in this path is the recogniser's request
    /// pointer, held for a single pointer read.
    private func handle(buffer: AVAudioPCMBuffer) {
        // `rmsActive` and `speechActive` are written on the main thread and
        // read here: a word-sized flag, and the worst case is one window of
        // loudness posted or dropped.
        if rmsActive { analyse(buffer: buffer) }
        if speechActive { listener.append(buffer) }
    }

    /// Runs on the real-time audio thread. RMS is the room's loudness; a
    /// zero-crossing count over the same window is a cheap dominant-frequency
    /// estimate. No FFT, no allocation, no Objective-C calls.
    private func analyse(buffer: AVAudioPCMBuffer) {
        guard let channel = buffer.floatChannelData?[0] else { return }
        let count = Int(buffer.frameLength)
        guard count > 1 else { return }
        let rate = buffer.format.sampleRate
        guard rate > 0 else { return }
        var sumSquares = 0.0
        var crossings = 0
        var previous = Double(channel[0])
        for i in 0..<count {
            let sample = Double(channel[i])
            sumSquares += sample * sample
            // Count only real sign changes: noise around zero must not inflate
            // the frequency estimate into kilohertz.
            if (previous < 0 && sample >= 0) || (previous > 0 && sample <= 0) {
                crossings += 1
            }
            previous = sample
        }
        let seconds = Double(count) / rate
        latestRMS = (sumSquares / Double(count)).squareRoot()
        latestPeakHz = seconds > 0 ? Double(crossings) / (2 * seconds) : 0
    }

    /// The 1 Hz summary of the room. A Timer needs a run loop; scheduling on
    /// the main one costs a 1 Hz closure, and the network work is handed to
    /// `queue` like CameraFeed's metadata path.
    private func startPosting() {
        postTimer?.invalidate()
        let timer = Timer(timeInterval: 1.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.queue.async { self.postReading() }
        }
        RunLoop.main.add(timer, forMode: .common)
        postTimer = timer
        // A one-second blank row is a worse answer than the first sample.
        queue.async { self.postReading() }
    }

    private func stopPosting() {
        postTimer?.invalidate()
        postTimer = nil
    }

    /// Runs on `queue`. The same envelope the camera metadata uses.
    private func postReading() {
        let body: [String: Any] = [
            "sensor": "microphone",
            "mode": "twin",
            "value": [
                "rms": latestRMS,
                "peak_hz": latestPeakHz,
            ],
            "tick_ms": 0,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let url = URL(string: "http://127.0.0.1:8765/sensor")
        else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = data
        request.timeoutInterval = 2
        URLSession.shared.dataTask(with: request).resume()
    }
}

/// What the duck's ear is doing.
///
/// These are states, not readings: a refused permission and a device with
/// no recogniser are both real answers, and the sheet must say which one
/// it is rather than showing a dash.
enum SpeechState: Equatable {
    /// The ear is closed: the user has not asked for voice commands.
    case off
    /// Listening for Mandarin.
    case listening
    /// The speech permission was refused.
    case denied
    /// There is no recogniser here: the simulator, or a locale this device
    /// does not know.
    case unavailable
    /// The recogniser works, but only in the cloud: the on-device model is
    /// not installed, and the audio would have to leave the phone.
    case onDeviceMissing
}

/// The duck's ear: Mandarin in, verbs out.
///
/// Apple's recogniser turns the room's sound into text; the core's word
/// table turns the text into the duck's own verbs. Only the text leaves the
/// phone, and only once an utterance is finished.
///
/// A recognition request holds about a minute of audio at most, so a session
/// is restarted when it ends. Only final results are posted: a partial
/// result per syllable would post the same sentence over and over.
final class SpeechListener: ObservableObject {
    /// The ear's state. Published for the sheet; `MicFeed` mirrors it.
    @Published private(set) var state: SpeechState = .off
    /// The last thing the duck heard.
    @Published private(set) var text: String = ""

    /// Mandarin. The duck's own words are Chinese.
    private let recognizer: SFSpeechRecognizer?
    /// All the recognition bookkeeping runs here: the task, the request, the
    /// restarts and the trailing POST. One serial queue, so only the request
    /// pointer itself needs a lock.
    private let queue = DispatchQueue(label: "duck.speech")
    /// The live request, shared with the audio thread.
    private let sink = RecognitionSink()
    /// Whether the user wants the duck listening. `queue` only.
    private var enabled = false
    /// The running task. `queue` only.
    private var task: SFSpeechRecognitionTask?
    /// The POST still owed for the last utterance. `queue` only.
    private var trailing: DispatchWorkItem?
    /// Consecutive failures, so a broken recogniser is not retried forever.
    private var failures = 0
    /// How long a finished session waits before the next one starts.
    private let cooldown: TimeInterval = 1.0
    /// The trailing delay that turns one utterance into one POST.
    private let trailingDelay: TimeInterval = 0.4
    /// How many sessions may fail in a row before the ear gives up.
    private let failureLimit = 5

    init() {
        // A recogniser for a locale this device does not know is nil, and a
        // nil recogniser is a normal answer, not a crash.
        recognizer = SFSpeechRecognizer(locale: Locale(identifier: "zh-CN"))
    }

    /// The user switched the duck's ear on. The speech permission is asked
    /// HERE and nowhere else — never at launch.
    func start() {
        guard let recognizer else {
            publish(.unavailable)
            return
        }
        // A recogniser that is not available (the simulator has none) must
        // not even ask for a permission: the prompt would promise something
        // this device cannot do.
        guard recognizer.isAvailable else {
            publish(.unavailable)
            return
        }
        switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized:
            begin()
        case .denied, .restricted:
            // The OS refuses a second prompt, so the sheet says "denied".
            publish(.denied)
        case .notDetermined:
            // The handler answers with the new status, not a flag.
            SFSpeechRecognizer.requestAuthorization { [weak self] status in
                // The answer arrives on an arbitrary queue; the state is
                // published on the main thread.
                DispatchQueue.main.async {
                    guard let self else { return }
                    if status == .authorized { self.begin() } else { self.publish(.denied) }
                }
            }
        @unknown default:
            publish(.denied)
        }
    }

    /// The user switched the ear off.
    func stop() {
        queue.async { [weak self] in
            guard let self else { return }
            self.enabled = false
            self.closeSession()
            // Nothing leaves the phone once the ear is shut.
            self.trailing?.cancel()
            self.trailing = nil
            DispatchQueue.main.async { self.state = .off }
        }
    }

    /// The microphone itself was refused, so the duck cannot hear at all.
    func microphoneRefused() {
        queue.async { [weak self] in
            guard let self else { return }
            self.enabled = false
            self.closeSession()
            DispatchQueue.main.async { self.state = .denied }
        }
    }

    /// A persisted "on" from a previous launch is resumed only when the
    /// permission is still granted. Nothing is asked here: `.notDetermined`
    /// and `.denied` both leave the ear shut.
    func resume() {
        guard SFSpeechRecognizer.authorizationStatus() == .authorized else { return }
        begin()
    }

    /// The audio thread hands its buffers here. Nothing else happens on that
    /// thread: no JSON, no HTTP, no UI.
    func append(_ buffer: AVAudioPCMBuffer) {
        sink.append(buffer)
    }

    /// Open the ear and start the first session.
    private func begin() {
        queue.async { [weak self] in
            guard let self else { return }
            self.enabled = true
            self.failures = 0
            // One recogniser at a time: the framework refuses a second
            // request while the first one is still running.
            guard self.task == nil else { return }
            self.startSession()
        }
    }

    /// One recognition session: a fresh request, a fresh task, and the
    /// request handed to the audio thread.
    private func startSession() {
        guard enabled, let recognizer else { return }
        guard recognizer.isAvailable else {
            enabled = false
            publish(.unavailable)
            return
        }
        // On-device only: the room's audio must never leave the phone. If
        // this device cannot recognise Mandarin locally, the ear stays shut
        // rather than send the sound to a server.
        guard recognizer.supportsOnDeviceRecognition else {
            enabled = false
            publish(.onDeviceMissing)
            return
        }
        let request = SFSpeechAudioBufferRecognitionRequest()
        // A partial result is the same sentence again and again; only the
        // final one is an utterance.
        request.shouldReportPartialResults = false
        request.taskHint = .dictation
        request.requiresOnDeviceRecognition = true
        let task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            // Apple's callback runs on its own queue; every piece of
            // bookkeeping happens on `queue` instead.
            self?.queue.async { [weak self] in
                self?.finished(result: result, error: error)
            }
        }
        self.task = task
        sink.set(request)
        publish(.listening)
    }

    /// A session ended: a final transcript, or an error.
    private func finished(result: SFSpeechRecognitionResult?, error: Error?) {
        guard enabled else { return }
        if let result, !result.isFinal {
            // A partial we do not want; the session is still running.
            return
        }
        closeSession()
        if let result {
            let transcript = result.bestTranscription.formattedString
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard !transcript.isEmpty else {
                // Silence is not an utterance: nothing is posted, but the ear
                // stays open for the next one.
                scheduleRestart()
                return
            }
            failures = 0
            publishText(transcript)
            schedulePost(transcript)
            scheduleRestart()
            return
        }
        if let error {
            failures += 1
            NSLog("duck: speech session failed: %@", String(describing: error))
            if failures >= failureLimit {
                // The recogniser is not going to work on this device; stop
                // asking it.
                enabled = false
                publish(.unavailable)
                return
            }
        }
        scheduleRestart()
    }

    /// Stop feeding a request the framework has finished with, and let it go.
    private func closeSession() {
        sink.set(nil)
        task?.finish()
        task = nil
    }

    /// One utterance, one POST. The trailing delay lets the recogniser
    /// settle: without it a sentence that is finalised twice would be posted
    /// twice, and the duck would hear it twice.
    private func schedulePost(_ text: String) {
        trailing?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.post(text)
        }
        trailing = work
        queue.asyncAfter(deadline: .now() + trailingDelay, execute: work)
    }

    /// A recognition request holds a minute of audio at most, so a session
    /// that ends must be followed by a new one while the ear is still open.
    private func scheduleRestart() {
        queue.asyncAfter(deadline: .now() + cooldown) { [weak self] in
            guard let self, self.enabled else { return }
            self.startSession()
        }
    }

    /// The transcript goes to the core's word table, which turns it into a
    /// verb. The same URLSession pattern the room's sensor uses.
    private func post(_ text: String) {
        let body: [String: Any] = ["text": text]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let url = URL(string: "http://127.0.0.1:8765/heard")
        else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = data
        request.timeoutInterval = 2
        URLSession.shared.dataTask(with: request).resume()
    }

    private func publish(_ next: SpeechState) {
        DispatchQueue.main.async { [weak self] in
            self?.state = next
        }
    }

    private func publishText(_ spoken: String) {
        DispatchQueue.main.async { [weak self] in
            self?.text = spoken
        }
    }
}

/// The live recognition request, shared between the audio thread and the
/// recognition queue.
///
/// The audio thread appends buffers to it while the queue swaps it in and
/// out, so the pointer needs a lock. The critical section is one pointer
/// read or write — never a network call, an allocation or a UI hop — so the
/// audio thread waits at most a few nanoseconds. Racing the pointer would be
/// worse: a buffer appended to a request the framework has already ended is
/// undefined.
private final class RecognitionSink {
    private let lock = NSLock()
    private var request: SFSpeechAudioBufferRecognitionRequest?

    func set(_ request: SFSpeechAudioBufferRecognitionRequest?) {
        lock.lock()
        self.request = request
        lock.unlock()
    }

    /// Called on the real-time audio thread.
    func append(_ buffer: AVAudioPCMBuffer) {
        lock.lock()
        request?.append(buffer)
        lock.unlock()
    }
}
