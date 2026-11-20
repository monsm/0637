import SwiftUI

/// The stage deck's two button shapes (B4 §3), lifted out of ContentView so
/// the D4 focus plumbing does not grow the already-capped shell file. Each
/// keeps the same pixels, label and identifier as the shell's old helpers.
struct DuckPrimaryButton: View {
    let id: String
    let label: String
    let icon: String
    var selected = false
    var focus: DuckStageFocus?
    var focusActive = false
    let stageFocus: AccessibilityFocusState<DuckStageFocus?>.Binding
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: DuckSpace.xxs) {
                Image(systemName: icon)
                    .font(DuckIcon.md)
                    .frame(width: 52, height: 52)
                    .background(
                        selected
                            ? AnyShapeStyle(Color.duckAction.opacity(0.35))
                            : AnyShapeStyle(.ultraThinMaterial),
                        in: Circle()
                    )
                    .contentShape(Circle())
                Text(label)
                    .font(.duckCaption.weight(.medium))
                    .foregroundStyle(.white.opacity(0.85))
                    .multilineTextAlignment(.center)
                    .frame(width: 52)
                    .lineLimit(2)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
        .modifier(StageFocusModifier(focus: focus, active: focusActive, binding: stageFocus))
    }
}

struct DuckSecondaryButton: View {
    let id: String
    let label: String
    let icon: String
    var focus: DuckStageFocus?
    var focusActive = false
    let stageFocus: AccessibilityFocusState<DuckStageFocus?>.Binding
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(DuckIcon.md)
                .frame(width: 48, height: 48)
                .background(.ultraThinMaterial, in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
        .modifier(StageFocusModifier(focus: focus, active: focusActive, binding: stageFocus))
    }
}