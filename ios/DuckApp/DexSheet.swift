import SwiftUI

/// The collection page: every official move and emotion, its real lifetime
/// count, and the first day this phone saw it happen. Locked entries are
/// shown greyed rather than hidden, so the child knows what exists.
struct DexSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                Text(Copy.dex)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("dex-title")
                    .duckSheetFirstFocus()
                Spacer()
                Button { dismiss() } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("dex-close")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.sm) {
                    section(Copy.dexMoves, entries: controller.dex.filter { !$0.isEmotion })
                    section(Copy.dexMoods, entries: controller.dex.filter { $0.isEmotion })
                }
            }
        }
        .padding(DuckSpace.xl)
    }

    @ViewBuilder
    private func section(_ title: String, entries: [DuckController.DexEntry]) -> some View {
        Text(title)
            .font(.duckCaption.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.top, DuckSpace.xs)
        ForEach(entries) { entry in
            HStack(spacing: DuckSpace.sm) {
                Text(entry.count > 0 ? "✓" : "·")
                    .font(.duckCaption.weight(.bold))
                    .foregroundStyle(entry.count > 0 ? .green : .secondary)
                    .frame(width: 16)
                Text(SkillLabel.of(entry.id))
                    .font(.duckLabel)
                    .foregroundStyle(entry.count > 0 ? .white : .secondary)
                Spacer()
                Text(entry.count > 0 ? "× \(entry.count)" : Copy.dexNever)
                    .font(.duckData)
                    .foregroundStyle(.secondary)
                if let first = entry.firstSeen {
                    Text("\(Copy.dexFirst) \(first)")
                        .font(.duckMicro)
                        .foregroundStyle(.tertiary)
                }
            }
            .frame(minHeight: 32)
            .accessibilityIdentifier("dex-\(entry.id)")
        }
    }
}
