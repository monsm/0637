import SwiftUI

/// The one 48 pt sheet close control (IA reorder, 2026-11-16).
///
/// Fifteen files each hand-rolled this button; the three new destination
/// pages must not make it eighteen. New pages use this; older sheets retire
/// their copies batch by batch (the close-button ratchet counts files, so
/// the debt can only shrink from here).
struct SheetCloseButton: View {
    /// Existing screens keep the identifier their UI tests already use.
    var identifier: String = "sheet-close-shared"
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        Button { dismiss() } label: {
            Image(systemName: "xmark")
                .font(DuckIcon.xs)
                .foregroundStyle(.secondary)
                .frame(width: 48, height: 48)
                .background(Color.white.opacity(0.10), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Copy.close)
        .accessibilityIdentifier(identifier)
    }
}
