import AVFoundation
import Foundation

/// The system voice speaking for the duck — NOT the duck's own synth.
///
/// The duck's own voice is the core's formant synth (`Voice`), the 1:1
/// channel: the app plays what the physical duck plays. This is the phone's
/// speech synthesizer reading the diary's real facts (or the paragraph the
/// person's own model wrote from them) out loud — a possession-mode channel,
/// and every sheet that offers it says so. Nothing here reaches the 61-dim
/// observation; the mouth opens through the same client intent a chorus
/// writes on the robot.
///
/// It deliberately does not touch `AVAudioSession`: `Voice` owns the ONE
/// session this app has, and a second engine would race it. A synthesizer
/// opens no engine — it renders through the shared `.playback` category
/// `Voice` already set, so it mixes with the duck's own sounds and stays
/// audible on silent (the reason `Voice` picks `.playback`, not `.ambient`).
///
/// Delegate callbacks arrive on the synthesizer's own queue; `isSpeaking` is
/// only meaningful on the main thread, where the 50 Hz tick reads it and the
/// UI observes it.
final class Speaker: NSObject, AVSpeechSynthesizerDelegate {
    private let synthesizer = AVSpeechSynthesizer()
    /// Main-thread only. `onChange` exists because the tick is a poll, not an
    /// observation: the diary button needs to flip its icon the moment speech
    /// starts and ends.
    private(set) var isSpeaking = false
    var onChange: ((Bool) -> Void)?
    private var languageCode = "en-US"
    /// The utterance the flag belongs to. A cancelled utterance's late
    /// `didCancel` must not close the beak under the utterance that replaced
    /// it, and a restart (speak over speak) must not flap the mouth closed
    /// then open — so callbacks are only honoured for the CURRENT utterance,
    /// and a restart cancels without dropping the flag.
    private var currentUtterance: AVSpeechUtterance?

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    /// The diary reads in the language the app is showing, not the device's.
    func updateLanguage(_ language: Copy.Language) {
        languageCode = language == .chinese ? "zh-CN" : "en-US"
    }

    /// Speak one text. Cancels what is mid-flight without dropping the flag:
    /// a restart keeps the mouth open instead of closing it for one frame.
    func speak(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        // Cancel without emitting: the old utterance's didCancel is dead by
        // identity, and `isSpeaking` stays true through the hand-over.
        synthesizer.stopSpeaking(at: .immediate)
        let utterance = AVSpeechUtterance(string: trimmed)
        utterance.voice = AVSpeechSynthesisVoice(language: languageCode)
        utterance.rate = AVSpeechUtteranceDefaultSpeechRate
        currentUtterance = utterance
        setSpeaking(true)
        synthesizer.speak(utterance)
    }

    /// The button's stop: drop the flag (and the beak) immediately.
    func stop() {
        currentUtterance = nil
        synthesizer.stopSpeaking(at: .immediate)
        setSpeaking(false)
    }

    private func setSpeaking(_ value: Bool) {
        guard isSpeaking != value else { return }
        isSpeaking = value
        onChange?(value)
    }

    // MARK: AVSpeechSynthesizerDelegate — bounce to main before anyone reads.

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        finish(utterance)
    }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
        finish(utterance)
    }

    private func finish(_ utterance: AVSpeechUtterance) {
        DispatchQueue.main.async {
            guard utterance === self.currentUtterance else { return }
            self.setSpeaking(false)
        }
    }
}
