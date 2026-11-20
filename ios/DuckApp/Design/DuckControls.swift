import SwiftUI

/// D2 wave 2 controls (contracts/ui-components.json): tokens only, one
/// implementation, a STATE MATRIX line per component.

/// STATE MATRIX: DuckToggle = off, on, disabled
struct DuckToggle: View {
    let title: String
    let isOn: Bool
    let onToggle: (Bool) -> Void
    var identifier = ""

    @Environment(\.isEnabled) private var enabled

    var body: some View {
        Button {
            onToggle(!isOn)
        } label: {
            HStack(spacing: DuckSpace.md) {
                Text(title)
                    .font(.duckBody)
                    .foregroundStyle(enabled ? Color.duckTextPrimary : Color.duckTextPrimary.opacity(0.4))
                Spacer(minLength: DuckSpace.sm)
                // The track: shape and the word carry the state beside the
                // colour, so TalkBack and a colour-blind eye both read it.
                HStack(spacing: DuckSpace.xxs) {
                    if isOn { Spacer() }
                    Text(isOn ? "ON" : "OFF")
                        .font(.system(.caption2, design: .rounded, weight: .semibold))
                        .foregroundStyle(isOn ? Color.duckOnAccent : Color.duckTextPrimary.opacity(0.7))
                    if !isOn { Spacer() }
                }
                .padding(DuckSpace.xxs)
                .frame(width: 44, height: 26)
                .background(
                    isOn ? Color.duckAction : Color.white.opacity(0.15),
                    in: RoundedRectangle(cornerRadius: DuckRadius.control)
                )
            }
            .frame(minHeight: 48)
            .contentShape(Rectangle())
        }
        .buttonStyle(DuckPressStyle())
        .disabled(!enabled)
        .accessibilityLabel(title)
        .accessibilityValue(isOn ? Copy.stateOn : Copy.stateOff)
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier(identifier)
    }
}

/// STATE MATRIX: DuckSegmented = default, selected, disabled
struct DuckSegmented: View {
    let options: [String]
    let selectedIndex: Int
    let onSelect: (Int) -> Void
    var identifier = ""

    @Environment(\.isEnabled) private var enabled
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        Group {
            if typeSize.duckIsAccessibility {
                VStack(spacing: DuckSpace.xxs) { optionsView }
            } else {
                HStack(spacing: DuckSpace.xxs) { optionsView }
            }
        }
        .padding(DuckSpace.xxs)
        .background(Color.white.opacity(0.10), in: RoundedRectangle(cornerRadius: DuckRadius.control))
        .accessibilityIdentifier(identifier)
        .opacity(enabled ? 1 : 0.4)
        .disabled(!enabled)
    }

    @ViewBuilder private var optionsView: some View {
        ForEach(Array(options.enumerated()), id: \.offset) { index, option in
            let picked = index == selectedIndex
            Text(option)
                .font(.duckLabel.weight(picked ? .semibold : .medium))
                .foregroundStyle(picked ? Color.duckOnAccent : Color.duckTextPrimary.opacity(enabled ? 0.85 : 0.4))
                .frame(maxWidth: .infinity, minHeight: 44)
                .background(
                    picked ? Color.duckAction : Color.white.opacity(0.10),
                    in: RoundedRectangle(cornerRadius: DuckRadius.control)
                )
                .contentShape(RoundedRectangle(cornerRadius: DuckRadius.control))
                .onTapGesture { onSelect(index) }
                .accessibilityAddTraits(picked ? .isSelected : [])
        }
    }
}

/// STATE MATRIX: DuckProgress = normal, warning, error
struct DuckProgress: View {
    enum State { case normal, warning, error }

    let value: Double
    var label: String?
    var state: State = .normal
    var identifier = ""

    private var tint: Color {
        switch state {
        case .normal: return .duckData
        case .warning: return .duckWarn
        case .error: return .duckBad
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.xxs) {
            if let label {
                Text(label)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckTextPrimary)
            }
            HStack(spacing: DuckSpace.sm) {
                GeometryReader { proxy in
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(Color.white.opacity(0.10))
                        Capsule()
                            .fill(tint)
                            .frame(width: max(2, proxy.size.width * min(max(value, 0), 1)))
                    }
                }
                .frame(height: 8)
                .accessibilityIdentifier(identifier)
                Text("\(Int((min(max(value, 0), 1)) * 100))%")
                    .font(.duckData)
                    .foregroundStyle(tint)
                    .monospacedDigit()
            }
        }
    }
}
