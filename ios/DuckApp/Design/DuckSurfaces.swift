import SwiftUI

/// Surfaces: the card, the sheet header and the list row (D2 wave 1,
/// contracts/ui-components.json). Tokens only; a state line per component.

/// STATE MATRIX: DuckCard = default, pressed, disabled
struct DuckCard<Content: View>: View {
    var padding: CGFloat = DuckSpace.lg
    @ViewBuilder let content: () -> Content

    var body: some View {
        content()
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                Color.duckBgSurface.opacity(0.94),
                in: RoundedRectangle(cornerRadius: DuckRadius.card)
            )
            .overlay(
                RoundedRectangle(cornerRadius: DuckRadius.card)
                    .stroke(Color.white.opacity(0.08), lineWidth: 1)
            )
    }
}

/// The one sheet chrome: title + the shared 48 pt close. Every sheet uses
/// this instead of hand-rolling an xmark button; per-test identifiers pass
/// through unchanged.
///
/// STATE MATRIX: DuckSheetHeader = default, closePressed, closeDisabled
struct DuckSheetHeader: View {
    let title: String
    var identifier = ""
    var closeIdentifier = "sheet-close-shared"
    let onClose: () -> Void

    var body: some View {
        HStack(spacing: DuckSpace.md) {
            Text(title)
                .font(.duckTitle)
                .foregroundStyle(.white)
                .accessibilityIdentifier(identifier)
            Spacer(minLength: DuckSpace.sm)
            DuckCloseControl(identifier: closeIdentifier, action: onClose)
        }
        .padding(.bottom, DuckSpace.sm)
    }
}

/// The close glyph the 48 pt close controls share: icon, hit target, fill.
/// Buttons keep their own actions and identifiers; only these pixels are
/// shared, so no sheet can drift from the rest.
///
/// STATE MATRIX: (shared with DuckSheetHeader)
struct DuckCloseGlyph: View {
    var body: some View {
        Image(systemName: "xmark")
            .font(DuckIcon.xs)
            .foregroundStyle(.secondary)
            .frame(width: 48, height: 48)
            .background(Color.white.opacity(0.10), in: Circle())
            .contentShape(Circle())
    }
}

/// The one 48 pt close control — the shape every sheet's close shares.
/// SheetCloseButton (dismiss-based) wraps this for sheets that close
/// themselves through the environment.
///
/// STATE MATRIX: (shared with DuckSheetHeader)
struct DuckCloseControl: View {
    var identifier = "sheet-close-shared"
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            DuckCloseGlyph()
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.close)
        .accessibilityIdentifier(identifier)
    }
}

/// The one list row: title, optional note, optional trailing value; the
/// whole row is the target and the chevron is never the only cue.
///
/// STATE MATRIX: DuckListRow = default, pressed, disabled, selected
struct DuckListRow: View {
    let title: String
    var note: String?
    var value: String?
    var selected = false
    var identifier = ""
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: DuckSpace.md) {
                VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                    Text(title)
                        .font(.duckBody.weight(.medium))
                        .foregroundStyle(.white)
                        .multilineTextAlignment(.leading)
                    if let note {
                        Text(note)
                            .font(.duckCaption)
                            .foregroundStyle(.white.opacity(0.6))
                            .multilineTextAlignment(.leading)
                    }
                }
                Spacer(minLength: DuckSpace.sm)
                if let value {
                    Text(value)
                        .font(.duckCaption.weight(.semibold))
                        .foregroundStyle(selected ? Color.duckAction : Color.white.opacity(0.55))
                }
                if selected {
                    Image(systemName: "checkmark")
                        .font(DuckIcon.xs)
                        .foregroundStyle(Color.duckAction)
                }
            }
            .frame(minHeight: 56)
            .contentShape(Rectangle())
        }
        .buttonStyle(DuckPressStyle())
        .accessibilityIdentifier(identifier)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
