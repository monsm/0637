import SwiftUI

/// D4's layout rule (UIUX_REFACTOR_DEEP §1/§3.5): any row of two or more
/// columns collapses to one column at the accessibility type sizes. One
/// predicate so every sheet agrees on where "accessibility" begins.
extension DynamicTypeSize {
    var duckIsAccessibility: Bool { self >= .accessibility1 }
}

/// A row that becomes a column at accessibility sizes. Use it for stat
/// triples and control rows that would otherwise squeeze their labels into
/// an ellipsis; it is not a replacement for the normal HStacks whose children
/// are fixed-size glyphs.
struct DuckAdaptiveRow<Content: View>: View {
    var spacing: CGFloat = DuckSpace.md
    var verticalAlignment: VerticalAlignment = .center
    let content: Content
    @Environment(\.dynamicTypeSize) private var typeSize

    init(
        spacing: CGFloat = DuckSpace.md,
        verticalAlignment: VerticalAlignment = .center,
        @ViewBuilder content: () -> Content
    ) {
        self.spacing = spacing
        self.verticalAlignment = verticalAlignment
        self.content = content()
    }

    var body: some View {
        Group {
            if typeSize.duckIsAccessibility {
                VStack(alignment: .leading, spacing: spacing) { content }
            } else {
                HStack(alignment: verticalAlignment, spacing: spacing) { content }
            }
        }
    }
}
