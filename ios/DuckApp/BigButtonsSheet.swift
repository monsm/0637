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
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(Copy.bigButtons)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("big-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("big-close")
            }
            Text(Copy.bigButtonsNote)
                .font(.system(size: 13, design: .rounded))
                .foregroundStyle(.secondary)

            VStack(spacing: 12) {
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
        .padding(20)
        .preferredColorScheme(.dark)
    }

    /// One verb: a full-width 72 pt button, 20 pt words, the accent colour
    /// so the target is unmistakable at arm's length.
    private func bigButton(_ index: Int, icon: String, action: @escaping () -> Void) -> some View {
        Button {
            action()
        } label: {
            HStack(spacing: 14) {
                Image(systemName: icon)
                    .font(.system(size: 26))
                    .frame(width: 40)
                Text(Copy.bigButtonAction(index))
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                Spacer()
            }
            .padding(.horizontal, 18)
            .frame(minHeight: 72)
            .background(Color.white.opacity(0.10), in: RoundedRectangle(cornerRadius: 16))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.bigButtonAction(index))
        .accessibilityIdentifier("big-action-\(index)")
    }
}
