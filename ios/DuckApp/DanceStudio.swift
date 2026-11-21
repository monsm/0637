import Foundation

/// A tiny choreography recorder. It records what the person actually did —
/// a skill, a sound, a treat, a pet — and replays the same intent route. It
/// never invents a motion: an action the core refuses is skipped, just as it
/// would be when a person pressed the button at that moment.

struct DanceStep: Codable, Equatable, Identifiable {
    var id = UUID()
    var kind: String
    var value: String = ""
    var label: String = ""

    enum CodingKeys: String, CodingKey { case kind, value, label }
}

final class DanceStudio: ObservableObject {
    @Published var recording = false
    @Published var playing = false
    @Published var steps: [DanceStep] = []
    @Published var activeStep = -1
    @Published var saved = false
    private var playingGeneration = 0
    private let limit = 12
    private static let key = "duck_dance_v1"

    init() {
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let steps = try? JSONDecoder().decode([DanceStep].self, from: data) {
            self.steps = steps
            self.saved = !steps.isEmpty
        }
    }

    func toggleRecording() {
        if playing { stop() }
        recording.toggle()
        if !recording {
            save()
        }
    }

    /// Record one action. Only recognized verbs, and never one of our own
    /// playback calls, so pressing Play cannot record itself.
    func record(_ kind: String, value: String = "", label: String = "") {
        guard recording, !playing, steps.count < limit else { return }
        steps.append(DanceStep(kind: kind, value: value, label: label))
        if steps.count >= limit {
            recording = false
            save()
        }
    }

    func clear() {
        steps = []
        recording = false
        activeStep = -1
        saved = false
        UserDefaults.standard.removeObject(forKey: Self.key)
    }

    private func save() {
        saved = !steps.isEmpty
        if let data = try? JSONEncoder().encode(steps) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }

    /// Play the same intents in order. The shell supplies the execution
    /// closure; this owns only pacing and the highlighted tile.
    func play(_ execute: @escaping (DanceStep) -> Void) {
        guard !steps.isEmpty else { return }
        recording = false
        playing = true
        playingGeneration &+= 1
        let generation = playingGeneration
        let steps = self.steps
        func step(_ index: Int) {
            guard generation == playingGeneration, playing else { return }
            guard index < steps.count else {
                playing = false
                activeStep = -1
                return
            }
            activeStep = index
            execute(steps[index])
            // Upstream skills last 0.5-1 s and the scheduler refuses a new
            // one while busy; 0.8 s is a deliberate child-friendly beat.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                step(index + 1)
            }
        }
        step(0)
    }

    func stop() {
        playingGeneration &+= 1
        playing = false
        activeStep = -1
    }
}
