import SwiftUI

/// A turn-taking memory game with the duck's own voice.
///
/// The duck sings a short sequence of its synth sounds (queue_sound tags),
/// the child answers by tapping the same pads in the same order. Each cleared
/// round adds one sound, up to five. Every note is the duck's real voice —
/// the same PCM a quack plays — and a win lets it celebrate with its own
/// excited mood. Nothing here is scored or timed; it is play, not a test.
struct EchoGame: View {
    @ObservedObject var controller: DuckController

    private static let pads = ["chirp", "coo", "peck", "inquire"]
    private static let topRounds = 5

    private enum Phase { case idle, singing, listening, won, lost }

    @State private var phase: Phase = .idle
    @State private var sequence: [String] = []
    @State private var answer = 0

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.echoTitle)
                    .font(.duckLabel.weight(.semibold))
                    .foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(statusWord)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("echo-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Self.pads, id: \.self) { tag in
                    Button {
                        tap(tag)
                    } label: {
                        Text(Copy.echoSound(tag))
                            .font(.duckBody.weight(.semibold))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(DuckPressStyle())
                    .disabled(phase != .listening)
                    .accessibilityIdentifier("echo-pad-\(tag)")
                }
            }
            Button(sequence.isEmpty ? Copy.echoStart : Copy.echoAgain) {
                start()
            }
            .font(.duckLabel.weight(.semibold))
            .frame(maxWidth: .infinity, minHeight: 48)
            .buttonStyle(.bordered)
            .accessibilityIdentifier("echo-start")
        }
    }

    private var statusWord: String {
        switch phase {
        case .idle: return Copy.echoStart
        case .singing: return "…"
        case .listening: return Copy.echoRound(sequence.count)
        case .won: return Copy.echoWon
        case .lost: return Copy.echoAgain
        }
    }

    private func start() {
        sequence = [Self.pads.randomElement() ?? "chirp"]
        answer = 0
        sing()
    }

    /// Play the whole sequence, then open the floor. The gap is the duck's
    /// own phrasing, not a metronome.
    private func sing() {
        phase = .singing
        let current = sequence
        Task { @MainActor in
            for tag in current {
                play(tag)
                try? await Task.sleep(nanoseconds: 550_000_000)
            }
            answer = 0
            phase = .listening
        }
    }

    private func tap(_ tag: String) {
        guard phase == .listening, answer < sequence.count else { return }
        play(tag)
        if tag == sequence[answer] {
            answer += 1
            guard answer == sequence.count else { return }
            if sequence.count >= Self.topRounds {
                phase = .won
                DuckHaptics.success()
                controller.trigger("excited")
            } else {
                sequence.append(Self.pads.randomElement() ?? "chirp")
                sing()
            }
        } else {
            phase = .lost
            DuckHaptics.error()
            play("alarm")
        }
    }

    private func play(_ tag: String) {
        if let core = controller.core {
            controller.voice.play(core: core, tag: tag)
        }
    }
}
