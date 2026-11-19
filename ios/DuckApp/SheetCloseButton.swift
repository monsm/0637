import SwiftUI

/// The one 48 pt sheet close control (IA reorder, 2026-11-16; D2 moved the
/// pixels into DuckCloseControl so DuckSheetHeader and this button share one
/// shape). New pages use this; nothing hand-rolls an xmark any more.
struct SheetCloseButton: View {
    /// Existing screens keep the identifier their UI tests already use.
    var identifier: String = "sheet-close-shared"
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        DuckCloseControl(identifier: identifier) { dismiss() }
    }
}
