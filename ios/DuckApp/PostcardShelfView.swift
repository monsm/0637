import SwiftUI
import UIKit

/// The kept postcards: every Director frame a person chose to keep, newest
/// first, with its honest one-line caption and a Share button.
struct PostcardShelfView: View {
    @ObservedObject var shelf: PostcardShelf
    @Environment(\.dismiss) private var dismiss
    @State private var shareURL: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(Copy.postcardShelf)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("postcards-title")
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
                .accessibilityIdentifier("postcards-close")
            }
            if shelf.cards.isEmpty {
                Text(Copy.postcardNone)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("postcards-empty")
            } else {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 150), spacing: 10)],
                        spacing: 10
                    ) {
                        ForEach(shelf.cards) { card in
                            VStack(alignment: .leading, spacing: 6) {
                                if let image = UIImage(
                                    contentsOfFile: shelf.url(for: card).path
                                ) {
                                    Image(uiImage: image)
                                        .resizable()
                                        .scaledToFill()
                                        .frame(height: 150)
                                        .clipped()
                                        .clipShape(RoundedRectangle(cornerRadius: 12))
                                }
                                Text(card.caption)
                                    .font(.system(size: 11, design: .rounded))
                                    .foregroundStyle(.white.opacity(0.8))
                                    .lineLimit(2)
                                HStack {
                                    Text(card.day)
                                        .font(.system(size: 9, design: .monospaced))
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
        .padding(20)
        .preferredColorScheme(.dark)
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
