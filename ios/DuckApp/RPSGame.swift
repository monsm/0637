import SwiftUI

/// Rock-paper-scissors, played with the duck's body.
///
/// The child throws; the duck answers with a real move — sit is the rock,
/// stand the paper, a kick the scissors. Its choice is drawn from the duck's
/// own provenance fingerprint (the same bytes its voice seed is derived
/// from) mixed with the round, so the same duck plays the same way on the
/// same round and two ducks differ: a personality, not a coin. The outcome
/// is spoken, felt and shown; nothing is scored or stored.
struct RPSGame: View {
    @ObservedObject var controller: DuckController

    private enum Move: String, CaseIterable {
        case rock, paper, scissors
        var word: String {
            switch self {
            case .rock: return Copy.rpsRock
            case .paper: return Copy.rpsPaper
            case .scissors: return Copy.rpsScissors
            }
        }
        /// The move this one beats.
        var beats: Move {
            switch self {
            case .rock: return .scissors
            case .paper: return .rock
            case .scissors: return .paper
            }
        }
        /// The body the duck answers with — its own skills, no new verbs.
        var skill: String {
            switch self {
            case .rock: return "sit"
            case .paper: return "stand"
            case .scissors: return "kick_left"
            }
        }
    }

    @State private var round = 0
    @State private var outcome = ""
    @State private var duckWord = ""
    // This page's own tally: the child's wins against the duck's. Session
    // only — nothing is stored, so a reopened page starts honest at zero.
    @State private var youScore = 0
    @State private var duckScore = 0

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.rpsTitle)
                    .font(.duckLabel.weight(.semibold))
                    .foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(outcome.isEmpty ? Copy.echoStart : outcome)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("rps-status")
                Text("\(Copy.rpsYou) \(youScore) · \(Copy.rpsDuck) \(duckScore)")
                    .font(.duckCaption.weight(.semibold))
                    .foregroundStyle(Color.duckBrass)
                    .accessibilityIdentifier("rps-score")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Move.allCases, id: \.self) { move in
                    Button {
                        throwMove(move)
                    } label: {
                        Text(move.word)
                            .font(.duckBody.weight(.semibold))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(DuckPressStyle())
                    .accessibilityIdentifier("rps-\(move.rawValue)")
                }
            }
            if !duckWord.isEmpty {
                Text(duckWord)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckBrass)
                    .accessibilityIdentifier("rps-duck")
            }
        }
    }

    private func throwMove(_ mine: Move) {
        round += 1
        // The duck's answer: its own fingerprint (the bytes its voice seed
        // comes from) mixed with the round. A personality that repeats,
        // not a fresh coin every throw.
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in controller.fingerprint.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x100000001b3
        }
        hash ^= UInt64(round &* 2_654_435_761)
        let theirs = Move.allCases[Int(hash % 3)]
        duckWord = "\(Copy.rpsDuckThrew) \(theirs.word)"
        controller.trigger(theirs.skill)
        if theirs == mine {
            outcome = Copy.rpsResult("draw")
            DuckHaptics.selection()
        } else if mine.beats == theirs {
            outcome = Copy.rpsResult("win")
            youScore += 1
            DuckHaptics.success()
            controller.trigger("laugh")
        } else {
            outcome = Copy.rpsResult("lose")
            duckScore += 1
            DuckHaptics.medium()
        }
        UIAccessibility.post(notification: .announcement, argument: outcome)
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
            duckWord = ""
        }
    }
}
