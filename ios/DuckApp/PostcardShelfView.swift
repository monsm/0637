import SwiftUI
import UIKit

/// The kept postcards: every Director frame a person chose to keep, newest
/// first, with its honest one-line caption and a Share button.
struct PostcardShelfView: View {
    @ObservedObject var shelf: PostcardShelf
    @Environment(\.dismiss) private var dismiss
    @State private var shareURL: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                Text(Copy.postcardShelf)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("postcards-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("postcards-close")
            }
            if shelf.cards.isEmpty {
                Text(Copy.postcardNone)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("postcards-empty")
            } else {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 150), spacing: DuckSpace.sm)],
                        spacing: DuckSpace.sm
                    ) {
                        ForEach(shelf.cards) { card in
                            VStack(alignment: .leading, spacing: DuckSpace.xs) {
                                if let image = UIImage(
                                    contentsOfFile: shelf.url(for: card).path
                                ) {
                                    Image(uiImage: image)
                                        .resizable()
                                        .scaledToFill()
                                        .frame(height: 150)
                                        .clipped()
                                        .clipShape(RoundedRectangle(cornerRadius: DuckRadius.thumb))
                                }
                                Text(card.caption)
                                    .font(.duckCaption)
                                    .foregroundStyle(.white.opacity(0.8))
                                    .lineLimit(2)
                                HStack {
                                    Text(card.day)
                                        .font(.duckMicro)
                                        .foregroundStyle(.secondary)
                                    Spacer()
                                    Button {
                                        shareURL = shelf.url(for: card)
                                    } label: {
                                        Image(systemName: "square.and.arrow.up")
                                            .frame(width: 44, height: 36)
                                            .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityIdentifier("postcard-share-\(card.id)")
                                }
                            }
                        }
                    }
                }
            }
        }
        .padding(DuckSpace.xl)
        .sheet(
            isPresented: Binding(
                get: { shareURL != nil },
                set: { if !$0 { shareURL = nil } }
            )
        ) {
            if let shareURL {
                ShareSheet(items: [shareURL])
            }
        }
    }
}
