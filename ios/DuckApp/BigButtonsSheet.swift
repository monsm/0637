import SwiftUI

/// Four verbs, 72 pt tall, one page. The big-button page is for a
/// grandparent (or a three-year-old): no drawer, no ring, no tiny chips —
/// the whole duck's day is a stroke, a treat, a voice and a sit. The verbs
/// go through the exact same controller calls the small buttons use, so
/// the duck cannot tell the difference. VoiceOver reads the whole action,
/// and Dynamic Type has nothing to shrink: the words are already large.
struct BigButtonsSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.lg) {
            HStack {
                Text(Copy.bigButtons)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("big-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("big-close")
            }
            Text(Copy.bigButtonsNote)
                .font(.duckLabel)
                .foregroundStyle(.secondary)

            VStack(spacing: DuckSpace.md) {
                bigButton(0, icon: "hand.raised.fill") { controller.pet() }
                bigButton(1, icon: Icons.feed) { controller.feed() }
                bigButton(2, icon: Icons.quack) {
                    if let core = controller.core {
                        controller.voice.play(core: core, tag: "chirp")
                    }
                }
                bigButton(3, icon: "figure.wave") { controller.sit() }
            }
        }
        .padding(DuckSpace.xl)
    }

    /// One verb: a full-width 72 pt button, 20 pt words, the accent colour
    /// so the target is unmistakable at arm's length.
    private func bigButton(_ index: Int, icon: String, action: @escaping () -> Void) -> some View {
        Button {
            action()
        } label: {
            HStack(spacing: DuckSpace.md) {
                Image(systemName: icon)
                    .font(DuckIcon.lg)
                    .frame(width: 40)
                Text(Copy.bigButtonAction(index))
                    .font(.duckTitle.weight(.semibold))
                Spacer()
            }
            .padding(.horizontal, DuckSpace.lg)
            .frame(minHeight: 72)
            .background(Color.white.opacity(0.10), in: RoundedRectangle(cornerRadius: DuckRadius.control))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.bigButtonAction(index))
        .accessibilityIdentifier("big-action-\(index)")
    }
}
