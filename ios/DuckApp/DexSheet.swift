import SwiftUI

/// The collection page: every official move and emotion, its real lifetime
/// count, and the first day this phone saw it happen. Locked entries are
/// shown greyed rather than hidden, so the child knows what exists.
struct DexSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(Copy.dex)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("dex-title")
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("dex-close")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    section(Copy.dexMoves, entries: controller.dex.filter { !$0.isEmotion })
                    section(Copy.dexMoods, entries: controller.dex.filter { $0.isEmotion })
                }
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
    }

    @ViewBuilder
    private func section(_ title: String, entries: [DuckController.DexEntry]) -> some View {
        Text(title)
            .font(.system(size: 12, weight: .semibold, design: .rounded))
            .foregroundStyle(.secondary)
            .padding(.top, 6)
        ForEach(entries) { entry in
            HStack(spacing: 10) {
                Text(entry.count > 0 ? "✓" : "·")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(entry.count > 0 ? .green : .secondary)
                    .frame(width: 16)
                Text(SkillLabel.of(entry.id))
                    .font(.system(size: 13, design: .rounded))
                    .foregroundStyle(entry.count > 0 ? .white : .secondary)
                Spacer()
                Text(entry.count > 0 ? "× \(entry.count)" : Copy.dexNever)
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(.secondary)
                if let first = entry.firstSeen {
                    Text("\(Copy.dexFirst) \(first)")
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundStyle(.tertiary)
                }
            }
            .frame(minHeight: 32)
            .accessibilityIdentifier("dex-\(entry.id)")
        }
    }
}
