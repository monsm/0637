import SwiftUI

/// The one press feedback for every component in the layer: scale down a
/// touch, dim a touch, nothing under Reduce Motion.
struct DuckPressStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
            .opacity(configuration.isPressed ? 0.82 : 1)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

/// The one button (D2 wave 1, contracts/ui-components.json). Every pressable
/// in the app goes through one of these styles; a sheet that hand-rolls its
/// own Button style is a defect the component gate counts.
///
/// STATE MATRIX: DuckButton = default, pressed, disabled, selected, focus
struct DuckButton: View {
    enum Style { case primary, secondary, ghost, icon }

    let title: String
    var style: Style = .primary
    var systemImage: String?
    var selected = false
    var identifier = ""
    let action: () -> Void

    @Environment(\.isEnabled) private var enabled

    var body: some View {
        Button(action: action) {
            HStack(spacing: DuckSpace.xs) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(style == .icon ? DuckIcon.md : DuckIcon.sm)
                }
                if style != .icon {
                    Text(title)
                        .font(.duckLabel.weight(.medium))
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, style == .icon ? DuckSpace.xs : DuckSpace.md)
            .frame(minHeight: 48)
            .foregroundStyle(foreground)
            .background(background, in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(DuckPressStyle())
        .disabled(disabled)
        .accessibilityLabel(title)
        .accessibilityIdentifier(identifier)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var disabled: Bool { !enabled }

    private var foreground: Color {
        if style == .primary { return .duckOnAccent }
        return selected ? .duckAction : .duckTextPrimary
    }

    private var background: AnyShapeStyle {
        if style == .primary {
            AnyShapeStyle(Color.duckAction)
        } else if selected {
            AnyShapeStyle(Color.duckAction.opacity(0.35))
        } else {
            AnyShapeStyle(Color.white.opacity(0.10))
        }
    }
}

/// The one chip: a state word that must never rely on hue alone — selected
/// adds the check glyph and the isSelected trait.
///
/// STATE MATRIX: DuckChip = default, pressed, disabled, selected
struct DuckChip: View {
    let title: String
    var selected = false
    var identifier: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: DuckSpace.xxs) {
                if selected {
                    Image(systemName: "checkmark")
                        .font(DuckIcon.xs)
                }
                Text(title)
                    .font(.duckLabel.weight(.medium))
                    .lineLimit(1)
            }
            .padding(.horizontal, DuckSpace.md)
            .frame(minHeight: 48)
            .foregroundStyle(selected ? Color.duckOnAccent : Color.duckTextPrimary)
            .background(selected ? Color.duckAction : Color.white.opacity(0.10), in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(DuckPressStyle())
        .accessibilityIdentifier(identifier ?? "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
