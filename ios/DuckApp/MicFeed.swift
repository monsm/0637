import AVFoundation
import Foundation

/// The room's microphone, as a Twin-mode sensor.
///
/// Twin semantics: this is the ROOM, never one of the duck's senses. Nothing
/// here feeds the 61-D observation; the RMS and a crude pitch estimate are
/// posted to `/sensor` for an AI client to react to, exactly as the camera's
/// metadata is. Only one summary per second leaves the phone, so the 50 Hz
/// control loop sees no per-frame audio callback.
///
/// There is exactly one `AVAudioSession` per app, and `Voice` already owns it
/// for playback. A second engine with an input tap would race Voice for that
/// same session (and `.ambient` cannot record). `AVAudioRecorder` uses the
/// shared session too, so it has the same conflict. The robust path is to tap
/// `Voice`'s own engine: Voice switches the one session to `.playAndRecord`
/// once, keeps the speaker path, and this class only installs/removes an input
/// tap. The session becomes `.playAndRecord` while the mic is live and goes
/// back to Voice's `.ambient` when it stops; playback uses the same engine, so
/// Voice's setup is idempotent. No audio is written to disk.
final class MicFeed: ObservableObject {
    /// The tap is installed and the engine is delivering input buffers.
    @Published private(set) var available = false
    /// The user refused the microphone; do not ask again from the sheet.
    @Published private(set) var denied = false

    private let voice: Voice
    /// Serial queue for the 1 Hz POST, so a slow local server never touches
    /// the audio thread or the main thread.
    private let queue = DispatchQueue(label: "duck.microphone")
    private var postTimer: Timer?
    /// Written on the real-time audio thread, read on `queue`. Swift 5 mode
    /// has no strict-concurrency checking, and a one-buffer-old snapshot of a
    /// scalar is harmless at 1 Hz; a lock in a tap callback is not (it can
    /// priority-invert the audio thread against the main thread).
    private var latestRMS: Double = 0
    private var latestPeakHz: Double = 0

    init(voice: Voice) {
        self.voice = voice
    }

    /// The user's toggle is off by default. Permission is asked only when the
    /// sheet calls `start()`, never at app launch. Like `CameraFeed`, the OS
    /// status is read first so returning to the sheet does not prompt again.
    func start() {
        switch AVAudioApplication.shared.recordPermission {
        case .granted:
            activate()
        case .undetermined:
            // iOS 17 is the deployment target; AVAudioSession's request API is
            // deprecated there. The completion handler is not on the main
            // thread, so publish the result back on it.
            AVAudioApplication.requestRecordPermission { [weak self] granted in
                DispatchQueue.main.async {
                    guard let self else { return }
                    if granted {
                        self.denied = false
                        self.activate()
                    } else {
                        self.setDenied()
                    }
                }
            }
        case .denied:
            setDenied()
        @unknown default:
            setDenied()
        }
    }

    /// The OS already refused on a previous launch. Reading the permission
    /// state never prompts, so publish the answer without re-asking.
    func setDenied() {
        denied = true
        available = false
    }

    func stop() {
        postTimer?.invalidate()
        postTimer = nil
        voice.stopMicTap()
        available = false
    }

    private func activate() {
        do {
            // Voice reconfigures the one shared session for play-and-record
            // and installs our tap without connecting input to output (that
            // would feed the speaker straight back into the microphone).
            try voice.startMicTap { [weak self] buffer in
                self?.analyse(buffer: buffer)
            }
        } catch {
            NSLog("duck: microphone unavailable: %@", error.localizedDescription)
            return
        }
        available = true
        // A Timer needs a run loop; scheduling on the main one costs a 1 Hz
        // closure, and the network work is handed to `queue` like CameraFeed's
        // metadata path.
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
