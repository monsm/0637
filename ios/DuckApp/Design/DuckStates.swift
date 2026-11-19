import SwiftUI

/// D2 wave 2 states (contracts/ui-components.json): the empty, error and
/// loading surfaces every data-backed screen must be able to show. Tokens
/// only; words come from Copy so the tone and the language stay the app's.

/// STATE MATRIX: DuckEmptyState = default, actionPressed
struct DuckEmptyState: View {
    let title: String
    var note: String?
    var actionTitle: String?
    var action: (() -> Void)?
    var identifier = ""

    var body: some View {
        VStack(spacing: DuckSpace.md) {
            Text(title)
                .font(.duckBody)
                .foregroundStyle(Color.duckTextPrimary)
                .multilineTextAlignment(.center)
            if let note {
                Text(note)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckTextSecondary)
                    .multilineTextAlignment(.center)
            }
            if let actionTitle, let action {
                DuckChip(title: actionTitle, identifier: "\(identifier)-action", action: action)
            }
        }
        .padding(DuckSpace.xl)
        .frame(maxWidth: .infinity)
        .accessibilityIdentifier(identifier)
        .accessibilityElement(children: .combine)
    }
}

/// STATE MATRIX: DuckErrorCard = default, retryPressed, retrying, offline
struct DuckErrorCard: View {
    let title: String
    let reason: String
    let retry: () -> Void
    var retrying = false
    var offline = false
    var identifier = ""

    var body: some View {
        VStack(spacing: DuckSpace.sm) {
            Text(title)
                .font(.duckLabel.weight(.semibold))
                .foregroundStyle(Color.duckBad)
                .multilineTextAlignment(.center)
            Text(reason)
                .font(.duckCaption)
                .foregroundStyle(Color.duckTextSecondary)
                .multilineTextAlignment(.center)
            DuckButton(
                title: Copy.retry,
                style: .secondary,
                systemImage: "arrow.clockwise",
                identifier: "\(identifier)-retry"
            ) {
                retry()
            }
            .disabled(retrying)
        }
        .padding(DuckSpace.lg)
        .frame(maxWidth: .infinity)
        .background(
            Color.duckBgSurface.opacity(0.94),
            in: RoundedRectangle(cornerRadius: DuckRadius.card)
        )
        .overlay(
            RoundedRectangle(cornerRadius: DuckRadius.card)
                .stroke(Color.duckBad.opacity(0.35), lineWidth: 1)
        )
        .accessibilityIdentifier(identifier)
        .accessibilityElement(children: .combine)
    }
}

/// STATE MATRIX: DuckLoading = default, reducedMotion
struct DuckLoading: View {
    let title: String
    var identifier = ""

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        // Plain words, no engine names; the duck animates elsewhere. Under
        // Reduce Motion the breath is a still frame, the words stay.
        HStack(spacing: DuckSpace.md) {
            Circle()
                .fill(Color.duckTextSecondary.opacity(reduceMotion ? 0.3 : 0.5))
                .frame(width: 8, height: 8)
                .scaleEffect(reduceMotion ? 1 : 0.85)
            Text(title)
                .font(.duckCaption)
                .foregroundStyle(Color.duckTextSecondary)
        }
        .padding(DuckSpace.lg)
        .frame(maxWidth: .infinity)
        .accessibilityIdentifier(identifier)
        .accessibilityElement(children: .combine)
    }
}
