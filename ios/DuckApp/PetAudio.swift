import AVFoundation
import Dispatch
import Foundation

/// The pet-detect audio wire: 16 kHz mono i16, one-second windows, base64.
///
/// The official classifier was trained on 16 kHz mono PCM (`pet-detect`
/// README: `arecord -f S16_LE -r 16000 -c 1`), so the phone's hardware rate
/// (usually 48 kHz) is reduced here, on the audio thread, into a
/// preallocated ring. Everything else — windowing, base64, JSON, the local
/// POST — happens on a serial queue. A one-second 16 kHz window is 32 000
/// bytes of PCM and ~42 KB of base64, which is the 42 KB/s the ticket says
/// the loopback tolerates.
///
/// The ticket's "1s=32000 样本" conflates samples and bytes: 16 000 samples
/// × 2 bytes = 32 000 bytes, i.e. 42 KB base64 per second. The wire rate is
/// what the core's one-second window needs, so a window is 16 000 samples.
enum PetPCM {
    static let sampleRate = 16_000
    static let windowSamples = 16_000
    static let wirePath = "/sensor"

    /// The model is the feature's gate: no staged ONNX, no timer, no POST,
    /// no audio thread work. The optional `pet-detect-enabled` marker is not
    /// required; the staged file is the fact. Two names are accepted because
    /// the ticket stages `pet-detect/` while the core's resolver looks for
    /// `models/pet_detect.onnx` beside the policy set; the staging script
    /// writes both.
    static let modelStaged: Bool = {
        guard let resource = Bundle.main.resourceURL else { return false }
        let root = resource.appendingPathComponent("DuckResources")
        return ["pet-detect/pet_detect.onnx", "models/pet_detect.onnx"].contains { relative in
            FileManager.default.fileExists(atPath: root.appendingPathComponent(relative).path)
        }
    }()

    /// How many target samples one tap buffer yields. Rates below 16 kHz
    /// would need upsampling, which would invent detail the microphone did
    /// not hear; those buffers are ignored.
    static func outputCount(frames: Int, sourceRate: Double) -> Int {
        guard frames > 0, sourceRate > 0 else { return 0 }
        guard sourceRate >= Double(sampleRate) else { return 0 }
        // 44 100 / 2.75625 can land a hair below 16 000 in binary; the
        // epsilon keeps a whole second a whole second without adding a
        // sample on any other ratio.
        let exact = Double(frames) * Double(sampleRate) / sourceRate
        return Int(exact + 1e-9)
    }

    /// One box-filtered output sample: average the source span that lands on
    /// this target sample, clamp to [-1, 1], scale to i16. `@inline` because
    /// the audio thread calls it once per output sample.
    @inline(__always)
    static func outputSample(
        _ input: UnsafePointer<Float>,
        frames: Int,
        sourceRate: Double,
        index: Int
    ) -> Int16 {
        guard frames > 0, index >= 0 else { return 0 }
        let ratio = sourceRate / Double(sampleRate)
        let start = Swift.min(frames - 1, Swift.max(0, Int(Double(index) * ratio)))
        let rawEnd = Int(Double(index + 1) * ratio)
        let end = Swift.min(frames, Swift.max(start + 1, rawEnd))
        var sum = 0.0
        for sample in start..<end {
            sum += Double(input[sample])
        }
        let average = sum / Double(end - start)
        let clamped: Double
        if average.isFinite {
            clamped = Swift.min(1.0, Swift.max(-1.0, average))
        } else {
            clamped = 0
        }
        return Int16((clamped * 32767.0).rounded())
    }

    /// The testable pure form of the same reduction.
    static func downsample(_ input: [Float], sourceRate: Double) -> [Int16] {
        let count = outputCount(frames: input.count, sourceRate: sourceRate)
        guard count > 0 else { return [] }
        return input.withUnsafeBufferPointer { buffer in
            guard let base = buffer.baseAddress else { return [] }
            return (0..<count).map {
                outputSample(base, frames: input.count, sourceRate: sourceRate, index: $0)
            }
        }
    }

    /// Little-endian i16 bytes in base64. iOS is little-endian on every
    /// supported device; the pure test pins the round trip.
    static func encodeBase64(_ samples: [Int16]) -> String {
        guard !samples.isEmpty else { return "" }
        return samples.withUnsafeBufferPointer { buffer -> String in
            let data = Data(
                bytes: UnsafeRawPointer(buffer.baseAddress!),
                count: buffer.count * MemoryLayout<Int16>.size
            )
            return data.base64EncodedString()
        }
    }

    /// The exact microphone-reading object the core routes on. Pure so the
    /// contract can be pinned without an audio engine or a network.
    static func postBody(
        pcm16: String,
        rms: Double,
        peakHz: Double,
        tickMS: Int
    ) -> [String: Any] {
        let value: [String: Any] = [
            "rms": rms,
            "peak_hz": peakHz,
            "pcm16": pcm16,
            "rate": sampleRate,
        ]
        return [
            "sensor": "microphone",
            "mode": "twin",
            "value": value,
            "tick_ms": tickMS,
        ]
    }
}

/// The microphone tap's pet-detect branch.
///
/// One producer (the real-time audio thread) writes downsampled i16 into a
/// preallocated four-window ring; one consumer (a 0.5 s timer on a serial
/// queue) takes whole one-second windows, encodes them and POSTs them to the
/// core's local `/sensor`.
///
/// The wire is the core's **microphone reading envelope**, not a bare body:
/// `{"sensor":"microphone","mode":"twin","value":{"rms":…,"peak_hz":…,
/// "pcm16":…,"rate":16000}}`. J1's `POST /sensor` routes on the `sensor`
/// field and `take_pcm16` reads `value.pcm16`; a bare
/// `{"pcm16":…,"rate":…}` is refused as "missing sensor". The ticket's bare
/// shape is therefore not what this build can use; the Android sender still
/// writes it and needs the same alignment. Carrying the latest RMS scalars
/// keeps the reading whole instead of blanking the summary once a second.
///
/// The audio thread never allocates, never locks around the sample loop,
/// never touches JSON or the network. The only lock is a few-instruction
/// index publication after a buffer has been written, which is also what
/// orders those writes before the consumer's reads. If the ring is full
/// (the consumer stalled for four seconds), the incoming buffer is dropped
/// rather than making the audio thread wait.
final class PetAudioStream {
    /// Read by the real-time thread. A one-word flag whose worst case is one
    /// dropped buffer after the user switches the mic off, exactly like
    /// `MicFeed`'s own flags.
    private(set) var active = false

    /// The latest RMS/peak the room feed measured. Set once on the main
    /// thread; called on the audio queue. Reading those scalars off the
    /// audio thread is the same one-buffer-old snapshot `MicFeed` already
    /// documents; it only keeps the microphone reading from losing its
    /// summary to an audio-only post.
    var reading: (() -> (rms: Double, peakHz: Double))?

    private let queue = DispatchQueue(label: "duck.pet-audio")
    private let lock = NSLock()
    /// Four seconds of audio: enough that a slow base64/POST never makes the
    /// producer wait, small enough to stay a trivial fixed allocation.
    private let capacity: Int
    private let storage: UnsafeMutablePointer<Int16>
    /// Published under `lock`; sample bytes are written before the publish.
    private var writeIndex: UInt64 = 0
    private var readIndex: UInt64 = 0
    private var timer: DispatchSourceTimer?

    init() {
        let capacity = PetPCM.windowSamples * 4
        self.capacity = capacity
        storage = UnsafeMutablePointer<Int16>.allocate(capacity: capacity)
        storage.initialize(repeating: 0, count: capacity)
    }

    deinit {
        timer?.cancel()
        timer = nil
        // The serial queue may still be copying a window; let it finish
        // before the fixed storage goes away. `stop()`/`stopAll()` already
        // set `active` false, so that drain will not POST.
        queue.sync {}
        storage.deinitialize(count: capacity)
        storage.deallocate()
    }

    /// Called from `MicFeed.start()` on the main thread, only after the
    /// microphone permission was granted and the tap exists. A missing model
    /// makes this a no-op: nothing runs, nothing leaves the phone.
    func start() {
        guard PetPCM.modelStaged, !active else { return }
        lock.lock()
        writeIndex = 0
        readIndex = 0
        lock.unlock()
        active = true
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(
            deadline: .now() + 0.5,
            repeating: 0.5,
            leeway: .milliseconds(100)
        )
        timer.setEventHandler { [weak self] in self?.drain() }
        self.timer = timer
        timer.resume()
    }

    /// Called from `MicFeed.stop()`/`stopAll()`. A drain already running on
    /// the serial queue sees `active == false` and posts nothing.
    func stop() {
        active = false
        timer?.cancel()
        timer = nil
        lock.lock()
        writeIndex = 0
        readIndex = 0
        lock.unlock()
    }

    /// Real-time audio thread. Downsample straight into the ring: no array,
    /// no closure, no dispatch, no network.
    ///
    /// The sample loop is outside the lock on purpose. The consumer only
    /// reads slots below `readIndex + windowSamples`, the producer only
    /// writes at `writeIndex`, and `count <= free` means those ranges do not
    /// overlap; the second lock publishes `writeIndex` after every sample of
    /// this buffer is written, which is the memory barrier that orders the
    /// consumer's reads after them.
    func append(_ buffer: AVAudioPCMBuffer) {
        guard active, let channel = buffer.floatChannelData?[0] else { return }
        let frames = Int(buffer.frameLength)
        let rate = buffer.format.sampleRate
        let count = PetPCM.outputCount(frames: frames, sourceRate: rate)
        guard count > 0 else { return }
        lock.lock()
        let free = capacity - Int(writeIndex - readIndex)
        lock.unlock()
        guard count <= free else { return }
        let start = Int(writeIndex)
        var index = 0
        while index < count {
            storage[(start + index) % capacity] = PetPCM.outputSample(
                UnsafePointer(channel),
                frames: frames,
                sourceRate: rate,
                index: index
            )
            index += 1
        }
        lock.lock()
        writeIndex += UInt64(count)
        lock.unlock()
    }

    /// Serial queue. Take every whole one-second window that is ready.
    private func drain() {
        guard active else { return }
        while true {
            lock.lock()
            let available = Int(writeIndex - readIndex)
            guard available >= PetPCM.windowSamples else {
                lock.unlock()
                return
            }
            let start = Int(readIndex)
            lock.unlock()

            var window = [Int16](repeating: 0, count: PetPCM.windowSamples)
            for offset in 0..<PetPCM.windowSamples {
                window[offset] = storage[(start + offset) % capacity]
            }

            lock.lock()
            readIndex += UInt64(PetPCM.windowSamples)
            lock.unlock()
            // A stop that landed while this window was being copied owes the
            // phone no POST; the ring is already being reset.
            guard active else { return }
            post(window)
        }
    }

    /// Serial queue. The core's microphone reading, with the PCM window
    /// riding inside `value` where the J1 `take_pcm16` looks for it.
    private func post(_ samples: [Int16]) {
        let latest = reading?() ?? (rms: 0, peakHz: 0)
        let body = PetPCM.postBody(
            pcm16: PetPCM.encodeBase64(samples),
            rms: latest.rms,
            peakHz: latest.peakHz,
            tickMS: Int(Date().timeIntervalSince1970 * 1000)
        )
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let url = URL(string: "http://127.0.0.1:8765\(PetPCM.wirePath)")
        else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = data
        request.timeoutInterval = 2
        URLSession.shared.dataTask(with: request).resume()
    }
}