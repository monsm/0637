import Combine
import CoreHaptics
import Foundation
import UIKit

/// The duck's heartbeat, felt in the palm.
///
/// The core publishes `care.heart_bpm` in /state — a pure reading of the
/// same mood/hunger/bond/asleep numbers the capsules show, never a new
/// event. This renders that reading as a haptic pulse on CoreHaptics
/// (iOS 13+); an engine that fails to start (simulator, no haptic hardware)
/// falls back to UIKit's impact generator, and both failures leave the
/// capsule row untouched — the heart is a bonus sense, not a load-bearing
/// one. The mapping bpm→pattern lives on both platforms; the duck never
/// feels its own heart.
final class Heart {
    /// The one engine, started lazily on first enable: creating a CHHapticEngine
    /// at launch would cost battery for a feature the person may never open.
    private var engine: CHHapticEngine?
    private var engineCapable = false
    /// UIKit fallback for devices CoreHaptics cannot serve.
    private let fallback = UIImpactFeedbackGenerator(style: .soft)
    /// Main-thread only: whether the palm should feel the duck right now.
    private(set) var enabled = false
    /// The last bpm the tick scheduled; the UI row shows it.
    @Published var bpm: Double = 0
    /// Main-thread only: beats scheduled since enable, for the diagnostics line.
    private(set) var beats = 0
    private var lastBeat: Date?

    init() {
        engineCapable = CHHapticEngine.capabilitiesForHardware().supportsHaptics
        if engineCapable {
            engine = try? CHHapticEngine()
            engine?.resetHandler = { [weak self] in
                try? self?.engine?.start()
            }
        }
    }

    /// One tick of the 50 Hz loop: if the next beat is due, play it.
    /// Called on the main thread with the freshly polled bpm (0 before the
    /// first /state lands — a beatless hand is honest, not a bug).
    func tick(pollBpm: Double) {
        guard enabled, pollBpm > 0 else { return }
        bpm = pollBpm
        let now = Date()
        let interval = 60.0 / pollBpm
        guard let last = lastBeat else {
            lastBeat = now
            return
        }
        guard now.timeIntervalSince(last) >= interval else { return }
        lastBeat = now
        beat()
    }

    /// One beat: a short soft transient. CoreHaptics when it can, the UIKit
    /// generator when it cannot — the palm does not care which.
    private func beat() {
        beats += 1
        if engineCapable, let engine {
            do {
                try engine.start()
                let intensity = CHHapticEventParameter(parameterID: .hapticIntensity, value: 0.55)
                let sharpness = CHHapticEventParameter(parameterID: .hapticSharpness, value: 0.35)
                let event = CHHapticEvent(eventType: .hapticTransient, parameters: [intensity, sharpness], relativeTime: 0)
                let pattern = try CHHapticPattern(events: [event], parameters: [])
                let player = try engine.makePlayer(with: pattern)
                try player.start(atTime: CHHapticTimeImmediate)
            } catch {
                // A failed engine must not retry-storm: drop to the fallback
                // for this beat only, and let the next beat retry the engine.
                fallback.impactOccurred()
            }
        } else {
            fallback.impactOccurred()
        }
    }

    func setEnabled(_ value: Bool) {
        enabled = value
        if value {
            lastBeat = nil
            if engineCapable {
                try? engine?.start()
            }
        } else {
            bpm = 0
        }
    }
}
