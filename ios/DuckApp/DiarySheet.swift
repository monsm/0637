import SwiftUI

/// The duck's diary: today's REAL record, and — when the person configured
/// their own model — the paragraph that model writes from those facts.
/// Without a model the facts stand alone; nothing here is fabricated.
struct DiarySheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(Copy.diary)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("diary-title")
                Spacer()
                if !shareText.isEmpty {
                    ShareLink(item: shareText) {
                        Image(systemName: "square.and.arrow.up")
                            .font(.system(size: 16))
                            .frame(width: 48, height: 48)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Copy.share)
                    .accessibilityIdentifier("diary-share")
                }
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
                .accessibilityIdentifier("diary-close")
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(Copy.diaryFactsTitle)
                        .font(.system(size: 12, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                    if controller.diaryLines.isEmpty {
                        Text(Copy.diaryEmpty)
                            .font(.system(size: 13, design: .rounded))
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(Array(controller.diaryLines.enumerated()), id: \.offset) { _, line in
                            HStack(spacing: 8) {
                                Circle()
                                    .fill(Color.accentColor.opacity(0.7))
                                    .frame(width: 5, height: 5)
                                Text(line)
                                    .font(.system(size: 13, design: .rounded))
                            }
                        }
                    }

                    Divider()

                    Text(Copy.diaryModelTitle)
                        .font(.system(size: 12, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                    if controller.diaryBusy {
                        Text(Copy.diaryBusy)
                            .font(.system(size: 13, design: .rounded))
                            .foregroundStyle(.secondary)
                    } else if !controller.diaryModelText.isEmpty {
                        Text(controller.diaryModelText)
                            .font(.system(size: 14, design: .serif))
                            .lineSpacing(3)
                            .accessibilityIdentifier("diary-model-text")
                    } else if let error = controller.diaryError {
                        Text(error)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                    } else {
                        Text(Copy.diaryNeedModel)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
        .onAppear { controller.loadDiary() }
    }

    private var shareText: String {
        if !controller.diaryModelText.isEmpty {
            return controller.diaryModelText
        }
        return controller.diaryLines.joined(separator: "\n")
    }
}
